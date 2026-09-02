//! Regression test for investigation
//! `2026-08-25-render-and-resource-bug-hunt` section 1.3, bug B
//! (ghost-cursor-fix-plan Phase C): `Pane::get_render_snapshot` must
//! return cursor, dimensions and lines that are consistent with the
//! pane's individual getters -- on `LocalPane` they are captured under
//! ONE `terminal.lock()` acquisition, so a paint can no longer combine a
//! cursor position from moment t0 with line contents from t2. This test
//! drives a real `LocalPane` (a real `onlyterm_term::Terminal` behind
//! the exact same `Mutex` used in production) and checks snapshot
//! equivalence with the composed-getters result, including for a
//! viewport range above `physical_top` (where `get_lines` clamps the
//! range and the returned stable row index is the clamped origin).
use crate::localpane::LocalPane;
use crate::pane::Pane;
use onlyterm_term::color::ColorPalette;
use onlyterm_term::{Terminal, TerminalConfiguration, TerminalSize};
use parking_lot::Mutex;
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use std::io::{Read, Result as IoResult, Write};
use std::sync::Arc;

/// A `Child` double that never exits on its own; `LocalPane` only needs
/// something that implements the trait so it can track process state,
/// it is never polled by this test.
#[derive(Debug)]
struct NeverExitChild;

impl Child for NeverExitChild {
    fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> IoResult<ExitStatus> {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

#[derive(Debug, Clone)]
struct NeverExitKiller;
impl ChildKiller for NeverExitKiller {
    fn kill(&mut self) -> IoResult<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(self.clone())
    }
}
impl ChildKiller for NeverExitChild {
    fn kill(&mut self) -> IoResult<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(NeverExitKiller)
    }
}

struct FakeMasterPty {
    size: Mutex<PtySize>,
}

impl MasterPty for FakeMasterPty {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        *self.size.lock() = size;
        Ok(())
    }
    fn get_size(&self) -> anyhow::Result<PtySize> {
        Ok(*self.size.lock())
    }
    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        Ok(Box::new(std::io::empty()))
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
        Ok(Box::new(Vec::new()))
    }
}

#[derive(Debug)]
struct TestConfig;

impl TerminalConfiguration for TestConfig {
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

const ROWS: usize = 6;
const COLS: usize = 40;

fn make_pane() -> Arc<LocalPane> {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let terminal = Terminal::new(
        size,
        Arc::new(TestConfig),
        "OnlyTerm",
        "0.0.0",
        Box::new(Vec::new()),
    );
    let pty = Box::new(FakeMasterPty {
        size: Mutex::new(PtySize {
            rows: ROWS as u16,
            cols: COLS as u16,
            pixel_width: 0,
            pixel_height: 0,
        }),
    });
    let writer = Box::new(Vec::new());
    Arc::new(LocalPane::new(
        1,
        terminal,
        Box::new(NeverExitChild),
        pty,
        writer,
        1,
        "render_snapshot".to_string(),
        None,
    ))
}

#[test]
fn render_snapshot_is_consistent_with_individual_getters() {
    let pane = make_pane();

    // Print some text and move the cursor, so cursor position and line
    // contents are both non-trivial.
    use termwiz::escape::csi::{Cursor, CSI};
    use termwiz::escape::{Action, ControlCode, OneBased};
    // Print some lines (building a little scrollback so a scrolled-back
    // viewport range is meaningful) and then move the cursor, so cursor
    // position and line contents are both non-trivial.
    let mut actions = Vec::new();
    for n in 0..20 {
        actions.push(Action::Print(char::from(
            b"abcdefghijklmnopqrstuvwxyz"[n % 26],
        )));
        actions.push(Action::Control(ControlCode::LineFeed));
    }
    actions.push(Action::CSI(CSI::Cursor(Cursor::Position {
        line: OneBased::new(3),
        col: OneBased::new(2),
    })));
    pane.perform_actions(actions);

    let dims = pane.get_dimensions();
    let top = dims.physical_top;
    let range = top..top + dims.viewport_rows as isize;

    let snapshot = pane.get_render_snapshot(None, &[]);
    assert_eq!(snapshot.cursor, pane.get_cursor_position());
    assert_eq!(snapshot.dims, dims);
    let (stable_top, lines) = pane.get_lines(range.clone());
    assert_eq!(snapshot.stable_top, stable_top);
    assert_eq!(snapshot.lines.len(), lines.len());
    for (a, b) in snapshot.lines.iter().zip(lines.iter()) {
        assert_eq!(a.as_str(), b.as_str());
    }

    // Request a viewport above physical_top (as when scrolled back):
    // the terminal clamps the range, and the snapshot must report the
    // same clamped origin and line contents as `get_lines` for the same
    // requested range.
    let scrolled_top = top - 2;
    let scrolled_range = scrolled_top..scrolled_top + dims.viewport_rows as isize;
    let snapshot = pane.get_render_snapshot(Some(scrolled_top), &[]);
    let (stable_top, lines) = pane.get_lines(scrolled_range);
    assert_eq!(snapshot.stable_top, stable_top);
    assert_eq!(snapshot.lines.len(), lines.len());
    for (a, b) in snapshot.lines.iter().zip(lines.iter()) {
        assert_eq!(a.as_str(), b.as_str());
    }
}
