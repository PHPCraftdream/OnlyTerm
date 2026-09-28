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
    make_pane_with_config(Arc::new(TestConfig))
}

fn make_pane_with_config(config: Arc<dyn TerminalConfiguration>) -> Arc<LocalPane> {
    let size = TerminalSize {
        rows: ROWS,
        cols: COLS,
        pixel_width: COLS * 8,
        pixel_height: ROWS * 16,
        dpi: 0,
    };
    let terminal = Terminal::new(size, config, "OnlyTerm", "0.0.0", Box::new(Vec::new()));
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

    let snapshot = pane.get_render_snapshot(None, &[], 0);
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
    let snapshot = pane.get_render_snapshot(Some(scrolled_top), &[], 0);
    let (stable_top, lines) = pane.get_lines(scrolled_range);
    assert_eq!(snapshot.stable_top, stable_top);
    assert_eq!(snapshot.lines.len(), lines.len());
    for (a, b) in snapshot.lines.iter().zip(lines.iter()) {
        assert_eq!(a.as_str(), b.as_str());
    }
}

/// Regression test for task B2: `LocalPane::get_render_snapshot` must
/// return the palette and the changed-since-`changed_since_seqno` rows
/// under the same single `terminal.lock()` acquisition as
/// dims/cursor/lines, consistent with what calling `palette()` and
/// `get_changed_since()` separately would report.
#[test]
fn render_snapshot_bundles_palette_and_changed_rows_under_one_lock() {
    use termwiz::escape::{Action, ControlCode};

    let pane = make_pane();
    let baseline_seqno = pane.get_current_seqno();

    let mut actions = Vec::new();
    for n in 0..3 {
        actions.extend(format!("line {}", n).chars().map(Action::Print));
        actions.push(Action::Control(ControlCode::CarriageReturn));
        actions.push(Action::Control(ControlCode::LineFeed));
    }
    pane.perform_actions(actions);

    let dims = pane.get_dimensions();
    let range = dims.physical_top..dims.physical_top + dims.viewport_rows as isize;

    let snapshot = pane.get_render_snapshot(None, &[], baseline_seqno);
    assert_eq!(snapshot.cursor, pane.get_cursor_position());
    assert_eq!(snapshot.dims, dims);
    assert_eq!(snapshot.palette, pane.palette());

    let expected_changed = pane.get_changed_since(range, baseline_seqno);
    assert_eq!(snapshot.changed_since, expected_changed);
    assert!(
        !snapshot.changed_since.is_empty(),
        "rows written after baseline_seqno must show up as changed"
    );

    // A snapshot taken with the pane's current (post-write) seqno as the
    // threshold must report no changed rows: nothing has changed "since
    // now".
    let now_seqno = pane.get_current_seqno();
    let quiet = pane.get_render_snapshot(None, &[], now_seqno);
    assert!(quiet.changed_since.is_empty());
}

#[test]
fn render_snapshot_stale_reflow_viewport_does_not_jump_to_oldest_history() {
    use termwiz::escape::{Action, ControlCode};

    let pane = make_pane();
    let mut actions = Vec::new();
    for n in 0..30 {
        let text = format!("{:02}{}", n, "x".repeat(COLS * 2 - 2));
        actions.extend(text.chars().map(Action::Print));
        actions.push(Action::Control(ControlCode::CarriageReturn));
        actions.push(Action::Control(ControlCode::LineFeed));
    }
    actions.extend("prompt> ".chars().map(Action::Print));
    pane.perform_actions(actions);
    let old_dims = pane.get_dimensions();
    let anchor = old_dims.physical_top - 1;
    let before = pane.get_render_snapshot(Some(anchor), &[], 0);
    assert_eq!(before.stable_top, anchor);

    pane.resize(TerminalSize {
        rows: ROWS,
        cols: COLS * 2,
        ..Default::default()
    })
    .unwrap();
    let dims = pane.get_dimensions();
    assert!(anchor >= dims.scrollback_top + dims.scrollback_rows as isize);

    // An obsolete anchor beyond the new end must clamp to the newest page.
    let snapshot = pane.get_render_snapshot(Some(anchor), &[], 0);
    assert_eq!(snapshot.stable_top, dims.physical_top);
    assert_eq!(snapshot.lines.len(), ROWS);
    assert!(snapshot.lines[0].as_str().starts_with("25"));
    assert_eq!(snapshot.lines[ROWS - 1].as_str(), "prompt> ");
    assert_eq!(snapshot.cursor.y - snapshot.stable_top, (ROWS - 1) as isize);
}

#[test]
fn render_snapshot_partly_past_bottom_returns_exactly_one_viewport() {
    use termwiz::escape::{Action, ControlCode};

    let pane = make_pane();
    let mut actions = Vec::new();
    for n in 0..12 {
        actions.extend(format!("line {}", n).chars().map(Action::Print));
        actions.push(Action::Control(ControlCode::CarriageReturn));
        actions.push(Action::Control(ControlCode::LineFeed));
    }
    pane.perform_actions(actions);
    let dims = pane.get_dimensions();
    let request = dims.physical_top + 1;
    assert!(request < dims.scrollback_top + dims.scrollback_rows as isize);

    let snapshot = pane.get_render_snapshot(Some(request), &[], 0);
    assert_eq!(snapshot.stable_top, dims.physical_top);
    assert_eq!(snapshot.lines.len(), ROWS);
    assert_eq!(snapshot.lines[0].as_str(), "line 7");
    assert_eq!(snapshot.cursor.y - snapshot.stable_top, 5);
}

#[test]
fn render_snapshot_after_unobserved_output_keeps_bottom_and_clamps_expired_history() {
    use termwiz::escape::{Action, ControlCode};

    #[derive(Debug)]
    struct SmallHistory;
    impl TerminalConfiguration for SmallHistory {
        fn color_palette(&self) -> ColorPalette {
            ColorPalette::default()
        }

        fn scrollback_size(&self) -> usize {
            20
        }
    }

    let pane = make_pane_with_config(Arc::new(SmallHistory));
    let write_lines = |start, end| {
        let mut actions = Vec::new();
        for n in start..end {
            actions.extend(format!("line {}", n).chars().map(Action::Print));
            actions.push(Action::Control(ControlCode::CarriageReturn));
            actions.push(Action::Control(ControlCode::LineFeed));
        }
        pane.perform_actions(actions);
    };
    write_lines(0, 20);
    let anchor = pane.get_dimensions().physical_top - 1;
    assert_eq!(
        pane.get_render_snapshot(Some(anchor), &[], 0).stable_top,
        anchor
    );

    // No reads, resize, or elapsed-time dependency while history rolls over.
    write_lines(20, 40);
    pane.perform_actions("prompt> ".chars().map(Action::Print).collect());
    let bottom = pane.get_render_snapshot(None, &[], 0);
    assert_eq!(bottom.stable_top, 35);
    assert_eq!(bottom.lines.len(), ROWS);
    assert_eq!(bottom.lines[0].as_str(), "line 35");
    assert_eq!(bottom.lines[ROWS - 1].as_str(), "prompt> ");
    assert_eq!(bottom.cursor.y - bottom.stable_top, 5);

    let history = pane.get_render_snapshot(Some(anchor), &[], 0);
    assert!(anchor < history.dims.scrollback_top);
    assert_eq!(history.stable_top, 15);
    assert_eq!(history.lines.len(), ROWS);
    assert_eq!(history.lines[0].as_str(), "line 15");
}

/// Applying hyperlinks rewrites every row of a logical line with the
/// newest seqno among them; the changed rows must be taken before that, so
/// an untouched first half of a wrapped URL does not read as changed.
#[test]
fn render_snapshot_changed_rows_precede_the_hyperlink_pass() {
    use termwiz::escape::csi::{Cursor, CSI};
    use termwiz::escape::{Action, ControlCode, OneBased};
    use termwiz::hyperlink::Rule;

    let rules = vec![Rule::new(r"\b\w+://(?:[\w.-]+)\.[a-z]{2,15}\S*\b", "$0").unwrap()];
    let pane = make_pane();
    let url = format!("http://example.com/{}", "a".repeat(COLS));
    let mut actions: Vec<Action> = url.chars().map(Action::Print).collect();
    actions.push(Action::Control(ControlCode::CarriageReturn));
    actions.push(Action::Control(ControlCode::LineFeed));
    pane.perform_actions(actions);

    // Scan once so both halves carry the link and the scanned bit.
    let _ = pane.get_render_snapshot(None, &rules, 0);
    let baseline = pane.get_current_seqno();

    // Touch only the second half of the wrapped URL.
    pane.perform_actions(vec![
        Action::CSI(CSI::Cursor(Cursor::Position {
            line: OneBased::new(2),
            col: OneBased::new(COLS as u32 - 2),
        })),
        Action::Print('x'),
    ]);

    let top = pane.get_dimensions().physical_top;
    let snapshot = pane.get_render_snapshot(None, &rules, baseline);
    assert!(snapshot.changed_since.contains(top + 1));
    assert!(
        !snapshot.changed_since.contains(top),
        "the untouched first row must not read as changed"
    );
    assert!(snapshot.lines[0].has_hyperlink());
}
