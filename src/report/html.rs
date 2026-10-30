use anyhow::{Context, Result};
use base64::Engine;
use sha2::{Digest, Sha256};

use super::Report;

const TEMPLATE: &str = include_str!("report.html");

/// A self-contained HTML page: the reports are embedded, so it works offline and
/// can be shared as one file or published as a site.
pub fn html(reports: &[Report]) -> Result<String> {
    // `<` only appears inside JSON strings, where < means the same thing, so
    // nothing in the data can close the <script> element.
    let data = serde_json::to_string(reports)?.replace('<', "\\u003c");
    Ok(TEMPLATE
        .replacen("<!--POOLPROOF_CSP-->", &csp_meta()?, 1)
        .replacen("/*POOLPROOF_DATA*/", &data, 1))
}

/// A Content-Security-Policy that allows nothing but the page's own inline script and
/// style, by hash. A results file is data from whoever ran it; if anything in it ever
/// got rendered as markup, it still couldn't run script or load anything.
fn csp_meta() -> Result<String> {
    let script = sha256(inline(TEMPLATE, "<script>", "</script>")?);
    let style = sha256(inline(TEMPLATE, "<style>", "</style>")?);
    Ok(format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
         script-src 'sha256-{script}'; style-src 'sha256-{style}'; base-uri 'none'; form-action 'none'\">"
    ))
}

/// The text of the inline element that starts with `open` (exactly, no attributes).
fn inline<'a>(page: &'a str, open: &str, close: &str) -> Result<&'a str> {
    let start = page.find(open).context("report template: no inline element")? + open.len();
    let len = page[start..]
        .find(close)
        .context("report template: element not closed")?;
    Ok(&page[start..start + len])
}

fn sha256(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_with_refs(refs: &[&str]) -> Report {
        let refs = serde_json::to_string(refs).unwrap();
        serde_json::from_str(&format!(
            r#"{{"poolproof":"0","targets":[{{"name":"p","mode":"transaction"}}],"scenarios":[{{"name":"s","description":"d",
            "refs":{refs},"steps":["x"]}}],"results":[{{"target":"p","scenario":"s","result":"pass","steps":[]}}]}}"#
        ))
        .unwrap()
    }

    #[test]
    fn csp_hashes_match_the_inline_script_and_style() {
        let page = html(&[report_with_refs(&[])]).unwrap();
        assert_eq!(page.matches("Content-Security-Policy").count(), 1);
        assert!(!page.contains("POOLPROOF_CSP"));
        let script = inline(&page, "<script>", "</script>").unwrap();
        let style = inline(&page, "<style>", "</style>").unwrap();
        assert!(page.contains(&format!("script-src 'sha256-{}'", sha256(script))));
        assert!(page.contains(&format!("style-src 'sha256-{}'", sha256(style))));
        assert!(page.contains("default-src 'none'"));
        // The CSP blocks inline style attributes and event handler attributes, so the
        // script must use classes and addEventListener.
        assert!(!TEMPLATE.contains("style: \""));
        assert!(!TEMPLATE.contains(" style=\""));
        // Exactly one executable inline script and one style element, both hashed.
        assert_eq!(page.matches("<script>").count(), 1);
        assert_eq!(page.matches("<style").count(), 1);
    }

    #[test]
    fn hostile_refs_stay_inside_the_data() {
        let hostile = [
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "\"><script>alert(1)</script>",
            "</script><script>alert(1)</script>",
        ];
        let page = html(&[report_with_refs(&hostile)]).unwrap();
        // The refs stay escaped JSON inside the data block: only the template's own two
        // scripts close.
        assert_eq!(page.matches("</script>").count(), 2);
        assert!(page.contains("javascript:alert(1)"));
        assert!(page.contains(r#"\u003cscript>alert(1)\u003c/script>"#));
        // Links are built only for http(s) refs.
        assert!(TEMPLATE.contains(r#"/^https?:\/\//i.test(r)"#));
        assert!(TEMPLATE.contains(r#"rel: "noopener noreferrer""#));
    }
}
