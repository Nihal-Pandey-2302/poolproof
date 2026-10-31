//! poolproof as a library, so integration tests in `tests/` can use its modules.
//! The command-line tool is the product: nothing here is a stable API, so the modules
//! are hidden from the generated docs.

#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod doctor;
#[doc(hidden)]
pub mod drivers;
#[doc(hidden)]
pub mod limitations;
#[doc(hidden)]
pub mod output;
#[doc(hidden)]
pub mod report;
#[doc(hidden)]
pub mod runner;
#[doc(hidden)]
pub mod scan;
#[doc(hidden)]
pub mod scenario;
#[doc(hidden)]
pub mod targets;
#[doc(hidden)]
pub mod version;

/// A file of the repository's own test bed, which the published crate leaves out. None,
/// with a note, when running from the packaged crate (no `.git`), so its tests skip
/// there; in a checkout a missing file still fails them.
#[cfg(test)]
pub(crate) fn repo_file(path: &str) -> Option<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let file = root.join(path);
    if !file.exists() && !root.join(".git").exists() {
        eprintln!("skipped: {path} isn't in the packaged crate");
        return None;
    }
    Some(file)
}
