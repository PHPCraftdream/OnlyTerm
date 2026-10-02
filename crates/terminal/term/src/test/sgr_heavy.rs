//! Differential test for SGR-heavy output streams (coloured program
//! output: compilers, `ls --color`, `git diff`). Drives two terminals --
//! the normal fast path and one with `force_slow_print_path` set -- with
//! the same deterministic random stream dense in SGR sequences (16-colour,
//! 256-colour, truecolor, bold/italic/underline/other styles, resets,
//! compound `\e[1;4;31m` forms) embedded inside runs of ASCII and Cyrillic
//! text, at wrap boundaries, with insert mode, scroll-region and
//! left/right-margin toggles, DECAWM off, and randomly-sized chunk splits
//! of the byte stream. Compares the whole terminal state: text, per-cell
//! attributes, line lengths, wrap flags, cursor and `wrap_next` for every
//! physical row including scrollback.
//!
//! The random generator is a deterministic LCG; the seed is printed on
//! failure.
use super::*;
use k9::assert_equal as assert_eq;
use std::sync::Arc;

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

const ROWS: usize = 8;
const COLS: usize = 20;
const SCROLLBACK: usize = 40;

#[derive(Debug)]
struct SgrHeavyConfig;

impl TerminalConfiguration for SgrHeavyConfig {
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
    fn scrollback_size(&self) -> usize {
        SCROLLBACK
    }
}

fn make_term(force_slow: bool, conpty: bool) -> Terminal {
    let mut term = Terminal::new(
        TerminalSize {
            rows: ROWS,
            cols: COLS,
            pixel_width: COLS * 8,
            pixel_height: ROWS * 16,
            dpi: 0,
        },
        Arc::new(SgrHeavyConfig),
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

/// Generates a random byte stream dense in SGR sequences mixed with
/// printable text, movement and mode toggles.
fn gen_sgr_stream(rng: &mut Lcg, events: usize) -> Vec<u8> {
    const ASCII_WORDS: &[&str] = &[
        "error",
        "warn",
        "note",
        "src/main.rs",
        "->",
        "12",
        "%",
        "|",
        "x",
        "ok ",
    ];
    const CYRILLIC_WORDS: &[&str] = &["ошибка", "мир", "тест", "ж", "ЁЖИК "];

    fn sgr_params(rng: &mut Lcg) -> Vec<String> {
        let mut p = Vec::new();
        let n = 1 + rng.below(3);
        for _ in 0..n {
            match rng.below(10) {
                // 16-colour foreground / bright variant
                0 => p.push((30 + rng.below(8)).to_string()),
                1 => p.push((90 + rng.below(8)).to_string()),
                // 16-colour background
                2 => p.push((40 + rng.below(8)).to_string()),
                3 => p.push((100 + rng.below(8)).to_string()),
                // 256-colour fg / bg
                4 => p.push(format!("38;5;{}", rng.below(256))),
                5 => p.push(format!("48;5;{}", rng.below(256))),
                // truecolor fg / bg
                6 => p.push(format!(
                    "38;2;{};{};{}",
                    rng.below(256),
                    rng.below(256),
                    rng.below(256)
                )),
                7 => p.push(format!(
                    "48;2;{};{};{}",
                    rng.below(256),
                    rng.below(256),
                    rng.below(256)
                )),
                // styles + resets
                8 => p.push(
                    [
                        "0", "1", "2", "3", "4", "5", "7", "8", "9", "21", "22", "23", "24", "25",
                        "27", "28", "29", "39", "49", "53", "55",
                    ][rng.below(21)]
                    .to_string(),
                ),
                _ => p.push("0".to_string()),
            }
        }
        p
    }

    let mut out = Vec::new();
    for _ in 0..events {
        match rng.below(24) {
            // SGR, possibly compound, followed by a word.
            0..=5 => {
                let params = sgr_params(rng).join(";");
                out.extend_from_slice(format!("\x1b[{}m", params).as_bytes());
                let word = if rng.below(3) == 0 {
                    CYRILLIC_WORDS[rng.below(CYRILLIC_WORDS.len())]
                } else {
                    ASCII_WORDS[rng.below(ASCII_WORDS.len())]
                };
                out.extend_from_slice(word.as_bytes());
                out.push(b' ');
            }
            6 => out.extend_from_slice(b"\x1b[m"), // bare reset
            7 => out.push(b'\r'),
            8 => out.push(b'\n'),
            9 => out.extend_from_slice(
                format!(
                    "\x1b[{};{}H",
                    1 + rng.below(ROWS + 1),
                    1 + rng.below(COLS + 1)
                )
                .as_bytes(),
            ),
            10 => out.extend_from_slice(
                format!(
                    "\x1b[{};{};{};{};{}m",
                    1,
                    4,
                    31 + rng.below(6),
                    44 + rng.below(4),
                    rng.below(2)
                )
                .as_bytes(),
            ),
            11 => out.extend_from_slice(if rng.below(2) == 0 {
                b"\x1b[4h".as_slice()
            } else {
                b"\x1b[4l".as_slice()
            }), // IRM
            12 => out.extend_from_slice(if rng.below(2) == 0 {
                b"\x1b[?7h".as_slice()
            } else {
                b"\x1b[?7l".as_slice()
            }), // DECAWM
            13 => {
                // DECLRMM + DECSLRM margins
                if rng.below(2) == 0 {
                    out.extend_from_slice(b"\x1b[?69h");
                    let l = rng.below(4);
                    out.extend_from_slice(
                        format!("\x1b[{};{}s", l + 1, l + 6 + rng.below(COLS)).as_bytes(),
                    );
                } else {
                    out.extend_from_slice(b"\x1b[?69l");
                }
            }
            14 => {
                // DECSTBM scroll region
                let t = rng.below(3);
                out.extend_from_slice(
                    format!("\x1b[{};{}r", t + 1, t + 3 + rng.below(ROWS)).as_bytes(),
                );
            }
            15 => out.extend_from_slice(b"\x1b[K"),  // EL
            16 => out.extend_from_slice(b"\x1b[2K"), // EL 2
            17 => out.extend_from_slice("е\u{0301}".as_bytes()), // combining acute
            18 => out.extend_from_slice(b"\x1b]8;;http://example.com/x\x1b\\"),
            19 => out.extend_from_slice(b"\x1b]8;;\x1b\\"),
            20 => out.extend_from_slice(b"\x1bG"), // bogus esc, exercises flush paths
            21 => out.extend_from_slice(if rng.below(2) == 0 {
                b"\x1b(0".as_slice()
            } else {
                b"\x1b(B".as_slice()
            }), // charset designation
            22 => out.push(if rng.below(2) == 0 { 0x0e } else { 0x0f }), // SO/SI
            _ => out.extend_from_slice(b"\x1bP$q\x1b\\"), // DCS (DECRQSS)
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

fn run_case(seed: u64, conpty: bool, events: usize) {
    let mut rng = Lcg::new(seed ^ ((conpty as u64) << 32));
    let stream = gen_sgr_stream(&mut rng, events);

    let mut fast = make_term(false, conpty);
    let mut slow = make_term(true, conpty);

    let mut off = 0usize;
    while off < stream.len() {
        let step = 1 + rng.below(37);
        let end = (off + step).min(stream.len());
        fast.advance_bytes(&stream[off..end]);
        slow.advance_bytes(&stream[off..end]);
        off = end;
    }

    assert_eq!(
        observe(&fast),
        observe(&slow),
        "conpty={} seed={}",
        conpty,
        seed
    );
}

#[test]
fn sgr_heavy_streams_match_slow_path() {
    for conpty in [false, true] {
        for seed in 0..48u64 {
            run_case(seed, conpty, 500);
        }
    }
}

/// Like `sgr_heavy_streams_match_slow_path`, but each stream starts by
/// filling the screen with colour so the scroll path must repeatedly
/// materialize blank rows that carry a non-default (colored) pen -- the
/// OPT-3 regression guard for `Screen::scroll_up`'s colored-blank path.
#[test]
fn sgr_heavy_full_screen_scroll_matches_slow_path() {
    for conpty in [false, true] {
        for seed in 0..16u64 {
            let mut rng = Lcg::new(seed ^ 0xABCD ^ ((conpty as u64) << 32));
            let mut stream = Vec::new();
            // Truecolor + 16-colour fill, more lines than the screen can
            // hold, so every line feed scrolls with a colored pen active.
            for i in 0..(ROWS * 6) {
                stream.extend_from_slice(
                    format!(
                        "\x1b[38;2;{};{};{}m",
                        i * 7 % 256,
                        i * 13 % 256,
                        i * 29 % 256
                    )
                    .as_bytes(),
                );
                stream.extend_from_slice(b"\x1b[41mword \x1b[32mgreen \x1b[mplain\r\n");
            }
            let events = gen_sgr_stream(&mut rng, 600);
            stream.extend_from_slice(&events);

            let mut fast = make_term(false, conpty);
            let mut slow = make_term(true, conpty);
            let mut off = 0usize;
            while off < stream.len() {
                let step = 1 + rng.below(53);
                let end = (off + step).min(stream.len());
                fast.advance_bytes(&stream[off..end]);
                slow.advance_bytes(&stream[off..end]);
                off = end;
            }
            assert_eq!(
                observe(&fast),
                observe(&slow),
                "conpty={} seed={}",
                conpty,
                seed
            );
        }
    }
}

/// Independent (non-differential) oracle for the colored-blank scroll
/// path: every time colored output scrolls, the blank row at the bottom
/// must consist of explicit blank cells carrying that pen's SGR
/// attributes (xterm semantics: erased cells take the current
/// rendition), while scrolled-out rows keep their content. Holds for
/// both the Vec-storage and the OPT-3 clustered construction of the
/// blank row. Checks after *every* scrolled line so both the
/// fresh-line and the recycle-the-evicted-line paths are covered (the
/// latter only kicks in once the scrollback is full, so checking only
/// the final state would leave one of the two untested).
#[test]
fn colored_scroll_blank_rows_carry_pen_attrs() {
    fn expect_bottom_blank(term: &Terminal, pen: &CellAttributes, conpty: bool) {
        let all = term.screen().all_lines();
        let bottom = &all[all.len() - 1];
        assert_eq!(
            bottom.as_str(),
            " ".repeat(COLS),
            "conpty={} bottom row text after scroll",
            conpty
        );
        assert_eq!(bottom.len(), COLS, "conpty={} bottom row len", conpty);
        let cells: Vec<(usize, usize, CellAttributes)> = bottom
            .visible_cells()
            .map(|c| (c.cell_index(), c.width(), c.attrs().clone()))
            .collect();
        assert_eq!(
            cells.len(),
            COLS,
            "conpty={} every blank cell must be explicit",
            conpty
        );
        for (idx, width, attrs) in &cells {
            assert_eq!(*width, 1, "conpty={} cell {} width", conpty, idx);
            assert_eq!(
                attrs, pen,
                "conpty={} blank cell {} must carry the scroll pen",
                conpty, idx
            );
        }
    }

    for conpty in [false, true] {
        // More lines than the screen + scrollback can hold, so both the
        // fresh-bottom-line path and (once the scrollback is full) the
        // recycle-the-evicted-line path run during the loop.
        let total = SCROLLBACK + ROWS + 3;
        let mut term = make_term(false, conpty);
        for i in 0..total {
            term.advance_bytes(format!("[{}mrow {}[44m bg\r\n", 31 + (i % 6), i).as_bytes());
            let pen = term.pen().clone_sgr_only();
            assert!(
                pen != CellAttributes::blank(),
                "conpty={} pen must be colored at scroll time",
                conpty
            );
            // Before the screen is full the unwritten bottom rows are
            // implicit (empty) blanks, not pen-carrying explicit blanks;
            // the invariant below holds once scrolling has begun.
            if i >= ROWS {
                expect_bottom_blank(&term, &pen, conpty);
            }
        }
        // The scrolled-out rows keep their colored content.
        let all = term.screen().all_lines();
        let top_visible = &all[all.len() - ROWS];
        // The final CRLF scrolled once more, so the topmost visible row
        // holds line `total - ROWS + 1`, and its unwritten tail is the
        // pen-carrying blank fill of the row it was scrolled out of.
        assert_eq!(
            top_visible.as_str().trim_end(),
            format!("row {} bg", total - ROWS + 1).as_str(),
            "conpty={} top visible row text",
            conpty
        );
        assert_eq!(
            top_visible.len(),
            COLS,
            "conpty={} top visible row len",
            conpty
        );
    }
}
