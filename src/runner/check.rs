use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use tokio::time::{Instant, timeout};

use super::compare::{FailKind, Mismatch, Verdict, baseline_failure};
use super::pg::{connect, wait_ready};
use super::steps::run_steps;
use super::{Outcome, READY_TIMEOUT, STEP_TIMEOUT};
use crate::scenario::Scenario;

/// Start of the ERROR reason when a target's pooler URL doesn't answer before the run.
/// The self-test matches on it, so keep it stable.
pub const POOLER_UNREACHABLE: &str = "target unreachable: pooler:";
/// Same for the target's direct (plain Postgres) URL.
pub const DIRECT_UNREACHABLE: &str = "target unreachable: direct:";

/// Limits for setup, teardown and reset statements. A pooler can leak a lock (some
/// scenarios test exactly that), and a teardown blocked on it must fail, not hang the
/// run. The statement timeout stays well above reset's 5 s wait per terminated backend.
const DIRECT_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const DIRECT_STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
/// Extra time on top of the statement timeout before giving up on the server itself.
const DIRECT_GRACE: Duration = Duration::from_secs(5);

async fn run_direct(url: &str, statements: &[String]) -> Result<()> {
    if statements.is_empty() {
        return Ok(());
    }
    let (client, driver, _) = connect(url).await?;
    let limits = format!(
        "SET lock_timeout = '{}ms'; SET statement_timeout = '{}ms'",
        DIRECT_LOCK_TIMEOUT.as_millis(),
        DIRECT_STATEMENT_TIMEOUT.as_millis()
    );
    let mut res = execute(&client, &limits).await;
    for sql in statements {
        if res.is_err() {
            break;
        }
        res = execute(&client, sql).await;
    }
    // Close the session even after an error, so it's gone before the next phase
    // (e.g. a failed setup's session while teardown runs).
    drop(client);
    let _ = timeout(STEP_TIMEOUT, driver).await;
    res
}

async fn execute(client: &tokio_postgres::Client, sql: &str) -> Result<()> {
    let limit = DIRECT_STATEMENT_TIMEOUT + DIRECT_GRACE;
    match timeout(limit, client.batch_execute(sql)).await {
        Ok(res) => res.with_context(|| format!("running `{sql}`")),
        Err(_) => Err(anyhow!("running `{sql}`: no reply within {limit:?}")),
    }
}

/// Connect and run `SELECT 1` once.
async fn probe(url: &str) -> Result<()> {
    let (client, driver, _) = connect(url).await?;
    let res = timeout(STEP_TIMEOUT, client.simple_query("SELECT 1")).await;
    drop(client);
    let _ = timeout(STEP_TIMEOUT, driver).await;
    res.context("SELECT 1 timed out")??;
    Ok(())
}

/// Probe until it works or `within` has passed.
async fn probe_until(url: &str, within: Duration) -> Result<()> {
    let deadline = Instant::now() + within;
    loop {
        match probe(url).await {
            Ok(()) => return Ok(()),
            Err(e) if Instant::now() >= deadline => return Err(e.context(format!("no answer within {within:?}"))),
            Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    }
}

/// Check that a target's direct and pooler URLs both answer, before running any
/// scenario on it. The error is the ERROR reason for each of its scenarios. A pooler
/// gets a few tries: it may still be starting, or reconnecting after an earlier reset.
pub async fn preflight(pooled: &str, direct: &str) -> Result<(), String> {
    probe(direct).await.map_err(|e| format!("{DIRECT_UNREACHABLE} {e:#}"))?;
    probe_until(pooled, READY_TIMEOUT)
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

/// What one run produced. Teardown is kept apart from the steps: on the target run,
/// a teardown that fails (say, on a lock the pooler leaked) mustn't hide a FAIL.
struct Run {
    outcomes: Result<Vec<Outcome>, String>,
    teardown: Result<(), String>,
}

impl Run {
    /// The outcomes, or the first error of the run, teardown included.
    fn all(self) -> Result<Vec<Outcome>, String> {
        let outcomes = self.outcomes?;
        self.teardown?;
        Ok(outcomes)
    }
}

/// One run of a scenario's steps with reset, setup and teardown around it. Teardown
/// runs even when setup or a step fails.
async fn run_once(scenario: &Scenario, pooled: &str, direct: &str, reset: &[String], side: Side) -> Run {
    let failed = |reason| Run {
        outcomes: Err(reason),
        teardown: Ok(()),
    };
    if let Err(e) = run_direct(direct, reset).await {
        return failed(format!("reset ({side}): {e:#}"));
    }
    // A reset kills the pooler's server connections, and it needs a moment to open new
    // ones. Without a reset there's nothing to wait for.
    if side == Side::Target
        && !reset.is_empty()
        && let Err(e) = wait_ready(pooled).await
    {
        return failed(format!("{e:#}"));
    }
    let outcomes = match run_direct(direct, &scenario.setup).await {
        Ok(()) => run_steps(scenario, pooled, direct)
            .await
            .map_err(|e| format!("steps ({side}): {e:#}")),
        Err(e) => Err(format!("setup ({side}): {e:#}")),
    };
    let teardown = run_direct(direct, &scenario.teardown)
        .await
        .map_err(|e| format!("teardown ({side}): {e:#}"));
    Run { outcomes, teardown }
}

/// Run a scenario against plain Postgres, then through the pooler, and compare.
/// `reset` runs on `direct` before each run so state leaked on the pooler's server
/// connections by an earlier scenario can't change this one's result. Anything that
/// stops the comparison is a `Verdict::Error` for this scenario only.
/// With `check_scenarios`, the baseline runs twice first: if Postgres gives different
/// results on two runs, the scenario is broken, not the pooler.
pub async fn check(
    scenario: &Scenario,
    pooled: &str,
    direct: &str,
    reset: &[String],
    check_scenarios: bool,
) -> Checked {
    let expected = match run_once(scenario, direct, direct, reset, Side::Baseline).await.all() {
        Ok(expected) => expected,
        Err(reason) => return Checked::error(reason),
    };
    if let Some(reason) = baseline_failure(&expected) {
        return Checked::error(reason);
    }
    if check_scenarios {
        let again = match run_once(scenario, direct, direct, reset, Side::Baseline).await.all() {
            Ok(again) => again,
            Err(reason) => return Checked::error(reason),
        };
        if let Some(reason) = baseline_failure(&again).or_else(|| baseline_diff(scenario, &expected, &again)) {
            return Checked::error(reason);
        }
    }

    let mut kept: Option<Checked> = None;
    let mut failures = 0;
    for _ in 0..scenario.repeat {
        let run = run_once(scenario, pooled, direct, reset, Side::Target).await;
        let checked = match run.outcomes {
            Ok(actual) => after_teardown(compare(scenario, &expected, actual), run.teardown),
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

/// A failed teardown on the target run only matters when the steps passed: a FAIL
/// found before it is the real result.
fn after_teardown(mut checked: Checked, teardown: Result<(), String>) -> Checked {
    if let (Verdict::Pass, Err(reason)) = (&checked.verdict, teardown) {
        checked.verdict = Verdict::Error { reason };
    }
    checked
}

/// The first step whose two baseline runs differ, compared the way the step says.
fn baseline_diff(scenario: &Scenario, first: &[Outcome], second: &[Outcome]) -> Option<String> {
    let (i, _) = scenario
        .steps
        .iter()
        .enumerate()
        .find(|(i, step)| !first[*i].matches(&second[*i], step.compare))?;
    Some(format!(
        "nondeterministic scenario: step {} gave {} then {} on Postgres",
        i + 1,
        first[i],
        second[i]
    ))
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
    // A direct step that couldn't reach Postgres says nothing about the pooler, unless
    // the pooler already went wrong at an earlier step. A TIMEOUT stays a failure: a
    // lock the pooler leaked can block a direct observer.
    if let Some(m) = &mismatch
        && scenario.steps[m.step - 1].direct
        && matches!(&m.actual, Outcome::Error { code, .. } if code == "CONNECT" || code == "CLIENT")
    {
        return Checked {
            verdict: Verdict::Error {
                reason: format!("postgres side failed at step {} (target run): {}", m.step, m.actual),
            },
            steps: expected.iter().cloned().zip(actual).collect(),
            runs: 1,
            failures: 0,
        };
    }
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
        let checked = check(&s, CLOSED_PORT, CLOSED_PORT, &[], false).await;
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
        let checked = check(&s, CLOSED_PORT, CLOSED_PORT, &[], false).await;
        let reason = reason(&checked);
        assert!(reason.starts_with("setup (baseline): "), "{reason}");
    }

    #[tokio::test]
    async fn repeats_without_reset_do_not_wait_for_the_pooler() {
        // No steps and no setup, so nothing connects except a needless wait_ready.
        let mut s = scenario("name = 't'\ndescription = 't'\nrepeat = 3\nstep = []\n");
        s.steps.clear();
        let started = Instant::now();
        let checked = check(&s, CLOSED_PORT, CLOSED_PORT, &[], false).await;
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
        // A pooler bug at an earlier step isn't hidden by a later direct failure.
        let s2 = scenario(
            "name = 't'\ndescription = 't'\n[[step]]\nclient = 'app'\nsimple = 'SELECT 1'\n\
             [[step]]\nclient = 'pg'\ndirect = true\nsimple = 'SELECT 1'\n",
        );
        let one = Outcome::Ok {
            rows: vec![vec![Some("1".into())]],
            tags: vec![1],
        };
        let checked = compare(
            &s2,
            &[one.clone(), one.clone()],
            vec![Outcome::empty(), Outcome::error("CONNECT", "refused")],
        );
        assert!(matches!(&checked.verdict, Verdict::Fail { mismatch } if mismatch.step == 1));
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
        preflight(&pooled, &url).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn teardown_blocked_by_a_held_lock_is_an_error() {
        let url = test_pg();
        let (holder, driver, _) = connect(&url).await.unwrap();
        // Separate queries: a BEGIN later in the same string would take the CREATE
        // into its transaction, and the table wouldn't exist for anyone else.
        holder
            .batch_execute("DROP TABLE IF EXISTS pp_c3_lock; CREATE TABLE pp_c3_lock ()")
            .await
            .unwrap();
        holder.batch_execute("BEGIN; LOCK pp_c3_lock").await.unwrap();

        let s = scenario("name = 't'\ndescription = 't'\nteardown = ['DROP TABLE pp_c3_lock']\nstep = []\n");
        let started = Instant::now();
        let checked = timeout(Duration::from_secs(30), check(&s, &url, &url, &[], false))
            .await
            .expect("teardown hung");
        let reason = reason(&checked);
        assert!(
            reason.starts_with("teardown (baseline): running `DROP TABLE pp_c3_lock`"),
            "{reason}"
        );
        assert!(reason.contains("lock timeout"), "{reason}");
        assert!(
            started.elapsed() < DIRECT_STATEMENT_TIMEOUT,
            "took {:?}",
            started.elapsed()
        );

        holder.batch_execute("ROLLBACK; DROP TABLE pp_c3_lock").await.unwrap();
        drop(holder);
        let _ = driver.await;
    }

    #[test]
    fn baseline_diff_follows_each_steps_compare() {
        let s = scenario(
            "name = 't'\ndescription = 't'\n\
             [[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n\
             [[step]]\nclient = 'a'\nsimple = 'SELECT random()'\ncompare = 'status'\n",
        );
        let row = |v: &str| Outcome::Ok {
            rows: vec![vec![Some(v.into())]],
            tags: vec![1],
        };
        let first = [row("1"), row("0.1")];
        assert_eq!(baseline_diff(&s, &first, &[row("1"), row("0.2")]), None);
        let diff = baseline_diff(&s, &first, &[row("2"), row("0.1")]).unwrap();
        assert!(
            diff.starts_with("nondeterministic scenario: step 1 gave ok rows=[(1)]"),
            "{diff}"
        );
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn check_scenarios_flags_a_random_baseline() {
        let url = test_pg();
        let toml = |compare: &str| {
            scenario(&format!(
                "name = 't'\ndescription = 't'\n[[step]]\nclient = 'a'\nsimple = 'SELECT random()'\ncompare = '{compare}'\n"
            ))
        };
        let checked = check(&toml("full"), &url, &url, &[], true).await;
        assert!(
            reason(&checked).starts_with("nondeterministic scenario: step 1"),
            "{}",
            reason(&checked)
        );
        let checked = check(&toml("status"), &url, &url, &[], true).await;
        assert!(matches!(checked.verdict, Verdict::Pass));
    }

    #[test]
    fn a_failed_target_teardown_keeps_a_fail() {
        let s = scenario("name = 't'\ndescription = 't'\n[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n");
        let one = Outcome::Ok {
            rows: vec![vec![Some("1".into())]],
            tags: vec![1],
        };
        let expected = [one.clone()];
        let teardown = || Err("teardown (target): lock timeout".to_string());
        let failed = after_teardown(compare(&s, &expected, vec![Outcome::empty()]), teardown());
        assert!(matches!(failed.verdict, Verdict::Fail { .. }));
        let passed = after_teardown(compare(&s, &expected, vec![one.clone()]), teardown());
        assert_eq!(reason(&passed), "teardown (target): lock timeout");
        assert_eq!(passed.steps.len(), 1);
        let clean = after_teardown(compare(&s, &expected, vec![one]), Ok(()));
        assert!(matches!(clean.verdict, Verdict::Pass));
    }

    #[tokio::test]
    async fn preflight_names_the_unreachable_side() {
        let reason = preflight(CLOSED_PORT, CLOSED_PORT).await.unwrap_err();
        assert!(reason.starts_with(DIRECT_UNREACHABLE), "{reason}");
    }
}
