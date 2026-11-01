//! What the `--pooler` target is called and says it is, and the warning for a targets
//! file entry that documented limitations could never match. Closed ports: nothing
//! here needs a server.

use std::process::Command;

const URL: &str = "postgres://x:y@127.0.0.1:1/app";

/// The report's target for `run --pooler URL --direct URL` plus `extra`.
fn quick_target(extra: &[&str]) -> serde_json::Value {
    let mut args = vec![
        "run",
        "--pooler",
        URL,
        "--direct",
        URL,
        "--format",
        "json",
        "--filter",
        "set-isolated",
    ];
    args.extend_from_slice(extra);
    let out = Command::new(env!("CARGO_BIN_EXE_poolproof"))
        .args(&args)
        .output()
        .unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    report["targets"][0].clone()
}

#[test]
fn quick_target_takes_its_name_and_identity_from_the_flags() {
    let t = quick_target(&[]);
    assert_eq!(
        (&t["name"], &t["product"], &t["config"]),
        (&"quick".into(), &serde_json::Value::Null, &serde_json::Value::Null)
    );

    let t = quick_target(&["--product", "pgbouncer", "--version", "1.26.1"]);
    assert_eq!(t["name"], "pgbouncer-1.26.1");
    assert_eq!(t["product"], "pgbouncer");
    assert_eq!(t["version"], "1.26.1");
    // Documented limitations apply to the default config unless told otherwise.
    assert_eq!(t["config"], "default");

    let t = quick_target(&["--product", "pgbouncer", "--config", "tls"]);
    assert_eq!(t["name"], "pgbouncer-tls");
    assert_eq!(t["config"], "tls");
    assert_eq!(t["version"], serde_json::Value::Null);
}

#[test]
fn product_without_config_in_a_targets_file_warns() {
    let dir = std::env::temp_dir().join(format!("poolproof-quick-target-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stderr = |config: &str| {
        let file = dir.join("targets.toml");
        std::fs::write(
            &file,
            format!(
                "[[target]]\nname = 'bouncer'\npooler = '{URL}'\ndirect = '{URL}'\nmode = 'transaction'\n\
                 product = 'pgbouncer'\nversion = '1.26.1'\n{config}"
            ),
        )
        .unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_poolproof"))
            .args([
                "run",
                "--targets",
                file.to_str().unwrap(),
                "--filter",
                "set-isolated",
                "--format",
                "json",
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    let warning = "target 'bouncer' sets product but not config, so no documented limitation applies to it; \
                   add config = \"default\"";
    assert!(stderr("").contains(warning), "{}", stderr(""));
    assert!(!stderr("config = 'default'").contains("sets product but not config"));
    std::fs::remove_dir_all(&dir).unwrap();
}
