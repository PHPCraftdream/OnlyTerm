use super::*;
use crate::renderable::*;
use parking_lot::{MappedMutexGuard, Mutex};
use rangeset::RangeSet;
use std::ops::Range;
use termwiz::surface::SequenceNo;
use url::Url;
use wezterm_term::color::ColorPalette;
use wezterm_term::{KeyCode, KeyModifiers, Line, MouseEvent, StableRowIndex};

// See `crate::test::MUX_TEST_GUARD`: the mux is a process-global
// singleton, so tests that install one via `Mux::set_mux` must run
// serially with every other such test in the crate, not just within
// this module.
use crate::test::MUX_TEST_GUARD;

struct FakePane {
    id: PaneId,
    size: Mutex<TerminalSize>,
}

impl FakePane {
    #[allow(clippy::new_ret_no_self)] // test-only factory deliberately returning Arc<dyn Pane>
    fn new(id: PaneId, size: TerminalSize) -> Arc<dyn Pane> {
        Arc::new(Self {
            id,
            size: Mutex::new(size),
        })
    }
}

impl Pane for FakePane {
    fn pane_id(&self) -> PaneId {
        self.id
    }

    fn get_cursor_position(&self) -> StableCursorPosition {
        unimplemented!();
    }

    fn get_current_seqno(&self) -> SequenceNo {
        unimplemented!();
    }

    fn get_changed_since(
        &self,
        _lines: Range<StableRowIndex>,
        _: SequenceNo,
    ) -> RangeSet<StableRowIndex> {
        unimplemented!();
    }

    fn with_lines_mut(
        &self,
        _stable_range: Range<StableRowIndex>,
        _with_lines: &mut dyn WithPaneLines,
    ) {
        unimplemented!();
    }

    fn for_each_logical_line_in_stable_range_mut(
        &self,
        _lines: Range<StableRowIndex>,
        _for_line: &mut dyn ForEachPaneLogicalLine,
    ) {
        unimplemented!();
    }

    fn get_lines(&self, _lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        unimplemented!();
    }

    fn get_logical_lines(&self, _lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
        unimplemented!();
    }

    fn get_dimensions(&self) -> RenderableDimensions {
        unimplemented!();
    }

    fn get_title(&self) -> String {
        unimplemented!()
    }
    fn send_paste(&self, _text: &str) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
        Ok(None)
    }
    fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write> {
        unimplemented!()
    }
    fn resize(&self, size: TerminalSize) -> anyhow::Result<()> {
        *self.size.lock() = size;
        Ok(())
    }

    fn key_down(&self, _key: KeyCode, _mods: KeyModifiers) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn key_up(&self, _: KeyCode, _: KeyModifiers) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn mouse_event(&self, _event: MouseEvent) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn is_dead(&self) -> bool {
        false
    }
    fn palette(&self) -> ColorPalette {
        unimplemented!()
    }
    fn domain_id(&self) -> DomainId {
        1
    }
    fn is_mouse_grabbed(&self) -> bool {
        false
    }
    fn is_alt_screen_active(&self) -> bool {
        false
    }
    fn get_current_working_dir(&self, _policy: CachePolicy) -> Option<Url> {
        None
    }
}

#[test]
fn tab_splitting() {
    let size = TerminalSize {
        rows: 24,
        cols: 80,
        pixel_width: 800,
        pixel_height: 600,
        dpi: 96,
    };

    let tab = Tab::new(&size);
    tab.assign_pane(&FakePane::new(1, size));

    let panes = tab.iter_panes();
    assert_eq!(1, panes.len());
    assert_eq!(0, panes[0].index);
    assert!(panes[0].is_active);
    assert_eq!(0, panes[0].left);
    assert_eq!(0, panes[0].top);
    assert_eq!(80, panes[0].width);
    assert_eq!(24, panes[0].height);

    assert!(tab
        .compute_split_size(
            1,
            SplitRequest {
                direction: SplitDirection::Horizontal,
                ..Default::default()
            }
        )
        .is_none());

    let horz_size = tab
        .compute_split_size(
            0,
            SplitRequest {
                direction: SplitDirection::Horizontal,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        horz_size,
        SplitDirectionAndSize {
            direction: SplitDirection::Horizontal,
            second: TerminalSize {
                rows: 24,
                cols: 40,
                pixel_width: 400,
                pixel_height: 600,
                dpi: 96,
            },
            first: TerminalSize {
                rows: 24,
                cols: 39,
                pixel_width: 390,
                pixel_height: 600,
                dpi: 96,
            },
        }
    );

    let vert_size = tab
        .compute_split_size(
            0,
            SplitRequest {
                direction: SplitDirection::Vertical,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        vert_size,
        SplitDirectionAndSize {
            direction: SplitDirection::Vertical,
            second: TerminalSize {
                rows: 12,
                cols: 80,
                pixel_width: 800,
                pixel_height: 300,
                dpi: 96,
            },
            first: TerminalSize {
                rows: 11,
                cols: 80,
                pixel_width: 800,
                pixel_height: 275,
                dpi: 96,
            }
        }
    );

    let new_index = tab
        .split_and_insert(
            0,
            SplitRequest {
                direction: SplitDirection::Horizontal,
                ..Default::default()
            },
            FakePane::new(2, horz_size.second),
        )
        .unwrap();
    assert_eq!(new_index, 1);

    let panes = tab.iter_panes();
    assert_eq!(2, panes.len());

    assert_eq!(0, panes[0].index);
    assert!(!panes[0].is_active);
    assert_eq!(0, panes[0].left);
    assert_eq!(0, panes[0].top);
    assert_eq!(39, panes[0].width);
    assert_eq!(24, panes[0].height);
    assert_eq!(390, panes[0].pixel_width);
    assert_eq!(600, panes[0].pixel_height);
    assert_eq!(1, panes[0].pane.pane_id());

    assert_eq!(1, panes[1].index);
    assert!(panes[1].is_active);
    assert_eq!(40, panes[1].left);
    assert_eq!(0, panes[1].top);
    assert_eq!(40, panes[1].width);
    assert_eq!(24, panes[1].height);
    assert_eq!(400, panes[1].pixel_width);
    assert_eq!(600, panes[1].pixel_height);
    assert_eq!(2, panes[1].pane.pane_id());

    let vert_size = tab
        .compute_split_size(
            0,
            SplitRequest {
                direction: SplitDirection::Vertical,
                ..Default::default()
            },
        )
        .unwrap();
    let new_index = tab
        .split_and_insert(
            0,
            SplitRequest {
                direction: SplitDirection::Vertical,
                top_level: false,
                target_is_second: true,
                size: Default::default(),
            },
            FakePane::new(3, vert_size.second),
        )
        .unwrap();
    assert_eq!(new_index, 1);

    let panes = tab.iter_panes();
    assert_eq!(3, panes.len());

    assert_eq!(0, panes[0].index);
    assert!(!panes[0].is_active);
    assert_eq!(0, panes[0].left);
    assert_eq!(0, panes[0].top);
    assert_eq!(39, panes[0].width);
    assert_eq!(11, panes[0].height);
    assert_eq!(390, panes[0].pixel_width);
    assert_eq!(275, panes[0].pixel_height);
    assert_eq!(1, panes[0].pane.pane_id());

    assert_eq!(1, panes[1].index);
    assert!(panes[1].is_active);
    assert_eq!(0, panes[1].left);
    assert_eq!(12, panes[1].top);
    assert_eq!(39, panes[1].width);
    assert_eq!(12, panes[1].height);
    assert_eq!(390, panes[1].pixel_width);
    assert_eq!(300, panes[1].pixel_height);
    assert_eq!(3, panes[1].pane.pane_id());

    assert_eq!(2, panes[2].index);
    assert!(!panes[2].is_active);
    assert_eq!(40, panes[2].left);
    assert_eq!(0, panes[2].top);
    assert_eq!(40, panes[2].width);
    assert_eq!(24, panes[2].height);
    assert_eq!(400, panes[2].pixel_width);
    assert_eq!(600, panes[2].pixel_height);
    assert_eq!(2, panes[2].pane.pane_id());

    tab.resize_split_to(1, 12);
    let panes = tab.iter_panes();
    assert_eq!(39, panes[0].width);
    assert_eq!(12, panes[0].height);
    assert_eq!(390, panes[0].pixel_width);
    assert_eq!(300, panes[0].pixel_height);

    assert_eq!(39, panes[1].width);
    assert_eq!(11, panes[1].height);
    assert_eq!(390, panes[1].pixel_width);
    assert_eq!(275, panes[1].pixel_height);

    assert_eq!(40, panes[2].width);
    assert_eq!(24, panes[2].height);
    assert_eq!(400, panes[2].pixel_width);
    assert_eq!(600, panes[2].pixel_height);
}

#[test]
fn top_level_split_restores_tab_size() {
    // Regression test for upstream wezterm/wezterm#5969 (issues #4984/#4686):
    // a top_level split performed while the tab already holds more than one
    // pane temporarily shrinks the tab to reuse the resize logic, and must
    // restore the original tab size afterwards; otherwise unusable "ghost"
    // space is left inside the tab.
    let size = TerminalSize {
        rows: 24,
        cols: 80,
        pixel_width: 800,
        pixel_height: 600,
        dpi: 96,
    };

    let tab = Tab::new(&size);
    tab.assign_pane(&FakePane::new(1, size));

    // First (non-top-level) split: the tab now holds two leaves, which is
    // the precondition for needs_resize on a subsequent top_level split.
    tab.split_and_insert(
        0,
        SplitRequest {
            direction: SplitDirection::Horizontal,
            target_is_second: true,
            ..Default::default()
        },
        FakePane::new(2, size),
    )
    .unwrap();
    assert_eq!(tab.get_size(), size);
    assert_eq!(2, tab.iter_panes().len());

    // Top-level split: triggers the needs_resize path because the tab has
    // more than one leaf.
    let top_level_split = tab
        .compute_split_size(
            0,
            SplitRequest {
                direction: SplitDirection::Horizontal,
                top_level: true,
                target_is_second: true,
                ..Default::default()
            },
        )
        .unwrap();
    tab.split_and_insert(
        0,
        SplitRequest {
            direction: SplitDirection::Horizontal,
            top_level: true,
            target_is_second: true,
            ..Default::default()
        },
        FakePane::new(3, top_level_split.second),
    )
    .unwrap();

    // The defining assertion of this bug: the tab's own size must be back to
    // the original dimensions, not stuck at the shrunk split_info.first.
    assert_eq!(tab.get_size(), size);

    // The top_level split inserted a new pane at the top of the tree.
    let panes = tab.iter_panes();
    assert_eq!(3, panes.len());
}

#[test]
fn resize_preserves_split_ratio() {
    // Regression test for upstream wezterm/wezterm#5011 ("Relative sizing
    // of panes within a tab do not persist on GUI resize") and #6052
    // ("resizing window does not resize panes proportionally").
    //
    // Before this fix, adjust_x_size/adjust_y_size handed the *entire*
    // size delta from a tab-level resize to one side of a split (or
    // distributed it via a naive alternating +1/-1), so a split that
    // started at ~40/60 would drift towards e.g. 90/10 after a handful
    // of GUI window resizes. The fix preserves the first/second *ratio*
    // (as it was immediately before the resize) instead.
    let size = TerminalSize {
        rows: 24,
        cols: 100,
        pixel_width: 1000,
        pixel_height: 600,
        dpi: 96,
    };

    let tab = Tab::new(&size);
    tab.assign_pane(&FakePane::new(1, size));

    // Split roughly 40/60: first gets ~40%, second gets the remainder.
    let split = tab
        .compute_split_size(
            0,
            SplitRequest {
                direction: SplitDirection::Horizontal,
                target_is_second: false,
                size: SplitSize::Percent(40),
                ..Default::default()
            },
        )
        .unwrap();
    tab.split_and_insert(
        0,
        SplitRequest {
            direction: SplitDirection::Horizontal,
            target_is_second: false,
            size: SplitSize::Percent(40),
            ..Default::default()
        },
        FakePane::new(2, split.second),
    )
    .unwrap();

    let panes = tab.iter_panes();
    let original_first = panes[0].width;
    let original_second = panes[1].width;
    // Sanity check: this really is ~40/60 (39.6% / 59.6%), not 50/50.
    assert_eq!(original_first, 40);
    assert_eq!(original_second, 59);

    // Simulate the user's scenario: repeatedly shrink and grow the GUI
    // window (never touching the split divider directly), ending back
    // at the original width.
    for &cols in &[50, 100, 45, 100, 60, 100] {
        tab.resize(TerminalSize {
            rows: 24,
            cols,
            pixel_width: cols * 10,
            pixel_height: 600,
            dpi: 96,
        });
    }

    let panes = tab.iter_panes();
    let final_first = panes[0].width;
    let final_second = panes[1].width;

    // The ratio must survive: first/second should still be close to the
    // original 40/59 (small drift from integer rounding is expected and
    // fine), not have collapsed towards e.g. 90/10 or 10/90 as it did
    // before this fix.
    assert!(
        (final_first as isize - original_first as isize).abs() <= 3,
        "expected first to stay near {}, got {}",
        original_first,
        final_first
    );
    assert!(
        (final_second as isize - original_second as isize).abs() <= 3,
        "expected second to stay near {}, got {}",
        original_second,
        final_second
    );
}

#[test]
fn resize_extreme_shrink_does_not_hang() {
    // Regression/verification test for a candidate infinite-loop report
    // surfaced while auditing the third-party fork wakamex/wakterm: they
    // claimed adjust_x_size/adjust_y_size could spin forever when a
    // window containing many splits is crushed down to a size smaller
    // than the sum of every leaf's minimum size.
    //
    // Reading the current implementation (post upstream #5011/#6052 fix,
    // which rewrote these functions to preserve split ratios via
    // compute_min_size-clamped proportional distribution) shows this
    // cannot happen structurally: adjust_x_size/adjust_y_size are a
    // single recursive descent that visits each tree node exactly once
    // (one call into `left`, one call into `right`, no loop/while/retry
    // anywhere in either function), so runtime is strictly bounded by
    // the number of splits regardless of how extreme the requested
    // shrink is. This test builds a deep chain of splits and drives a
    // sequence of extreme resizes (down to 1x1 and back up) to
    // demonstrate that in practice, guarded by a background-thread
    // timeout so the test itself fails fast instead of hanging forever
    // if this analysis is ever invalidated by a future change.
    let _guard = MUX_TEST_GUARD.lock();
    let size = TerminalSize {
        rows: 200,
        cols: 200,
        pixel_width: 2000,
        pixel_height: 2000,
        dpi: 96,
    };

    let tab = Tab::new(&size);
    tab.assign_pane(&FakePane::new(1, size));

    // Build a deep chain of 20 nested splits, alternating direction, so
    // compute_min_size has to recurse through 20 levels and every
    // resize touches every node in the tree. Each split peels off just
    // 1 cell (plus a 1-cell divider) for the new pane -- using the
    // default 50% split here would halve the remaining space on every
    // step and exhaust the 200-cell budget well before 20 splits.
    const NUM_SPLITS: usize = 20;
    for i in 0..NUM_SPLITS {
        let direction = if i % 2 == 0 {
            SplitDirection::Horizontal
        } else {
            SplitDirection::Vertical
        };
        let request = SplitRequest {
            direction,
            target_is_second: true,
            size: SplitSize::Cells(1),
            ..Default::default()
        };
        let split = tab.compute_split_size(0, request).unwrap();
        tab.split_and_insert(0, request, FakePane::new(2 + i, split.second))
            .unwrap();
    }
    assert_eq!(tab.iter_panes().len(), NUM_SPLITS + 1);

    // Run the extreme resize sequence on a background thread and join
    // with a generous but finite timeout: if adjust_x_size/adjust_y_size
    // ever regress into a genuine infinite loop, this test fails with a
    // clear timeout message instead of hanging the test suite forever.
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        // Crush the window down to 1x1 -- far smaller than the sum of
        // the minimum sizes of the 21 leaves in this tree -- and then
        // grow it back, repeatedly, to also exercise the shrink/grow
        // boundary that #5011/#6052 cared about.
        for &(rows, cols) in &[
            (1usize, 1usize),
            (200, 200),
            (1, 200),
            (200, 1),
            (1, 1),
            (5, 5),
            (200, 200),
        ] {
            tab.resize(TerminalSize {
                rows,
                cols,
                pixel_width: cols * 10,
                pixel_height: rows * 10,
                dpi: 96,
            });
        }
        let _ = tx.send(());
    });

    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(()) => {
            handle.join().expect("resize thread panicked");
        }
        Err(_) => {
            panic!(
                "adjust_x_size/adjust_y_size did not complete within 10s under \
                     extreme shrink of a {}-split tree -- possible infinite loop",
                NUM_SPLITS
            );
        }
    }
}

#[test]
fn set_active_pane_can_suppress_mux_notification() {
    let _guard = MUX_TEST_GUARD.lock();
    let size = TerminalSize {
        rows: 24,
        cols: 80,
        pixel_width: 800,
        pixel_height: 600,
        dpi: 96,
    };

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    // Record every PaneFocused the mux broadcasts. `recorded` is the clone
    // moved into the subscriber; `notified_panes` is read by the assertion.
    let notified_panes = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&notified_panes);
    mux.subscribe(move |notification| {
        if let MuxNotification::PaneFocused(pane_id) = notification {
            recorded.lock().push(pane_id);
        }
        true
    });

    // Build a tab with two side-by-side panes (pane 2 becomes active on split).
    let tab = Tab::new(&size);
    let pane_1 = FakePane::new(1, size);
    tab.assign_pane(&pane_1);
    let pane_2 = FakePane::new(2, size);
    let split = tab
        .compute_split_size(
            0,
            SplitRequest {
                direction: SplitDirection::Horizontal,
                ..Default::default()
            },
        )
        .unwrap();
    tab.split_and_insert(0, SplitRequest::default(), Arc::clone(&pane_2))
        .unwrap();
    pane_1.resize(split.first).unwrap();

    // Act: switch the active pane back to pane 1, requesting NO mux notification.
    tab.set_active_pane_with_notify(&pane_1, NotifyMux::No);

    // Assert: the active pane changed, but not a single PaneFocused was emitted.
    assert_eq!(Vec::<PaneId>::new(), *notified_panes.lock());
    assert_eq!(1, tab.get_active_pane().unwrap().pane_id());

    Mux::shutdown();
}

#[test]
fn set_active_pane_notifies_mux_by_default() {
    let _guard = MUX_TEST_GUARD.lock();
    let size = TerminalSize {
        rows: 24,
        cols: 80,
        pixel_width: 800,
        pixel_height: 600,
        dpi: 96,
    };

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    // Record every PaneFocused the mux broadcasts. `recorded` is the clone
    // moved into the subscriber; `notified_panes` is read by the assertion.
    let notified_panes = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&notified_panes);
    mux.subscribe(move |notification| {
        if let MuxNotification::PaneFocused(pane_id) = notification {
            recorded.lock().push(pane_id);
        }
        true
    });

    // Build a tab with two side-by-side panes (pane 2 becomes active on split).
    let tab = Tab::new(&size);
    let pane_1 = FakePane::new(1, size);
    tab.assign_pane(&pane_1);
    let pane_2 = FakePane::new(2, size);
    let split = tab
        .compute_split_size(
            0,
            SplitRequest {
                direction: SplitDirection::Horizontal,
                ..Default::default()
            },
        )
        .unwrap();
    tab.split_and_insert(0, SplitRequest::default(), Arc::clone(&pane_2))
        .unwrap();
    pane_1.resize(split.first).unwrap();

    // Act: switch the active pane back to pane 1 via the default helper,
    // which requests a mux notification.
    tab.set_active_pane(&pane_1);

    // Assert: the active pane changed and exactly one PaneFocused was emitted.
    assert_eq!(vec![1], *notified_panes.lock());
    assert_eq!(1, tab.get_active_pane().unwrap().pane_id());

    Mux::shutdown();
}

#[allow(clippy::extra_unused_type_parameters)] // T is a compile-time Send+Sync assertion via is_send_and_sync::<Tab>(); removing it breaks call sites
fn is_send_and_sync<T: Send + Sync>() -> bool {
    true
}

#[test]
fn tab_is_send_and_sync() {
    assert!(is_send_and_sync::<Tab>());
}
