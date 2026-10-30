use std::collections::BTreeMap;

use pg_query::protobuf::{OnCommitAction, VariableSetKind, a_const};
use pg_query::{NodeEnum, NodeRef};

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
        }

        if let Some(is_local) = set_config_is_local(&parsed) {
            let detail = match is_local {
                IsLocal::False => {
                    "set_config(..., false) changes the setting for the session, which other \
                     clients of the same server connection will see. Pass true to scope it to \
                     the transaction."
                }
                IsLocal::Parameter => {
                    "set_config()'s third argument (is_local) is a parameter here, as in \
                     pg_stat_statements, which replaces literals. If it is false, the setting \
                     lasts for the session and other clients of the same server connection will \
                     see it. Check the value: true scopes it to the transaction."
                }
                IsLocal::Expression => {
                    "set_config()'s third argument (is_local) is not a literal. If it is false, \
                     the setting lasts for the session and other clients of the same server \
                     connection will see it."
                }
            };
            push("session-set-config", Severity::Warn, detail.to_string());
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

/// What a `set_config(name, value, is_local)` call passes as `is_local`, when it
/// could be false. Ordered from least to most certain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum IsLocal {
    Expression,
    Parameter,
    False,
}

/// The most certain session-scoped `is_local` among the query's set_config calls,
/// or None if every call passes literal true.
fn set_config_is_local(parsed: &pg_query::ParseResult) -> Option<IsLocal> {
    parsed
        .protobuf
        .nodes()
        .into_iter()
        .filter_map(|(node, ..)| match node {
            NodeRef::FuncCall(call) => Some(call),
            _ => None,
        })
        .filter(|call| {
            let name = call.funcname.last().and_then(|n| n.node.as_ref());
            matches!(name, Some(NodeEnum::String(s)) if s.sval == "set_config")
        })
        .filter_map(|call| match call.args.get(2).and_then(|a| a.node.as_ref()) {
            Some(NodeEnum::AConst(c)) => match &c.val {
                Some(a_const::Val::Boolval(b)) if b.boolval => None,
                Some(a_const::Val::Boolval(_)) => Some(IsLocal::False),
                _ => Some(IsLocal::Expression),
            },
            Some(NodeEnum::ParamRef(_)) => Some(IsLocal::Parameter),
            _ => Some(IsLocal::Expression),
        })
        .max()
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
    fn set_config_checks_its_own_is_local_argument() {
        let detail = |sql: &str| {
            let findings = scan(&[query(sql, 1)]).findings;
            assert!(findings.iter().all(|f| f.rule == "session-set-config"), "{sql}");
            assert!(findings.len() <= 1, "{sql}: one finding per query");
            findings.first().map(|f| f.detail.clone())
        };
        assert!(
            detail("SELECT set_config('a', 'b', false)")
                .unwrap()
                .contains("set_config(..., false)")
        );
        assert!(detail("SELECT pg_catalog.set_config('search_path', '', false)").is_some());
        assert_eq!(detail("SELECT set_config('a', 'b', true)"), None);
        // pg_stat_statements turns the literal into a parameter.
        assert!(detail("SELECT set_config($1, $2, $3)").unwrap().contains("parameter"));
        assert!(
            detail("select set_config($1, $2, $3), set_config($4, $5, $6)")
                .unwrap()
                .contains("parameter")
        );
        assert!(
            detail("SELECT set_config('a', 'b', current_setting('x')::bool)")
                .unwrap()
                .contains("not a literal")
        );
        // A false elsewhere in the query is not set_config's argument.
        assert_eq!(
            detail("SELECT set_config('a', $1, true) FROM t WHERE archived = false"),
            None
        );
        assert_eq!(detail("SELECT setval('s', 1, false)"), None);
        // The most certain finding wins.
        assert!(
            detail("SELECT set_config('a', 'b', $1), set_config('c', 'd', false)")
                .unwrap()
                .contains("set_config(..., false)")
        );
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
