mod runner;
mod scan;
mod scenario;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

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
        /// Scenario directory.
        #[arg(short, long, default_value = "scenarios")]
        scenarios: PathBuf,
        /// Only run scenarios whose name contains this.
        #[arg(short, long)]
        filter: Option<String>,
        /// Run scenarios in reverse order. Verdicts should not change; if they do,
        /// state is leaking between scenarios.
        #[arg(long)]
        reverse: bool,
        #[arg(long, value_enum, default_value_t = RunFormat::Text)]
        format: RunFormat,
    },
    /// Scan queries (pg_stat_statements CSV or a .sql file) for patterns that
    /// break under transaction pooling.
    Scan {
        input: PathBuf,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Exit non-zero on warnings too, not just on queries that break.
        #[arg(long)]
        strict: bool,
    },
    /// List bundled scenarios.
    List {
        #[arg(short, long, default_value = "scenarios")]
        scenarios: PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum RunFormat {
    Text,
    Json,
    /// A scenario x target table plus failure details, for a README or CI summary.
    Markdown,
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
    /// Statements run on `direct` before every run to drop state leaked on the
    /// pooler's server connections, usually by terminating them. Without it,
    /// one scenario's leak can change the next one's result.
    #[serde(default)]
    reset: Vec<String>,
}

#[derive(Serialize)]
struct ResultRow<'a> {
    target: &'a str,
    scenario: &'a str,
    #[serde(flatten)]
    verdict: Verdict,
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
        Command::Run { targets, scenarios, filter, reverse, format } => {
            let text = std::fs::read_to_string(&targets)
                .with_context(|| format!("reading {}", targets.display()))?;
            let targets: Targets = toml::from_str(&text).context("parsing targets")?;
            let mut scenarios = scenario::load_dir(&scenarios)?;
            if reverse {
                scenarios.reverse();
            }

            let mut results = Vec::new();
            for target in &targets.targets {
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
                    let verdict = if s.modes.contains(&target.mode) {
                        runner::check(s, &target.pooler, &target.direct, &target.reset)
                            .await
                            .with_context(|| format!("{} / {}", target.name, s.name))?
                    } else {
                        Verdict::Skip { reason: format!("not applicable to {} mode", target.mode) }
                    };
                    if matches!(format, RunFormat::Text) {
                        print_verdict(&target.name, s, &verdict);
                    }
                    results.push(ResultRow { target: &target.name, scenario: &s.name, verdict });
                }
            }

            let count = |f: fn(&Verdict) -> bool| results.iter().filter(|r| f(&r.verdict)).count();
            let failed = count(|v| matches!(v, Verdict::Fail { .. }));
            let errors = count(|v| matches!(v, Verdict::Error { .. }));
            match format {
                RunFormat::Json => println!("{}", serde_json::to_string_pretty(&results)?),
                RunFormat::Markdown => print!("{}", markdown(&targets.targets, &scenarios, &results)),
                RunFormat::Text => {
                    let passed = count(|v| matches!(v, Verdict::Pass));
                    let skipped = count(|v| matches!(v, Verdict::Skip { .. }));
                    println!("\n{passed} passed, {failed} failed, {errors} errors, {skipped} skipped");
                }
            }
            Ok(if failed + errors > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS })
        }
        Command::Scan { input, format, strict } => {
            let queries = scan::load_queries(&input)?;
            let report = scan::scan(&queries);
            match format {
                Format::Json => println!("{}", serde_json::to_string_pretty(&report.findings)?),
                Format::Text => print_scan(&report),
            }
            let failing = report
                .findings
                .iter()
                .any(|f| strict || f.severity == scan::Severity::Breaks);
            Ok(if failing { ExitCode::FAILURE } else { ExitCode::SUCCESS })
        }
        Command::List { scenarios } => {
            for s in scenario::load_dir(&scenarios)? {
                println!("{:<40} {}", s.name, s.description);
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn print_verdict(target: &str, s: &scenario::Scenario, verdict: &Verdict) {
    match verdict {
        Verdict::Pass => println!("PASS  {target:<24} {}", s.name),
        Verdict::Skip { reason } => println!("SKIP  {target:<24} {} ({reason})", s.name),
        Verdict::Error { reason } => println!("ERROR {target:<24} {} ({reason})", s.name),
        Verdict::Fail { mismatch } => {
            println!("FAIL  {target:<24} {}", s.name);
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

fn markdown(targets: &[Target], scenarios: &[scenario::Scenario], results: &[ResultRow]) -> String {
    use std::fmt::Write;

    let verdict = |t: &str, s: &str| results.iter().find(|r| r.target == t && r.scenario == s).map(|r| &r.verdict);
    let ran: Vec<&scenario::Scenario> =
        scenarios.iter().filter(|s| results.iter().any(|r| r.scenario == s.name)).collect();

    let mut out = String::from("# poolproof results\n\n");
    out.push_str("Each scenario runs against plain Postgres, then through the pooler. FAIL means the\n");
    out.push_str("pooler gave a different result than Postgres. Not every FAIL is a bug: some are\n");
    out.push_str("documented limits of the pooling mode. Links point to the report each scenario came from.\n\n");
    let _ = write!(out, "| Scenario |");
    for t in targets {
        let _ = write!(out, " {} ({}) |", t.name, t.mode);
    }
    let _ = write!(out, "\n|---|{}\n", "---|".repeat(targets.len()));
    for s in &ran {
        let _ = write!(out, "| `{}` |", s.name);
        for t in targets {
            let cell = match verdict(&t.name, &s.name) {
                Some(Verdict::Pass) => "pass",
                Some(Verdict::Fail { .. }) => "**FAIL**",
                Some(Verdict::Error { .. }) => "ERROR",
                Some(Verdict::Skip { .. }) => "skip",
                None => "",
            };
            let _ = write!(out, " {cell} |");
        }
        out.push('\n');
    }

    let mut details = String::new();
    for s in &ran {
        for t in targets {
            match verdict(&t.name, &s.name) {
                Some(Verdict::Fail { mismatch: m }) => {
                    let _ = write!(details, "\n### {} / {}\n\n{}\n\n", t.name, s.name, s.description);
                    let _ = writeln!(details, "- step {}: `{}`", m.step, m.action);
                    let _ = writeln!(details, "- postgres: `{}`", m.expected);
                    let _ = writeln!(details, "- pooler: `{}`", m.actual);
                    for r in &s.refs {
                        let _ = writeln!(details, "- from {r}");
                    }
                }
                Some(Verdict::Error { reason }) => {
                    let _ = write!(details, "\n### {} / {}: ERROR\n\n{reason}\n", t.name, s.name);
                }
                _ => {}
            }
        }
    }
    if !details.is_empty() {
        out.push_str("\n## Differences\n");
        out.push_str(&details);
    }
    out
}

fn print_scan(report: &scan::Report) {
    for f in &report.findings {
        let label = match f.severity {
            scan::Severity::Breaks => "BREAKS",
            scan::Severity::Warn => "WARN",
        };
        let query: String = f.query.split_whitespace().collect::<Vec<_>>().join(" ");
        let query = if query.len() > 100 { format!("{}...", &query[..query.floor_char_boundary(100)]) } else { query };
        println!("{label:<7} {:<22} calls={:<8} {query}", f.rule, f.calls);
        println!("        {}", f.detail);
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
