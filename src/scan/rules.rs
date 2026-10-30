use std::collections::BTreeMap;

use pg_query::NodeEnum;
use pg_query::protobuf::{OnCommitAction, VariableSetKind};

use super::{Finding, Query, Report, Severity, scenarios_for};

// CURSOR_OPT_HOLD from parsenodes.h
const CURSOR_OPT_HOLD: i32 = 0x0020;

const SESSION_ADVISORY_LOCKS: &[&str] = &[
    "pg_advisory_lock",
    "pg_advisory_lock_shared",
    "pg_try_advisory_lock",
    "pg_try_advisory_lock_shared",
];

pub fn scan(queries: &[Query]) -> Report {
    // Merge duplicates so each query is reported once with its total calls.
    let mut merged: BTreeMap<&str, (u64, usize)> = BTreeMap::new();
    for q in queries {
        let entry = merged.entry(q.text.as_str()).or_insert((0, q.line));
        entry.0 += q.calls;
        entry.1 = entry.1.min(q.line);
    }

    let mut findings = Vec::new();
    let mut unparsed = 0;
    for (query, (calls, line)) in &merged {
        let Ok(parsed) = pg_query::parse(query, pg_query::ParserOptions::DEFAULT) else {
            unparsed += 1;
            continue;
        };
        let first = findings.len();
        // Per session-set finding: scenarios specific to the parameter, if any.
        let mut set_scenarios: Vec<Option<Vec<&'static str>>> = Vec::new();
        let mut push = |rule, severity, detail: String| {
            let scenarios = scenarios_for(rule);
            findings.push(Finding {
                rule,
                severity,
                detail,
                query: query.to_string(),
                calls: *calls,
                line: *line,
                scenarios,
                on_pooler: None,
            });
        };

        let starts_transaction = parsed.protobuf.stmts.first().is_some_and(|raw| {
            matches!(
                raw.stmt.as_ref().and_then(|s| s.node.as_ref()),
                Some(NodeEnum::TransactionStmt(_))
            )
        });
        if starts_transaction && parsed.protobuf.stmts.len() > 1 {
            push(
                "multi-statement-transaction",
                Severity::Warn,
                "This one query string starts with BEGIN, COMMIT or ROLLBACK and has more statements \
                 after it. Some poolers only handle the first statement and defer or drop the rest \
                 (PgDog: https://github.com/pgdogdev/pgdog/issues/1849). Send them separately."
                    .to_string(),
            );
        }

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
                        let target = if kind == VariableSetKind::VarResetAll {
                            "ALL"
                        } else {
                            set.name.as_str()
                        };
                        push(
                            "session-set",
                            Severity::Warn,
                            format!(
                                "SET/RESET of `{target}` outside SET LOCAL lasts for the session. \
                                 Some poolers track a few parameters per client; most leak the rest \
                                 to whoever gets the server connection next."
                            ),
                        );
                        let refined = match target {
                            "role" | "session_authorization" => Some(vec![
                                "set-role-isolated-between-clients",
                                "set-role-reset-after-pinned-disconnect",
                            ]),
                            "search_path" => Some(vec!["search-path-isolated-between-clients"]),
                            _ => None,
                        };
                        set_scenarios.push(refined);
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
                            "CREATE TEMP TABLE ... AS / SELECT INTO TEMP creates a session-scoped table.".to_string(),
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
                NodeEnum::DiscardStmt(_) => push(
                    "discard",
                    Severity::Warn,
                    "DISCARD resets the server connection it runs on. Under transaction pooling \
                     that connection is shared, and the pooler's view of it (tracked settings, \
                     prepared statements) can stop matching. It is usually only needed in session \
                     mode, where the pooler runs it for you."
                        .to_string(),
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
        let sets = findings[first..].iter_mut().filter(|f| f.rule == "session-set");
        for (finding, scenarios) in sets.zip(set_scenarios) {
            if let Some(scenarios) = scenarios {
                finding.scenarios = scenarios;
            }
        }
    }

    findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(b.calls.cmp(&a.calls)));
    Report {
        findings,
        queries: merged.len(),
        unparsed,
    }
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
    use crate::scan::RULES;

    fn query(text: &str, calls: u64) -> Query {
        Query {
            text: text.to_string(),
            calls,
            line: 1,
        }
    }

    fn rules(sql: &str) -> Vec<&'static str> {
        scan(&[query(sql, 1)]).findings.iter().map(|f| f.rule).collect()
    }

    #[test]
    fn flags_session_state() {
        assert_eq!(rules("SET search_path TO app"), vec!["session-set"]);
        assert_eq!(rules("RESET ALL"), vec!["session-set"]);
        assert_eq!(rules("SELECT pg_advisory_lock(42)"), vec!["session-advisory-lock"]);
        assert_eq!(rules("LISTEN jobs"), vec!["listen"]);
        assert_eq!(rules("CREATE TEMP TABLE t (x int)"), vec!["temp-table"]);
        assert_eq!(rules("SELECT 1 INTO TEMP t"), vec!["temp-table"]);
        assert_eq!(
            rules("DECLARE c CURSOR WITH HOLD FOR SELECT 1"),
            vec!["with-hold-cursor"]
        );
        assert_eq!(rules("PREPARE q AS SELECT 1"), vec!["sql-prepare"]);
        assert_eq!(
            rules("SELECT set_config('app.user', '1', false)"),
            vec!["session-set-config"]
        );
        assert_eq!(rules("SET ROLE app_reader"), vec!["session-set"]);
        assert_eq!(rules("DISCARD ALL"), vec!["discard"]);
        assert_eq!(
            rules("BEGIN; INSERT INTO t VALUES (1); COMMIT"),
            vec!["multi-statement-transaction"]
        );
        assert_eq!(rules("ROLLBACK; SELECT 1"), vec!["multi-statement-transaction"]);
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
        assert!(rules("BEGIN").is_empty());
        assert!(rules("SELECT 1; SELECT 2").is_empty());
    }

    #[test]
    fn merges_duplicate_queries() {
        let report = scan(&[
            query("LISTEN a", 2),
            Query {
                line: 7,
                ..query("LISTEN a", 3)
            },
        ]);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].calls, 5);
        assert_eq!(report.findings[0].line, 1);
    }

    #[test]
    fn findings_point_to_scenarios() {
        let scenarios = |sql: &str| scan(&[query(sql, 1)]).findings[0].scenarios.clone();
        assert_eq!(
            scenarios("SET work_mem = '1MB'"),
            vec!["set-isolated-between-clients", "set-does-not-leak-after-disconnect"]
        );
        assert_eq!(
            scenarios("SET search_path TO app"),
            vec!["search-path-isolated-between-clients"]
        );
        assert_eq!(
            scenarios("SET ROLE r"),
            vec![
                "set-role-isolated-between-clients",
                "set-role-reset-after-pinned-disconnect"
            ]
        );
        assert_eq!(scenarios("LISTEN a"), vec!["listen-notify-delivered"]);
        // Every scenario a rule points to must exist.
        let names: Vec<String> = crate::scenario::bundled()
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        for (rule, _) in RULES {
            for s in scenarios_for(rule) {
                assert!(names.iter().any(|n| n == s), "{rule} points to missing scenario {s}");
            }
        }
    }
}
