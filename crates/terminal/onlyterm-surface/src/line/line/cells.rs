use super::Line;
use crate::alloc::string::ToString;
use crate::line::clusterline::ClusteredLine;
use crate::line::linebits::LineBits;
use crate::line::storage::{CellStorage, VecStorageIter, VisibleCellIter};
use crate::line::CellRef;
use crate::{Change, SequenceNo};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::Range;
use finl_unicode::grapheme_clusters::Graphemes;
use onlyterm_cell::{Cell, CellAttributes};

extern crate alloc;

impl Line {
    /// If we're about to modify a cell obscured by a double-width
    /// character ahead of that cell, we need to nerf that sequence
    /// of cells to avoid partial rendering concerns.
    /// Similarly, when we assign a cell, we need to blank out those
    /// occluded successor cells.
    pub fn set_cell(&mut self, idx: usize, cell: Cell, seqno: SequenceNo) {
        self.set_cell_impl(idx, cell, false, seqno);
    }

    /// Assign a cell using grapheme text with a known width and attributes.
    /// This is a micro-optimization over first constructing a Cell from
    /// the grapheme info. If assigning this particular cell can be optimized
    /// to an append to the interal clustered storage then the cost of
    /// constructing and dropping the Cell can be avoided.
    pub fn set_cell_grapheme(
        &mut self,
        idx: usize,
        text: &str,
        width: usize,
        attr: CellAttributes,
        seqno: SequenceNo,
    ) {
        if attr.hyperlink().is_some() {
            self.bits |= LineBits::HAS_HYPERLINK;
        }

        if let CellStorage::C(cl) = &mut self.cells {
            if idx > cl.len() && text == " " && attr == CellAttributes::blank() {
                // Appending blank beyond end of line; is already
                // implicitly blank
                return;
            }
            if idx == cl.len() {
                let cl = Arc::make_mut(cl);
                cl.append_grapheme(text, width, attr);
                self.invalidate_implicit_hyperlinks(seqno);
                self.invalidate_zones();
                self.update_last_change_seqno(seqno);
                return;
            }
            if idx > cl.len() {
                let cl = Arc::make_mut(cl);
                while cl.len() < idx {
                    // Fill out any implied blanks until we can append
                    // their intended cell content
                    cl.append_grapheme(" ", 1, CellAttributes::blank());
                }
                cl.append_grapheme(text, width, attr);
                self.invalidate_implicit_hyperlinks(seqno);
                self.invalidate_zones();
                self.update_last_change_seqno(seqno);
                return;
            }
        }

        self.set_cell(idx, Cell::new_grapheme_with_width(text, width, attr), seqno);
    }

    /// Writes a run of single-width printable ASCII (0x20..=0x7E) cells
    /// starting at `idx`, all sharing `attr`, in one call. Equivalent to
    /// calling `set_cell_grapheme(idx + i, &text[i..i+1], 1, attr.clone(),
    /// seqno)` for each byte of `text` (ASCII is one byte per cell), but
    /// does at most one storage conversion/resize and one left-edge
    /// nerf-lookback instead of one per cell.
    ///
    /// Right-edge behavior matches the per-cell loop too: if the old
    /// content had a wide character whose leading column falls inside
    /// `[idx, idx + text.len())`, both of its columns get overwritten by
    /// new narrow cells (same as calling `set_cell` on each in turn); if a
    /// wide character's leading column is the last cell *before* the
    /// range, only the lookback at `idx` nerfs it. A wide character whose
    /// leading column is the last cell *overwritten* by this run leaves
    /// its own continuation cell (just past the run) untouched, exactly
    /// like the per-cell loop.
    pub fn set_ascii_run(
        &mut self,
        idx: usize,
        text: &str,
        attr: &CellAttributes,
        seqno: SequenceNo,
    ) {
        debug_assert!(!text.is_empty());
        debug_assert!(
            text.bytes().all(|b| (0x20..=0x7e).contains(&b)),
            "set_ascii_run requires printable ASCII"
        );

        if attr.hyperlink().is_some() {
            self.bits |= LineBits::HAS_HYPERLINK;
        }

        if let CellStorage::C(cl) = &mut self.cells {
            if idx >= cl.len() {
                // Like `set_cell_grapheme`, default blanks past the end are
                // implicit: skip them without touching the line.
                let mut idx = idx;
                let mut text = text;
                if *attr == CellAttributes::blank() {
                    while idx > cl.len() && text.starts_with(' ') {
                        idx += 1;
                        text = &text[1..];
                    }
                    if text.is_empty() {
                        return;
                    }
                }
                let cl = Arc::make_mut(cl);
                while cl.len() < idx {
                    // Fill out any implied blanks until we can append
                    // their intended cell content
                    cl.append_grapheme(" ", 1, CellAttributes::blank());
                }
                cl.append_ascii_run(text, attr.clone());
                self.invalidate_implicit_hyperlinks(seqno);
                self.invalidate_zones();
                self.update_last_change_seqno(seqno);
                return;
            }
            // Interior write into existing clustered content: falls
            // through to the Vec-storage path below, same as
            // `set_cell_grapheme`.
        }

        let end = idx + text.len();
        {
            let cells = self.coerce_vec_storage();
            if end > cells.len() {
                cells.resize_with(end, Cell::blank);
            }
        }
        self.invalidate_grapheme_at_or_before(idx);
        let cells = self.coerce_vec_storage();
        for (i, byte) in text.bytes().enumerate() {
            // `set_cell` (not a raw assignment) so that, like the per-cell
            // loop's `raw_set_cell(.., clear=false)`, any image placement
            // attached to the cell being overwritten is preserved.
            cells.set_cell(idx + i, Cell::new(byte as char, attr.clone()), false);
        }
        self.invalidate_implicit_hyperlinks(seqno);
        self.invalidate_zones();
        self.update_last_change_seqno(seqno);
    }

    pub fn set_cell_clearing_image_placements(
        &mut self,
        idx: usize,
        cell: Cell,
        seqno: SequenceNo,
    ) {
        self.set_cell_impl(idx, cell, true, seqno)
    }

    fn raw_set_cell(&mut self, idx: usize, cell: Cell, clear: bool) {
        let cells = self.coerce_vec_storage();
        cells.set_cell(idx, cell, clear);
    }

    fn set_cell_impl(&mut self, idx: usize, cell: Cell, clear: bool, seqno: SequenceNo) {
        // The .max(1) stuff is here in case we get called with a
        // zero-width cell.  That shouldn't happen: those sequences
        // should get filtered out in the terminal parsing layer,
        // but in case one does sneak through, we need to ensure that
        // we grow the cells array to hold this bogus entry.
        // https://github.com/wezterm/wezterm/issues/768
        let width = cell.width().max(1);

        self.invalidate_implicit_hyperlinks(seqno);
        self.invalidate_zones();
        self.update_last_change_seqno(seqno);
        if cell.attrs().hyperlink().is_some() {
            self.bits |= LineBits::HAS_HYPERLINK;
        }

        if let CellStorage::C(cl) = &mut self.cells {
            if idx > cl.len() && cell == Cell::blank() {
                // Appending blank beyond end of line; is already
                // implicitly blank
                return;
            }
            if idx >= cl.len() {
                let cl = Arc::make_mut(cl);
                while cl.len() < idx {
                    // Fill out any implied blanks until we can append
                    // their intended cell content
                    cl.append_grapheme(" ", 1, CellAttributes::blank());
                }
                cl.append(cell);
                return;
            }
            /*
            log::info!(
                "cannot append {cell:?} to {:?} as idx={idx} and cl.len is {}",
                cl,
                cl.len
            );
            */
        }

        // if the line isn't wide enough, pad it out with the default attributes.
        {
            let cells = self.coerce_vec_storage();
            if idx + width > cells.len() {
                cells.resize_with(idx + width, Cell::blank);
            }
        }

        self.invalidate_grapheme_at_or_before(idx);

        // For double-wide or wider chars, ensure that the cells that
        // are overlapped by this one are blanked out.
        for i in 1..=width.saturating_sub(1) {
            self.raw_set_cell(idx + i, Cell::blank_with_attrs(cell.attrs().clone()), clear);
        }

        self.raw_set_cell(idx, cell, clear);
    }

    /// Place text starting at the specified column index.
    /// Each grapheme of the text run has the same attributes.
    pub fn overlay_text_with_attribute(
        &mut self,
        mut start_idx: usize,
        text: &str,
        attr: CellAttributes,
        seqno: SequenceNo,
    ) {
        for (i, c) in Graphemes::new(text).enumerate() {
            let cell = Cell::new_grapheme(c, attr.clone(), None);
            let width = cell.width();
            self.set_cell(i + start_idx, cell, seqno);

            // Compensate for required spacing/placement of
            // double width characters
            start_idx += width.saturating_sub(1);
        }
    }

    fn invalidate_grapheme_at_or_before(&mut self, idx: usize) {
        // Assumption: that the width of a grapheme is never > 2.
        // This constrains the amount of look-back that we need to do here.
        if idx > 0 {
            let prior = idx - 1;
            let cells = self.coerce_vec_storage();
            let width = cells[prior].width();
            if width > 1 {
                let attrs = cells[prior].attrs().clone();
                for nerf in prior..prior + width {
                    cells[nerf] = Cell::blank_with_attrs(attrs.clone());
                }
            }
        }
    }

    pub fn insert_cell(&mut self, x: usize, cell: Cell, right_margin: usize, seqno: SequenceNo) {
        self.invalidate_implicit_hyperlinks(seqno);

        let cells = self.coerce_vec_storage();
        if right_margin <= cells.len() {
            cells.remove(right_margin - 1);
        }

        if x >= cells.len() {
            cells.resize_with(x, Cell::blank);
        }

        // If we're inserting a wide cell, we should also insert the overlapped cells.
        // We insert them first so that the grapheme winds up left-most.
        let width = cell.width();
        for _ in 1..=width.saturating_sub(1) {
            cells.insert(x, Cell::blank_with_attrs(cell.attrs().clone()));
        }

        cells.insert(x, cell);
        self.update_last_change_seqno(seqno);
        self.invalidate_zones();
    }

    pub fn erase_cell(&mut self, x: usize, seqno: SequenceNo) {
        if x >= self.len() {
            // Already implicitly erased
            return;
        }
        self.invalidate_implicit_hyperlinks(seqno);
        self.invalidate_grapheme_at_or_before(x);
        {
            let cells = self.coerce_vec_storage();
            cells.remove(x);
            cells.push(Cell::default());
        }
        self.update_last_change_seqno(seqno);
        self.invalidate_zones();
    }

    pub fn remove_cell(&mut self, x: usize, seqno: SequenceNo) {
        if x >= self.len() {
            // Already implicitly removed
            return;
        }
        self.invalidate_implicit_hyperlinks(seqno);
        self.invalidate_grapheme_at_or_before(x);
        self.coerce_vec_storage().remove(x);
        self.update_last_change_seqno(seqno);
        self.invalidate_zones();
    }

    pub fn erase_cell_with_margin(
        &mut self,
        x: usize,
        right_margin: usize,
        seqno: SequenceNo,
        blank_attr: CellAttributes,
    ) {
        self.invalidate_implicit_hyperlinks(seqno);
        if x < self.len() {
            self.invalidate_grapheme_at_or_before(x);
            self.coerce_vec_storage().remove(x);
        }
        if right_margin <= self.len() + 1
        /* we just removed one */
        {
            self.coerce_vec_storage()
                .insert(right_margin - 1, Cell::blank_with_attrs(blank_attr));
        }
        self.update_last_change_seqno(seqno);
        self.invalidate_zones();
    }

    pub fn prune_trailing_blanks(&mut self, seqno: SequenceNo) {
        if let CellStorage::C(cl) = &mut self.cells {
            if cl.has_prunable_trailing_blanks() && Arc::make_mut(cl).prune_trailing_blanks() {
                self.update_last_change_seqno(seqno);
                self.invalidate_zones();
            }
            return;
        }

        let def_attr = CellAttributes::blank();
        let cells = self.coerce_vec_storage();
        if let Some(end_idx) = cells
            .iter()
            .rposition(|c| c.str() != " " || c.attrs() != &def_attr)
        {
            cells.resize_with(end_idx + 1, Cell::blank);
            self.update_last_change_seqno(seqno);
            self.invalidate_zones();
        }
    }

    pub fn fill_range(&mut self, cols: Range<usize>, cell: &Cell, seqno: SequenceNo) {
        if self.is_empty() && *cell == Cell::blank() {
            // We would be filling it with blanks only to prune
            // them all away again before we return; NOP
            return;
        }
        let mut already_pruned = false;
        if cols.start < cols.end {
            if cell.width() > 1 {
                // Wide fill cells are not a real-world erase pattern; keep
                // the simple, obviously-correct per-cell path for them.
                for x in cols {
                    self.set_cell_impl(x, cell.clone(), true, seqno);
                }
            } else {
                already_pruned = self.fill_range_narrow(cols.start, cols.end, cell, seqno);
            }
        }
        if !already_pruned {
            self.prune_trailing_blanks(seqno);
        }
    }

    /// Bulk implementation of `fill_range` for a fill cell of width <= 1
    /// (covers EL/ED/ECH, which always fill with a single blank or plain
    /// character). Does at most one storage conversion, and when the
    /// range reaches or extends past the end of a clustered line with a
    /// blank fill cell, does no conversion at all.
    ///
    /// Returns `true` if pruning has already been fully resolved and the
    /// caller must *not* call `prune_trailing_blanks` again: the clustered
    /// blank-fill fast path below replicates a legacy quirk (see
    /// `fill_cluster_to_end`) that a second, storage-agnostic prune pass
    /// would silently undo.
    fn fill_range_narrow(
        &mut self,
        start: usize,
        end: usize,
        cell: &Cell,
        seqno: SequenceNo,
    ) -> bool {
        self.invalidate_implicit_hyperlinks(seqno);
        self.invalidate_zones();
        self.update_last_change_seqno(seqno);
        if cell.attrs().hyperlink().is_some() {
            self.bits |= LineBits::HAS_HYPERLINK;
        }

        if let CellStorage::C(cl) = &mut self.cells {
            if end >= cl.len() {
                return fill_cluster_to_end(Arc::make_mut(cl), start, end, cell);
            }
            // Interior range: falls through to the Vec-storage path below,
            // exactly like the old per-cell loop would once it reached an
            // index inside the existing clustered content.
        }

        self.fill_range_vec_bulk(start, end, cell);
        false
    }

    /// Bulk-fills `[start, end)` in Vec storage: pads once (if needed),
    /// nerfs a wide character straddling `start` once, then does a plain
    /// slice fill. `clear_image_placement` is always true for fill_range's
    /// callers, so (unlike `raw_set_cell`) no per-cell image bookkeeping
    /// is needed.
    fn fill_range_vec_bulk(&mut self, start: usize, end: usize, cell: &Cell) {
        {
            let cells = self.coerce_vec_storage();
            if end > cells.len() {
                cells.resize_with(end, Cell::blank);
            }
        }
        self.invalidate_grapheme_at_or_before(start);
        let cells = self.coerce_vec_storage();
        for c in &mut cells[start..end] {
            *c = cell.clone();
        }
    }

    pub fn len(&self) -> usize {
        match &self.cells {
            CellStorage::V(cells) => cells.len(),
            CellStorage::C(cl) => cl.len(),
        }
    }

    /// Returns true if the line contains no cells.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterates the visible cells, respecting the width of the cell.
    /// For instance, a double-width cell overlaps the following (blank)
    /// cell, so that blank cell is omitted from the iterator results.
    /// The iterator yields (column_index, Cell).  Column index is the
    /// index into Self::cells, and due to the possibility of skipping
    /// the characters that follow wide characters, the column index may
    /// skip some positions.  It is returned as a convenience to the consumer
    /// as using .enumerate() on this iterator wouldn't be as useful.
    pub fn visible_cells<'a>(&'a self) -> impl Iterator<Item = CellRef<'a>> {
        match &self.cells {
            CellStorage::V(cells) => VisibleCellIter::V(VecStorageIter {
                cells: cells.iter(),
                idx: 0,
                skip_width: 0,
            }),
            CellStorage::C(cl) => VisibleCellIter::C(cl.iter()),
        }
    }

    pub fn get_cell(&self, cell_index: usize) -> Option<CellRef<'_>> {
        self.visible_cells()
            .find(|cell| cell.cell_index() == cell_index)
    }

    /// If `column` falls within a multi-column (wide) character, returns the
    /// `[start, start + width)` range of the columns that character occupies.
    /// Returns `None` for single-width cells and for columns at or beyond the
    /// end of the line. This is used to snap mouse/selection coordinates so
    /// they never land in the middle of a wide character.
    pub fn wide_cell_covering(&self, column: usize) -> Option<Range<usize>> {
        for cell in self.visible_cells() {
            let start = cell.cell_index();
            if start > column {
                break;
            }
            let width = cell.width().max(1);
            if column < start + width {
                return if width > 1 {
                    Some(start..start + width)
                } else {
                    None
                };
            }
        }
        None
    }

    /// Given a starting attribute value, produce a series of Change
    /// entries to recreate the current line
    pub fn changes(&self, start_attr: &CellAttributes) -> Vec<Change> {
        let mut result = Vec::new();
        let mut attr = start_attr.clone();
        let mut text_run = String::new();

        for cell in self.visible_cells() {
            if *cell.attrs() == attr {
                text_run.push_str(cell.str());
            } else {
                // flush out the current text run
                if !text_run.is_empty() {
                    result.push(Change::Text(text_run.clone()));
                    text_run.clear();
                }

                attr = cell.attrs().clone();
                result.push(Change::AllAttributes(attr.clone()));
                text_run.push_str(cell.str());
            }
        }

        // flush out any remaining text run
        if !text_run.is_empty() {
            // if this is just spaces then it is likely cheaper
            // to emit ClearToEndOfLine instead.
            if attr
                == CellAttributes::default()
                    .set_background(attr.background())
                    .clone()
            {
                let left = text_run.trim_end_matches(' ').to_string();
                let num_trailing_spaces = text_run.len() - left.len();

                if num_trailing_spaces > 0 {
                    if !left.is_empty() {
                        result.push(Change::Text(left));
                    } else if result.len() == 1 {
                        // if the only queued result prior to clearing
                        // to the end of the line is an attribute change,
                        // we can prune it out and return just the line
                        // clearing operation
                        if let Change::AllAttributes(_) = result[0] {
                            result.clear()
                        }
                    }

                    // Since this function is only called in the full repaint
                    // case, and we always emit a clear screen with the default
                    // background color, we don't need to emit an instruction
                    // to clear the remainder of the line unless it has a different
                    // background color.
                    if attr.background() != Default::default() {
                        result.push(Change::ClearToEndOfLine(attr.background()));
                    }
                } else {
                    result.push(Change::Text(text_run));
                }
            } else {
                result.push(Change::Text(text_run));
            }
        }

        result
    }
}

/// Fills `[start, end)` in a clustered line whose current length is <=
/// `end`, without ever converting to Vec storage. Since everything from
/// `start` onward either already ends at/before `start` or is about to be
/// fully overwritten/erased, this reduces to: nerf a wide character that
/// straddles `start` (matching `invalidate_grapheme_at_or_before`),
/// truncate there, then append the fill cell (skipped entirely when it is
/// the default blank, since a trailing default blank would just be
/// pruned straight back off).
///
/// Returns `true` if the caller must *not* run its own, storage-agnostic
/// `prune_trailing_blanks` afterward: see the `is_blank` branch below.
fn fill_cluster_to_end(cl: &mut ClusteredLine, start: usize, end: usize, cell: &Cell) -> bool {
    let len = cl.len();
    let is_blank = *cell == Cell::blank();

    if start >= len {
        if is_blank {
            // Already implicitly blank beyond the current content; NOP.
            // Storage is untouched, so the generic caller-side prune must
            // still run (it may have pre-existing trailing blanks of its
            // own to clean up).
            return false;
        }
        while cl.len() < start {
            cl.append_grapheme(" ", 1, CellAttributes::blank());
        }
        for _ in start..end {
            cl.append(cell.clone());
        }
        return false;
    }

    let nerf_attrs = if start > 0 && cl.is_double_wide(start - 1) {
        cl.attrs_at(start - 1).cloned()
    } else {
        None
    };

    match nerf_attrs {
        Some(attrs) => {
            cl.truncate(start - 1);
            cl.append(Cell::blank_with_attrs(attrs));
        }
        None => cl.truncate(start),
    }

    if !is_blank {
        for _ in start..end {
            cl.append(cell.clone());
        }
        return false;
    }

    // `start < len` means the old per-cell loop would have hit an
    // in-bounds index and converted to Vec storage for the rest of this
    // call, including the final `prune_trailing_blanks`. Vec storage's
    // version can't fully collapse an all-default-blank line (its
    // `rposition` finds nothing and leaves it at the padded length
    // instead of truncating) -- reproduce that: prune the truncated
    // prefix ourselves, and if that would empty it completely,
    // materialize `end` blank cells instead of collapsing to empty. This
    // result must not be re-pruned by a generic, storage-agnostic pass
    // afterward (which would use the *stronger*, always-fully-collapsing
    // clustered pruning and undo exactly this), so signal that to the
    // caller.
    cl.prune_trailing_blanks();
    if cl.len() == 0 {
        for _ in 0..end {
            cl.append(cell.clone());
        }
    }
    true
}
