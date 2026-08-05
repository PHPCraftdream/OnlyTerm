use alloc::string::ToString;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, FromDynamic, ToDynamic, PartialEq, Eq, Clone, Copy)]
pub enum IntegratedTitleButton {
    Hide,
    Maximize,
    Close,
}
