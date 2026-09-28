//! Differential test for the bulk-ASCII fast path in
//! `Performer::flush_print` (see `ascii_bulk_run_len`/`print_ascii_run`).
//! Drives two terminals -- one using the fast path, one with
//! `force_slow_print_path` set so every character goes through the
//! original per-grapheme loop (`print_one_grapheme`) -- with the same
//! random byte streams, and compares the resulting screen contents,
//! cursor and `wrap_next` state. Covers plain ASCII, CR/LF, cursor moves,
//! SGR, OSC 8 hyperlinks, combining marks right after ASCII, CJK wide
//! chars, VS16 emoji, G0/G1 charset switches + SO/SI, insert mode,
//! DECAWM, DECLRMM/DECSLRM margins and scroll regions, crossed with NFC
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

#[derive(Debug)]
struct DiffConfig {
    nfc: bool,
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
        Arc::new(DiffConfig { nfc }),
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

    let mut out = Vec::new();
    for _ in 0..events {
        match rng.below(20) {
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

                fast.advance_bytes(&stream);
                slow.advance_bytes(&stream);

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
