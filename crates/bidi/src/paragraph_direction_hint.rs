use crate::Direction;
use alloc::string::ToString;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum ParagraphDirectionHint {
    #[default]
    LeftToRight,
    RightToLeft,
    /// Attempt to auto-detect but fall back to LTR
    AutoLeftToRight,
    /// Attempt to auto-detect but fall back to RTL
    AutoRightToLeft,
}

impl ParagraphDirectionHint {
    /// Returns just the direction portion of the hint, independent
    /// of the auto-detection state.
    pub fn direction(self) -> Direction {
        match self {
            ParagraphDirectionHint::AutoLeftToRight | ParagraphDirectionHint::LeftToRight => {
                Direction::LeftToRight
            }
            ParagraphDirectionHint::AutoRightToLeft | ParagraphDirectionHint::RightToLeft => {
                Direction::RightToLeft
            }
        }
    }
}
