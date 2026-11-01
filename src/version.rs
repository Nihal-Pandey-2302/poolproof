//! Version strings like `17.11`, `0.1.62` or `2.10.0-rc.1`, ordered the way semver
//! orders them: numeric parts compared as numbers (missing ones count as 0), and a
//! pre-release before its release.

use std::cmp::Ordering;

/// Equal when the order says so: `17` equals `17.0`.
#[derive(Debug)]
pub struct Version {
    core: Vec<u64>,
    pre: Vec<Ident>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ident {
    // Declared first: numeric identifiers sort below alphanumeric ones.
    Number(u64),
    Text(String),
}

impl Version {
    /// None for anything that isn't dotted numbers with an optional `-pre.release`,
    /// e.g. `dev` or `git-1a2b3c4`. Build metadata after `+` is ignored, as in semver.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.split_once('+').map_or(s, |(v, _)| v);
        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (s, None),
        };
        let core = core
            .split('.')
            .map(|p| p.parse::<u64>().ok().filter(|_| p.bytes().all(|b| b.is_ascii_digit())))
            .collect::<Option<Vec<_>>>()?;
        let pre = match pre {
            None => vec![],
            Some(pre) => pre
                .split('.')
                .map(|id| {
                    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                        None
                    } else if id.bytes().all(|b| b.is_ascii_digit()) {
                        id.parse().ok().map(Ident::Number)
                    } else {
                        Some(Ident::Text(id.to_string()))
                    }
                })
                .collect::<Option<Vec<_>>>()?,
        };
        Some(Version { core, pre })
    }

    /// `2.10.0-rc.1`, not `2.9.13`.
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let len = self.core.len().max(other.core.len());
        let part = |v: &Version, i: usize| v.core.get(i).copied().unwrap_or(0);
        (0..len)
            .map(|i| part(self, i).cmp(&part(other, i)))
            .find(|o| o.is_ne())
            .unwrap_or_else(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A total order on version strings: parsed versions by semver rules, anything that
/// doesn't parse below all of them, and among those by the text itself.
pub fn cmp(a: &str, b: &str) -> Ordering {
    match (Version::parse(a), Version::parse(b)) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.cmp(b)),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => a.cmp(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_like_semver() {
        let sorted = [
            "0.1.50",
            "0.1.56",
            "0.1.62",
            "2.9.13",
            "2.10.0-alpha",
            "2.10.0-rc.1",
            "2.10.0-rc.2",
            "2.10.0-rc.10",
            "2.10.0",
            "9.6",
            "17",
            "17.11",
            "18.4",
        ];
        for pair in sorted.windows(2) {
            assert_eq!(cmp(pair[0], pair[1]), Ordering::Less, "{pair:?}");
            assert_eq!(cmp(pair[1], pair[0]), Ordering::Greater, "{pair:?}");
        }
        // Missing parts count as 0, but the order stays total.
        assert_eq!(Version::parse("17").cmp(&Version::parse("17.0")), Ordering::Equal);
        assert_ne!(cmp("17", "17.0"), Ordering::Equal);
        // Numeric pre-release parts sort below text ones.
        assert_eq!(cmp("1.0.0-1", "1.0.0-a"), Ordering::Less);
        assert_eq!(cmp("1.0.0+build", "1.0.0-rc.1"), Ordering::Greater);
    }

    #[test]
    fn unparseable_versions_match_nothing_and_sort_first() {
        for bad in [
            "dev",
            "git-1a2b3c4",
            "",
            "1..2",
            "1.x",
            "1.0-",
            "1.0-rc..1",
            "v1.2",
            "1.0-rc_1",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad}");
            assert_eq!(cmp(bad, "0.0.1"), Ordering::Less, "{bad}");
        }
        assert_eq!(cmp("dev", "git-1"), Ordering::Less);
    }
}
