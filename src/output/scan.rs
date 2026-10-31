use crate::scan;

pub fn pooler_summary(check: &scan::PoolerCheck) -> String {
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
    let documented = match check.documented.as_slice() {
        [] => String::new(),
        docs => format!(" (documented: {})", docs.join(", ")),
    };
    let from = match (check.other_postgres, &check.postgres) {
        (true, Some(pg)) => format!(", driver results from Postgres {pg}"),
        _ => String::new(),
    };
    format!("on {}{from}: {}{documented}", check.target, parts.join(", "))
}

/// A driver finding's documented fix and how it does on the pooler.
pub fn fix_summary(fix: &scan::FixCheck) -> String {
    let how = match fix.status {
        scan::PoolerStatus::Ok => "passes",
        scan::PoolerStatus::Rejected => "still fails with an error",
        scan::PoolerStatus::Breaks => "still fails silently",
    };
    format!("with {}: {how}", fix.setting)
}

pub fn print_scan(report: &scan::Report) {
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
            if let Some(fix) = &check.fix {
                println!("         {}", fix_summary(fix));
            }
        }
        if let Some(note) = &f.pooler_note {
            println!("         {note}");
        }
        // How to fix it, unless the pooler was shown to handle it.
        if f.on_pooler.as_ref().is_none_or(|c| c.status != scan::PoolerStatus::Ok) {
            println!("         {}", f.detail);
        }
    }
    if let Some(check) = report.findings.iter().find_map(|f| f.on_pooler.as_ref()) {
        let pg = check.postgres.as_deref().unwrap_or("?");
        let count = |status| {
            report
                .findings
                .iter()
                .filter(|f| f.on_pooler.as_ref().is_some_and(|c| c.status == status))
                .count()
        };
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
        print_uninspected(report);
        return;
    }
    let breaks = report
        .findings
        .iter()
        .filter(|f| f.severity == scan::Severity::Breaks)
        .count();
    println!(
        "\n{} queries scanned, {} findings ({breaks} break under transaction pooling), {} could not be parsed",
        report.queries,
        report.findings.len(),
        report.unparsed
    );
    print_uninspected(report);
}

/// Bodies only word-scanned are not parse failures, but the reader should know. Nor
/// are the notes.
fn print_uninspected(report: &scan::Report) {
    for note in &report.notes {
        println!("Note: {note}");
    }
    if report.uninspected > 0 {
        println!(
            "Function and DO bodies not fully parsed, so only their words were checked: {}",
            report.uninspected
        );
    }
}
