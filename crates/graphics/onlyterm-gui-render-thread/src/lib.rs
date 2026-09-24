//! Scaffolding for moving WebGpu frame submission off the GUI thread.
//!
//! Task 221.4 set up the channel and thread lifecycle; 221.5 wired
//! `RenderMsg::Frame` up to `WebGpuState::submit_frame`, with single-slot
//! back-pressure so at most one frame is ever in flight on the render thread
//! at a time. This module (221.6) wires `RenderMsg::Resize` up to
//! `WebGpuState::resize` (i.e. `surface.configure`), with coalescing so a
//! flood of resize messages (e.g. a live drag) collapses to just the latest
//! one, and gives `submit_one_frame` real `SurfaceError::Lost`/`Outdated`
//! recovery via `WebGpuState::reconfigure`.
//!
//! Task 221.7 adds window-teardown safety and hang visibility on top of
//! that: a `window_destroyed` flag so a `Frame`/`Resize` message that was
//! already queued before `Shutdown` don't reach into a dead HWND's GPU
//! resources, and a `submit_started_at` timestamp so a future per-window
//! supervisor (task #223) can ask "is this window's render thread currently
//! stuck inside a submit/reconfigure call".
//!
//! Gated behind `onlyterm_config::webgpu_render_thread`, which now defaults to
//! `true` (221.8 flipped the default) -- this render thread is the live
//! path for GPU frame submission, not dormant scaffolding.

mod dispatch;
mod handle;
mod render;

#[cfg(test)]
mod tests;

pub use handle::{RenderMsg, RenderThreadHandle, RenderThreadSeed};
