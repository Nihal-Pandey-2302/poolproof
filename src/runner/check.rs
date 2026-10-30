use anyhow::{Context, Result};

use super::Outcome;
use super::compare::{FailKind, Mismatch, Verdict};
use super::pg::{connect, wait_ready};
use super::steps::run_steps;
use crate::scenario::Scenario;

async fn run_direct(url: &str, statements: &[String]) -> Result<()> {
    if statements.is_empty() {
        return Ok(());
    }
    let (client, driver, _) = connect(url).await?;
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
    run_direct(direct, &scenario.teardown)
        .await
        .context("teardown (baseline)")?;
    let expected = expected?;

    run_direct(direct, reset).await.context("reset (target)")?;
    if !reset.is_empty()
        && let Err(e) = wait_ready(pooled).await
    {
        return Ok(Checked {
            verdict: Verdict::Error {
                reason: format!("{e:#}"),
            },
            steps: vec![],
            runs: 0,
            failures: 0,
        });
    }

    let mut kept: Option<Checked> = None;
    let mut failures = 0;
    for run in 1..=scenario.repeat {
        let checked = if run == 1 {
            check_target(scenario, pooled, direct, &expected).await?
        } else {
            run_direct(direct, reset).await.context("reset (target)")?;
            match (reset.is_empty(), wait_ready(pooled).await) {
                (false, Err(e)) => Checked {
                    verdict: Verdict::Error {
                        reason: format!("{e:#}"),
                    },
                    steps: vec![],
                    runs: 1,
                    failures: 0,
                },
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
    run_direct(direct, &scenario.teardown)
        .await
        .context("teardown (target)")?;
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
    Ok(Checked {
        verdict,
        steps: expected.iter().cloned().zip(actual).collect(),
        runs: 1,
        failures: 0,
    })
}
