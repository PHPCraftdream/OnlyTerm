//! Owns a window's GPU surface, in-process or via a child process.
//!
//! Extracted from the `onlyterm-gui` crate so the GPU-surface ownership code
//! (shared WebGPU context, per-window surface, the `--gpu-tab-host` child
//! process and its parent-side `HostProcessBackend`, plus the
//! `RenderBackend` abstraction both render paths implement) forms its own
//! smaller compile unit. `onlyterm-gui` keeps everything `TermWindow`-shaped
//! and talks to this crate through its public API.

pub mod backpressure;
pub mod gpu_tab_host;
pub mod host_process_backend;
mod quad;
pub mod webgpu;

pub use host_process_backend::HostProcessBackend;
pub use onlyterm_gpu_protocol::{wire, ShaderUniform};
pub use quad::{
    CornerVertex, QuadInstance, VERTICES_PER_CELL, V_BOT_LEFT, V_BOT_RIGHT, V_TOP_LEFT, V_TOP_RIGHT,
};
pub use webgpu::{
    adapter_info_to_gpu_info, GpuDraw, GpuFrame, ProcessGpuContext, WebGpuState, WebGpuTexture,
    WindowGpuSurface,
};

/// A callback the embedder (onlyterm-gui) supplies when registering a window
/// for GPU error recovery, so this crate never needs to know about
/// `TermWindow`/`TermWindowNotif`. Receives the human-readable reason
/// message; the embedder's closure is expected to route it to the window's
/// render-error recovery handler.
pub type GpuRecoveryNotifier = Box<dyn Fn(&str) + Send + Sync>;

/// Which shape of frame a `RenderBackend` wants from its caller, and
/// whether that frame must be complete rather than incremental.
///
/// Answered by `frame_form()` *before* the caller builds anything, so an
/// `InProcess`-shaped backend never pays for a `Wire` frame's CPU-side work
/// (and vice versa) only to throw it away -- see `render/draw.rs`'s frame
/// dispatch, which asks this first and only then calls the matching
/// `build_*_frame`.
pub enum FrameForm {
    /// Build a `GpuFrame` the ordinary way (`build_webgpu_frame`).
    InProcess,
    /// Build a `wire::WireFrame` (`build_wire_frame`) instead -- this
    /// backend submits across a process boundary and cannot accept live
    /// `wgpu::Buffer`/`wgpu::Texture` handles tied to the caller's own
    /// device. `full_resync: true` means this backend just (re)attached to a
    /// fresh child (first attach, or a post-respawn reattach) whose atlas
    /// mirror is empty regardless of whether the real atlas's identity
    /// changed -- the caller must force `atlas_reset` on this build even if
    /// its own atlas-identity comparison says nothing changed.
    Wire { full_resync: bool },
}

/// A built frame ready to hand to `RenderBackend::send_frame`, in whichever
/// shape that backend's `frame_form()` asked for.
pub enum SubmittableFrame {
    InProcess(GpuFrame),
    Wire(wire::WireFrame),
}

/// Whatever produces pixels for one window's GPU surface, decoupled from
/// *how* it does that.
///
/// Two implementations: `RenderThreadHandle` (in-process, sharing this
/// window's process-wide `ProcessGpuContext` -- see
/// `termwindow::webgpu::context`) and `HostProcessBackend` (task #651,
/// per-window, a dedicated `--gpu-tab-host` child process; see
/// docs/plans/2026-08-21-per-tab-gpu-process-isolation.md and the `@ox`
/// architecture review that settled per-window granularity with silent
/// respawn-in-place over per-tab or a crash-visible epitaph screen).
/// `TermWindow`/`render_pipeline.rs` talk only to this trait, never to a
/// concrete implementation directly, so the config-selected backend
/// (`webgpu_engine`, task #646) can be swapped in behind it without
/// touching any call site here -- the same shape `per_tab_process_isolation`
/// already uses for pty/shell isolation, applied to GPU rendering instead.
///
/// Deliberately narrow: CPU-side frame *building* (`RenderState`,
/// `GlyphCache`, quad construction in `box_model.rs`) stays entirely in the
/// GUI process in every variant. None of the three crashes diagnosed this
/// session happened there -- all three were inside `Surface::configure`/
/// `Surface::present`/device creation, which is exactly what a
/// `RenderBackend` covers: taking an already-built frame and getting it on
/// screen.
pub trait RenderBackend {
    /// Which frame shape (and whether it must be a full resync) this
    /// backend wants built for its next `send_frame` call. `RenderThreadHandle`
    /// always answers `FrameForm::InProcess`.
    fn frame_form(&self) -> FrameForm;
    /// Whether this backend needs every write to the window's glyph atlas
    /// recorded (`WebGpuTexture::enable_mirroring`) so that it can be
    /// replayed into a mirror living somewhere this process's own
    /// `wgpu::Texture` cannot reach.
    ///
    /// `true` only for `HostProcessBackend`, whose child process renders
    /// against its own device and therefore its own copy of the atlas.
    ///
    /// This has to be a separate question from `frame_form()`, asked before
    /// `RenderState::new` builds the atlas rather than derived from the
    /// first frame: `UtilSprites::new` writes the util sprites -- including
    /// `white_space`, the texel every glyph/underline/cursor quad samples --
    /// during atlas construction, and mirroring switched on any later misses
    /// them permanently. The child then draws those quads with alpha 0,
    /// producing a window whose solid-color backgrounds are correct and
    /// whose text is entirely absent.
    fn wants_atlas_mirroring(&self) -> bool {
        false
    }
    /// Report that the parent-side atlas replay log could not retain a
    /// complete mirror within its memory budget.  A host-process backend can
    /// use this as a graceful signal to demote to in-process rendering;
    /// ordinary in-process backends have nothing to do.
    fn atlas_mirroring_failed(&self) {}
    /// Send a resize/reconfigure request. See `RenderThreadHandle::send_resize`.
    fn send_resize(&self, dims: ::window::Dimensions);
    /// Send a frame to be rendered, built in whichever shape `frame_form()`
    /// asked for. See `RenderThreadHandle::send_frame`.
    fn send_frame(&self, frame: SubmittableFrame);
    /// Whether a frame sent via `send_frame` is still being processed. See
    /// `RenderThreadHandle::is_in_flight`.
    fn is_in_flight(&self) -> bool;
    /// Record that a fresh repaint is needed once any in-flight frame
    /// finishes. See `RenderThreadHandle::set_repaint_pending`.
    fn set_repaint_pending(&self);
    /// Ask this backend to stop. See `RenderThreadHandle::shutdown`.
    fn shutdown(&self);
    /// Whether this backend appears stuck and cannot resolve it on its own.
    /// For `RenderThreadHandle`: stuck inside a single GPU call past the
    /// hang threshold, full stop (there is no recovery path other than the
    /// window's own supervisor rebuilding the renderer). For
    /// `HostProcessBackend`: true once a submitted frame has gone un-acked
    /// past the hang threshold while no respawn is already in flight -- the
    /// child is alive but not making progress, so the window's supervisor
    /// rebuilding the renderer is the only remaining recovery. A respawn in
    /// flight is the one exception: it resets the clock itself (and its
    /// backoff window routinely exceeds how long the last frame has been
    /// running), so the supervisor must not pile a full rebuild on top of
    /// it (see `render_thread_has_died`'s contract, which the demoted
    /// end-state still mirrors).
    fn render_thread_is_hung(&self) -> bool;
    /// Whether this backend has died **permanently** -- not coming back on
    /// its own. For `RenderThreadHandle`: the thread exited without being
    /// asked to (see the existing doc/tests). For `HostProcessBackend`: a
    /// child dying is normal and handled internally by respawning it
    /// (silently, no user-visible epitaph -- a dead child's composition
    /// surface keeps showing its last presented frame, confirmed
    /// empirically this session); this must read `false` while a respawn is
    /// in progress or could still be attempted, and only flips to `true`
    /// once the respawn budget (`rebuild_backoff_for_attempt`'s shape, 3
    /// attempts / 30s) is exhausted and this backend has demoted itself
    /// (sticky, for the window's remaining life) rather than keep retrying.
    fn render_thread_has_died(&self) -> bool;
    /// Type-erased teardown signal for `Window::recreate_webgpu_child_window`/
    /// `sweep_retired_webgpu_children`. See `RenderThreadHandle::teardown_sentinel`.
    fn teardown_sentinel(&self) -> std::sync::Weak<dyn std::any::Any + Send + Sync>;
    /// Returns the shared draw-buffer pool for wire-frame construction, if
    /// this backend uses one. `build_wire_frame` takes buffers from this pool
    /// (instead of cloning fresh allocations every frame), and the backend's
    /// writer thread returns them after serialization -- eliminating the
    /// unbounded per-frame allocation stream that caused the 4 GB OOM.
    ///
    /// Only `HostProcessBackend` returns `Some`; in-process backends never
    /// build wire frames and return `None`.
    fn wire_draw_pool(&self) -> Option<wire::WireDrawPool> {
        None
    }
}

use std::time::Duration;

/// The first attempt never waits: the common case is a one-off driver stall,
/// and delaying its recovery would show the user a frozen window for no
/// reason. Every attempt after that follows one that just failed, and the
/// reason it failed is usually that the machine is out of memory right now --
/// so re-entering device/surface creation immediately walks straight back
/// into it. Spacing them lets the shortage clear, and bounds how often we
/// hand a driver in that state the one call it is worst at.
///
/// Kept as a free function so the schedule can be asserted without a
/// `TermWindow`, a GPU or a window. `pub` so both this crate's
/// `HostProcessBackend` (task #651) and the GUI crate's renderer-rebuild retry
/// logic can reuse the exact same schedule for its own child-process
/// respawns -- the reasoning transfers unchanged, and if anything applies
/// more strongly there (a whole process's device creation, not just a
/// surface reconfigure).
pub fn rebuild_backoff_for_attempt(attempt: usize) -> Option<Duration> {
    match attempt {
        0 | 1 => None,
        2 => Some(Duration::from_millis(250)),
        _ => Some(Duration::from_millis(1000)),
    }
}

#[cfg(test)]
mod rebuild_backoff_tests {
    use super::rebuild_backoff_for_attempt;
    use std::time::Duration;

    /// The first attempt of an episode must be immediate: a single transient
    /// driver stall is the common case, and making the user stare at a frozen
    /// window before recovering it would be a regression in its own right.
    #[test]
    fn the_first_rebuild_attempt_is_not_delayed() {
        assert_eq!(rebuild_backoff_for_attempt(1), None);
    }

    /// Retries must be spaced. A crash dump from this machine showed two
    /// attempts 107ms apart, the second one dying inside the GPU driver while
    /// it was still out of memory from the first.
    #[test]
    fn retries_are_spaced_and_do_not_shrink() {
        let second = rebuild_backoff_for_attempt(2).expect("the second attempt must wait");
        let third = rebuild_backoff_for_attempt(3).expect("the third attempt must wait");

        assert!(
            second >= Duration::from_millis(100),
            "a retry delay shorter than the 107ms gap that crashed is no delay at all, got {:?}",
            second
        );
        assert!(
            third >= second,
            "backoff must not shrink as attempts pile up: attempt 2 waits {:?}, attempt 3 waits {:?}",
            second,
            third
        );
    }
}
