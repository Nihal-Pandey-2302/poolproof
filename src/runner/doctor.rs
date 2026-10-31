//! The live side of `poolproof doctor`: connects to one target and runs its checks.
//! SELECTs only. Through the pooler nothing that changes session state, since in
//! transaction mode it would stay on a server connection other clients get next.

use anyhow::{Context, Result};
use bytes::BytesMut;
use postgres_protocol::message::frontend;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_postgres::Client;
use tokio_postgres::config::Host;

use super::Timeouts;
use super::check::{DIRECT_UNREACHABLE, POOLER_UNREACHABLE};
use super::pg::{Privileges, ResetRefusal, connect, privileges, resolve_reset, tcp_connect};
use super::tls::SslMode;
use crate::doctor::{
    Check, Connected, Status, TargetReport, first_row, mode_check, probe_mode, same_server, server_facts,
};
use crate::scenario::Scenario;
use crate::targets::{Endpoint, Reset, Target};

/// Counts the other sessions on the database, as `reset = "all"` sees them; more
/// conditions can follow. Without pg_read_all_stats, other roles' sessions have no
/// backend_type here, so they count only when it is NULL.
const OTHER_SESSIONS_COUNT: &str = "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() \
    AND pid <> pg_backend_pid() AND (backend_type = 'client backend' OR backend_type IS NULL)";

pub async fn inspect(target: &Target, scenarios: &[Scenario], yes: bool, t: &Timeouts) -> TargetReport {
    let mut checks = vec![];
    let direct = open_side(&mut checks, "direct", &target.direct, t).await;
    let pooler = open_side(&mut checks, "pooler", &target.pooler, t).await;

    // Who Postgres sees the pooler's sessions come from, for the reset preview.
    let mut pooler_session = None;
    if let (Some((d, _)), Some((p, _))) = (&direct, &pooler) {
        match (
            server_facts(p, t.housekeeping).await,
            server_facts(d, t.housekeeping).await,
        ) {
            (Ok(p), Ok(d)) => checks.push(same_server(&p, &d)),
            (p, d) => {
                let e = p.err().or(d.err()).unwrap();
                checks.push(Check::new("same server", Status::Fail, format!("{e:#}")).public(failed("query", &e)));
            }
        }
        pooler_session = row(p, "SELECT host(inet_client_addr()), session_user", t).await.ok();
    }
    // Before the mode probe: in session mode with a pool of one, this client would
    // hold the only server connection.
    if let Some(pooler) = pooler {
        close(pooler, t).await;
    }

    if let Some((d, _)) = &direct {
        match privileges(&target.direct, t).await {
            Ok(have) => {
                checks.push(requirements(target, scenarios, have));
                checks.push(reset(target, yes, have, d, pooler_session.as_deref(), t).await);
            }
            Err(e) => checks.push(Check::new("privileges", Status::Fail, format!("{e:#}")).public(failed("query", &e))),
        }
    }
    if let Some(direct) = direct {
        close(direct, t).await;
    }

    if checks.iter().any(|c| c.check == "pooler" && c.status == Status::Ok) {
        let connect = || async {
            let (client, driver, _) = connect(&target.pooler, t).await?;
            Ok::<Connected, anyhow::Error>((client, driver))
        };
        checks.push(match probe_mode(connect, t.housekeeping).await {
            Ok(observed) => mode_check(observed, target.mode),
            Err(e) => Check::new("mode", Status::Warn, format!("probe failed: {e:#}")).public(failed("probe", &e)),
        });
    }
    TargetReport {
        name: target.name.clone(),
        checks,
    }
}

/// Connects to one side and pushes its connect and TLS lines. On direct, this session
/// is made read-only as a guard (privileges() and resolve_reset() open their own,
/// which run only SELECTs).
async fn open_side(
    checks: &mut Vec<Check>,
    side: &'static str,
    endpoint: &Endpoint,
    t: &Timeouts,
) -> Option<(Client, JoinHandle<()>)> {
    let unreachable = if side == "direct" {
        DIRECT_UNREACHABLE
    } else {
        POOLER_UNREACHABLE
    };
    let connected = async {
        let (client, driver, _) = connect(endpoint, t).await?;
        let first = if side == "direct" {
            "SET default_transaction_read_only = on"
        } else {
            "SELECT 1"
        };
        match timeout(t.housekeeping, client.simple_query(first)).await {
            Ok(Ok(_)) => Ok((client, driver)),
            Ok(Err(e)) => Err(anyhow::Error::from(e)),
            Err(elapsed) => Err(anyhow::Error::from(elapsed).context(format!("no reply within {:?}", t.housekeeping))),
        }
    };
    match connected.await {
        Ok((client, driver)) => {
            checks.push(Check::new(side, Status::Ok, format!("connected to {endpoint}")).public("connected"));
            checks.push(tls(side, endpoint, &client, t).await);
            Some((client, driver))
        }
        Err(e) => {
            checks.push(
                Check::new(side, Status::Fail, format!("{unreachable} {e:#}"))
                    .public(format!("{unreachable} {}", failed("connect", &e))),
            );
            None
        }
    }
}

async fn tls(side: &'static str, endpoint: &Endpoint, client: &Client, t: &Timeouts) -> Check {
    let check = if side == "direct" { "direct tls" } else { "pooler tls" };
    let mode = endpoint.tls().mode;
    #[cfg(unix)]
    if matches!(endpoint.config().get_hosts().first(), Some(Host::Unix(_))) {
        return Check::new(check, Status::Ok, "Unix socket, no TLS");
    }
    if mode == SslMode::Disable {
        return Check::new(check, Status::Ok, "plaintext (sslmode=disable)");
    }
    // On direct Postgres says what this session uses. Through a pooler that would
    // describe the pooler's own server connection, so ask the pooler instead.
    let encrypted = if side == "direct" {
        row(
            client,
            "SELECT ssl, version FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
            t,
        )
        .await
        .map(|r| {
            (
                r.first().cloned().flatten() == Some("t".into()),
                r.get(1).cloned().flatten(),
            )
        })
    } else if mode == SslMode::Prefer {
        offers_tls(endpoint, t).await.map(|offered| (offered, None))
    } else {
        // Anything stricter than prefer fails to connect without TLS.
        Ok((true, None))
    };
    match encrypted {
        Ok((true, version)) => {
            let version = version.map(|v| format!("{v}, ")).unwrap_or_default();
            Check::new(check, Status::Ok, format!("TLS ({version}sslmode={mode})"))
        }
        Ok((false, _)) => Check::new(
            check,
            Status::Warn,
            "plaintext: sslmode=prefer and the server declined TLS; set sslmode=require to insist",
        ),
        Err(e) => Check::new(check, Status::Warn, format!("couldn't tell: {e:#}"))
            .public(format!("couldn't tell: {}", error_class(&e))),
    }
}

/// Sends an SSLRequest and reads the one-byte answer, then hangs up.
async fn offers_tls(endpoint: &Endpoint, t: &Timeouts) -> Result<bool> {
    let config = endpoint.config();
    let port = config.get_ports().first().copied().unwrap_or(5432);
    let Some(Host::Tcp(host)) = config.get_hosts().first() else {
        return Ok(false);
    };
    let asking = async {
        let mut stream = tcp_connect(config, host, port).await?;
        let mut out = BytesMut::new();
        frontend::ssl_request(&mut out);
        stream.write_all(&out).await?;
        let answer = stream.read_u8().await.context("reading the SSLRequest answer")?;
        Ok(answer == b'S')
    };
    timeout(t.housekeeping, asking)
        .await
        .with_context(|| format!("no answer to SSLRequest within {:?}", t.housekeeping))?
}

/// The scenarios that would SKIP for privileges the direct user lacks.
fn requirements(target: &Target, scenarios: &[Scenario], have: Privileges) -> Check {
    let skipped: Vec<String> = scenarios
        .iter()
        .filter(|s| s.modes.contains(&target.mode))
        .filter_map(|s| have.missing(&s.requires).map(|why| format!("{} ({why})", s.name)))
        .collect();
    if skipped.is_empty() {
        return Check::new("privileges", Status::Ok, "every scenario's requirements are met");
    }
    let detail = format!("{} scenarios will SKIP: {}", skipped.len(), skipped.join(", "));
    Check::new("privileges", Status::Warn, detail)
}

/// What reset would do, and whether the direct user can end those sessions. For a
/// backend of a role whose privileges this user doesn't have, pg_terminate_backend
/// needs pg_signal_backend, and the reset's own filter needs pg_read_all_stats:
/// without it pg_stat_activity hides that backend's type and address, so it never
/// matches. Only a superuser can end a superuser's backends, whatever else it has.
async fn reset(
    target: &Target,
    yes: bool,
    have: Privileges,
    direct: &Client,
    pooler_session: Option<&[Option<String>]>,
    t: &Timeouts,
) -> Check {
    let fail = |detail: String| Check::new("reset", Status::Fail, detail);
    let query_failed = |e: anyhow::Error| fail(format!("{e:#}")).public(failed("query", &e));
    let line = match &target.reset {
        Reset::None => {
            return Check::new(
                "reset",
                Status::Warn,
                "none: one scenario's leaked state can change the next one's result",
            );
        }
        Reset::Statements(s) => {
            return Check::new(
                "reset",
                Status::Ok,
                format!("custom: {} statements, not previewed", s.len()),
            );
        }
        Reset::All | Reset::Auto => resolve_reset(target, yes, t).await,
    };
    let missing: Vec<&str> = [
        (have.pg_signal_backend, "pg_signal_backend"),
        (have.pg_read_all_stats, "pg_read_all_stats"),
    ]
    .into_iter()
    .filter(|(has, _)| !has && !have.superuser)
    .map(|(_, name)| name)
    .collect();
    let missing = missing.join(" and ");

    if let Reset::All = target.reset {
        let counts = row(
            direct,
            &format!(
                "SELECT ({OTHER_SESSIONS_COUNT}), ({OTHER_SESSIONS_COUNT} AND usesysid IS NOT NULL \
                 AND NOT pg_has_role(current_user, usesysid, 'USAGE')), \
                 ({OTHER_SESSIONS_COUNT} AND usesysid IN (SELECT oid FROM pg_roles WHERE rolsuper))"
            ),
            t,
        )
        .await;
        let (total, others, superusers) = match counts {
            Ok(r) => (cell(&r, 0), cell(&r, 1), cell(&r, 2)),
            Err(e) => return query_failed(e),
        };
        let now = format!("other sessions now: {total}");
        // Privileges first: once those are fine, the only refusal left is `run --yes`.
        if superusers != "0" && !have.superuser {
            return fail(format!(
                "all: {superusers} of the {total} other sessions belong to superusers, and only a superuser can \
                 end them"
            ));
        }
        if others != "0" && !missing.is_empty() {
            return fail(format!(
                "all: {others} of the {total} other sessions belong to roles whose sessions this user can't end \
                 without {missing}"
            ));
        }
        return match line {
            Err(ResetRefusal::CantEnd { text, public }) => {
                fail(format!("all: {text}")).public(format!("all: {public}"))
            }
            Err(reason @ ResetRefusal::NeedsYes(_)) => {
                Check::new("reset", Status::Warn, format!("all: {reason}; {now}")).public(format!(
                    "all: run needs --yes for a Postgres that isn't on this machine; {now}"
                ))
            }
            Err(reason @ ResetRefusal::CantTell(_)) => fail(format!("all: {reason}")).public("all: refused"),
            Ok((_, line)) => Check::new("reset", Status::Ok, format!("{}; {now}", summary(&line)))
                .public(format!("all: ends every other session on the database; {now}")),
        };
    }

    let line = match line {
        Ok((_, line)) => line,
        Err(ResetRefusal::CantEnd { text, public }) => {
            return fail(format!("auto: {text}")).public(format!("auto: {public}"));
        }
        Err(reason) => {
            return fail(format!("auto: {reason}"))
                .public("auto: refused: reset = \"auto\" can't tell the pooler's sessions apart");
        }
    };
    let (Some(Some(addr)), Some(Some(user))) = (
        pooler_session.and_then(|r| r.first()),
        pooler_session.and_then(|r| r.get(1)),
    ) else {
        return Check::new("reset", Status::Ok, summary(&line)).public(AUTO_PUBLIC);
    };
    let sql = format!(
        "SELECT ({OTHER_SESSIONS_COUNT} AND host(client_addr) = $1 AND usename = $2)::text, \
         pg_has_role(current_user, $2, 'USAGE')::text, \
         coalesce((SELECT rolsuper FROM pg_roles WHERE rolname = $2), false)::text"
    );
    let checked = timeout(t.housekeeping, direct.query_one(&sql, &[addr, user])).await;
    let (count, member, superuser) = match checked {
        Ok(Ok(r)) => (
            r.get::<_, String>(0),
            r.get::<_, String>(1) == "true",
            r.get::<_, String>(2) == "true",
        ),
        Ok(Err(e)) => return query_failed(e.into()),
        Err(elapsed) => {
            return query_failed(anyhow::Error::from(elapsed).context(format!("no reply within {:?}", t.housekeeping)));
        }
    };
    if superuser && !have.superuser {
        return fail(format!(
            "{}, but {user} is a superuser, and only a superuser can end its sessions",
            summary(&line)
        ))
        .public(format!(
            "{AUTO_PUBLIC}, but that user is a superuser, and only a superuser can end its sessions"
        ));
    }
    if !member && !missing.is_empty() {
        return fail(format!(
            "{}, but this user can't end sessions of {user} without {missing}",
            summary(&line)
        ))
        .public(format!(
            "{AUTO_PUBLIC}, but this user can't end that user's sessions without {missing}"
        ));
    }
    Check::new(
        "reset",
        Status::Ok,
        format!("{}; matching sessions now: {count}", summary(&line)),
    )
    .public(format!("{AUTO_PUBLIC}; matching sessions now: {count}"))
}

/// "<what> failed: <class>", the JSON form of an error.
fn failed(what: &str, e: &anyhow::Error) -> String {
    format!("{what} failed: {}", error_class(e))
}

/// What kind of error this is, without its message: server and TLS errors can name
/// users, hosts and every name on a certificate, and JSON goes into issues.
pub fn error_class(e: &anyhow::Error) -> String {
    for cause in e.chain() {
        if let Some(db) = cause
            .downcast_ref::<tokio_postgres::Error>()
            .and_then(|e| e.as_db_error())
        {
            let code = db.code().code();
            return if code.starts_with("28") {
                format!("authentication ({code})")
            } else {
                format!("SQLSTATE {code}")
            };
        }
        if cause.is::<tokio::time::error::Elapsed>() {
            return "timeout".into();
        }
        let io = cause.downcast_ref::<std::io::Error>();
        let tls = cause.downcast_ref::<rustls::Error>().or_else(|| {
            io.and_then(|io| io.get_ref())
                .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        });
        match tls {
            Some(rustls::Error::InvalidCertificate(_)) => return "TLS certificate".into(),
            Some(_) => return "TLS".into(),
            None => {}
        }
        match io.map(std::io::Error::kind) {
            Some(std::io::ErrorKind::ConnectionRefused) => return "refused".into(),
            Some(std::io::ErrorKind::TimedOut) => return "timeout".into(),
            _ => {}
        }
    }
    "other".into()
}

/// What `auto` ends, for JSON: without the user, address and host it names in text.
const AUTO_PUBLIC: &str = "auto: ends every session of the pooler's user from the pooler's address";

/// resolve_reset's line without its "reset: 'name': " prefix.
fn summary(line: &str) -> String {
    line.splitn(3, ": ").nth(2).unwrap_or(line).to_string()
}

fn cell(row: &[Option<String>], i: usize) -> String {
    row.get(i).cloned().flatten().unwrap_or_default()
}

async fn row(client: &Client, sql: &str, t: &Timeouts) -> Result<Vec<Option<String>>> {
    first_row(client, sql, t.housekeeping).await
}

async fn close((client, driver): (Client, JoinHandle<()>), t: &Timeouts) {
    drop(client);
    let _ = timeout(t.housekeeping, driver).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::PoolMode;
    use crate::targets::Targets;

    #[tokio::test]
    async fn error_classes_hide_the_message() {
        let refused = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "10.0.0.7:5432");
        assert_eq!(
            error_class(&anyhow::Error::from(refused).context("connecting")),
            "refused"
        );
        let elapsed = timeout(std::time::Duration::ZERO, std::future::pending::<()>())
            .await
            .unwrap_err();
        assert_eq!(
            error_class(&anyhow::Error::from(elapsed).context("connect timed out")),
            "timeout"
        );
        let name = rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidForName);
        let io = std::io::Error::new(std::io::ErrorKind::InvalidData, name);
        assert_eq!(error_class(&anyhow::Error::from(io)), "TLS certificate");
        assert_eq!(error_class(&anyhow::anyhow!("internal.host.example")), "other");
    }

    #[tokio::test]
    async fn a_reset_that_cannot_end_the_poolers_sessions_fails() {
        use super::super::pg::tests::{answering, target};
        let t = Timeouts::default();
        // A plain direct user, and a pooler connecting from 10.0.0.7 as another plain role.
        let port = answering(|sql| {
            if sql.contains("count(*)") {
                vec![Some("2"), Some("0"), Some("0")]
            } else if sql.contains("rolcreaterole") {
                vec![Some("f"), Some("f"), Some("f"), Some("f")]
            } else if sql.contains("inet_client_addr()), session_user") {
                vec![Some("10.0.0.7"), Some("app")]
            } else if sql.contains("session_user") {
                vec![Some("app")]
            } else if sql.contains("WHERE rolname = E'app'") {
                vec![Some("f"), Some("f"), Some("plain")]
            } else {
                vec![Some("10.0.0.1")]
            }
        })
        .await;
        let plain = Privileges {
            superuser: false,
            createrole: false,
            pg_signal_backend: false,
            pg_read_all_stats: false,
        };
        for (reset_kind, kind) in [(Reset::All, "all"), (Reset::Auto, "auto")] {
            let target = target(port, reset_kind);
            let (client, _driver, _) = connect(&target.direct, &t).await.unwrap();
            let check = reset(&target, false, plain, &client, None, &t).await;
            assert!(matches!(check.status, Status::Fail), "{kind}: {}", check.detail);
            assert!(
                check.detail.contains("without pg_signal_backend and pg_read_all_stats"),
                "{kind}: {}",
                check.detail
            );
            // JSON names neither role.
            assert_eq!(
                check.public.as_deref(),
                Some(
                    format!("{kind}: can't end the pooler's sessions: missing pg_signal_backend and pg_read_all_stats")
                        .as_str()
                )
            );
        }
    }

    #[test]
    fn summary_drops_the_target_prefix() {
        assert_eq!(
            summary("reset: 'pg': ends every other session on database app at h:5432 (all)"),
            "ends every other session on database app at h:5432 (all)"
        );
    }

    /// Plain Postgres as both sides: every check runs, and the missing reset warns.
    /// TLS depends on the server and the URL's sslmode, so only sslmode=disable has a
    /// fixed answer.
    #[tokio::test]
    #[ignore = "needs POOLPROOF_TEST_PG"]
    async fn inspect_plain_postgres() {
        let url = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
        let target = Targets::single(&url, &url, PoolMode::Transaction, Reset::None)
            .unwrap()
            .targets
            .remove(0);
        let report = inspect(
            &target,
            &crate::scenario::bundled().unwrap(),
            false,
            &Timeouts::default(),
        )
        .await;
        let tls_checks: Vec<Status> = report
            .checks
            .iter()
            .filter(|c| c.check.ends_with(" tls"))
            .map(|c| c.status)
            .collect();
        assert_eq!(tls_checks.len(), 2, "{:#?}", report.checks);
        if target.direct.tls().mode == SslMode::Disable {
            assert_eq!(tls_checks, [Status::Ok, Status::Ok]);
        }
        let checks: Vec<(&str, Status)> = report
            .checks
            .iter()
            .filter(|c| !c.check.ends_with(" tls"))
            .map(|c| (c.check, c.status))
            .collect();
        assert_eq!(
            checks,
            [
                ("direct", Status::Ok),
                ("pooler", Status::Ok),
                ("same server", Status::Ok),
                ("privileges", Status::Ok),
                ("reset", Status::Warn),
                ("mode", Status::Ok),
            ],
            "{:#?}",
            report.checks
        );
        let mode = report.checks.last().unwrap();
        assert!(mode.detail.starts_with("not determined"), "{mode:?}");
    }
}
