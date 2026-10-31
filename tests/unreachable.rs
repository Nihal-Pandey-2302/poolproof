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
