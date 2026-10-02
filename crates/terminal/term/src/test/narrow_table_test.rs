//! Soundness tests for `is_narrow_table_char`, the codepoint allow-list
//! behind the narrow bulk-print fast path
//! (`Performer::narrow_bulk_run_len`). For EVERY char in the table this
//! verifies, using the same `grapheme_column_width` and unicode version
//! the performer uses:
//!
//! 1. width == 1;
//! 2. no grapheme fusion: `Graphemes` segmentation of `a{c}a`, `{c}{c}`,
//!    `a{c}`, `{c}a`, `{c}` and `x{c}` yields exactly one grapheme per
//!    char;
//! 3. NFC stability: the char is unchanged by NFC, and NFC of
//!    `a{c}a`/`{c}{c}`/`{c}a`/`a{c}` leaves the same codepoints in the
//!    same order (no re-composition or re-ordering).
//!
//! If a range fails for some chars, the range must be narrowed in the
//! table (not the test).
use crate::terminal::terminalstate::performer::is_narrow_table_char;
use finl_unicode::grapheme_clusters::Graphemes;
use k9::assert_equal as assert_eq;
use onlyterm_cell::{grapheme_column_width, UnicodeVersion, LATEST_UNICODE_VERSION};
use unicode_normalization::UnicodeNormalization;

const TABLE_RANGES: &[(u32, u32)] = &[
    (0x00a1, 0x00ac),
    (0x00ae, 0x00ff),
    (0x0100, 0x024f),
    (0x0384, 0x0386),
    (0x0388, 0x03ff),
    (0x0400, 0x0481),
    (0x048a, 0x04ff),
    (0x2010, 0x2027),
    (0x2030, 0x205e),
    (0x2500, 0x259f),
];

/// Every codepoint in TABLE_RANGES is in the table and vice versa.
#[test]
fn table_matches_documented_ranges() {
    let mut covered = std::collections::BTreeSet::new();
    for &(lo, hi) in TABLE_RANGES {
        for cp in lo..=hi {
            covered.insert(cp);
        }
    }
    // Scan the whole BMP: any table member outside the documented ranges
    // is a documentation bug.
    for cp in 0u32..0x1_0000 {
        let c = match char::from_u32(cp) {
            Some(c) => c,
            None => continue,
        };
        assert_eq!(
            is_narrow_table_char(c),
            covered.contains(&cp),
            "table membership mismatch for U+{:04X}",
            cp
        );
    }
}

#[test]
fn narrow_table_soundness() {
    // Collect all offenders first so one run reports every char that must
    // be dropped from the table, not just the first.
    let mut failures: Vec<String> = Vec::new();
    let versions = [
        LATEST_UNICODE_VERSION,
        UnicodeVersion::new(8),
        UnicodeVersion::new(9),
        UnicodeVersion::new(14),
    ];

    for &(lo, hi) in TABLE_RANGES {
        for cp in lo..=hi {
            let c = match char::from_u32(cp) {
                Some(c) => c,
                None => continue,
            };
            assert!(is_narrow_table_char(c), "U+{:04X} not in table?", cp);
            let s = c.to_string();

            for version in &versions {
                if grapheme_column_width(&s, Some(version)) != 1 {
                    failures.push(format!(
                        "U+{:04X} width != 1 (version {})",
                        cp, version.version
                    ));
                }
            }

            // No grapheme fusion next to ASCII or fellow table members.
            for context in [
                format!("a{}a", s),
                format!("{}{}", s, s),
                format!("a{}", s),
                format!("{}a", s),
                s.clone(),
                format!("x{}", s),
            ] {
                let count = Graphemes::new(context.as_str()).count();
                let expected = context.chars().count();
                if count != expected {
                    failures.push(format!(
                        "U+{:04X} fuses with neighbours in {:?} ({} graphemes != {} chars)",
                        cp, context, count, expected
                    ));
                }
            }

            // NFC stability: the char alone, and adjacent to ASCII and
            // fellow table members, is unchanged by NFC.
            let nfc_s: String = s.nfc().collect();
            if nfc_s != s {
                failures.push(format!(
                    "U+{:04X} is not NFC-stable in isolation ({:?} -> {:?})",
                    cp, s, nfc_s
                ));
            }
            for context in [
                format!("a{}a", s),
                format!("{}{}", s, s),
                format!("{}a", s),
                format!("a{}", s),
            ] {
                let nfc: String = context.nfc().collect();
                if nfc != context {
                    failures.push(format!(
                        "U+{:04X} changes/reorders under NFC in {:?} -> {:?}",
                        cp, context, nfc
                    ));
                }
            }
        }
    }
    assert_eq!(
        failures,
        Vec::<String>::new(),
        "narrow table offenders found; drop them from is_narrow_table_char"
    );
}
