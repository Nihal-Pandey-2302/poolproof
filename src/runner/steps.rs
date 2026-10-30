use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::time::timeout;

use super::pg::{Conn, cancel, connect_with, db_error, row_value, simple_outcome};
use super::raw::{ExchangeOpts, RawConn, ServerError};
use super::{Outcome, Timeouts};
use crate::scenario::{Action, Scenario};
use crate::targets::Endpoint;

/// A connect that failed. A server that refused with a SQLSTATE (53300 too many
/// connections, 57P03 starting up) gives that code, so it compares like any other
/// outcome on the pooler side: a pooler exhausting Postgres's connections is the
/// pooler's fault. It is marked `connect`, so on the Postgres side it is a harness
/// failure. Transport failures (refused, timed out, unknown host) are CONNECT.
fn connect_failure(e: &anyhow::Error) -> Outcome {
    let refused = e.chain().find_map(|cause| {
        if let Some(e) = cause.downcast_ref::<tokio_postgres::Error>() {
            return e
                .as_db_error()
                .map(|db| (db.code().code().to_string(), db.message().to_string()));
        }
        cause
            .downcast_ref::<ServerError>()
            .map(|e| (e.code.clone(), e.message.clone()))
    });
    match refused {
        Some((code, message)) => Outcome::Error {
            code,
            message,
            connect: true,
        },
        None => Outcome::error("CONNECT", format!("{e:#}")),
    }
}

/// Run a scenario's steps. `pooled` is where normal clients connect; `direct` is
/// plain Postgres. For the baseline run both are the same endpoint.
pub async fn run_steps(
    scenario: &Scenario,
    pooled: &Endpoint,
    direct: &Endpoint,
    t: &Timeouts,
) -> Result<Vec<Outcome>> {
    // Loading already checks this; scenarios built in code don't go through loading.
    scenario.check_clients()?;
    // In the baseline every client is on Postgres.
    let on_postgres = |step_direct: bool| step_direct || pooled == direct;
    let mut conns: HashMap<&str, Conn> = HashMap::new();
    let mut outcomes = Vec::with_capacity(scenario.steps.len());

    // Clients that use `send` talk raw protocol on their own connection.
    let raw_clients: std::collections::HashSet<&str> = scenario
        .steps
        .iter()
        .filter(|s| s.send.is_some())
        .map(|s| s.client.as_str())
        .collect();
    let mut raw: HashMap<&str, RawConn> = HashMap::new();
    let exchange_opts = ExchangeOpts {
        step: t.step,
        quiet: t.quiet,
        parameter_status: false,
    };

    for step in &scenario.steps {
        if raw_clients.contains(step.client.as_str()) {
            if let Some(ms) = step.delay_ms {
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
            let outcome = match step.action()? {
                Action::Send(messages) => {
                    if !raw.contains_key(step.client.as_str()) {
                        let url = if step.direct { direct } else { pooled };
                        match RawConn::connect(url, t.connect).await {
                            Ok(conn) => {
                                raw.insert(&step.client, conn);
                            }
                            Err(e) => {
                                outcomes.push(connect_failure(&e));
                                continue;
                            }
                        }
                    }
                    let conn = raw.get_mut(step.client.as_str()).expect("just connected");
                    match conn.exchange(&messages, &exchange_opts).await {
                        Ok(messages) => Outcome::Messages { messages },
                        Err(e) => Outcome::error("CLIENT", format!("{e:#}")),
                    }
                }
                Action::Disconnect => {
                    if let Some(conn) = raw.remove(step.client.as_str()) {
                        conn.terminate(on_postgres(step.direct).then_some(t.step))
                            .await
                            .with_context(|| format!("client '{}' disconnecting", step.client))?;
                    }
                    Outcome::empty()
                }
                _ => bail!(
                    "client '{}' uses send, so its only other step can be disconnect",
                    step.client
                ),
            };
            outcomes.push(outcome);
            continue;
        }
        if !conns.contains_key(step.client.as_str()) {
            let url = if step.direct { direct } else { pooled };
            match connect_with(url, t, on_postgres(step.direct)).await {
                Ok((client, driver, notices)) => {
                    conns.insert(
                        &step.client,
                        Conn {
                            client: Arc::new(client),
                            driver,
                            statements: HashMap::new(),
                            pending: None,
                            notices,
                            on_postgres: on_postgres(step.direct),
                        },
                    );
                }
                Err(e) => {
                    outcomes.push(connect_failure(&e));
                    continue;
                }
            }
        }
        let Some(conn) = conns.get_mut(step.client.as_str()) else {
            bail!("client '{}' is not connected", step.client);
        };
        if let Some(ms) = step.delay_ms {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }

        let outcome = match step.action()? {
            Action::Send(_) => unreachable!("send clients are handled above"),
            Action::Simple(sql) => match timeout(t.step, conn.client.simple_query(sql)).await {
                Err(_) => Outcome::error("TIMEOUT", format!("no reply within {:?}", t.step)),
                Ok(res) => simple_outcome(res),
            },
            Action::Abandon { sql, after } => {
                let outcome = match timeout(after, conn.client.simple_query(sql)).await {
                    Err(_) => Outcome::Abandoned,
                    Ok(res) => simple_outcome(res),
                };
                // Aborting the driver drops the socket without sending Terminate.
                if let Some(conn) = conns.remove(step.client.as_str()) {
                    if let Some(pending) = conn.pending {
                        pending.abort();
                    }
                    conn.driver.abort();
                    let _ = conn.driver.await;
                }
                outcome
            }
            Action::Prepare { sql, name } => match timeout(t.step, conn.client.prepare(sql)).await {
                Err(_) => Outcome::error("TIMEOUT", format!("no reply within {:?}", t.step)),
                Ok(Err(e)) => db_error(&e),
                Ok(Ok(statement)) => {
                    conn.statements.insert(name.to_string(), statement);
                    Outcome::empty()
                }
            },
            Action::Execute(name) => {
                // The prepare step may have failed through the pooler; that mismatch is
                // already reported, so record this step as an error instead of aborting.
                let Some(statement) = conn.statements.get(name) else {
                    outcomes.push(Outcome::error("NO_STATEMENT", format!("'{name}' was never prepared")));
                    continue;
                };
                match timeout(t.step, conn.client.query(statement, &[])).await {
                    Err(_) => Outcome::error("TIMEOUT", format!("no reply within {:?}", t.step)),
                    Ok(Err(e)) => db_error(&e),
                    Ok(Ok(rows)) => Outcome::Ok {
                        rows: rows
                            .iter()
                            .map(|r| (0..r.len()).map(|i| row_value(r, i)).collect())
                            .collect(),
                        tags: vec![rows.len() as u64],
                    },
                }
            }
            Action::Disconnect => {
                if let Some(conn) = conns.remove(step.client.as_str()) {
                    conn.close(t)
                        .await
                        .with_context(|| format!("client '{}' disconnecting", step.client))?;
                }
                Outcome::empty()
            }
            Action::Background(sql) => {
                if conn.pending.is_some() {
                    bail!("client '{}' already has a background query", step.client);
                }
                let client = conn.client.clone();
                let sql = sql.to_string();
                let step_timeout = t.step;
                conn.pending = Some(tokio::spawn(async move {
                    match timeout(step_timeout, client.simple_query(&sql)).await {
                        Err(_) => Outcome::error("TIMEOUT", format!("no reply within {step_timeout:?}")),
                        Ok(res) => simple_outcome(res),
                    }
                }));
                Outcome::empty()
            }
            Action::Wait => {
                let Some(pending) = conn.pending.take() else {
                    bail!("client '{}' has no background query to wait for", step.client);
                };
                // The query has its own timeout; this one only guards against a stuck task.
                match timeout(t.step * 2, pending).await {
                    Ok(Ok(outcome)) => outcome,
                    Ok(Err(e)) => Outcome::error("CLIENT", e.to_string()),
                    Err(_) => Outcome::error("TIMEOUT", "background query never finished"),
                }
            }
            Action::Cancel => {
                let token = conn.client.cancel_token();
                let endpoint = if step.direct { direct } else { pooled };
                match timeout(t.step, cancel(&token, endpoint)).await {
                    Err(_) => Outcome::error("TIMEOUT", format!("no reply within {:?}", t.step)),
                    Ok(Err(e)) => match e.downcast_ref::<tokio_postgres::Error>() {
                        Some(e) => db_error(e),
                        None => Outcome::error("CLIENT", format!("{e:#}")),
                    },
                    Ok(Ok(())) => Outcome::empty(),
                }
            }
            Action::Notifications => {
                let deadline = tokio::time::Instant::now() + t.notify;
                while conn.notices.lock().unwrap().is_empty() && tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                // Give any NOTIFYs sent together with the first one time to arrive.
                tokio::time::sleep(Duration::from_millis(100)).await;
                let rows = std::mem::take(&mut *conn.notices.lock().unwrap());
                Outcome::Ok { rows, tags: vec![] }
            }
        };
        outcomes.push(outcome);
    }

    // Every client closes here or at its disconnect step, and on Postgres each waits
    // for its backend to exit, so the next run doesn't see this one's session state.
    // A client that abandoned its query dropped the socket on purpose and isn't waited
    // for: its backend keeps running the query.
    let direct_clients: HashMap<&str, bool> = scenario.steps.iter().map(|s| (s.client.as_str(), s.direct)).collect();
    for (client, conn) in conns.drain() {
        conn.close(t)
            .await
            .with_context(|| format!("client '{client}' closing at the end"))?;
    }
    for (client, conn) in raw.drain() {
        let wait = on_postgres(direct_clients.get(client).copied().unwrap_or(false)).then_some(t.step);
        conn.terminate(wait)
            .await
            .with_context(|| format!("client '{client}' closing at the end"))?;
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server that answers every startup with a FATAL error `code`, like a Postgres
    /// that has run out of connections. Declines TLS if asked.
    async fn refusing_server(code: &'static str) -> Endpoint {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    loop {
                        let len = socket.read_u32().await.unwrap() as usize;
                        let mut body = vec![0; len - 4];
                        socket.read_exact(&mut body).await.unwrap();
                        // SSLRequest: say no and wait for the startup message.
                        if body == [0x04, 0xd2, 0x16, 0x2f] {
                            socket.write_all(b"N").await.unwrap();
                            continue;
                        }
                        break;
                    }
                    let fields = format!("SFATAL\0VFATAL\0C{code}\0Msorry, too many clients already\0\0");
                    let mut reply = vec![b'E'];
                    reply.extend((fields.len() as u32 + 4).to_be_bytes());
                    reply.extend(fields.as_bytes());
                    let _ = socket.write_all(&reply).await;
                });
            }
        });
        Endpoint::parse(&format!("postgres://u@127.0.0.1:{port}/db")).unwrap()
    }

    #[tokio::test]
    async fn connect_refused_by_the_server_records_its_sqlstate() {
        let scenario: Scenario = toml::from_str(
            r#"
            name = "refused"
            description = ""
            [[step]]
            client = "a"
            simple = "SELECT 1"
            [[step]]
            client = "b"
            send = ["sync"]
            "#,
        )
        .unwrap();
        let server = refusing_server("53300").await;
        let outcomes = run_steps(&scenario, &server, &server, &Timeouts::default())
            .await
            .unwrap();
        for outcome in outcomes {
            assert!(
                matches!(&outcome, Outcome::Error { code, .. } if code == "53300"),
                "{outcome}"
            );
        }

        // Nothing listening: a transport failure stays CONNECT.
        let closed = Endpoint::parse("postgres://u@127.0.0.1:1/db").unwrap();
        let outcomes = run_steps(&scenario, &closed, &closed, &Timeouts::default())
            .await
            .unwrap();
        for outcome in outcomes {
            assert!(
                matches!(&outcome, Outcome::Error { code, .. } if code == "CONNECT"),
                "{outcome}"
            );
        }
    }

    #[tokio::test]
    async fn postgres_refusing_connections_is_an_error_not_a_pass() {
        let scenario: Scenario =
            toml::from_str("name = 'r'\ndescription = ''\n[[step]]\nclient = 'a'\nsimple = 'SELECT 1'\n").unwrap();
        let server = refusing_server("53300").await;
        let checked = crate::runner::check(&scenario, &server, &server, &[], &Timeouts::default()).await;
        assert!(
            matches!(&checked.verdict, crate::runner::Verdict::Error { reason } if reason.contains("53300")),
            "{:?}",
            checked.verdict
        );
    }

    #[test]
    fn connect_flag_stays_out_of_stored_results() {
        let plain = serde_json::to_string(&Outcome::error("42P01", "x")).unwrap();
        assert!(!plain.contains("connect"), "{plain}");
        let back: Outcome = serde_json::from_str(&plain).unwrap();
        assert_eq!(back, Outcome::error("42P01", "x"));
    }

    /// A Postgres URL for tests that need a real server, e.g. a throwaway container.
    fn test_pg() -> Endpoint {
        Endpoint::parse(&std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL")).unwrap()
    }

    fn canary(text: &str) -> Scenario {
        toml::from_str(&format!("name = 'c4'\ndescription = ''\n{text}")).unwrap()
    }

    /// Steps for client `b` that check `a`'s session is gone: its advisory lock free
    /// and its temp tables dropped.
    const CHECK_GONE: &str = "
        [[step]]
        client = 'b'
        simple = \"SELECT pg_try_advisory_lock(4204)::text, (SELECT count(*) FROM pg_class WHERE relname LIKE 'pp_c4_tmp%')::text\"
        [[step]]
        client = 'b'
        simple = 'SELECT pg_advisory_unlock_all()'
        [[step]]
        client = 'b'
        disconnect = true
    ";

    fn assert_gone(outcomes: &[Outcome], step: usize, run: usize) {
        let expected = Outcome::Ok {
            rows: vec![vec![Some("true".into()), Some("0".into())]],
            tags: vec![1],
        };
        assert_eq!(
            outcomes[step], expected,
            "run {run}: the closed session was still there"
        );
    }

    /// C4: once a client on Postgres is closed, its backend has exited, on every close
    /// path. 200 runs each, as a regression test for the race fixed in 36578eb.
    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn closed_sessions_are_gone_before_the_next_step() {
        let pg = test_pg();
        let t = Timeouts::default();
        let lock = "SELECT pg_advisory_lock(4204)::text";
        // Many temp tables make the backend's exit slow enough that a check that
        // doesn't wait for it sees the session still there.
        let temp = "DO $$ BEGIN FOR i IN 1..400 LOOP \
                    EXECUTE format('CREATE TEMP TABLE pp_c4_tmp%s (x int)', i); END LOOP; END $$";
        // b connects first, as an observer in a scenario usually has, so its check
        // runs the moment a's close returns.
        let b_first = "[[step]]\nclient = 'b'\nsimple = 'SELECT 1'\n";
        let disconnect = canary(&format!(
            "{b_first}[[step]]\nclient = 'a'\nsimple = \"{lock}; {temp}\"\n[[step]]\nclient = 'a'\ndisconnect = true\n{CHECK_GONE}"
        ));
        let raw = canary(&format!(
            "{b_first}[[step]]\nclient = 'a'\nsend = [\"query {lock}; {temp}\"]\n[[step]]\nclient = 'a'\ndisconnect = true\n{CHECK_GONE}"
        ));
        let left_open = canary(&format!("[[step]]\nclient = 'a'\nsimple = \"{lock}; {temp}\"\n"));
        let raw_left_open = canary(&format!("[[step]]\nclient = 'a'\nsend = [\"query {lock}; {temp}\"]\n"));
        let check = canary(CHECK_GONE);
        for run in 0..200 {
            assert_gone(&run_steps(&disconnect, &pg, &pg, &t).await.unwrap(), 3, run);
            assert_gone(&run_steps(&raw, &pg, &pg, &t).await.unwrap(), 3, run);
            // Closed at the end of the run, not by a step.
            run_steps(&left_open, &pg, &pg, &t).await.unwrap();
            assert_gone(&run_steps(&check, &pg, &pg, &t).await.unwrap(), 0, run);
            run_steps(&raw_left_open, &pg, &pg, &t).await.unwrap();
            assert_gone(&run_steps(&check, &pg, &pg, &t).await.unwrap(), 0, run);
        }
    }

    #[tokio::test]
    async fn send_client_cannot_mix_direct_and_pooled() {
        let scenario: Scenario = toml::from_str(
            r#"
            name = "mixed"
            description = ""
            [[step]]
            client = "a"
            send = ["sync"]
            [[step]]
            client = "a"
            direct = true
            send = ["sync"]
            "#,
        )
        .unwrap();
        // Rejected before connecting, so the unreachable URLs are never used.
        let pooled = Endpoint::parse("postgres://u@127.0.0.1:1/db").unwrap();
        let direct = Endpoint::parse("postgres://u@127.0.0.1:2/db").unwrap();
        let err = run_steps(&scenario, &pooled, &direct, &Timeouts::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("mixes direct and pooled"), "{err}");
    }
}
