//! `poolproof doctor`: checks a target before a run. It only reads: SELECTs, and
//! never a SET through the pooler, where it would leak to other clients.

use std::future::Future;
use std::time::Duration;

use serde::Serialize;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_postgres::{Client, SimpleQueryMessage};

use crate::scenario::PoolMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub check: &'static str,
    pub status: Status,
    pub detail: String,
}

impl Check {
    fn new(check: &'static str, status: Status, detail: impl Into<String>) -> Self {
        Check {
            check,
            status,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetReport {
    pub name: String,
    pub checks: Vec<Check>,
}

/// 0 when nothing failed, 1 otherwise. With `strict`, a warning fails too.
pub fn exit_code(reports: &[TargetReport], strict: bool) -> i32 {
    let limit = if strict { Status::Warn } else { Status::Fail };
    let bad = reports.iter().flat_map(|r| &r.checks).any(|c| c.status >= limit);
    i32::from(bad)
}

/// One line per check, then a summary.
pub fn text(reports: &[TargetReport]) -> String {
    let width = reports.iter().map(|r| r.name.len()).max().unwrap_or(0);
    let mut out = String::new();
    let (mut warn, mut fail) = (0, 0);
    for r in reports {
        for c in &r.checks {
            out.push_str(&format!(
                "{:<5} {:<width$} {:<12} {}\n",
                c.status, r.name, c.check, c.detail
            ));
            warn += usize::from(c.status == Status::Warn);
            fail += usize::from(c.status == Status::Fail);
        }
    }
    out.push_str(&format!(
        "\n{} targets checked: {fail} failed, {warn} warnings.\n",
        reports.len()
    ));
    out
}

pub fn json(reports: &[TargetReport]) -> String {
    serde_json::to_string_pretty(&serde_json::json!({ "targets": reports })).unwrap()
}

/// What tells two connections apart as the same Postgres server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFacts {
    pub database: String,
    pub started: String,
    pub version: String,
    /// The server's listen port, the same through any path to it.
    pub port: Option<String>,
    /// The address this connection reached the server on. Differs by path (the pooler
    /// may use a private network), so it's shown, never compared.
    pub addr: Option<String>,
}

pub const FACTS_SQL: &str = "SELECT current_database(), \
     to_char(pg_postmaster_start_time() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS'), \
     current_setting('server_version'), inet_server_port()::text, host(inet_server_addr())";

pub async fn server_facts(client: &Client, within: Duration) -> anyhow::Result<ServerFacts> {
    let row = first_row(client, FACTS_SQL, within).await?;
    let get = |i: usize| row.get(i).cloned().flatten();
    Ok(ServerFacts {
        database: get(0).unwrap_or_default(),
        started: get(1).unwrap_or_default(),
        version: get(2).unwrap_or_default(),
        port: get(3),
        addr: get(4),
    })
}

/// FAIL unless both URLs reach the same database on the same server: otherwise the
/// baseline isn't the Postgres behind the pooler and no verdict means anything.
pub fn same_server(pooler: &ServerFacts, direct: &ServerFacts) -> Check {
    let mut differ = vec![];
    if pooler.database != direct.database {
        differ.push(format!(
            "database {} via pooler, {} direct",
            pooler.database, direct.database
        ));
    }
    if pooler.started != direct.started {
        differ.push(format!(
            "started {} via pooler, {} direct",
            pooler.started, direct.started
        ));
    }
    if pooler.version != direct.version {
        differ.push(format!(
            "version {} via pooler, {} direct",
            pooler.version, direct.version
        ));
    }
    if let (Some(p), Some(d)) = (&pooler.port, &direct.port)
        && p != d
    {
        differ.push(format!("port {p} via pooler, {d} direct"));
    }
    let addr = |a: &Option<String>| a.as_deref().unwrap_or("socket").to_string();
    let addrs = format!(
        "(addr {} via pooler, {} direct)",
        addr(&pooler.addr),
        addr(&direct.addr)
    );
    if !differ.is_empty() {
        return Check::new("same server", Status::Fail, format!("{} {addrs}", differ.join("; ")));
    }
    let port = direct
        .port
        .as_deref()
        .map(|p| format!(", port {p}"))
        .unwrap_or_default();
    let detail = format!(
        "database {}{port}, started {}, {} {addrs}",
        direct.database, direct.started, direct.version
    );
    Check::new("same server", Status::Ok, detail)
}

/// What the mode probe saw: client a took a server connection, then client b asked for
/// one while a stayed connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// b got a's backend: the pooler shares server connections between clients.
    Shared,
    /// b got another backend: session mode, or a pool with a free connection.
    Separate,
    /// b got no reply in time: it waited for a's server connection.
    Waited,
}

/// A connected client and the task driving its connection.
pub type Connected = (Client, JoinHandle<()>);

/// Runs the mode probe. `connect` opens a client through the pooler. b's connect and
/// query are bounded by `within`, and a is closed before this returns.
pub async fn probe_mode<F, Fut>(connect: F, within: Duration) -> anyhow::Result<Observed>
where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<Connected>>,
{
    let (a, a_driver) = connect().await?;
    let a_pid = backend_pid(&a, within).await;
    let b = timeout(within, async {
        let (b, b_driver) = connect().await?;
        let pid = backend_pid(&b, within).await;
        drop(b);
        let _ = timeout(within, b_driver).await;
        pid
    })
    .await;
    drop(a);
    let _ = timeout(within, a_driver).await;
    let a_pid = a_pid?;
    let Ok(b_pid) = b else {
        return Ok(Observed::Waited);
    };
    Ok(if b_pid? == a_pid {
        Observed::Shared
    } else {
        Observed::Separate
    })
}

/// The mode check: what was seen, against the target's `mode`.
pub fn mode_check(observed: Observed, configured: PoolMode) -> Check {
    let (seen, matches) = match observed {
        Observed::Shared => (
            "transaction or statement pooling (b got a's server connection)",
            configured != PoolMode::Session,
        ),
        Observed::Waited => (
            "session mode (b waited for a's server connection; a pool exhausted by other clients looks the same)",
            configured == PoolMode::Session,
        ),
        Observed::Separate => ("not determined (b got another server connection)", true),
    };
    let status = if matches { Status::Ok } else { Status::Warn };
    let detail = if matches {
        seen.to_string()
    } else {
        format!("{seen}, but the target says mode = {configured}")
    };
    Check::new("mode", status, detail)
}

async fn backend_pid(client: &Client, within: Duration) -> anyhow::Result<String> {
    let row = first_row(client, "SELECT pg_backend_pid()", within).await?;
    row.into_iter()
        .next()
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("pg_backend_pid() returned NULL"))
}

async fn first_row(client: &Client, sql: &str, within: Duration) -> anyhow::Result<Vec<Option<String>>> {
    let messages = timeout(within, client.simple_query(sql))
        .await
        .map_err(|_| anyhow::anyhow!("no reply within {within:?}"))??;
    messages
        .into_iter()
        .find_map(|m| match m {
            SimpleQueryMessage::Row(row) => Some((0..row.len()).map(|i| row.get(i).map(str::to_string)).collect()),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("no row"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI32, Ordering};

    use bytes::{BufMut, BytesMut};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Semaphore;

    use super::*;

    fn facts(port: &str, addr: &str) -> ServerFacts {
        ServerFacts {
            database: "pp".into(),
            started: "2026-10-31 21:04:11".into(),
            version: "17.11".into(),
            port: Some(port.into()),
            addr: Some(addr.into()),
        }
    }

    #[test]
    fn same_server_compares_the_port_and_only_shows_addresses() {
        let ok = same_server(&facts("5432", "172.18.0.2"), &facts("5432", "127.0.0.1"));
        assert_eq!(ok.status, Status::Ok);
        assert_eq!(
            ok.detail,
            "database pp, port 5432, started 2026-10-31 21:04:11, 17.11 (addr 172.18.0.2 via pooler, 127.0.0.1 direct)"
        );
        let other = same_server(&facts("5433", "10.0.0.1"), &facts("5432", "127.0.0.1"));
        assert_eq!(other.status, Status::Fail);
        assert!(
            other.detail.starts_with("port 5433 via pooler, 5432 direct"),
            "{}",
            other.detail
        );
        let mut restarted = facts("5432", "127.0.0.1");
        restarted.started = "2026-10-31 22:00:00".into();
        assert_eq!(
            same_server(&restarted, &facts("5432", "127.0.0.1")).status,
            Status::Fail
        );
    }

    #[test]
    fn mode_check_warns_when_the_target_says_otherwise() {
        assert_eq!(mode_check(Observed::Shared, PoolMode::Transaction).status, Status::Ok);
        assert_eq!(mode_check(Observed::Shared, PoolMode::Session).status, Status::Warn);
        assert_eq!(mode_check(Observed::Waited, PoolMode::Session).status, Status::Ok);
        assert_eq!(mode_check(Observed::Waited, PoolMode::Transaction).status, Status::Warn);
        assert_eq!(mode_check(Observed::Separate, PoolMode::Transaction).status, Status::Ok);
    }

    #[test]
    fn strict_fails_on_warnings() {
        let report = |status| TargetReport {
            name: "t".into(),
            checks: vec![Check::new("mode", status, "")],
        };
        assert_eq!(exit_code(&[report(Status::Ok)], true), 0);
        assert_eq!(exit_code(&[report(Status::Warn)], false), 0);
        assert_eq!(exit_code(&[report(Status::Warn)], true), 1);
        assert_eq!(exit_code(&[report(Status::Fail)], false), 1);
    }

    /// How the fake pooler hands out backend pids.
    #[derive(Clone, Copy)]
    enum Pool {
        /// Every client gets backend 7, like transaction pooling.
        Shared,
        /// Each client gets its own backend.
        Separate,
        /// One backend, held by a client until it disconnects, like a full session pool.
        SessionOfOne,
    }

    /// A Postgres server that knows only startup and `SELECT pg_backend_pid()`.
    async fn fake_pooler(pool: Pool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let next = Arc::new(AtomicI32::new(100));
        let backend = Arc::new(Semaphore::new(1));
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let pid = match pool {
                    Pool::Shared => 7,
                    _ => next.fetch_add(1, Ordering::SeqCst),
                };
                let backend = backend.clone();
                tokio::spawn(async move {
                    let _held = match pool {
                        Pool::SessionOfOne => Some(backend.acquire_owned().await.unwrap()),
                        _ => None,
                    };
                    let _ = serve(socket, pid).await;
                });
            }
        });
        format!("postgres://pp@127.0.0.1:{port}/pp?sslmode=disable")
    }

    async fn serve(mut socket: TcpStream, pid: i32) -> std::io::Result<()> {
        let len = socket.read_i32().await?;
        let mut startup = vec![0; len as usize - 4];
        socket.read_exact(&mut startup).await?;
        let mut out = BytesMut::new();
        message(&mut out, b'R', |b| b.put_i32(0));
        message(&mut out, b'K', |b| {
            b.put_i32(pid);
            b.put_i32(0);
        });
        message(&mut out, b'Z', |b| b.put_u8(b'I'));
        socket.write_all(&out).await?;
        loop {
            let kind = socket.read_u8().await?;
            let len = socket.read_i32().await?;
            let mut body = vec![0; len as usize - 4];
            socket.read_exact(&mut body).await?;
            if kind != b'Q' {
                return Ok(());
            }
            let value = pid.to_string();
            let mut out = BytesMut::new();
            message(&mut out, b'T', |b| {
                b.put_i16(1);
                b.put_slice(b"pg_backend_pid\0");
                b.put_i32(0);
                b.put_i16(0);
                b.put_u32(23);
                b.put_i16(4);
                b.put_i32(-1);
                b.put_i16(0);
            });
            message(&mut out, b'D', |b| {
                b.put_i16(1);
                b.put_i32(value.len() as i32);
                b.put_slice(value.as_bytes());
            });
            message(&mut out, b'C', |b| b.put_slice(b"SELECT 1\0"));
            message(&mut out, b'Z', |b| b.put_u8(b'I'));
            socket.write_all(&out).await?;
        }
    }

    fn message(out: &mut BytesMut, kind: u8, body: impl FnOnce(&mut BytesMut)) {
        let mut b = BytesMut::new();
        body(&mut b);
        out.put_u8(kind);
        out.put_i32(b.len() as i32 + 4);
        out.put_slice(&b);
    }

    async fn probe(pool: Pool) -> Observed {
        let url = fake_pooler(pool).await;
        let connect = || async {
            let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await?;
            Ok((client, tokio::spawn(async move { drop(connection.await) })))
        };
        probe_mode(connect, Duration::from_millis(500)).await.unwrap()
    }

    #[tokio::test]
    async fn mode_probe_tells_shared_separate_and_waiting_apart() {
        assert_eq!(probe(Pool::Shared).await, Observed::Shared);
        assert_eq!(probe(Pool::Separate).await, Observed::Separate);
        assert_eq!(probe(Pool::SessionOfOne).await, Observed::Waited);
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn server_facts_reads_a_real_server() {
        let url = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
        let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.unwrap();
        let driver = tokio::spawn(async move { drop(connection.await) });
        let one = server_facts(&client, Duration::from_secs(5)).await.unwrap();
        let two = server_facts(&client, Duration::from_secs(5)).await.unwrap();
        drop(client);
        let _ = driver.await;
        assert!(one.port.is_some() && !one.started.is_empty(), "{one:?}");
        assert_eq!(same_server(&one, &two).status, Status::Ok);
    }

    #[test]
    fn json_lists_checks_per_target() {
        let reports = [TargetReport {
            name: "pgbouncer".into(),
            checks: vec![Check::new("mode", Status::Warn, "x")],
        }];
        let v: serde_json::Value = serde_json::from_str(&json(&reports)).unwrap();
        assert_eq!(v["targets"][0]["checks"][0]["status"], "warn");
        assert_eq!(v["targets"][0]["checks"][0]["check"], "mode");
    }
}
