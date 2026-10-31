//! What a target's pooler does with a second client, probed before its scenarios: once
//! with the first client idle, once inside its open transaction. The results check the
//! declared mode, decide the server-connection requirements, and say whether a session
//! pool has room for a scenario's clients.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::Timeouts;
use super::check::run_direct;
use super::pg::{connect, wait_ready};
use crate::doctor::{Connected, Observed, probe};
use crate::scenario::{PoolMode, Requirement, Scenario};
use crate::targets::{Endpoint, Statement};

/// How long a probe's second client waits before the first one closes. On a pool of
/// one it always waits this long, twice (a wait is probed again, to rule out a pool
/// that was still opening its second server), so about 6 s per probe; it stays short
/// for that. A pooler slower than this to open a second server connection reads as
/// unknown, not as one connection.
pub const BUSY_WAIT: Duration = Duration::from_secs(3);

/// One probe's result, as reports record it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Probe {
    pub result: ProbeResult,
    /// What the pooler said when it refused the second client, or why the probe failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ProbeResult {
    /// The second client got the first one's backend.
    Shared,
    /// The second client got a backend of its own.
    Separate,
    /// The second client waited for the first one's server connection.
    Waited,
    /// The pooler refused the second client until the first one was gone.
    Refused,
    /// Not probed: statement mode can't hold a transaction, or the probe failed.
    Unknown,
}

impl Probe {
    fn unknown(detail: impl Into<String>) -> Self {
        Probe {
            result: ProbeResult::Unknown,
            detail: Some(detail.into()),
        }
    }

    fn of(observed: Observed) -> Self {
        let (result, detail) = match observed {
            Observed::Shared => (ProbeResult::Shared, None),
            Observed::Separate => (ProbeResult::Separate, None),
            Observed::Waited => (ProbeResult::Waited, None),
            Observed::Refused(refusal) => (ProbeResult::Refused, Some(refusal)),
            Observed::Unclear(why) => (ProbeResult::Unknown, Some(why)),
        };
        Probe { result, detail }
    }

    /// One server connection: the second client had to wait, or was turned away.
    pub fn one(&self) -> bool {
        matches!(self.result, ProbeResult::Waited | ProbeResult::Refused)
    }
}

/// Both probes of a target.
#[derive(Debug, Clone)]
pub struct Probes {
    pub idle: Probe,
    pub busy: Probe,
    /// The busy probe's first backend, for the error when a second client got it.
    busy_pid: String,
}

/// Runs the target's reset, waits for the pooler, then probes it. A probe that fails
/// is `unknown`, with why; it doesn't stop the target.
pub async fn probe_target(
    pooled: &Endpoint,
    direct: &Endpoint,
    reset: &[Statement],
    mode: PoolMode,
    t: &Timeouts,
) -> Result<Probes, String> {
    run_direct(direct, reset, t)
        .await
        .map_err(|e| format!("reset (probe): {e:#}"))?;
    if !reset.is_empty() {
        wait_ready(pooled, t).await.map_err(|e| format!("{e:#}"))?;
    }
    let connect = || async {
        let (client, driver, _) = connect(pooled, t).await?;
        Ok::<Connected, anyhow::Error>((client, driver))
    };
    // A second client that waits gets its late answer checked, so the first wait can
    // be short.
    let within = t.housekeeping.min(BUSY_WAIT);
    let idle = match probe(connect, within, t.housekeeping, false).await {
        Ok((observed, _)) => Probe::of(observed),
        Err(e) => Probe::unknown(format!("probe failed: {e:#}")),
    };
    let (busy, busy_pid) = if mode == PoolMode::Statement {
        (Probe::unknown("statement mode can't hold a transaction"), String::new())
    } else {
        match probe(connect, within, t.housekeeping, true).await {
            Ok((observed, pid)) => (Probe::of(observed), pid),
            Err(e) => (Probe::unknown(format!("probe failed: {e:#}")), String::new()),
        }
    };
    Ok(Probes { idle, busy, busy_pid })
}

impl Probes {
    /// Why the target can't be tested as declared, or None. Only a clear contradiction
    /// counts: a pool with room for both clients fits every mode.
    pub fn mode_error(&self, mode: PoolMode) -> Option<String> {
        if self.busy.result == ProbeResult::Shared {
            return Some(format!(
                "the pooler gave another client a server connection inside an open transaction (pid {})",
                self.busy_pid
            ));
        }
        let idle = &self.idle;
        let seen = match (mode, idle.result) {
            (PoolMode::Transaction | PoolMode::Statement, ProbeResult::Waited) => {
                "a second client waited for the first one's server connection".to_string()
            }
            (PoolMode::Transaction | PoolMode::Statement, ProbeResult::Refused) => format!(
                "the pooler refused a second client while the first was connected ({})",
                idle.detail.as_deref().unwrap_or("")
            ),
            (PoolMode::Session, ProbeResult::Shared) => {
                "a second client got the first one's server connection".to_string()
            }
            _ => return None,
        };
        Some(format!("declared {mode} mode, probe saw {seen}"))
    }

    /// Why a scenario can't run on this target, or None. `ignore_requirements` lets
    /// scenarios with unmet server-connection requirements run anyway (Postgres as its
    /// own pooler); it doesn't change how many connections a session pool has.
    pub fn unmet(&self, s: &Scenario, mode: PoolMode, ignore_requirements: bool) -> Option<String> {
        let busy = &self.busy;
        if !ignore_requirements {
            for r in s.requires.iter().filter(|r| r.is_server()) {
                let unknown = if mode == PoolMode::Statement {
                    "server connections unknown in statement mode"
                } else {
                    "server connections unknown (probe failed)"
                };
                let reason = match (r, busy.result) {
                    (_, ProbeResult::Unknown | ProbeResult::Shared) => unknown,
                    (Requirement::OneServerConnection, ProbeResult::Separate) => {
                        "needs one server connection (probe: several)"
                    }
                    (Requirement::SeveralServerConnections, _) if busy.one() => {
                        "needs several server connections (probe: one)"
                    }
                    _ => continue,
                };
                return Some(reason.to_string());
            }
        }
        let clients = s.max_concurrent();
        if mode != PoolMode::Session || clients < 2 {
            return None;
        }
        match busy.result {
            _ if busy.one() => Some(format!(
                "needs {clients} server connections at once (session pool of 1)"
            )),
            // A session pool of one would hang it; without knowing, don't run it.
            ProbeResult::Unknown => Some("server connections unknown (probe failed)".into()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probes(idle: ProbeResult, busy: ProbeResult) -> Probes {
        let p = |result| Probe { result, detail: None };
        Probes {
            idle: p(idle),
            busy: p(busy),
            busy_pid: "42".into(),
        }
    }

    #[test]
    fn only_a_clear_contradiction_is_a_mode_error() {
        use ProbeResult::*;
        // Postgres as its own pooler: every client its own backend.
        for mode in [PoolMode::Transaction, PoolMode::Statement, PoolMode::Session] {
            assert_eq!(probes(Separate, Separate).mode_error(mode), None);
        }
        assert_eq!(probes(Shared, Waited).mode_error(PoolMode::Transaction), None);
        assert_eq!(probes(Shared, Unknown).mode_error(PoolMode::Statement), None);
        assert_eq!(probes(Waited, Waited).mode_error(PoolMode::Session), None);
        assert_eq!(probes(Refused, Refused).mode_error(PoolMode::Session), None);
        assert_eq!(probes(Unknown, Unknown).mode_error(PoolMode::Transaction), None);
        assert_eq!(
            probes(Waited, Waited).mode_error(PoolMode::Transaction).unwrap(),
            "declared transaction mode, probe saw a second client waited for the first one's server connection"
        );
        let mut refused = probes(Refused, Refused);
        refused.idle.detail = Some("XX000: max clients reached".into());
        assert!(
            refused
                .mode_error(PoolMode::Statement)
                .unwrap()
                .ends_with("refused a second client while the first was connected (XX000: max clients reached)")
        );
        assert_eq!(
            probes(Shared, Waited).mode_error(PoolMode::Session).unwrap(),
            "declared session mode, probe saw a second client got the first one's server connection"
        );
        assert_eq!(
            probes(Shared, Shared).mode_error(PoolMode::Transaction).unwrap(),
            "the pooler gave another client a server connection inside an open transaction (pid 42)"
        );
    }

    fn scenario(requires: &str, steps: &str) -> Scenario {
        toml::from_str(&format!(
            "name = 's'\ndescription = ''\nrequires = [{requires}]\n{steps}"
        ))
        .unwrap()
    }

    const TWO_CLIENTS: &str =
        "[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n[[step]]\nclient = 'b'\nsimple = 'SELECT 1'\n";

    #[test]
    fn server_requirements_follow_the_busy_probe() {
        use ProbeResult::*;
        let one = scenario("'one_server_connection'", TWO_CLIENTS);
        let several = scenario("'several_server_connections'", TWO_CLIENTS);
        let tx = PoolMode::Transaction;
        assert_eq!(probes(Shared, Waited).unmet(&one, tx, false), None);
        assert_eq!(probes(Shared, Refused).unmet(&one, tx, false), None);
        assert_eq!(
            probes(Separate, Separate).unmet(&one, tx, false).unwrap(),
            "needs one server connection (probe: several)"
        );
        assert_eq!(probes(Separate, Separate).unmet(&several, tx, false), None);
        assert_eq!(
            probes(Shared, Waited).unmet(&several, tx, false).unwrap(),
            "needs several server connections (probe: one)"
        );
        // Statement mode has no busy probe.
        for s in [&one, &several] {
            assert_eq!(
                probes(Shared, Unknown).unmet(s, PoolMode::Statement, false).unwrap(),
                "server connections unknown in statement mode"
            );
        }
        assert_eq!(
            probes(Unknown, Unknown).unmet(&one, tx, false).unwrap(),
            "server connections unknown (probe failed)"
        );
        // Postgres as its own pooler runs them anyway.
        assert_eq!(probes(Separate, Separate).unmet(&one, tx, true), None);
        assert_eq!(probes(Shared, Unknown).unmet(&one, PoolMode::Statement, true), None);
    }

    #[test]
    fn a_session_pool_of_one_skips_scenarios_needing_two_connections() {
        use ProbeResult::*;
        let plain = scenario("", TWO_CLIENTS);
        let session = PoolMode::Session;
        assert_eq!(
            probes(Waited, Waited).unmet(&plain, session, false).unwrap(),
            "needs 2 server connections at once (session pool of 1)"
        );
        // Not even Postgres-as-pooler's opt-in changes the pool size.
        assert!(probes(Refused, Refused).unmet(&plain, session, true).is_some());
        assert_eq!(probes(Separate, Separate).unmet(&plain, session, false), None);
        assert_eq!(
            probes(Unknown, Unknown).unmet(&plain, session, false).unwrap(),
            "server connections unknown (probe failed)"
        );
        assert_eq!(probes(Shared, Waited).unmet(&plain, PoolMode::Transaction, false), None);
        let one_at_a_time = scenario(
            "",
            "[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n[[step]]\nclient = 'a'\ndisconnect = true\n\
             [[step]]\nclient = 'b'\nsimple = 'SELECT 1'\n",
        );
        assert_eq!(probes(Waited, Waited).unmet(&one_at_a_time, session, false), None);
    }
}
