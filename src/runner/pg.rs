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
use tokio::time::{timeout, timeout_at};
use tokio_postgres::config::{Config, Host, SslMode};
use tokio_postgres::tls::MakeTlsConnect;
use tokio_postgres::{AsyncMessage, CancelToken, Client, Connection, NoTls, SimpleQueryMessage, Statement};
use tokio_postgres_rustls::MakeRustlsConnect;

use super::tls::wants_client_certificate;
use super::{Outcome, Timeouts};
use crate::scenario::Requirement;
use crate::targets::{Endpoint, Reset, Statement as TargetStatement, Target};

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

/// Opens a TCP connection with the URL's socket options: `connect_timeout` for the TCP
/// connect (as tokio-postgres; the whole connect also has the harness's own limit),
/// `tcp_user_timeout` in milliseconds as in libpq, and keepalives (on unless
/// `keepalives=0`, with `keepalives_idle`, `_interval` and `_retries`). An option this
/// system can't set is an error, not ignored.
pub(super) async fn tcp_connect(config: &Config, host: &str, port: u16) -> Result<TcpStream> {
    let connecting = TcpStream::connect((host, port));
    let stream = match config.get_connect_timeout() {
        Some(limit) => timeout(*limit, connecting)
            .await
            .with_context(|| format!("no connection within connect_timeout ({limit:?})"))??,
        None => connecting.await?,
    };
    stream.set_nodelay(true)?;
    let socket = socket2::SockRef::from(&stream);
    if let Some(parsed) = config.get_tcp_user_timeout() {
        // libpq's tcp_user_timeout is in milliseconds; tokio-postgres reads the number
        // as seconds.
        set_user_timeout(&socket, Duration::from_millis(parsed.as_secs()))?;
    }
    if config.get_keepalives() {
        let keepalive = socket2::TcpKeepalive::new().with_time(config.get_keepalives_idle());
        let keepalive = with_interval_and_retries(
            keepalive,
            config.get_keepalives_interval(),
            config.get_keepalives_retries(),
        )?;
        socket.set_tcp_keepalive(&keepalive)?;
    }
    Ok(stream)
}

// The targets socket2 0.6.5 sets each option on, copied from its cfgs.
#[cfg(any(
    target_os = "android",
    target_os = "cygwin",
    target_os = "fuchsia",
    target_os = "linux"
))]
fn set_user_timeout(socket: &socket2::SockRef<'_>, limit: Duration) -> Result<()> {
    Ok(socket.set_tcp_user_timeout(Some(limit))?)
}

#[cfg(not(any(
    target_os = "android",
    target_os = "cygwin",
    target_os = "fuchsia",
    target_os = "linux"
)))]
fn set_user_timeout(_: &socket2::SockRef<'_>, limit: Duration) -> Result<()> {
    bail!("not supported on this system: tcp_user_timeout ({limit:?})")
}

#[cfg(any(
    target_os = "android",
    target_os = "cygwin",
    target_os = "dragonfly",
    target_os = "emscripten",
    target_os = "freebsd",
    target_os = "fuchsia",
    target_os = "illumos",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "nuttx",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "windows",
    all(target_os = "wasi", not(target_env = "p1")),
))]
fn with_interval_and_retries(
    mut keepalive: socket2::TcpKeepalive,
    interval: Option<Duration>,
    retries: Option<u32>,
) -> Result<socket2::TcpKeepalive> {
    if let Some(interval) = interval {
        keepalive = keepalive.with_interval(interval);
    }
    if let Some(retries) = retries {
        keepalive = keepalive.with_retries(retries);
    }
    Ok(keepalive)
}

#[cfg(not(any(
    target_os = "android",
    target_os = "cygwin",
    target_os = "dragonfly",
    target_os = "emscripten",
    target_os = "freebsd",
    target_os = "fuchsia",
    target_os = "illumos",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "nuttx",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "windows",
    all(target_os = "wasi", not(target_env = "p1")),
)))]
fn with_interval_and_retries(
    keepalive: socket2::TcpKeepalive,
    interval: Option<Duration>,
    retries: Option<u32>,
) -> Result<socket2::TcpKeepalive> {
    if let Some(interval) = interval {
        bail!("not supported on this system: keepalives_interval ({interval:?})");
    }
    if let Some(retries) = retries {
        bail!("not supported on this system: keepalives_retries ({retries})");
    }
    Ok(keepalive)
}

/// Opens the socket to an endpoint's first host. Returns it with the host name, for TLS.
async fn open(endpoint: &Endpoint) -> Result<(Box<dyn Io>, String)> {
    let config = endpoint.config();
    let port = config.get_ports().first().copied().unwrap_or(5432);
    match config.get_hosts().first() {
        Some(Host::Tcp(host)) => Ok((Box::new(tcp_connect(config, host, port).await?), host.clone())),
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
        let connected = match (endpoint.tls().client_config()?, host.is_empty()) {
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
                let mut make = MakeRustlsConnect::new((*tls).clone());
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
    match (endpoint.tls().client_config()?, host.is_empty()) {
        (None, _) | (Some(_), true) => token.cancel_query_raw(stream, NoTls).await?,
        (Some(tls), false) => {
            let mut make = MakeRustlsConnect::new((*tls).clone());
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
    pub pg_read_all_stats: bool,
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
    // USAGE, not MEMBER: the server checks the privileges a role has, which a NOINHERIT
    // membership doesn't give.
    let sql = "SELECT rolsuper, rolcreaterole, pg_has_role(current_user, 'pg_signal_backend', 'USAGE'), \
               pg_has_role(current_user, 'pg_read_all_stats', 'USAGE') FROM pg_roles WHERE rolname = current_user";
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
        pg_read_all_stats: flag(3),
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

/// The statement `reset = "all"` runs: end every other session on the database that
/// this user may end, and wait up to 5 s for each to exit. Without superuser, that is
/// sessions of roles whose privileges it has, and with pg_signal_backend those of other
/// roles, but never a superuser's: only a superuser may end those, even for a user that
/// has a superuser role's privileges. resolve_reset checks the pooler's sessions are
/// among them. The filter keeps another role's session from making the reset fail.
const RESET_ALL: &str = "SELECT pg_terminate_backend(pid, 5000) FROM pg_stat_activity \
    WHERE datname = current_database() AND pid <> pg_backend_pid() AND backend_type = 'client backend' \
    AND (NOT (SELECT rolsuper FROM pg_roles WHERE oid = usesysid) \
    OR (SELECT rolsuper FROM pg_roles WHERE rolname = current_user)) \
    AND (pg_has_role(usesysid, 'USAGE') OR pg_has_role('pg_signal_backend', 'USAGE'))";

/// `reset = "auto"`: the same, for the sessions from one address and user only.
/// Both values come from the system under test, so neither may steer the statement:
/// the address is a parsed IpAddr, and the user an E'' literal with backslashes and
/// quotes doubled, safe whatever standard_conforming_strings is.
fn reset_auto_sql(addr: std::net::IpAddr, user: &str) -> String {
    let user = literal(user);
    format!(
        "SELECT pg_terminate_backend(pid, 5000) FROM pg_stat_activity WHERE datname = current_database() \
         AND client_addr = '{addr}'::inet AND usename = {user} AND pid <> pg_backend_pid() \
         AND backend_type = 'client backend'"
    )
}

/// A string as an E'' literal, backslashes and quotes doubled.
fn literal(text: &str) -> String {
    format!("E'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
}

/// A role name as a quoted identifier for GRANT: quoted always, so a reserved word like
/// `user` works too.
fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// What the `direct` user can do about the sessions of the pooler's user.
struct Reach {
    /// The pooler's user is a superuser.
    superuser: bool,
    /// The direct user has its privileges (it is that user, or inherits from it).
    privileges_of: bool,
    pg_signal_backend: bool,
    pg_read_all_stats: bool,
    /// The direct user, for the GRANT.
    me: String,
}

/// Why a reset can't end the sessions of `user`, the role the pooler connects as, or
/// None if it can. Without its privileges, the direct user needs pg_read_all_stats to
/// see them (pg_stat_activity hides another role's client address and backend type, so
/// the reset would match nothing) and pg_signal_backend to end them. Only a superuser
/// can end a superuser's sessions.
fn cannot_end(user: &str, reach: &Reach) -> Option<(String, String)> {
    if reach.superuser {
        return Some((
            format!(
                "{user} is a superuser, and only a superuser can end its sessions; connect direct as a \
                 superuser, or let the pooler connect as a role that isn't one"
            ),
            "the pooler's user is a superuser".into(),
        ));
    }
    if reach.privileges_of {
        return None;
    }
    let missing: Vec<&str> = [
        (!reach.pg_signal_backend).then_some("pg_signal_backend"),
        (!reach.pg_read_all_stats).then_some("pg_read_all_stats"),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!missing.is_empty()).then(|| {
        (
            format!(
                "this user can't end sessions of {user} without {}; as a superuser: GRANT {} TO {}",
                missing.join(" and "),
                missing.join(", "),
                ident(&reach.me)
            ),
            format!("missing {}", missing.join(" and ")),
        )
    })
}

/// Why resolve_reset won't run a reset. Displays as the target's ERROR reason.
#[derive(Debug)]
pub enum ResetRefusal {
    /// `all` against a Postgres that isn't on this machine, without `run --yes`.
    NeedsYes(String),
    /// `auto` can't tell which sessions are the pooler's.
    CantTell(String),
    /// The direct user can't see or end the pooler's sessions. `public` says why
    /// without naming roles: "... missing pg_signal_backend", "... the pooler's user is
    /// a superuser", or that the check failed.
    CantEnd { text: String, public: String },
}

impl std::fmt::Display for ResetRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResetRefusal::NeedsYes(text) | ResetRefusal::CantTell(text) | ResetRefusal::CantEnd { text, .. } => {
                f.write_str(text)
            }
        }
    }
}

/// For "all" and "auto": whether the direct user can end the pooler's sessions, asking
/// the pooler which user it connects as unless `pooler_user` says. Ok(true) for a
/// superuser, who can end every session.
async fn check_reach(
    target: &Target,
    kind: &str,
    pooler_user: Option<&str>,
    t: &Timeouts,
) -> Result<bool, ResetRefusal> {
    let refuse_as = |why: String, public: &str| ResetRefusal::CantEnd {
        text: format!("reset = \"{kind}\" can't end the pooler's sessions: {why}"),
        public: format!("can't end the pooler's sessions: {public}"),
    };
    let refuse = |why: String| refuse_as(why, "the check failed");
    let privs = privileges(&target.direct, t)
        .await
        .map_err(|e| refuse(format!("checking the direct user's privileges failed: {e:#}")))?;
    if privs.superuser {
        return Ok(true);
    }
    let user = match pooler_user {
        Some(user) => user.to_string(),
        None => query_row(&target.pooler, "SELECT session_user", t)
            .await
            .ok()
            .and_then(|row| row.into_iter().next().flatten())
            .ok_or_else(|| refuse("asking the pooler which user it connects as failed".into()))?,
    };
    let sql = format!(
        "SELECT rolsuper, pg_has_role(current_user, oid, 'USAGE'), current_user FROM pg_roles WHERE rolname = {}",
        literal(&user)
    );
    let row = query_row(&target.direct, &sql, t).await.map_err(|_| {
        refuse(format!(
            "{user}, the user the pooler connects as, isn't a role on direct"
        ))
    })?;
    let flag = |i: usize| row.get(i).cloned().flatten().as_deref() == Some("t");
    let reach = Reach {
        superuser: flag(0),
        privileges_of: flag(1),
        pg_signal_backend: privs.pg_signal_backend,
        pg_read_all_stats: privs.pg_read_all_stats,
        me: row.get(2).cloned().flatten().unwrap_or_default(),
    };
    match cannot_end(&user, &reach) {
        Some((why, public)) => Err(refuse_as(why, &public)),
        None => Ok(false),
    }
}

/// Whether a host is this machine: a Unix socket, `localhost` or a loopback address.
fn is_local(config: &tokio_postgres::Config) -> bool {
    match config.get_hosts().first() {
        Some(Host::Tcp(host)) => {
            host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
        }
        #[cfg(unix)]
        Some(Host::Unix(_)) => true,
        None => false,
    }
}

/// One row of text values from a query, or an error.
async fn query_row(endpoint: &Endpoint, sql: &str, t: &Timeouts) -> Result<Vec<Option<String>>> {
    let (client, driver, _) = connect(endpoint, t).await?;
    let res = timeout(t.housekeeping, client.simple_query(sql)).await;
    drop(client);
    let _ = timeout(t.housekeeping, driver).await;
    res.context("timed out")??
        .into_iter()
        .find_map(|m| match m {
            SimpleQueryMessage::Row(row) => Some((0..row.len()).map(|i| row.get(i).map(str::to_string)).collect()),
            _ => None,
        })
        .context("no row")
}

/// Turns a target's `reset` into the statements to run before each run, plus the line
/// that says what they touch (empty for no reset). The error is the target's ERROR
/// reason. `yes` allows `all` against a Postgres that isn't on this machine.
pub async fn resolve_reset(
    target: &Target,
    yes: bool,
    t: &Timeouts,
) -> Result<(Vec<TargetStatement>, String), ResetRefusal> {
    let config = target.direct.config();
    let db = config.get_dbname().or(config.get_user()).unwrap_or_default();
    let host = target.direct.host();
    let name = &target.name;
    match &target.reset {
        Reset::None => Ok((vec![], String::new())),
        Reset::Statements(statements) => Ok((
            statements.clone(),
            format!(
                "reset: '{name}': runs {} statements on direct before each run (custom)",
                statements.len()
            ),
        )),
        Reset::All if !yes && !is_local(config) => Err(ResetRefusal::NeedsYes(format!(
            "reset = \"all\" ends every other session on database {db} at {host}; pass --yes to allow it"
        ))),
        Reset::All => {
            let superuser = check_reach(target, "all", None, t).await?;
            // Without superuser, sessions of roles this user may not end stay.
            let which = if superuser { "" } else { " that this user may end" };
            Ok((
                vec![TargetStatement::plain(RESET_ALL)],
                format!("reset: '{name}': ends every other session{which} on database {db} at {host} (all)"),
            ))
        }
        Reset::Auto => {
            let refuse = |why: String| {
                ResetRefusal::CantTell(format!(
                    "reset = \"auto\" can't tell the pooler's sessions apart: {why}; use reset = \"all\" or your own statements"
                ))
            };
            let pooler = query_row(&target.pooler, "SELECT host(inet_client_addr()), session_user", t)
                .await
                .map_err(|e| refuse(format!("asking through the pooler failed: {e:#}")))?;
            let ours = query_row(&target.direct, "SELECT host(inet_client_addr())", t)
                .await
                .map_err(|e| refuse(format!("asking direct failed: {e:#}")))?;
            let (Some(Some(addr)), Some(Some(user))) = (pooler.first(), pooler.get(1)) else {
                return Err(refuse(
                    "Postgres sees no client address for them (a Unix socket?)".into(),
                ));
            };
            let Ok(addr) = addr.parse::<std::net::IpAddr>() else {
                return Err(refuse(
                    "the address Postgres reports for the pooler isn't an IP address".into(),
                ));
            };
            // 10.0.0.1 and ::ffff:10.0.0.1 are the same host.
            let ours = ours
                .first()
                .cloned()
                .flatten()
                .and_then(|a| a.parse::<std::net::IpAddr>().ok());
            if ours.is_some_and(|ours| ours.to_canonical() == addr.to_canonical()) {
                return Err(refuse(format!(
                    "the pooler connects from {addr}, the same address as poolproof"
                )));
            }
            check_reach(target, "auto", Some(user), t).await?;
            // Anything else from that address and user goes too: a NAT or a sidecar
            // that shares the pooler's address can't be told apart from it.
            Ok((
                vec![TargetStatement::plain(&reset_auto_sql(addr, user))],
                format!(
                    "reset: '{name}': ends every session of user {user} from {addr} (not only the pooler's) \
                     on database {db} at {host} (auto)"
                ),
            ))
        }
    }
}

/// Retry `SELECT 1` through the pooler until it answers, then let it settle (see
/// `settle`). A reset kills the pooler's server connections, and it needs a moment to
/// notice and open new ones. Errors only if the pooler never answers; returns whether
/// it settled.
pub(super) async fn wait_ready(endpoint: &Endpoint, t: &Timeouts) -> Result<bool> {
    let deadline = tokio::time::Instant::now() + t.ready;
    let mut answered = false;
    // Settling is skipped for poolers already seen rotating their server connections.
    let skip_settle = ROTATING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&endpoint.to_string());
    loop {
        let err = match connect(endpoint, t).await {
            Ok((client, driver, _)) => {
                let res = match timeout(t.housekeeping, client.simple_query("SELECT 1")).await {
                    Ok(Ok(_)) if skip_settle => Ok(Settle::Unstable),
                    Ok(Ok(_)) => {
                        answered = true;
                        Ok(settle(&client, t).await)
                    }
                    Ok(Err(e)) => Err(anyhow::Error::from(e)),
                    Err(_) => Err(anyhow::anyhow!("SELECT 1 timed out")),
                };
                drop(client);
                if matches!(res, Ok(Settle::Stable)) || skip_settle {
                    let _ = timeout(t.housekeeping, driver).await;
                } else {
                    // A probe may still be waiting for its reply; don't wait for it.
                    driver.abort();
                }
                match res {
                    Ok(Settle::Stable) => return Ok(true),
                    Ok(Settle::Unstable) => return Ok(false),
                    Ok(Settle::Rotating) => {
                        eprintln!("note: pooler {endpoint} rotates server connections; settle skipped from now on");
                        ROTATING
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push(endpoint.to_string());
                        return Ok(false);
                    }
                    // The server connection went away while settling: start over.
                    Ok(Settle::Broken) => anyhow::anyhow!("server connection changed while settling"),
                    Err(e) => e,
                }
            }
            Err(e) => e,
        };
        if tokio::time::Instant::now() >= deadline {
            // It answered at least once: go on, settling is best effort.
            if answered {
                return Ok(false);
            }
            return Err(err.context(format!("pooler not ready {:?} after reset", t.ready)));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Poolers (by endpoint) that answered from SETTLE_PROBES different backends without
/// settling; see `settle`. A pooler that was only slow isn't added: Supavisor 2.10
/// sometimes needs more than SETTLE_MAX to replace its server connection.
static ROTATING: Mutex<Vec<String>> = Mutex::new(Vec::new());

enum Settle {
    /// The same backend answered SETTLE_PROBES times in a row.
    Stable,
    /// The time ran out before the pid settled.
    Unstable,
    /// At least SETTLE_PROBES different backends answered and none settled: a pool
    /// with several server connections, not worth settling again.
    Rotating,
    /// A probe failed: the server connection went away under us.
    Broken,
}

const SETTLE_PROBES: usize = 3;
const SETTLE_GAP: Duration = Duration::from_millis(50);
const SETTLE_MAX: Duration = Duration::from_secs(2);

/// Some poolers learn only later that reset killed their server connection, and
/// replace it after they've already answered (Supavisor 2.10 logs "Session terminated
/// by server while idle in the pool"). A scenario that starts in that window loses its
/// session state halfway. So ask for the backend pid until the same one answers
/// SETTLE_PROBES times in a row. Best effort: with several server connections the pid
/// may never settle, and after SETTLE_MAX the run goes on.
async fn settle(client: &Client, t: &Timeouts) -> Settle {
    let deadline = tokio::time::Instant::now() + SETTLE_MAX;
    let mut last = None;
    let mut same = 0;
    let mut seen = std::collections::HashSet::new();
    while tokio::time::Instant::now() < deadline {
        // Each probe ends by the overall deadline too, so settling never takes longer.
        let until = deadline.min(tokio::time::Instant::now() + t.housekeeping);
        let pid = match timeout_at(until, client.simple_query("SELECT pg_backend_pid()")).await {
            Ok(Ok(messages)) => messages.into_iter().find_map(|m| match m {
                SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
                _ => None,
            }),
            Err(_) if tokio::time::Instant::now() >= deadline => return Settle::Unstable,
            _ => return Settle::Broken,
        };
        same = if pid.is_some() && pid == last { same + 1 } else { 1 };
        if same >= SETTLE_PROBES {
            return Settle::Stable;
        }
        seen.extend(pid.clone());
        last = pid;
        tokio::time::sleep(SETTLE_GAP).await;
    }
    if seen.len() >= SETTLE_PROBES {
        Settle::Rotating
    } else {
        Settle::Unstable
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// How the stand-in pooler answers the n-th `SELECT pg_backend_pid()`.
    enum Pid {
        Is(usize),
        /// Never answers.
        Hang,
        /// Drops the connection, as when the pooler swaps its server connection.
        Close,
    }

    /// A pooler stand-in: accepts any login, answers SELECT 1, and answers
    /// `SELECT pg_backend_pid()` as `pid(n)` says, counting over all connections.
    async fn pooler_with_pids(name: &str, pid: fn(usize) -> Pid) -> Endpoint {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        fn message(tag: u8, body: &[u8]) -> Vec<u8> {
            [&[tag][..], &(body.len() as u32 + 4).to_be_bytes(), body].concat()
        }
        fn row(value: &str) -> Vec<u8> {
            let mut desc = b"\0\x01x\0".to_vec();
            desc.extend([0u8; 6]);
            desc.extend(25u32.to_be_bytes());
            desc.extend((-1i16).to_be_bytes());
            desc.extend((-1i32).to_be_bytes());
            desc.extend(0i16.to_be_bytes());
            let mut data = 1u16.to_be_bytes().to_vec();
            data.extend((value.len() as u32).to_be_bytes());
            data.extend(value.as_bytes());
            [message(b'T', &desc), message(b'D', &data), message(b'C', b"SELECT 1\0")].concat()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let asked = Arc::new(Mutex::new(0));
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let asked = asked.clone();
                tokio::spawn(async move {
                    let len = s.read_u32().await.unwrap() as usize;
                    let mut startup = vec![0; len - 4];
                    s.read_exact(&mut startup).await.unwrap();
                    let ready = message(b'Z', b"I");
                    let hello = [message(b'R', &[0; 4]), message(b'K', &[0; 8]), ready.clone()].concat();
                    s.write_all(&hello).await.unwrap();
                    while let Ok(tag) = s.read_u8().await {
                        let len = s.read_u32().await.unwrap() as usize;
                        let mut body = vec![0; len - 4];
                        s.read_exact(&mut body).await.unwrap();
                        if tag != b'Q' {
                            return;
                        }
                        let reply = if body.starts_with(b"SELECT pg_backend_pid()") {
                            let n = {
                                let mut asked = asked.lock().unwrap();
                                *asked += 1;
                                *asked
                            };
                            match pid(n) {
                                Pid::Is(pid) => row(&pid.to_string()),
                                Pid::Hang => {
                                    tokio::time::sleep(Duration::from_secs(60)).await;
                                    return;
                                }
                                Pid::Close => return,
                            }
                        } else {
                            row("1")
                        };
                        s.write_all(&[reply, ready.clone()].concat()).await.unwrap();
                    }
                });
            }
        });
        // A dbname per test: ports can repeat across tests, and the skip list is by endpoint.
        Endpoint::parse(&format!("postgres://u@127.0.0.1:{port}/{name}?sslmode=disable")).unwrap()
    }

    /// A Postgres stand-in that answers each simple query with one row of text values
    /// from `answer` (None = NULL).
    pub(crate) async fn answering(answer: fn(&str) -> Vec<Option<&'static str>>) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        fn message(tag: u8, body: &[u8]) -> Vec<u8> {
            [&[tag][..], &(body.len() as u32 + 4).to_be_bytes(), body].concat()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let len = s.read_u32().await.unwrap() as usize;
                    let mut startup = vec![0; len - 4];
                    s.read_exact(&mut startup).await.unwrap();
                    let ready = message(b'Z', b"I");
                    let hello = [message(b'R', &[0; 4]), message(b'K', &[0; 8]), ready.clone()].concat();
                    s.write_all(&hello).await.unwrap();
                    while let Ok(tag) = s.read_u8().await {
                        let len = s.read_u32().await.unwrap() as usize;
                        let mut body = vec![0; len - 4];
                        s.read_exact(&mut body).await.unwrap();
                        if tag != b'Q' {
                            return;
                        }
                        let values = answer(std::str::from_utf8(&body[..body.len() - 1]).unwrap());
                        let mut desc = (values.len() as u16).to_be_bytes().to_vec();
                        let mut data = (values.len() as u16).to_be_bytes().to_vec();
                        for value in &values {
                            desc.extend(b"c\0");
                            desc.extend([0u8; 6]);
                            desc.extend(25u32.to_be_bytes());
                            desc.extend((-1i16).to_be_bytes());
                            desc.extend((-1i32).to_be_bytes());
                            desc.extend(0i16.to_be_bytes());
                            match value {
                                Some(v) => {
                                    data.extend((v.len() as u32).to_be_bytes());
                                    data.extend(v.as_bytes());
                                }
                                None => data.extend((-1i32).to_be_bytes()),
                            }
                        }
                        let reply = [
                            message(b'T', &desc),
                            message(b'D', &data),
                            message(b'C', b"SELECT 1\0"),
                            ready.clone(),
                        ]
                        .concat();
                        s.write_all(&reply).await.unwrap();
                    }
                });
            }
        });
        port
    }

    pub(crate) fn target(port: u16, reset: Reset) -> Target {
        let url = format!("postgres://u@127.0.0.1:{port}/app?sslmode=disable");
        crate::targets::Targets::single(&url, &url, crate::scenario::PoolMode::Transaction, reset)
            .unwrap()
            .targets
            .remove(0)
    }

    #[test]
    fn auto_reset_quotes_what_it_was_told() {
        let sql = reset_auto_sql("10.0.0.7".parse().unwrap(), "o'neil");
        assert!(sql.contains("client_addr = '10.0.0.7'::inet"), "{sql}");
        assert!(sql.contains("usename = E'o''neil'"), "{sql}");
        // A user name trying to end the literal, with or without standard_conforming_strings.
        let sql = reset_auto_sql("::1".parse().unwrap(), "x\\' OR true --");
        assert!(
            sql.contains("client_addr = '::1'::inet AND usename = E'x\\\\'' OR true --' AND pid"),
            "{sql}"
        );
    }

    #[test]
    fn local_hosts() {
        let local = |url: &str| is_local(&url.parse::<tokio_postgres::Config>().unwrap());
        assert!(local("postgres://u@localhost/db"));
        assert!(local("postgres://u@127.0.0.2/db"));
        assert!(local("postgres://u@[::1]/db"));
        assert!(!local("postgres://u@db.example.com/db"));
        assert!(!local("postgres://u@10.0.0.1/db"));
        #[cfg(unix)]
        assert!(local("host=/var/run/postgresql user=u"));
    }

    /// Answers poolproof's privilege queries for a direct user that is a superuser.
    fn superuser(sql: &str) -> Vec<Option<&'static str>> {
        if sql.contains("rolcreaterole") {
            vec![Some("t"), Some("t"), Some("t"), Some("t")]
        } else {
            vec![Some("10.0.0.1")]
        }
    }

    #[tokio::test]
    async fn all_needs_yes_beyond_this_machine() {
        let t = Timeouts::default();
        let remote = {
            let mut target = target(1, Reset::All);
            target.direct = Endpoint::parse("postgres://u@db.example.com/app").unwrap();
            target
        };
        let err = resolve_reset(&remote, false, &t).await.unwrap_err().to_string();
        assert!(err.contains("pass --yes"), "{err}");
        // On this machine, no --yes needed.
        let port = answering(superuser).await;
        let (sql, line) = resolve_reset(&target(port, Reset::All), false, &t).await.unwrap();
        assert_eq!(sql[0].sql(), RESET_ALL);
        assert!(
            line.ends_with(&format!(
                "ends every other session on database app at 127.0.0.1:{port} (all)"
            )),
            "{line}"
        );
        assert!(
            resolve_reset(&target(1, Reset::None), false, &t)
                .await
                .unwrap()
                .0
                .is_empty()
        );
    }

    #[test]
    fn a_reset_must_be_able_to_end_the_poolers_sessions() {
        let reach = Reach {
            superuser: false,
            privileges_of: false,
            pg_signal_backend: false,
            pg_read_all_stats: false,
            me: "plain".into(),
        };
        let (text, public) = cannot_end("app", &reach).unwrap();
        assert_eq!(
            text,
            "this user can't end sessions of app without pg_signal_backend and pg_read_all_stats; as a \
             superuser: GRANT pg_signal_backend, pg_read_all_stats TO \"plain\""
        );
        // The public form names no role.
        assert_eq!(public, "missing pg_signal_backend and pg_read_all_stats");
        let signal = Reach {
            pg_signal_backend: true,
            ..reach
        };
        let (text, public) = cannot_end("app", &signal).unwrap();
        assert!(text.contains("without pg_read_all_stats; as a"), "{text}");
        assert_eq!(public, "missing pg_read_all_stats");
        let both = Reach {
            pg_read_all_stats: true,
            ..signal
        };
        assert_eq!(cannot_end("app", &both), None);
        // Its own sessions, or a role it inherits from: nothing more needed.
        let own = Reach {
            privileges_of: true,
            ..both
        };
        assert_eq!(cannot_end("app", &own), None);
        // A superuser's sessions: no privilege short of superuser helps.
        let root = Reach { superuser: true, ..own };
        let (text, public) = cannot_end("postgres", &root).unwrap();
        assert!(
            text.starts_with("postgres is a superuser, and only a superuser can end its sessions"),
            "{text}"
        );
        assert_eq!(public, "the pooler's user is a superuser");
    }

    #[test]
    fn grant_names_are_always_quoted() {
        assert_eq!(ident("app_1"), "\"app_1\"");
        // A reserved word: GRANT ... TO user is a syntax error.
        assert_eq!(ident("user"), "\"user\"");
        assert_eq!(ident("App"), "\"App\"");
        assert_eq!(ident("a\"b; DROP"), "\"a\"\"b; DROP\"");
    }

    #[tokio::test]
    async fn all_and_auto_refuse_what_they_cannot_end() {
        let t = Timeouts::default();
        // A plain direct user, and a pooler connecting as another plain role.
        let port = answering(|sql| {
            if sql.contains("rolcreaterole") {
                vec![Some("f"), Some("f"), Some("f"), Some("f")]
            } else if sql.contains("inet_client_addr()), session_user") {
                vec![Some("10.0.0.7"), Some("app")]
            } else if sql.contains("session_user") {
                vec![Some("app")]
            } else if sql.contains("WHERE rolname = E'app'") {
                vec![Some("f"), Some("f"), Some("plain")]
            } else {
                vec![Some("10.0.0.1")]
            }
        })
        .await;
        for (reset, kind) in [(Reset::All, "all"), (Reset::Auto, "auto")] {
            let err = resolve_reset(&target(port, reset), false, &t)
                .await
                .unwrap_err()
                .to_string();
            assert_eq!(
                err,
                format!(
                    "reset = \"{kind}\" can't end the pooler's sessions: this user can't end sessions of app \
                     without pg_signal_backend and pg_read_all_stats; as a superuser: GRANT pg_signal_backend, \
                     pg_read_all_stats TO \"plain\""
                )
            );
        }
        // The pooler connects as a superuser.
        let port = answering(|sql| {
            if sql.contains("rolcreaterole") {
                vec![Some("f"), Some("f"), Some("t"), Some("t")]
            } else if sql.contains("session_user") {
                vec![Some("postgres")]
            } else if sql.contains("WHERE rolname = E'postgres'") {
                vec![Some("t"), Some("f"), Some("plain")]
            } else {
                vec![None]
            }
        })
        .await;
        let err = resolve_reset(&target(port, Reset::All), false, &t)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("postgres is a superuser, and only a superuser can end its sessions"),
            "{err}"
        );
        // Its own role: fine, and the line says not everything goes.
        let port = answering(|sql| {
            if sql.contains("rolcreaterole") {
                vec![Some("f"), Some("f"), Some("f"), Some("f")]
            } else if sql.contains("session_user") {
                vec![Some("app")]
            } else {
                vec![Some("f"), Some("t"), Some("app")]
            }
        })
        .await;
        let (_, line) = resolve_reset(&target(port, Reset::All), false, &t).await.unwrap();
        assert!(
            line.contains("ends every other session that this user may end on database app"),
            "{line}"
        );
    }

    /// The first value of the first row.
    async fn value(client: &Client, sql: &str) -> String {
        let messages = client.simple_query(sql).await.unwrap();
        messages
            .iter()
            .find_map(|m| match m {
                SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
                _ => None,
            })
            .unwrap()
    }

    /// POOLPROOF_TEST_PG with a startup option that sets the current role, so a NOLOGIN
    /// role can be a target's direct user. Postgres checks privileges against the
    /// current role, as after SET ROLE.
    fn as_role(url: &str, role: &str) -> Endpoint {
        let sep = if url.contains('?') { '&' } else { '?' };
        Endpoint::parse(&format!("{url}{sep}options=-c%20role%3D{role}")).unwrap()
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn reset_all_leaves_a_superusers_session_to_an_heir() {
        let url = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
        let t = Timeouts::default();
        let root = Endpoint::parse(&url).unwrap();
        let (admin, _admin_driver, _) = connect(&root, &t).await.unwrap();
        let superuser = value(&admin, "SELECT current_user").await;
        // NOLOGIN, taken with SET ROLE: it has a superuser role's privileges without
        // being a superuser itself, and never a password.
        let heir = format!("pp_reset_heir_{}", std::process::id());
        admin
            .batch_execute(&format!(
                "DROP ROLE IF EXISTS {heir}; CREATE ROLE {heir} NOLOGIN; GRANT {} TO {heir}",
                ident(&superuser)
            ))
            .await
            .unwrap();
        // A superuser's session that is busy when the reset runs.
        let (sleeper, _sleeper_driver, _) = connect(&root, &t).await.unwrap();
        let pid = value(&sleeper, "SELECT pg_backend_pid()").await;
        let sleeping = tokio::spawn(async move { sleeper.simple_query("SELECT pg_sleep(30)").await });
        tokio::time::sleep(Duration::from_millis(200)).await;

        let (me, me_driver, _) = connect(&root, &t).await.unwrap();
        let reset = me.batch_execute(&format!("SET ROLE {heir}; {RESET_ALL}")).await;
        let alive = value(
            &admin,
            &format!("SELECT count(*) FROM pg_stat_activity WHERE pid = {pid}"),
        )
        .await;

        // Clean up before any assert.
        drop(me);
        let _ = me_driver.await;
        admin
            .batch_execute(&format!("SELECT pg_terminate_backend({pid}, 5000); DROP ROLE {heir}"))
            .await
            .unwrap();
        let _ = sleeping.await;

        reset.unwrap();
        assert_eq!(alive, "1", "the superuser's session was ended");
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn reset_all_as_a_plain_user() {
        let url = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
        let t = Timeouts::default();
        let root = Endpoint::parse(&url).unwrap();
        let (admin, _admin_driver, _) = connect(&root, &t).await.unwrap();
        let id = std::process::id();
        let (plain, noinherit, app) = (
            format!("pp_reset_plain_{id}"),
            format!("pp_reset_noinherit_{id}"),
            format!("pp_reset_app_{id}"),
        );
        // The direct users are NOLOGIN, taken through a role option or SET ROLE. The
        // pooler's role has to log in to own a session, so it gets a password made up
        // for this run, and no privileges.
        let password = format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                ^ u128::from(id)
        );
        admin
            .batch_execute(&format!(
                "DROP ROLE IF EXISTS {plain}; DROP ROLE IF EXISTS {app}; DROP ROLE IF EXISTS {noinherit}; \
                 CREATE ROLE {plain} NOLOGIN; CREATE ROLE {app} LOGIN PASSWORD '{password}'; \
                 CREATE ROLE {noinherit} NOLOGIN NOINHERIT IN ROLE pg_signal_backend, pg_read_all_stats"
            ))
            .await
            .unwrap();
        let rest = url.split_once('@').expect("a URL with user@host").1;
        let pooler = Endpoint::parse(&format!("postgres://{app}:{password}@{rest}")).unwrap();
        let mut target = target(1, Reset::All);
        target.direct = as_role(&url, &plain);
        target.pooler = pooler.clone();
        let (pooled, _pooled_driver, _) = connect(&pooler, &t).await.unwrap();

        // Without pg_read_all_stats another role's session shows no backend type, so the
        // reset would match nothing: refused.
        let (me, me_driver, _) = connect(&root, &t).await.unwrap();
        me.batch_execute(&format!("SET ROLE {plain}")).await.unwrap();
        let hidden = value(
            &me,
            &format!("SELECT count(*) FROM pg_stat_activity WHERE usename = '{app}' AND backend_type IS NULL"),
        )
        .await;
        let neither = resolve_reset(&target, false, &t).await.map(|_| ());
        admin
            .batch_execute(&format!("GRANT pg_read_all_stats TO {plain}"))
            .await
            .unwrap();
        let no_signal = resolve_reset(&target, false, &t).await.map(|_| ());
        admin
            .batch_execute(&format!("GRANT pg_signal_backend TO {plain}"))
            .await
            .unwrap();
        let both = resolve_reset(&target, false, &t).await;
        // It ends the pooler's session and leaves the superuser's alone, without an error.
        let reset = me.batch_execute(RESET_ALL).await;
        let pooled_after = pooled.simple_query("SELECT 1").await;
        let admin_after = admin.simple_query("SELECT 1").await;
        // A member that doesn't inherit their privileges doesn't have them.
        let mut without_inherit = super::tests::target(1, Reset::All);
        without_inherit.direct = as_role(&url, &noinherit);
        without_inherit.pooler = pooler.clone();
        let not_inherited = resolve_reset(&without_inherit, false, &t).await.map(|_| ());
        // A pooler connecting as a superuser: refused, whatever was granted.
        target.pooler = root.clone();
        let superuser_pooler = resolve_reset(&target, false, &t).await.map(|_| ());

        // Clean up before any assert.
        drop(me);
        let _ = me_driver.await;
        admin
            .batch_execute(&format!(
                "SELECT pg_terminate_backend(pid, 5000) FROM pg_stat_activity WHERE usename = '{app}'; \
                 DROP ROLE {plain}; DROP ROLE {app}; DROP ROLE {noinherit}"
            ))
            .await
            .unwrap();

        assert_eq!(hidden, "1", "{app}'s session wasn't hidden");
        let err = neither.unwrap_err().to_string();
        assert!(
            err.ends_with(&format!(
                "without pg_signal_backend and pg_read_all_stats; as a superuser: GRANT pg_signal_backend, \
                 pg_read_all_stats TO \"{plain}\""
            )),
            "{err}"
        );
        let err = no_signal.unwrap_err().to_string();
        assert!(err.contains("without pg_signal_backend;"), "{err}");
        let (_, line) = both.unwrap();
        assert!(line.contains("that this user may end"), "{line}");
        reset.unwrap();
        assert!(pooled_after.is_err(), "the pooler's session survived");
        admin_after.unwrap();
        let err = not_inherited.unwrap_err().to_string();
        assert!(err.contains("without pg_signal_backend and pg_read_all_stats"), "{err}");
        let err = superuser_pooler.unwrap_err().to_string();
        assert!(
            err.contains("is a superuser, and only a superuser can end its sessions"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn auto_finds_the_poolers_address_or_refuses() {
        let t = Timeouts::default();
        // Through the pooler Postgres sees 10.0.0.7; poolproof itself comes from 10.0.0.1.
        let port = answering(|sql| {
            if sql.contains("rolcreaterole") {
                vec![Some("t"), Some("t"), Some("t"), Some("t")]
            } else if sql.contains("session_user") {
                vec![Some("10.0.0.7"), Some("app")]
            } else {
                vec![Some("10.0.0.1")]
            }
        })
        .await;
        let (sql, line) = resolve_reset(&target(port, Reset::Auto), false, &t).await.unwrap();
        assert_eq!(sql[0].sql(), reset_auto_sql("10.0.0.7".parse().unwrap(), "app"));
        assert!(
            line.contains("ends every session of user app from 10.0.0.7 (not only the pooler's) on database app"),
            "{line}"
        );

        let unix = answering(|sql| {
            if sql.contains("session_user") {
                vec![None, Some("app")]
            } else {
                vec![None]
            }
        })
        .await;
        let err = resolve_reset(&target(unix, Reset::Auto), false, &t)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no client address"), "{err}");

        let same = answering(|sql| {
            if sql.contains("session_user") {
                vec![Some("10.0.0.1"), Some("app")]
            } else {
                vec![Some("10.0.0.1")]
            }
        })
        .await;
        let err = resolve_reset(&target(same, Reset::Auto), false, &t)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("the same address as poolproof"), "{err}");

        // The same host written as an IPv4-mapped IPv6 address.
        let mapped = answering(|sql| {
            if sql.contains("session_user") {
                vec![Some("::ffff:10.0.0.1"), Some("app")]
            } else {
                vec![Some("10.0.0.1")]
            }
        })
        .await;
        let err = resolve_reset(&target(mapped, Reset::Auto), false, &t)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("the same address as poolproof"), "{err}");

        let not_ip = answering(|sql| {
            if sql.contains("session_user") {
                vec![Some("10.0.0.7' OR true --"), Some("app")]
            } else {
                vec![Some("10.0.0.1")]
            }
        })
        .await;
        let err = resolve_reset(&target(not_ip, Reset::Auto), false, &t)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("isn't an IP address"), "{err}");
    }

    #[tokio::test]
    async fn ready_waits_until_the_same_backend_answers() {
        let t = Timeouts::default();
        // The pooler swaps its server connection twice, then keeps one.
        let settling = pooler_with_pids("settling", |n| Pid::Is(n.min(3)));
        assert!(wait_ready(&settling.await, &t).await.unwrap());
        // A new backend every time, as with a larger pool: give up, but don't fail,
        // and don't try again for this pooler.
        let rotating = pooler_with_pids("rotating", Pid::Is).await;
        let started = tokio::time::Instant::now();
        assert!(!wait_ready(&rotating, &t).await.unwrap());
        assert!(started.elapsed() >= SETTLE_MAX);
        let started = tokio::time::Instant::now();
        assert!(!wait_ready(&rotating, &t).await.unwrap());
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "settle skipped the second time"
        );
    }

    #[tokio::test]
    async fn settling_never_takes_longer_than_its_limit() {
        let hangs = pooler_with_pids("hangs", |n| if n == 1 { Pid::Is(1) } else { Pid::Hang }).await;
        let started = tokio::time::Instant::now();
        assert!(!wait_ready(&hangs, &Timeouts::default()).await.unwrap());
        assert!(
            started.elapsed() < SETTLE_MAX + Duration::from_millis(500),
            "{:?}",
            started.elapsed()
        );
        // Only slow, not rotating: it gets another chance next time.
        assert!(!ROTATING.lock().unwrap().contains(&hangs.to_string()));
    }

    #[tokio::test]
    async fn a_connection_dropped_while_settling_is_retried() {
        // The second pid query loses the connection (Supavisor swapping its backend);
        // the next connection settles.
        let swaps = pooler_with_pids("swaps", |n| if n == 2 { Pid::Close } else { Pid::Is(7) }).await;
        assert!(wait_ready(&swaps, &Timeouts::default()).await.unwrap());
    }

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
            pg_read_all_stats: false,
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

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn sockets_get_the_url_options() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let open = |options: &str| {
            let url = format!("postgres://u@127.0.0.1:{port}/db?{options}");
            async move {
                let endpoint = Endpoint::parse(&url).unwrap();
                tcp_connect(endpoint.config(), "127.0.0.1", port).await.unwrap()
            }
        };

        let stream = open("keepalives_idle=77&keepalives_interval=5&keepalives_retries=3&tcp_user_timeout=9000").await;
        let socket = socket2::SockRef::from(&stream);
        assert!(socket.keepalive().unwrap());
        assert_eq!(socket.tcp_keepalive_time().unwrap(), Duration::from_secs(77));
        assert_eq!(socket.tcp_keepalive_interval().unwrap(), Duration::from_secs(5));
        assert_eq!(socket.tcp_keepalive_retries().unwrap(), 3);
        assert_eq!(socket.tcp_user_timeout().unwrap(), Some(Duration::from_secs(9)));

        // The defaults, as in tokio-postgres: on, after 2 hours idle.
        let stream = open("").await;
        let socket = socket2::SockRef::from(&stream);
        assert!(socket.keepalive().unwrap());
        assert_eq!(socket.tcp_keepalive_time().unwrap(), Duration::from_secs(2 * 60 * 60));

        let stream = open("keepalives=0").await;
        assert!(!socket2::SockRef::from(&stream).keepalive().unwrap());
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
