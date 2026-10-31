//! The loop over targets and scenarios behind `poolproof run`.

use anyhow::{Result, bail};

use super::{FailKind, Timeouts, Verdict, check, preflight, server_version};
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
    pub check_scenarios: bool,
}

/// Runs every scenario on every target, in order.
pub async fn run(
    targets: &[Target],
    scenarios: &[Scenario],
    opts: &RunOptions<'_>,
) -> Result<(Vec<TargetInfo>, Vec<ResultRow>)> {
    // Not configurable yet; F2 adds the flags.
    let timeouts = Timeouts::default();
    let targets = select(targets, opts.targets)?;
    let wanted = |s: &&Scenario| opts.filter.is_none_or(|f| s.name.contains(f));
    let total = targets.len() * scenarios.iter().filter(wanted).count();
    let mut results = Vec::new();
    let mut target_infos = Vec::new();
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
                let checked = check(
                    s,
                    &target.pooler,
                    &target.direct,
                    &target.reset,
                    &timeouts,
                    opts.check_scenarios,
                )
                .await;
                (checked.verdict, checked.steps, checked.runs, checked.failures)
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
    /// Silent failures and errors the pooler returned where Postgres didn't.
    Error,
    /// Every failure, hangs included.
    #[default]
    Any,
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
