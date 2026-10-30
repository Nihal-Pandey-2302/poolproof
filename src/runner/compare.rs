use serde::{Deserialize, Serialize};

use super::Outcome;
use crate::scenario::Compare;

impl Outcome {
    pub(super) fn matches(&self, other: &Outcome, compare: Compare) -> bool {
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

#[derive(Debug, Serialize, Deserialize)]
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

/// The first baseline step that failed in the harness rather than in Postgres. Postgres
/// is the oracle, so a baseline like that can't be compared: an unreachable server
/// would otherwise "match" an unreachable pooler.
pub(super) fn baseline_failure(expected: &[Outcome]) -> Option<String> {
    let failed = |o: &Outcome| match o {
        Outcome::Error { code, .. } => HARNESS_CODES.contains(&code.as_str()),
        Outcome::Messages { messages } => messages.iter().any(|l| l == "TIMEOUT" || l == "CLOSED"),
        Outcome::Ok { .. } | Outcome::Abandoned => false,
    };
    let (i, outcome) = expected.iter().enumerate().find(|(_, o)| failed(o))?;
    Some(format!("baseline failed at step {}: {outcome}", i + 1))
}

impl FailKind {
    pub(super) fn of(expected: &Outcome, actual: &Outcome) -> Self {
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
                let cut_short = messages.len() < expected_lines.len() && expected_lines.starts_with(messages);
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
