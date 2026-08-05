use crate::alloc::string::ToString;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Hash, Copy, Default, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum CursorVisibility {
    Hidden,
    #[default]
    Visible,
}

#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, FromDynamic, ToDynamic)]
pub enum CursorShape {
    #[default]
    Default,
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

impl CursorShape {
    pub fn is_blinking(self) -> bool {
        matches!(
            self,
            Self::BlinkingBlock | Self::BlinkingUnderline | Self::BlinkingBar
        )
    }
}