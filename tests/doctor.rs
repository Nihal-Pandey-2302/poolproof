//! `doctor --format json` goes into issues, so it must not name hosts, users or
//! addresses. Text output keeps them.

use std::process::Command;

#[tokio::test]
#[ignore = "needs POOLPROOF_TEST_PG"]
async fn json_leaves_out_hosts_users_and_addresses() {
    let base = std::env::var("POOLPROOF_TEST_PG").expect("set POOLPROOF_TEST_PG to a Postgres URL");
    let (client, connection) = tokio_postgres::connect(&base, tokio_postgres::NoTls).await.unwrap();
    let driver = tokio::spawn(connection);
    client
        .batch_execute(
            "DO $$ BEGIN CREATE ROLE pp_doctor_user LOGIN SUPERUSER PASSWORD 'pp_doctor_pw'; \
             EXCEPTION WHEN duplicate_object THEN NULL; END $$",
        )
        .await
        .unwrap();
    let row = client.query_one("SELECT host(inet_server_addr())", &[]).await.unwrap();
    let addr: String = row.get(0);
    drop(client);
    let _ = driver.await;

    let config: tokio_postgres::Config = base.parse().unwrap();
    let tokio_postgres::config::Host::Tcp(host) = &config.get_hosts()[0] else {
        panic!("tcp host")
    };
    let port = config.get_ports()[0];
    let url = format!("postgres://pp_doctor_user:pp_doctor_pw@{host}:{port}/postgres");
    let doctor = |format: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_poolproof"))
            .args([
                "doctor", "--pooler", &url, "--direct", &url, "--reset", "all", "--format", format,
            ])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap()
    };
    let text = doctor("text");
    let json = doctor("json");

    for secret in ["pp_doctor_user", host.as_str(), addr.as_str(), &format!(":{port}")] {
        assert!(text.contains(secret), "text should show {secret}:\n{text}");
        assert!(!json.contains(secret), "json shows {secret}:\n{json}");
    }
    assert!(!text.contains("pp_doctor_pw") && !json.contains("pp_doctor_pw"));
    let report: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(report["targets"][0]["checks"][0]["detail"], "connected");
}
