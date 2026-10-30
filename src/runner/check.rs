use std::time::Duration;

use anyhow::{Context, Result};
use tokio::time::{Instant, timeout};

use super::compare::{FailKind, Mismatch, Verdict, baseline_failure};
use super::pg::{connect, wait_ready};
use super::steps::run_steps;
use super::{Outcome, Timeouts};
use crate::scenario::Scenario;
use crate::targets::{Endpoint, Statement};

/// Start of the ERROR reason when a target's pooler URL doesn't answer before the run.
/// The self-test matches on it, so keep it stable.
pub const POOLER_UNREACHABLE: &str = "target unreachable: pooler:";
/// Same for the target's direct (plain Postgres) URL.
pub const DIRECT_UNREACHABLE: &str = "target unreachable: direct:";

async fn run_direct(direct: &Endpoint, statements: &[Statement], t: &Timeouts) -> Result<()> {
    if statements.is_empty() {
        return Ok(());
    }
    let (client, driver, _) = connect(direct, t).await?;
    for s in statements {
        if let Err(e) = client.batch_execute(s.sql()).await {
            // The statement is shown as written. The server's message can quote the
            // executed text, so with substituted values only its SQLSTATE is kept.
            return Err(if s.has_substitutions() {
                let code = e.code().map_or("an error without SQLSTATE", |c| c.code());
                anyhow::anyhow!("running `{s}`: {code} (details hidden: the statement has substituted values)")
            } else {
                anyhow::Error::from(e).context(format!("running `{s}`"))
            });
        }
    }
    drop(client);
    let _ = driver.await;
    Ok(())
}

/// Connect and run `SELECT 1` once.
async fn probe(endpoint: &Endpoint, t: &Timeouts) -> Result<()> {
    let (client, driver, _) = connect(endpoint, t).await?;
    let res = timeout(t.step, client.simple_query("SELECT 1")).await;
    drop(client);
    let _ = timeout(t.step, driver).await;
    res.context("SELECT 1 timed out")??;
    Ok(())
}

/// Probe until it works or `within` has passed.
async fn probe_until(endpoint: &Endpoint, within: Duration, t: &Timeouts) -> Result<()> {
    let deadline = Instant::now() + within;
    loop {
        match probe(endpoint, t).await {
            Ok(()) => return Ok(()),
            Err(e) if Instant::now() >= deadline => return Err(e.context(format!("no answer within {within:?}"))),
            Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    }
}

/// Check that a target's direct and pooler URLs both answer, before running any
/// scenario on it. The error is the ERROR reason for each of its scenarios. A pooler
/// gets a few tries: it may still be starting, or reconnecting after an earlier reset.
pub async fn preflight(pooled: &Endpoint, direct: &Endpoint, t: &Timeouts) -> Result<(), String> {
    probe(direct, t)
        .await
        .map_err(|e| format!("{DIRECT_UNREACHABLE} {e:#}"))?;
    probe_until(pooled, t.ready, t)
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
    /// An ERROR before anything was compared, so `runs` is 0.
    pub(super) fn error(reason: String) -> Self {
        Checked {
            verdict: Verdict::Error { reason },
            steps: vec![],
            runs: 0,
            failures: 0,
        }
    }
}

/// Which run of a scenario: plain Postgres, or through the pooler.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Baseline,
    Target,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Side::Baseline => "baseline",
            Side::Target => "target",
        })
    }
}

/// One run of a scenario's steps with reset, setup and teardown around it. Teardown
/// runs even when setup or a step fails; the first error is the one returned.
async fn run_once(
    scenario: &Scenario,
    pooled: &Endpoint,
    direct: &Endpoint,
    reset: &[Statement],
    side: Side,
    t: &Timeouts,
) -> Result<Vec<Outcome>, String> {
    run_direct(direct, reset, t)
        .await
        .map_err(|e| format!("reset ({side}): {e:#}"))?;
    // A reset kills the pooler's server connections, and it needs a moment to open new
    // ones. Without a reset there's nothing to wait for.
    if side == Side::Target
        && !reset.is_empty()
        && let Err(e) = wait_ready(pooled, t).await
    {
        return Err(format!("{e:#}"));
    }
    let outcomes = match run_direct(direct, &Statement::plain_all(&scenario.setup), t).await {
        Ok(()) => run_steps(scenario, pooled, direct, t)
            .await
            .map_err(|e| format!("steps ({side}): {e:#}")),
        Err(e) => Err(format!("setup ({side}): {e:#}")),
    };
    let teardown = run_direct(direct, &Statement::plain_all(&scenario.teardown), t)
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
pub async fn check(
    scenario: &Scenario,
    pooled: &Endpoint,
    direct: &Endpoint,
    reset: &[Statement],
    t: &Timeouts,
) -> Checked {
    let expected = match run_once(scenario, direct, direct, reset, Side::Baseline, t).await {
        Ok(expected) => expected,
        Err(reason) => return Checked::error(reason),
    };
    if let Some(reason) = baseline_failure(&expected) {
        return Checked::error(reason);
    }

    let mut kept: Option<Checked> = None;
    let mut failures = 0;
    for _ in 0..scenario.repeat {
        let checked = match run_once(scenario, pooled, direct, reset, Side::Target, t).await {
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
    // A direct step that couldn't reach Postgres says nothing about the pooler. A
    // TIMEOUT stays a failure: a lock the pooler leaked can block a direct observer.
    let direct_failed = scenario.steps.iter().zip(&actual).position(|(step, outcome)| {
        step.direct && matches!(outcome, Outcome::Error { code, .. } if code == "CONNECT" || code == "CLIENT")
    });
    if let Some(i) = direct_failed {
        return Checked {
            verdict: Verdict::Error {
                reason: format!("postgres side failed at step {} (target run): {}", i + 1, actual[i]),
            },
            steps: expected.iter().cloned().zip(actual).collect(),
            runs: 1,
            failures: 0,
        };
    }
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

    fn closed_port() -> Endpoint {
        Endpoint::parse("postgres://x:y@127.0.0.1:1/app").unwrap()
    }

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
        let checked = check(&s, &closed_port(), &closed_port(), &[], &Timeouts::default()).await;
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
        let checked = check(&s, &closed_port(), &closed_port(), &[], &Timeouts::default()).await;
        let reason = reason(&checked);
        assert!(reason.starts_with("setup (baseline): "), "{reason}");
    }

    #[tokio::test]
    async fn repeats_without_reset_do_not_wait_for_the_pooler() {
        // No steps and no setup, so nothing connects except a needless wait_ready.
        let mut s = scenario("name = 't'\ndescription = 't'\nrepeat = 3\nstep = []\n");
        s.steps.clear();
        let started = Instant::now();
        let checked = check(&s, &closed_port(), &closed_port(), &[], &Timeouts::default()).await;
        assert!(matches!(checked.verdict, Verdict::Pass));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn direct_step_that_fails_to_connect_on_the_target_run_is_an_error() {
        let s =
            scenario("name = 't'\ndescription = 't'\n[[step]]\nclient = 'pg'\ndirect = true\nsimple = 'SELECT 1'\n");
        let expected = [Outcome::empty()];
        for code in ["CONNECT", "CLIENT"] {
            let checked = compare(&s, &expected, vec![Outcome::error(code, "refused")]);
            let reason = reason(&checked);
            assert!(
                reason.starts_with("postgres side failed at step 1 (target run)"),
                "{reason}"
            );
        }
        // A direct observer blocked by a lock the pooler leaked is the pooler's fault.
        let checked = compare(&s, &expected, vec![Outcome::error("TIMEOUT", "no reply")]);
        assert!(matches!(checked.verdict, Verdict::Fail { .. }));
    }

    /// Accepts on a local port, but only forwards to `to` after `delay`, like a pooler
    /// that is still starting. Connections before that are refused by closing them.
    async fn late_forwarder(to: String, delay: Duration) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let ready = Instant::now() + delay;
        let pg = to.parse::<tokio_postgres::Config>().unwrap();
        let tokio_postgres::config::Host::Tcp(host) = &pg.get_hosts()[0] else {
            panic!("tcp host")
        };
        let upstream = format!("{host}:{}", pg.get_ports()[0]);
        tokio::spawn(async move {
            while let Ok((mut client, _)) = listener.accept().await {
                if Instant::now() < ready {
                    continue;
                }
                let upstream = upstream.clone();
                tokio::spawn(async move {
                    let mut server = tokio::net::TcpStream::connect(upstream).await.unwrap();
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                });
            }
        });
        to.replace(&format!(":{}/", pg.get_ports()[0]), &format!(":{port}/"))
    }

    /// A Postgres URL for tests that need a real server, e.g. a throwaway container.
    fn test_pg() -> String {
        std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL")
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn preflight_waits_for_a_pooler_that_is_still_starting() {
        let url = test_pg();
        let pooled = late_forwarder(url.clone(), Duration::from_secs(2)).await;
        preflight(
            &Endpoint::parse(&pooled).unwrap(),
            &Endpoint::parse(&url).unwrap(),
            &Timeouts::default(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn preflight_names_the_unreachable_side() {
        let reason = preflight(&closed_port(), &closed_port(), &Timeouts::default())
            .await
            .unwrap_err();
        assert!(reason.starts_with(DIRECT_UNREACHABLE), "{reason}");
    }
}
