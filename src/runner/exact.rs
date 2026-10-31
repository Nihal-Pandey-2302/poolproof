//! `reset = "exact"`: end only the pooler's own server connections. They are found by
//! holding `pool_size` transactions through the pooler, each reading its backend's
//! identity, so neighbours with the same user and address (a NAT, a sidecar) are left
//! alone. Only complete when the pool size is known; a target whose pool turns out
//! bigger, or which can't be held, is an ERROR, never a partial reset.

use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout};
use tokio_postgres::{Client, SimpleQueryMessage};

use super::Timeouts;
use super::check::run_direct;
use super::pg::{connect, literal};
use super::probe::BUSY_WAIT;
use crate::doctor::server_refusal;
use crate::targets::{Endpoint, Statement};

/// One server connection of the pooler, as Postgres sees it. The start time tells a
/// reused pid apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backend {
    pub pid: i32,
    pub start: String,
    pub user: String,
    pub addr: Option<String>,
}

/// A backend's start time in UTC, whatever the session's TimeZone.
const START: &str = "to_char(backend_start AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS.US')";

/// The reset for one target. It remembers the server connections it found after its
/// last reset, so a server that is busy at the next one (an abandoned query still
/// running) is still ended. Once it can't account for every server, it fails for good:
/// the target's remaining scenarios ERROR with the same reason.
pub struct Exact {
    pooled: Endpoint,
    direct: Endpoint,
    size: usize,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    known: Vec<Backend>,
    failed: Option<String>,
}

/// Clients holding transactions through the pooler, and the backends they got.
struct Held {
    clients: Vec<(Client, JoinHandle<()>)>,
    backends: Vec<Backend>,
}

impl Exact {
    pub fn new(pooled: Endpoint, direct: Endpoint, size: u32) -> Self {
        Exact {
            pooled,
            direct,
            size: size as usize,
            state: Mutex::default(),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fail(&self, reason: String) -> anyhow::Error {
        self.state().failed = Some(reason.clone());
        anyhow::anyhow!(reason)
    }

    /// Holds up to `n` transactions, each client waiting `wait` for its server. Stops
    /// at the first client that gets none. BEGIN and the pid are two queries: a pooler
    /// that defers BEGIN takes the server at the second one. Only the pid is asked
    /// through the pooler: a server can carry a leaked role that can't see its own row
    /// in pg_stat_activity. The rest is read on direct while they are held.
    async fn hold(&self, n: usize, wait: Duration, t: &Timeouts) -> Result<Held> {
        let mut clients = vec![];
        let mut pids = vec![];
        for _ in 0..n {
            let Ok(Ok((client, driver, _))) = timeout(wait, connect(&self.pooled, t)).await else {
                break;
            };
            let pid = timeout(wait, async {
                client.simple_query("BEGIN").await?;
                client.simple_query("SELECT pg_backend_pid()").await
            })
            .await;
            clients.push((client, driver));
            match pid.map(|r| r.map_err(anyhow::Error::from).and_then(first_pid)) {
                Ok(Ok(pid)) => pids.push(pid),
                Ok(Err(e)) => {
                    close(
                        Held {
                            clients,
                            backends: vec![],
                        },
                        t,
                    )
                    .await;
                    return Err(e);
                }
                Err(_) => break,
            }
        }
        match self.confirm(&pids, t).await {
            Ok(backends) => Ok(Held { clients, backends }),
            Err(e) => {
                close(
                    Held {
                        clients,
                        backends: vec![],
                    },
                    t,
                )
                .await;
                Err(e)
            }
        }
    }

    /// Every held backend is a different one, and Postgres sees each idle in a
    /// transaction; returns who they are as Postgres sees them (after a pooler's user
    /// mapping). Read while they are held, so no pid can be reused in between.
    async fn confirm(&self, pids: &[i32], t: &Timeouts) -> Result<Vec<Backend>> {
        let mut sorted = pids.to_vec();
        sorted.sort_unstable();
        if let Some(w) = sorted.windows(2).find(|w| w[0] == w[1]) {
            let reason = format!(
                "the pooler gave another client a server connection inside an open transaction (pid {})",
                w[0]
            );
            return Err(self.fail(reason));
        }
        if pids.is_empty() {
            return Ok(vec![]);
        }
        let sql = format!(
            "SELECT pid::text, {START}, usename, host(client_addr), state FROM pg_stat_activity \
             WHERE datname = current_database() AND pid IN ({})",
            join(pids)
        );
        let rows = rows(&self.direct, &sql, t)
            .await
            .context("exact reset: reading the held backends")?;
        let open: Vec<&Vec<Option<String>>> = rows
            .iter()
            .filter(|r| r.get(4).cloned().flatten().as_deref() == Some("idle in transaction"))
            .collect();
        if open.len() != pids.len() {
            let reason = format!(
                "exact reset: Postgres sees {} of the {} held transactions open on this database; the pooler doesn't \
                 keep a server connection for a client's transaction",
                open.len(),
                pids.len()
            );
            return Err(self.fail(reason));
        }
        open.into_iter().map(|r| backend(r)).collect()
    }

    /// The backends found by holding every transaction, tried again until
    /// `t.housekeeping` passes while fewer than `want` are accounted for by them and
    /// `extra`. Returns what it found last.
    async fn discover(&self, extra: &[Backend], t: &Timeouts) -> Result<Vec<Backend>> {
        let deadline = Instant::now() + t.housekeeping;
        loop {
            let held = self.hold(self.size, t.housekeeping.min(BUSY_WAIT), t).await?;
            let found = held.backends.clone();
            close(held, t).await;
            if union(&found, extra).len() >= self.size || Instant::now() >= deadline {
                return Ok(found);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// The `known` backends still on Postgres with the same pid and start.
    async fn alive(&self, known: &[Backend], t: &Timeouts) -> Result<Vec<Backend>> {
        if known.is_empty() {
            return Ok(vec![]);
        }
        let pids: Vec<i32> = known.iter().map(|b| b.pid).collect();
        let sql = format!(
            "SELECT pid::text, {START} FROM pg_stat_activity WHERE datname = current_database() AND pid IN ({})",
            join(&pids)
        );
        let now = rows(&self.direct, &sql, t)
            .await
            .context("exact reset: reading known backends")?;
        Ok(known
            .iter()
            .filter(|b| {
                now.iter()
                    .any(|r| r[0].as_deref() == Some(b.pid.to_string().as_str()) && r[1].as_deref() == Some(&b.start))
            })
            .cloned()
            .collect())
    }

    /// Runs before each run: finds the pooler's server connections, ends them, waits
    /// for the pooler, and learns the new ones the run will use.
    pub async fn reset(&self, t: &Timeouts) -> Result<()> {
        if let Some(reason) = self.state().failed.clone() {
            anyhow::bail!(reason);
        }
        let known = self.state().known.clone();
        let alive = self.alive(&known, t).await?;
        let found = self.discover(&alive, t).await?;
        let all = union(&found, &alive);
        if all.len() < self.size {
            let reason = self.leftover(found.len(), &all, t).await;
            return Err(self.fail(reason));
        }
        run_direct(&self.direct, &[Statement::plain(&terminate_sql(&all))], t).await?;
        self.answers(t).await?;
        // The servers the pooler opens now are the ones the run uses: remembered, so one
        // still busy at the next reset is ended too.
        let fresh = self.discover(&[], t).await?;
        if fresh.len() < self.size {
            let reason = format!(
                "exact reset: after ending its server connections, could hold only {} of {} through the pooler \
                 (pool_size = {})",
                fresh.len(),
                transactions(self.size),
                self.size
            );
            return Err(self.fail(reason));
        }
        self.state().known = fresh;
        Ok(())
    }

    /// Why the reset can't account for every server connection, with the sessions that
    /// might be them. They are listed, never ended: poolproof didn't see them through
    /// the pooler.
    async fn leftover(&self, held: usize, seen: &[Backend], t: &Timeouts) -> String {
        let n = self.size;
        let head = format!(
            "exact reset: could hold only {held} of {} through the pooler, and no server connection \
             seen before accounts for the rest",
            transactions(n)
        );
        // The pooler connects as one user from one address, so the first seen stands for
        // all of them.
        let Some(user) = seen.first().map(|b| b.user.as_str()) else {
            return format!("{head}; no server connection answered, so the pooler's user and address are unknown");
        };
        let addr = seen.iter().find_map(|b| b.addr.clone());
        let pids: Vec<i32> = seen.iter().map(|b| b.pid).collect();
        let sql = format!(
            "SELECT pid::text, state, to_char(query_start AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS'), \
             to_char(backend_start AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') FROM pg_stat_activity \
             WHERE datname = current_database() AND usename = {} AND host(client_addr) IS NOT DISTINCT FROM {} \
             AND pid <> pg_backend_pid() AND backend_type = 'client backend' AND pid NOT IN ({}) ORDER BY pid",
            literal(user),
            addr.as_deref().map_or("NULL".into(), literal),
            join(&pids)
        );
        let from = addr.as_deref().unwrap_or("a Unix socket");
        let candidates = match rows(&self.direct, &sql, t).await {
            Ok(rows) if rows.is_empty() => "no other session of that user and address".to_string(),
            Ok(rows) => rows
                .iter()
                .map(|r| {
                    let v = |i: usize| r.get(i).cloned().flatten().unwrap_or_else(|| "?".into());
                    format!("pid {} ({}, query since {}, connected {})", v(0), v(1), v(2), v(3))
                })
                .collect::<Vec<_>>()
                .join(", "),
            Err(e) => format!("listing them failed: {e:#}"),
        };
        format!(
            "{head}; sessions of user {user} from {from} that poolproof didn't see: {candidates}. End those that are \
             the pooler's yourself, or restart the pooler (reset = \"auto\" would end them too, but it ends every \
             session of user {user} from {from})"
        )
    }

    /// Checked once before a target's scenarios: `pool_size` transactions can be held,
    /// and one client more waits for a server, twice (a lazy pool may still be opening
    /// one the first time). Returns what doctor shows. Ends nothing.
    pub async fn check_start(&self, t: &Timeouts) -> Result<String> {
        let mut seen: Vec<Backend> = vec![];
        for _ in 0..2 {
            seen = union(&seen, &self.check_round(t).await?);
        }
        self.state().known = seen.clone();
        let pids: Vec<String> = seen.iter().map(|b| b.pid.to_string()).collect();
        let pid = if self.size == 1 { "pid" } else { "pids" };
        Ok(format!(
            "held {}, client {} waited; would end {pid} {}",
            transactions(self.size),
            self.size + 1,
            pids.join(", ")
        ))
    }

    async fn check_round(&self, t: &Timeouts) -> Result<Vec<Backend>> {
        let n = self.size;
        let mut held = self.hold(n, t.housekeeping.min(BUSY_WAIT), t).await?;
        let deadline = Instant::now() + t.housekeeping;
        while held.backends.len() < n && Instant::now() < deadline {
            close(held, t).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
            held = self.hold(n, t.housekeeping.min(BUSY_WAIT), t).await?;
        }
        if held.backends.len() < n {
            let got = held.backends.len();
            close(held, t).await;
            return Err(self.fail(format!(
                "exact reset: could hold only {got} of {} through the pooler (pool_size = {n})",
                transactions(n)
            )));
        }
        let backends = held.backends.clone();
        let extra = async {
            let (client, driver, _) = connect(&self.pooled, t).await?;
            let pid = client.simple_query("SELECT pg_backend_pid()").await;
            drop(client);
            let _ = timeout(t.housekeeping, driver).await;
            pid.map_err(anyhow::Error::from).and_then(first_pid)
        };
        tokio::pin!(extra);
        let first = timeout(t.housekeeping.min(BUSY_WAIT), &mut extra).await;
        close(held, t).await;
        let late = match first {
            Ok(answer) => Err(answer),
            Err(_) => Ok(timeout(t.housekeeping, &mut extra).await),
        };
        // The held servers stay up while this runs, so a pid names one of them.
        let mine = |pid: &i32| backends.iter().any(|h| h.pid == *pid);
        // Refused while the pool was full, and given a held server once it wasn't: one at
        // a time. Some poolers accept any connect and refuse at the query, so getting in
        // again proves nothing by itself.
        if let Err(Err(e)) = &late
            && server_refusal(e).is_some()
        {
            return match self.pid(t).await {
                Ok(pid) if mine(&pid) => Ok(backends),
                Ok(_) => Err(self.fail(format!(
                    "pool_size = {n}, but client {} was refused, then got another backend",
                    n + 1
                ))),
                Err(again) => Err(self.fail(format!(
                    "exact reset: client {} was refused ({e:#}), and again after the pool was free: {again:#}",
                    n + 1
                ))),
            };
        }
        let reason = match late {
            // It waited, and got one of the held servers once they were free.
            Ok(Ok(Ok(pid))) if mine(&pid) => return Ok(backends),
            Ok(Ok(Ok(_))) => format!(
                "pool_size = {n}, but client {} answered late with another backend",
                n + 1
            ),
            Ok(Ok(Err(e))) => format!("exact reset: client {} failed: {e:#}", n + 1),
            Ok(Err(_)) => format!("couldn't confirm pool_size = {n}: client {} had no answer", n + 1),
            Err(Ok(pid)) if mine(&pid) => {
                format!("the pooler gave another client a server connection inside an open transaction (pid {pid})")
            }
            Err(Ok(_)) => format!("pool_size = {n}, but client {} got its own server connection", n + 1),
            Err(Err(e)) => format!("exact reset: client {} failed: {e:#}", n + 1),
        };
        Err(self.fail(reason))
    }

    /// Waits until the pooler answers again after its servers were ended. No settling
    /// as wait_ready does: the discovery right after holds every server and checks each.
    async fn answers(&self, t: &Timeouts) -> Result<()> {
        let deadline = Instant::now() + t.ready;
        loop {
            let err = match connect(&self.pooled, t).await {
                Ok((client, driver, _)) => {
                    let res = timeout(t.housekeeping, client.simple_query("SELECT 1")).await;
                    drop(client);
                    let _ = timeout(t.housekeeping, driver).await;
                    match res {
                        Ok(Ok(_)) => return Ok(()),
                        Ok(Err(e)) => anyhow::Error::from(e),
                        Err(_) => anyhow::anyhow!("no reply within {:?}", t.housekeeping),
                    }
                }
                Err(e) => e,
            };
            if Instant::now() >= deadline {
                return Err(err.context(format!("exact reset: the pooler didn't answer within {:?}", t.ready)));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// The backend a new client gets through the pooler now.
    async fn pid(&self, t: &Timeouts) -> Result<i32> {
        let (client, driver, _) = timeout(t.housekeeping, connect(&self.pooled, t))
            .await
            .context("no connect")??;
        let pid = timeout(t.housekeeping, client.simple_query("SELECT pg_backend_pid()")).await;
        drop(client);
        let _ = timeout(t.housekeeping, driver).await;
        pid.context("no answer")?
            .map_err(anyhow::Error::from)
            .and_then(first_pid)
    }
}

/// Rolls back and closes every held client, then waits for each to be gone. In session
/// mode only the disconnect gives the server back; the pooler then runs its own reset
/// query on it before the next client gets it.
async fn close(held: Held, t: &Timeouts) {
    for (client, _) in &held.clients {
        let _ = timeout(t.housekeeping.min(BUSY_WAIT), client.simple_query("ROLLBACK")).await;
    }
    for (client, driver) in held.clients {
        drop(client);
        let _ = timeout(t.housekeeping, driver).await;
    }
}

/// A backend from a row of `confirm`'s query.
fn backend(row: &[Option<String>]) -> Result<Backend> {
    let field = |i: usize| row.get(i).cloned().flatten();
    Ok(Backend {
        pid: field(0).and_then(|p| p.parse().ok()).context("exact reset: no pid")?,
        start: field(1).context("exact reset: no backend_start")?,
        user: field(2).context("exact reset: no user")?,
        addr: field(3),
    })
}

/// The pid from `SELECT pg_backend_pid()`.
fn first_pid(messages: Vec<SimpleQueryMessage>) -> Result<i32> {
    messages
        .into_iter()
        .find_map(|m| match m {
            SimpleQueryMessage::Row(row) => row.get(0).and_then(|p| p.parse().ok()),
            _ => None,
        })
        .context("exact reset: pg_backend_pid() returned no pid")
}

/// `a` and the backends of `b` not in it.
fn union(a: &[Backend], b: &[Backend]) -> Vec<Backend> {
    let mut all = a.to_vec();
    all.extend(
        b.iter()
            .filter(|x| !a.iter().any(|y| y.pid == x.pid && y.start == x.start))
            .cloned(),
    );
    all
}

/// `1 transaction`, `2 transactions`.
pub(super) fn transactions(n: usize) -> String {
    format!("{n} transaction{}", if n == 1 { "" } else { "s" })
}

fn join(pids: &[i32]) -> String {
    pids.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
}

/// Ends exactly these backends: same pid, start, database, user and address. A pid
/// reused by anyone else has another start time and is left alone.
fn terminate_sql(backends: &[Backend]) -> String {
    let each: Vec<String> = backends
        .iter()
        .map(|b| {
            format!(
                "(pid = {} AND {START} = {} AND usename = {} AND host(client_addr) IS NOT DISTINCT FROM {})",
                b.pid,
                literal(&b.start),
                literal(&b.user),
                b.addr.as_deref().map_or("NULL".into(), literal)
            )
        })
        .collect();
    format!(
        "SELECT pg_terminate_backend(pid, 5000) FROM pg_stat_activity WHERE datname = current_database() AND ({})",
        each.join(" OR ")
    )
}

/// Text rows from a query on `endpoint`.
async fn rows(endpoint: &Endpoint, sql: &str, t: &Timeouts) -> Result<Vec<Vec<Option<String>>>> {
    let (client, driver, _) = connect(endpoint, t).await?;
    let res = timeout(t.housekeeping, client.simple_query(sql)).await;
    drop(client);
    let _ = timeout(t.housekeeping, driver).await;
    Ok(res
        .context("timed out")??
        .into_iter()
        .filter_map(|m| match m {
            SimpleQueryMessage::Row(row) => Some((0..row.len()).map(|i| row.get(i).map(str::to_string)).collect()),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use bytes::{BufMut, BytesMut};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Notify;

    use super::*;

    fn b(pid: i32, start: &str) -> Backend {
        Backend {
            pid,
            start: start.into(),
            user: "o'neil\\x".into(),
            addr: Some("10.0.0.7".into()),
        }
    }

    #[test]
    fn terminate_matches_pid_start_user_and_address() {
        let sql = terminate_sql(&[
            b(12, "2026-10-31 07:59:09.418230"),
            Backend {
                addr: None,
                ..b(13, "s")
            },
        ]);
        assert!(sql.contains("datname = current_database()"), "{sql}");
        assert!(
            sql.contains(
                "(pid = 12 AND to_char(backend_start AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS.US') = \
                 E'2026-10-31 07:59:09.418230' AND usename = E'o''neil\\\\x' AND host(client_addr) IS NOT DISTINCT \
                 FROM E'10.0.0.7')"
            ),
            "{sql}"
        );
        assert!(
            sql.contains("pid = 13") && sql.contains("IS NOT DISTINCT FROM NULL)"),
            "{sql}"
        );
    }

    #[test]
    fn a_reused_pid_is_another_backend() {
        let known = [b(12, "t1"), b(13, "t1")];
        let all = union(&[b(12, "t2")], &known);
        assert_eq!(all.len(), 3);
        assert_eq!(union(&known, &[b(13, "t1")]).len(), 2);
    }

    /// A pooler and the Postgres behind it, faked together: the pooler hands out at most
    /// `size` server connections, and the Postgres side answers the queries the exact
    /// reset sends on direct from the same state.
    #[derive(Default)]
    struct Fake {
        size: usize,
        /// Hold a server for the connection's life, not only in a transaction.
        session: bool,
        /// Answer BEGIN without a server, like PgDog.
        defer_begin: bool,
        /// Give every client the same backend, even inside a transaction.
        shared: bool,
        servers: Vec<Server>,
        next: i32,
        /// Sessions on direct that aren't the pooler's: (pid, state).
        neighbours: Vec<(i32, &'static str)>,
        terminate_sql: Vec<String>,
    }

    struct Server {
        pid: i32,
        start: String,
        holder: Option<u64>,
        in_txn: bool,
    }

    struct Shared {
        fake: std::sync::Mutex<Fake>,
        freed: Notify,
    }

    impl Shared {
        fn lock(&self) -> std::sync::MutexGuard<'_, Fake> {
            self.fake.lock().unwrap()
        }

        async fn acquire(&self, conn: u64) -> (i32, String) {
            loop {
                let freed = self.freed.notified();
                {
                    let mut f = self.lock();
                    if f.shared {
                        return (7, "s7".into());
                    }
                    if let Some(s) = f.servers.iter().find(|s| s.holder == Some(conn)) {
                        return (s.pid, s.start.clone());
                    }
                    if let Some(s) = f.servers.iter_mut().find(|s| s.holder.is_none()) {
                        s.holder = Some(conn);
                        return (s.pid, s.start.clone());
                    }
                    if f.servers.len() < f.size {
                        f.next += 1;
                        let (pid, start) = (100 + f.next, format!("t{}", f.next));
                        f.servers.push(Server {
                            pid,
                            start: start.clone(),
                            holder: Some(conn),
                            in_txn: false,
                        });
                        return (pid, start);
                    }
                }
                freed.await;
            }
        }

        fn release(&self, conn: u64) {
            for s in self.lock().servers.iter_mut().filter(|s| s.holder == Some(conn)) {
                s.holder = None;
                s.in_txn = false;
            }
            self.freed.notify_waiters();
        }

        fn set_txn(&self, conn: u64, on: bool) {
            for s in self.lock().servers.iter_mut().filter(|s| s.holder == Some(conn)) {
                s.in_txn = on;
            }
        }
    }

    type Rows = Vec<Vec<Option<String>>>;

    fn row(values: &[&str]) -> Vec<Option<String>> {
        values.iter().map(|v| Some(v.to_string())).collect()
    }

    /// The numbers after `pid IN (`.
    fn pids_in(sql: &str) -> Vec<i32> {
        let list = sql
            .split("pid IN (")
            .nth(1)
            .and_then(|r| r.split(')').next())
            .unwrap_or("");
        list.split(", ").filter_map(|p| p.parse().ok()).collect()
    }

    async fn pooler_query(shared: &Shared, conn: u64, sql: &str, pending_txn: &mut bool) -> Rows {
        if sql == "BEGIN" {
            *pending_txn = true;
            if !shared.lock().defer_begin {
                shared.acquire(conn).await;
                shared.set_txn(conn, true);
            }
            return vec![];
        }
        if sql == "ROLLBACK" {
            *pending_txn = false;
            shared.set_txn(conn, false);
            if !shared.lock().session {
                shared.release(conn);
            }
            return vec![];
        }
        let (pid, start) = shared.acquire(conn).await;
        if *pending_txn {
            shared.set_txn(conn, true);
        }
        let rows = if sql.starts_with("SELECT pid::text") {
            // Postgres sees the pooler's own user, not the client's.
            vec![row(&[&pid.to_string(), &start, "postgres", "10.0.0.7"])]
        } else if sql == "SELECT pg_backend_pid()" {
            vec![row(&[&pid.to_string()])]
        } else {
            vec![row(&["1"])]
        };
        if !*pending_txn && !shared.lock().session {
            shared.release(conn);
        }
        rows
    }

    fn direct_query(shared: &Shared, sql: &str) -> Rows {
        let mut f = shared.lock();
        if sql.contains(", state FROM pg_stat_activity") {
            let pids = pids_in(sql);
            // The one shared backend is in its clients' transactions.
            if f.shared {
                return vec![row(&["7", "s7", "postgres", "10.0.0.7", "idle in transaction"])];
            }
            return f
                .servers
                .iter()
                .filter(|s| pids.contains(&s.pid))
                .map(|s| {
                    let state = if s.in_txn { "idle in transaction" } else { "idle" };
                    // Postgres sees the pooler's own user, not the client's.
                    row(&[&s.pid.to_string(), &s.start, "postgres", "10.0.0.7", state])
                })
                .collect();
        }
        if sql.contains("pg_terminate_backend") {
            f.terminate_sql.push(sql.to_string());
            let before = f.servers.len();
            f.servers
                .retain(|s| !sql.contains(&format!("pid = {} AND {START} = E'{}'", s.pid, s.start)));
            let ended = before - f.servers.len();
            drop(f);
            shared.freed.notify_waiters();
            return (0..ended).map(|_| row(&["t"])).collect();
        }
        if sql.contains("NOT IN") {
            return f
                .neighbours
                .iter()
                .map(|(pid, state)| row(&[&pid.to_string(), state, "q", "s"]))
                .collect();
        }
        if sql.starts_with("SELECT pid::text, to_char(") {
            let pids = pids_in(sql);
            return f
                .servers
                .iter()
                .filter(|s| pids.contains(&s.pid))
                .map(|s| row(&[&s.pid.to_string(), &s.start]))
                .collect();
        }
        vec![]
    }

    async fn serve(shared: Arc<Shared>, mut socket: TcpStream, conn: u64, pooler: bool) -> std::io::Result<()> {
        let len = socket.read_i32().await?;
        let mut startup = vec![0; len as usize - 4];
        socket.read_exact(&mut startup).await?;
        let mut out = BytesMut::new();
        message(&mut out, b'R', |b| b.put_i32(0));
        message(&mut out, b'K', |b| {
            b.put_i32(conn as i32);
            b.put_i32(0);
        });
        message(&mut out, b'Z', |b| b.put_u8(b'I'));
        socket.write_all(&out).await?;
        let mut pending_txn = false;
        let result = async {
            loop {
                let kind = socket.read_u8().await?;
                let len = socket.read_i32().await?;
                let mut body = vec![0; len as usize - 4];
                socket.read_exact(&mut body).await?;
                if kind != b'Q' {
                    return Ok(());
                }
                let sql = String::from_utf8_lossy(&body[..body.len() - 1]).to_string();
                let rows = if pooler {
                    pooler_query(&shared, conn, &sql, &mut pending_txn).await
                } else {
                    direct_query(&shared, &sql)
                };
                let mut out = BytesMut::new();
                if let Some(first) = rows.first() {
                    message(&mut out, b'T', |b| {
                        b.put_i16(first.len() as i16);
                        for _ in first {
                            b.put_slice(b"c\0");
                            b.put_i32(0);
                            b.put_i16(0);
                            b.put_u32(25);
                            b.put_i16(-1);
                            b.put_i32(-1);
                            b.put_i16(0);
                        }
                    });
                }
                for r in &rows {
                    message(&mut out, b'D', |b| {
                        b.put_i16(r.len() as i16);
                        for v in r {
                            match v {
                                Some(v) => {
                                    b.put_i32(v.len() as i32);
                                    b.put_slice(v.as_bytes());
                                }
                                None => b.put_i32(-1),
                            }
                        }
                    });
                }
                message(&mut out, b'C', |b| b.put_slice(b"OK\0"));
                message(&mut out, b'Z', |b| b.put_u8(b'I'));
                socket.write_all(&out).await?;
            }
        }
        .await;
        if pooler {
            shared.release(conn);
        }
        result
    }

    fn message(out: &mut BytesMut, kind: u8, body: impl FnOnce(&mut BytesMut)) {
        let mut b = BytesMut::new();
        body(&mut b);
        out.put_u8(kind);
        out.put_i32(b.len() as i32 + 4);
        out.put_slice(&b);
    }

    async fn listen(shared: Arc<Shared>, pooler: bool, ids: Arc<AtomicU64>) -> Endpoint {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let id = ids.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serve(shared.clone(), socket, id, pooler));
            }
        });
        Endpoint::parse(&format!("postgres://pp.tenant@127.0.0.1:{port}/pp?sslmode=disable")).unwrap()
    }

    async fn fake(fake: Fake) -> (Arc<Shared>, Endpoint, Endpoint) {
        let shared = Arc::new(Shared {
            fake: std::sync::Mutex::new(fake),
            freed: Notify::new(),
        });
        let ids = Arc::new(AtomicU64::new(1));
        let pooled = listen(shared.clone(), true, ids.clone()).await;
        let direct = listen(shared.clone(), false, ids).await;
        (shared, pooled, direct)
    }

    fn quick() -> Timeouts {
        Timeouts {
            housekeeping: Duration::from_millis(400),
            ready: Duration::from_secs(2),
            ..Timeouts::default()
        }
    }

    fn pids(shared: &Shared) -> Vec<i32> {
        shared.lock().servers.iter().map(|s| s.pid).collect()
    }

    #[tokio::test]
    async fn finds_and_ends_only_the_pools_servers() {
        for (size, session, defer_begin) in [(1, false, false), (2, false, false), (1, false, true), (1, true, false)] {
            let what = format!("size {size}, session {session}, deferred BEGIN {defer_begin}");
            let (shared, pooled, direct) = fake(Fake {
                size,
                session,
                defer_begin,
                neighbours: vec![(555, "idle")],
                ..Fake::default()
            })
            .await;
            let exact = Exact::new(pooled, direct, size as u32);
            let t = quick();
            let line = exact.check_start(&t).await.unwrap_or_else(|e| panic!("{what}: {e:#}"));
            assert!(
                line.starts_with(&format!("held {}, client {} waited", transactions(size), size + 1)),
                "{line}"
            );
            let before = pids(&shared);
            assert_eq!(before.len(), size, "{what}");
            exact.reset(&t).await.unwrap_or_else(|e| panic!("{what}: {e:#}"));
            let after = pids(&shared);
            assert_eq!(after.len(), size, "{what}");
            assert!(
                after.iter().all(|p| !before.contains(p)),
                "{what}: {before:?} -> {after:?}"
            );
            // The user Postgres sees, not the client's.
            let sql = shared.lock().terminate_sql.join("\n");
            assert!(
                sql.contains("usename = E'postgres'") && !sql.contains("pp.tenant"),
                "{sql}"
            );
            exact.reset(&t).await.unwrap_or_else(|e| panic!("{what}, again: {e:#}"));
        }
    }

    #[tokio::test]
    async fn a_busy_server_seen_before_is_still_ended() {
        let (shared, pooled, direct) = fake(Fake {
            size: 1,
            ..Fake::default()
        })
        .await;
        let exact = Exact::new(pooled, direct, 1);
        let t = quick();
        exact.reset(&t).await.unwrap();
        // An abandoned query keeps the only server busy.
        let busy = pids(&shared)[0];
        shared.lock().servers[0].holder = Some(9999);
        exact.reset(&t).await.unwrap();
        assert!(!pids(&shared).contains(&busy));
    }

    #[tokio::test]
    async fn a_server_never_seen_is_listed_not_ended_and_stops_the_target() {
        let (shared, pooled, direct) = fake(Fake {
            size: 2,
            neighbours: vec![(555, "active")],
            ..Fake::default()
        })
        .await;
        // A crashed run's query keeps one of two servers busy before poolproof saw it.
        shared.lock().servers.push(Server {
            pid: 555,
            start: "old".into(),
            holder: Some(9999),
            in_txn: false,
        });
        let exact = Exact::new(pooled, direct, 2);
        let t = quick();
        let err = format!("{:#}", exact.reset(&t).await.unwrap_err());
        assert!(
            err.starts_with("exact reset: could hold only 1 of 2 transactions"),
            "{err}"
        );
        assert!(err.contains("pid 555 (active"), "{err}");
        assert!(
            err.contains("End those that are the pooler's yourself, or restart the pooler"),
            "{err}"
        );
        assert!(
            err.contains("but it ends every session of user postgres from 10.0.0.7"),
            "{err}"
        );
        assert!(shared.lock().terminate_sql.is_empty());
        // Every later reset of the target fails the same way, at once.
        let again = format!("{:#}", exact.reset(&t).await.unwrap_err());
        assert_eq!(again, err);
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn postgres_as_its_own_pooler_is_refused() {
        let url = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
        let pg = Endpoint::parse(&url).unwrap();
        let exact = Exact::new(pg.clone(), pg, 1);
        let err = format!("{:#}", exact.check_start(&quick()).await.unwrap_err());
        assert_eq!(err, "pool_size = 1, but client 2 got its own server connection");
        // The target is done: its reset fails the same way.
        assert_eq!(format!("{:#}", exact.reset(&quick()).await.unwrap_err()), err);
    }

    async fn pid_of(c: &Client) -> i32 {
        first_pid(c.simple_query("SELECT pg_backend_pid()").await.unwrap()).unwrap()
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn on_a_real_server_only_the_held_backend_is_ended() {
        let url = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
        let direct = Endpoint::parse(&url).unwrap();
        let sep = if url.contains('?') { '&' } else { '?' };
        let pooled = Endpoint::parse(&format!("{url}{sep}application_name=pp_stand_in")).unwrap();
        let t = Timeouts::default();
        // A pooler's server connection, held in a transaction, and a neighbour with the same
        // role and address connected straight to Postgres.
        let (stand_in, _stand_in_driver, _) = connect(&pooled, &t).await.unwrap();
        stand_in.simple_query("BEGIN").await.unwrap();
        let pid = pid_of(&stand_in).await;
        let (neighbour, _neighbour_driver, _) = connect(&direct, &t).await.unwrap();
        let neighbour_pid = pid_of(&neighbour).await;
        let exact = Exact::new(pooled, direct.clone(), 1);
        let held = exact.confirm(&[pid], &t).await.unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].pid, pid);
        // The neighbour's pid with another start, as if it were reused: left alone.
        let reused = Backend {
            pid: neighbour_pid,
            start: "2000-01-01 00:00:00.000000".into(),
            ..held[0].clone()
        };
        let sql = terminate_sql(&[held[0].clone(), reused]);
        run_direct(&direct, &[Statement::plain(&sql)], &t).await.unwrap();
        assert!(
            stand_in.simple_query("SELECT 1").await.is_err(),
            "the held backend survived"
        );
        assert!(
            neighbour.simple_query("SELECT 1").await.is_ok(),
            "the neighbour was ended"
        );
    }

    #[tokio::test]
    async fn the_start_check_catches_a_bigger_or_shared_pool() {
        let t = quick();
        let (_, pooled, direct) = fake(Fake {
            size: 2,
            ..Fake::default()
        })
        .await;
        let err = format!("{:#}", Exact::new(pooled, direct, 1).check_start(&t).await.unwrap_err());
        assert_eq!(err, "pool_size = 1, but client 2 got its own server connection");
        let (_, pooled, direct) = fake(Fake {
            size: 1,
            shared: true,
            ..Fake::default()
        })
        .await;
        let err = format!(
            "{:#}",
            Exact::new(pooled.clone(), direct.clone(), 2)
                .check_start(&t)
                .await
                .unwrap_err()
        );
        assert_eq!(
            err,
            "the pooler gave another client a server connection inside an open transaction (pid 7)"
        );
        let err = format!("{:#}", Exact::new(pooled, direct, 1).check_start(&t).await.unwrap_err());
        assert!(err.ends_with("inside an open transaction (pid 7)"), "{err}");
        let (_, pooled, direct) = fake(Fake {
            size: 1,
            ..Fake::default()
        })
        .await;
        let err = format!("{:#}", Exact::new(pooled, direct, 2).check_start(&t).await.unwrap_err());
        assert_eq!(
            err,
            "exact reset: could hold only 1 of 2 transactions through the pooler (pool_size = 2)"
        );
    }
}
