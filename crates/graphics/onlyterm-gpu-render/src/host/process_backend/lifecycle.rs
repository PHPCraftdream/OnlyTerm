use super::*;
use crate::rebuild_backoff_for_attempt;
use onlyterm_client::client::windows_job::assign_to_kill_on_close_job;
use std::convert::TryFrom;
use std::os::windows::io::AsRawHandle;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, WAIT_OBJECT_0};
use windows::Win32::Graphics::DirectComposition::DCompositionCreateSurfaceHandle;
use windows::Win32::System::Threading::{GetCurrentProcess, WaitForSingleObject};

/// How long a child told to shut down gets to exit by itself before the
/// process is ended.
pub(super) const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

/// Spawns a fresh child generation, ties its lifetime to this process via a
/// Job Object (falling back to the `--supervise-pid` watcher thread inside
/// the child if that setup fails, exactly like `per_tab_process_isolation`'s
/// pty-hosting children), creates a new composition-surface handle for it,
/// and starts its writer/reader threads. Returns `false` (logged) if the
/// process itself couldn't be spawned at all, or the window was closed in the
/// meantime (no child is left running then).
pub(super) fn spawn_generation(shared: &Arc<Shared>) -> bool {
    if shared.window_destroyed.load(Ordering::Acquire) {
        return false;
    }
    let mut command = Command::new(&shared.child_exe);
    command
        .arg("gpu-tab-host")
        .arg("--supervise-pid")
        .arg(std::process::id().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            log::error!("HostProcessBackend: failed to spawn gpu-tab-host child: {err}");
            return false;
        }
    };

    let job = assign_to_kill_on_close_job(&child, "onlyterm-gui.exe (gpu-tab-host)");

    // SAFETY: `GENERIC_ALL`/`None` security attributes match the validated
    // Phase A spike exactly.
    let h_surface: HANDLE = match unsafe { DCompositionCreateSurfaceHandle(GENERIC_ALL, None) } {
        Ok(h) => h,
        Err(err) => {
            log::error!("HostProcessBackend: DCompositionCreateSurfaceHandle failed: {err}");
            let _ = child.kill();
            return false;
        }
    };
    // SAFETY: just created above and used nowhere else; from here the wrapper
    // closes it on every path that does not hand it on.
    let surface = unsafe { surface_handles::SurfaceHandle::from_raw(h_surface) };

    let child_process_handle = HANDLE(child.as_raw_handle());
    let mut h_surface_in_child = HANDLE::default();
    // SAFETY: `surface` was just created above and is valid; `child_process_handle`
    // is the freshly-spawned child's own process handle (still owned by `child`,
    // not yet closed), satisfying `DuplicateHandle`'s requirement that both
    // handles be valid for the duration of the call.
    let dup_result = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            surface.raw(),
            child_process_handle,
            &mut h_surface_in_child,
            0,
            false,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if let Err(err) = dup_result {
        log::error!("HostProcessBackend: DuplicateHandle into child failed: {err}");
        let _ = child.kill();
        return false;
    }

    let child_pid = child.id();
    let generation = shared.next_generation.fetch_add(1, Ordering::AcqRel);
    *shared.renderer_info.lock() = None;
    let dimensions = *shared.dimensions.lock();

    let (writer_tx, writer_rx) = std::sync::mpsc::channel::<HostToChildMsg>();
    let stdin = child.stdin.take().expect("stdin was piped");
    let stdout = child.stdout.take().expect("stdout was piped");

    {
        // The window can be closed while this child was starting. `shutdown`
        // sets the flag before it takes `current`, so checking under the same
        // lock means either shutdown sees this generation or this sees the
        // flag: a child is never installed behind a closed window.
        let mut current = shared.current.lock();
        if shared.window_destroyed.load(Ordering::Acquire) {
            drop(current);
            log::info!(
                "HostProcessBackend: window closed while a child was starting; discarding it"
            );
            let _ = child.kill();
            return false;
        }
        *current = Some(ChildGeneration {
            generation,
            child,
            _job: job,
            writer_tx: writer_tx.clone(),
        });
    }
    io::spawn_writer_thread(generation, stdin, writer_rx, Arc::clone(&shared.draw_pool));
    io::spawn_reader_thread_with_death_handling(Arc::clone(shared), generation, stdout);

    writer_tx
        .send(HostToChildMsg::AttachSurface {
            surface_handle: h_surface_in_child.0 as i64,
            width: dimensions.pixel_width as u32,
            height: dimensions.pixel_height as u32,
        })
        .ok();
    shared.needs_full_resync.store(true, Ordering::Release);
    // The frame the previous generation was still working on when it died
    // will never be acked, so its `in_flight` claim has to be released here
    // or `call_draw_webgpu`'s back-pressure check refuses to build another
    // one -- forever, since only an ack clears it. That would leave this
    // fresh generation with no frame to present, no first ack, and therefore
    // no `swap_visual_content`: a window frozen on the dead child's last
    // frame for good, which is exactly the outcome respawning exists to
    // avoid.
    // SeqCst: one of the ops of the shared in_flight/repaint_pending handshake -- see backpressure.rs.
    shared.in_flight.store(false, Ordering::SeqCst);
    *shared.submit_started_at.lock() = None;

    log::info!(
        "HostProcessBackend: generation {generation} running as PID {} \
         (see that PID's own onlyterm-gui.exe-log-<pid>.txt for its diagnostics)",
        child_pid
    );
    // The visual keeps showing whatever the previous generation last
    // presented (or nothing yet, on the very first spawn) until this
    // generation's reader thread sees its first `Presented` ack and swaps
    // `SetContent` -- see `spawn_reader_thread`. The handle for *this*
    // generation is wrapped there, not here, since the swap must not happen
    // before that ack.
    shared
        .surface_handles
        .lock()
        .add_pending(generation, surface);

    // Ask for a repaint: nothing else necessarily will. A static screen
    // produces no frames on its own, so without this a respawned child sits
    // attached-but-idle until some unrelated event (a keystroke, the cursor
    // blink timer) happens to invalidate the window -- and the visual only
    // moves off the dead generation's surface once this one has acked a
    // frame.
    (shared.invalidate)();

    true
}

/// Gives a child that was just told to shut down time to exit by itself, then
/// ends it. Returns whether it exited on its own. Blocks for at most `grace`.
pub(super) fn wait_or_kill(child: &mut Child, grace: Duration) -> bool {
    let millis = u32::try_from(grace.as_millis()).unwrap_or(u32::MAX - 1);
    // SAFETY: the handle belongs to `child`, which outlives this call.
    let waited = unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle()), millis) };
    let exited = waited == WAIT_OBJECT_0;
    if !exited {
        let _ = child.kill();
    }
    let _ = child.wait();
    exited
}

/// Ends a child generation that is no longer wanted without blocking the
/// caller: the child gets `SHUTDOWN_GRACE` to act on its `Shutdown` message,
/// and its Job Object (which kills it) is only released afterwards. Dropping
/// the generation right away would kill the child before it could read the
/// message.
pub(super) fn retire_in_background(generation: ChildGeneration) {
    let ChildGeneration {
        generation: number,
        mut child,
        _job,
        writer_tx,
    } = generation;
    // Messages already queued (the `Shutdown` just sent) are still delivered.
    drop(writer_tx);
    let spawned = std::thread::Builder::new()
        .name("gpu-host-retire".to_string())
        .spawn(move || {
            if !wait_or_kill(&mut child, SHUTDOWN_GRACE) {
                log::warn!(
                    "HostProcessBackend: generation {} did not exit within {:?} of Shutdown; ended",
                    number,
                    SHUTDOWN_GRACE
                );
            }
            drop(_job);
        });
    if let Err(err) = spawned {
        // The closure (and the generation inside it) was dropped, which ended
        // the child at once -- the same as before this grace existed.
        log::warn!(
            "HostProcessBackend: cannot start the retire thread: {}",
            err
        );
    }
}

/// Called when a generation's reader thread observes its child is gone
/// (EOF or a read error). Respawns within budget, or demotes permanently.
pub(super) fn handle_child_death(shared: &Arc<Shared>, dead_generation: u64) {
    {
        let current = shared.current.lock();
        match current.as_ref() {
            Some(current) if current.generation == dead_generation => {}
            _ => return, // already superseded or torn down; nothing to do
        }
    }

    if shared.window_destroyed.load(Ordering::Acquire) {
        return; // window is going away; no point respawning
    }

    let attempt = {
        let mut attempts = shared.respawn_attempts.lock();
        let now = Instant::now();
        attempts.retain(|t| now.duration_since(*t) < RESPAWN_WINDOW);
        attempts.push(now);
        attempts.len()
    };

    if attempt > MAX_RESPAWNS_PER_WINDOW {
        log::error!(
            "HostProcessBackend: {} respawns within {:?}, giving up -- demoting this window to \
             the in-process renderer",
            attempt,
            RESPAWN_WINDOW,
        );
        metrics::counter!("gui.host_process.demoted_to_in_process").increment(1);
        shared.demoted.store(true, Ordering::Release);
        shared.in_flight.store(false, Ordering::SeqCst);
        (shared.invalidate)();
        return;
    }

    // From here on a respawn is scheduled (immediately or after backoff):
    // gate `render_thread_is_hung` off for its duration so the window's
    // supervisor doesn't rebuild the whole renderer on top of it. The flag
    // is cleared by the respawn closure below whether the spawn succeeds
    // (the new generation resets the clock) or fails (the hang check then
    // becomes the recovery path).
    shared.respawn_pending.store(true, Ordering::SeqCst);
    let delay = rebuild_backoff_for_attempt(attempt);
    let shared = Arc::clone(shared);
    let respawn = move || run_scheduled_respawn(&shared);
    match delay {
        None => respawn(),
        Some(delay) => {
            std::thread::Builder::new()
                .name("gpu-host-respawn-delay".to_string())
                .spawn(move || {
                    std::thread::sleep(delay);
                    respawn();
                })
                .ok();
        }
    }
}

/// The scheduled respawn itself. The window can be closed while a respawn
/// waits out its backoff, so `spawn_generation` re-checks that.
pub(super) fn run_scheduled_respawn(shared: &Arc<Shared>) {
    let spawned = spawn_generation(shared);
    shared.respawn_pending.store(false, Ordering::SeqCst);
    if !spawned && !shared.window_destroyed.load(Ordering::Acquire) {
        log::error!("HostProcessBackend: respawn attempt failed to even launch a child");
    }
}
