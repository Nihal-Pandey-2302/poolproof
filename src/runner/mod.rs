//! Runs scenarios: steps on real connections, checks against the Postgres
//! baseline, and the loop over targets.

use std::time::Duration;

use serde::{Deserialize, Serialize};

mod check;
mod compare;
mod pg;
pub mod raw;
mod steps;
pub mod suite;

pub use check::{Checked, DIRECT_UNREACHABLE, POOLER_UNREACHABLE, check, deterministic, preflight};
pub use compare::{FailKind, Mismatch, Verdict};
pub use pg::server_version;

/// How long the harness waits for each kind of thing. `Default` holds the values
/// every run used before these were configurable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// Connecting: TCP connect, startup and authentication together, one deadline.
    pub connect: Duration,
    /// The reply to one step.
    pub step: Duration,
    /// For `send` steps without a Sync: how long the server must be quiet before the
    /// replies are considered complete.
    pub quiet: Duration,
    /// How long a `notifications` step waits for the first NOTIFY to arrive.
    pub notify: Duration,
    /// How long to wait for the pooler to answer again after a reset.
    pub ready: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            connect: Duration::from_secs(10),
            step: Duration::from_secs(10),
            quiet: Duration::from_millis(500),
            notify: Duration::from_secs(1),
            ready: Duration::from_secs(10),
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
    },
    /// The client dropped the connection before the reply came.
    Abandoned,
    /// Every reply to a `send` step, one line each.
    Messages {
        messages: Vec<String>,
    },
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
