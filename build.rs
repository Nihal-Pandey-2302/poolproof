//! Embeds scenarios/*.toml, limitations/*.toml and results/postgres-*.json so an installed
//! binary has them without the repo, and the verdicts from results/drivers-postgres-*.json.

use std::path::Path;

fn main() {
    let root = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).to_path_buf();
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).to_path_buf();
    embed(&root.join("scenarios"), "", "toml", &out.join("bundled.rs"));
    // Only the core suite's reports: `scan --pooler` looks targets up by name and
    // Postgres version, and a drivers report uses the same target names.
    embed(
        &root.join("results"),
        "postgres-",
        "json",
        &out.join("bundled_results.rs"),
    );
    embed(
        &root.join("limitations"),
        "",
        "toml",
        &out.join("bundled_limitations.rs"),
    );
    driver_verdicts(&root.join("results"), &out.join("bundled_driver_verdicts.rs"));
}

/// Writes `&[(target, postgres, scenario, result, kind), ...]` from the drivers reports:
/// all `scan --pooler` needs of them, a few KB instead of every step's outcomes. Fails
/// the build when the core reports are there but these aren't, so a build from a copy
/// that left them out can't quietly know nothing about drivers.
fn driver_verdicts(dir: &Path, out: &Path) {
    let files = |prefix: &str| -> Vec<std::path::PathBuf> {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map(|entries| entries.map(|e| e.unwrap().path()).collect())
            .unwrap_or_default();
        files.retain(|p: &std::path::PathBuf| {
            p.extension().is_some_and(|e| e == "json")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(prefix))
        });
        files.sort();
        files
    };
    let drivers = files("drivers-postgres-");
    if drivers.is_empty() && !files("postgres-").is_empty() {
        panic!(
            "{} has core results but no drivers-postgres-*.json; scan --pooler needs both",
            dir.display()
        );
    }
    let text = |v: &serde_json::Value, key: &str| v.get(key).and_then(|s| s.as_str()).unwrap_or("").to_string();
    let mut code = String::from("&[\n");
    for path in drivers {
        println!("cargo:rerun-if-changed={}", path.display());
        let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let targets = report["targets"].as_array().cloned().unwrap_or_default();
        for row in report["results"].as_array().cloned().unwrap_or_default() {
            let target = text(&row, "target");
            let postgres = targets
                .iter()
                .find(|t| text(t, "name") == target)
                .map(|t| text(t, "postgres"))
                .unwrap_or_default();
            let kind = row.get("mismatch").map(|m| text(m, "kind")).unwrap_or_default();
            code.push_str(&format!(
                "    ({target:?}, {postgres:?}, {:?}, {:?}, {kind:?}),\n",
                text(&row, "scenario"),
                text(&row, "result"),
            ));
        }
    }
    code.push_str("]\n");
    std::fs::write(out, code).unwrap();
}

/// Writes `&[(file name, include_str!(path)), ...]` for the files in `dir` whose names
/// start with `prefix`. A missing directory gives an empty list.
fn embed(dir: &Path, prefix: &str, extension: &str, out: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|entries| entries.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default();
    files.retain(|p: &std::path::PathBuf| {
        p.extension().is_some_and(|e| e == extension)
            && p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix))
    });
    files.sort();

    let mut code = String::from("&[\n");
    for path in files {
        let name = path.file_name().unwrap().to_str().unwrap();
        code.push_str(&format!(
            "    ({name:?}, include_str!({:?})),\n",
            path.to_str().unwrap()
        ));
    }
    code.push_str("]\n");
    std::fs::write(out, code).unwrap();
}
