use super::Line;
use crate::cellcluster::CellCluster;
use crate::line::linebits::LineBits;
use crate::line::storage::{CellStorage, VecStorage};
use crate::line::DoubleClickRange;
use crate::SequenceNo;
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;
use onlyterm_bidi::ParagraphDirectionHint;
#[cfg(feature = "appdata")]
use std::sync::Mutex;

extern crate alloc;

impl Line {
    /// Recompose line into the corresponding utf8 string.
    pub fn as_str(&self) -> Cow<'_, str> {
        match &self.cells {
            CellStorage::V(_) => {
                let mut s = String::new();
                for cell in self.visible_cells() {
                    s.push_str(cell.str());
                }
                Cow::Owned(s)
            }
            CellStorage::C(cl) => Cow::Borrowed(&cl.text),
        }
    }

    pub fn split_off(&mut self, idx: usize, seqno: SequenceNo) -> Self {
        let my_cells = self.coerce_vec_storage();
        // Clamp to avoid out of bounds panic if the line is shorter
        // than the requested split point
        // <https://github.com/wezterm/wezterm/issues/2355>
        let idx = idx.min(my_cells.len());
        let cells = my_cells.split_off(idx);
        // `self` keeps only the first `idx` cells, which changes its own
        // last cell -- but unlike other mutators, this method never called
        // `update_last_change_seqno` on `self` at all, so its wrap cache
        // (and seqno) could go stale silently. Invalidate directly.
        self.cached_last_cell_wrapped_seqno
            .store(usize::MAX, core::sync::atomic::Ordering::Relaxed);
        Self {
            bits: self.bits,
            cells: CellStorage::V(Arc::new(VecStorage::new(cells))),
            seqno,
            zones: Arc::new(vec![]),
            cached_last_cell_was_wrapped: core::sync::atomic::AtomicBool::new(false),
            cached_last_cell_wrapped_seqno: core::sync::atomic::AtomicUsize::new(usize::MAX),
            #[cfg(feature = "appdata")]
            appdata: Mutex::new(None),
        }
    }

    pub fn compute_double_click_range<F: Fn(&str) -> bool>(
        &self,
        click_col: usize,
        is_word: F,
    ) -> DoubleClickRange {
        let len = self.len();

        if click_col >= len {
            return DoubleClickRange::Range(click_col..click_col);
        }

        let mut lower = click_col;
        let mut upper = click_col;

        let cells = self.visible_cells().collect::<Vec<_>>();
        for cell in &cells {
            // Skip cells that end at or before the click. Comparing against the
            // end of the cell (rather than its start) means that clicking on the
            // hidden second column of a wide character still starts the scan at
            // the wide character that covers the click.
            if cell.cell_index() + cell.width().max(1) <= click_col {
                continue;
            }
            if !is_word(cell.str()) {
                break;
            }
            // Advance past the full width of the cell so that the second
            // (and subsequent) columns occupied by a wide character are
            // included in the range; otherwise a trailing wide character
            // would be left half-selected.
            upper = cell.cell_index() + cell.width().max(1);
        }
        for cell in cells.iter().rev() {
            if cell.cell_index() > click_col {
                continue;
            }
            if !is_word(cell.str()) {
                break;
            }
            lower = cell.cell_index();
        }

        if upper > lower
            && upper >= len
            && cells
                .last()
                .map(|cell| cell.attrs().wrapped())
                .unwrap_or(false)
        {
            DoubleClickRange::RangeWithWrap(lower..upper)
        } else {
            DoubleClickRange::Range(lower..upper)
        }
    }

    /// Returns a substring from the line.
    pub fn columns_as_str(&self, range: Range<usize>) -> String {
        let mut s = String::new();
        for c in self.visible_cells() {
            if c.cell_index() < range.start {
                continue;
            }
            if c.cell_index() >= range.end {
                break;
            }
            s.push_str(c.str());
        }
        s
    }

    pub fn columns_as_line(&self, range: Range<usize>) -> Self {
        let mut cells = vec![];
        for c in self.visible_cells() {
            if c.cell_index() < range.start {
                continue;
            }
            if c.cell_index() >= range.end {
                break;
            }
            cells.push(c.as_cell());
        }
        Self {
            bits: LineBits::NONE,
            cells: CellStorage::V(Arc::new(VecStorage::new(cells))),
            seqno: self.current_seqno(),
            zones: Arc::new(vec![]),
            cached_last_cell_was_wrapped: core::sync::atomic::AtomicBool::new(false),
            cached_last_cell_wrapped_seqno: core::sync::atomic::AtomicUsize::new(usize::MAX),
            #[cfg(feature = "appdata")]
            appdata: Mutex::new(None),
        }
    }

    pub fn cluster(&self, bidi_hint: Option<ParagraphDirectionHint>) -> Vec<CellCluster> {
        self.cluster_with_wrap_context(bidi_hint, false)
    }

    /// Same as `cluster`, but `is_wrap_continuation` tells the bidi-aware
    /// Hebrew-phrase reordering (see `CellCluster::make_cluster`) whether
    /// this line is itself the tail half of a previous line that wrapped
    /// (ie: the prior physical row's last cell had the `wrapped` attribute
    /// set). A physical row only ever sees its own cells -- it has no way
    /// to know whether a Hebrew phrase touching its first cell actually
    /// continues a phrase that started on the row before it, or whether a
    /// phrase touching its last cell (`self.last_cell_was_wrapped()`)
    /// continues onto the next row. Reversing a phrase we can't confirm is
    /// complete produces worse results (eg: a bracket ending up on the
    /// wrong side) than just leaving that boundary-touching span
    /// unreversed, so callers that know the wrap topology (the pane
    /// renderer, which sees all on-screen physical rows at once) should
    /// use this instead of `cluster` whenever wrapping is possible.
    pub fn cluster_with_wrap_context(
        &self,
        bidi_hint: Option<ParagraphDirectionHint>,
        is_wrap_continuation: bool,
    ) -> Vec<CellCluster> {
        let continues_next = self.last_cell_was_wrapped();
        CellCluster::make_cluster(
            self.len(),
            self.visible_cells(),
            bidi_hint,
            is_wrap_continuation,
            continues_next,
        )
    }
}
