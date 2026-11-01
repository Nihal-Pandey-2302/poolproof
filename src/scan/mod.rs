//! `scan`: flags queries that break under transaction pooling.

use serde::Serialize;

mod input;
mod parameters;
mod rules;
mod statements;

pub use input::{InputFormat, load_queries, read_queries};
pub use parameters::{check_parameter, scenarios_for_parameter};
pub use rules::scan;
pub use statements::check_driver;

/// Severity of a finding under transaction pooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Depends on the pooler and its config. Check before switching.
    Warn,
    /// Breaks or leaks state under transaction pooling.
    Breaks,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule: &'static str,
    pub severity: Severity,
    pub detail: String,
    pub query: String,
    pub calls: u64,
    /// Line in the input where the query first appears.
    pub line: usize,
    /// poolproof scenarios that test this pattern, so `--pooler` can show what a
    /// given pooler actually does with it.
    pub scenarios: Vec<&'static str>,
    /// What those scenarios showed on the pooler given with `--pooler`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_pooler: Option<PoolerCheck>,
    /// For `driver-prepared-statements`: the driver whose statement names these look
    /// like, as `poolproof drivers` names it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<&'static str>,
    /// With `--pooler`, why there is no `on_pooler`, e.g. no driver results for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pooler_note: Option<String>,
    /// For `session-set`: the parameter it sets, as Postgres folds the name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    /// With `--pooler` and no `--postgres`: the same pooler on other Postgres versions,
    /// where the verdict differs from `on_pooler`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub differs_on: Vec<PoolerCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PoolerCheck {
    pub target: String,
    pub postgres: Option<String>,
    pub status: PoolerStatus,
    /// Each related scenario and its result there: pass, silent, error, hang.
    pub scenarios: Vec<(String, String)>,
    /// When every related FAIL is one the pooler's own docs describe: those docs. The
    /// status doesn't change: it still breaks the app.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documented: Vec<String>,
    /// For a driver finding: how the driver does on the pooler with the setting its docs
    /// give for poolers. Shown beside the status, never changing it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixCheck>,
    /// The results are from another Postgres version than the rest of `--pooler`'s
    /// (driver results exist for fewer versions), so the text says which.
    #[serde(skip)]
    pub other_postgres: bool,
    /// For a SET finding whose parameter none of the scenarios sets: the parameter they
    /// set instead, so the verdict only says how the pooler treats that one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stand_in: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixCheck {
    pub setting: String,
    pub status: PoolerStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PoolerStatus {
    /// A related scenario fails silently: wrong results, no error.
    Breaks,
    /// A related scenario fails with an error or hangs: the pooler refuses the pattern,
    /// or another client's query fails because of it.
    Rejected,
    /// The pooler passes every related scenario.
    Ok,
}

/// A query from the input, with how often it ran and where it first appears.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub text: String,
    pub calls: u64,
    pub line: usize,
}

/// A prepared statement name from a log's `parse`, `bind` or `execute` lines.
#[derive(Debug, Clone, PartialEq)]
pub struct StatementName {
    pub name: String,
    /// How many `execute` lines name it. Zero if only `parse` or `bind` lines do, as
    /// in a log that samples durations.
    pub executions: u64,
    /// The first line that names it, and the SQL there.
    pub line: usize,
    pub query: String,
}

/// What scan reads from its input.
#[derive(Debug, Default)]
pub struct Input {
    pub queries: Vec<Query>,
    /// Only from logs: pg_stat_statements and SQL files have no statement names.
    pub statement_names: Vec<StatementName>,
    /// pg_stat_statements rows hidden as `<insufficient privilege>`.
    pub hidden: usize,
}

impl Input {
    pub fn is_empty(&self) -> bool {
        self.queries.is_empty() && self.statement_names.is_empty()
    }
}

/// Scenarios that exercise each rule. `session-set` is refined by parameter below.
fn scenarios_for(rule: &str) -> Vec<&'static str> {
    match rule {
        // Each state rule: other clients mustn't see it (isolated), and its own client must
        // keep it across its transactions (survives-own-transactions) and when another
        // client holds the server connection it was made on (survives-server-change).
        "session-advisory-lock" => vec![
            "advisory-lock-released-on-disconnect",
            "advisory-lock-survives-own-transactions",
        ],
        "session-set-config" => vec![
            "empty-search-path-from-set-config",
            "set-isolated-between-clients",
            "set-survives-server-change",
        ],
        "session-set" => vec![
            "set-isolated-between-clients",
            "set-does-not-leak-after-disconnect",
            "set-survives-server-change",
        ],
        "listen" => vec!["listen-notify-delivered"],
        "temp-table" => vec![
            "temp-table-isolated-between-clients",
            "temp-table-survives-own-transactions",
            "temp-table-survives-server-change",
        ],
        "with-hold-cursor" => vec!["cursor-isolated-between-clients", "cursor-survives-own-transactions"],
        "sql-prepare" => vec![
            "sql-prepare-isolated-between-clients",
            "sql-prepare-survives-server-change",
        ],
        "discard" => vec!["discard-all-resets-settings"],
        // Each finding picks the ones with its query's shape (rules.rs).
        "multi-statement-transaction" => vec![
            "multi-statement-begin",
            "multi-statement-begin-variants",
            "multi-statement-begin-error",
            "multi-statement-begin-left-open",
            "multi-statement-commit-first",
            "multi-statement-begin-not-first",
            "multi-statement-error-aborts-transaction",
        ],
        "deallocate-all" => vec!["deallocate-all-isolated"],
        "load" => vec!["load-isolated-between-clients"],
        _ => vec![],
    }
}

/// Every rule with a one-line summary, for SARIF output.
pub const RULES: &[(&str, &str)] = &[
    ("session-advisory-lock", "Session-level advisory lock"),
    ("session-set-config", "set_config() scoped to the session"),
    ("session-set", "SET or RESET scoped to the session"),
    ("listen", "LISTEN needs a dedicated session"),
    ("temp-table", "Temp table that outlives the transaction"),
    ("with-hold-cursor", "WITH HOLD cursor"),
    ("sql-prepare", "SQL-level PREPARE"),
    ("discard", "DISCARD on a shared server connection"),
    ("deallocate-all", "DEALLOCATE ALL on a shared server connection"),
    ("load", "LOAD of a library into the session"),
    (
        "driver-prepared-statements",
        "A driver's named prepared statements, recognized by their names in the log",
    ),
    (
        "multi-statement-transaction",
        "Several statements in one query string, with BEGIN, COMMIT or ROLLBACK among them",
    ),
];

/// What `scan` found. `--format json` prints it as is.
#[derive(Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub queries: usize,
    /// Queries the SQL parser rejected.
    pub unparsed: usize,
    /// pg_stat_statements rows shown as `<insufficient privilege>`: other roles' queries,
    /// hidden from the role that exported them.
    pub hidden: usize,
    /// DO blocks and function bodies that could not be parsed (PL/pgSQL even after a
    /// retry), so only their words were checked.
    pub uninspected: usize,
    /// What scan could tell but isn't a finding, e.g. statements a pooler renamed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}
