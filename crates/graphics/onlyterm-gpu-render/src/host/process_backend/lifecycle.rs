use super::*;
use crate::rebuild_backoff_for_attempt;
use onlyterm_client::client::windows_job::assign_to_kill_on_close_job;
use std::os::windows::io::AsRawHandle;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
use windows::Win32::Graphics::DirectComposition::DCompositionCreateSurfaceHandle;
use windows::Win32::System::Threading::GetCurrentProcess;

/// Spawns a fresh child generation, ties its lifetime to this process via a
/// Job Object (falling back to the `--supervise-pid` watcher thread inside
/// the child if that setup fails, exactly like `per_tab_process_isolation`'s
/// pty-hosting children), creates a new composition-surface handle for it,
/// and starts its writer/reader threads. Returns `false` (logged) if the
/// process itself couldn't be spawned at all.
pub(super) fn spawn_generation(shared: &Arc<Shared>) -> bool {
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
    // Phase A spike (`.scratch/dcomp-spike/parent/src/main.rs`) exactly.
    let h_surface: HANDLE = match unsafe { DCompositionCreateSurfaceHandle(GENERIC_ALL, None) } {
        Ok(h) => h,
        Err(err) => {
            log::error!("HostProcessBackend: DCompositionCreateSurfaceHandle failed: {err}");
            let _ = child.kill();
            return false;
        }
    };

    let child_process_handle = HANDLE(child.as_raw_handle());
    let mut h_surface_in_child = HANDLE::default();
    // SAFETY: `h_surface` was just created above and is valid; `child_process_handle`
    // is the freshly-spawned child's own process handle (still owned by `child`,
    // not yet closed), satisfying `DuplicateHandle`'s requirement that both
    // handles be valid for the duration of the call.
    let dup_result = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            h_surface,
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

    let generation = shared.next_generation.fetch_add(1, Ordering::AcqRel);
    let dimensions = *shared.dimensions.lock();

    let (writer_tx, writer_rx) = std::sync::mpsc::channel::<HostToChildMsg>();
    let stdin = child.stdin.take().expect("stdin was piped");
    let stdout = child.stdout.take().expect("stdout was piped");

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
        child.id()
    );
    *shared.current.lock() = Some(ChildGeneration {
        generation,
        child,
        _job: job,
        writer_tx,
    });

    // The visual keeps showing whatever the previous generation last
    // presented (or nothing yet, on the very first spawn) until this
    // generation's reader thread sees its first `Presented` ack and swaps
    // `SetContent` -- see `spawn_reader_thread`. `h_surface`/`content` for
    // *this* generation are wrapped there, not here, since the swap must
    // not happen before that ack.
    shared_generation_surface_handle_store(shared, generation, h_surface);

    // Ask for a repaint: nothing else necessarily will. A static screen
    // produces no frames on its own, so without this a respawned child sits
    // attached-but-idle until some unrelated event (a keystroke, the cursor
    // blink timer) happens to invalidate the window -- and the visual only
    // moves off the dead generation's surface once this one has acked a
    // frame.
    (shared.invalidate)();

    true
}

/// Per-generation composition-surface handles the parent side still needs
/// once the child has acked its first frame (to wrap via
/// `CreateSurfaceFromHandle` and swap into the visual). Keyed by generation
/// so a late ack from an already-superseded generation is ignored rather
/// than reaching for a handle that may have already been closed.
fn shared_generation_surface_handle_store(shared: &Arc<Shared>, generation: u64, handle: HANDLE) {
    shared
        .pending_surface_handles
        .lock()
        .insert(generation, handle);
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
    let respawn = move || {
        let spawned = spawn_generation(&shared);
        shared.respawn_pending.store(false, Ordering::SeqCst);
        if !spawned {
            log::error!("HostProcessBackend: respawn attempt failed to even launch a child");
        }
    };
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
