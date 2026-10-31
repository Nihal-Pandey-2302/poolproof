//! The JSON report `run` writes, and the renderers that work from it. Rendering
//! from saved reports lets several runs (e.g. Postgres 17 and 18) share one table.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::runner::{Outcome, Verdict};
use crate::scenario::{Compare, PoolMode};

pub mod compare;
mod html;
mod markdown;
#[cfg(test)]
mod schema;

pub use html::html;
pub use markdown::markdown;

/// The report format this poolproof writes and the newest it reads. Bump it when a
/// change would make an older poolproof misread a report; a new optional field doesn't.
pub const SCHEMA_VERSION: u32 = 1;

/// Where the JSON Schemas are published (the site, from `schemas/`). `v1` follows
/// SCHEMA_VERSION.
pub const SCHEMAS_URL: &str = "https://nihal-pandey-2302.github.io/poolproof/schemas/v1";

/// The report's JSON Schema, which `run` names in `$schema` so editors validate it.
pub const REPORT_SCHEMA: &str = "https://nihal-pandey-2302.github.io/poolproof/schemas/v1/report.schema.json";

/// Self-describing results of one `run`. Connection URLs are left out because they
/// hold credentials.
#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Report {
    /// The report's JSON Schema. Missing in reports from before it was recorded.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Reports from before it was recorded are version 1.
    #[serde(default = "one")]
    pub schema_version: u32,
    pub poolproof: String,
    /// When the run finished, RFC 3339 in UTC. Missing in reports from before it was
    /// recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
    pub targets: Vec<TargetInfo>,
    pub scenarios: Vec<ScenarioInfo>,
    pub results: Vec<ResultRow>,
    /// With `run --compare-to`: what changed since that report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<compare::Comparison>,
    /// Which documented limitations marked FAILs as expected: `built-in from poolproof
    /// X.Y.Z` or `custom` (from `--limitations`). Missing when none were applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limitations: Option<String>,
    /// Documented limitations that applied to a target here but didn't match its
    /// result: the scenario passes now, or fails some other way. Those rows are plain
    /// FAILs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale_expectations: Vec<crate::limitations::Stale>,
}

#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct TargetInfo {
    pub name: String,
    pub product: Option<String>,
    pub version: Option<String>,
    pub config: Option<String>,
    pub mode: PoolMode,
    /// Version of the Postgres server behind the pooler, if it could be read.
    pub postgres: Option<String>,
    /// The target's reset: none, auto, all or custom. Never the SQL. Missing in
    /// reports from before it was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<String>,
    /// What a second client got while the first was connected and idle. Missing when
    /// the target wasn't probed, and in older reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_probe: Option<crate::runner::probe::Probe>,
    /// The same inside the first client's open transaction: whether the pooler has
    /// one server connection or several.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy_probe: Option<crate::runner::probe::Probe>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct ScenarioInfo {
    pub name: String,
    pub description: String,
    pub refs: Vec<String>,
    pub steps: Vec<String>,
    /// More than 1 for timing-sensitive scenarios, whose worst case can vary by machine.
    #[serde(default = "one")]
    pub repeat: u32,
    /// How each step is compared, in step order. Missing in reports from before it was
    /// recorded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compare: Vec<Compare>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct ResultRow {
    pub target: String,
    pub scenario: String,
    #[serde(flatten)]
    pub verdict: Verdict,
    /// Every step's outcome on both sides; empty if the pooler run never happened.
    pub steps: Vec<StepOutcomes>,
    /// How many times the pooler side ran (the scenario's `repeat`) and how many of
    /// those runs failed.
    #[serde(default = "one")]
    pub runs: u32,
    #[serde(default)]
    pub failures: u32,
    /// With more than one run: whether a failing run failed differently from the one
    /// whose steps are kept here. Missing in older reports, and then unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub varied: Option<bool>,
    /// For a scenario with a precondition: runs where it wasn't met. They aren't in
    /// `runs` or `failures`, so runs + unmet is how many ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precondition_unmet: Option<u32>,
    /// Set on a FAIL the pooler's own documentation describes. It is still a FAIL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<crate::limitations::Expected>,
}

fn one() -> u32 {
    1
}

/// The current time for `generated_at`, e.g. `2026-10-31T09:23:05Z`.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339(secs)
}

/// Seconds since the Unix epoch as an RFC 3339 UTC time. The date part is Howard
/// Hinnant's civil_from_days.
fn rfc3339(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem / 60 % 60,
        rem % 60
    )
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
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
        .map(|(name, text)| Report::parse(text).with_context(|| format!("parsing bundled {name}")))
        .collect()
}

impl Report {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Checks the format version before anything else, so a report from a newer
    /// poolproof says so instead of failing on whatever field changed.
    fn parse(text: &str) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let version = match value.get("schema_version") {
            None => 1,
            Some(v) => v.as_u64().context("schema_version is not a number")?,
        };
        if version > u64::from(SCHEMA_VERSION) {
            anyhow::bail!(
                "report format {version} is newer than this poolproof reads ({SCHEMA_VERSION}); \
                 use a newer poolproof"
            );
        }
        if version == 0 {
            anyhow::bail!("report format 0 is not valid");
        }
        Ok(serde_json::from_value(value)?)
    }

    fn verdict(&self, target: &str, scenario: &str) -> Option<&Verdict> {
        self.row(target, scenario).map(|r| &r.verdict)
    }

    fn row(&self, target: &str, scenario: &str) -> Option<&ResultRow> {
        self.results
            .iter()
            .find(|r| r.target == target && r.scenario == scenario)
    }
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
    found.sort_by(|a, b| {
        crate::version::cmp(
            a.1.postgres.as_deref().unwrap_or(""),
            b.1.postgres.as_deref().unwrap_or(""),
        )
    });
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
    let mut documented: Vec<String> = Vec::new();
    let mut undocumented = false;
    let results: Vec<(String, String)> = scenarios
        .iter()
        .filter_map(|s| {
            let row = report.row(&target.name, s)?;
            if let Verdict::Fail { .. } = row.verdict {
                match &row.expected {
                    Some(e) if !documented.contains(&e.doc) => documented.push(e.doc.clone()),
                    Some(_) => {}
                    None => undocumented = true,
                }
            }
            let verdict = &row.verdict;
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
        documented: if undocumented { vec![] } else { documented },
    })
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
    fn findings_backed_only_by_documented_fails_still_break() {
        use crate::scan::PoolerStatus;
        let fail = |expected: bool| {
            let doc = expected.then(|| r#","expected":{"doc":"https://d/","quote":"q"}"#.to_string());
            format!(
                r#"{{"target":"p","scenario":"{}","result":"fail","mismatch":{{"step":1,"action":"x","kind":"silent",
                "expected":{{"status":"ok","rows":[],"tags":[]}},"actual":{{"status":"ok","rows":[],"tags":[]}}}},
                "steps":[]{}}}"#,
                if expected { "a" } else { "b" },
                doc.unwrap_or_default()
            )
        };
        let report: Report = serde_json::from_str(&format!(
            r#"{{"poolproof":"0","targets":[{{"name":"p","product":"p","version":"1","config":null,
            "mode":"transaction","postgres":"17"}}],"scenarios":[],"results":[{},{}]}}"#,
            fail(true),
            fail(false)
        ))
        .unwrap();
        let target = &report.targets[0];
        let only_documented = check_finding(&report, target, &["a"]).unwrap();
        assert_eq!(only_documented.status, PoolerStatus::Breaks);
        assert_eq!(only_documented.documented, ["https://d/"]);
        let mixed = check_finding(&report, target, &["a", "b"]).unwrap();
        assert_eq!(mixed.status, PoolerStatus::Breaks);
        assert!(mixed.documented.is_empty());
        let line = crate::output::scan::pooler_summary(&only_documented);
        assert_eq!(line, "on p: a fails silently (documented: https://d/)");
    }

    #[test]
    fn schema_version_defaults_to_1_and_newer_is_refused() {
        let text = serde_json::to_string(&sample()).unwrap();
        assert!(text.starts_with(r#"{"schema_version":1,"#));
        assert!(!text.contains("generated_at"));
        let old = r#"{"poolproof":"0","targets":[],"scenarios":[],"results":[]}"#;
        assert_eq!(Report::parse(old).unwrap().schema_version, 1);
        // A newer format is refused by its version, even if its fields changed.
        let newer = r#"{"schema_version":2,"poolproof":"9","results":{"changed":true}}"#;
        let err = Report::parse(newer).err().unwrap().to_string();
        assert!(err.contains("report format 2 is newer"), "{err}");
        assert!(Report::parse(r#"{"schema_version":0}"#).is_err());
        assert!(Report::parse(r#"{"schema_version":"1"}"#).is_err());
    }

    #[test]
    fn rfc3339_dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_793_438_585), "2026-10-31T09:23:05Z");
        assert_eq!(rfc3339(4_107_542_399), "2100-02-28T23:59:59Z");
        assert_eq!(now_rfc3339().len(), "2026-10-31T09:23:05Z".len());
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
