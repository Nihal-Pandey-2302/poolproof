//! The loop over targets and scenarios behind `poolproof run`.

use anyhow::Result;

use super::{DIRECT_UNREACHABLE, Privileges, Timeouts, Verdict, check, preflight, privileges, server_version};
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
    // Not configurable yet; F2 adds the flags.
    let timeouts = Timeouts::default();
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
        let applies = |s: &Scenario| filter.is_none_or(|f| s.name.contains(f)) && s.modes.contains(&target.mode);
        // Without both URLs answering, every scenario would compare two failures.
        let unreachable = if scenarios.iter().any(applies) {
            preflight(&target.pooler, &target.direct, &timeouts).await.err()
        } else {
            None
        };
        if let Some(reason) = &unreachable {
            eprintln!("error: target '{}': {reason}", target.name);
        }
        let privs = match unreachable {
            Some(_) => Err(String::new()),
            None => privileges(&target.direct, &timeouts)
                .await
                .map_err(|e| format!("{DIRECT_UNREACHABLE} privilege check: {e:#}")),
        };
        if let Ok(p) = &privs
            && !p.superuser
            && !p.pg_signal_backend
            && target
                .reset
                .iter()
                .any(|s| s.sql().to_lowercase().contains("pg_terminate_backend"))
        {
            eprintln!(
                "warning: target '{}': reset terminates sessions, but the direct user has neither superuser \
                 nor pg_signal_backend, so it can only end sessions of its own role (and only a superuser \
                 can end a superuser's)",
                target.name
            );
        }
        for s in scenarios {
            if filter.is_some_and(|f| !s.name.contains(f)) {
                continue;
            }
            let (verdict, steps, runs, failures) = match before_run(s, target, unreachable.as_deref(), &privs) {
                Some(verdict) => (verdict, vec![], 0, 0),
                None => {
                    let checked = check(s, &target.pooler, &target.direct, &target.reset, &timeouts).await;
                    (checked.verdict, checked.steps, checked.runs, checked.failures)
                }
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

/// The verdict for a scenario that won't run on this target, or `None` to run it.
/// `privs` is the direct user's privileges, or why they couldn't be read.
fn before_run(
    s: &Scenario,
    target: &Target,
    unreachable: Option<&str>,
    privs: &Result<Privileges, String>,
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
    if s.requires.is_empty() {
        return None;
    }
    match privs {
        Ok(privs) => privs.missing(&s.requires).map(|reason| Verdict::Skip { reason }),
        Err(reason) => Some(Verdict::Error { reason: reason.clone() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::Targets;

    #[test]
    fn unmet_requirements_skip_before_running() {
        let target = Targets::single(
            "postgres://u@127.0.0.1:1/db",
            "postgres://u@127.0.0.1:1/db",
            crate::scenario::PoolMode::Transaction,
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
        };
        let creator = Privileges {
            createrole: true,
            ..plain
        };
        let verdict = |s: &Scenario, privs: Result<Privileges, String>| before_run(s, &target, None, &privs);

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
            before_run(&roles, &target, Some("down"), &Ok(creator)),
            Some(Verdict::Error { reason }) if reason == "down"
        ));
    }
}
