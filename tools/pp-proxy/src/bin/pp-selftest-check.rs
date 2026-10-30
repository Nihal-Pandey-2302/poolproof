//! pp-selftest-check EXPECTED.toml REPORT.json
//!
//! Compares a `poolproof run --format json` report of the self-test targets with the
//! expected verdicts. Any other verdict, and any target or scenario the expected file
//! doesn't cover, exits 1.

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

/// The verdict as the expected file names it, plus the error reason if any.
fn got(r: &Value) -> (String, String) {
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    match r["result"].as_str().unwrap_or("") {
        "fail" => (text(&r["mismatch"]["kind"]), String::new()),
        "error" => ("harness-error".into(), text(&r["reason"])),
        other => (other.to_string(), String::new()),
    }
}

fn check(expected: &Expected, report: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, t) in &expected.targets {
        for want in std::iter::once(&t.default).chain(t.scenarios.values()) {
            if !RESULTS.contains(&want.result()) {
                problems.push(format!("{name}: unknown expected result {:?}", want.result()));
            }
        }
    }
    let scenario_names: BTreeSet<&str> = report["scenarios"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["name"].as_str())
        .collect();
    for (target, t) in &expected.targets {
        for s in t.scenarios.keys() {
            if !scenario_names.contains(s.as_str()) {
                problems.push(format!("{target}: expected file names unknown scenario {s}"));
            }
        }
    }

    let mut seen = BTreeSet::new();
    for r in report["results"].as_array().into_iter().flatten() {
        let target = r["target"].as_str().unwrap_or("");
        let scenario = r["scenario"].as_str().unwrap_or("");
        seen.insert(target.to_string());
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
    for target in expected.targets.keys() {
        if !seen.contains(target) {
            problems.push(format!("{target}: no results in the report"));
        }
    }
    problems
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [expected, report] = args.as_slice() else {
        eprintln!("usage: pp-selftest-check EXPECTED.toml REPORT.json");
        return ExitCode::from(2);
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
    let problems = check(&expected, &report);
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
        assert_eq!(check(&expected(FILE), &r), Vec::<String>::new());
    }

    #[test]
    fn reports_every_difference() {
        let r = report(json!([
            {"target": "t", "scenario": "a", "result": "fail", "mismatch": {"kind": "hang"}, "runs": 1, "failures": 1},
            {"target": "t", "scenario": "b", "result": "pass", "runs": 1, "failures": 0},
            {"target": "down", "scenario": "a", "result": "error", "reason": "baseline failed at step 1"},
            {"target": "extra", "scenario": "a", "result": "pass"},
        ]));
        let p = check(&expected(FILE), &r);
        assert_eq!(p.len(), 4, "{p:?}");
        assert!(p[0].starts_with("t / a: expected pass, got hang"));
    }

    #[test]
    fn flaky_pass_is_a_failure() {
        let r = report(json!([
            {"target": "t", "scenario": "a", "result": "pass", "runs": 5, "failures": 1},
            {"target": "t", "scenario": "b", "result": "fail", "mismatch": {"kind": "silent"}},
            {"target": "down", "scenario": "a", "result": "error", "reason": "target unreachable: pooler: x"},
        ]));
        assert_eq!(check(&expected(FILE), &r).len(), 1);
    }

    #[test]
    fn unknown_names_in_expected_file() {
        let e = expected("[targets.t]\ndefault = \"pass\"\nnope = \"silent\"\nb = \"wrong\"\n");
        let r = report(json!([{"target": "t", "scenario": "a", "result": "pass"}]));
        let p = check(&e, &r);
        assert!(p.iter().any(|l| l.contains("unknown scenario nope")), "{p:?}");
        assert!(p.iter().any(|l| l.contains("unknown expected result")), "{p:?}");
    }
}
