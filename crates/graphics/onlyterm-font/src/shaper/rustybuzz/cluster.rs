//! Resolves rustybuzz clusters to byte ranges and terminal cell widths.

use crate::shaper::PresentationWidth;
use std::collections::HashMap;
use std::ops::Range;
use termwiz::cell::unicode_column_width;

#[derive(Debug)]
pub(super) struct ClusterInfo {
    pub(super) start: usize,
    pub(super) byte_len: usize,
    pub(super) cell_width: u8,
    pub(super) incomplete: bool,
}

#[derive(Default, Debug)]
pub(super) struct ClusterResolver<'a> {
    map: HashMap<usize, ClusterInfo>,
    pub(super) presentation_width: Option<&'a PresentationWidth<'a>>,
    start_by_cell_idx: HashMap<usize, usize>,
}

impl<'a> ClusterResolver<'a> {
    pub(super) fn new(presentation_width: Option<&'a PresentationWidth<'a>>) -> Self {
        Self {
            presentation_width,
            ..Default::default()
        }
    }

    pub(super) fn build(
        &mut self,
        rb_infos: &[rustybuzz::GlyphInfo],
        s: &str,
        range: &Range<usize>,
    ) {
        #[derive(PartialOrd, Ord, Eq, PartialEq, Copy, Clone)]
        struct Item {
            cell_idx: Option<usize>,
            start: usize,
        }

        let mut map = HashMap::new();

        for info in rb_infos.iter() {
            // Convert cluster indexes from the shaped substring to absolute
            // offsets used to slice the full input and presentation width.
            let start = info.cluster as usize + range.start;

            let cell_idx = match self.presentation_width {
                Some(pw) => {
                    let cell_idx = pw.byte_to_cell_idx(start);

                    let entry = self.start_by_cell_idx.entry(cell_idx).or_insert(start);
                    *entry = (*entry).min(start);

                    Some(cell_idx)
                }
                None => None,
            };

            map.entry(start).or_insert_with(|| Item { start, cell_idx });
        }

        let mut cluster_starts: Vec<Item> = map.into_values().collect();
        // Sort by byte position, not derived Ord (which compares cell_idx
        // first). Adjacent entries define byte ranges even when bidi reordered
        // cells, so cell order cannot define this ordering.
        cluster_starts.sort_by_key(|item| item.start);

        cluster_starts.dedup_by(|a, b| match (a.cell_idx, b.cell_idx) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        });

        let mut iter = cluster_starts.iter().peekable();
        while let Some(item) = iter.next().copied() {
            let start = item.start;
            let next_start = iter.peek().map(|&&s| s.start).unwrap_or(range.end);
            let byte_len = next_start - start;
            let cell_width = match self.presentation_width {
                Some(p) => p.num_cells(start..next_start),
                None => unicode_column_width(&s[start..next_start], None) as u8,
            };
            self.map.entry(start).or_insert_with(|| ClusterInfo {
                start,
                byte_len,
                cell_width,
                incomplete: false,
            });
        }
    }

    pub(super) fn get_mut(&mut self, start: usize) -> Option<&mut ClusterInfo> {
        match self.presentation_width {
            Some(pw) => {
                let cell_idx = pw.byte_to_cell_idx(start);
                let actual_start = self.start_by_cell_idx.get(&cell_idx)?;
                self.map.get_mut(actual_start)
            }
            None => self.map.get_mut(&start),
        }
    }

    pub(super) fn get(&self, start: usize) -> Option<&ClusterInfo> {
        match self.presentation_width {
            Some(pw) => {
                let cell_idx = pw.byte_to_cell_idx(start);
                let actual_start = self.start_by_cell_idx.get(&cell_idx)?;
                self.map.get(actual_start)
            }
            None => self.map.get(&start),
        }
    }
}
