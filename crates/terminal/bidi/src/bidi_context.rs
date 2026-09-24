//! Bidi context: the UAX #9 driver.
//!
//! `BidiContext` owns the mutable state (original and resolved bidi classes,
//! resolved levels, the identified runs) and orchestrates the rule pipeline.
//! The individual rule families live in sibling modules:
//!
//! * [`level_stack`] - the BD16 overflow-aware embedding-level stack (X1-X8).
//! * [`explicit`] - rules X1 through X9.
//! * [`paragraph`] - rules P2 and P3.
//! * [`runs`] - rules X10 and BD13, plus the run/sequence data types.
//! * [`resolve`] - rules W1 through W7, N0, N1, N2 and I1/I2.
//! * [`reorder`] - rules L1, L2 and L3.

mod explicit;
mod level_stack;
mod paragraph;
mod reorder;
mod resolve;
mod runs;

use self::paragraph::paragraph_level;
use self::runs::{span_len, Run, RunIter};
use crate::{
    bidi_class_for_char, BidiClass, BidiRun, Direction, Level, ParagraphDirectionHint,
    ReorderedRun, NO_LEVEL,
};
use alloc::borrow::Cow;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;
use log::trace;

#[derive(Debug, Default)]
pub struct BidiContext {
    orig_char_types: Vec<BidiClass>,
    char_types: Vec<BidiClass>,
    levels: Vec<Level>,
    base_level: Level,
    runs: Vec<Run>,
    reorder_nsm: bool,
}

impl BidiContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn base_level(&self) -> Level {
        self.base_level
    }

    /// When `reorder` is set to true, reordering will apply rule L3 to
    /// non-spacing marks.  This is likely more desirable for terminal
    /// based applications than it is for more modern GUI applications
    /// that feed into eg: harfbuzz.
    pub fn set_reorder_non_spacing_marks(&mut self, reorder: bool) {
        self.reorder_nsm = reorder;
    }

    /// Produces a sequence of `BidiRun` structs that represent runs of
    /// text and their direction (and level) across the entire paragraph.
    pub fn runs<'a>(&'a self) -> impl Iterator<Item = BidiRun> + 'a {
        RunIter {
            pos: 0,
            levels: Cow::Borrowed(&self.levels),
            line_range: 0..self.levels.len(),
        }
    }

    /// Given a line_range (a subslice of the current paragraph that represents
    /// a single wrapped line), this method resets whitespace levels for the line
    /// boundaries, and then returns the set of runs for that line.
    pub fn line_runs(&self, line_range: Range<usize>) -> impl Iterator<Item = BidiRun> {
        let levels = self.reset_whitespace_levels(line_range.clone());

        RunIter {
            pos: 0,
            levels: levels.into(),
            line_range,
        }
    }

    pub fn reordered_runs(&self, line_range: Range<usize>) -> Vec<ReorderedRun> {
        // reorder_line's `level` result includes entries that were
        // removed_by_x9() but `reordered` does NOT (for compatibility with
        // the UCD test suite).
        // We need to account for that when we reorder the levels here!
        let (levels, reordered) = self.reorder_line(line_range);
        let mut reordered_levels = vec![Level(NO_LEVEL); reordered.len()];

        for (vis_idx, &log_idx) in reordered.iter().enumerate() {
            reordered_levels[vis_idx] = levels[log_idx];
        }

        reordered_levels.retain(|l| !l.removed_by_x9());

        let mut runs = vec![];

        let mut idx = 0;
        while idx < reordered_levels.len() {
            let len = span_len(idx, &reordered_levels);
            let level = reordered_levels[idx];
            if !level.removed_by_x9() {
                let idx_range = idx..idx + len;
                let start = reordered[idx_range.clone()].iter().min().unwrap();
                let end = reordered[idx_range.clone()].iter().max().unwrap();
                runs.push(ReorderedRun {
                    direction: level.direction(),
                    level,
                    range: *start..*end + 1,
                    indices: reordered[idx_range].to_vec(),
                });
            }
            idx += len;
        }

        runs
    }

    /// `line_range` indicates a contiguous range of character indices
    /// in the paragraph set via `resolve_paragraph`.
    /// This method returns the reordered set of indices for display
    /// purposes.
    pub fn reorder_line(&self, line_range: Range<usize>) -> (Vec<Level>, Vec<usize>) {
        self.dump_state("before L1");
        let mut levels = self.reset_whitespace_levels(line_range.clone());
        assert_eq!(levels.len(), line_range.end - line_range.start);
        let reordered = self.reverse_levels(line_range.start, &mut levels);

        (levels, reordered)
    }

    /// <http://unicode.org/reports/tr9/>
    pub fn resolve_paragraph(&mut self, paragraph: &[char], hint: ParagraphDirectionHint) {
        self.populate_char_types(paragraph);
        self.resolve(hint, paragraph);
    }

    /// BD1: The bidirectional character types are values assigned to each
    /// Unicode character, including unassigned characters
    fn populate_char_types(&mut self, paragraph: &[char]) {
        self.orig_char_types.clear();
        self.orig_char_types.reserve(paragraph.len());
        self.orig_char_types
            .extend(paragraph.iter().map(|&c| bidi_class_for_char(c)));
    }

    pub fn set_char_types(&mut self, char_types: &[BidiClass], hint: ParagraphDirectionHint) {
        self.orig_char_types.clear();
        self.orig_char_types.extend(char_types);
        self.resolve(hint, &[]);
    }

    fn resolve(&mut self, hint: ParagraphDirectionHint, paragraph: &[char]) {
        trace!("\n**** resolve \n");
        self.char_types.clear();
        self.char_types.extend(self.orig_char_types.iter());

        self.base_level = match hint {
            ParagraphDirectionHint::LeftToRight => Level(0),
            ParagraphDirectionHint::RightToLeft => Level(1),
            ParagraphDirectionHint::AutoLeftToRight => {
                paragraph_level(&self.char_types, false, Direction::LeftToRight)
            }
            ParagraphDirectionHint::AutoRightToLeft => {
                paragraph_level(&self.char_types, false, Direction::RightToLeft)
            }
        };

        self.dump_state("before X1-X8");
        self.explicit_embedding_levels();
        self.dump_state("before X9");
        self.delete_format_characters();
        self.dump_state("after X9");
        self.identify_runs();
        let iso_runs = self.identify_isolating_run_sequences();

        self.dump_state("before W1");
        self.resolve_combining_marks(&iso_runs); // W1
        self.dump_state("before W2");
        self.resolve_european_numbers(&iso_runs); // W2
        self.dump_state("before W3");
        self.resolve_arabic_letters(&iso_runs); // W3
        self.dump_state("before W4");
        self.resolve_separators(&iso_runs); // W4
        self.dump_state("before W5");
        self.resolve_terminators(&iso_runs); // W5
        self.dump_state("before W6");
        self.resolve_es_cs_et(&iso_runs); // W6
        self.dump_state("before W7");
        self.resolve_en(&iso_runs); // W7

        self.dump_state("before N0");
        self.resolve_paired_brackets(&iso_runs, paragraph); // N0

        self.dump_state("before N1");
        self.resolve_neutrals_by_context(&iso_runs); // N1
        self.dump_state("before N2");
        self.resolve_neutrals_by_level(&iso_runs); // N2

        self.dump_state("before I1, I2");
        self.resolve_implicit_levels();
    }

    fn dump_state(&self, label: &str) {
        trace!("State: {}", label);
        trace!("BidiClass: {:?}", self.char_types);
        trace!("Levels: {:?}", self.levels);
        trace!("");
    }
}

impl BidiClass {
    pub fn is_iso_init(self) -> bool {
        matches!(
            self,
            BidiClass::RightToLeftIsolate
                | BidiClass::LeftToRightIsolate
                | BidiClass::FirstStrongIsolate
        )
    }

    pub fn is_iso_control(self) -> bool {
        matches!(
            self,
            BidiClass::RightToLeftIsolate
                | BidiClass::LeftToRightIsolate
                | BidiClass::PopDirectionalIsolate
                | BidiClass::FirstStrongIsolate
        )
    }

    pub fn is_neutral(self) -> bool {
        match self {
            BidiClass::OtherNeutral
            | BidiClass::WhiteSpace
            | BidiClass::SegmentSeparator
            | BidiClass::ParagraphSeparator => true,
            _ => self.is_iso_control(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use k9::assert_equal as assert_eq;

    #[test]
    fn runs() {
        let text = vec!['א', 'ב', 'ג', 'a', 'b', 'c'];

        let mut context = BidiContext::new();
        context.resolve_paragraph(&text, ParagraphDirectionHint::AutoLeftToRight);
        k9::snapshot!(
            context.runs().collect::<Vec<_>>(),
            "
[
    BidiRun {
        direction: RightToLeft,
        level: Level(
            1,
        ),
        range: 0..3,
        removed_by_x9: [],
    },
    BidiRun {
        direction: LeftToRight,
        level: Level(
            2,
        ),
        range: 3..6,
        removed_by_x9: [],
    },
]
"
        );
    }

    /// This example is taken from
    /// <https://terminal-wg.pages.freedesktop.org/bidi/recommendation/combining.html>
    #[test]
    fn reorder_nsm() {
        let shalom: Vec<char> = vec![
            '\u{5e9}', '\u{5b8}', '\u{5c1}', '\u{5dc}', '\u{5d5}', '\u{05b9}', '\u{5dd}',
        ];
        let mut context = BidiContext::new();
        context.set_reorder_non_spacing_marks(true);
        context.resolve_paragraph(&shalom, ParagraphDirectionHint::LeftToRight);

        let mut reordered = vec![];
        for run in context.reordered_runs(0..shalom.len()) {
            for idx in run.indices {
                reordered.push(shalom[idx]);
            }
        }

        let explicit_ltr = vec![
            '\u{5dd}', '\u{5d5}', '\u{5b9}', '\u{5dc}', '\u{5e9}', '\u{5b8}', '\u{5c1}',
        ];
        assert_eq!(reordered, explicit_ltr);
    }
}
