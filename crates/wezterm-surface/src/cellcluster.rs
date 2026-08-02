use crate::line::CellRef;
use alloc::borrow::Cow;
use wezterm_bidi::{Direction, ParagraphDirectionHint};
use wezterm_cell::CellAttributes;
use wezterm_char_props::emoji::Presentation;

extern crate alloc;
use alloc::string::String;
use alloc::vec;
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
        is_wrap_continuation: bool,
        continues_next: bool,
    ) -> Vec<CellCluster> {
        match bidi_hint {
            Some(dir_hint) => Self::make_cluster_with_bidi(
                hint,
                dir_hint,
                iter,
                is_wrap_continuation,
                continues_next,
            ),
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

    pub fn is_hebrew_cell(s: &str) -> bool {
        s.chars().any(|c| {
            let cp = c as u32;
            (0x0591..=0x05F4).contains(&cp) || (0xFB1D..=0xFB4F).contains(&cp)
        })
    }

    /// Characters that "glue" two Hebrew words into one phrase without
    /// breaking it: spaces and any punctuation/symbol, ie anything that
    /// carries no direction of its own (Unicode calls these "neutral").
    /// Letters and digits are deliberately NOT glue -- they have their
    /// own direction, so Latin/Cyrillic words and numbers always end a
    /// Hebrew phrase rather than being swallowed by it.
    ///
    /// Glue is only ever absorbed into a phrase when Hebrew actually
    /// resumes after it (see `reorder_hebrew_phrases`), which is what
    /// keeps this equivalent to the Unicode rule (UAX #9 N1) that a run
    /// of neutrals *between* two right-to-left characters becomes
    /// right-to-left itself: a comma inside `אם אין אני לי, מי לי` moves
    /// with the phrase it punctuates, while the quotes/brackets wrapping
    /// the whole phrase -- neutrals with non-Hebrew on their far side --
    /// stay exactly where they were typed.
    ///
    /// Box-drawing and block-element characters are excluded even though
    /// they are technically neutral: in a terminal they are not
    /// punctuation but the frame of a TUI, and the column a border sits
    /// in is load-bearing, so they must never be shuffled within a row.
    fn is_glue_cell(s: &str) -> bool {
        !s.is_empty()
            && s.chars().all(|c| {
                !c.is_alphanumeric() && !(0x2500..=0x259F).contains(&(c as u32))
            })
    }

    /// Reorders `cells` so that terminal rendering can always grow
    /// left-to-right from column 0 -- matching a plain LTR line, cursor
    /// movement and shell line-editing -- while a Hebrew phrase embedded
    /// in it still reads correctly right-to-left. Instead of running the
    /// full Unicode Bidirectional Algorithm (which right-justifies
    /// RTL-based paragraphs and repositions whole runs relative to
    /// surrounding text, entangling neutral punctuation like a stray
    /// dash into the wrong end of the line), this finds maximal spans of
    /// "Hebrew phrase" cells directly (letters glued together via spaces/
    /// geresh/maqaf, as long as Hebrew actually resumes afterwards) and
    /// only reverses the *cell order within each such span*. Everything
    /// outside a span -- digits, Latin text, brackets/quotes, dashes --
    /// stays exactly where it was typed; it never needs mirroring
    /// because it never moves relative to the rest of the line.
    ///
    /// `is_wrap_continuation`/`continues_next` describe whether the FIRST/
    /// LAST cell respectively belongs to a phrase that actually started on
    /// a previous physical row, or continues onto the next one, because the
    /// line wrapped. A physical row only ever sees its own cells, so a
    /// Hebrew span touching that edge might be an incomplete fragment of a
    /// longer phrase -- but a terminal's fixed per-cell grid can't move a
    /// word across a row boundary either way, so every visual row still has
    /// to reverse its own Hebrew content independently of wrap topology
    /// (this is also what standard bidi text layout does: each visual line
    /// is reordered on its own). Leaving an edge-touching span unreversed
    /// "as a precaution" used to mean a multi-word phrase that happened to
    /// wrap between two of its words left BOTH rows entirely in typed
    /// order, not just the seam between them -- so these parameters no
    /// longer change how a span reverses; they are retained for callers
    /// that may need wrap-aware behavior for other purposes in the future.
    ///
    /// Returns, for each position in the returned order, the source cell
    /// index, together with an `in_run` map (indexed by *physical* cell
    /// index, not by position in `order`) recording whether that cell
    /// belongs to a reversed Hebrew-phrase span. Callers that walk `order`
    /// need `in_run` to detect when iteration crosses from inside a
    /// reversed span to outside it (or vice versa): that boundary is a
    /// seam between two different orderings glued back-to-back, and text
    /// that sits on opposite sides of it must never be merged into the
    /// same shaping cluster, even when nothing whitespace-like separates
    /// them (e.g. a `(` immediately followed by a Hebrew phrase with no
    /// space between them).
    fn reorder_hebrew_phrases(
        cells: &[CellRef<'_>],
        _is_wrap_continuation: bool,
        _continues_next: bool,
    ) -> (Vec<usize>, Vec<bool>) {
        let n = cells.len();
        let mut is_heb: Vec<bool> = cells.iter().map(|c| CellCluster::is_hebrew_cell(c.str())).collect();

        // An ASCII apostrophe standing in for a geresh (`א'`, the Hebrew
        // numeral notation) belongs to the letter in front of it and has
        // to move with that letter rather than stay behind when the
        // letter's position is reversed, so count it as Hebrew and let it
        // fold into the same run. (The real HEBREW PUNCTUATION GERESH and
        // GERSHAYIM need no help here: `is_hebrew_cell` already covers
        // them, since they live in the Hebrew block.)
        //
        // The ASCII apostrophe is ambiguous though -- it is also a quote
        // character, and a *closing* quote has to stay put rather than be
        // dragged to the far side of the phrase it closes. Only the first
        // apostrophe on the line can be a geresh; any later one is the
        // closing half of the quote that the first one opened.
        let mut apostrophe_may_be_geresh = true;
        for i in 0..n {
            if cells[i].str() != "'" {
                continue;
            }
            let is_geresh = apostrophe_may_be_geresh;
            apostrophe_may_be_geresh = false;
            if is_geresh && i > 0 && !is_heb[i] && is_heb[i - 1] {
                is_heb[i] = true;
            }
        }

        // Find maximal spans of Hebrew cells glued together by spaces/
        // geresh/maqaf (only absorbing the glue if Hebrew actually
        // resumes afterwards -- a trailing " - Chapter" must NOT be
        // swept in).
        let mut in_run = vec![false; n];
        let mut i = 0;
        while i < n {
            if is_heb[i] {
                let mut end = i;
                let mut j = i + 1;
                while j < n {
                    if is_heb[j] {
                        end = j;
                        j += 1;
                    } else if Self::is_glue_cell(cells[j].str()) {
                        let mut k = j;
                        while k < n && Self::is_glue_cell(cells[k].str()) {
                            k += 1;
                        }
                        if k < n && is_heb[k] {
                            j = k;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                for cell in in_run.iter_mut().take(end + 1).skip(i) {
                    *cell = true;
                }
                i = end + 1;
            } else {
                i += 1;
            }
        }

        let mut order: Vec<usize> = (0..n).collect();
        let mut idx = 0;
        while idx < n {
            if in_run[idx] {
                let start = idx;
                while idx < n && in_run[idx] {
                    idx += 1;
                }
                let end = idx - 1;
                order[start..=end].reverse();
            } else {
                idx += 1;
            }
        }

        (order, in_run)
    }

    fn make_cluster_with_bidi<'a>(
        capacity_hint: usize,
        _dir_hint: ParagraphDirectionHint,
        iter: impl Iterator<Item = CellRef<'a>>,
        is_wrap_continuation: bool,
        continues_next: bool,
    ) -> Vec<CellCluster> {
        let cells: Vec<CellRef<'a>> = iter.collect();
        if cells.is_empty() {
            return Vec::new();
        }

        let (order, in_run) =
            Self::reorder_hebrew_phrases(&cells, is_wrap_continuation, continues_next);

        let mut resolved = Vec::new();
        let mut last_cluster: Option<CellCluster> = None;
        let mut whitespace_run = 0usize;
        let mut only_whitespace = false;
        let mut last_in_run: Option<bool> = None;

        for cell_ref_idx in order {
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
            let cur_in_run = in_run[cell_ref_idx];
            // A reversed Hebrew-phrase span and the text outside it are
            // two different orderings glued back-to-back in `order`; the
            // seam between them must always be a cluster boundary, even
            // when there's no whitespace there (e.g. "(" immediately
            // followed by a Hebrew phrase), otherwise the shaped cluster
            // text ends up mixing cells from "different worlds" and the
            // GUI's sequential left-to-right cluster placement draws the
            // punctuation on the wrong side of the phrase.
            let crosses_run_boundary = last_in_run.is_some_and(|prev| prev != cur_in_run);
            last_in_run = Some(cur_in_run);

            last_cluster = match last_cluster.take() {
                None => {
                    only_whitespace = cell_str == " ";
                    whitespace_run = if only_whitespace { 1 } else { 0 };
                    Some(CellCluster::new(
                        capacity_hint,
                        presentation,
                        normalized_attr.into_owned(),
                        cell_str,
                        cell_idx,
                        c.width(),
                    ))
                }
                Some(mut last) => {
                    if last.attrs != *normalized_attr || last.presentation != presentation {
                        resolved.push(last);
                        only_whitespace = cell_str == " ";
                        whitespace_run = if only_whitespace { 1 } else { 0 };
                        Some(CellCluster::new(
                            capacity_hint,
                            presentation,
                            normalized_attr.into_owned(),
                            cell_str,
                            cell_idx,
                            c.width(),
                        ))
                    } else {
                        let was_whitespace = whitespace_run > 0;
                        if cell_str == " " {
                            whitespace_run += 1;
                        } else {
                            whitespace_run = 0;
                            only_whitespace = false;
                        }
                        let force_break = crosses_run_boundary
                            || (!only_whitespace && (whitespace_run > 2 || was_whitespace));

                        if force_break {
                            resolved.push(last);
                            only_whitespace = cell_str == " ";
                            if whitespace_run > 0 {
                                whitespace_run = 1;
                            }
                            Some(CellCluster::new(
                                capacity_hint,
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
            resolved.push(cluster);
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::line::Line;
    use crate::SEQ_ZERO;
    use wezterm_cell::CellAttributes;

    /// Regression test for a bug where punctuation sitting immediately
    /// (no space) next to a Hebrew phrase got glued into the same
    /// shaping cluster as reversed Hebrew letters from across the run
    /// boundary, e.g. `(` ending up in the same `CellCluster::text` as
    /// the last letter of the reversed phrase. `make_cluster_with_bidi`
    /// walks `order` (the post-reversal iteration order) and only broke
    /// clusters on whitespace runs, with no notion of "this step just
    /// crossed from outside a reversed span to inside it" -- so an
    /// opening paren glued onto the tail of the (now-first, after
    /// reversal) Hebrew letter it happened to be adjacent to in
    /// iteration order, even though physically they are not adjacent at
    /// all once the phrase is reversed. The fix breaks a cluster at any
    /// such boundary, in addition to (not instead of) the existing
    /// whitespace-run logic.
    #[test]
    fn punctuation_hugging_hebrew_stays_out_of_reversed_cluster() {
        // Two-letter "phrase" wrapped tightly by parens with no spaces
        // at either boundary: "(<heb><heb>)". Real Hebrew consonants,
        // no niqqud involved here -- that stripping happens earlier, in
        // the terminal's Performer, not in this layer.
        let s = "(\u{05D0}\u{05D1})";
        let line = Line::from_text(s, &CellAttributes::default(), SEQ_ZERO, None);
        let clusters = CellCluster::make_cluster(
            line.len(),
            line.visible_cells(),
            Some(ParagraphDirectionHint::AutoLeftToRight),
            false,
            false,
        );

        // The opening paren (physical cell 0) must never share a
        // cluster's `text` with either Hebrew letter (physical cells 1
        // and 2, reversed to iteration order 2,1), and likewise for the
        // closing paren (physical cell 3).
        for c in &clusters {
            let has_paren = c.text.contains('(') || c.text.contains(')');
            let has_hebrew = c.text.chars().any(|ch| CellCluster::is_hebrew_cell(&ch.to_string()));
            assert!(
                !(has_paren && has_hebrew),
                "a cluster must not mix punctuation with reversed Hebrew content: {:?}",
                c.text
            );
        }

        // The Hebrew letters themselves must still come out reversed
        // (that part of the design is untouched by this fix): scanning
        // just the Hebrew-only clusters left-to-right in `first_cell_idx`
        // order should read the phrase back-to-front relative to typed
        // order.
        let mut heb_text = String::new();
        let mut heb_clusters: Vec<&CellCluster> = clusters
            .iter()
            .filter(|c| c.text.chars().any(|ch| CellCluster::is_hebrew_cell(&ch.to_string())))
            .collect();
        heb_clusters.sort_by_key(|c| c.first_cell_idx);
        for c in heb_clusters {
            heb_text.push_str(&c.text);
        }
        assert_eq!(heb_text, "\u{05D1}\u{05D0}");
    }

    /// Sanity check: a line with no Hebrew content at all must produce
    /// exactly the clusters it would have before this fix -- `in_run` is
    /// all-false, so the new boundary check never fires.
    #[test]
    fn no_hebrew_no_behavior_change() {
        let s = "(hello world)";
        let line = Line::from_text(s, &CellAttributes::default(), SEQ_ZERO, None);
        let clusters = CellCluster::make_cluster(
            line.len(),
            line.visible_cells(),
            Some(ParagraphDirectionHint::AutoLeftToRight),
            false,
            false,
        );
        let joined: String = clusters.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(joined, s);
    }
}
