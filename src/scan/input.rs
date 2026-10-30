use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::Query;

/// What `scan` reads. Without `--input`, the file extension decides: `.csv` is
/// pg_stat_statements or csvlog (told apart by the first row), `.log` stderr log,
/// `.json` jsonlog, anything else SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum InputFormat {
    /// pg_stat_statements export as CSV, with a header row (`query`, optional `calls`).
    Pgss,
    /// Postgres csvlog (`log_destination = 'csvlog'`).
    Csvlog,
    /// Postgres stderr log.
    Log,
    /// Postgres jsonlog.
    Jsonlog,
    /// SQL statements, e.g. migrations or a pg_dump script.
    Sql,
}

/// Load the queries in `path` (`-` for stdin), each once with its total calls and
/// the line where it first appears. Logs and CSV are read as a stream, so memory
/// grows with the number of distinct queries, not the size of the input.
pub fn load_queries(path: &Path, format: Option<InputFormat>) -> Result<Vec<Query>> {
    let stdin = path == Path::new("-");
    let format = match format {
        Some(format) => Some(format),
        None if stdin => bail!("reading stdin needs --input to say what it is"),
        None => match path.extension().and_then(|e| e.to_str()) {
            Some("csv") => None,
            Some("log") => Some(InputFormat::Log),
            Some("json") => Some(InputFormat::Jsonlog),
            _ => Some(InputFormat::Sql),
        },
    };
    let name = if stdin {
        "stdin".to_string()
    } else {
        path.display().to_string()
    };
    let reader: Box<dyn BufRead> = if stdin {
        Box::new(io::stdin().lock())
    } else {
        let file = File::open(path).with_context(|| format!("reading {name}"))?;
        Box::new(BufReader::with_capacity(1 << 16, file))
    };
    load(reader, format).with_context(|| format!("reading {name}"))
}

/// `format` None means a CSV file of either kind.
fn load(mut reader: impl BufRead, format: Option<InputFormat>) -> Result<Vec<Query>> {
    let mut queries = Queries::default();
    let log_lines = match format {
        None => load_csv(reader, None, &mut queries)?,
        Some(InputFormat::Pgss) => load_csv(reader, Some(CsvKind::Pgss), &mut queries)?,
        Some(InputFormat::Csvlog) => load_csv(reader, Some(CsvKind::Csvlog), &mut queries)?,
        Some(InputFormat::Log) => load_stderr_log(reader, &mut queries)?,
        Some(InputFormat::Jsonlog) => load_json_log(reader, &mut queries)?,
        Some(InputFormat::Sql) => {
            let mut text = String::new();
            reader.read_to_string(&mut text)?;
            for q in load_sql(&text)? {
                queries.add(&q.text, q.calls, q.line);
            }
            0
        }
    };
    if queries.0.is_empty() && log_lines > 0 {
        bail!(
            "found no logged statements in {log_lines} log lines. Statements are logged with \
             log_statement = 'all' or log_min_duration_statement. poolproof reads the English \
             message prefixes (LOG, statement:, duration:), so the server needs lc_messages = 'C' \
             or another English locale."
        );
    }
    Ok(queries.into_vec())
}

/// Queries merged by text while reading: total calls and first line.
#[derive(Default)]
struct Queries(HashMap<String, (u64, usize)>);

impl Queries {
    fn add(&mut self, text: &str, calls: u64, line: usize) {
        match self.0.get_mut(text) {
            Some(entry) => {
                entry.0 += calls;
                entry.1 = entry.1.min(line);
            }
            None => {
                self.0.insert(text.to_string(), (calls, line));
            }
        }
    }

    fn into_vec(self) -> Vec<Query> {
        let mut out: Vec<Query> = self
            .0
            .into_iter()
            .map(|(text, (calls, line))| Query { text, calls, line })
            .collect();
        out.sort_by(|a, b| a.line.cmp(&b.line).then_with(|| a.text.cmp(&b.text)));
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CsvKind {
    Pgss,
    Csvlog,
}

// csvlog columns, fixed since Postgres 13 (23 columns there, 26 from 14).
const CSVLOG_MIN_COLUMNS: usize = 23;
const CSVLOG_SEVERITY: usize = 11;
const CSVLOG_MESSAGE: usize = 13;

/// A pg_stat_statements export or a csvlog file. `want` None tells them apart by
/// the first row: a header with a `query` column, or a row starting with a timestamp.
/// Returns the number of csvlog rows read (0 for pg_stat_statements).
fn load_csv(reader: impl BufRead, want: Option<CsvKind>, out: &mut Queries) -> Result<usize> {
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(reader);
    let mut record = csv::ByteRecord::new();
    if !csv.read_byte_record(&mut record)? {
        return Ok(0);
    }
    let query_col = record.iter().position(|f| f == b"query");
    let csvlog = record.len() >= CSVLOG_MIN_COLUMNS && record.get(0).is_some_and(is_timestamp);
    let kind = match (want, query_col.is_some(), csvlog) {
        (None | Some(CsvKind::Pgss), true, _) => CsvKind::Pgss,
        (None | Some(CsvKind::Csvlog), false, true) => CsvKind::Csvlog,
        (Some(CsvKind::Pgss), false, true) => {
            bail!("no `query` column in the first row; this looks like csvlog (--input csvlog)")
        }
        (Some(CsvKind::Pgss), false, false) => bail!("no `query` column in the first row"),
        (Some(CsvKind::Csvlog), true, _) => bail!(
            "the first row is a header with a `query` column; this looks like a pg_stat_statements \
             export (--input pgss), not csvlog"
        ),
        (Some(CsvKind::Csvlog), false, false) => {
            bail!("the first row is not a csvlog row (a timestamp, then at least {CSVLOG_MIN_COLUMNS} columns)")
        }
        (None, false, false) => bail!(
            "can't tell what this CSV is: a pg_stat_statements export starts with a header row \
             with a `query` column, csvlog rows start with a timestamp. Use --input pgss or csvlog"
        ),
    };
    let line = |r: &csv::ByteRecord| r.position().map_or(0, |p| p.line() as usize);

    if let (CsvKind::Pgss, Some(query_col)) = (kind, query_col) {
        let calls_col = record.iter().position(|f| f == b"calls");
        while csv.read_byte_record(&mut record)? {
            let text = String::from_utf8_lossy(record.get(query_col).unwrap_or_default());
            let calls = calls_col
                .and_then(|c| record.get(c))
                .and_then(|c| std::str::from_utf8(c).ok()?.parse().ok())
                .unwrap_or(1);
            out.add(&text, calls, line(&record));
        }
        return Ok(0);
    }

    let mut rows = 0;
    loop {
        rows += 1;
        // Only LOG rows carry statements. With a non-English lc_messages the severity is
        // translated too, which the caller reports as "no statements found".
        if record.get(CSVLOG_SEVERITY) == Some(b"LOG".as_slice()) {
            let message = String::from_utf8_lossy(record.get(CSVLOG_MESSAGE).unwrap_or_default());
            if let Some(sql) = logged_sql(&message) {
                out.add(sql, 1, line(&record));
            }
        }
        if !csv.read_byte_record(&mut record)? {
            return Ok(rows);
        }
    }
}

/// `YYYY-MM-DD HH:MM:SS`, how csvlog starts each row.
fn is_timestamp(field: &[u8]) -> bool {
    field.len() >= 19
        && field[..19].iter().enumerate().all(|(i, &c)| match i {
            4 | 7 => c == b'-',
            10 => c == b' ',
            13 | 16 => c == b':',
            _ => c.is_ascii_digit(),
        })
}

/// Lines of a log, without the newline. Invalid UTF-8 (a log can hold anything a
/// client sent) is replaced rather than failing the whole file.
fn for_each_line(mut reader: impl BufRead, mut f: impl FnMut(usize, &str)) -> Result<usize> {
    let mut buf = Vec::new();
    let mut n = 0;
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            return Ok(n);
        }
        n += 1;
        let line = String::from_utf8_lossy(&buf);
        f(n, line.trim_end_matches(['\n', '\r']));
    }
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
/// Returns the number of lines read.
fn load_stderr_log(reader: impl BufRead, out: &mut Queries) -> Result<usize> {
    let mut open: Option<(String, usize)> = None;
    let lines = for_each_line(reader, |n, line| {
        if let (Some((text, _)), Some(rest)) = (open.as_mut(), line.strip_prefix('\t')) {
            text.push('\n');
            text.push_str(rest);
            return;
        }
        if let Some((text, first)) = open.take() {
            out.add(&text, 1, first);
        }
        let sql = line.split_once("LOG:  ").and_then(|(_, message)| logged_sql(message));
        open = sql.map(|sql| (sql.to_string(), n));
    })?;
    if let Some((text, first)) = open {
        out.add(&text, 1, first);
    }
    Ok(lines)
}

/// Postgres jsonlog: one JSON object per line with a `message` field.
/// Returns the number of lines read.
fn load_json_log(reader: impl BufRead, out: &mut Queries) -> Result<usize> {
    for_each_line(reader, |n, line| {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        if let Some(sql) = entry.get("message").and_then(|m| m.as_str()).and_then(logged_sql) {
            out.add(sql, 1, n);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str, format: Option<InputFormat>) -> Result<Vec<(String, u64, usize)>> {
        Ok(load(text.as_bytes(), format)?
            .into_iter()
            .map(|q| (q.text, q.calls, q.line))
            .collect())
    }

    fn q(text: &str, calls: u64, line: usize) -> (String, u64, usize) {
        (text.to_string(), calls, line)
    }

    fn error(text: &str, format: Option<InputFormat>) -> String {
        format!("{:#}", read(text, format).unwrap_err())
    }

    /// A csvlog row as Postgres 17 writes it: 26 columns.
    fn csvlog_row(severity: &str, message: &str) -> String {
        format!(
            "2026-10-30 12:00:00.123 UTC,\"app\",\"db\",42,\"10.0.0.1:5000\",6720a1b0.2a,3,\"SELECT\",\
             2026-10-30 11:59:00 UTC,3/7,0,{severity},00000,\"{message}\",,,,,,,,,\"psql\",client backend,,0\n"
        )
    }

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
2026-10-30 12:00:00.500 UTC [43] app@db LOG:  execute S_1: LISTEN jobs
";
        assert_eq!(
            read(log, Some(InputFormat::Log)).unwrap(),
            vec![
                q("BEGIN; INSERT INTO t VALUES (1); COMMIT", 1, 1),
                q("SELECT *\nFROM t", 1, 2),
                q("SELECT pg_advisory_lock($1)", 1, 4),
                q("LISTEN jobs", 2, 6),
            ]
        );
    }

    #[test]
    fn stderr_log_with_invalid_utf8() {
        let mut log = b"x LOG:  statement: SELECT '".to_vec();
        log.extend_from_slice(&[0xff, 0xfe]);
        log.extend_from_slice(b"'\nx LOG:  statement: LISTEN a\n");
        let got: Vec<_> = load(log.as_slice(), Some(InputFormat::Log)).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].text, "LISTEN a");
    }

    #[test]
    fn json_log() {
        let log = r#"{"timestamp":"2026-10-30 12:00:00 UTC","error_severity":"LOG","message":"statement: LISTEN jobs"}
{"timestamp":"2026-10-30 12:00:01 UTC","error_severity":"LOG","message":"connection received: host=x"}
{"timestamp":"2026-10-30 12:00:02 UTC","error_severity":"LOG","message":"duration: 2.1 ms  execute <unnamed>: SELECT 1"}"#;
        assert_eq!(
            read(log, Some(InputFormat::Jsonlog)).unwrap(),
            vec![q("LISTEN jobs", 1, 1), q("SELECT 1", 1, 3)]
        );
    }

    #[test]
    fn csv_detects_pg_stat_statements() {
        let csv = "userid,query,calls\n10,SELECT pg_advisory_lock($1),48211\n10,\"SET search_path\nTO $1\",3\n";
        let want = vec![
            q("SELECT pg_advisory_lock($1)", 48211, 2),
            q("SET search_path\nTO $1", 3, 3),
        ];
        assert_eq!(read(csv, None).unwrap(), want);
        assert_eq!(read(csv, Some(InputFormat::Pgss)).unwrap(), want);
        // Without a calls column every query counts once.
        assert_eq!(read("query\nLISTEN a\n", None).unwrap(), vec![q("LISTEN a", 1, 2)]);
    }

    #[test]
    fn csv_detects_csvlog() {
        let log = [
            csvlog_row("LOG", "statement: SELECT pg_advisory_lock(42)"),
            csvlog_row("LOG", "connection authorized: user=app"),
            csvlog_row("ERROR", "statement: LISTEN not_a_statement_log"),
            csvlog_row("LOG", "duration: 0.3 ms  execute <unnamed>: SELECT *\nFROM t"),
        ]
        .concat();
        let want = vec![q("SELECT pg_advisory_lock(42)", 1, 1), q("SELECT *\nFROM t", 1, 4)];
        assert_eq!(read(&log, None).unwrap(), want);
        assert_eq!(read(&log, Some(InputFormat::Csvlog)).unwrap(), want);
    }

    #[test]
    fn csv_kind_mismatch_is_an_error() {
        let pgss = "query,calls\nLISTEN a,1\n";
        let csvlog = csvlog_row("LOG", "statement: LISTEN a");
        assert!(error(pgss, Some(InputFormat::Csvlog)).contains("--input pgss"));
        assert!(error(&csvlog, Some(InputFormat::Pgss)).contains("--input csvlog"));
        assert!(error("a,b\n1,2\n", None).contains("can't tell"));
        assert!(error("a,b\n1,2\n", Some(InputFormat::Csvlog)).contains("not a csvlog row"));
        assert!(error("a,b\n1,2\n", Some(InputFormat::Pgss)).contains("no `query` column"));
    }

    #[test]
    fn non_english_log_says_why_nothing_was_found() {
        // lc_messages = 'de_DE': "LOG" and "statement" are translated.
        let log = "2026-10-30 12:00:00 UTC [42] LOG:  Anweisung: SELECT 1\n";
        assert!(error(log, Some(InputFormat::Log)).contains("lc_messages"));
        let csvlog = csvlog_row("LOG", "Anweisung: SELECT 1");
        assert!(error(&csvlog, None).contains("lc_messages"));
        // An empty log is not an error.
        assert_eq!(read("", Some(InputFormat::Log)).unwrap(), vec![]);
    }

    #[test]
    fn stdin_needs_a_format() {
        let err = load_queries(Path::new("-"), None).unwrap_err();
        assert!(err.to_string().contains("--input"));
    }
}
