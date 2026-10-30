use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::runner::raw::RawMessage;
use serde::{Deserialize, Serialize};

/// Pooling mode of a target. Scenarios list the modes they apply to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
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
    pub repeat: u32,
    /// Statements run directly on Postgres before each run.
    #[serde(default)]
    pub setup: Vec<String>,
    /// Statements run directly on Postgres after each run.
    #[serde(default)]
    pub teardown: Vec<String>,
    #[serde(rename = "step")]
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
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
    /// Sleep this long before the step, e.g. so a cancel lands while a query runs.
    pub delay_ms: Option<u64>,
    #[serde(default)]
    pub compare: Compare,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
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
}

impl Step {
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
        ]
        .iter()
        .filter(|s| **s)
        .count();
        if set != 1 {
            bail!(
                "step for client '{}' must have exactly one of simple, prepare, execute, disconnect, wait, cancel, notifications, send",
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
        } else {
            Action::Disconnect
        })
    }

    pub fn describe(&self) -> String {
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
            Err(e) => e.to_string(),
        };
        format!("[{}{via}] {what}", self.client)
    }
}

impl Scenario {
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
    let mut prepared = std::collections::HashSet::new();
    for step in &scenario.steps {
        match step.action().with_context(|| format!("in {origin}"))? {
            Action::Prepare { name, .. } => {
                prepared.insert((step.client.as_str(), name));
            }
            // Otherwise the execute would fail the same way on both sides and pass.
            Action::Execute(name) if !prepared.contains(&(step.client.as_str(), name)) => {
                bail!(
                    "{origin}: client '{}' executes '{name}', which it never prepared before",
                    step.client
                );
            }
            _ => {}
        }
    }
    scenario.check_clients().with_context(|| format!("in {origin}"))?;
    Ok(scenario)
}

/// The scenarios built into the binary.
pub fn bundled() -> Result<Vec<Scenario>> {
    BUNDLED.iter().map(|(name, text)| parse(name, text)).collect()
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
        scenarios.push(parse(&path.display().to_string(), &text)?);
    }
    Ok(scenarios)
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
    fn execute_needs_an_earlier_prepare_by_the_same_client() {
        let head = "name = 't'\ndescription = 't'\n";
        let prepare = |client: &str| format!("[[step]]\nclient = '{client}'\nprepare = 'SELECT 1'\nname = 'q'\n");
        let execute = "[[step]]\nclient = 'a'\nexecute = 'q'\n";
        assert!(parse("t", &format!("{head}{}{execute}", prepare("a"))).is_ok());
        let err = parse("t", &format!("{head}{execute}{}", prepare("a"))).unwrap_err();
        assert!(format!("{err:#}").contains("never prepared"), "{err:#}");
        assert!(parse("t", &format!("{head}{}{execute}", prepare("b"))).is_err());
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
