use super::*;
use ordered_float::NotNan;
use std::rc::Rc;

/// Build a RetainedPaneRows with one retained row per slot, whose
/// `contains_cursor` flags are taken from `flags`.
fn make_rows(viewport_top: StableRowIndex, flags: &[bool]) -> RetainedPaneRows {
    RetainedPaneRows {
        stamp: RetainedStamp {
            config_generation: 0,
            shape_generation: 0,
            quad_generation: 0,
            pixel_width: 0,
            pixel_height: 0,
            cell_height: 0,
            left_pixel_x: NotNan::new(0.0).unwrap(),
            top_pixel_y: NotNan::new(0.0).unwrap(),
            num_rows: flags.len(),
            num_cols: 0,
        },
        viewport_top,
        rows: flags
            .iter()
            .map(|&contains_cursor| {
                Some(RetainedRow {
                    quads: Rc::new(HeapQuadAllocator::default()),
                    expires: None,
                    contains_cursor,
                })
            })
            .collect(),
        resume_row: 0,
    }
}

fn flags_of(rows: &RetainedPaneRows) -> Vec<Option<bool>> {
    rows.rows
        .iter()
        .map(|r| r.as_ref().map(|row| row.contains_cursor))
        .collect()
}

/// Scrolling down by 2 rows moves what was at slot N to slot N-2:
/// content at slot 2 (`true`) lands at slot 0, the bottom two slots
/// are fresh (None, i.e. must-build).
#[test]
fn shift_down_moves_content_to_earlier_slots() {
    let mut rows = make_rows(100, &[true, false, true, false]);
    rows.shift_origin(102);
    assert_eq!(flags_of(&rows), vec![Some(true), Some(false), None, None]);
    assert_eq!(rows.viewport_top, 102);
}

/// Scrolling up by 2 rows moves what was at slot N to slot N+2: the
/// top two slots are fresh, the previously recorded content lands in
/// the bottom half.
#[test]
fn shift_up_moves_content_to_later_slots() {
    let mut rows = make_rows(100, &[true, false, true, false]);
    rows.shift_origin(98);
    assert_eq!(flags_of(&rows), vec![None, None, Some(true), Some(false)]);
    assert_eq!(rows.viewport_top, 98);
}

/// A shift larger than the recorded window leaves no previously
/// recorded row visible: every slot must be None (must-build) and
/// the resume point resets to the top. Same for a large upward shift.
#[test]
fn shift_larger_than_the_window_clears_everything() {
    let mut rows = make_rows(100, &[true, false, true, false]);
    rows.resume_row = 2;
    rows.shift_origin(200);
    assert_eq!(flags_of(&rows), vec![None, None, None, None]);
    assert_eq!(rows.resume_row, 0);

    let mut rows = make_rows(100, &[true, false, true, false]);
    rows.resume_row = 2;
    rows.shift_origin(90);
    assert_eq!(flags_of(&rows), vec![None, None, None, None]);
    assert_eq!(rows.resume_row, 0);
}

/// No scroll: shift_origin must not disturb the recorded rows or the
/// resume point.
#[test]
fn shift_by_zero_is_a_noop() {
    let mut rows = make_rows(100, &[true, false, true, false]);
    rows.resume_row = 3;
    rows.shift_origin(100);
    assert_eq!(
        flags_of(&rows),
        vec![Some(true), Some(false), Some(true), Some(false)]
    );
    assert_eq!(rows.resume_row, 3);
    assert_eq!(rows.viewport_top, 100);
}

/// The cursor flag travels with its row: wherever the cursor-bearing
/// quads land after a shift, they stay marked, so RowSweep's
/// must-build protection keeps tracking them.
#[test]
fn contains_cursor_flag_travels_with_its_row() {
    let mut rows = make_rows(100, &[false, true, false, false]);
    rows.shift_origin(99);
    assert_eq!(
        flags_of(&rows),
        vec![None, Some(false), Some(true), Some(false)]
    );
}

/// The resume point tracks the slots, not the content: it shifts by
/// the same delta as the rows (clamped/saturated at the edges).
#[test]
fn resume_row_shifts_along_with_the_rows() {
    let mut rows = make_rows(100, &[true, false, true, false]);
    rows.resume_row = 3;
    rows.shift_origin(102);
    assert_eq!(rows.resume_row, 1);

    let mut rows = make_rows(100, &[true, false, true, false]);
    rows.resume_row = 0;
    rows.shift_origin(98);
    assert_eq!(rows.resume_row, 2);

    let mut rows = make_rows(100, &[true, false, true, false]);
    rows.resume_row = 2;
    rows.shift_origin(90);
    assert_eq!(rows.resume_row, 0);
}
