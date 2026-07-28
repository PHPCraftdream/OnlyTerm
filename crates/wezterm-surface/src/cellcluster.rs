use crate::line::CellRef;
use alloc::borrow::Cow;
use wezterm_bidi::{BidiContext, Direction, ParagraphDirectionHint};
use wezterm_cell::CellAttributes;
use wezterm_char_props::emoji::Presentation;

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// A `CellCluster` is another representation of a Line.
/// A `Vec<CellCluster>` is produced by walking through the Cells in
/// a line and collecting succesive Cells with the same attributes
/// together into a `CellCluster` instance.  Additional metadata to
/// aid in font rendering is also collected.
#[derive(Debug, Clone)]
pub struct CellCluster {
    pub attrs: CellAttributes,
    pub text: String,
    pub width: usize,
    pub presentation: Presentation,
    pub direction: Direction,
    byte_to_cell_idx: Vec<usize>,
    byte_to_cell_width: Vec<u8>,
    pub first_cell_idx: usize,
}

impl CellCluster {
    /// Given a byte index into `self.text`, return the corresponding
    /// cell index in the originating line.
    pub fn byte_to_cell_idx(&self, byte_idx: usize) -> usize {
        if self.byte_to_cell_idx.is_empty() {
            self.first_cell_idx + byte_idx
        } else {
            self.byte_to_cell_idx[byte_idx]
        }
    }

    pub fn byte_to_cell_width(&self, byte_idx: usize) -> u8 {
        if self.byte_to_cell_width.is_empty() {
            1
        } else {
            self.byte_to_cell_width[byte_idx]
        }
    }

    /// Compute the list of CellClusters from a set of visible cells.
    /// The input is typically the result of calling `Line::visible_cells()`.
    pub fn make_cluster<'a>(
        hint: usize,
        iter: impl Iterator<Item = CellRef<'a>>,
        bidi_hint: Option<ParagraphDirectionHint>,
    ) -> Vec<CellCluster> {
        match bidi_hint {
            Some(dir_hint) => Self::make_cluster_with_bidi(hint, dir_hint, iter),
            None => Self::make_cluster_no_bidi(hint, iter),
        }
    }

    fn make_cluster_no_bidi<'a>(
        hint: usize,
        iter: impl Iterator<Item = CellRef<'a>>,
    ) -> Vec<CellCluster> {
        let mut last_cluster = None;
        let mut clusters = Vec::new();
        let mut whitespace_run = 0;
        let mut only_whitespace = false;

        for c in iter {
            let cell_idx = c.cell_index();
            let presentation = c.presentation();
            let cell_str = c.str();
            let normalized_attr = if c.attrs().wrapped() {
                let mut attr_storage = c.attrs().clone();
                attr_storage.set_wrapped(false);
                Cow::Owned(attr_storage)
            } else {
                Cow::Borrowed(c.attrs())
            };

            last_cluster = match last_cluster.take() {
                None => {
                    // Start new cluster
                    only_whitespace = cell_str == " ";
                    whitespace_run = if only_whitespace { 1 } else { 0 };
                    Some(CellCluster::new(
                        hint,
                        presentation,
                        normalized_attr.into_owned(),
                        cell_str,
                        cell_idx,
                        c.width(),
                    ))
                }
                Some(mut last) => {
                    if last.attrs != *normalized_attr || last.presentation != presentation {
                        // Flush pending cluster and start a new one
                        clusters.push(last);

                        only_whitespace = cell_str == " ";
                        whitespace_run = if only_whitespace { 1 } else { 0 };
                        Some(CellCluster::new(
                            hint,
                            presentation,
                            normalized_attr.into_owned(),
                            cell_str,
                            cell_idx,
                            c.width(),
                        ))
                    } else {
                        // Add to current cluster.

                        // Force cluster to break when we get a run of 2 whitespace
                        // characters following non-whitespace, or immediately after
                        // any whitespace-to-non-whitespace transition (bidi is off
                        // here, so there's no reordering concern to preserve
                        // contiguous runs for).
                        // This reduces the amount of shaping work for scenarios where
                        // the terminal is wide and a long series of short lines are printed;
                        // the shaper can cache the few variations of trailing whitespace
                        // and focus on shaping the shorter cluster sequences.
                        let was_whitespace = whitespace_run > 0;
                        if cell_str == " " {
                            whitespace_run += 1;
                        } else {
                            whitespace_run = 0;
                            only_whitespace = false;
                        }

                        let force_break = !only_whitespace && (whitespace_run > 2 || was_whitespace);

                        if force_break {
                            clusters.push(last);

                            only_whitespace = cell_str == " ";
                            if whitespace_run > 0 {
                                whitespace_run = 1;
                            }
                            Some(CellCluster::new(
                                hint,
                                presentation,
                                normalized_attr.into_owned(),
                                cell_str,
                                cell_idx,
                                c.width(),
                            ))
                        } else {
                            last.add(cell_str, cell_idx, c.width());
                            Some(last)
                        }
                    }
                }
            };
        }

        if let Some(cluster) = last_cluster {
            // Don't forget to include any pending cluster on the final step!
            clusters.push(cluster);
        }

        clusters
    }

    /// Same idea as `make_cluster_no_bidi`, but bidi-aware: resolves the
    /// Unicode Bidirectional Algorithm across the *entire* line as a single
    /// paragraph FIRST, then splits the resulting (already visually
    /// reordered) runs by cell attributes/presentation.
    ///
    /// This ordering matters: if we instead split by attributes first (as
    /// `make_cluster_no_bidi` does) and resolved bidi independently within
    /// each attribute-uniform span, then any program that emits different
    /// attributes per character or per short span -- eg: a chatty
    /// program applying per-token/per-color styling to streamed output --
    /// would fragment a single RTL word into several single-character bidi
    /// "paragraphs", each auto-detecting its own direction with no
    /// knowledge of its neighbors. That breaks RTL layout entirely (each
    /// letter ends up positioned independently, with visible gaps) even
    /// though each character's own resolved direction looks individually
    /// "correct" in isolation.
    fn make_cluster_with_bidi<'a>(
        capacity_hint: usize,
        dir_hint: ParagraphDirectionHint,
        iter: impl Iterator<Item = CellRef<'a>>,
    ) -> Vec<CellCluster> {
        let cells: Vec<CellRef<'a>> = iter.collect();
        if cells.is_empty() {
            return Vec::new();
        }

        // Build one full-line paragraph (ignoring attribute boundaries)
        // so that bidi resolution always sees complete context, and
        // remember which source cell each codepoint came from.
        let mut paragraph: Vec<char> = Vec::new();
        let mut cp_cell_ref_idx: Vec<usize> = Vec::new();
        for (cell_ref_idx, c) in cells.iter().enumerate() {
            for cp in c.str().chars() {
                paragraph.push(cp);
                cp_cell_ref_idx.push(cell_ref_idx);
            }
        }

        let mut context = BidiContext::new();
        context.resolve_paragraph(&paragraph, dir_hint);

        let mut resolved = Vec::new();

        for run in context.reordered_runs(0..paragraph.len()) {
            // `run.range` is a `min..max+1` envelope over this run's
            // (visually contiguous) codepoint indices -- it is NOT
            // guaranteed to be exactly the set of codepoints belonging to
            // this run when other runs are nested/interleaved with it (eg:
            // multiple Hebrew words and Latin/punctuation runs on the same
            // line). Iterating `run.range` directly can therefore visit a
            // codepoint that actually belongs to a *different* run,
            // duplicating characters (a stray repeated comma/quote) or
            // pulling in the wrong glyph. `run.indices` is the exact,
            // deduplicated set of logical codepoint indices assigned to
            // this run; sort it ascending to recover logical order (needed
            // for feeding text to harfbuzz, which expects logical order,
            // not the visually-reordered order).
            let mut cp_idxs: Vec<usize> = run.indices.clone();
            cp_idxs.sort_unstable();

            // The distinct, logically-ordered sequence of source cells
            // covered by this run.
            let mut run_cell_indices: Vec<usize> = Vec::new();
            for cp_idx in cp_idxs {
                let cell_ref_idx = cp_cell_ref_idx[cp_idx];
                if run_cell_indices.last() != Some(&cell_ref_idx) {
                    run_cell_indices.push(cell_ref_idx);
                }
            }

            let mut last_cluster: Option<CellCluster> = None;
            let mut whitespace_run = 0usize;
            let mut only_whitespace = false;

            for cell_ref_idx in run_cell_indices {
                let c = &cells[cell_ref_idx];
                let cell_idx = c.cell_index();
                let presentation = c.presentation();
                let cell_str = c.str();
                let normalized_attr = if c.attrs().wrapped() {
                    let mut attr_storage = c.attrs().clone();
                    attr_storage.set_wrapped(false);
                    Cow::Owned(attr_storage)
                } else {
                    Cow::Borrowed(c.attrs())
                };

                last_cluster = match last_cluster.take() {
                    None => {
                        only_whitespace = cell_str == " ";
                        whitespace_run = if only_whitespace { 1 } else { 0 };
                        let mut cluster = CellCluster::new(
                            capacity_hint,
                            presentation,
                            normalized_attr.into_owned(),
                            cell_str,
                            cell_idx,
                            c.width(),
                        );
                        cluster.direction = run.direction;
                        Some(cluster)
                    }
                    Some(mut last) => {
                        if last.attrs != *normalized_attr || last.presentation != presentation {
                            resolved.push(last);
                            only_whitespace = cell_str == " ";
                            whitespace_run = if only_whitespace { 1 } else { 0 };
                            let mut cluster = CellCluster::new(
                                capacity_hint,
                                presentation,
                                normalized_attr.into_owned(),
                                cell_str,
                                cell_idx,
                                c.width(),
                            );
                            cluster.direction = run.direction;
                            Some(cluster)
                        } else {
                            // Cache-locality heuristic only: bidi is
                            // active, so (unlike make_cluster_no_bidi) we
                            // don't force-break on every whitespace
                            // transition, only on long whitespace runs,
                            // since reordering needs runs to stay
                            // contiguous where possible.
                            if cell_str == " " {
                                whitespace_run += 1;
                            } else {
                                whitespace_run = 0;
                                only_whitespace = false;
                            }
                            let force_break = !only_whitespace && whitespace_run > 2;

                            if force_break {
                                resolved.push(last);
                                only_whitespace = cell_str == " ";
                                if whitespace_run > 0 {
                                    whitespace_run = 1;
                                }
                                let mut cluster = CellCluster::new(
                                    capacity_hint,
                                    presentation,
                                    normalized_attr.into_owned(),
                                    cell_str,
                                    cell_idx,
                                    c.width(),
                                );
                                cluster.direction = run.direction;
                                Some(cluster)
                            } else {
                                last.add(cell_str, cell_idx, c.width());
                                Some(last)
                            }
                        }
                    }
                };
            }

            if let Some(cluster) = last_cluster {
                resolved.push(cluster);
            }
        }

        resolved
    }

    /// Start off a new cluster with some initial data
    fn new(
        hint: usize,
        presentation: Presentation,
        attrs: CellAttributes,
        text: &str,
        cell_idx: usize,
        width: usize,
    ) -> CellCluster {
        let mut idx = Vec::new();
        if text.len() > 1 {
            // Prefer to avoid pushing any index data; this saves
            // allocating any storage until we have any cells that
            // are multibyte
            for _ in 0..text.len() {
                idx.push(cell_idx);
            }
        }

        let mut byte_to_cell_width = Vec::new();
        if width > 1 {
            for _ in 0..text.len() {
                byte_to_cell_width.push(width as u8);
            }
        }
        let mut storage = String::with_capacity(hint);
        storage.push_str(text);

        CellCluster {
            attrs,
            width,
            text: storage,
            presentation,
            byte_to_cell_idx: idx,
            byte_to_cell_width,
            first_cell_idx: cell_idx,
            direction: Direction::LeftToRight,
        }
    }

    /// Add to this cluster
    fn add(&mut self, text: &str, cell_idx: usize, width: usize) {
        self.width += width;
        if !self.byte_to_cell_idx.is_empty() {
            // We had at least one multi-byte cell in the past
            for _ in 0..text.len() {
                self.byte_to_cell_idx.push(cell_idx);
            }
        } else if text.len() > 1 {
            // Extrapolate the indices so far
            for n in 0..self.text.len() {
                self.byte_to_cell_idx.push(n + self.first_cell_idx);
            }
            // Now add this new multi-byte cell text
            for _ in 0..text.len() {
                self.byte_to_cell_idx.push(cell_idx);
            }
        }

        if !self.byte_to_cell_width.is_empty() {
            // We had at least one double-wide cell in the past
            for _ in 0..text.len() {
                self.byte_to_cell_width.push(width as u8);
            }
        } else if width > 1 {
            // Extrapolate the widths so far; they must all be single width
            for _ in 0..self.text.len() {
                self.byte_to_cell_width.push(1);
            }
            // and add the current double width cell
            for _ in 0..text.len() {
                self.byte_to_cell_width.push(width as u8);
            }
        }
        self.text.push_str(text);
    }
}
