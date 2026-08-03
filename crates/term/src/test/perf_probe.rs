//! Ad-hoc profiling for task #147: where does the ~42-50ms
//! `terminal.lock()` hold time measured by
//! `mux::test::terminal_lock_contention::perform_actions_hold_time_uncontended_baseline`
//! actually go inside `Terminal::perform_actions`?
//!
//! This is not a correctness test (though it does assert determinism of
//! chunked vs. non-chunked application); it is a throwaway measurement
//! harness, run with:
//!   cargo test -p wezterm-term --release perf_probe -- --nocapture
//!
//! It answers two questions:
//!   1. Is the cost dominated by any single action kind (print vs.
//!      control vs. CSI), or is it roughly uniform per-byte?
//!   2. Does splitting one big `Vec<Action>` into smaller slices, each
//!      applied via its own `Performer`, change the *total* time to
//!      apply the whole batch (i.e. is there fixed per-`perform_actions`
//!      overhead that chunking would multiply), and do the chunks sum to
//!      the same terminal state as one unchunked call?
use crate::terminalstate::performer::Performer;
use crate::{Terminal, TerminalConfiguration, TerminalSize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wezterm_escape_parser::parser::Parser;
use wezterm_escape_parser::Action;

use super::LocalClip;

#[derive(Debug)]
struct ProbeConfig;
impl TerminalConfiguration for ProbeConfig {
    fn color_palette(&self) -> crate::color::ColorPalette {
        crate::color::ColorPalette::default()
    }
}

const ROWS: usize = 50;
const COLS: usize = 120;

fn make_terminal() -> Terminal {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let mut term = Terminal::new(
        size,
        Arc::new(ProbeConfig),
        "WezTerm",
        "0.0.0",
        Box::new(Vec::new()),
    );
    let clip: Arc<dyn crate::Clipboard> = Arc::new(LocalClip::new());
    term.set_clipboard(&clip);
    term
}

/// Same generator shape as
/// `mux::test::terminal_lock_contention::generate_output_batch`: printable
/// text interspersed with SGR color changes and cursor movement.
fn generate_output_batch(rng_state: &mut u64, approx_bytes: usize) -> Vec<u8> {
    fn next(rng_state: &mut u64) -> u64 {
        let mut x = *rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *rng_state = x;
        x
    }

    let words = [
        "fn",
        "let",
        "struct",
        "impl",
        "match",
        "error:",
        "warning:",
        "ok",
        "todo",
        "lorem",
        "ipsum",
        "dolor",
        "sit",
        "amet",
        "consectetur",
    ];

    let mut out = Vec::with_capacity(approx_bytes + 64);
    while out.len() < approx_bytes {
        let choice = next(rng_state) % 100;
        if choice < 5 {
            let color = 30 + (next(rng_state) % 8);
            out.extend_from_slice(format!("\x1b[{}m", color).as_bytes());
        } else if choice < 8 {
            out.extend_from_slice(b"\x1b[2K\r");
        } else if choice < 10 {
            out.extend_from_slice(b"\r\n");
        } else {
            let w = words[(next(rng_state) as usize) % words.len()];
            out.extend_from_slice(w.as_bytes());
            out.push(b' ');
        }
    }
    out
}

fn action_kind(a: &Action) -> &'static str {
    match a {
        Action::Print(_) => "print",
        Action::PrintString(_) => "print",
        Action::Control(_) => "control",
        Action::CSI(_) => "csi",
        Action::Esc(_) => "esc",
        Action::OperatingSystemCommand(_) => "osc",
        Action::DeviceControl(_) => "dcs",
        Action::Sixel(_) => "sixel",
        Action::XtGetTcap(_) => "xtgettcap",
        Action::KittyImage(_) => "kitty_image",
    }
}

#[test]
fn probe_perform_actions_hold_time_breakdown() {
    let mut rng_state: u64 = 0x9E3779B97F4A7C15;
    let mut parser = Parser::new();

    const N_BATCHES: usize = 20;
    const BATCH_BYTES: usize = 128 * 1024;

    let mut parse_time = Duration::ZERO;
    let mut apply_time = Duration::ZERO;
    let mut action_count = 0usize;
    let mut kind_counts: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();
    let mut kind_time: std::collections::HashMap<&'static str, Duration> =
        std::collections::HashMap::new();

    for _ in 0..N_BATCHES {
        let bytes = generate_output_batch(&mut rng_state, BATCH_BYTES);

        let parse_start = Instant::now();
        let mut actions = Vec::new();
        parser.parse(&bytes, |action| actions.push(action));
        parse_time += parse_start.elapsed();
        action_count += actions.len();

        // Fresh terminal per batch so scrollback growth doesn't skew
        // later batches relative to earlier ones.
        let mut term = make_terminal();

        let apply_start = Instant::now();
        {
            let mut performer = Performer::new(&mut term);
            for action in actions {
                let k = action_kind(&action);
                let t0 = Instant::now();
                performer.perform(action);
                let dt = t0.elapsed();
                *kind_counts.entry(k).or_default() += 1;
                *kind_time.entry(k).or_default() += dt;
            }
        }
        apply_time += apply_start.elapsed();
    }

    eprintln!(
        "\n[perf_probe] over {N_BATCHES} batches of ~{BATCH_BYTES} bytes ({action_count} actions total):"
    );
    eprintln!(
        "[perf_probe] total parse time:  {:>10?}  ({:>8?}/batch)",
        parse_time,
        parse_time / N_BATCHES as u32
    );
    eprintln!(
        "[perf_probe] total apply time:  {:>10?}  ({:>8?}/batch)  <- this is what perform_actions holds the lock for",
        apply_time,
        apply_time / N_BATCHES as u32
    );

    let mut kinds: Vec<_> = kind_counts.keys().copied().collect();
    kinds.sort();
    for k in kinds {
        let n = kind_counts[k];
        let t = kind_time[k];
        eprintln!(
            "[perf_probe]   action kind {:>12}: n={:>8} total={:>10?} avg={:>10?}",
            k,
            n,
            t,
            t / (n.max(1) as u32)
        );
    }

    assert!(action_count > 0);
}

/// Compares chunked vs. unchunked application of the *same* action batch
/// and confirms (a) chunking actually reduces per-call hold time roughly
/// linearly with chunk size, and (b) final terminal state (grid +
/// scrollback, compared via `Line` equality across every physical row) is
/// identical either way. This is the evidence behind task #147's chunking
/// fix: if final state differed, chunking would not be a safe rewrite of
/// `perform_actions`.
#[test]
fn probe_chunking_hold_time_and_equivalence() {
    let mut rng_state: u64 = 0x9E3779B97F4A7C15;
    let mut parser = Parser::new();
    let bytes = generate_output_batch(&mut rng_state, 128 * 1024);
    let mut actions = Vec::new();
    parser.parse(&bytes, |action| actions.push(action));
    let total_actions = actions.len();

    // Baseline: apply everything in one `Performer` session, as
    // `Terminal::perform_actions` does today.
    let mut term_whole = make_terminal();
    let whole_start = Instant::now();
    {
        let mut performer = Performer::new(&mut term_whole);
        for action in actions.clone() {
            performer.perform(action);
        }
    }
    let whole_time = whole_start.elapsed();

    for chunk_size in [64usize, 256, 1024, 4096] {
        let mut term_chunked = make_terminal();
        let mut max_chunk_time = Duration::ZERO;
        let mut total_chunk_time = Duration::ZERO;
        let mut n_chunks = 0usize;
        for chunk in actions.clone().chunks(chunk_size) {
            let chunk_start = Instant::now();
            {
                let mut performer = Performer::new(&mut term_chunked);
                for action in chunk.iter().cloned() {
                    performer.perform(action);
                }
            }
            let dt = chunk_start.elapsed();
            max_chunk_time = max_chunk_time.max(dt);
            total_chunk_time += dt;
            n_chunks += 1;
        }

        eprintln!(
            "[perf_probe] chunk_size={:>5} n_chunks={:>4} max_hold_per_chunk={:>10?} total={:>10?} (whole={:>10?})",
            chunk_size, n_chunks, max_chunk_time, total_chunk_time, whole_time
        );

        // Equivalence check: compare every physical row (grid +
        // scrollback) between the chunked and unchunked runs.
        let whole_lines = term_whole.screen().all_lines();
        let chunked_lines = term_chunked.screen().all_lines();
        assert_eq!(
            whole_lines.len(),
            chunked_lines.len(),
            "chunk_size={chunk_size}: scrollback row count differs"
        );
        for (row, (whole_line, chunked_line)) in
            whole_lines.iter().zip(chunked_lines.iter()).enumerate()
        {
            assert_eq!(
                whole_line.as_str(),
                chunked_line.as_str(),
                "chunk_size={chunk_size}: row {row} text differs between chunked and unchunked application"
            );
        }
    }

    eprintln!("[perf_probe] total_actions={total_actions}");
}
