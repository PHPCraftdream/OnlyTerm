#![cfg(test)]
//! Differential tests for `ClusteredLine`'s grapheme iteration: the ASCII
//! fast path in `next_grapheme_at` (used by `ClusterLineCellIter` and by
//! `ClusteredLine::truncate`) must produce byte-for-byte identical
//! grapheme boundaries to plain `finl_unicode` segmentation, for every
//! text this crate could plausibly build a clustered line from -- plus a
//! few pathological cases the fast path's own correctness argument leans
//! on (ASCII immediately before a combining mark, CRLF, regional
//! indicators, ZWJ sequences, VS16, Indic conjuncts).
//!
//! Lives as a child of `crate::line::clusterline` (registered from
//! `clusterline.rs`) so it can read `ClusteredLine`'s private `text` and
//! `clusters` fields to build a reference traversal that never takes the
//! fast path, the same way `line/tests/fill_range_test.rs` reads `Line`'s
//! private fields from `line/line/mod.rs`.

use super::*;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use k9::assert_equal as assert_eq;
use onlyterm_cell::color::AnsiColor;
use onlyterm_cell::Intensity;

/// Small deterministic PRNG so failures are reproducible without pulling
/// in an external `rand` dependency (same recipe as `fill_range_test.rs`).
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

    fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next_u64() as usize) % items.len()]
    }
}

/// Units that are each a single extended grapheme cluster, covering the
/// categories the fast path's reasoning depends on: printable ASCII,
/// ASCII base + combining mark (Extend), CJK (wide), a ZWJ emoji
/// sequence, a regional-indicator flag pair, an emoji + VS16 variation
/// selector, and a Devanagari conjunct (GB9c).
fn grapheme_units() -> Vec<&'static str> {
    vec![
        "a",
        "Z",
        "0",
        "!",
        " ",
        "~",
        "e\u{0301}",
        "o\u{0308}",
        "日",
        "本",
        "語",
        "\u{1f468}\u{1f3fe}\u{200d}\u{1f9b0}",
        "\u{1f1fa}\u{1f1f8}",
        "\u{2764}\u{fe0f}",
        "\u{0915}\u{094d}\u{0937}",
    ]
}

fn random_text(seed: u64, units: usize) -> String {
    let mut lcg = Lcg::new(seed);
    let pool = grapheme_units();
    let mut s = String::new();
    for _ in 0..units {
        s.push_str(lcg.choose(&pool));
    }
    s
}

/// Plain-segmentation collection: `Graphemes` over the whole string from
/// the start, the pre-optimization behavior.
fn collect_reference(text: &str) -> Vec<&str> {
    Graphemes::new(text).collect()
}

/// Fast-path collection: repeated `next_grapheme_at` calls, exactly what
/// `ClusterLineCellIter`/`truncate` now do.
fn collect_fast(text: &str) -> Vec<&str> {
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < text.len() {
        let g = next_grapheme_at(text, pos);
        pos += g.len();
        out.push(g);
    }
    out
}

#[test]
fn next_grapheme_at_matches_plain_segmentation() {
    let mut cases: Vec<String> = vec![
        "".to_string(),
        "hello world".to_string(),
        "e\u{0301}clair".to_string(),
        "a\r\nb".to_string(),
        "\r\n".to_string(),
        "plain\r\nCRLF\r\nmix".to_string(),
        "\u{1f1fa}\u{1f1f8}\u{1f1ec}\u{1f1e7}".to_string(), // two flags back to back
        "\u{0915}\u{094d}\u{0937}ascii".to_string(),
        "tail\u{2764}\u{fe0f}".to_string(),
    ];
    for seed in 0..20u64 {
        cases.push(random_text(seed, 40));
    }

    for text in &cases {
        assert_eq!(
            collect_fast(text),
            collect_reference(text),
            "text={:?}",
            text
        );
    }
}

fn bold() -> CellAttributes {
    CellAttributes::default()
        .set_intensity(Intensity::Bold)
        .clone()
}

fn colored() -> CellAttributes {
    CellAttributes::default()
        .set_background(AnsiColor::Blue)
        .clone()
}

fn attr_pool() -> Vec<CellAttributes> {
    vec![CellAttributes::default(), bold(), colored()]
}

/// Builds a `ClusteredLine` the way production code does: one
/// `append_grapheme` call per grapheme of `text` (segmented once, up
/// front, with plain `Graphemes`), with attributes cycling through a
/// small pool (seeded by `seed`) so the built line has several clusters.
fn build_cluster_line(text: &str, seed: u64) -> ClusteredLine {
    let mut lcg = Lcg::new(seed);
    let attrs = attr_pool();
    let mut cl = ClusteredLine::new();
    for g in Graphemes::new(text) {
        let attr = lcg.choose(&attrs).clone();
        let cell = Cell::new_grapheme(g, attr, None);
        cl.append_grapheme(cell.str(), cell.width(), cell.attrs().clone());
    }
    cl
}

/// Reference: the pre-optimization traversal, built directly on `text`
/// and `clusters` with plain `Graphemes` -- exactly what
/// `ClusterLineCellIter::next` did before the ASCII fast path.
fn reference_iter(cl: &ClusteredLine) -> Vec<(usize, String, usize, CellAttributes)> {
    let mut clusters = cl.clusters.iter();
    let mut cluster = clusters.next();
    let mut idx = 0usize;
    let mut cluster_total = 0usize;
    let mut out = Vec::new();
    for text in Graphemes::new(&cl.text) {
        let cell_index = idx;
        let width = if cl.is_double_wide(cell_index) { 2 } else { 1 };
        idx += width;
        cluster_total += width;
        let c = cluster.expect("cluster covers every cell");
        out.push((cell_index, text.to_string(), width, c.attrs.clone()));
        if cluster_total >= c.cell_width as usize {
            cluster = clusters.next();
            cluster_total = 0;
        }
    }
    out
}

/// The production iterator under test.
fn optimized_iter(cl: &ClusteredLine) -> Vec<(usize, String, usize, CellAttributes)> {
    cl.iter()
        .map(|c| {
            (
                c.cell_index(),
                c.str().to_string(),
                c.width(),
                c.attrs().clone(),
            )
        })
        .collect()
}

#[test]
fn cluster_iter_matches_reference_traversal() {
    let mut texts: Vec<String> = vec![
        "".to_string(),
        "hello".to_string(),
        "e\u{0301}clair au \u{1f1eb}\u{1f1f7}".to_string(),
        "\u{2764}\u{fe0f} caf\u{0065}\u{0301}".to_string(),
        "\u{0915}\u{094d}\u{0937}etra".to_string(),
        "\u{1f468}\u{1f3fe}\u{200d}\u{1f9b0} team".to_string(),
        "日本語mix日本語".to_string(),
        "a".to_string(),
        "日".to_string(),
    ];
    for seed in 0..20u64 {
        texts.push(random_text(seed + 100, 60));
    }

    for (i, text) in texts.iter().enumerate() {
        let cl = build_cluster_line(text, i as u64);
        assert_eq!(optimized_iter(&cl), reference_iter(&cl), "text={:?}", text);
    }
}
