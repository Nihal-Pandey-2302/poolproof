use anyhow::{Context, Result};
use tokio::time::timeout;

use super::compare::{FailKind, Mismatch, Verdict, baseline_failure};
use super::pg::{connect, wait_ready};
use super::steps::run_steps;
use super::{Outcome, Timeouts};
use crate::scenario::Scenario;

/// Start of the ERROR reason when a target's pooler URL doesn't answer before the run.
/// The self-test matches on it, so keep it stable.
pub const POOLER_UNREACHABLE: &str = "target unreachable: pooler:";
/// Same for the target's direct (plain Postgres) URL.
pub const DIRECT_UNREACHABLE: &str = "target unreachable: direct:";

async fn run_direct(url: &str, statements: &[String], t: &Timeouts) -> Result<()> {
    if statements.is_empty() {
        return Ok(());
    }
    let (client, driver, _) = connect(url, t).await?;
    for sql in statements {
        client
            .batch_execute(sql)
            .await
            .with_context(|| format!("running `{sql}`"))?;
    }
    drop(client);
    let _ = driver.await;
    Ok(())
}

/// Connect and run `SELECT 1` once.
async fn probe(url: &str, t: &Timeouts) -> Result<()> {
    let (client, driver, _) = connect(url, t).await?;
    let res = timeout(t.step, client.simple_query("SELECT 1")).await;
    drop(client);
    let _ = timeout(t.step, driver).await;
    res.context("SELECT 1 timed out")??;
    Ok(())
}

/// Check that a target's direct and pooler URLs both answer, before running any
/// scenario on it. The error is the ERROR reason for each of its scenarios.
pub async fn preflight(pooled: &str, direct: &str, t: &Timeouts) -> Result<(), String> {
    probe(direct, t)
        .await
        .map_err(|e| format!("{DIRECT_UNREACHABLE} {e:#}"))?;
    probe(pooled, t)
        .await
        .map_err(|e| format!("{POOLER_UNREACHABLE} {e:#}"))
}

/// The verdict plus each step's (postgres, pooler) outcomes. `steps` is empty when the
/// pooler run never happened. With `repeat`, the first failing run is the one kept.
pub struct Checked {
    pub verdict: Verdict,
    pub steps: Vec<(Outcome, Outcome)>,
    pub runs: u32,
    pub failures: u32,
}

impl Checked {
    fn error(reason: String) -> Self {
        Checked {
            verdict: Verdict::Error { reason },
            steps: vec![],
            runs: 1,
            failures: 0,
        }
    }
}

/// One run of a scenario's steps with reset, setup and teardown around it. `side` is
/// "baseline" or "target" and names the run in errors. Teardown runs even when setup
/// or a step fails; the first error is the one returned.
async fn run_once(
    scenario: &Scenario,
    pooled: &str,
    direct: &str,
    reset: &[String],
    side: &str,
    t: &Timeouts,
) -> Result<Vec<Outcome>, String> {
    run_direct(direct, reset, t)
        .await
        .map_err(|e| format!("reset ({side}): {e:#}"))?;
    // A reset kills the pooler's server connections, and it needs a moment to open new
    // ones. Without a reset there's nothing to wait for.
    if side == "target"
        && !reset.is_empty()
        && let Err(e) = wait_ready(pooled, t).await
    {
        return Err(format!("{e:#}"));
    }
    let outcomes = match run_direct(direct, &scenario.setup, t).await {
        Ok(()) => run_steps(scenario, pooled, direct, t)
            .await
            .map_err(|e| format!("steps ({side}): {e:#}")),
        Err(e) => Err(format!("setup ({side}): {e:#}")),
    };
    let teardown = run_direct(direct, &scenario.teardown, t)
        .await
        .map_err(|e| format!("teardown ({side}): {e:#}"));
    let outcomes = outcomes?;
    teardown?;
    Ok(outcomes)
}

/// Run a scenario against plain Postgres, then through the pooler, and compare.
/// `reset` runs on `direct` before each run so state leaked on the pooler's server
/// connections by an earlier scenario can't change this one's result. Anything that
/// stops the comparison is a `Verdict::Error` for this scenario only.
pub async fn check(scenario: &Scenario, pooled: &str, direct: &str, reset: &[String], t: &Timeouts) -> Checked {
    let expected = match run_once(scenario, direct, direct, reset, "baseline", t).await {
        Ok(expected) => expected,
        Err(reason) => return Checked::error(reason),
    };
    if let Some(reason) = baseline_failure(&expected) {
        return Checked::error(reason);
    }

    let mut kept: Option<Checked> = None;
    let mut failures = 0;
    for _ in 0..scenario.repeat {
        let checked = match run_once(scenario, pooled, direct, reset, "target", t).await {
            Ok(actual) => compare(scenario, &expected, actual),
            Err(reason) => Checked::error(reason),
        };
        let failed = !matches!(checked.verdict, Verdict::Pass);
        failures += u32::from(failed);
        // Keep the first failure; otherwise the latest pass.
        if kept.as_ref().is_none_or(|k| matches!(k.verdict, Verdict::Pass)) {
            kept = Some(checked);
        }
    }
    // Loading rejects repeat = 0, so the loop ran at least once.
    let mut checked = kept.unwrap_or_else(|| Checked::error("repeat is 0".into()));
    checked.runs = scenario.repeat;
    checked.failures = failures;
    checked
}

/// Compare one pooler-side run with the baseline outcomes.
fn compare(scenario: &Scenario, expected: &[Outcome], actual: Vec<Outcome>) -> Checked {
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
    Checked {
        verdict,
        steps: expected.iter().cloned().zip(actual).collect(),
        runs: 1,
        failures: 0,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    const CLOSED_PORT: &str = "postgres://x:y@127.0.0.1:1/app";

    fn scenario(toml_text: &str) -> Scenario {
        toml::from_str(toml_text).unwrap()
    }

    fn reason(checked: &Checked) -> &str {
        match &checked.verdict {
            Verdict::Error { reason } => reason,
            _ => panic!("expected an error verdict"),
        }
    }

    #[tokio::test]
    async fn unreachable_postgres_is_an_error_not_a_pass() {
        let s = scenario("name = 't'\ndescription = 't'\n[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n");
        let checked = check(&s, CLOSED_PORT, CLOSED_PORT, &[], &Timeouts::default()).await;
        let reason = reason(&checked);
        assert!(
            reason.starts_with("baseline failed at step 1: error CONNECT"),
            "{reason}"
        );
    }

    #[tokio::test]
    async fn failing_setup_is_a_scenario_error() {
        let s = scenario(
            "name = 't'\ndescription = 't'\nsetup = ['SELECT 1']\n[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n",
        );
        let checked = check(&s, CLOSED_PORT, CLOSED_PORT, &[], &Timeouts::default()).await;
        let reason = reason(&checked);
        assert!(reason.starts_with("setup (baseline): "), "{reason}");
    }

    #[tokio::test]
    async fn repeats_without_reset_do_not_wait_for_the_pooler() {
        // No steps and no setup, so nothing connects except a needless wait_ready.
        let mut s = scenario("name = 't'\ndescription = 't'\nrepeat = 3\nstep = []\n");
        s.steps.clear();
        let started = Instant::now();
        let checked = check(&s, CLOSED_PORT, CLOSED_PORT, &[], &Timeouts::default()).await;
        assert!(matches!(checked.verdict, Verdict::Pass));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn preflight_names_the_unreachable_side() {
        let reason = preflight(CLOSED_PORT, CLOSED_PORT, &Timeouts::default())
            .await
            .unwrap_err();
        assert!(reason.starts_with(DIRECT_UNREACHABLE), "{reason}");
    }
}
