//! Embeds scenarios/*.toml so an installed binary has them without the repo.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=scenarios");
    let dir = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("scenarios");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("reading scenarios/")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();

    let mut out = String::from("&[\n");
    for path in files {
        let name = path.file_name().unwrap().to_str().unwrap();
        out.push_str(&format!("    ({name:?}, include_str!({:?})),\n", path.to_str().unwrap()));
    }
    out.push_str("]\n");
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("bundled.rs"), out).unwrap();
}
