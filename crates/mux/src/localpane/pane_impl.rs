//! Pane trait implementation: terminal I/O, rendering and lifecycle.
use super::*;

#[async_trait(?Send)]
impl Pane for LocalPane {
    fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    fn get_metadata(&self) -> Value {
        let map: BTreeMap<Value, Value> = BTreeMap::new();

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

    /// Single-lock snapshot for rendering (ghost-cursor-fix-plan Phase C;
    /// investigation `2026-08-25-render-and-resource-bug-hunt` section 1.3,
    /// bug B): dimensions, the hyperlink-rule pass, the cursor position and
    /// the cloned viewport lines are all captured under ONE
    /// `terminal.lock()` acquisition, so the pty parser thread cannot apply
    /// output between them and a paint can no longer combine a cursor
    /// position from moment t0 with line contents from t2. The lock is
    /// still held only for the duration of the clone (one viewport's worth
    /// of lines), not across shaping/quad-building: input handling and the
    /// parser keep interleaving between frames exactly as before.
    fn get_render_snapshot(
        &self,
        viewport: Option<StableRowIndex>,
        hyperlink_rules: &[Rule],
    ) -> PaneRenderSnapshot {
        let mut snapshot = lock_terminal_timed(
            &self.terminal,
            "localpane.terminal_lock.wait.render_snapshot",
            |term| {
                let dims = terminal_get_dimensions(term);
                let top = viewport.unwrap_or(dims.physical_top);
                let lines = top..top + dims.viewport_rows as StableRowIndex;
                // Same application of the hyperlink rules that the default
                // (multi-lock) path performs via `Pane::apply_hyperlinks`,
                // done here under the same lock acquisition.
                terminal_for_each_logical_line_in_stable_range_mut(
                    term,
                    lines.clone(),
                    &mut ApplyHyperlinksInLock {
                        rules: hyperlink_rules,
                    },
                );
                let cursor = terminal_get_cursor_position(term);
                let (stable_top, lines) = terminal_get_lines(term, lines);
                PaneRenderSnapshot {
                    cursor,
                    dims,
                    stable_top,
                    lines,
                }
            },
        );
        // Same tmux special case as `get_cursor_position` above.
        if self.tmux_domain.lock().is_some() {
            snapshot.cursor.visibility = termwiz::surface::CursorVisibility::Hidden;
        }
        snapshot
    }

    fn copy_user_vars(&self) -> HashMap<String, String> {
        match try_lock_terminal_for(
            &self.terminal,
            &self.unresponsive,
            "localpane.terminal_lock.timeout.copy_user_vars",
            |term| term.user_vars().clone(),
        ) {
            Some(user_vars) => {
                self.last_known_good.lock().user_vars = user_vars.clone();
                user_vars
            }
            None => self.last_known_good.lock().user_vars.clone(),
        }
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
    /// `CopySelectionOrInterrupt` in `onlyterm-gui`). On Windows, ConPTY's
    /// hosted conhost/OpenConsole reads that byte and raises
    /// `CTRL_C_EVENT` for every process attached to the pseudoconsole (the
    /// whole tree), giving well-behaved processes a chance to shut down
    /// cleanly.
    ///
    /// That write is **not** performed on the calling thread. `kill()` is
    /// called synchronously from the GUI thread in several real paths
    /// (window close, render-thread-hang recovery, lost-context
    /// recovery). `self.writer` (a `WriterWrapper`, see
    /// `crates/mux/src/domain.rs`) is itself non-blocking these days --
    /// `write`/`flush` just enqueue onto its own background thread -- so
    /// this is no longer strictly required to avoid blocking the caller.
    /// It's kept anyway: deferring the write onto the same detached
    /// background thread described below, ahead of its sleep, means the
    /// soft-signal byte is enqueued from a thread that's guaranteed to
    /// outlive the grace period below, rather than depending on
    /// `WriterWrapper`'s own background thread (which could in principle
    /// be torn down independently) to still be around by the time the
    /// pty/writer are actually dropped.
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
                    let mut taken_writer =
                        std::mem::replace(&mut *self.writer.lock(), Box::new(std::io::sink()));

                    // Send the soft signal, and drop the pty/writer once
                    // the grace period has elapsed, all on a detached
                    // background thread -- never on the caller's thread.
                    //
                    // `kill()` is called synchronously from the GUI
                    // thread in several real paths (window close, render
                    // -hang recovery, lost-context recovery). `self.writer`
                    // (see `WriterWrapper` in `crates/mux/src/domain.rs`)
                    // is itself non-blocking now -- `write_all`/`flush`
                    // just enqueue onto `WriterWrapper`'s own background
                    // thread -- so this can't block on the real pty write
                    // either way. Doing it here regardless keeps the
                    // soft-signal enqueue on a thread whose lifetime is
                    // tied to the grace period itself, rather than
                    // depending on `WriterWrapper`'s independent
                    // background thread outliving that period.
                    //
                    // Detached, not joined -- mirrors
                    // `RenderThreadHandle::spawn` in
                    // `onlyterm-gui/src/renderthread.rs`. Moving
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
            Ok(())
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
        let _resize_guard = self.resize_guard.lock();

        // Hold `terminal.lock()` across both the pty resize and the
        // terminal model's resize, not just the latter: `pty.resize()`
        // (ConPTY on Windows) can react to the new geometry by writing a
        // repaint to the pty immediately, which the per-pane reader
        // thread (`read_from_pane_pty`) parses and applies via
        // `perform_actions`/`perform_actions_chunked` -- both of which
        // also take `terminal.lock()`. Without holding it here too, that
        // reader thread could win the race and apply ConPTY's
        // new-geometry-relative cursor moves to a `Screen` that's still
        // sized for the old geometry, landing text on the wrong row.
        let mut term = self.terminal.lock();

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
        term.resize(size);
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
        let title = match try_lock_terminal_for(
            &self.terminal,
            &self.unresponsive,
            "localpane.terminal_lock.timeout.get_title",
            |term| term.get_title().to_string(),
        ) {
            Some(title) => {
                self.last_known_good.lock().title = title.clone();
                title
            }
            None => self.last_known_good.lock().title.clone(),
        };

        // If the title is the default pane title, then try to spice
        // things up a bit by returning the process basename instead
        if title == onlyterm_term::DEFAULT_TERMINAL_TITLE {
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
        match try_lock_terminal_for(
            &self.terminal,
            &self.unresponsive,
            "localpane.terminal_lock.timeout.get_progress",
            |term| term.get_progress(),
        ) {
            Some(progress) => {
                self.last_known_good.lock().progress = progress.clone();
                progress
            }
            None => self.last_known_good.lock().progress.clone(),
        }
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
        self.unseen_output.load(Ordering::Acquire)
    }

    /// Task #269: OR of two independently-written signals -- a genuine
    /// `terminal.lock()` timeout (`unresponsive`, written only by
    /// `try_lock_terminal_for`) and the GUI's per-frame render-budget
    /// signal (`render_budget_exceeded`, written only by
    /// `set_render_budget_exceeded`). These used to share a single cell,
    /// which let the render-budget path's frequent `false` writes
    /// silently clobber a real lock-timeout `true` for whichever pane was
    /// actively being painted (i.e. exactly the pane the user is looking
    /// at). Combining them only here, at the read site, means a real
    /// lock-timeout stays visible regardless of what the render-budget
    /// path is concurrently doing.
    ///
    /// Task #273: `render_budget_exceeded` only counts if it was observed
    /// within `RENDER_BUDGET_EXCEEDED_EXPIRY` -- see that constant and the
    /// field's doc comment for why a plain sticky `bool` isn't enough
    /// (panes that stop being painted, e.g. because their tab is no
    /// longer active, never get another `set_render_budget_exceeded`
    /// call at all, so a plain `bool` could latch `true` forever).
    fn is_unresponsive(&self) -> bool {
        let render_budget_exceeded = match *self.render_budget_exceeded.lock() {
            Some(when) => when.elapsed() < RENDER_BUDGET_EXCEEDED_EXPIRY,
            None => false,
        };
        self.unresponsive.load(Ordering::Acquire) || render_budget_exceeded
    }

    /// Task #251/#269: set directly by the GUI's per-frame content-build
    /// budget when this pane's rendering couldn't be finished within
    /// `tab_frame_build_budget_ms`. This is a distinct trigger from a
    /// wedged `terminal.lock()` -- the lock may be perfectly healthy,
    /// it's the shaping/rasterization work itself that is too slow --
    /// and, since task #269, it is stored in its own
    /// `render_budget_exceeded` cell rather than sharing `unresponsive`
    /// with `try_lock_terminal_for`: this setter runs on essentially
    /// every frame for every painted pane and writes `false` far more
    /// often than `true`, which would otherwise race with and overwrite a
    /// still-active lock-timeout signal for the same pane.
    /// `is_unresponsive()` reports the OR of both cells.
    ///
    /// Task #273: records `Instant::now()` on `true` and clears back to
    /// `None` on `false`, rather than storing the `bool` directly, so that
    /// `is_unresponsive()` can let a stale `true` expire on its own (see
    /// `RENDER_BUDGET_EXCEEDED_EXPIRY`) for a pane that simply stops
    /// being painted, instead of relying on this setter -- which won't be
    /// called again at all once painting stops -- to ever run a clearing
    /// `false` call for it.
    fn set_render_budget_exceeded(&self, exceeded: bool) {
        let mut slot = self.render_budget_exceeded.lock();
        *slot = if exceeded { Some(Instant::now()) } else { None };
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
        // Ask the OS for the pane's root (real interactive shell) process's
        // real cwd first: OSC 7 is just text the shell chooses to print, so
        // a build script that cd's around internally and back (or a shell
        // integration that fires it on every prompt render) can make the
        // OSC-7-reported value flap independently of where the process
        // tree actually is. `divine_current_working_dir()` reads the OS's
        // own bookkeeping for the root process specifically (not whatever
        // deepest/youngest process happens to be running -- see that
        // function's own comment) and can't be spoofed or skipped by
        // whatever the pane is running. It also doesn't touch
        // `self.terminal`, so this is cheaper than the OSC-7 path under the
        // same contention task #246 was written for.
        if let Some(cwd) = self.divine_current_working_dir(policy) {
            self.last_known_good.lock().cwd = Some(cwd.clone());
            return Some(cwd);
        }

        // Nothing to divine from (most commonly: the foreground-process
        // cache is cold and a background fetch is already in flight, see
        // task #247) -- fall back to the shell's own OSC 7 announcement,
        // bounded the same way as the other terminal-lock accessors below.
        match try_lock_terminal_for(
            &self.terminal,
            &self.unresponsive,
            "localpane.terminal_lock.timeout.get_current_working_dir",
            |term| term.get_current_dir().cloned(),
        ) {
            Some(cwd) => {
                if cwd.is_some() {
                    self.last_known_good.lock().cwd = cwd.clone();
                }
                cwd
            }
            None => self.last_known_good.lock().cwd.clone(),
        }
    }

    fn tty_name(&self) -> Option<String> {
        None
    }

    fn get_foreground_process_info(&self, policy: CachePolicy) -> Option<LocalProcessInfo> {
        self.divine_foreground_process(policy)
    }

    fn get_foreground_process_name(&self, policy: CachePolicy) -> Option<String> {
        if let Some(fg) = self.divine_foreground_process(policy) {
            return Some(fg.executable.to_string_lossy().to_string());
        }

        None
    }

    fn get_process_tree_exe_names(&self, policy: CachePolicy) -> Option<HashSet<String>> {
        self.process_tree_exe_names(policy)
    }

    fn can_close_without_prompting(&self, _reason: CloseReason) -> bool {
        self.process_tree_allows_close()
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
            let coord = coords.get(c).copied().unwrap_or_else(|| {
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
