//! The loop over targets and scenarios behind `poolproof run`.

use std::time::Duration;

use anyhow::{Result, bail};

use super::probe::{ProbeResult, Probes, probe_target};
use super::{
    DIRECT_UNREACHABLE, FailKind, Privileges, Timeouts, Verdict, check, deterministic, preflight, privileges,
    resolve_reset, server_version,
};
use crate::limitations::Limitations;
use crate::output::text::{Style, print_progress, print_result};
use crate::report::{ResultRow, StepOutcomes, TargetInfo};
use crate::scenario::{PoolMode, Scenario};
use crate::targets::{Endpoint, Reset, Statement, Target};

/// A --check-scenarios verdict, by scenario, direct endpoint (compared in full: options
/// and password too) and resolved reset.
type Determinism<'a> = (&'a str, &'a Endpoint, Vec<Statement>, Result<(), String>);

/// How `run` goes about it.
#[derive(Default)]
pub struct RunOptions<'a> {
    /// Only scenarios whose name contains this.
    pub filter: Option<&'a str>,
    /// Only the targets with these names; all when empty.
    pub targets: &'a [String],
    /// Print each result as text as it comes in. Otherwise a progress line per result
    /// goes to stderr.
    pub text: bool,
    pub style: Style,
    /// Run each baseline twice and report a scenario whose results differ as ERROR.
    /// Done once per scenario and Postgres server, not once per target.
    pub check_scenarios: bool,
    /// A scenario's `timeout_ms` overrides `step`.
    pub timeouts: Timeouts,
    /// Allow reset = "all" against a Postgres that isn't on this machine.
    pub yes: bool,
    /// Documented limitations, to mark expected FAILs as they come in.
    pub limitations: Option<&'a Limitations>,
}

/// Runs every scenario on every target, in order.
pub async fn run(
    targets: &[Target],
    scenarios: &[Scenario],
    opts: &RunOptions<'_>,
) -> Result<(Vec<TargetInfo>, Vec<ResultRow>)> {
    let timeouts = opts.timeouts;
    let targets = select(targets, opts.targets)?;
    let wanted = |s: &&Scenario| opts.filter.is_none_or(|f| s.name.contains(f));
    let total = targets.len() * scenarios.iter().filter(wanted).count();
    let mut results = Vec::new();
    let mut target_infos = Vec::new();
    // Few enough to search in order.
    let mut determinism: Vec<Determinism> = Vec::new();
    for target in targets {
        target_infos.push(TargetInfo {
            name: target.name.clone(),
            product: target.product.clone(),
            version: target.version.clone(),
            config: target.config.clone(),
            mode: target.mode,
            postgres: server_version(&target.direct, &timeouts).await.ok(),
            reset: Some(target.reset.kind().to_string()),
            idle_probe: None,
            busy_probe: None,
        });
        if matches!(target.reset, Reset::None) {
            eprintln!(
                "warning: target '{}' has no reset; leaked state can carry over between scenarios",
                target.name
            );
        }
        let applies = |s: &&Scenario| wanted(s) && s.modes.contains(&target.mode);
        let any_applies = scenarios.iter().any(|s| applies(&s));
        // Without both URLs answering, every scenario would compare two failures.
        let unreachable = if any_applies {
            preflight(&target.pooler, &target.direct, &timeouts).await.err()
        } else {
            None
        };
        if let Some(reason) = &unreachable {
            eprintln!("error: target '{}': {reason}", target.name);
        }
        // What reset will touch, said before it touches anything; a reset that can't
        // be done safely makes the target an ERROR like an unreachable one. Same gate
        // as preflight: nothing is asked or printed when no scenario will run.
        let (reset, unreachable) = match &unreachable {
            Some(_) => (vec![], unreachable),
            None if !any_applies => (vec![], None),
            None => match resolve_reset(target, opts.yes, &timeouts).await {
                Ok((statements, line)) => {
                    if !line.is_empty() {
                        eprintln!("{line}");
                    }
                    (statements, None)
                }
                Err(reason) => {
                    eprintln!("error: target '{}': {reason}", target.name);
                    (vec![], Some(reason.to_string()))
                }
            },
        };
        // Same gate as preflight: no connection when nothing will run.
        let privs = match unreachable {
            Some(_) => Err(String::new()),
            None if !any_applies => Err(String::new()),
            None => privileges(&target.direct, &timeouts)
                .await
                .map_err(|e| format!("{DIRECT_UNREACHABLE} privilege check: {e:#}")),
        };
        // "all" and "auto" check this exactly in resolve_reset; own statements can't be.
        if let Ok(p) = &privs
            && let Reset::Statements(statements) = &target.reset
            && !p.superuser
            && !(p.pg_signal_backend && p.pg_read_all_stats)
            && statements
                .iter()
                .any(|s| s.sql().to_lowercase().contains("pg_terminate_backend"))
        {
            let missing: Vec<&str> = [
                (!p.pg_signal_backend).then_some("pg_signal_backend"),
                (!p.pg_read_all_stats).then_some("pg_read_all_stats"),
            ]
            .into_iter()
            .flatten()
            .collect();
            eprintln!(
                "warning: target '{}': reset terminates sessions, but the direct user isn't a superuser and \
                 lacks {}, so it can only see and end sessions of roles whose privileges it has (and only a \
                 superuser can end a superuser's)",
                target.name,
                missing.join(" and ")
            );
        }
        // What a second client gets, for the mode check and the server-connection
        // requirements. Same gate as preflight.
        let mut unreachable = unreachable;
        let probes = match &unreachable {
            None if any_applies => {
                match probe_target(&target.pooler, &target.direct, &reset, target.mode, &timeouts).await {
                    Ok(probes) => Some(probes),
                    Err(reason) => {
                        eprintln!("error: target '{}': {reason}", target.name);
                        unreachable = Some(reason);
                        None
                    }
                }
            }
            _ => None,
        };
        if let Some(probes) = &probes {
            for (which, probe) in [("idle", &probes.idle), ("busy", &probes.busy)] {
                if let (ProbeResult::Unknown, Some(detail)) = (probe.result, &probe.detail)
                    && target.mode != PoolMode::Statement
                {
                    eprintln!("warning: target '{}': {which} probe: {detail}", target.name);
                }
            }
            if let Some(reason) = probes.mode_error(target.mode) {
                eprintln!("error: target '{}': {reason}", target.name);
                unreachable = Some(reason);
            }
            if let Some(info) = target_infos.last_mut() {
                info.idle_probe = Some(probes.idle.clone());
                info.busy_probe = Some(probes.busy.clone());
            }
        }
        let has_precondition = |s: &Scenario| s.steps.iter().any(|st| st.precondition);
        for s in scenarios.iter().filter(wanted) {
            let checks = before_run(s, target, unreachable.as_deref(), &privs, probes.as_ref());
            let (verdict, steps, runs, failures, varied, unmet) = match checks {
                Some(verdict) => (verdict, vec![], 0, 0, false, 0),
                None => {
                    let seen = |(name, direct, statements, _): &&Determinism| {
                        *name == s.name && *direct == &target.direct && *statements == reset
                    };
                    if opts.check_scenarios && !determinism.iter().any(|d| seen(&d)) {
                        let result = deterministic(s, &target.direct, &reset, &for_scenario(&timeouts, s)).await;
                        determinism.push((&s.name, &target.direct, reset.clone(), result));
                    }
                    match determinism.iter().find(seen) {
                        Some((_, _, _, Err(reason))) => {
                            (Verdict::Error { reason: reason.clone() }, vec![], 0, 0, false, 0)
                        }
                        _ => {
                            let c = check(s, &target.pooler, &target.direct, &reset, &for_scenario(&timeouts, s)).await;
                            (c.verdict, c.steps, c.runs, c.failures, c.varied, c.unmet)
                        }
                    }
                }
            };
            let steps = steps
                .into_iter()
                .map(|(postgres, pooler)| StepOutcomes { postgres, pooler })
                .collect();
            let mut row = ResultRow {
                target: target.name.clone(),
                scenario: s.name.clone(),
                verdict,
                steps,
                runs,
                failures,
                varied: (runs > 1).then_some(varied),
                precondition_unmet: has_precondition(s).then_some(unmet),
                expected: None,
            };
            if let (Some(limitations), Some(info)) = (opts.limitations, target_infos.last()) {
                row.expected = limitations.expected_for(info, s, &row);
            }
            if opts.text {
                print_result(s, &row, opts.style);
            } else {
                print_progress(results.len() + 1, total, &target.name, s, &row.verdict);
            }
            results.push(row);
        }
    }
    Ok((target_infos, results))
}

/// The verdict for a scenario that won't run on this target, or `None` to run it.
/// `privs` is the direct user's privileges, or why they couldn't be read.
fn before_run(
    s: &Scenario,
    target: &Target,
    unreachable: Option<&str>,
    privs: &Result<Privileges, String>,
    probes: Option<&Probes>,
) -> Option<Verdict> {
    if !s.modes.contains(&target.mode) {
        let reason = format!("not applicable to {} mode", target.mode);
        return Some(Verdict::Skip { reason });
    }
    if let Some(reason) = unreachable {
        return Some(Verdict::Error {
            reason: reason.to_string(),
        });
    }
    if s.requires.iter().any(|r| !r.is_server()) {
        match privs {
            Ok(privs) => {
                if let Some(reason) = privs.missing(&s.requires) {
                    return Some(Verdict::Skip { reason });
                }
            }
            Err(reason) => return Some(Verdict::Error { reason: reason.clone() }),
        }
    }
    let reason = probes?.unmet(s, target.mode, target.ignore_server_requirements)?;
    Some(Verdict::Skip { reason })
}

/// Which results make `run` exit 1. Each level includes the ones before it. An ERROR
/// verdict always counts, except with `none`: it means there was no verdict at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum FailOn {
    /// Never.
    None,
    /// Silent failures only: wrong results with no error.
    Silent,
    /// Silent FAILs and FAILs where the pooler returned an error Postgres didn't.
    /// Not about ERROR verdicts, which count for every level but `none`.
    Error,
    /// Every failure, hangs included.
    #[default]
    Any,
}

impl std::fmt::Display for FailOn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            FailOn::None => "none",
            FailOn::Silent => "silent",
            FailOn::Error => "error",
            FailOn::Any => "any",
        })
    }
}

impl FailOn {
    /// Whether this verdict should make `run` exit 1.
    pub fn fails(self, verdict: &Verdict) -> bool {
        let kind = match verdict {
            Verdict::Pass | Verdict::Skip { .. } => return false,
            Verdict::Error { .. } => return self != FailOn::None,
            Verdict::Fail { mismatch } => mismatch.kind,
        };
        match self {
            FailOn::None => false,
            FailOn::Silent => kind == FailKind::Silent,
            FailOn::Error => kind != FailKind::Hang,
            FailOn::Any => true,
        }
    }
}

/// What makes `run` exit 1: `--fail-on`, and with `--ignore-expected`, not the FAILs
/// the pooler's own docs describe.
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    pub fail_on: FailOn,
    pub ignore_expected: bool,
}

impl Gate {
    pub fn fails(self, row: &ResultRow) -> bool {
        self.fail_on.fails(&row.verdict) && !(self.ignore_expected && row.expected.is_some())
    }
}

impl From<FailOn> for Gate {
    fn from(fail_on: FailOn) -> Self {
        Gate {
            fail_on,
            ignore_expected: false,
        }
    }
}

/// The timeouts for one scenario: its own step timeout, if it sets one.
fn for_scenario(timeouts: &Timeouts, s: &Scenario) -> Timeouts {
    match s.timeout_ms {
        Some(ms) => Timeouts {
            step: Duration::from_millis(ms),
            ..*timeouts
        },
        None => *timeouts,
    }
}

/// The targets named in `names`, in file order; all of them when `names` is empty.
pub fn select<'t>(targets: &'t [Target], names: &[String]) -> Result<Vec<&'t Target>> {
    if let Some(unknown) = names.iter().find(|n| !targets.iter().any(|t| &t.name == *n)) {
        let known: Vec<&str> = targets.iter().map(|t| t.name.as_str()).collect();
        bail!("no target named '{unknown}'; targets: {}", known.join(", "));
    }
    Ok(targets
        .iter()
        .filter(|t| names.is_empty() || names.contains(&t.name))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::PoolMode;
    use crate::targets::Targets;

    #[test]
    fn unmet_requirements_skip_before_running() {
        let target = Targets::single(
            "postgres://u@127.0.0.1:1/db",
            "postgres://u@127.0.0.1:1/db",
            PoolMode::Transaction,
            Reset::None,
        )
        .unwrap()
        .targets
        .remove(0);
        let scenario = |requires: &str| -> Scenario {
            toml::from_str(&format!(
                "name = 's'\ndescription = ''\nrequires = [{requires}]\nstep = []\n"
            ))
            .unwrap()
        };
        let plain = Privileges {
            superuser: false,
            createrole: false,
            pg_signal_backend: false,
            pg_read_all_stats: false,
        };
        let creator = Privileges {
            createrole: true,
            ..plain
        };
        let verdict = |s: &Scenario, privs: Result<Privileges, String>| before_run(s, &target, None, &privs, None);

        let roles = scenario("'createrole'");
        assert!(matches!(
            verdict(&roles, Ok(plain)),
            Some(Verdict::Skip { reason }) if reason == "needs createrole on direct"
        ));
        assert!(verdict(&roles, Ok(creator)).is_none());
        assert!(matches!(
            verdict(&roles, Err("no answer".into())),
            Some(Verdict::Error { .. })
        ));
        // Nothing required: runs even when the privileges couldn't be read.
        assert!(verdict(&scenario(""), Err("no answer".into())).is_none());
        // Unreachable wins over requirements.
        assert!(matches!(
            before_run(&roles, &target, Some("down"), &Ok(creator), None),
            Some(Verdict::Error { reason }) if reason == "down"
        ));
    }

    #[test]
    fn fail_on_is_a_ladder_and_errors_always_count() {
        use crate::runner::{Mismatch, Outcome};
        let fail = |kind| Verdict::Fail {
            mismatch: Mismatch {
                step: 1,
                action: String::new(),
                kind,
                expected: Outcome::Abandoned,
                actual: Outcome::Abandoned,
            },
        };
        let error = Verdict::Error { reason: "x".into() };
        let verdicts = [
            Verdict::Pass,
            fail(FailKind::Silent),
            fail(FailKind::Error),
            fail(FailKind::Hang),
            error,
        ];
        let exits = |on: FailOn| verdicts.iter().map(|v| on.fails(v)).collect::<Vec<_>>();
        assert_eq!(exits(FailOn::None), [false, false, false, false, false]);
        assert_eq!(exits(FailOn::Silent), [false, true, false, false, true]);
        assert_eq!(exits(FailOn::Error), [false, true, true, false, true]);
        assert_eq!(exits(FailOn::Any), [false, true, true, true, true]);
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn check_scenarios_runs_once_per_postgres_server() {
        let url = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
        // The crate's own connect, so the URL's sslmode works here too.
        let endpoint = crate::targets::Endpoint::parse(&url).unwrap();
        let (client, _driver, _) = crate::runner::pg::connect(&endpoint, &Timeouts::default())
            .await
            .unwrap();
        client
            .batch_execute("DROP TABLE IF EXISTS pp_runs; CREATE TABLE pp_runs ()")
            .await
            .unwrap();
        // Setup runs once per baseline and once per target run.
        let s: Scenario = toml::from_str(
            "name = 't'\ndescription = 't'\nsetup = ['INSERT INTO pp_runs DEFAULT VALUES']\n\
             [[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n",
        )
        .unwrap();
        let mut targets = Targets::single(&url, &url, PoolMode::Transaction, Reset::None)
            .unwrap()
            .targets;
        let mut second = Targets::single(&url, &url, PoolMode::Transaction, Reset::None)
            .unwrap()
            .targets;
        second[0].name = "b".into();
        targets.append(&mut second);
        // Same server, other options: may behave differently, so it's checked again.
        let other = format!("{url}{}application_name=pp", if url.contains('?') { '&' } else { '?' });
        let mut third = Targets::single(&url, &other, PoolMode::Transaction, Reset::None)
            .unwrap()
            .targets;
        third[0].name = "c".into();
        targets.append(&mut third);
        // Same direct as a, but a reset: also checked again.
        let reset = Reset::Statements(vec![Statement::plain("SELECT 1")]);
        let mut fourth = Targets::single(&url, &url, PoolMode::Transaction, reset)
            .unwrap()
            .targets;
        fourth[0].name = "d".into();
        targets.append(&mut fourth);
        let opts = RunOptions {
            check_scenarios: true,
            ..RunOptions::default()
        };
        let (_, results) = run(&targets, std::slice::from_ref(&s), &opts).await.unwrap();
        assert!(results.iter().all(|r| matches!(r.verdict, Verdict::Pass)));
        let rows = client.query_one("SELECT count(*) FROM pp_runs", &[]).await.unwrap();
        // 2 for the check shared by a and b, 2 each for c's and d's, then baseline +
        // target run on each target.
        assert_eq!(rows.get::<_, i64>(0), 2 + 2 + 2 + 4 * 2);
        client.batch_execute("DROP TABLE pp_runs").await.unwrap();
    }

    #[tokio::test]
    async fn reset_is_not_resolved_when_no_scenario_applies() {
        // reset = "auto" asks the pooler first; it must not even connect.
        let pooler = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        pooler.set_nonblocking(true).unwrap();
        let port = pooler.local_addr().unwrap().port();
        let targets = Targets::single(
            &format!("postgres://u@127.0.0.1:{port}/db"),
            "postgres://u@127.0.0.1:1/db",
            PoolMode::Session,
            Reset::Auto,
        )
        .unwrap()
        .targets;
        let s: Scenario = toml::from_str("name = 's'\ndescription = ''\nmodes = ['transaction']\nstep = []\n").unwrap();
        let (_, results) = run(&targets, std::slice::from_ref(&s), &RunOptions::default())
            .await
            .unwrap();
        assert!(
            matches!(&results[0].verdict, Verdict::Skip { .. }),
            "{:?}",
            results[0].verdict
        );
        assert!(pooler.accept().is_err(), "the pooler was asked");
    }

    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn a_refused_reset_runs_no_baseline() {
        let url = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
        let endpoint = crate::targets::Endpoint::parse(&url).unwrap();
        let (client, _driver, _) = crate::runner::pg::connect(&endpoint, &Timeouts::default())
            .await
            .unwrap();
        client
            .batch_execute("DROP TABLE IF EXISTS pp_refused; CREATE TABLE pp_refused ()")
            .await
            .unwrap();
        let s: Scenario = toml::from_str(
            "name = 't'\ndescription = 't'\nsetup = ['INSERT INTO pp_refused DEFAULT VALUES']\n\
             [[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n",
        )
        .unwrap();
        // The pooler is Postgres itself, on this machine: "auto" refuses that address.
        let targets = Targets::single(&url, &url, PoolMode::Transaction, Reset::Auto)
            .unwrap()
            .targets;
        let opts = RunOptions {
            check_scenarios: true,
            ..RunOptions::default()
        };
        let (_, results) = run(&targets, std::slice::from_ref(&s), &opts).await.unwrap();
        let Verdict::Error { reason } = &results[0].verdict else {
            panic!("expected ERROR, got {:?}", results[0].verdict)
        };
        assert!(reason.contains("the same address as poolproof"), "{reason}");
        let rows = client.query_one("SELECT count(*) FROM pp_refused", &[]).await.unwrap();
        assert_eq!(rows.get::<_, i64>(0), 0, "a baseline ran");
        client.batch_execute("DROP TABLE pp_refused").await.unwrap();
    }

    #[test]
    fn a_scenario_timeout_overrides_only_the_step_timeout() {
        let cli = Timeouts {
            step: Duration::from_secs(3),
            ..Timeouts::default()
        };
        let mut s: Scenario = toml::from_str("name = 't'\ndescription = 't'\nstep = []\n").unwrap();
        assert_eq!(for_scenario(&cli, &s), cli);
        s.timeout_ms = Some(1500);
        let t = for_scenario(&cli, &s);
        assert_eq!(t.step, Duration::from_millis(1500));
        assert_eq!(Timeouts { step: cli.step, ..t }, cli);
        // Housekeeping waits keep their own timeout, however short the step is.
        assert_eq!(t.housekeeping, cli.housekeeping);
    }

    #[test]
    fn select_keeps_file_order_and_rejects_unknown_names() {
        let mut targets = Targets::single(
            "postgres://u@p/db",
            "postgres://u@d/db",
            PoolMode::Transaction,
            Reset::None,
        )
        .unwrap()
        .targets;
        let mut second = Targets::single(
            "postgres://u@p/db",
            "postgres://u@d/db",
            PoolMode::Transaction,
            Reset::None,
        )
        .unwrap()
        .targets;
        second[0].name = "b".into();
        targets.append(&mut second);
        let names = |v: Vec<&Target>| v.into_iter().map(|t| t.name.clone()).collect::<Vec<_>>();

        assert_eq!(names(select(&targets, &[]).unwrap()), ["pooler", "b"]);
        assert_eq!(
            names(select(&targets, &["b".into(), "pooler".into()]).unwrap()),
            ["pooler", "b"]
        );
        let Err(err) = select(&targets, &["nope".into()]) else {
            panic!("unknown target accepted")
        };
        assert_eq!(err.to_string(), "no target named 'nope'; targets: pooler, b");
    }
}
