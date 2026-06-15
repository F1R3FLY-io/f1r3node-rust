//! Semver parsing and matching for the versioned registry.
//!
//! Per the Versioned Registry FIP: version patterns are `M.m.p`,
//! `M.m.*`, `M.*`, or `*`. Wildcards never match a prerelease
//! (`M.m.p-pre`); only an exact pattern can. Release versions are
//! ordered greater than their prereleases.

use std::cmp::Ordering;
use std::fmt;

/// A concrete semver version. `pre` is `None` for releases and
/// `Some` for prereleases like `1.2.3-alpha`. Build metadata
/// (`1.2.3+build`) is not supported by this FIP.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub pre: Option<String>,
}

/// A version pattern. Only the four shapes the FIP enumerates are
/// accepted; mid-component wildcards like `1.*.3` are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pattern {
    Exact(Version),
    PatchWild { major: u32, minor: u32 },
    MinorWild { major: u32 },
    MajorWild,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemverError {
    Empty,
    WrongComponentCount,
    EmptyComponent,
    NotANumber(String),
    EmptyPrerelease,
    PrereleaseInPattern,
    MidComponentWildcard,
}

impl fmt::Display for SemverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SemverError::Empty => write!(f, "empty version or pattern"),
            SemverError::WrongComponentCount => {
                write!(
                    f,
                    "a version or pattern must have 1, 2, or 3 dot-separated components"
                )
            }
            SemverError::EmptyComponent => write!(f, "empty component between dots"),
            SemverError::NotANumber(s) => write!(f, "component is not a non-negative integer: {s}"),
            SemverError::EmptyPrerelease => write!(f, "prerelease tag after '-' is empty"),
            SemverError::PrereleaseInPattern => {
                write!(
                    f,
                    "patterns with a wildcard cannot include a prerelease tag"
                )
            }
            SemverError::MidComponentWildcard => {
                write!(
                    f,
                    "wildcards are only allowed in trailing position (e.g. '1.*', not '1.*.3')"
                )
            }
        }
    }
}

impl std::error::Error for SemverError {}

/// Standard semver ordering: lex by major, then minor, then patch,
/// then release > prerelease, then lex on the prerelease tag.
impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

impl Version {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: None,
        }
    }

    /// True iff this version was tagged as a prerelease.
    pub fn is_prerelease(&self) -> bool { self.pre.is_some() }
}

/// Parse a concrete version `M.m.p` or `M.m.p-pre`. Every component
/// must be a non-negative decimal integer; partial forms (`1`,
/// `1.2`) are rejected — use `parse_pattern` if you want
/// wildcard-style imports.
pub fn parse_version(s: &str) -> Result<Version, SemverError> {
    if s.is_empty() {
        return Err(SemverError::Empty);
    }

    let (core, pre) = match s.split_once('-') {
        Some((_, "")) => return Err(SemverError::EmptyPrerelease),
        Some((c, p)) => (c, Some(p.to_string())),
        None => (s, None),
    };

    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return Err(SemverError::WrongComponentCount);
    }

    let major = parse_component(parts[0])?;
    let minor = parse_component(parts[1])?;
    let patch = parse_component(parts[2])?;

    Ok(Version {
        major,
        minor,
        patch,
        pre,
    })
}

/// Parse a version pattern. Accepts `*`, `M.*`, `M.m.*`, or an exact
/// `M.m.p[-pre]`. Mid-component wildcards (`1.*.3`) are rejected.
pub fn parse_pattern(s: &str) -> Result<Pattern, SemverError> {
    if s.is_empty() {
        return Err(SemverError::Empty);
    }

    if s == "*" {
        return Ok(Pattern::MajorWild);
    }

    // A wildcard pattern can't have a prerelease — only exact patterns can.
    if s.contains('*') && s.contains('-') {
        return Err(SemverError::PrereleaseInPattern);
    }

    let parts: Vec<&str> = s.split('.').collect();
    match parts.as_slice() {
        // `M.*` — minor wildcard
        [maj, "*"] => Ok(Pattern::MinorWild {
            major: parse_component(maj)?,
        }),
        // `M.m.*` — patch wildcard
        [maj, min, "*"] => Ok(Pattern::PatchWild {
            major: parse_component(maj)?,
            minor: parse_component(min)?,
        }),
        // `M.*.p` or `*.m.p` etc. — illegal
        [_, _, _] if parts.contains(&"*") => Err(SemverError::MidComponentWildcard),
        [_, _] if parts.contains(&"*") => Err(SemverError::MidComponentWildcard),
        // exact — defer to `parse_version`
        [_, _, _] => Ok(Pattern::Exact(parse_version(s)?)),
        _ => Err(SemverError::WrongComponentCount),
    }
}

fn parse_component(s: &str) -> Result<u32, SemverError> {
    if s.is_empty() {
        return Err(SemverError::EmptyComponent);
    }
    s.parse::<u32>()
        .map_err(|_| SemverError::NotANumber(s.to_string()))
}

impl Pattern {
    /// True iff `v` matches `self`. Wildcards never match a
    /// prerelease; an exact pattern matches whatever it spells.
    pub fn matches(&self, v: &Version) -> bool {
        match self {
            Pattern::Exact(want) => v == want,
            Pattern::PatchWild { major, minor } => {
                !v.is_prerelease() && v.major == *major && v.minor == *minor
            }
            Pattern::MinorWild { major } => !v.is_prerelease() && v.major == *major,
            Pattern::MajorWild => !v.is_prerelease(),
        }
    }

    /// Highest matching version in an iterator, or `None` if no
    /// version matches. Ties broken by `Version`'s `Ord`.
    pub fn best_match<'a, I>(&self, candidates: I) -> Option<&'a Version>
    where I: IntoIterator<Item = &'a Version> {
        candidates.into_iter().filter(|v| self.matches(v)).max()
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pattern::Exact(v) => write!(f, "{v}"),
            Pattern::PatchWild { major, minor } => write!(f, "{major}.{minor}.*"),
            Pattern::MinorWild { major } => write!(f, "{major}.*"),
            Pattern::MajorWild => write!(f, "*"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- parse_version -----

    #[test]
    fn parses_release_version() {
        assert_eq!(parse_version("1.2.3"), Ok(Version::new(1, 2, 3)));
        assert_eq!(parse_version("0.0.0"), Ok(Version::new(0, 0, 0)));
    }

    #[test]
    fn parses_prerelease() {
        let v = parse_version("2.6.3-alpha").unwrap();
        assert_eq!(v.major, 2);
        assert_eq!(v.minor, 6);
        assert_eq!(v.patch, 3);
        assert_eq!(v.pre.as_deref(), Some("alpha"));
        assert!(v.is_prerelease());
    }

    #[test]
    fn allows_large_components() {
        let v = parse_version(&format!("{}.0.0", u32::MAX)).unwrap();
        assert_eq!(v.major, u32::MAX);
    }

    #[test]
    fn rejects_partial_versions() {
        assert_eq!(parse_version("1.2"), Err(SemverError::WrongComponentCount));
        assert_eq!(parse_version("1"), Err(SemverError::WrongComponentCount));
    }

    #[test]
    fn rejects_too_many_components() {
        assert_eq!(
            parse_version("1.2.3.4"),
            Err(SemverError::WrongComponentCount)
        );
    }

    #[test]
    fn rejects_empty_prerelease_tag() {
        assert_eq!(parse_version("1.2.3-"), Err(SemverError::EmptyPrerelease));
    }

    #[test]
    fn rejects_non_numeric() {
        assert!(matches!(
            parse_version("1.x.3"),
            Err(SemverError::NotANumber(_))
        ));
        // "1.-2.3" is read as core="1." + prerelease="2.3", so the
        // core has only two segments — rejected on shape, not on
        // numeric content. Either way, no version comes out.
        assert!(parse_version("1.-2.3").is_err());
    }

    #[test]
    fn rejects_empty_string_and_empty_components() {
        assert_eq!(parse_version(""), Err(SemverError::Empty));
        assert_eq!(parse_version("1..3"), Err(SemverError::EmptyComponent));
        assert_eq!(parse_version(".2.3"), Err(SemverError::EmptyComponent));
    }

    #[test]
    fn rejects_wildcards_in_exact_version() {
        // parse_version is strict — wildcards belong to parse_pattern.
        assert!(matches!(
            parse_version("1.2.*"),
            Err(SemverError::NotANumber(_))
        ));
    }

    // ----- parse_pattern -----

    #[test]
    fn parses_star() {
        assert_eq!(parse_pattern("*"), Ok(Pattern::MajorWild));
    }

    #[test]
    fn parses_minor_wildcard() {
        assert_eq!(parse_pattern("2.*"), Ok(Pattern::MinorWild { major: 2 }));
    }

    #[test]
    fn parses_patch_wildcard() {
        assert_eq!(
            parse_pattern("2.6.*"),
            Ok(Pattern::PatchWild { major: 2, minor: 6 }),
        );
    }

    #[test]
    fn parses_exact_pattern() {
        assert_eq!(
            parse_pattern("2.6.3"),
            Ok(Pattern::Exact(Version::new(2, 6, 3)))
        );
        let prep = parse_pattern("2.6.3-alpha").unwrap();
        assert!(matches!(prep, Pattern::Exact(_)));
    }

    #[test]
    fn rejects_mid_component_wildcards() {
        assert_eq!(
            parse_pattern("*.2.3"),
            Err(SemverError::MidComponentWildcard)
        );
        assert_eq!(
            parse_pattern("1.*.3"),
            Err(SemverError::MidComponentWildcard)
        );
        assert_eq!(parse_pattern("*.2"), Err(SemverError::MidComponentWildcard));
    }

    #[test]
    fn rejects_wildcard_with_prerelease() {
        assert_eq!(
            parse_pattern("2.6.*-alpha"),
            Err(SemverError::PrereleaseInPattern)
        );
        assert_eq!(
            parse_pattern("2.*-beta"),
            Err(SemverError::PrereleaseInPattern)
        );
        assert_eq!(
            parse_pattern("*-rc1"),
            Err(SemverError::PrereleaseInPattern)
        );
    }

    #[test]
    fn rejects_empty_pattern() {
        assert_eq!(parse_pattern(""), Err(SemverError::Empty));
    }

    // ----- Pattern::matches -----

    #[test]
    fn exact_pattern_matches_only_that_version() {
        let p = Pattern::Exact(Version::new(1, 2, 3));
        assert!(p.matches(&Version::new(1, 2, 3)));
        assert!(!p.matches(&Version::new(1, 2, 4)));
        assert!(!p.matches(&Version::new(1, 3, 3)));
    }

    #[test]
    fn exact_pattern_matches_corresponding_prerelease() {
        let p = parse_pattern("1.2.3-alpha").unwrap();
        let v = parse_version("1.2.3-alpha").unwrap();
        assert!(p.matches(&v));
    }

    #[test]
    fn patch_wildcard_matches_any_patch() {
        let p = Pattern::PatchWild { major: 1, minor: 2 };
        assert!(p.matches(&Version::new(1, 2, 0)));
        assert!(p.matches(&Version::new(1, 2, 99)));
        assert!(!p.matches(&Version::new(1, 3, 0)));
        assert!(!p.matches(&Version::new(2, 2, 0)));
    }

    #[test]
    fn minor_wildcard_matches_any_minor_patch() {
        let p = Pattern::MinorWild { major: 1 };
        assert!(p.matches(&Version::new(1, 0, 0)));
        assert!(p.matches(&Version::new(1, 9, 9)));
        assert!(!p.matches(&Version::new(2, 0, 0)));
    }

    #[test]
    fn major_wildcard_matches_any_release() {
        let p = Pattern::MajorWild;
        assert!(p.matches(&Version::new(0, 0, 0)));
        assert!(p.matches(&Version::new(99, 99, 99)));
    }

    #[test]
    fn wildcards_never_match_prereleases() {
        let pre = parse_version("1.2.3-alpha").unwrap();
        assert!(!Pattern::MajorWild.matches(&pre));
        assert!(!Pattern::MinorWild { major: 1 }.matches(&pre));
        assert!(!Pattern::PatchWild { major: 1, minor: 2 }.matches(&pre));
    }

    // ----- Pattern::best_match -----

    #[test]
    fn best_match_picks_highest() {
        let versions = vec![
            Version::new(1, 0, 0),
            Version::new(1, 0, 1),
            Version::new(1, 1, 0),
            Version::new(2, 0, 0),
        ];
        let p = parse_pattern("1.*").unwrap();
        assert_eq!(p.best_match(&versions), Some(&Version::new(1, 1, 0)));
        let p = parse_pattern("1.0.*").unwrap();
        assert_eq!(p.best_match(&versions), Some(&Version::new(1, 0, 1)));
        let p = parse_pattern("*").unwrap();
        assert_eq!(p.best_match(&versions), Some(&Version::new(2, 0, 0)));
    }

    #[test]
    fn best_match_skips_prereleases_under_wildcard() {
        let alpha = parse_version("1.1.0-alpha").unwrap();
        let stable = Version::new(1, 0, 1);
        let versions = vec![alpha.clone(), stable.clone()];
        let p = parse_pattern("1.*").unwrap();
        assert_eq!(p.best_match(&versions), Some(&stable));
    }

    #[test]
    fn best_match_returns_none_when_nothing_matches() {
        let versions = vec![Version::new(2, 0, 0)];
        let p = parse_pattern("1.*").unwrap();
        assert_eq!(p.best_match(&versions), None);
    }

    // ----- ordering -----

    #[test]
    fn release_is_greater_than_its_prereleases() {
        let stable = parse_version("1.2.3").unwrap();
        let alpha = parse_version("1.2.3-alpha").unwrap();
        let beta = parse_version("1.2.3-beta").unwrap();
        assert!(stable > alpha);
        assert!(stable > beta);
        assert!(alpha < beta); // lexicographic
    }

    #[test]
    fn standard_semver_ordering() {
        assert!(Version::new(1, 0, 0) < Version::new(1, 0, 1));
        assert!(Version::new(1, 0, 1) < Version::new(1, 1, 0));
        assert!(Version::new(1, 1, 0) < Version::new(2, 0, 0));
    }

    // ----- display -----

    #[test]
    fn display_roundtrips() {
        for s in ["1.2.3", "0.0.0", "1.2.3-alpha"] {
            let v = parse_version(s).unwrap();
            assert_eq!(v.to_string(), s);
        }
        for s in ["*", "1.*", "1.2.*", "1.2.3"] {
            let p = parse_pattern(s).unwrap();
            assert_eq!(p.to_string(), s);
        }
    }
}
