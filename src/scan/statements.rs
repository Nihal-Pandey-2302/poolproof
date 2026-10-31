//! Prepared statement names in a log, and the driver they look like.
//!
//! A driver that prepares statements on the server names them its own way, and Postgres
//! logs the name on every `parse`, `bind` and `execute` line. The names below were seen
//! in poolproof's test bed (asyncpg's with asyncpg 0.30, sqlx's from its source): any
//! app could pick the same names, so a match only says the names look like a driver's.
//! Only these five are recognized. An app's own names, and other drivers' (ActiveRecord's
//! `a1`, ...), give no finding, which doesn't mean they're safe under a pooler.

use super::{Finding, FixCheck, PoolerCheck, PoolerStatus, Severity, StatementName};

/// A driver's statement names.
struct Driver {
    /// As `poolproof drivers` names it, or the driver's own name if it isn't tested.
    name: &'static str,
    /// For the finding: `Statement names like {label}'s`.
    label: &'static str,
    matches: fn(&str) -> bool,
    /// The setting its docs give for transaction pooling.
    fix: &'static str,
    /// The `poolproof drivers` cases about prepared statements: with its defaults, and
    /// with `fix`. None for a driver that isn't tested.
    cases: Option<(&'static [&'static str], &'static [&'static str])>,
}

/// The `poolproof drivers` cases about prepared statements (not `set-leak`).
macro_rules! cases {
    ($variant:literal) => {
        &[
            concat!("driver/", $variant, "/auto-prepare"),
            concat!("driver/", $variant, "/same-names-different-sql"),
            concat!("driver/", $variant, "/same-sql-both-connections"),
            concat!("driver/", $variant, "/transaction"),
        ]
    };
}

const DRIVERS: &[Driver] = &[
    Driver {
        name: "psycopg",
        label: "psycopg 3",
        matches: |n| digits_after(n, "_pg3_"),
        fix: "prepare_threshold = None",
        cases: Some((cases!("psycopg-default"), cases!("psycopg-no-prepare"))),
    },
    Driver {
        name: "pgjdbc",
        label: "pgjdbc",
        matches: |n| digits_after(n, "S_"),
        fix: "prepareThreshold = 0",
        cases: Some((cases!("pgjdbc-default"), cases!("pgjdbc-no-prepare"))),
    },
    Driver {
        name: "pgx",
        label: "pgx v5",
        matches: |n| n.strip_prefix("stmtcache_").is_some_and(|h| h.len() >= 16 && is_hex(h)),
        fix: "QueryExecModeExec (default_query_exec_mode=exec)",
        cases: Some((cases!("pgx-default"), cases!("pgx-exec"))),
    },
    Driver {
        name: "asyncpg",
        label: "asyncpg",
        matches: |n| {
            n.strip_prefix("__asyncpg_stmt_")
                .and_then(|r| r.strip_suffix("__"))
                .is_some_and(|h| !h.is_empty() && is_hex(h))
        },
        fix: "statement_cache_size = 0",
        cases: None,
    },
    Driver {
        name: "sqlx",
        label: "sqlx",
        matches: |n| digits_after(n, "sqlx_s_"),
        fix: ".persistent(false) on its queries",
        cases: None,
    },
];

/// Poolers that prepare statements under their own names, seen in the test bed's logs:
/// behind them, the server never sees the driver's names.
const RENAMING_POOLERS: &[(&str, &str)] = &[
    ("PgBouncer", "PGBOUNCER_"),
    ("PgDog", "__pgdog_"),
    ("Odyssey", "odyssey_pstmt_"),
];

fn digits_after(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

fn is_hex(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// At most three names, long ones cut short, then how many there are in all.
fn examples<'a>(names: impl ExactSizeIterator<Item = &'a str>) -> String {
    let count = names.len();
    let shown: Vec<String> = names
        .take(3)
        .map(|n| match n.char_indices().nth(20) {
            Some((i, _)) => format!("{}...", &n[..i]),
            None => n.to_string(),
        })
        .collect();
    let more = if count > shown.len() { ", ..." } else { "" };
    let names = if count == 1 { "name" } else { "names" };
    format!("{}{more}; {count} {names}", shown.join(", "))
}

/// One finding per driver whose names appear, and a note per renaming pooler.
pub(super) fn findings(names: &[StatementName]) -> (Vec<Finding>, Vec<String>) {
    let mut findings = Vec::new();
    for driver in DRIVERS {
        let seen: Vec<&StatementName> = names.iter().filter(|n| (driver.matches)(&n.name)).collect();
        let Some(first) = seen.iter().min_by_key(|n| n.line) else {
            continue;
        };
        let executions: u64 = seen.iter().map(|n| n.executions).sum();
        let detail = format!(
            "Statement names like {}'s ({}), run {executions} times. A named statement lives on \
             the server connection, so through a transaction pooler it needs the pooler's \
             prepared-statement support. The driver's setting for poolers: {}.",
            driver.label,
            examples(seen.iter().map(|n| n.name.as_str())),
            driver.fix,
        );
        findings.push(Finding {
            rule: "driver-prepared-statements",
            severity: Severity::Warn,
            detail,
            query: first.query.clone(),
            calls: executions,
            line: first.line,
            scenarios: driver.cases.map_or(vec![], |(default, _)| default.to_vec()),
            on_pooler: None,
            driver: Some(driver.name),
            pooler_note: None,
        });
    }
    let notes = RENAMING_POOLERS
        .iter()
        .filter_map(|(pooler, prefix)| {
            let seen: Vec<&str> = names
                .iter()
                .map(|n| n.name.as_str())
                .filter(|n| digits_after(n, prefix))
                .collect();
            (!seen.is_empty()).then(|| {
                format!(
                    "Statements prepared through {pooler} ({}), which renames them, so the driver \
                     behind it is unknown.",
                    examples(seen.iter().copied()),
                )
            })
        })
        .collect();
    (findings, notes)
}

/// `scan --pooler` for a driver finding: what the driver's prepared-statement cases
/// did on `target`, from the built-in driver results, and the same with the driver's
/// setting for poolers. Without results, says so in `pooler_note` and leaves the
/// finding a warning. `postgres` is the version the core results are from; with
/// `pinned` (`--postgres` given) only results from exactly that version count.
pub fn check_driver(finding: &mut Finding, target: &str, postgres: Option<&str>, pinned: bool) {
    let Some(driver) = finding.driver.and_then(|d| DRIVERS.iter().find(|x| x.name == d)) else {
        return;
    };
    let Some((default, fix)) = driver.cases else {
        finding.pooler_note = Some(format!("no driver results for {}", driver.name));
        return;
    };
    let Some((pg, results)) = crate::report::driver_results(target, postgres, pinned) else {
        let on = match (postgres, pinned) {
            (Some(pg), true) => format!("{target} on Postgres {pg}"),
            _ => target.to_string(),
        };
        finding.pooler_note = Some(format!("no driver results for {} on {on}", driver.name));
        return;
    };
    let pick = |cases: &[&str]| -> Vec<(String, String)> {
        results
            .iter()
            .filter(|(s, _)| cases.contains(s))
            .map(|(s, r)| (s.to_string(), r.clone()))
            .collect()
    };
    let Some(status) = status(&pick(default)) else {
        finding.pooler_note = Some(format!("no driver results for {} on {target}", driver.name));
        return;
    };
    finding.on_pooler = Some(PoolerCheck {
        target: target.to_string(),
        postgres: Some(pg),
        status,
        scenarios: pick(default),
        documented: vec![],
        fix: status_of_fix(&pick(fix), driver.fix),
    });
}

fn status_of_fix(results: &[(String, String)], setting: &str) -> Option<FixCheck> {
    Some(FixCheck {
        setting: setting.to_string(),
        status: status(results)?,
    })
}

/// As for the other rules: any silent FAIL breaks, any other FAIL is rejected.
fn status(results: &[(String, String)]) -> Option<PoolerStatus> {
    if results.is_empty() {
        None
    } else if results.iter().any(|(_, r)| r == "silent") {
        Some(PoolerStatus::Breaks)
    } else if results.iter().any(|(_, r)| r != "pass") {
        Some(PoolerStatus::Rejected)
    } else {
        Some(PoolerStatus::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(name: &str, executions: u64, line: usize) -> StatementName {
        StatementName {
            name: name.into(),
            executions,
            line,
            query: format!("SELECT {line}"),
        }
    }

    #[test]
    fn names_as_seen_in_the_test_bed() {
        let driver = |n: &str| DRIVERS.iter().find(|d| (d.matches)(n)).map(|d| d.name);
        assert_eq!(driver("_pg3_0"), Some("psycopg"));
        assert_eq!(driver("S_1"), Some("pgjdbc"));
        assert_eq!(
            driver("stmtcache_f511a4ab4af9922ce03043515e8726414f332f6366c5eaca"),
            Some("pgx")
        );
        assert_eq!(driver("__asyncpg_stmt_1__"), Some("asyncpg"));
        assert_eq!(driver("__asyncpg_stmt_1a__"), Some("asyncpg"));
        assert_eq!(driver("sqlx_s_12"), Some("sqlx"));
        // An app's own names, other drivers' and near misses.
        for other in [
            "pp_7b9b071af95b",
            "a1",
            "s0",
            "S_",
            "S_x",
            "_pg3_",
            "stmtcache_xyz",
            "my_stmt",
            "PGBOUNCER_1",
        ] {
            assert_eq!(driver(other), None, "{other}");
        }
    }

    #[test]
    fn one_finding_per_driver_and_a_note_per_renaming_pooler() {
        let names = [
            name("S_2", 4, 9),
            name("S_1", 4, 3),
            name("_pg3_0", 3, 5),
            name("pp_mine", 8, 1),
            name("PGBOUNCER_1", 3, 2),
            name("__pgdog_4", 1, 7),
        ];
        let (findings, notes) = findings(&names);
        let jdbc = findings.iter().find(|f| f.driver == Some("pgjdbc")).unwrap();
        assert_eq!((jdbc.calls, jdbc.line, jdbc.query.as_str()), (8, 3, "SELECT 3"));
        assert!(
            jdbc.detail
                .starts_with("Statement names like pgjdbc's (S_2, S_1; 2 names), run 8 times."),
            "{}",
            jdbc.detail
        );
        assert_eq!(jdbc.scenarios.len(), 4);
        assert_eq!(findings.len(), 2, "pp_mine is an app's own name");
        assert_eq!(notes.len(), 2);
        assert!(notes[0].starts_with("Statements prepared through PgBouncer (PGBOUNCER_1; 1 name), which"));
        let many: Vec<String> = (1..=5).map(|i| format!("S_{i}")).collect();
        assert_eq!(examples(many.iter().map(String::as_str)), "S_1, S_2, S_3, ...; 5 names");
    }

    #[test]
    fn case_names_are_real_driver_scenarios() {
        let all: Vec<String> = crate::drivers::scenarios(&[])
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        for driver in DRIVERS {
            assert_eq!(
                driver.cases.is_some(),
                crate::drivers::names().iter().any(|n| n == driver.name)
            );
            for case in driver.cases.iter().flat_map(|(a, b)| a.iter().chain(b.iter())) {
                assert!(all.iter().any(|s| s == case), "{case}");
            }
        }
    }

    /// The expected verdicts come from the committed driver results, so regenerating
    /// them can't leave this test checking stale values.
    #[test]
    fn pooler_checks_follow_the_driver_results() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("results/drivers-postgres-17.11.json");
        let report = crate::report::Report::load(&root).unwrap();
        let expected = |target: &str| {
            let results: Vec<(String, String)> = report
                .results
                .iter()
                .filter(|r| r.target == target && cases!("pgx-default").contains(&r.scenario.as_str()))
                .filter_map(|r| match &r.verdict {
                    crate::runner::Verdict::Pass => Some((r.scenario.clone(), "pass".to_string())),
                    crate::runner::Verdict::Fail { mismatch } => Some((r.scenario.clone(), mismatch.kind.to_string())),
                    _ => None,
                })
                .collect();
            status(&results)
        };
        let (mut found, _) = findings(&[name("stmtcache_f511a4ab4af9922ce03043515e87", 8, 1)]);
        let pgx = &mut found[0];
        for target in ["supavisor-2.10.0-rc.1", "pgdog-0.1.62"] {
            check_driver(pgx, target, Some("17.11"), false);
            let check = pgx.on_pooler.take().unwrap();
            assert_eq!(Some(check.status), expected(target), "{target}");
            assert_eq!(
                check.fix.unwrap().setting,
                "QueryExecModeExec (default_query_exec_mode=exec)"
            );
        }
        // What the drivers report says today, to read the test by.
        assert_eq!(expected("supavisor-2.10.0-rc.1"), Some(PoolerStatus::Rejected));
        assert_eq!(expected("pgdog-0.1.62"), Some(PoolerStatus::Ok));

        // A driver without cases, or a target without results: a note, still a warning.
        let (mut asyncpg, _) = findings(&[name("__asyncpg_stmt_1__", 3, 1)]);
        check_driver(&mut asyncpg[0], "pgdog-0.1.62", Some("17.11"), false);
        assert!(asyncpg[0].on_pooler.is_none());
        assert_eq!(asyncpg[0].pooler_note.as_deref(), Some("no driver results for asyncpg"));
        let (mut pgx, _) = findings(&[name("stmtcache_f511a4ab4af9922ce03043515e87", 8, 1)]);
        check_driver(&mut pgx[0], "pgbouncer-1.24.1", None, false);
        assert_eq!(
            pgx[0].pooler_note.as_deref(),
            Some("no driver results for pgx on pgbouncer-1.24.1")
        );
        check_driver(&mut pgx[0], "pgdog-0.1.62", Some("18.4"), true);
        assert_eq!(
            pgx[0].pooler_note.as_deref(),
            Some("no driver results for pgx on pgdog-0.1.62 on Postgres 18.4")
        );
    }
}
