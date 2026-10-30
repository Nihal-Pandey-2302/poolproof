use std::collections::{BTreeMap, BTreeSet};

use pg_query::protobuf::{FuncCall, OnCommitAction, VariableSetKind, a_const};
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

        let calls = function_calls(&parsed);
        let names: BTreeSet<&str> = calls.iter().filter_map(|call| function_name(call)).collect();
        for short in names.into_iter().filter(|n| SESSION_ADVISORY_LOCKS.contains(n)) {
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

        if let Some(is_local) = set_config_is_local(&calls) {
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
                NodeEnum::DeallocateStmt(deallocate) if deallocate.isall => push(
                    "deallocate-all",
                    Severity::Warn,
                    "DEALLOCATE ALL drops every prepared statement on the server connection, \
                     including other clients' when it is shared. Poolers that track protocol-level \
                     prepared statements can lose track of them. Drivers send it to recover from \
                     \"prepared statement does not exist\"; with a pooler that tracks statements \
                     it is usually not needed."
                        .to_string(),
                ),
                NodeEnum::LoadStmt(load) => push(
                    "load",
                    Severity::Warn,
                    format!(
                        "LOAD '{}' loads a library into the server process for the rest of the \
                         session. Under transaction pooling, later transactions may run on a \
                         connection without it, and other clients get it where they didn't ask.",
                        load.filename
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

/// Every function call the query runs, including inside PREPARE and DECLARE CURSOR,
/// which pg_query's own walk doesn't enter. Calls in views, functions and rules are
/// left out: creating them runs nothing.
fn function_calls(parsed: &pg_query::ParseResult) -> Vec<&FuncCall> {
    let mut statements: Vec<&NodeEnum> = parsed
        .protobuf
        .stmts
        .iter()
        .filter_map(|raw| raw.stmt.as_ref()?.node.as_ref())
        .collect();
    let mut calls = Vec::new();
    while let Some(statement) = statements.pop() {
        match statement {
            NodeEnum::ViewStmt(_) | NodeEnum::CreateFunctionStmt(_) | NodeEnum::RuleStmt(_) => continue,
            NodeEnum::CreateSchemaStmt(schema) => {
                statements.extend(schema.schema_elts.iter().filter_map(|e| e.node.as_ref()));
                continue;
            }
            _ => {}
        }
        for (node, ..) in statement.nodes() {
            let inner = match node {
                NodeRef::FuncCall(call) => {
                    calls.push(call);
                    None
                }
                NodeRef::PrepareStmt(prepare) => prepare.query.as_deref(),
                NodeRef::DeclareCursorStmt(cursor) => cursor.query.as_deref(),
                _ => None,
            };
            statements.extend(inner.and_then(|n| n.node.as_ref()));
        }
    }
    calls
}

/// The function's name without its schema.
fn function_name(call: &FuncCall) -> Option<&str> {
    match call.funcname.last()?.node.as_ref()? {
        NodeEnum::String(s) => Some(&s.sval),
        _ => None,
    }
}

/// The most certain session-scoped `is_local` among the query's set_config calls,
/// or None if every call passes literal true.
fn set_config_is_local(calls: &[&FuncCall]) -> Option<IsLocal> {
    calls
        .iter()
        .filter(|call| function_name(call) == Some("set_config"))
        .filter_map(|call| {
            let mut arg = is_local_arg(call);
            while let Some(NodeEnum::TypeCast(cast)) = arg {
                arg = cast.arg.as_ref().and_then(|a| a.node.as_ref());
            }
            match arg {
                Some(NodeEnum::AConst(c)) => match &c.val {
                    Some(a_const::Val::Boolval(b)) => (!b.boolval).then_some(IsLocal::False),
                    Some(a_const::Val::Sval(s)) => match parse_bool(&s.sval) {
                        Some(true) => None,
                        Some(false) => Some(IsLocal::False),
                        None => Some(IsLocal::Expression),
                    },
                    _ => Some(IsLocal::Expression),
                },
                Some(NodeEnum::ParamRef(_)) => Some(IsLocal::Parameter),
                _ => Some(IsLocal::Expression),
            }
        })
        .max()
}

/// set_config's third argument, `is_local`: by name, or third by position.
/// Postgres puts positional arguments before named ones.
fn is_local_arg(call: &FuncCall) -> Option<&NodeEnum> {
    let named = call.args.iter().find_map(|a| match a.node.as_ref()? {
        NodeEnum::NamedArgExpr(n) if n.name == "is_local" => n.arg.as_ref()?.node.as_ref(),
        _ => None,
    });
    named.or_else(|| {
        call.args
            .iter()
            .filter_map(|a| a.node.as_ref())
            .take_while(|n| !matches!(n, NodeEnum::NamedArgExpr(_)))
            .nth(2)
    })
}

/// A string as Postgres reads it as a boolean (`parse_bool` in bool.c): any prefix
/// of true/false/yes/no, on, off (at least "of"), 1, 0, case-insensitive, trimmed.
fn parse_bool(s: &str) -> Option<bool> {
    let s = s.trim().to_ascii_lowercase();
    let prefix_of = |word: &str, min: usize| s.len() >= min && word.starts_with(s.as_str());
    if prefix_of("true", 1) || prefix_of("yes", 1) || s == "on" || s == "1" {
        Some(true)
    } else if prefix_of("false", 1) || prefix_of("no", 1) || prefix_of("off", 2) || s == "0" {
        Some(false)
    } else {
        None
    }
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
    fn flags_deallocate_all_and_load() {
        assert_eq!(rules("DEALLOCATE ALL"), vec!["deallocate-all"]);
        assert_eq!(rules("deallocate prepare all"), vec!["deallocate-all"]);
        assert_eq!(rules("LOAD 'auto_explain'"), vec!["load"]);
        // Removing one SQL-level statement by name only touches that statement.
        assert!(rules("DEALLOCATE get_user").is_empty());
    }

    #[test]
    fn ignores_session_independent_statements() {
        // COMMIT PREPARED works from any connection.
        assert!(rules("PREPARE TRANSACTION 'order-42'").is_empty());
        assert!(rules("COMMIT PREPARED 'order-42'").is_empty());
        // Function-level SET applies only during the call; ALTER ROLE ... SET at next login.
        assert!(rules("ALTER FUNCTION app.audit() SET search_path = app, pg_temp").is_empty());
        assert!(rules("CREATE FUNCTION f() RETURNS int LANGUAGE sql SET work_mem = '64MB' AS 'SELECT 1'").is_empty());
        assert!(rules("ALTER ROLE app SET statement_timeout = '30s'").is_empty());
        assert!(rules("ALTER DATABASE app SET timezone = 'UTC'").is_empty());
    }

    #[test]
    fn session_authorization_is_a_session_set() {
        let findings = scan(&[query("SET SESSION AUTHORIZATION app_user", 1)]).findings;
        assert_eq!(findings.iter().map(|f| f.rule).collect::<Vec<_>>(), vec!["session-set"]);
        assert_eq!(
            findings[0].scenarios,
            vec![
                "set-role-isolated-between-clients",
                "set-role-reset-after-pinned-disconnect"
            ]
        );
        assert_eq!(rules("RESET SESSION AUTHORIZATION"), vec!["session-set"]);
    }

    #[test]
    fn looks_inside_prepare_and_declare() {
        let mut got = rules("PREPARE x AS SELECT pg_try_advisory_lock($1)");
        got.sort();
        assert_eq!(got, vec!["session-advisory-lock", "sql-prepare"]);
        assert_eq!(
            rules("PREPARE x AS SELECT set_config('a', $1, false)"),
            vec!["session-set-config", "sql-prepare"]
        );
        assert_eq!(
            rules("DECLARE c CURSOR FOR SELECT pg_advisory_lock(id) FROM jobs"),
            vec!["session-advisory-lock"]
        );
        assert_eq!(
            rules("EXPLAIN ANALYZE SELECT pg_advisory_lock(1)"),
            vec!["session-advisory-lock"]
        );
        // One finding per function, however often it is called.
        assert_eq!(
            rules("SELECT pg_advisory_lock(1), pg_advisory_lock(2)"),
            vec!["session-advisory-lock"]
        );
    }

    #[test]
    fn set_config_is_local_as_postgres_reads_it() {
        let rule = |sql: &str| rules(sql).contains(&"session-set-config");
        // Casts and string literals Postgres coerces to boolean.
        assert!(rule("SELECT set_config('a', 'b', false::boolean)"));
        assert!(!rule("SELECT set_config('a', 'b', true::bool)"));
        for s in ["false", "f", "FALSE", "off", "of", "no", "n", "0", " false "] {
            assert!(rule(&format!("SELECT set_config('a', 'b', '{s}')")), "{s}");
            assert!(rule(&format!("SELECT set_config('a', 'b', '{s}'::bool)")), "{s}");
        }
        for s in ["true", "t", "tr", "on", "yes", "y", "1", "TRUE"] {
            assert!(!rule(&format!("SELECT set_config('a', 'b', '{s}')")), "{s}");
        }
        // Not a boolean Postgres accepts: the call fails, but we can't say how.
        assert!(rule("SELECT set_config('a', 'b', 'o')"));
        // Named arguments, in any order.
        assert!(!rule("SELECT set_config('a', 'b', is_local => true)"));
        assert!(!rule(
            "SELECT set_config(is_local => true, new_value => 'b', setting_name => 'a')"
        ));
        assert!(rule(
            "SELECT set_config(is_local => false, setting_name => 'a', new_value => 'b')"
        ));
        assert!(rule("SELECT set_config('a', new_value => 'b', is_local => $1)"));
    }

    #[test]
    fn ignores_calls_in_stored_definitions() {
        // Creating these runs nothing; the call happens later, wherever they are used.
        assert!(rules("CREATE VIEW v AS SELECT pg_advisory_lock(5)").is_empty());
        assert!(rules("CREATE OR REPLACE VIEW v AS SELECT set_config('a', 'b', false)").is_empty());
        assert!(
            rules("CREATE FUNCTION f() RETURNS bool LANGUAGE sql BEGIN ATOMIC SELECT pg_advisory_lock(1); END")
                .is_empty()
        );
        assert!(rules("CREATE RULE r AS ON INSERT TO t DO ALSO SELECT pg_advisory_lock(1)").is_empty());
        assert!(rules("CREATE SCHEMA s CREATE VIEW v AS SELECT pg_advisory_lock(1)").is_empty());
        // These do run the query now.
        assert_eq!(
            rules("CREATE MATERIALIZED VIEW m AS SELECT pg_advisory_lock(1)"),
            vec!["session-advisory-lock"]
        );
        assert_eq!(
            rules("SELECT pg_advisory_lock(1); CREATE VIEW v AS SELECT 1"),
            vec!["session-advisory-lock"]
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
