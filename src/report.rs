//! The JSON report `run` writes, and the renderers that work from it. Rendering
//! from saved reports lets several runs (e.g. Postgres 17 and 18) share one table.

use std::fmt::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::runner::{FailKind, Outcome, Verdict};
use crate::scenario::PoolMode;

/// Self-describing results of one `run`. Connection URLs are left out because they
/// hold credentials.
#[derive(Serialize, Deserialize)]
pub struct Report {
    pub poolproof: String,
    pub targets: Vec<TargetInfo>,
    pub scenarios: Vec<ScenarioInfo>,
    pub results: Vec<ResultRow>,
}

#[derive(Serialize, Deserialize)]
pub struct TargetInfo {
    pub name: String,
    pub product: Option<String>,
    pub version: Option<String>,
    pub config: Option<String>,
    pub mode: PoolMode,
    /// Version of the Postgres server behind the pooler, if it could be read.
    pub postgres: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ScenarioInfo {
    pub name: String,
    pub description: String,
    pub refs: Vec<String>,
    pub steps: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ResultRow {
    pub target: String,
    pub scenario: String,
    #[serde(flatten)]
    pub verdict: Verdict,
    /// Every step's outcome on both sides; empty if the pooler run never happened.
    pub steps: Vec<StepOutcomes>,
}

#[derive(Serialize, Deserialize)]
pub struct StepOutcomes {
    pub postgres: Outcome,
    pub pooler: Outcome,
}

/// The results in `results/`, embedded at build time as (file name, contents).
const BUNDLED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/bundled_results.rs"));

/// The results built into the binary: the test bed's run on each Postgres version.
pub fn bundled() -> Result<Vec<Report>> {
    BUNDLED
        .iter()
        .map(|(name, text)| serde_json::from_str(text).with_context(|| format!("parsing bundled {name}")))
        .collect()
}

impl Report {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    fn verdict(&self, target: &str, scenario: &str) -> Option<&Verdict> {
        self.results.iter().find(|r| r.target == target && r.scenario == scenario).map(|r| &r.verdict)
    }
}

/// Compares version strings like `17.11` or `2.10.0-rc.1` part by part.
fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| v.split(['.', '-']).map(|p| p.parse::<u64>().map_err(|_| p.to_string())).collect::<Vec<_>>();
    parts(a).cmp(&parts(b))
}

/// The report and target for `--pooler NAME [--postgres VERSION]`; the newest
/// Postgres version when none is given.
pub fn find_target<'a>(
    reports: &'a [Report],
    name: &str,
    postgres: Option<&str>,
) -> Result<(&'a Report, &'a TargetInfo)> {
    let mut found: Vec<(&Report, &TargetInfo)> = reports
        .iter()
        .flat_map(|r| r.targets.iter().map(move |t| (r, t)))
        .filter(|(_, t)| t.name == name && postgres.is_none_or(|pg| t.postgres.as_deref() == Some(pg)))
        .collect();
    found.sort_by(|a, b| cmp_version(a.1.postgres.as_deref().unwrap_or(""), b.1.postgres.as_deref().unwrap_or("")));
    if let Some(last) = found.pop() {
        return Ok(last);
    }
    let mut names: Vec<String> = reports
        .iter()
        .flat_map(|r| &r.targets)
        .map(|t| format!("{} (Postgres {})", t.name, t.postgres.as_deref().unwrap_or("?")))
        .collect();
    names.sort();
    names.dedup();
    anyhow::bail!("no results for pooler '{name}'. Available:\n  {}", names.join("\n  "))
}

/// What `target` did on the given scenarios, or None if none of them ran there.
pub fn check_finding(report: &Report, target: &TargetInfo, scenarios: &[&str]) -> Option<crate::scan::PoolerCheck> {
    use crate::scan::PoolerStatus;
    let results: Vec<(String, String)> = scenarios
        .iter()
        .filter_map(|s| {
            let verdict = report.verdict(&target.name, s)?;
            let result = match verdict {
                Verdict::Pass => "pass".to_string(),
                Verdict::Fail { mismatch } => mismatch.kind.to_string(),
                Verdict::Skip { .. } | Verdict::Error { .. } => return None,
            };
            Some((s.to_string(), result))
        })
        .collect();
    let status = if results.is_empty() {
        return None;
    } else if results.iter().any(|(_, r)| r == "silent") {
        PoolerStatus::Breaks
    } else if results.iter().any(|(_, r)| r != "pass") {
        PoolerStatus::Rejected
    } else {
        PoolerStatus::Ok
    };
    Some(crate::scan::PoolerCheck {
        target: target.name.clone(),
        postgres: target.postgres.clone(),
        status,
        scenarios: results,
    })
}

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
                let label = if several { format!("{} (pg {pg})", target.name) } else { target.name.clone() };
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
    out.push_str("report each scenario came from.\n\n");
    let _ = write!(out, "| Scenario |");
    for c in &columns {
        let _ = write!(out, " {} |", c.label);
    }
    let _ = write!(out, "\n|---|{}\n", "---|".repeat(columns.len()));
    for s in &scenarios {
        let _ = write!(out, "| `{}` |", s.name);
        for c in &columns {
            let cell = match c.report.verdict(&c.target.name, &s.name) {
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
            let _ = write!(out, " {cell} |");
        }
        out.push('\n');
    }

    let mut details = String::new();
    for s in &scenarios {
        for c in &columns {
            match c.report.verdict(&c.target.name, &s.name) {
                Some(Verdict::Fail { mismatch: m }) => {
                    let _ = write!(details, "\n### {} / {} ({})\n\n{}\n\n", c.label, s.name, m.kind, s.description);
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

/// A self-contained HTML page: the reports are embedded, so it works offline and
/// can be shared as one file or published as a site.
pub fn html(reports: &[Report]) -> Result<String> {
    // `<` only appears inside JSON strings, where \u003c means the same thing, so
    // nothing in the data can close the <script> element.
    let data = serde_json::to_string(reports)?.replace('<', "\\u003c");
    Ok(include_str!("report.html").replacen("/*POOLPROOF_DATA*/", &data, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Report {
        serde_json::from_str(
            r#"{"poolproof":"0","targets":[{"name":"p","product":"p","version":"1","config":null,
            "mode":"transaction","postgres":"17"}],"scenarios":[{"name":"s","description":"</script><b>",
            "refs":[],"steps":["x"]}],"results":[{"target":"p","scenario":"s","result":"pass","steps":[]}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn html_embeds_data_safely() {
        let page = html(&[sample()]).unwrap();
        assert!(!page.contains("/*POOLPROOF_DATA*/"));
        assert!(!page.contains("</script><b>"));
        assert!(page.contains("\\u003c/script>\\u003cb>"));
    }

    #[test]
    fn finds_target_on_newest_postgres() {
        let mut older = sample();
        older.targets[0].postgres = Some("9.6".into());
        let reports = [older, sample()];
        let (_, t) = find_target(&reports, "p", None).unwrap();
        assert_eq!(t.postgres.as_deref(), Some("17"));
        let (_, t) = find_target(&reports, "p", Some("9.6")).unwrap();
        assert_eq!(t.postgres.as_deref(), Some("9.6"));
        assert!(find_target(&reports, "nope", None).is_err());
    }

    #[test]
    fn checks_findings_against_results() {
        use crate::scan::PoolerStatus;
        let report = sample();
        let target = &report.targets[0];
        assert_eq!(check_finding(&report, target, &["s"]).unwrap().status, PoolerStatus::Ok);
        assert!(check_finding(&report, target, &["not-run"]).is_none());
    }

    #[test]
    fn bundled_results_parse() {
        // Empty when results/ is missing, which is fine; when present it must parse.
        bundled().unwrap();
    }

    #[test]
    fn markdown_has_a_row_per_scenario() {
        let md = markdown(&[sample()]);
        assert!(md.contains("| `s` | pass |"));
    }
}
