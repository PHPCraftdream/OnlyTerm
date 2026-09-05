mod pane_impl;
mod process_info;

use crate::domain::DomainId;
use crate::pane::{
    CachePolicy, CloseReason, ForEachPaneLogicalLine, LogicalLine, Pane, PaneId, Pattern,
    SearchResult, WithPaneLines,
};
use crate::renderable::*;
use crate::tmux::{TmuxDomain, TmuxDomainState};
use crate::{Domain, Mux, MuxNotification};
use anyhow::Error;
use async_trait::async_trait;
use config::keyassignment::ScrollbackEraseMode;
use config::{configuration, ExitBehavior, ExitBehaviorMessaging};
use fancy_regex::Regex;
use onlyterm_dynamic::Value;
use onlyterm_term::color::ColorPalette;
use onlyterm_term::{
    Alert, AlertHandler, Clipboard, DownloadHandler, KeyCode, KeyModifiers, MouseEvent, Progress,
    SemanticZone, StableRowIndex, Terminal, TerminalConfiguration, TerminalSize,
};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use procinfo::LocalProcessInfo;
use rangeset::RangeSet;
use smol::channel::{bounded, Receiver, TryRecvError};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryInto;
use std::io::{Result as IoResult, Write};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::escape::csi::{Sgr, CSI};
use termwiz::escape::{Action, DeviceControlMode};
use termwiz::hyperlink::Rule;
use termwiz::input::KeyboardEncoding;
use termwiz::surface::{Line, SequenceNo};
use url::Url;

const PROC_INFO_CACHE_TTL: Duration = Duration::from_millis(300);

/// Grace period between `kill()` sending the soft-interrupt byte (`\x03`,
/// the same byte the user's physical Ctrl+C writes -- see the doc comment
/// on `kill()`) and actually dropping the pane's pty.
///
/// Dropping the pty tears down the underlying pty/ConPTY immediately, which
/// on Windows would very likely race the soft signal: conhost/OpenConsole
/// needs to actually read and process the `\x03` byte and raise
/// `CTRL_C_EVENT` for the whole attached process tree before the pty goes
/// away, and that doesn't happen synchronously with the write. Deferring
/// the drop by this same duration means the pty teardown lands at roughly
/// the same moment the hard-kill path (`pty::win::mod::kill_gracefully_then_forcefully`)
/// gives up waiting and escalates to `TerminateProcess`, rather than
/// racing ahead of the soft signal it's supposed to precede.
///
/// Intentionally kept equal to `pty::win::mod::GRACEFUL_KILL_TIMEOUT_MS`
/// (5000ms): that's the existing constant governing how long the
/// hard-kill path waits for the child to exit on its own before
/// force-terminating it, and there's no reason for this mux-level grace
/// period to differ from it. It's duplicated here (rather than imported)
/// because it lives in a `#[cfg(windows)]`-only, private module of the
/// `pty` crate, while this deferred-drop mechanism is deliberately
/// cross-platform (see `kill()`'s doc comment).
const PTY_DROP_GRACE_MS: u64 = 5000;

#[derive(Debug)]
enum ProcessState {
    Running {
        child_waiter: Receiver<IoResult<ExitStatus>>,
        pid: Option<u32>,
        signaller: Box<dyn ChildKiller + Sync>,
        // Whether we've explicitly killed the child
        killed: bool,
    },
    DeadPendingClose {
        killed: bool,
    },
    Dead,
}

struct CachedProcInfo {
    root: LocalProcessInfo,
    updated: Instant,
    foreground: LocalProcessInfo,
    /// Task #247: set while a background thread (spawned by
    /// `divine_process_list`) is busy recomputing this cache entry, so a
    /// second caller that also observes an expired-but-present cache
    /// doesn't spawn a duplicate concurrent refresh.
    updating: bool,
}

impl CachedProcInfo {
    fn expired(&self) -> bool {
        self.updated.elapsed() > PROC_INFO_CACHE_TTL
    }

    fn can_update(&self) -> bool {
        !self.updating
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LocalPaneConnectionState {
    Connecting,
    Connected,
}

/// Records, via the `metrics` crate, how long the calling thread had to
/// wait to acquire `LocalPane::terminal` before running `body`.
///
/// This is deliberately measuring *wait* time (the interval between
/// deciding to lock and actually getting the lock), not *hold* time (how
/// long the critical section itself takes): the two are conflated easily,
/// but only wait time tells us whether a given caller (input handling,
/// the pty output parser, or the renderer) is being blocked by contention
/// from the others. `histogram_name` should be one of the
/// `localpane.terminal_lock.wait.*` metrics so profiling data collected
/// on a real machine can distinguish which side of the lock is the
/// bottleneck.
fn lock_terminal_timed<R>(
    terminal: &Mutex<Terminal>,
    histogram_name: &'static str,
    body: impl FnOnce(&mut Terminal) -> R,
) -> R {
    let wait_start = Instant::now();
    let mut term = terminal.lock();
    metrics::histogram!(histogram_name).record(wait_start.elapsed());
    body(&mut term)
}

/// Applies hyperlink rules to the lines of a stable range; the
/// single-lock analogue of the `ApplyHyperLinks` visitor in the default
/// `Pane::apply_hyperlinks` implementation (which would re-lock the
/// terminal if called from inside a `terminal.lock()` closure).
struct ApplyHyperlinksInLock<'a> {
    rules: &'a [Rule],
}

impl ForEachPaneLogicalLine for ApplyHyperlinksInLock<'_> {
    fn with_logical_line_mut(&mut self, _: Range<StableRowIndex>, lines: &mut [&mut Line]) -> bool {
        Line::apply_hyperlink_rules(self.rules, lines);

        true
    }
}

/// Task #246: how long a GUI-thread-reachable accessor (`get_title()`,
/// `get_progress()`, `copy_user_vars()`, the cache-lookup half of
/// `get_current_working_dir()`) is willing to wait for `terminal.lock()`
/// before giving up and falling back to the last known-good cached value.
///
/// `get_tab_information()` (`onlyterm-gui/src/termwindow/mod.rs`) calls
/// these for the *active* pane of *every* tab in the window, and it's
/// invoked from `update_title_impl` on essentially every key/mouse event
/// on the GUI thread. That means a single background tab whose terminal
/// mutex is wedged (or just held a little too long by the pty parser
/// thread applying a large output batch, see `perform_actions_chunked`
/// above) can stall the *whole* window's message loop once per polled
/// tab, not just the one tab that's slow.
///
/// 8ms was chosen to sit clearly on the "imperceptible" side of input
/// latency (commonly-cited UI responsiveness budgets put "instantaneous"
/// around 100ms and "avoid perceptible lag" around 16ms/one frame at
/// 60Hz) while still being generous compared to how long the parser
/// thread normally holds the lock for a single chunk of actions -- chunks
/// are capped (see `perform_actions_chunked`) specifically so that a
/// single critical section is short, on the order of microseconds to a
/// couple of milliseconds, not tens of milliseconds. A few polled
/// background tabs each spending up to 8ms here in the worst case still
/// keeps total added latency for one GUI event to a low single-digit
/// number of milliseconds, while genuinely wedged/stuck panes (the
/// motivating case) are bounded rather than blocking forever.
const TERMINAL_ACCESSOR_LOCK_TIMEOUT: Duration = Duration::from_millis(8);

/// Task #273: how long a `set_render_budget_exceeded(true)` observation
/// (see `LocalPane::render_budget_exceeded`) is allowed to keep
/// contributing a `true` to `is_unresponsive()` after it was last
/// refreshed, before it's treated as stale and ignored.
///
/// The producer of this signal (the render loop in
/// `crates/onlyterm-gui/src/termwindow/render/pane.rs`) only runs for
/// panes that are actually painted *this frame*, which for the active,
/// focused window happens at up to the display's refresh rate --
/// commonly 60Hz, i.e. a fresh observation (`true` or `false`) roughly
/// every 16ms for as long as the pane keeps being painted. A window
/// needs to be comfortably wider than that cadence (so ordinary frame-
/// to-frame jitter, a briefly-minimized/occluded window, or a couple of
/// dropped frames can't make a *currently* budget-exceeded pane flicker
/// back to "responsive" between refreshes) while still being short in
/// absolute terms, so that a pane whose tab stops being painted
/// altogether (e.g. the user switches to a different tab) "goes quiet"
/// within a bounded, human-imperceptible-as-a-hang delay rather than
/// staying flagged forever. One second comfortably clears both bars: it
/// is tens of frame-intervals even at a sluggish ~10fps, yet is short
/// enough that no caller of the public `Pane::is_unresponsive()` method
/// (it's part of the `Pane` trait, so anything holding a pane reference can
/// call it) could mistake a transient render-budget hiccup for a permanent,
/// sticky flag.
const RENDER_BUDGET_EXCEEDED_EXPIRY: Duration = Duration::from_secs(1);

/// Attempts to acquire `terminal.lock()` within `TERMINAL_ACCESSOR_LOCK_TIMEOUT`
/// and run `body` against it, returning `None` on timeout instead of
/// blocking indefinitely. See `TERMINAL_ACCESSOR_LOCK_TIMEOUT` for why a
/// bounded wait matters here. `metric_name` is incremented via
/// `metrics::counter!` whenever the timeout is hit, so a wedged pane
/// shows up in metrics rather than only as an unexplained (bounded, but
/// still real) latency blip.
///
/// Task #248: `unresponsive` (each `LocalPane`'s `unresponsive` field) is
/// set to `true` on timeout and cleared back to `false` on success, so
/// `LocalPane::is_unresponsive()` reflects the outcome of the most recent
/// bounded lock attempt made by *any* of the four callers below, not just
/// metrics/logs.
fn try_lock_terminal_for<R>(
    terminal: &Mutex<Terminal>,
    unresponsive: &AtomicBool,
    metric_name: &'static str,
    body: impl FnOnce(&mut Terminal) -> R,
) -> Option<R> {
    match terminal.try_lock_for(TERMINAL_ACCESSOR_LOCK_TIMEOUT) {
        Some(mut term) => {
            unresponsive.store(false, Ordering::Release);
            Some(body(&mut term))
        }
        None => {
            unresponsive.store(true, Ordering::Release);
            metrics::counter!(metric_name).increment(1);
            log::debug!(
                "{metric_name}: gave up waiting {:?} for terminal.lock(); \
                 falling back to last known-good cached value",
                TERMINAL_ACCESSOR_LOCK_TIMEOUT
            );
            None
        }
    }
}

pub struct LocalPane {
    pane_id: PaneId,
    terminal: Mutex<Terminal>,
    process: Mutex<ProcessState>,
    // `None` once `kill()` has taken the pty out in order to defer its
    // `Drop` (see `kill()` and `PTY_DROP_GRACE_MS` for why): a killed pane's
    // pty is intentionally kept alive for a short grace period on a
    // detached background thread rather than being torn down the instant
    // the last `Arc<LocalPane>` goes away, so every call site below has to
    // treat "pty already gone" as a normal, silent case rather than a bug.
    pty: Mutex<Option<Box<dyn MasterPty>>>,
    writer: Mutex<Box<dyn Write + Send>>,
    /// Serializes `resize()` against `perform_actions_chunked()`'s batch
    /// loop (held for the whole batch there, not per-chunk -- see its
    /// call site). Without this, a resize could land between two chunks
    /// of one logical ConPTY repaint that our own chunking split apart,
    /// applying the chunk's tail under a different geometry than the one
    /// it was drawn for. Always acquired before `terminal`/`pty` below,
    /// in both places that take it, to keep lock order consistent.
    resize_guard: Mutex<()>,
    domain_id: DomainId,
    tmux_domain: Mutex<Option<Arc<TmuxDomainState>>>,
    /// Task #247: `Arc`-wrapped so that `divine_process_list`'s
    /// background refresh thread can clone just this handle and update
    /// the cache in place without needing to keep the whole `LocalPane`
    /// alive or borrow `self` across the thread spawn.
    proc_list: Arc<Mutex<Option<CachedProcInfo>>>,
    /// Task #471: guards the *cold-cache* case in `divine_process_list`
    /// (no `CachedProcInfo` entry at all yet). Unlike the warm-refresh
    /// case, there's no existing `CachedProcInfo` to stash an `updating`
    /// flag on while the first fetch is in flight, so this lives
    /// alongside `proc_list` instead. Set right before a background cold
    /// fetch is kicked off, cleared once that fetch stores its result (or
    /// fails) in `proc_list`; a second `AllowStale` call that observes a
    /// cold cache while this is already `true` just returns `None` again
    /// rather than queuing a duplicate concurrent fetch.
    proc_list_cold_fetch_in_flight: Arc<AtomicBool>,
    command_description: String,
    /// Lock-free mirror of `terminal.has_unseen_output()`, kept in sync
    /// by the terminal via the shared `Arc<AtomicBool>` whenever focus
    /// or the sequence number changes. `has_unseen_output()` reads this
    /// directly instead of taking `terminal.lock()`: the GUI
    /// title-refresh path polls every pane on essentially every event,
    /// and a single background pane whose terminal mutex is wedged must
    /// not be able to block the GUI thread (and thus the whole
    /// process's message loop).
    unseen_output: Arc<AtomicBool>,
    /// Task #248: set to `true` whenever a bounded `terminal.lock()`
    /// attempt (`try_lock_terminal_for`, task #246) times out for *any*
    /// of the four GUI-thread-reachable accessors below, and cleared
    /// back to `false` the moment any subsequent bounded attempt
    /// succeeds. `is_unresponsive()` reads this (OR'd with
    /// `render_budget_exceeded` below, see task #269) directly, with no
    /// lock of its own, mirroring `unseen_output` above and for the same
    /// reason: this needs to be safely pollable from the GUI thread for
    /// *every* pane, including ones whose terminal mutex is currently
    /// wedged.
    ///
    /// A plain bool (rather than e.g. an `AtomicU64` storing the
    /// `Instant` of the last timeout, which would let a consumer show
    /// "stale for how long") was chosen because nothing in this
    /// codebase yet consumes a richer signal than "is this pane
    /// currently suspect right now" -- the existing `has_unseen_output`
    /// field this mirrors is the same shape, and `Instant` isn't
    /// `Copy`-friendly for lock-free atomic storage without an extra
    /// indirection (it would need to be encoded as a duration-since-some-
    /// epoch to fit in an `AtomicU64`, adding complexity for a signal
    /// nothing yet reads). If a future consumer needs "how long has this
    /// been stuck" this field can be upgraded then.
    ///
    /// Task #269: this is written ONLY by `try_lock_terminal_for` (the
    /// real hang-detection signal). It used to also be written directly
    /// by the GUI's per-frame render-budget path (task #251), which
    /// clobbered this signal back to `false` on almost every frame for
    /// whichever pane the user was actively looking at, since the
    /// render-budget write ran ~60 times/second and was `false` far more
    /// often than not. That producer now has its own cell,
    /// `render_budget_exceeded` below, and the two are combined only at
    /// the `is_unresponsive()` read site.
    unresponsive: Arc<AtomicBool>,
    /// Task #269: companion to `unresponsive` above, written ONLY by the
    /// GUI's per-frame content-build budget (`set_render_budget_exceeded`,
    /// task #251) in `crates/onlyterm-gui/src/termwindow/render/pane.rs`.
    /// Kept as a separate cell rather than sharing `unresponsive` so that
    /// this producer -- which legitimately writes both `true` and `false`
    /// every single frame for every painted pane -- can never race with
    /// and overwrite a still-active lock-timeout `true` written
    /// concurrently by `try_lock_terminal_for` for the same pane.
    /// `is_unresponsive()` reports the OR of both cells.
    ///
    /// Task #273: this stores the `Instant` of the most recent
    /// budget-exceeded observation (`None` once a frame completes within
    /// budget) rather than a plain sticky `bool`. A plain `bool` only gets
    /// cleared back to `false` by a subsequent `set_render_budget_exceeded
    /// (false)` call, and that call only ever happens from the render
    /// loop in `crates/onlyterm-gui/src/termwindow/render/pane.rs`, which
    /// only runs for panes that are actually painted this frame --
    /// panes belonging to a tab that isn't the active tab (e.g. after the
    /// user switches away) simply stop being painted at all, so a `bool`
    /// would latch `true` forever the moment its tab is backgrounded
    /// right after a slow frame. Storing "when was this last observed"
    /// instead lets the read side (`is_unresponsive()`) treat the signal
    /// as expiring on its own: see `RENDER_BUDGET_EXCEEDED_EXPIRY` for the
    /// window and rationale. The currently-active, currently-painted case
    /// is unaffected, since that path keeps refreshing this `Instant`
    /// every frame (well within the expiry window) for as long as the
    /// condition is genuinely ongoing.
    render_budget_exceeded: Arc<Mutex<Option<Instant>>>,
    /// Task #246: last known-good values for the other GUI-thread-polled
    /// accessors (`get_title()`, `get_progress()`, `copy_user_vars()`, and
    /// the terminal-cache half of `get_current_working_dir()`). Each is
    /// updated whenever a bounded `terminal.lock()` attempt (see
    /// `try_lock_terminal_for`) actually succeeds, and served back as a
    /// stale-but-safe fallback whenever the lock can't be acquired within
    /// `TERMINAL_ACCESSOR_LOCK_TIMEOUT` -- e.g. because a *different*
    /// background tab's terminal is wedged. This is a separate, small
    /// mutex rather than `terminal`'s own lock, so reading/writing the
    /// cache never itself contends with the pty parser thread's much
    /// more frequent, longer-held locking of `terminal`.
    last_known_good: Mutex<CachedTerminalInfo>,
}

/// See `LocalPane::last_known_good`.
#[derive(Clone, Default)]
struct CachedTerminalInfo {
    title: String,
    progress: Progress,
    user_vars: HashMap<String, String>,
    cwd: Option<Url>,
}

struct LocalPaneDCSHandler {
    pane_id: PaneId,
    tmux_domain: Option<Arc<TmuxDomainState>>,
}

pub(crate) fn emit_output_for_pane(pane_id: PaneId, message: &str) {
    let mut parser = termwiz::escape::parser::Parser::new();
    let mut actions = vec![Action::CSI(CSI::Sgr(Sgr::Reset))];
    parser.parse(message.as_bytes(), |action| actions.push(action));

    promise::spawn::spawn_into_main_thread(async move {
        let mux = Mux::get();
        if let Some(pane) = mux.get_pane(pane_id) {
            pane.perform_actions(actions);
            mux.notify(MuxNotification::PaneOutput(pane_id));
        }
    })
    .detach();
}

impl onlyterm_term::DeviceControlHandler for LocalPaneDCSHandler {
    fn handle_device_control(&mut self, control: termwiz::escape::DeviceControlMode) {
        match control {
            DeviceControlMode::Enter(mode) => {
                if !mode.ignored_extra_intermediates
                    && mode.params.len() == 1
                    && mode.params[0] == 1000
                    && mode.intermediates.is_empty()
                {
                    log::info!("tmux -CC mode requested");

                    // Create a new domain to host these tmux tabs
                    let domain = TmuxDomain::new(self.pane_id);
                    let tmux_domain = Arc::clone(&domain.inner);

                    let domain: Arc<dyn Domain> = Arc::new(domain);
                    let mux = Mux::get();
                    mux.add_domain(&domain);

                    if let Some(pane) = mux.get_pane(self.pane_id) {
                        let pane = pane.downcast_ref::<LocalPane>().unwrap();
                        pane.tmux_domain.lock().replace(Arc::clone(&tmux_domain));

                        emit_output_for_pane(
                            self.pane_id,
                            "\r\n[This pane is running tmux control mode. Press q to detach]",
                        );
                    }

                    self.tmux_domain.replace(tmux_domain);

                // TODO: do we need to proactively list available tabs here?
                // if so we should arrange to call domain.attach() and make
                // it do the right thing.
                } else if configuration().log_unknown_escape_sequences {
                    log::warn!("unknown DeviceControlMode::Enter {:?}", mode,);
                }
            }
            DeviceControlMode::Exit => {
                if let Some(tmux) = self.tmux_domain.take() {
                    let mux = Mux::get();
                    if let Some(pane) = mux.get_pane(self.pane_id) {
                        let pane = pane.downcast_ref::<LocalPane>().unwrap();
                        pane.tmux_domain.lock().take();
                    }
                    mux.domain_was_detached(tmux.domain_id);
                }
            }
            DeviceControlMode::Data(c) => {
                if configuration().log_unknown_escape_sequences {
                    log::warn!(
                        "unhandled DeviceControlMode::Data {:x} {}",
                        c,
                        (c as char).escape_debug()
                    );
                }
            }
            DeviceControlMode::TmuxEvents(events) => {
                if let Some(tmux) = self.tmux_domain.as_ref() {
                    tmux.advance(&events);
                } else {
                    log::warn!("unhandled DeviceControlMode::TmuxEvents {:?}", events);
                }
            }
            _ => {
                if configuration().log_unknown_escape_sequences {
                    log::warn!("unhandled: {:?}", control);
                }
            }
        }
    }
}

struct LocalPaneNotifHandler {
    pane_id: PaneId,
}

impl AlertHandler for LocalPaneNotifHandler {
    fn alert(&mut self, alert: Alert) {
        let pane_id = self.pane_id;
        promise::spawn::spawn_into_main_thread(async move {
            let mux = Mux::get();
            match &alert {
                Alert::WindowTitleChanged(title) => {
                    if let Some((_domain, window_id, _tab_id)) = mux.resolve_pane_id(pane_id) {
                        if let Some(mut window) = mux.get_window_mut(window_id) {
                            window.set_title(title);
                        }
                    }
                }
                Alert::TabTitleChanged(title) => {
                    if let Some((_domain, _window_id, tab_id)) = mux.resolve_pane_id(pane_id) {
                        if let Some(tab) = mux.get_tab(tab_id) {
                            tab.set_title(title.as_deref().unwrap_or(""));
                        }
                    }
                }
                _ => {}
            }

            mux.notify(MuxNotification::Alert { pane_id, alert });
        })
        .detach();
    }
}

/// This is a little gross; on some systems, our pipe reader will continue
/// to be blocked in read even after the child process has died.
/// We need to wake up and notice that the child terminated in order
/// for our state to wind down.
/// This block schedules a background thread to wait for the child
/// to terminate, and then nudge the muxer to check for dead processes.
/// Without this, typing `exit` in `cmd.exe` would keep the pane around
/// until something else triggered the mux to prune dead processes.
fn split_child(
    mut process: Box<dyn Child>,
) -> (
    Receiver<IoResult<ExitStatus>>,
    Box<dyn ChildKiller + Sync>,
    Option<u32>,
) {
    let pid = process.process_id();
    let signaller = process.clone_killer();

    let (tx, rx) = bounded(1);

    std::thread::spawn(move || {
        let status = process.wait();
        tx.try_send(status).ok();
        promise::spawn::spawn_into_main_thread(async move {
            let mux = Mux::get();
            mux.prune_dead_windows();
        })
        .detach();
    });

    (rx, signaller, pid)
}

impl LocalPane {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pane_id: PaneId,
        mut terminal: Terminal,
        process: Box<dyn Child + Send>,
        pty: Box<dyn MasterPty>,
        writer: Box<dyn Write + Send>,
        domain_id: DomainId,
        command_description: String,
        starting_cwd: Option<Url>,
    ) -> Self {
        let (process, signaller, pid) = split_child(process);

        terminal.set_device_control_handler(Box::new(LocalPaneDCSHandler {
            pane_id,
            tmux_domain: None,
        }));
        terminal.set_notification_handler(Box::new(LocalPaneNotifHandler { pane_id }));

        // Clone the lock-free unseen-output handle before moving
        // `terminal` into its mutex, so `has_unseen_output()` can read
        // it without ever taking `terminal.lock()`.
        let unseen_output = terminal.unseen_output_handle();

        Self {
            pane_id,
            terminal: Mutex::new(terminal),
            process: Mutex::new(ProcessState::Running {
                child_waiter: process,
                pid,
                signaller,
                killed: false,
            }),
            pty: Mutex::new(Some(pty)),
            writer: Mutex::new(writer),
            resize_guard: Mutex::new(()),
            domain_id,
            tmux_domain: Mutex::new(None),
            proc_list: Arc::new(Mutex::new(None)),
            proc_list_cold_fetch_in_flight: Arc::new(AtomicBool::new(false)),
            command_description,
            unseen_output,
            unresponsive: Arc::new(AtomicBool::new(false)),
            render_budget_exceeded: Arc::new(Mutex::new(None)),
            last_known_good: Mutex::new(CachedTerminalInfo {
                cwd: starting_cwd,
                ..Default::default()
            }),
        }
    }

    /// Applies a batch of already-parsed actions to `self.terminal` in
    /// chunks of at most `chunk_size` actions, releasing and
    /// re-acquiring `terminal.lock()` between chunks.
    ///
    /// Task #147: a full `mux_output_parser_buffer_size` (128KiB
    /// default) batch of actions can be tens of thousands of `Action`s,
    /// and measurements (see `crates/term/src/test/perf_probe.rs`) show
    /// that applying all of them under one lock acquisition holds
    /// `terminal.lock()` for 40-60ms, which starves both keyboard/mouse
    /// input (`key_down`/`mouse_event`) and rendering (`with_lines_mut`)
    /// -- both block on the same mutex. Splitting the batch between
    /// whole `Action`s (never inside one) and taking the lock separately
    /// per chunk lets those callers interleave between chunks while
    /// leaving the final terminal state identical to applying the whole
    /// batch under a single lock acquisition, since each `Action` is
    /// self-contained and `Terminal::perform_actions` makes no
    /// assumption that spans a call boundary (each call only bumps the
    /// sequence number and re-triggers the "unseen output" check, both
    /// of which are correct to do once per chunk) -- PROVIDED nothing
    /// changes the terminal's geometry between chunks, which is exactly
    /// what `resize_guard` (held below, for the whole batch) rules out:
    /// without it, a `resize()` landing between two chunks of one
    /// logical ConPTY repaint would apply the chunk's tail under a
    /// different geometry than the one it was drawn for.
    fn perform_actions_chunked(&self, actions: Vec<Action>, chunk_size: usize) {
        if actions.len() <= chunk_size.max(1) {
            // Common case (small batches, e.g. interactive typing): no
            // benefit to chunking, so avoid the `Vec` chunking overhead
            // and just take the lock once, as before. A single lock
            // acquisition can't be sliced by a concurrent resize, so
            // `resize_guard` isn't needed here.
            lock_terminal_timed(
                &self.terminal,
                "localpane.terminal_lock.wait.perform_actions",
                |term| term.perform_actions(actions),
            );
            return;
        }

        let _resize_guard = self.resize_guard.lock();
        for chunk in actions.chunks(chunk_size.max(1)) {
            lock_terminal_timed(
                &self.terminal,
                "localpane.terminal_lock.wait.perform_actions",
                |term| term.perform_actions(chunk.to_vec()),
            );
        }
    }

    /// Test-only escape hatch that measures `terminal.lock()` wait time
    /// with the exact same semantics as the `localpane.terminal_lock.wait.*`
    /// metrics (time to acquire, not time held), without going through
    /// the `metrics` crate's global recorder. This exists for
    /// `test::terminal_lock_contention`, which needs real wait-time
    /// numbers per call site and has no test-side metrics exporter
    /// wired up in this crate.
    ///
    /// Also returns hold time (how long the critical section itself
    /// took) as the second element, purely so the load test can
    /// distinguish "this call waited a long time" from "this call is
    /// the one that made everyone else wait a long time" -- production
    /// only needs wait time, but the load test's interpretation of its
    /// results depends on telling these apart.
    #[cfg(test)]
    pub(crate) fn with_lines_mut_timed(
        &self,
        lines: Range<StableRowIndex>,
        with_lines: &mut dyn WithPaneLines,
    ) -> (Duration, Duration) {
        let wait_start = Instant::now();
        let mut term = self.terminal.lock();
        let waited = wait_start.elapsed();
        let hold_start = Instant::now();
        terminal_with_lines_mut(&mut term, lines, with_lines);
        (waited, hold_start.elapsed())
    }

    #[cfg(test)]
    pub(crate) fn perform_actions_timed(
        &self,
        actions: Vec<termwiz::escape::Action>,
    ) -> (Duration, Duration) {
        let wait_start = Instant::now();
        let mut term = self.terminal.lock();
        let waited = wait_start.elapsed();
        let hold_start = Instant::now();
        term.perform_actions(actions);
        (waited, hold_start.elapsed())
    }

    /// Like `perform_actions_timed`, but goes through the same chunked
    /// path as production's `perform_actions` (see
    /// `perform_actions_chunked`), returning one (wait, hold) sample per
    /// chunk so `test::terminal_lock_contention` can confirm that
    /// per-chunk hold time actually stays bounded regardless of total
    /// batch size.
    #[cfg(test)]
    pub(crate) fn perform_actions_chunked_timed(
        &self,
        actions: Vec<termwiz::escape::Action>,
        chunk_size: usize,
    ) -> Vec<(Duration, Duration)> {
        let mut samples = Vec::new();
        let _resize_guard = self.resize_guard.lock();
        for chunk in actions.chunks(chunk_size.max(1)) {
            let wait_start = Instant::now();
            let mut term = self.terminal.lock();
            let waited = wait_start.elapsed();
            let hold_start = Instant::now();
            term.perform_actions(chunk.to_vec());
            samples.push((waited, hold_start.elapsed()));
        }
        samples
    }

    #[cfg(test)]
    pub(crate) fn key_down_timed(
        &self,
        key: KeyCode,
        mods: KeyModifiers,
    ) -> (Duration, Result<(), Error>) {
        let wait_start = Instant::now();
        let mut term = self.terminal.lock();
        let waited = wait_start.elapsed();
        (waited, term.key_down(key, mods))
    }

    #[cfg(test)]
    pub(crate) fn mouse_event_timed(&self, event: MouseEvent) -> (Duration, Result<(), Error>) {
        let wait_start = Instant::now();
        let mut term = self.terminal.lock();
        let waited = wait_start.elapsed();
        (waited, term.mouse_event(event))
    }

    /// Like the other `_timed` helpers, but for `resize()`: measures only
    /// the wait to acquire `resize_guard` (the coordination point shared
    /// with `perform_actions_chunked`), not the whole call. Used by
    /// `test::terminal_lock_contention` to prove the guard is genuinely
    /// contended by a concurrent chunked batch, rather than by asserting
    /// on wall-clock overlap of the two outer calls (which is a weaker
    /// and, worse, misleading signal: a caller that's queued waiting for
    /// the guard has an outer call span that legitimately overlaps the
    /// holder's, even though the guard is working correctly).
    #[cfg(test)]
    pub(crate) fn resize_timed(&self, size: TerminalSize) -> anyhow::Result<(Duration, Duration)> {
        let wait_start = Instant::now();
        let _resize_guard = self.resize_guard.lock();
        let waited = wait_start.elapsed();

        let hold_start = Instant::now();
        let mut term = self.terminal.lock();
        if let Some(pty) = self.pty.lock().as_ref() {
            pty.resize(PtySize {
                rows: size.rows.try_into()?,
                cols: size.cols.try_into()?,
                pixel_width: size.pixel_width.try_into()?,
                pixel_height: size.pixel_height.try_into()?,
            })?;
        }
        term.resize(size);
        Ok((waited, hold_start.elapsed()))
    }

    /// Test-only escape hatch for `test::wedged_pane_isolation`: installs a
    /// no-op `AlertHandler` (so a `SetWindowTitle` OSC below doesn't fire
    /// `LocalPaneNotifHandler::alert`, which needs a process-global
    /// scheduler installed -- see
    /// `get_title_does_not_block_on_a_locked_terminal`'s doc comment for
    /// the same rationale) and sets the terminal's title directly against
    /// the model, mirroring exactly what a real `SetWindowTitle` OSC from
    /// pty output would do.
    #[cfg(test)]
    pub(crate) fn set_title_for_test(&self, title: &str) {
        struct NoopAlertHandler;
        impl AlertHandler for NoopAlertHandler {
            fn alert(&mut self, _alert: Alert) {}
        }
        let mut term = self.terminal.lock();
        term.set_notification_handler(Box::new(NoopAlertHandler));
        term.perform_actions(vec![termwiz::escape::Action::OperatingSystemCommand(
            Box::new(termwiz::escape::OperatingSystemCommand::SetWindowTitle(
                title.to_string(),
            )),
        )]);
    }

    /// Test-only escape hatch: bumps the terminal's sequence number so
    /// `has_unseen_output()` observes genuine new output, mirroring
    /// `has_unseen_output_does_not_block_on_a_locked_terminal`'s setup.
    #[cfg(test)]
    pub(crate) fn increment_seqno_for_test(&self) {
        self.terminal.lock().increment_seqno();
    }

    /// Test-only escape hatch: spawns a thread that holds `terminal.lock()`
    /// until `release` is signaled, and blocks the calling thread until the
    /// lock has actually been acquired. Standing in for a wedged/held
    /// terminal mutex, exactly the technique used by
    /// `tests::has_unseen_output_does_not_block_on_a_locked_terminal` /
    /// `tests::get_title_does_not_block_on_a_locked_terminal`, factored out
    /// here so `test::wedged_pane_isolation` (a different module, which
    /// cannot reach the private `terminal` field directly) can reuse it.
    #[cfg(test)]
    pub(crate) fn spawn_terminal_lock_blocker(
        self: &Arc<Self>,
        release: Arc<std::sync::atomic::AtomicBool>,
    ) -> std::thread::JoinHandle<()> {
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handle = {
            let pane = Arc::clone(self);
            let started = Arc::clone(&started);
            std::thread::spawn(move || {
                let _guard = pane.terminal.lock();
                started.store(true, Ordering::SeqCst);
                while !release.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        };
        while !started.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        handle
    }
}

impl Drop for LocalPane {
    fn drop(&mut self) {
        // Avoid lingering zombies if we can, but don't block forever.
        // <https://github.com/wezterm/wezterm/issues/558>
        if let ProcessState::Running { signaller, .. } = &mut *self.process.lock() {
            let _ = signaller.kill();
        }
    }
}

/// Task #237: `kill()` sends a soft `\x03` interrupt (the same byte the
/// user's physical Ctrl+C writes) and then takes `self.pty` out to
/// `None`, deferring the actual pty drop to a background thread so
/// conhost/OpenConsole has time to read that byte before the pty goes
/// away. These tests confirm the `Option`-aware call sites added for
/// that change behave sanely (no panics, sensible degraded results) once
/// `kill()` has run and `self.pty` is `None`, without needing a real OS
/// process or waiting out `PTY_DROP_GRACE_MS` -- the `take()` itself
/// happens synchronously on the calling thread inside `kill()`, so its
/// effect on `self.pty` is observable immediately after `kill()` returns.
#[cfg(test)]
mod tests;
