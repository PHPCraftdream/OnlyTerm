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

use crate::backpressure::LogRateLimiter;
use crate::wire;
use parking_lot::Mutex;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};
use window::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use window::{Dimensions, WindowOps};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice3, IDCompositionDesktopDevice, IDCompositionTarget,
    IDCompositionVisual2,
};

const GENERIC_ALL: u32 = 0x1000_0000;
const MAX_RESPAWNS_PER_WINDOW: usize = 3;
const RESPAWN_WINDOW: Duration = Duration::from_secs(30);

/// Rate limit for the handshake diagnostics (added with the SeqCst fix):
/// these paths can fire on every colliding paint under sustained terminal
/// output; their job is to leave a trace correlatable with the
/// `activate_tab:`/`focus_changed:` logging after a real incident, not to
/// record every occurrence.
const HANDSHAKE_LOG_RATE: Duration = Duration::from_secs(1);
static PRESENTED_PENDING_REPAINT_LOG: LogRateLimiter = LogRateLimiter::new();
static FAILED_FRAME_LOG: LogRateLimiter = LogRateLimiter::new();
static SEND_QUEUED_FAILED_LOG: LogRateLimiter = LogRateLimiter::new();

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
    /// True from the moment `handle_child_death` schedules a respawn
    /// (immediately or after backoff) until that respawn attempt actually
    /// runs. Gates `render_thread_is_hung`: the backoff window routinely
    /// exceeds how long ago the last frame was sent, and the window's
    /// supervisor piling a full renderer rebuild on top of a working
    /// respawn would defeat the respawn.
    respawn_pending: AtomicBool,
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
    /// Shared pool of reusable `Vec<QuadInstance>` draw buffers.
    /// `build_wire_frame` (GUI thread) takes buffers from this pool instead
    /// of cloning fresh allocations; the writer thread returns them after
    /// `write_frame` has serialized their contents onto the wire. See
    /// `wire::WireDrawPool`'s doc comment for the full rationale.
    draw_pool: wire::WireDrawPool,
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
            respawn_pending: AtomicBool::new(false),
            window_destroyed: Arc::new(AtomicBool::new(false)),
            submit_started_at: Arc::new(Mutex::new(None)),
            respawn_attempts: Mutex::new(Vec::new()),
            demoted: AtomicBool::new(false),
            teardown_sentinel_strong: Arc::new(()),
            pending_surface_handles: Mutex::new(std::collections::HashMap::new()),
            child_exe,
            draw_pool: wire::new_draw_pool(),
        });

        if !lifecycle::spawn_generation(&shared) {
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

#[path = "process_backend/backend.rs"]
mod backend;
#[path = "process_backend/io.rs"]
mod io;
#[path = "process_backend/lifecycle.rs"]
mod lifecycle;

#[cfg(test)]
#[path = "process_backend/tests.rs"]
mod tests;
