use crate::alloc::string::ToString;
use crate::color::ColorAttribute;
use alloc::sync::Arc;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};
use wezterm_dynamic::{FromDynamic, ToDynamic};
use wezterm_escape_parser::csi::{Blink, Intensity, Underline};
use wezterm_escape_parser::osc::Hyperlink;

/// Models a change in the attributes of a cell in a stream of changes.
/// Each variant specifies one of the possible attributes; the corresponding
/// value holds the new value to be used for that attribute.
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Eq, PartialEq, FromDynamic, ToDynamic)]
pub enum AttributeChange {
    Intensity(Intensity),
    Underline(Underline),
    Italic(bool),
    Blink(Blink),
    Reverse(bool),
    StrikeThrough(bool),
    Invisible(bool),
    Foreground(ColorAttribute),
    Background(ColorAttribute),
    Hyperlink(Option<Arc<Hyperlink>>),
}
