use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use tokio::time::timeout;
use tokio_postgres::NoTls;

use super::pg::{Conn, backend_pid, connect, db_error, row_value, simple_outcome, wait_backend_gone};
use super::raw::{ExchangeOpts, RawConn};
use super::{Outcome, Timeouts};
use crate::scenario::{Action, Scenario};
use crate::targets::Endpoint;

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
                        match RawConn::connect(url.config(), t.connect).await {
                            Ok(conn) => {
                                raw.insert(&step.client, conn);
                            }
                            Err(e) => {
                                outcomes.push(Outcome::error("CONNECT", format!("{e:#}")));
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
                        let pid = conn.pid;
                        conn.terminate().await;
                        if let (true, Some(pid)) = (on_postgres(step.direct), pid) {
                            wait_backend_gone(direct, pid, t).await;
                        }
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
            match connect(url, t).await {
                Ok((client, driver, notices)) => {
                    let pid = if url == direct {
                        backend_pid(&client, t).await
                    } else {
                        None
                    };
                    conns.insert(
                        &step.client,
                        Conn {
                            client: Arc::new(client),
                            driver,
                            statements: HashMap::new(),
                            pending: None,
                            notices,
                            pid,
                        },
                    );
                }
                Err(e) => {
                    outcomes.push(Outcome::error("CONNECT", format!("{e:#}")));
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
                    let pid = conn.pid;
                    conn.close(t).await;
                    if let Some(pid) = pid {
                        wait_backend_gone(direct, pid, t).await;
                    }
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
                match timeout(t.step, token.cancel_query(NoTls)).await {
                    Err(_) => Outcome::error("TIMEOUT", format!("no reply within {:?}", t.step)),
                    Ok(Err(e)) => db_error(&e),
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

    for (_, conn) in conns.drain() {
        conn.close(t).await;
    }
    for (_, conn) in raw.drain() {
        conn.terminate().await;
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

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
