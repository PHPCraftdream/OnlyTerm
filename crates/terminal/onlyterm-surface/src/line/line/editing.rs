use super::Line;
use crate::line::clusterline::ClusteredLine;
use crate::line::linebits::LineBits;
use crate::line::storage::{CellStorage, VecStorage};
use crate::line::CellRef;
use crate::SequenceNo;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use onlyterm_cell::{Cell, CellAttributes};

extern crate alloc;

impl Line {
    pub fn resize_and_clear(
        &mut self,
        width: usize,
        seqno: SequenceNo,
        blank_attr: CellAttributes,
    ) {
        {
            let cells = self.coerce_vec_storage();
            for c in cells.iter_mut() {
                *c = Cell::blank_with_attrs(blank_attr.clone());
            }
            cells.resize_with(width, || Cell::blank_with_attrs(blank_attr.clone()));
            cells.shrink_to_fit();
        }
        self.update_last_change_seqno(seqno);
        self.invalidate_zones();
        self.bits = LineBits::NONE;
    }

    pub fn resize(&mut self, width: usize, seqno: SequenceNo) {
        self.coerce_vec_storage().resize_with(width, Cell::blank);
        self.update_last_change_seqno(seqno);
        self.invalidate_zones();
    }

    /// Wrap the line so that it fits within the provided width.
    /// Returns the list of resultant line(s)
    pub fn wrap(self, width: usize, seqno: SequenceNo) -> Vec<Self> {
        // Every piece is still the same line, so each must keep its bidi
        // settings: a fresh `Line` defaults to bidi *disabled*, which
        // would silently switch right-to-left reordering off for anything
        // rewrapped by a window resize.
        let (bidi_enabled, bidi_direction) = self.bidi_info();
        let mut cells: Vec<CellRef> = self.visible_cells().collect();
        if let Some(end_idx) = cells.iter().rposition(|c| c.str() != " ") {
            cells.truncate(end_idx + 1);

            let mut lines: Vec<Self> = vec![];
            let mut delta = 0;
            for cell in cells {
                let need_new_line = lines
                    .last_mut()
                    .map(|line| line.len() + cell.width() > width)
                    .unwrap_or(true);
                if need_new_line {
                    if let Some(line) = lines.last_mut() {
                        line.set_last_cell_was_wrapped(true, seqno);
                    }
                    let mut line = Line::new(seqno);
                    line.set_bidi_info(bidi_enabled, bidi_direction, seqno);
                    lines.push(line);
                    delta = cell.cell_index();
                }
                let line = lines.last_mut().unwrap();
                line.set_cell_grapheme(
                    cell.cell_index() - delta,
                    cell.str(),
                    cell.width(),
                    (*cell.attrs()).clone(),
                    seqno,
                );
            }

            lines
        } else {
            vec![self]
        }
    }

    fn make_cells(&mut self) {
        let cells = match &self.cells {
            CellStorage::V(_) => return,
            CellStorage::C(cl) => cl.to_cell_vec(),
        };
        // log::info!("make_cells\n{:?}", backtrace::Backtrace::new());
        self.cells = CellStorage::V(Arc::new(VecStorage::new(cells)));
    }

    pub(crate) fn coerce_vec_storage(&mut self) -> &mut VecStorage {
        self.make_cells();

        match &mut self.cells {
            CellStorage::V(c) => Arc::make_mut(c),
            CellStorage::C(_) => unreachable!(),
        }
    }

    /// Adjusts the internal storage so that it occupies less
    /// space. Subsequent mutations will incur some overhead to
    /// re-materialize the storage in a form that is suitable
    /// for mutation.
    pub fn compress_for_scrollback(&mut self) {
        let cv = match &self.cells {
            CellStorage::V(v) => ClusteredLine::from_cell_vec(v.len(), self.visible_cells()),
            CellStorage::C(_) => return,
        };
        self.cells = CellStorage::C(Arc::new(cv));
    }

    pub fn cells_mut(&mut self) -> &mut [Cell] {
        self.coerce_vec_storage().as_mut_slice()
    }

    /// Return true if the line consists solely of whitespace cells
    pub fn is_whitespace(&self) -> bool {
        self.visible_cells().all(|c| c.str() == " ")
    }

    /// Return true if the last cell in the line has the wrapped attribute,
    /// indicating that the following line is logically a part of this one.
    pub fn last_cell_was_wrapped(&self) -> bool {
        use core::sync::atomic::Ordering::Relaxed;
        if self.cached_last_cell_wrapped_seqno.load(Relaxed) == self.seqno {
            return self.cached_last_cell_was_wrapped.load(Relaxed);
        }
        let wrapped = self
            .visible_cells()
            .last()
            .map(|c| c.attrs().wrapped())
            .unwrap_or(false);
        self.cached_last_cell_was_wrapped.store(wrapped, Relaxed);
        self.cached_last_cell_wrapped_seqno
            .store(self.seqno, Relaxed);
        wrapped
    }

    /// Adjust the value of the wrapped attribute on the last cell of this
    /// line.
    pub fn set_last_cell_was_wrapped(&mut self, wrapped: bool, seqno: SequenceNo) {
        use core::sync::atomic::Ordering::Relaxed;
        self.update_last_change_seqno(seqno);
        if let CellStorage::C(cl) = &mut self.cells {
            let cl = Arc::make_mut(cl);
            if cl.len() == 0 {
                // Need to mark that implicit space as wrapped, so
                // explicitly add it
                cl.append(Cell::blank());
            }
            cl.set_last_cell_was_wrapped(wrapped);
            self.cached_last_cell_was_wrapped.store(wrapped, Relaxed);
            self.cached_last_cell_wrapped_seqno
                .store(self.seqno, Relaxed);
            return;
        }

        // Target the same cell that `last_cell_was_wrapped()` reads: the
        // last *visible* cell. `cells.last_mut()` would instead be the
        // padding cell that follows a trailing wide (e.g. CJK) character,
        // which the visible-cell reader skips over, so setting the
        // attribute there would never be observed by the reader.
        let last_visible_index = self.visible_cells().last().map(|c| c.cell_index());
        let cells = self.coerce_vec_storage();
        if let Some(cell) = last_visible_index.and_then(|idx| cells.get_mut(idx)) {
            cell.attrs_mut().set_wrapped(wrapped);
            // Only cache when a cell was actually mutated: an empty line
            // (no last cell to set the attribute on) means `wrapped` was
            // never really applied, so caching it here would make
            // `last_cell_was_wrapped()` return a value that was never true
            // of the line's actual (empty) content.
            self.cached_last_cell_was_wrapped.store(wrapped, Relaxed);
            self.cached_last_cell_wrapped_seqno
                .store(self.seqno, Relaxed);
        }
    }

    /// Concatenate the cells from other with this line, appending them
    /// to this line.
    /// This function is used by rewrapping logic when joining wrapped
    /// lines back together.
    pub fn append_line(&mut self, other: Line, seqno: SequenceNo) {
        match &mut self.cells {
            CellStorage::V(cells) => {
                let cells = Arc::make_mut(cells);
                for cell in other.visible_cells() {
                    cells.push(cell.as_cell());
                    for _ in 1..cell.width() {
                        cells.push(Cell::new(' ', cell.attrs().clone()));
                    }
                }
            }
            CellStorage::C(cl) => {
                let cl = Arc::make_mut(cl);
                for cell in other.visible_cells() {
                    cl.append(cell.as_cell());
                }
            }
        }
        self.update_last_change_seqno(seqno);
        self.invalidate_zones();
        // `update_last_change_seqno` takes `max(self.seqno, seqno)`, and
        // callers that join already-existing lines (e.g.
        // `apply_hyperlink_rules`) pass `self.seqno.max(other.seqno)`,
        // which is a no-op when `self` already has the larger seqno --
        // even though the append always changes the last cell. Relying on
        // the seqno alone would leave a stale cached value in that case,
        // so invalidate directly instead.
        self.cached_last_cell_wrapped_seqno
            .store(usize::MAX, core::sync::atomic::Ordering::Relaxed);
    }

    /// mutable access the cell data, but the caller must take care
    /// to only mutate attributes rather than the cell textual content.
    /// Use set_cell if you need to modify the textual content of the
    /// cell, so that important invariants are upheld.
    pub fn cells_mut_for_attr_changes_only(&mut self) -> &mut [Cell] {
        self.coerce_vec_storage().as_mut_slice()
    }
}
