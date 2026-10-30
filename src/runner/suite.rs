//! The loop over targets and scenarios behind `poolproof run`.

use anyhow::Result;

use super::{Verdict, check, preflight, server_version};
use crate::output::text::print_result;
use crate::report::{ResultRow, StepOutcomes, TargetInfo};
use crate::scenario::Scenario;
use crate::targets::Target;

/// Runs every scenario on every target, in order. `text` prints each result as it
/// comes in; `verbose` adds every step's outcomes to that.
pub async fn run(
    targets: &[Target],
    scenarios: &[Scenario],
    filter: Option<&str>,
    text: bool,
    verbose: bool,
) -> Result<(Vec<TargetInfo>, Vec<ResultRow>)> {
    let mut results = Vec::new();
    let mut target_infos = Vec::new();
    for target in targets {
        target_infos.push(TargetInfo {
            name: target.name.clone(),
            product: target.product.clone(),
            version: target.version.clone(),
            config: target.config.clone(),
            mode: target.mode,
            postgres: server_version(&target.direct).await.ok(),
        });
        if target.reset.is_empty() {
            eprintln!(
                "warning: target '{}' has no reset; leaked state can carry over between scenarios",
                target.name
            );
        }
        let applies = |s: &Scenario| filter.is_none_or(|f| s.name.contains(f)) && s.modes.contains(&target.mode);
        // Without both URLs answering, every scenario would compare two failures.
        let unreachable = if scenarios.iter().any(applies) {
            preflight(&target.pooler, &target.direct).await.err()
        } else {
            None
        };
        if let Some(reason) = &unreachable {
            eprintln!("error: target '{}': {reason}", target.name);
        }
        for s in scenarios {
            if filter.is_some_and(|f| !s.name.contains(f)) {
                continue;
            }
            let (verdict, steps, runs, failures) = if !s.modes.contains(&target.mode) {
                let reason = format!("not applicable to {} mode", target.mode);
                (Verdict::Skip { reason }, vec![], 0, 0)
            } else if let Some(reason) = &unreachable {
                (Verdict::Error { reason: reason.clone() }, vec![], 0, 0)
            } else {
                let checked = check(s, &target.pooler, &target.direct, &target.reset).await;
                (checked.verdict, checked.steps, checked.runs, checked.failures)
            };
            if text {
                print_result(&target.name, s, &verdict, &steps, runs, failures, verbose);
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
