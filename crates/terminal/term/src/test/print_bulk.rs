//! Differential test for the bulk-ASCII fast path in
//! `Performer::flush_print` (see `ascii_bulk_run_len`/`print_ascii_run`).
//! Drives two terminals -- one using the fast path, one with
//! `force_slow_print_path` set so every character goes through the
//! original per-grapheme loop (`print_one_grapheme`) -- with the same
//! random byte streams, and compares the resulting screen contents,
//! cursor and `wrap_next` state. Covers plain ASCII, CR/LF, cursor moves,
//! SGR, OSC 8 hyperlinks, combining marks right after ASCII, CJK wide
//! chars, VS16 emoji, G0/G1 charset switches + SO/SI, insert mode,
//! DECAWM, DECLRMM/DECSLRM margins and scroll regions, Hebrew diacritics,
//! random mid-run chunk splits, crossed with NFC
//! normalization and ConPTY quirks on/off.
use super::*;
use k9::assert_equal as assert_eq;

/// Small deterministic PRNG so failures are reproducible without pulling
/// in an external `rand` dependency (same approach as
/// `onlyterm-surface`'s `fill_range_test.rs`).
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

    fn chance_one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
}

#[derive(Debug, Clone)]
struct DiffConfig {
    nfc: bool,
    unicode_version: UnicodeVersion,
}

impl DiffConfig {
    fn new(nfc: bool) -> Self {
        DiffConfig {
            nfc,
            // Matches `TerminalConfiguration::unicode_version`'s default
            // (version 9, ambiguous narrow, no cell_widths overrides), so
            // the pre-existing differential tests are unchanged.
            unicode_version: UnicodeVersion {
                version: 9,
                ambiguous_are_wide: false,
                cell_widths: None,
            },
        }
    }

    fn with_unicode_version(nfc: bool, unicode_version: UnicodeVersion) -> Self {
        DiffConfig {
            nfc,
            unicode_version,
        }
    }
}

impl TerminalConfiguration for DiffConfig {
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
    fn normalize_output_to_unicode_nfc(&self) -> bool {
        self.nfc
    }
    fn scrollback_size(&self) -> usize {
        SCROLLBACK
    }
    fn unicode_version(&self) -> UnicodeVersion {
        self.unicode_version.clone()
    }
}

const ROWS: usize = 6;
const COLS: usize = 12;
const SCROLLBACK: usize = 30;

fn make_term(nfc: bool, conpty: bool, force_slow: bool) -> Terminal {
    let mut term = Terminal::new(
        TerminalSize {
            rows: ROWS,
            cols: COLS,
            pixel_width: COLS * 8,
            pixel_height: ROWS * 16,
            dpi: 0,
        },
        Arc::new(DiffConfig::new(nfc)),
        "OnlyTerm",
        "0.0.0",
        Box::new(Vec::new()),
    );
    if conpty {
        term.enable_conpty_quirks();
    }
    term.force_slow_print_path = force_slow;
    term
}

/// Generates a random byte stream exercising the scenarios listed in the
/// module doc comment.
fn gen_stream(rng: &mut Lcg, events: usize) -> Vec<u8> {
    const ASCII_WORDS: &[&str] = &[
        "hello",
        "world",
        "OnlyTerm",
        "abcXYZ",
        "123!?",
        "quick brown",
        "-> <-",
        "x",
        "y",
        "ok",
        // Space-only runs, e.g. after a cursor move past the end of a line.
        "   ",
        " ",
    ];
    const SGR_CODES: &[u32] = &[0, 1, 4, 7, 22, 27, 31, 32, 34, 41, 44, 90];
    const CJK: &[&str] = &["日", "本", "中", "文", "国"];
    const CYRILLIC: &[&str] = &["привет", "мир", "ЁЖИК", "ъэюя", "Эпопея"];
    const GREEK: &[&str] = &["αβγδε", "Ωμέγα"];
    const LATIN: &[&str] = &["café", "naïve", "Straße", "ĐžŔ", "ændřę"];
    const BOX: &[&str] = &["┌─┐│└┘", "████▓▒░", "╔═╝"];
    const PUNCT: &[&str] = &["«…»", "—–", "†‡•", "‰′″"];
    // Hebrew consonant + niqqud/cantillation (dropped by the slow path).
    const HEBREW: &[&str] = &["אְ", "בּ", "שָׁ", "לְ"];
    // Combining mark directly after a table char.
    const TABLE_PLUS_MARK: &[&str] = &["я́", "α̈", "ё̑"];
    const ZWJ_SEQ: &[&str] = &["👨‍👩", "❤️"];

    let mut out = Vec::new();
    for _ in 0..events {
        match rng.below(27) {
            0 | 1 => {
                let w = ASCII_WORDS[rng.below(ASCII_WORDS.len())];
                out.extend_from_slice(w.as_bytes());
            }
            2 => out.push(b'\r'),
            3 => out.push(b'\n'),
            4 => out.extend_from_slice(
                format!(
                    "\x1b[{};{}H",
                    1 + rng.below(ROWS + 2),
                    1 + rng.below(COLS + 2)
                )
                .as_bytes(),
            ),
            5 => out.extend_from_slice(format!("\x1b[{}C", 1 + rng.below(6)).as_bytes()),
            6 => out.extend_from_slice(format!("\x1b[{}D", 1 + rng.below(6)).as_bytes()),
            7 => out.extend_from_slice(
                format!("\x1b[{}m", SGR_CODES[rng.below(SGR_CODES.len())]).as_bytes(),
            ),
            8 => out.extend_from_slice(b"\x1b]8;;http://example.com/x\x1b\\"),
            9 => out.extend_from_slice(b"\x1b]8;;\x1b\\"),
            10 => out.extend_from_slice("e\u{0301}".as_bytes()), // "e" + combining acute
            11 => out.extend_from_slice("n\u{0303}".as_bytes()), // "n" + combining tilde
            12 => out.extend_from_slice(CJK[rng.below(CJK.len())].as_bytes()),
            19 => out.extend_from_slice(CYRILLIC[rng.below(CYRILLIC.len())].as_bytes()),
            20 => out.extend_from_slice(GREEK[rng.below(GREEK.len())].as_bytes()),
            21 => out.extend_from_slice(LATIN[rng.below(LATIN.len())].as_bytes()),
            22 => out.extend_from_slice(BOX[rng.below(BOX.len())].as_bytes()),
            23 => out.extend_from_slice(PUNCT[rng.below(PUNCT.len())].as_bytes()),
            24 => out.extend_from_slice(HEBREW[rng.below(HEBREW.len())].as_bytes()),
            25 => {
                out.extend_from_slice(TABLE_PLUS_MARK[rng.below(TABLE_PLUS_MARK.len())].as_bytes())
            }
            26 => out.extend_from_slice(ZWJ_SEQ[rng.below(ZWJ_SEQ.len())].as_bytes()),
            13 => out.extend_from_slice("1\u{fe0f}\u{20e3}".as_bytes()), // keycap "1" + VS16
            14 => out.extend_from_slice(if rng.chance_one_in(2) {
                b"\x1b(0"
            } else {
                b"\x1b(B"
            }),
            15 => out.push(if rng.chance_one_in(2) { 0x0e } else { 0x0f }), // SO / SI
            16 => out.extend_from_slice(if rng.chance_one_in(2) {
                b"\x1b[4h" // IRM on
            } else {
                b"\x1b[4l" // IRM off
            }),
            17 => out.extend_from_slice(if rng.chance_one_in(2) {
                b"\x1b[?7h" // DECAWM on
            } else {
                b"\x1b[?7l" // DECAWM off
            }),
            18 => out.extend_from_slice(if rng.chance_one_in(2) {
                b"\x1b[?69h" // DECLRMM on
            } else {
                b"\x1b[?69l" // DECLRMM off
            }),
            _ => match rng.below(2) {
                0 => {
                    let l = 1 + rng.below(4);
                    let r = l + 2 + rng.below(COLS);
                    out.extend_from_slice(format!("\x1b[{};{}s", l, r).as_bytes());
                }
                _ => {
                    let t = 1 + rng.below(3);
                    let b = t + 2 + rng.below(ROWS);
                    out.extend_from_slice(format!("\x1b[{};{}r", t, b).as_bytes());
                }
            },
        }
    }
    out
}

#[derive(Debug, PartialEq)]
struct ObservedLine {
    text: String,
    cells: Vec<(usize, usize, CellAttributes)>,
    len: usize,
    last_wrapped: bool,
}

fn observe_line(line: &Line) -> ObservedLine {
    ObservedLine {
        text: line.as_str().to_string(),
        cells: line
            .visible_cells()
            .map(|c| (c.cell_index(), c.width(), c.attrs().clone()))
            .collect(),
        len: line.len(),
        last_wrapped: line.last_cell_was_wrapped(),
    }
}

#[derive(Debug, PartialEq)]
struct ObservedTerm {
    lines: Vec<ObservedLine>,
    cursor_x: usize,
    cursor_y: i64,
    wrap_next: bool,
}

fn observe(term: &Terminal) -> ObservedTerm {
    let cursor = term.cursor_pos();
    ObservedTerm {
        lines: term.screen().all_lines().iter().map(observe_line).collect(),
        cursor_x: cursor.x,
        cursor_y: cursor.y,
        wrap_next: term.wrap_next(),
    }
}

#[test]
fn bulk_ascii_print_matches_slow_path_for_random_streams() {
    for nfc in [false, true] {
        for conpty in [false, true] {
            for seed in 0..40u64 {
                let mut rng = Lcg::new(seed ^ ((nfc as u64) << 1) ^ (conpty as u64));
                let stream = gen_stream(&mut rng, 400);

                let mut fast = make_term(nfc, conpty, false);
                let mut slow = make_term(nfc, conpty, true);

                // Feed in randomly-sized chunks so the print buffer is
                // flushed at arbitrary byte offsets, including in the
                // middle of a multi-byte run.
                let mut off = 0usize;
                while off < stream.len() {
                    let step = 1 + rng.below(24);
                    let end = (off + step).min(stream.len());
                    fast.advance_bytes(&stream[off..end]);
                    slow.advance_bytes(&stream[off..end]);
                    off = end;
                }

                assert_eq!(
                    observe(&fast),
                    observe(&slow),
                    "nfc={} conpty={} seed={}",
                    nfc,
                    conpty,
                    seed
                );
            }
        }
    }
}

/// Default blanks printed past the end of a clustered line stay implicit
/// on both paths (rows scrolled in are clustered).
#[test]
fn space_run_past_end_of_clustered_line_matches_slow_path() {
    let mut stream = Vec::new();
    for _ in 0..(ROWS + 2) {
        stream.extend_from_slice(b"x\r\n");
    }
    stream.extend_from_slice(b"ab\x1b[5C   \x1b[1m\x1b[3C  Z\x1b[m\r\n\x1b[4C \r\n");

    for conpty in [false, true] {
        let mut fast = make_term(false, conpty, false);
        let mut slow = make_term(false, conpty, true);
        fast.advance_bytes(&stream);
        slow.advance_bytes(&stream);
        assert_eq!(observe(&fast), observe(&slow), "conpty={}", conpty);
    }
}

/// The bulk fast path must be taken for Cyrillic text, and must NOT be
/// taken in insert mode, under `force_slow_print_path`, or for combining
/// sequences (a base char directly followed by a combining mark is left
/// to the slow path).
#[test]
fn fast_path_counter_cyrillic_taken_and_conditions() {
    fn make() -> Terminal {
        let mut term = make_term(false, false, false);
        term.bulk_print_cells = 0;
        term
    }

    // Plain Cyrillic must hit the bulk path.
    let mut term = make();
    term.advance_bytes("привет мир".as_bytes());
    assert_eq!(
        term.bulk_print_cells, 10,
        "Cyrillic must be printed via the bulk fast path"
    );

    // Box drawing too.
    let mut term = make();
    term.advance_bytes("┌─┐".as_bytes());
    assert_eq!(term.bulk_print_cells, 3);

    // force_slow_print_path disables it entirely.
    let mut slow = make_term(false, false, true);
    slow.bulk_print_cells = 0;
    slow.advance_bytes("привет".as_bytes());
    assert_eq!(slow.bulk_print_cells, 0);

    // Insert mode disables it.
    let mut ins = make();
    ins.advance_bytes(b"\x1b[4h");
    ins.bulk_print_cells = 0;
    ins.advance_bytes("привет".as_bytes());
    assert_eq!(ins.bulk_print_cells, 0);

    // A combining mark directly after a Cyrillic base: the base must be
    // left to the slow path (the mark fuses with it into one grapheme).
    let mut marks = make();
    marks.advance_bytes("я́".as_bytes());
    assert_eq!(
        marks.bulk_print_cells, 0,
        "a base directly followed by a combining mark must not be bulked"
    );

    // ...but the same text followed by regular members resumes bulking.
    let mut resume = make();
    resume.advance_bytes("я́привет".as_bytes());
    assert_eq!(resume.bulk_print_cells, 6);
}

/// Builds a terminal configured with an explicit `UnicodeVersion`.
fn make_term_uv(uv: UnicodeVersion, conpty: bool, force_slow: bool) -> Terminal {
    let mut term = Terminal::new(
        TerminalSize {
            rows: ROWS,
            cols: COLS,
            pixel_width: COLS * 8,
            pixel_height: ROWS * 16,
            dpi: 0,
        },
        Arc::new(DiffConfig::with_unicode_version(false, uv)),
        "OnlyTerm",
        "0.0.0",
        Box::new(Vec::new()),
    );
    if conpty {
        term.enable_conpty_quirks();
    }
    term.force_slow_print_path = force_slow;
    term
}

/// Table chars that are East-Asian Ambiguous (box drawing, dashes,
/// punctuation) must NOT be bulked when the config says
/// `ambiguous_are_wide`: the fast path would write width-1 cells where the
/// slow path writes width 2. ASCII stays eligible for the bulk path.
#[test]
fn bulk_skips_table_chars_when_ambiguous_are_wide() {
    let uv = UnicodeVersion {
        version: 9,
        ambiguous_are_wide: true,
        cell_widths: None,
    };
    // Short enough to stay on one line (COLS=12), so the assertion below
    // is not confounded by wrap/handback effects at the right margin.
    let stream: &str = "ab\u{410}\u{44f}\u{00ab}cd";

    let mut fast = make_term_uv(uv.clone(), false, false);
    fast.bulk_print_cells = 0;
    let mut slow = make_term_uv(uv, false, true);
    slow.bulk_print_cells = 0;

    fast.advance_bytes(stream.as_bytes());
    slow.advance_bytes(stream.as_bytes());

    // With allow_table off, only ASCII is bulked; the run "ab" stops at
    // the table char and its last char is left to the slow path (possible
    // combining mark), as is "b". So the bulked cells are "a" + "cd" = 3.
    assert_eq!(
        fast.bulk_print_cells, 3,
        "with ambiguous_are_wide, only ASCII cells may be bulked"
    );
    assert_eq!(slow.bulk_print_cells, 0);
    assert_eq!(
        observe(&fast),
        observe(&slow),
        "ambiguous_are_wide terminal state must match the slow path"
    );
}

/// A config `cell_widths` override that widens a table char must disable
/// the table-char fast path for the whole flush: bulk only for ASCII.
#[test]
fn bulk_skips_table_chars_when_cell_widths_override() {
    let mut widths = std::collections::HashMap::new();
    // Make a Cyrillic letter and a box-drawing char 2 cells wide.
    widths.insert(0x0410u32, 2u8);
    widths.insert(0x2500u32, 2u8);
    let uv = UnicodeVersion {
        version: 9,
        ambiguous_are_wide: false,
        cell_widths: Some(Arc::new(widths)),
    };
    let stream: &str = "ab\u{410}\u{431}\u{2500}cd";

    let mut fast = make_term_uv(uv.clone(), false, false);
    fast.bulk_print_cells = 0;
    let mut slow = make_term_uv(uv, false, true);
    slow.bulk_print_cells = 0;

    fast.advance_bytes(stream.as_bytes());
    slow.advance_bytes(stream.as_bytes());

    // Same handback arithmetic as in the ambiguous_are_wide test:
    // only "a" + "cd" = 3 ASCII cells may be bulked.
    assert_eq!(
        fast.bulk_print_cells, 3,
        "with cell_widths overrides, only ASCII cells may be bulked"
    );
    assert_eq!(slow.bulk_print_cells, 0);
    assert_eq!(
        observe(&fast),
        observe(&slow),
        "cell_widths-overridden terminal state must match the slow path"
    );
}
