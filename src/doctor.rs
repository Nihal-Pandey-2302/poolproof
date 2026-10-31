//! `poolproof doctor`: checks a target before a run. It only reads: SELECTs, and
//! never a SET through the pooler, where it would leak to other clients.

use std::future::Future;
use std::time::Duration;

use anyhow::Context;
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
        // pad, not write_str, so `{:<5}` lines the column up.
        f.pad(match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub check: &'static str,
    pub status: Status,
    /// For text output: may name URLs, hosts, users and addresses.
    pub detail: String,
    /// For JSON, which people paste into issues: the detail without those, when it has
    /// any. Same rule as the run report.
    pub public: Option<String>,
}

impl Check {
    pub fn new(check: &'static str, status: Status, detail: impl Into<String>) -> Self {
        Check {
            check,
            status,
            detail: detail.into(),
            public: None,
        }
    }

    /// Sets the detail JSON shows instead.
    pub fn public(mut self, detail: impl Into<String>) -> Self {
        self.public = Some(detail.into());
        self
    }
}

impl Serialize for Check {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut out = s.serialize_struct("Check", 3)?;
        out.serialize_field("check", self.check)?;
        out.serialize_field("status", &self.status)?;
        out.serialize_field("detail", self.public.as_ref().unwrap_or(&self.detail))?;
        out.end()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetReport {
    pub name: String,
    pub checks: Vec<Check>,
}

/// Whether anything failed. With `strict`, a warning counts too.
pub fn failed(reports: &[TargetReport], strict: bool) -> bool {
    let limit = if strict { Status::Warn } else { Status::Fail };
    reports.iter().flat_map(|r| &r.checks).any(|c| c.status >= limit)
}

/// One target's lines, `status target check detail`, with target names padded to
/// `width` so several targets line up.
pub fn text(report: &TargetReport, width: usize) -> String {
    report
        .checks
        .iter()
        .map(|c| format!("{:<5} {:<width$} {:<18} {}\n", c.status, report.name, c.check, c.detail))
        .collect()
}

/// The line after all targets' checks.
pub fn summary(reports: &[TargetReport]) -> String {
    let count = |status| {
        reports
            .iter()
            .flat_map(|r| &r.checks)
            .filter(|c| c.status == status)
            .count()
    };
    format!(
        "\n{} targets checked: {} failed, {} warnings\n",
        reports.len(),
        count(Status::Fail),
        count(Status::Warn)
    )
}

pub fn json(reports: &[TargetReport]) -> String {
    #[derive(Serialize)]
    struct Doc<'a> {
        targets: &'a [TargetReport],
    }
    serde_json::to_string_pretty(&Doc { targets: reports }).unwrap()
}

/// What tells two connections apart as the same Postgres server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFacts {
    pub database: String,
    /// pg_postmaster_start_time() in UTC, to the microsecond: two servers started in
    /// the same second still differ.
    pub started: String,
    pub version: String,
    /// The server's listen port, the same through any path to it.
    pub port: Option<String>,
    /// The address this connection reached the server on. Differs by path (the pooler
    /// may use a private network), so it's shown, never compared.
    pub addr: Option<String>,
}

pub const FACTS_SQL: &str = "SELECT current_database(), \
     to_char(pg_postmaster_start_time() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS.US'), \
     split_part(current_setting('server_version'), ' ', 1), inet_server_port()::text, host(inet_server_addr())";

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
        let differ = differ.join("; ");
        return Check::new("same server", Status::Fail, format!("{differ} {addrs}")).public(differ);
    }
    let port = direct
        .port
        .as_deref()
        .map(|p| format!(", port {p}"))
        .unwrap_or_default();
    let detail = format!(
        "database {}{port}, started {}, {}",
        direct.database, direct.started, direct.version
    );
    Check::new("same server", Status::Ok, format!("{detail} {addrs}")).public(detail)
}

/// What the mode probe saw: client a took a server connection, then client b asked for
/// one while a stayed connected (or, for the busy probe, inside a's open transaction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// b got a's backend: the pooler shares server connections between clients.
    Shared,
    /// b got another backend: session mode, or a pool with a free connection.
    Separate,
    /// b got no reply in time: it waited for a's server connection.
    Waited,
    /// The pooler refused b with a server error while a held its connection, and let
    /// b in once a was gone: one client at a time. The SQLSTATE and message.
    Refused(String),
    /// b answered neither in time nor with a's backend once a was gone, so the probe
    /// can't tell. Why.
    Unclear(String),
}

impl Observed {
    /// Whether b had to wait for a, or was turned away: a pool of one.
    pub fn one_at_a_time(&self) -> bool {
        matches!(self, Observed::Waited | Observed::Refused(_))
    }
}

/// A connected client and the task driving its connection.
pub type Connected = (Client, JoinHandle<()>);

/// Runs the mode probe with a idle. `connect` opens a client through the pooler. b's
/// first answer is awaited for `within`, a late one for `late`, and both clients are
/// closed before this returns.
pub async fn probe_mode<F, Fut>(connect: F, within: Duration, late: Duration) -> anyhow::Result<Observed>
where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<Connected>>,
{
    probe(connect, within, late, false).await.map(|(observed, _)| observed)
}

/// Runs the probe; with `in_transaction`, a holds an open transaction while b asks.
/// Also returns a's backend pid. If b has no answer within `within`, a is closed and
/// b's answer awaited for `late`: only a's own backend then says b waited for it.
///
/// A pool that opens server connections lazily can still look like that: b waits
/// while the pooler opens a second one, and gets a's freed server first. So a b that
/// waited is probed once more, right away, with that second server now idle: only a
/// second wait says one server connection.
pub async fn probe<F, Fut>(
    connect: F,
    within: Duration,
    late: Duration,
    in_transaction: bool,
) -> anyhow::Result<(Observed, String)>
where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<Connected>>,
{
    let first = probe_once(&connect, within, late, in_transaction).await?;
    if !first.0.one_at_a_time() {
        return Ok(first);
    }
    let (again, pid) = probe_once(&connect, within, late, in_transaction).await?;
    Ok(match again {
        // Sharing a backend inside a transaction is the pooler's bug whenever it shows.
        Observed::Waited | Observed::Refused(_) | Observed::Shared => (again, pid),
        _ => {
            let once = if first.0 == Observed::Waited {
                "waited"
            } else {
                "was refused"
            };
            (Observed::Unclear(format!("{once} once, then got another backend")), pid)
        }
    })
}

async fn probe_once<F, Fut>(
    connect: &F,
    within: Duration,
    late: Duration,
    in_transaction: bool,
) -> anyhow::Result<(Observed, String)>
where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<Connected>>,
{
    let (a, a_driver) = connect().await?;
    let a_pid = async {
        if in_transaction {
            timeout(within, a.simple_query("BEGIN"))
                .await
                .with_context(|| format!("BEGIN: no reply within {within:?}"))??;
        }
        backend_pid(&a, within).await
    }
    .await;
    let close_a = async {
        if in_transaction {
            let _ = timeout(within, a.simple_query("ROLLBACK")).await;
        }
        drop(a);
        let _ = timeout(within, a_driver).await;
    };
    let a_pid = match a_pid {
        Ok(pid) => pid,
        Err(e) => {
            close_a.await;
            return Err(e);
        }
    };
    // b's connect and query as one future, kept across the first timeout so a late
    // answer is still seen.
    let b = async {
        let (b, b_driver) = connect().await?;
        let reply = b.simple_query("SELECT pg_backend_pid()").await;
        let pid = reply.map_err(anyhow::Error::from).and_then(first_value);
        drop(b);
        let _ = timeout(within, b_driver).await;
        pid
    };
    tokio::pin!(b);
    let (b, late_answer) = match timeout(within, &mut b).await {
        Ok(answer) => {
            close_a.await;
            (answer, false)
        }
        Err(_) => {
            close_a.await;
            match timeout(late, &mut b).await {
                Ok(answer) => (answer, true),
                Err(_) => {
                    let reason = format!("b had no answer within {within:?}, nor {late:?} after a closed");
                    return Ok((Observed::Unclear(reason), a_pid));
                }
            }
        }
    };
    let observed = match b {
        Ok(b_pid) if late_answer && b_pid == a_pid => Observed::Waited,
        Ok(_) if late_answer => Observed::Unclear("b answered late, with another backend than a's".into()),
        Ok(b_pid) if b_pid == a_pid => Observed::Shared,
        Ok(_) => Observed::Separate,
        Err(e) => {
            // Only a server error that goes away once a is gone says "one at a time": a
            // wrong password, TLS or a pooler that went down must not read as a pool size.
            let Some(refusal) = server_refusal(&e).filter(|_| !late_answer) else {
                return Err(e.context("second client"));
            };
            // Some poolers accept any connect and refuse only at the query, so a
            // reconnect proves nothing: b must now get a's backend.
            let again = async {
                let (again, driver) = timeout(within, connect())
                    .await
                    .map_err(|_| anyhow::anyhow!("no reconnect within {within:?}"))??;
                let pid = timeout(late, again.simple_query("SELECT pg_backend_pid()")).await;
                drop(again);
                let _ = timeout(within, driver).await;
                pid.map_err(|_| anyhow::anyhow!("no answer within {late:?}"))?
                    .map_err(anyhow::Error::from)
                    .and_then(first_value)
            };
            let pid = again
                .await
                .with_context(|| format!("second client: {e:#}; then, after the first closed"))?;
            if pid == a_pid {
                Observed::Refused(refusal)
            } else {
                Observed::Unclear("was refused while a was connected, then got another backend".into())
            }
        }
    };
    Ok((observed, a_pid))
}

/// `SQLSTATE: message` for an error the server sent, except authentication errors
/// (class 28).
fn server_refusal(e: &anyhow::Error) -> Option<String> {
    let db = e
        .chain()
        .find_map(|c| c.downcast_ref::<tokio_postgres::Error>())?
        .as_db_error()?;
    let code = db.code().code();
    (!code.starts_with("28")).then(|| format!("{code}: {}", db.message()))
}

fn first_value(messages: Vec<SimpleQueryMessage>) -> anyhow::Result<String> {
    messages
        .into_iter()
        .find_map(|m| match m {
            SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("pg_backend_pid() returned no row"))
}

/// The mode check: what was seen, against the target's `mode`.
pub fn mode_check(observed: Observed, configured: PoolMode) -> Check {
    let (seen, matches) = match observed {
        Observed::Shared => (
            "transaction or statement pooling (b got a's server connection)".to_string(),
            configured != PoolMode::Session,
        ),
        Observed::Waited => (
            "session mode (b waited for a's server connection; a pool exhausted by other clients looks the same)"
                .into(),
            configured == PoolMode::Session,
        ),
        Observed::Refused(refusal) => (
            format!("session mode (the pooler refused b while a was connected: {refusal})"),
            configured == PoolMode::Session,
        ),
        Observed::Separate => ("not determined (b got another server connection)".into(), true),
        Observed::Unclear(why) => (format!("not determined ({why})"), true),
    };
    let status = if matches { Status::Ok } else { Status::Warn };
    let detail = if matches {
        seen
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

pub async fn first_row(client: &Client, sql: &str, within: Duration) -> anyhow::Result<Vec<Option<String>>> {
    let messages = timeout(within, client.simple_query(sql))
        .await
        .with_context(|| format!("no reply within {within:?}"))??;
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
        // JSON drops the addresses.
        assert_eq!(
            ok.public.as_deref(),
            Some("database pp, port 5432, started 2026-10-31 21:04:11, 17.11")
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
        assert!(!failed(&[report(Status::Ok)], true));
        assert!(!failed(&[report(Status::Warn)], false));
        assert!(failed(&[report(Status::Warn)], true));
        assert!(failed(&[report(Status::Fail)], false));
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
        /// One backend; a client that asks while it's held is refused with this code
        /// (XX000 like Supavisor's EMAXCONNSESSION, or 28P01 like a bad password).
        RefusesWhileHeld(&'static str),
        /// Only the first client ever gets in.
        RefusesAllButFirst,
        /// Later clients answer after `delay` ms, or never, with a backend of their own.
        Slow(Option<u64>),
        /// A pool of two that opens its second server only when a client has to wait,
        /// 600 ms later. Waiters get servers first come, first served, so the first
        /// one waiting gets a's freed server.
        LazySecond,
        /// Like `SessionOfOne` for the first two clients; after that every client gets
        /// backend 7 at once, even inside another's transaction.
        WaitsThenShares,
        /// Accepts every connect, and answers a query with 58000 while another client
        /// holds the one backend, like PgCat. Once it's free, a client gets `After`.
        ErrorsWhileHeld(After),
    }

    /// What a client of `ErrorsWhileHeld` gets once the backend is free.
    #[derive(Clone, Copy)]
    enum After {
        /// Backend 7, the one the first client had.
        Same,
        /// Another backend.
        Other,
        /// Another 58000: it can't open a server at all.
        Error,
    }

    /// A Postgres server that knows only startup and `SELECT pg_backend_pid()`.
    async fn fake_pooler(pool: Pool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let next = Arc::new(AtomicI32::new(100));
        let backend = Arc::new(Semaphore::new(1));
        let idle = Arc::new(std::sync::Mutex::new(vec![7]));
        let opened = Arc::new(std::sync::atomic::AtomicBool::new(false));
        tokio::spawn(async move {
            let holder = Arc::new(std::sync::Mutex::new(None));
            while let Ok((socket, _)) = listener.accept().await {
                if let Pool::ErrorsWhileHeld(after) = pool {
                    let conn = next.fetch_add(1, Ordering::SeqCst);
                    tokio::spawn(serve_holding(socket, conn, holder.clone(), after));
                    continue;
                }
                if let Pool::LazySecond = pool {
                    let (backend, idle, opened) = (backend.clone(), idle.clone(), opened.clone());
                    tokio::spawn(async move {
                        let permit = match backend.clone().try_acquire_owned() {
                            Ok(permit) => permit,
                            Err(_) => {
                                if !opened.swap(true, Ordering::SeqCst) {
                                    let (backend, idle) = (backend.clone(), idle.clone());
                                    tokio::spawn(async move {
                                        tokio::time::sleep(Duration::from_millis(600)).await;
                                        idle.lock().unwrap().push(8);
                                        backend.add_permits(1);
                                    });
                                }
                                backend.clone().acquire_owned().await.unwrap()
                            }
                        };
                        let pid = idle.lock().unwrap().pop().unwrap();
                        let _ = serve(socket, pid).await;
                        idle.lock().unwrap().push(pid);
                        drop(permit);
                    });
                    continue;
                }
                let n = next.fetch_add(1, Ordering::SeqCst);
                let pid = match pool {
                    // One backend, whoever holds it.
                    Pool::Shared | Pool::SessionOfOne | Pool::WaitsThenShares | Pool::RefusesWhileHeld(_) => 7,
                    _ => n,
                };
                let backend = backend.clone();
                tokio::spawn(async move {
                    let _held = match pool {
                        Pool::SessionOfOne => Some(backend.acquire_owned().await.unwrap()),
                        Pool::WaitsThenShares if n < 102 => Some(backend.acquire_owned().await.unwrap()),
                        Pool::RefusesWhileHeld(code) => match backend.try_acquire_owned() {
                            Ok(held) => Some(held),
                            Err(_) => return drop(refuse(socket, code).await),
                        },
                        Pool::RefusesAllButFirst if pid > 100 => return drop(refuse(socket, "XX000").await),
                        Pool::Slow(delay) if pid > 100 => {
                            let wait = delay.map_or(Duration::from_secs(3600), Duration::from_millis);
                            tokio::time::sleep(wait).await;
                            None
                        }
                        _ => None,
                    };
                    let _ = serve(socket, pid).await;
                });
            }
        });
        format!("postgres://pp@127.0.0.1:{port}/pp?sslmode=disable")
    }

    /// One backend for `ErrorsWhileHeld`: the first client to query holds it until it
    /// disconnects; meanwhile others get 58000 at query time.
    async fn serve_holding(
        mut socket: TcpStream,
        conn: i32,
        holder: Arc<std::sync::Mutex<Option<i32>>>,
        after: After,
    ) -> std::io::Result<()> {
        let len = socket.read_i32().await?;
        let mut startup = vec![0; len as usize - 4];
        socket.read_exact(&mut startup).await?;
        let mut out = BytesMut::new();
        message(&mut out, b'R', |b| b.put_i32(0));
        message(&mut out, b'K', |b| {
            b.put_i32(conn);
            b.put_i32(0);
        });
        message(&mut out, b'Z', |b| b.put_u8(b'I'));
        socket.write_all(&out).await?;
        let result = async {
            loop {
                let kind = socket.read_u8().await?;
                let len = socket.read_i32().await?;
                let mut body = vec![0; len as usize - 4];
                socket.read_exact(&mut body).await?;
                if kind != b'Q' {
                    return Ok(());
                }
                let pid = {
                    let mut held = holder.lock().unwrap();
                    match (*held, after) {
                        (Some(other), _) if other != conn => None,
                        (Some(_), _) => Some(7),
                        // The first client ever gets backend 7; later ones what `after` says.
                        (None, _) if conn == 100 => Some(7),
                        (None, After::Same) => Some(7),
                        (None, After::Other) => Some(8),
                        (None, After::Error) => None,
                    }
                    .inspect(|_| *held = Some(conn))
                };
                let mut out = BytesMut::new();
                match pid {
                    Some(pid) => {
                        let value = pid.to_string();
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
                    }
                    None => message(&mut out, b'E', |b| {
                        for (field, value) in [
                            (b'S', "ERROR"),
                            (b'V', "ERROR"),
                            (b'C', "58000"),
                            (b'M', "AllServersDown"),
                        ] {
                            b.put_u8(field);
                            b.put_slice(value.as_bytes());
                            b.put_u8(0);
                        }
                        b.put_u8(0);
                    }),
                }
                message(&mut out, b'Z', |b| b.put_u8(b'I'));
                socket.write_all(&out).await?;
            }
        }
        .await;
        let mut held = holder.lock().unwrap();
        if *held == Some(conn) {
            *held = None;
        }
        result
    }

    /// Reads the startup message and answers with a FATAL error.
    async fn refuse(mut socket: TcpStream, code: &str) -> std::io::Result<()> {
        let len = socket.read_i32().await?;
        let mut startup = vec![0; len as usize - 4];
        socket.read_exact(&mut startup).await?;
        let mut out = BytesMut::new();
        message(&mut out, b'E', |b| {
            for (field, value) in [
                (b'S', "FATAL"),
                (b'V', "FATAL"),
                (b'C', code),
                (b'M', "max clients reached"),
            ] {
                b.put_u8(field);
                b.put_slice(value.as_bytes());
                b.put_u8(0);
            }
            b.put_u8(0);
        });
        socket.write_all(&out).await
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

    async fn run_probe(pool: Pool, in_transaction: bool) -> anyhow::Result<Observed> {
        let url = fake_pooler(pool).await;
        let connect = || async {
            let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await?;
            Ok((client, tokio::spawn(async move { drop(connection.await) })))
        };
        let (within, late) = (Duration::from_millis(300), Duration::from_millis(1500));
        if in_transaction {
            probe(connect, within, late, true).await.map(|(observed, _)| observed)
        } else {
            probe_mode(connect, within, late).await
        }
    }

    async fn probe_idle(pool: Pool) -> Observed {
        run_probe(pool, false).await.unwrap()
    }

    #[tokio::test]
    async fn mode_probe_tells_shared_separate_and_waiting_apart() {
        assert_eq!(probe_idle(Pool::Shared).await, Observed::Shared);
        assert_eq!(probe_idle(Pool::Separate).await, Observed::Separate);
        assert_eq!(probe_idle(Pool::SessionOfOne).await, Observed::Waited);
        for busy in [Pool::Shared, Pool::Separate, Pool::SessionOfOne] {
            let idle = probe_idle(busy).await;
            assert_eq!(run_probe(busy, true).await.unwrap(), idle);
        }
    }

    #[tokio::test]
    async fn only_a_late_answer_from_the_same_backend_means_b_waited() {
        // The session pool of one answers b with a's backend once a is gone.
        assert_eq!(run_probe(Pool::SessionOfOne, true).await.unwrap(), Observed::Waited);
        // Slow to open a second server connection: not a pool of one.
        let late = run_probe(Pool::Slow(Some(600)), true).await.unwrap();
        assert_eq!(
            late,
            Observed::Unclear("b answered late, with another backend than a's".into())
        );
        let never = run_probe(Pool::Slow(None), true).await.unwrap();
        assert!(
            matches!(never, Observed::Unclear(ref why) if why.contains("no answer")),
            "{never:?}"
        );
        // b got a's freed server while the second one was still opening: it waited once,
        // and the second round finds two servers.
        let lazy = run_probe(Pool::LazySecond, true).await.unwrap();
        assert_eq!(lazy, Observed::Unclear("waited once, then got another backend".into()));
        // Refused at query time, like PgCat: only a's backend afterwards says one.
        let refused = Observed::Refused("58000: AllServersDown".into());
        for busy in [false, true] {
            assert_eq!(
                run_probe(Pool::ErrorsWhileHeld(After::Same), busy).await.unwrap(),
                refused
            );
            let other = run_probe(Pool::ErrorsWhileHeld(After::Other), busy).await.unwrap();
            assert!(
                matches!(other, Observed::Unclear(ref why) if why.contains("refused")),
                "{other:?}"
            );
            let down = run_probe(Pool::ErrorsWhileHeld(After::Error), busy).await;
            assert!(down.is_err(), "{down:?}");
        }
        // A backend shared inside a transaction in the second round stays visible.
        assert_eq!(run_probe(Pool::WaitsThenShares, true).await.unwrap(), Observed::Shared);
    }

    #[tokio::test]
    async fn a_refusal_counts_only_if_it_is_a_server_error_that_goes_away() {
        let refused = Observed::Refused("XX000: max clients reached".into());
        assert_eq!(probe_idle(Pool::RefusesWhileHeld("XX000")).await, refused);
        assert_eq!(run_probe(Pool::RefusesWhileHeld("XX000"), true).await.unwrap(), refused);
        assert!(refused.one_at_a_time());
        // A bad password isn't a pool size, and neither is a refusal that stays.
        for pool in [Pool::RefusesWhileHeld("28P01"), Pool::RefusesAllButFirst] {
            let err = run_probe(pool, false).await.err().map(|e| format!("{e:#}")).unwrap();
            assert!(err.starts_with("second client"), "{err}");
        }
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
