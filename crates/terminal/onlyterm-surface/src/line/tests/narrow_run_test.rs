#![cfg(test)]
//! Differential test for `Line::set_narrow_run` (and
//! `ClusteredLine::append_narrow_run`), the bulk API backing the `term`
//! crate's narrow bulk-print fast path: a randomized comparison of one
//! bulk call against the per-char `set_cell_grapheme` loop, over both V
//! and clustered storage, with interior overwrites (including over wide
//! cells), implicit-blank padding past the end, and attributes including
//! hyperlinks. Deterministic LCG; the seed is printed on failure.
//!
//! Lives as a child of `crate::line::line` (registered from
//! `line/line/mod.rs`, like `ascii_run_test.rs`) so it can read `Line`'s
//! private `bits` field directly.

use super::*;
use crate::hyperlink::Hyperlink;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use k9::assert_equal as assert_eq;
use onlyterm_cell::color::AnsiColor;
use onlyterm_cell::{grapheme_column_width, Intensity};

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_u64() % bound as u64) as usize
        }
    }
}

/// Single-cell chars from the performer's narrow table (Cyrillic, Greek,
/// Latin-1/Extended, box drawing, punctuation) plus ASCII.
const NARROW_CHARS: &[char] = &[
    'a', 'Z', '0', '9', '!', ' ', '\u{410}', '\u{44f}', '\u{401}', '\u{451}', '\u{03b1}',
    '\u{03a9}', '\u{00e9}', '\u{00fc}', '\u{00df}', '\u{0100}', '\u{024f}', '\u{2500}', '\u{2593}',
    '\u{2014}', '\u{00bb}',
];

type Piece = (&'static str, usize, CellAttributes);

/// a=0 日=1-2 本=3-4 b=5 c=6 d=7; len=8.
fn pieces() -> Vec<Piece> {
    let hyperlink = Arc::new(Hyperlink::new("http://example.com/narrow-run-base"));
    let link_attr = CellAttributes::default()
        .set_hyperlink(Some(hyperlink))
        .clone();
    vec![
        ("a", 1, CellAttributes::default()),
        (
            "日",
            2,
            CellAttributes::default()
                .set_background(AnsiColor::Blue)
                .clone(),
        ),
        ("本", 2, CellAttributes::default()),
        ("b", 1, link_attr),
        ("c", 1, CellAttributes::default()),
        ("d", 1, CellAttributes::default()),
    ]
}

fn build_v_line(pieces: &[Piece], seqno: SequenceNo) -> Line {
    let mut cells = Vec::new();
    for (text, width, attrs) in pieces {
        cells.push(Cell::new_grapheme_with_width(text, *width, attrs.clone()));
        for _ in 1..*width {
            cells.push(Cell::new(' ', attrs.clone()));
        }
    }
    Line::from_cells(cells, seqno)
}

fn build_c_line(pieces: &[Piece], seqno: SequenceNo) -> Line {
    let mut line = Line::new(seqno);
    for (text, width, attrs) in pieces {
        let idx = line.len();
        line.set_cell_grapheme(idx, text, *width, attrs.clone(), seqno);
    }
    line
}

#[derive(Debug, PartialEq)]
struct Observed {
    text: String,
    cells: Vec<(usize, usize, CellAttributes)>,
    len: usize,
    has_hyperlink: bool,
    bits: u16,
}

fn observe(line: &Line) -> Observed {
    Observed {
        text: line.as_str().to_string(),
        cells: line
            .visible_cells()
            .map(|c| (c.cell_index(), c.width(), c.attrs().clone()))
            .collect(),
        len: line.len(),
        has_hyperlink: line.has_hyperlink(),
        bits: line.bits.bits(),
    }
}

/// Reference: write `text` one char at a time via `set_cell_grapheme`.
fn reference_narrow_run(
    line: &mut Line,
    idx: usize,
    text: &str,
    attr: &CellAttributes,
    seqno: SequenceNo,
) {
    for (i, c) in text.chars().enumerate() {
        let ch = c.to_string();
        line.set_cell_grapheme(idx + i, &ch, 1, attr.clone(), seqno);
    }
}

fn random_run(rng: &mut Lcg, min: usize, max: usize) -> String {
    let len = min + rng.below(max - min + 1);
    let mut s = String::new();
    for _ in 0..len {
        s.push(NARROW_CHARS[rng.below(NARROW_CHARS.len())]);
    }
    s
}

#[test]
fn set_narrow_run_matches_per_cell_loop_random() {
    let base_pieces = pieces();
    let base_v = build_v_line(&base_pieces, SEQ_ZERO);
    let base_c = build_c_line(&base_pieces, SEQ_ZERO);

    let hyperlink = Arc::new(Hyperlink::new("http://example.com/narrow-run-write"));
    let run_attrs = [
        CellAttributes::default(),
        CellAttributes::default()
            .set_intensity(Intensity::Bold)
            .clone(),
        CellAttributes::blank(),
        CellAttributes::default()
            .set_hyperlink(Some(Arc::clone(&hyperlink)))
            .clone(),
    ];

    for seed in 0..300u64 {
        let mut rng = Lcg::new(seed);
        let run = random_run(&mut rng, 1, 6);
        let idx = rng.below(12);
        let attr = &run_attrs[rng.below(run_attrs.len())];
        let (label, base) = if rng.below(2) == 0 {
            ("v", &base_v)
        } else {
            ("c", &base_c)
        };

        let mut under_test = base.clone();
        let mut reference = base.clone();
        under_test.set_narrow_run(idx, &run, attr, SEQ_ZERO + 1);
        reference_narrow_run(&mut reference, idx, &run, attr, SEQ_ZERO + 1);

        assert_eq!(
            observe(&under_test),
            observe(&reference),
            "storage={} seed={} idx={} run={:?} attr={:?}",
            label,
            seed,
            idx,
            run,
            attr
        );
    }
}

/// Appending exactly at the end of clustered storage must take the
/// `append_narrow_run` fast path and stay clustered, producing exactly
/// what repeated `append_grapheme(c, 1, attr)` produces.
#[test]
fn set_narrow_run_append_at_end_stays_clustered() {
    let mut line = build_c_line(&pieces(), SEQ_ZERO);
    assert!(matches!(line.cells, CellStorage::C(_)));

    let run = "\u{410}\u{443}\u{2500}x";
    let len = line.len();
    line.set_narrow_run(len, run, &CellAttributes::default(), SEQ_ZERO + 1);

    assert!(
        matches!(line.cells, CellStorage::C(_)),
        "appending exactly at the end of clustered storage must stay clustered"
    );

    let mut reference = build_c_line(&pieces(), SEQ_ZERO);
    let start = reference.len();
    for (i, c) in run.chars().enumerate() {
        reference.set_cell_grapheme(
            start + i,
            &c.to_string(),
            1,
            CellAttributes::default(),
            SEQ_ZERO + 1,
        );
    }
    assert_eq!(observe(&line), observe(&reference));
    assert_eq!(line.as_str(), "a日本bcd\u{410}\u{443}\u{2500}x");
}

/// Past-the-end writes pad with implicit blanks and stay clustered.
#[test]
fn set_narrow_run_past_end_pads_with_blanks_and_stays_clustered() {
    let mut line = build_c_line(&pieces(), SEQ_ZERO);
    let len = line.len();

    line.set_narrow_run(len + 3, "\u{44e}", &CellAttributes::default(), SEQ_ZERO + 1);

    assert!(matches!(line.cells, CellStorage::C(_)));
    assert_eq!(line.as_str(), "a日本bcd   \u{44e}");
}

/// Interior writes coerce clustered storage to Vec, like the per-cell loop.
#[test]
fn set_narrow_run_interior_write_coerces_clustered_to_vec() {
    let mut line = build_c_line(&pieces(), SEQ_ZERO);
    assert!(matches!(line.cells, CellStorage::C(_)));

    line.set_narrow_run(
        5,
        "\u{416}\u{43b}",
        &CellAttributes::default(),
        SEQ_ZERO + 1,
    );

    assert!(matches!(line.cells, CellStorage::V(_)));
    assert_eq!(line.as_str(), "a日本\u{416}\u{43b}d");
}

/// A run whose attributes carry a hyperlink must set `HAS_HYPERLINK`.
#[test]
fn set_narrow_run_sets_has_hyperlink_bit() {
    let plain_pieces: Vec<Piece> = vec![
        ("a", 1, CellAttributes::default()),
        ("日", 2, CellAttributes::default()),
        ("b", 1, CellAttributes::default()),
    ];
    let hyperlink = Arc::new(Hyperlink::new("http://example.com/narrow-run-bit"));
    let attr = CellAttributes::default()
        .set_hyperlink(Some(hyperlink))
        .clone();

    for mut line in [
        build_v_line(&plain_pieces, SEQ_ZERO),
        build_c_line(&plain_pieces, SEQ_ZERO),
    ] {
        assert!(!line.has_hyperlink());
        let idx = line.len();
        line.set_narrow_run(idx, "\u{410}\u{431}", &attr, SEQ_ZERO + 1);
        assert!(line.has_hyperlink());
    }
}

/// Every char used by the random test must genuinely be single-cell under
/// the default unicode version (guards against the test vector itself
/// drifting out of the narrow set).
#[test]
fn random_test_charset_is_single_cell() {
    for c in NARROW_CHARS {
        assert_eq!(
            grapheme_column_width(&c.to_string(), None),
            1,
            "test charset char U+{:04X} must be width 1",
            *c as u32
        );
    }
}
