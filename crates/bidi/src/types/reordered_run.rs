use crate::{Direction, Level};
use alloc::vec::Vec;
use core::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReorderedRun {
    /// The direction for this run.  Derived from the level.
    pub direction: Direction,

    /// Embedding level of this run.
    pub level: Level,

    /// The starting and ending codepoint indices for this run
    pub range: Range<usize>,

    /// The indices in their adjusted order
    pub indices: Vec<usize>,
}
