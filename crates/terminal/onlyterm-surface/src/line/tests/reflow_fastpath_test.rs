#![cfg(test)]
//! Differential harness for the OPT-4 reflow work: `Line::wrap_keeping`
//! and `Line::append_line` versus their `*_reference` oracles (verbatim
//! copies of the pre-optimization bodies). In this phase the public
//! functions simply delegate to the references, so equality is trivially
//! true by construction; the point is that the harness (generator,
//! comparator, targeted cases) is in place and has proven teeth, so the
//! fast-path phase only swaps the implementation in.
//!
//! Lives as a child of `crate::line::line` (registered from
//! `line/line/mod.rs`, like `fill_range_test.rs`) for private-field access
//! if needed.

use super::Line;
use crate::hyperlink::Hyperlink;
use crate::{SequenceNo, SEQ_ZERO};
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use onlyterm_bidi::ParagraphDirectionHint;
use onlyterm_cell::color::{AnsiColor, ColorAttribute, RgbColor};
use onlyterm_cell::{CellAttributes, Intensity, SemanticType};

/// Small deterministic PRNG so failures are reproducible without pulling in
/// an external `rand` dependency.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        // Avoid an all-zero state, which would make the LCG degenerate.
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

    fn chance_one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
}

const GRAPHEMES: &[(&str, usize)] = &[
    ("a", 1),
    (" ", 1),
    ("é", 1),
    // Combining mark, kept at explicit width 1 like `fill_range_test.rs`.
    ("e\u{301}", 1),
    ("ж", 1),
    ("漢", 2),
    ("👍", 2),
    // Flag sequence: one grapheme cluster, width 2.
    ("\u{1f1f7}\u{1f1fa}", 2),
];

const ATTR_COUNT: usize = 11;

fn attrs_pool(link_a: &Arc<Hyperlink>, link_b: &Arc<Hyperlink>) -> Vec<CellAttributes> {
    vec![
        CellAttributes::default(),
        CellAttributes::default()
            .set_intensity(Intensity::Bold)
            .clone(),
        CellAttributes::default()
            .set_foreground(AnsiColor::Red)
            .clone(),
        CellAttributes::default()
            .set_foreground(ColorAttribute::TrueColorWithDefaultFallback(
                RgbColor::new_8bpc(0x12, 0x34, 0x56).into(),
            ))
            .clone(),
        CellAttributes::default()
            .set_background(ColorAttribute::TrueColorWithDefaultFallback(
                RgbColor::new_8bpc(0xaa, 0xbb, 0xcc).into(),
            ))
            .clone(),
        CellAttributes::default()
            .set_underline_color(ColorAttribute::TrueColorWithDefaultFallback(
                RgbColor::new_8bpc(0xff, 0x88, 0x00).into(),
            ))
            .clone(),
        CellAttributes::default()
            .set_hyperlink(Some(Arc::clone(link_a)))
            .clone(),
        CellAttributes::default()
            .set_hyperlink(Some(Arc::clone(link_b)))
            .clone(),
        CellAttributes::default()
            .set_semantic_type(SemanticType::Prompt)
            .clone(),
        CellAttributes::default()
            .set_semantic_type(SemanticType::Input)
            .clone(),
        CellAttributes::default().set_wrapped(true).clone(),
    ]
}

const DIRECTIONS: [ParagraphDirectionHint; 4] = [
    ParagraphDirectionHint::LeftToRight,
    ParagraphDirectionHint::RightToLeft,
    ParagraphDirectionHint::AutoLeftToRight,
    ParagraphDirectionHint::AutoRightToLeft,
];

/// A deterministic recipe of mutations; replayed identically to build two
/// independent copies of the same input line.
#[derive(Debug, Clone)]
enum Op {
    Grapheme {
        text: usize,
        attrs: usize,
    },
    AsciiRun {
        back: usize,
        len: usize,
        attrs: usize,
    },
    Wrapped(bool),
    Prune,
    Bidi {
        enabled: bool,
        dir: usize,
    },
    Ctrl {
        text: usize,
    },
    Compress,
    // OPT-4 phase C additions (coverage only ever grows): a long line so
    // that width is often much smaller than len, and an interior attribute
    // change so pieces regularly cut through many attribute runs.
    LongAsciiRun {
        len: usize,
        attrs: usize,
    },
    Recolor {
        back: usize,
        attrs: usize,
    },
}

const CTRL_TEXTS: &[(&str, usize)] = &[
    // Control byte: the reference "neutralizes" it via `as_cell()`.
    ("\x07", 1),
    // Zero-width combining cluster.
    ("\u{301}", 0),
];

fn gen_ops(rng: &mut Lcg) -> Vec<Op> {
    let mut ops = Vec::new();
    let base = 4 + rng.below(36);
    for _ in 0..base {
        ops.push(Op::Grapheme {
            text: rng.below(GRAPHEMES.len()),
            attrs: rng.below(ATTR_COUNT),
        });
    }
    let extra = rng.below(6);
    for _ in 0..extra {
        match rng.below(10) {
            0 => ops.push(Op::AsciiRun {
                back: 1 + rng.below(6),
                len: 1 + rng.below(8),
                attrs: rng.below(ATTR_COUNT),
            }),
            1 => ops.push(Op::Wrapped(rng.chance_one_in(2))),
            2 => ops.push(Op::Prune),
            3 => ops.push(Op::Bidi {
                enabled: rng.chance_one_in(2),
                dir: rng.below(4),
            }),
            4 => ops.push(Op::Ctrl { text: 0 }),
            5 => ops.push(Op::Ctrl { text: 1 }),
            6 => ops.push(Op::Compress),
            8 => ops.push(Op::LongAsciiRun {
                len: 48 + rng.below(160),
                attrs: rng.below(ATTR_COUNT),
            }),
            9 => {
                ops.push(Op::Recolor {
                    back: 1 + rng.below(12),
                    attrs: rng.below(ATTR_COUNT),
                });
                ops.push(Op::Compress);
            }
            _ => ops.push(Op::Grapheme {
                text: rng.below(GRAPHEMES.len()),
                attrs: rng.below(ATTR_COUNT),
            }),
        }
    }
    // Some inputs stay in Vec storage; some get left in clustered storage
    // either by an interior `Compress` or by this trailing one.
    if rng.chance_one_in(4) {
        ops.push(Op::Compress);
    }
    ops
}

fn build_line(ops: &[Op], pool: &[CellAttributes], seqno: SequenceNo) -> Line {
    let mut line = Line::new(seqno);
    for op in ops {
        match op {
            Op::Grapheme { text, attrs } => {
                let (g, w) = GRAPHEMES[*text];
                let idx = line.len();
                line.set_cell_grapheme(idx, g, w, pool[*attrs].clone(), seqno);
            }
            Op::AsciiRun { back, len, attrs } => {
                let idx = line.len().saturating_sub(*back);
                let run: String = "abcdefghijklmnopqrstuvwxyz"
                    .chars()
                    .cycle()
                    .take((*len).max(1))
                    .collect();
                line.set_ascii_run(idx, &run, &pool[*attrs], seqno);
            }
            Op::Wrapped(v) => {
                // Skip on lines carrying a zero-width cluster (e.g. after
                // Ctrl): the *reference* implementation underflows there --
                // in C storage the iterator reports width 1 for a
                // zero-width cell while the cluster accounting recorded 0,
                // so `set_last_cell_was_wrapped` computes `0 - 1`. A
                // pre-existing limitation of the shared oracle, so the
                // input and its replay skip it identically. (Such lines are
                // still wrapped by the differential tests, where
                // `wrap_keeping` falls back to the reference for them.)
                let degenerate = matches!(
                    &line.cells,
                    crate::line::storage::CellStorage::C(cl) if !cl.clusters_consistent()
                );
                if !degenerate {
                    line.set_last_cell_was_wrapped(*v, seqno);
                }
            }
            Op::Prune => line.prune_trailing_blanks(seqno),
            Op::Bidi { enabled, dir } => line.set_bidi_info(*enabled, DIRECTIONS[*dir], seqno),
            Op::Ctrl { text } => {
                let (g, w) = CTRL_TEXTS[*text];
                let idx = line.len();
                line.set_cell_grapheme(idx, g, w, pool[0].clone(), seqno);
            }
            Op::Compress => line.compress_for_scrollback(),
            Op::LongAsciiRun { len, attrs } => {
                let idx = line.len();
                let run: String = "abcdefghijklmnopqrstuvwxyz"
                    .chars()
                    .cycle()
                    .take(*len)
                    .collect();
                line.set_ascii_run(idx, &run, &pool[*attrs], seqno);
            }
            Op::Recolor { back, attrs } => {
                if !line.is_empty() {
                    let idx = line.len().saturating_sub(1 + *back);
                    let cells = line.cells_mut_for_attr_changes_only();
                    // Skip recolors that could isolate a trailing zero-width
                    // cell into its own width-0 cluster (via a later
                    // compress): the *reference* implementation underflows
                    // there (`from_cell_vec` records `last_cell_width = 1`
                    // for a zero-width cell, then
                    // `set_last_cell_was_wrapped` computes `0 - 1`). A
                    // limitation of the shared oracle, not of the fast path.
                    let n = cells.len();
                    let last_is_zero_width = cells[n - 1].width() == 0;
                    let could_isolate_tail = idx + 2 >= n;
                    if let Some(cell) = cells.get_mut(idx) {
                        if cell.width() > 0 && !(last_is_zero_width && could_isolate_tail) {
                            *cell.attrs_mut() = pool[*attrs].clone();
                        }
                    }
                }
            }
        }
    }
    line
}

/// Fully observable comparison: `Line: PartialEq` (catches storage-level
/// differences like cluster splits and bitset contents) plus an explicit
/// observable projection (visible cells, len, wrapped flag, bidi,
/// hyperlink presence, seqno) so a failure message shows *what* differs.
#[derive(Debug, PartialEq)]
struct Observed {
    cells: Vec<(usize, String, usize, CellAttributes)>,
    len: usize,
    last_wrapped: bool,
    bidi: (bool, u8),
    has_hyperlink: bool,
    seqno: SequenceNo,
}

fn direction_id(dir: ParagraphDirectionHint) -> u8 {
    match dir {
        ParagraphDirectionHint::LeftToRight => 0,
        ParagraphDirectionHint::RightToLeft => 1,
        ParagraphDirectionHint::AutoLeftToRight => 2,
        ParagraphDirectionHint::AutoRightToLeft => 3,
    }
}

fn observe(line: &Line) -> Observed {
    let (bidi_enabled, dir) = line.bidi_info();
    Observed {
        cells: line
            .visible_cells()
            .map(|c| {
                (
                    c.cell_index(),
                    c.str().to_string(),
                    c.width(),
                    c.attrs().clone(),
                )
            })
            .collect(),
        len: line.len(),
        last_wrapped: line.last_cell_was_wrapped(),
        bidi: (bidi_enabled, direction_id(dir)),
        has_hyperlink: line.has_hyperlink(),
        seqno: line.current_seqno(),
    }
}

/// The reusable comparison harness for `wrap_keeping`-style results.
fn assert_same(a: &[Line], b: &[Line], ctx: &str) {
    if a.len() != b.len() {
        panic!(
            "{}: line count differs: public {} vs reference {}",
            ctx,
            a.len(),
            b.len()
        );
    }
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            panic!(
                "{}: line {} differs by PartialEq\npublic: {:?}\nreference: {:?}",
                ctx,
                i,
                observe(x),
                observe(y)
            );
        }
        if observe(x) != observe(y) {
            panic!(
                "{}: line {} differs observably\npublic: {:?}\nreference: {:?}",
                ctx,
                i,
                observe(x),
                observe(y)
            );
        }
    }
}

fn keep_for(rng: &mut Lcg, len: usize) -> usize {
    match rng.below(4) {
        0 => 0,
        1 => rng.below(len + 1),
        2 => len,
        _ => len + 5,
    }
}

#[test]
fn wrap_matches_reference_on_random_lines() {
    let link_a = Arc::new(Hyperlink::new("http://example.com/a"));
    let link_b = Arc::new(Hyperlink::new("http://example.com/b"));
    let cases = 20000u64;
    for seed in 0..cases {
        let mut rng = Lcg::new(seed);
        let ops = gen_ops(&mut rng);
        let pool = attrs_pool(&link_a, &link_b);
        let seqno = SEQ_ZERO + (seed % 7) as SequenceNo;

        let probe = build_line(&ops, &pool, seqno);
        let len = probe.len();
        let width = 1 + rng.below(len + 3);
        let keep = keep_for(&mut rng, len);

        let ctx = format!("seed={} width={} keep={} len={}", seed, width, keep, len);

        let a = build_line(&ops, &pool, seqno);
        let b = build_line(&ops, &pool, seqno);
        let seq = seqno + 100;
        let res_pub = a.wrap_keeping(width, keep, seq);
        let res_ref = b.wrap_keeping_reference(width, keep, seq);
        assert_same(&res_pub, &res_ref, &ctx);

        // Every 5th case also exercises a shared-storage input: the line
        // handed to the reference is a live clone of the one handed to the
        // public function (shared `Arc`s inside the storage).
        if seed % 5 == 0 {
            let shared = build_line(&ops, &pool, seqno);
            let _holder = shared.clone();
            let res_shared = shared.wrap_keeping_reference(width, keep, seq);
            assert_same(&res_pub, &res_shared, &format!("{} shared-clone", ctx));
        }
    }
}

#[test]
fn append_line_matches_reference_on_random_pairs() {
    let link_a = Arc::new(Hyperlink::new("http://example.com/a"));
    let link_b = Arc::new(Hyperlink::new("http://example.com/b"));
    let cases = 5000u64;
    for seed in 0..cases {
        let mut rng = Lcg::new(seed);
        let pool = attrs_pool(&link_a, &link_b);
        let seqno = SEQ_ZERO + (seed % 5) as SequenceNo;
        let ops_a = gen_ops(&mut rng);
        let ops_b = gen_ops(&mut rng);

        let shared_other = seed % 2 == 0;
        let ctx = format!("seed={} shared_other={}", seed, shared_other);

        let mut a = build_line(&ops_a, &pool, seqno);
        let mut b = build_line(&ops_b, &pool, seqno);
        if rng.chance_one_in(3) {
            a.compress_for_scrollback();
        }
        if rng.chance_one_in(3) {
            b.compress_for_scrollback();
        }
        let _keeper = if shared_other { Some(b.clone()) } else { None };

        let mut a_ref = build_line(&ops_a, &pool, seqno);
        let mut b_ref = build_line(&ops_b, &pool, seqno);
        // Mirror the storage-shaping that happened to `a`/`b`.
        if matches!(a.cells, crate::line::storage::CellStorage::C(_)) {
            a_ref.compress_for_scrollback();
        }
        if matches!(b.cells, crate::line::storage::CellStorage::C(_)) {
            b_ref.compress_for_scrollback();
        }

        let seq = seqno + 50;
        a.append_line(b, seq);
        a_ref.append_line_reference(b_ref, seq);
        assert_same(&[a], &[a_ref], &ctx);
    }
}

/// Targeted boundary cases that the random generator only hits by luck.
type TargetedCase = (
    &'static str,
    Vec<(&'static str, usize, CellAttributes)>,
    Vec<usize>,
);

#[test]
fn wrap_targeted_cases_match_reference() {
    let link = Arc::new(Hyperlink::new("http://example.com/tail"));
    let plain = CellAttributes::default();
    let colored = CellAttributes::default()
        .set_foreground(AnsiColor::Blue)
        .clone();
    let linked = CellAttributes::default()
        .set_hyperlink(Some(Arc::clone(&link)))
        .clone();

    // (name, [(text, width, attrs)], widths to try)
    let cases: Vec<TargetedCase> = vec![
        (
            "wide char at boundary",
            vec![
                ("a", 1, plain.clone()),
                ("b", 1, plain.clone()),
                ("漢", 2, plain.clone()),
                ("c", 1, plain.clone()),
                ("d", 1, plain.clone()),
            ],
            vec![1, 2, 3, 4, 5, 7],
        ),
        (
            "width 1 with wide chars",
            vec![("👍", 2, plain.clone()), ("🇷🇺", 2, plain.clone())],
            vec![1, 2, 3, 4, 5],
        ),
        (
            "all spaces",
            vec![(" ", 1, plain.clone()); 10],
            vec![1, 2, 5, 10, 11, 12],
        ),
        (
            "colored trailing spaces",
            vec![
                ("a", 1, plain.clone()),
                ("b", 1, colored.clone()),
                (" ", 1, colored.clone()),
                (" ", 1, colored.clone()),
                (" ", 1, colored.clone()),
            ],
            vec![1, 2, 3, 5, 6],
        ),
        (
            "hyperlink only in tail",
            vec![
                ("h", 1, plain.clone()),
                ("i", 1, plain.clone()),
                ("l", 1, linked.clone()),
                ("k", 1, linked.clone()),
            ],
            vec![1, 2, 3, 4, 5],
        ),
    ];

    for (name, pieces, widths) in &cases {
        for &width in widths {
            for &keep in &[0usize, 2, 5, 100] {
                let build = || {
                    let mut line = Line::new(SEQ_ZERO);
                    for (text, w, attrs) in pieces {
                        let idx = line.len();
                        line.set_cell_grapheme(idx, text, *w, attrs.clone(), SEQ_ZERO);
                    }
                    line
                };
                let seq = SEQ_ZERO + 9;
                let res_pub = build().wrap_keeping(width, keep, seq);
                let res_ref = build().wrap_keeping_reference(width, keep, seq);
                assert_same(
                    &res_pub,
                    &res_ref,
                    &format!("case={} width={} keep={}", name, width, keep),
                );
            }
        }
    }
}

/// Targeted append cases: control bytes and zero-width clusters in
/// `other` (the fast path must fall back to the reference for these).
#[test]
fn append_line_targeted_cases_match_reference() {
    let plain = CellAttributes::default();
    let bold = CellAttributes::default()
        .set_intensity(Intensity::Bold)
        .clone();

    for (name, tail) in &[
        ("control byte", vec![("\x07", 1, plain.clone())]),
        ("zero width", vec![("\u{301}", 0, plain.clone())]),
        (
            "control then wide",
            vec![("\x07", 1, plain.clone()), ("漢", 2, bold.clone())],
        ),
    ] {
        for self_clustered in [false, true] {
            let build_a = |clustered: bool| {
                let mut line = Line::new(SEQ_ZERO);
                for (i, g) in ["a", "b", "c"].iter().enumerate() {
                    line.set_cell_grapheme(line.len(), g, 1, plain.clone(), SEQ_ZERO + i);
                }
                if clustered {
                    line.compress_for_scrollback();
                }
                line
            };
            let build_b = || {
                let mut line = Line::new(SEQ_ZERO);
                for (text, w, attrs) in tail {
                    line.set_cell_grapheme(line.len(), text, *w, attrs.clone(), SEQ_ZERO + 3);
                }
                line
            };
            let seq = SEQ_ZERO + 20;
            let mut a = build_a(self_clustered);
            let mut a_ref = build_a(self_clustered);
            a.append_line(build_b(), seq);
            a_ref.append_line_reference(build_b(), seq);
            assert_same(&[a], &[a_ref], &format!("case={}", name));
        }
    }
}

/// Proves the fast path is actually taken for ordinary C-storage input
/// (and still produces the right number of pieces), and that Vec storage
/// falls back to the reference oracle.

#[test]
fn wrap_keeping_fast_path_taken_on_clustered_storage() {
    let plain = CellAttributes::default();
    let build = || {
        let mut line = Line::new(SEQ_ZERO);
        for g in ["a", "b", "c", "d", "e", "f"].iter() {
            line.set_cell_grapheme(line.len(), g, 1, plain.clone(), SEQ_ZERO);
        }
        line
    };

    super::editing::reset_wrap_fast_path_hits();
    let pieces = build().wrap_keeping(2, 0, SEQ_ZERO + 1);
    assert_eq!(pieces.len(), 3);
    assert_eq!(
        super::editing::wrap_fast_path_hits(),
        1,
        "ordinary clustered input must take the OPT-4 fast path"
    );

    super::editing::reset_wrap_fast_path_hits();
    let v = Line::from_text("abcdef", &plain, SEQ_ZERO, None);
    let pieces = v.wrap_keeping(2, 0, SEQ_ZERO + 1);
    assert_eq!(pieces.len(), 3);
    assert_eq!(
        super::editing::wrap_fast_path_hits(),
        0,
        "Vec storage must fall back to the reference oracle"
    );
}
