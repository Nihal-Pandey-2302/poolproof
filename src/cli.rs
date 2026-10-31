use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

use crate::output::sarif::sarif;
use crate::output::scan::print_scan;
use crate::output::text::{Style, color_enabled};
use crate::runner::suite::FailOn;
use crate::runner::{self, Verdict};
use crate::scenario::{self, PoolMode};
use crate::targets::{Reset, Targets};
use crate::{report, scan};

#[derive(Parser)]
#[command(
    version,
    about = "Find out what breaks when you put a connection pooler in front of Postgres"
)]
pub struct Cli {
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
        /// With --pooler: what to run on --direct before every run. `auto` ends only
        /// the pooler's own sessions; `all` ends every other session on the database.
        /// Without it, nothing is reset.
        #[arg(long, value_enum, requires = "pooler")]
        reset: Option<ResetArg>,
        /// Allow reset = "all" against a Postgres that isn't on this machine. It gates
        /// nothing else.
        #[arg(long)]
        yes: bool,
        /// Scenario directory. Defaults to the scenarios built into poolproof.
        #[arg(short, long)]
        scenarios: Option<PathBuf>,
        /// Only run scenarios whose name contains this.
        #[arg(short, long)]
        filter: Option<String>,
        /// Only run this target from the targets file. Repeat for several.
        #[arg(long = "target", value_name = "NAME")]
        only: Vec<String>,
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
        /// Run each scenario's Postgres side twice first. A scenario whose results
        /// differ between the two runs is an ERROR: it tests timing, not the pooler.
        #[arg(long)]
        check_scenarios: bool,
        /// How long each step may take before it counts as a hang, e.g. `10s` or
        /// `500ms` (a bare number is seconds). A scenario's `timeout_ms` overrides it.
        #[arg(long, value_name = "DURATION", default_value = "10s", value_parser = parse_duration)]
        step_timeout: Duration,
        /// Which FAILs make the exit code 1, by kind. An ERROR verdict (nothing
        /// compared) counts unless this is `none`.
        #[arg(long, value_enum, default_value_t = FailOn::Any)]
        fail_on: FailOn,
    },
    /// Scan queries (pg_stat_statements CSV, Postgres logs or a .sql file) for
    /// patterns that break under transaction pooling.
    Scan {
        /// pg_stat_statements CSV, Postgres log (.log stderr, .json jsonlog, .csv csvlog)
        /// or .sql file. `-` reads stdin (needs --input).
        input: PathBuf,
        /// What the input is. Without it, the file extension decides.
        #[arg(long = "input", value_enum, value_name = "FORMAT")]
        input_format: Option<scan::InputFormat>,
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

/// `run --reset`.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum ResetArg {
    Auto,
    All,
}

pub async fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Run {
            targets,
            pooler,
            direct,
            mode,
            reset,
            yes,
            scenarios,
            filter,
            only,
            reverse,
            verbose,
            format,
            check_scenarios,
            fail_on,
            step_timeout,
        } => {
            let targets = match (pooler, direct) {
                (Some(pooler), Some(direct)) => {
                    let reset = match reset {
                        None => Reset::None,
                        Some(ResetArg::Auto) => Reset::Auto,
                        Some(ResetArg::All) => Reset::All,
                    };
                    Targets::single(&pooler, &direct, mode, reset)?
                }
                _ => Targets::load(&targets)?,
            };
            let mut scenarios = load_scenarios(scenarios.as_deref())?;
            if reverse {
                scenarios.reverse();
            }

            let text = matches!(format, RunFormat::Text);
            let opts = runner::suite::RunOptions {
                filter: filter.as_deref(),
                targets: &only,
                text,
                style: Style {
                    verbose,
                    color: color_enabled(),
                },
                check_scenarios,
                yes,
                timeouts: runner::Timeouts {
                    step: step_timeout,
                    ..Default::default()
                },
            };
            let (target_infos, results) = runner::suite::run(&targets.targets, &scenarios, &opts).await?;

            let count = |f: fn(&Verdict) -> bool| results.iter().filter(|r| f(&r.verdict)).count();
            let failed = count(|v| matches!(v, Verdict::Fail { .. }));
            let errors = count(|v| matches!(v, Verdict::Error { .. }));
            let exit_failure = results.iter().any(|r| fail_on.fails(&r.verdict));
            let ignored = results
                .iter()
                .filter(|r| matches!(r.verdict, Verdict::Fail { .. } | Verdict::Error { .. }))
                .filter(|r| !fail_on.fails(&r.verdict))
                .count();
            if ignored > 0 {
                eprintln!("note: {ignored} failed or errored results ignored by --fail-on {fail_on}");
            }
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
                    schema_version: report::SCHEMA_VERSION,
                    poolproof: env!("CARGO_PKG_VERSION").to_string(),
                    generated_at: Some(report::now_rfc3339()),
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
            Ok(if exit_failure {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Scan {
            input,
            input_format,
            format,
            strict,
            pooler,
            postgres,
            results,
        } => {
            let queries = scan::load_queries(&input, input_format)?;
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
                ScanFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
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
            let reports = files
                .iter()
                .map(|f| report::Report::load(f))
                .collect::<Result<Vec<_>>>()?;
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

/// `500ms`, `10s` or `10` (seconds). Zero is rejected: every step would time out.
fn parse_duration(s: &str) -> Result<Duration, String> {
    let (number, unit) = match s.strip_suffix("ms") {
        Some(n) => (n, Duration::from_millis(1)),
        None => (s.strip_suffix('s').unwrap_or(s), Duration::from_secs(1)),
    };
    let n: u32 = number
        .trim()
        .parse()
        .map_err(|_| format!("`{s}` is not a duration like 10s or 500ms"))?;
    if n == 0 {
        return Err("must be more than 0".into());
    }
    Ok(unit * n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_take_ms_s_or_bare_seconds() {
        assert_eq!(parse_duration("500ms"), Ok(Duration::from_millis(500)));
        assert_eq!(parse_duration("10s"), Ok(Duration::from_secs(10)));
        assert_eq!(parse_duration("3"), Ok(Duration::from_secs(3)));
        for bad in ["0", "0ms", "", "s", "1.5s", "10m", "-1s"] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
    }
}
