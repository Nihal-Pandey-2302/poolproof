//! Embeds scenarios/*.toml, limitations/*.toml and results/postgres-*.json so an installed
//! binary has them without the repo.

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
