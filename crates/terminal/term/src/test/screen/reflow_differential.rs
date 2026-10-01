//! Screen-level differential test for reflow: two identically fed
//! terminals must end up in identical whole-screen states, with one of
//! them resizing through the verbatim reference implementations
//! (`with_reference_reflow`) and the other through the production path.
use super::*;

/// Small deterministic generator; no external crate needed.
struct Lcg(u64);

impl Lcg {
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        lo + ((self.0 >> 33) as usize) % (hi - lo + 1)
    }
}

/// The tiny valid PNG the image tests use, for occasional kitty
/// graphics placement.
const TINY_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAsAAAALCAYAAACprHcmAAAACXBIWXMAAAGKAAABigEzlzBYAAAAOUlEQVQYlZXOwQ0AMAzCQEdi7yaT0xWAN7JuDCac2PQKYxflycOoICOKtPIuqFCg4/LzKxiz6xjyAYh9DR1sLUN1AAAAAElFTkSuQmCC";

/// One random terminal "chunk" of output, covering the feature list of
/// the OPT-4 step 3 design: words of ASCII, Cyrillic, CJK (width 2),
/// emoji (width 2), combining marks; SGR 16/256/24-bit colours and
/// underline colour (58); OSC 8 hyperlinks; OSC 133 semantic prompts;
/// kitty images; CR/LF/CRLF; long lines; coloured trailing spaces;
/// EL 0/1/2; ED 0/1; CUP; DECAWM off/on; occasional 1049 h/l.
struct Generator {
    rng: Lcg,
}

/// Upper bound on the number of words the colourful long-line episode
/// emits. Terminals of moderate width do not need `words * cols` words
/// (several thousand) to cover wrap and reflow; this cap keeps the line
/// spanning multiple terminal widths while bounding the episode bytes.
const LONG_LINE_WORD_CAP: usize = 32;

impl Generator {
    fn new(seed: u64) -> Self {
        Self {
            rng: Lcg(seed.wrapping_add(0x9e37_79b9_7f4a_7c15)),
        }
    }

    fn word(&mut self) -> String {
        let alphabet: &[&str] = &[
            "a",
            "b",
            "word",
            "кон",
            "Привет",
            "漢字",
            "界面",
            "👍",
            "🚀",
            "e\u{301}",
            "и\u{306}",
            "x",
        ];
        let mut word = String::new();
        let count = self.rng.range(1, 4);
        for _ in 0..count {
            word.push_str(alphabet[self.rng.range(0, alphabet.len() - 1)]);
        }
        word
    }

    /// A single deterministic "output episode" for the given terminal
    /// width, used identically for both terminals of a pair.
    fn episode(&mut self, cols: usize) -> String {
        let mut out = String::new();
        match self.rng.range(0, 7) {
            0 => {
                // Semantic prompt + input + output markers.
                out.push_str("\x1b]133;A\x1b\\");
                out.push_str(&self.word());
                out.push_str("\x1b]133;B\x1b\\");
                out.push_str(&self.word());
                out.push_str("\x1b]133;C\x1b\\");
                out.push_str(&self.word());
                out.push_str("\x1b]133;D\x1b\\");
            }
            1 => {
                // Hyperlinked words.
                out.push_str("\x1b]8;;https://example.invalid/a\x1b\\");
                out.push_str(&self.word());
                out.push_str("\x1b]8;;\x1b\\ ");
                out.push_str("\x1b]8;;https://example.invalid/b\x1b\\");
                out.push_str(&self.word());
                out.push_str("\x1b]8;;\x1b\\");
            }
            2 => {
                // Colourful long line: 2-5 terminal widths of coloured
                // words plus coloured trailing spaces. The total word
                // count is capped so that wide terminals emit far fewer
                // bytes while the line still wraps over several widths.
                let words = self.rng.range(2, 5);
                let total = (words * cols).min(LONG_LINE_WORD_CAP);
                for _ in 0..total {
                    match self.rng.range(0, 3) {
                        0 => out.push_str(&format!("\x1b[38;5;{}m", self.rng.range(0, 255))),
                        1 => out.push_str(&format!(
                            "\x1b[38;2;{};{};{}m",
                            self.rng.range(0, 255),
                            self.rng.range(0, 255),
                            self.rng.range(0, 255)
                        )),
                        _ => out.push_str(&format!(
                            "\x1b[58;2;{};{};{}m",
                            self.rng.range(0, 255),
                            self.rng.range(0, 255),
                            self.rng.range(0, 255)
                        )),
                    }
                    out.push_str(&self.word());
                    out.push(' ');
                }
                out.push_str("\x1b[41m   \x1b[0m");
            }
            3 => {
                // Kitty graphics image, occasionally.
                out.push_str(&format!("\x1b_Ga=T,t=d,f=100;{}\x1b\\", TINY_PNG_BASE64));
                out.push_str(&self.word());
            }
            4 => {
                // Cursor and erase traffic.
                out.push_str(&format!(
                    "\x1b[{};{}H",
                    self.rng.range(1, 20),
                    self.rng.range(1, 40)
                ));
                match self.rng.range(0, 5) {
                    0 => out.push_str("\x1b[K"),
                    1 => out.push_str("\x1b[1K"),
                    2 => out.push_str("\x1b[2K"),
                    3 => out.push_str("\x1b[J"),
                    _ => out.push_str("\x1b[1J"),
                }
                out.push_str(&self.word());
            }
            5 => {
                // Alternate screen round trip, occasionally.
                if self.rng.range(0, 1) == 0 {
                    out.push_str("\x1b[?1049h");
                    out.push_str(&self.word());
                    out.push_str("\x1b[?1049l");
                } else {
                    // DECAWM off/on.
                    out.push_str("\x1b[?7l");
                    out.push_str(&self.word());
                    out.push_str("\x1b[?7h");
                }
            }
            _ => {
                // Plain words with mixed line endings. Kept short so the
                // episode bytes stay small while still exercising CR/LF/CRLF.
                let lines = self.rng.range(1, 4);
                for _ in 0..lines {
                    for _ in 0..self.rng.range(1, 4) {
                        out.push_str(&self.word());
                        out.push(' ');
                    }
                    match self.rng.range(0, 2) {
                        0 => out.push('\r'),
                        1 => out.push('\n'),
                        _ => out.push_str("\r\n"),
                    }
                }
            }
        }
        out
    }
}

/// Snapshot of every observable bit of terminal state that the reflow
/// is supposed to preserve, so both terminals can be compared.
struct Snapshot {
    alt_active: bool,
    lines: Vec<Line>,
    scrollback_rows: usize,
    top_stable_row: StableRowIndex,
    physical_rows: usize,
    physical_cols: usize,
    cursor: CursorPosition,
    semantic_zones: Option<Vec<SemanticZone>>,
    changed_rows: Vec<StableRowIndex>,
}

impl Snapshot {
    fn take(term: &mut TestTerm, seqno: SequenceNo) -> Self {
        let semantic_zones = term.get_semantic_zones().ok();
        let screen = term.screen();
        Self {
            alt_active: term.is_alt_screen_active(),
            lines: screen.all_lines(),
            scrollback_rows: screen.scrollback_rows(),
            top_stable_row: screen.phys_to_stable_row_index(0),
            physical_rows: screen.physical_rows,
            physical_cols: screen.physical_cols,
            cursor: term.cursor_pos(),
            semantic_zones,
            changed_rows: screen.get_changed_stable_rows(
                screen.phys_to_stable_row_index(0)..screen.phys_to_stable_row_index(0) + 100_000,
                seqno,
            ),
        }
    }
}

fn assert_same(reference: &Snapshot, actual: &Snapshot, context: &str) {
    std::assert_eq!(
        reference.alt_active,
        actual.alt_active,
        "alt screen after {}",
        context
    );
    std::assert_eq!(
        reference.lines,
        actual.lines,
        "lines (left=reference, right=actual) after {}",
        context
    );
    std::assert_eq!(
        reference.scrollback_rows,
        actual.scrollback_rows,
        "scrollback after {}",
        context
    );
    std::assert_eq!(
        reference.top_stable_row,
        actual.top_stable_row,
        "top stable row after {}",
        context
    );
    std::assert_eq!(
        reference.physical_rows,
        actual.physical_rows,
        "rows after {}",
        context
    );
    std::assert_eq!(
        reference.physical_cols,
        actual.physical_cols,
        "cols after {}",
        context
    );
    std::assert_eq!(reference.cursor, actual.cursor, "cursor after {}", context);
    std::assert_eq!(
        reference.semantic_zones,
        actual.semantic_zones,
        "semantic zones after {}",
        context
    );
    std::assert_eq!(
        reference.changed_rows,
        actual.changed_rows,
        "changed rows after {}",
        context
    );
}

/// Feeds both terminals identical bytes and resizes only one of them
/// through the reference path; every observable state must match.
#[test]
fn reflow_fastpath_matches_reference_on_whole_terminal_state() {
    // 200 seeds keep the seed/scrollback/conpty matrix coverage while staying
    // inside the debug-mode runtime budget.
    for seed in 0..200u64 {
        let conpty = seed % 2 == 1;
        let mut gen = Generator::new(seed);
        let scrollback = [0usize, 5, 50, 400][seed as usize % 4];
        let cols = gen.rng.range(10, 140);
        let rows = gen.rng.range(3, 40);

        let mut reference = TestTerm::new(rows, cols, scrollback);
        let mut actual = TestTerm::new(rows, cols, scrollback);
        if conpty {
            reference.enable_conpty_quirks();
            actual.enable_conpty_quirks();
        }

        let mut sizes = vec![];
        for step in 0..12 {
            // Output before the resize.
            let bytes = gen.episode(cols);
            reference.print(&bytes);
            actual.print(&bytes);

            // 12 resizes per terminal: by one column, big jumps, and
            // down to 1 column and back.
            let new_cols = match step % 3 {
                0 => cols.max(2) - 1,
                1 => 1,
                _ => gen.rng.range(10, 140),
            };
            let new_rows = gen.rng.range(3, 40);
            sizes.push((new_cols, new_rows));
            let size = TerminalSize {
                rows: new_rows,
                cols: new_cols,
                ..Default::default()
            };
            let pre_resize_seqno = actual.current_seqno();
            resize_with_hook(&mut reference, &size, true);
            resize_with_hook(&mut actual, &size, false);

            let context = format!(
                "seed {} (conpty={}, scrollback={}) step {} sizes {:?}",
                seed, conpty, scrollback, step, sizes
            );
            assert_same(
                &Snapshot::take(&mut reference, pre_resize_seqno),
                &Snapshot::take(&mut actual, pre_resize_seqno),
                &context,
            );

            // Additional output following the resize must also match.
            let more = gen.episode(new_cols);
            reference.print(&more);
            actual.print(&more);
            assert_same(
                &Snapshot::take(&mut reference, pre_resize_seqno),
                &Snapshot::take(&mut actual, pre_resize_seqno),
                &format!("{} after post-resize output", context),
            );
        }
    }
}

/// Resizes `term`, routing the reflow through the verbatim reference
/// implementations when `reference` is set.
fn resize_with_hook(term: &mut TestTerm, size: &TerminalSize, reference: bool) {
    if reference {
        crate::screen::resize::with_reference_reflow(|| term.resize(*size));
    } else {
        term.resize(*size);
    }
}
