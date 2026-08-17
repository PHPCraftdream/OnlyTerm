//! Load test for task #146: which caller actually dominates the wait to
//! acquire `LocalPane::terminal` (a `parking_lot::Mutex<Terminal>`) --
//! the pty output parser (`perform_actions`), simulated user input
//! (`key_down`/`mouse_event`), or the renderer (`with_lines_mut`)?
//!
//! This exists to give task #147 ("stop the renderer from holding
//! `terminal.lock()` for the whole frame") an evidence-based go/no-go
//! instead of an architectural guess. It drives a real `LocalPane` (a
//! real `wezterm_term::Terminal` behind the exact same `Mutex` used in
//! production) with:
//!   - one thread that continuously calls `perform_actions` with large
//!     batches of freshly-parsed output (mirrors `parse_buffered_data`'s
//!     `mux_output_parser_buffer_size`-sized batches feeding
//!     `send_actions_to_mux`),
//!   - one thread that hammers `key_down`/`mouse_event` at a high,
//!     steady rate (mirrors GUI-thread input dispatch),
//!   - the main thread repeatedly calling `with_lines_mut` over the
//!     full visible viewport and doing a small amount of per-line work
//!     to stand in for shaping/rasterization (mirrors
//!     `render/pane.rs`'s `LineRender`, which runs the real render work
//!     *inside* the lock in production).
//!
//! Rather than reading histogram values back out of the global
//! `metrics` recorder (this crate has no test-side exporter wired up,
//! and adding one would be a bigger, separate change), this test uses
//! `LocalPane::{perform_actions,with_lines_mut,key_down,mouse_event}_timed`
//! -- `#[cfg(test)]`-only accessors defined next to the production code
//! in `localpane.rs` -- which return exactly the wait time (interval
//! from deciding to lock to actually acquiring it) that the
//! `localpane.terminal_lock.wait.*` metrics record, without going
//! through the global recorder. Note that calling `pane.perform_actions(..)`
//! (the trait method) directly and timing the whole call from the test
//! would conflate wait time with hold time, since the call only returns
//! once the critical section has finished; the `_timed` accessors avoid
//! that by measuring only up to the moment `.lock()` returns.
use crate::localpane::LocalPane;
use crate::pane::Pane;
use parking_lot::Mutex;
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use std::io::{Read, Result as IoResult, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};
use wezterm_term::color::ColorPalette;
use wezterm_term::{
    KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, Terminal,
    TerminalConfiguration, TerminalSize,
};

/// A `Child` double that never exits on its own; `LocalPane` only needs
/// something that implements the trait so it can track process state,
/// it is never polled by this test.
#[derive(Debug)]
struct NeverExitChild;

impl Child for NeverExitChild {
    fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> IoResult<ExitStatus> {
        // Parked forever; the test process exits before this matters,
        // and `LocalPane` doesn't join this thread.
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

#[derive(Debug, Clone)]
struct NeverExitKiller;
impl ChildKiller for NeverExitKiller {
    fn kill(&mut self) -> IoResult<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(self.clone())
    }
}
impl ChildKiller for NeverExitChild {
    fn kill(&mut self) -> IoResult<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(NeverExitKiller)
    }
}

/// A `MasterPty` double; `LocalPane` needs one to construct, but this
/// test drives the terminal model directly via `perform_actions`, so
/// nothing here needs to actually round-trip bytes to a real pty.
struct FakeMasterPty {
    size: Mutex<PtySize>,
}

impl MasterPty for FakeMasterPty {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        *self.size.lock() = size;
        Ok(())
    }
    fn get_size(&self) -> anyhow::Result<PtySize> {
        Ok(*self.size.lock())
    }
    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        Ok(Box::new(std::io::empty()))
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
        Ok(Box::new(Vec::new()))
    }
}

#[derive(Debug)]
struct LoadTestConfig;
impl TerminalConfiguration for LoadTestConfig {
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

const ROWS: usize = 50;
const COLS: usize = 120;

fn make_pane() -> Arc<LocalPane> {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let terminal = Terminal::new(
        size,
        Arc::new(LoadTestConfig),
        "WezTerm",
        "0.0.0",
        Box::new(Vec::new()),
    );
    let pty = Box::new(FakeMasterPty {
        size: Mutex::new(PtySize {
            rows: ROWS as u16,
            cols: COLS as u16,
            pixel_width: 0,
            pixel_height: 0,
        }),
    });
    let writer = Box::new(Vec::new());
    Arc::new(LocalPane::new(
        1,
        terminal,
        Box::new(NeverExitChild),
        pty,
        writer,
        1,
        "loadtest".to_string(),
        None,
    ))
}

/// Generates one ~`mux_output_parser_buffer_size`-shaped batch of
/// realistic terminal output: printable text interspersed with SGR
/// color changes and cursor movement, similar to what a chatty CLI /
/// AI coding tool produces (this scenario is explicitly called out in
/// the prior architecture research backing this task, task #115/UP-42).
fn generate_output_batch(rng_state: &mut u64, approx_bytes: usize) -> Vec<u8> {
    fn next(rng_state: &mut u64) -> u64 {
        // xorshift64 -- fast, deterministic, no external dependency.
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
            // SGR color change
            let color = 30 + (next(rng_state) % 8);
            out.extend_from_slice(format!("\x1b[{}m", color).as_bytes());
        } else if choice < 8 {
            // Cursor movement
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

/// A minimal `WithPaneLines` consumer that stands in for the real
/// per-line render work (`render/pane.rs`'s `LineRender`): touching
/// each cell is not full shaping/rasterization, but it holds the lock
/// for a comparable, non-trivial amount of *work per visible line*,
/// which is the property that matters for this measurement (how long
/// the lock is *held* by the renderer, and therefore how long input
/// and the parser have to *wait* for it).
struct FauxRenderWork {
    touched_cells: usize,
}
impl crate::pane::WithPaneLines for FauxRenderWork {
    fn with_lines_mut(
        &mut self,
        _first_row: wezterm_term::StableRowIndex,
        lines: &mut [&mut wezterm_term::Line],
    ) {
        for line in lines.iter() {
            // Touch every cluster, similar in spirit to iterating
            // cells for shaping.
            for cluster in line.cluster(None) {
                self.touched_cells += cluster.text.len();
            }
        }
    }
}

struct WaitSamples {
    samples: Mutex<Vec<Duration>>,
}
impl WaitSamples {
    fn new() -> Self {
        Self {
            samples: Mutex::new(Vec::new()),
        }
    }
    fn record(&self, d: Duration) {
        self.samples.lock().push(d);
    }
    fn stats(&self) -> (usize, Duration, Duration, Duration) {
        let mut samples = self.samples.lock().clone();
        samples.sort();
        let n = samples.len();
        if n == 0 {
            return (0, Duration::ZERO, Duration::ZERO, Duration::ZERO);
        }
        let p50 = samples[n / 2];
        let p95 = samples[((n * 95) / 100).min(n - 1)];
        let max = samples[n - 1];
        (n, p50, p95, max)
    }
}

/// Runs the three concurrent access patterns against one `LocalPane`
/// for `duration` and returns per-path wait-time samples.
///
/// `render_row_span` controls how many rows `with_lines_mut` asks for
/// per call (the full viewport, matching production's per-frame
/// behavior of rendering all visible rows under one lock acquisition).
fn run_contention_load(
    duration: Duration,
    render_row_span: std::ops::Range<wezterm_term::StableRowIndex>,
) -> (
    WaitSamples,
    WaitSamples,
    WaitSamples,
    WaitSamples,
    WaitSamples,
) {
    let pane = make_pane();
    let stop = Arc::new(AtomicBool::new(false));
    let key_input_samples = Arc::new(WaitSamples::new());
    let perform_actions_samples = Arc::new(WaitSamples::new());
    let with_lines_samples = Arc::new(WaitSamples::new());
    // Hold-time (not wait-time) samples for perform_actions/with_lines_mut,
    // recorded purely to distinguish "this call waited a long time" from
    // "this call is the one that made everyone else wait", i.e. to
    // attribute contention to a specific caller rather than just
    // observing that contention exists.
    let perform_actions_hold_samples = Arc::new(WaitSamples::new());
    let with_lines_hold_samples = Arc::new(WaitSamples::new());

    let start_barrier = Arc::new(Barrier::new(3));

    // Parser thread: feeds large batches of output via perform_actions,
    // exactly like `send_actions_to_mux` does for real pty output.
    let parser_handle = {
        let pane = Arc::clone(&pane);
        let stop = Arc::clone(&stop);
        let samples = Arc::clone(&perform_actions_samples);
        let hold_samples = Arc::clone(&perform_actions_hold_samples);
        let barrier = Arc::clone(&start_barrier);
        std::thread::spawn(move || {
            let mut rng_state: u64 = 0x9E3779B97F4A7C15;
            let mut parser = termwiz::escape::parser::Parser::new();
            barrier.wait();
            while !stop.load(Ordering::Relaxed) {
                // mux_output_parser_buffer_size defaults to 128KiB; use
                // the same order of magnitude for a batch.
                let bytes = generate_output_batch(&mut rng_state, 128 * 1024);
                let mut actions = Vec::new();
                parser.parse(&bytes, |action| actions.push(action));

                let (waited, held) = pane.perform_actions_timed(actions);
                samples.record(waited);
                hold_samples.record(held);
            }
        })
    };

    // Input thread: hammers key_down/mouse_event at a high, steady
    // rate, mirroring GUI-thread dispatch during fast typing/scrolling.
    let input_handle = {
        let pane = Arc::clone(&pane);
        let stop = Arc::clone(&stop);
        let samples = Arc::clone(&key_input_samples);
        let barrier = Arc::clone(&start_barrier);
        std::thread::spawn(move || {
            let mut toggle = false;
            barrier.wait();
            while !stop.load(Ordering::Relaxed) {
                let waited = if toggle {
                    let (waited, _) = pane.key_down_timed(KeyCode::Char('x'), KeyModifiers::NONE);
                    waited
                } else {
                    let (waited, _) = pane.mouse_event_timed(MouseEvent {
                        kind: MouseEventKind::Move,
                        x: 10,
                        y: 5,
                        x_pixel_offset: 0,
                        y_pixel_offset: 0,
                        button: MouseButton::None,
                        modifiers: KeyModifiers::NONE,
                    });
                    waited
                };
                samples.record(waited);
                toggle = !toggle;
                // 5-10ms between input events, as specified for this
                // load test: alternate a fixed 5ms/10ms cadence so the
                // average lands in the middle of that range.
                let delay_ms = if toggle { 5 } else { 10 };
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
        })
    };

    // Render thread (standing in for the GUI paint path): repeatedly
    // asks for the full visible viewport, doing per-line work while
    // still holding the lock, exactly like `render/pane.rs` does today.
    start_barrier.wait();
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        let mut work = FauxRenderWork { touched_cells: 0 };
        let (waited, held) = pane.with_lines_mut_timed(render_row_span.clone(), &mut work);
        with_lines_samples.record(waited);
        with_lines_hold_samples.record(held);
        std::hint::black_box(work.touched_cells);
        // Roughly simulate a 60fps paint cadence between frames spent
        // outside the lock (shading, GPU submission, etc.).
        std::thread::sleep(Duration::from_millis(2));
    }

    stop.store(true, Ordering::Relaxed);
    parser_handle.join().unwrap();
    input_handle.join().unwrap();

    fn unwrap_samples(arc: Arc<WaitSamples>) -> WaitSamples {
        Arc::try_unwrap(arc).unwrap_or_else(|arc| WaitSamples {
            samples: Mutex::new(arc.samples.lock().clone()),
        })
    }

    (
        unwrap_samples(key_input_samples),
        unwrap_samples(perform_actions_samples),
        unwrap_samples(with_lines_samples),
        unwrap_samples(perform_actions_hold_samples),
        unwrap_samples(with_lines_hold_samples),
    )
}

#[test]
fn terminal_lock_wait_time_under_sustained_output_and_input() {
    let _mux_guard = super::MUX_TEST_GUARD.lock();

    // Note: this test drives `LocalPane` through the `_timed` test-only
    // accessors (`key_down_timed`, `mouse_event_timed`, etc.) rather
    // than the `Pane` trait methods directly, so it does not need a
    // `Mux` singleton installed even though the trait methods normally
    // call `Mux::get().record_input_for_current_identity()`.
    let (key_input, perform_actions, with_lines, perform_actions_hold, with_lines_hold) =
        run_contention_load(
            Duration::from_secs(5),
            0..(ROWS as wezterm_term::StableRowIndex),
        );

    let (n_key, p50_key, p95_key, max_key) = key_input.stats();
    let (n_parse, p50_parse, p95_parse, max_parse) = perform_actions.stats();
    let (n_render, p50_render, p95_render, max_render) = with_lines.stats();
    let (_, p50_parse_hold, p95_parse_hold, max_parse_hold) = perform_actions_hold.stats();
    let (_, p50_render_hold, p95_render_hold, max_render_hold) = with_lines_hold.stats();

    eprintln!(
        "terminal.lock() WAIT time -- key_input:      n={n_key:>6} p50={p50_key:>10?} p95={p95_key:>10?} max={max_key:>10?}"
    );
    eprintln!(
        "terminal.lock() WAIT time -- perform_actions: n={n_parse:>6} p50={p50_parse:>10?} p95={p95_parse:>10?} max={max_parse:>10?}"
    );
    eprintln!(
        "terminal.lock() WAIT time -- with_lines_mut:  n={n_render:>6} p50={p50_render:>10?} p95={p95_render:>10?} max={max_render:>10?}"
    );
    eprintln!(
        "terminal.lock() HOLD time -- perform_actions: p50={p50_parse_hold:>10?} p95={p95_parse_hold:>10?} max={max_parse_hold:>10?}"
    );
    eprintln!(
        "terminal.lock() HOLD time -- with_lines_mut:  p50={p50_render_hold:>10?} p95={p95_render_hold:>10?} max={max_render_hold:>10?}"
    );

    // Sanity: all three paths actually ran and produced samples; a
    // regression that starved one of the threads (e.g. a real
    // deadlock) would show up as zero samples here rather than a
    // silently-empty, falsely-reassuring test.
    assert!(n_key > 0, "key/mouse input path produced no samples");
    assert!(n_parse > 0, "perform_actions path produced no samples");
    assert!(n_render > 0, "with_lines_mut path produced no samples");
}

/// Single-threaded control for the measurement above: applies the same
/// 128KiB-batch workload to `perform_actions` with *no* contention from
/// input or the renderer, so the hold-time numbers in the contended
/// test can be checked against an uncontended baseline. If this
/// baseline is already tens of milliseconds, that confirms the
/// dominant cost is intrinsic to applying a large batch of actions to
/// the terminal model (screen mutation, dirty-row tracking, etc.), not
/// an artifact of scheduling/contention with the other two threads.
#[test]
fn perform_actions_hold_time_uncontended_baseline() {
    let _mux_guard = super::MUX_TEST_GUARD.lock();

    let pane = make_pane();
    let mut rng_state: u64 = 0x9E3779B97F4A7C15;
    let mut parser = termwiz::escape::parser::Parser::new();
    let samples = WaitSamples::new();

    for _ in 0..50 {
        let bytes = generate_output_batch(&mut rng_state, 128 * 1024);
        let mut actions = Vec::new();
        parser.parse(&bytes, |action| actions.push(action));
        let (_waited, held) = pane.perform_actions_timed(actions);
        samples.record(held);
    }

    let (n, p50, p95, max) = samples.stats();
    eprintln!(
        "terminal.lock() HOLD time -- perform_actions (uncontended, single-threaded): n={n:>6} p50={p50:>10?} p95={p95:>10?} max={max:>10?}"
    );
    assert!(n > 0);
}

/// Regression test for task #147: chunking `perform_actions` (splitting
/// a large batch of already-parsed actions into pieces, taking
/// `terminal.lock()` separately per piece via `LocalPane::perform_actions`'s
/// `perform_actions_chunked`) must both (a) actually bound per-acquisition
/// hold time to something far below the ~40-60ms baseline measured by
/// `perform_actions_hold_time_uncontended_baseline` for an unchunked
/// 128KiB batch, and (b) produce byte-for-byte identical terminal state
/// (every visible + scrollback row, compared via text content) to
/// applying the same batch unchunked in one call.
#[test]
fn perform_actions_chunking_bounds_hold_time_and_preserves_state() {
    let _mux_guard = super::MUX_TEST_GUARD.lock();

    let mut rng_state: u64 = 0x9E3779B97F4A7C15;
    let mut parser = termwiz::escape::parser::Parser::new();
    let bytes = generate_output_batch(&mut rng_state, 128 * 1024);
    let mut actions = Vec::new();
    parser.parse(&bytes, |action| actions.push(action));
    assert!(
        actions.len() > 1000,
        "sanity: expected a large batch of actions from a 128KiB payload, got {}",
        actions.len()
    );

    // Baseline: apply the whole batch in a single lock acquisition, as
    // `perform_actions` did before task #147 (this is exactly what
    // `perform_actions_hold_time_uncontended_baseline` measures, but we
    // need our own instance here so we can compare final grid/scrollback
    // state against the chunked run below).
    let whole_pane = make_pane();
    let (_waited, whole_hold) = whole_pane.perform_actions_timed(actions.clone());
    eprintln!("perform_actions_chunking: unchunked hold time = {whole_hold:?}");
    assert!(
        whole_hold > Duration::from_millis(5),
        "expected the unchunked baseline to reproduce task #146's multi-ms hold time \
         (got {:?}); if this fails, the workload generator or terminal size \
         changed enough that this test's premise (\"unchunked is slow\") no longer holds",
        whole_hold,
    );

    // Chunked: same batch, but through the production chunk size
    // (config::mux_output_parser_chunk_size's default, 256 actions).
    const CHUNK_SIZE: usize = 256;
    let chunked_pane = make_pane();
    let samples = chunked_pane.perform_actions_chunked_timed(actions.clone(), CHUNK_SIZE);
    assert!(
        samples.len() > 1,
        "expected batch of {} actions with chunk_size={CHUNK_SIZE} to produce more than \
         one chunk",
        actions.len()
    );

    let max_hold = samples
        .iter()
        .map(|(_wait, hold)| *hold)
        .max()
        .expect("at least one chunk");
    let total_hold: Duration = samples.iter().map(|(_wait, hold)| *hold).sum();
    eprintln!(
        "perform_actions_chunking: n_chunks={} max_hold_per_chunk={max_hold:?} \
         total_hold={total_hold:?} (unchunked={whole_hold:?})",
        samples.len()
    );

    // The core claim of task #147: chunking bounds the *per-acquisition*
    // hold time to a small fraction of the unchunked baseline. This used
    // to be asserted against a fixed absolute ceiling ("max_hold < 10ms"),
    // calibrated on an idle machine. Under CI/dev-box load (other tests
    // or builds competing for CPU), OS scheduler noise inflates *both*
    // `whole_hold` and `max_hold` by roughly the same wall-clock factor
    // (they're measured back-to-back, moments apart, under the same
    // momentary contention), so an absolute threshold false-positives
    // under load even though the chunking logic itself is unchanged --
    // this was observed directly (task #261): the same code produced
    // max_hold anywhere from ~3ms (idle-ish) to ~58ms (machine pinned at
    // 100% CPU by an unrelated process) in back-to-back runs.
    //
    // Comparing `max_hold` to `whole_hold` measured in the *same run*
    // self-calibrates against however fast/slow the machine happens to
    // be at test time. The ratio is what actually matters: healthy
    // chunking (chunk_size=256, ~488 chunks for a 128KiB batch) reduces
    // max single-hold time by a large factor regardless of ambient load,
    // because each chunk does ~1/488th of the work. Real measurements
    // taken for task #261 across a range of machine load (idle through
    // the machine pinned at 100% CPU by an unrelated process) show the
    // ratio (whole_hold / max_hold) for healthy chunking stays in the
    // 34x-264x range across 6 runs:
    //   unchunked=837ms    max_hold=3.2ms    ratio=264x  (idle-ish)
    //   unchunked=1222ms   max_hold=18.7ms   ratio=65x
    //   unchunked=1198ms   max_hold=35.4ms   ratio=34x
    //   unchunked=1239ms   max_hold=29.7ms   ratio=42x
    //   unchunked=1294ms   max_hold=27.2ms   ratio=48x
    //   unchunked=2062ms   max_hold=58.0ms   ratio=36x   (CPU pinned 100%)
    //
    // To confirm the ratio still has teeth (i.e. it would catch a real
    // regression, not just a tighter constant that stops testing
    // anything), the chunking was deliberately broken by setting
    // CHUNK_SIZE to ~60x its real value (15600 instead of 256, still
    // producing multiple chunks so the "more than one chunk" sanity
    // check still passes) and re-measured 4 times:
    //   unchunked=792ms    max_hold=106.6ms  ratio=7.4x  (broken chunking)
    //   unchunked=1242ms   max_hold=162.4ms  ratio=7.7x
    //   unchunked=1036ms   max_hold=184.7ms  ratio=5.6x
    //   unchunked=788ms    max_hold=108.1ms  ratio=7.3x
    //
    // That's a clean separation: healthy chunking never dropped below
    // 34x even under heavy ambient load, broken chunking never rose
    // above 7.7x even though the ratio measurement is itself sensitive
    // to load. Requiring at least an 8x reduction sits in the gap
    // between those two clusters, with >4x of headroom below the worst
    // healthy-case ratio actually observed, while still failing on the
    // broken-chunking case with margin to spare.
    const MIN_HOLD_TIME_REDUCTION_RATIO: u32 = 8;
    assert!(
        max_hold.saturating_mul(MIN_HOLD_TIME_REDUCTION_RATIO) < whole_hold,
        "expected chunked application (chunk_size={}) to reduce max per-acquisition \
         hold time by at least {}x versus the unchunked baseline measured in this \
         same run, but max_hold={:?} and unchunked={:?} (ratio={:.1}x); this compares \
         against the unchunked baseline measured moments earlier in the same run \
         specifically so it self-calibrates against ambient machine load -- see the \
         comment above for the real measured ratios (healthy: 34x-264x, deliberately \
         broken chunking: 5.6x-7.7x) that this threshold was calibrated against",
        CHUNK_SIZE,
        MIN_HOLD_TIME_REDUCTION_RATIO,
        max_hold,
        whole_hold,
        whole_hold.as_secs_f64() / max_hold.as_secs_f64(),
    );

    // Correctness: chunked and unchunked application of the same batch
    // must produce identical terminal state. Compare every row (visible
    // + scrollback) as text.
    fn dump_all_rows(pane: &Arc<LocalPane>) -> Vec<String> {
        let dims = pane.get_dimensions();
        let (_first_row, lines) = pane.get_lines(
            dims.scrollback_top
                ..dims.physical_top + dims.viewport_rows as wezterm_term::StableRowIndex,
        );
        lines.iter().map(|line| line.as_str().to_string()).collect()
    }

    let whole_rows = dump_all_rows(&whole_pane);
    let chunked_rows = dump_all_rows(&chunked_pane);
    assert_eq!(
        whole_rows.len(),
        chunked_rows.len(),
        "chunked and unchunked application produced different numbers of rows"
    );
    for (row, (whole_row, chunked_row)) in whole_rows.iter().zip(chunked_rows.iter()).enumerate() {
        assert_eq!(
            whole_row, chunked_row,
            "row {row} differs between chunked and unchunked application of the same batch"
        );
    }
}
