use super::*;

impl TermWindow {
    /// Schedules the next tick of this window's render-thread hang
    /// supervisor. Self-rearming: each tick either closes the window (if its
    /// render thread is hung) or calls this again to schedule the next tick,
    /// exactly like `scheduled_animation`'s `Timer::at` + `notify` pattern in
    /// `paint_impl` reschedules itself.
    ///
    /// Only ever called (initially from `new_window`, then from
    /// `finish_renderer_rebuild` after a successful rebuild, and recursively
    /// from `check_render_thread_hang_tick`) while running on the GUI thread
    /// -- `onlyterm_promise::spawn::spawn` is GUI-thread-only (it uses `spawn_local`
    /// under the hood), which holds here since all call sites already run
    /// on the GUI thread.
    ///
    /// Guarded by `hang_check_scheduled` (task #287): if a chain is already
    /// pending for this window, this is a no-op rather than arming a second,
    /// concurrent chain. See that field's doc comment for the race this
    /// closes. The guard is set here, at the point a new timer is actually
    /// armed, and cleared at the very top of `check_render_thread_hang_tick`
    /// -- i.e. it tracks "is a tick currently in flight for this window",
    /// not "has a chain ever been started".
    pub(super) fn schedule_render_thread_hang_check(&self, window: &Window) {
        if self.hang_check_scheduled.get() {
            // A chain is already pending (its timer tick hasn't fired yet);
            // do not start a second, concurrent chain. See this call's
            // doc comment and `hang_check_scheduled`'s own doc comment.
            return;
        }
        self.hang_check_scheduled.set(true);

        // Poll at a fraction of the hang threshold, the same style as
        // `window::os::windows::watchdog`'s `poll_interval = (threshold /
        // 4).max(Duration::from_millis(50))`. This check is cheaper than the
        // GUI watchdog's (just a `Mutex<Option<Instant>>` read, no syscalls),
        // so a smaller minimum is fine, but we still don't want a
        // misconfigured (very low) threshold to turn into a busy-poll.
        let threshold =
            Duration::from_millis(onlyterm_config::configuration().render_thread_hang_threshold_ms);
        let poll_interval = (threshold / 2).max(Duration::from_millis(500));
        let next_check = Instant::now() + poll_interval;

        let window = window.clone();
        onlyterm_promise::spawn::spawn(async move {
            Timer::at(next_check).await;
            let win = window.clone();
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.check_render_thread_hang_tick(&win);
            })));
        })
        .detach();
    }

    /// Circuit breaker thresholds for the in-place renderer rebuild
    /// performed by `check_render_thread_hang_tick`. If rebuilding the
    /// renderer doesn't actually fix things -- the GPU/driver/adapter is
    /// fundamentally broken rather than having suffered a one-off transient
    /// stall -- the render thread will simply hang again almost
    /// immediately after each rebuild. `3` rebuilds within `30` seconds is
    /// enough slack for a couple of unlucky-but-unrelated stalls (e.g. two
    /// independent brief driver hiccups minutes apart would never trip
    /// this), while still catching an immediate re-hang loop quickly: three
    /// full rebuild-and-rehang cycles within half a minute is well outside
    /// what a real transient stall looks like.
    const MAX_REBUILDS_PER_WINDOW: usize = 3;
    const REBUILD_WINDOW: Duration = Duration::from_secs(30);

    /// How often a focused window refreshes the CPU/memory figures in its
    /// title. Also the delay before a window that just regained focus
    /// produces its first figure, which is why it stays short rather than
    /// being stretched to cover idle windows -- those skip sampling
    /// entirely instead (see `process_usage_tick`).
    const PROCESS_USAGE_INTERVAL: Duration = Duration::from_secs(5);

    /// One tick of the render-thread hang supervisor: if this window's
    /// render thread appears hung, rebuild the renderer in place (new
    /// WebGpu device/surface, new render thread) so the window and all its
    /// tabs/panes survive -- unless the circuit breaker has tripped, in
    /// which case fall back to the old destructive close. Otherwise re-arms
    /// for another tick. See `schedule_render_thread_hang_check` for the
    /// scheduling half.
    fn check_render_thread_hang_tick(&mut self, window: &Window) {
        // Clear the "a chain is pending" guard before any other logic in
        // this tick runs (task #287): this tick *is* that pending chain
        // firing, so from this point on `schedule_render_thread_hang_check`
        // must be willing to arm a fresh timer again -- whether that
        // happens below (the `!hung` re-arm path) or later, from
        // `finish_renderer_rebuild` once an in-place rebuild triggered by
        // this same tick completes. Clearing it late (or conditionally)
        // would reopen the race this flag exists to close: a rebuild
        // finishing and calling `schedule_render_thread_hang_check` while
        // this flag was still `true` would be wrongly suppressed, leaving
        // this window with no supervisor at all.
        self.hang_check_scheduled.set(false);

        // Sweep any WebGpu child HWNDs retired by an earlier
        // `begin_renderer_rebuild` (task #283): destroys the ones whose
        // paired `Weak<WebGpuState>` has since hit zero strong references
        // (i.e. the old render thread has actually returned), leaving any
        // others -- still possibly referenced by a render thread that
        // hasn't unwedged yet -- in place for the next tick. Runs
        // unconditionally, before the early-return guards below, so this
        // ~2s cadence is what actually reclaims a retired HWND promptly
        // instead of leaving it until the window closes; see
        // `Window::sweep_retired_webgpu_children`'s doc comment for the
        // full rationale, including why leaving one unswept is never a
        // leak (the top-level window's own `WS_CHILD` teardown is a
        // backstop).
        #[cfg(windows)]
        window.sweep_retired_webgpu_children();

        if self.render_thread_hang_handled.get() {
            // Already rebuilding/closing this window for a hang detected on
            // an earlier tick; a tick that fires after that (a race between
            // the scheduled timer and the rebuild/close actually completing)
            // must be a no-op, not a double-rebuild or double-close.
            return;
        }
        // Two distinct failures, both fatal to this window's rendering and
        // both recovered the same way. Hung: the thread is alive but stuck
        // inside one GPU call. Died: it unwound out of `render_thread_loop`
        // altogether -- a panic in a wgpu call is the observed cause -- and
        // `std::thread` swallowed the panic, so the process lives on with
        // nobody left to turn `Frame` messages into pixels. The second one
        // reads as "not hung" by design (see `render_thread_has_died`), so
        // supervising only `render_thread_is_hung` let a dead render thread
        // freeze a window silently and indefinitely: the message loop keeps
        // pumping, Windows keeps reporting the window as responding, and
        // nothing ever repaints.
        let (hung, died) = match self.render_thread.as_ref() {
            Some(rt) => (rt.render_thread_is_hung(), rt.render_thread_has_died()),
            None => {
                // Render thread is gone (e.g. window already tearing down);
                // nothing left to supervise.
                return;
            }
        };
        if !hung && !died {
            self.schedule_render_thread_hang_check(window);
            return;
        }

        let reason = if died {
            "this window's render thread has exited unexpectedly (a panic inside a GPU \
             call unwinds it, and nothing is left to paint this window)"
        } else {
            "this window's render thread appears stuck inside a GPU submit/reconfigure \
             call (not the whole app -- just this window's GPU driver call)"
        };
        self.attempt_renderer_rebuild_or_close(
            window,
            reason,
            "this window's render thread has hung and been rebuilt",
            "gui.render_thread.window_renderer_rebuilt",
        );
    }

    /// Arms (or, if a chain is already pending, no-ops) the self-rearming
    /// timer chain that samples this process tree's CPU/memory usage every
    /// 5 seconds for the OS window title suffix. Same guard pattern as
    /// `schedule_render_thread_hang_check`: `process_usage_scheduled` is set
    /// here and cleared at the top of `process_usage_tick`, so at most one
    /// chain is ever pending per window.
    ///
    /// Unlike the hang check, this always re-arms regardless of
    /// `show_process_tree_stats_in_title` -- see `process_usage_tick` for
    /// why: it lets the config be toggled live without restarting the
    /// window, at the cost of one cheap `bool` check every tick while off.
    ///
    /// The timer keeps ticking at a fixed interval whether or not the
    /// window is focused, but an unfocused tick does no sampling at all
    /// (see `process_usage_tick`) -- so re-arming here stays trivial and a
    /// window that regains focus produces a figure within one interval.
    pub(super) fn schedule_process_usage_tick(&self, window: &Window) {
        if self.process_usage_scheduled.get() {
            return;
        }
        self.process_usage_scheduled.set(true);

        let next = Instant::now() + Self::PROCESS_USAGE_INTERVAL;
        let window = window.clone();
        onlyterm_promise::spawn::spawn(async move {
            Timer::at(next).await;
            let win = window.clone();
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.process_usage_tick(&win);
            })));
        })
        .detach();
    }

    /// One tick of the process-usage sampler. Reads this process tree's
    /// current CPU time + memory via `procinfo`, derives a CPU% from the
    /// delta against the previous tick's sample (a single sample has no
    /// rate, hence no title update on the very first tick after a window is
    /// created), and stores the formatted suffix for `update_title_impl` to
    /// append -- then requests a title rebuild so it actually shows up.
    /// Always re-arms itself (see `schedule_process_usage_tick`).
    fn process_usage_tick(&mut self, window: &Window) {
        self.process_usage_scheduled.set(false);

        // An unfocused window samples nothing. Each sample walks *every*
        // process on the machine, and every GUI window is a separate
        // process with its own snapshot cache, so this walk rate scales
        // with the number of open windows -- a dozen idle background
        // windows enumerating the whole machine on a timer is pure waste,
        // and it is exactly the work that aborted two windows on
        // 2026-09-14 when an unrelated process storm exhausted system
        // memory mid-walk. The last computed suffix stays in the title
        // (no flicker); sampling resumes within one interval of the window
        // being focused again.
        if self.focused.is_none() {
            // Drop the baseline rather than carry it across the idle gap:
            // the next sample would otherwise report a CPU average
            // stretched over the whole time the window sat unfocused.
            self.last_process_usage_sample.borrow_mut().take();
            self.schedule_process_usage_tick(window);
            return;
        }

        if !self.config.show_process_tree_stats_in_title {
            // Drop any stale suffix from before the config was toggled off,
            // and drop the baseline sample so re-enabling starts a fresh
            // delta rather than reporting usage accrued while disabled.
            if self.process_usage_suffix.borrow_mut().take().is_some() {
                self.last_process_usage_sample.borrow_mut().take();
                self.update_title_coalesced();
            }
            self.schedule_process_usage_tick(window);
            return;
        }

        match onlyterm_procinfo::LocalProcessInfo::process_tree_resource_usage(std::process::id()) {
            Ok(usage) => {
                let now = crate::termwindow::process_stats::UsageSample {
                    at: Instant::now(),
                    total_cpu_time_100ns: usage.total_cpu_time_100ns,
                };
                let prev = self.last_process_usage_sample.replace(Some(now));
                if let Some(prev) = prev {
                    let logical_cpus = std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(1);
                    let suffix = crate::termwindow::process_stats::format_usage_suffix(
                        &prev,
                        &now,
                        usage.total_working_set_bytes,
                        onlyterm_procinfo::total_physical_memory_bytes(),
                        logical_cpus,
                    );
                    self.process_usage_suffix
                        .replace(Some(process_stats::UsageText::new(suffix)));
                    self.update_title_coalesced();
                }
            }
            Err(err) => {
                log::warn!("process_tree_resource_usage failed: {err:#}");
            }
        }

        self.schedule_process_usage_tick(window);
    }

    /// Re-entry point (via `TermWindowNotif::Apply`) for render-error
    /// recovery signals raised from the render thread that aren't a plain
    /// hang: a `wgpu::SurfaceError` variant other than `Lost`/`Outdated`
    /// (see `renderthread::submit_one_frame`'s `other` branch), or a genuine
    /// wgpu device-lost event (see `WebGpuState::new`'s
    /// `set_device_lost_callback` registration). Both signals mean this
    /// window's GPU device/surface is in a broken state that a fresh
    /// `submit_frame` call won't recover from on its own, so this reuses
    /// exactly the same in-place rebuild (and circuit breaker) that
    /// `check_render_thread_hang_tick` uses for a stuck render thread --
    /// from the GUI thread's point of view, "the renderer needs rebuilding"
    /// is the same recovery action regardless of which symptom (hang vs.
    /// repeated surface error vs. device-lost) triggered it.
    ///
    /// Unlike `check_render_thread_hang_tick`, this does not check
    /// `render_thread_is_hung()` (the render thread may not be hung at all
    /// -- `submit_frame` returned promptly with an error, or the device-lost
    /// callback fired inline during a call that itself returned) and does
    /// not self-reschedule (it's not a polling loop; each call corresponds
    /// to one observed error event). It still honors
    /// `render_thread_hang_handled` as a one-shot-per-episode guard, exactly
    /// like the hang path, so a burst of repeated `SurfaceError::Other`
    /// values across several frames (or an error arriving while a
    /// hang-triggered rebuild is already in flight) collapses into a single
    /// rebuild attempt rather than one per event.
    pub(crate) fn handle_render_error_recovery(&mut self, window: &Window, reason: &str) {
        if self.render_thread_hang_handled.get() {
            // A rebuild (or close) for an earlier episode -- hang or error --
            // is already in flight; this event is redundant.
            return;
        }
        self.attempt_renderer_rebuild_or_close(
            window,
            reason,
            "this window's renderer has failed and been rebuilt",
            "gui.render_thread.window_renderer_rebuilt_after_error",
        );
    }

    /// Shared circuit-breaker bookkeeping + rebuild-or-close decision, used
    /// by both the render-thread hang supervisor
    /// (`check_render_thread_hang_tick`) and the render-error recovery entry
    /// point (`handle_render_error_recovery`). Callers are responsible for
    /// their own "should we even consider recovering right now" checks
    /// (hang detection, one-shot guard) before calling this; this function
    /// always sets the one-shot guard, records an attempt, and either
    /// rebuilds or closes.
    ///
    /// `log_reason` describes what was observed (used in the "rebuilding..."
    /// log line); `circuit_breaker_log_reason` is the shorter phrase used in
    /// the circuit-breaker-tripped log line; `rebuilt_metric` is the counter
    /// incremented when a rebuild is actually attempted, so the two call
    /// sites (hang vs. error recovery) remain distinguishable in metrics
    /// even though they now share this implementation.
    fn attempt_renderer_rebuild_or_close(
        &mut self,
        window: &Window,
        log_reason: &str,
        circuit_breaker_log_reason: &str,
        rebuilt_metric: &'static str,
    ) {
        // Set the one-shot guard immediately: everything below this point
        // (the circuit breaker check, the async rebuild, the fallback close)
        // must not race with another recovery trigger for this same
        // episode. It gets reset to `false` once a rebuild actually
        // succeeds (see `finish_renderer_rebuild`), so a *later*, separate
        // failure can also be recovered from -- this is "one-shot per
        // episode", not "one-shot ever".
        self.render_thread_hang_handled.set(true);

        let now = Instant::now();
        {
            let mut attempts = self.rebuild_attempts.borrow_mut();
            attempts.retain(|t| now.duration_since(*t) < Self::REBUILD_WINDOW);
            attempts.push(now);
        }
        let attempts_in_window = self.rebuild_attempts.borrow().len();

        if attempts_in_window > Self::MAX_REBUILDS_PER_WINDOW {
            // The GPU/driver/adapter looks fundamentally broken, not just
            // transiently stuck: rebuilding WebGpu in place has already
            // failed to produce a working renderer `MAX_REBUILDS_PER_WINDOW`
            // times within `REBUILD_WINDOW`. There is no other renderer left
            // to fall back to (task #414 removed the OpenGL/Mesa fallback
            // that used to catch this), so the only thing left to do is
            // close this window cleanly rather than let it sit there
            // silently broken or spin forever retrying.
            log::error!(
                "{} {} times in the last {:?}; giving up on rebuilding WebGpu (the \
                 GPU/driver/adapter looks fundamentally broken, not just transiently \
                 stuck) and closing this window -- OnlyTerm has no other rendering \
                 backend to fall back to",
                circuit_breaker_log_reason,
                attempts_in_window,
                Self::REBUILD_WINDOW,
            );
            metrics::counter!("gui.render_thread.rebuild_circuit_breaker_tripped").increment(1);
            self.close_window_for_unrecoverable_render_hang(window);
            return;
        }

        log::error!(
            "{}; rebuilding this window's renderer in place (attempt {} of {} allowed \
             within {:?}) so its tabs/panes survive",
            log_reason,
            attempts_in_window,
            Self::MAX_REBUILDS_PER_WINDOW,
            Self::REBUILD_WINDOW,
        );
        metrics::counter!(rebuilt_metric).increment(1);

        match rebuild_backoff_for_attempt(attempts_in_window) {
            None => self.begin_renderer_rebuild(window),
            Some(delay) => {
                // Wait before touching the driver again. The reason this
                // matters is not politeness: the first attempt usually fails
                // because the system is momentarily out of memory, and
                // creating a device/surface is exactly what a driver in that
                // state handles worst. A crash dump from this machine caught
                // the failure mode -- a NULL dereference inside
                // `igd10iumd64.dll` while DXGI was building the D3D11 child
                // device for a flip-model swapchain -- with the two rebuild
                // attempts 107ms apart. The circuit breaker counts attempts
                // but never spaced them, so a transient shortage was met with
                // three immediate re-entries into the code that was failing
                // because of it.
                //
                // Deferred through the same timer/notify path
                // `schedule_render_thread_hang_check` uses, rather than a
                // sleep: this runs on the GUI thread, and blocking it here
                // would freeze every other window in the process.
                log::warn!(
                    "waiting {:?} before rebuild attempt {} so a transient GPU memory \
                     shortage has a chance to clear before we ask the driver again",
                    delay,
                    attempts_in_window,
                );
                let deadline = Instant::now() + delay;
                let window = window.clone();
                onlyterm_promise::spawn::spawn(async move {
                    Timer::at(deadline).await;
                    let win = window.clone();
                    window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                        tw.begin_renderer_rebuild(&win);
                    })));
                })
                .detach();
            }
        }
    }

    /// The destructive fallback: kill this window's panes (and their child
    /// processes) before destroying the OS window, otherwise the
    /// shells/programs running in them are orphaned with no controlling
    /// terminal left. This is the same sequence `close_requested` uses; it's
    /// the true last resort, reached only once the in-place WebGpu rebuild's
    /// circuit breaker trips (task #414 removed the OpenGL fallback that used
    /// to be tried before resorting to this).
    fn close_window_for_unrecoverable_render_hang(&mut self, window: &Window) {
        let mux = Mux::get();
        mux.kill_window(self.mux_window_id);
        window.close();
        front_end().forget_known_window(window);
        metrics::counter!("gui.render_thread.window_closed_for_hang").increment(1);
    }

    /// Kick off the async half of the in-place renderer rebuild (abandoning
    /// the old render thread and dropping the old GPU resources are cheap
    /// and synchronous, so they happen here; `WebGpuState::new` is `async`,
    /// so the rest is done in a spawned task, mirroring the established
    /// pattern in `schedule_render_thread_hang_check` for bridging sync
    /// code -> async GUI-thread-only work -> re-entry via
    /// `TermWindowNotif::Apply`).
    fn begin_renderer_rebuild(&mut self, window: &Window) {
        // Grab the outgoing render thread's teardown sentinel (task #292)
        // *before* taking/shutting it down below: `RenderThreadHandle::
        // teardown_sentinel` only exists on the handle itself, so this has
        // to happen while `self.render_thread` still holds it. See that
        // method's doc comment for why this -- not a `Weak` obtained by
        // downgrading `self.webgpu` -- is the correct signal for
        // `recreate_webgpu_child_window`/`sweep_retired_webgpu_children` to
        // poll: a `Weak<WebGpuState>`'s strong count can read zero while
        // `WebGpuState::drop` (and the `wgpu::Surface`/DXGI swapchain
        // teardown inside it) is still running on the render thread, since
        // `Arc::drop` decrements the strong count before running the
        // value's own `drop_in_place`. The sentinel instead only reports
        // zero strong references once the render thread has fully returned
        // from `render_thread_loop`, strictly after every `Arc<WebGpuState>`
        // clone it held has already been dropped.
        //
        // Only actually consumed on Windows (`recreate_webgpu_child_window`
        // below), since that's the only platform with a dedicated WebGpu
        // child HWND to retire in the first place; the `#[allow]` avoids an
        // unused-variable warning on other platforms where it's computed
        // but never read.
        #[allow(unused_variables)]
        let old_webgpu_weak: std::sync::Weak<dyn std::any::Any + Send + Sync> = self
            .render_thread
            .as_ref()
            .map(|rt| rt.teardown_sentinel())
            .unwrap_or_else(|| {
                std::sync::Weak::<()>::new() as std::sync::Weak<dyn std::any::Any + Send + Sync>
            });

        // Step 1: abandon the old render thread. Detach, don't join --
        // exactly like the `Destroyed` handler: a stuck GPU driver call
        // can't freeze the GUI thread, so blocking here via `.join()` would
        // defeat the whole purpose of having a separate render thread.
        // Sending `Shutdown` (which also sets `window_destroyed` on the
        // shared flag) is enough to let the thread's `recv()` loop end on
        // its own, whenever the driver call it may currently be stuck in
        // eventually returns.
        if let Some(rt) = self.render_thread.take() {
            rt.shutdown();
        }

        // Step 2: retire the old GPU resources in the same order the
        // `Destroyed` handler documents: render_state first, then the
        // device+surface (webgpu) -- but neither is dropped synchronously
        // here any more.
        //
        // We are in this function because this window's device/render
        // thread was already judged unreliable: a hang, a device-lost
        // event, or -- the case that motivated this -- a GPU driver being
        // reinstalled out from under the process. Running `RenderState`'s
        // ordinary `Drop` right now would call straight back into that same
        // suspect driver to release its buffers/textures/glyph atlas,
        // synchronously, on the GUI thread. Observed live: with the driver
        // genuinely unavailable mid-reinstall, that call never returned --
        // it froze the GUI message loop for the rest of the process's life,
        // until the user killed it by hand. Windows' own AppHang mechanism
        // only *reports* a stuck message loop; it does not recover one.
        //
        // `RenderState` holds `Rc`/`RefCell` internally (its glyph
        // cache/layers are shared with the rest of the GUI-thread-only
        // rendering code), so it is `!Send` and cannot be hosted on a
        // background thread for a clean deferred drop. The device it was
        // built against is being discarded here regardless -- a fresh one
        // is what the rest of this function goes on to build -- so once the
        // OS/driver finishes tearing down (or replacing) that device, its
        // own device-removed cleanup reclaims whatever GPU memory this
        // `RenderState` still referenced, whether or not wgpu's `Drop` ever
        // ran for it. `mem::forget` trades a resource release we can no
        // longer safely attempt for a GUI thread that can no longer hang on
        // it.
        if let Some(render_state) = self.render_state.take() {
            std::mem::forget(render_state);
        }
        // Mark the outgoing device stale (task #267) before dropping it: its
        // `set_device_lost_callback` closure keeps living (wgpu gives no way
        // to unregister it) for as long as the underlying `wgpu::Device`
        // handle does, so a *late* device-lost event from this now-abandoned
        // device must be able to tell it's stale and no-op, instead of
        // charging a spurious rebuild attempt against the freshly-rebuilt,
        // perfectly healthy device that replaces it below.
        if let Some(webgpu) = self.webgpu.take() {
            webgpu.mark_stale();
            // Unlike `RenderState` above, `Arc<WebGpuState>` holds no
            // `Rc`/`RefCell` and is `Send`, so its eventual drop can be
            // deferred to a background thread instead of risking the same
            // suspect-driver call on the GUI thread. This is usually a
            // no-op: per `old_webgpu_weak`'s comment above, the
            // just-shutdown render thread typically still holds its own
            // clone, so this is rarely the last strong reference and this
            // thread's only job is decrementing a refcount. It matters in
            // the edge case where that render thread had already exited
            // (e.g. a rebuild-after-failed-rebuild), where this would
            // otherwise be the last reference and run the real teardown.
            std::thread::Builder::new()
                .name("webgpu-drop".to_string())
                .spawn(move || drop(webgpu))
                .ok();
        }

        let window_for_async = window.clone();
        let dimensions = self.dimensions;
        let config = self.config.clone();

        onlyterm_promise::spawn::spawn(async move {
            // Step 3: retire the old WebGpu child HWND and create a fresh
            // one, *before* rebuilding `WebGpuState` below. This has to
            // happen ahead of the `WebGpuState::new` call, not after it:
            // `WebGpuState::new` picks whichever child HWND
            // `window.webgpu_child_hwnd()` currently returns, so rebuilding
            // the surface against the *old* child HWND (the one whose
            // swapchain may itself be the thing that's wedged) would defeat
            // the entire point of task #252's dedicated child HWND.
            //
            // This can't run synchronously back in `begin_renderer_rebuild`
            // (unlike steps 1-2 above): `Window::recreate_webgpu_child_window`
            // needs to borrow this window's `WindowInner`, but
            // `begin_renderer_rebuild` is always reached synchronously from
            // inside `notify()`'s dispatch, which is itself invoked from
            // `Connection::with_window_inner` while that exact `WindowInner`
            // is already mutably borrowed -- a synchronous re-borrow here
            // panics with "already mutably borrowed" (hit in this task's own
            // manual verification). `recreate_webgpu_child_window` is
            // `async` and internally defers its borrow via
            // `onlyterm_promise::spawn::spawn` for exactly this reason (see its doc
            // comment), so awaiting it here, one spawned task removed from
            // the original `notify()` call, is what actually avoids the
            // re-entrant borrow.
            #[cfg(windows)]
            if let Err(err) = window_for_async
                .recreate_webgpu_child_window(old_webgpu_weak)
                .await
            {
                let win = window_for_async.clone();
                window_for_async.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    tw.finish_renderer_rebuild(&win, Err(err));
                })));
                return;
            }

            let result = WebGpuState::new(
                &window_for_async,
                dimensions,
                &config,
                gpu_recovery_notifier(&window_for_async),
            )
            .await;
            let win = window_for_async.clone();
            window_for_async.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.finish_renderer_rebuild(&win, result);
            })));
        })
        .detach();
    }

    /// Re-entry point (via `TermWindowNotif::Apply`) once the async half of
    /// the rebuild (`WebGpuState::new`) has resolved. On success, rebuilds
    /// `RenderState` against the new device and spawns a fresh render
    /// thread, mirroring `new_window`'s original setup sequence.
    ///
    /// There are two sequential failure points here -- `WebGpuState::new`
    /// itself failing, and (task #272) `self.created` (i.e. `RenderState::new`:
    /// shader compilation, glyph atlas allocation, etc.) failing even though
    /// `WebGpuState::new` just succeeded -- and both re-enter the same
    /// circuit-breaker-gated path (`attempt_renderer_rebuild_or_close`) that
    /// got us here, rather than closing the window immediately. Rationale
    /// (task #255, extended by #272): either failure is, from the circuit
    /// breaker's point of view, just another WebGpu rebuild attempt that
    /// didn't pan out -- exactly like a rebuild that "succeeds" (returns a
    /// device) but then immediately re-hangs, which the breaker already
    /// tolerates up to `MAX_REBUILDS_PER_WINDOW` times. Treating either of
    /// these failures differently (skip straight to close, bypassing the
    /// retry budget entirely) would be an inconsistency with no real
    /// justification: all three symptoms mean "this WebGpu attempt didn't
    /// produce a working renderer", and the breaker's 3-attempts/30s budget
    /// is exactly the mechanism designed to decide when to stop retrying and
    /// give up. So a failure at either point counts as one attempt against
    /// that same budget, and re-calling `attempt_renderer_rebuild_or_close`
    /// naturally either retries WebGpu again (if attempts remain) or, once
    /// the breaker trips, closes the window (see that function; task #414
    /// removed the OpenGL fallback that used to be tried at that point).
    fn finish_renderer_rebuild(&mut self, window: &Window, result: anyhow::Result<WebGpuState>) {
        // Opportunistic extra sweep (task #283): by the time `WebGpuState::new`
        // (real adapter/device/surface setup work, not instantaneous) has
        // resolved, the old render thread this rebuild abandoned has
        // usually already observed the shutdown signal and returned,
        // dropping its `Arc<WebGpuState>`. Sweeping here means the common
        // case -- a healthy render thread that wasn't really wedged, just
        // slow -- gets its retired HWND destroyed right away instead of
        // waiting for the next ~2s `check_render_thread_hang_tick`. Not
        // load-bearing: if the old thread is still wedged, this is a
        // harmless no-op and the next tick (or eventually window close)
        // will catch it once it's safe.
        #[cfg(windows)]
        window.sweep_retired_webgpu_children();

        let webgpu = match result {
            Ok(state) => Arc::new(state),
            Err(err) => {
                // Same failure modes `WebGpuState::new` can hit at initial
                // window creation: RDP session, no GPU passthrough in a VM,
                // a driver mismatch, etc. Retry through the circuit
                // breaker (see doc comment above) instead of closing
                // immediately; `render_thread_hang_handled` is still `true`
                // from the original `attempt_renderer_rebuild_or_close`
                // call that led here, so this is safe to re-enter directly
                // without re-checking it.
                log::error!(
                    "failed to rebuild WebGpu renderer after a render-thread hang ({:#}); \
                     retrying through the rebuild circuit breaker",
                    err
                );
                metrics::counter!("gui.render_thread.rebuild_failed").increment(1);
                self.attempt_renderer_rebuild_or_close(
                    window,
                    "the previous WebGpu rebuild attempt itself failed to create a device/surface",
                    "the WebGpu rebuild attempt has failed",
                    "gui.render_thread.window_renderer_rebuilt",
                );
                return;
            }
        };

        // The WebGpu child HWND was already retired and a fresh one
        // recreated (task #283 onward: the old HWND is *retired*, not
        // destroyed here -- it's swept later once the outgoing render
        // thread's `WebGpuState` has actually dropped, see
        // `sweep_retired_webgpu_children` above) via the spawned task in
        // `begin_renderer_rebuild` that awaits
        // `recreate_webgpu_child_window`, before `WebGpuState::new` was even
        // called. So the surface/device just resolved above already targets
        // the fresh child HWND. Nothing left to do for the HWND here.
        self.webgpu.replace(Arc::clone(&webgpu));
        // Reset frame signature on renderer rebuild - new renderer/surface,
        // so the previous frame is no longer comparable (task #450)
        self.last_frame_signature = None;
        if let Err(err) = self.created(RenderContext(Arc::clone(&webgpu))) {
            // Same reasoning as the `WebGpuState::new` failure arm above
            // (task #272): the device/surface rebuild itself just
            // succeeded, so this is a `RenderState::new` failure (shader
            // compilation, glyph atlas allocation, etc.) on top of a
            // healthy device -- still just another WebGpu rebuild attempt
            // that didn't pan out, from the circuit breaker's point of
            // view. Retry through it instead of closing immediately, so a
            // transient failure right after a device rebuild gets the same
            // retry/OpenGL-fallback chance as any other rebuild hiccup.
            // `self.created` already reset `self.render_state` to `None` on
            // this failure, and the next `begin_renderer_rebuild` (if the
            // breaker allows another attempt) will mark this `self.webgpu`
            // stale and clear it before creating a fresh one, so no stale
            // partial state is left around for a subsequent attempt to trip
            // over.
            log::error!(
                "failed to rebuild RenderState after a successful WebGpu device/surface \
                 rebuild ({:#}); retrying through the rebuild circuit breaker",
                err
            );
            metrics::counter!("gui.render_thread.rebuild_failed").increment(1);
            self.attempt_renderer_rebuild_or_close(
                window,
                "RenderState build failed after a successful device/surface rebuild",
                "the WebGpu rebuild attempt has failed",
                "gui.render_thread.window_renderer_rebuilt",
            );
            return;
        }

        let config = onlyterm_config::configuration();
        if config.webgpu_render_thread {
            let (tx, rx) = std::sync::mpsc::channel();
            let in_flight = Arc::new(AtomicBool::new(false));
            let repaint_pending = Arc::new(AtomicBool::new(false));
            let window_destroyed = Arc::new(AtomicBool::new(false));
            let submit_started_at = Arc::new(parking_lot::Mutex::new(None));
            let seed = crate::renderthread::RenderThreadSeed {
                window: window.clone(),
                webgpu: Arc::clone(&webgpu),
                rx,
                in_flight,
                repaint_pending,
                window_destroyed,
                submit_started_at,
                on_renderer_error: Box::new(|win, reason| {
                    let recovery_window = win.clone();
                    win.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(
                        move |tw| {
                            tw.handle_render_error_recovery(&recovery_window, &reason);
                        },
                    )));
                }),
            };
            self.render_thread =
                crate::renderthread::RenderThreadHandle::spawn(seed, tx, self.mux_window_id)
                    .map(|handle| Box::new(handle) as Box<dyn onlyterm_gpu_render::RenderBackend>);
            if self.render_thread.is_some() {
                self.schedule_render_thread_hang_check(window);
            }
        }

        // The rebuild succeeded and a fresh render thread (if configured)
        // is running: re-arm the one-shot guard so a later, separate hang
        // on this same window can also be recovered from.
        self.render_thread_hang_handled.set(false);

        // The old frame's content is gone (new device, new/blank surface);
        // force a full repaint rather than waiting for the next organic
        // invalidate.
        window.invalidate();

        log::info!(
            "successfully rebuilt this window's WebGpu renderer in place after a \
             render-thread hang; window and all its tabs/panes survived"
        );
    }
}
