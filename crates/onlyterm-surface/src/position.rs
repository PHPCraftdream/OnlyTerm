use core::cmp::min;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};

/// Position holds 0-based positioning information, where
/// Absolute(0) is the start of the line or column,
/// Relative(0) is the current position in the line or
/// column and EndRelative(0) is the end position in the
/// line or column.
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Position {
    /// Negative values move up, positive values down, 0 means no change
    Relative(isize),
    /// Relative to the start of the line or top of the screen
    Absolute(usize),
    /// Relative to the end of line or bottom of screen
    EndRelative(usize),
}

/// Applies a Position update to either the x or y position.
/// The value is clamped to be in the range: 0..limit
pub(crate) fn compute_position_change(current: usize, pos: &Position, limit: usize) -> usize {
    use Position::*;
    match pos {
        Relative(delta) => {
            if *delta >= 0 {
                min(
                    current.saturating_add(*delta as usize),
                    limit.saturating_sub(1),
                )
            } else {
                current.saturating_sub((*delta).unsigned_abs())
            }
        }
        Absolute(abs) => min(*abs, limit.saturating_sub(1)),
        EndRelative(delta) => limit.saturating_sub(*delta),
    }
}
