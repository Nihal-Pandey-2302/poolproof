//! `run --compare-to`: what changed since a saved report. Only regressions count for
//! the exit code, so a pooler with known failures can still gate its CI on new ones.

use std::fmt::Write as _;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{Report, ResultRow};
use crate::runner::suite::FailOn;
use crate::runner::{FailKind, Verdict};
use crate::scenario::PoolMode;

/// What changed between the baseline and this run. Results match by target and
/// scenario name.
#[derive(Serialize, Deserialize)]
pub struct Comparison {
    pub baseline: Baseline,
    /// Worse than in the baseline: PASS to FAIL or ERROR, a FAIL to a worse kind
    /// (hang, then error, then silent), a FAIL to ERROR, or an ERROR to FAIL.
    pub regressed: Vec<Entry>,
    /// FAIL or ERROR in the baseline, PASS now.
    pub fixed: Vec<Entry>,
    /// A FAIL of a milder kind, or for a repeated scenario any change of kind: its
    /// mismatch can come from a different run each time.
    pub changed: Vec<Entry>,
    /// SKIP on one side only, so nothing to compare.
    pub skipped: Vec<Entry>,
    /// In this run but not in the baseline. Never a regression.
    pub new: Vec<Key>,
    /// In the baseline but not in this run, e.g. after --filter. Never a regression.
    pub gone: Vec<Key>,
    /// Targets whose product, version, config, Postgres or reset differ, and
    /// scenarios whose steps differ.
    pub warnings: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Baseline {
    pub poolproof: String,
    pub schema_version: u32,
    pub generated_at: Option<String>,
    pub results: usize,
}

#[derive(Serialize, Deserialize)]
pub struct Key {
    pub target: String,
    pub scenario: String,
}

#[derive(Serialize, Deserialize)]
pub struct Entry {
    pub target: String,
    pub scenario: String,
    pub before: State,
    pub after: State,
    /// Whether --fail-on counts the new result. Only regressions that do make the
    /// exit code 1.
    pub counted: bool,
    /// The scenario's steps differ from the baseline's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scenario_changed: bool,
}

/// One side of an entry, without the step details.
#[derive(Serialize, Deserialize)]
pub struct State {
    /// pass, fail, error or skip.
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<FailKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// For repeated scenarios: how many runs failed, of how many.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_runs: Option<(u32, u32)>,
}

impl State {
    fn of(row: &ResultRow) -> Self {
        let (result, kind, reason) = match &row.verdict {
            Verdict::Pass => ("pass", None, None),
            Verdict::Fail { mismatch } => ("fail", Some(mismatch.kind), None),
            Verdict::Skip { reason } => ("skip", None, Some(reason.clone())),
            Verdict::Error { reason } => ("error", None, Some(reason.clone())),
        };
        State {
            result: result.to_string(),
            kind,
            reason,
            failed_runs: (row.runs > 1).then_some((row.failures, row.runs)),
        }
    }

    /// e.g. `PASS`, `FAIL silent (2/5 runs)`, `ERROR (target unreachable: ...)`.
    fn label(&self) -> String {
        let mut s = self.result.to_uppercase();
        if let Some(kind) = self.kind {
            write!(s, " {kind}").unwrap();
        }
        if let (Some(reason), "error") = (&self.reason, self.result.as_str()) {
            write!(s, " ({})", short(reason)).unwrap();
        }
        if let Some((failures, runs)) = self.failed_runs.filter(|(f, _)| *f > 0) {
            write!(s, " ({failures}/{runs} runs)").unwrap();
        }
        s
    }
}

/// The first line of an error reason, cut to fit on a line.
fn short(reason: &str) -> String {
    const MAX: usize = 80;
    let line = reason.lines().next().unwrap_or("");
    match line.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}...", &line[..cut]),
        None => line.to_string(),
    }
}

enum Change {
    Same,
    Regressed,
    Fixed,
    Changed,
    Skipped,
}

/// Hang, then error, then silent: the --fail-on ladder, worst last.
fn severity(kind: FailKind) -> u8 {
    match kind {
        FailKind::Hang => 0,
        FailKind::Error => 1,
        FailKind::Silent => 2,
    }
}

fn classify(before: &Verdict, after: &Verdict, repeated: bool) -> Change {
    use Verdict::*;
    match (before, after) {
        (Skip { .. }, Skip { .. }) | (Pass, Pass) | (Error { .. }, Error { .. }) => Change::Same,
        (Skip { .. }, _) | (_, Skip { .. }) => Change::Skipped,
        (Pass, _) => Change::Regressed,
        (_, Pass) => Change::Fixed,
        // No verdict now, or a FAIL where the baseline had no verdict to call it known.
        (_, Error { .. }) | (Error { .. }, Fail { .. }) => Change::Regressed,
        (Fail { mismatch: b }, Fail { mismatch: a }) => match severity(a.kind).cmp(&severity(b.kind)) {
            std::cmp::Ordering::Equal => Change::Same,
            std::cmp::Ordering::Greater if !repeated => Change::Regressed,
            _ => Change::Changed,
        },
    }
}

/// Compares `current` with `baseline`. A target whose pool mode differs is an error:
/// its results would be about a different setup.
pub fn compare(baseline: &Report, current: &Report, fail_on: FailOn) -> Result<Comparison> {
    check_modes(baseline, current.targets.iter().map(|t| (t.name.as_str(), t.mode)))?;
    let mut warnings = Vec::new();
    for t in &current.targets {
        let Some(b) = baseline.targets.iter().find(|b| b.name == t.name) else {
            continue;
        };
        let fields = [
            ("product", &b.product, &t.product),
            ("version", &b.version, &t.version),
            ("config", &b.config, &t.config),
            ("postgres", &b.postgres, &t.postgres),
            ("reset", &b.reset, &t.reset),
        ];
        let diffs: Vec<String> = fields
            .iter()
            // A Postgres version that couldn't be read this time isn't a difference.
            .filter(|(name, b, t)| b != t && !(*name == "postgres" && t.is_none()))
            .map(|(name, b, t)| {
                format!(
                    "{name} {} -> {}",
                    b.as_deref().unwrap_or("?"),
                    t.as_deref().unwrap_or("?")
                )
            })
            .collect();
        if !diffs.is_empty() {
            warnings.push(format!(
                "target {} differs from the baseline: {}",
                t.name,
                diffs.join(", ")
            ));
        }
    }
    let mut changed_scenarios = Vec::new();
    for s in &current.scenarios {
        if let Some(b) = baseline.scenarios.iter().find(|b| b.name == s.name)
            && b.steps != s.steps
        {
            warnings.push(format!("scenario {} has changed since the baseline", s.name));
            changed_scenarios.push(s.name.as_str());
        }
    }

    let mut cmp = Comparison {
        baseline: Baseline {
            poolproof: baseline.poolproof.clone(),
            schema_version: baseline.schema_version,
            generated_at: baseline.generated_at.clone(),
            results: baseline.results.len(),
        },
        regressed: Vec::new(),
        fixed: Vec::new(),
        changed: Vec::new(),
        skipped: Vec::new(),
        new: Vec::new(),
        gone: Vec::new(),
        warnings,
    };
    let key = |r: &ResultRow| Key {
        target: r.target.clone(),
        scenario: r.scenario.clone(),
    };
    for after in &current.results {
        let Some(before) = baseline.row(&after.target, &after.scenario) else {
            cmp.new.push(key(after));
            continue;
        };
        let list = match classify(&before.verdict, &after.verdict, before.runs > 1 || after.runs > 1) {
            Change::Same => continue,
            Change::Regressed => &mut cmp.regressed,
            Change::Fixed => &mut cmp.fixed,
            Change::Changed => &mut cmp.changed,
            Change::Skipped => &mut cmp.skipped,
        };
        list.push(Entry {
            target: after.target.clone(),
            scenario: after.scenario.clone(),
            before: State::of(before),
            after: State::of(after),
            counted: fail_on.fails(&after.verdict),
            scenario_changed: changed_scenarios.contains(&after.scenario.as_str()),
        });
    }
    cmp.gone = baseline
        .results
        .iter()
        .filter(|b| current.row(&b.target, &b.scenario).is_none())
        .map(key)
        .collect();
    // A renamed target or the wrong file would otherwise turn the gate off quietly.
    if !current.results.is_empty() && cmp.new.len() == current.results.len() {
        cmp.warnings
            .push("no results in common with the baseline; nothing was compared".to_string());
    }
    Ok(cmp)
}

/// Fails if a target is in a different pool mode than in the baseline: its results
/// would be about a different setup. `run` checks this before running anything.
pub fn check_modes<'a>(baseline: &Report, targets: impl IntoIterator<Item = (&'a str, PoolMode)>) -> Result<()> {
    for (name, mode) in targets {
        if let Some(b) = baseline.targets.iter().find(|b| b.name == name)
            && b.mode != mode
        {
            anyhow::bail!(
                "target '{name}' is in {mode} mode, but in {} mode in the baseline; compare runs of the same setup",
                b.mode
            );
        }
    }
    Ok(())
}

impl Comparison {
    /// Whether the exit code should be 1: a regression that --fail-on counts.
    pub fn fails(&self) -> bool {
        self.regressed.iter().any(|e| e.counted)
    }

    fn header(&self, file: &str) -> String {
        let b = &self.baseline;
        format!(
            "compared to {file} (poolproof {}, {}, {} results): {} regressed, {} fixed, {} changed, {} skipped, {} new, {} gone",
            b.poolproof,
            b.generated_at.as_deref().unwrap_or("time unknown"),
            b.results,
            self.regressed.len(),
            self.fixed.len(),
            self.changed.len(),
            self.skipped.len(),
            self.new.len(),
            self.gone.len(),
        )
    }

    /// One line per change. A target whose every result in this run is ERROR gets one
    /// line instead, so an outage reads as an outage.
    fn lines(&self, current: &Report) -> Vec<String> {
        let down: Vec<&str> = current
            .targets
            .iter()
            .map(|t| t.name.as_str())
            .filter(|name| {
                let mut rows = current.results.iter().filter(|r| r.target == *name).peekable();
                rows.peek().is_some() && rows.all(|r| matches!(r.verdict, Verdict::Error { .. }))
            })
            .collect();
        let mut lines = Vec::new();
        for name in &down {
            let rows: Vec<&Entry> = self.regressed.iter().filter(|e| e.target == *name).collect();
            if let Some(first) = rows.first() {
                let total = current.results.iter().filter(|r| r.target == *name).count();
                let reason = first.after.reason.as_deref().map(short).unwrap_or_default();
                lines.push(format!(
                    "regressed {name}: all {total} results ERROR ({reason}); {} were not ERROR in the baseline",
                    rows.len()
                ));
            }
        }
        let groups = [
            ("regressed", &self.regressed),
            ("fixed", &self.fixed),
            ("changed", &self.changed),
            ("skipped", &self.skipped),
        ];
        for (what, entries) in groups {
            for e in entries.iter() {
                if what == "regressed" && down.contains(&e.target.as_str()) {
                    continue;
                }
                let mut line = format!(
                    "{what:<9} {} / {}: {} -> {}",
                    e.target,
                    e.scenario,
                    e.before.label(),
                    e.after.label()
                );
                if e.scenario_changed {
                    line.push_str(" (scenario changed)");
                }
                if what == "regressed" && !e.counted {
                    line.push_str(" (not counted by --fail-on)");
                }
                lines.push(line);
            }
        }
        lines
    }

    /// The comparison as `run` prints it after the summary.
    pub fn text(&self, file: &str, current: &Report) -> String {
        let mut out = format!("\n{}\n", self.header(file));
        for w in &self.warnings {
            writeln!(out, "warning: {w}").unwrap();
        }
        for line in self.lines(current) {
            writeln!(out, "  {line}").unwrap();
        }
        out
    }

    /// The same, as a markdown section to follow `run --format markdown`'s table.
    pub fn markdown(&self, file: &str, current: &Report) -> String {
        let mut out = format!(
            "\n## Compared to the baseline\n\n{}\n",
            self.header(&format!("`{file}`"))
        );
        if !self.warnings.is_empty() {
            out.push('\n');
            for w in &self.warnings {
                writeln!(out, "- warning: {w}").unwrap();
            }
        }
        let lines = self.lines(current);
        if !lines.is_empty() {
            out.push('\n');
            for line in lines {
                writeln!(out, "- {line}").unwrap();
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A report with one target `p` and the given (scenario, result) rows. A result is
    /// pass, skip, error (the ERROR verdict), silent, hang or fail:error (a FAIL of
    /// kind error); `name*N/K` makes it a repeated scenario whose K of N runs failed.
    fn report(rows: &[(&str, &str)]) -> Report {
        let results: Vec<String> = rows
            .iter()
            .map(|(name, result)| {
                let (name, runs) = match name.split_once('*') {
                    Some((n, r)) => (n, r.split_once('/').unwrap()),
                    None => (*name, ("1", "0")),
                };
                let verdict = match *result {
                    "pass" => r#""result":"pass""#.to_string(),
                    "skip" => r#""result":"skip","reason":"needs createrole""#.to_string(),
                    "error" => r#""result":"error","reason":"target unreachable: pooler: refused""#.to_string(),
                    kind => format!(
                        r#""result":"fail","mismatch":{{"step":1,"action":"x","kind":"{}",
                        "expected":{{"status":"ok","rows":[],"tags":[]}},"actual":{{"status":"ok","rows":[],"tags":[]}}}}"#,
                        kind.strip_prefix("fail:").unwrap_or(kind)
                    ),
                };
                format!(
                    r#"{{"target":"p","scenario":"{name}",{verdict},"steps":[],"runs":{},"failures":{}}}"#,
                    runs.0, runs.1
                )
            })
            .collect();
        let scenarios: Vec<String> = rows
            .iter()
            .map(|(name, _)| {
                let name = name.split('*').next().unwrap();
                format!(r#"{{"name":"{name}","description":"d","refs":[],"steps":["x"]}}"#)
            })
            .collect();
        Report::parse(&format!(
            r#"{{"poolproof":"0.1.0","generated_at":"2026-10-31T09:23:05Z",
            "targets":[{{"name":"p","product":"pgdog","version":"0.1.62","config":null,"mode":"transaction","postgres":"17.11"}}],
            "scenarios":[{}],"results":[{}]}}"#,
            scenarios.join(","),
            results.join(",")
        ))
        .unwrap()
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.scenario.as_str()).collect()
    }

    #[test]
    fn classifies_each_change() {
        let before = report(&[
            ("pass-fail", "pass"),
            ("pass-error", "pass"),
            ("hang-silent", "hang"),
            ("silent-hang", "silent"),
            ("silent-silent", "silent"),
            ("fail-pass", "silent"),
            ("error-pass", "error"),
            ("error-fail", "error"),
            ("fail-error", "hang"),
            ("error-error", "error"),
            ("skip-pass", "skip"),
            ("gone", "pass"),
        ]);
        let after = report(&[
            ("pass-fail", "hang"),
            ("pass-error", "error"),
            ("hang-silent", "silent"),
            ("silent-hang", "hang"),
            ("silent-silent", "silent"),
            ("fail-pass", "pass"),
            ("error-pass", "pass"),
            ("error-fail", "hang"),
            ("fail-error", "error"),
            ("error-error", "error"),
            ("skip-pass", "pass"),
            ("new", "silent"),
        ]);
        let cmp = compare(&before, &after, FailOn::Any).unwrap();
        assert_eq!(
            names(&cmp.regressed),
            ["pass-fail", "pass-error", "hang-silent", "error-fail", "fail-error"]
        );
        assert_eq!(names(&cmp.fixed), ["fail-pass", "error-pass"]);
        assert_eq!(names(&cmp.changed), ["silent-hang"]);
        assert_eq!(names(&cmp.skipped), ["skip-pass"]);
        assert_eq!(cmp.new.iter().map(|k| k.scenario.as_str()).collect::<Vec<_>>(), ["new"]);
        assert_eq!(
            cmp.gone.iter().map(|k| k.scenario.as_str()).collect::<Vec<_>>(),
            ["gone"]
        );
        assert!(cmp.fails());
    }

    #[test]
    fn known_failures_and_new_scenarios_do_not_fail() {
        let before = report(&[("a", "silent"), ("b", "pass")]);
        let after = report(&[("a", "silent"), ("b", "pass"), ("c", "silent")]);
        let cmp = compare(&before, &after, FailOn::Any).unwrap();
        assert!(cmp.regressed.is_empty() && !cmp.fails());
        // A filtered run against a full baseline: the rest is gone, not regressed.
        let cmp = compare(&before, &report(&[("b", "pass")]), FailOn::Any).unwrap();
        assert_eq!(cmp.gone.len(), 1);
        assert!(!cmp.fails());
    }

    #[test]
    fn fail_on_decides_which_regressions_count() {
        let before = report(&[("a", "pass"), ("b", "pass")]);
        let after = report(&[("a", "hang"), ("b", "fail:error")]);
        let cmp = compare(&before, &after, FailOn::Silent).unwrap();
        assert_eq!(cmp.regressed.len(), 2);
        assert!(!cmp.fails());
        assert!(cmp.text("base.json", &after).contains("(not counted by --fail-on)"));
        let after = report(&[("a", "silent"), ("b", "pass")]);
        assert!(compare(&before, &after, FailOn::Silent).unwrap().fails());
        // ERROR counts for every level but none.
        let after = report(&[("a", "error"), ("b", "pass")]);
        assert!(compare(&before, &after, FailOn::Silent).unwrap().fails());
        assert!(!compare(&before, &after, FailOn::None).unwrap().fails());
    }

    #[test]
    fn repeated_scenarios_gate_only_on_pass_and_fail() {
        let before = report(&[("r*5/3", "hang"), ("s*5/0", "pass")]);
        let after = report(&[("r*5/1", "silent"), ("s*5/1", "silent")]);
        let cmp = compare(&before, &after, FailOn::Any).unwrap();
        assert_eq!(names(&cmp.changed), ["r"]);
        assert_eq!(names(&cmp.regressed), ["s"]);
        let text = cmp.text("base.json", &after);
        assert!(text.contains("p / s: PASS -> FAIL silent (1/5 runs)"), "{text}");
        assert!(
            text.contains("p / r: FAIL hang (3/5 runs) -> FAIL silent (1/5 runs)"),
            "{text}"
        );
    }

    #[test]
    fn a_target_that_is_down_is_one_line() {
        let before = report(&[("a", "pass"), ("b", "silent"), ("c", "error")]);
        let after = report(&[("a", "error"), ("b", "error"), ("c", "error")]);
        let cmp = compare(&before, &after, FailOn::Any).unwrap();
        let text = cmp.text("base.json", &after);
        assert!(
            text.contains(
                "regressed p: all 3 results ERROR (target unreachable: pooler: refused); 2 were not ERROR in the baseline"
            ),
            "{text}"
        );
        assert!(!text.contains("p / a"), "{text}");
        // One ERROR among other results is listed on its own, with its reason.
        let after = report(&[("a", "error"), ("b", "silent"), ("c", "error")]);
        let text = compare(&before, &after, FailOn::Any).unwrap().text("base.json", &after);
        assert!(
            text.contains("regressed p / a: PASS -> ERROR (target unreachable: pooler: refused)"),
            "{text}"
        );
    }

    #[test]
    fn header_names_the_baseline_and_warns_on_a_different_setup() {
        let before = report(&[("a", "pass")]);
        let mut after = report(&[("a", "pass")]);
        after.targets[0].version = Some("0.1.63".into());
        after.scenarios[0].steps = vec!["y".into()];
        let cmp = compare(&before, &after, FailOn::Any).unwrap();
        let text = cmp.text("base.json", &after);
        assert!(text.contains(
            "compared to base.json (poolproof 0.1.0, 2026-10-31T09:23:05Z, 1 results): 0 regressed, 0 fixed, 0 changed, 0 skipped, 0 new, 0 gone"
        ));
        assert!(!text.contains("no results in common"));
        assert!(text.contains("warning: target p differs from the baseline: version 0.1.62 -> 0.1.63"));
        assert!(text.contains("warning: scenario a has changed since the baseline"));
        after.targets[0].postgres = None;
        let text = compare(&before, &after, FailOn::Any).unwrap().text("base.json", &after);
        assert!(!text.contains("postgres 17.11"), "{text}");
        after.targets[0].mode = PoolMode::Session;
        let err = compare(&before, &after, FailOn::Any).err().unwrap().to_string();
        assert!(
            err.contains("target 'p' is in session mode, but in transaction mode"),
            "{err}"
        );
        assert!(check_modes(&before, [("p", PoolMode::Transaction), ("other", PoolMode::Session)]).is_ok());
        assert!(check_modes(&before, [("p", PoolMode::Session)]).is_err());
    }

    #[test]
    fn warns_when_nothing_matches() {
        let before = report(&[("a", "pass")]);
        let mut after = report(&[("a", "silent")]);
        after.results[0].target = "renamed".into();
        let cmp = compare(&before, &after, FailOn::Any).unwrap();
        assert!(!cmp.fails());
        assert!(
            cmp.text("base.json", &after)
                .contains("warning: no results in common with the baseline; nothing was compared")
        );
    }

    #[test]
    fn json_round_trips_inside_the_report() {
        let before = report(&[("a", "pass")]);
        let mut after = report(&[("a", "error")]);
        after.comparison = Some(compare(&before, &after, FailOn::Any).unwrap());
        let text = serde_json::to_string(&after).unwrap();
        assert!(
            text.contains(r#""comparison":{"baseline":{"poolproof":"0.1.0""#),
            "{text}"
        );
        let back = Report::parse(&text).unwrap();
        assert_eq!(back.comparison.unwrap().regressed[0].after.result, "error");
        // Plain reports don't carry the field.
        assert!(!serde_json::to_string(&before).unwrap().contains("comparison"));
    }
}
