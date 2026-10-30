use crate::runner::{Outcome, Verdict};
use crate::scenario;

pub fn print_verdict(target: &str, s: &scenario::Scenario, verdict: &Verdict) {
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

/// One scenario's result on one target, as `run` prints it.
pub fn print_result(
    target: &str,
    s: &scenario::Scenario,
    verdict: &Verdict,
    steps: &[(Outcome, Outcome)],
    runs: u32,
    failures: u32,
    verbose: bool,
) {
    print_verdict(target, s, verdict);
    if runs > 1 && failures > 0 {
        println!("      failed in {failures} of {runs} runs (timing-sensitive)");
    }
    if verbose {
        for (i, (step, (expected, actual))) in s.steps.iter().zip(steps).enumerate() {
            println!("      {}. {}", i + 1, step.describe());
            println!("         postgres: {expected}");
            println!("         pooler:   {actual}");
        }
    }
}
