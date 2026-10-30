use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_postgres::{AsyncMessage, Client, NoTls, SimpleQueryMessage, Statement};

use super::{Outcome, Timeouts};

/// NOTIFY messages received on a connection, as (channel, payload) rows.
pub(super) type Notices = Arc<Mutex<Vec<Vec<Option<String>>>>>;

pub(super) struct Conn {
    pub(super) client: Arc<Client>,
    pub(super) driver: JoinHandle<()>,
    pub(super) statements: HashMap<String, Statement>,
    /// The query started by a `background` step, if not yet waited for.
    pub(super) pending: Option<JoinHandle<Outcome>>,
    pub(super) notices: Notices,
    /// Backend pid when connected straight to Postgres, so a disconnect can wait for
    /// the backend to exit.
    pub(super) pid: Option<i32>,
}

/// The backend pid of a direct connection.
pub(super) async fn backend_pid(client: &Client, t: &Timeouts) -> Option<i32> {
    let messages = timeout(t.housekeeping, client.simple_query("SELECT pg_backend_pid()"))
        .await
        .ok()?
        .ok()?;
    messages.into_iter().find_map(|m| match m {
        SimpleQueryMessage::Row(row) => row.get(0)?.parse().ok(),
        _ => None,
    })
}

/// Waits until a Postgres backend has exited. Closing the socket returns before the
/// backend is gone, and until then its session state (locks, temp tables) is still
/// visible, which would make the baseline racy.
pub(super) async fn wait_backend_gone(direct: &str, pid: i32, t: &Timeouts) {
    let Ok((client, driver, _)) = connect(direct, t).await else {
        return;
    };
    let deadline = tokio::time::Instant::now() + t.housekeeping;
    let sql = format!("SELECT 1 FROM pg_stat_activity WHERE pid = {pid}");
    while tokio::time::Instant::now() < deadline {
        match client.simple_query(&sql).await {
            Ok(rows) if rows.iter().any(|m| matches!(m, SimpleQueryMessage::Row(_))) => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            _ => break,
        }
    }
    drop(client);
    let _ = timeout(t.housekeeping, driver).await;
}

impl Conn {
    /// Close the connection and wait for Terminate to go out, so the pooler sees the
    /// disconnect before the next step runs.
    pub(super) async fn close(self, t: &Timeouts) {
        if let Some(pending) = self.pending {
            pending.abort();
        }
        drop(self.client);
        let _ = timeout(t.housekeeping, self.driver).await;
    }
}

pub(super) async fn connect(url: &str, t: &Timeouts) -> Result<(Client, JoinHandle<()>, Notices)> {
    let (client, mut connection) = timeout(t.connect, tokio_postgres::connect(url, NoTls))
        .await
        .context("connect timed out")??;
    let notices = Notices::default();
    let sink = notices.clone();
    // Polling messages instead of just awaiting the connection keeps the NOTIFYs.
    let driver = tokio::spawn(async move {
        while let Some(Ok(message)) = std::future::poll_fn(|cx| connection.poll_message(cx)).await {
            if let AsyncMessage::Notification(n) = message {
                let row = vec![Some(n.channel().to_string()), Some(n.payload().to_string())];
                sink.lock().unwrap().push(row);
            }
        }
    });
    Ok((client, driver, notices))
}

pub(super) fn db_error(e: &tokio_postgres::Error) -> Outcome {
    match e.as_db_error() {
        Some(db) => Outcome::error(db.code().code(), db.message()),
        None if e.is_closed() => Outcome::error("CLOSED", e.to_string()),
        None => Outcome::error("CLIENT", e.to_string()),
    }
}

pub(super) fn simple_outcome(res: Result<Vec<SimpleQueryMessage>, tokio_postgres::Error>) -> Outcome {
    let messages = match res {
        Ok(messages) => messages,
        Err(e) => return db_error(&e),
    };
    let mut rows = Vec::new();
    let mut tags = Vec::new();
    for message in messages {
        match message {
            SimpleQueryMessage::Row(row) => {
                rows.push((0..row.len()).map(|i| row.get(i).map(str::to_string)).collect());
            }
            SimpleQueryMessage::CommandComplete(n) => tags.push(n),
            _ => {}
        }
    }
    Outcome::Ok { rows, tags }
}

pub(super) fn row_value(row: &tokio_postgres::Row, idx: usize) -> Option<String> {
    // Extended-protocol rows come back in binary. Text-like columns decode directly;
    // anything else should be cast to text in the scenario SQL.
    match row.try_get::<_, Option<String>>(idx) {
        Ok(v) => v,
        Err(_) => Some(format!("<{}>", row.columns()[idx].type_().name())),
    }
}

/// The server's version number, e.g. `17.11`.
pub async fn server_version(url: &str, t: &Timeouts) -> Result<String> {
    let (client, driver, _) = connect(url, t).await?;
    let res = timeout(t.housekeeping, client.simple_query("SHOW server_version")).await;
    drop(client);
    let _ = timeout(t.housekeeping, driver).await;
    let version = res
        .context("SHOW server_version timed out")??
        .into_iter()
        .find_map(|m| match m {
            SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
            _ => None,
        });
    // "17.11 (Debian 17.11-1.pgdg13+2)" -> "17.11"
    let version = version.context("no server_version")?;
    Ok(version.split_whitespace().next().unwrap_or_default().to_string())
}

/// Retry `SELECT 1` through the pooler until it answers. A reset kills the pooler's
/// server connections, and it needs a moment to notice and open new ones.
pub(super) async fn wait_ready(url: &str, t: &Timeouts) -> Result<()> {
    let deadline = tokio::time::Instant::now() + t.ready;
    loop {
        let err = match connect(url, t).await {
            Ok((client, driver, _)) => {
                let res = timeout(t.housekeeping, client.simple_query("SELECT 1")).await;
                drop(client);
                let _ = timeout(t.housekeeping, driver).await;
                match res {
                    Ok(Ok(_)) => return Ok(()),
                    Ok(Err(e)) => anyhow::Error::from(e),
                    Err(_) => anyhow::anyhow!("SELECT 1 timed out"),
                }
            }
            Err(e) => e,
        };
        if tokio::time::Instant::now() >= deadline {
            return Err(err.context(format!("pooler not ready {:?} after reset", t.ready)));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
