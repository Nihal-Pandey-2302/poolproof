//! Runs every query in `tests/fixtures/scan/*.toml` through `scan` and checks its findings.
//! See `tests/fixtures/scan/README.md` for the format.

use std::collections::BTreeSet;
use std::path::Path;

use poolproof::scan::{Query, RULES, scan};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    query: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    sql: String,
    form: String,
    expect: Vec<String>,
    #[allow(dead_code)]
    note: Option<String>,
    /// TODO.md item that will make scan agree with `expect`.
    pending: Option<String>,
}

const FORMS: &[&str] = &["pgss", "pgss-17", "pgss-18", "log", "sql"];
/// Rule IDs reserved for rules not written yet, so `expect` can name them.
const PLANNED_RULES: &[&str] = &["deallocate-all", "load"];

#[test]
fn corpus() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no corpus files in {}", dir.display());

    let mut checked = 0;
    let mut failures = Vec::new();
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy();
        let corpus: Corpus =
            toml::from_str(&std::fs::read_to_string(file).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        for entry in corpus.query {
            let sql = entry.sql.trim();
            assert!(
                FORMS.contains(&entry.form.as_str()),
                "{name}: unknown form {:?}",
                entry.form
            );
            for rule in &entry.expect {
                let known = RULES.iter().any(|(r, _)| r == rule) || PLANNED_RULES.contains(&rule.as_str());
                assert!(known, "{name}: unknown rule {rule:?} for {sql}");
            }

            let report = scan(&[Query {
                text: sql.to_string(),
                calls: 1,
                line: 1,
            }]);
            let got: BTreeSet<&str> = report.findings.iter().map(|f| f.rule).collect();
            let want: BTreeSet<&str> = entry.expect.iter().map(String::as_str).collect();
            match (&entry.pending, report.unparsed == 0 && got == want) {
                _ if report.unparsed > 0 => failures.push(format!("{name}: does not parse: {sql}")),
                (None, false) => failures.push(format!("{name}: got {got:?}, want {want:?}: {sql}")),
                (Some(item), true) => failures.push(format!("{name}: {item} fixed this one, remove `pending`: {sql}")),
                _ => {}
            }
            checked += 1;
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} corpus queries:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
