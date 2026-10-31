//! Runs scenarios: steps on real connections, checks against the Postgres
//! baseline, and the loop over targets.

use std::time::Duration;

use serde::{Deserialize, Serialize};

mod check;
mod compare;
pub mod doctor;
mod pg;
pub(crate) mod raw;
mod steps;
pub mod suite;
pub(crate) mod tls;

pub use check::{Checked, DIRECT_UNREACHABLE, POOLER_UNREACHABLE, check, deterministic, preflight};
pub use compare::{FailKind, Mismatch, Verdict};
pub use pg::{Privileges, privileges, resolve_reset, server_version};
pub use raw::RawMessage;

/// How long the harness waits for each kind of thing. `Default` holds the values
/// every run used before these were configurable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// Connecting: TCP connect, startup and authentication together, one deadline.
    pub connect: Duration,
    /// The reply to one step: the step timeout that `--step-timeout` and a scenario's
    /// `timeout_ms` set. Only waits for replies the scenario asked for use it.
    pub step: Duration,
    /// The harness's own waits: a backend's pid, a disconnected backend going away,
    /// a driver finishing, probes and `SHOW server_version`. Never shortened per
    /// scenario: if a disconnect wait gave up early, a baseline client's lock or temp
    /// table could still be visible to the next step.
    pub housekeeping: Duration,
    /// For `send` steps without a Sync: how long the server must be quiet before the
    /// replies are considered complete.
    pub quiet: Duration,
    /// How long a `notifications` step waits for the first NOTIFY to arrive.
    pub notify: Duration,
    /// How long to wait for the pooler to answer again after a reset.
    pub ready: Duration,
    /// `lock_timeout` for setup, teardown and reset statements. A pooler can leak a
    /// lock (some scenarios test exactly that), and a teardown blocked on it must fail,
    /// not hang the run.
    pub direct_lock: Duration,
    /// `statement_timeout` for the same statements. Stays well above reset's 5 s wait
    /// per terminated backend.
    pub direct_statement: Duration,
    /// Extra time on top of `direct_statement` before giving up on the server itself.
    pub direct_grace: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            connect: Duration::from_secs(10),
            step: Duration::from_secs(10),
            housekeeping: Duration::from_secs(10),
            quiet: Duration::from_millis(500),
            notify: Duration::from_secs(1),
            ready: Duration::from_secs(10),
            direct_lock: Duration::from_secs(5),
            direct_statement: Duration::from_secs(30),
            direct_grace: Duration::from_secs(5),
        }
    }
}

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
        /// The server refused the connection (e.g. 53300 too many connections). On the
        /// Postgres side that is a harness failure, not something to compare. Left out
        /// of JSON when false, so stored results don't change.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        connect: bool,
    },
    /// The client dropped the connection before the reply came.
    Abandoned,
    /// Every reply to a `send` step, one line each.
    Messages { messages: Vec<String> },
}

impl Outcome {
    fn empty() -> Self {
        Outcome::Ok {
            rows: vec![],
            tags: vec![],
        }
    }

    fn error(code: &str, message: impl Into<String>) -> Self {
        Outcome::Error {
            code: code.to_string(),
            message: message.into(),
            connect: false,
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
            Outcome::Error { code, message, .. } => write!(f, "error {code}: {message}"),
            Outcome::Abandoned => write!(f, "abandoned"),
            Outcome::Messages { messages } => write!(f, "{}", messages.join(", ")),
        }
    }
}
