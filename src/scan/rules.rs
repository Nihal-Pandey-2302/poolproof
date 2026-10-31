use std::collections::{BTreeMap, BTreeSet};

use pg_query::protobuf::{FuncCall, OnCommitAction, VariableSetKind, a_const};
use pg_query::{NodeEnum, NodeRef};

use super::{Finding, Query, RULES, Report, Severity, scenarios_for};

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
    let mut uninspected_bodies = 0;
    for (query, (calls, line)) in &merged {
        let Ok(parsed) = pg_query::parse(query, pg_query::ParserOptions::DEFAULT) else {
            unparsed += 1;
            continue;
        };
        let mut hits = Vec::new();
        let starts_transaction = parsed.protobuf.stmts.first().is_some_and(|raw| {
            matches!(
                raw.stmt.as_ref().and_then(|s| s.node.as_ref()),
                Some(NodeEnum::TransactionStmt(_))
            )
        });
        if starts_transaction && parsed.protobuf.stmts.len() > 1 {
            hits.push(Hit::new(
                "multi-statement-transaction",
                Severity::Warn,
                "This one query string starts with BEGIN, COMMIT or ROLLBACK and has more statements \
                 after it. Some poolers only handle the first statement and defer or drop the rest \
                 (PgDog: https://github.com/pgdogdev/pgdog/issues/1849). Send them separately."
                    .to_string(),
            ));
        }
        let statements: Vec<&NodeEnum> = parsed
            .protobuf
            .stmts
            .iter()
            .filter_map(|raw| raw.stmt.as_ref()?.node.as_ref())
            .collect();
        hits.extend(check(&statements, &[]));
        let (embedded, uninspected) = embedded_hits(query, &parsed);
        hits.extend(embedded);
        uninspected_bodies += uninspected;

        findings.extend(hits.into_iter().map(|hit| Finding {
            rule: hit.rule,
            severity: hit.severity,
            detail: hit.detail,
            query: query.to_string(),
            calls: *calls,
            line: *line,
            scenarios: hit.scenarios,
            on_pooler: None,
        }));
    }

    findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(b.calls.cmp(&a.calls)));
    Report {
        findings,
        queries: merged.len(),
        unparsed,
        uninspected: uninspected_bodies,
    }
}

/// A rule match, before it becomes a [`Finding`] for a query.
struct Hit {
    rule: &'static str,
    severity: Severity,
    detail: String,
    scenarios: Vec<&'static str>,
}

impl Hit {
    fn new(rule: &'static str, severity: Severity, detail: String) -> Hit {
        Hit {
            rule,
            severity,
            detail,
            scenarios: scenarios_for(rule),
        }
    }
}

/// The rules that look at what statements run: function calls and statement kinds.
/// `scoped` lists parameters that are reverted afterwards anyway (a function's own
/// SET clause), so setting them is not a finding.
fn check(statements: &[&NodeEnum], scoped: &[String]) -> Vec<Hit> {
    let mut hits = Vec::new();
    let calls = function_calls(statements);
    let names: BTreeSet<&str> = calls.iter().filter_map(|call| function_name(call)).collect();
    for short in names.into_iter().filter(|n| SESSION_ADVISORY_LOCKS.contains(n)) {
        hits.push(Hit::new(
            "session-advisory-lock",
            Severity::Breaks,
            format!(
                "{short}() holds the lock for the whole session. Under transaction pooling \
                 the server connection goes back to the pool still holding it. Use the \
                 pg_advisory_xact_* variant."
            ),
        ));
    }

    if let Some(is_local) = set_config_is_local(&calls, scoped) {
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
        hits.push(Hit::new("session-set-config", Severity::Warn, detail.to_string()));
    }

    for node in statements {
        match node {
            NodeEnum::VariableSetStmt(set) => {
                let kind = VariableSetKind::try_from(set.kind).unwrap_or(VariableSetKind::Undefined);
                let transaction_scoped =
                    set.is_local || (kind == VariableSetKind::VarSetMulti && set.name == "TRANSACTION");
                if !transaction_scoped && !scoped.iter().any(|p| p.eq_ignore_ascii_case(&set.name)) {
                    let target = if kind == VariableSetKind::VarResetAll {
                        "ALL"
                    } else {
                        set.name.as_str()
                    };
                    let mut hit = Hit::new(
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
                    if let Some(scenarios) = refined {
                        hit.scenarios = scenarios;
                    }
                    hits.push(hit);
                }
            }
            NodeEnum::ListenStmt(listen) => hits.push(Hit::new(
                "listen",
                Severity::Breaks,
                format!(
                    "LISTEN {} needs a dedicated session. Notifications go to whichever client \
                 holds the server connection, or nobody.",
                    listen.conditionname
                ),
            )),
            NodeEnum::CreateStmt(create) => {
                let temp = create.relation.as_ref().is_some_and(|r| r.relpersistence == "t");
                let dropped_at_commit = create.oncommit == OnCommitAction::OncommitDrop as i32;
                if temp && !dropped_at_commit {
                    hits.push(Hit::new(
                        "temp-table",
                        Severity::Breaks,
                        "Temp tables live for the session. Other clients can see them and later \
                     transactions may land on a server connection without them. Use ON \
                     COMMIT DROP inside a transaction."
                            .to_string(),
                    ));
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
                    hits.push(Hit::new(
                        "temp-table",
                        Severity::Breaks,
                        "CREATE TEMP TABLE ... AS / SELECT INTO TEMP creates a session-scoped table.".to_string(),
                    ));
                }
            }
            NodeEnum::SelectStmt(select) => {
                let temp = select
                    .into_clause
                    .as_ref()
                    .and_then(|i| i.rel.as_ref())
                    .is_some_and(|r| r.relpersistence == "t");
                if temp {
                    hits.push(Hit::new(
                        "temp-table",
                        Severity::Breaks,
                        "SELECT ... INTO TEMP creates a session-scoped table.".to_string(),
                    ));
                }
            }
            NodeEnum::DeclareCursorStmt(cursor) if cursor.options & CURSOR_OPT_HOLD != 0 => hits.push(Hit::new(
                "with-hold-cursor",
                Severity::Breaks,
                format!(
                    "Cursor {} is WITH HOLD, so it outlives the transaction. Later FETCHes can \
                 land on a different server connection.",
                    cursor.portalname
                ),
            )),
            NodeEnum::DiscardStmt(_) => hits.push(Hit::new(
                "discard",
                Severity::Warn,
                "DISCARD resets the server connection it runs on. Under transaction pooling \
             that connection is shared, and the pooler's view of it (tracked settings, \
             prepared statements) can stop matching. It is usually only needed in session \
             mode, where the pooler runs it for you."
                    .to_string(),
            )),
            NodeEnum::PrepareStmt(prepare) => hits.push(Hit::new(
                "sql-prepare",
                Severity::Warn,
                format!(
                    "SQL-level PREPARE {} creates a session object. Poolers that handle \
                 protocol-level prepared statements often don't track this form.",
                    prepare.name
                ),
            )),
            NodeEnum::DeallocateStmt(deallocate) if deallocate.isall => hits.push(Hit::new(
                "deallocate-all",
                Severity::Warn,
                "DEALLOCATE ALL drops every prepared statement on the server connection, \
             including other clients' when it is shared. Poolers that track protocol-level \
             prepared statements can lose track of them. Drivers send it to recover from \
             \"prepared statement does not exist\"; with a pooler that tracks statements \
             it is usually not needed."
                    .to_string(),
            )),
            NodeEnum::LoadStmt(load) => hits.push(Hit::new(
                "load",
                Severity::Warn,
                format!(
                    "LOAD '{}' loads a library into the server process for the rest of the \
                 session. Under transaction pooling, later transactions may run on a \
                 connection without it, and other clients get it where they didn't ask.",
                    load.filename
                ),
            )),
            _ => {}
        }
    }
    hits
}

/// What a `set_config(name, value, is_local)` call passes as `is_local`, when it
/// could be false. Ordered from least to most certain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum IsLocal {
    Expression,
    Parameter,
    False,
}

/// Statements that run elsewhere: DO blocks run now, function bodies whenever the
/// function is called. Function hits become warnings that name the function.
/// Also returns how many bodies could only be word-scanned.
fn embedded_hits(query: &str, parsed: &pg_query::ParseResult) -> (Vec<Hit>, usize) {
    let mut hits: Vec<Hit> = Vec::new();
    let mut uninspected = 0;
    for raw in &parsed.protobuf.stmts {
        let Some(node) = raw.stmt.as_ref().and_then(|s| s.node.as_ref()) else {
            continue;
        };
        // The statement's own text, for the PL/pgSQL parser. stmt_len 0 means to the end.
        let start = raw.stmt_location as usize;
        let end = if raw.stmt_len == 0 {
            query.len()
        } else {
            start + raw.stmt_len as usize
        };
        let text = query.get(start..end).unwrap_or(query);

        let (owner, scoped, body) = match node {
            NodeEnum::DoStmt(stmt) if language(&stmt.args).unwrap_or("plpgsql") == "plpgsql" => {
                let source = function_source(&stmt.args).unwrap_or_default();
                (
                    Owner::DoBlock,
                    Vec::new(),
                    plpgsql_statements(text, source, Wrapper::Do),
                )
            }
            NodeEnum::CreateFunctionStmt(stmt) => {
                let name = stmt
                    .funcname
                    .last()
                    .and_then(|n| match n.node.as_ref()? {
                        NodeEnum::String(s) => Some(s.sval.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let owner = Owner::Function {
                    name,
                    procedure: stmt.is_procedure,
                };
                let scoped = set_options(&stmt.options);
                let body = match (&stmt.sql_body, language(&stmt.options)) {
                    // BEGIN ATOMIC ... END or RETURN expr: already parsed.
                    (Some(sql_body), _) => {
                        let mut roots = Vec::new();
                        flatten_lists(sql_body, &mut roots);
                        hits.extend(owner.wrap(check(&roots, &scoped)));
                        continue;
                    }
                    (None, Some("sql")) => {
                        let source = function_source(&stmt.options).unwrap_or_default();
                        match pg_query::parse(source, pg_query::ParserOptions::DEFAULT) {
                            Ok(parsed) => Body::Parsed(vec![parsed]),
                            Err(_) => Body::Mentions(mentions(source)),
                        }
                    }
                    (None, Some("plpgsql")) => {
                        let source = function_source(&stmt.options).unwrap_or_default();
                        let params = stmt
                            .parameters
                            .iter()
                            .filter_map(|p| match p.node.as_ref()? {
                                NodeEnum::FunctionParameter(p) if !p.name.is_empty() => Some(p.name.clone()),
                                _ => None,
                            })
                            .collect();
                        let wrapper = if stmt.is_procedure {
                            Wrapper::Procedure(params)
                        } else {
                            Wrapper::Function(params)
                        };
                        plpgsql_statements(text, source, wrapper)
                    }
                    _ => continue,
                };
                (owner, scoped, body)
            }
            _ => continue,
        };
        match body {
            Body::Parsed(body) => {
                let roots: Vec<&NodeEnum> = body
                    .iter()
                    .flat_map(|parsed| &parsed.protobuf.stmts)
                    .filter_map(|raw| raw.stmt.as_ref()?.node.as_ref())
                    .collect();
                hits.extend(owner.wrap(check(&roots, &scoped)));
            }
            Body::Mentions(rules) => {
                uninspected += 1;
                hits.extend(rules.into_iter().map(|(rule, word)| {
                    let summary = RULES.iter().find(|(r, _)| *r == rule).map_or("", |(_, s)| s);
                    let detail = format!(
                        "{} mentions `{word}` ({summary}), but its body could not be fully parsed, \
                         so check what it runs.",
                        owner.describe()
                    );
                    Hit::new(rule, Severity::Warn, detail)
                }));
            }
        }
    }
    // Several statements in one body can match the same rule the same way.
    let mut seen = BTreeSet::new();
    hits.retain(|hit| seen.insert((hit.rule, hit.detail.clone())));
    (hits, uninspected)
}

/// Where embedded statements come from.
enum Owner {
    DoBlock,
    Function { name: String, procedure: bool },
}

impl Owner {
    fn describe(&self) -> String {
        match self {
            Owner::DoBlock => "A DO block".to_string(),
            Owner::Function { name, procedure: true } => format!("Procedure {name}()"),
            Owner::Function { name, .. } => format!("Function {name}()"),
        }
    }

    /// A DO block runs now, so its hits keep their severity. A function body runs on
    /// each call, which scan can't see, so its hits become warnings that name it.
    fn wrap(&self, hits: Vec<Hit>) -> Vec<Hit> {
        hits.into_iter()
            .map(|hit| match self {
                Owner::DoBlock => Hit {
                    detail: format!("In a DO block: {}", hit.detail),
                    ..hit
                },
                Owner::Function { name, procedure } => Hit {
                    severity: Severity::Warn,
                    detail: format!(
                        "In {} {name}(), each time it is called: {}",
                        if *procedure { "procedure" } else { "function" },
                        hit.detail
                    ),
                    ..hit
                },
            })
            .collect()
    }
}

/// A function or DO body: its statements, or, when it can't be parsed (for
/// PL/pgSQL even after the retry), the rules whose functions or keywords it mentions.
enum Body {
    Parsed(Vec<pg_query::ParseResult>),
    Mentions(Vec<(&'static str, String)>),
}

/// How to rebuild a PL/pgSQL body as a standalone block for a second parse attempt.
enum Wrapper {
    Do,
    Function(Vec<String>),
    Procedure(Vec<String>),
}

/// Parameters a function sets in its own SET clause, which Postgres restores when it
/// returns.
fn set_options(options: &[pg_query::protobuf::Node]) -> Vec<String> {
    options
        .iter()
        .filter_map(|o| match o.node.as_ref()? {
            NodeEnum::DefElem(d) if d.defname == "set" => match d.arg.as_ref()?.node.as_ref()? {
                NodeEnum::VariableSetStmt(set) => Some(set.name.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// `LANGUAGE x` from DO or CREATE FUNCTION options.
fn language(options: &[pg_query::protobuf::Node]) -> Option<&str> {
    def_elem_string(options, "language")
}

/// The body of `CREATE FUNCTION ... AS 'source'`.
fn function_source(options: &[pg_query::protobuf::Node]) -> Option<&str> {
    def_elem_string(options, "as")
}

/// The first string value of the DefElem option named `name`: a String, or the first
/// of a List of them.
fn def_elem_string<'a>(options: &'a [pg_query::protobuf::Node], name: &str) -> Option<&'a str> {
    let arg = options.iter().find_map(|o| match o.node.as_ref()? {
        NodeEnum::DefElem(d) if d.defname == name => d.arg.as_ref()?.node.as_ref(),
        _ => None,
    })?;
    let first = match arg {
        NodeEnum::List(list) => list.items.first()?.node.as_ref()?,
        other => other,
    };
    match first {
        NodeEnum::String(s) => Some(&s.sval),
        _ => None,
    }
}

/// The statements in a BEGIN ATOMIC body, which arrives as nested Lists.
fn flatten_lists<'a>(node: &'a pg_query::protobuf::Node, out: &mut Vec<&'a NodeEnum>) {
    match node.node.as_ref() {
        Some(NodeEnum::List(list)) => list.items.iter().for_each(|item| flatten_lists(item, out)),
        Some(other) => out.push(other),
        None => {}
    }
}

/// Variables Postgres declares for trigger and event trigger functions. Without
/// them the PL/pgSQL parser rejects a body that uses NEW or TG_OP.
const TRIGGER_VARIABLES: &str = "new record; old record; tg_name text; tg_when text; tg_level text; \
     tg_op text; tg_relid oid; tg_relname text; tg_table_name text; tg_table_schema text; \
     tg_nargs int; tg_argv text[]; tg_event text; tg_tag text;";

/// The SQL in a PL/pgSQL DO block or function (`text` is the whole statement, `source`
/// its body), each statement and expression parsed on its own. Dynamic EXECUTE
/// strings stay opaque.
///
/// The PL/pgSQL parser has no catalog, so it rejects trigger functions (NEW, OLD),
/// `%TYPE` and `%ROWTYPE`. Then the body is parsed again as a nested block with those
/// variables and the parameters declared, and type references as text or record.
/// If that fails too, the body is only scanned for words.
fn plpgsql_statements(text: &str, source: &str, wrapper: Wrapper) -> Body {
    let mut attempts = vec![text.to_string()];
    let declare = |params: &[String]| {
        let params: String = params.iter().map(|p| format!("\"{p}\" record; ")).collect();
        format!("DECLARE {TRIGGER_VARIABLES} {params}")
    };
    let block = |params: &[String]| {
        let body = without_type_references(source);
        // The body is a block that needs a `;` after its END, unless it has one. The
        // newline ends a trailing line comment, which would otherwise swallow ours.
        let terminated = ends_with_semicolon(&body);
        format!(
            "$pp_inspect$ {} BEGIN {body}\n{} END $pp_inspect$",
            declare(params),
            if terminated { "" } else { ";" }
        )
    };
    match &wrapper {
        Wrapper::Do => attempts.push(format!("DO {}", block(&[]))),
        Wrapper::Procedure(params) => attempts.push(format!(
            "CREATE PROCEDURE pp_inspect() LANGUAGE plpgsql AS {}",
            block(params)
        )),
        // The return type decides which RETURN forms parse; try the usual ones.
        Wrapper::Function(params) => {
            for returns in ["record", "void", "SETOF record"] {
                attempts.push(format!(
                    "CREATE FUNCTION pp_inspect() RETURNS {returns} LANGUAGE plpgsql AS {}",
                    block(params)
                ));
            }
        }
    }
    let Some(tree) = attempts.iter().find_map(|sql| pg_query::parse_plpgsql(sql).ok()) else {
        return Body::Mentions(mentions(source));
    };
    let mut exprs = Vec::new();
    collect_plpgsql_exprs(&tree, &mut exprs);
    Body::Parsed(
        exprs
            .into_iter()
            .filter_map(|(sql, mode)| {
                // RawParseMode in parser.h: 0 a statement, 2 an expression, 3-5 an assignment.
                let sql = match mode {
                    0 => sql.to_string(),
                    2 => format!("SELECT {sql}"),
                    3..=5 => {
                        let (_, value) = sql.split_once(":=").or_else(|| sql.split_once('='))?;
                        format!("SELECT {value}")
                    }
                    _ => return None,
                };
                pg_query::parse(&sql, pg_query::ParserOptions::DEFAULT).ok()
            })
            .collect(),
    )
}

/// True if the last token, ignoring comments, is `;`.
fn ends_with_semicolon(body: &str) -> bool {
    use pg_query::protobuf::Token;
    let Ok(tokens) = pg_query::scan_tokens(body) else {
        return body.trim_end().ends_with(';');
    };
    tokens
        .iter()
        .rev()
        .find(|t| t.token != Token::SqlComment as i32 && t.token != Token::CComment as i32)
        .is_some_and(|t| t.token == Token::Ascii59 as i32)
}

/// `t.col%TYPE` becomes `text` and `t%ROWTYPE` becomes `record`, which the PL/pgSQL
/// parser accepts without a catalog.
fn without_type_references(body: &str) -> String {
    let lower = body.to_ascii_lowercase();
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let (mut copied, mut from) = (0, 0);
    while let Some(found) = lower[from..].find('%') {
        let percent = from + found;
        from = percent + 1;
        let rest = &lower[percent + 1..];
        let (len, replacement) = if rest.starts_with("rowtype") {
            (7, "record")
        } else if rest.starts_with("type") {
            (4, "text")
        } else {
            continue;
        };
        let mut start = percent;
        while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || b"_.\"".contains(&bytes[start - 1])) {
            start -= 1;
        }
        if start == percent {
            continue;
        }
        out.push_str(&body[copied..start]);
        out.push_str(replacement);
        copied = percent + 1 + len;
        from = copied;
    }
    out.push_str(&body[copied..]);
    out
}

/// The rules a body that couldn't be parsed probably triggers, from its words.
fn mentions(body: &str) -> Vec<(&'static str, String)> {
    let lower = body.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .collect();
    let mut found = Vec::new();
    for (i, word) in words.iter().enumerate() {
        let rule = match *word {
            w if SESSION_ADVISORY_LOCKS.contains(&w) => Some("session-advisory-lock"),
            "set_config" => Some("session-set-config"),
            "listen" => Some("listen"),
            "temp" | "temporary" if words.get(i + 1) == Some(&"table") => Some("temp-table"),
            _ => None,
        };
        if let Some(rule) = rule
            && !found.iter().any(|(r, _)| *r == rule)
        {
            found.push((rule, word.to_string()));
        }
    }
    found
}

/// Every `PLpgSQL_expr` in parse_plpgsql's JSON: its SQL and parse mode.
fn collect_plpgsql_exprs<'a>(value: &'a serde_json::Value, out: &mut Vec<(&'a str, i64)>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(expr) = map.get("PLpgSQL_expr")
                && let Some(sql) = expr.get("query").and_then(|q| q.as_str())
            {
                out.push((sql, expr.get("parseMode").and_then(|m| m.as_i64()).unwrap_or(0)));
            }
            map.values().for_each(|v| collect_plpgsql_exprs(v, out));
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_plpgsql_exprs(v, out)),
        _ => {}
    }
}

/// Every function call the statements run, including inside PREPARE and DECLARE
/// CURSOR, which pg_query's own walk doesn't enter. Calls in views, functions and
/// rules are left out here: creating them runs nothing (see [`embedded_hits`]).
fn function_calls<'a>(statements: &[&'a NodeEnum]) -> Vec<&'a FuncCall> {
    let mut statements = statements.to_vec();
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
/// or None if every call passes literal true or sets a parameter in `scoped`.
fn set_config_is_local(calls: &[&FuncCall], scoped: &[String]) -> Option<IsLocal> {
    calls
        .iter()
        .filter(|call| function_name(call) == Some("set_config"))
        .filter(|call| {
            let setting = match without_casts(set_config_arg(call, "setting_name", 0)) {
                Some(NodeEnum::AConst(c)) => match &c.val {
                    Some(a_const::Val::Sval(s)) => Some(s.sval.as_str()),
                    _ => None,
                },
                _ => None,
            };
            !setting.is_some_and(|name| scoped.iter().any(|p| p.eq_ignore_ascii_case(name)))
        })
        .filter_map(|call| match without_casts(set_config_arg(call, "is_local", 2)) {
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
        })
        .max()
}

/// A set_config argument by name, or by position (0-based). Postgres puts
/// positional arguments before named ones.
fn set_config_arg<'a>(call: &'a FuncCall, name: &str, position: usize) -> Option<&'a NodeEnum> {
    let named = call.args.iter().find_map(|a| match a.node.as_ref()? {
        NodeEnum::NamedArgExpr(n) if n.name == name => n.arg.as_ref()?.node.as_ref(),
        _ => None,
    });
    named.or_else(|| {
        call.args
            .iter()
            .filter_map(|a| a.node.as_ref())
            .take_while(|n| !matches!(n, NodeEnum::NamedArgExpr(_)))
            .nth(position)
    })
}

/// The expression under any `::type` casts.
fn without_casts(mut node: Option<&NodeEnum>) -> Option<&NodeEnum> {
    while let Some(NodeEnum::TypeCast(cast)) = node {
        node = cast.arg.as_ref().and_then(|a| a.node.as_ref());
    }
    node
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
        // (Functions are reported as warnings, see looks_inside_do_blocks_and_functions.)
        assert!(rules("CREATE VIEW v AS SELECT pg_advisory_lock(5)").is_empty());
        assert!(rules("CREATE OR REPLACE VIEW v AS SELECT set_config('a', 'b', false)").is_empty());
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
    fn looks_inside_do_blocks_and_functions() {
        let findings = |sql: &str| {
            let mut f: Vec<_> = scan(&[query(sql, 1)])
                .findings
                .into_iter()
                .map(|f| (f.rule, f.severity, f.detail))
                .collect();
            f.sort();
            f
        };
        // A DO block runs now: same rules, same severity.
        let found = findings("DO $$ BEGIN PERFORM pg_advisory_lock(3); END $$");
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].0, found[0].1), ("session-advisory-lock", Severity::Breaks));
        assert!(found[0].2.starts_with("In a DO block: "));
        let body = "DO $$ DECLARE got bool; BEGIN \
                    got := pg_try_advisory_lock(4); \
                    IF set_config('app.x', 'y', false) IS NULL THEN RAISE NOTICE 'n'; END IF; \
                    SET search_path TO app; LISTEN jobs; CREATE TEMP TABLE t (a int); \
                    PERFORM pg_try_advisory_lock(5); \
                    END $$";
        let rules_found: Vec<_> = findings(body).into_iter().map(|f| f.0).collect();
        assert_eq!(
            rules_found,
            vec![
                "listen",
                "session-advisory-lock",
                "session-set",
                "session-set-config",
                "temp-table"
            ]
        );
        // Transaction-scoped work, other languages and dynamic SQL give nothing.
        assert!(rules("DO $$ BEGIN PERFORM pg_advisory_xact_lock(3); SET LOCAL work_mem = '1MB'; END $$").is_empty());
        assert!(rules("DO LANGUAGE plpython3u $$ plpy.execute('LISTEN a') $$").is_empty());
        assert!(rules("DO $$ BEGIN EXECUTE 'LISTEN a'; END $$").is_empty());

        // A function body runs each time the function is called: a warning naming it.
        for sql in [
            "CREATE FUNCTION app.take(job bigint) RETURNS void LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_lock(job); END $$",
            "CREATE FUNCTION app.take(job bigint) RETURNS bool LANGUAGE sql AS $$ SELECT pg_advisory_lock(job) IS NOT NULL $$",
            "CREATE FUNCTION app.take(job bigint) RETURNS void LANGUAGE sql BEGIN ATOMIC SELECT pg_advisory_lock(job); END",
        ] {
            let found = findings(sql);
            assert_eq!(found.len(), 1, "{sql}");
            assert_eq!(
                (found[0].0, found[0].1),
                ("session-advisory-lock", Severity::Warn),
                "{sql}"
            );
            assert!(
                found[0].2.starts_with("In function take(), each time it is called: "),
                "{sql}"
            );
        }
        assert!(rules("CREATE FUNCTION f() RETURNS int LANGUAGE plpgsql AS $$ BEGIN RETURN 1; END $$").is_empty());
        assert!(rules("CREATE FUNCTION f() RETURNS int LANGUAGE c AS 'mylib', 'f'").is_empty());

        // A body that can't be parsed even after the retry is word-scanned and counted
        // apart from SQL parse failures.
        let report = scan(&[query("DO $$ BEGIN undeclared := pg_advisory_lock(1); END $$", 1)]);
        assert_eq!((report.unparsed, report.uninspected), (0, 1));
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule, "session-advisory-lock");
        assert_eq!(report.findings[0].severity, Severity::Warn);
        assert!(
            report.findings[0].detail.contains("could not be fully parsed"),
            "{}",
            report.findings[0].detail
        );
        // With other statements in the same string.
        assert_eq!(
            rules("SET work_mem = '1MB'; DO $$ BEGIN LISTEN jobs; END $$"),
            vec!["listen", "session-set"]
        );
    }

    #[test]
    fn parses_trigger_functions_and_type_references() {
        let report = |sql: &str| {
            let report = scan(&[query(sql, 1)]);
            assert_eq!((report.unparsed, report.uninspected), (0, 0), "{sql}");
            let mut rules: Vec<_> = report.findings.iter().map(|f| f.rule).collect();
            rules.sort();
            rules
        };
        // The PL/pgSQL parser alone rejects all three; the retry reads them.
        let empty: Vec<&str> = vec![];
        assert_eq!(
            report(
                "CREATE FUNCTION trg() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.updated_at := now(); RETURN NEW; END $$"
            ),
            empty
        );
        assert_eq!(
            report(
                "CREATE FUNCTION typ(x t.id%TYPE) RETURNS int LANGUAGE plpgsql AS $$ DECLARE v t%ROWTYPE; \
                 BEGIN SELECT * INTO v FROM t WHERE id = x; RETURN v.id; END $$"
            ),
            empty
        );
        assert_eq!(
            report(
                "CREATE FUNCTION evt() RETURNS event_trigger LANGUAGE plpgsql AS $$ BEGIN RAISE NOTICE '%', tg_tag; END $$"
            ),
            empty
        );
        // Findings inside them still count.
        assert_eq!(
            report(
                "CREATE FUNCTION audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
                 PERFORM set_config('audit.user', NEW.owner, false); \
                 IF TG_OP = 'DELETE' THEN RETURN OLD; END IF; RETURN NEW; END; $$"
            ),
            vec!["session-set-config"]
        );
        assert_eq!(
            report(
                "CREATE FUNCTION f(job jobs.id%TYPE, OUT ok boolean) LANGUAGE plpgsql AS $$ \
                 DECLARE r jobs%ROWTYPE; BEGIN ok := pg_try_advisory_lock(job); RETURN; END $$"
            ),
            vec!["session-advisory-lock"]
        );
        assert_eq!(
            report("DO $$ DECLARE r pg_class%ROWTYPE; BEGIN LISTEN jobs; END $$"),
            vec!["listen"]
        );
    }

    #[test]
    fn function_set_clause_scopes_its_parameters() {
        // Postgres restores search_path when the function returns.
        assert!(
            rules(
                "CREATE FUNCTION f() RETURNS void LANGUAGE plpgsql SET search_path = app AS $$ \
             BEGIN SET search_path = app, public; PERFORM set_config('search_path', 'x', false); END $$"
            )
            .is_empty()
        );
        assert!(rules(
            "CREATE FUNCTION f() RETURNS void LANGUAGE sql SET work_mem FROM CURRENT AS $$ SELECT set_config('work_mem', '1GB', false) $$"
        )
        .is_empty());
        // Other parameters still count.
        assert_eq!(
            rules(
                "CREATE FUNCTION f() RETURNS void LANGUAGE plpgsql SET search_path = app AS $$ \
                 BEGIN SET work_mem = '1GB'; END $$"
            ),
            vec!["session-set"]
        );
    }

    #[test]
    fn procedures_are_called_procedures() {
        let findings = scan(&[query(
            "CREATE PROCEDURE app.lock_all() LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_lock(1); END $$",
            1,
        )])
        .findings;
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0]
                .detail
                .starts_with("In procedure lock_all(), each time it is called: "),
            "{}",
            findings[0].detail
        );
    }

    #[test]
    fn retry_keeps_a_trailing_line_comment_out_of_the_way() {
        // The retry appends `; END`; a body ending in a line comment must not swallow it.
        let report = scan(&[query(
            "CREATE FUNCTION trg() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
             PERFORM pg_advisory_lock(1); RETURN NEW; END; -- done\n$$",
            1,
        )]);
        assert_eq!(report.uninspected, 0);
        assert_eq!(report.findings.len(), 1);
        assert!(!report.findings[0].detail.contains("could not be fully parsed"));
    }

    #[test]
    fn unparsable_sql_function_body_is_counted() {
        // The outer statement parses; the string body doesn't.
        let report = scan(&[query(
            "CREATE FUNCTION f() RETURNS void LANGUAGE sql AS $$ SELEC pg_advisory_lock(1) $$",
            1,
        )]);
        assert_eq!((report.unparsed, report.uninspected), (0, 1));
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule, "session-advisory-lock");
        assert!(report.findings[0].detail.contains("could not be fully parsed"));
    }

    #[test]
    fn json_report_has_counts() {
        let report = scan(&[query("LISTEN a", 1), query("SELEC 1", 1)]);
        let json = serde_json::to_value(&report).unwrap();
        let keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, vec!["findings", "queries", "uninspected", "unparsed"]);
        assert_eq!(
            (json["queries"].as_u64(), json["unparsed"].as_u64()),
            (Some(2), Some(1))
        );
        assert_eq!(json["findings"][0]["rule"], "listen");
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
        assert_eq!(scenarios("DEALLOCATE ALL"), vec!["deallocate-all-isolated"]);
        assert_eq!(scenarios("LOAD 'auto_explain'"), vec!["load-isolated-between-clients"]);
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
