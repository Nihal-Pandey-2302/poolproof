use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll, ready};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_postgres::config::{Host, SslMode};
use tokio_postgres::tls::MakeTlsConnect;
use tokio_postgres::{AsyncMessage, CancelToken, Client, Connection, NoTls, SimpleQueryMessage, Statement};
use tokio_postgres_rustls::MakeRustlsConnect;

use super::tls::wants_client_certificate;
use super::{Outcome, Timeouts};
use crate::scenario::Requirement;
use crate::targets::Endpoint;

/// NOTIFY messages received on a connection, as (channel, payload) rows.
pub(super) type Notices = Arc<Mutex<Vec<Vec<Option<String>>>>>;

pub(super) struct Conn {
    pub(super) client: Arc<Client>,
    pub(super) driver: JoinHandle<()>,
    pub(super) statements: HashMap<String, Statement>,
    /// The query started by a `background` step, if not yet waited for.
    pub(super) pending: Option<JoinHandle<Outcome>>,
    pub(super) notices: Notices,
    /// Connected straight to Postgres, so closing waits for the backend to exit.
    pub(super) on_postgres: bool,
}

impl Conn {
    /// Close the connection and wait for Terminate to go out, so the pooler sees the
    /// disconnect before the next step runs. On Postgres it also waits for the backend
    /// to exit (see `DrainOnShutdown`), and fails if it doesn't in time.
    pub(super) async fn close(self, t: &Timeouts) -> Result<()> {
        if let Some(pending) = self.pending {
            pending.abort();
        }
        drop(self.client);
        if timeout(t.housekeeping, self.driver).await.is_err() && self.on_postgres {
            bail!("the backend did not exit within {:?} of the disconnect", t.housekeeping);
        }
        Ok(())
    }
}

/// Any byte stream a connection can run over.
trait Io: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// The stream under a tokio-postgres connection. With `drain`, shutting it down after
/// Terminate also reads until the server closes its end, which on Postgres means the
/// backend's session state is gone: proc_exit_prepare runs shmem_exit (the
/// before_shmem_exit and on_shmem_exit callbacks: lock release, temp schema drop,
/// leaving pg_stat_activity) before the on_proc_exit callbacks, among them the
/// socket_close that pq_init registers. socket_close sends TLS close_notify
/// (secure_close) and leaves the socket to the kernel, which closes it when the
/// process exits. Both come after shmem_exit; checked in the PostgreSQL 13 to 18
/// sources. Over TLS this sits under the TLS layer: close_notify arrives as ordinary
/// bytes here, so the wait ends at EOF. (`send` clients read above TLS, so for them
/// close_notify or EOF, whichever comes first, ends it.)
struct DrainOnShutdown<S> {
    inner: S,
    drain: bool,
    write_closed: bool,
}

impl<S: AsyncRead + Unpin> AsyncRead for DrainOnShutdown<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for DrainOnShutdown<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if !this.write_closed {
            ready!(Pin::new(&mut this.inner).poll_shutdown(cx))?;
            this.write_closed = true;
        }
        while this.drain {
            let mut scratch = [0u8; 512];
            let mut buf = ReadBuf::new(&mut scratch);
            match ready!(Pin::new(&mut this.inner).poll_read(cx, &mut buf)) {
                Ok(()) if !buf.filled().is_empty() => {}
                // EOF, or an error that also means the connection is gone.
                _ => this.drain = false,
            }
        }
        Poll::Ready(Ok(()))
    }
}

/// Opens the socket to an endpoint's first host. Returns it with the host name, for TLS.
async fn open(endpoint: &Endpoint) -> Result<(Box<dyn Io>, String)> {
    let config = endpoint.config();
    let port = config.get_ports().first().copied().unwrap_or(5432);
    match config.get_hosts().first() {
        Some(Host::Tcp(host)) => {
            let stream = TcpStream::connect((host.as_str(), port)).await?;
            stream.set_nodelay(true)?;
            // As tokio-postgres does on the sockets it opens: keepalives after 2 hours idle.
            let keepalive = socket2::TcpKeepalive::new().with_time(Duration::from_secs(2 * 60 * 60));
            socket2::SockRef::from(&stream).set_tcp_keepalive(&keepalive)?;
            Ok((Box::new(stream), host.clone()))
        }
        #[cfg(unix)]
        Some(Host::Unix(dir)) => {
            let stream = tokio::net::UnixStream::connect(dir.join(format!(".s.PGSQL.{port}"))).await?;
            Ok((Box::new(stream), String::new()))
        }
        None => bail!("no host in the connection URL"),
    }
}

pub(super) async fn connect(endpoint: &Endpoint, t: &Timeouts) -> Result<(Client, JoinHandle<()>, Notices)> {
    connect_with(endpoint, t, false).await
}

/// Connects; with `wait_for_exit` (connections straight to Postgres), closing the
/// connection later waits for the backend to exit.
pub(super) async fn connect_with(
    endpoint: &Endpoint,
    t: &Timeouts,
    wait_for_exit: bool,
) -> Result<(Client, JoinHandle<()>, Notices)> {
    let notices = Notices::default();
    let connecting = async {
        let (stream, host) = open(endpoint).await?;
        let stream = DrainOnShutdown {
            inner: stream,
            drain: wait_for_exit,
            write_closed: false,
        };
        let connected = match (&endpoint.tls().config, host.is_empty()) {
            // A Unix socket never uses TLS, as in libpq.
            (None, _) | (Some(_), true) => {
                let mut config = endpoint.config().clone();
                if host.is_empty() {
                    config.ssl_mode(SslMode::Disable);
                }
                let (client, connection) = config.connect_raw(stream, NoTls).await?;
                (client, spawn_driver(connection, notices.clone()))
            }
            (Some(tls), false) => {
                let mut make = MakeRustlsConnect::new((**tls).clone());
                let tls = MakeTlsConnect::<DrainOnShutdown<Box<dyn Io>>>::make_tls_connect(&mut make, &host)?;
                let (client, connection) = match endpoint.config().connect_raw(stream, tls).await {
                    Ok(connected) => connected,
                    Err(e) if wants_client_certificate(&e) => {
                        return Err(anyhow::Error::from(e)
                            .context("the server wants a client certificate, which poolproof doesn't support yet"));
                    }
                    Err(e) => return Err(e.into()),
                };
                (client, spawn_driver(connection, notices.clone()))
            }
        };
        Ok::<_, anyhow::Error>(connected)
    };
    let (client, driver) = timeout(t.connect, connecting).await.context("connect timed out")??;
    Ok((client, driver, notices))
}

/// Sends a CancelRequest for the query running on the client's connection, over TLS
/// when the endpoint uses it.
pub(super) async fn cancel(token: &CancelToken, endpoint: &Endpoint) -> Result<()> {
    let (stream, host) = open(endpoint).await?;
    match (&endpoint.tls().config, host.is_empty()) {
        (None, _) | (Some(_), true) => token.cancel_query_raw(stream, NoTls).await?,
        (Some(tls), false) => {
            let mut make = MakeRustlsConnect::new((**tls).clone());
            let tls = MakeTlsConnect::<Box<dyn Io>>::make_tls_connect(&mut make, &host)?;
            token.cancel_query_raw(stream, tls).await?
        }
    }
    Ok(())
}

/// Drives a connection until it closes, keeping the NOTIFYs it receives in `sink`.
fn spawn_driver<S, T>(mut connection: Connection<S, T>, sink: Notices) -> JoinHandle<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    // Polling messages instead of just awaiting the connection keeps the NOTIFYs.
    tokio::spawn(async move {
        while let Some(Ok(message)) = std::future::poll_fn(|cx| connection.poll_message(cx)).await {
            if let AsyncMessage::Notification(n) = message {
                let row = vec![Some(n.channel().to_string()), Some(n.payload().to_string())];
                sink.lock().unwrap().push(row);
            }
        }
    })
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

/// What the `direct` user may do, for skipping scenarios that need more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Privileges {
    pub superuser: bool,
    pub createrole: bool,
    pub pg_signal_backend: bool,
}

impl Privileges {
    /// Every requirement these privileges don't meet, as a SKIP reason.
    pub fn missing(&self, requires: &[Requirement]) -> Option<String> {
        let unmet: Vec<String> = requires
            .iter()
            .filter(|r| {
                !self.superuser
                    && match r {
                        Requirement::Superuser => true,
                        Requirement::Createrole => !self.createrole,
                        Requirement::PgSignalBackend => !self.pg_signal_backend,
                    }
            })
            .map(|r| r.to_string())
            .collect();
        (!unmet.is_empty()).then(|| format!("needs {} on direct", unmet.join(", ")))
    }
}

/// The privileges of the user `direct` connects as.
pub async fn privileges(direct: &Endpoint, t: &Timeouts) -> Result<Privileges> {
    let (client, driver, _) = connect(direct, t).await?;
    let sql = "SELECT rolsuper, rolcreaterole, pg_has_role(current_user, 'pg_signal_backend', 'MEMBER') \
               FROM pg_roles WHERE rolname = current_user";
    let res = timeout(t.housekeeping, client.simple_query(sql)).await;
    drop(client);
    let _ = timeout(t.housekeeping, driver).await;
    let row = res
        .context("privilege check timed out")??
        .into_iter()
        .find_map(|m| match m {
            SimpleQueryMessage::Row(row) => Some(row),
            _ => None,
        })
        .context("current user not in pg_roles")?;
    let flag = |i: usize| row.get(i) == Some("t");
    Ok(Privileges {
        superuser: flag(0),
        createrole: flag(1),
        pg_signal_backend: flag(2),
    })
}

/// The server's version number, e.g. `17.11`.
pub async fn server_version(endpoint: &Endpoint, t: &Timeouts) -> Result<String> {
    let (client, driver, _) = connect(endpoint, t).await?;
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
pub(super) async fn wait_ready(endpoint: &Endpoint, t: &Timeouts) -> Result<()> {
    let deadline = tokio::time::Instant::now() + t.ready;
    loop {
        let err = match connect(endpoint, t).await {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_waits_for_the_server_to_close_when_draining() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for drain in [true, false] {
            let (client, mut server) = tokio::io::duplex(64);
            // The server sees the client's end close, then takes its time to close its own.
            tokio::spawn(async move {
                let mut buf = Vec::new();
                server.read_to_end(&mut buf).await.unwrap();
                tokio::time::sleep(Duration::from_millis(300)).await;
                let _ = server.write_all(b"bye").await;
            });
            let mut stream = DrainOnShutdown {
                inner: client,
                drain,
                write_closed: false,
            };
            let started = tokio::time::Instant::now();
            stream.shutdown().await.unwrap();
            let waited = started.elapsed() >= Duration::from_millis(300);
            assert_eq!(waited, drain, "drain = {drain}");
        }
    }

    #[test]
    fn missing_privileges() {
        let none = Privileges {
            superuser: false,
            createrole: false,
            pg_signal_backend: false,
        };
        let createrole = Privileges {
            createrole: true,
            ..none
        };
        let superuser = Privileges {
            superuser: true,
            ..none
        };
        let all = [
            Requirement::Createrole,
            Requirement::PgSignalBackend,
            Requirement::Superuser,
        ];
        assert_eq!(none.missing(&[]), None);
        assert_eq!(
            none.missing(&all).as_deref(),
            Some("needs createrole, pg_signal_backend, superuser on direct")
        );
        assert_eq!(
            createrole.missing(&all).as_deref(),
            Some("needs pg_signal_backend, superuser on direct")
        );
        assert_eq!(createrole.missing(&[Requirement::Createrole]), None);
        assert_eq!(superuser.missing(&all), None);
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn the_step_timeout_does_not_limit_the_privilege_check() {
        let url = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
        let t = Timeouts {
            step: Duration::from_nanos(1),
            ..Timeouts::default()
        };
        privileges(&Endpoint::parse(&url).unwrap(), &t).await.unwrap();
    }
}
