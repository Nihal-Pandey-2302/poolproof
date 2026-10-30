use std::fmt::Write;

use super::{Report, ScenarioInfo, TargetInfo};
use crate::runner::{FailKind, Verdict};

/// One table column: a target in one report.
struct Column<'a> {
    report: &'a Report,
    target: &'a TargetInfo,
    label: String,
}

fn columns(reports: &[Report]) -> Vec<Column<'_>> {
    let several = reports.len() > 1;
    reports
        .iter()
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

pub fn markdown(reports: &[Report]) -> String {
    let columns = columns(reports);
    let scenarios = scenarios(reports);

    let mut out = String::from("# poolproof results\n\n");
    out.push_str("Each scenario runs against plain Postgres, then through the pooler. FAIL means the\n");
    out.push_str("pooler gave a different result than Postgres: **silent** means a wrong result with no\n");
    out.push_str("error (the dangerous kind), error means the pooler refused, hang means no reply. Not\n");
    out.push_str("every FAIL is a bug: some are documented limits of the pooling mode. Links point to the\n");
    out.push_str("report each scenario came from. A fraction like 2/5 means the scenario is timing-sensitive\n");
    out.push_str("and failed in 2 of 5 runs.\n\n");
    let _ = write!(out, "| Scenario |");
    for c in &columns {
        let _ = write!(out, " {} |", c.label);
    }
    let _ = write!(out, "\n|---|{}\n", "---|".repeat(columns.len()));
    for s in &scenarios {
        let _ = write!(out, "| `{}` |", s.name);
        for c in &columns {
            let row = c.report.row(&c.target.name, &s.name);
            let cell = match row.map(|r| &r.verdict) {
                Some(Verdict::Pass) => "pass",
                Some(Verdict::Fail { mismatch }) => match mismatch.kind {
                    FailKind::Silent => "**FAIL** silent",
                    FailKind::Error => "FAIL error",
                    FailKind::Hang => "FAIL hang",
                },
                Some(Verdict::Error { .. }) => "ERROR",
                Some(Verdict::Skip { .. }) => "skip",
                None => "",
            };
            let runs = match row {
                Some(r) if r.runs > 1 && r.failures > 0 => format!(" {}/{}", r.failures, r.runs),
                _ => String::new(),
            };
            let _ = write!(out, " {cell}{runs} |");
        }
        out.push('\n');
    }

    let mut details = String::new();
    for s in &scenarios {
        for c in &columns {
            match c.report.verdict(&c.target.name, &s.name) {
                Some(Verdict::Fail { mismatch: m }) => {
                    let _ = write!(
                        details,
                        "\n### {} / {} ({})\n\n{}\n\n",
                        c.label, s.name, m.kind, s.description
                    );
                    let _ = writeln!(details, "- step {}: `{}`", m.step, m.action);
                    let _ = writeln!(details, "- postgres: `{}`", m.expected);
                    let _ = writeln!(details, "- pooler: `{}`", m.actual);
                    for r in &s.refs {
                        let _ = writeln!(details, "- from {r}");
                    }
                }
                Some(Verdict::Error { reason }) => {
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
    out
}
