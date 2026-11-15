//! Release tags and how they order, for the update check.
//!
//! Kept free of I/O so it can be unit tested.

use std::cmp::Ordering;

/// A build or release tag such as `v0.0.44-alpha`.
///
/// `git describe` output between two tags (`v0.0.42-alpha-19-g605b335f3`)
/// carries a commit-distance suffix; it is dropped, so such a build counts as
/// its nearest tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseVersion {
    /// Dotted numbers with trailing zeros removed, so `1.2` equals `1.2.0`.
    numbers: Vec<u64>,
    /// Text after the numbers, for example `alpha`; `None` for a stable release.
    pre_release: Option<String>,
}

impl ReleaseVersion {
    /// `None` for anything that is not `[v]N.N[.N...][-suffix]`: nightly
    /// date tags, `unknown`, a bare commit hash.
    pub fn parse(tag: &str) -> Option<Self> {
        let tag = tag.trim();
        let tag = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
        let mut parts: Vec<&str> = tag.split('-').collect();

        if let [.., distance, hash] = parts.as_slice() {
            let is_distance = !distance.is_empty() && distance.bytes().all(|b| b.is_ascii_digit());
            let is_hash = hash.len() > 1
                && hash.starts_with('g')
                && hash[1..].bytes().all(|b| b.is_ascii_hexdigit());
            if is_distance && is_hash {
                parts.truncate(parts.len() - 2);
            }
        }

        let (numbers, suffix) = parts.split_first()?;
        let mut numbers = numbers
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect::<Option<Vec<u64>>>()?;
        if numbers.len() < 2 {
            return None;
        }
        while numbers.last() == Some(&0) {
            numbers.pop();
        }

        let pre_release = (!suffix.is_empty()).then(|| suffix.join("-"));
        Some(Self {
            numbers,
            pre_release,
        })
    }
}

impl Ord for ReleaseVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let len = self.numbers.len().max(other.numbers.len());
        for index in 0..len {
            let mine = self.numbers.get(index).copied().unwrap_or(0);
            let theirs = other.numbers.get(index).copied().unwrap_or(0);
            match mine.cmp(&theirs) {
                Ordering::Equal => {}
                unequal => return unequal,
            }
        }
        match (&self.pre_release, &other.pre_release) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(mine), Some(theirs)) => mine.cmp(theirs),
        }
    }
}

impl PartialOrd for ReleaseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Whether the release tagged `latest` is newer than the running build.
/// A tag that cannot be parsed never counts as newer: no nagging on doubt.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (
        ReleaseVersion::parse(latest),
        ReleaseVersion::parse(current),
    ) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// The item with the highest version tag. Items whose tag cannot be parsed
/// are skipped.
pub fn newest_by_tag<T>(items: impl IntoIterator<Item = T>, tag: impl Fn(&T) -> &str) -> Option<T> {
    items
        .into_iter()
        .filter_map(|item| ReleaseVersion::parse(tag(&item)).map(|version| (version, item)))
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, item)| item)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(tag: &str) -> ReleaseVersion {
        ReleaseVersion::parse(tag).unwrap_or_else(|| panic!("{} should parse", tag))
    }

    #[test]
    fn parses_release_and_describe_tags() {
        assert_eq!(version("v0.0.44-alpha"), version("0.0.44-alpha"));
        assert_eq!(
            version("v0.0.42-alpha-19-g605b335f3"),
            version("v0.0.42-alpha")
        );
        assert_eq!(version("v1.2-3-gabcdef0"), version("v1.2"));
    }

    #[test]
    fn rejects_tags_that_are_not_versions() {
        for tag in [
            "",
            "unknown",
            "nightly",
            "20240203-110809-5046fc22",
            "v1",
            "v1.x.2",
            "605b335f3",
        ] {
            assert!(ReleaseVersion::parse(tag).is_none(), "{:?}", tag);
        }
    }

    #[test]
    fn compares_numbers_not_text() {
        assert!(version("v0.0.100-alpha") > version("v0.0.44-alpha"));
        assert!(version("v0.0.44-alpha") > version("v0.0.9-alpha"));
        assert!(version("v0.1.0-alpha") > version("v0.0.99-alpha"));
        assert!(version("v1.0.0-alpha") > version("v0.99.99-alpha"));
    }

    #[test]
    fn trailing_zeros_do_not_change_the_version() {
        assert_eq!(version("v1.2"), version("v1.2.0"));
        assert_eq!(version("v1.2.0.0").cmp(&version("v1.2")), Ordering::Equal);
        assert!(version("v1.2.1") > version("v1.2"));
    }

    #[test]
    fn stable_beats_its_own_pre_release() {
        assert!(version("v1.0.0") > version("v1.0.0-alpha"));
        assert!(version("v1.0.0-beta") > version("v1.0.0-alpha"));
        assert!(version("v1.0.1-alpha") > version("v1.0.0"));
    }

    #[test]
    fn newer_means_strictly_newer() {
        assert!(is_newer("v0.0.45-alpha", "v0.0.44-alpha"));
        assert!(!is_newer("v0.0.44-alpha", "v0.0.44-alpha"));
        assert!(!is_newer("v0.0.43-alpha", "v0.0.44-alpha"));
    }

    #[test]
    fn a_build_past_a_tag_is_not_older_than_that_tag() {
        assert!(!is_newer("v0.0.44-alpha", "v0.0.44-alpha-12-gdeadbeef"));
        assert!(is_newer("v0.0.44-alpha", "v0.0.42-alpha-19-g605b335f3"));
    }

    #[test]
    fn unparseable_tags_never_trigger_an_update() {
        assert!(!is_newer("nightly", "v0.0.44-alpha"));
        assert!(!is_newer("v0.0.45-alpha", "unknown"));
        assert!(!is_newer("", ""));
    }

    #[test]
    fn newest_by_tag_picks_the_highest_version_and_skips_junk() {
        let tags = ["v0.0.9-alpha", "nightly", "v0.0.100-alpha", "v0.0.44-alpha"];
        let newest = newest_by_tag(tags, |tag| *tag);
        assert_eq!(newest, Some("v0.0.100-alpha"));
        assert_eq!(newest_by_tag(["nightly", "latest"], |tag| *tag), None);
        assert_eq!(newest_by_tag(Vec::<&str>::new(), |tag| *tag), None);
    }
}
