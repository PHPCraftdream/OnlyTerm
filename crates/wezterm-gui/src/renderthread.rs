//! Scaffolding for moving WebGpu frame submission off the GUI thread.
//!
//! This module is intentionally inert for now: it sets up the channel and
//! thread lifecycle (task 221.4) but does not yet route any real frames
//! through it (that's task 221.5) or handle resizes (221.6). Enabling the
//! `webgpu_render_thread` config flag today spawns a per-window thread that
//! receives `RenderMsg`s and silently drops them, and shuts down cleanly
//! when the window closes.
//!
//! Only used behind `config::webgpu_render_thread`, which defaults to
//! `false`, so none of this changes behavior until a later task flips the
//! default (221.8).

/// A message sent from the GUI thread to a window's dedicated render
/// thread.
pub enum RenderMsg {
    /// A fully built frame ready to submit to the GPU. 221.5 will make the
    /// render thread actually call `WebGpuState::submit_frame` with this;
    /// for now the render thread just drops it.
    #[allow(dead_code)]
    Frame(crate::termwindow::webgpu::GpuFrame),
    /// A resize/reconfigure request. 221.6 will make the render thread
    /// actually call `WebGpuState::resize` (i.e. `surface.configure`) with
    /// this; for now the render thread just ignores it.
    #[allow(dead_code)]
    Resize(::window::Dimensions),
    /// Ask the render thread to stop its message loop and exit.
    Shutdown,
}

/// Everything a newly spawned render thread needs to run, handed over from
/// the GUI thread. `window`/`webgpu` aren't read yet in this task (the loop
/// below doesn't touch the GPU at all), but are threaded through now so
/// 221.5/221.6 don't need to change this struct's shape.
pub struct RenderThreadSeed {
    #[allow(dead_code)]
    pub window: ::window::Window,
    #[allow(dead_code)]
    pub webgpu: std::sync::Arc<crate::termwindow::webgpu::WebGpuState>,
    pub rx: std::sync::mpsc::Receiver<RenderMsg>,
}

/// A handle to a window's dedicated render thread, owned by `TermWindow`.
///
/// Deliberately holds no `JoinHandle`. The entire point of moving GPU
/// submission off the GUI thread is so that a stuck driver call (a TDR, a
/// swapchain `present()` that never returns) can't freeze the message loop.
/// If window-close code called `.join()` on this thread, a hung render
/// thread would hang the close operation too, which defeats the purpose.
/// So this handle can only ever send messages to the thread and drop its
/// `Sender`; the thread itself is fire-and-forget from the GUI thread's
/// point of view.
pub struct RenderThreadHandle {
    tx: std::sync::mpsc::Sender<RenderMsg>,
}

impl RenderThreadHandle {
    /// Spawn a dedicated render thread for one window, identified by
    /// `window_id` for the thread's name (used e.g. as `TermWindow`'s
    /// `mux_window_id`, so the thread is identifiable in a debugger/task
    /// manager without inventing new plumbing just for this).
    ///
    /// The caller creates the `mpsc::channel()` pair once: `tx` is handed
    /// here so it can be wrapped up into the returned handle, and `rx` is
    /// handed here already embedded in `seed` (`RenderThreadSeed::rx`) so it
    /// can be moved onto the new thread. This way the channel is
    /// constructed exactly once by the caller, never duplicated inside
    /// `spawn`.
    ///
    /// Returns `Some(handle)` on Windows, where the render thread is
    /// actually spawned. Returns `None` everywhere else: the render-thread
    /// pipeline (221.1-221.9) is Windows-only for now (see the plan doc,
    /// "Уровень C"), and other platforms keep rendering synchronously on
    /// the GUI thread with no functional change.
    ///
    /// The uniform `Option<RenderThreadHandle>` return type (rather than
    /// `#[cfg]`-ing the function signature itself) means call sites in
    /// `TermWindow` never need their own `#[cfg(windows)]`.
    #[cfg(windows)]
    pub fn spawn(
        seed: RenderThreadSeed,
        tx: std::sync::mpsc::Sender<RenderMsg>,
        window_id: impl std::fmt::Display,
    ) -> Option<Self> {
        let name = format!("render-{window_id}");
        let builder = std::thread::Builder::new().name(name);
        match builder.spawn(move || render_thread_loop(seed)) {
            Ok(join_handle) => {
                // We deliberately never join this thread; see the doc
                // comment on `RenderThreadHandle`. Discard the
                // `JoinHandle` so it's clear this is intentional, not an
                // oversight.
                drop(join_handle);
                Some(Self { tx })
            }
            Err(err) => {
                log::error!("Failed to spawn render thread: {:#}", err);
                None
            }
        }
    }

    #[cfg(not(windows))]
    pub fn spawn(
        _seed: RenderThreadSeed,
        _tx: std::sync::mpsc::Sender<RenderMsg>,
        _window_id: impl std::fmt::Display,
    ) -> Option<Self> {
        None
    }

    /// Send a message to the render thread. Returns an error if the thread
    /// has already exited (its `Receiver` was dropped); callers generally
    /// don't need to do anything about that other than not panic.
    ///
    /// `clippy::result_large_err` fires because `SendError<RenderMsg>`
    /// carries a whole `GpuFrame` back out on failure; not boxing it since
    /// this is a cold, infrequent-call path (once per frame at most, and
    /// only ever hit once the render thread is already gone), not a hot
    /// loop where the extra stack size would matter.
    #[allow(dead_code)]
    #[allow(clippy::result_large_err)]
    pub fn send(&self, msg: RenderMsg) -> Result<(), std::sync::mpsc::SendError<RenderMsg>> {
        self.tx.send(msg)
    }

    /// Ask the render thread to stop. This does not wait for it to actually
    /// exit (no `.join()` - see the struct doc comment). A send error here
    /// just means the thread is already gone, which is fine.
    pub fn shutdown(&self) {
        let _ = self.tx.send(RenderMsg::Shutdown);
    }
}

/// The render thread's message loop. Runs until the channel disconnects
/// (all `Sender`s, including the one held by `RenderThreadHandle`, were
/// dropped) or a `RenderMsg::Shutdown` is received - whichever happens
/// first.
///
/// For this task, no GPU calls happen here at all: frames are dropped and
/// resizes are ignored. 221.5 wires `RenderMsg::Frame` up to
/// `seed.webgpu.submit_frame(frame)`; 221.6 wires `RenderMsg::Resize` up to
/// `seed.webgpu.resize(dims)`.
#[cfg_attr(not(windows), allow(dead_code))]
fn render_thread_loop(seed: RenderThreadSeed) {
    // The `ran` counter isn't read here (production has nothing to check it
    // against); `dispatch_loop` is shared with the unit tests below, where
    // it's used to observe that messages were actually processed without
    // needing a real `WebGpuState`/GPU.
    let ran = std::sync::atomic::AtomicUsize::new(0);
    dispatch_loop(&seed.rx, &ran);
}

/// The message-dispatch loop shared by `render_thread_loop` and its unit
/// tests. Kept free of any `WebGpuState`/`Window` dependency (just the
/// `Receiver<RenderMsg>` half of the channel, plus a counter of messages
/// seen) specifically so it - and by extension the real thread lifecycle -
/// can be exercised in a plain `cargo test` run, without needing a GPU
/// adapter to construct a real `RenderThreadSeed`.
fn dispatch_loop(rx: &std::sync::mpsc::Receiver<RenderMsg>, ran: &std::sync::atomic::AtomicUsize) {
    while let Ok(msg) = rx.recv() {
        ran.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match msg {
            RenderMsg::Frame(frame) => {
                // 221.5 will call seed.webgpu.submit_frame(frame) here.
                // For now: just drop it, no GPU calls yet.
                drop(frame);
            }
            RenderMsg::Resize(_) => {
                // 221.6 will handle this.
            }
            RenderMsg::Shutdown => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    /// Exercises the channel mechanics directly (no real OS thread): a
    /// `Shutdown` message stops `dispatch_loop`, and messages before it are
    /// each observed exactly once.
    #[test]
    fn dispatch_loop_stops_on_shutdown() {
        let (tx, rx) = mpsc::channel();
        let ran = AtomicUsize::new(0);

        tx.send(RenderMsg::Resize(::window::Dimensions {
            pixel_width: 100,
            pixel_height: 100,
            dpi: 96,
        }))
        .unwrap();
        tx.send(RenderMsg::Shutdown).unwrap();
        // Anything sent after Shutdown must never be observed, because the
        // loop breaks as soon as it processes the Shutdown message.
        tx.send(RenderMsg::Shutdown).unwrap();

        dispatch_loop(&rx, &ran);

        assert_eq!(ran.load(Ordering::SeqCst), 2);
    }

    /// Confirms `dispatch_loop` also stops when the channel disconnects
    /// (every `Sender` dropped) even if no explicit `Shutdown` was ever
    /// sent - this is the fallback path relied on by
    /// `TermWindow::drop`/`WindowEvent::Destroyed`, which drop the handle
    /// without necessarily guaranteeing the explicit `Shutdown` message is
    /// processed first.
    #[test]
    fn dispatch_loop_stops_on_disconnect() {
        let (tx, rx) = mpsc::channel();
        let ran = AtomicUsize::new(0);
        drop(tx);

        dispatch_loop(&rx, &ran);

        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }

    /// End-to-end thread-lifecycle test: spawns a *real* OS thread running
    /// `dispatch_loop` (not the GPU-touching `render_thread_loop`, since
    /// building a real `RenderThreadSeed` needs a `WebGpuState`, which
    /// needs a GPU adapter that isn't available in CI/unit tests), sends it
    /// a `Shutdown`, and confirms the thread actually exits within a short
    /// timeout. This is the cross-platform-compiling, GPU-free stand-in for
    /// testing `RenderThreadHandle::spawn`'s thread lifecycle; `spawn`
    /// itself is `#[cfg(windows)]`-gated to actually spawn anything, but the
    /// underlying loop/channel mechanics it relies on are exercised here on
    /// every platform.
    #[test]
    fn spawned_thread_exits_after_shutdown() {
        let (tx, rx) = mpsc::channel();
        let ran = std::sync::Arc::new(AtomicUsize::new(0));
        let ran_thread = std::sync::Arc::clone(&ran);

        let join_handle = std::thread::Builder::new()
            .name("render-test".to_string())
            .spawn(move || dispatch_loop(&rx, &ran_thread))
            .expect("spawn test render thread");

        tx.send(RenderMsg::Shutdown).unwrap();

        // Only test code joins here; production code (RenderThreadHandle)
        // never does, for the reasons documented on that struct. Bound the
        // wait so a regression (loop never exiting) fails the test instead
        // of hanging the suite forever.
        let start = std::time::Instant::now();
        loop {
            if join_handle.is_finished() {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "render thread did not exit within timeout after Shutdown"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        join_handle.join().expect("render thread panicked");
    }
}
