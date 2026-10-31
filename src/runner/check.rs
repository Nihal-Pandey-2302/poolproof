use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use tokio::time::{Instant, timeout};

use super::compare::{FailKind, Mismatch, Verdict, baseline_failure, same_failure};
use super::pg::{connect, connect_with, wait_ready};
use super::steps::{precondition_met, run_steps_on};
use super::tls::TlsSetup;
use super::{Outcome, Timeouts};
use crate::drivers::Broken;
use crate::scenario::{Compare, Scenario};
use crate::targets::{Endpoint, Statement};

/// Start of the ERROR reason when a target's pooler URL doesn't answer before the run.
/// The self-test matches on it, so keep it stable.
pub const POOLER_UNREACHABLE: &str = "target unreachable: pooler:";
/// Same for the target's direct (plain Postgres) URL.
pub const DIRECT_UNREACHABLE: &str = "target unreachable: direct:";

pub(super) async fn run_direct(direct: &Endpoint, statements: &[Statement], t: &Timeouts) -> Result<()> {
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
            // A missing or unreadable CA file won't appear by waiting.
            Err(e) if e.downcast_ref::<TlsSetup>().is_some() => return Err(e),
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
    /// With `repeat`: a failing run failed differently from the kept one (other steps,
    /// other outcomes, or no comparison at all), so the kept steps don't speak for it.
    pub varied: bool,
    /// Runs whose precondition wasn't met. They aren't in `runs` or `failures`.
    pub unmet: u32,
}

impl Checked {
    /// An ERROR before anything was compared, so `runs` is 0.
    pub(super) fn error(reason: String) -> Self {
        Checked {
            verdict: Verdict::Error { reason },
            steps: vec![],
            runs: 0,
            failures: 0,
            varied: false,
            unmet: 0,
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
    /// For a driver scenario: what went wrong with the program beyond its outcomes.
    broken: Option<Broken>,
}

impl Run {
    /// The outcomes, or the first error of the run, teardown included. For the
    /// baseline: a broken driver program there means the driver failed, not Postgres.
    fn all(self) -> Result<Vec<Outcome>, String> {
        let outcomes = self.outcomes?;
        self.teardown?;
        if let Some(broken) = self.broken {
            return Err(format!("steps (baseline): the driver program {}", broken.what));
        }
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
        broken: None,
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
    let mut broken = None;
    let outcomes = match run_direct(direct, &Statement::plain_all(&scenario.setup), t).await {
        Ok(()) => match &scenario.program {
            Some(program) => program.run(pooled, t).await.map(|(outcomes, b)| {
                broken = b;
                outcomes
            }),
            None => run_steps_on(scenario, pooled, direct, side == Side::Baseline, t).await,
        }
        .map_err(|e| format!("steps ({side}): {e:#}")),
        Err(e) => Err(format!("setup ({side}): {e:#}")),
    };
    let teardown = run_direct(direct, &Statement::plain_all(&scenario.teardown), t)
        .await
        .map_err(|e| format!("teardown ({side}): {e:#}"));
    Run {
        outcomes,
        teardown,
        broken,
    }
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
    let modes = scenario.compare_modes();
    let mut kept: Option<Checked> = None;
    let mut failures = 0;
    let mut varied = false;
    // Runs whose precondition wasn't met don't count; the first is kept in case no run met it.
    let mut unmet = 0;
    let mut first_unmet: Option<Checked> = None;
    for _ in 0..scenario.repeat {
        let run = run_once(scenario, pooled, direct, reset, Side::Target, t).await;
        let checked = match run.outcomes {
            Ok(actual) => after_teardown(compare(scenario, &expected, actual, run.broken), run.teardown),
            Err(reason) => Checked::error(reason),
        };
        if checked.unmet > 0 {
            unmet += 1;
            first_unmet.get_or_insert(checked);
            continue;
        }
        let failed = !matches!(checked.verdict, Verdict::Pass);
        failures += u32::from(failed);
        if failed && let Some(first) = kept.as_ref().filter(|k| !matches!(k.verdict, Verdict::Pass)) {
            let both_fail = matches!(
                (&first.verdict, &checked.verdict),
                (Verdict::Fail { .. }, Verdict::Fail { .. })
            );
            varied |= !(both_fail && same_failure(&modes, &first.steps, &checked.steps));
        }
        // Keep the first failure; otherwise the latest pass.
        if kept.as_ref().is_none_or(|k| matches!(k.verdict, Verdict::Pass)) {
            kept = Some(checked);
        }
    }
    // Loading rejects repeat = 0, so the loop ran at least once.
    let mut checked = match (kept, first_unmet) {
        (Some(kept), _) => kept,
        (None, Some(mut skipped)) => {
            if let (Verdict::Skip { reason }, true) = (&mut skipped.verdict, scenario.repeat > 1) {
                *reason = format!("{reason} (in all {} runs)", scenario.repeat);
            }
            skipped
        }
        (None, None) => Checked::error("repeat is 0".into()),
    };
    checked.runs = scenario.repeat - unmet;
    checked.failures = failures;
    checked.varied = varied;
    checked.unmet = unmet;
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
        .find(|(i, step)| !first[*i].matches(&second[*i], step.compare_mode()))?;
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

/// Compare one pooler-side run with the baseline outcomes. `broken` is what went wrong
/// with a driver program beyond its outcomes: a FAIL the steps found keeps its step and
/// kind and says it too; steps that would pass become a FAIL (error) at the last step,
/// with the program's state as the pooler outcome, while `steps` keeps what it reported.
fn compare(scenario: &Scenario, expected: &[Outcome], actual: Vec<Outcome>, broken: Option<Broken>) -> Checked {
    // The first precondition that wasn't met: the steps after it never ran.
    let unmet = scenario
        .steps
        .iter()
        .zip(&actual)
        .position(|(step, outcome)| step.precondition && !precondition_met(outcome));
    let mismatch = scenario
        .steps
        .iter()
        .enumerate()
        .take(unmet.unwrap_or(usize::MAX))
        .find_map(|(i, step)| {
            (!expected[i].matches(&actual[i], step.compare_mode())).then(|| Mismatch {
                step: i + 1,
                action: step.describe(),
                kind: FailKind::of(&expected[i], &actual[i]),
                expected: expected[i].clone(),
                actual: actual[i].clone(),
            })
        });
    // False means this run can't test it; anything else (an error, a hang, no row) is
    // the pooler failing a plain query.
    let met = Outcome::Ok {
        rows: vec![vec![Some("t".into())]],
        tags: vec![1],
    };
    let not_met = Outcome::Ok {
        rows: vec![vec![Some("f".into())]],
        tags: vec![1],
    };
    let mismatch = mismatch.or_else(|| {
        let p = unmet?;
        let actual = &actual[p];
        (!not_met.matches(actual, Compare::Full)).then(|| Mismatch {
            step: p + 1,
            action: scenario.steps[p].describe(),
            kind: FailKind::of(&met, actual),
            expected: met.clone(),
            actual: actual.clone(),
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
            varied: false,
            unmet: 0,
        };
    }
    // Driver scenarios (the only ones with `broken`) have no preconditions.
    let (verdict, unmet) = match (mismatch, unmet, broken) {
        (Some(mut mismatch), _, Some(broken)) => {
            mismatch.action = format!("{}; the program then {}", mismatch.action, broken.what);
            (Verdict::Fail { mismatch }, 0)
        }
        (Some(mismatch), _, None) => (Verdict::Fail { mismatch }, 0),
        (None, Some(p), _) => (
            Verdict::Skip {
                reason: format!("precondition at step {} not met: {}", p + 1, actual[p]),
            },
            1,
        ),
        (None, None, Some(broken)) => match expected.len().checked_sub(1) {
            Some(last) => (
                Verdict::Fail {
                    mismatch: Mismatch {
                        step: last + 1,
                        action: format!("program: {}", broken.what),
                        kind: FailKind::Error,
                        expected: expected[last].clone(),
                        actual: crate::drivers::driver_error(broken.code),
                    },
                },
                0,
            ),
            None => (
                Verdict::Error {
                    reason: format!("the driver program {}", broken.what),
                },
                0,
            ),
        },
        (None, None, None) => (Verdict::Pass, 0),
    };
    Checked {
        verdict,
        steps: expected.iter().cloned().zip(actual).collect(),
        runs: 1,
        failures: 0,
        varied: false,
        unmet,
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
    fn a_precondition_decides_whether_the_run_counts() {
        let s = scenario(
            "name = 't'\ndescription = 't'\n[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n\
             [[step]]\nclient = 'a'\nprecondition = true\nsimple = 'SELECT true'\n\
             [[step]]\nclient = 'a'\nsimple = 'SELECT 2'\n",
        );
        let one = |v: &str| Outcome::Ok {
            rows: vec![vec![Some(v.into())]],
            tags: vec![1],
        };
        let expected = [one("1"), Outcome::NotRun, one("2")];
        let run = |pre: Outcome, last: Outcome| compare(&s, &expected, vec![one("1"), pre, last], None);
        // True: the steps after it are compared as usual.
        assert!(matches!(run(one("t"), one("2")).verdict, Verdict::Pass));
        assert!(matches!(run(one("t"), one("3")).verdict, Verdict::Fail { .. }));
        // False: this run can't test it, whatever ran after.
        let skipped = run(one("f"), Outcome::NotRun);
        assert_eq!(skipped.unmet, 1);
        match skipped.verdict {
            Verdict::Skip { reason } => assert_eq!(reason, "precondition at step 2 not met: ok rows=[(f)] tags=[1]"),
            _ => panic!("not a skip"),
        }
        // Anything else is the pooler failing a plain query.
        let kind = |pre: Outcome| match run(pre, Outcome::NotRun).verdict {
            Verdict::Fail { mismatch } => (mismatch.step, mismatch.kind),
            _ => panic!("not a fail"),
        };
        assert_eq!(kind(Outcome::error("08P01", "x")), (2, FailKind::Error));
        assert_eq!(kind(Outcome::error("TIMEOUT", "x")), (2, FailKind::Hang));
        assert_eq!(kind(Outcome::empty()), (2, FailKind::Silent));
        // A step before it that differs is the FAIL, not the precondition.
        let earlier = compare(&s, &expected, vec![one("9"), one("f"), Outcome::NotRun], None);
        assert!(matches!(earlier.verdict, Verdict::Fail { mismatch } if mismatch.step == 1));
    }

    #[test]
    fn direct_step_that_fails_to_connect_on_the_target_run_is_an_error() {
        let s =
            scenario("name = 't'\ndescription = 't'\n[[step]]\nclient = 'pg'\ndirect = true\nsimple = 'SELECT 1'\n");
        let expected = [Outcome::empty()];
        for code in ["CONNECT", "CLIENT"] {
            let checked = compare(&s, &expected, vec![Outcome::error(code, "refused")], None);
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
            None,
        );
        assert!(matches!(&checked.verdict, Verdict::Fail { mismatch } if mismatch.step == 1));
        // An ignored direct step that can't connect is still no result.
        let s3 = scenario(
            "name = 't'\ndescription = 't'\n[[step]]\nclient = 'pg'\ndirect = true\nsimple = 'SELECT 1'\ncompare = 'ignore'\n",
        );
        let checked = compare(&s3, &expected, vec![Outcome::error("CLIENT", "reset")], None);
        assert!(
            reason(&checked).starts_with("postgres side failed at step 1 (target run)"),
            "{}",
            reason(&checked)
        );
        // A direct observer blocked by a lock the pooler leaked is the pooler's fault.
        let checked = compare(&s, &expected, vec![Outcome::error("TIMEOUT", "no reply")], None);
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
        let failed = after_teardown(compare(&s, &expected, vec![Outcome::empty()], None), teardown());
        assert!(matches!(failed.verdict, Verdict::Fail { .. }));
        let passed = after_teardown(compare(&s, &expected, vec![one.clone()], None), teardown());
        assert_eq!(reason(&passed), "teardown (target): lock timeout");
        assert_eq!(passed.steps.len(), 1);
        let clean = after_teardown(compare(&s, &expected, vec![one], None), Ok(()));
        assert!(matches!(clean.verdict, Verdict::Pass));
    }

    #[test]
    fn a_broken_driver_program_never_hides_or_rewrites_a_result() {
        let s = scenario(
            "name = 't'\ndescription = 't'\n[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n\
             [[step]]\nclient = 'a'\nsimple = 'SHOW x'\n",
        );
        let row = |v: &str| Outcome::Ok {
            rows: vec![vec![Some(v.into())]],
            tags: vec![],
        };
        let expected = [row("1"), row("0")];
        let stopped = || {
            Some(Broken {
                code: "DRIVER:Stopped",
                what: "stopped after 2 of 2 steps (exit status: 1)".into(),
            })
        };

        // A silent difference at the last step stays silent, and says what else happened.
        let leak = compare(&s, &expected, vec![row("1"), row("5s")], stopped());
        let Verdict::Fail { mismatch } = &leak.verdict else {
            panic!("{:?}", leak.verdict)
        };
        assert_eq!((mismatch.step, mismatch.kind), (2, FailKind::Silent));
        assert!(
            mismatch
                .action
                .ends_with("; the program then stopped after 2 of 2 steps (exit status: 1)")
        );
        assert_eq!(mismatch.actual, row("5s"));

        // Steps that would pass: a FAIL (error) at the last step, with the program's state
        // as the pooler outcome. The trace keeps what the program reported.
        let clean = compare(&s, &expected, vec![row("1"), row("0")], stopped());
        let Verdict::Fail { mismatch } = &clean.verdict else {
            panic!("{:?}", clean.verdict)
        };
        assert_eq!((mismatch.step, mismatch.kind), (2, FailKind::Error));
        assert_eq!(mismatch.action, "program: stopped after 2 of 2 steps (exit status: 1)");
        assert_eq!(mismatch.expected, row("0"));
        assert!(matches!(&mismatch.actual, Outcome::Error { code, .. } if code == "DRIVER:Stopped"));
        assert_eq!(clean.steps[1].1, row("0"));
        assert!(matches!(
            compare(&s, &expected, vec![row("1"), row("0")], None).verdict,
            Verdict::Pass
        ));

        // On Postgres, a broken program means the driver failed: ERROR.
        let baseline = Run {
            outcomes: Ok(expected.to_vec()),
            teardown: Ok(()),
            broken: stopped(),
        };
        assert!(baseline.all().unwrap_err().contains("stopped after 2 of 2 steps"));
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn preconditions_run_only_through_the_pooler() {
        let direct = Endpoint::parse(&test_pg()).unwrap();
        // The same server under another name, standing in for a pooler.
        let sep = if test_pg().contains('?') { '&' } else { '?' };
        let pooled = Endpoint::parse(&format!("{}{sep}application_name=pp_pooled", test_pg())).unwrap();
        let with = |pre: &str, repeat: u32| {
            scenario(&format!(
                "name = 't'\ndescription = 't'\nrepeat = {repeat}\n[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n\
                 [[step]]\nclient = 'a'\nprecondition = true\nsimple = \"{pre}\"\n\
                 [[step]]\nclient = 'a'\nsimple = 'SELECT 2'\n"
            ))
        };
        let t = Timeouts::default();
        let met = check(&with("SELECT true", 3), &pooled, &direct, &[], &t).await;
        assert!(matches!(met.verdict, Verdict::Pass));
        assert_eq!((met.runs, met.unmet), (3, 0));
        // Postgres never runs it: there this would fail with 22012.
        let error = check(&with("SELECT 1/0 = 1", 1), &pooled, &direct, &[], &t).await;
        assert_eq!(error.steps[1].0, Outcome::NotRun);
        match error.verdict {
            Verdict::Fail { mismatch } => assert_eq!((mismatch.step, mismatch.kind), (2, FailKind::Error)),
            _ => panic!("not a fail"),
        }
        assert_eq!(error.steps[2].1, Outcome::NotRun);
        let never = check(&with("SELECT false", 3), &pooled, &direct, &[], &t).await;
        assert_eq!((never.runs, never.failures, never.unmet), (0, 0, 3));
        match never.verdict {
            Verdict::Skip { reason } => assert!(reason.ends_with("(in all 3 runs)"), "{reason}"),
            _ => panic!("not a skip"),
        }
        assert_eq!(never.steps[2].1, Outcome::NotRun);
        // Postgres standing in for its own pooler, at the very same URL: the pooler
        // side still runs the precondition.
        let passthrough = check(&with("SELECT false", 1), &direct, &direct, &[], &t).await;
        assert!(matches!(passthrough.verdict, Verdict::Skip { .. }));
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
