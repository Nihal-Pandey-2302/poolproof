use serde::{Deserialize, Serialize};

use super::Outcome;
use crate::scenario::Compare;

impl Outcome {
    pub(crate) fn matches(&self, other: &Outcome, compare: Compare) -> bool {
        match compare {
            Compare::Ignore => true,
            Compare::Status => match (self, other) {
                (Outcome::Ok { .. }, Outcome::Ok { .. }) | (Outcome::Abandoned, Outcome::Abandoned) => true,
                (Outcome::Messages { messages: a }, Outcome::Messages { messages: b }) => {
                    let failures = |m: &[String]| m.iter().filter(|l| is_failure_line(l)).cloned().collect::<Vec<_>>();
                    failures(a) == failures(b)
                }
                (Outcome::Error { code: a, .. }, Outcome::Error { code: b, .. }) => a == b,
                _ => false,
            },
            // Error messages can differ in wording between servers, so only the code counts.
            Compare::Full => match (self, other) {
                (Outcome::Error { code: a, .. }, Outcome::Error { code: b, .. }) => a == b,
                (Outcome::Messages { messages: a }, Outcome::Messages { messages: b }) => {
                    split_parameter_status(a) == split_parameter_status(b)
                }
                _ => self == other,
            },
        }
    }
}

/// The steps (from 0) whose (postgres, pooler) outcomes differ, each compared the way
/// its step says. Direct steps count: a leak often shows only there.
pub fn differing<'a>(compare: &[Compare], steps: impl IntoIterator<Item = (&'a Outcome, &'a Outcome)>) -> Vec<usize> {
    steps
        .into_iter()
        .zip(compare)
        .enumerate()
        .filter(|(_, ((expected, actual), compare))| !expected.matches(actual, **compare))
        .map(|(i, _)| i)
        .collect()
}

/// Whether two pooler runs of a scenario failed the same way: the same steps differ,
/// and at each one the pooler's outcomes match the way the step compares them.
pub fn same_failure(compare: &[Compare], a: &[(Outcome, Outcome)], b: &[(Outcome, Outcome)]) -> bool {
    let pairs = |steps: &'_ [(Outcome, Outcome)]| differing(compare, steps.iter().map(|(e, a)| (e, a)));
    let steps = pairs(a);
    !steps.is_empty() && steps == pairs(b) && steps.iter().all(|&i| a[i].1.matches(&b[i].1, compare[i]))
}

/// An outcome the harness made up because the step never got a real answer.
pub fn from_harness(outcome: &Outcome) -> bool {
    match outcome {
        Outcome::Error { code, connect, .. } => {
            *connect || HARNESS_CODES.contains(&code.as_str()) || code.starts_with(DRIVER_CODE)
        }
        Outcome::Messages { messages } => messages.iter().any(|l| l == "TIMEOUT" || l == "CLOSED"),
        Outcome::Ok { .. } | Outcome::Abandoned => false,
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Mismatch {
    pub step: usize,
    pub action: String,
    pub kind: FailKind,
    pub expected: Outcome,
    pub actual: Outcome,
}

/// How a failure shows up to the application. Silent failures are the dangerous ones:
/// the client gets a result and no hint that it's wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum FailKind {
    /// The pooler returned a result, but not the one Postgres returned.
    Silent,
    /// The pooler returned an error where Postgres didn't, or a different one.
    Error,
    /// The pooler didn't answer within the step timeout.
    Hang,
}

/// A reply split into its ParameterStatus lines, sorted, and the rest in order. Poolers
/// move ParameterStatus around within a reply, which is harmless; dropping one or
/// sending a different value is not. The lines only exist when a step asks for them.
fn split_parameter_status(lines: &[String]) -> (Vec<&str>, Vec<&str>) {
    let (mut status, rest): (Vec<&str>, Vec<&str>) = lines
        .iter()
        .map(String::as_str)
        .partition(|l| l.starts_with("ParameterStatus "));
    status.sort_unstable();
    (status, rest)
}

/// A `send` reply line that means something went wrong: an error, a timeout, or
/// the connection closing.
fn is_failure_line(line: &str) -> bool {
    line.starts_with("Error ") || line == "TIMEOUT" || line == "CLOSED"
}

/// Error codes the harness itself produces when a step never got a real answer.
/// NO_STATEMENT isn't one: it follows a prepare that failed, and if Postgres rejected
/// that prepare, the outcome is deterministic and worth comparing.
const HARNESS_CODES: &[&str] = &["CONNECT", "TIMEOUT", "CLIENT", "CLOSED"];

/// Start of the code for a driver error without a SQLSTATE (`poolproof drivers`), e.g.
/// `DRIVER:OperationalError`. A harness code too: on the baseline it means the driver
/// failed, not Postgres.
pub const DRIVER_CODE: &str = "DRIVER:";

/// The first baseline step that failed in the harness rather than in Postgres. Postgres
/// is the oracle, so a baseline like that can't be compared: an unreachable server
/// would otherwise "match" an unreachable pooler.
pub(super) fn baseline_failure(expected: &[Outcome]) -> Option<String> {
    // Includes a connect Postgres refused (53300 when it's out of connections): it would
    // give the same code on both sides and pass without testing anything.
    let (i, outcome) = expected.iter().enumerate().find(|(_, o)| from_harness(o))?;
    Some(format!("baseline failed at step {}: {outcome}", i + 1))
}

impl FailKind {
    pub fn of(expected: &Outcome, actual: &Outcome) -> Self {
        match actual {
            Outcome::Error { code, .. } if code == "TIMEOUT" => FailKind::Hang,
            Outcome::Error { .. } => FailKind::Error,
            Outcome::Messages { messages } => {
                let expected_lines = match expected {
                    Outcome::Messages { messages } => messages.as_slice(),
                    _ => &[],
                };
                // Replies that stop short of what Postgres sent, with nothing wrong in
                // them, are a hang too: after a Flush there's no ReadyForQuery to wait
                // for, so the step ends when the server goes quiet instead of at TIMEOUT.
                // ParameterStatus lines don't count: a dropped one is a silent failure.
                let (_, got) = split_parameter_status(messages);
                let (_, wanted) = split_parameter_status(expected_lines);
                let cut_short = got.len() < wanted.len() && wanted.starts_with(&got);
                if cut_short || messages.iter().any(|l| l == "TIMEOUT") {
                    FailKind::Hang
                } else if messages
                    .iter()
                    .any(|l| is_failure_line(l) && !expected_lines.contains(l))
                {
                    FailKind::Error
                } else {
                    FailKind::Silent
                }
            }
            Outcome::Ok { .. } | Outcome::Abandoned => FailKind::Silent,
        }
    }
}

impl std::fmt::Display for FailKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            FailKind::Silent => "silent",
            FailKind::Error => "error",
            FailKind::Hang => "hang",
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail {
        mismatch: Mismatch,
    },
    Skip {
        reason: String,
    },
    /// The run could not be set up, so there is nothing to compare.
    Error {
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_compare_by_code_only() {
        let a = Outcome::error("42P01", "relation \"x\" does not exist");
        let b = Outcome::error("42P01", "relation x does not exist");
        assert!(a.matches(&b, Compare::Full));
        assert!(!a.matches(&Outcome::empty(), Compare::Status));
    }

    #[test]
    fn fail_kind_follows_pooler_outcome() {
        let ok = Outcome::empty();
        assert_eq!(FailKind::of(&ok, &Outcome::empty()), FailKind::Silent);
        assert_eq!(
            FailKind::of(&ok, &Outcome::error("XX000", "not allowed")),
            FailKind::Error
        );
        assert_eq!(
            FailKind::of(&ok, &Outcome::error("TIMEOUT", "no reply")),
            FailKind::Hang
        );
        let msgs = |m: &[&str]| Outcome::Messages {
            messages: m.iter().map(|s| s.to_string()).collect(),
        };
        let pg = msgs(&["ParseComplete", "Error 42601", "ReadyForQuery I"]);
        assert_eq!(
            FailKind::of(&pg, &msgs(&["ParseComplete", "ReadyForQuery I"])),
            FailKind::Silent
        );
        assert_eq!(
            FailKind::of(&pg, &msgs(&["Error 08P01", "ReadyForQuery I"])),
            FailKind::Error
        );
        assert_eq!(FailKind::of(&pg, &msgs(&["ParseComplete", "TIMEOUT"])), FailKind::Hang);
        // Pooler swallowed the reply to a Flush: nothing, or less than Postgres sent.
        let flushed = msgs(&["ParseComplete", "Error 42601"]);
        assert_eq!(FailKind::of(&flushed, &msgs(&[])), FailKind::Hang);
        assert_eq!(FailKind::of(&flushed, &msgs(&["ParseComplete"])), FailKind::Hang);
        // Same length but different stays what it was.
        assert_eq!(
            FailKind::of(&flushed, &msgs(&["ParseComplete", "NoData"])),
            FailKind::Silent
        );
        assert_eq!(
            FailKind::of(&flushed, &msgs(&["ParseComplete", "Error 08P01"])),
            FailKind::Error
        );
    }

    #[test]
    fn harness_errors_in_the_baseline_are_failures() {
        let msgs = |m: &[&str]| Outcome::Messages {
            messages: m.iter().map(|s| s.to_string()).collect(),
        };
        for code in HARNESS_CODES {
            let failure = baseline_failure(&[Outcome::empty(), Outcome::error(code, "x")]).unwrap();
            assert!(failure.starts_with("baseline failed at step 2: error"), "{failure}");
        }
        assert!(baseline_failure(&[msgs(&["ParseComplete", "TIMEOUT"])]).is_some());
        assert!(baseline_failure(&[msgs(&["CLOSED"])]).is_some());
        // Errors from Postgres itself are what scenarios test for.
        assert!(baseline_failure(&[Outcome::error("42P01", "x"), msgs(&["Error 42P05", "ReadyForQuery I"])]).is_none());
        assert!(baseline_failure(&[Outcome::Abandoned]).is_none());
        // An execute after a prepare that Postgres itself rejected: deterministic, so
        // it's compared like any other outcome.
        assert!(baseline_failure(&[Outcome::error("25P02", "x"), Outcome::error("NO_STATEMENT", "x")]).is_none());
    }

    #[test]
    fn driver_errors_without_sqlstate() {
        let driver = Outcome::error("DRIVER:OperationalError", "");
        // Baseline: the driver failed, not Postgres, so nothing to compare.
        let reason = baseline_failure(&[Outcome::empty(), driver.clone()]).unwrap();
        assert!(reason.contains("step 2"), "{reason}");
        // Pooler side: an error Postgres didn't give.
        assert_eq!(FailKind::of(&Outcome::empty(), &driver), FailKind::Error);
        assert!(!Outcome::empty().matches(&driver, Compare::Full));
    }

    #[test]
    fn parameter_status_lines_compare_as_a_set() {
        let msgs = |m: &[&str]| Outcome::Messages {
            messages: m.iter().map(|s| s.to_string()).collect(),
        };
        let pg = msgs(&[
            "CommandComplete SET",
            "ParameterStatus application_name=pp",
            "ParameterStatus TimeZone=UTC",
            "ReadyForQuery I",
        ]);
        // Moved within the reply: poolers do that, and it's harmless.
        let moved = msgs(&[
            "ParameterStatus TimeZone=UTC",
            "CommandComplete SET",
            "ReadyForQuery I",
            "ParameterStatus application_name=pp",
        ]);
        assert!(pg.matches(&moved, Compare::Full));
        let dropped = msgs(&["CommandComplete SET", "ParameterStatus TimeZone=UTC", "ReadyForQuery I"]);
        assert!(!pg.matches(&dropped, Compare::Full));
        assert_eq!(FailKind::of(&pg, &dropped), FailKind::Silent);
        // After a Flush, a dropped ParameterStatus at the end leaves a prefix of the
        // Postgres reply. That's still silent: the reply itself is complete.
        let flushed = msgs(&[
            "ParseComplete",
            "BindComplete",
            "CommandComplete SET",
            "ParameterStatus application_name=x",
        ]);
        let no_status = msgs(&["ParseComplete", "BindComplete", "CommandComplete SET"]);
        assert!(!flushed.matches(&no_status, Compare::Full));
        assert_eq!(FailKind::of(&flushed, &no_status), FailKind::Silent);
        // A reply really cut short is still a hang.
        assert_eq!(FailKind::of(&flushed, &msgs(&["ParseComplete"])), FailKind::Hang);
        let faked = msgs(&[
            "CommandComplete SET",
            "ParameterStatus application_name=other",
            "ParameterStatus TimeZone=UTC",
            "ReadyForQuery I",
        ]);
        assert!(!pg.matches(&faked, Compare::Full));
        // The other lines still count in order.
        let reordered = msgs(&[
            "ReadyForQuery I",
            "ParameterStatus application_name=pp",
            "ParameterStatus TimeZone=UTC",
            "CommandComplete SET",
        ]);
        assert!(!pg.matches(&reordered, Compare::Full));
        // `status` only looks at failures.
        assert!(pg.matches(&dropped, Compare::Status));
    }

    #[test]
    fn runs_fail_the_same_way_only_with_the_same_steps_and_outcomes() {
        let row = |v: &str| Outcome::Ok {
            rows: vec![vec![Some(v.into())]],
            tags: vec![1],
        };
        let modes = [Compare::Full, Compare::Status];
        let run = |first: &str, second: Outcome| vec![(row("pg"), row(first)), (Outcome::empty(), second)];
        let leak = run("x", Outcome::empty());
        assert_eq!(differing(&modes, leak.iter().map(|(e, a)| (e, a))), [0]);
        assert!(same_failure(&modes, &leak, &run("x", Outcome::empty())));
        // Another value at the same step, or another step differing too.
        assert!(!same_failure(&modes, &leak, &run("y", Outcome::empty())));
        assert!(!same_failure(&modes, &leak, &run("x", Outcome::error("42P05", "x"))));
        // A `status` step only compares the status: other rows are the same failure.
        let other_rows = vec![(row("pg"), row("x")), (Outcome::empty(), row("z"))];
        assert!(same_failure(&modes, &leak, &other_rows));
        // Runs that both pass didn't fail at all.
        let pass = run("pg", Outcome::empty());
        assert!(!same_failure(&modes, &pass, &pass));
    }

    #[test]
    fn full_compare_checks_rows() {
        let a = Outcome::Ok {
            rows: vec![vec![Some("1".into())]],
            tags: vec![1],
        };
        let b = Outcome::Ok {
            rows: vec![vec![Some("2".into())]],
            tags: vec![1],
        };
        assert!(!a.matches(&b, Compare::Full));
        assert!(a.matches(&b, Compare::Status));
    }
}
