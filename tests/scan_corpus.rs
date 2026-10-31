//! Runs every query in `tests/fixtures/scan/*.toml` through the input path its `form`
//! names, then `scan`, and checks the findings.
//! See `tests/fixtures/scan/README.md` for the format.

use std::collections::BTreeSet;
use std::path::Path;

use poolproof::scan::{InputFormat, RULES, read_queries, scan};
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

/// The query as scan would read it in its `form`: a pg_stat_statements CSV export,
/// a stderr log line (continuation lines start with a tab), stderr log lines whose
/// messages are given as they are (`log-lines`, one per line), or a SQL file.
fn as_input(form: &str, sql: &str) -> Option<(String, Option<InputFormat>)> {
    match form {
        // No format given, so the CSV detection runs too.
        "pgss" | "pgss-17" | "pgss-18" => Some((format!("query,calls\n\"{}\",1\n", sql.replace('"', "\"\"")), None)),
        "log" => Some((
            format!(
                "2026-10-31 09:00:00.000 UTC [4242] app@db LOG:  statement: {}\n",
                sql.replace('\n', "\n\t")
            ),
            Some(InputFormat::Log),
        )),
        "log-lines" => Some((
            sql.lines()
                .map(|message| format!("2026-10-31 09:00:00.000 UTC [4242] app@db LOG:  {}\n", message.trim()))
                .collect(),
            Some(InputFormat::Log),
        )),
        "sql" => Some((format!("{sql};\n"), Some(InputFormat::Sql))),
        _ => None,
    }
}
/// Rule IDs reserved for rules not written yet, so `expect` can name them.
const PLANNED_RULES: &[&str] = &[];

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
            checked += 1;
            let (input, format) =
                as_input(&entry.form, sql).unwrap_or_else(|| panic!("{name}: unknown form {:?}", entry.form));
            for rule in &entry.expect {
                let known = RULES.iter().any(|(r, _)| r == rule) || PLANNED_RULES.contains(&rule.as_str());
                assert!(known, "{name}: unknown rule {rule:?} for {sql}");
            }

            let queries = match read_queries(input.as_bytes(), format) {
                Ok(queries) if !queries.is_empty() => queries,
                Ok(_) => {
                    failures.push(format!("{name}: {} input has no queries: {sql}", entry.form));
                    continue;
                }
                Err(e) => {
                    failures.push(format!("{name}: {} input: {e:#}: {sql}", entry.form));
                    continue;
                }
            };
            let report = scan(&queries);
            let got: BTreeSet<&str> = report.findings.iter().map(|f| f.rule).collect();
            let want: BTreeSet<&str> = entry.expect.iter().map(String::as_str).collect();
            match (&entry.pending, report.unparsed == 0 && got == want) {
                _ if report.unparsed > 0 => failures.push(format!("{name}: does not parse: {sql}")),
                (None, false) => failures.push(format!("{name}: got {got:?}, want {want:?}: {sql}")),
                (Some(item), true) => failures.push(format!("{name}: {item} fixed this one, remove `pending`: {sql}")),
                _ => {}
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} corpus queries:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
