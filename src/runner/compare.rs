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

/// A `send` reply line that means something went wrong: an error, a timeout, or
/// the connection closing.
fn is_failure_line(line: &str) -> bool {
    line.starts_with("Error ") || line == "TIMEOUT" || line == "CLOSED"
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
                if messages.iter().any(|l| l == "TIMEOUT") {
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
