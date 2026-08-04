use crate::pane::Pane;
use crate::{Mux, MuxNotification};
use config::{configuration, ExitBehavior};
use crossbeam::channel::RecvTimeoutError;
use log::error;
use metrics::histogram;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use termwiz::escape::csi::{DecPrivateMode, DecPrivateModeCode, Device, Keyboard, Mode};
use termwiz::escape::{Action, CSI};

// The reader thread's own pty-read buffer size. This read is always
// outstanding (the reader thread blocks in it continuously), so the whole
// buffer is resident for as long as the pane exists -- measured on Windows
// at ~1 MB of extra resident memory per pane at 1 MiB vs. ~28 KB at 8 KiB.
// The parser already reads from its side of the reader<->parser channel
// with its own, independently-sized `mux_output_parser_buffer_size` (128
// KiB default), so a larger reader-side buffer doesn't help throughput --
// even an unrealistic 50 MiB/s pty flood only needs ~800 reads/sec at 64
// KiB.
const BUFSIZE: usize = 64 * 1024;

// Capacity (in messages, not bytes) of the bounded channel that carries pty
// reads from the reader thread to the parser thread. Each message is at
// most one `BUFSIZE`-sized chunk (or the startup banner). A handful of
// slots is enough to smooth over a burst of reads landing before the
// parser drains them, while still giving the same backpressure the old
// SO_SNDBUF-limited socket write provided: once the channel is full,
// `Sender::send` blocks the reader thread until the parser catches up,
// bounding how far the pty reader can run ahead of the parser.
const CHANNEL_CAPACITY: usize = 4;

/// This function applies parsed actions to the pane and notifies any
/// mux subscribers about the output event
fn send_actions_to_mux(pane: &Weak<dyn Pane>, dead: &Arc<AtomicBool>, actions: Vec<Action>) {
    let start = Instant::now();
    match pane.upgrade() {
        Some(pane) => {
            pane.perform_actions(actions);
            histogram!("send_actions_to_mux.perform_actions.latency").record(start.elapsed());
            Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane.pane_id()));
        }
        None => {
            // Something else removed the pane from
            // the mux, so signal that we should stop
            // trying to process it in read_from_pane_pty.
            dead.store(true, Ordering::Relaxed);
        }
    }
    histogram!("send_actions_to_mux.rate").record(1.);
}

/// Returns true for queries that are safe to answer while a synchronized
/// update (DEC private mode 2026) is holding back the output stream:
/// their responses reflect terminal capabilities rather than screen state,
/// so they don't need to wait for the held actions to be applied.
/// Applications commonly block waiting for these responses, and would
/// otherwise stall until the update closes.
fn is_passthrough_query(action: &Action) -> bool {
    match action {
        Action::CSI(CSI::Device(dev)) => matches!(
            **dev,
            Device::RequestPrimaryDeviceAttributes
                | Device::RequestSecondaryDeviceAttributes
                | Device::RequestTertiaryDeviceAttributes
                | Device::RequestTerminalNameAndVersion
                | Device::StatusReport
        ),
        // The query is answered eagerly, but kitty state mutations stay
        // held: the keyboard stacks live on the screens themselves, so a
        // held screen switch or reset could route an eager mutation to
        // the wrong screen's stack.
        Action::CSI(CSI::Keyboard(Keyboard::QueryKittySupport)) => true,
        // Notably, CPR and DECRQM stay held: their answers (the cursor
        // position, the state of a mode) may be changed by the actions
        // that are being held back.
        _ => false,
    }
}

/// The poll timeout is a c_int of milliseconds on the platforms this
/// runs on, and the deadline is Instant arithmetic; clamp the configured
/// value (to ~24.8 days) so that an absurdly large setting can't wrap
/// into a negative poll timeout or overflow the deadline
pub(crate) fn hold_timeout_from(config: &config::ConfigHandle) -> Duration {
    Duration::from_millis(
        config
            .mux_synchronized_output_timeout_ms
            .min(i32::MAX as u64),
    )
}

/// Mutable parser state threaded through `process_chunk` across calls.
struct ParseState {
    hold: bool,
    hold_deadline: Option<Instant>,
    hold_timeout: Duration,
    actions: Vec<Action>,
    action_size: usize,
}

/// Feeds one chunk of pty bytes through the escape-sequence parser,
/// applying the DEC 2026 synchronized-output hold/flush rules, and
/// forwards any resulting action batches to the mux. Shared by both the
/// main receive loop and the hold-timeout/coalescing branches so the
/// hold/flush logic only lives in one place.
fn process_chunk(
    pane: &Weak<dyn Pane>,
    dead: &Arc<AtomicBool>,
    state: &mut ParseState,
    parser: &mut termwiz::escape::parser::Parser,
    bytes: &[u8],
) {
    parser.parse(bytes, |action| {
        if state.hold && is_passthrough_query(&action) {
            send_actions_to_mux(pane, dead, vec![action]);
            return;
        }
        let mut flush = false;
        match &action {
            Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SynchronizedOutput,
            )))) => {
                // Setting the mode while it is already active is
                // idempotent: the update keeps its original
                // deadline and the frame held so far stays held
                if !state.hold {
                    state.hold = true;
                    state.hold_deadline = Some(Instant::now() + state.hold_timeout);

                    // Flush prior actions
                    flush = true;
                }
            }
            Action::CSI(CSI::Mode(Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                DecPrivateModeCode::SynchronizedOutput,
            )))) => {
                // Synchronized output frame ended:
                // => We flush out all pending actions to the terminal.
                state.hold = false;
                state.hold_deadline = None;
                flush = true;
            }
            Action::CSI(CSI::Device(dev)) if matches!(**dev, Device::SoftReset) => {
                // Soft reset requested
                state.hold = false;
                state.hold_deadline = None;
                flush = true;
            }
            _ => {}
        };
        action.append_to(&mut state.actions);

        if flush && !state.actions.is_empty() {
            send_actions_to_mux(pane, dead, std::mem::take(&mut state.actions));
            state.action_size = 0;
        }
    });
    state.action_size += bytes.len();
}

/// This is the parsing loop for the given pane.
/// It reads all data sent to `rx` (from pane PTY) and handles all terminal events for this pane.
pub(crate) fn parse_buffered_data(
    pane: Weak<dyn Pane>,
    dead: &Arc<AtomicBool>,
    rx: crossbeam::channel::Receiver<Vec<u8>>,
) {
    let mut parser = termwiz::escape::parser::Parser::new();
    let mut delay = Duration::from_millis(configuration().mux_output_parser_coalesce_delay_ms);
    let mut deadline = None;
    let mut state = ParseState {
        hold: false,
        hold_deadline: None,
        hold_timeout: hold_timeout_from(&configuration()),
        actions: vec![],
        action_size: 0,
    };

    loop {
        if state.hold {
            let hold_timeout = state.hold_timeout;
            let target = *state
                .hold_deadline
                .get_or_insert_with(|| Instant::now() + hold_timeout);
            let expired = match target.checked_duration_since(Instant::now()) {
                Some(remaining) => match rx.recv_timeout(remaining) {
                    Err(RecvTimeoutError::Timeout) => true,
                    Ok(bytes) => {
                        // Data arrived before the deadline: process it and
                        // go back around the loop (re-checking the hold
                        // deadline/state) rather than falling through to
                        // the unconditional `rx.recv()` below, which would
                        // otherwise consume a second, unrelated message.
                        process_chunk(&pane, dead, &mut state, &mut parser, &bytes);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        dead.store(true, Ordering::Relaxed);
                        if !state.actions.is_empty() {
                            send_actions_to_mux(&pane, dead, std::mem::take(&mut state.actions));
                        }
                        return;
                    }
                },
                None => true,
            };
            if expired {
                // The synchronized update has been open for longer than
                // the configured timeout; release the hold so that a
                // stalled application cannot freeze the pane indefinitely
                state.hold = false;
                state.hold_deadline = None;
                if !state.actions.is_empty() {
                    send_actions_to_mux(&pane, dead, std::mem::take(&mut state.actions));
                    state.action_size = 0;
                }
                continue;
            }
        }

        match rx.recv() {
            Err(_) => {
                dead.store(true, Ordering::Relaxed);
                break;
            }
            Ok(bytes) => {
                process_chunk(&pane, dead, &mut state, &mut parser, &bytes);

                // If we haven't accumulated too much data, pause for a
                // short while to increase the chances that we coalesce a
                // full "frame" from an unoptimized TUI program.
                //
                // This must be a loop that ends in the flush below, not a
                // `continue` back to the top: the blocking `rx.recv()` up
                // there would strand whatever is already parsed until the
                // pty produces MORE bytes. That's not hypothetical -- it
                // held the answers to ConPTY's startup DSR/DA1 queries
                // hostage while ConPTY sat silent waiting for exactly
                // those answers, a mutual wait broken only by ConPTY's
                // 3000ms WaitUntilDA1 timeout, i.e. a 3 second stall on
                // every single pane spawn (task #328). The upstream code
                // this was ported from could `continue` safely because it
                // used poll(), which only *checks* readiness -- the data
                // stayed queued for the outer read. `recv_timeout`
                // *consumes* the message, so the outer recv blocks on an
                // empty channel instead.
                while !state.actions.is_empty()
                    && !state.hold
                    && state.action_size < configuration().mux_output_parser_buffer_size
                {
                    let target = *deadline.get_or_insert_with(|| Instant::now() + delay);
                    let remaining = match target.checked_duration_since(Instant::now()) {
                        Some(remaining) => remaining,
                        // Deadline already passed: flush what we have.
                        None => break,
                    };
                    match rx.recv_timeout(remaining) {
                        Ok(more) => {
                            process_chunk(&pane, dead, &mut state, &mut parser, &more);
                        }
                        // Timeout or disconnect: flush what we have. A
                        // disconnect will be observed by the next
                        // `rx.recv()` above.
                        Err(_) => break,
                    }
                }

                if !state.actions.is_empty() && !state.hold {
                    send_actions_to_mux(&pane, dead, std::mem::take(&mut state.actions));
                    state.action_size = 0;
                }
                deadline = None;

                let config = configuration();
                delay = Duration::from_millis(config.mux_output_parser_coalesce_delay_ms);
                state.hold_timeout = hold_timeout_from(&config);
            }
        }
    }

    // Don't forget to send anything that we might have buffered
    // to be displayed before we return from here; this is important
    // for very short lived commands so that we don't forget to
    // display what they displayed.
    if !state.actions.is_empty() {
        send_actions_to_mux(&pane, dead, std::mem::take(&mut state.actions));
    }
}

/// This function is run in a separate thread; its purpose is to perform
/// blocking reads from the pty (non-blocking reads are not portable to
/// all platforms and pty/tty types), parse the escape sequences and
/// relay the actions to the mux thread to apply them to the pane.
pub(crate) fn read_from_pane_pty(
    pane: Weak<dyn Pane>,
    banner: Option<String>,
    mut reader: Box<dyn std::io::Read>,
) {
    let mut buf = vec![0; BUFSIZE];

    // This is used to signal that an error occurred either in this thread,
    // or in the main mux thread.  If `true`, this thread will terminate.
    let dead = Arc::new(AtomicBool::new(false));

    let (pane_id, exit_behavior) = match pane.upgrade() {
        Some(pane) => (pane.pane_id(), pane.exit_behavior()),
        None => return,
    };

    let (tx, rx) = crossbeam::channel::bounded::<Vec<u8>>(CHANNEL_CAPACITY);

    // Spawn parser thread for this pane
    std::thread::spawn({
        let dead = Arc::clone(&dead);
        move || parse_buffered_data(pane, &dead, rx)
    });

    if let Some(banner) = banner {
        tx.send(banner.into_bytes()).ok();
    }

    // Loop until the pane or the main mux thread is dead.
    // Read data from the pane pty and send it to the parser thread via tx/rx.
    while !dead.load(Ordering::Relaxed) {
        match reader.read(&mut buf) {
            Ok(0) => {
                log::trace!("read_pty EOF: pane_id {}", pane_id);
                break;
            }
            Err(err) => {
                error!("read_pty failed: pane {} {:?}", pane_id, err);
                break;
            }
            Ok(size) => {
                histogram!("read_from_pane_pty.bytes.rate").record(size as f64);
                // Send received data to this pane's parser thread. This
                // blocks if the channel is full, which is the intended
                // backpressure: it bounds how far the pty reader can run
                // ahead of the parser.
                if tx.send(buf[..size].to_vec()).is_err() {
                    error!(
                        "read_pty failed to send to parser for pane {}: parser thread is gone",
                        pane_id
                    );
                    break;
                }
            }
        }
    }

    match exit_behavior.unwrap_or_else(|| configuration().exit_behavior) {
        ExitBehavior::Hold | ExitBehavior::CloseOnCleanExit => {
            // We don't know if we can unilaterally close
            // this pane right now, so don't!
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::get();
                log::trace!("checking for dead windows after EOF on pane {}", pane_id);
                mux.prune_dead_windows();
            })
            .detach();
        }
        ExitBehavior::Close => {
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::get();
                mux.remove_pane(pane_id);
            })
            .detach();
        }
    }

    dead.store(true, Ordering::Relaxed);
}
