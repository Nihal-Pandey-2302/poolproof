//! pp-selftest-check EXPECTED.toml REPORT.json SCENARIOS_DIR
//!
//! Compares a `poolproof run --format json` report of the self-test targets with the
//! expected verdicts. Any other verdict, any target or scenario the expected file
//! doesn't cover, or a missing result for any target and scenario in SCENARIOS_DIR,
//! exits 1. The scenario list comes from the directory, not the report: a report from
//! `run --filter` only lists the scenarios it ran.

use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

#[derive(Deserialize)]
struct Expected {
    targets: BTreeMap<String, TargetExpected>,
}

#[derive(Deserialize)]
struct TargetExpected {
    default: Want,
    #[serde(flatten)]
    scenarios: BTreeMap<String, Want>,
}

#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum Want {
    Short(String),
    Full {
        result: String,
        reason_prefix: Option<String>,
        #[allow(dead_code)]
        why: Option<String>,
    },
}

impl Want {
    fn result(&self) -> &str {
        match self {
            Want::Short(r) | Want::Full { result: r, .. } => r,
        }
    }

    fn reason_prefix(&self) -> Option<&str> {
        match self {
            Want::Full { reason_prefix, .. } => reason_prefix.as_deref(),
            Want::Short(_) => None,
        }
    }
}

const RESULTS: [&str; 6] = ["pass", "silent", "error", "hang", "harness-error", "skip"];

/// The results that carry a reason, so a row can check it with `reason_prefix`.
const WITH_REASON: [&str; 2] = ["harness-error", "skip"];

/// The verdict as the expected file names it, plus the reason of an ERROR or SKIP.
fn got(r: &Value) -> (String, String) {
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    match r["result"].as_str().unwrap_or("") {
        "fail" => (text(&r["mismatch"]["kind"]), String::new()),
        "error" => ("harness-error".into(), text(&r["reason"])),
        // Checked too, so a SKIP for another reason (the probe failed instead of the
        // precondition not being met) doesn't pass as expected.
        "skip" => ("skip".into(), text(&r["reason"])),
        other => (other.to_string(), String::new()),
    }
}

fn check(expected: &Expected, report: &Value, scenario_names: &BTreeSet<String>) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, t) in &expected.targets {
        let rows = std::iter::once(("default", &t.default)).chain(t.scenarios.iter().map(|(s, w)| (s.as_str(), w)));
        for (row, want) in rows {
            if !RESULTS.contains(&want.result()) {
                problems.push(format!("{name} / {row}: unknown expected result {:?}", want.result()));
            }
            if want.reason_prefix().is_some() && !WITH_REASON.contains(&want.result()) {
                problems.push(format!(
                    "{name} / {row}: reason_prefix on a {} row, which has no reason to check",
                    want.result()
                ));
            }
        }
    }
    for (target, t) in &expected.targets {
        for s in t.scenarios.keys() {
            if !scenario_names.contains(s) {
                problems.push(format!("{target}: expected file names unknown scenario {s}"));
            }
        }
    }

    let mut seen = BTreeSet::new();
    for r in report["results"].as_array().into_iter().flatten() {
        let target = r["target"].as_str().unwrap_or("");
        let scenario = r["scenario"].as_str().unwrap_or("");
        seen.insert((target, ""));
        seen.insert((target, scenario));
        if !scenario_names.contains(scenario) {
            problems.push(format!(
                "{target} / {scenario}: not a scenario in the scenarios directory"
            ));
            continue;
        }
        let Some(t) = expected.targets.get(target) else {
            problems.push(format!("{target}: not in the expected file"));
            continue;
        };
        let want = t.scenarios.get(scenario).unwrap_or(&t.default);
        let (result, reason) = got(r);
        let runs = r["runs"].as_u64().unwrap_or(1);
        let failures = r["failures"].as_u64().unwrap_or(0);
        let mut wrong = result != want.result();
        if let Some(prefix) = want.reason_prefix() {
            wrong |= !reason.starts_with(prefix);
        }
        // A repeated scenario that should pass must pass every run; one expected to
        // fail counts as failing if any run did, which is how `run` reports it.
        if want.result() == "pass" && failures > 0 {
            wrong = true;
        }
        if wrong {
            let mut line = format!("{target} / {scenario}: expected {}, got {result}", want.result());
            if runs > 1 {
                line.push_str(&format!(" ({failures} of {runs} runs failed)"));
            }
            if !reason.is_empty() {
                line.push_str(&format!(": {reason}"));
            }
            problems.push(line);
        }
    }
    // A partial report (e.g. from --filter) must not pass as complete.
    for target in expected.targets.keys() {
        if !seen.contains(&(target.as_str(), "")) {
            problems.push(format!("{target}: no results in the report"));
            continue;
        }
        for s in scenario_names {
            if !seen.contains(&(target.as_str(), s.as_str())) {
                problems.push(format!("{target} / {s}: no result in the report"));
            }
        }
    }
    problems
}

/// The `name` of every scenario file in a directory.
fn scenario_names(dir: &str) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|x| x == "toml") {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let table: toml::Table = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let name = table.get("name").and_then(|n| n.as_str());
            names.insert(name.ok_or(format!("{}: no name", path.display()))?.to_string());
        }
    }
    if names.is_empty() {
        return Err("no scenario files".into());
    }
    Ok(names)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [expected, report, dir] = args.as_slice() else {
        eprintln!("usage: pp-selftest-check EXPECTED.toml REPORT.json SCENARIOS_DIR");
        return ExitCode::from(2);
    };
    let scenarios = match scenario_names(dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{dir}: {e}");
            return ExitCode::from(2);
        }
    };
    let expected: Expected = match std::fs::read_to_string(expected)
        .map_err(|e| e.to_string())
        .and_then(|s| toml::from_str(&s).map_err(|e| e.to_string()))
    {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{expected}: {e}");
            return ExitCode::from(2);
        }
    };
    let report: Value = match std::fs::read_to_string(report)
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{report}: {e}");
            return ExitCode::from(2);
        }
    };
    let problems = check(&expected, &report, &scenarios);
    for p in &problems {
        println!("{p}");
    }
    let total = report["results"].as_array().map_or(0, Vec::len);
    if problems.is_empty() {
        println!("self-test: all {total} verdicts as expected");
        ExitCode::SUCCESS
    } else {
        println!("self-test: {} unexpected", problems.len());
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn expected(s: &str) -> Expected {
        toml::from_str(s).unwrap()
    }

    fn report(results: Value) -> Value {
        json!({ "scenarios": [{"name": "a"}, {"name": "b"}], "results": results })
    }

    fn names() -> BTreeSet<String> {
        ["a", "b"].map(String::from).into()
    }

    const FILE: &str = r#"
        [targets.t]
        default = "pass"
        b = { result = "silent", why = "tag rewritten" }
        [targets.down]
        default = { result = "harness-error", reason_prefix = "target unreachable: pooler:" }
    "#;

    #[test]
    fn matching_report_is_clean() {
        let r = report(json!([
            {"target": "t", "scenario": "a", "result": "pass", "runs": 1, "failures": 0},
            {"target": "t", "scenario": "b", "result": "fail", "mismatch": {"kind": "silent"}, "runs": 1, "failures": 1},
            {"target": "down", "scenario": "a", "result": "error", "reason": "target unreachable: pooler: refused"},
            {"target": "down", "scenario": "b", "result": "error", "reason": "target unreachable: pooler: refused"},
        ]));
        assert_eq!(check(&expected(FILE), &r, &names()), Vec::<String>::new());
    }

    #[test]
    fn reports_every_difference() {
        let r = report(json!([
            {"target": "t", "scenario": "a", "result": "fail", "mismatch": {"kind": "hang"}, "runs": 1, "failures": 1},
            {"target": "t", "scenario": "b", "result": "pass", "runs": 1, "failures": 0},
            {"target": "down", "scenario": "a", "result": "error", "reason": "baseline failed at step 1"},
            {"target": "down", "scenario": "b", "result": "error", "reason": "target unreachable: pooler: x"},
            {"target": "extra", "scenario": "a", "result": "pass"},
        ]));
        let p = check(&expected(FILE), &r, &names());
        assert_eq!(p.len(), 4, "{p:?}");
        assert!(p[0].starts_with("t / a: expected pass, got hang"));
    }

    #[test]
    fn flaky_pass_is_a_failure() {
        let r = report(json!([
            {"target": "t", "scenario": "a", "result": "pass", "runs": 5, "failures": 1},
            {"target": "t", "scenario": "b", "result": "fail", "mismatch": {"kind": "silent"}},
            {"target": "down", "scenario": "a", "result": "error", "reason": "target unreachable: pooler: x"},
            {"target": "down", "scenario": "b", "result": "error", "reason": "target unreachable: pooler: x"},
        ]));
        assert_eq!(check(&expected(FILE), &r, &names()).len(), 1);
    }

    #[test]
    fn filtered_report_is_incomplete() {
        // `run --filter a` lists only scenario a in the report itself.
        let r = json!({ "scenarios": [{"name": "a"}], "results": [
            {"target": "t", "scenario": "a", "result": "pass"},
            {"target": "down", "scenario": "a", "result": "error", "reason": "target unreachable: pooler: x"},
        ]});
        let p = check(&expected(FILE), &r, &names());
        assert_eq!(
            p,
            vec!["down / b: no result in the report", "t / b: no result in the report"]
        );
    }

    #[test]
    fn unknown_scenario_in_report() {
        let r = report(json!([{"target": "t", "scenario": "zz", "result": "pass"}]));
        let p = check(&expected(FILE), &r, &names());
        assert!(
            p.iter()
                .any(|l| l == "t / zz: not a scenario in the scenarios directory"),
            "{p:?}"
        );
    }

    #[test]
    fn partial_report_is_incomplete() {
        let r = report(json!([
            {"target": "t", "scenario": "a", "result": "pass"},
            {"target": "down", "scenario": "a", "result": "error", "reason": "target unreachable: pooler: x"},
        ]));
        let p = check(&expected(FILE), &r, &names());
        assert_eq!(
            p,
            vec!["down / b: no result in the report", "t / b: no result in the report"]
        );
    }

    #[test]
    fn skip_rows_check_the_reason() {
        let e = expected(
            r#"
            [targets.t]
            default = { result = "skip", reason_prefix = "precondition at step" }
            b = { result = "skip", why = "no prefix: any reason" }
            "#,
        );
        let skip = |scenario: &str, reason: &str| json!({"target": "t", "scenario": scenario, "result": "skip", "reason": reason});
        let ok = report(json!([
            skip("a", "precondition at step 4 not met"),
            skip("b", "server connections unknown")
        ]));
        assert_eq!(check(&e, &ok, &names()), Vec::<String>::new());
        let wrong = report(json!([
            skip("a", "server connections unknown (probe failed)"),
            skip("b", "x")
        ]));
        assert_eq!(
            check(&e, &wrong, &names()),
            vec!["t / a: expected skip, got skip: server connections unknown (probe failed)"]
        );
    }

    #[test]
    fn reason_prefix_only_on_results_with_a_reason() {
        let e = expected(
            "[targets.t]\ndefault = { result = \"pass\", reason_prefix = \"y\" }\nb = { result = \"silent\", reason_prefix = \"x\" }\n",
        );
        let r = report(json!([{"target": "t", "scenario": "a", "result": "pass"}]));
        let p = check(&e, &r, &names());
        for row in [
            "t / default: reason_prefix on a pass row",
            "t / b: reason_prefix on a silent row",
        ] {
            let line = format!("{row}, which has no reason to check");
            assert!(p.contains(&line), "{line} in {p:?}");
        }
    }

    #[test]
    fn unknown_names_in_expected_file() {
        let e = expected("[targets.t]\ndefault = \"pass\"\nnope = \"silent\"\nb = \"wrong\"\n");
        let r = report(json!([{"target": "t", "scenario": "a", "result": "pass"}]));
        let p = check(&e, &r, &names());
        assert!(p.iter().any(|l| l.contains("unknown scenario nope")), "{p:?}");
        assert!(p.iter().any(|l| l.contains("unknown expected result")), "{p:?}");
    }
}
