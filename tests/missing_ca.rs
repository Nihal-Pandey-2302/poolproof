//! A target whose CA file is missing fails on its own; the others still run.

use std::path::PathBuf;
use std::process::Command;

mod common;
use common::echoing_server;

#[test]
fn missing_ca_file_is_an_error_for_its_target_only() {
    let port = echoing_server();
    let plain = format!("postgres://u@127.0.0.1:{port}/app?sslmode=disable");
    let missing = format!("postgres://u@127.0.0.1:{port}/app?sslmode=verify-full&sslrootcert=/nonexistent/ca.pem");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("missing-ca-targets.toml");
    std::fs::write(
        &path,
        format!(
            "[[target]]\nname = 'good'\nmode = 'transaction'\npooler = '{plain}'\ndirect = '{plain}'\n\
             [[target]]\nname = 'missing-ca'\nmode = 'transaction'\npooler = '{missing}'\ndirect = '{plain}'\n"
        ),
    )
    .unwrap();
    let started = std::time::Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_poolproof"))
        .args(["run", "--targets", path.to_str().unwrap(), "--format", "json"])
        .args(["--filter", "set-isolated-between-clients"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    // Not retried for the 10 s a pooler gets to come up: the file won't appear.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for r in report["results"].as_array().unwrap() {
        let reason = r["reason"].as_str().unwrap_or_default();
        match r["target"].as_str().unwrap() {
            "missing-ca" => {
                assert_eq!(r["result"], "error");
                assert!(
                    reason.contains("reading root certificates from /nonexistent/ca.pem"),
                    "{reason}"
                );
            }
            // It ran: whatever its verdict, not a target-level failure.
            _ => assert!(!reason.contains("target unreachable"), "{reason}"),
        }
    }
}
