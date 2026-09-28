use crate::line::CellRef;
use core::convert::TryInto;
use core::num::NonZeroU8;
use finl_unicode::grapheme_clusters::Graphemes;
use fixedbitset::FixedBitSet;
use onlyterm_cell::{Cell, CellAttributes};
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

extern crate alloc;
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
    pub fn new() -> Self {
        Self {
            text: String::with_capacity(80),
            is_double_wide: None,
            clusters: vec![],
            len: 0,
            last_cell_width: None,
        }
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
}
