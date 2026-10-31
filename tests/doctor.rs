//! `doctor --format json` goes into issues, so it must not name hosts, users or
//! addresses, nor carry server error messages. Text output keeps them.

use std::process::Command;

/// Runs a statement on a fresh connection from a plain thread, so it works in Drop.
fn run_sql(url: &str, sql: &str) {
    let (url, sql) = (url.to_string(), sql.to_string());
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.unwrap();
            let driver = tokio::spawn(connection);
            client.batch_execute(&sql).await.unwrap();
            drop(client);
            let _ = driver.await;
        });
    })
    .join()
    .unwrap();
}

/// A login role for one test run, dropped even when an assert fails.
struct TempRole {
    base: String,
    name: String,
}

impl Drop for TempRole {
    fn drop(&mut self) {
        run_sql(&self.base, &format!("DROP ROLE IF EXISTS {}", self.name));
    }
}

fn doctor(url: &str, format: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_poolproof"))
        .args([
            "doctor", "--pooler", url, "--direct", url, "--reset", "all", "--format", format,
        ])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

#[tokio::test]
#[ignore = "needs POOLPROOF_TEST_PG"]
async fn json_leaves_out_hosts_users_addresses_and_error_messages() {
    let base = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let password = format!("pp{}x{nanos}", std::process::id());
    let role = TempRole {
        base: base.clone(),
        name: format!("pp_doctor_{}", std::process::id()),
    };
    run_sql(&base, &format!("CREATE ROLE {} LOGIN PASSWORD '{password}'", role.name));

    let (client, connection) = tokio_postgres::connect(&base, tokio_postgres::NoTls).await.unwrap();
    let driver = tokio::spawn(connection);
    let addr: String = client
        .query_one("SELECT host(inet_server_addr())", &[])
        .await
        .unwrap()
        .get(0);
    drop(client);
    let _ = driver.await;

    let config: tokio_postgres::Config = base.parse().unwrap();
    let tokio_postgres::config::Host::Tcp(host) = &config.get_hosts()[0] else {
        panic!("tcp host")
    };
    let port = config.get_ports()[0];
    let url = |password: &str| format!("postgres://{}:{password}@{host}:{port}/postgres", role.name);

    let text = doctor(&url(&password), "text");
    let json = doctor(&url(&password), "json");
    for secret in [role.name.as_str(), host.as_str(), addr.as_str(), &format!(":{port}")] {
        assert!(text.contains(secret), "text should show {secret}:\n{text}");
        assert!(!json.contains(secret), "json shows {secret}:\n{json}");
    }
    assert!(!text.contains(&password) && !json.contains(&password));
    let report: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(report["targets"][0]["checks"][0]["detail"], "connected");

    // Postgres names the user in its error; JSON keeps only the class. This half needs
    // a server that checks passwords: with trust auth the wrong one connects.
    let json = doctor(&url("wrong"), "json");
    let report: serde_json::Value = serde_json::from_str(&json).unwrap();
    let detail = report["targets"][0]["checks"][0]["detail"].as_str().unwrap();
    if detail == "connected" {
        eprintln!("note: the server doesn't check passwords (trust auth?), so the failed-connect half is skipped");
        return;
    }
    let text = doctor(&url("wrong"), "text");
    assert!(text.contains(&role.name), "{text}");
    assert!(!json.contains(&role.name), "{json}");
    assert!(detail.ends_with("connect failed: authentication (28P01)"), "{detail}");
}
