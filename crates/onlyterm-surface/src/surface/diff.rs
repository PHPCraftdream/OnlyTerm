use crate::alloc::string::ToString;
use crate::change::Change;
use crate::cursor::position::Position;
use crate::line::{CellRef, Line};
use crate::{SequenceNo, Surface};
use alloc::vec::Vec;
use onlyterm_cell::CellAttributes;

#[derive(Default)]
struct DiffState {
    changes: Vec<Change>,
    /// Keep track of the cursor position that the change stream
    /// selects for updates so that we can avoid emitting redundant
    /// position changes.
    cursor: Option<(usize, usize)>,
    /// Similarly, we keep track of the cell attributes that we have
    /// activated for change stream to avoid over-emitting.
    /// Tracking the cursor and attributes in this way helps to coalesce
    /// lines of text into simpler strings.
    attr: Option<CellAttributes>,
}

impl DiffState {
    #[inline]
    fn diff_cells(&mut self, col_num: usize, row_num: usize, cell: CellRef, other_cell: CellRef) {
        if cell.same_contents(&other_cell) {
            return;
        }

        self.set_cell(col_num, row_num, other_cell);
    }

    #[inline]
    fn set_cell(&mut self, col_num: usize, row_num: usize, other_cell: CellRef) {
        self.cursor = match self.cursor.take() {
            Some((cursor_row, cursor_col)) if cursor_row == row_num && cursor_col == col_num => {
                // It is on the current column, so we don't need
                // to explicitly move it.  Move the cursor by the
                // width of the text we're about to add.
                Some((row_num, col_num + other_cell.width()))
            }
            _ => {
                // Need to explicitly move the cursor
                self.changes.push(Change::CursorPosition {
                    y: Position::Absolute(row_num),
                    x: Position::Absolute(col_num),
                });
                // and update the position for next time
                Some((row_num, col_num + other_cell.width()))
            }
        };

        // we could get fancy and try to minimize the update traffic
        // by computing a series of AttributeChange values here.
        // For now, let's just record the new value
        self.attr = match self.attr.take() {
            Some(ref attr) if attr == other_cell.attrs() => {
                // Active attributes match, so we don't need
                // to emit a change for them
                Some(attr.clone())
            }
            _ => {
                // Attributes are different
                self.changes
                    .push(Change::AllAttributes(other_cell.attrs().clone()));
                Some(other_cell.attrs().clone())
            }
        };
        // A little bit of bloat in the code to avoid runs of single
        // character Text entries; just append to the string.
        let result_len = self.changes.len();
        if result_len > 0 && self.changes[result_len - 1].is_text() {
            if let Some(Change::Text(ref mut prefix)) = self.changes.get_mut(result_len - 1) {
                prefix.push_str(other_cell.str());
            }
        } else {
            self.changes
                .push(Change::Text(other_cell.str().to_string()));
        }
    }
}

impl Surface {
    /// Computes the change stream required to make the region within `self`
    /// at coordinates `x`, `y` and size `width`, `height` look like the
    /// same sized region within `other` at coordinates `other_x`, `other_y`.
    ///
    /// `other` and `self` may be the same, causing regions within the same
    /// `Surface` to be differenced; this is used by the `copy_region` method.
    ///
    /// The returned list of `Change`s can be passed to the `add_changes` method
    /// to make the region within self match the region within other.
    #[allow(clippy::too_many_arguments)]
    pub fn diff_region(
        &self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        other: &Surface,
        other_x: usize,
        other_y: usize,
    ) -> Vec<Change> {
        let mut diff_state = DiffState::default();

        for ((row_num, line), other_line) in self
            .lines
            .iter()
            .enumerate()
            .skip(y)
            .take_while(|(row_num, _)| *row_num < y + height)
            .zip(other.lines.iter().skip(other_y))
        {
            diff_line(
                &mut diff_state,
                line,
                row_num,
                other_line,
                x,
                width,
                other_x,
            );
        }

        diff_state.changes
    }

    pub fn diff_lines(&self, other_lines: Vec<&Line>) -> Vec<Change> {
        let mut diff_state = DiffState::default();
        for ((row_num, line), other_line) in self.lines.iter().enumerate().zip(other_lines.iter()) {
            diff_line(&mut diff_state, line, row_num, other_line, 0, line.len(), 0);
        }
        diff_state.changes
    }

    pub fn diff_against_numbered_line(&self, row_num: usize, other_line: &Line) -> Vec<Change> {
        let mut diff_state = DiffState::default();
        if let Some(line) = self.lines.get(row_num) {
            diff_line(&mut diff_state, line, row_num, other_line, 0, line.len(), 0);
        }
        diff_state.changes
    }

    /// Computes the change stream required to make `self` have the same
    /// screen contents as `other`.
    pub fn diff_screens(&self, other: &Surface) -> Vec<Change> {
        self.diff_region(0, 0, self.width, self.height, other, 0, 0)
    }

    /// Draw the contents of `other` into self at the specified coordinates.
    /// The required updates are recorded as Change entries as well as stored
    /// in the screen line/cell data.
    /// Saves the cursor position and attributes that were in effect prior to
    /// calling `draw_from_screen` and restores them after applying the changes
    /// from the other surface.
    pub fn draw_from_screen(&mut self, other: &Surface, x: usize, y: usize) -> SequenceNo {
        let attrs = self.attributes.clone();
        let cursor = (self.xpos, self.ypos);
        let changes = self.diff_region(x, y, other.width, other.height, other, 0, 0);
        let seq = self.add_changes(changes);
        self.xpos = cursor.0;
        self.ypos = cursor.1;
        self.attributes = attrs;
        seq
    }

    /// Copy the contents of the specified region to the same sized
    /// region elsewhere in the screen display.
    /// The regions may overlap.
    /// # Panics
    /// The destination region must be the same size as the source
    /// (which is implied by the function parameters) and must fit
    /// within the width and height of the Surface or this operation
    /// will panic.
    pub fn copy_region(
        &mut self,
        src_x: usize,
        src_y: usize,
        width: usize,
        height: usize,
        dest_x: usize,
        dest_y: usize,
    ) -> SequenceNo {
        let changes = self.diff_region(dest_x, dest_y, width, height, self, src_x, src_y);
        self.add_changes(changes)
    }
}

/// Populate `diff_state` with changes to replace contents of `line` in range [x,x+width)
/// with the contents of `other_line` in range [other_x,other_x+width).
fn diff_line(
    diff_state: &mut DiffState,
    line: &Line,
    row_num: usize,
    other_line: &Line,
    x: usize,
    width: usize,
    other_x: usize,
) {
    let mut cells = line
        .visible_cells()
        .skip_while(|cell| cell.cell_index() < x)
        .take_while(|cell| cell.cell_index() < x + width)
        .peekable();
    let other_cells = other_line
        .visible_cells()
        .skip_while(|cell| cell.cell_index() < other_x)
        .take_while(|cell| cell.cell_index() < other_x + width);

    for other_cell in other_cells {
        let rel_x = other_cell.cell_index() - other_x;
        let mut comparison_cell = None;

        // Advance the `cells` iterator to try to find the visible cell in `line` in the equivalent
        // position to `other_cell`. If there is no visible cell in equivalent position, advance
        // one past and wait for next iteration.
        while let Some(cell) = cells.peek() {
            let cell_rel_x = cell.cell_index() - x;

            if cell_rel_x == rel_x {
                comparison_cell = Some(*cell);
                break;
            } else if cell_rel_x > rel_x {
                break;
            }

            cells.next();
        }

        // If we find a cell in the equivalent position, diff against it. If not, we know
        // there is a multi-cell grapheme in `line` that partially overlaps `other_cell`,
        // so we have to overwrite anyway.
        if let Some(comparison_cell) = comparison_cell {
            diff_state.diff_cells(x + rel_x, row_num, comparison_cell, other_cell);
        } else {
            diff_state.set_cell(x + rel_x, row_num, other_cell);
        }
    }
}
