use super::scan::{differs_summary, fix_summary, hidden_hint, pooler_summary};
use crate::scan;

pub fn sarif(report: &scan::Report, input: &std::path::Path) -> serde_json::Value {
    use serde_json::json;

    let rules: Vec<_> = scan::RULES
        .iter()
        .map(|(id, summary)| json!({"id": id, "shortDescription": {"text": summary}}))
        .collect();
    let uri = input.to_string_lossy().replace('\\', "/");
    let results: Vec<_> = report
        .findings
        .iter()
        .map(|f| {
            let level = match (&f.on_pooler, f.severity) {
                (Some(check), _) if check.status == scan::PoolerStatus::Ok => "note",
                (Some(_), _) | (None, scan::Severity::Breaks) => "error",
                (None, scan::Severity::Warn) => "warning",
            };
            let fix = |c: &scan::PoolerCheck| c.fix.as_ref().map(|x| format!(" {}.", fix_summary(x)));
            let pooler = match (&f.on_pooler, &f.pooler_note) {
                (Some(c), _) => format!(" Checked {}.{}", pooler_summary(c), fix(c).unwrap_or_default()),
                (None, Some(note)) => format!(" {}.", note.trim_end_matches('.')),
                (None, None) => String::new(),
            };
            let differs: String = f
                .differs_on
                .iter()
                .map(|c| format!(" {}.", differs_summary(c)))
                .collect();
            json!({
                "ruleId": f.rule,
                "level": level,
                "message": {"text": format!("{} Calls: {}.{pooler}{differs}", f.detail, f.calls)},
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": {"uri": uri},
                        "region": {"startLine": f.line.max(1)},
                    },
                }],
            })
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {"driver": {
                "name": "poolproof",
                "informationUri": env!("CARGO_PKG_REPOSITORY"),
                "version": env!("CARGO_PKG_VERSION"),
                "rules": rules,
            }},
            "results": results,
            "invocations": [{
                "executionSuccessful": true,
                "toolExecutionNotifications": notifications(report),
            }],
        }],
    })
}

/// What scan could not fully read, as SARIF notifications rather than results.
fn notifications(report: &scan::Report) -> Vec<serde_json::Value> {
    use serde_json::json;

    let mut out = Vec::new();
    if report.unparsed > 0 {
        out.push(json!({
            "level": "warning",
            "message": {"text": format!("{} queries could not be parsed.", report.unparsed)},
        }));
    }
    if report.hidden > 0 {
        out.push(json!({
            "level": "warning",
            "message": {"text": format!("{}.", hidden_hint(report.hidden))},
        }));
    }
    if report.uninspected > 0 {
        out.push(json!({
            "level": "note",
            "message": {"text": format!(
                "Function and DO bodies not fully parsed, so only their words were checked: {}",
                report.uninspected
            )},
        }));
    }
    for note in &report.notes {
        out.push(json!({"level": "note", "message": {"text": note}}));
    }
    out
}
