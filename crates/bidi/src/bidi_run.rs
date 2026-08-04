use crate::{Direction, Level};
use alloc::vec::Vec;
use core::ops::Range;

/// A `BidiRun` represents a run which is a contiguous sequence of codepoints
/// from the original paragraph that have been resolved to the same embedding
/// level, and that thus all have the same direction.
///
/// The `range` field encapsulates the starting and ending codepoint indices
/// into the original paragraph.
///
/// Note: while the run sequence has the same level throughout, the X9 portion
/// of the bidi algorithm can logically delete some control characters.
/// I haven't been able to prove to myself that those control characters
/// never manifest in the middle of a run, so it is recommended that you use the `indices`
/// method to skip over any such elements if your shaper doesn't want them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidiRun {
    /// The direction for this run.  Derived from the level.
    pub direction: Direction,

    /// Embedding level of this run.
    pub level: Level,

    /// The starting and ending codepoint indices for this run
    pub range: Range<usize>,

    /// the list of control codepoint indices that were removed from the text
    /// by the X9 portion of the bidi algorithm.
    // Expected to have low cardinality and be generally empty, so we're
    // using a simple vec for this.
    pub removed_by_x9: Vec<usize>,
}

impl BidiRun {
    pub fn indices<'a>(&'a self) -> impl Iterator<Item = usize> + 'a {
        struct Iter<'a> {
            range: Range<usize>,
            removed_by_x9: &'a [usize],
        }

        impl<'a> Iterator for Iter<'a> {
            type Item = usize;
            fn next(&mut self) -> Option<usize> {
                for idx in self.range.by_ref() {
                    if self.removed_by_x9.contains(&idx) {
                        // Skip it
                        continue;
                    }
                    return Some(idx);
                }
                None
            }
        }

        Iter {
            range: self.range.clone(),
            removed_by_x9: &self.removed_by_x9,
        }
    }
}
