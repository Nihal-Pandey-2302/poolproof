use std::path::Path;

use anyhow::{Context, Result};

use super::Query;

/// Load queries from a pg_stat_statements CSV export (needs a `query` column,
/// `calls` is optional), a Postgres log (`.log` for stderr format, `.json` for
/// jsonlog), or a plain .sql file.
pub fn load_queries(path: &Path) -> Result<Vec<Query>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    match path.extension().and_then(|e| e.to_str()) {
        Some("csv") => load_csv(&text).with_context(|| format!("reading {}", path.display())),
        Some("log") => Ok(load_stderr_log(&text)),
        Some("json") => Ok(load_json_log(&text)),
        _ => load_sql(&text).with_context(|| format!("splitting {}", path.display())),
    }
}

fn load_csv(text: &str) -> Result<Vec<Query>> {
    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let headers = reader.headers()?.clone();
    let Some(query_col) = headers.iter().position(|h| h == "query") else {
        anyhow::bail!("no `query` column");
    };
    let calls_col = headers.iter().position(|h| h == "calls");
    let mut out = Vec::new();
    for record in reader.records() {
        let record = record?;
        out.push(Query {
            text: record.get(query_col).unwrap_or_default().to_string(),
            calls: calls_col
                .and_then(|c| record.get(c))
                .and_then(|c| c.parse().ok())
                .unwrap_or(1),
            line: record.position().map_or(0, |p| p.line() as usize),
        });
    }
    Ok(out)
}

fn load_sql(text: &str) -> Result<Vec<Query>> {
    let statements = pg_query::split_with_parser(text)
        .or_else(|_| pg_query::split_with_scanner(text))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(statements
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            // The splitter returns slices of `text`, so the offset gives the line.
            let start = s.as_ptr() as usize - text.as_ptr() as usize + (s.len() - s.trim_start().len());
            Query {
                text: s.trim().to_string(),
                calls: 1,
                line: text[..start].matches('\n').count() + 1,
            }
        })
        .collect())
}

/// The SQL in a log message from `log_statement` or `log_min_duration_statement`:
/// `statement: ...` or `execute <name>: ...`, optionally after `duration: N ms  `.
fn logged_sql(message: &str) -> Option<&str> {
    let message = match message.strip_prefix("duration: ") {
        Some(rest) => rest.split_once(" ms  ")?.1,
        None => message,
    };
    if let Some(sql) = message.strip_prefix("statement: ") {
        return Some(sql);
    }
    message.strip_prefix("execute ")?.split_once(": ").map(|(_, sql)| sql)
}

/// Postgres stderr log. The line prefix is configurable, so look for `LOG:  `.
/// Continuation lines of a multi-line statement start with a tab.
fn load_stderr_log(text: &str) -> Vec<Query> {
    let mut out: Vec<Query> = Vec::new();
    let mut open = false;
    for (i, line) in text.lines().enumerate() {
        if let (true, Some(rest)) = (open, line.strip_prefix('\t')) {
            let last = out.last_mut().expect("open implies a query");
            last.text.push('\n');
            last.text.push_str(rest);
            continue;
        }
        let sql = line.split_once("LOG:  ").and_then(|(_, message)| logged_sql(message));
        open = sql.is_some();
        if let Some(sql) = sql {
            out.push(Query {
                text: sql.to_string(),
                calls: 1,
                line: i + 1,
            });
        }
    }
    out
}

/// Postgres jsonlog: one JSON object per line with a `message` field.
fn load_json_log(text: &str) -> Vec<Query> {
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let entry: serde_json::Value = serde_json::from_str(line).ok()?;
            let sql = logged_sql(entry.get("message")?.as_str()?)?;
            Some(Query {
                text: sql.to_string(),
                calls: 1,
                line: i + 1,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_file_lines() {
        let queries = load_sql("SELECT 1;\n\n  LISTEN a;\nSELECT\n  2;").unwrap();
        let lines: Vec<_> = queries.iter().map(|q| (q.text.as_str(), q.line)).collect();
        assert_eq!(lines, vec![("SELECT 1", 1), ("LISTEN a", 3), ("SELECT\n  2", 4)]);
    }

    #[test]
    fn stderr_log() {
        let log = "\
2026-10-30 12:00:00.000 UTC [42] app@db LOG:  statement: BEGIN; INSERT INTO t VALUES (1); COMMIT
2026-10-30 12:00:00.100 UTC [42] app@db LOG:  duration: 0.512 ms  statement: SELECT *
\tFROM t
2026-10-30 12:00:00.200 UTC [42] app@db LOG:  execute <unnamed>: SELECT pg_advisory_lock($1)
2026-10-30 12:00:00.200 UTC [42] app@db DETAIL:  parameters: $1 = '7'
2026-10-30 12:00:00.300 UTC [42] app@db LOG:  duration: 1.000 ms  execute S_1: LISTEN jobs
2026-10-30 12:00:00.400 UTC [42] app@db LOG:  connection authorized: user=app
";
        let queries = load_stderr_log(log);
        let got: Vec<_> = queries.iter().map(|q| (q.text.as_str(), q.line)).collect();
        assert_eq!(
            got,
            vec![
                ("BEGIN; INSERT INTO t VALUES (1); COMMIT", 1),
                ("SELECT *\nFROM t", 2),
                ("SELECT pg_advisory_lock($1)", 4),
                ("LISTEN jobs", 6),
            ]
        );
    }

    #[test]
    fn json_log() {
        let log = r#"{"timestamp":"2026-10-30 12:00:00 UTC","error_severity":"LOG","message":"statement: LISTEN jobs"}
{"timestamp":"2026-10-30 12:00:01 UTC","error_severity":"LOG","message":"connection received: host=x"}
{"timestamp":"2026-10-30 12:00:02 UTC","error_severity":"LOG","message":"duration: 2.1 ms  execute <unnamed>: SELECT 1"}"#;
        let got: Vec<_> = load_json_log(log).into_iter().map(|q| (q.text, q.line)).collect();
        assert_eq!(got, vec![("LISTEN jobs".to_string(), 1), ("SELECT 1".to_string(), 3)]);
    }
}
