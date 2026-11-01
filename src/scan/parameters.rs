//! Which parameter a `session-set` verdict is about. The SET scenarios each set one
//! parameter, mostly work_mem, while poolers treat parameters differently: PgBouncer
//! keeps a few per client and leaks the rest. So a scenario's result only stands for the
//! finding's parameter when the scenario sets it, or when the pooler doesn't track
//! either.

use super::Finding;

/// The parameter each SET scenario sets.
fn scenario_parameter(scenario: &str) -> Option<&'static str> {
    match scenario {
        "set-isolated-between-clients" | "set-does-not-leak-after-disconnect" | "set-survives-server-change" => {
            Some("work_mem")
        }
        "search-path-isolated-between-clients" => Some("search_path"),
        s if s.starts_with("set-role-") => Some("role"),
        _ => None,
    }
}

/// The parameters a pooler keeps per client by default, from its docs: (product, the
/// first version the list was checked for, the docs, the list). A later version uses the
/// last list at or below it. Each parameter comes with the first Postgres major that
/// reports it (0: all), since a pooler only sees a SET of a parameter Postgres reports.
#[allow(clippy::type_complexity)]
const TRACKED: &[(&str, &str, &str, &[(&str, u64)])] = &[
    (
        "pgbouncer",
        "1.24.1",
        "https://github.com/pgbouncer/pgbouncer/blob/pgbouncer_1_24_1/doc/config.md#track_extra_parameters",
        // The five it always tracks, and track_extra_parameters' default, IntervalStyle.
        &[
            ("application_name", 0),
            ("client_encoding", 0),
            ("datestyle", 0),
            ("intervalstyle", 0),
            ("standard_conforming_strings", 0),
            ("timezone", 0),
        ],
    ),
    (
        "pgbouncer",
        "1.26.1",
        "https://github.com/pgbouncer/pgbouncer/blob/pgbouncer_1_26_1/doc/config.md#track_extra_parameters",
        &[
            ("application_name", 0),
            ("client_encoding", 0),
            ("datestyle", 0),
            ("default_transaction_read_only", 14),
            ("intervalstyle", 0),
            ("scram_iterations", 16),
            ("search_path", 18),
            ("session_authorization", 0),
            ("standard_conforming_strings", 0),
            ("timezone", 0),
        ],
    ),
];

/// The docs that say `product` `version` keeps `parameter` per client on `postgres`.
fn tracked_by(product: &str, version: &str, postgres: Option<&str>, parameter: &str) -> Option<&'static str> {
    let (_, _, doc, params) = TRACKED
        .iter()
        .rfind(|(p, from, ..)| *p == product && crate::version::cmp(from, version).is_le())?;
    let major = postgres.and_then(|pg| pg.split('.').next()?.parse::<u64>().ok());
    params
        .iter()
        .any(|&(name, since)| name == parameter && (since == 0 || major.is_some_and(|m| m >= since)))
        .then_some(*doc)
}

/// The scenarios to judge a finding by. For a SET: the ones that set its parameter, since
/// the others measure how the pooler treats another one. When none does, the ones of the
/// parameter that stands in for it, the first scenario's. Otherwise all of them.
pub fn scenarios_for_parameter(finding: &Finding) -> Vec<&'static str> {
    let Some(parameter) = finding.parameter.as_deref() else {
        return finding.scenarios.clone();
    };
    let setting = |p: &str| -> Vec<&'static str> {
        finding
            .scenarios
            .iter()
            .copied()
            .filter(|s| scenario_parameter(s) == Some(p))
            .collect()
    };
    match setting(parameter) {
        own if !own.is_empty() => own,
        _ => match stand_in(finding) {
            Some(other) => setting(other),
            None => finding.scenarios.clone(),
        },
    }
}

/// The parameter the first of a finding's scenarios sets: what its verdict is measured
/// with when no scenario sets the finding's own.
fn stand_in(finding: &Finding) -> Option<&'static str> {
    finding.scenarios.iter().find_map(|s| scenario_parameter(s))
}

/// Checks a `session-set` finding's `--pooler` verdict against its parameter. When none of
/// the scenarios sets it, a pooler whose docs say it tracks it by default gets no verdict
/// and a note with its docs (for another config, that the config may change it);
/// otherwise the verdict stays and says which parameter it was measured with.
pub fn check_parameter(
    finding: &mut Finding,
    product: Option<&str>,
    version: Option<&str>,
    postgres: Option<&str>,
    config: Option<&str>,
) {
    let Some(parameter) = finding.parameter.as_deref() else {
        return;
    };
    let tested: Vec<&str> = finding.scenarios.iter().filter_map(|s| scenario_parameter(s)).collect();
    if tested.is_empty() || tested.contains(&parameter) {
        return;
    }
    if let (Some(product), Some(version)) = (product, version)
        && let Some(doc) = tracked_by(product, version, postgres, parameter)
    {
        let target = finding
            .on_pooler
            .take()
            .map_or_else(|| product.to_string(), |c| c.target);
        let name = if product == "pgbouncer" { "PgBouncer" } else { product };
        finding.pooler_note = Some(match config {
            None | Some("default") => {
                let own = if product == "pgbouncer" {
                    " Those docs say they are \"restored in the server whenever the client \
                     becomes active\", so a server log can show SETs of them that the app never ran."
                } else {
                    ""
                };
                format!(
                    "{target} keeps {parameter} per client by default ({doc}). No poolproof \
                     scenario sets {parameter}, so there is no verdict for it.{own}"
                )
            }
            Some(config) => format!(
                "{target}: by default {name} keeps {parameter} per client ({doc}); this target's \
                 config ({config}) may change that, and no poolproof scenario sets {parameter}, \
                 so there is no verdict for it."
            ),
        });
        return;
    }
    let other = stand_in(finding);
    if let (Some(check), Some(other)) = (&mut finding.on_pooler, other) {
        check.stand_in = Some(other.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{PoolerCheck, PoolerStatus, Severity};

    fn finding(parameter: &str, scenarios: Vec<&'static str>) -> Finding {
        Finding {
            rule: "session-set",
            severity: Severity::Warn,
            detail: String::new(),
            query: format!("SET {parameter} = 'x'"),
            calls: 1,
            line: 1,
            scenarios,
            on_pooler: Some(PoolerCheck {
                target: "pgbouncer-1.26.1".into(),
                postgres: Some("17.11".into()),
                status: PoolerStatus::Breaks,
                scenarios: vec![("set-isolated-between-clients".into(), "silent".into())],
                documented: vec![],
                fix: None,
                other_postgres: false,
                stand_in: None,
            }),
            driver: None,
            pooler_note: None,
            parameter: Some(parameter.into()),
            differs_on: vec![],
        }
    }

    const WORK_MEM: [&str; 3] = [
        "set-isolated-between-clients",
        "set-does-not-leak-after-disconnect",
        "set-survives-server-change",
    ];

    #[test]
    fn tracked_parameter_gets_no_work_mem_verdict() {
        let mut f = finding("application_name", WORK_MEM.to_vec());
        check_parameter(&mut f, Some("pgbouncer"), Some("1.26.1"), Some("17.11"), None);
        assert!(f.on_pooler.is_none());
        let note = f.pooler_note.unwrap();
        assert!(
            note.starts_with("pgbouncer-1.26.1 keeps application_name per client"),
            "{note}"
        );
        assert!(note.contains("pgbouncer_1_26_1/doc/config.md"), "{note}");
        assert!(
            note.contains("restored in the server whenever the client becomes active"),
            "{note}"
        );
    }

    #[test]
    fn untracked_parameter_says_what_was_measured() {
        let mut f = finding("statement_timeout", WORK_MEM.to_vec());
        check_parameter(&mut f, Some("pgbouncer"), Some("1.26.1"), Some("17.11"), None);
        let check = f.on_pooler.unwrap();
        assert_eq!(check.status, PoolerStatus::Breaks);
        assert_eq!(check.stand_in.as_deref(), Some("work_mem"));
        assert!(f.pooler_note.is_none());
        // Measured directly: no note.
        let mut f = finding("work_mem", WORK_MEM.to_vec());
        check_parameter(&mut f, Some("pgbouncer"), Some("1.26.1"), Some("17.11"), None);
        assert!(f.on_pooler.unwrap().stand_in.is_none());
    }

    #[test]
    fn a_scenario_for_the_parameter_wins_over_the_docs() {
        // 1.26.1 tracks search_path on Postgres 18, and a scenario measures it there.
        let mut f = finding("search_path", vec!["search-path-isolated-between-clients", WORK_MEM[2]]);
        check_parameter(&mut f, Some("pgbouncer"), Some("1.26.1"), Some("18.4"), None);
        assert!(f.on_pooler.unwrap().stand_in.is_none());
    }

    #[test]
    fn another_config_may_change_the_docs_list() {
        let mut f = finding("application_name", WORK_MEM.to_vec());
        check_parameter(&mut f, Some("pgbouncer"), Some("1.26.1"), Some("17.11"), Some("pool4"));
        assert!(f.on_pooler.is_none());
        assert_eq!(
            f.pooler_note.unwrap(),
            "pgbouncer-1.26.1: by default PgBouncer keeps application_name per client \
             (https://github.com/pgbouncer/pgbouncer/blob/pgbouncer_1_26_1/doc/config.md#track_extra_parameters); \
             this target's config (pool4) may change that, and no poolproof scenario sets \
             application_name, so there is no verdict for it."
        );
        // A parameter that isn't on the list keeps the stand-in verdict.
        let mut f = finding("statement_timeout", WORK_MEM.to_vec());
        check_parameter(&mut f, Some("pgbouncer"), Some("1.26.1"), Some("17.11"), Some("pool4"));
        assert_eq!(f.on_pooler.unwrap().stand_in.as_deref(), Some("work_mem"));
    }

    #[test]
    fn scenarios_that_set_the_parameter_are_the_ones_judged() {
        let f = finding("search_path", vec!["search-path-isolated-between-clients", WORK_MEM[2]]);
        assert_eq!(
            scenarios_for_parameter(&f),
            vec!["search-path-isolated-between-clients"]
        );
        let f = finding(
            "role",
            vec![
                "set-role-isolated-between-clients",
                "set-role-reset-after-pinned-disconnect",
                WORK_MEM[2],
            ],
        );
        assert_eq!(
            scenarios_for_parameter(&f),
            vec![
                "set-role-isolated-between-clients",
                "set-role-reset-after-pinned-disconnect"
            ]
        );
        // None sets it: the stand-in's, the first scenario's parameter.
        let f = finding("statement_timeout", WORK_MEM.to_vec());
        assert_eq!(scenarios_for_parameter(&f), WORK_MEM.to_vec());
        let f = finding(
            "session_authorization",
            vec![
                "set-role-isolated-between-clients",
                "set-role-reset-after-pinned-disconnect",
                WORK_MEM[2],
            ],
        );
        assert_eq!(
            scenarios_for_parameter(&f),
            vec![
                "set-role-isolated-between-clients",
                "set-role-reset-after-pinned-disconnect"
            ]
        );
    }

    #[test]
    fn tracked_lists_follow_the_versions() {
        let tracked = |version, pg, parameter| tracked_by("pgbouncer", version, pg, parameter).is_some();
        assert!(tracked("1.24.1", Some("17.11"), "application_name"));
        assert!(tracked("1.25.2", Some("17.11"), "intervalstyle"));
        // session_authorization is on the list from 1.26.1.
        assert!(!tracked("1.25.2", Some("17.11"), "session_authorization"));
        assert!(tracked("1.26.1", Some("17.11"), "session_authorization"));
        assert!(tracked("1.27.0", Some("17.11"), "session_authorization"));
        // Only where Postgres reports it.
        assert!(!tracked("1.26.1", Some("15.8"), "scram_iterations"));
        assert!(tracked("1.26.1", Some("16.4"), "scram_iterations"));
        assert!(!tracked("1.26.1", None, "scram_iterations"));
        // Older versions and other poolers: not checked, so nothing is assumed.
        assert!(!tracked("1.23.1", Some("17.11"), "application_name"));
        assert!(tracked_by("pgdog", "0.1.62", Some("17.11"), "application_name").is_none());
        assert!(!tracked("1.26.1", Some("17.11"), "work_mem"));
    }

    #[test]
    fn session_authorization_on_pgbouncer_is_not_judged_by_set_role() {
        let mut f = finding(
            "session_authorization",
            vec!["set-role-isolated-between-clients", WORK_MEM[2]],
        );
        check_parameter(&mut f, Some("pgbouncer"), Some("1.26.1"), Some("17.11"), None);
        assert!(f.on_pooler.is_none());
        // Elsewhere SET ROLE stands in for it.
        let mut f = finding(
            "session_authorization",
            vec!["set-role-isolated-between-clients", WORK_MEM[2]],
        );
        check_parameter(&mut f, Some("pgdog"), Some("0.1.62"), Some("17.11"), None);
        assert_eq!(f.on_pooler.unwrap().stand_in.as_deref(), Some("role"));
    }
}
