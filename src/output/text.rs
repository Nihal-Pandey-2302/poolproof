use std::io::IsTerminal;

use crate::limitations::Expected;
use crate::report::ResultRow;
use crate::runner::Verdict;
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

pub fn print_verdict(
    target: &str,
    product: Option<&str>,
    s: &scenario::Scenario,
    verdict: &Verdict,
    expected: Option<&Expected>,
    color: bool,
) {
    let label = label(verdict, color);
    match verdict {
        Verdict::Pass => println!("{label} {target:<24} {}", s.name),
        Verdict::Skip { reason } | Verdict::Error { reason } => println!("{label} {target:<24} {} ({reason})", s.name),
        Verdict::Fail { mismatch } => {
            let documented = if expected.is_some() { ", documented" } else { "" };
            println!("{label} {target:<24} {} ({}{documented})", s.name, mismatch.kind);
            println!("      {}", s.description);
            println!("      step {}: {}", mismatch.step, mismatch.action);
            println!("      postgres: {}", mismatch.expected);
            println!("      pooler:   {}", mismatch.actual);
            if let Some(e) = expected {
                let version = e.doc_version.as_deref().map(|v| format!(" ({v})")).unwrap_or_default();
                println!("      documented: \"{}\" {}{version}", e.quote, e.doc);
            }
            for r in &s.refs {
                println!("      {} {r}", scenario::ref_label(r, product));
            }
        }
    }
}

/// One scenario's result on one target, as `run` prints it.
pub fn print_result(s: &scenario::Scenario, row: &ResultRow, product: Option<&str>, style: Style) {
    print_verdict(
        &row.target,
        product,
        s,
        &row.verdict,
        row.expected.as_ref(),
        style.color,
    );
    if let Some(unmet) = row.precondition_unmet.filter(|u| *u > 0 || row.runs > 1) {
        let ran = row.runs + unmet;
        let rests = if row.runs == 1 && ran > 1 && matches!(row.verdict, Verdict::Pass) {
            "; the PASS rests on one run"
        } else {
            ""
        };
        println!("      precondition met in {} of {ran} runs{rests}", row.runs);
    }
    if let Some(apart) = row.reuse_unmet.filter(|u| *u > 0 || row.runs > 1) {
        let ran = row.runs + apart + row.precondition_unmet.unwrap_or(0);
        let rests = if row.runs == 1 && ran > 1 && matches!(row.verdict, Verdict::Pass) {
            "; the PASS rests on one run"
        } else {
            ""
        };
        println!(
            "      clients shared a server connection in {} of {ran} runs{rests}",
            row.runs
        );
    }
    if row.runs > 1 && row.failures > 0 {
        println!(
            "      failed in {} of {} runs (timing-sensitive)",
            row.failures, row.runs
        );
    }
    if style.verbose {
        for (i, (step, outcomes)) in s.steps.iter().zip(&row.steps).enumerate() {
            println!("      {}. {}", i + 1, step.describe());
            println!("         postgres: {}", outcomes.postgres);
            println!("         pooler:   {}", outcomes.pooler);
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
