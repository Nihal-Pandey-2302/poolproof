//! What a new user sees on their first `poolproof run`, outside the repository.

use std::process::Command;

#[test]
fn no_targets_file_says_what_to_pass() {
    let dir = std::env::temp_dir().join(format!("poolproof-no-targets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stderr = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_poolproof"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    for command in ["run", "doctor", "drivers"] {
        let err = stderr(&[command]);
        assert!(
            err.contains("no targets.toml here: pass --targets FILE, or --pooler URL --direct URL"),
            "{command}: {err}"
        );
    }
    // A file the user named is reported as it is.
    let err = stderr(&["run", "--targets", "mine.toml"]);
    assert!(err.contains("reading mine.toml"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}
