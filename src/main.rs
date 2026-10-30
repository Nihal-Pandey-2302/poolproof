mod raw;
mod report;
mod runner;
mod scan;
mod scenario;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Deserialize;

use runner::Verdict;
use scenario::PoolMode;

#[derive(Parser)]
#[command(version, about = "Find out what breaks when you put a connection pooler in front of Postgres")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run scenarios through each pooler and compare against plain Postgres.
    Run {
        /// Targets file.
        #[arg(short, long, default_value = "targets.toml")]
        targets: PathBuf,
        /// Test one pooler without a targets file: its connection URL.
        #[arg(long, requires = "direct")]
        pooler: Option<String>,
        /// With --pooler: the URL of the Postgres server behind it.
        #[arg(long, requires = "pooler")]
        direct: Option<String>,
        /// With --pooler: the pooler's mode.
        #[arg(long, value_enum, default_value_t = PoolMode::Transaction)]
        mode: PoolMode,
        /// Scenario directory. Defaults to the scenarios built into poolproof.
        #[arg(short, long)]
        scenarios: Option<PathBuf>,
        /// Only run scenarios whose name contains this.
        #[arg(short, long)]
        filter: Option<String>,
        /// Run scenarios in reverse order. Verdicts should not change; if they do,
        /// state is leaking between scenarios.
        #[arg(long)]
        reverse: bool,
        /// Print every step's outcome on Postgres and through the pooler, not just
        /// the first difference. Use it to check a new scenario does what you meant.
        #[arg(short, long)]
        verbose: bool,
        #[arg(long, value_enum, default_value_t = RunFormat::Text)]
        format: RunFormat,
    },
    /// Scan queries (pg_stat_statements CSV or a .sql file) for patterns that
    /// break under transaction pooling.
    Scan {
        /// pg_stat_statements CSV, Postgres log (.log stderr, .json jsonlog) or .sql file.
        input: PathBuf,
        #[arg(long, value_enum, default_value_t = ScanFormat::Text)]
        format: ScanFormat,
        /// Exit non-zero on warnings too, not just on queries that break.
        #[arg(long)]
        strict: bool,
        /// Check each finding against what this pooler actually does, using poolproof's
        /// own results (e.g. `pgbouncer-1.26.1`), or a target in --results.
        #[arg(long)]
        pooler: Option<String>,
        /// With --pooler: the Postgres version to use results for. Defaults to the newest.
        #[arg(long, requires = "pooler")]
        postgres: Option<String>,
        /// With --pooler: a saved `run --format json` report to use instead of the
        /// built-in results, e.g. from a run against your own setup.
        #[arg(long, requires = "pooler")]
        results: Option<PathBuf>,
    },
    /// Render saved `run --format json` reports, e.g. one per Postgres version, as
    /// one table.
    Report {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[arg(long, value_enum, default_value_t = ReportFormat::Markdown)]
        format: ReportFormat,
    },
    /// List bundled scenarios.
    List {
        /// Scenario directory. Defaults to the scenarios built into poolproof.
        #[arg(short, long)]
        scenarios: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ScanFormat {
    Text,
    Json,
    /// SARIF 2.1.0, for GitHub code scanning and other SARIF viewers.
    Sarif,
}

#[derive(Clone, Copy, ValueEnum)]
enum ReportFormat {
    Markdown,
    /// One self-contained page with a filterable grid and step-by-step details.
    Html,
}

#[derive(Clone, Copy, ValueEnum)]
enum RunFormat {
    Text,
    Json,
    /// A scenario x target table plus failure details, for a README or CI summary.
    Markdown,
    /// One self-contained page with a filterable grid and step-by-step details.
    Html,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Targets {
    #[serde(rename = "target")]
    targets: Vec<Target>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    name: String,
    /// Connection URL through the pooler.
    pooler: String,
    /// Connection URL for the Postgres server behind it.
    direct: String,
    mode: PoolMode,
    /// What is being tested, for grouping in reports: e.g. `pgbouncer`, `1.26.1`, and a
    /// config label like `default` or `strict`.
    #[serde(default)]
    product: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    config: Option<String>,
    /// Statements run on `direct` before every run to drop state leaked on the
    /// pooler's server connections, usually by terminating them. Without it,
    /// one scenario's leak can change the next one's result.
    #[serde(default)]
    reset: Vec<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Run { targets, pooler, direct, mode, scenarios, filter, reverse, verbose, format } => {
            let targets = match (pooler, direct) {
                (Some(pooler), Some(direct)) => {
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
                _ => {
                    let text = std::fs::read_to_string(&targets)
                        .with_context(|| format!("reading {}", targets.display()))?;
                    toml::from_str(&text).context("parsing targets")?
                }
            };
            let mut scenarios = load_scenarios(scenarios.as_deref())?;
            if reverse {
                scenarios.reverse();
            }

            let mut results = Vec::new();
            let mut target_infos = Vec::new();
            for target in &targets.targets {
                target_infos.push(report::TargetInfo {
                    name: target.name.clone(),
                    product: target.product.clone(),
                    version: target.version.clone(),
                    config: target.config.clone(),
                    mode: target.mode,
                    postgres: runner::server_version(&target.direct).await.ok(),
                });
                if target.reset.is_empty() {
                    eprintln!(
                        "warning: target '{}' has no reset; leaked state can carry over between scenarios",
                        target.name
                    );
                }
                for s in &scenarios {
                    if filter.as_ref().is_some_and(|f| !s.name.contains(f.as_str())) {
                        continue;
                    }
                    let (verdict, steps, runs, failures) = if s.modes.contains(&target.mode) {
                        let checked = runner::check(s, &target.pooler, &target.direct, &target.reset)
                            .await
                            .with_context(|| format!("{} / {}", target.name, s.name))?;
                        (checked.verdict, checked.steps, checked.runs, checked.failures)
                    } else {
                        let reason = format!("not applicable to {} mode", target.mode);
                        (Verdict::Skip { reason }, vec![], 0, 0)
                    };
                    if matches!(format, RunFormat::Text) {
                        print_verdict(&target.name, s, &verdict);
                        if runs > 1 && failures > 0 {
                            println!("      failed in {failures} of {runs} runs (timing-sensitive)");
                        }
                        if verbose {
                            for (i, (step, (expected, actual))) in s.steps.iter().zip(&steps).enumerate() {
                                println!("      {}. {}", i + 1, step.describe());
                                println!("         postgres: {expected}");
                                println!("         pooler:   {actual}");
                            }
                        }
                    }
                    let steps =
                        steps.into_iter().map(|(postgres, pooler)| report::StepOutcomes { postgres, pooler }).collect();
                    results.push(report::ResultRow {
                        target: target.name.clone(),
                        scenario: s.name.clone(),
                        verdict,
                        steps,
                        runs,
                        failures,
                    });
                }
            }

            let count = |f: fn(&Verdict) -> bool| results.iter().filter(|r| f(&r.verdict)).count();
            let failed = count(|v| matches!(v, Verdict::Fail { .. }));
            let errors = count(|v| matches!(v, Verdict::Error { .. }));
            if matches!(format, RunFormat::Text) {
                let passed = count(|v| matches!(v, Verdict::Pass));
                let skipped = count(|v| matches!(v, Verdict::Skip { .. }));
                println!("\n{passed} passed, {failed} failed, {errors} errors, {skipped} skipped");
            } else {
                let scenarios = scenarios
                    .iter()
                    .filter(|s| results.iter().any(|r| r.scenario == s.name))
                    .map(|s| report::ScenarioInfo {
                        name: s.name.clone(),
                        description: s.description.clone(),
                        refs: s.refs.clone(),
                        steps: s.steps.iter().map(|step| step.describe()).collect(),
                        repeat: s.repeat,
                    })
                    .collect();
                let report = report::Report {
                    poolproof: env!("CARGO_PKG_VERSION").to_string(),
                    targets: target_infos,
                    scenarios,
                    results,
                };
                match format {
                    RunFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
                    RunFormat::Markdown => print!("{}", report::markdown(&[report])),
                    RunFormat::Html => print!("{}", report::html(&[report])?),
                    RunFormat::Text => unreachable!(),
                }
            }
            Ok(if failed + errors > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS })
        }
        Command::Scan { input, format, strict, pooler, postgres, results } => {
            let queries = scan::load_queries(&input)?;
            let mut report = scan::scan(&queries);
            if let Some(pooler) = &pooler {
                let reports = match &results {
                    Some(path) => vec![report::Report::load(path)?],
                    None => report::bundled()?,
                };
                let (results, target) = report::find_target(&reports, pooler, postgres.as_deref())?;
                for finding in &mut report.findings {
                    finding.on_pooler = report::check_finding(results, target, &finding.scenarios);
                }
                // Proven problems first. Findings without results keep their place after them.
                let rank = |f: &scan::Finding| match f.on_pooler.as_ref().map(|c| c.status) {
                    Some(scan::PoolerStatus::Breaks) => 0,
                    Some(scan::PoolerStatus::Rejected) => 1,
                    None => 2,
                    Some(scan::PoolerStatus::Ok) => 3,
                };
                report.findings.sort_by_key(rank);
            }
            match format {
                ScanFormat::Json => println!("{}", serde_json::to_string_pretty(&report.findings)?),
                ScanFormat::Sarif => println!("{}", serde_json::to_string_pretty(&sarif(&report, &input))?),
                ScanFormat::Text => print_scan(&report),
            }
            // With --pooler, a finding fails only if the pooler was shown to mishandle it.
            let failing = report.findings.iter().any(|f| match &f.on_pooler {
                Some(check) => check.status != scan::PoolerStatus::Ok,
                None => strict || f.severity == scan::Severity::Breaks,
            });
            Ok(if failing { ExitCode::FAILURE } else { ExitCode::SUCCESS })
        }
        Command::Report { files, format } => {
            let reports = files.iter().map(|f| report::Report::load(f)).collect::<Result<Vec<_>>>()?;
            match format {
                ReportFormat::Markdown => print!("{}", report::markdown(&reports)),
                ReportFormat::Html => print!("{}", report::html(&reports)?),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::List { scenarios } => {
            for s in load_scenarios(scenarios.as_deref())? {
                println!("{:<40} {}", s.name, s.description);
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn load_scenarios(dir: Option<&std::path::Path>) -> Result<Vec<scenario::Scenario>> {
    match dir {
        Some(dir) => scenario::load_dir(dir),
        None => scenario::bundled(),
    }
}

fn print_verdict(target: &str, s: &scenario::Scenario, verdict: &Verdict) {
    match verdict {
        Verdict::Pass => println!("PASS  {target:<24} {}", s.name),
        Verdict::Skip { reason } => println!("SKIP  {target:<24} {} ({reason})", s.name),
        Verdict::Error { reason } => println!("ERROR {target:<24} {} ({reason})", s.name),
        Verdict::Fail { mismatch } => {
            println!("FAIL  {target:<24} {} ({})", s.name, mismatch.kind);
            println!("      {}", s.description);
            println!("      step {}: {}", mismatch.step, mismatch.action);
            println!("      postgres: {}", mismatch.expected);
            println!("      pooler:   {}", mismatch.actual);
            for r in &s.refs {
                println!("      from {r}");
            }
        }
    }
}

fn sarif(report: &scan::Report, input: &std::path::Path) -> serde_json::Value {
    use serde_json::json;

    let rules: Vec<_> =
        scan::RULES.iter().map(|(id, summary)| json!({"id": id, "shortDescription": {"text": summary}})).collect();
    let uri = input.to_string_lossy().replace('\\', "/");
    let results: Vec<_> = report
        .findings
        .iter()
        .map(|f| {
            let level = match (&f.on_pooler, f.severity) {
                (Some(check), _) if check.status == scan::PoolerStatus::Ok => "note",
                (Some(_), _) | (None, scan::Severity::Breaks) => "error",
                (None, scan::Severity::Warn) => "warning",
            };
            let pooler = f.on_pooler.as_ref().map(|c| format!(" Checked {}.", pooler_summary(c))).unwrap_or_default();
            json!({
                "ruleId": f.rule,
                "level": level,
                "message": {"text": format!("{} Calls: {}.{pooler}", f.detail, f.calls)},
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": {"uri": uri},
                        "region": {"startLine": f.line.max(1)},
                    },
                }],
            })
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {"driver": {
                "name": "poolproof",
                "informationUri": env!("CARGO_PKG_REPOSITORY"),
                "version": env!("CARGO_PKG_VERSION"),
                "rules": rules,
            }},
            "results": results,
        }],
    })
}

/// "On pgbouncer-1.26.1: set-isolated-between-clients fails silently, ..."
fn pooler_summary(check: &scan::PoolerCheck) -> String {
    let parts: Vec<String> = check
        .scenarios
        .iter()
        .map(|(scenario, result)| {
            let what = match result.as_str() {
                "pass" => "passes",
                "silent" => "fails silently",
                "error" => "fails with an error",
                "hang" => "hangs",
                other => other,
            };
            format!("{scenario} {what}")
        })
        .collect();
    format!("on {}: {}", check.target, parts.join(", "))
}

fn print_scan(report: &scan::Report) {
    for f in &report.findings {
        let label = match (&f.on_pooler, f.severity) {
            (Some(check), _) => match check.status {
                scan::PoolerStatus::Breaks => "BREAKS",
                scan::PoolerStatus::Rejected => "REJECTED",
                scan::PoolerStatus::Ok => "OK",
            },
            (None, scan::Severity::Breaks) => "BREAKS",
            (None, scan::Severity::Warn) => "WARN",
        };
        let query: String = f.query.split_whitespace().collect::<Vec<_>>().join(" ");
        let query = match (0..=100).rev().find(|&i| query.is_char_boundary(i)) {
            Some(end) if query.len() > 100 => format!("{}...", &query[..end]),
            _ => query,
        };
        println!("{label:<8} {:<27} calls={:<8} {query}", f.rule, f.calls);
        if let Some(check) = &f.on_pooler {
            println!("         {}", pooler_summary(check));
        }
        // How to fix it, unless the pooler was shown to handle it.
        if f.on_pooler.as_ref().is_none_or(|c| c.status != scan::PoolerStatus::Ok) {
            println!("         {}", f.detail);
        }
    }
    if let Some(check) = report.findings.iter().find_map(|f| f.on_pooler.as_ref()) {
        let pg = check.postgres.as_deref().unwrap_or("?");
        let count = |status| report.findings.iter().filter(|f| f.on_pooler.as_ref().is_some_and(|c| c.status == status)).count();
        println!(
            "\n{} queries scanned, {} findings. On {} (Postgres {pg}): {} break silently, {} rejected, {} handled.",
            report.queries,
            report.findings.len(),
            check.target,
            count(scan::PoolerStatus::Breaks),
            count(scan::PoolerStatus::Rejected),
            count(scan::PoolerStatus::Ok),
        );
        println!("See a scenario step by step: poolproof run -v --filter <scenario>");
        return;
    }
    let breaks = report.findings.iter().filter(|f| f.severity == scan::Severity::Breaks).count();
    println!(
        "\n{} queries scanned, {} findings ({breaks} break under transaction pooling), {} could not be parsed",
        report.queries,
        report.findings.len(),
        report.unparsed
    );
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
