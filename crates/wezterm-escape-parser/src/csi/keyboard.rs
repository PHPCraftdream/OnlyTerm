use super::*;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u16)]
pub enum KittyKeyboardMode {
    AssignAll = 1,
    SetSpecified = 2,
    ClearSpecified = 3,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Keyboard {
    SetKittyState {
        flags: KittyKeyboardFlags,
        mode: KittyKeyboardMode,
    },
    PushKittyState {
        flags: KittyKeyboardFlags,
        mode: KittyKeyboardMode,
    },
    PopKittyState(u32),
    QueryKittySupport,
    ReportKittyState(KittyKeyboardFlags),
}
