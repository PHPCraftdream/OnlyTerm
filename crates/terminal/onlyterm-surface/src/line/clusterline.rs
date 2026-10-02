use crate::line::CellRef;
use core::convert::TryInto;
use core::num::NonZeroU8;
use finl_unicode::grapheme_clusters::Graphemes;
use fixedbitset::FixedBitSet;
use onlyterm_cell::{Cell, CellAttributes};
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

extern crate alloc;
use crate::alloc::string::ToString;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq)]
struct Cluster {
    cell_width: u16,
    attrs: CellAttributes,
}

/// Stores line data as a contiguous string and a series of
/// clusters of attribute data describing attributed ranges
/// within the line
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClusteredLine {
    pub text: String,
    #[cfg_attr(
        feature = "use_serde",
        serde(
            deserialize_with = "deserialize_bitset",
            serialize_with = "serialize_bitset"
        )
    )]
    is_double_wide: Option<Box<FixedBitSet>>,
    clusters: Vec<Cluster>,
    /// Length, measured in cells
    len: u32,
    last_cell_width: Option<NonZeroU8>,
}

#[cfg(feature = "use_serde")]
fn deserialize_bitset<'de, D>(deserializer: D) -> Result<Option<Box<FixedBitSet>>, D::Error>
where
    D: Deserializer<'de>,
{
    let wide_indices = <Vec<usize>>::deserialize(deserializer)?;
    if wide_indices.is_empty() {
        Ok(None)
    } else {
        let max_idx = wide_indices.iter().max().unwrap_or(&1);
        let mut bitset = FixedBitSet::with_capacity(max_idx + 1);
        for idx in wide_indices {
            bitset.set(idx, true);
        }
        Ok(Some(Box::new(bitset)))
    }
}

/// Serialize the bitset as a vector of the indices of just the 1 bits;
/// the thesis is that most of the cells on a given line are single width.
/// That may not be strictly true for users that heavily use asian scripts,
/// but we'll start with this and see if we need to improve it.
#[cfg(feature = "use_serde")]
fn serialize_bitset<S>(value: &Option<Box<FixedBitSet>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut wide_indices: Vec<usize> = vec![];
    if let Some(bits) = value {
        for idx in bits.ones() {
            wide_indices.push(idx);
        }
    }
    wide_indices.serialize(serializer)
}

impl ClusteredLine {
    /// `text` starts with no reservation: most `Line::new()` calls create
    /// rows that are never printed into (e.g. every blank row below the
    /// cursor on `Screen::new`/resize, or a row that gets scrolled off
    /// again before anything is written to it), so an eager reservation
    /// would be wasted for those. A row that *does* get appended to pays
    /// for exactly one allocation sized to what it actually needed, via
    /// `String::push_str`'s own amortized growth -- no worse than before
    /// for the common case, and free for the blank case.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            is_double_wide: None,
            clusters: vec![],
            len: 0,
            last_cell_width: None,
        }
    }

    /// Resets this line back to the same observable state as `new()`
    /// (empty text, no clusters, no double-wide bits, zero length), but
    /// keeps the already-allocated `text`/`clusters` capacity so the
    /// caller can recycle this storage for a future blank line instead of
    /// allocating a fresh one. Only safe to call on storage that isn't
    /// shared (see `Line::recycle_as_blank`, the only caller).
    pub(crate) fn clear_in_place(&mut self) {
        self.text.clear();
        self.clusters.clear();
        self.is_double_wide = None;
        self.len = 0;
        self.last_cell_width = None;
    }

    pub fn to_cell_vec(&self) -> Vec<Cell> {
        let mut cells = vec![];

        for c in self.iter() {
            cells.push(c.as_cell());
            for _ in 1..c.width() {
                cells.push(Cell::blank_with_attrs(c.attrs().clone()));
            }
        }

        cells
    }

    pub fn from_cell_vec<'a>(hint: usize, iter: impl Iterator<Item = CellRef<'a>>) -> Self {
        let mut last_cluster: Option<Cluster> = None;
        let mut is_double_wide = FixedBitSet::with_capacity(hint);
        let mut text = String::new();
        let mut clusters = vec![];
        let mut any_double = false;
        let mut len = 0;
        let mut last_cell_width = None;

        for cell in iter {
            len += cell.width();
            // Track the *actual* width of whichever cell turns out to be
            // last, not a hardcoded 1: `set_last_cell_was_wrapped` uses
            // this to know how many trailing columns the final cluster
            // needs to be split into, and a wrong (too-narrow) width there
            // corrupts the cluster's own column bookkeeping for a line
            // ending in a double-width (e.g. CJK) grapheme, not just the
            // wrapped attribute.
            last_cell_width = NonZeroU8::new(cell.width().max(1) as u8);

            if cell.width() > 1 {
                any_double = true;
                is_double_wide.set(cell.cell_index(), true);
            }

            text.push_str(cell.str());

            last_cluster = match last_cluster.take() {
                None => Some(Cluster {
                    cell_width: cell.width() as u16,
                    attrs: cell.attrs().clone(),
                }),
                Some(cluster) if cluster.attrs != *cell.attrs() => {
                    clusters.push(cluster);
                    Some(Cluster {
                        cell_width: cell.width() as u16,
                        attrs: cell.attrs().clone(),
                    })
                }
                Some(mut cluster) => {
                    cluster.cell_width += cell.width() as u16;
                    Some(cluster)
                }
            };
        }

        if let Some(cluster) = last_cluster.take() {
            clusters.push(cluster);
        }

        Self {
            text,
            is_double_wide: if any_double {
                Some(Box::new(is_double_wide))
            } else {
                None
            },
            clusters,
            len: len.try_into().unwrap(),
            last_cell_width,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub(crate) fn is_double_wide(&self, cell_index: usize) -> bool {
        match &self.is_double_wide {
            Some(bitset) => bitset.contains(cell_index),
            None => false,
        }
    }

    /// Attributes of the cluster covering `cell_index`, if any. Cost is
    /// proportional to the number of attribute runs scanned, not to
    /// `cell_index` itself.
    pub(crate) fn attrs_at(&self, cell_index: usize) -> Option<&CellAttributes> {
        let mut pos = 0usize;
        for cluster in &self.clusters {
            let end = pos + cluster.cell_width as usize;
            if cell_index < end {
                return Some(&cluster.attrs);
            }
            pos = end;
        }
        None
    }

    /// Removes cells from `new_len` onward; `new_len` must be a cell
    /// boundary. Segments the text the same way `iter` does, so the cut
    /// always agrees with the cells the rest of the line sees.
    pub(crate) fn truncate(&mut self, new_len: usize) {
        let len = self.len();
        if new_len >= len {
            return;
        }
        if new_len == 0 {
            *self = ClusteredLine::new();
            return;
        }

        let mut cell_index = 0;
        let mut byte_cut = 0;
        while cell_index < new_len && byte_cut < self.text.len() {
            let grapheme = next_grapheme_at(&self.text, byte_cut);
            cell_index += if self.is_double_wide(cell_index) {
                2
            } else {
                1
            };
            byte_cut += grapheme.len();
        }
        debug_assert_eq!(cell_index, new_len, "truncate must cut on a cell boundary");
        self.text.truncate(byte_cut);

        if let Some(bitset) = self.is_double_wide.as_mut() {
            if new_len < bitset.len() {
                bitset.set_range(new_len.., false);
            }
        }
        // The new last cell is wide if a wide cell starts right before it.
        let last_width = if new_len >= 2 && self.is_double_wide(new_len - 2) {
            2
        } else {
            1
        };

        // Trim clusters from the tail; cost is proportional to the
        // number of attribute runs touched by the removed range.
        let mut remaining = len - new_len;
        while remaining > 0 {
            let last = self
                .clusters
                .last_mut()
                .expect("a cluster exists while cells remain");
            let cw = last.cell_width as usize;
            if cw <= remaining {
                remaining -= cw;
                self.clusters.pop();
            } else {
                last.cell_width -= remaining as u16;
                remaining = 0;
            }
        }

        self.len = new_len as u32;
        self.last_cell_width = NonZeroU8::new(last_width as u8);
    }

    pub fn iter(&self) -> ClusterLineCellIter<'_> {
        let mut clusters = self.clusters.iter();
        let cluster = clusters.next();
        ClusterLineCellIter {
            text: &self.text,
            pos: 0,
            clusters,
            cluster,
            idx: 0,
            cluster_total: 0,
            line: self,
        }
    }

    pub fn append_grapheme(&mut self, text: &str, cell_width: usize, attrs: CellAttributes) {
        let cell_width = cell_width as u16;
        let new_cluster = match self.clusters.last() {
            Some(cluster) => {
                if cluster.attrs != attrs {
                    true
                } else {
                    // If we overflow the max length of a run,
                    // then we need a new cluster
                    let (_, did_overflow) = cluster.cell_width.overflowing_add(cell_width);
                    did_overflow
                }
            }
            None => true,
        };
        let new_cell_index = self.len as usize;
        if new_cluster {
            self.clusters.push(Cluster { attrs, cell_width });
        } else if let Some(cluster) = self.clusters.last_mut() {
            cluster.cell_width += cell_width;
        }
        self.text.push_str(text);

        if cell_width > 1 {
            let bitset = match self.is_double_wide.take() {
                Some(mut bitset) => {
                    bitset.grow(new_cell_index + 1);
                    bitset.set(new_cell_index, true);
                    bitset
                }
                None => {
                    let mut bitset = FixedBitSet::with_capacity(new_cell_index + 1);
                    bitset.set(new_cell_index, true);
                    Box::new(bitset)
                }
            };
            self.is_double_wide.replace(bitset);
        }
        self.last_cell_width = NonZeroU8::new(cell_width as u8);
        self.len += cell_width as u32;
    }

    pub fn append(&mut self, cell: Cell) {
        let cell_width = cell.width() as u16;
        let new_cluster = match self.clusters.last() {
            Some(cluster) => {
                if cluster.attrs != *cell.attrs() {
                    true
                } else {
                    // If we overflow the max length of a run,
                    // then we need a new cluster
                    let (_, did_overflow) = cluster.cell_width.overflowing_add(cell_width);
                    did_overflow
                }
            }
            None => true,
        };
        let new_cell_index = self.len as usize;
        if new_cluster {
            self.clusters.push(Cluster {
                attrs: (*cell.attrs()).clone(),
                cell_width,
            });
        } else if let Some(cluster) = self.clusters.last_mut() {
            cluster.cell_width += cell_width;
        }
        self.text.push_str(cell.str());

        if cell_width > 1 {
            let bitset = match self.is_double_wide.take() {
                Some(mut bitset) => {
                    bitset.grow(new_cell_index + 1);
                    bitset.set(new_cell_index, true);
                    bitset
                }
                None => {
                    let mut bitset = FixedBitSet::with_capacity(new_cell_index + 1);
                    bitset.set(new_cell_index, true);
                    Box::new(bitset)
                }
            };
            self.is_double_wide.replace(bitset);
        }
        self.last_cell_width = NonZeroU8::new(cell_width as u8);
        self.len += cell_width as u32;
    }

    /// Appends a run of single-width printable ASCII (0x20..=0x7E) cells
    /// that all share `attrs`, in one pass. Equivalent to calling
    /// `append_grapheme(&text[i..i+1], 1, attrs.clone())` for each byte of
    /// `text`, but avoids the per-byte cluster/bitset bookkeeping: since
    /// every cell is single-width, `is_double_wide` never needs updating,
    /// and `text.len()` (bytes) equals the number of cells added (ASCII is
    /// one byte per char).
    pub fn append_ascii_run(&mut self, text: &str, attrs: CellAttributes) {
        debug_assert!(
            text.bytes().all(|b| (0x20..=0x7e).contains(&b)),
            "append_ascii_run requires printable ASCII"
        );
        if text.is_empty() {
            return;
        }

        let mut remaining = text;
        while !remaining.is_empty() {
            let extend_last = matches!(
                self.clusters.last(),
                Some(c) if c.attrs == attrs && (c.cell_width as usize) < u16::MAX as usize
            );
            let cap = if extend_last {
                u16::MAX as usize - self.clusters.last().unwrap().cell_width as usize
            } else {
                u16::MAX as usize
            };
            let take = remaining.len().min(cap);
            let (piece, rest) = remaining.split_at(take);
            if extend_last {
                self.clusters.last_mut().unwrap().cell_width += take as u16;
            } else {
                self.clusters.push(Cluster {
                    attrs: attrs.clone(),
                    cell_width: take as u16,
                });
            }
            self.text.push_str(piece);
            remaining = rest;
        }

        self.len += text.len() as u32;
        self.last_cell_width = NonZeroU8::new(1);
    }

    /// Appends a run of single-cell characters (printable ASCII and/or
    /// the term crate's `is_narrow_table_char` codepoints; chars ==
    /// cells) that all share `attrs`, in one pass. Equivalent to calling
    /// `append_grapheme(&char_i, 1, attrs.clone())` for each char of
    /// `text` (cluster merge, `len`, `last_cell_width`, no wide bits),
    /// but avoids the per-char cluster/bitset bookkeeping: since every
    /// cell is single-width, `is_double_wide` never needs updating, and
    /// the number of cells added is `text.chars().count()`.
    pub fn append_narrow_run(&mut self, text: &str, attrs: CellAttributes) {
        debug_assert!(!text.is_empty());
        if text.is_empty() {
            return;
        }

        let mut remaining = text;
        while !remaining.is_empty() {
            let extend_last = matches!(
                self.clusters.last(),
                Some(c) if c.attrs == attrs && (c.cell_width as usize) < u16::MAX as usize
            );
            let cap = if extend_last {
                u16::MAX as usize - self.clusters.last().unwrap().cell_width as usize
            } else {
                u16::MAX as usize
            };
            // Byte offset of the first `cap` chars of `remaining` (all of
            // it if it has no more than `cap` chars).
            let mut take_bytes = remaining.len();
            let mut left = cap;
            for (i, _) in remaining.char_indices() {
                if left == 0 {
                    take_bytes = i;
                    break;
                }
                left -= 1;
            }
            let (piece, rest) = remaining.split_at(take_bytes);
            let piece_cells = piece.chars().count();
            debug_assert!(piece_cells <= cap);
            if extend_last {
                self.clusters.last_mut().unwrap().cell_width += piece_cells as u16;
            } else {
                self.clusters.push(Cluster {
                    attrs: attrs.clone(),
                    cell_width: piece_cells as u16,
                });
            }
            self.text.push_str(piece);
            remaining = rest;
        }

        self.len += text.chars().count() as u32;
        self.last_cell_width = NonZeroU8::new(1);
    }

    pub fn prune_trailing_blanks(&mut self) -> bool {
        let num_spaces = self.text.chars().rev().take_while(|&c| c == ' ').count();
        if num_spaces == 0 {
            return false;
        }

        let blank = CellAttributes::blank();
        let mut pruned = false;
        for _ in 0..num_spaces {
            let mut need_pop = false;
            if let Some(cluster) = self.clusters.last_mut() {
                if cluster.attrs != blank {
                    break;
                }
                cluster.cell_width -= 1;
                self.text.pop();
                self.len -= 1;
                self.last_cell_width.take();
                pruned = true;
                if cluster.cell_width == 0 {
                    need_pop = true;
                }
            }
            if need_pop {
                self.clusters.pop();
            }
        }

        pruned
    }

    pub(crate) fn has_prunable_trailing_blanks(&self) -> bool {
        self.text.chars().rev().take_while(|&c| c == ' ').count() > 0
            && self
                .clusters
                .last()
                .map(|cluster| cluster.attrs == CellAttributes::blank())
                .unwrap_or(false)
    }

    /// True when the line carries no cells at all. Unlike `len() == 0`,
    /// this is false for a line whose only cell is zero-width (its cluster
    /// exists but covers zero columns): such a line has real content and
    /// must not have an implicit blank appended in front of it.
    pub(crate) fn has_no_clusters(&self) -> bool {
        self.clusters.is_empty()
    }

    fn compute_last_cell_width(&mut self) -> Option<NonZeroU8> {
        if self.last_cell_width.is_none() {
            if let Some(last_cell) = self.iter().last() {
                self.last_cell_width = NonZeroU8::new(last_cell.width() as u8);
            }
        }
        self.last_cell_width
    }

    pub fn set_last_cell_was_wrapped(&mut self, wrapped: bool) {
        if let Some(width) = self.compute_last_cell_width() {
            let width = width.get() as u16;
            if let Some(last_cluster) = self.clusters.last_mut() {
                let mut attrs = last_cluster.attrs.clone();
                attrs.set_wrapped(wrapped);

                if last_cluster.cell_width >= width {
                    if last_cluster.cell_width == width {
                        // Re-purpose final cluster
                        last_cluster.attrs = attrs;
                    } else {
                        last_cluster.cell_width -= width;
                        self.clusters.push(Cluster {
                            cell_width: width,
                            attrs,
                        });
                    }
                } else {
                    // Degenerate trailing zero-width cell (e.g. a control
                    // byte or combining mark stored at width 0): its
                    // cluster covers zero columns, so there is nothing to
                    // split off -- the wrapped attribute goes on that
                    // cluster itself, exactly like Vec storage sets it on
                    // the last visible cell. (BUG-33: this used to compute
                    // `0 - width`.)
                    last_cluster.attrs = attrs;
                }
            }
        }
    }

    /// O(1) read of the last cell's attributes. Clusters partition the
    /// line into contiguous cell ranges (invariant maintained by every
    /// mutator: `append*`, `truncate`, `prune_trailing_blanks`,
    /// `set_last_cell_was_wrapped`), so the last cluster always covers
    /// exactly the last grapheme -- no segmentation needed to find it.
    pub(crate) fn last_cell_attrs(&self) -> Option<&CellAttributes> {
        self.clusters.last().map(|c| &c.attrs)
    }

    /// O(1) test used by `Line::is_whitespace`: a space is always
    /// single-width and (being outside the Extend/ZWJ/SpacingMark
    /// categories) never merges with a neighboring grapheme, so the line
    /// is all-blank cells (`c.str() == " "` for every cell) exactly when
    /// its backing text is nothing but ASCII space bytes.
    pub(crate) fn is_all_spaces(&self) -> bool {
        self.text.bytes().all(|b| b == b' ')
    }
}

/// One output line of a fast-path wrap: the wrapped `ClusteredLine`
/// itself plus whether any of its cells carry a hyperlink (so the caller
/// can set `HAS_HYPERLINK` on the assembled `Line`).
pub(crate) struct WrapPiece {
    pub(crate) line: ClusteredLine,
    pub(crate) has_hyperlink: bool,
}

/// State of the output piece currently being filled by the general-tier
/// wrap pass (`PieceBuilder::finish` turns it into a `WrapPiece`).
struct PieceBuilder {
    byte_from: usize,
    byte_to: usize,
    clusters: Vec<Cluster>,
    wide: Vec<usize>,
    has_hyperlink: bool,
    len: usize,
    last_cell_width: usize,
}

impl PieceBuilder {
    /// Freezes the piece: slices `text` to the piece's byte range and
    /// builds the double-wide bitset the same way `append_grapheme` does
    /// (`with_capacity(first+1)`, then `grow(idx+1)` + `set` for each
    /// subsequent wide cell), so the bit length equals the highest wide
    /// cell index + 1 -- exactly what the per-cell reference path builds.
    fn finish(self, text: &str) -> WrapPiece {
        let is_double_wide = match self.wide.as_slice() {
            [] => None,
            [first, rest @ ..] => {
                let mut bitset = FixedBitSet::with_capacity(first + 1);
                bitset.set(*first, true);
                for &idx in rest {
                    bitset.grow(idx + 1);
                    bitset.set(idx, true);
                }
                Some(Box::new(bitset))
            }
        };
        WrapPiece {
            line: ClusteredLine {
                text: text[self.byte_from..self.byte_to].to_string(),
                is_double_wide,
                clusters: self.clusters,
                len: self.len as u32,
                last_cell_width: NonZeroU8::new(self.last_cell_width as u8),
            },
            has_hyperlink: self.has_hyperlink,
        }
    }
}

impl ClusteredLine {
    /// Fast-path gate: every cluster covers at least one cell and the
    /// clusters tile the line exactly (sum of cluster widths == `len`).
    /// Degenerate storage (zero-width clusters, or clusters not reaching
    /// `len`) must go through the reference path instead.
    pub(crate) fn clusters_consistent(&self) -> bool {
        let mut total = 0usize;
        if !self.clusters.iter().all(|c| {
            total += c.cell_width as usize;
            c.cell_width > 0
        }) {
            return false;
        }
        total == self.len()
    }

    /// OPT-4 fast path: split into pieces of at most `width` cells each,
    /// after trimming trailing blank cells (a cell counts as visible when
    /// its text is not a single space or its cell index is below `keep`).
    /// Returns `None` when no cell is visible; the caller must then leave
    /// the line unchanged, like the reference path does.
    ///
    /// Must only be called on lines passing `clusters_consistent()`.
    pub(crate) fn wrap_pieces(&self, width: usize, keep: usize) -> Option<Vec<WrapPiece>> {
        if self.is_double_wide.is_none()
            && self.text.len() == self.len as usize
            && self.text.bytes().all(|b| (0x20..=0x7e).contains(&b))
        {
            self.wrap_pieces_ascii(width, keep)
        } else {
            self.wrap_pieces_general(width, keep)
        }
    }

    /// ASCII tier: every cell is exactly one printable-ASCII byte of width
    /// 1 (no wide bits, `text.len() == len`), so cell indices equal byte
    /// offsets and piece boundaries are plain arithmetic; only the cluster
    /// attributes need to be intersected with each piece.
    fn wrap_pieces_ascii(&self, width: usize, keep: usize) -> Option<Vec<WrapPiece>> {
        let end = self
            .text
            .bytes()
            .enumerate()
            .rev()
            .find(|&(idx, b)| b != b' ' || idx < keep)
            .map(|(idx, _)| idx + 1)?;

        let mut cluster_ends = Vec::with_capacity(self.clusters.len());
        let mut pos = 0usize;
        for cluster in &self.clusters {
            pos += cluster.cell_width as usize;
            cluster_ends.push(pos);
        }
        debug_assert_eq!(pos, self.len as usize, "clusters tile the line");

        let mut pieces = Vec::new();
        let mut begin = 0usize;
        // Index of the cluster covering `begin`; pieces are cut left to
        // right, so the cursor only ever moves forward.
        let mut cursor = 0usize;
        while begin < end {
            let stop = (begin + width).min(end);
            while cluster_ends[cursor] <= begin {
                cursor += 1;
            }
            let mut base = if cursor == 0 {
                0
            } else {
                cluster_ends[cursor - 1]
            };
            let mut clusters: Vec<Cluster> = Vec::new();
            let mut has_hyperlink = false;
            let mut ci = cursor;
            while base < stop {
                let cluster = &self.clusters[ci];
                let cend = base + cluster.cell_width as usize;
                let take = cend.min(stop) - base.max(begin);
                if take > 0 {
                    has_hyperlink |= cluster.attrs.hyperlink().is_some();
                    match clusters.last_mut() {
                        Some(last) if last.attrs == cluster.attrs => {
                            last.cell_width += take as u16;
                        }
                        _ => clusters.push(Cluster {
                            cell_width: take as u16,
                            attrs: cluster.attrs.clone(),
                        }),
                    }
                }
                base = cend;
                ci += 1;
            }
            pieces.push(WrapPiece {
                line: ClusteredLine {
                    text: self.text[begin..stop].to_string(),
                    is_double_wide: None,
                    clusters,
                    len: (stop - begin) as u32,
                    last_cell_width: NonZeroU8::new(1),
                },
                has_hyperlink,
            });
            begin = stop;
        }
        Some(pieces)
    }

    /// General tier: one pass over the visible cells (grapheme iteration)
    /// to find the last visible cell, then a second pass to cut pieces on
    /// cell boundaries, tracking byte offsets, per-cell widths and the
    /// double-wide cells.
    fn wrap_pieces_general(&self, width: usize, keep: usize) -> Option<Vec<WrapPiece>> {
        let mut end_ord = 0usize;
        for (ord, cell) in self.iter().enumerate() {
            if cell.str() != " " || cell.cell_index() < keep {
                end_ord = ord + 1;
            }
        }
        if end_ord == 0 {
            return None;
        }

        let mut pieces: Vec<PieceBuilder> = Vec::new();
        let mut pos = 0usize;
        for (ord, cell) in self.iter().enumerate() {
            if ord == end_ord {
                break;
            }
            let w = cell.width();
            let need_new_piece = match pieces.last() {
                Some(last) => last.len + w > width,
                None => true,
            };
            if need_new_piece {
                pieces.push(PieceBuilder {
                    byte_from: pos,
                    byte_to: pos,
                    clusters: Vec::new(),
                    wide: Vec::new(),
                    has_hyperlink: false,
                    len: 0,
                    last_cell_width: 0,
                });
            }
            let last = pieces.last_mut().expect("a piece was just created");
            let attrs = cell.attrs();
            last.has_hyperlink |= attrs.hyperlink().is_some();
            match last.clusters.last_mut() {
                Some(cluster) if cluster.attrs == *attrs => cluster.cell_width += w as u16,
                _ => last.clusters.push(Cluster {
                    cell_width: w as u16,
                    attrs: attrs.clone(),
                }),
            }
            if w > 1 {
                last.wide.push(last.len);
            }
            last.len += w;
            last.last_cell_width = w;
            pos += cell.str().len();
            last.byte_to = pos;
        }

        Some(
            pieces
                .into_iter()
                .map(|piece| piece.finish(&self.text))
                .collect(),
        )
    }
}

/// Returns the grapheme starting at byte `pos` in `text` (`pos` must be a
/// grapheme boundary and `pos < text.len()`).
///
/// ASCII fast path: a printable ASCII byte (0x20..=0x7E) followed by
/// another ASCII byte, or by the end of the string, is always its own
/// one-byte grapheme. No rule of the extended grapheme cluster algorithm
/// ever joins two bytes in that range: the only ASCII multi-codepoint
/// cluster is CRLF, and CR (0x0D) falls outside 0x20..=0x7E so it always
/// takes the slow path below; and every category that can extend a
/// cluster forward (Extend, ZWJ, SpacingMark, Prepend, regional
/// indicators, Indic conjuncts) consists entirely of code points >=
/// U+0080, whose UTF-8 encoding starts with a byte >= 0x80. So checking
/// only "is the next byte ASCII or is there no next byte" is sufficient
/// to rule out any extension of the current byte's grapheme.
///
/// Anything else falls back to full segmentation from `pos`, which is
/// always correct (just not always necessary).
fn next_grapheme_at(text: &str, pos: usize) -> &str {
    let bytes = text.as_bytes();
    let b = bytes[pos];
    if (0x20..=0x7e).contains(&b) {
        let next_is_ascii_or_end = bytes.get(pos + 1).is_none_or(|&nb| nb < 0x80);
        if next_is_ascii_or_end {
            return &text[pos..pos + 1];
        }
    }
    Graphemes::new(&text[pos..])
        .next()
        .expect("pos < text.len() implies a grapheme exists at pos")
}

pub(crate) struct ClusterLineCellIter<'a> {
    text: &'a str,
    pos: usize,
    clusters: core::slice::Iter<'a, Cluster>,
    cluster: Option<&'a Cluster>,
    idx: usize,
    cluster_total: usize,
    line: &'a ClusteredLine,
}

impl<'a> Iterator for ClusterLineCellIter<'a> {
    type Item = CellRef<'a>;

    fn next(&mut self) -> Option<CellRef<'a>> {
        if self.pos >= self.text.len() {
            return None;
        }
        let text = next_grapheme_at(self.text, self.pos);
        self.pos += text.len();

        let cell_index = self.idx;
        let width = if self.line.is_double_wide(cell_index) {
            2
        } else {
            1
        };
        self.idx += width;
        self.cluster_total += width;
        let attrs = &self.cluster.as_ref()?.attrs;

        if self.cluster_total >= self.cluster.as_ref()?.cell_width as usize {
            self.cluster = self.clusters.next();
            self.cluster_total = 0;
        }

        Some(CellRef::ClusterRef {
            cell_index,
            width,
            text,
            attrs,
        })
    }
}

impl ClusteredLine {
    /// OPT-4 fast path for `Line::append_line`: append the whole of
    /// `other` in bulk (`push_str` + cluster merge + wide-bit shift)
    /// instead of cell by cell. The caller must have gated the input so
    /// that both sides pass `clusters_consistent()` and
    /// `self.len + other.len <= u16::MAX` (which also rules out `u16`
    /// overflow when merging cluster runs). The result must be exactly
    /// equal -- including cluster splitting and `FixedBitSet` length --
    /// to what the per-cell reference path produces; see
    /// `reflow_fastpath_test.rs`.
    pub(crate) fn append_clustered(&mut self, other: &Self) {
        if other.len == 0 {
            return;
        }
        let base = self.len as usize;
        self.text.push_str(&other.text);
        for cluster in &other.clusters {
            self.merge_cluster(cluster.cell_width, cluster.attrs.clone());
        }
        self.append_shifted_wide_bits(base, other.is_double_wide.as_deref());
        self.last_cell_width = other
            .last_cell_width
            .or_else(|| other.last_cell_width_from_bits());
        self.len += other.len;
    }

    /// Same as `append_clustered`, but takes `other` by value so that
    /// cluster attributes are moved instead of cloned when the caller
    /// knows the source storage is unshared (`Arc::try_unwrap`
    /// succeeded).
    pub(crate) fn append_clustered_owned(&mut self, mut other: Self) {
        if other.len == 0 {
            return;
        }
        let base = self.len as usize;
        self.text.push_str(&other.text);
        let clusters = core::mem::take(&mut other.clusters);
        for cluster in clusters {
            self.merge_cluster(cluster.cell_width, cluster.attrs);
        }
        self.append_shifted_wide_bits(base, other.is_double_wide.as_deref());
        self.last_cell_width = other
            .last_cell_width
            .or_else(|| other.last_cell_width_from_bits());
        self.len += other.len;
    }

    /// Extends the last cluster when its attributes match the appended
    /// run, exactly like the per-cell `append` does -- including merging
    /// the first appended cluster into `self`'s last one. The caller's
    /// gate (`total len <= u16::MAX`) guarantees this cannot overflow.
    fn merge_cluster(&mut self, cell_width: u16, attrs: CellAttributes) {
        match self.clusters.last_mut() {
            Some(cluster) if cluster.attrs == attrs => {
                cluster.cell_width += cell_width;
            }
            _ => self.clusters.push(Cluster { cell_width, attrs }),
        }
    }

    /// Shifts `other`'s double-wide bits by `base` cells, using the same
    /// `grow(idx+1)` + `set` sequence the per-cell reference path ends up
    /// with, so the resulting bitset *length* (which `PartialEq`
    /// compares) matches exactly: highest shifted wide index + 1, grown
    /// only when a wide cell is actually recorded.
    fn append_shifted_wide_bits(&mut self, base: usize, other_bits: Option<&FixedBitSet>) {
        let bits = match other_bits {
            Some(bits) => bits,
            None => return,
        };
        for idx in bits.ones() {
            let idx = base + idx;
            let bitset = match self.is_double_wide.take() {
                Some(mut bitset) => {
                    bitset.grow(idx + 1);
                    bitset.set(idx, true);
                    bitset
                }
                None => {
                    let mut bitset = FixedBitSet::with_capacity(idx + 1);
                    bitset.set(idx, true);
                    Box::new(bitset)
                }
            };
            self.is_double_wide = Some(bitset);
        }
    }

    /// Width of the last cell computed from the wide bits. Used when the
    /// stored `last_cell_width` is `None` for a non-empty line, which
    /// `prune_trailing_blanks` can leave behind; the per-cell reference
    /// path always recomputes this from the appended cell, so the fast
    /// path must too.
    fn last_cell_width_from_bits(&self) -> Option<NonZeroU8> {
        let width = if self.len() >= 2 && self.is_double_wide(self.len() - 2) {
            2
        } else {
            1
        };
        NonZeroU8::new(width)
    }

    /// OPT-4 append gate, second half: the fast path copies `other`'s
    /// recorded cluster/bitset/`len` bookkeeping in bulk, so that
    /// bookkeeping must agree with how the *joined* text actually
    /// segments. Two things can disagree, and in both cases the caller
    /// must fall back to the reference (which counts real cells):
    ///
    /// 1. `other`'s own text merges recorded neighbours into one
    ///    grapheme (e.g. cells `"k"` and `U+0301` recorded separately
    ///    become `"k" + U+0301`): then `other.len()` overcounts what the
    ///    reference appends.
    /// 2. the boundary between `prev_text` (self's text) and `other`'s
    ///    text is not a grapheme boundary (e.g. self ends "e", other
    ///    starts `U+0301`): then bulk-appending shifts every later
    ///    cell index, unlike the reference, which appends cell by cell.
    pub(crate) fn append_fast_path_safe(&self, prev_text: &str) -> bool {
        // Cheap tier: if no byte of `other.text` can begin a
        // non-starter char (Extender, ZWJ, SpacingMark, second
        // regional indicator, emoji modifier -- their UTF-8 leading
        // bytes are all in the set below), recorded cells cannot
        // fuse internally and the width scan is unnecessary. The
        // join boundary can then only fuse if self's last char is
        // a Prepend char or a regional indicator; every such char
        // has a leading byte >= 0xD8, testable by walking back over
        // at most three continuation bytes.
        if !self.text.bytes().any(dirty_non_starter_lead) {
            return match last_char_lead(prev_text) {
                Some(lead) => lead < 0xD8,
                None => true,
            };
        }
        let mut width_sum = 0usize;
        for cell in self.iter() {
            width_sum += cell.width();
        }
        if width_sum != self.len() {
            return false;
        }
        !boundary_merges_graphemes(prev_text, &self.text)
    }
}

/// True when the join of `a` and `b` would fuse `a`'s last grapheme with
/// `b`'s first grapheme into one cluster. Pure-ASCII boundaries (printable
/// bytes on both sides) are always breaks, so the common case costs four
/// byte compares; anything else segments the two edge graphemes.
/// UTF-8 leading byte of a char that can be a grapheme non-starter
/// (Extend, ZWJ, SpacingMark, second regional indicator, emoji
/// modifier). Every such codepoint is >= U+0300 and its encoding
/// starts with one of these bytes; bytes outside the set always
/// begin a grapheme starter. Conservative: some starter-only
/// scripts (Hebrew, CJK, Thai...) share leading bytes in the upper
/// range and just take the precise path.
fn dirty_non_starter_lead(b: u8) -> bool {
    matches!(b, 0xCC | 0xCD | 0xD2 | 0xD5..=0xF4)
}

/// Leading byte of the last char of `text`, or `None` when empty.
fn last_char_lead(text: &str) -> Option<u8> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut i = bytes.len() - 1;
    while i > 0 && bytes[i] & 0xC0 == 0x80 {
        i -= 1;
    }
    Some(bytes[i])
}

fn boundary_merges_graphemes(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let a_bytes = a.as_bytes();
    let a_last = a_bytes[a_bytes.len() - 1];
    let b_first = b.as_bytes()[0];
    if (0x20..=0x7e).contains(&a_last) && (0x20..=0x7e).contains(&b_first) {
        return false;
    }
    let mut last_start = 0usize;
    let mut pos = 0usize;
    for g in Graphemes::new(a) {
        last_start = pos;
        pos += g.len();
    }
    let mut probe = String::with_capacity(a.len() - last_start + b.len());
    probe.push_str(&a[last_start..]);
    probe.push_str(next_grapheme_at(b, 0));
    Graphemes::new(&probe).count() < 2
}
#[cfg(test)]
#[path = "tests/grapheme_fastpath_test.rs"]
mod grapheme_fastpath_test;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn memory_usage() {
        assert_eq!(core::mem::size_of::<ClusteredLine>(), 64);
        assert_eq!(core::mem::size_of::<String>(), 24);
        assert_eq!(core::mem::size_of::<Vec<Cluster>>(), 24);
        assert_eq!(core::mem::size_of::<Option<Box<FixedBitSet>>>(), 8);
        assert_eq!(core::mem::size_of::<Option<NonZeroU8>>(), 1);
    }

    /// Regression: `new()` used to eagerly `String::with_capacity(80)`, even
    /// though most `Line::new()` calls create blank rows that are never
    /// printed into (every blank row `Screen::new`/resize/`scroll_up` fills
    /// ahead of the cursor). Reserve nothing up front; `push_str`'s own
    /// amortized growth pays for exactly what a row that DOES get appended
    /// to actually needs.
    #[test]
    fn new_reserves_no_text_capacity() {
        let cl = ClusteredLine::new();
        assert_eq!(cl.text.capacity(), 0);
        assert_eq!(cl.text.len(), 0);
    }

    /// `clear_in_place` must produce something indistinguishable (by value)
    /// from a fresh `ClusteredLine::new()`, while actually keeping the
    /// allocation around -- that's the entire point of using it to recycle a
    /// scrolled-off line's storage instead of allocating a new one.
    #[test]
    fn clear_in_place_matches_new_but_keeps_capacity() {
        let mut cl = ClusteredLine::new();
        cl.append(Cell::new_grapheme("h", CellAttributes::default(), None));
        cl.append(Cell::new_grapheme("i", CellAttributes::default(), None));
        let cap_before = cl.text.capacity();
        assert!(
            cap_before >= 2,
            "precondition: appending must have allocated"
        );

        cl.clear_in_place();

        assert_eq!(
            cl,
            ClusteredLine::new(),
            "clear_in_place must match a fresh ClusteredLine::new() by value"
        );
        assert!(
            cl.text.capacity() >= cap_before,
            "clear_in_place must keep the allocation, not reset it"
        );
    }
}
