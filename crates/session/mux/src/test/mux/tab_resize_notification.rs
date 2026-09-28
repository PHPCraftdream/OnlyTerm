//! Regression coverage for a deadlock hazard from an earlier version of
//! task B3 (lazy per-tab resize).
//!
//! `TabInner::resize` (see `model/tab/splits.rs`) ends with a synchronous
//! `Mux::notify(MuxNotification::TabReflowed(..))`. The GUI's own
//! subscriber (`mux_pane_output_event_callback`) reacts to `TabReflowed`
//! by calling `Mux::window_containing_tab`, which takes a *read* lock on
//! `Mux::windows`. An earlier version of the lazy-resize change called
//! `Tab::resize` from inside `mux::window::Window::set_active_without_saving`,
//! while a caller further up the stack still held that same `windows` map
//! *write*-locked (eg. via `Mux::get_window_mut`, in `apply_dimensions`).
//! parking_lot's `RwLock` is not reentrant, so a write-then-read on the
//! same thread is an unconditional self-deadlock, not merely a race under
//! contention -- the same class of bug as the "wakterm" hazard documented
//! in `domain_detach.rs`.
//!
//! The fix moved the resize entirely out of the mux `Window`/`Tab` model
//! and into the GUI (`TermWindow::sync_active_tab_size`): it fetches the
//! active tab as a plain `Arc<Tab>` via `Mux::get_active_tab_for_window`
//! (whose internal read guard is dropped before the `Arc` is returned) and
//! only *then* calls `Tab::resize`, with no mux `Window` guard held on the
//! calling thread at all. This test pins that property directly: a
//! `TabReflowed` subscriber that itself calls `Mux::window_containing_tab`
//! observes `Mux::windows` as free (`probe_windows_try_write` succeeds) at
//! the moment it fires, and the whole "fetch Arc, drop guard, resize"
//! sequence is additionally run on a background thread with a bounded
//! `recv_timeout`, so that if this ever regresses back to resizing while a
//! `windows` guard is held, this test fails fast with a clear timeout
//! message instead of hanging the suite forever (mirroring
//! `model::tab::test::resize_extreme_shrink_does_not_hang`'s approach to
//! the same class of hazard).
use super::*;
use crate::tab::Tab;
use crate::window::{Window, WindowId};
use crate::{Mux, MuxNotification};
use std::sync::mpsc;

struct FakePane {
    id: PaneId,
}

impl FakePane {
    fn new(id: PaneId) -> Arc<Self> {
        Arc::new(Self { id })
    }
}

impl Pane for FakePane {
    fn pane_id(&self) -> PaneId {
        self.id
    }
    fn is_dead(&self) -> bool {
        false
    }
    fn get_cursor_position(&self) -> StableCursorPosition {
        unreachable!()
    }
    fn get_current_seqno(&self) -> SequenceNo {
        unreachable!()
    }
    fn get_changed_since(
        &self,
        _lines: Range<StableRowIndex>,
        _seqno: SequenceNo,
    ) -> RangeSet<StableRowIndex> {
        unreachable!()
    }
    fn get_lines(&self, _lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        unreachable!()
    }
    fn with_lines_mut(&self, _lines: Range<StableRowIndex>, _with_lines: &mut dyn WithPaneLines) {
        unreachable!()
    }
    fn for_each_logical_line_in_stable_range_mut(
        &self,
        _lines: Range<StableRowIndex>,
        _for_line: &mut dyn ForEachPaneLogicalLine,
    ) {
        unreachable!()
    }
    fn get_logical_lines(&self, _lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
        unreachable!()
    }
    fn get_dimensions(&self) -> RenderableDimensions {
        unreachable!()
    }
    fn get_title(&self) -> String {
        unreachable!()
    }
    fn send_paste(&self, _text: &str) -> anyhow::Result<()> {
        unreachable!()
    }
    fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
        Ok(None)
    }
    fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write> {
        unreachable!()
    }
    fn resize(&self, _size: TerminalSize) -> anyhow::Result<()> {
        Ok(())
    }
    fn key_down(&self, _key: KeyCode, _mods: KeyModifiers) -> anyhow::Result<()> {
        unreachable!()
    }
    fn key_up(&self, _key: KeyCode, _mods: KeyModifiers) -> anyhow::Result<()> {
        unreachable!()
    }
    fn mouse_event(&self, _event: MouseEvent) -> anyhow::Result<()> {
        unreachable!()
    }
    fn palette(&self) -> ColorPalette {
        unreachable!()
    }
    fn domain_id(&self) -> DomainId {
        1
    }
    fn get_current_working_dir(&self, _policy: CachePolicy) -> Option<Url> {
        None
    }
    fn is_mouse_grabbed(&self) -> bool {
        false
    }
    fn is_alt_screen_active(&self) -> bool {
        false
    }
}

#[test]
fn resizing_the_active_tab_after_dropping_the_window_guard_does_not_deadlock() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let size = TerminalSize {
        rows: 24,
        cols: 80,
        pixel_width: 800,
        pixel_height: 600,
        dpi: 96,
    };
    let pane = FakePane::new(1);
    let tab = Arc::new(Tab::new(&size));
    tab.assign_pane(&(Arc::clone(&pane) as Arc<dyn Pane>));

    let mut window = Window::new(None, None);
    let window_id = window.window_id();
    window.push(&tab);
    mux.insert_window_for_test(window_id, window);

    // Records, from inside the subscriber (mirroring the GUI's own
    // `mux_pane_output_event_callback`), whether `Mux::windows` was free
    // at the moment `TabReflowed` fired, and the outcome of the real
    // (blocking) `window_containing_tab` call made right alongside it.
    let windows_lock_was_free: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
    let observed_window_id: Arc<Mutex<Option<WindowId>>> = Arc::new(Mutex::new(None));
    {
        let windows_lock_was_free = Arc::clone(&windows_lock_was_free);
        let observed_window_id = Arc::clone(&observed_window_id);
        let watched_tab_id = tab.tab_id();
        mux.subscribe(move |n| {
            if let MuxNotification::TabReflowed(reflowed_tab_id) = n {
                if reflowed_tab_id == watched_tab_id {
                    let mux = Mux::get();
                    *windows_lock_was_free.lock() = Some(mux.probe_windows_try_write());
                    *observed_window_id.lock() = mux.window_containing_tab(reflowed_tab_id);
                }
            }
            true
        });
    }

    // Run the actual production flow (`TermWindow::sync_active_tab_size`'s
    // "fetch Arc, drop guard, resize" sequence) on a background thread
    // with a bounded wait: if a future change reintroduces resizing while
    // a `Window` guard is held, this hangs forever instead of completing,
    // and the test fails with a clear timeout rather than blocking the
    // suite.
    let (tx, rx) = mpsc::channel();
    let mux_for_thread = Arc::clone(&mux);
    let handle = std::thread::spawn(move || {
        let target = TerminalSize {
            rows: 40,
            cols: 120,
            pixel_width: 1200,
            pixel_height: 800,
            dpi: 96,
        };
        let tab = mux_for_thread
            .get_active_tab_for_window(window_id)
            .expect("active tab");
        // No `Window` guard is held here: `get_active_tab_for_window`'s
        // internal read guard was already dropped when it returned the
        // `Arc<Tab>` above.
        tab.resize(target);
        let _ = tx.send(());
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => {
            handle.join().expect("resize thread panicked");
        }
        Err(_) => {
            panic!(
                "Tab::resize (called after dropping the mux Window guard) did not \
                 complete within 10s -- the TabReflowed subscriber calling \
                 Mux::window_containing_tab likely deadlocked against a still-held \
                 `windows` guard; see this file's module doc"
            );
        }
    }

    assert_eq!(
        *windows_lock_was_free.lock(),
        Some(true),
        "TabReflowed's subscriber observed Mux::windows as write-locked \
         (try_write failed) -- Tab::resize must never be called while any \
         mux Window guard is held on the same thread"
    );
    assert_eq!(
        *observed_window_id.lock(),
        Some(window_id),
        "window_containing_tab should find the resized tab's window"
    );
}
