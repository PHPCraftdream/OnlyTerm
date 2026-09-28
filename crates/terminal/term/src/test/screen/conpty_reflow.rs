//! ConPTY resize fidelity: `Screen::resize` must place rows and the cursor
//! exactly where OpenConsole 1.22 puts them, because ConPTY never repaints on
//! resize and positions later edits (e.g. cmd.exe's input echo) absolutely.
//!
//! `Conhost` below is a transcription of the bundled OpenConsole's resize
//! path (`ConhostInternalGetSet::ResizeWindow` -> `ResizeWithReflow` ->
//! `TextBuffer::Reflow`, microsoft/terminal v1.22.10352.0). The row numbers in
//! `native_captures` were read from the real bundled ConPTY with cmd.exe
//! (`GetConsoleScreenBufferInfo` on the child's console); the model is
//! checked against them first, then used as the oracle for random sequences.
use super::*;

/// One buffer row of the native console.
#[derive(Clone, Debug, Default)]
struct Row {
    text: Vec<char>,
    /// `ROW::WasWrapForced`: set once the last column is written, cleared by
    /// a line feed.
    wrapped: bool,
}

impl Row {
    fn as_string(&self) -> String {
        self.text.iter().collect::<String>().trim_end().to_string()
    }
}

/// The native console buffer of a ConPTY session (buffer == viewport).
struct Conhost {
    rows: Vec<Row>,
    width: usize,
    cursor: (usize, usize),
    /// Delayed EOL wrap: the last column was written, the cursor stays on it.
    delayed_wrap: bool,
}

impl Conhost {
    fn new(width: usize, height: usize, filled: bool) -> Self {
        let rows = (0..height)
            .map(|_| Row {
                text: vec![],
                wrapped: filled,
            })
            .collect();
        Self {
            rows,
            width,
            cursor: (0, 0),
            delayed_wrap: false,
        }
    }

    /// `ROW::MeasureRight`.
    fn measure(row: &Row, width: usize) -> usize {
        if row.wrapped {
            width
        } else {
            row.text
                .iter()
                .rposition(|c| *c != ' ')
                .map_or(0, |i| i + 1)
        }
    }

    fn line_feed(&mut self) {
        let y = self.cursor.1;
        self.rows[y].wrapped = false;
        if y + 1 == self.rows.len() {
            self.rows.remove(0);
            self.rows.push(Row::default());
        } else {
            self.cursor.1 += 1;
        }
    }

    /// Stream output with delayed wrap, as cmd.exe's `WriteConsole` does.
    fn write(&mut self, text: &str) {
        for c in text.chars() {
            match c {
                '\r' => {
                    self.cursor.0 = 0;
                    self.delayed_wrap = false;
                }
                '\n' => {
                    self.line_feed();
                    self.delayed_wrap = false;
                }
                c => {
                    if self.delayed_wrap {
                        let y = self.cursor.1;
                        self.line_feed();
                        let row = if self.cursor.1 == y { y - 1 } else { y };
                        self.rows[row].wrapped = true;
                        self.cursor.0 = 0;
                        self.delayed_wrap = false;
                    }
                    let (x, y) = self.cursor;
                    let row = &mut self.rows[y];
                    if row.text.len() <= x {
                        row.text.resize(x + 1, ' ');
                    }
                    row.text[x] = c;
                    if x + 1 == self.width {
                        row.wrapped = true;
                        self.delayed_wrap = true;
                    } else {
                        self.cursor.0 += 1;
                    }
                }
            }
        }
    }

    /// `TextBuffer::Reflow` into a `new_w` x `new_h` buffer.
    fn reflow(&mut self, new_w: usize, new_h: usize) {
        let old_w = self.width;
        let cx = self.cursor.0.min(old_w - 1);
        let cy = self.cursor.1.min(self.rows.len() - 1);
        let last_text = self
            .rows
            .iter()
            .rposition(|r| Self::measure(r, old_w) > 0)
            .unwrap_or(0);
        let old_height = last_text.max(cy) + 1;

        fn row_at(out: &mut Vec<Row>, y: usize) -> &mut Row {
            if out.len() <= y {
                out.resize_with(y + 1, Row::default);
            }
            &mut out[y]
        }

        let mut out: Vec<Row> = vec![];
        let (mut new_x, mut new_y) = (0, 0);
        let mut limit_y = usize::MAX;
        let mut new_cursor = (0, 0);
        let mut old_y = 0;
        while old_y < old_height && new_y < limit_y {
            let row = self.rows[old_y].clone();
            let mut limit = Self::measure(&row, old_w);
            if old_y == cy {
                limit = limit.max(cx + 1);
            }
            let mut old_x = 0;
            loop {
                if new_x >= new_w {
                    row_at(&mut out, new_y).wrapped = true;
                    new_x = 0;
                    new_y += 1;
                }
                if new_y >= new_h && new_x == 0 && new_y >= limit_y {
                    break;
                }
                let n = (limit - old_x).min(new_w - new_x);
                let dst = row_at(&mut out, new_y);
                if dst.text.len() < new_x + n {
                    dst.text.resize(new_x + n, ' ');
                }
                for i in 0..n {
                    dst.text[new_x + i] = row.text.get(old_x + i).copied().unwrap_or(' ');
                }
                if old_y == cy && cx >= old_x {
                    new_cursor = (cx - old_x + new_x, new_y);
                    limit_y = new_y + new_h;
                }
                old_x += n;
                new_x += n;
                if old_x >= limit {
                    break;
                }
            }
            if !row.wrapped {
                new_x = 0;
                new_y += 1;
            }
            old_y += 1;
        }
        if new_x != 0 {
            new_y += 1;
        }
        let drop = new_y.saturating_sub(new_h);
        self.rows = (drop..drop + new_h)
            .map(|y| out.get(y).cloned().unwrap_or_default())
            .collect();
        self.width = new_w;
        self.cursor = (new_cursor.0, new_cursor.1 - drop);
        self.delayed_wrap = false;
    }

    /// `ConhostInternalGetSet::ResizeWindow`.
    fn resize(&mut self, new_w: usize, new_h: usize) {
        let old = (self.width, self.rows.len());
        let tallest = old.1.max(new_h);
        if (new_w, tallest) != old {
            self.reflow(new_w, tallest);
        }
        if new_h < tallest {
            self.reflow(new_w, new_h);
        }
    }

    /// `CSI J` from `(x, y)`: `TextBuffer::FillRect` blanks the cells but
    /// keeps every row's wrap mark.
    fn erase_below(&mut self, (x, y): (usize, usize)) {
        for (row_y, row) in self.rows.iter_mut().enumerate().skip(y) {
            row.text.truncate(if row_y == y { x } else { 0 });
        }
    }
}

/// A cmd.exe session in both the native model and OnlyTerm's terminal: the
/// optional `color f8` fill, then `commands` echo commands and a prompt,
/// optionally with input still pending in cmd's cooked read.
struct Session {
    native: Conhost,
    term: TestTerm,
    pending: String,
    /// Where the pending input starts (cooked read's origin).
    origin: (usize, usize),
}

impl Session {
    fn new(
        cols: usize,
        rows: usize,
        color: bool,
        commands: usize,
        long_output: bool,
        pending: &str,
    ) -> Self {
        let mut native = Conhost::new(cols, rows, color);
        let mut term = TestTerm::new(rows, cols, 1000);
        term.enable_conpty_quirks();
        if color {
            super::resize::replay_conpty_color_fill(&mut term, rows, cols);
        }
        let mut text = "\r\n".to_string();
        for i in 0..commands {
            let output = if long_output && i % 3 == 1 {
                format!("{:0>150}", i)
            } else {
                format!("line{:02}", i)
            };
            text.push_str(&format!("PROBE>echo {}\r\n{}\r\n\r\n", output, output));
        }
        text.push_str("PROBE>");
        native.write(&text);
        term.print(&text);
        let origin = native.cursor;
        native.write(pending);
        term.print(pending);
        let session = Self {
            native,
            term,
            pending: pending.to_string(),
            origin,
        };
        session.assert_same("initial state");
        session
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        if !self.pending.is_empty() {
            // `COOKED_READ_DATA::EraseBeforeResize` parks the cursor on the
            // input origin so the reflow reports where it went.
            self.native.cursor = self.origin;
            self.native.delayed_wrap = false;
        }
        self.native.resize(cols, rows);
        self.term.resize(TerminalSize {
            rows,
            cols,
            ..Default::default()
        });
        if !self.pending.is_empty() {
            // `RedrawAfterResize`: erase from the new origin, rewrite the
            // input. ConPTY forwards exactly this VT.
            self.origin = self.native.cursor;
            let (x, y) = self.origin;
            self.native.erase_below(self.origin);
            let pending = self.pending.clone();
            self.native.write(&pending);
            self.term.print(format!(
                "\x1b[{};{}H\x1b[J\x1b[{};{}H{}",
                y + 1,
                x + 1,
                y + 1,
                x + 1,
                pending
            ));
        }
    }

    fn assert_same(&self, context: &str) {
        let cursor = self.term.cursor_pos();
        let native: Vec<String> = self.native.rows.iter().map(Row::as_string).collect();
        let model: Vec<String> = self
            .term
            .screen()
            .visible_lines()
            .iter()
            .map(|line| line.as_str().trim_end().to_string())
            .collect();
        std::assert_eq!(
            (cursor.x, cursor.y as usize),
            self.native.cursor,
            "cursor after {}",
            context
        );
        std::assert_eq!(model, native, "rows after {}", context);
    }

    /// What ConPTY sends when the user types: an absolute move to the
    /// native cursor, then the echo. It must land right after the prompt.
    fn assert_input_attached(&mut self, context: &str) {
        if self.native.delayed_wrap {
            // The next key wraps first; ConPTY's echo for that is not modelled.
            return;
        }
        let (x, y) = self.native.cursor;
        self.native.write("xyz");
        self.term.print(format!("\x1b[{};{}Hxyz", y + 1, x + 1));
        self.assert_same(&format!("typing after {}", context));
        let expected = format!("PROBE>{}xyz", self.pending);
        if self.origin.0 + self.pending.len() + 3 <= self.native.width {
            let row = self.term.screen().visible_lines()[y].as_str().to_string();
            std::assert!(
                row.trim_end().ends_with(&expected),
                "input detached from the prompt after {}: row {} is {:?}",
                context,
                y,
                row
            );
        }
    }
}

/// Native cursor rows read from the bundled ConPTY with `cmd /k "chcp
/// 65001>nul & color f8"` at 120x48 after `commands` echo commands.
#[test]
fn native_captures() {
    // (commands, sizes, native prompt row after each size)
    type Case = (usize, &'static [(usize, usize)], &'static [usize]);
    let cases: &[Case] = &[
        // Width shrink only: the fill below the prompt is one soft-wrapped
        // line, so narrowing pushes the prompt up.
        (10, &[(119, 48)], &[30]),
        (10, &[(100, 48)], &[27]),
        (10, &[(80, 48)], &[22]),
        (10, &[(60, 48)], &[14]),
        (3, &[(100, 48)], &[2]),
        (3, &[(80, 48)], &[0]),
        (13, &[(80, 48)], &[36]),
        (13, &[(60, 48)], &[32]),
        (15, &[(60, 48)], &[44]),
        (20, &[(60, 48)], &[47]),
        // The reported bug: wider, then shorter, then back (display wake).
        (10, &[(160, 48), (160, 43), (120, 48)], &[31, 31, 31]),
        (
            10,
            &[
                (80, 24),
                (160, 53),
                (80, 24),
                (90, 28),
                (110, 42),
                (120, 48),
            ],
            &[0, 0, 0, 0, 0, 0],
        ),
        (
            12,
            &[
                (103, 43),
                (60, 51),
                (137, 27),
                (134, 38),
                (134, 42),
                (65, 24),
                (120, 48),
            ],
            &[31, 31, 20, 20, 20, 14, 14],
        ),
        (
            12,
            &[
                (139, 44),
                (165, 24),
                (143, 47),
                (113, 29),
                (62, 46),
                (106, 52),
                (120, 48),
            ],
            &[35, 18, 18, 18, 18, 18, 18],
        ),
    ];
    for (commands, sizes, native_rows) in cases {
        let mut session = Session::new(120, 48, true, *commands, false, "");
        for (&(cols, rows), &row) in sizes.iter().zip(native_rows.iter()) {
            session.resize(cols, rows);
            let context = format!("{} commands, {}x{} in {:?}", commands, cols, rows, sizes);
            std::assert_eq!(
                session.native.cursor,
                (6, row),
                "model vs capture: {}",
                context
            );
            session.assert_same(&context);
        }
        session.assert_input_attached(&format!("{:?}", sizes));
    }
}

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

#[test]
fn random_resizes_match_native_model() {
    for seed in 0..300u64 {
        let mut rng = Lcg(seed.wrapping_add(0x9e37_79b9_7f4a_7c15));
        let color = seed % 4 != 3;
        let long_output = seed % 5 == 2;
        let pending = ["", "abc", "", "a longer pending command line"][seed as usize % 4];
        let commands = rng.range(0, 20);
        let (cols, rows) = (rng.range(40, 160), rng.range(8, 50));
        let mut session = Session::new(cols, rows, color, commands, long_output, pending);
        let mut sizes = vec![];
        for _ in 0..10 {
            // Mix one-row nudges (display power cycles) with large jumps.
            let (cols, rows) = if rng.range(0, 2) == 0 {
                let (c, r) = (session.native.width, session.native.rows.len());
                (
                    c,
                    if rng.range(0, 1) == 0 {
                        r + 1
                    } else {
                        r.max(2) - 1
                    },
                )
            } else {
                (rng.range(20, 170), rng.range(3, 60))
            };
            sizes.push((cols, rows));
            session.resize(cols, rows);
            session.assert_same(&format!(
                "seed {} (color={}, long={}, commands={}, pending={:?}) sizes {:?}",
                seed, color, long_output, commands, pending, sizes
            ));
        }
        session.assert_input_attached(&format!("seed {}", seed));
    }
}

#[test]
fn line_feed_clears_conpty_wrap_mark() {
    let mut term = TestTerm::new(4, 5, 0);
    term.enable_conpty_quirks();
    term.print("abcde");
    std::assert!(
        term.screen().visible_lines()[0].last_cell_was_wrapped(),
        "writing the last column marks the row, like ConPTY"
    );
    term.print("\r\n");
    std::assert!(!term.screen().visible_lines()[0].last_cell_was_wrapped());

    // A fill painted row by row with absolute moves stays marked.
    term.print("\x1b[2;1Hvwxyz\x1b[3;1H     ");
    std::assert!(term.screen().visible_lines()[1].last_cell_was_wrapped());
    std::assert!(term.screen().visible_lines()[2].last_cell_was_wrapped());

    // IND and NEL are line feeds too.
    term.print("\x1b[2;1H\x1bD");
    std::assert!(!term.screen().visible_lines()[1].last_cell_was_wrapped());
    term.print("\x1b[3;1H\x1bE");
    std::assert!(!term.screen().visible_lines()[2].last_cell_was_wrapped());
}

#[test]
fn wrap_mark_on_last_column_is_conpty_only() {
    let mut term = TestTerm::new(4, 5, 0);
    term.print("abcde\x1b[2;1H");
    std::assert!(
        !term.screen().visible_lines()[0].last_cell_was_wrapped(),
        "other terminals mark a row only once text actually wraps"
    );
    term.print("fghij\r\n");
    std::assert!(!term.screen().visible_lines()[1].last_cell_was_wrapped());

    let mut term = TestTerm::new(4, 5, 0);
    term.enable_conpty_quirks();
    term.set_auto_wrap(false);
    term.print("abcde");
    std::assert!(
        !term.screen().visible_lines()[0].last_cell_was_wrapped(),
        "without DECAWM ConPTY does not mark the row either"
    );
}

/// The whole buffer below the prompt is one soft-wrapped line after
/// `color f8`, including the bottom row, which has no successor to join.
#[test]
fn conpty_fill_keeps_prompt_through_width_shrink() {
    let mut term = TestTerm::new(10, 12, 1000);
    term.enable_conpty_quirks();
    super::resize::replay_conpty_color_fill(&mut term, 10, 12);
    term.print("\r\nPROBE>");
    term.resize(TerminalSize {
        rows: 10,
        cols: 10,
        ..Default::default()
    });
    // 9 rows x 12 cells reflow into 11 rows; ConPTY stops 10 rows below
    // the cursor, which therefore reaches the top.
    std::assert_eq!((term.cursor_pos().x, term.cursor_pos().y), (6, 0));
    std::assert_eq!(
        term.screen().visible_lines()[0].as_str().trim_end(),
        "PROBE>"
    );
    term.print("\x1b[1;7Hxyz");
    std::assert_eq!(
        term.screen().visible_lines()[0].as_str().trim_end(),
        "PROBE>xyz"
    );
}

/// cmd's cooked read redraws pending input after every resize with
/// `CSI J`; ConPTY's erase (`TextBuffer::FillRect`) keeps wrap marks, so
/// the rows below the prompt still reflow as one line afterwards.
#[test]
fn conpty_erase_keeps_wrap_marks() {
    let mut term = TestTerm::new(4, 5, 0);
    term.enable_conpty_quirks();
    for row in 1..=4 {
        term.print(format!("\x1b[{};1Habcde", row));
    }
    let wrapped = |term: &TestTerm| -> Vec<bool> {
        term.screen()
            .visible_lines()
            .iter()
            .map(|line| line.last_cell_was_wrapped())
            .collect()
    };
    std::assert_eq!(wrapped(&term), vec![true; 4]);

    term.print("\x1b[1;3H\x1b[K\x1b[2;1H\x1b[2K\x1b[3;5H\x1b[X");
    std::assert_eq!(wrapped(&term), vec![true; 4], "EL and ECH");
    term.print("\x1b[2;2H\x1b[J\x1b[3;2H\x1b[1J");
    std::assert_eq!(wrapped(&term), vec![true; 4], "ED below and above");
    std::assert_eq!(term.screen().visible_lines()[3].as_str().trim_end(), "");

    term.print("\x1b[2J");
    std::assert_eq!(wrapped(&term), vec![false; 4], "a full erase resets rows");
}
