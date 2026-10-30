use anyhow::Result;

use super::Report;

/// A self-contained HTML page: the reports are embedded, so it works offline and
/// can be shared as one file or published as a site.
pub fn html(reports: &[Report]) -> Result<String> {
    // `<` only appears inside JSON strings, where \u003c means the same thing, so
    // nothing in the data can close the <script> element.
    let data = serde_json::to_string(reports)?.replace('<', "\\u003c");
    Ok(include_str!("report.html").replacen("/*POOLPROOF_DATA*/", &data, 1))
}
