use crate::alloc::string::ToString;
use crate::color::ColorAttribute;
use alloc::sync::Arc;
use onlyterm_dynamic::{FromDynamic, ToDynamic};
use onlyterm_escape_parser::csi::{Blink, Intensity, Underline};
use onlyterm_escape_parser::osc::Hyperlink;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};

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
