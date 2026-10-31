use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::runner::RawMessage;
use serde::{Deserialize, Serialize};

/// Pooling mode of a target. Scenarios list the modes they apply to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum PoolMode {
    Session,
    Transaction,
    Statement,
}

impl std::fmt::Display for PoolMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            PoolMode::Session => "session",
            PoolMode::Transaction => "transaction",
            PoolMode::Statement => "statement",
        };
        f.write_str(name)
    }
}

/// A privilege a scenario's setup or steps need on Postgres. A target whose `direct`
/// user lacks one skips the scenario instead of failing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Requirement {
    /// Create, drop and grant roles.
    Createrole,
    Superuser,
    /// Cancel or terminate other users' sessions.
    PgSignalBackend,
    /// The pooler gives a second client no server connection while the first holds one
    /// in a transaction: a pool of one. Judged by a probe on the pooler, not a privilege.
    OneServerConnection,
    /// The pooler gives a second client its own server connection while the first holds
    /// one in a transaction.
    SeveralServerConnections,
}

impl Requirement {
    /// Whether the pooler's server connections decide it, rather than a privilege of the
    /// `direct` user.
    pub fn is_server(self) -> bool {
        matches!(
            self,
            Requirement::OneServerConnection | Requirement::SeveralServerConnections
        )
    }
}

impl std::fmt::Display for Requirement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Requirement::Createrole => "createrole",
            Requirement::Superuser => "superuser",
            Requirement::PgSignalBackend => "pg_signal_backend",
            Requirement::OneServerConnection => "one_server_connection",
            Requirement::SeveralServerConnections => "several_server_connections",
        })
    }
}

fn one() -> u32 {
    1
}

fn all_modes() -> Vec<PoolMode> {
    vec![PoolMode::Session, PoolMode::Transaction, PoolMode::Statement]
}

/// A scenario is a script of client actions. It runs once against plain Postgres
/// (every client gets its own connection) and once through the pooler. Any step
/// whose outcome differs is a failure.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub description: String,
    /// Links to the bug reports this scenario came from.
    #[serde(default)]
    pub refs: Vec<String>,
    #[serde(default = "all_modes")]
    pub modes: Vec<PoolMode>,
    /// Run the pooler side this many times and fail if any run fails. For scenarios
    /// whose outcome depends on timing inside the pooler, such as pipelines.
    #[serde(default = "one")]
    #[cfg_attr(test, schemars(range(min = 1)))]
    pub repeat: u32,
    /// Privileges the `direct` user needs; without them the scenario is skipped.
    #[serde(default)]
    pub requires: Vec<Requirement>,
    /// How long each step may take, overriding `run --step-timeout`.
    #[cfg_attr(test, schemars(range(min = 1)))]
    pub timeout_ms: Option<u64>,
    /// Statements run directly on Postgres before each run.
    #[serde(default)]
    pub setup: Vec<String>,
    /// Statements run directly on Postgres after each run.
    #[serde(default)]
    pub teardown: Vec<String>,
    #[serde(rename = "step")]
    pub steps: Vec<Step>,
    /// Set for `poolproof drivers` scenarios, never from a file: the steps run in a
    /// driver's own program instead of poolproof's clients.
    #[serde(skip)]
    pub program: Option<crate::drivers::Program>,
    /// Pairs of `evidence` steps (1-based) that must name the same backend for a run
    /// through the pooler to count: proof that the second client really reached the
    /// first one's server connection. A run where a pair differs doesn't count.
    #[serde(default)]
    pub same_backend: Vec<[usize; 2]>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Client name. A client connects on its first step.
    pub client: String,
    /// Connect straight to Postgres, bypassing the pooler. Used to check server state.
    #[serde(default)]
    pub direct: bool,
    /// Run SQL with the simple query protocol. May contain several statements.
    pub simple: Option<String>,
    /// With `simple`: drop the connection without Terminate if the query hasn't
    /// finished after this many ms, like a client that crashed or timed out.
    pub abandon_after_ms: Option<u64>,
    /// Prepare SQL with the extended protocol and store it under `name`.
    pub prepare: Option<String>,
    pub name: Option<String>,
    /// Execute a statement this client prepared earlier.
    pub execute: Option<String>,
    /// Close the client connection.
    #[serde(default)]
    pub disconnect: bool,
    /// With `simple`: start the query and move on. A later `wait` step collects the result.
    #[serde(default)]
    pub background: bool,
    /// Wait for this client's background query and record its result.
    #[serde(default)]
    pub wait: bool,
    /// Send a CancelRequest with this client's key, the way a driver would.
    #[serde(default)]
    pub cancel: bool,
    /// Record the NOTIFY messages this client has received since the last such step.
    #[serde(default)]
    pub notifications: bool,
    /// Write these protocol messages directly (e.g. `parse s1 SELECT 1`, `bind - s1`,
    /// `execute -`, `sync`) and record every reply. A client that uses `send` talks
    /// raw protocol for its whole life; `disconnect` is its only other step.
    pub send: Option<Vec<String>>,
    /// With `send`: record ParameterStatus replies too and compare them, ignoring where
    /// in the reply they come. For poolers that drop or fake them, e.g. after a SET.
    #[serde(default)]
    pub compare_parameter_status: bool,
    /// Sleep this long before the step, e.g. so a cancel lands while a query runs.
    pub delay_ms: Option<u64>,
    /// With `simple` on a pooled client: a query returning one boolean that says whether
    /// this run tests what the scenario means to, e.g. that this client got a given
    /// backend. Not run on Postgres and never compared. Through the pooler, false stops
    /// the run as a SKIP; anything but true or false is a FAIL.
    #[serde(default)]
    pub precondition: bool,
    /// Record which backend this pooled client is on: poolproof asks the pooler for
    /// `pg_backend_pid()` and reads the backend's start time on direct. Not run on
    /// Postgres and never compared; `same_backend` checks it.
    #[serde(default)]
    pub evidence: bool,
    #[serde(default)]
    pub compare: Compare,
    /// How a driver step describes itself. Never from a file: driver steps have no
    /// action of poolproof's own.
    #[serde(skip)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Compare {
    /// Rows, row counts and SQLSTATE must match.
    #[default]
    Full,
    /// Only success vs. error (and the SQLSTATE) must match.
    Status,
    /// Run the step but don't compare it.
    Ignore,
}

pub enum Action<'a> {
    Send(Vec<RawMessage>),
    Simple(&'a str),
    Abandon { sql: &'a str, after: Duration },
    Prepare { sql: &'a str, name: &'a str },
    Execute(&'a str),
    Disconnect,
    Background(&'a str),
    Wait,
    Cancel,
    Notifications,
    Evidence,
}

impl Step {
    /// A step that a driver program runs (see `crate::drivers`).
    pub(crate) fn labeled(client: &str, label: String, compare: Compare) -> Self {
        Step {
            client: client.to_string(),
            direct: false,
            simple: None,
            abandon_after_ms: None,
            prepare: None,
            name: None,
            execute: None,
            disconnect: false,
            background: false,
            wait: false,
            cancel: false,
            notifications: false,
            send: None,
            compare_parameter_status: false,
            delay_ms: None,
            precondition: false,
            evidence: false,
            compare,
            label: Some(label),
        }
    }

    pub fn action(&self) -> Result<Action<'_>> {
        let set = [
            self.simple.is_some(),
            self.prepare.is_some(),
            self.execute.is_some(),
            self.disconnect,
            self.wait,
            self.cancel,
            self.notifications,
            self.send.is_some(),
            self.evidence,
        ]
        .iter()
        .filter(|s| **s)
        .count();
        if set != 1 {
            bail!(
                "step for client '{}' must have exactly one of simple, prepare, execute, disconnect, wait, cancel, notifications, send, evidence",
                self.client
            );
        }
        if let Some(sql) = &self.simple {
            return Ok(match (self.abandon_after_ms, self.background) {
                (Some(_), true) => bail!(
                    "step for client '{}' can't be both abandoned and background",
                    self.client
                ),
                (Some(ms), false) => Action::Abandon {
                    sql,
                    after: Duration::from_millis(ms),
                },
                (None, true) => Action::Background(sql),
                (None, false) => Action::Simple(sql),
            });
        }
        if self.abandon_after_ms.is_some() || self.background {
            bail!(
                "abandon_after_ms and background for client '{}' only work with simple",
                self.client
            );
        }
        if let Some(sql) = &self.prepare {
            let Some(name) = &self.name else {
                bail!("prepare step for client '{}' needs a name", self.client);
            };
            return Ok(Action::Prepare { sql, name });
        }
        if let Some(name) = &self.execute {
            return Ok(Action::Execute(name));
        }
        if let Some(messages) = &self.send {
            let messages = messages
                .iter()
                .map(|m| {
                    m.parse()
                        .with_context(|| format!("in send for client '{}': `{m}`", self.client))
                })
                .collect::<Result<Vec<RawMessage>>>()?;
            return Ok(Action::Send(messages));
        }
        Ok(if self.wait {
            Action::Wait
        } else if self.cancel {
            Action::Cancel
        } else if self.notifications {
            Action::Notifications
        } else if self.evidence {
            Action::Evidence
        } else {
            Action::Disconnect
        })
    }

    pub fn describe(&self) -> String {
        if let Some(label) = &self.label {
            return format!("[{}] {label}", self.client);
        }
        let via = if self.direct { " (direct)" } else { "" };
        let what = match self.action() {
            Ok(Action::Simple(sql)) => format!("simple: {sql}"),
            Ok(Action::Send(_)) => format!("send: {}", self.send.as_deref().unwrap_or_default().join(" / ")),
            Ok(Action::Abandon { sql, after }) => format!("abandon after {after:?}: {sql}"),
            Ok(Action::Prepare { sql, name }) => format!("prepare {name}: {sql}"),
            Ok(Action::Execute(name)) => format!("execute {name}"),
            Ok(Action::Disconnect) => "disconnect".to_string(),
            Ok(Action::Background(sql)) => format!("background: {sql}"),
            Ok(Action::Wait) => "wait".to_string(),
            Ok(Action::Cancel) => "cancel".to_string(),
            Ok(Action::Notifications) => "notifications".to_string(),
            Ok(Action::Evidence) => "evidence: backend".to_string(),
            Err(e) => e.to_string(),
        };
        let precondition = if self.precondition { "precondition " } else { "" };
        format!("[{}{via}] {precondition}{what}", self.client)
    }

    /// How the step is compared: a precondition or evidence step never is.
    pub fn compare_mode(&self) -> Compare {
        if self.precondition || self.evidence {
            Compare::Ignore
        } else {
            self.compare
        }
    }
}

impl Scenario {
    /// How each step is compared, in order.
    pub fn compare_modes(&self) -> Vec<Compare> {
        self.steps.iter().map(Step::compare_mode).collect()
    }

    /// The most server connections the pooler's clients hold at once, if each pooled
    /// client holds one from its first step until it disconnects or abandons a query (or
    /// the scenario ends). Direct clients don't count, and a cancel opens nothing: a
    /// CancelRequest takes no server connection.
    pub fn max_concurrent(&self) -> usize {
        let mut open: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut most = 0;
        for step in self.steps.iter().filter(|s| !s.direct) {
            let closes = step.disconnect || step.abandon_after_ms.is_some();
            if !step.cancel && !(step.disconnect && !open.contains(step.client.as_str())) {
                open.insert(&step.client);
            }
            most = most.max(open.len());
            if closes {
                open.remove(step.client.as_str());
            }
        }
        most
    }

    /// Every client connects either straight to Postgres or through the pooler, never
    /// both, so all of a client's steps must agree on `direct`.
    pub fn check_clients(&self) -> Result<()> {
        let mut direct = std::collections::HashMap::new();
        for step in &self.steps {
            if *direct.entry(step.client.as_str()).or_insert(step.direct) != step.direct {
                bail!("client '{}' mixes direct and pooled steps", step.client);
            }
        }
        Ok(())
    }
}

/// The scenarios in `scenarios/`, embedded at build time as (file name, contents).
const BUNDLED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/bundled.rs"));

fn parse(origin: &str, text: &str) -> Result<Scenario> {
    let scenario: Scenario = toml::from_str(text).with_context(|| format!("parsing {origin}"))?;
    if scenario.repeat == 0 {
        bail!("{origin}: repeat must be at least 1");
    }
    if let Some(step) = scenario
        .steps
        .iter()
        .find(|s| s.compare_parameter_status && s.send.is_none())
    {
        bail!(
            "{origin}: compare_parameter_status needs a send step (client '{}')",
            step.client
        );
    }
    if scenario.timeout_ms == Some(0) {
        bail!("{origin}: timeout_ms must be at least 1");
    }
    check_evidence(origin, &scenario)?;
    for (i, step) in scenario.steps.iter().enumerate().filter(|(_, s)| s.precondition) {
        let simple = matches!(
            step.action().with_context(|| format!("in {origin}"))?,
            Action::Simple(_)
        );
        if step.direct || !simple || i == 0 {
            bail!(
                "{origin}: the precondition at step {} must be a plain `simple` step of a pooled client, after the first step",
                i + 1
            );
        }
    }
    let mut prepared = std::collections::HashSet::new();
    for step in &scenario.steps {
        match step.action().with_context(|| format!("in {origin}"))? {
            Action::Prepare { name, .. } => {
                prepared.insert((step.client.as_str(), name));
            }
            // Otherwise the execute would fail the same way on both sides and pass.
            Action::Execute(name) if !prepared.contains(&(step.client.as_str(), name)) => {
                bail!(
                    "{origin}: client '{}' executes '{name}', which it never prepared on this connection",
                    step.client
                );
            }
            // The client's next step opens a new connection, without its statements.
            Action::Disconnect | Action::Abandon { .. } => prepared.retain(|(client, _)| *client != step.client),
            _ => {}
        }
    }
    scenario.check_clients().with_context(|| format!("in {origin}"))?;
    Ok(scenario)
}

/// Evidence steps are pooled and follow an earlier step of their client; a raw client's
/// only after a step that ended with `sync`, since a Query mid-exchange would change
/// the test. `same_backend` names pairs of them, in order.
fn check_evidence(origin: &str, scenario: &Scenario) -> Result<()> {
    for (i, step) in scenario.steps.iter().enumerate().filter(|(_, s)| s.evidence) {
        let n = i + 1;
        if step.direct {
            bail!("{origin}: the evidence at step {n} must be a pooled client's");
        }
        let Some(before) = scenario.steps[..i].iter().rev().find(|s| s.client == step.client) else {
            bail!(
                "{origin}: the evidence at step {n} is client '{}''s first step; it must follow one",
                step.client
            );
        };
        let raw = scenario
            .steps
            .iter()
            .any(|s| s.client == step.client && s.send.is_some());
        if raw && !before.evidence {
            let synced = before
                .send
                .as_ref()
                .and_then(|m| m.last())
                .is_some_and(|m| m.trim() == "sync");
            if !synced {
                bail!(
                    "{origin}: the evidence at step {n} must follow a send step of client '{}' that ends with sync",
                    step.client
                );
            }
        }
    }
    for &[i, j] in &scenario.same_backend {
        let is_evidence = |k: usize| k >= 1 && k <= scenario.steps.len() && scenario.steps[k - 1].evidence;
        if !(i < j && is_evidence(i) && is_evidence(j)) {
            bail!("{origin}: same_backend [{i}, {j}] must name two evidence steps, the earlier first");
        }
    }
    Ok(())
}

/// The scenarios built into the binary.
pub fn bundled() -> Result<Vec<Scenario>> {
    let parsed = BUNDLED
        .iter()
        .map(|(name, text)| Ok((name.to_string(), parse(name, text)?)));
    unique_names(parsed.collect::<Result<_>>()?)
}

pub fn load_dir(dir: &Path) -> Result<Vec<Scenario>> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading scenario dir {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();

    let mut scenarios = Vec::with_capacity(paths.len());
    for path in paths {
        let text = std::fs::read_to_string(&path)?;
        let origin = path.display().to_string();
        let scenario = parse(&origin, &text)?;
        scenarios.push((origin, scenario));
    }
    unique_names(scenarios)
}

/// Results, reports, deep links, `--filter` and the determinism cache all go by name,
/// so two files with the same name would mix up their results.
fn unique_names(scenarios: Vec<(String, Scenario)>) -> Result<Vec<Scenario>> {
    let mut seen = std::collections::HashMap::new();
    for (origin, s) in &scenarios {
        if let Some(first) = seen.insert(s.name.as_str(), origin.as_str()) {
            bail!("{origin}: scenario name '{}' is already used by {first}", s.name);
        }
    }
    Ok(scenarios.into_iter().map(|(_, s)| s).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_scenarios_match_dir() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");
        let names = |v: Vec<Scenario>| v.into_iter().map(|s| s.name).collect::<Vec<_>>();
        let bundled = names(bundled().unwrap());
        assert!(!bundled.is_empty());
        assert_eq!(bundled, names(load_dir(&dir).unwrap()));
    }

    #[test]
    fn role_scenarios_declare_createrole() {
        for (name, text) in BUNDLED {
            let scenario = parse(name, text).unwrap();
            let declared = scenario
                .requires
                .iter()
                .any(|r| matches!(r, Requirement::Createrole | Requirement::Superuser));
            let text = text.to_lowercase();
            let grants = text.contains("grant ") && text.contains(" to ");
            let roles = ["pp_role", "create role", "alter role", "drop role", "create user"]
                .iter()
                .any(|s| text.contains(s));
            if roles || grants {
                assert!(declared, "{name} manages roles but doesn't require createrole");
            }
        }
    }

    #[test]
    fn a_precondition_is_a_later_simple_step_of_a_pooled_client() {
        let steps = |pre: &str| {
            format!(
                "name = 'p'\ndescription = ''\n[[step]]\nclient = 'a'\nsimple = 'BEGIN'\n\
                 [[step]]\nclient = 'c'\n{pre}\n"
            )
        };
        assert!(parse("ok", &steps("precondition = true\nsimple = 'SELECT true'")).is_ok());
        for bad in [
            "precondition = true\ndirect = true\nsimple = 'SELECT true'",
            "precondition = true\nsimple = 'SELECT true'\nbackground = true",
            "precondition = true\nsimple = 'SELECT true'\nabandon_after_ms = 5",
            "precondition = true\nprepare = 'SELECT true'\nname = 's'",
        ] {
            let err = format!("{:#}", parse("bad", &steps(bad)).err().unwrap());
            assert!(err.contains("must be a plain `simple` step"), "{bad}: {err}");
        }
        let first =
            "name = 'p'\ndescription = ''\n[[step]]\nclient = 'a'\nprecondition = true\nsimple = 'SELECT true'\n";
        assert!(parse("first", first).is_err());
        // Never compared, and named in the step's description.
        let s = parse("ok", &steps("precondition = true\nsimple = 'SELECT true'")).unwrap();
        assert_eq!(s.compare_modes(), [Compare::Full, Compare::Ignore]);
        assert_eq!(s.steps[1].describe(), "[c] precondition simple: SELECT true");
    }

    #[test]
    fn evidence_follows_a_step_of_its_pooled_client() {
        let with = |steps: &str, pairs: &str| {
            parse(
                "e",
                &format!("name = 'e'\ndescription = ''\nsame_backend = [{pairs}]\n{steps}"),
            )
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
        };
        let step = |client: &str, what: &str| format!("[[step]]\nclient = '{client}'\n{what}\n");
        let q = "simple = 'SELECT 1'";
        let ok = [
            step("a", q),
            step("a", "evidence = true"),
            step("b", q),
            step("b", "evidence = true"),
        ]
        .concat();
        assert_eq!(with(&ok, "[2, 4]"), Ok(()));
        for (steps, wanted) in [
            (step("a", "evidence = true"), "first step"),
            (
                [
                    step("pg", &format!("direct = true\n{q}")),
                    step("pg", "direct = true\nevidence = true"),
                ]
                .concat(),
                "pooled",
            ),
            (
                [step("a", q), step("a", &format!("evidence = true\n{q}"))].concat(),
                "exactly one of",
            ),
            (
                [
                    step("r", "send = ['parse s1 SELECT 1', 'flush']"),
                    step("r", "evidence = true"),
                ]
                .concat(),
                "ends with sync",
            ),
        ] {
            let err = with(&steps, "").unwrap_err();
            assert!(err.contains(wanted), "{err}");
        }
        let raw = [
            step("r", "send = ['query SELECT 1', 'sync']"),
            step("r", "evidence = true"),
        ]
        .concat();
        assert_eq!(with(&raw, ""), Ok(()));
        for pairs in ["[4, 2]", "[1, 4]", "[2, 9]"] {
            assert!(
                with(&ok, pairs).unwrap_err().contains("must name two evidence steps"),
                "{pairs}"
            );
        }
        let s = parse("e", &format!("name = 'e'\ndescription = ''\n{ok}")).unwrap();
        assert_eq!(s.steps[1].describe(), "[a] evidence: backend");
        assert_eq!(s.compare_modes()[1], Compare::Ignore);
    }

    /// Validated on session PgBouncer 1.26.1 with a pool of 1: exactly these hang there
    /// (two clients hold a server connection at once), and every other scenario passes.
    #[test]
    fn scenarios_needing_two_session_connections() {
        let mut two: Vec<String> = bundled()
            .unwrap()
            .into_iter()
            .filter(|s| s.max_concurrent() > 1)
            .map(|s| s.name)
            .collect();
        two.sort();
        assert_eq!(
            two,
            [
                "cancel-from-idle-client-is-harmless",
                "cancel-under-saturated-pool",
                "cursor-isolated-between-clients",
                "deallocate-all-isolated",
                "empty-search-path-from-set-config",
                "listen-notify-delivered",
                "load-isolated-between-clients",
                "prepared-statement-survives-other-clients",
                "protocol-statement-names-isolated",
                "protocol-statement-reparsed-after-close",
                "protocol-unnamed-statement-survives-other-clients",
                "search-path-isolated-between-clients",
                "set-isolated-between-clients",
                "set-role-isolated-between-clients",
                "sql-prepare-isolated-between-clients",
                "temp-table-isolated-between-clients",
            ]
        );
    }

    #[test]
    fn connections_open_at_first_step_and_close_at_disconnect_or_abandon() {
        let s =
            |steps: &str| -> Scenario { toml::from_str(&format!("name = 'c'\ndescription = ''\n{steps}")).unwrap() };
        let step = |client: &str, what: &str| format!("[[step]]\nclient = '{client}'\n{what}\n");
        let q = "simple = 'SELECT 1'";
        // A direct observer and a cancel take no server connection.
        let observed = s(&[
            step("a", q),
            step("pg", &format!("direct = true\n{q}")),
            step("a", "cancel = true"),
        ]
        .concat());
        assert_eq!(observed.max_concurrent(), 1);
        // Reconnecting after a disconnect or an abandoned query opens a new interval.
        let handoff = s(&[
            step("a", q),
            step("a", "disconnect = true"),
            step("b", q),
            step("b", &format!("{q}\nabandon_after_ms = 10")),
            step("a", q),
        ]
        .concat());
        assert_eq!(handoff.max_concurrent(), 1);
        // A background query keeps its connection.
        let overlap = s(&[
            step("a", &format!("{q}\nbackground = true")),
            step("b", q),
            step("a", "wait = true"),
        ]
        .concat());
        assert_eq!(overlap.max_concurrent(), 2);
    }

    #[test]
    fn parses_requirements() {
        let s: Scenario = toml::from_str(
            "name = 'r'\ndescription = ''\nrequires = ['createrole', 'superuser', 'pg_signal_backend']\nstep = []\n",
        )
        .unwrap();
        assert_eq!(
            s.requires,
            [
                Requirement::Createrole,
                Requirement::Superuser,
                Requirement::PgSignalBackend
            ]
        );
        assert!(toml::from_str::<Scenario>("name = 'r'\ndescription = ''\nrequires = ['root']\nstep = []\n").is_err());
    }

    #[test]
    fn loading_rejects_a_client_on_both_sides() {
        let text = "name = 'm'\ndescription = ''\n[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n\
                    [[step]]\nclient = 'a'\ndirect = true\nsimple = 'SELECT 1'\n";
        let err = parse("m.toml", text).unwrap_err();
        assert!(
            format!("{err:#}").contains("client 'a' mixes direct and pooled steps"),
            "{err:#}"
        );
    }

    #[test]
    fn scenario_names_are_unique() {
        let dir = std::env::temp_dir().join(format!("pp-unique-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let one = "name = 'x'\ndescription = 't'\nstep = []\n";
        std::fs::write(dir.join("a.toml"), one).unwrap();
        std::fs::write(dir.join("b.toml"), one.replace("'x'", "'y'")).unwrap();
        assert_eq!(load_dir(&dir).unwrap().len(), 2);
        std::fs::write(dir.join("c.toml"), one).unwrap();
        let err = format!("{:#}", load_dir(&dir).unwrap_err());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(err.contains("c.toml: scenario name 'x' is already used by"), "{err}");
        assert!(err.contains("a.toml"), "{err}");
    }

    #[test]
    fn execute_needs_an_earlier_prepare_by_the_same_client() {
        let head = "name = 't'\ndescription = 't'\n";
        let prepare = |client: &str| format!("[[step]]\nclient = '{client}'\nprepare = 'SELECT 1'\nname = 'q'\n");
        let execute = "[[step]]\nclient = 'a'\nexecute = 'q'\n";
        assert!(parse("t", &format!("{head}{}{execute}", prepare("a"))).is_ok());
        let err = parse("t", &format!("{head}{execute}{}", prepare("a"))).unwrap_err();
        assert!(format!("{err:#}").contains("never prepared"), "{err:#}");
        assert!(parse("t", &format!("{head}{}{execute}", prepare("b"))).is_err());
        // A new connection after disconnect or abandon has no statements.
        let disconnect = "[[step]]\nclient = 'a'\ndisconnect = true\n";
        assert!(parse("t", &format!("{head}{}{disconnect}{execute}", prepare("a"))).is_err());
        let abandon = "[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\nabandon_after_ms = 100\n";
        assert!(parse("t", &format!("{head}{}{abandon}{execute}", prepare("a"))).is_err());
    }

    #[test]
    fn timeout_ms_must_be_positive() {
        let text = |ms: u64| format!("name = 't'\ndescription = 't'\ntimeout_ms = {ms}\nstep = []\n");
        assert_eq!(parse("t", &text(1500)).unwrap().timeout_ms, Some(1500));
        assert!(parse("t", &text(0)).is_err());
    }

    #[test]
    fn compare_parameter_status_needs_send() {
        let head = "name = 't'\ndescription = 't'\n[[step]]\nclient = 'a'\ncompare_parameter_status = true\n";
        let send = parse("t", &format!("{head}send = ['query SET application_name = 1']\n")).unwrap();
        assert!(send.steps[0].compare_parameter_status);
        assert!(parse("t", &format!("{head}simple = 'SELECT 1'\n")).is_err());
    }

    #[test]
    fn step_needs_exactly_one_action() {
        let step: Step = toml::from_str("client = \"a\"\nsimple = \"SELECT 1\"\ndisconnect = true").unwrap();
        assert!(step.action().is_err());
    }

    #[test]
    fn abandon_needs_simple() {
        let step: Step = toml::from_str("client = \"a\"\nexecute = \"q\"\nabandon_after_ms = 100").unwrap();
        assert!(step.action().is_err());
        let step: Step = toml::from_str("client = \"a\"\nsimple = \"SELECT 1\"\nabandon_after_ms = 100").unwrap();
        assert!(matches!(step.action(), Ok(Action::Abandon { .. })));
    }

    #[test]
    fn background_needs_simple() {
        let step: Step = toml::from_str("client = \"a\"\nsimple = \"SELECT 1\"\nbackground = true").unwrap();
        assert!(matches!(step.action(), Ok(Action::Background(_))));
        let step: Step = toml::from_str("client = \"a\"\nwait = true\nbackground = true").unwrap();
        assert!(step.action().is_err());
        let step: Step = toml::from_str("client = \"a\"\nwait = true\ncancel = true").unwrap();
        assert!(step.action().is_err());
    }
}
