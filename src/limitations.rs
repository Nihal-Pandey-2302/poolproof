//! Documented pooler limitations: failures the pooler's own docs say will happen, like
//! a SET leaking between clients under PgBouncer's transaction pooling. A result that
//! matches an entry exactly is still a FAIL, marked expected with the doc that says
//! so. Anything else stays a bug: another step or outcome, a hang, a result that
//! varies between runs, or a product, version, config or Postgres the entry doesn't
//! name.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::report::{Report, ResultRow, TargetInfo};
use crate::runner::{FailKind, Outcome, Verdict, differing, from_harness};
use crate::scenario::{Action, Compare, PoolMode, Scenario};
use crate::version::Version;

/// The limitations in `limitations/`, embedded at build time as (file name, contents).
const BUNDLED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/bundled_limitations.rs"));

/// What a row carries when a documented limitation explains its FAIL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Expected {
    /// The pooler's own documentation, pinned to a version or commit.
    pub doc: String,
    /// The pooler version the doc was read for, when the link isn't pinned to a commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_version: Option<String>,
    /// The doc's own words.
    pub quote: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A documented limitation that applied to a target but didn't match its result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Stale {
    pub target: String,
    pub scenario: String,
    pub doc: String,
    /// Why it didn't match, e.g. `passes now`. Plain text for people, not for parsing.
    pub reason: String,
}

/// A set of documented limitations, built in or from `--limitations`.
pub struct Limitations {
    entries: Vec<Limitation>,
    /// `built-in from poolproof X.Y.Z` or `custom`, never a path.
    pub source: String,
}

#[derive(Debug)]
struct Limitation {
    /// The file's name: matches a target's `product`.
    product: String,
    scenario: String,
    mode: PoolMode,
    config: Vec<String>,
    since: Version,
    fixed_in: Option<Version>,
    postgres_since: Option<Version>,
    postgres_fixed_in: Option<Version>,
    /// The scenario's steps as `describe()` gives them, and how each is compared, when
    /// the entry was checked.
    steps: Vec<String>,
    compare: Vec<Compare>,
    /// Every step (from 0) where the pooler differs, with what it returns there.
    differs: Vec<(usize, Outcome)>,
    expected: Expected,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(rename = "limitation", default)]
    entries: Vec<Entry>,
}

/// One `[[limitation]]` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    scenario: String,
    mode: PoolMode,
    /// Target configs it applies to. Without it, only `default`.
    config: Option<Vec<String>>,
    /// The lowest pooler version it was checked against.
    since: String,
    /// The first version that no longer has it.
    fixed_in: Option<String>,
    postgres_since: Option<String>,
    postgres_fixed_in: Option<String>,
    differs: Vec<Differs>,
    doc: String,
    doc_version: Option<String>,
    quote: String,
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Differs {
    /// 1-based, as in a FAIL's step number.
    step: usize,
    pooler: Pooler,
}

/// A pooler outcome as an entry writes it. Error messages vary, so only the code.
#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
enum Pooler {
    Ok { rows: Vec<Vec<String>>, tags: Vec<u64> },
    Error { code: String },
    Abandoned,
    Messages { messages: Vec<String> },
}

impl From<Pooler> for Outcome {
    fn from(p: Pooler) -> Self {
        match p {
            Pooler::Ok { rows, tags } => Outcome::Ok {
                rows: rows.into_iter().map(|r| r.into_iter().map(Some).collect()).collect(),
                tags,
            },
            Pooler::Error { code } => Outcome::Error {
                code,
                message: String::new(),
                connect: false,
            },
            Pooler::Abandoned => Outcome::Abandoned,
            Pooler::Messages { messages } => Outcome::Messages { messages },
        }
    }
}

/// The built-in set, or the one in `dir`. Built-in entries are checked against the
/// built-in scenarios; entries in `dir` against `scenarios`, the ones the run uses.
/// Either way an entry only applies to a scenario with the same steps as the one it
/// was checked against, so a custom scenario with a built-in name isn't judged by it.
pub fn load(dir: Option<&Path>, scenarios: &[Scenario]) -> Result<Limitations> {
    let bundled;
    let (files, source, scenarios) = match dir {
        None => {
            bundled = crate::scenario::bundled()?;
            (
                BUNDLED.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(),
                format!("built-in from poolproof {}", env!("CARGO_PKG_VERSION")),
                bundled.as_slice(),
            )
        }
        Some(dir) => (read_dir(dir)?, "custom".to_string(), scenarios),
    };
    let mut entries: Vec<Limitation> = Vec::new();
    for (name, text) in files {
        let product = name.strip_suffix(".toml").unwrap_or(&name);
        for entry in parse(&name, product, &text, scenarios)? {
            if entries.iter().any(|e| e.same_scope(&entry)) {
                bail!(
                    "{name}: two entries for scenario '{}' with the same scope and steps",
                    entry.scenario
                );
            }
            entries.push(entry);
        }
    }
    Ok(Limitations { entries, source })
}

fn read_dir(dir: &Path) -> Result<Vec<(String, String)>> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading limitations dir {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            Ok((name, text))
        })
        .collect()
}

fn parse(origin: &str, product: &str, text: &str, scenarios: &[Scenario]) -> Result<Vec<Limitation>> {
    let file: File = toml::from_str(text).with_context(|| format!("parsing {origin}"))?;
    file.entries
        .into_iter()
        .map(|e| {
            let scenario = e.scenario.clone();
            check(product, e, scenarios).with_context(|| format!("{origin}: scenario '{scenario}'"))
        })
        .collect()
}

/// A commit hash in a URL pins the doc; anything else needs `doc_version`.
fn pinned_to_commit(url: &str) -> bool {
    url.split(['/', '#', '?'])
        .any(|part| part.len() == 40 && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn version(field: &str, v: &str) -> Result<Version> {
    Version::parse(v).with_context(|| format!("{field} `{v}` is not a version like 1.2.3"))
}

fn check(product: &str, e: Entry, scenarios: &[Scenario]) -> Result<Limitation> {
    let Some(scenario) = scenarios.iter().find(|s| s.name == e.scenario) else {
        bail!("no such scenario");
    };
    if !scenario.modes.contains(&e.mode) {
        bail!("the scenario doesn't run in {} mode", e.mode);
    }
    let config = e.config.unwrap_or_else(|| vec!["default".to_string()]);
    if config.is_empty() || config.iter().any(|c| c.trim().is_empty()) {
        bail!("config must name at least one target config");
    }
    let since = version("since", &e.since)?;
    let fixed_in = e.fixed_in.as_deref().map(|v| version("fixed_in", v)).transpose()?;
    let postgres_since = e
        .postgres_since
        .as_deref()
        .map(|v| version("postgres_since", v))
        .transpose()?;
    let postgres_fixed_in = e
        .postgres_fixed_in
        .as_deref()
        .map(|v| version("postgres_fixed_in", v))
        .transpose()?;
    if fixed_in.as_ref().is_some_and(|f| *f <= since) {
        bail!("fixed_in must be after since");
    }
    if let (Some(s), Some(f)) = (&postgres_since, &postgres_fixed_in)
        && f <= s
    {
        bail!("postgres_fixed_in must be after postgres_since");
    }
    if !e.doc.starts_with("https://") {
        bail!("doc must be an https link to the pooler's own documentation");
    }
    if e.doc_version.as_deref().is_none_or(|v| v.trim().is_empty()) && !pinned_to_commit(&e.doc) {
        bail!("doc isn't pinned to a commit, so doc_version must say which version it was read for");
    }
    if e.quote.trim().is_empty() {
        bail!("quote must hold the doc's own words");
    }
    if e.differs.is_empty() {
        bail!("differs must list every step where the pooler differs");
    }
    let mut differs: Vec<(usize, Outcome)> = Vec::new();
    for d in e.differs {
        let n = d.step;
        if n == 0 || n > scenario.steps.len() {
            bail!("step {n} doesn't exist (the scenario has {})", scenario.steps.len());
        }
        if differs.last().is_some_and(|(prev, _)| *prev >= n - 1) {
            bail!("differs must list steps in order, each once (step {n})");
        }
        let step = &scenario.steps[n - 1];
        if step.compare_mode() == Compare::Ignore {
            bail!("step {n} is never compared (compare = \"ignore\", or a precondition), so it never differs");
        }
        let outcome = Outcome::from(d.pooler);
        // Hangs, timeouts and dropped connections are never a documented limitation.
        if from_harness(&outcome) {
            bail!("step {n}: {outcome} is the harness's own outcome, never a documented one");
        }
        let fits = match (step.action()?, &outcome) {
            (Action::Send(_), Outcome::Messages { .. }) => true,
            (Action::Send(_), _) | (_, Outcome::Messages { .. }) => false,
            (Action::Abandon { .. }, _) => true,
            (_, outcome) => !matches!(outcome, Outcome::Abandoned),
        };
        if !fits {
            bail!("step {n}: {outcome} can't be the outcome of `{}`", step.describe());
        }
        differs.push((n - 1, outcome));
    }
    Ok(Limitation {
        product: product.to_string(),
        steps: scenario.steps.iter().map(|st| st.describe()).collect(),
        compare: scenario.compare_modes(),
        scenario: e.scenario,
        mode: e.mode,
        config,
        since,
        fixed_in,
        postgres_since,
        postgres_fixed_in,
        differs,
        expected: Expected {
            doc: e.doc,
            doc_version: e.doc_version,
            quote: e.quote,
            note: e.note,
        },
    })
}

/// `v` parses and is in [since, until).
fn in_range(v: Option<&str>, since: Option<&Version>, until: Option<&Version>) -> bool {
    let Some(v) = v.and_then(Version::parse) else {
        return false;
    };
    since.is_none_or(|s| *s <= v) && until.is_none_or(|u| v < *u)
}

impl Limitation {
    fn same_scope(&self, other: &Limitation) -> bool {
        let mut a = self.config.clone();
        let mut b = other.config.clone();
        a.sort();
        b.sort();
        (
            &self.product,
            &self.scenario,
            self.mode,
            a,
            &self.since,
            &self.fixed_in,
            &self.differs,
        ) == (
            &other.product,
            &other.scenario,
            other.mode,
            b,
            &other.since,
            &other.fixed_in,
            &other.differs,
        ) && (&self.postgres_since, &self.postgres_fixed_in) == (&other.postgres_since, &other.postgres_fixed_in)
    }

    /// Whether the entry is about this scenario, with the steps and compare modes it was
    /// checked against, on this target.
    fn about(&self, scenario: &str, steps: &[String], compare: &[Compare], t: &TargetInfo) -> bool {
        self.scenario == scenario && self.steps == steps && self.compare == compare && self.applies(t)
    }

    /// Whether the entry is about this target: product, mode, config, pooler version
    /// and, if it names one, Postgres version. Unknown values match nothing.
    fn applies(&self, t: &TargetInfo) -> bool {
        let pg_bounded = self.postgres_since.is_some() || self.postgres_fixed_in.is_some();
        t.product.as_deref() == Some(self.product.as_str())
            && t.mode == self.mode
            && t.config.as_ref().is_some_and(|c| self.config.contains(c))
            && in_range(t.version.as_deref(), Some(&self.since), self.fixed_in.as_ref())
            && (!pg_bounded
                || in_range(
                    t.postgres.as_deref(),
                    self.postgres_since.as_ref(),
                    self.postgres_fixed_in.as_ref(),
                ))
    }

    /// Why this row isn't exactly what the entry describes, or None if it is.
    fn mismatch(&self, compare: &[Compare], row: &ResultRow) -> Option<String> {
        match &row.verdict {
            Verdict::Pass => return Some("passes now".into()),
            Verdict::Fail { .. } => {}
            Verdict::Skip { .. } | Verdict::Error { .. } => return Some("no comparison".into()),
        }
        if row.failures != row.runs {
            return Some(format!("failed in {} of {} runs", row.failures, row.runs));
        }
        match row.varied {
            _ if row.runs == 1 => {}
            Some(false) => {}
            Some(true) => return Some("its runs failed in different ways".into()),
            None => return Some("not recorded whether its runs failed the same way".into()),
        }
        if row.steps.len() != compare.len() {
            return Some("its steps don't match the scenario's".into());
        }
        let pairs = row.steps.iter().map(|s| (&s.postgres, &s.pooler));
        let diff = differing(compare, pairs);
        let hang = |i: usize| FailKind::of(&row.steps[i].postgres, &row.steps[i].pooler) == FailKind::Hang;
        let same = diff.len() == self.differs.len()
            && diff
                .iter()
                .zip(&self.differs)
                .all(|(&i, (step, outcome))| i == *step && outcome.matches(&row.steps[i].pooler, compare[i]));
        if same && !diff.iter().any(|&i| hang(i)) {
            return None;
        }
        let steps: Vec<String> = diff
            .iter()
            .map(|&i| {
                let what = if hang(i) { " (hang)" } else { "" };
                format!("step {} {}{what}", i + 1, row.steps[i].pooler)
            })
            .collect();
        Some(format!("fails differently: {}", steps.join("; ")))
    }
}

impl Limitations {
    /// Marks every row of `report` this set explains, clearing any mark it had, and
    /// lists the entries that applied to a target but didn't match. Each step's
    /// compare mode comes from the report, or for reports from before it was recorded,
    /// from `scenarios` when the steps are the same; otherwise no row of that scenario
    /// can be expected.
    pub fn apply(&self, report: &mut Report, scenarios: &[Scenario]) {
        let modes: HashMap<String, Vec<Compare>> = report
            .scenarios
            .iter()
            .filter_map(|info| {
                if !info.compare.is_empty() {
                    return Some((info.name.clone(), info.compare.clone()));
                }
                let s = scenarios.iter().find(|s| s.name == info.name)?;
                let same = s.steps.iter().map(|st| st.describe()).eq(info.steps.iter().cloned());
                same.then(|| (info.name.clone(), s.compare_modes()))
            })
            .collect();
        let mut stale = Vec::new();
        for row in &mut report.results {
            row.expected = None;
            let (Some(target), Some(compare), Some(info)) = (
                report.targets.iter().find(|t| t.name == row.target),
                modes.get(&row.scenario),
                report.scenarios.iter().find(|s| s.name == row.scenario),
            ) else {
                continue;
            };
            let mut missed = Vec::new();
            for entry in self
                .entries
                .iter()
                .filter(|e| e.about(&row.scenario, &info.steps, compare, target))
            {
                match entry.mismatch(compare, row) {
                    None if row.expected.is_none() => row.expected = Some(entry.expected.clone()),
                    None => {}
                    Some(reason) => missed.push((entry, reason)),
                }
            }
            // An entry is stale only if no other one explains the row, and only for a
            // row that was compared at all.
            if row.expected.is_some() || matches!(row.verdict, Verdict::Skip { .. } | Verdict::Error { .. }) {
                continue;
            }
            stale.extend(missed.into_iter().map(|(entry, reason)| Stale {
                target: row.target.clone(),
                scenario: row.scenario.clone(),
                doc: entry.expected.doc.clone(),
                reason,
            }));
        }
        report.limitations = Some(self.source.clone());
        report.stale_expectations = stale;
    }

    /// The mark for one row as it comes in, before the report exists.
    pub fn expected_for(&self, target: &TargetInfo, scenario: &Scenario, row: &ResultRow) -> Option<Expected> {
        let compare = scenario.compare_modes();
        let steps: Vec<String> = scenario.steps.iter().map(|s| s.describe()).collect();
        self.entries
            .iter()
            .filter(|e| e.about(&row.scenario, &steps, &compare, target))
            .find(|e| e.mismatch(&compare, row).is_none())
            .map(|e| e.expected.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::StepOutcomes;
    use crate::runner::Mismatch;

    fn scenarios() -> Vec<Scenario> {
        crate::scenario::bundled().unwrap()
    }

    fn target(product: &str, version: &str, config: &str, postgres: &str) -> TargetInfo {
        TargetInfo {
            name: format!("{product}-{version}-{config}"),
            product: Some(product.into()),
            version: Some(version.into()),
            config: Some(config.into()),
            mode: PoolMode::Transaction,
            postgres: Some(postgres.into()),
            reset: None,
            pool_size: None,
            idle_probe: None,
            busy_probe: None,
        }
    }

    fn err(code: &str, message: &str) -> Outcome {
        Outcome::Error {
            code: code.into(),
            message: message.into(),
            connect: false,
        }
    }

    fn ok(rows: &[&str]) -> Outcome {
        Outcome::Ok {
            rows: rows.iter().map(|r| vec![Some(r.to_string())]).collect(),
            tags: vec![0],
        }
    }

    /// The bundled set-isolated-between-clients, whose SHOW leaks on the pooler.
    fn set_isolated() -> Scenario {
        scenarios()
            .into_iter()
            .find(|s| s.name == "set-isolated-between-clients")
            .unwrap()
    }

    /// The 0-based index of set-isolated's SHOW.
    fn show() -> usize {
        set_isolated()
            .steps
            .iter()
            .position(|st| st.simple.as_deref() == Some("SHOW work_mem"))
            .unwrap()
    }

    /// A row for set-isolated-between-clients with `pooler_show` as the SHOW's outcome on
    /// the pooler; evidence steps never run on Postgres, every other step matches.
    fn row(target: &TargetInfo, pooler_show: Outcome) -> ResultRow {
        let empty = Outcome::Ok {
            rows: vec![],
            tags: vec![],
        };
        let show = show();
        let steps = set_isolated()
            .steps
            .iter()
            .enumerate()
            .map(|(i, st)| match (i == show, st.evidence) {
                (true, _) => StepOutcomes {
                    postgres: ok(&["7MB"]),
                    pooler: pooler_show.clone(),
                },
                (false, true) => StepOutcomes {
                    postgres: Outcome::NotRun,
                    pooler: ok(&["1"]),
                },
                (false, false) => StepOutcomes {
                    postgres: empty.clone(),
                    pooler: empty.clone(),
                },
            })
            .collect();
        let verdict = if pooler_show == ok(&["7MB"]) {
            Verdict::Pass
        } else {
            Verdict::Fail {
                mismatch: Mismatch {
                    step: show + 1,
                    action: String::new(),
                    kind: FailKind::of(&ok(&["7MB"]), &pooler_show),
                    expected: ok(&["7MB"]),
                    actual: pooler_show,
                },
            }
        };
        ResultRow {
            target: target.name.clone(),
            scenario: "set-isolated-between-clients".into(),
            verdict,
            steps,
            runs: 1,
            failures: 1,
            varied: None,
            precondition_unmet: None,
            reuse_unmet: None,
            expected: None,
        }
    }

    /// An entry for set-isolated's SHOW, at whatever step the scenario has it.
    fn set_leak() -> String {
        format!(
            r#"
[[limitation]]
scenario = "set-isolated-between-clients"
mode = "transaction"
since = "1.24.1"
fixed_in = "1.30"
differs = [{{ step = {}, pooler = {{ status = "ok", rows = [["9MB"]], tags = [0] }} }}]
doc = "https://github.com/pgbouncer/pgbouncer.github.io/blob/ccb120f859a47fe3a6f3b2120d16194fd11f562f/features.md"
quote = "SET/RESET | Yes | Never"
"#,
            show() + 1
        )
    }

    fn set(text: &str) -> Limitations {
        let entries = parse("pgbouncer.toml", "pgbouncer", text, &scenarios()).unwrap();
        Limitations {
            entries,
            source: "custom".into(),
        }
    }

    fn report(targets: Vec<TargetInfo>, rows: Vec<ResultRow>) -> Report {
        let s = scenarios();
        let s = s.iter().find(|s| s.name == "set-isolated-between-clients").unwrap();
        Report {
            schema: None,
            schema_version: 1,
            poolproof: "0".into(),
            generated_at: None,
            targets,
            scenarios: vec![crate::report::ScenarioInfo {
                name: s.name.clone(),
                description: String::new(),
                refs: vec![],
                steps: s.steps.iter().map(|st| st.describe()).collect(),
                repeat: 1,
                compare: vec![],
            }],
            results: rows,
            comparison: None,
            limitations: None,
            stale_expectations: vec![],
            label: None,
        }
    }

    fn expected(set: &Limitations, t: &TargetInfo, r: &ResultRow) -> bool {
        let s = scenarios();
        let s = s.iter().find(|s| s.name == r.scenario).unwrap();
        set.expected_for(t, s, r).is_some()
    }

    #[test]
    fn only_the_exact_outcome_on_the_named_target_is_expected() {
        let set = set(&set_leak());
        let t = target("pgbouncer", "1.26.1", "default", "17.11");
        assert!(expected(&set, &t, &row(&t, ok(&["9MB"]))));
        // Another value, an error, or a hang at the same step is a bug.
        assert!(!expected(&set, &t, &row(&t, ok(&["8MB"]))));
        assert!(!expected(&set, &t, &row(&t, err("42P05", "x"))));
        assert!(!expected(&set, &t, &row(&t, err("TIMEOUT", "x"))));
        // Other configs, products, versions, or nothing known about them.
        for other in [
            target("pgbouncer", "1.26.1", "strict", "17.11"),
            target("pgdog", "1.26.1", "default", "17.11"),
            target("pgbouncer", "1.24.0", "default", "17.11"),
            target("pgbouncer", "1.30.0", "default", "17.11"),
            target("pgbouncer", "git-1a2b3c4", "default", "17.11"),
        ] {
            assert!(!expected(&set, &other, &row(&other, ok(&["9MB"]))), "{}", other.name);
        }
        // A pre-release comes before its release, so fixed_in's own rc is still in range.
        let rc = target("pgbouncer", "1.30.0-rc.1", "default", "17.11");
        assert!(expected(&set, &rc, &row(&rc, ok(&["9MB"]))));
        let mut unknown = t.clone();
        unknown.config = None;
        assert!(!expected(&set, &unknown, &row(&unknown, ok(&["9MB"]))));
    }

    #[test]
    fn another_differing_step_makes_it_a_bug() {
        let set = set(&set_leak());
        let t = target("pgbouncer", "1.26.1", "default", "17.11");
        // Step 1 differs too, even though the FAIL's first mismatch would be step 1.
        let mut r = row(&t, ok(&["9MB"]));
        r.steps[0].pooler = err("42P05", "x");
        assert!(!expected(&set, &t, &r));
    }

    #[test]
    fn a_direct_step_that_also_differs_makes_it_a_bug() {
        // advisory-lock-released-on-disconnect: step 4 is a direct check.
        let s = scenarios();
        let s = s
            .iter()
            .find(|s| s.name == "advisory-lock-released-on-disconnect")
            .unwrap();
        assert!(s.steps[3].direct);
        let n = s.steps.len();
        let direct_step = s.steps.iter().position(|st| st.direct).unwrap();
        let pooled_step = s.steps.iter().position(|st| !st.direct).unwrap();
        let text = format!(
            "[[limitation]]\nscenario = '{}'\nmode = 'transaction'\nsince = '1.0'\ndoc_version = '1.26.1'\n\
             doc = 'https://example.org/'\nquote = 'q'\n\
             differs = [{{ step = {}, pooler = {{ status = 'ok', rows = [['x']], tags = [1] }} }}]\n",
            s.name,
            pooled_step + 1
        );
        let set = set(&text);
        let t = target("pgbouncer", "1.26.1", "default", "17.11");
        let same = Outcome::Ok {
            rows: vec![],
            tags: vec![],
        };
        let mut steps: Vec<StepOutcomes> = (0..n)
            .map(|_| StepOutcomes {
                postgres: same.clone(),
                pooler: same.clone(),
            })
            .collect();
        steps[pooled_step].pooler = Outcome::Ok {
            rows: vec![vec![Some("x".into())]],
            tags: vec![1],
        };
        let mut r = ResultRow {
            target: t.name.clone(),
            scenario: s.name.clone(),
            verdict: Verdict::Fail {
                mismatch: Mismatch {
                    step: pooled_step + 1,
                    action: String::new(),
                    kind: FailKind::Silent,
                    expected: same.clone(),
                    actual: same.clone(),
                },
            },
            steps,
            runs: 1,
            failures: 1,
            varied: None,
            precondition_unmet: None,
            reuse_unmet: None,
            expected: None,
        };
        let compare = s.compare_modes();
        assert!(set.entries[0].mismatch(&compare, &r).is_none());
        r.steps[direct_step].pooler = ok(&["1"]);
        let reason = set.entries[0].mismatch(&compare, &r).unwrap();
        assert!(reason.starts_with("fails differently"), "{reason}");
    }

    #[test]
    fn repeated_runs_must_all_fail_the_same_known_way() {
        let set = set(&set_leak());
        let t = target("pgbouncer", "1.26.1", "default", "17.11");
        let mut r = row(&t, ok(&["9MB"]));
        r.runs = 5;
        r.failures = 5;
        r.varied = Some(false);
        assert!(expected(&set, &t, &r));
        r.varied = None;
        assert!(!expected(&set, &t, &r));
        r.varied = Some(true);
        assert!(!expected(&set, &t, &r));
        r.varied = Some(false);
        r.failures = 4;
        assert!(!expected(&set, &t, &r));
    }

    #[test]
    fn postgres_bounds_need_a_known_version() {
        let set = set(&set_leak().replace("fixed_in = \"1.30\"", "postgres_since = \"18\""));
        let t18 = target("pgbouncer", "1.26.1", "default", "18.4");
        let t17 = target("pgbouncer", "1.26.1", "default", "17.11");
        assert!(expected(&set, &t18, &row(&t18, ok(&["9MB"]))));
        assert!(!expected(&set, &t17, &row(&t17, ok(&["9MB"]))));
        let mut unknown = t18.clone();
        unknown.postgres = None;
        assert!(!expected(&set, &unknown, &row(&unknown, ok(&["9MB"]))));
    }

    #[test]
    fn apply_marks_rows_and_reports_stale_entries() {
        let set = set(&set_leak());
        let leaks = target("pgbouncer", "1.26.1", "default", "17.11");
        let passes = target("pgbouncer", "1.25.2", "default", "17.11");
        let differs = target("pgbouncer", "1.24.1", "default", "17.11");
        let other = target("pgdog", "0.1.62", "default", "17.11");
        let mut stored = row(&other, ok(&["9MB"]));
        stored.expected = Some(set.entries[0].expected.clone());
        let mut report = report(
            vec![leaks.clone(), passes.clone(), differs.clone(), other.clone()],
            vec![
                row(&leaks, ok(&["9MB"])),
                row(&passes, ok(&["7MB"])),
                row(&differs, ok(&["8MB"])),
                stored,
            ],
        );
        set.apply(&mut report, &scenarios());
        let marks: Vec<bool> = report.results.iter().map(|r| r.expected.is_some()).collect();
        // A stored mark the set doesn't give is cleared.
        assert_eq!(marks, [true, false, false, false]);
        let stale: Vec<(&str, &str)> = report
            .stale_expectations
            .iter()
            .map(|s| (s.target.as_str(), s.reason.as_str()))
            .collect();
        let differently = format!("fails differently: step {} ok rows=[(8MB)] tags=[0]", show() + 1);
        assert_eq!(
            stale,
            [
                (passes.name.as_str(), "passes now"),
                (differs.name.as_str(), differently.as_str()),
            ]
        );
        assert_eq!(report.limitations.as_deref(), Some("custom"));
    }

    #[test]
    fn apply_needs_compare_modes_from_the_report_or_the_same_steps() {
        let set = set(&set_leak());
        let t = target("pgbouncer", "1.26.1", "default", "17.11");
        let mut old = report(vec![t.clone()], vec![row(&t, ok(&["9MB"]))]);
        // No modes recorded and no scenario to take them from.
        set.apply(&mut old, &[]);
        assert!(old.results[0].expected.is_none());
        set.apply(&mut old, &scenarios());
        assert!(old.results[0].expected.is_some());
        // Recorded modes don't need the scenario at all.
        old.scenarios[0].compare = set_isolated().compare_modes();
        set.apply(&mut old, &[]);
        assert!(old.results[0].expected.is_some());
    }

    #[test]
    fn a_scenario_with_other_steps_is_not_judged_by_the_entry() {
        let set = set(&set_leak());
        let t = target("pgbouncer", "1.26.1", "default", "17.11");
        let mut changed = report(vec![t.clone()], vec![row(&t, ok(&["9MB"]))]);
        changed.scenarios[0].steps[0] = "[a] simple: SET work_mem = '1MB'".into();
        changed.scenarios[0].compare = set_isolated().compare_modes();
        set.apply(&mut changed, &[]);
        // Neither expected nor stale: the entry is about another scenario.
        assert!(changed.results[0].expected.is_none());
        assert!(changed.stale_expectations.is_empty());
        // The same as the run sees it.
        let custom: Scenario = toml::from_str(
            "name = 'set-isolated-between-clients'\ndescription = ''\n\
             [[step]]\nclient = 'a'\nsimple = 'SET work_mem = \"1MB\"'\n\
             [[step]]\nclient = 'b'\nsimple = 'SET work_mem = \"9MB\"'\n\
             [[step]]\nclient = 'a'\nsimple = 'SHOW work_mem'\n",
        )
        .unwrap();
        assert!(set.expected_for(&t, &custom, &row(&t, ok(&["9MB"]))).is_none());
        // The same steps compared another way are another scenario too.
        let mut loose = report(vec![t.clone()], vec![row(&t, ok(&["9MB"]))]);
        loose.scenarios[0].compare = set_isolated().compare_modes();
        loose.scenarios[0].compare[show()] = Compare::Status;
        set.apply(&mut loose, &[]);
        assert!(loose.results[0].expected.is_none());
        assert!(loose.stale_expectations.is_empty());
        let mut status = set_isolated();
        assert!(set.expected_for(&t, &status, &row(&t, ok(&["9MB"]))).is_some());
        status.steps[show()].compare = Compare::Status;
        assert!(set.expected_for(&t, &status, &row(&t, ok(&["9MB"]))).is_none());
    }

    #[test]
    fn built_in_entries_load_whatever_scenarios_the_run_uses() {
        let only: Vec<Scenario> = scenarios()
            .into_iter()
            .filter(|s| s.name == "multi-statement-begin")
            .collect();
        let set = load(None, &only).unwrap();
        assert!(!set.entries.is_empty());
        // A custom set is checked against the run's scenarios.
        let dir = std::env::temp_dir().join(format!("pp-limitations-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pgbouncer.toml"), set_leak()).unwrap();
        let err = load(Some(&dir), &only).err().map(|e| format!("{e:#}"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(err.unwrap().contains("no such scenario"));
    }

    #[test]
    fn an_entry_explained_by_another_is_not_stale() {
        let leak = set_leak();
        let other = leak
            .replace("[[\"9MB\"]]", "[[\"8MB\"]]")
            .replace("fixed_in = \"1.30\"", "fixed_in = \"1.31\"");
        let set = set(&format!("{leak}\n{other}"));
        let t = target("pgbouncer", "1.26.1", "default", "17.11");
        let mut r = report(vec![t.clone()], vec![row(&t, ok(&["9MB"]))]);
        set.apply(&mut r, &scenarios());
        assert!(r.results[0].expected.is_some());
        assert!(r.stale_expectations.is_empty());
        let mut r = report(vec![t.clone()], vec![row(&t, ok(&["7.5MB"]))]);
        set.apply(&mut r, &scenarios());
        assert_eq!(r.stale_expectations.len(), 2);
    }

    #[test]
    fn entries_are_checked_when_loaded() {
        let bad = |from: &str, to: &str, wanted: &str| {
            let leak = set_leak();
            let text = leak.replace(from, to);
            assert_ne!(text, leak, "{from}");
            let err = match parse("pgbouncer.toml", "pgbouncer", &text, &scenarios()) {
                Ok(_) => panic!("accepted: {to}"),
                Err(e) => format!("{e:#}"),
            };
            assert!(err.contains(wanted), "{to}: {err}");
        };
        bad("set-isolated-between-clients\"", "nope\"", "no such scenario");
        let s = set_isolated();
        let at = format!("step = {}", show() + 1);
        let past = s.steps.len() + 1;
        let evidence = s.steps.iter().rposition(|st| st.evidence).unwrap() + 1;
        bad(&at, &format!("step = {past}"), &format!("step {past} doesn't exist"));
        bad(
            &at,
            &format!("step = {evidence}"),
            &format!("step {evidence} is never compared"),
        );
        bad(&at, "step = 0", "step 0 doesn't exist");
        bad(
            "status = \"ok\", rows = [[\"9MB\"]], tags = [0]",
            "status = \"error\", code = \"TIMEOUT\"",
            "harness's own outcome",
        );
        bad(
            "status = \"ok\", rows = [[\"9MB\"]], tags = [0]",
            "status = \"messages\", messages = [\"x\"]",
            "can't be the outcome",
        );
        bad(
            "status = \"ok\", rows = [[\"9MB\"]], tags = [0]",
            "status = \"abandoned\"",
            "can't be the outcome",
        );
        bad(
            "status = \"ok\", rows = [[\"9MB\"]], tags = [0]",
            "status = \"ok\", rows = [[\"9MB\"]]",
            "missing field `tags`",
        );
        bad(
            "status = \"ok\", rows = [[\"9MB\"]], tags = [0]",
            "status = \"error\", code = \"42P05\", message = \"x\"",
            "unknown field `message`",
        );
        bad("since = \"1.24.1\"", "since = \"dev\"", "is not a version");
        bad(
            "fixed_in = \"1.30\"",
            "fixed_in = \"1.24.1\"",
            "fixed_in must be after since",
        );
        bad("https://github.com", "http://github.com", "https link");
        bad(
            "ccb120f859a47fe3a6f3b2120d16194fd11f562f",
            "master",
            "doc_version must say",
        );
        bad(
            "quote = \"SET/RESET | Yes | Never\"",
            "quote = \" \"",
            "quote must hold",
        );
        bad(
            "mode = \"transaction\"",
            "mode = \"transaction\"\nconfig = []",
            "config must name",
        );
        bad(
            "mode = \"transaction\"",
            "mode = \"transaction\"\nkind = \"silent\"",
            "unknown field `kind`",
        );
        let leak = set_leak();
        let twice = format!(
            "{leak}\n[[limitation]]\nscenario = \"set-isolated-between-clients\"\nmode = \"transaction\"\nsince = \"1.24.1\"\ndiffers = [{{ step = 2, pooler = {{ status = \"error\", code = \"42P05\" }} }}, {{ step = 2, pooler = {{ status = \"error\", code = \"42P05\" }} }}]\ndoc = \"https://x.org/\"\ndoc_version = \"1\"\nquote = \"q\"\n"
        );
        let err = format!("{:#}", parse("p.toml", "p", &twice, &scenarios()).err().unwrap());
        assert!(err.contains("in order, each once"), "{err}");
    }

    #[test]
    fn a_duplicate_entry_is_refused() {
        let dir = std::env::temp_dir().join(format!("pp-limitations-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // The same versions written two ways are the same scope.
        let leak = set_leak();
        let again = leak.replace("since = \"1.24.1\"", "since = \"1.24.1.0\"");
        std::fs::write(dir.join("pgbouncer.toml"), format!("{leak}\n{again}")).unwrap();
        let err = load(Some(&dir), &scenarios()).err().map(|e| format!("{e:#}"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(err.unwrap().contains("same scope and steps"));
    }

    /// Every built-in entry must explain a result in the committed results, so a typo
    /// or a fixed limitation can't sit in the files unnoticed.
    #[test]
    fn built_in_entries_match_committed_results() {
        let scenarios = scenarios();
        let set = load(None, &scenarios).unwrap();
        let mut reports = crate::report::bundled().unwrap();
        if reports.is_empty() {
            return;
        }
        for report in &mut reports {
            set.apply(report, &scenarios);
            let stale: Vec<String> = report
                .stale_expectations
                .iter()
                .map(|s| format!("{} / {}: {}", s.target, s.scenario, s.reason))
                .collect();
            assert!(stale.is_empty(), "stale expectations: {stale:#?}");
        }
        for (i, entry) in set.entries.iter().enumerate() {
            let used = reports.iter().any(|r| {
                r.results.iter().any(|row| {
                    row.scenario == entry.scenario
                        && row.expected.as_ref() == Some(&entry.expected)
                        && r.targets.iter().any(|t| t.name == row.target && entry.applies(t))
                })
            });
            assert!(
                used,
                "entry {i} ({} {}) matches no committed result",
                entry.product, entry.scenario
            );
        }
    }
}
