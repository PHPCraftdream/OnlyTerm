//! Throughput benchmarks for the output-apply and resize paths. Ignored by
//! default; compare runs before and after a change with:
//!   cargo test -p onlyterm-term --release --lib perf_bench -- --ignored --nocapture --test-threads=1
use crate::terminalstate::performer::Performer;
use crate::{Terminal, TerminalConfiguration, TerminalSize};
use onlyterm_escape_parser::parser::Parser;
use onlyterm_escape_parser::Action;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::LocalClip;

const ROWS: usize = 50;
const COLS: usize = 120;
const SCROLLBACK: usize = 3500;

#[derive(Debug)]
struct BenchConfig;
impl TerminalConfiguration for BenchConfig {
    fn color_palette(&self) -> crate::color::ColorPalette {
        crate::color::ColorPalette::default()
    }
    fn scrollback_size(&self) -> usize {
        SCROLLBACK
    }
}

fn size(rows: usize, cols: usize) -> TerminalSize {
    TerminalSize {
        rows,
        cols,
        pixel_width: cols * 8,
        pixel_height: rows * 16,
        dpi: 0,
    }
}

fn make_terminal() -> Terminal {
    let mut term = Terminal::new(
        size(ROWS, COLS),
        Arc::new(BenchConfig),
        "OnlyTerm",
        "0.0.0",
        Box::new(Vec::new()),
    );
    let clip: Arc<dyn crate::Clipboard> = Arc::new(LocalClip::new());
    term.set_clipboard(&clip);
    term
}

/// Parses the way the mux does: consecutive prints merge into `PrintString`.
fn mux_actions(bytes: &[u8]) -> Vec<Action> {
    let mut parser = Parser::new();
    let mut actions = Vec::new();
    parser.parse(bytes, |a| a.append_to(&mut actions));
    actions
}

fn apply_throughput(name: &str, bytes: &[u8], reps: usize) {
    let mut parse = Duration::ZERO;
    let mut apply = Duration::ZERO;
    for _ in 0..reps {
        let start = Instant::now();
        let actions = mux_actions(bytes);
        parse += start.elapsed();
        let mut term = make_terminal();
        let start = Instant::now();
        {
            let mut performer = Performer::new(&mut term);
            for action in actions {
                performer.perform(action);
            }
        }
        apply += start.elapsed();
    }
    let mb = (bytes.len() * reps) as f64 / 1e6;
    eprintln!(
        "[perf_bench] {:<26} parse {:>7.1} MB/s  apply {:>7.1} MB/s  ({:?}/rep)",
        name,
        mb / parse.as_secs_f64(),
        mb / apply.as_secs_f64(),
        apply / reps as u32
    );
}

fn text_lines(n: usize, width: usize) -> Vec<u8> {
    let words = [
        "lorem", "ipsum", "dolor", "sit", "amet", "error:", "fn", "let",
    ];
    let mut out = Vec::new();
    let mut i = 0usize;
    for _ in 0..n {
        let mut line = String::new();
        while line.len() + 8 < width {
            line.push_str(words[i % words.len()]);
            line.push(' ');
            i += 1;
        }
        out.extend_from_slice(line.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

#[test]
#[ignore]
fn perf_bench_apply() {
    let reps = 20;
    let plain = text_lines(1500, 110);
    apply_throughput("plain ascii + CRLF", &plain, reps);

    let cyrillic = String::from_utf8(plain.clone())
        .unwrap()
        .replace("lorem", "привет")
        .replace("ipsum", "мир")
        .into_bytes();
    apply_throughput("cyrillic mix + CRLF", &cyrillic, reps);

    // A line, then erase it (progress bars, prompts).
    let mut erase_per_line = Vec::new();
    for chunk in plain.split(|b| *b == b'\n') {
        erase_per_line.extend_from_slice(chunk);
        erase_per_line.extend_from_slice(b"\x1b[2K\r\n");
    }
    apply_throughput("text + EL(2K) per line", &erase_per_line, reps);

    // Full-screen repaint with absolute moves and EL, as ConPTY emits.
    let line = &plain[..110];
    let mut redraw = Vec::new();
    for _ in 0..30 {
        for row in 1..=ROWS {
            redraw.extend_from_slice(format!("\x1b[{};1H", row).as_bytes());
            redraw.extend_from_slice(line);
            redraw.extend_from_slice(b"\x1b[K");
        }
    }
    apply_throughput("CUP+text+EL0 redraw", &redraw, reps);

    let mut erase_then_write = Vec::new();
    erase_then_write.extend_from_slice(&plain[..ROWS * 112]);
    for _ in 0..200 {
        for row in 1..=ROWS {
            erase_then_write.extend_from_slice(format!("\x1b[{};1H\x1b[2K", row).as_bytes());
            erase_then_write.extend_from_slice(&line[..100]);
        }
    }
    apply_throughput("CUP+EL2+text", &erase_then_write, reps);

    let mut sgr = Vec::new();
    for (i, word) in String::from_utf8(plain.clone())
        .unwrap()
        .split(' ')
        .enumerate()
    {
        sgr.extend_from_slice(format!("\x1b[{}m{} ", 31 + (i % 7), word).as_bytes());
    }
    apply_throughput("SGR per word", &sgr, reps);
}

#[test]
#[ignore]
fn perf_bench_resize() {
    for conpty in [false, true] {
        let mut term = make_terminal();
        if conpty {
            term.enable_conpty_quirks();
        }
        term.advance_bytes(text_lines(SCROLLBACK + 500, 110));

        let steps = 20;
        let start = Instant::now();
        for step in 0..steps {
            let cols = if step % 2 == 0 { 100 } else { COLS };
            term.resize(size(ROWS, cols));
        }
        eprintln!(
            "[perf_bench] resize width,  {} rows history (conpty={}): {:?}/step",
            SCROLLBACK,
            conpty,
            start.elapsed() / steps
        );

        let start = Instant::now();
        for step in 0..steps {
            let rows = if step % 2 == 0 { ROWS - 2 } else { ROWS };
            term.resize(size(rows, COLS));
        }
        eprintln!(
            "[perf_bench] resize height, {} rows history (conpty={}): {:?}/step",
            SCROLLBACK,
            conpty,
            start.elapsed() / steps
        );
    }
}
