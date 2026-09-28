//! Tests for the pty-read buffer pool (task C3.2): pool reuse, byte
//! integrity/order across varied read sizes, forward-channel backpressure
//! under a stalled parser, and shutdown in both directions.
use super::*;
use crate::pane::Pane;
use crate::test::RecordingPane;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{mpsc, Arc, Weak};
use std::time::Duration;

/// Takes the crate's test locks in the order every other test uses
/// (`TEST_LOCK`, then `MUX_TEST_GUARD`); the reverse order deadlocks
/// against them in a parallel run.
fn lock_test_globals() -> (
    parking_lot::MutexGuard<'static, ()>,
    parking_lot::MutexGuard<'static, ()>,
) {
    let test_lock = crate::test::TEST_LOCK.lock();
    let mux_guard = crate::test::MUX_TEST_GUARD.lock();
    (test_lock, mux_guard)
}

// A watchdog bounds a broken (deadlocked) implementation, not a
// performance assertion -- mirrors `wedged_pane_isolation.rs`.
const WATCHDOG: Duration = Duration::from_secs(30);

/// Configures the promise schedulers the first time any test in this file
/// runs. `send_actions_to_mux` -> `Mux::notify_from_any_thread` requires a
/// scheduler to be installed; there is no `Mux` in these tests, so running
/// the notification inline is a no-op. Mirrors `crate::test::start_parser`.
fn init_schedulers_once() {
    static SCHEDULER: std::sync::Once = std::sync::Once::new();
    SCHEDULER.call_once(|| {
        onlyterm_promise::spawn::set_schedulers(
            Box::new(|runnable| {
                runnable.run();
            }),
            Box::new(|runnable| {
                runnable.run();
            }),
        );
    });
}

/// A `Read` double that replays a fixed byte buffer through a caller-chosen,
/// cycling sequence of read sizes, so a single logical stream gets split
/// across the reader's `BUFSIZE` scratch buffer in varied, deterministic
/// chunks -- exactly the "many reads of varying sizes" scenario the
/// buffer pool needs to survive intact. Counts how many `read()` calls
/// returned data, independent of whether the parser has consumed it, so
/// backpressure tests can observe the READER thread's own progress.
struct ScriptedReader {
    data: Vec<u8>,
    pos: usize,
    sizes: Vec<usize>,
    size_idx: usize,
    reads_dispatched: Arc<AtomicUsize>,
}

impl std::io::Read for ScriptedReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let want = self.sizes[self.size_idx % self.sizes.len()].max(1);
        self.size_idx += 1;
        let n = want.min(out.len()).min(self.data.len() - self.pos);
        out[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        self.reads_dispatched.fetch_add(1, AtomicOrdering::SeqCst);
        Ok(n)
    }
}

/// Polls `pane`'s reconstructed printed text until it reaches `want_len`
/// bytes or `timeout` elapses, then returns whatever text has accumulated
/// so far (which the caller then asserts against). `read_from_pane_pty`
/// returning only means the READER side is done; the parser thread it
/// spawns keeps running independently (there is no join handle for it
/// here) and may still be draining/flushing its last few buffered
/// messages, so tests must wait for the parser to actually catch up
/// rather than assume it's finished the instant the reader thread exits.
fn wait_for_printed_text(pane: &RecordingPane, want_len: usize, timeout: Duration) -> String {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let text = reconstruct_printed_text(&pane.flattened_actions());
        if text.len() >= want_len || std::time::Instant::now() >= deadline {
            return text;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Every action in a pure-printable-ASCII payload must come back as
/// `Print`/`PrintString` (see `Action::append_to`, which coalesces runs of
/// `Print` into `PrintString`); anything else means the parser saw
/// something it interpreted as a control/escape sequence, which would mean
/// the test payload leaked a byte it shouldn't have.
fn reconstruct_printed_text(actions: &[Action]) -> String {
    let mut out = String::new();
    for action in actions {
        match action {
            Action::Print(c) => out.push(*c),
            Action::PrintString(s) => out.push_str(s),
            other => panic!(
                "unexpected non-print action in pure-ASCII payload: {:?}",
                other
            ),
        }
    }
    out
}

/// Unit-level proof that `take_pooled_buffer`/`recycle_buffer` reuse a
/// `Vec`'s actual allocation (not just its bytes), clear it before handing
/// it back out, and drop rather than pool an oversized buffer.
#[test]
fn pool_reuses_buffer_and_caps_oversized() {
    let (ret_tx, ret_rx) = crossbeam::channel::bounded::<Vec<u8>>(RETURN_CHANNEL_CAPACITY);

    // Empty pool -> fresh allocation sized to BUFSIZE.
    let first = take_pooled_buffer(&ret_rx);
    assert_eq!(first.capacity(), BUFSIZE);
    assert!(first.is_empty());

    let mut used = first;
    used.extend_from_slice(b"hello");
    let ptr = used.as_ptr();
    recycle_buffer(&ret_tx, used);

    let reused = take_pooled_buffer(&ret_rx);
    assert_eq!(reused.as_ptr(), ptr, "must reuse the exact same allocation");
    assert!(reused.is_empty(), "reused buffer must come back cleared");

    // Oversized buffer must be dropped, not pooled.
    let oversized = Vec::<u8>::with_capacity(MAX_POOLED_BUFFER_CAPACITY + 1);
    recycle_buffer(&ret_tx, oversized);
    assert!(
        ret_rx.try_recv().is_err(),
        "oversized buffer must not be pooled"
    );
}

/// The return channel's own shutdown, in both directions, must be silent:
/// no panic, no block. This is the pooling feature's own new failure
/// surface -- the forward tx/rx channel's shutdown is exercised by
/// `reader_parser_preserves_bytes_across_varied_read_sizes` (reader-initiated
/// EOF) and `parser_thread_terminates_when_reader_disconnects`
/// (reader-side sender dropped).
#[test]
fn return_channel_shutdown_is_silent_in_both_directions() {
    let (ret_tx, ret_rx) = crossbeam::channel::bounded::<Vec<u8>>(RETURN_CHANNEL_CAPACITY);
    drop(ret_rx);
    // Nobody can ever receive this -- must not panic.
    recycle_buffer(&ret_tx, vec![1, 2, 3]);

    let (ret_tx2, ret_rx2) = crossbeam::channel::bounded::<Vec<u8>>(RETURN_CHANNEL_CAPACITY);
    drop(ret_tx2);
    // Must fall back to a fresh allocation, not panic/block.
    let buf = take_pooled_buffer(&ret_rx2);
    assert_eq!(buf.capacity(), BUFSIZE);
}

/// Drives the real reader thread (`read_from_pane_pty`) against a scripted
/// source that hands back data in varied, deliberately awkward chunk sizes
/// (1 byte, a few bytes, sizes both under and over `BUFSIZE`), and checks
/// that the reconstructed printed text is byte-for-byte identical to the
/// input, in order. This is the same code path the buffer pool sits in
/// (`take_pooled_buffer`/`recycle_buffer` in the real read loop), so a
/// pool bug that corrupts or reorders bytes would fail this test.
#[test]
fn reader_parser_preserves_bytes_across_varied_read_sizes() {
    let _guards = lock_test_globals();
    init_schedulers_once();
    onlyterm_config::use_this_configuration(onlyterm_config::Config::default_config());
    // `read_from_pane_pty`'s post-EOF cleanup always calls `Mux::get()`
    // (via `spawn_into_main_thread`, run inline by `init_schedulers_once`'s
    // scheduler), so a real `Mux` singleton must exist for the whole test.
    let mux = Arc::new(crate::Mux::new(None));
    crate::Mux::set_mux(&mux);

    let pane = Arc::new(RecordingPane::new());
    let weak: Weak<dyn Pane> = Arc::downgrade(&(pane.clone() as Arc<dyn Pane>));

    // Deterministic, easily-diffable payload spanning several BUFSIZE (64
    // KiB) boundaries, built from pure printable ASCII so every byte comes
    // back as Print/PrintString. Large enough that the read-size cycle
    // below (dominated by one 64 KiB entry per 8-read cycle) still adds up
    // to well over a hundred individual reads.
    let mut expected = String::new();
    for i in 0..300_000 {
        expected.push_str(&format!("L{:05}#", i));
    }
    let data = expected.clone().into_bytes();
    assert!(
        data.len() > 3 * BUFSIZE,
        "payload must span multiple BUFSIZE-sized reader buffers"
    );

    let reads_dispatched = Arc::new(AtomicUsize::new(0));
    let reader = ScriptedReader {
        data,
        pos: 0,
        sizes: vec![1, 3, 7, 4096, 65536, 17, 900, 2],
        size_idx: 0,
        reads_dispatched: Arc::clone(&reads_dispatched),
    };

    let (done_tx, done_rx) = mpsc::sync_channel::<()>(1);
    let handle = std::thread::spawn(move || {
        read_from_pane_pty(weak, None, Box::new(reader));
        let _ = done_tx.send(());
    });
    done_rx
        .recv_timeout(WATCHDOG)
        .expect("reader did not reach EOF and shut down in time");
    handle.join().expect("reader thread panicked");

    assert!(
        reads_dispatched.load(AtomicOrdering::SeqCst) > 100,
        "payload must have been split across many reads"
    );

    // The reader thread exiting only means it finished reading/sending;
    // the parser thread it spawned may still be draining/flushing its
    // last few buffered messages.
    let actual = wait_for_printed_text(&pane, expected.len(), WATCHDOG);
    assert_eq!(actual, expected, "bytes must arrive intact and in order");
    crate::Mux::shutdown();
}

/// Holds the pane's `perform_actions` gate closed before starting the
/// reader, so the parser stalls on its first flush. Proves the forward
/// channel's backpressure (bounded at `CHANNEL_CAPACITY`) still blocks the
/// READER thread itself -- not just "the parser is behind" -- while the
/// parser can't drain, and that releasing the stall lets everything
/// resume and terminate cleanly (reader hits EOF once the scripted data is
/// exhausted).
#[test]
fn reader_blocks_on_full_channel_when_parser_stalls() {
    let _guards = lock_test_globals();
    init_schedulers_once();
    onlyterm_config::use_this_configuration(onlyterm_config::Config::default_config());
    // See the comment in `reader_parser_preserves_bytes_across_varied_read_sizes`:
    // `read_from_pane_pty`'s EOF cleanup needs a real `Mux` singleton.
    let mux = Arc::new(crate::Mux::new(None));
    crate::Mux::set_mux(&mux);

    let pane = Arc::new(RecordingPane::new());
    let weak: Weak<dyn Pane> = Arc::downgrade(&(pane.clone() as Arc<dyn Pane>));

    // Held for the whole "stalled" phase: every `perform_actions` call
    // made by the parser thread blocks until this is dropped.
    let gate = pane.lock_gate();

    let reads_dispatched = Arc::new(AtomicUsize::new(0));
    const TOTAL_BYTES: usize = 2_000_000;
    let reader = ScriptedReader {
        data: vec![b'x'; TOTAL_BYTES],
        pos: 0,
        sizes: vec![4096],
        size_idx: 0,
        reads_dispatched: Arc::clone(&reads_dispatched),
    };

    let (done_tx, done_rx) = mpsc::sync_channel::<()>(1);
    let handle = std::thread::spawn(move || {
        read_from_pane_pty(weak, None, Box::new(reader));
        let _ = done_tx.send(());
    });

    // Wait until the reader stops advancing (no fixed sleep: a loaded
    // machine can be slow to reach the plateau). An unbounded channel
    // would instead run to EOF, which the bound below catches.
    let deadline = std::time::Instant::now() + WATCHDOG;
    let mut last = reads_dispatched.load(AtomicOrdering::SeqCst);
    let mut unchanged_samples = 0;
    while unchanged_samples < 5 {
        assert!(
            std::time::Instant::now() < deadline,
            "reader never stopped advancing (reads={})",
            last
        );
        std::thread::sleep(Duration::from_millis(50));
        let now = reads_dispatched.load(AtomicOrdering::SeqCst);
        if now == last && now > 0 {
            unchanged_samples += 1;
        } else {
            unchanged_samples = 0;
        }
        last = now;
    }
    assert!(
        last * 4096 < TOTAL_BYTES / 4,
        "reader must block on the full channel while the parser is stalled (reads={})",
        last
    );

    // Release the stall: the parser drains, the reader's blocked send
    // unblocks, and both run to completion (EOF) without any further help.
    drop(gate);
    done_rx
        .recv_timeout(WATCHDOG)
        .expect("reader did not resume and finish after backpressure was released");
    handle.join().expect("reader thread panicked");

    // The reader thread exiting only means it finished reading/sending;
    // the parser thread it spawned may still be draining/flushing its
    // last few buffered messages.
    let actual = wait_for_printed_text(&pane, TOTAL_BYTES, WATCHDOG);
    assert_eq!(
        actual.len(),
        TOTAL_BYTES,
        "all bytes must have made it through once unblocked"
    );
    crate::Mux::shutdown();
}

/// Drives `parse_buffered_data` directly (bypassing the reader thread) so
/// the test can drop the reader's sender (`tx`) itself and confirm the
/// parser thread observes the disconnect and terminates -- the "parser
/// shuts down when the reader is gone" direction. Also checks a buffer
/// sent through `tx` comes back on `ret_rx`, proving the parser is
/// actually recycling in the normal case before the shutdown is tested.
#[test]
fn parser_thread_terminates_when_reader_disconnects() {
    // `send_actions_to_mux` notifies through the global mux and needs a
    // scheduler: without both, test order decides whether this panics.
    let _guards = lock_test_globals();
    init_schedulers_once();
    let dead = Arc::new(AtomicBool::new(false));
    let (tx, rx) = crossbeam::channel::bounded::<Vec<u8>>(CHANNEL_CAPACITY);
    let (ret_tx, ret_rx) = crossbeam::channel::bounded::<Vec<u8>>(RETURN_CHANNEL_CAPACITY);
    let pane = Arc::new(RecordingPane::new());
    let weak: Weak<dyn Pane> = Arc::downgrade(&(pane.clone() as Arc<dyn Pane>));

    let (done_tx, done_rx) = mpsc::sync_channel::<()>(1);
    let handle = {
        let dead = Arc::clone(&dead);
        std::thread::spawn(move || {
            parse_buffered_data(weak, &dead, rx, ret_tx);
            let _ = done_tx.send(());
        })
    };

    tx.send(b"plain text".to_vec())
        .expect("send to live parser");
    let recycled = ret_rx
        .recv_timeout(WATCHDOG)
        .expect("parser did not recycle the buffer it drained");
    assert!(
        recycled.capacity() >= "plain text".len(),
        "recycled buffer must retain its allocation"
    );

    // Reader-side shutdown: drop `tx`. The parser's `rx.recv()` must
    // observe the disconnect, flush pending actions, and return -- not
    // hang waiting for more data that will never come.
    drop(tx);
    done_rx
        .recv_timeout(WATCHDOG)
        .expect("parser thread did not terminate after the reader disconnected");
    handle.join().expect("parser thread panicked");
    assert!(dead.load(AtomicOrdering::Relaxed));

    let text = reconstruct_printed_text(&pane.flattened_actions());
    assert_eq!(text, "plain text");
}
