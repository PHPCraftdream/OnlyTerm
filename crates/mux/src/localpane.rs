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
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::escape::csi::{Sgr, CSI};
use termwiz::escape::{Action, DeviceControlMode};
use termwiz::input::KeyboardEncoding;
use termwiz::surface::{Line, SequenceNo};
use url::Url;
use wezterm_dynamic::{ToDynamic, Value};
use wezterm_term::color::ColorPalette;
use wezterm_term::{
    Alert, AlertHandler, Clipboard, DownloadHandler, KeyCode, KeyModifiers, MouseEvent, Progress,
    SemanticZone, StableRowIndex, Terminal, TerminalConfiguration, TerminalSize,
};

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
}

/// This is a bit horrible; it can take 700us to tcgetpgrp, so if we have
/// 10 tabs open and run the mouse over them, hovering them each in turn,
/// we can spend 7ms per evaluation of the tab bar state on fetching those
/// pids alone, which can easily lead to stuttering when moving the mouse
/// over all of the tabs.
///
/// This implements a cache holding that fg process and the often queried
/// cwd and process path that allows for stale reads to proceed quickly
/// while the writes can happen in a background thread.
#[cfg(unix)]
#[derive(Clone)]
struct CachedLeaderInfo {
    updated: Instant,
    fd: std::os::fd::RawFd,
    pid: u32,
    path: Option<std::path::PathBuf>,
    current_working_dir: Option<std::path::PathBuf>,
    updating: bool,
}

#[cfg(unix)]
impl CachedLeaderInfo {
    fn new(fd: Option<std::os::fd::RawFd>) -> Self {
        let mut me = Self {
            updated: Instant::now(),
            fd: fd.unwrap_or(-1),
            pid: 0,
            path: None,
            current_working_dir: None,
            updating: false,
        };
        me.update();
        me
    }

    fn can_update(&self) -> bool {
        self.fd != -1 && !self.updating
    }

    fn update(&mut self) {
        // SAFETY: `self.fd` is a valid open terminal file descriptor -- guarded
        // by `can_update`, which requires `self.fd != -1`. `tcgetpgrp` only reads
        // the foreground process group of that terminal and takes no pointers,
        // so there is no aliasing or lifetime concern.
        self.pid = unsafe { libc::tcgetpgrp(self.fd) } as u32;
        if self.pid > 0 {
            self.path = LocalProcessInfo::executable_path(self.pid);
            self.current_working_dir = LocalProcessInfo::current_working_dir(self.pid);
        } else {
            self.path.take();
            self.current_working_dir.take();
        }
        self.updated = Instant::now();
        self.updating = false;
    }

    fn expired(&self) -> bool {
        self.updated.elapsed() > PROC_INFO_CACHE_TTL
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
    domain_id: DomainId,
    tmux_domain: Mutex<Option<Arc<TmuxDomainState>>>,
    proc_list: Mutex<Option<CachedProcInfo>>,
    #[cfg(unix)]
    leader: Arc<Mutex<Option<CachedLeaderInfo>>>,
    command_description: String,
}

#[async_trait(?Send)]
impl Pane for LocalPane {
    fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    fn get_metadata(&self) -> Value {
        #[allow(unused_mut)]
        let mut map: BTreeMap<Value, Value> = BTreeMap::new();

        #[cfg(unix)]
        if let Some(tio) = self.pty.lock().as_ref().and_then(|pty| pty.get_termios()) {
            use nix::sys::termios::LocalFlags;
            // Detect whether we might be in password input mode.
            // If local echo is disabled and canonical input mode
            // is enabled, then we assume that we're in some kind
            // of password-entry mode.
            let pw_input = !tio.local_flags.contains(LocalFlags::ECHO)
                && tio.local_flags.contains(LocalFlags::ICANON);
            map.insert(
                Value::String("password_input".to_string()),
                Value::Bool(pw_input),
            );
        }

        Value::Object(map.into())
    }

    fn get_cursor_position(&self) -> StableCursorPosition {
        let mut cursor = terminal_get_cursor_position(&mut self.terminal.lock());
        if self.tmux_domain.lock().is_some() {
            cursor.visibility = termwiz::surface::CursorVisibility::Hidden;
        }
        cursor
    }

    fn get_keyboard_encoding(&self) -> KeyboardEncoding {
        if self.tmux_domain.lock().is_some() {
            KeyboardEncoding::Xterm
        } else {
            self.terminal.lock().get_keyboard_encoding()
        }
    }

    fn get_current_seqno(&self) -> SequenceNo {
        self.terminal.lock().current_seqno()
    }

    fn get_changed_since(
        &self,
        lines: Range<StableRowIndex>,
        seqno: SequenceNo,
    ) -> RangeSet<StableRowIndex> {
        terminal_get_dirty_lines(&mut self.terminal.lock(), lines, seqno)
    }

    fn for_each_logical_line_in_stable_range_mut(
        &self,
        lines: Range<StableRowIndex>,
        for_line: &mut dyn ForEachPaneLogicalLine,
    ) {
        terminal_for_each_logical_line_in_stable_range_mut(
            &mut self.terminal.lock(),
            lines,
            for_line,
        );
    }

    fn with_lines_mut(&self, lines: Range<StableRowIndex>, with_lines: &mut dyn WithPaneLines) {
        lock_terminal_timed(
            &self.terminal,
            "localpane.terminal_lock.wait.with_lines",
            |term| terminal_with_lines_mut(term, lines, with_lines),
        )
    }

    fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        crate::pane::impl_get_lines_via_with_lines(self, lines)
    }

    fn get_logical_lines(&self, lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
        crate::pane::impl_get_logical_lines_via_get_lines(self, lines)
    }

    fn get_dimensions(&self) -> RenderableDimensions {
        terminal_get_dimensions(&mut self.terminal.lock())
    }

    fn copy_user_vars(&self) -> HashMap<String, String> {
        self.terminal.lock().user_vars().clone()
    }

    fn exit_behavior(&self) -> Option<ExitBehavior> {
        // `None` here means the pty has already been taken by `kill()`
        // pending its deferred drop; there's nothing left to inspect, so
        // just fall through to the default behavior rather than treating
        // this as the `FailedSpawnPty` case.
        let is_failed_spawn = self
            .pty
            .lock()
            .as_ref()
            .map(|pty| pty.is::<crate::domain::FailedSpawnPty>())
            .unwrap_or(false);

        if is_failed_spawn {
            Some(ExitBehavior::CloseOnCleanExit)
        } else {
            None
        }
    }

    /// Kills the process (tree) running in this pane.
    ///
    /// Before actually terminating anything, this arranges to send a
    /// "soft" interrupt: the same `\x03` byte that the user's physical
    /// Ctrl+C writes into the pane (see
    /// `crates/term/src/terminalstate/keyboard.rs` and
    /// `CopySelectionOrInterrupt` in `wezterm-gui`). On Windows, ConPTY's
    /// hosted conhost/OpenConsole reads that byte and raises
    /// `CTRL_C_EVENT` for every process attached to the pseudoconsole (the
    /// whole tree), giving well-behaved processes a chance to shut down
    /// cleanly.
    ///
    /// That write is **not** performed on the calling thread. `kill()` is
    /// called synchronously from the GUI thread in several real paths
    /// (window close, render-thread-hang recovery, lost-context
    /// recovery), and `self.writer` is a thin, unbuffered pass-through
    /// over the real ConPTY stdin pipe. If the target process isn't
    /// reading its stdin (e.g. its pipe buffer is full), a synchronous
    /// write here could block forever and freeze every window in the
    /// process -- exactly the kind of hang this mechanism exists to avoid
    /// causing. Instead, the write is deferred onto the same detached
    /// background thread described below, ahead of its sleep.
    ///
    /// That signal only does any good if the pty sticks around long
    /// enough for conhost to actually read and process the byte, so this
    /// takes `self.pty` out (leaving `None` behind) and moves it onto a
    /// detached background thread that writes the soft signal, then
    /// drops the pty only after `PTY_DROP_GRACE_MS` has elapsed, instead
    /// of letting it drop immediately when this `LocalPane` itself is
    /// dropped moments later. See `PTY_DROP_GRACE_MS`'s doc comment for
    /// why that duration was chosen.
    ///
    /// `self.writer` has to be deferred the same way: `ConPtyMasterPty`
    /// (`crates/pty/src/win/conpty.rs`, `take_writer()`) hands the pty's
    /// stdin `FileDescriptor` to whoever calls `take_writer()` (this pane,
    /// at construction time) and keeps no reference of its own -- so
    /// `self.writer` is the *only* thing keeping that pipe open. Dropping
    /// only `self.pty` while `self.writer` still dropped immediately
    /// (as it would without this) would close the pipe right away and
    /// defeat the grace period just as completely as not deferring
    /// anything at all.
    ///
    /// The existing hard-kill machinery (`signaller.kill()`, which on
    /// Windows waits up to `pty::win::mod::GRACEFUL_KILL_TIMEOUT_MS` for
    /// the child to exit before force-terminating it and closing the Job
    /// Object) is unchanged and still runs synchronously from here, on its
    /// own independent background thread; this method never blocks.
    fn kill(&self) {
        let mut proc = self.process.lock();
        log::debug!(
            "killing process in pane {}, state is {:?}",
            self.pane_id,
            proc
        );
        match &mut *proc {
            ProcessState::Running {
                signaller, killed, ..
            } => {
                if !*killed {
                    // Take the pty out without dropping it yet, so that
                    // `LocalPane::drop()` (which will run shortly after
                    // the last `Arc<LocalPane>` goes away) finds `None`
                    // here and has nothing left to tear down.
                    let taken_pty = self.pty.lock().take();

                    // Swap the real writer out for a no-op sink, so any
                    // late write (e.g. from a caller still holding a
                    // `writer()` guard) is harmlessly discarded instead of
                    // erroring, and so `self.writer`'s Mutex always holds
                    // a valid `Box<dyn Write + Send>` per its field type
                    // (avoiding a wider `Option`-ifying change to the
                    // `writer()` accessor, which is part of the `Pane`
                    // trait's public surface). The real writer is moved
                    // out for deferred dropping, same as the pty.
                    let mut taken_writer = std::mem::replace(
                        &mut *self.writer.lock(),
                        Box::new(std::io::sink()),
                    );

                    // Send the soft signal, and drop the pty/writer once
                    // the grace period has elapsed, all on a detached
                    // background thread -- never on the caller's thread.
                    //
                    // `kill()` is called synchronously from the GUI
                    // thread in several real paths (window close, render
                    // -hang recovery, lost-context recovery), and
                    // `self.writer` is a thin, unbuffered pass-through
                    // over the real ConPTY stdin pipe (see
                    // `WriterWrapper` in `crates/mux/src/domain.rs`): if
                    // the child isn't reading its stdin, a synchronous
                    // `write_all` here can block forever and freeze every
                    // window in the process. Moving the write onto this
                    // same detached thread keeps the byte reaching the
                    // pty well within the grace period (writing one byte
                    // takes microseconds next to `PTY_DROP_GRACE_MS`)
                    // without ever risking the caller's thread.
                    //
                    // Detached, not joined -- mirrors
                    // `RenderThreadHandle::spawn` in
                    // `wezterm-gui/src/renderthread.rs`. Moving
                    // `taken_pty`/`taken_writer` into the closure and
                    // letting them fall out of scope at the end is what
                    // actually drops (and so tears down) them, just
                    // deferred past the grace period.
                    //
                    // Spawned unconditionally (not just when the pty is
                    // still present): `self.writer` is guaranteed to hold
                    // a real, usable writer at this point (the `!*killed`
                    // guard means this whole block only ever runs once
                    // per pane), so there's always a writer worth
                    // signaling here even on the rare path where an
                    // earlier caller had already taken `self.pty`,
                    // leaving `taken_pty` as `None`. Gating this spawn on
                    // `taken_pty.is_some()` (as an earlier version of
                    // this code did) would silently skip the soft signal
                    // on that path even though writing to `taken_writer`
                    // is still meaningful.
                    let builder = std::thread::Builder::new().name("pty-drop-grace".into());
                    match builder.spawn(move || {
                        // Best-effort soft signal; the pty may already be
                        // broken or gone, and that's fine -- this must
                        // never panic or propagate an error.
                        let _ = taken_writer.write_all(b"\x03");
                        let _ = taken_writer.flush();
                        std::thread::sleep(Duration::from_millis(PTY_DROP_GRACE_MS));
                        drop(taken_pty);
                        drop(taken_writer);
                    }) {
                        Ok(join_handle) => drop(join_handle),
                        Err(err) => {
                            log::error!(
                                "Failed to spawn pty-drop-grace thread, \
                                 dropping pty/writer immediately without \
                                 sending the soft signal: {:#}",
                                err
                            );
                        }
                    }
                }

                let _ = signaller.kill();
                *killed = true;
            }
            ProcessState::DeadPendingClose { killed } => {
                *killed = true;
            }
            _ => {}
        }
    }

    fn is_dead(&self) -> bool {
        let mut proc = self.process.lock();

        const EXIT_BEHAVIOR: &str = "This message is shown because \
            \x1b]8;;https://wezterm.org/\
            config/reference/config/exit_behavior.html\
            \x1b\\exit_behavior\x1b]8;;\x1b\\";

        let mut terse = String::new();
        let mut brief = String::new();
        let mut trailer = String::new();
        let cmd = &self.command_description;

        match &mut *proc {
            ProcessState::Running {
                child_waiter,
                killed,
                ..
            } => {
                let status = match child_waiter.try_recv() {
                    Ok(Ok(s)) => Some(s),
                    Err(TryRecvError::Empty) => None,
                    _ => Some(ExitStatus::with_exit_code(1)),
                };

                if let Some(status) = status {
                    let success = match status.success() {
                        true => true,
                        false => configuration()
                            .clean_exit_codes
                            .contains(&status.exit_code()),
                    };

                    match (
                        self.exit_behavior()
                            .unwrap_or_else(|| configuration().exit_behavior),
                        success,
                        killed,
                    ) {
                        (ExitBehavior::Close, _, _) => *proc = ProcessState::Dead,
                        (ExitBehavior::CloseOnCleanExit, false, _) => {
                            brief = format!("⚠️  Process {cmd} didn't exit cleanly");
                            terse = format!("{status}.");
                            trailer = format!("{EXIT_BEHAVIOR}=\"CloseOnCleanExit\"");

                            *proc = ProcessState::DeadPendingClose { killed: false }
                        }
                        (ExitBehavior::CloseOnCleanExit, ..) => *proc = ProcessState::Dead,
                        (ExitBehavior::Hold, success, false) => {
                            trailer = format!("{EXIT_BEHAVIOR}=\"Hold\"");

                            if success {
                                brief = format!("👍 Process {cmd} completed.");
                                terse = "done".to_string();
                            } else {
                                brief = format!("⚠️  Process {cmd} didn't exit cleanly");
                                terse = format!("{status}");
                            }
                            *proc = ProcessState::DeadPendingClose { killed: false }
                        }
                        (ExitBehavior::Hold, _, true) => *proc = ProcessState::Dead,
                    }
                    log::debug!("child terminated, new state is {:?}", proc);
                }
            }
            ProcessState::DeadPendingClose { killed } => {
                if *killed {
                    *proc = ProcessState::Dead;
                    log::debug!("child state -> {:?}", proc);
                }
            }
            ProcessState::Dead => {}
        }

        let mut notify = None;
        if !terse.is_empty() {
            match configuration().exit_behavior_messaging {
                ExitBehaviorMessaging::Verbose => {
                    if terse == "done" {
                        notify = Some(format!("\r\n{brief}\r\n{trailer}"));
                    } else {
                        notify = Some(format!("\r\n{brief}\r\n{terse}\r\n{trailer}"));
                    }
                }
                ExitBehaviorMessaging::Brief => {
                    if terse == "done" {
                        notify = Some(format!("\r\n{brief}"));
                    } else {
                        notify = Some(format!("\r\n{brief}\r\n{terse}"));
                    }
                }
                ExitBehaviorMessaging::Terse => {
                    notify = Some(format!("\r\n[{terse}]"));
                }
                ExitBehaviorMessaging::None => {}
            }
        }

        if let Some(notify) = notify {
            emit_output_for_pane(self.pane_id, &notify);
        }

        match &*proc {
            ProcessState::Running { .. } => false,
            ProcessState::DeadPendingClose { .. } => false,
            ProcessState::Dead => true,
        }
    }

    fn set_clipboard(&self, clipboard: &Arc<dyn Clipboard>) {
        self.terminal.lock().set_clipboard(clipboard);
    }

    fn set_download_handler(&self, handler: &Arc<dyn DownloadHandler>) {
        self.terminal.lock().set_download_handler(handler);
    }

    fn set_config(&self, config: Arc<dyn TerminalConfiguration>) {
        self.terminal.lock().set_config(config);
    }

    fn get_config(&self) -> Option<Arc<dyn TerminalConfiguration>> {
        Some(self.terminal.lock().get_config())
    }

    fn perform_actions(&self, actions: Vec<termwiz::escape::Action>) {
        self.perform_actions_chunked(actions, configuration().mux_output_parser_chunk_size)
    }

    fn mouse_event(&self, event: MouseEvent) -> Result<(), Error> {
        Mux::get().record_input_for_current_identity();
        lock_terminal_timed(
            &self.terminal,
            "localpane.terminal_lock.wait.key_input",
            |term| term.mouse_event(event),
        )
    }

    fn key_down(&self, key: KeyCode, mods: KeyModifiers) -> Result<(), Error> {
        Mux::get().record_input_for_current_identity();
        if self.tmux_domain.lock().is_some() {
            log::trace!("key: {:?}", key);
            if key == KeyCode::Char('q') {
                lock_terminal_timed(
                    &self.terminal,
                    "localpane.terminal_lock.wait.key_input",
                    |term| term.send_paste("detach\n"),
                )?;
            }
            return Ok(());
        } else {
            lock_terminal_timed(
                &self.terminal,
                "localpane.terminal_lock.wait.key_input",
                |term| term.key_down(key, mods),
            )
        }
    }

    fn key_up(&self, key: KeyCode, mods: KeyModifiers) -> Result<(), Error> {
        Mux::get().record_input_for_current_identity();
        lock_terminal_timed(
            &self.terminal,
            "localpane.terminal_lock.wait.key_input",
            |term| term.key_up(key, mods),
        )
    }

    fn resize(&self, size: TerminalSize) -> Result<(), Error> {
        // If the pty is already gone (pane killed, teardown deferred --
        // see `kill()`), there's nowhere for a resize to go; silently
        // skip the pty resize but still update the terminal model, since
        // a killed pane may still be rendered briefly (e.g. while
        // `DeadPendingClose`) and its dimensions should stay coherent.
        if let Some(pty) = self.pty.lock().as_ref() {
            pty.resize(PtySize {
                rows: size.rows.try_into()?,
                cols: size.cols.try_into()?,
                pixel_width: size.pixel_width.try_into()?,
                pixel_height: size.pixel_height.try_into()?,
            })?;
        }
        self.terminal.lock().resize(size);
        Ok(())
    }

    fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write> {
        Mux::get().record_input_for_current_identity();
        MutexGuard::map(self.writer.lock(), |writer| {
            let w: &mut dyn std::io::Write = writer;
            w
        })
    }

    fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
        // `None` pty (already taken by `kill()`, pending deferred drop):
        // there's nothing left to read from.
        match self.pty.lock().as_ref() {
            Some(pty) => Ok(Some(pty.try_clone_reader()?)),
            None => Ok(None),
        }
    }

    fn send_paste(&self, text: &str) -> Result<(), Error> {
        Mux::get().record_input_for_current_identity();
        if self.tmux_domain.lock().is_some() {
            Ok(())
        } else {
            self.terminal.lock().send_paste(text)
        }
    }

    fn get_title(&self) -> String {
        let title = self.terminal.lock().get_title().to_string();
        // If the title is the default pane title, then try to spice
        // things up a bit by returning the process basename instead
        if title == "wezterm" {
            if let Some(proc_name) = self.get_foreground_process_name(CachePolicy::AllowStale) {
                let proc_name = std::path::Path::new(&proc_name);
                if let Some(name) = proc_name.file_name() {
                    return name.to_string_lossy().to_string();
                }
            }
        }

        title
    }

    fn get_progress(&self) -> Progress {
        self.terminal.lock().get_progress()
    }

    fn palette(&self) -> ColorPalette {
        self.terminal.lock().palette()
    }

    fn domain_id(&self) -> DomainId {
        self.domain_id
    }

    fn erase_scrollback(&self, erase_mode: ScrollbackEraseMode) {
        match erase_mode {
            ScrollbackEraseMode::ScrollbackOnly => {
                self.terminal.lock().erase_scrollback();
            }
            ScrollbackEraseMode::ScrollbackAndViewport => {
                self.terminal.lock().erase_scrollback_and_viewport();
            }
        }
    }

    fn focus_changed(&self, focused: bool) {
        self.terminal.lock().focus_changed(focused);
    }

    fn has_unseen_output(&self) -> bool {
        self.terminal.lock().has_unseen_output()
    }

    fn is_mouse_grabbed(&self) -> bool {
        if self.tmux_domain.lock().is_some() {
            false
        } else {
            self.terminal.lock().is_mouse_grabbed()
        }
    }

    fn is_alt_screen_active(&self) -> bool {
        if self.tmux_domain.lock().is_some() {
            false
        } else {
            self.terminal.lock().is_alt_screen_active()
        }
    }

    fn get_current_working_dir(&self, policy: CachePolicy) -> Option<Url> {
        self.terminal
            .lock()
            .get_current_dir()
            .cloned()
            .or_else(|| self.divine_current_working_dir(policy))
    }

    fn tty_name(&self) -> Option<String> {
        #[cfg(unix)]
        {
            // `None` pty (already taken by `kill()`): nothing to name.
            let name = self.pty.lock().as_ref()?.tty_name()?;
            Some(name.to_string_lossy().into_owned())
        }

        #[cfg(windows)]
        {
            None
        }
    }

    fn get_foreground_process_info(&self, policy: CachePolicy) -> Option<LocalProcessInfo> {
        #[cfg(unix)]
        if let Some(pid) = self
            .pty
            .lock()
            .as_ref()
            .and_then(|pty| pty.process_group_leader())
        {
            return LocalProcessInfo::with_root_pid(pid as u32);
        }

        self.divine_foreground_process(policy)
    }

    fn get_foreground_process_name(&self, policy: CachePolicy) -> Option<String> {
        #[cfg(unix)]
        {
            let leader = self.get_leader(policy);
            if let Some(path) = &leader.path {
                return Some(path.to_string_lossy().to_string());
            }
            return None;
        }

        #[cfg(windows)]
        if let Some(fg) = self.divine_foreground_process(policy) {
            return Some(fg.executable.to_string_lossy().to_string());
        }

        #[allow(unreachable_code)]
        None
    }

    fn can_close_without_prompting(&self, _reason: CloseReason) -> bool {
        if let Some(info) = self.divine_process_list(CachePolicy::FetchImmediate) {
            log::trace!(
                "can_close_without_prompting? procs in pane {:#?}",
                info.root
            );

            let hook_result = config::run_immediate_with_rhai_config(|state| {
                let state = match state {
                    Some(state) => state,
                    None => return Ok(None),
                };
                let arg = config::rhai_value::dynamic_to_rhai_dynamic(&info.root.to_dynamic());
                let v = config::rhai_bridge::emit_sync_callback(
                    &state,
                    "mux-is-process-stateful",
                    vec![arg],
                )?;
                if v.is_unit() {
                    Ok(None)
                } else {
                    Ok(v.as_bool().ok())
                }
            });

            fn default_stateful_check(proc_list: &LocalProcessInfo) -> bool {
                // Fig uses `figterm` a pseudo terminal for a lot of functionality, it runs between
                // the shell and terminal. Unfortunately it is typically named `<shell> (figterm)`,
                // which prevents the statuful check from passing. This strips the suffix from the
                // process name to allow the check to pass.
                let names = proc_list
                    .flatten_to_exe_names()
                    .into_iter()
                    .map(|s| match s.strip_suffix(" (figterm)") {
                        Some(s) => s.into(),
                        None => s,
                    })
                    .collect::<HashSet<_>>();

                let skip = configuration()
                    .skip_close_confirmation_for_processes_named
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>();

                if !names.is_subset(&skip) {
                    // There are other processes running than are listed,
                    // so we consider this to be stateful
                    return true;
                }
                false
            }

            let is_stateful = match hook_result {
                Ok(None) => default_stateful_check(&info.root),
                Ok(Some(s)) => s,
                Err(err) => {
                    log::error!(
                        "Error while running mux-is-process-stateful \
                         hook: {:#}, falling back to default behavior",
                        err
                    );
                    default_stateful_check(&info.root)
                }
            };

            !is_stateful
        } else {
            #[cfg(unix)]
            {
                // If the process is dead but exit_behavior is holding the
                // window, we don't need to prompt to confirm closing.
                // That is detectable as no longer having a process group leader.
                // A `None` pty (already taken by `kill()`) counts the same
                // way: there's definitely no leader left to speak of.
                let has_leader = self
                    .pty
                    .lock()
                    .as_ref()
                    .and_then(|pty| pty.process_group_leader())
                    .is_some();
                if !has_leader {
                    return true;
                }
            }

            false
        }
    }

    fn get_semantic_zones(&self) -> anyhow::Result<Vec<SemanticZone>> {
        let mut term = self.terminal.lock();
        term.get_semantic_zones()
    }

    async fn search(
        &self,
        pattern: Pattern,
        range: Range<StableRowIndex>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let term = self.terminal.lock();
        let screen = term.screen();

        enum CompiledPattern {
            CaseSensitiveString(String),
            CaseInSensitiveString(String),
            Regex(Regex),
        }

        let pattern = match pattern {
            Pattern::CaseSensitiveString(s) => CompiledPattern::CaseSensitiveString(s),
            Pattern::CaseInSensitiveString(s) => {
                // normalize the case so we match everything lowercase
                CompiledPattern::CaseInSensitiveString(s.to_lowercase())
            }
            Pattern::Regex(r) => CompiledPattern::Regex(Regex::new(&r)?),
        };

        let mut results = vec![];
        let mut uniq_matches: HashMap<String, usize> = HashMap::new();

        screen.for_each_logical_line_in_stable_range(range, |sr, lines| {
            if let Some(limit) = limit {
                if results.len() == limit as usize {
                    // We've reach the limit, stop iteration.
                    return false;
                }
            }

            if lines.is_empty() {
                // Nothing to do on this iteration, carry on with the next.
                return true;
            }
            let haystack = if lines.len() == 1 {
                lines[0].as_str()
            } else {
                let mut s = String::new();
                for line in lines {
                    s.push_str(&line.as_str());
                }
                Cow::Owned(s)
            };
            let stable_idx = sr.start;

            if haystack.is_empty() {
                return true;
            }

            let haystack = match &pattern {
                CompiledPattern::CaseInSensitiveString(_) => Cow::Owned(haystack.to_lowercase()),
                _ => haystack,
            };
            let mut coords = None;

            match &pattern {
                CompiledPattern::CaseInSensitiveString(s)
                | CompiledPattern::CaseSensitiveString(s) => {
                    for (idx, s) in haystack.match_indices(s) {
                        found_match(
                            s,
                            idx,
                            lines,
                            stable_idx,
                            &mut uniq_matches,
                            &mut coords,
                            &mut results,
                        );
                    }
                }
                CompiledPattern::Regex(re) => {
                    // Allow for the regex to contain captures
                    for capture_res in re.captures_iter(&haystack) {
                        match capture_res {
                            Ok(c) => {
                                // Look for the captures in reverse order, as index==0 is
                                // the whole matched string.  We can't just call
                                // `c.iter().rev()` as the capture iterator isn't double-ended.
                                for idx in (0..c.len()).rev() {
                                    if let Some(m) = c.get(idx) {
                                        found_match(
                                            m.as_str(),
                                            m.start(),
                                            lines,
                                            stable_idx,
                                            &mut uniq_matches,
                                            &mut coords,
                                            &mut results,
                                        );
                                        break;
                                    }
                                }
                            }
                            Err(err) => {
                                // On errors like max backtracking limit reached, fancy_regex does
                                // NOT advance the iterator position, so silently ignoring Err
                                // would loop forever.
                                log::warn!("line {stable_idx} search error: {err}");
                                log::warn!("stopping collecting matches on line {stable_idx}");
                                break;
                            }
                        }
                    }
                }
            }

            // Keep iterating
            true
        });

        #[derive(Copy, Clone, Debug)]
        struct Coord {
            byte_idx: usize,
            grapheme_idx: usize,
            stable_row: StableRowIndex,
        }

        fn found_match(
            text: &str,
            byte_idx: usize,
            lines: &[&Line],
            stable_idx: StableRowIndex,
            uniq_matches: &mut HashMap<String, usize>,
            coords: &mut Option<Vec<Coord>>,
            results: &mut Vec<SearchResult>,
        ) {
            if coords.is_none() {
                coords.replace(make_coords(lines, stable_idx));
            }
            let coords = coords.as_ref().unwrap();

            let match_id = match uniq_matches.get(text).copied() {
                Some(id) => id,
                None => {
                    let id = uniq_matches.len();
                    uniq_matches.insert(text.to_owned(), id);
                    id
                }
            };
            let (start_x, start_y) = haystack_idx_to_coord(byte_idx, coords);
            let (end_x, end_y) = haystack_idx_to_coord(byte_idx + text.len(), coords);
            results.push(SearchResult {
                start_x,
                start_y,
                end_x,
                end_y,
                match_id,
            });
        }

        fn make_coords(lines: &[&Line], stable_row: StableRowIndex) -> Vec<Coord> {
            let mut byte_idx = 0;
            let mut coords = vec![];

            for (row_idx, line) in lines.iter().enumerate() {
                for cell in line.visible_cells() {
                    coords.push(Coord {
                        byte_idx,
                        grapheme_idx: cell.cell_index(),
                        stable_row: stable_row + row_idx as StableRowIndex,
                    });
                    byte_idx += cell.str().len();
                }
            }

            coords
        }

        fn haystack_idx_to_coord(idx: usize, coords: &[Coord]) -> (usize, StableRowIndex) {
            let c = coords
                .binary_search_by(|ele| ele.byte_idx.cmp(&idx))
                .or_else(|i| -> Result<usize, usize> { Ok(i) })
                .unwrap();
            let coord = coords.get(c).map(|c| *c).unwrap_or_else(|| {
                let last = coords.last().unwrap();
                Coord {
                    grapheme_idx: last.grapheme_idx + 1,
                    ..*last
                }
            });
            (coord.grapheme_idx, coord.stable_row)
        }

        Ok(results)
    }
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

impl wezterm_term::DeviceControlHandler for LocalPaneDCSHandler {
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
                    tmux.advance(events);
                } else {
                    log::warn!("unhandled DeviceControlMode::TmuxEvents {:?}", &events);
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
    pub fn new(
        pane_id: PaneId,
        mut terminal: Terminal,
        process: Box<dyn Child + Send>,
        pty: Box<dyn MasterPty>,
        writer: Box<dyn Write + Send>,
        domain_id: DomainId,
        command_description: String,
    ) -> Self {
        let (process, signaller, pid) = split_child(process);

        terminal.set_device_control_handler(Box::new(LocalPaneDCSHandler {
            pane_id,
            tmux_domain: None,
        }));
        terminal.set_notification_handler(Box::new(LocalPaneNotifHandler { pane_id }));

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
            domain_id,
            tmux_domain: Mutex::new(None),
            proc_list: Mutex::new(None),
            #[cfg(unix)]
            leader: Arc::new(Mutex::new(None)),
            command_description,
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
    /// of which are correct to do once per chunk).
    fn perform_actions_chunked(&self, actions: Vec<Action>, chunk_size: usize) {
        if actions.len() <= chunk_size.max(1) {
            // Common case (small batches, e.g. interactive typing): no
            // benefit to chunking, so avoid the `Vec` chunking overhead
            // and just take the lock once, as before.
            lock_terminal_timed(
                &self.terminal,
                "localpane.terminal_lock.wait.perform_actions",
                |term| term.perform_actions(actions),
            );
            return;
        }

        for chunk in actions.chunks(chunk_size.max(1)) {
            lock_terminal_timed(
                &self.terminal,
                "localpane.terminal_lock.wait.perform_actions",
                |term| term.perform_actions(chunk.to_vec()),
            );
        }
    }

    #[cfg(unix)]
    fn get_leader(&self, policy: CachePolicy) -> CachedLeaderInfo {
        let mut leader = self.leader.lock();

        if policy == CachePolicy::FetchImmediate {
            // `None` pty (already taken by `kill()`) has no fd to offer;
            // `CachedLeaderInfo::new(None)` degrades to fd `-1`, which
            // `can_update()` already treats as "nothing to query".
            leader.replace(CachedLeaderInfo::new(
                self.pty.lock().as_ref().and_then(|pty| pty.as_raw_fd()),
            ));
        } else if let Some(info) = leader.as_mut() {
            // If stale, queue up some work in another thread to update.
            // Right now, we'll return the stale data.
            if info.expired() && info.can_update() {
                info.updating = true;
                let leader_ref = Arc::clone(&self.leader);
                std::thread::spawn(move || {
                    let mut leader = leader_ref.lock();
                    if let Some(leader) = leader.as_mut() {
                        leader.update();
                    }
                });
            }
        } else {
            // `None` pty (already taken by `kill()`) has no fd to offer;
            // `CachedLeaderInfo::new(None)` degrades to fd `-1`, which
            // `can_update()` already treats as "nothing to query".
            leader.replace(CachedLeaderInfo::new(
                self.pty.lock().as_ref().and_then(|pty| pty.as_raw_fd()),
            ));
        }

        (*leader).clone().unwrap()
    }

    fn divine_current_working_dir(&self, policy: CachePolicy) -> Option<Url> {
        #[cfg(unix)]
        {
            let leader = self.get_leader(policy);
            if let Some(path) = &leader.current_working_dir {
                return Url::from_directory_path(path).ok();
            }
            return None;
        }

        #[cfg(windows)]
        if let Some(fg) = self.divine_foreground_process(policy) {
            return Url::from_directory_path(fg.cwd.clone()).ok();
        }

        #[allow(unreachable_code)]
        None
    }

    fn divine_process_list(
        &self,
        policy: CachePolicy,
    ) -> Option<MappedMutexGuard<'_, CachedProcInfo>> {
        if let ProcessState::Running { pid: Some(pid), .. } = &*self.process.lock() {
            let mut proc_list = self.proc_list.lock();

            let expired = policy == CachePolicy::FetchImmediate
                || proc_list
                    .as_ref()
                    .map(|info| info.updated.elapsed() > PROC_INFO_CACHE_TTL)
                    .unwrap_or(true);

            if expired {
                log::trace!("CachedProcInfo expired, refresh");
                let root = LocalProcessInfo::with_root_pid(*pid)?;

                // Windows doesn't have any job control or session concept,
                // so we infer that the equivalent to the process group
                // leader is the most recently spawned program running
                // in the console
                let mut youngest = &root;

                // Walk the process tree with an explicit stack rather
                // than recursion: this tree is rebuilt from a live
                // system-wide process snapshot every time the cache
                // expires, and its depth is not bounded by anything
                // wezterm controls, so a recursive walk here could in
                // principle overflow the stack for a sufficiently deep
                // process tree.
                let mut stack: Vec<&LocalProcessInfo> = vec![&root];
                while let Some(proc) = stack.pop() {
                    if proc.start_time >= youngest.start_time {
                        youngest = proc;
                    }

                    for child in proc.children.values() {
                        #[cfg(windows)]
                        if child.console == 0 {
                            continue;
                        }
                        stack.push(child);
                    }
                }
                let mut foreground = youngest.clone();
                foreground.children.clear();

                proc_list.replace(CachedProcInfo {
                    root,
                    foreground,
                    updated: Instant::now(),
                });
                log::trace!("CachedProcInfo updated");
            }

            return Some(MutexGuard::map(proc_list, |info| info.as_mut().unwrap()));
        }
        None
    }

    #[allow(dead_code)]
    fn divine_foreground_process(&self, policy: CachePolicy) -> Option<LocalProcessInfo> {
        if let Some(info) = self.divine_process_list(policy) {
            Some(info.foreground.clone())
        } else {
            None
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
mod tests {
    use super::*;
    use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
    use std::io::{Read, Result as IoResult};
    use wezterm_term::color::ColorPalette;
    use wezterm_term::{TerminalConfiguration, TerminalSize};

    /// A `Child` double that never exits on its own, mirroring
    /// `test::terminal_lock_contention::NeverExitChild`: `LocalPane` only
    /// needs something implementing the trait to track process state,
    /// and these tests never let it actually run to completion.
    #[derive(Debug)]
    struct NeverExitChild;

    impl Child for NeverExitChild {
        fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
            Ok(None)
        }
        fn wait(&mut self) -> IoResult<ExitStatus> {
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

    /// A `MasterPty` double that records whether it has been dropped, so
    /// tests can confirm `kill()` doesn't drop it inline (the whole point
    /// of the deferred-drop mechanism).
    struct FakeMasterPty {
        size: Mutex<PtySize>,
        dropped: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Drop for FakeMasterPty {
        fn drop(&mut self) {
            self.dropped.store(true, std::sync::atomic::Ordering::SeqCst);
        }
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
        #[cfg(unix)]
        fn process_group_leader(&self) -> Option<libc::pid_t> {
            None
        }
        #[cfg(unix)]
        fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
            None
        }
        #[cfg(unix)]
        fn tty_name(&self) -> Option<std::path::PathBuf> {
            None
        }
    }

    /// A `Write` double that records whether it has been dropped, so
    /// tests can confirm `kill()` defers dropping the *real* writer (not
    /// just the pty) rather than letting it drop inline along with the
    /// rest of `LocalPane` -- see `kill()`'s doc comment for why that
    /// matters (`ConPtyMasterPty::take_writer()` hands out the pty's only
    /// reference to its stdin pipe).
    struct TrackedWriter {
        dropped: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Write for TrackedWriter {
        fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> IoResult<()> {
            Ok(())
        }
    }

    impl Drop for TrackedWriter {
        fn drop(&mut self) {
            self.dropped.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// A `Write` double whose `write` call blocks until the test
    /// explicitly releases it, standing in for a real ConPTY stdin pipe
    /// whose reader (the child process) has stopped reading -- the
    /// scenario that used to make `kill()` hang the GUI thread forever
    /// (see the regression this test guards against, below). Records
    /// whether a write was ever observed, so the test can confirm the
    /// soft `\x03` byte does eventually get written on the background
    /// thread once released.
    struct BlockingWriter {
        /// Held locked by the test until it wants to let a pending
        /// `write` call through.
        gate: Arc<Mutex<()>>,
        wrote: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Write for BlockingWriter {
        fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
            // Blocks for as long as the test is holding `gate` locked,
            // mirroring a real write blocking on a full, unread pipe.
            let _guard = self.gate.lock();
            self.wrote.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(buf.len())
        }
        fn flush(&mut self) -> IoResult<()> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct TestConfig;
    impl TerminalConfiguration for TestConfig {
        fn color_palette(&self) -> ColorPalette {
            ColorPalette::default()
        }
    }

    const ROWS: usize = 24;
    const COLS: usize = 80;

    fn make_pane() -> (Arc<LocalPane>, Arc<std::sync::atomic::AtomicBool>) {
        let size = TerminalSize {
            rows: ROWS,
            cols: COLS,
            pixel_width: COLS * 8,
            pixel_height: ROWS * 16,
            dpi: 0,
        };
        let terminal = Terminal::new(
            size,
            Arc::new(TestConfig),
            "WezTerm",
            "0.0.0",
            Box::new(Vec::new()),
        );
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pty = Box::new(FakeMasterPty {
            size: Mutex::new(PtySize {
                rows: ROWS as u16,
                cols: COLS as u16,
                pixel_width: 0,
                pixel_height: 0,
            }),
            dropped: Arc::clone(&dropped),
        });
        let writer = Box::new(Vec::new());
        let pane = Arc::new(LocalPane::new(
            1,
            terminal,
            Box::new(NeverExitChild),
            pty,
            writer,
            1,
            "test".to_string(),
        ));
        (pane, dropped)
    }

    /// Like `make_pane`, but with a `TrackedWriter` in place of the plain
    /// `Vec::new()` writer, so tests can also observe whether the *real*
    /// writer's drop was deferred alongside the pty's.
    fn make_pane_with_tracked_writer() -> (
        Arc<LocalPane>,
        Arc<std::sync::atomic::AtomicBool>,
        Arc<std::sync::atomic::AtomicBool>,
    ) {
        let size = TerminalSize {
            rows: ROWS,
            cols: COLS,
            pixel_width: COLS * 8,
            pixel_height: ROWS * 16,
            dpi: 0,
        };
        let terminal = Terminal::new(
            size,
            Arc::new(TestConfig),
            "WezTerm",
            "0.0.0",
            Box::new(Vec::new()),
        );
        let pty_dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pty = Box::new(FakeMasterPty {
            size: Mutex::new(PtySize {
                rows: ROWS as u16,
                cols: COLS as u16,
                pixel_width: 0,
                pixel_height: 0,
            }),
            dropped: Arc::clone(&pty_dropped),
        });
        let writer_dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer = Box::new(TrackedWriter {
            dropped: Arc::clone(&writer_dropped),
        });
        let pane = Arc::new(LocalPane::new(
            1,
            terminal,
            Box::new(NeverExitChild),
            pty,
            writer,
            1,
            "test".to_string(),
        ));
        (pane, pty_dropped, writer_dropped)
    }

    /// Like `make_pane`, but with a `BlockingWriter` in place of the
    /// plain `Vec::new()` writer, so tests can prove `kill()` never
    /// blocks on the caller's thread even when writing the soft-signal
    /// byte would block forever.
    fn make_pane_with_blocking_writer() -> (
        Arc<LocalPane>,
        Arc<Mutex<()>>,
        Arc<std::sync::atomic::AtomicBool>,
    ) {
        let size = TerminalSize {
            rows: ROWS,
            cols: COLS,
            pixel_width: COLS * 8,
            pixel_height: ROWS * 16,
            dpi: 0,
        };
        let terminal = Terminal::new(
            size,
            Arc::new(TestConfig),
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
            dropped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        let gate = Arc::new(Mutex::new(()));
        let wrote = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer = Box::new(BlockingWriter {
            gate: Arc::clone(&gate),
            wrote: Arc::clone(&wrote),
        });
        let pane = Arc::new(LocalPane::new(
            1,
            terminal,
            Box::new(NeverExitChild),
            pty,
            writer,
            1,
            "test".to_string(),
        ));
        (pane, gate, wrote)
    }

    /// Regression test for the bug fixed alongside this test: `kill()`
    /// used to write the soft `\x03` signal synchronously on the calling
    /// thread, via `self.writer` -- a thin, unbuffered pass-through over
    /// the real ConPTY stdin pipe. Since `kill()` is called from the GUI
    /// thread in several real paths, a blocked write (e.g. the child's
    /// stdin pipe is full and it isn't reading) used to freeze every
    /// window in the process. Confirms `kill()` now returns promptly even
    /// when the write would block forever, and that the byte still gets
    /// written once the write is able to proceed, proving the write moved
    /// to the background `pty-drop-grace` thread instead of being skipped
    /// outright.
    #[test]
    fn kill_does_not_block_on_a_stuck_writer() {
        let (pane, gate, wrote) = make_pane_with_blocking_writer();

        // Hold the gate so any write into `BlockingWriter` blocks, as it
        // would on a real pipe whose reader has stopped reading.
        let guard = gate.lock();

        let start = Instant::now();
        pane.kill();
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "kill() must not block on a stuck writer; took {:?}",
            elapsed
        );
        assert!(
            !wrote.load(std::sync::atomic::Ordering::SeqCst),
            "the write is still blocked behind the gate, so it must not \
             have completed yet"
        );

        // Release the gate so the background thread's blocked write can
        // complete, then give it a moment to actually run.
        drop(guard);
        let mut waited = Duration::ZERO;
        while !wrote.load(std::sync::atomic::Ordering::SeqCst)
            && waited < Duration::from_secs(5)
        {
            std::thread::sleep(Duration::from_millis(10));
            waited += Duration::from_millis(10);
        }
        assert!(
            wrote.load(std::sync::atomic::Ordering::SeqCst),
            "the soft-signal write must still happen on the background \
             thread once it's able to proceed"
        );
    }

    /// After `kill()`, `self.pty` must be `None` (taken, not yet
    /// dropped): this is the mechanism that lets `LocalPane::drop()` run
    /// moments later without tearing down the pty inline.
    #[test]
    fn kill_takes_pty_leaving_none() {
        let (pane, dropped) = make_pane();
        assert!(pane.pty.lock().is_some());
        pane.kill();
        assert!(pane.pty.lock().is_none());
        // The deferred-drop thread hasn't had time to run yet (it sleeps
        // for PTY_DROP_GRACE_MS before dropping), so the pty is still
        // alive, just no longer reachable through `self.pty`.
        assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// Regression test for the bug fixed alongside this test: `kill()`
    /// used to defer dropping `self.pty` but left `self.writer` dropping
    /// immediately, which closed the pty's underlying pipe right away
    /// regardless -- because `ConPtyMasterPty::take_writer()`
    /// (`crates/pty/src/win/conpty.rs`) hands the pty's stdin
    /// `FileDescriptor` to whoever calls it and keeps no reference of its
    /// own, `self.writer` is the *only* thing keeping that pipe open.
    /// Confirms the *real* writer's drop is now deferred the same way the
    /// pty's is, and that `self.writer` is left holding a harmless sink in
    /// the meantime rather than `None` (avoiding a wider `Option`-ifying
    /// change to the `writer()` accessor, part of the `Pane` trait's
    /// public surface).
    #[test]
    fn kill_defers_the_real_writer_too() {
        let (pane, pty_dropped, writer_dropped) = make_pane_with_tracked_writer();
        pane.kill();

        // Neither the pty nor the real writer have been dropped yet (the
        // deferred-drop thread sleeps for PTY_DROP_GRACE_MS first).
        assert!(!pty_dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert!(
            !writer_dropped.load(std::sync::atomic::Ordering::SeqCst),
            "the real writer must not drop immediately, or the deferred \
             pty drop accomplishes nothing"
        );

        // `self.writer` still holds *something* usable -- the sink
        // swapped in by `kill()` -- rather than being left in a state
        // that would panic a caller still holding a `writer()` guard.
        // Goes at the field directly (not the public `writer()`
        // accessor, which also calls `Mux::get()` for unrelated
        // bookkeeping that isn't set up in this unit test).
        let mut writer = pane.writer.lock();
        writer
            .write_all(b"late write after kill")
            .expect("writing to the post-kill sink must not error");
    }

    /// Calling `kill()` twice must not panic or double-fire the
    /// signal-and-defer sequence; the second call should see `killed ==
    /// true` and skip straight past it.
    #[test]
    fn kill_is_idempotent() {
        let (pane, _dropped) = make_pane();
        pane.kill();
        pane.kill();
        assert!(pane.pty.lock().is_none());
    }

    /// Every `Option`-aware call site added for task #237 must degrade
    /// gracefully (no panic, sensible default) once `self.pty` is `None`
    /// after `kill()`.
    #[test]
    fn pty_dependent_calls_dont_panic_after_kill() {
        let (pane, _dropped) = make_pane();
        pane.kill();

        // exit_behavior(): falls through to `None` rather than treating
        // a gone pty as `FailedSpawnPty`.
        assert_eq!(pane.exit_behavior(), None);

        // resize(): silently skips the pty resize, still updates the
        // terminal model, and must not error.
        let new_size = TerminalSize {
            rows: ROWS + 1,
            cols: COLS + 1,
            pixel_width: (COLS + 1) * 8,
            pixel_height: (ROWS + 1) * 16,
            dpi: 0,
        };
        assert!(pane.resize(new_size).is_ok());

        // reader(): nothing left to read from.
        assert!(pane.reader().unwrap().is_none());

        // get_metadata(): must not panic digging for termios on a gone pty.
        let _ = pane.get_metadata();

        #[cfg(unix)]
        {
            assert_eq!(pane.tty_name(), None);
            assert_eq!(pane.get_foreground_process_info(CachePolicy::FetchImmediate), None);

            // can_close_without_prompting()'s pty-derived "no leader"
            // check only exists on unix (see the `#[cfg(unix)]` block in
            // its `else` branch); a gone pty has no leader, so this
            // should report "safe to close without prompting".
            assert!(pane.can_close_without_prompting(CloseReason::Tab));
        }

        // On non-unix platforms `can_close_without_prompting` has no
        // pty-derived fallback at all (it falls through to `false`
        // regardless of pty state once `divine_process_list` finds no
        // pid), so the only thing this call must do post-kill is *not
        // panic* digging through a gone pty.
        #[cfg(not(unix))]
        {
            let _ = pane.can_close_without_prompting(CloseReason::Tab);
        }
    }
}
