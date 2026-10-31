//! An unreachable Postgres must give errors, never passes: both sides failing to
//! connect the same way is not a match.

use std::process::Command;

use poolproof::runner::DIRECT_UNREACHABLE;

#[test]
fn closed_port_gives_no_passes() {
    let url = "postgres://x:y@127.0.0.1:1/app";
    let out = Command::new(env!("CARGO_BIN_EXE_poolproof"))
        .args(["run", "--pooler", url, "--direct", url, "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));

    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let results = report["results"].as_array().unwrap();
    assert!(!results.is_empty());
    for r in results {
        match r["result"].as_str().unwrap() {
            "skip" => {}
            "error" => assert!(r["reason"].as_str().unwrap().starts_with(DIRECT_UNREACHABLE)),
            other => panic!("{}: {other}", r["scenario"]),
        }
    }
}

#[test]
fn doctor_fails_on_a_closed_port_without_showing_the_password() {
    let url = "postgres://x:secret@127.0.0.1:1/app";
    let out = Command::new(env!("CARGO_BIN_EXE_poolproof"))
        .args(["doctor", "--pooler", url, "--direct", url, "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("secret"), "{text}");

    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let checks = report["targets"][0]["checks"].as_array().unwrap();
    // Nothing past the connections can run.
    assert_eq!(checks.len(), 2, "{checks:?}");
    assert_eq!(checks[0]["check"], "direct");
    assert_eq!(checks[0]["status"], "fail");
    assert!(checks[0]["detail"].as_str().unwrap().starts_with(DIRECT_UNREACHABLE));
    assert_eq!(checks[1]["check"], "pooler");
    assert_eq!(checks[1]["status"], "fail");
}
