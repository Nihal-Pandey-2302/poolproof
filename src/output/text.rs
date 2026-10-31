use std::io::IsTerminal;

use crate::runner::{Outcome, Verdict};
use crate::scenario;

/// How `run` prints results as text.
#[derive(Clone, Copy, Default)]
pub struct Style {
    /// Every step's outcomes, not just the first difference.
    pub verbose: bool,
    /// Color the verdict labels.
    pub color: bool,
}

/// Color on a terminal, or anywhere with FORCE_COLOR or CLICOLOR_FORCE; never with
/// NO_COLOR (https://no-color.org).
pub fn color_enabled() -> bool {
    let var = |name| std::env::var(name).ok();
    color_from(
        var("NO_COLOR"),
        var("FORCE_COLOR").or(var("CLICOLOR_FORCE")),
        std::io::stdout().is_terminal(),
    )
}

fn color_from(no_color: Option<String>, force: Option<String>, terminal: bool) -> bool {
    let set = |v: &Option<String>| v.as_deref().is_some_and(|v| !v.is_empty() && v != "0");
    let no_color = no_color.is_some_and(|v| !v.is_empty());
    !no_color && (set(&force) || terminal)
}

/// The verdict's label, padded to line up, in its color if `color`.
fn label(verdict: &Verdict, color: bool) -> String {
    let (text, code) = match verdict {
        Verdict::Pass => ("PASS", "32"),
        Verdict::Skip { .. } => ("SKIP", "2"),
        Verdict::Error { .. } => ("ERROR", "33"),
        Verdict::Fail { .. } => ("FAIL", "31"),
    };
    let padded = format!("{text:<5}");
    if color {
        format!("\x1b[{code}m{padded}\x1b[0m")
    } else {
        padded
    }
}

pub fn print_verdict(target: &str, s: &scenario::Scenario, verdict: &Verdict, color: bool) {
    let label = label(verdict, color);
    match verdict {
        Verdict::Pass => println!("{label} {target:<24} {}", s.name),
        Verdict::Skip { reason } | Verdict::Error { reason } => println!("{label} {target:<24} {} ({reason})", s.name),
        Verdict::Fail { mismatch } => {
            println!("{label} {target:<24} {} ({})", s.name, mismatch.kind);
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
    style: Style,
) {
    print_verdict(target, s, verdict, style.color);
    if runs > 1 && failures > 0 {
        println!("      failed in {failures} of {runs} runs (timing-sensitive)");
    }
    if style.verbose {
        for (i, (step, (expected, actual))) in s.steps.iter().zip(steps).enumerate() {
            println!("      {}. {}", i + 1, step.describe());
            println!("         postgres: {expected}");
            println!("         pooler:   {actual}");
        }
    }
}

/// One line on stderr per result, for formats that print everything at the end.
pub fn print_progress(done: usize, total: usize, target: &str, s: &scenario::Scenario, verdict: &Verdict) {
    let what = match verdict {
        Verdict::Fail { mismatch } => format!("FAIL ({})", mismatch.kind),
        _ => label(verdict, false).trim_end().to_string(),
    };
    eprintln!("[{done}/{total}] {target} / {} {what}", s.name);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_beats_force_and_force_beats_no_terminal() {
        let s = |v: &str| Some(v.to_string());
        assert!(color_from(None, None, true));
        assert!(!color_from(None, None, false));
        assert!(!color_from(s("1"), None, true));
        assert!(!color_from(s("1"), s("1"), true));
        assert!(color_from(None, s("1"), false));
        assert!(!color_from(None, s("0"), false));
        assert!(color_from(s(""), None, true));
    }

    #[test]
    fn labels_line_up_and_color_only_when_asked() {
        let pass = Verdict::Pass;
        let error = Verdict::Error { reason: "x".into() };
        assert_eq!(label(&pass, false), "PASS ");
        assert_eq!(label(&error, false), "ERROR");
        assert_eq!(label(&pass, true), "\x1b[32mPASS \x1b[0m");
    }
}
