//! The loop over targets and scenarios behind `poolproof run`.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Result, bail};

use super::{FailKind, Timeouts, Verdict, check, deterministic, preflight, server_version};
use crate::output::text::{Style, print_progress, print_result};
use crate::report::{ResultRow, StepOutcomes, TargetInfo};
use crate::scenario::Scenario;
use crate::targets::Target;

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
    // --check-scenarios verdicts by (scenario, direct URL).
    let mut determinism: HashMap<(&str, &str), Result<(), String>> = HashMap::new();
    for target in targets {
        target_infos.push(TargetInfo {
            name: target.name.clone(),
            product: target.product.clone(),
            version: target.version.clone(),
            config: target.config.clone(),
            mode: target.mode,
            postgres: server_version(&target.direct, &timeouts).await.ok(),
        });
        if target.reset.is_empty() {
            eprintln!(
                "warning: target '{}' has no reset; leaked state can carry over between scenarios",
                target.name
            );
        }
        let applies = |s: &&Scenario| wanted(s) && s.modes.contains(&target.mode);
        // Without both URLs answering, every scenario would compare two failures.
        let unreachable = if scenarios.iter().any(|s| applies(&s)) {
            preflight(&target.pooler, &target.direct, &timeouts).await.err()
        } else {
            None
        };
        if let Some(reason) = &unreachable {
            eprintln!("error: target '{}': {reason}", target.name);
        }
        for s in scenarios.iter().filter(wanted) {
            let (verdict, steps, runs, failures) = if !s.modes.contains(&target.mode) {
                let reason = format!("not applicable to {} mode", target.mode);
                (Verdict::Skip { reason }, vec![], 0, 0)
            } else if let Some(reason) = &unreachable {
                (Verdict::Error { reason: reason.clone() }, vec![], 0, 0)
            } else {
                let key = (s.name.as_str(), target.direct.as_str());
                if opts.check_scenarios && !determinism.contains_key(&key) {
                    determinism.insert(
                        key,
                        deterministic(s, &target.direct, &target.reset, &for_scenario(&timeouts, s)).await,
                    );
                }
                match determinism.get(&key) {
                    Some(Err(reason)) => (Verdict::Error { reason: reason.clone() }, vec![], 0, 0),
                    _ => {
                        let checked = check(
                            s,
                            &target.pooler,
                            &target.direct,
                            &target.reset,
                            &for_scenario(&timeouts, s),
                        )
                        .await;
                        (checked.verdict, checked.steps, checked.runs, checked.failures)
                    }
                }
            };
            if opts.text {
                print_result(&target.name, s, &verdict, &steps, runs, failures, opts.style);
            } else {
                print_progress(results.len() + 1, total, &target.name, s, &verdict);
            }
            let steps = steps
                .into_iter()
                .map(|(postgres, pooler)| StepOutcomes { postgres, pooler })
                .collect();
            results.push(ResultRow {
                target: target.name.clone(),
                scenario: s.name.clone(),
                verdict,
                steps,
                runs,
                failures,
            });
        }
    }
    Ok((target_infos, results))
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
fn select<'t>(targets: &'t [Target], names: &[String]) -> Result<Vec<&'t Target>> {
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
        let (client, driver) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.unwrap();
        tokio::spawn(driver);
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
        let mut targets = Targets::single(url.clone(), url.clone(), PoolMode::Transaction).targets;
        let mut second = Targets::single(url.clone(), url.clone(), PoolMode::Transaction).targets;
        second[0].name = "b".into();
        targets.append(&mut second);
        let opts = RunOptions {
            check_scenarios: true,
            ..RunOptions::default()
        };
        let (_, results) = run(&targets, std::slice::from_ref(&s), &opts).await.unwrap();
        assert!(results.iter().all(|r| matches!(r.verdict, Verdict::Pass)));
        let rows = client.query_one("SELECT count(*) FROM pp_runs", &[]).await.unwrap();
        // 2 for the shared check, then baseline + target run on each target.
        assert_eq!(rows.get::<_, i64>(0), 2 + 2 * 2);
        client.batch_execute("DROP TABLE pp_runs").await.unwrap();
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
        let mut targets = Targets::single("p".into(), "d".into(), PoolMode::Transaction).targets;
        let mut second = Targets::single("p".into(), "d".into(), PoolMode::Transaction).targets;
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
