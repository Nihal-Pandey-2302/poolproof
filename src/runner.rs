use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_postgres::{AsyncMessage, Client, NoTls, SimpleQueryMessage, Statement};

use crate::raw::RawConn;
use crate::scenario::{Action, Compare, Scenario};

const STEP_TIMEOUT: Duration = Duration::from_secs(10);
/// For `send` steps without a Sync: how long the server must be quiet before the
/// replies are considered complete.
const RAW_QUIET: Duration = Duration::from_millis(500);
/// How long a `notifications` step waits for the first NOTIFY to arrive.
const NOTIFY_WAIT: Duration = Duration::from_secs(1);
/// How long to wait for the pooler to answer again after a reset.
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// What a client observed for one step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Outcome {
    Ok {
        rows: Vec<Vec<Option<String>>>,
        /// Affected-row counts from CommandComplete, one per statement.
        tags: Vec<u64>,
    },
    Error {
        code: String,
        message: String,
    },
    /// The client dropped the connection before the reply came.
    Abandoned,
    /// Every reply to a `send` step, one line each.
    Messages { messages: Vec<String> },
}

impl Outcome {
    fn empty() -> Self {
        Outcome::Ok { rows: vec![], tags: vec![] }
    }

    fn error(code: &str, message: impl Into<String>) -> Self {
        Outcome::Error { code: code.to_string(), message: message.into() }
    }

    fn matches(&self, other: &Outcome, compare: Compare) -> bool {
        match compare {
            Compare::Ignore => true,
            Compare::Status => match (self, other) {
                (Outcome::Ok { .. }, Outcome::Ok { .. }) | (Outcome::Abandoned, Outcome::Abandoned) => true,
                (Outcome::Messages { messages: a }, Outcome::Messages { messages: b }) => {
                    let failures = |m: &[String]| m.iter().filter(|l| is_failure_line(l)).cloned().collect::<Vec<_>>();
                    failures(a) == failures(b)
                }
                (Outcome::Error { code: a, .. }, Outcome::Error { code: b, .. }) => a == b,
                _ => false,
            },
            // Error messages can differ in wording between servers, so only the code counts.
            Compare::Full => match (self, other) {
                (Outcome::Error { code: a, .. }, Outcome::Error { code: b, .. }) => a == b,
                _ => self == other,
            },
        }
    }
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Ok { rows, tags } => {
                let rows: Vec<String> = rows
                    .iter()
                    .map(|r| {
                        let cols: Vec<&str> = r.iter().map(|c| c.as_deref().unwrap_or("NULL")).collect();
                        format!("({})", cols.join(", "))
                    })
                    .collect();
                write!(f, "ok rows=[{}] tags={tags:?}", rows.join(" "))
            }
            Outcome::Error { code, message } => write!(f, "error {code}: {message}"),
            Outcome::Abandoned => write!(f, "abandoned"),
            Outcome::Messages { messages } => write!(f, "{}", messages.join(", ")),
        }
    }
}

/// NOTIFY messages received on a connection, as (channel, payload) rows.
type Notices = Arc<Mutex<Vec<Vec<Option<String>>>>>;

struct Conn {
    client: Arc<Client>,
    driver: JoinHandle<()>,
    direct: bool,
    statements: HashMap<String, Statement>,
    /// The query started by a `background` step, if not yet waited for.
    pending: Option<JoinHandle<Outcome>>,
    notices: Notices,
    /// Backend pid when connected straight to Postgres, so a disconnect can wait for
    /// the backend to exit.
    pid: Option<i32>,
}

/// The backend pid of a direct connection.
async fn backend_pid(client: &Client) -> Option<i32> {
    let messages = timeout(STEP_TIMEOUT, client.simple_query("SELECT pg_backend_pid()")).await.ok()?.ok()?;
    messages.into_iter().find_map(|m| match m {
        SimpleQueryMessage::Row(row) => row.get(0)?.parse().ok(),
        _ => None,
    })
}

/// Waits until a Postgres backend has exited. Closing the socket returns before the
/// backend is gone, and until then its session state (locks, temp tables) is still
/// visible, which would make the baseline racy.
async fn wait_backend_gone(direct: &str, pid: i32) {
    let Ok((client, driver, _)) = connect(direct).await else { return };
    let deadline = tokio::time::Instant::now() + STEP_TIMEOUT;
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
    let _ = timeout(STEP_TIMEOUT, driver).await;
}

impl Conn {
    /// Close the connection and wait for Terminate to go out, so the pooler sees the
    /// disconnect before the next step runs.
    async fn close(self) {
        if let Some(pending) = self.pending {
            pending.abort();
        }
        drop(self.client);
        let _ = timeout(STEP_TIMEOUT, self.driver).await;
    }
}

async fn connect(url: &str) -> Result<(Client, JoinHandle<()>, Notices)> {
    let (client, mut connection) = timeout(STEP_TIMEOUT, tokio_postgres::connect(url, NoTls))
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

fn db_error(e: &tokio_postgres::Error) -> Outcome {
    match e.as_db_error() {
        Some(db) => Outcome::error(db.code().code(), db.message()),
        None if e.is_closed() => Outcome::error("CLOSED", e.to_string()),
        None => Outcome::error("CLIENT", e.to_string()),
    }
}

fn simple_outcome(res: Result<Vec<SimpleQueryMessage>, tokio_postgres::Error>) -> Outcome {
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

fn row_value(row: &tokio_postgres::Row, idx: usize) -> Option<String> {
    // Extended-protocol rows come back in binary. Text-like columns decode directly;
    // anything else should be cast to text in the scenario SQL.
    match row.try_get::<_, Option<String>>(idx) {
        Ok(v) => v,
        Err(_) => Some(format!("<{}>", row.columns()[idx].type_().name())),
    }
}

/// Run a scenario's steps. `pooled` is where normal clients connect; `direct` is
/// plain Postgres. For the baseline run both are the same URL.
pub async fn run_steps(scenario: &Scenario, pooled: &str, direct: &str) -> Result<Vec<Outcome>> {
    let mut conns: HashMap<&str, Conn> = HashMap::new();
    let mut outcomes = Vec::with_capacity(scenario.steps.len());

    // Clients that use `send` talk raw protocol on their own connection.
    let raw_clients: std::collections::HashSet<&str> =
        scenario.steps.iter().filter(|s| s.send.is_some()).map(|s| s.client.as_str()).collect();
    let mut raw: HashMap<&str, RawConn> = HashMap::new();

    for step in &scenario.steps {
        if raw_clients.contains(step.client.as_str()) {
            if let Some(ms) = step.delay_ms {
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
            let outcome = match step.action()? {
                Action::Send(messages) => {
                    if !raw.contains_key(step.client.as_str()) {
                        let url = if step.direct { direct } else { pooled };
                        match RawConn::connect(url, STEP_TIMEOUT).await {
                            Ok(conn) => {
                                raw.insert(&step.client, conn);
                            }
                            Err(e) => {
                                outcomes.push(Outcome::error("CONNECT", format!("{e:#}")));
                                continue;
                            }
                        }
                    }
                    let conn = raw.get_mut(step.client.as_str()).expect("just connected");
                    match conn.exchange(&messages, STEP_TIMEOUT, RAW_QUIET).await {
                        Ok(messages) => Outcome::Messages { messages },
                        Err(e) => Outcome::error("CLIENT", format!("{e:#}")),
                    }
                }
                Action::Disconnect => {
                    if let Some(conn) = raw.remove(step.client.as_str()) {
                        let pid = conn.pid;
                        let was_direct = conn.url == direct;
                        conn.terminate().await;
                        if let (true, Some(pid)) = (was_direct, pid) {
                            wait_backend_gone(direct, pid).await;
                        }
                    }
                    Outcome::empty()
                }
                _ => bail!("client '{}' uses send, so its only other step can be disconnect", step.client),
            };
            outcomes.push(outcome);
            continue;
        }
        if !conns.contains_key(step.client.as_str()) {
            let url = if step.direct { direct } else { pooled };
            match connect(url).await {
                Ok((client, driver, notices)) => {
                    let pid = if url == direct { backend_pid(&client).await } else { None };
                    conns.insert(
                        &step.client,
                        Conn {
                            client: Arc::new(client),
                            driver,
                            direct: step.direct,
                            statements: HashMap::new(),
                            pending: None,
                            notices,
                            pid,
                        },
                    );
                }
                Err(e) => {
                    outcomes.push(Outcome::error("CONNECT", format!("{e:#}")));
                    continue;
                }
            }
        }
        let Some(conn) = conns.get_mut(step.client.as_str()) else {
            bail!("client '{}' is not connected", step.client);
        };
        if conn.direct != step.direct {
            bail!("client '{}' mixes direct and pooled steps", step.client);
        }
        if let Some(ms) = step.delay_ms {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }

        let outcome = match step.action()? {
            Action::Send(_) => unreachable!("send clients are handled above"),
            Action::Simple(sql) => match timeout(STEP_TIMEOUT, conn.client.simple_query(sql)).await {
                Err(_) => Outcome::error("TIMEOUT", format!("no reply within {STEP_TIMEOUT:?}")),
                Ok(res) => simple_outcome(res),
            },
            Action::Abandon { sql, after } => {
                let outcome = match timeout(after, conn.client.simple_query(sql)).await {
                    Err(_) => Outcome::Abandoned,
                    Ok(res) => simple_outcome(res),
                };
                // Aborting the driver drops the socket without sending Terminate.
                if let Some(conn) = conns.remove(step.client.as_str()) {
                    if let Some(pending) = conn.pending {
                        pending.abort();
                    }
                    conn.driver.abort();
                    let _ = conn.driver.await;
                }
                outcome
            }
            Action::Prepare { sql, name } => match timeout(STEP_TIMEOUT, conn.client.prepare(sql)).await {
                Err(_) => Outcome::error("TIMEOUT", format!("no reply within {STEP_TIMEOUT:?}")),
                Ok(Err(e)) => db_error(&e),
                Ok(Ok(statement)) => {
                    conn.statements.insert(name.to_string(), statement);
                    Outcome::empty()
                }
            },
            Action::Execute(name) => {
                // The prepare step may have failed through the pooler; that mismatch is
                // already reported, so record this step as an error instead of aborting.
                let Some(statement) = conn.statements.get(name) else {
                    outcomes.push(Outcome::error("NO_STATEMENT", format!("'{name}' was never prepared")));
                    continue;
                };
                match timeout(STEP_TIMEOUT, conn.client.query(statement, &[])).await {
                    Err(_) => Outcome::error("TIMEOUT", format!("no reply within {STEP_TIMEOUT:?}")),
                    Ok(Err(e)) => db_error(&e),
                    Ok(Ok(rows)) => Outcome::Ok {
                        rows: rows
                            .iter()
                            .map(|r| (0..r.len()).map(|i| row_value(r, i)).collect())
                            .collect(),
                        tags: vec![rows.len() as u64],
                    },
                }
            }
            Action::Disconnect => {
                if let Some(conn) = conns.remove(step.client.as_str()) {
                    let pid = conn.pid;
                    conn.close().await;
                    if let Some(pid) = pid {
                        wait_backend_gone(direct, pid).await;
                    }
                }
                Outcome::empty()
            }
            Action::Background(sql) => {
                if conn.pending.is_some() {
                    bail!("client '{}' already has a background query", step.client);
                }
                let client = conn.client.clone();
                let sql = sql.to_string();
                conn.pending = Some(tokio::spawn(async move {
                    match timeout(STEP_TIMEOUT, client.simple_query(&sql)).await {
                        Err(_) => Outcome::error("TIMEOUT", format!("no reply within {STEP_TIMEOUT:?}")),
                        Ok(res) => simple_outcome(res),
                    }
                }));
                Outcome::empty()
            }
            Action::Wait => {
                let Some(pending) = conn.pending.take() else {
                    bail!("client '{}' has no background query to wait for", step.client);
                };
                // The query has its own timeout; this one only guards against a stuck task.
                match timeout(STEP_TIMEOUT * 2, pending).await {
                    Ok(Ok(outcome)) => outcome,
                    Ok(Err(e)) => Outcome::error("CLIENT", e.to_string()),
                    Err(_) => Outcome::error("TIMEOUT", "background query never finished"),
                }
            }
            Action::Cancel => {
                let token = conn.client.cancel_token();
                match timeout(STEP_TIMEOUT, token.cancel_query(NoTls)).await {
                    Err(_) => Outcome::error("TIMEOUT", format!("no reply within {STEP_TIMEOUT:?}")),
                    Ok(Err(e)) => db_error(&e),
                    Ok(Ok(())) => Outcome::empty(),
                }
            }
            Action::Notifications => {
                let deadline = tokio::time::Instant::now() + NOTIFY_WAIT;
                while conn.notices.lock().unwrap().is_empty() && tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                // Give any NOTIFYs sent together with the first one time to arrive.
                tokio::time::sleep(Duration::from_millis(100)).await;
                let rows = std::mem::take(&mut *conn.notices.lock().unwrap());
                Outcome::Ok { rows, tags: vec![] }
            }
        };
        outcomes.push(outcome);
    }

    for (_, conn) in conns.drain() {
        conn.close().await;
    }
    for (_, conn) in raw.drain() {
        conn.terminate().await;
    }
    Ok(outcomes)
}

async fn run_direct(url: &str, statements: &[String]) -> Result<()> {
    if statements.is_empty() {
        return Ok(());
    }
    let (client, driver, _) = connect(url).await?;
    for sql in statements {
        client.batch_execute(sql).await.with_context(|| format!("running `{sql}`"))?;
    }
    drop(client);
    let _ = driver.await;
    Ok(())
}

/// The server's version number, e.g. `17.11`.
pub async fn server_version(url: &str) -> Result<String> {
    let (client, driver, _) = connect(url).await?;
    let res = timeout(STEP_TIMEOUT, client.simple_query("SHOW server_version")).await;
    drop(client);
    let _ = timeout(STEP_TIMEOUT, driver).await;
    let version = res.context("SHOW server_version timed out")??.into_iter().find_map(|m| match m {
        SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
        _ => None,
    });
    // "17.11 (Debian 17.11-1.pgdg13+2)" -> "17.11"
    let version = version.context("no server_version")?;
    Ok(version.split_whitespace().next().unwrap_or_default().to_string())
}

/// Retry `SELECT 1` through the pooler until it answers. A reset kills the pooler's
/// server connections, and it needs a moment to notice and open new ones.
async fn wait_ready(url: &str) -> Result<()> {
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        let err = match connect(url).await {
            Ok((client, driver, _)) => {
                let res = timeout(STEP_TIMEOUT, client.simple_query("SELECT 1")).await;
                drop(client);
                let _ = timeout(STEP_TIMEOUT, driver).await;
                match res {
                    Ok(Ok(_)) => return Ok(()),
                    Ok(Err(e)) => anyhow::Error::from(e),
                    Err(_) => anyhow::anyhow!("SELECT 1 timed out"),
                }
            }
            Err(e) => e,
        };
        if tokio::time::Instant::now() >= deadline {
            return Err(err.context(format!("pooler not ready {READY_TIMEOUT:?} after reset")));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Mismatch {
    pub step: usize,
    pub action: String,
    pub kind: FailKind,
    pub expected: Outcome,
    pub actual: Outcome,
}

/// How a failure shows up to the application. Silent failures are the dangerous ones:
/// the client gets a result and no hint that it's wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailKind {
    /// The pooler returned a result, but not the one Postgres returned.
    Silent,
    /// The pooler returned an error where Postgres didn't, or a different one.
    Error,
    /// The pooler didn't answer within the step timeout.
    Hang,
}

/// A `send` reply line that means something went wrong: an error, a timeout, or
/// the connection closing.
fn is_failure_line(line: &str) -> bool {
    line.starts_with("Error ") || line == "TIMEOUT" || line == "CLOSED"
}

impl FailKind {
    fn of(expected: &Outcome, actual: &Outcome) -> Self {
        match actual {
            Outcome::Error { code, .. } if code == "TIMEOUT" => FailKind::Hang,
            Outcome::Error { .. } => FailKind::Error,
            Outcome::Messages { messages } => {
                let expected_lines = match expected {
                    Outcome::Messages { messages } => messages.as_slice(),
                    _ => &[],
                };
                if messages.iter().any(|l| l == "TIMEOUT") {
                    FailKind::Hang
                } else if messages.iter().any(|l| is_failure_line(l) && !expected_lines.contains(l)) {
                    FailKind::Error
                } else {
                    FailKind::Silent
                }
            }
            Outcome::Ok { .. } | Outcome::Abandoned => FailKind::Silent,
        }
    }
}

impl std::fmt::Display for FailKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            FailKind::Silent => "silent",
            FailKind::Error => "error",
            FailKind::Hang => "hang",
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail { mismatch: Mismatch },
    Skip { reason: String },
    /// The run could not be set up, so there is nothing to compare.
    Error { reason: String },
}

/// The verdict plus each step's (postgres, pooler) outcomes. `steps` is empty when the
/// pooler run never happened. With `repeat`, the first failing run is the one kept.
pub struct Checked {
    pub verdict: Verdict,
    pub steps: Vec<(Outcome, Outcome)>,
    pub runs: u32,
    pub failures: u32,
}

/// Run a scenario against plain Postgres, then through the pooler, and compare.
/// `reset` runs on `direct` before each run so state leaked on the pooler's server
/// connections by an earlier scenario can't change this one's result.
pub async fn check(scenario: &Scenario, pooled: &str, direct: &str, reset: &[String]) -> Result<Checked> {
    run_direct(direct, reset).await.context("reset (baseline)")?;
    run_direct(direct, &scenario.setup).await.context("setup (baseline)")?;
    let expected = run_steps(scenario, direct, direct).await;
    run_direct(direct, &scenario.teardown).await.context("teardown (baseline)")?;
    let expected = expected?;

    run_direct(direct, reset).await.context("reset (target)")?;
    if !reset.is_empty()
        && let Err(e) = wait_ready(pooled).await
    {
        return Ok(Checked { verdict: Verdict::Error { reason: format!("{e:#}") }, steps: vec![], runs: 0, failures: 0 });
    }

    let mut kept: Option<Checked> = None;
    let mut failures = 0;
    for run in 1..=scenario.repeat {
        let checked = if run == 1 {
            check_target(scenario, pooled, direct, &expected).await?
        } else {
            run_direct(direct, reset).await.context("reset (target)")?;
            match (reset.is_empty(), wait_ready(pooled).await) {
                (false, Err(e)) => {
                    Checked { verdict: Verdict::Error { reason: format!("{e:#}") }, steps: vec![], runs: 1, failures: 0 }
                }
                _ => check_target(scenario, pooled, direct, &expected).await?,
            }
        };
        let failed = !matches!(checked.verdict, Verdict::Pass);
        failures += u32::from(failed);
        // Keep the first failure; otherwise the latest pass.
        if kept.as_ref().is_none_or(|k| matches!(k.verdict, Verdict::Pass)) {
            kept = Some(checked);
        }
    }
    let mut checked = kept.expect("repeat is at least 1");
    checked.runs = scenario.repeat;
    checked.failures = failures;
    Ok(checked)
}

/// One pooler-side run of a scenario, compared with the baseline outcomes.
async fn check_target(scenario: &Scenario, pooled: &str, direct: &str, expected: &[Outcome]) -> Result<Checked> {
    run_direct(direct, &scenario.setup).await.context("setup (target)")?;
    let actual = run_steps(scenario, pooled, direct).await;
    run_direct(direct, &scenario.teardown).await.context("teardown (target)")?;
    let actual = actual?;

    let mismatch = scenario.steps.iter().enumerate().find_map(|(i, step)| {
        (!expected[i].matches(&actual[i], step.compare)).then(|| Mismatch {
            step: i + 1,
            action: step.describe(),
            kind: FailKind::of(&expected[i], &actual[i]),
            expected: expected[i].clone(),
            actual: actual[i].clone(),
        })
    });
    let verdict = match mismatch {
        Some(mismatch) => Verdict::Fail { mismatch },
        None => Verdict::Pass,
    };
    Ok(Checked { verdict, steps: expected.iter().cloned().zip(actual).collect(), runs: 1, failures: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_compare_by_code_only() {
        let a = Outcome::error("42P01", "relation \"x\" does not exist");
        let b = Outcome::error("42P01", "relation x does not exist");
        assert!(a.matches(&b, Compare::Full));
        assert!(!a.matches(&Outcome::empty(), Compare::Status));
    }

    #[test]
    fn fail_kind_follows_pooler_outcome() {
        let ok = Outcome::empty();
        assert_eq!(FailKind::of(&ok, &Outcome::empty()), FailKind::Silent);
        assert_eq!(FailKind::of(&ok, &Outcome::error("XX000", "not allowed")), FailKind::Error);
        assert_eq!(FailKind::of(&ok, &Outcome::error("TIMEOUT", "no reply")), FailKind::Hang);
        let msgs = |m: &[&str]| Outcome::Messages { messages: m.iter().map(|s| s.to_string()).collect() };
        let pg = msgs(&["ParseComplete", "Error 42601", "ReadyForQuery I"]);
        assert_eq!(FailKind::of(&pg, &msgs(&["ParseComplete", "ReadyForQuery I"])), FailKind::Silent);
        assert_eq!(FailKind::of(&pg, &msgs(&["Error 08P01", "ReadyForQuery I"])), FailKind::Error);
        assert_eq!(FailKind::of(&pg, &msgs(&["ParseComplete", "TIMEOUT"])), FailKind::Hang);
    }

    #[test]
    fn full_compare_checks_rows() {
        let a = Outcome::Ok { rows: vec![vec![Some("1".into())]], tags: vec![1] };
        let b = Outcome::Ok { rows: vec![vec![Some("2".into())]], tags: vec![1] };
        assert!(!a.matches(&b, Compare::Full));
        assert!(a.matches(&b, Compare::Status));
    }
}
