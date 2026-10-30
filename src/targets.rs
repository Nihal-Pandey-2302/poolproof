//! The targets file: which poolers to test and the Postgres behind each.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::scenario::PoolMode;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Targets {
    #[serde(rename = "target")]
    pub targets: Vec<Target>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub name: String,
    /// Connection URL through the pooler.
    pub pooler: String,
    /// Connection URL for the Postgres server behind it.
    pub direct: String,
    pub mode: PoolMode,
    /// What is being tested, for grouping in reports: e.g. `pgbouncer`, `1.26.1`, and a
    /// config label like `default` or `strict`.
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub config: Option<String>,
    /// Statements run on `direct` before every run to drop state leaked on the
    /// pooler's server connections, usually by terminating them. Without it,
    /// one scenario's leak can change the next one's result.
    #[serde(default)]
    pub reset: Vec<String>,
}

impl Targets {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).context("parsing targets")
    }

    /// The one target given with `run --pooler URL --direct URL`.
    pub fn single(pooler: String, direct: String, mode: PoolMode) -> Self {
        Targets {
            targets: vec![Target {
                name: "pooler".into(),
                pooler,
                direct,
                mode,
                product: None,
                version: None,
                config: None,
                reset: vec![],
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_targets_parse_with_reset() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("targets.toml");
        let targets: Targets = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(targets.targets.iter().all(|t| !t.reset.is_empty()));
    }
}
