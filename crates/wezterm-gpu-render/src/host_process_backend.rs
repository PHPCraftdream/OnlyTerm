//! `HostProcessBackend` (task #651): the parent-side `RenderBackend` for
//! `webgpu_engine: HostProcess` (see
//! docs/plans/2026-08-21-per-tab-gpu-process-isolation.md, Phase B, and the
//! `@ox` architecture review this session that settled per-window
//! granularity, silent respawn-in-place, and no crash-visible epitaph
//! screen).
//!
//! Owns, for one window: a DirectComposition device/target/visual (the
//! window's real swapchain-equivalent, but this process never touches D3D
//! for it -- `Surface::configure`/`Surface::present`, the exact calls behind
//! every crash diagnosed this session, never run here), the current
//! `--gpu-tab-host` child process and its composition-surface generation,
//! and a writer/reader thread pair that talk to it (mirroring
//! `RenderThreadHandle`'s in-flight/repaint_pending/submit_started_at
//! handshake, just carried over a pipe instead of an in-memory channel).
//!
//! Respawn-in-place is silent: a composition surface keeps displaying its
//! last presented frame even after its producer process dies (confirmed
//! empirically this session via the Phase A spike -- see
//! `.scratch/dcomp-spike`), so nothing visibly drops while a replacement
//! child spawns, attaches to a *new* surface generation, and only takes over
//! the visual once it has acked its first presented frame. Respawns are
//! rate-limited with `rebuild_backoff_for_attempt`'s exact schedule
//! (reused, not reimplemented) and a 3-per-30s sliding-window budget; once
//! exhausted, `render_thread_has_died()` starts reporting `true`, which lets
//! the window's *existing* hang supervisor (`check_render_thread_hang_tick`)
//! rebuild via a plain in-process `RenderThreadHandle` instead -- demotion
//! is a side effect of the existing rebuild path, not new machinery.

use crate::rebuild_backoff_for_attempt;
use crate::wire;
use crate::{FrameForm, RenderBackend, SubmittableFrame};
use parking_lot::Mutex;
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use wezterm_client::client::windows_job::assign_to_kill_on_close_job;
use window::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use window::{Dimensions, WindowOps};
use windows::core::IUnknown;
use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice3, DCompositionCreateSurfaceHandle, IDCompositionDesktopDevice,
    IDCompositionTarget, IDCompositionVisual2,
};
use windows::Win32::System::Threading::GetCurrentProcess;

const GENERIC_ALL: u32 = 0x1000_0000;
const MAX_RESPAWNS_PER_WINDOW: usize = 3;
const RESPAWN_WINDOW: Duration = Duration::from_secs(30);

/// Messages the writer thread relays to the child's stdin.
enum HostToChildMsg {
    AttachSurface {
        surface_handle: i64,
        width: u32,
        height: u32,
    },
    Frame(wire::WireFrame),
    Resize {
        width: u32,
        height: u32,
    },
    Shutdown,
}

/// State that changes across a respawn: the live child, its supervision
/// handles, and the channel to its writer thread. Replaced wholesale on
/// every (re)spawn; `generation` disambiguates a dying old child's
/// background threads from a freshly spawned replacement's.
struct ChildGeneration {
    generation: u64,
    // Kept alive for its `id()` (read by `debug_current_child_pid`, used
    // only by tests -- production code never needs this generation's PID
    // again once its writer/reader threads and Job Object are set up) and
    // so its `stdin`/`stdout` (already `.take()`n into the writer/reader
    // threads) and process handle stay valid for as long as this
    // generation is current.
    #[allow(dead_code)]
    child: Child,
    _job: Option<filedescriptor::OwnedHandle>,
    writer_tx: Sender<HostToChildMsg>,
}

struct Shared {
    /// Requests a fresh repaint. In production this is `Window::invalidate`;
    /// tests supply a plain closure instead, so this backend's respawn/
    /// backoff/demotion machinery can be exercised end-to-end (real child
    /// process, really killed) without needing this crate's full
    /// `Connection`/event-loop-registered `::window::Window` machinery --
    /// see `host_process_backend::tests`.
    invalidate: Box<dyn Fn() + Send + Sync>,
    /// Tears down the Windows GDI "Loading..." placeholder (task #407/#330,
    /// `Window::clear_placeholder_background`) the first time real content
    /// actually lands on screen. In production this is exactly that method;
    /// idempotent, so safe to call once per generation's first ack rather
    /// than tracking "have we ever cleared it" separately. Without this, a
    /// `HostProcessBackend`-backed window never clears the placeholder at
    /// all: that normally happens inside `renderthread.rs`'s in-process
    /// `submit_one_frame`, which this backend never runs.
    clear_placeholder_background: Box<dyn Fn() + Send + Sync>,
    dcomp_device: IDCompositionDesktopDevice,
    _dcomp_target: IDCompositionTarget,
    dcomp_visual: IDCompositionVisual2,
    dimensions: Mutex<Dimensions>,
    current: Mutex<Option<ChildGeneration>>,
    next_generation: AtomicU64,
    /// Set on every (re)spawn, cleared once that generation's first frame
    /// has been sent with `atlas_reset` forced -- see `FrameForm::Wire`'s
    /// `full_resync` field.
    needs_full_resync: AtomicBool,
    in_flight: Arc<AtomicBool>,
    repaint_pending: Arc<AtomicBool>,
    window_destroyed: Arc<AtomicBool>,
    submit_started_at: Arc<Mutex<Option<Instant>>>,
    respawn_attempts: Mutex<Vec<Instant>>,
    /// Once true, this backend has given up retrying and is reporting
    /// `render_thread_has_died() == true` permanently, so the window's own
    /// supervisor rebuilds via a plain in-process `RenderThreadHandle`.
    demoted: AtomicBool,
    teardown_sentinel_strong: Arc<()>,
    /// Composition-surface handles awaiting their generation's first
    /// `Presented` ack before being wrapped and swapped into the visual
    /// (`on_presented`/`swap_visual_content`). Keyed by generation so a late
    /// ack from an already-superseded generation can't reach for a handle
    /// that may already have been closed by a newer one taking over.
    pending_surface_handles: Mutex<std::collections::HashMap<u64, HANDLE>>,
    /// Path to the `onlyterm-gui.exe` binary to spawn as `gpu-tab-host`.
    /// `std::env::current_exe()` in production; a test overrides it because
    /// under `cargo test` that would resolve to the test harness binary
    /// instead of the real GUI executable (see `spawn_for_hwnd_with_exe`).
    child_exe: std::path::PathBuf,
}

// SAFETY: `HANDLE` is an opaque kernel object identifier (an index into a
// process-wide table), not a pointer to process-local memory -- valid to
// use, store, and pass between threads within the owning process regardless
// of which thread obtained it. `windows-rs` spells it as a raw pointer only
// because Win32's ABI does, which is why the compiler can't infer `Send`/
// `Sync` for it on its own.
unsafe impl Send for Shared {}
// SAFETY: see the `Send` impl's comment above -- the same reasoning (opaque
// kernel handles, COM interfaces documented as usable across threads with
// external synchronization, which every mutable field here already gets via
// `Mutex`/atomics) applies equally to `Sync`.
unsafe impl Sync for Shared {}

pub struct HostProcessBackend {
    shared: Arc<Shared>,
}

impl HostProcessBackend {
    /// Creates the DirectComposition visual tree for `window` and spawns the
    /// first `--gpu-tab-host` child generation. Returns `None` (logged) on
    /// any failure building the visual tree itself -- the caller falls back
    /// to `RenderThreadHandle` exactly as it would for any other renderer
    /// construction failure.
    pub fn spawn(window: &::window::Window, dimensions: Dimensions) -> Option<Self> {
        // Target the dedicated WebGpu child HWND, exactly like the
        // in-process path (`WebGpuState::new`) does -- it's a `WS_CHILD`
        // window already kept sized/positioned to exactly cover the
        // top-level window's client area (see `Window::webgpu_child_hwnd`'s
        // doc comment), stacked in front of it. Attaching this backend's
        // DirectComposition target to the top-level HWND *instead* would
        // put its content one z-order layer *behind* that (empty, opaque)
        // child window -- confirmed live: the window opened but stayed
        // blank until this fix, because that child HWND, not the top-level
        // one, is what's actually visible in the client area.
        let hwnd = match window.webgpu_child_hwnd() {
            Some(child_hwnd) => child_hwnd,
            None => match window.window_handle().ok()?.as_raw() {
                RawWindowHandle::Win32(h) => h.hwnd.get(),
                _ => {
                    log::error!(
                        "HostProcessBackend: window has no Win32 HWND, cannot attach DirectComposition"
                    );
                    return None;
                }
            },
        };
        let window = window.clone();
        let window_for_placeholder = window.clone();
        let child_exe = std::env::current_exe().ok()?;
        Self::spawn_for_hwnd_with_exe(
            hwnd,
            dimensions,
            Box::new(move || window.invalidate()),
            Box::new(move || window_for_placeholder.clear_placeholder_background()),
            child_exe,
        )
    }

    /// Core constructor, decoupled from `::window::Window` so it can be
    /// exercised in a test against a bare Win32 HWND (this crate's `Window`
    /// type can only be built through the full `Connection`/event-loop
    /// machinery, which a unit test has no reasonable way to stand up), and
    /// from `std::env::current_exe()` (under `cargo test` that resolves to
    /// the test harness binary, not the real `onlyterm-gui.exe`, which has
    /// no `gpu-tab-host` subcommand) -- see `host_process_backend::tests`.
    fn spawn_for_hwnd_with_exe(
        hwnd: isize,
        dimensions: Dimensions,
        invalidate: Box<dyn Fn() + Send + Sync>,
        clear_placeholder_background: Box<dyn Fn() + Send + Sync>,
        child_exe: std::path::PathBuf,
    ) -> Option<Self> {
        // SAFETY: `DCompositionCreateDevice3` has no preconditions on the
        // caller beyond a valid `iid` (inferred here as `IDCompositionDesktopDevice`
        // via the return type) -- `None` for `renderingDevice` is documented as
        // valid when the caller doesn't need device-tied composition surfaces,
        // which this backend never creates (its surfaces come from
        // `DCompositionCreateSurfaceHandle`, a separate device-independent call).
        let dcomp_device: IDCompositionDesktopDevice = unsafe { DCompositionCreateDevice3(None) }
            .inspect_err(|err| {
                log::error!("HostProcessBackend: DCompositionCreateDevice3 failed: {err}")
            })
            .ok()?;
        // SAFETY: `hwnd` is a live HWND (the caller's window, or a test's own
        // window -- see `spawn`/`host_process_backend::tests`), valid for the
        // duration of this call; `CreateTargetForHwnd` does not retain it beyond
        // establishing the composition target.
        let dcomp_target = unsafe {
            dcomp_device.CreateTargetForHwnd(windows::Win32::Foundation::HWND(hwnd as _), true)
        }
        .inspect_err(|err| log::error!("HostProcessBackend: CreateTargetForHwnd failed: {err}"))
        .ok()?;
        // SAFETY: `CreateVisual`/`SetRoot` are plain COM calls on a device/target
        // just created above in this same function; there is no external
        // resource whose validity this call depends on beyond `dcomp_device`/
        // `dcomp_target` themselves, which outlive this scope (owned by `shared`
        // below).
        let dcomp_visual = unsafe { dcomp_device.CreateVisual() }
            .inspect_err(|err| log::error!("HostProcessBackend: CreateVisual failed: {err}"))
            .ok()?;
        // SAFETY: see the comment on the `CreateVisual` call immediately above --
        // same device/target/visual, same scope, same reasoning.
        if let Err(err) = unsafe { dcomp_target.SetRoot(&dcomp_visual) } {
            log::error!("HostProcessBackend: SetRoot failed: {err}");
            return None;
        }

        let shared = Arc::new(Shared {
            invalidate,
            clear_placeholder_background,
            dcomp_device,
            _dcomp_target: dcomp_target,
            dcomp_visual,
            dimensions: Mutex::new(dimensions),
            current: Mutex::new(None),
            next_generation: AtomicU64::new(0),
            needs_full_resync: AtomicBool::new(true),
            in_flight: Arc::new(AtomicBool::new(false)),
            repaint_pending: Arc::new(AtomicBool::new(false)),
            window_destroyed: Arc::new(AtomicBool::new(false)),
            submit_started_at: Arc::new(Mutex::new(None)),
            respawn_attempts: Mutex::new(Vec::new()),
            demoted: AtomicBool::new(false),
            teardown_sentinel_strong: Arc::new(()),
            pending_surface_handles: Mutex::new(std::collections::HashMap::new()),
            child_exe,
        });

        if !spawn_generation(&shared) {
            log::error!("HostProcessBackend: initial child spawn failed");
            return None;
        }

        Some(Self { shared })
    }

    /// The current generation's child process id, if one is running. Used
    /// by tests to forcibly kill the exact child this backend spawned (see
    /// the project's process-safety rule: only ever by a PID captured at
    /// spawn time, never by image name).
    #[cfg(test)]
    pub(crate) fn debug_current_child_pid(&self) -> Option<u32> {
        self.shared.current.lock().as_ref().map(|c| c.child.id())
    }
}

/// Spawns a fresh child generation, ties its lifetime to this process via a
/// Job Object (falling back to the `--supervise-pid` watcher thread inside
/// the child if that setup fails, exactly like `per_tab_process_isolation`'s
/// pty-hosting children), creates a new composition-surface handle for it,
/// and starts its writer/reader threads. Returns `false` (logged) if the
/// process itself couldn't be spawned at all.
fn spawn_generation(shared: &Arc<Shared>) -> bool {
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

    spawn_writer_thread(generation, stdin, writer_rx);
    spawn_reader_thread_with_death_handling(Arc::clone(shared), generation, stdout);

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
    shared.in_flight.store(false, Ordering::Release);
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

fn spawn_writer_thread(
    generation: u64,
    mut stdin: std::process::ChildStdin,
    rx: Receiver<HostToChildMsg>,
) {
    std::thread::Builder::new()
        .name(format!("gpu-host-writer-{generation}"))
        .spawn(move || {
            for msg in rx {
                let result = match msg {
                    HostToChildMsg::AttachSurface {
                        surface_handle,
                        width,
                        height,
                    } => wire::write_attach_surface(&mut stdin, surface_handle, width, height),
                    HostToChildMsg::Frame(frame) => wire::write_frame(&mut stdin, &frame),
                    HostToChildMsg::Resize { width, height } => {
                        wire::write_resize(&mut stdin, width, height)
                    }
                    HostToChildMsg::Shutdown => {
                        let res = wire::write_shutdown(&mut stdin);
                        let _ = stdin.flush();
                        res
                    }
                };
                if let Err(err) = result.and_then(|()| stdin.flush()) {
                    log::warn!(
                        "gpu-host-writer-{generation}: write failed (child likely dead): {err}"
                    );
                    break;
                }
            }
        })
        .expect("failed to spawn gpu-host writer thread");
}

/// Spawns the reader thread for one child generation. Runs `reader_loop`
/// until the child's ack channel closes or errors, then always calls
/// `handle_child_death` exactly once -- regardless of which condition ended
/// the loop.
fn spawn_reader_thread_with_death_handling(
    shared: Arc<Shared>,
    generation: u64,
    stdout: std::process::ChildStdout,
) {
    let shared_for_thread = Arc::clone(&shared);
    std::thread::Builder::new()
        .name(format!("gpu-host-reader-{generation}"))
        .spawn(move || {
            reader_loop(&shared_for_thread, generation, stdout);
            handle_child_death(&shared_for_thread, generation);
        })
        .expect("failed to spawn gpu-host reader thread");
}

fn reader_loop(shared: &Arc<Shared>, generation: u64, mut stdout: std::process::ChildStdout) {
    loop {
        match wire::read_message(&mut stdout) {
            Ok(Some(wire::WireMessage::Presented(presented))) => {
                on_presented(shared, generation, presented.seq);
            }
            Ok(Some(wire::WireMessage::Fatal(fatal))) => {
                log::error!(
                    "gpu-host generation {generation}: child reported Fatal({})",
                    fatal.code
                );
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                log::info!("gpu-host generation {generation}: child closed its ack channel");
                return;
            }
            Err(err) => {
                log::warn!("gpu-host generation {generation}: ack channel read failed: {err}");
                return;
            }
        }
    }
}

fn on_presented(shared: &Arc<Shared>, generation: u64, _seq: u64) {
    {
        let current = shared.current.lock();
        let Some(current) = current.as_ref() else {
            return;
        };
        if current.generation != generation {
            // A stale ack from a generation we've already moved past.
            return;
        }
    }

    shared.in_flight.store(false, Ordering::Release);
    *shared.submit_started_at.lock() = None;

    // First ack of a freshly (re)attached generation: swap the visual over
    // to it now -- never before this point, or a broken/blank child would
    // flash on screen instead of the previous generation's last good frame.
    let mut pending = shared.pending_surface_handles.lock();
    if let Some(handle) = pending.remove(&generation) {
        drop(pending);
        if let Err(err) = swap_visual_content(shared, handle) {
            log::error!("HostProcessBackend: failed to swap visual content: {err:#}");
        }
        // Real content has now actually landed on screen for the first time
        // (this generation's first ack) -- safe to tear down the Windows
        // GDI placeholder. Idempotent, so unconditional here is fine even
        // across respawns.
        (shared.clear_placeholder_background)();
        // Reset the respawn budget: a generation that reached its first
        // real frame is a success, not a lingering failure to hold against
        // future budget checks.
        shared.respawn_attempts.lock().clear();
    }

    if shared.repaint_pending.swap(false, Ordering::AcqRel) {
        (shared.invalidate)();
    }
}

fn swap_visual_content(shared: &Shared, handle: HANDLE) -> anyhow::Result<()> {
    // SAFETY: `handle` is a composition-surface handle this generation's
    // child just successfully presented into (we're here because its first
    // `Presented` ack arrived), still open (not yet closed by any
    // now-superseded generation), satisfying `CreateSurfaceFromHandle`'s
    // requirement that the handle be valid and usable to build a swapchain
    // on. `SetContent`/`Commit` are plain COM calls on a live device/visual
    // this struct owns for its whole lifetime.
    unsafe {
        let content: IUnknown = shared.dcomp_device.CreateSurfaceFromHandle(handle)?;
        shared.dcomp_visual.SetContent(&content)?;
        shared.dcomp_device.Commit()?;
    }
    Ok(())
}

/// Called when a generation's reader thread observes its child is gone
/// (EOF or a read error). Respawns within budget, or demotes permanently.
fn handle_child_death(shared: &Arc<Shared>, dead_generation: u64) {
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
        shared.in_flight.store(false, Ordering::Release);
        (shared.invalidate)();
        return;
    }

    let delay = rebuild_backoff_for_attempt(attempt);
    let shared = Arc::clone(shared);
    let respawn = move || {
        if !spawn_generation(&shared) {
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

impl RenderBackend for HostProcessBackend {
    fn frame_form(&self) -> FrameForm {
        FrameForm::Wire {
            full_resync: self.shared.needs_full_resync.swap(false, Ordering::AcqRel),
        }
    }

    fn wants_atlas_mirroring(&self) -> bool {
        // The child process renders against its own `wgpu::Device`, so it
        // needs a pixel-for-pixel replay of this window's atlas; see the
        // trait method's doc comment for why answering this before the
        // atlas is built (rather than inferring it from `frame_form`) is
        // load-bearing.
        true
    }

    fn send_resize(&self, dims: Dimensions) {
        *self.shared.dimensions.lock() = dims;
        if let Some(current) = self.shared.current.lock().as_ref() {
            current
                .writer_tx
                .send(HostToChildMsg::Resize {
                    width: dims.pixel_width as u32,
                    height: dims.pixel_height as u32,
                })
                .ok();
        }
    }

    fn send_frame(&self, frame: SubmittableFrame) {
        let SubmittableFrame::Wire(frame) = frame else {
            log::error!(
                "HostProcessBackend::send_frame received an InProcess frame; dropping it \
                 (frame_form() said Wire)"
            );
            return;
        };
        if self.shared.in_flight.swap(true, Ordering::AcqRel) {
            self.shared.repaint_pending.store(true, Ordering::Release);
            metrics::counter!("gui.host_process.frames_dropped").increment(1);
            return;
        }
        *self.shared.submit_started_at.lock() = Some(Instant::now());
        let Some(current) = self
            .shared
            .current
            .lock()
            .as_ref()
            .map(|c| c.writer_tx.clone())
        else {
            self.shared.in_flight.store(false, Ordering::Release);
            return;
        };
        if current.send(HostToChildMsg::Frame(frame)).is_err() {
            self.shared.in_flight.store(false, Ordering::Release);
        }
    }

    fn is_in_flight(&self) -> bool {
        self.shared.in_flight.load(Ordering::Acquire)
    }

    fn set_repaint_pending(&self) {
        self.shared.repaint_pending.store(true, Ordering::Release);
    }

    fn shutdown(&self) {
        self.shared.window_destroyed.store(true, Ordering::Release);
        if let Some(current) = self.shared.current.lock().take() {
            current.writer_tx.send(HostToChildMsg::Shutdown).ok();
        }
    }

    fn render_thread_is_hung(&self) -> bool {
        // A respawn already in flight resolves its own stall; only a
        // permanently-demoted backend can be "hung" in the sense the
        // window's supervisor cares about, and that case is already
        // reported via `render_thread_has_died`.
        false
    }

    fn render_thread_has_died(&self) -> bool {
        self.shared.demoted.load(Ordering::Acquire)
    }

    fn teardown_sentinel(&self) -> Weak<dyn std::any::Any + Send + Sync> {
        Arc::downgrade(&self.shared.teardown_sentinel_strong)
            as Weak<dyn std::any::Any + Send + Sync>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP,
    };

    /// Resolves the real `onlyterm-gui.exe` next to the test harness binary.
    /// `std::env::current_exe()` under `cargo test` returns the harness
    /// itself (which has no `gpu-tab-host` subcommand -- it parses its own
    /// test-filter arguments instead), not the plain GUI executable, so
    /// these tests need the real one built first (`cargo build -p
    /// wezterm-gui`, already a prerequisite for every check this session).
    fn real_gui_exe() -> std::path::PathBuf {
        let mut path = std::env::current_exe().expect("current_exe");
        path.pop();
        if path.file_name().and_then(|n| n.to_str()) == Some("deps") {
            path.pop();
        }
        path.push("onlyterm-gui.exe");
        assert!(
            path.exists(),
            "expected the real onlyterm-gui.exe already built at {:?} -- run \
             `cargo build -p wezterm-gui` first",
            path
        );
        path
    }

    static REGISTER_CLASS: Once = Once::new();

    unsafe extern "system" fn test_wndproc(
        hwnd: windows::Win32::Foundation::HWND,
        msg: u32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        // SAFETY: forwarding the exact arguments the OS just handed this
        // window procedure to the default handler; no additional invariant
        // to uphold beyond what the OS already guarantees for any wndproc call.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// A minimal, invisible Win32 window good enough to hand
    /// `IDCompositionDesktopDevice::CreateTargetForHwnd` -- see
    /// `HostProcessBackend::spawn_for_hwnd_with_exe`'s doc comment for why a
    /// real `::window::Window` isn't used here.
    fn create_test_hwnd() -> isize {
        // SAFETY: `RegisterClassW`/`CreateWindowExW` are standard Win32 calls;
        // `class_name`/`instance` outlive this block (`w!` is a `'static`
        // wide-string literal, `instance` comes from `GetModuleHandleW`), and
        // `test_wndproc` matches the required `WNDPROC` signature.
        unsafe {
            let instance = GetModuleHandleW(None).expect("GetModuleHandleW");
            let class_name = windows::core::w!("HostProcessBackendTestWindow");
            REGISTER_CLASS.call_once(|| {
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(test_wndproc),
                    hInstance: instance.into(),
                    lpszClassName: class_name,
                    ..Default::default()
                };
                RegisterClassW(&wc);
            });
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                windows::core::w!("host-process-backend-test"),
                WS_POPUP,
                0,
                0,
                64,
                64,
                None,
                None,
                instance,
                None,
            )
            .expect("CreateWindowExW");
            hwnd.0 as isize
        }
    }

    /// Terminates a process by the exact PID the caller captured at spawn
    /// time -- never by image name. These are `onlyterm-gui.exe` (in
    /// `gpu-tab-host` mode) processes this test itself spawned moments ago
    /// via `HostProcessBackend`, which is exactly the documented exception
    /// to this repo's "never kill onlyterm-gui.exe" rule.
    fn kill_process_by_captured_pid(pid: u32) {
        // SAFETY: `pid` is the exact PID this test captured moments ago from
        // its own `HostProcessBackend`'s `debug_current_child_pid()`, not
        // discovered by name/enumeration; `handle` is closed before this
        // block ends.
        unsafe {
            let handle =
                OpenProcess(PROCESS_TERMINATE, false, pid).expect("OpenProcess on our own child");
            TerminateProcess(handle, 1).ok();
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
    }

    fn wait_until(mut pred: impl FnMut() -> bool, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if pred() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn spawn_test_backend() -> HostProcessBackend {
        let hwnd = create_test_hwnd();
        let dimensions = Dimensions {
            pixel_width: 320,
            pixel_height: 240,
            dpi: 96,
        };
        HostProcessBackend::spawn_for_hwnd_with_exe(
            hwnd,
            dimensions,
            Box::new(|| {}),
            Box::new(|| {}),
            real_gui_exe(),
        )
        .expect("HostProcessBackend::spawn_for_hwnd_with_exe should succeed on this machine")
    }

    #[test]
    fn a_forcibly_killed_child_is_respawned_without_the_backend_dying() {
        let backend = spawn_test_backend();
        assert!(!backend.render_thread_has_died());

        let pid_before = wait_until_child_pid(&backend).expect("a child should be running");
        kill_process_by_captured_pid(pid_before);

        assert!(
            wait_until(
                || backend
                    .debug_current_child_pid()
                    .is_some_and(|pid| pid != pid_before),
                Duration::from_secs(10)
            ),
            "expected a new child PID within 10s of forcibly killing the old one"
        );
        assert!(
            !backend.render_thread_has_died(),
            "a single forced death within budget must not demote the backend"
        );
    }

    #[test]
    fn repeated_forced_deaths_exhaust_the_budget_and_demote_without_panicking() {
        let backend = spawn_test_backend();

        // Kill and wait for each respawn to actually land (a new, distinct
        // PID) before the next kill -- otherwise a kill can race an
        // in-flight respawn and land on an already-dead PID, undercounting
        // real death events against the budget.
        for _ in 0..MAX_RESPAWNS_PER_WINDOW {
            let pid = wait_until_child_pid(&backend).expect("a child should be running");
            kill_process_by_captured_pid(pid);
            assert!(
                wait_until(
                    || backend
                        .debug_current_child_pid()
                        .is_some_and(|new_pid| new_pid != pid),
                    Duration::from_secs(10)
                ),
                "expected a respawn within budget"
            );
        }

        // This last kill is the one that exhausts the budget -- the backend
        // demotes instead of respawning again, so there is no new PID to
        // wait for here, only the death report flipping.
        let pid = wait_until_child_pid(&backend).expect("a child should be running");
        kill_process_by_captured_pid(pid);

        assert!(
            wait_until(|| backend.render_thread_has_died(), Duration::from_secs(15)),
            "expected the backend to demote itself after exhausting its respawn budget \
             ({} respawns within {:?})",
            MAX_RESPAWNS_PER_WINDOW,
            RESPAWN_WINDOW
        );
    }

    /// Waits (briefly) for a child to actually be running and returns its
    /// PID -- immediately after spawning, `debug_current_child_pid` can
    /// observe a moment where the previous generation was just replaced and
    /// the new `Child` handle isn't stored yet.
    fn wait_until_child_pid(backend: &HostProcessBackend) -> Option<u32> {
        let mut pid = None;
        wait_until(
            || {
                pid = backend.debug_current_child_pid();
                pid.is_some()
            },
            Duration::from_secs(5),
        );
        pid
    }
}
