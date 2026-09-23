use crate::alloc::borrow::ToOwned;
use crate::change::Change;
use crate::cursor::position::{compute_position_change, Position};
use crate::cursor::{CursorShape, CursorVisibility};
use crate::line::Line;
use crate::SequenceNo;
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use onlyterm_cell::color::ColorAttribute;
use onlyterm_cell::{Cell, CellAttributes};

mod changes;
mod diff;
#[cfg(test)]
mod test;

/// The `Surface` type represents the contents of a terminal screen.
/// It is not directly connected to a terminal device.
/// It consists of a buffer and a log of changes.  You can accumulate
/// updates to the screen by adding instances of the `Change` enum
/// that describe the updates.
///
/// When ready to render the `Surface` to a `Terminal`, you can use
/// the `get_changes` method to return an optimized stream of `Change`s
/// since the last render and then pass it to an instance of `Renderer`.
///
/// `Surface`s can also be composited together; this is useful when
/// building up a UI with layers or widgets: each widget can be its
/// own `Surface` instance and have its content maintained independently
/// from the other widgets on the screen and can then be copied into
/// the target `Surface` buffer for rendering.
///
/// To support more efficient updates in the composite use case, a
/// `draw_from_screen` method is available; the intent is to have one
/// `Surface` be hold the data that was last rendered, and a second `Surface`
/// of the same size that is repeatedly redrawn from the composite
/// of the widgets.  `draw_from_screen` is used to extract the smallest
/// difference between the updated screen and apply those changes to
/// the render target, and then use `get_changes` to render those without
/// repainting the world on each update.
#[derive(Default, Clone)]
pub struct Surface {
    width: usize,
    height: usize,
    lines: Vec<Line>,
    attributes: CellAttributes,
    xpos: usize,
    ypos: usize,
    seqno: SequenceNo,
    changes: Vec<Change>,
    cursor_shape: Option<CursorShape>,
    cursor_visibility: CursorVisibility,
    cursor_color: ColorAttribute,
    title: String,
}

impl Surface {
    /// Create a new Surface with the specified width and height.
    pub fn new(width: usize, height: usize) -> Self {
        let mut scr = Surface {
            width,
            height,
            ..Default::default()
        };
        scr.resize(width, height);
        scr
    }

    /// Returns the (width, height) of the surface
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn cursor_position(&self) -> (usize, usize) {
        (self.xpos, self.ypos)
    }

    pub fn cursor_shape(&self) -> Option<CursorShape> {
        self.cursor_shape
    }

    pub fn cursor_visibility(&self) -> CursorVisibility {
        self.cursor_visibility
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// Resize the Surface to the specified width and height.
    /// If the width and/or height are smaller than previously, the rows and/or
    /// columns are truncated.  If the width and/or height are larger than
    /// previously then an appropriate number of cells are added to the
    /// buffer and filled with default attributes.
    /// The resize event invalidates the change stream, discarding it and
    /// causing a subsequent `get_changes` call to yield a full repaint.
    /// If the cursor position would be outside the bounds of the newly resized
    /// screen, it will be moved to be within the new bounds.
    pub fn resize(&mut self, width: usize, height: usize) {
        // We need to invalidate the change stream prior to this
        // event, so we nominally generate an entry for the resize
        // here.  Since rendering a resize doesn't make sense, we
        // don't record a Change entry.  Instead what we do is
        // increment the sequence number and then flush the whole
        // stream.  The next call to get_changes() will perform a
        // full repaint, and that is what we want.
        // We only do this if we have any changes buffered.
        if !self.changes.is_empty() {
            self.seqno += 1;
            self.changes.clear();
        }

        self.lines
            .resize(height, Line::with_width(width, self.seqno));
        for line in &mut self.lines {
            line.resize(width, self.seqno);
        }
        self.width = width;
        self.height = height;

        // Ensure that the cursor position is well-defined
        self.xpos = compute_position_change(self.xpos, &Position::Relative(0), self.width);
        self.ypos = compute_position_change(self.ypos, &Position::Relative(0), self.height);
    }
}

impl Surface {
    /// Returns the entire contents of the screen as a string.
    /// Only the character data is returned.  The end of each line is
    /// returned as a \n character.
    /// This function exists primarily for testing purposes.
    pub fn screen_chars_to_string(&self) -> String {
        let mut s = String::new();

        for line in &self.lines {
            for cell in line.visible_cells() {
                s.push_str(cell.str());
            }
            s.push('\n');
        }

        s
    }

    /// Returns the cell data for the screen.
    /// This is intended to be used for testing purposes.
    pub fn screen_cells(&mut self) -> Vec<&mut [Cell]> {
        let mut lines = Vec::new();
        for line in &mut self.lines {
            lines.push(line.cells_mut());
        }
        lines
    }

    pub fn screen_lines(&self) -> Vec<Cow<'_, Line>> {
        self.lines.iter().map(Cow::Borrowed).collect()
    }

    /// Returns a stream of changes suitable to update the screen
    /// to match the model.  The input `seq` argument should be 0
    /// on the first call, or in any situation where the screen
    /// contents may have been invalidated, otherwise it should
    /// be set to the `SequenceNo` returned by the most recent call
    /// to `get_changes`.
    /// `get_changes` will use a heuristic to decide on the lower
    /// cost approach to updating the screen and return some sequence
    /// of `Change` entries that will update the display accordingly.
    /// The worst case is that this function will fabricate a sequence
    /// of Change entries to paint the screen from scratch.
    pub fn get_changes(&self, seq: SequenceNo) -> (SequenceNo, Cow<'_, [Change]>) {
        // Do we have continuity in the sequence numbering?
        let first = self.seqno.saturating_sub(self.changes.len());
        if seq == 0 || first > seq || self.seqno == 0 {
            // No, we have folded away some data, we'll need a full paint
            return (self.seqno, Cow::Owned(self.repaint_all()));
        }

        // Approximate cost to render the change screen
        let delta_cost = self.seqno - seq;
        // Approximate cost to repaint from scratch
        let full_cost = self.estimate_full_paint_cost();

        if delta_cost > full_cost {
            (self.seqno, Cow::Owned(self.repaint_all()))
        } else {
            (self.seqno, Cow::Borrowed(&self.changes[seq - first..]))
        }
    }

    pub fn has_changes(&self, seq: SequenceNo) -> bool {
        self.seqno != seq
    }

    pub fn current_seqno(&self) -> SequenceNo {
        self.seqno
    }

    /// After having called `get_changes` and processed the resultant
    /// change stream, the caller can then pass the returned `SequenceNo`
    /// value to this call to prune the list of changes and free up
    /// resources from the change log.
    pub fn flush_changes_older_than(&mut self, seq: SequenceNo) {
        let first = self.seqno.saturating_sub(self.changes.len());
        let idx = seq.saturating_sub(first);
        if idx > self.changes.len() {
            return;
        }
        self.changes = self.changes.split_off(idx);
    }

    /// Without allocating resources, estimate how many Change entries
    /// we would produce in repaint_all for the current state.
    fn estimate_full_paint_cost(&self) -> usize {
        // assume 1 per cell with 20% overhead for attribute changes
        3 + (((self.width * self.height) as f64) * 1.2) as usize
    }
}

impl Surface {
    fn repaint_all(&self) -> Vec<Change> {
        let mut result = vec![
            // Home the cursor and clear the screen to defaults.  Hide the
            // cursor while we're repainting.
            Change::CursorVisibility(CursorVisibility::Hidden),
            Change::ClearScreen(Default::default()),
        ];

        if !self.title.is_empty() {
            result.push(Change::Title(self.title.to_owned()));
        }

        let mut attr = CellAttributes::default();

        let crlf = Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Relative(1),
        };

        // Walk backwards through the lines; the goal is to determine
        // if the screen ends with a number of clear lines that we
        // can coalesce together as a ClearToEndOfScreen op.
        // We track the index (from the end) of the last matching
        // run, together with the color of that run.
        let mut trailing_color = None;
        let mut trailing_idx = None;

        for (idx, line) in self.lines.iter().rev().enumerate() {
            let changes = line.changes(&attr);
            if changes.is_empty() {
                // The line recorded no changes; this means that the line
                // consists of spaces and the default background color
                match trailing_color {
                    Some(other) if other != Default::default() => {
                        // Color doesn't match up, so we have to stop
                        // looking for the ClearToEndOfScreen run here
                        break;
                    }
                    // Color does match
                    Some(_) => continue,
                    // we don't have a run, we should start one
                    None => {
                        trailing_color = Some(Default::default());
                        trailing_idx = Some(idx);
                        continue;
                    }
                }
            } else {
                let last_change = changes.len() - 1;
                match (&changes[last_change], trailing_color) {
                    (Change::ClearToEndOfLine(color), None) => {
                        trailing_color = Some(*color);
                        trailing_idx = Some(idx);
                    }
                    (Change::ClearToEndOfLine(color), Some(other)) => {
                        if other == *color {
                            trailing_idx = Some(idx);
                            continue;
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        }

        for (idx, line) in self.lines.iter().enumerate() {
            match trailing_idx {
                Some(t) if self.height - t == idx => {
                    let color =
                        trailing_color.expect("didn't set trailing_color along with trailing_idx");

                    // The first in the sequence of the ClearToEndOfLine may
                    // be batched up here; let's remove it if that is the case.
                    let last_result = result.len() - 1;
                    match result[last_result] {
                        Change::ClearToEndOfLine(col) if col == color => {
                            result.remove(last_result);
                        }
                        _ => {}
                    }

                    result.push(Change::ClearToEndOfScreen(color));
                    break;
                }
                _ => {}
            }

            let mut changes = line.changes(&attr);

            if idx != 0 {
                // We emit a relative move at the end of each
                // line with the theory that this will translate
                // to a short \r\n sequence rather than the longer
                // absolute cursor positioning sequence
                result.push(crlf.clone());
            }

            result.append(&mut changes);
            if let Some(c) = line.visible_cells().last() {
                attr = c.attrs().clone();
            }
        }

        // Remove any trailing sequence of cursor movements, as we're
        // going to just finish up with an absolute move anyway.
        loop {
            let result_len = result.len();
            if result_len == 0 {
                break;
            }
            match result[result_len - 1] {
                Change::CursorPosition { .. } => {
                    result.remove(result_len - 1);
                }
                _ => break,
            }
        }

        // Place the cursor at its intended position, but only if we moved the
        // cursor.  We don't explicitly track movement but can infer it from the
        // size of the results: results will have an initial ClearScreen entry
        // that homes the cursor and a CursorShape entry that hides the cursor.
        // If the screen is otherwise blank there will be no further entries
        // and we don't need to emit cursor movement.  However, in the
        // optimization passes above, we may have removed some number of
        // movement entries, so let's be sure to check the cursor position to
        // make sure that we don't fail to emit movement.

        let moved_cursor = result.len() != 2;
        if moved_cursor || self.xpos != 0 || self.ypos != 0 {
            result.push(Change::CursorPosition {
                x: Position::Absolute(self.xpos),
                y: Position::Absolute(self.ypos),
            });
        }

        // Set the intended cursor shape.  We hid the cursor at the start
        // of the repaint, so no need to hide it again.
        if self.cursor_visibility != CursorVisibility::Hidden {
            result.push(Change::CursorVisibility(CursorVisibility::Visible));
            if let Some(shape) = self.cursor_shape {
                result.push(Change::CursorShape(shape));
            }
        }

        result
    }
}
