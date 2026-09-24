//! Rules L1, L2 and L3: reset whitespace at segment/line boundaries and
//! reorder the resolved levels into visual order.

use super::BidiContext;
use crate::types::level::MAX_DEPTH;
use crate::{BidiClass, Direction, Level};
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

/// Placeholder codepoint index that corresponds to NO_LEVEL
const DELETED: usize = usize::MAX;

impl BidiContext {
    /// This function runs Rule L1.
    ///
    /// The strategy here for Rule L1 is to scan forward through
    /// the text searching for segment separators or paragraph
    /// separators. If a segment separator or paragraph
    /// separator is found, it is reset to the paragraph embedding
    /// level. Then scan backwards from the separator to
    /// find any contiguous stretch of whitespace characters
    /// and reset any which are found to the paragraph embedding
    /// level, as well. When we reach the *last* character in the
    /// text (which will also constitute, by definition, the last
    /// character in the line being processed here), check if it
    /// is whitespace. If so, reset it to the paragraph embedding
    /// level. Then scan backwards to find any contiguous stretch
    /// of whitespace characters and reset those as well.
    ///
    /// These checks for whitespace are done with the *original*
    /// Bidi_Class values for characters, not the resolved values.
    ///
    /// As for many rules, this rule simply ignores any character
    /// whose level has been set to NO_LEVEL, which is the way
    /// this reference algorithm "deletes" boundary neutrals and
    /// embedding and override controls from the text.
    pub(crate) fn reset_whitespace_levels(&self, line_range: Range<usize>) -> Vec<Level> {
        fn reset_contiguous_whitespace_before(
            line_range: Range<usize>,
            base_level: Level,
            orig_char_types: &[BidiClass],
            levels: &mut [Level],
        ) {
            for i in line_range.rev() {
                if orig_char_types[i] == BidiClass::WhiteSpace
                    || orig_char_types[i].is_iso_control()
                {
                    levels[i] = base_level;
                } else if levels[i].removed_by_x9() {
                    // Skip over deleted entries
                } else {
                    // end of contiguous section
                    break;
                }
            }
        }

        let mut levels = self.levels.clone();

        for (idx, orig_bc) in self
            .orig_char_types
            .iter()
            .enumerate()
            .skip(line_range.start)
            .take(line_range.end - line_range.start)
        {
            match orig_bc {
                // Explicit boundary
                BidiClass::SegmentSeparator | BidiClass::ParagraphSeparator => {
                    levels[idx] = self.base_level;
                    reset_contiguous_whitespace_before(
                        line_range.start..idx,
                        self.base_level,
                        &self.orig_char_types,
                        &mut levels,
                    );
                }
                _ => {}
            }
        }

        reset_contiguous_whitespace_before(
            line_range.clone(),
            self.base_level,
            &self.orig_char_types,
            &mut levels,
        );

        levels[line_range].to_vec()
    }

    /// Performs Rule L3.
    /// This rule is optional and must be enabled by calling the
    /// set_reorder_non_spacing_marks method
    fn reorder_non_spacing_marks(&self, levels: &mut [Level], visual: &mut [usize]) {
        let mut idx = levels.len() - 1;
        loop {
            if idx > 0
                && !levels[idx].removed_by_x9()
                && levels[idx].direction() == Direction::RightToLeft
                && self.orig_char_types[visual[idx]] == BidiClass::NonspacingMark
            {
                // Keep scanning backwards within this level
                let level = levels[idx];
                let seq_end = idx;

                idx -= 1;
                while idx > 0 && levels[idx].removed_by_x9()
                    || (levels[idx] == level
                        && matches!(
                            self.orig_char_types[visual[idx]],
                            BidiClass::LeftToRightEmbedding
                                | BidiClass::RightToLeftEmbedding
                                | BidiClass::LeftToRightOverride
                                | BidiClass::RightToLeftOverride
                                | BidiClass::PopDirectionalFormat
                                | BidiClass::BoundaryNeutral
                                | BidiClass::NonspacingMark
                        ))
                {
                    idx -= 1;
                }

                if levels[idx] != level {
                    idx += 1;
                }

                if seq_end > idx {
                    visual[idx..=seq_end].reverse();
                    levels[idx..=seq_end].reverse();
                }
            }

            if idx == 0 {
                return;
            }
            idx -= 1;
        }
    }

    /// This function runs Rule L2.
    ///
    /// Find the highest level among the resolved levels.
    /// Then from that highest level down to the lowest odd
    /// level, reverse any contiguous runs at that level or higher.
    // Rule L2: the loop index doubles as a character position value
    // (e.g. `i + first_cidx`), not merely an index into `levels`, so an
    // iterator rewrite would obscure the spec logic.
    #[allow(clippy::needless_range_loop)]
    pub(crate) fn reverse_levels(&self, first_cidx: usize, levels: &mut [Level]) -> Vec<usize> {
        // Not typed as Level because the Step trait required by the loop
        // below is nightly only
        let mut highest_level = 0;
        let mut lowest_odd_level = MAX_DEPTH as i8 + 1;
        let mut no_levels = true;

        for &level in levels.iter() {
            if level.removed_by_x9() {
                continue;
            }

            // Found something other than NO_LEVEL
            no_levels = false;
            highest_level = highest_level.max(level.0);
            if level.0 % 2 == 1 && level.0 < lowest_odd_level {
                lowest_odd_level = level.0;
            }
        }

        if no_levels {
            return vec![];
        }

        // Initial visual order
        let mut visual = vec![];
        for i in 0..levels.len() {
            if levels[i].removed_by_x9() {
                visual.push(DELETED);
            } else {
                visual.push(i + first_cidx);
            }
        }

        // Apply L3. UAX9 has this occur after L2, but we do it
        // before that for consistency with FriBidi's implementation.
        if self.reorder_nsm {
            self.reorder_non_spacing_marks(levels, &mut visual);
        }

        // Apply L2.
        for level in (lowest_odd_level..=highest_level).rev() {
            let level = Level(level);
            let mut i = 0;
            let mut in_range = false;
            let mut significant_range = false;
            let mut first_pos = None;
            let mut last_pos = None;

            while i < levels.len() {
                if levels[i] >= level {
                    if !in_range {
                        in_range = true;
                        first_pos.replace(i);
                    } else {
                        // Hit a second explicit level
                        significant_range = true;
                        last_pos.replace(i);
                    }
                } else if levels[i].removed_by_x9() {
                    // Don't break ranges for deleted controls
                    if in_range {
                        last_pos.replace(i);
                    }
                } else {
                    // End of a range.  Reset the range flag
                    // and rever the range.
                    in_range = false;
                    match (last_pos, first_pos, significant_range) {
                        (Some(last_pos), Some(first_pos), true) if last_pos > first_pos => {
                            visual[first_pos..=last_pos].reverse();
                        }
                        _ => {}
                    }
                    first_pos = None;
                    last_pos = None;
                }
                i += 1;
            }

            if in_range && significant_range {
                match (last_pos, first_pos) {
                    (Some(last_pos), Some(first_pos)) if last_pos > first_pos => {
                        visual[first_pos..=last_pos].reverse();
                    }
                    _ => {}
                }
            }
        }

        visual.retain(|&i| i != DELETED);
        visual
    }
}
