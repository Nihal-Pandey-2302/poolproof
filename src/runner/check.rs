use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use tokio::time::{Instant, timeout};

use super::compare::{FailKind, Mismatch, Verdict, baseline_failure};
use super::pg::{connect, connect_with, wait_ready};
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
    // Waits for the backend to exit on close, so a setup that ends holding a lock or a
    // temp table is fully gone before the steps start.
    let (client, driver, _) = connect_with(direct, t, true).await?;
    let limits = Statement::plain(&format!(
        "SET lock_timeout = '{}ms'; SET statement_timeout = '{}ms'",
        t.direct_lock.as_millis(),
        t.direct_statement.as_millis()
    ));
    let mut res = execute(&client, &limits, t).await;
    for s in statements {
        if res.is_err() {
            break;
        }
        res = execute(&client, s, t).await;
    }
    // Close the session even after an error, so it's gone before the next phase
    // (e.g. a failed setup's session while teardown runs). The first error wins.
    drop(client);
    let exited = timeout(t.housekeeping, driver).await.is_ok();
    res?;
    if !exited {
        bail!("the backend did not exit within {:?} of the disconnect", t.housekeeping);
    }
    Ok(())
}

/// Errors show the statement as written. The server's message can quote the executed
/// text, so with substituted values only its SQLSTATE is kept.
async fn execute(client: &tokio_postgres::Client, s: &Statement, t: &Timeouts) -> Result<()> {
    let limit = t.direct_statement + t.direct_grace;
    match timeout(limit, client.batch_execute(s.sql())).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) if s.has_substitutions() => {
            let code = e.code().map_or("an error without SQLSTATE", |c| c.code());
            Err(anyhow!(
                "running `{s}`: {code} (details hidden: the statement has substituted values)"
            ))
        }
        Ok(Err(e)) => Err(anyhow::Error::from(e).context(format!("running `{s}`"))),
        Err(_) => Err(anyhow!("running `{s}`: no reply within {limit:?}")),
    }
}

/// Connect and run `SELECT 1` once.
async fn probe(endpoint: &Endpoint, t: &Timeouts) -> Result<()> {
    let (client, driver, _) = connect(endpoint, t).await?;
    let res = timeout(t.housekeeping, client.simple_query("SELECT 1")).await;
    drop(client);
    let _ = timeout(t.housekeeping, driver).await;
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
async fn run_once(
    scenario: &Scenario,
    pooled: &Endpoint,
    direct: &Endpoint,
    reset: &[Statement],
    side: Side,
    t: &Timeouts,
) -> Run {
    let failed = |reason| Run {
        outcomes: Err(reason),
        teardown: Ok(()),
    };
    if let Err(e) = run_direct(direct, reset, t).await {
        return failed(format!("reset ({side}): {e:#}"));
    }
    // A reset kills the pooler's server connections, and it needs a moment to open new
    // ones. Without a reset there's nothing to wait for.
    if side == Side::Target
        && !reset.is_empty()
        && let Err(e) = wait_ready(pooled, t).await
    {
        return failed(format!("{e:#}"));
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
    Run { outcomes, teardown }
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
    let expected = match run_once(scenario, direct, direct, reset, Side::Baseline, t).await.all() {
        Ok(expected) => expected,
        Err(reason) => return Checked::error(reason),
    };
    if let Some(reason) = baseline_failure(&expected) {
        return Checked::error(reason);
    }
    let mut kept: Option<Checked> = None;
    let mut failures = 0;
    for _ in 0..scenario.repeat {
        let run = run_once(scenario, pooled, direct, reset, Side::Target, t).await;
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

/// Run the baseline twice. If Postgres gives different results, the scenario tests
/// timing, not the pooler, and the error says where. Back-to-back runs catch random
/// values, clocks and pids in results; they don't catch effects of the order scenarios
/// run in (`run --reverse` is for that).
pub async fn deterministic(
    scenario: &Scenario,
    direct: &Endpoint,
    reset: &[Statement],
    t: &Timeouts,
) -> Result<(), String> {
    let first = run_once(scenario, direct, direct, reset, Side::Baseline, t)
        .await
        .all()?;
    let second = run_once(scenario, direct, direct, reset, Side::Baseline, t)
        .await
        .all()?;
    let problem = baseline_failure(&first)
        .or_else(|| baseline_failure(&second))
        .or_else(|| baseline_diff(scenario, &first, &second));
    problem.map_or(Ok(()), Err)
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
    // the pooler already went wrong at an earlier step. That includes direct steps with
    // `compare = "ignore"`, which never show up as a mismatch. A TIMEOUT stays a
    // failure: a lock the pooler leaked can block a direct observer.
    let direct_failed = scenario.steps.iter().zip(&actual).position(|(step, outcome)| {
        step.direct && matches!(outcome, Outcome::Error { code, .. } if code == "CONNECT" || code == "CLIENT")
    });
    if let Some(i) = direct_failed
        && mismatch.as_ref().is_none_or(|m| m.step > i)
    {
        return Checked {
            verdict: Verdict::Error {
                reason: format!("postgres side failed at step {} (target run): {}", i + 1, actual[i]),
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
        // An ignored direct step that can't connect is still no result.
        let s3 = scenario(
            "name = 't'\ndescription = 't'\n[[step]]\nclient = 'pg'\ndirect = true\nsimple = 'SELECT 1'\ncompare = 'ignore'\n",
        );
        let checked = compare(&s3, &expected, vec![Outcome::error("CLIENT", "reset")]);
        assert!(
            reason(&checked).starts_with("postgres side failed at step 1 (target run)"),
            "{}",
            reason(&checked)
        );
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

    /// A server that accepts any login, answers SET, and never answers anything else.
    fn silent_server() -> Endpoint {
        use std::io::{Read, Write};
        fn message(tag: u8, body: &[u8]) -> Vec<u8> {
            [&[tag][..], &(body.len() as u32 + 4).to_be_bytes(), body].concat()
        }
        fn serve(mut s: std::net::TcpStream) -> std::io::Result<()> {
            let read = |s: &mut std::net::TcpStream, n: usize| -> std::io::Result<Vec<u8>> {
                let mut buf = vec![0; n];
                s.read_exact(&mut buf)?;
                Ok(buf)
            };
            loop {
                let len = u32::from_be_bytes(read(&mut s, 4)?.try_into().unwrap()) as usize;
                // SSLRequest: refuse TLS, the startup message follows.
                if read(&mut s, len - 4)? != [0x04, 0xd2, 0x16, 0x2f] {
                    break;
                }
                s.write_all(b"N")?;
            }
            let ready = message(b'Z', b"I");
            s.write_all(&[message(b'R', &[0; 4]), ready.clone()].concat())?;
            loop {
                let head = read(&mut s, 5)?;
                let body = read(&mut s, u32::from_be_bytes(head[1..].try_into().unwrap()) as usize - 4)?;
                if head[0] == b'X' {
                    return Ok(());
                }
                if body.starts_with(b"SET ") {
                    s.write_all(&[message(b'C', b"SET\0"), ready.clone()].concat())?;
                }
            }
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || serve(stream));
            }
        });
        Endpoint::parse(&format!("postgres://u@127.0.0.1:{port}/app")).unwrap()
    }

    #[tokio::test]
    async fn a_reset_with_substituted_values_that_times_out_is_shown_as_written() {
        let t = Timeouts {
            direct_statement: Duration::from_millis(200),
            direct_grace: Duration::from_millis(100),
            housekeeping: Duration::from_millis(500),
            ..Timeouts::default()
        };
        let reset = Statement::substituted(
            "ALTER ROLE app PASSWORD 'canary-1c9e'",
            "ALTER ROLE app PASSWORD '${P}'",
        );
        let started = Instant::now();
        let err = run_direct(&silent_server(), &[reset], &t).await.unwrap_err();
        let err = format!("{err:#}");
        assert_eq!(err, "running `ALTER ROLE app PASSWORD '${P}'`: no reply within 300ms");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
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
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn teardown_blocked_by_a_held_lock_is_an_error() {
        let url = Endpoint::parse(&test_pg()).unwrap();
        let (holder, driver, _) = connect(&url, &Timeouts::default()).await.unwrap();
        // Separate queries: a BEGIN later in the same string would take the CREATE
        // into its transaction, and the table wouldn't exist for anyone else.
        holder
            .batch_execute("DROP TABLE IF EXISTS pp_c3_lock; CREATE TABLE pp_c3_lock ()")
            .await
            .unwrap();
        holder.batch_execute("BEGIN; LOCK pp_c3_lock").await.unwrap();

        let s = scenario("name = 't'\ndescription = 't'\nteardown = ['DROP TABLE pp_c3_lock']\nstep = []\n");
        let started = Instant::now();
        let checked = timeout(
            Duration::from_secs(30),
            check(&s, &url, &url, &[], &Timeouts::default()),
        )
        .await
        .expect("teardown hung");
        let reason = reason(&checked);
        assert!(
            reason.starts_with("teardown (baseline): running `DROP TABLE pp_c3_lock`"),
            "{reason}"
        );
        assert!(reason.contains("lock timeout"), "{reason}");
        assert!(
            started.elapsed() < Timeouts::default().direct_statement,
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
        let url = Endpoint::parse(&test_pg()).unwrap();
        let toml = |compare: &str| {
            scenario(&format!(
                "name = 't'\ndescription = 't'\n[[step]]\nclient = 'a'\nsimple = 'SELECT random()'\ncompare = '{compare}'\n"
            ))
        };
        let reason = deterministic(&toml("full"), &url, &[], &Timeouts::default())
            .await
            .unwrap_err();
        assert!(reason.starts_with("nondeterministic scenario: step 1"), "{reason}");
        deterministic(&toml("status"), &url, &[], &Timeouts::default())
            .await
            .unwrap();
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
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn the_step_timeout_is_configurable() {
        let url = Endpoint::parse(&test_pg()).unwrap();
        let s = scenario("name = 't'\ndescription = 't'\n[[step]]\nclient = 'a'\nsimple = 'SELECT pg_sleep(2)'\n");
        let short = Timeouts {
            step: Duration::from_secs(1),
            ..Timeouts::default()
        };
        let checked = check(&s, &url, &url, &[], &short).await;
        let reason = reason(&checked);
        assert!(
            reason.starts_with("baseline failed at step 1: error TIMEOUT"),
            "{reason}"
        );
        let checked = check(&s, &url, &url, &[], &Timeouts::default()).await;
        assert!(matches!(checked.verdict, Verdict::Pass));
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn send_steps_record_parameter_status_when_asked() {
        let url = Endpoint::parse(&test_pg()).unwrap();
        let s = scenario(
            "name = 't'\ndescription = 't'\n[[step]]\nclient = 'a'\n\
             send = [\"query SET application_name = 'pp_c10'\"]\ncompare_parameter_status = true\n",
        );
        let checked = check(&s, &url, &url, &[], &Timeouts::default()).await;
        assert!(matches!(checked.verdict, Verdict::Pass));
        let Outcome::Messages { messages } = &checked.steps[0].0 else {
            panic!("expected send replies")
        };
        assert!(
            messages.contains(&"ParameterStatus application_name=pp_c10".to_string()),
            "{messages:?}"
        );
    }

    #[tokio::test]
    async fn preflight_names_the_unreachable_side() {
        let reason = preflight(&closed_port(), &closed_port(), &Timeouts::default())
            .await
            .unwrap_err();
        assert!(reason.starts_with(DIRECT_UNREACHABLE), "{reason}");
    }
}
