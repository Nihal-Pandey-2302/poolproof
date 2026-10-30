use super::scan::pooler_summary;
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
            let pooler = f
                .on_pooler
                .as_ref()
                .map(|c| format!(" Checked {}.", pooler_summary(c)))
                .unwrap_or_default();
            json!({
                "ruleId": f.rule,
                "level": level,
                "message": {"text": format!("{} Calls: {}.{pooler}", f.detail, f.calls)},
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
        }],
    })
}
