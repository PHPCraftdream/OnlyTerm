//! Measures two hot paths in isolation, with real OS objects:
//!
//! 1. **Key-compat process lookup.** Which executables run in a pane, asked on
//!    the GUI thread for Ctrl+letter / Shift+Enter chords. Compares the pane
//!    job's own process list with a machine-wide Toolhelp snapshot.
//! 2. **Render snapshot line clones.** `terminal_get_lines` clones every
//!    viewport row under the terminal lock each frame. Reports the clone cost
//!    and, for a typing-like and a full-redraw workload, how many of those
//!    rows were unchanged since the previous frame (what reuse could save).
//!
//! Needs the sideloaded `conpty.dll` and `OpenConsole.exe` (from
//! `assets/windows/conhost`) next to the example binary, like
//! `conpty_resize_cycle`. Not a test: it drives a real shell, and ends only
//! the processes it started itself.
//!
//! ```text
//! cargo run --release -p onlyterm-mux --example hot_path_bench -- 300
//! ```
//! Argument: iterations per measurement (default 300).

use onlyterm_mux::pane::ForEachPaneLogicalLine;
use onlyterm_mux::renderable::{
    terminal_for_each_logical_line_in_stable_range_mut, terminal_get_dirty_lines,
    terminal_get_lines,
};
use onlyterm_term::{Line, StableRowIndex, Terminal, TerminalSize};
use portable_pty::win::conpty::ConPtyMasterPty;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Stats {
    median: Duration,
    p95: Duration,
    max: Duration,
}

fn stats(mut samples: Vec<Duration>) -> Stats {
    samples.sort_unstable();
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    Stats {
        median: at(0.5),
        p95: at(0.95),
        max: *samples.last().unwrap(),
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn print_row(name: &str, s: &Stats) {
    println!(
        "  {:<46} median {:>8.3} ms   p95 {:>8.3} ms   max {:>8.3} ms",
        name,
        ms(s.median),
        ms(s.p95),
        ms(s.max)
    );
}

fn time<T>(iterations: usize, mut f: impl FnMut() -> T) -> Stats {
    // One warm-up call so first-use costs do not skew the median.
    std::hint::black_box(f());
    stats(
        (0..iterations)
            .map(|_| {
                let start = Instant::now();
                std::hint::black_box(f());
                start.elapsed()
            })
            .collect(),
    )
}

fn key_compat_lookup(iterations: usize) -> anyhow::Result<()> {
    println!("1. Key-compat process lookup (GUI thread, per affected chord)");
    let pair = NativePtySystem::default().openpty(PtySize::default())?;
    // A shell with a child, so the pane tree has more than one process.
    let mut cmd = CommandBuilder::new("cmd.exe");
    cmd.args(["/d", "/q", "/c", "ping", "-n", "60", "127.0.0.1"]);
    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);
    let pid = child.process_id().expect("spawned child has a pid");
    let master: Box<dyn portable_pty::MasterPty> = pair.master;
    let conpty = master
        .downcast_ref::<ConPtyMasterPty>()
        .expect("native pty on Windows is ConPTY");
    std::thread::sleep(Duration::from_millis(800));

    let job_names = conpty.child_process_names()?;
    let snapshot_names = onlyterm_procinfo::LocalProcessInfo::fresh_process_tree_exe_names(pid)?;
    println!(
        "  processes in the pane job: {:?}; in the snapshot tree: {:?}",
        job_names, snapshot_names
    );

    let snapshot = time(iterations, || {
        onlyterm_procinfo::LocalProcessInfo::fresh_process_tree_exe_names(pid)
    });
    let job = time(iterations, || conpty.child_process_names());
    print_row("old: machine-wide snapshot", &snapshot);
    print_row("new: pane job list", &job);
    println!(
        "  speed-up by median: {:.0}x",
        ms(snapshot.median) / ms(job.median).max(1e-6)
    );

    // Only our own child, by the handle we got when spawning it.
    child.kill()?;
    child.wait()?;
    Ok(())
}

fn size(cols: usize, rows: usize) -> TerminalSize {
    TerminalSize {
        rows,
        cols,
        pixel_width: cols * 8,
        pixel_height: rows * 16,
        dpi: 96,
    }
}

fn new_terminal(cols: usize, rows: usize) -> Terminal {
    Terminal::new(
        size(cols, rows),
        Arc::new(onlyterm_config::TermConfig::new()),
        "OnlyTerm",
        onlyterm_config::onlyterm_version(),
        Box::new(std::io::sink()),
    )
}

fn viewport(term: &mut Terminal, rows: usize) -> std::ops::Range<StableRowIndex> {
    let top = term.screen().visible_row_to_stable_row(0);
    top..top + rows as StableRowIndex
}

/// One full screen of `rows` lines, each `cols` wide, mixing SGR colours and
/// CJK so lines are not trivially compressible.
fn screenful(cols: usize, rows: usize, frame: usize) -> String {
    let mut out = String::from("\x1b[2J\x1b[H");
    for row in 0..rows {
        let body = format!(
            "\x1b[3{}m{:03} frame {:05} \x1b[0m status={} payload {}",
            (row + frame) % 8,
            row,
            frame,
            if (row + frame).is_multiple_of(2) {
                "busy"
            } else {
                "idle"
            },
            "界".repeat(8)
        );
        out.push_str(&body);
        let used = 30 + 16;
        out.push_str(&"x".repeat(cols.saturating_sub(used)));
        if row + 1 < rows {
            out.push_str("\r\n");
        }
    }
    out
}

/// What `get_render_snapshot` runs for the configured hyperlink rules before
/// it clones anything.
struct ApplyRules<'a> {
    rules: &'a [termwiz::hyperlink::Rule],
}

impl ForEachPaneLogicalLine for ApplyRules<'_> {
    fn with_logical_line_mut(
        &mut self,
        _: std::ops::Range<StableRowIndex>,
        lines: &mut [&mut Line],
    ) -> bool {
        Line::apply_hyperlink_rules(self.rules, lines);
        true
    }
}

fn snapshot_clones(iterations: usize) -> anyhow::Result<()> {
    println!("2. Render snapshot: cloning viewport rows under the terminal lock");
    for (cols, rows) in [(120usize, 40usize), (200, 60)] {
        let mut term = new_terminal(cols, rows);
        term.advance_bytes(screenful(cols, rows, 0));
        let range = viewport(&mut term, rows);
        let (stable_top, sample) = terminal_get_lines(&mut term, range.clone());
        let cells: usize = sample.iter().map(|line| line.len()).sum();
        println!(
            "  sanity: {} lines from stable row {} holding {} cells",
            sample.len(),
            stable_top,
            cells
        );
        let clone = time(iterations, || terminal_get_lines(&mut term, range.clone()));
        print_row(&format!("clone {}x{} viewport", cols, rows), &clone);
        println!(
            "  {:<46} {:.2} us per row",
            "",
            ms(clone.median) * 1000.0 / rows as f64
        );
        // The rest of what `get_render_snapshot` does under the same lock.
        let palette = time(iterations, || term.palette());
        print_row("palette clone", &palette);
        let seqno = term.current_seqno();
        let dirty = time(iterations, || {
            terminal_get_dirty_lines(&mut term, range.clone(), seqno)
        });
        print_row("dirty-lines query", &dirty);
        let dimensions = time(iterations, || {
            onlyterm_mux::renderable::terminal_get_dimensions(&mut term)
        });
        print_row("dimensions", &dimensions);
        let cursor = time(iterations, || {
            onlyterm_mux::renderable::terminal_get_cursor_position(&mut term)
        });
        print_row("cursor position", &cursor);

        // The hyperlink-rule pass: it runs under the same lock, before the
        // clone, on every row not yet scanned. Fresh content (a redraw, new
        // output) means every row is unscanned.
        let rules = onlyterm_config::configuration().hyperlink_rules.clone();
        println!("  hyperlink rules configured: {}", rules.len());
        let mut samples = Vec::new();
        for frame in 100..100 + iterations.min(100) {
            term.advance_bytes(screenful(cols, rows, frame));
            let range = viewport(&mut term, rows);
            let started = Instant::now();
            terminal_for_each_logical_line_in_stable_range_mut(
                &mut term,
                range,
                &mut ApplyRules { rules: &rules },
            );
            samples.push(started.elapsed());
        }
        let scan_fresh = stats(samples);
        print_row("hyperlink pass over a freshly drawn screen", &scan_fresh);
        let range = viewport(&mut term, rows);
        let scan_clean = time(iterations, || {
            terminal_for_each_logical_line_in_stable_range_mut(
                &mut term,
                range.clone(),
                &mut ApplyRules { rules: &rules },
            )
        });
        print_row("hyperlink pass over already scanned rows", &scan_clean);

        // Typing: one character per frame on the prompt row.
        let mut seqno = term.current_seqno();
        let mut unchanged = 0usize;
        let frames = 200;
        for i in 0..frames {
            term.advance_bytes(if i % 40 == 39 { "\r\n> " } else { "a" });
            let range = viewport(&mut term, rows);
            let dirty = terminal_get_dirty_lines(&mut term, range, seqno);
            unchanged += rows - (dirty.len() as usize).min(rows);
            seqno = term.current_seqno();
        }
        println!(
            "  {:<46} {:.1}% of rows unchanged per frame",
            "typing workload:",
            unchanged as f64 * 100.0 / (frames * rows) as f64
        );

        // Full-screen redraw every frame, like the profiler's TUI scenario.
        let mut seqno = term.current_seqno();
        let mut unchanged = 0usize;
        let frames = 50;
        for frame in 1..=frames {
            term.advance_bytes(screenful(cols, rows, frame));
            let range = viewport(&mut term, rows);
            let dirty = terminal_get_dirty_lines(&mut term, range, seqno);
            unchanged += rows - (dirty.len() as usize).min(rows);
            seqno = term.current_seqno();
        }
        println!(
            "  {:<46} {:.1}% of rows unchanged per frame",
            "full-redraw workload:",
            unchanged as f64 * 100.0 / (frames * rows) as f64
        );
    }
    Ok(())
}

/// Feeds `total` bytes built by repeating `unit` through the terminal model in
/// 64 KiB chunks (the production print budget) and reports throughput.
fn throughput(name: &str, unit: &str, total: usize) {
    let mut term = new_terminal(120, 40);
    let mut chunk = String::with_capacity(64 * 1024 + unit.len());
    while chunk.len() < 64 * 1024 {
        chunk.push_str(unit);
    }
    let lines_per_chunk = chunk.matches('\n').count();
    // Fill the scrollback first so the steady state is measured.
    for _ in 0..8 {
        term.advance_bytes(&chunk);
    }
    let rounds = total / chunk.len();
    let started = Instant::now();
    for _ in 0..rounds {
        term.advance_bytes(&chunk);
    }
    let elapsed = started.elapsed();
    let bytes = rounds * chunk.len();
    let lines = rounds * lines_per_chunk;
    let per_line = if lines > 0 {
        format!(
            "   {:>7.0} ns/line",
            elapsed.as_nanos() as f64 / lines as f64
        )
    } else {
        String::new()
    };
    println!(
        "  {:<34} {:>7.1} MiB/s   {:>6.2} ns/byte{}",
        name,
        bytes as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64(),
        elapsed.as_nanos() as f64 / bytes as f64,
        per_line
    );
}

fn model_throughput() {
    println!("3. Terminal model throughput (parse + apply, no rendering)");
    let total = 16 * 1024 * 1024;
    let ascii = "The quick brown fox jumps over the lazy dog 0123456789 ".repeat(2);
    throughput("ASCII lines, scrolling", &format!("{}\r\n", ascii), total);
    throughput(
        "ASCII, same row (no scroll)",
        &format!("{}\r", ascii),
        total,
    );
    throughput("empty lines (scroll only)", "\r\n", 2 * 1024 * 1024);
    let sgr =
        "\x1b[31mred \x1b[1;32mbold green \x1b[0;34mblue \x1b[38;5;208morange \x1b[0m".repeat(2);
    throughput("SGR-coloured lines", &format!("{}\r\n", sgr), total);
    // The same pieces apart, to see where the SGR cost comes from.
    let words = "red bold green blue orange ".repeat(2);
    throughput("  same words, no SGR", &format!("{}\r\n", words), total);
    let only_sgr = "\x1b[31m\x1b[1;32m\x1b[0;34m\x1b[38;5;208m\x1b[0m".repeat(2);
    throughput(
        "  the 10 SGR alone, no text",
        &format!("{}\r\n", only_sgr),
        total,
    );
    let simple = "\x1b[31m".repeat(10);
    throughput(
        "  10 x simple SGR 31, no text",
        &format!("{}\r\n", simple),
        total,
    );
    let same_pen = "word \x1b[39m".repeat(10);
    throughput(
        "  10 runs split by a no-op SGR",
        &format!("{}\r\n", same_pen),
        total,
    );
    let cursor = "word \x1b[?25h".repeat(10);
    throughput(
        "  10 runs split by show-cursor",
        &format!("{}\r\n", cursor),
        total,
    );
    let cyr = "\u{421}\u{44a}\u{435}\u{448}\u{44c} \u{436}\u{435} \u{435}\u{449}\u{451} \u{44d}\u{442}\u{438}\u{445} \u{431}\u{443}\u{43b}\u{43e}\u{43a} ".repeat(4);
    throughput("Cyrillic lines", &format!("{}\r\n", cyr), total);
    let cjk = "\u{754c}\u{6f22}\u{5b57}\u{65e5}\u{672c}\u{8a9e} ".repeat(8);
    throughput("CJK (wide) lines", &format!("{}\r\n", cjk), total);
    let long = "x".repeat(4000);
    throughput(
        "4000-column lines (wrapping)",
        &format!("{}\r\n", long),
        total,
    );
}

fn main() -> anyhow::Result<()> {
    let iterations = std::env::args()
        .nth(1)
        .map(|a| a.parse().expect("iterations"))
        .unwrap_or(300);
    key_compat_lookup(iterations)?;
    println!();
    snapshot_clones(iterations)?;
    println!();
    model_throughput();
    Ok(())
}
