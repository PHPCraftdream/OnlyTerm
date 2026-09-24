use crate::line::clusterline::ClusteredLine;
use crate::line::linebits::LineBits;
use crate::line::storage::{CellStorage, VecStorage};
use crate::line::ZoneRange;
use crate::{SequenceNo, SEQ_ZERO};
use alloc::sync::Arc;
#[cfg(feature = "appdata")]
use alloc::sync::Weak;
use alloc::vec;
use alloc::vec::Vec;
#[cfg(feature = "appdata")]
use core::any::Any;
use core::hash::Hash;
use finl_unicode::grapheme_clusters::Graphemes;
use onlyterm_cell::{Cell, CellAttributes, UnicodeVersion};
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};
use siphasher::sip128::{Hasher128, SipHasher};
#[cfg(feature = "appdata")]
use std::sync::Mutex;

extern crate alloc;

#[cfg(test)]
#[path = "../tests/cow_test.rs"]
mod cow_test;

#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
pub struct Line {
    pub(crate) cells: CellStorage,
    zones: Arc<Vec<ZoneRange>>,
    seqno: SequenceNo,
    bits: LineBits,
    #[cfg(feature = "appdata")]
    #[cfg_attr(feature = "use_serde", serde(skip))]
    appdata: Mutex<Option<Weak<dyn Any + Send + Sync>>>,
    // Memoizes `last_cell_was_wrapped`'s grapheme-cluster scan (expensive:
    // it's re-run for every visible line on every paint by the
    // wrap-boundary walk in `Screen::for_each_logical_line_in_stable_range_mut`).
    // Validity is tied to `seqno` rather than an explicit dirty flag so that
    // it can't be invalidated from `&self`, and so it can't be missed by a
    // cell-mutating method that forgets to invalidate it explicitly: any
    // method that bumps `seqno` already invalidates it for free, the same
    // contract `shape_hash_for_line`'s cache already relies on.
    #[cfg_attr(feature = "use_serde", serde(skip))]
    cached_last_cell_was_wrapped: core::sync::atomic::AtomicBool,
    // A plain `serde(skip)` would deserialize this to `AtomicUsize::default()`
    // (0), which is a valid seqno (`SEQ_ZERO`) -- if the deserialized line's
    // real `seqno` also happens to be 0, the cache would wrongly read as
    // valid. Use a default that can never equal a real seqno instead.
    #[cfg_attr(
        feature = "use_serde",
        serde(skip, default = "never_valid_cached_seqno")
    )]
    cached_last_cell_wrapped_seqno: core::sync::atomic::AtomicUsize,
}

#[cfg(feature = "use_serde")]
fn never_valid_cached_seqno() -> core::sync::atomic::AtomicUsize {
    core::sync::atomic::AtomicUsize::new(usize::MAX)
}

// Manual impl (rather than `#[derive(Debug)]`) so that `Debug` output is
// identical regardless of whether the `appdata` feature is enabled: the
// field holds a `Weak` reference with no meaningful printable state, and a
// derived impl would make Debug-based snapshots (see line/test.rs) depend
// on which other workspace member happened to pull the feature in, since
// Cargo unifies features workspace-wide.
impl core::fmt::Debug for Line {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Line")
            .field("cells", &self.cells)
            .field("zones", &self.zones)
            .field("seqno", &self.seqno)
            .field("bits", &self.bits)
            .finish()
    }
}

impl Clone for Line {
    fn clone(&self) -> Self {
        Self {
            cells: self.cells.clone(),
            zones: Arc::clone(&self.zones),
            seqno: self.seqno,
            bits: self.bits,
            cached_last_cell_was_wrapped: core::sync::atomic::AtomicBool::new(
                self.cached_last_cell_was_wrapped
                    .load(core::sync::atomic::Ordering::Relaxed),
            ),
            cached_last_cell_wrapped_seqno: core::sync::atomic::AtomicUsize::new(
                self.cached_last_cell_wrapped_seqno
                    .load(core::sync::atomic::Ordering::Relaxed),
            ),
            #[cfg(feature = "appdata")]
            appdata: Mutex::new(self.appdata.lock().unwrap().clone()),
        }
    }
}

impl PartialEq for Line {
    fn eq(&self, other: &Self) -> bool {
        self.seqno == other.seqno && self.bits == other.bits && self.cells == other.cells
    }
}

impl Line {
    pub fn with_width_and_cell(width: usize, cell: Cell, seqno: SequenceNo) -> Self {
        let mut cells = Vec::with_capacity(width);
        cells.resize(width, cell.clone());
        let bits = LineBits::NONE;
        Self {
            bits,
            cells: CellStorage::V(Arc::new(VecStorage::new(cells))),
            seqno,
            zones: Arc::new(vec![]),
            cached_last_cell_was_wrapped: core::sync::atomic::AtomicBool::new(false),
            cached_last_cell_wrapped_seqno: core::sync::atomic::AtomicUsize::new(usize::MAX),
            #[cfg(feature = "appdata")]
            appdata: Mutex::new(None),
        }
    }

    pub fn from_cells(cells: Vec<Cell>, seqno: SequenceNo) -> Self {
        let bits = LineBits::NONE;
        Self {
            bits,
            cells: CellStorage::V(Arc::new(VecStorage::new(cells))),
            seqno,
            zones: Arc::new(vec![]),
            cached_last_cell_was_wrapped: core::sync::atomic::AtomicBool::new(false),
            cached_last_cell_wrapped_seqno: core::sync::atomic::AtomicUsize::new(usize::MAX),
            #[cfg(feature = "appdata")]
            appdata: Mutex::new(None),
        }
    }

    /// Create a new line using cluster storage, optimized for appending
    /// and lower memory utilization.
    /// The line will automatically switch to cell storage when necessary
    /// to apply edits.
    pub fn new(seqno: SequenceNo) -> Self {
        Self {
            bits: LineBits::NONE,
            cells: CellStorage::C(Arc::new(ClusteredLine::new())),
            seqno,
            zones: Arc::new(vec![]),
            cached_last_cell_was_wrapped: core::sync::atomic::AtomicBool::new(false),
            cached_last_cell_wrapped_seqno: core::sync::atomic::AtomicUsize::new(usize::MAX),
            #[cfg(feature = "appdata")]
            appdata: Mutex::new(None),
        }
    }

    /// Computes a hash over the line that will change if the way that
    /// the line contents are shaped would change.
    /// This is independent of the seqno and is based purely on the
    /// content of the line.
    ///
    /// Line doesn't implement Hash in terms of this function as compute_shape_hash
    /// doesn't every possible bit of internal state, and we don't want to
    /// encourage using Line directly as a hash key.
    pub fn compute_shape_hash(&self) -> [u8; 16] {
        let mut hasher = SipHasher::new();
        self.bits.bits().hash(&mut hasher);
        for cell in self.visible_cells() {
            cell.compute_shape_hash(&mut hasher);
        }
        hasher.finish128().as_bytes()
    }

    pub fn with_width(width: usize, seqno: SequenceNo) -> Self {
        let mut cells = Vec::with_capacity(width);
        cells.resize_with(width, Cell::blank);
        let bits = LineBits::NONE;
        Self {
            bits,
            cells: CellStorage::V(Arc::new(VecStorage::new(cells))),
            seqno,
            zones: Arc::new(vec![]),
            cached_last_cell_was_wrapped: core::sync::atomic::AtomicBool::new(false),
            cached_last_cell_wrapped_seqno: core::sync::atomic::AtomicUsize::new(usize::MAX),
            #[cfg(feature = "appdata")]
            appdata: Mutex::new(None),
        }
    }

    pub fn from_text(
        s: &str,
        attrs: &CellAttributes,
        seqno: SequenceNo,
        unicode_version: Option<&UnicodeVersion>,
    ) -> Line {
        let mut cells = Vec::new();

        for sub in Graphemes::new(s) {
            let cell = Cell::new_grapheme(sub, attrs.clone(), unicode_version);
            let width = cell.width();
            cells.push(cell);
            for _ in 1..width {
                cells.push(Cell::new(' ', attrs.clone()));
            }
        }

        Line {
            cells: CellStorage::V(Arc::new(VecStorage::new(cells))),
            bits: LineBits::NONE,
            seqno,
            zones: Arc::new(vec![]),
            cached_last_cell_was_wrapped: core::sync::atomic::AtomicBool::new(false),
            cached_last_cell_wrapped_seqno: core::sync::atomic::AtomicUsize::new(usize::MAX),
            #[cfg(feature = "appdata")]
            appdata: Mutex::new(None),
        }
    }

    pub fn from_text_with_wrapped_last_col(
        s: &str,
        attrs: &CellAttributes,
        seqno: SequenceNo,
    ) -> Line {
        let mut line = Self::from_text(s, attrs, seqno, None);
        // Use the same setter `last_cell_was_wrapped()` is meant to agree
        // with, rather than a raw `cells_mut().last_mut()`: for a string
        // ending in a double-width grapheme, the raw last cell is the
        // padding cell that the visible-cell reader skips over, so this
        // would otherwise silently build a line whose wrap flag is
        // unobservable.
        line.set_last_cell_was_wrapped(true, seqno);
        line
    }

    /// Set arbitrary application specific data for the line.
    /// Only one piece of appdata can be tracked per line,
    /// so this is only suitable for the overall application
    /// and not for use by "middleware" crates.
    /// A Weak reference is stored.
    /// `get_appdata` is used to retrieve a previously stored reference.
    #[cfg(feature = "appdata")]
    pub fn set_appdata<T: Any + Send + Sync>(&self, appdata: Arc<T>) {
        let appdata: Arc<dyn Any + Send + Sync> = appdata;
        self.appdata
            .lock()
            .unwrap()
            .replace(Arc::downgrade(&appdata));
    }

    #[cfg(feature = "appdata")]
    pub fn clear_appdata(&self) {
        self.appdata.lock().unwrap().take();
    }

    /// Retrieve the appdata for the line, if any.
    /// This may return None in the case where the underlying data has
    /// been released: Line only stores a Weak reference to it.
    #[cfg(feature = "appdata")]
    pub fn get_appdata(&self) -> Option<Arc<dyn Any + Send + Sync>> {
        self.appdata
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|data| data.upgrade())
    }

    /// Returns true if the line's last changed seqno is more recent
    /// than the provided seqno parameter
    pub fn changed_since(&self, seqno: SequenceNo) -> bool {
        self.seqno == SEQ_ZERO || self.seqno > seqno
    }

    pub fn current_seqno(&self) -> SequenceNo {
        self.seqno
    }

    /// Annotate the line with the sequence number of a change.
    /// This can be used together with Line::changed_since to
    /// manage caching and rendering
    #[inline]
    pub fn update_last_change_seqno(&mut self, seqno: SequenceNo) {
        self.seqno = self.seqno.max(seqno);
    }

    fn invalidate_zones(&mut self) {
        if !self.zones.is_empty() {
            if let Some(zones) = Arc::get_mut(&mut self.zones) {
                zones.clear();
            } else {
                // This is a cache, not line content. When a snapshot still
                // owns the populated cache, detach with an empty allocation
                // rather than cloning the cache solely to discard it.
                self.zones = Arc::new(vec![]);
            }
        }
    }
}

impl From<&str> for Line {
    fn from(s: &str) -> Line {
        Line::from_text(s, &CellAttributes::default(), SEQ_ZERO, None)
    }
}

mod cells;
mod editing;
mod hyperlinks;
mod state;
mod text;
