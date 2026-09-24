use alloc::string::ToString;
use onlyterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, FromDynamic, ToDynamic, PartialEq, Eq, Clone, Copy)]
pub enum IntegratedTitleButton {
    Hide,
    Maximize,
    Close,
}
