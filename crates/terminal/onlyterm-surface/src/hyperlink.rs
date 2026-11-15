//! Handling hyperlinks.
//! This gist describes an escape sequence for explicitly managing hyperlinks:
//! <https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda>
//! We use that as the foundation of our hyperlink support, and the game
//! plan is to then implicitly enable the hyperlink attribute for a cell
//! as we recognize linkable input text during print() processing.
use alloc::sync::Arc;
use core::cmp::Reverse;
use core::ops::Range;
use fancy_regex::{Captures, Regex};
use onlyterm_dynamic::{FromDynamic, FromDynamicOptions, ToDynamic, Value};
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

extern crate alloc;
use crate::alloc::borrow::ToOwned;
use crate::alloc::string::ToString;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub use onlyterm_escape_parser::hyperlink::Hyperlink;

/// In addition to handling explicit escape sequences to enable
/// hyperlinks, we also support defining rules that match text
/// from screen lines and generate implicit hyperlinks.  This
/// can be used both for making http URLs clickable and also to
/// make other text clickable.  For example, you might define
/// a rule that makes bug or issue numbers expand to the corresponding
/// URL to view the details for that issue.
/// The Rule struct is configuration that is passed to the terminal
/// and is evaluated when processing mouse hover events.
#[cfg_attr(feature = "use_serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub struct Rule {
    /// The compiled regex for the rule.  This is used to match
    /// against a line of text from the screen (typically the line
    /// over which the mouse is hovering).
    #[cfg_attr(
        feature = "use_serde",
        serde(
            deserialize_with = "deserialize_regex",
            serialize_with = "serialize_regex"
        )
    )]
    #[dynamic(into = "RegexWrap", try_from = "RegexWrap")]
    pub regex: Regex,
    /// The format string that defines how to transform the matched
    /// text into a URL.  For example, a format string of `$0` expands
    /// to the entire matched text, whereas `mailto:$0` expands to
    /// the matched text with a `mailto:` prefix.  More formally,
    /// each instance of `$N` (where N is a number) in the `format`
    /// string is replaced by the capture number N from the regex.
    /// The replacements are carried out in reverse order, starting
    /// with the highest numbered capture first.  This avoids issues
    /// with ambiguous replacement of `$11` vs `$1` in the case of
    /// more complex regexes.
    pub format: String,

    /// Which capture to highlight
    #[dynamic(default)]
    pub highlight: usize,

    /// Text that every match of `regex` must contain. A line without it
    /// cannot match, so the regex is not run on it at all. Purely an
    /// accelerator: results are identical with or without it. Optional in
    /// configuration.
    #[cfg_attr(feature = "use_serde", serde(skip))]
    #[dynamic(default)]
    required_text: Option<String>,
}

struct RegexWrap(Regex);

impl FromDynamic for RegexWrap {
    fn from_dynamic(
        value: &Value,
        options: FromDynamicOptions,
    ) -> Result<RegexWrap, onlyterm_dynamic::Error> {
        let s = String::from_dynamic(value, options)?;
        Ok(RegexWrap(Regex::new(&s).map_err(|e| e.to_string())?))
    }
}

impl From<&Regex> for RegexWrap {
    fn from(regex: &Regex) -> RegexWrap {
        RegexWrap(regex.clone())
    }
}

impl From<RegexWrap> for Regex {
    fn from(value: RegexWrap) -> Self {
        value.0
    }
}

impl ToDynamic for RegexWrap {
    fn to_dynamic(&self) -> Value {
        self.0.to_string().to_dynamic()
    }
}

#[cfg(feature = "use_serde")]
fn deserialize_regex<'de, D>(deserializer: D) -> Result<Regex, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Regex::new(&s).map_err(|e| serde::de::Error::custom(format!("{:?}", e)))
}

#[cfg(feature = "use_serde")]
fn serialize_regex<S>(regex: &Regex, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let s = regex.to_string();
    s.serialize(serializer)
}

/// Holds a resolved rule match.
#[derive(Debug, PartialEq)]
pub struct RuleMatch {
    /// Holds the span (measured in bytes) of the matched text
    pub range: Range<usize>,
    /// Holds the created Hyperlink object that should be associated
    /// the cells that correspond to the span.
    pub link: Arc<Hyperlink>,
}

/// An internal intermediate match result
#[derive(Debug)]
struct Match<'t> {
    rule: &'t Rule,
    captures: Captures<'t>,
}

impl<'t> Match<'t> {
    /// Returns the length of the matched text in bytes (not cells!)
    fn len(&self) -> usize {
        let c0 = self.highlight().unwrap();
        c0.end() - c0.start()
    }

    /// Returns the span of the matched text, measured in bytes (not cells!)
    fn range(&self) -> Range<usize> {
        let c0 = self.highlight().unwrap();
        c0.start()..c0.end()
    }

    fn highlight(&self) -> Option<fancy_regex::Match<'_>> {
        self.captures.get(self.rule.highlight)
    }

    /// Expand replacements in the format string to yield the URL
    /// The replacement is as described on Rule::format.
    fn expand(&self) -> String {
        let mut result = self.rule.format.clone();
        // Start with the highest numbered capture and decrement.
        // This avoids ambiguity when replacing $11 vs $1.
        for n in (0..self.captures.len()).rev() {
            let search = format!("${}", n);
            if let Some(rep) = self.captures.get(n) {
                result = result.replace(&search, rep.as_str());
            } else {
                result = result.replace(&search, "");
            }
        }
        result
    }
}
// The parenthesized tail is matched as either a run of non-paren,
// non-space characters, or a single nested `(...)` group; this keeps
// the match's trailing parenthesis properly balanced against the one
// that opens it, instead of greedily absorbing any `)` that happens to
// follow (which would otherwise swallow an unrelated wrapping
// parenthesis from the surrounding text, e.g. `(see http://example.com/Foo_(Bar))`).
pub const CLOSING_PARENTHESIS_HYPERLINK_PATTERN: &str =
    r"\b\w+://[^\s()]*\((?:[^\s()]|\([^\s()]*\))*\)(?=\s|$|[^_/a-zA-Z0-9-])";
pub const GENERIC_HYPERLINK_PATTERN: &str = r"\b\w+://\S+[_/a-zA-Z0-9-]";

// `fancy_regex::Error` is ~136 bytes; returning it unboxed is a deliberate
// public API choice. Boxing it would add a heap allocation on every failed
// regex construction and change the type callers see, so suppress here.
#[allow(clippy::result_large_err)]
impl Rule {
    /// Construct a new rule.  It may fail if the regex is invalid.
    pub fn new(regex: &str, format: &str) -> Result<Self, fancy_regex::Error> {
        Self::with_highlight(regex, format, 0)
    }

    pub fn with_highlight(
        regex: &str,
        format: &str,
        highlight: usize,
    ) -> Result<Self, fancy_regex::Error> {
        Ok(Self {
            regex: Regex::new(regex)?,
            format: format.to_owned(),
            highlight,
            required_text: None,
        })
    }

    /// Declares text that every match of this rule's regex contains, so
    /// lines without it skip the regex. Only declare text that really is
    /// required: a wrong declaration makes the rule miss matches.
    ///
    /// Worth it for patterns that start with `\b\w+` (URLs, e-mail
    /// addresses): on a line without a link the regex engine still tries
    /// every word start, which costs ~10 us per 120-column line, and falls
    /// back to a slower engine when the line has non-ASCII text.
    pub fn requiring_text(mut self, text: &str) -> Self {
        self.required_text = Some(text.to_owned());
        self
    }

    /// See [`Rule::requiring_text`].
    pub fn required_text(&self) -> Option<&str> {
        self.required_text.as_deref()
    }

    /// Given a line of text from the terminal screen, and a set of
    /// rules, return the set of RuleMatches.
    pub fn match_hyperlinks(line: &str, rules: &[Rule]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        for rule in rules.iter() {
            if let Some(required) = rule.required_text.as_deref() {
                if !line.contains(required) {
                    continue;
                }
            }
            for captures in rule.regex.captures_iter(line).flatten() {
                let m = Match { rule, captures };
                if m.highlight().is_some() {
                    matches.push(m);
                }
            }
        }
        // Sort the matches by descending match length.
        // This is to avoid confusion if multiple rules match the
        // same sections of text.
        matches.sort_by_key(|m| Reverse(m.len()));

        matches
            .into_iter()
            .map(|m| {
                let url = m.expand();
                let link = Arc::new(Hyperlink::new_implicit(url));
                RuleMatch {
                    link,
                    range: m.range(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_implicit() {
        let rules = vec![
            Rule::new(r"\b\w+://(?:[\w.-]+)\.[a-z]{2,15}\S*\b", "$0").unwrap(),
            Rule::new(r"\b\w+@[\w-]+(\.[\w-]+)+\b", "mailto:$0").unwrap(),
        ];

        assert_eq!(
            Rule::match_hyperlinks("  http://example.com", &rules),
            vec![RuleMatch {
                range: 2..20,
                link: Arc::new(Hyperlink::new_implicit("http://example.com")),
            }]
        );

        assert_eq!(
            Rule::match_hyperlinks("  foo@example.com woot@example.com", &rules),
            vec![
                // Longest match first
                RuleMatch {
                    range: 18..34,
                    link: Arc::new(Hyperlink::new_implicit("mailto:woot@example.com")),
                },
                RuleMatch {
                    range: 2..17,
                    link: Arc::new(Hyperlink::new_implicit("mailto:foo@example.com")),
                },
            ]
        );
    }

    #[test]
    fn parse_with_parentheses() {
        fn assert_helper(test_uri: &str, expected_uri: &str, msg: &str) {
            let rules = vec![
                Rule::new(CLOSING_PARENTHESIS_HYPERLINK_PATTERN, "$0").unwrap(),
                Rule::new(GENERIC_HYPERLINK_PATTERN, "$0").unwrap(),
            ];

            assert_eq!(
                Rule::match_hyperlinks(test_uri, &rules)[0].link.uri(),
                expected_uri,
                "{}",
                msg,
            );
        }

        assert_helper(
            "   http://example.com)",
            "http://example.com",
            "Unblanced terminating parenthesis should not be captured.",
        );

        assert_helper(
            "http://example.com/(complete_parentheses)",
            "http://example.com/(complete_parentheses)",
            "Balanced terminating parenthesis should be captureed.",
        );

        assert_helper(
            "http://example.com/(complete_parentheses)>",
            "http://example.com/(complete_parentheses)",
            "Non-URL characters after a balanced terminating parenthesis should be dropped.",
        );

        assert_helper(
            "http://example.com/(complete_parentheses))",
            "http://example.com/(complete_parentheses)",
            "A second, unbalanced trailing parenthesis should not be captured, consistent with the single-dangling-parenthesis case above.",
        );

        assert_helper(
            "http://example.com/(complete_parentheses)-((-)-()-_-",
            "http://example.com/(complete_parentheses)-((-)-()-_-",
            "Non-terminating parentheses should not impact matching the entire URL - Terminated with a valid character",
        );
    }

    #[test]
    fn parse_url_wrapped_in_surrounding_parentheses() {
        // Regression test: a bare URL that is wrapped in a pair of
        // parentheses that belong to the *surrounding text* (not the URL
        // itself) must not have the closing parenthesis captured as part
        // of the URL. This mirrors onlyterm_config::default_hyperlink_rules(),
        // which layers a `\(...\)`-stripping rule ahead of
        // CLOSING_PARENTHESIS_HYPERLINK_PATTERN / GENERIC_HYPERLINK_PATTERN.
        fn rules() -> Vec<Rule> {
            vec![
                Rule::with_highlight(r"\((\w+://\S+)\)", "$1", 1).unwrap(),
                Rule::with_highlight(r"\[(\w+://\S+)\]", "$1", 1).unwrap(),
                Rule::with_highlight(r"<(\w+://\S+)>", "$1", 1).unwrap(),
                Rule::new(CLOSING_PARENTHESIS_HYPERLINK_PATTERN, "$0").unwrap(),
                Rule::new(GENERIC_HYPERLINK_PATTERN, "$0").unwrap(),
            ]
        }

        fn assert_helper(test_uri: &str, expected_uri: &str, msg: &str) {
            assert_eq!(
                Rule::match_hyperlinks(test_uri, &rules())[0].link.uri(),
                expected_uri,
                "{}",
                msg,
            );
        }

        assert_helper(
            "see (http://example.com/foo) for details",
            "http://example.com/foo",
            "Wrapping parenthesis from surrounding text should not be captured.",
        );

        // The URL itself legitimately contains a balanced parenthesis pair
        // (as in Wikipedia-style URLs), and is *also* wrapped in an outer
        // pair of parentheses that belong to the surrounding sentence.
        // Only the inner, URL-owned parenthesis should be kept.
        assert_helper(
            "see (http://en.wikipedia.org/wiki/Bar_(foo)) for details",
            "http://en.wikipedia.org/wiki/Bar_(foo)",
            "Outer wrapping parenthesis must be dropped even when the URL has its own balanced parenthesis.",
        );

        // Sanity check: without any outer wrapping, a URL-owned balanced
        // parenthesis pair must still be preserved in full.
        assert_helper(
            "http://en.wikipedia.org/wiki/Bar_(foo)",
            "http://en.wikipedia.org/wiki/Bar_(foo)",
            "A URL's own balanced parenthesis must be preserved when there is no outer wrapping.",
        );
    }

    /// The default rule set as configured in `onlyterm_config`, with and
    /// without the required-text hints.
    fn default_rules(hints: bool) -> Vec<Rule> {
        let hint = |rule: Rule, text: &str| {
            if hints {
                rule.requiring_text(text)
            } else {
                rule
            }
        };
        vec![
            hint(
                Rule::with_highlight(r"\((\w+://\S+)\)", "$1", 1).unwrap(),
                "://",
            ),
            hint(
                Rule::with_highlight(r"\[(\w+://\S+)\]", "$1", 1).unwrap(),
                "://",
            ),
            hint(
                Rule::with_highlight(r"<(\w+://\S+)>", "$1", 1).unwrap(),
                "://",
            ),
            hint(
                Rule::new(CLOSING_PARENTHESIS_HYPERLINK_PATTERN, "$0").unwrap(),
                "://",
            ),
            hint(Rule::new(GENERIC_HYPERLINK_PATTERN, "$0").unwrap(), "://"),
            hint(
                Rule::new(r"\b\w+@[\w-]+(\.[\w-]+)+\b", "mailto:$0").unwrap(),
                "@",
            ),
        ]
    }

    #[test]
    fn required_text_never_changes_what_matches() {
        let hinted = default_rules(true);
        let plain = default_rules(false);
        let lines = [
            "",
            "plain text with no link at all",
            "status=busy worker=042 redraw payload \u{754c}\u{754c}\u{754c}",
            "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442} \u{43c}\u{438}\u{440}, \u{43d}\u{435}\u{442} \u{441}\u{441}\u{44b}\u{43b}\u{43e}\u{43a}",
            "see http://example.com for details",
            "  http://example.com",
            "(http://example.com) and [https://example.org/path] and <ftp://host.example.net/x>",
            "http://example.com/(complete_parentheses)-((-)-()-_-",
            "(see http://example.com/Foo_(Bar))",
            "mail foo@example.com or woot@example.com now",
            "a@b is not an address, but a@b.co is",
            "scheme:// alone and ://x and ftp:/missing",
            "user@host and https://example.com/a@b.c together",
            "\u{754c}http://example.com\u{754c}",
            "@://@://",
        ];
        for line in lines {
            assert_eq!(
                Rule::match_hyperlinks(line, &hinted),
                Rule::match_hyperlinks(line, &plain),
                "hints changed the result for {:?}",
                line
            );
        }
    }

    #[test]
    fn a_line_without_the_required_text_skips_its_rule() {
        // `foo` matches this regex, but the declared text is absent, so the
        // rule is skipped: proof that the hint is what gates the regex.
        let gated = Rule::new(r"\bfoo\b", "$0").unwrap().requiring_text("bar");
        assert!(Rule::match_hyperlinks("foo", std::slice::from_ref(&gated)).is_empty());
        assert_eq!(Rule::match_hyperlinks("foo bar", &[gated]).len(), 1);
        let open = Rule::new(r"\bfoo\b", "$0").unwrap();
        assert_eq!(Rule::match_hyperlinks("foo", &[open]).len(), 1);
    }

    #[test]
    fn every_hint_of_the_default_rules_appears_in_its_pattern() {
        for rule in default_rules(true) {
            let hint = rule.required_text().expect("default rules carry a hint");
            assert!(
                rule.regex.as_str().contains(hint),
                "{:?} does not contain its hint {:?}",
                rule.regex.as_str(),
                hint
            );
        }
    }
}
