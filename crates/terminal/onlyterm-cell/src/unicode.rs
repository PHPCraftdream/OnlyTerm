#[cfg(feature = "std")]
use alloc::sync::Arc;
use finl_unicode::grapheme_clusters::Graphemes;
use onlyterm_char_props::emoji::Presentation;
use onlyterm_char_props::emoji_variation::WCWIDTH_TABLE;
use onlyterm_char_props::widechar_width::WcWidth;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnicodeVersion {
    pub version: u8,
    pub ambiguous_are_wide: bool,
    #[cfg(feature = "std")]
    pub cell_widths: Option<Arc<std::collections::HashMap<u32, u8>>>,
}

impl UnicodeVersion {
    pub const fn new(version: u8) -> Self {
        Self {
            version,
            ambiguous_are_wide: false,
            #[cfg(feature = "std")]
            cell_widths: None,
        }
    }

    #[inline]
    fn width(&self, c: WcWidth) -> usize {
        // Special case for symbol fonts that are naughtly and use
        // the unassigned range instead of the private use range.
        // <https://github.com/wezterm/wezterm/issues/1864>
        if c == WcWidth::Unassigned {
            1
        } else if c == WcWidth::Ambiguous && self.ambiguous_are_wide {
            2
        } else if self.version >= 9 {
            c.width_unicode_9_or_later() as usize
        } else {
            c.width_unicode_8_or_earlier() as usize
        }
    }

    #[inline]
    fn wcwidth(&self, c: char) -> usize {
        #[cfg(feature = "std")]
        if let Some(width) = self
            .cell_widths
            .as_ref()
            .and_then(|cell_widths| cell_widths.get(&(c as u32)))
        {
            return (*width).into();
        }
        self.width(WCWIDTH_TABLE.classify(c))
    }

    #[inline]
    pub fn idx(&self) -> usize {
        (if self.version > 9 { 2 } else { 0 }) | (if self.ambiguous_are_wide { 1 } else { 0 })
    }
}

pub const LATEST_UNICODE_VERSION: UnicodeVersion = UnicodeVersion {
    version: 14,
    ambiguous_are_wide: false,
    #[cfg(feature = "std")]
    cell_widths: None,
};

/// Returns true if the char `c` has the unicode White_Space property
pub fn is_white_space_char(c: char) -> bool {
    onlyterm_char_props::white_space::WHITE_SPACE.contains_u32(c as u32)
}

/// Returns true if the grapheme string `g` consists entirely of characters
/// that have the unicode White_Space property.
pub fn is_white_space_grapheme(g: &str) -> bool {
    for c in g.chars() {
        if !is_white_space_char(c) {
            return false;
        }
    }
    true
}

/// Returns the number of cells visually occupied by a sequence
/// of graphemes.
/// Calls through to `grapheme_column_width` for each grapheme
/// and sums up the length.
pub fn unicode_column_width(s: &str, version: Option<&UnicodeVersion>) -> usize {
    Graphemes::new(s)
        .map(|g| grapheme_column_width(g, version))
        .sum()
}

/// Returns the number of cells visually occupied by a grapheme.
/// The input string must be a single grapheme.
///
/// There are some frustrating dragons in the realm of terminal cell widths:
///
/// a) wcwidth and wcswidth are widely used by applications and may be
///    several versions of unicode behind the current version
/// b) The width of characters has and will change in the future.
///    Unicode Version 8 -> 9 made some characters wider.
///    Unicode 14 defines Emoji variation selectors that change the
///    width depending on trailing context in the unicode sequence.
///
/// Differing opinions about the width leads to visual artifacts in
/// text and and line editors, especially with respect to cursor placement.
///
/// There aren't any really great solutions to this problem, as a given
/// terminal emulator may be fine locally but essentially breaks when
/// ssh'ing into a remote system with a divergent wcwidth implementation.
///
/// This means that a global understanding of the unicode version that
/// is in use isn't a good solution.
///
/// The approach that onlyterm wants to take here is to define a
/// configuration value that sets the starting level of unicode conformance,
/// and to define an escape sequence that can push/pop a desired confirmance
/// level onto a stack maintained by the terminal emulator.
///
/// The terminal emulator can then pass the unicode version through to
/// the Cell that is used to hold a grapheme, and that per-Cell version
/// can then be used to calculate width.
pub fn grapheme_column_width(s: &str, version: Option<&UnicodeVersion>) -> usize {
    let version = version.unwrap_or(&LATEST_UNICODE_VERSION);

    // Optimization: if there is a single byte we can directly cast
    // that byte as a char which will be in the range 0.255.
    // This takes ~1.5ns, and we can then look that up in the table
    // which is valid for chars in the range 0-0xffff.
    // That lookup also takes ~1.5ns, giving us a hot path latency
    // of ~3-4ns for a grapheme string that is comprised of a single
    // ASCII byte.
    //
    // Since we know this is a single ASCII char, we know that it
    // cannot be a sequence with a variation selector, so we don't
    // need to requested `Presentation` for it.
    if s.len() == 1 {
        return version.wcwidth(s.as_bytes()[0] as char);
    }

    // Slow path: `s.chars()` will dominate and pull up the minimum
    // runtime to ~20ns
    if version.version >= 14 {
        // Lookup the grapheme to see if the presentation of
        // the grapheme forces the width. We can bypass
        // the WcWidth classification if that is true.
        match Presentation::for_grapheme(s) {
            (_, Some(Presentation::Emoji)) => return 2,
            (_, Some(Presentation::Text)) => return 1,
            (Presentation::Emoji, None) => return 2,
            (Presentation::Text, None) => {}
        }
    }

    // Otherwise, classify and sum up
    let mut width = 0;
    for c in s.chars() {
        width += version.wcwidth(c);
    }

    width.min(2)
}
