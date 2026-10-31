use std::fmt::Write;

use super::{Report, ResultRow, ScenarioInfo, TargetInfo};
use crate::runner::{FailKind, Verdict};

/// One table column: a target in one report.
struct Column<'a> {
    report: &'a Report,
    target: &'a TargetInfo,
    label: String,
}

/// Labeled reports (`report --label`) come after the others, in the order given.
fn columns(reports: &[Report]) -> Vec<Column<'_>> {
    let several = reports.len() > 1;
    let unlabeled = reports.iter().filter(|r| r.label.is_none());
    unlabeled
        .chain(reports.iter().filter(|r| r.label.is_some()))
        .flat_map(|report| {
            report.targets.iter().map(move |target| {
                let pg = target.postgres.as_deref().unwrap_or("?");
                let label = if several {
                    format!("{} (pg {pg})", target.name)
                } else {
                    target.name.clone()
                };
                Column { report, target, label }
            })
        })
        .collect()
}

/// Scenarios across all reports, by name, each once.
fn scenarios(reports: &[Report]) -> Vec<&ScenarioInfo> {
    let mut all: Vec<&ScenarioInfo> = reports.iter().flat_map(|r| &r.scenarios).collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all.dedup_by(|a, b| a.name == b.name);
    all
}

/// Text from the command line, made inert in Markdown at the start of a line: no
/// emphasis, links, code, HTML, headings, quotes or list items.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    // A leading `-` or `+`, or digits and `.` or `)`, would start a list.
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    for (i, ch) in text.chars().enumerate() {
        let list = (i == 0 && matches!(ch, '-' | '+')) || (i == digits && digits > 0 && matches!(ch, '.' | ')'));
        if list || "\\`*_[]<>|#!~".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// For a scenario with a precondition, (runs that met it, runs in all), when there's
/// something to say: some run didn't meet it, or it ran more than once.
fn precondition(row: &ResultRow) -> Option<(u32, u32)> {
    let unmet = row.precondition_unmet.filter(|u| *u > 0 || row.runs > 1)?;
    Some((row.runs, row.runs + unmet))
}

pub fn markdown(reports: &[Report]) -> String {
    let columns = columns(reports);
    let scenarios = scenarios(reports);

    let mut out = String::from("# poolproof results\n\n");
    out.push_str("Each scenario runs against plain Postgres, then through the pooler. FAIL means the\n");
    out.push_str("pooler gave a different result than Postgres: **silent** means a wrong result with no\n");
    out.push_str("error (the dangerous kind), error means the pooler refused, hang means no reply. A FAIL\n");
    out.push_str("marked (expected) is a limitation the pooler's own documentation describes: not a bug, but\n");
    out.push_str("it still breaks apps that rely on the feature, and it fails the run unless\n");
    out.push_str("--ignore-expected is given. Links point to the report each scenario came from. A\n");
    out.push_str("fraction like 2/5 means the scenario is timing-sensitive and failed in 2 of 5 runs.\n");
    out.push_str("(precondition 1/5) means the scenario's precondition held in 1 of 5 runs, and only that\n");
    out.push_str("run counts.\n\n");
    let _ = write!(out, "| Scenario |");
    for c in &columns {
        let _ = write!(out, " {} |", c.label);
    }
    let _ = write!(out, "\n|---|{}\n", "---|".repeat(columns.len()));
    for s in &scenarios {
        let _ = write!(out, "| `{}` |", s.name);
        for c in &columns {
            let row = c.report.row(&c.target.name, &s.name);
            let expected = row.is_some_and(|r| r.expected.is_some());
            let cell = match row.map(|r| &r.verdict) {
                Some(Verdict::Pass) => "pass",
                // Expected failures aren't bold: bold marks the silent bugs.
                Some(Verdict::Fail { mismatch }) => match (mismatch.kind, expected) {
                    (FailKind::Silent, false) => "**FAIL** silent",
                    (FailKind::Silent, true) => "FAIL silent (expected)",
                    (FailKind::Error, false) => "FAIL error",
                    (FailKind::Error, true) => "FAIL error (expected)",
                    (FailKind::Hang, _) => "FAIL hang",
                },
                Some(Verdict::Error { .. }) => "ERROR",
                Some(Verdict::Skip { .. }) => "skip",
                None => "",
            };
            let runs = match row {
                Some(r) if r.runs > 1 && r.failures > 0 => format!(" {}/{}", r.failures, r.runs),
                _ => String::new(),
            };
            let pre = match row.and_then(precondition) {
                Some((met, ran)) => format!(" (precondition {met}/{ran})"),
                None => String::new(),
            };
            let _ = write!(out, " {cell}{runs}{pre} |");
        }
        out.push('\n');
    }

    // Which columns each label covers.
    let mut labels: Vec<(&str, Vec<&str>)> = Vec::new();
    for c in &columns {
        let Some(label) = c.report.label.as_deref() else {
            continue;
        };
        match labels.iter_mut().find(|(l, _)| *l == label) {
            Some((_, cols)) => cols.push(&c.label),
            None => labels.push((label, vec![&c.label])),
        }
    }
    for (label, cols) in &labels {
        let _ = write!(out, "\n{}: {}.\n", escape(label), cols.join(", "));
    }

    // "built-in" without the poolproof version, so a version bump doesn't change RESULTS.md.
    let mut sources: Vec<&str> = reports
        .iter()
        .filter_map(|r| r.limitations.as_deref())
        .map(|l| if l.starts_with("built-in") { "built-in" } else { l })
        .collect();
    sources.sort_unstable();
    sources.dedup();
    if !sources.is_empty() {
        let _ = write!(out, "\nDocumented limitations: {}.\n", sources.join(", "));
    }

    // Documented limitations that no longer match what the pooler does.
    let mut stale = String::new();
    for c in &columns {
        for e in c.report.stale_expectations.iter().filter(|e| e.target == c.target.name) {
            let _ = writeln!(stale, "- {} / `{}`: {} ({})", c.label, e.scenario, e.reason, e.doc);
        }
    }
    if !stale.is_empty() {
        out.push_str("\n## Stale documented limitations\n\n");
        out.push_str("These no longer match what the pooler does, so those results are plain FAILs or passes.\n\n");
        out.push_str(&stale);
    }

    let mut details = String::new();
    // Expected differences that read the same (same scenario, doc and outcome) are listed
    // once, with every target they apply to: (scenario, kind, text, targets).
    let mut documented: Vec<(&str, FailKind, String, Vec<&str>)> = Vec::new();
    for s in &scenarios {
        for c in &columns {
            let Some(row) = c.report.row(&c.target.name, &s.name) else {
                continue;
            };
            match &row.verdict {
                Verdict::Fail { mismatch: m } => {
                    let mut text = String::new();
                    if let Some(e) = &row.expected {
                        let _ = write!(text, "- documented: \"{}\", {}", e.quote, e.doc);
                        if let Some(v) = &e.doc_version {
                            let product = c.target.product.as_deref().unwrap_or(&c.target.name);
                            let _ = write!(text, " (documentation for {product} {v})");
                        }
                        text.push('\n');
                        if let Some(note) = &e.note {
                            let _ = writeln!(text, "- note: {note}");
                        }
                    }
                    if let Some((met, ran)) = precondition(row) {
                        let _ = writeln!(text, "- precondition met in {met} of {ran} runs");
                    }
                    let _ = writeln!(text, "- step {}: `{}`", m.step, m.action);
                    let _ = writeln!(text, "- postgres: `{}`", m.expected);
                    let _ = writeln!(text, "- pooler: `{}`", m.actual);
                    for r in &s.refs {
                        let _ = writeln!(text, "- from {r}");
                    }
                    if row.expected.is_none() {
                        let _ = write!(
                            details,
                            "\n### {} / {} ({})\n\n{}\n\n{text}",
                            c.label, s.name, m.kind, s.description
                        );
                    } else if let Some(group) = documented
                        .iter_mut()
                        .find(|g| g.0 == s.name && g.1 == m.kind && g.2 == text)
                    {
                        group.3.push(&c.label);
                    } else {
                        documented.push((&s.name, m.kind, text, vec![&c.label]));
                    }
                }
                Verdict::Error { reason } => {
                    let _ = write!(details, "\n### {} / {}: ERROR\n\n{reason}\n", c.label, s.name);
                }
                _ => {}
            }
        }
    }
    if !details.is_empty() {
        out.push_str("\n## Differences\n");
        out.push_str(&details);
    }
    if !documented.is_empty() {
        out.push_str("\n## Expected differences (documented limitations)\n");
        for (name, kind, text, targets) in &documented {
            let description = scenarios
                .iter()
                .find(|s| s.name == *name)
                .map_or("", |s| &s.description);
            let _ = write!(
                out,
                "\n### {name} ({kind})\n\n{description}\n\n- on {}\n{text}",
                targets.join(", ")
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_labels_start_no_list_or_markup() {
        assert_eq!(escape("- nightly"), "\\- nightly");
        assert_eq!(escape("+ nightly"), "\\+ nightly");
        assert_eq!(escape("12. nightly"), "12\\. nightly");
        assert_eq!(escape("3) nightly"), "3\\) nightly");
        assert_eq!(escape("# <b>x</b>"), "\\# \\<b\\>x\\</b\\>");
        // Only at the start: elsewhere these are plain text.
        assert_eq!(escape("v1.2 - 18.4 (nightly)"), "v1.2 - 18.4 (nightly)");
        assert_eq!(escape("2026 nightly"), "2026 nightly");
    }
}
