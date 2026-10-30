use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use pg_query::NodeEnum;
use pg_query::protobuf::{OnCommitAction, VariableSetKind};
use serde::Serialize;

/// Severity of a finding under transaction pooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Depends on the pooler and its config. Check before switching.
    Warn,
    /// Breaks or leaks state under transaction pooling.
    Breaks,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule: &'static str,
    pub severity: Severity,
    pub detail: String,
    pub query: String,
    pub calls: u64,
}

pub struct Report {
    pub findings: Vec<Finding>,
    pub queries: usize,
    pub unparsed: usize,
}

// CURSOR_OPT_HOLD from parsenodes.h
const CURSOR_OPT_HOLD: i32 = 0x0020;

const SESSION_ADVISORY_LOCKS: &[&str] = &[
    "pg_advisory_lock",
    "pg_advisory_lock_shared",
    "pg_try_advisory_lock",
    "pg_try_advisory_lock_shared",
];

/// Load queries from a pg_stat_statements CSV export (needs a `query` column,
/// `calls` is optional) or from a plain .sql file.
pub fn load_queries(path: &Path) -> Result<Vec<(String, u64)>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    if path.extension().is_some_and(|e| e == "csv") {
        let mut reader = csv::Reader::from_reader(text.as_bytes());
        let headers = reader.headers()?.clone();
        let Some(query_col) = headers.iter().position(|h| h == "query") else {
            anyhow::bail!("{} has no `query` column", path.display());
        };
        let calls_col = headers.iter().position(|h| h == "calls");
        let mut out = Vec::new();
        for record in reader.records() {
            let record = record?;
            let query = record.get(query_col).unwrap_or_default().to_string();
            let calls = calls_col
                .and_then(|c| record.get(c))
                .and_then(|c| c.parse().ok())
                .unwrap_or(1);
            out.push((query, calls));
        }
        return Ok(out);
    }
    let statements = pg_query::split_with_parser(&text)
        .or_else(|_| pg_query::split_with_scanner(&text))
        .map_err(|e| anyhow::anyhow!("splitting {}: {e}", path.display()))?;
    Ok(statements.into_iter().map(|s| (s.trim().to_string(), 1)).filter(|(s, _)| !s.is_empty()).collect())
}

pub fn scan(queries: &[(String, u64)]) -> Report {
    // Merge duplicates so each query is reported once with its total calls.
    let mut merged: BTreeMap<&str, u64> = BTreeMap::new();
    for (query, calls) in queries {
        *merged.entry(query.as_str()).or_default() += calls;
    }

    let mut findings = Vec::new();
    let mut unparsed = 0;
    for (query, calls) in &merged {
        let Ok(parsed) = pg_query::parse(query, pg_query::ParserOptions::DEFAULT) else {
            unparsed += 1;
            continue;
        };
        let mut push = |rule, severity, detail: String| {
            findings.push(Finding { rule, severity, detail, query: query.to_string(), calls: *calls });
        };

        for function in parsed.functions() {
            let short = function.rsplit('.').next().unwrap_or(&function);
            if SESSION_ADVISORY_LOCKS.contains(&short) {
                push(
                    "session-advisory-lock",
                    Severity::Breaks,
                    format!(
                        "{short}() holds the lock for the whole session. Under transaction pooling \
                         the server connection goes back to the pool still holding it. Use the \
                         pg_advisory_xact_* variant."
                    ),
                );
            }
            if short == "set_config" && has_false_literal(&parsed) {
                push(
                    "session-set-config",
                    Severity::Warn,
                    "set_config(..., false) changes the setting for the session, which other \
                     clients of the same server connection will see. Pass true to scope it to \
                     the transaction."
                        .to_string(),
                );
            }
        }

        for raw in &parsed.protobuf.stmts {
            let Some(node) = raw.stmt.as_ref().and_then(|s| s.node.as_ref()) else {
                continue;
            };
            match node {
                NodeEnum::VariableSetStmt(set) => {
                    let kind = VariableSetKind::try_from(set.kind).unwrap_or(VariableSetKind::Undefined);
                    let transaction_scoped =
                        set.is_local || (kind == VariableSetKind::VarSetMulti && set.name == "TRANSACTION");
                    if !transaction_scoped {
                        let target = if kind == VariableSetKind::VarResetAll { "ALL" } else { set.name.as_str() };
                        push(
                            "session-set",
                            Severity::Warn,
                            format!(
                                "SET/RESET of `{target}` outside SET LOCAL lasts for the session. \
                                 Some poolers track a few parameters per client; most leak the rest \
                                 to whoever gets the server connection next."
                            ),
                        );
                    }
                }
                NodeEnum::ListenStmt(listen) => push(
                    "listen",
                    Severity::Breaks,
                    format!(
                        "LISTEN {} needs a dedicated session. Notifications go to whichever client \
                         holds the server connection, or nobody.",
                        listen.conditionname
                    ),
                ),
                NodeEnum::CreateStmt(create) => {
                    let temp = create.relation.as_ref().is_some_and(|r| r.relpersistence == "t");
                    let dropped_at_commit = create.oncommit == OnCommitAction::OncommitDrop as i32;
                    if temp && !dropped_at_commit {
                        push(
                            "temp-table",
                            Severity::Breaks,
                            "Temp tables live for the session. Other clients can see them and later \
                             transactions may land on a server connection without them. Use ON \
                             COMMIT DROP inside a transaction."
                                .to_string(),
                        );
                    }
                }
                NodeEnum::CreateTableAsStmt(ctas) => {
                    let temp = ctas
                        .into
                        .as_ref()
                        .and_then(|i| i.rel.as_ref())
                        .is_some_and(|r| r.relpersistence == "t");
                    let dropped_at_commit = ctas
                        .into
                        .as_ref()
                        .is_some_and(|i| i.on_commit == OnCommitAction::OncommitDrop as i32);
                    if temp && !dropped_at_commit {
                        push(
                            "temp-table",
                            Severity::Breaks,
                            "CREATE TEMP TABLE ... AS / SELECT INTO TEMP creates a session-scoped table."
                                .to_string(),
                        );
                    }
                }
                NodeEnum::SelectStmt(select) => {
                    let temp = select
                        .into_clause
                        .as_ref()
                        .and_then(|i| i.rel.as_ref())
                        .is_some_and(|r| r.relpersistence == "t");
                    if temp {
                        push(
                            "temp-table",
                            Severity::Breaks,
                            "SELECT ... INTO TEMP creates a session-scoped table.".to_string(),
                        );
                    }
                }
                NodeEnum::DeclareCursorStmt(cursor) if cursor.options & CURSOR_OPT_HOLD != 0 => push(
                    "with-hold-cursor",
                    Severity::Breaks,
                    format!(
                        "Cursor {} is WITH HOLD, so it outlives the transaction. Later FETCHes can \
                         land on a different server connection.",
                        cursor.portalname
                    ),
                ),
                NodeEnum::PrepareStmt(prepare) => push(
                    "sql-prepare",
                    Severity::Warn,
                    format!(
                        "SQL-level PREPARE {} creates a session object. Poolers that handle \
                         protocol-level prepared statements often don't track this form.",
                        prepare.name
                    ),
                ),
                _ => {}
            }
        }
    }

    findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(b.calls.cmp(&a.calls)));
    Report { findings, queries: merged.len(), unparsed }
}

/// True if the statement passes a literal `false` anywhere. Good enough to tell
/// `set_config(x, y, false)` from `set_config(x, y, true)`.
fn has_false_literal(parsed: &pg_query::ParseResult) -> bool {
    let Ok(json) = serde_json::to_value(&parsed.protobuf) else {
        return false;
    };
    fn walk(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Object(b)) = map.get("Boolval")
                    && b.get("boolval") == Some(&serde_json::Value::Bool(false))
                {
                    return true;
                }
                map.values().any(walk)
            }
            serde_json::Value::Array(items) => items.iter().any(walk),
            _ => false,
        }
    }
    walk(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(sql: &str) -> Vec<&'static str> {
        scan(&[(sql.to_string(), 1)]).findings.iter().map(|f| f.rule).collect()
    }

    #[test]
    fn flags_session_state() {
        assert_eq!(rules("SET search_path TO app"), vec!["session-set"]);
        assert_eq!(rules("RESET ALL"), vec!["session-set"]);
        assert_eq!(rules("SELECT pg_advisory_lock(42)"), vec!["session-advisory-lock"]);
        assert_eq!(rules("LISTEN jobs"), vec!["listen"]);
        assert_eq!(rules("CREATE TEMP TABLE t (x int)"), vec!["temp-table"]);
        assert_eq!(rules("SELECT 1 INTO TEMP t"), vec!["temp-table"]);
        assert_eq!(rules("DECLARE c CURSOR WITH HOLD FOR SELECT 1"), vec!["with-hold-cursor"]);
        assert_eq!(rules("PREPARE q AS SELECT 1"), vec!["sql-prepare"]);
        assert_eq!(rules("SELECT set_config('app.user', '1', false)"), vec!["session-set-config"]);
    }

    #[test]
    fn ignores_transaction_scoped_state() {
        assert!(rules("SET LOCAL search_path TO app").is_empty());
        assert!(rules("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE").is_empty());
        assert!(rules("SELECT pg_advisory_xact_lock(42)").is_empty());
        assert!(rules("CREATE TEMP TABLE t (x int) ON COMMIT DROP").is_empty());
        assert!(rules("DECLARE c CURSOR FOR SELECT 1").is_empty());
        assert!(rules("SELECT set_config('app.user', '1', true)").is_empty());
        assert!(rules("SELECT * FROM users WHERE id = $1").is_empty());
    }

    #[test]
    fn merges_duplicate_queries() {
        let report = scan(&[("LISTEN a".into(), 2), ("LISTEN a".into(), 3)]);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].calls, 5);
    }
}
