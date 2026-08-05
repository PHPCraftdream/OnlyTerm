use alloc::string::ToString;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, Default, FromDynamic, ToDynamic, PartialEq, Eq, Clone, Copy)]
pub enum IntegratedTitleButtonAlignment {
    #[default]
    Right,
    Left,
}
