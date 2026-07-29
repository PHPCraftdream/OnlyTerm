#![allow(dead_code)]

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u16)]
pub enum Action {
    None = 0,
    Ignore = 1,
    Print = 2,
    Execute = 3,
    Clear = 4,
    Collect = 5,
    Param = 6,
    EscDispatch = 7,
    CsiDispatch = 8,
    Hook = 9,
    Put = 10,
    Unhook = 11,
    OscStart = 12,
    OscPut = 13,
    OscEnd = 14,
    Utf8 = 15,
    ApcStart = 16,
    ApcPut = 17,
    ApcEnd = 18,
}

impl Action {
    #[inline(always)]
    pub fn from_u16(v: u16) -> Self {
        // The transition/entry/exit tables only ever pack valid Action
        // discriminants (0..=18), so every reachable call lands on an explicit
        // arm. The catch-all maps any (impossible) out-of-range value to the
        // zero discriminant instead of invoking UB via transmute.
        match v {
            0 => Action::None,
            1 => Action::Ignore,
            2 => Action::Print,
            3 => Action::Execute,
            4 => Action::Clear,
            5 => Action::Collect,
            6 => Action::Param,
            7 => Action::EscDispatch,
            8 => Action::CsiDispatch,
            9 => Action::Hook,
            10 => Action::Put,
            11 => Action::Unhook,
            12 => Action::OscStart,
            13 => Action::OscPut,
            14 => Action::OscEnd,
            15 => Action::Utf8,
            16 => Action::ApcStart,
            17 => Action::ApcPut,
            _ => Action::ApcEnd,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[repr(u16)]
pub enum State {
    Ground = 0,
    Escape = 1,
    EscapeIntermediate = 2,
    CsiEntry = 3,
    CsiParam = 4,
    CsiIntermediate = 5,
    CsiIgnore = 6,
    DcsEntry = 7,
    DcsParam = 8,
    DcsIntermediate = 9,
    DcsPassthrough = 10,
    DcsIgnore = 11,
    OscString = 12,
    SosPmString = 13,
    ApcString = 14,
    // Special states, always last (no tables for these)
    Anywhere = 15,
    Utf8Sequence = 16,
}

impl State {
    #[inline(always)]
    pub fn from_u16(v: u16) -> Self {
        // The transition/entry/exit tables only ever pack valid State
        // discriminants (0..=16), so every reachable call lands on an explicit
        // arm. The catch-all maps any (impossible) out-of-range value to the
        // zero discriminant instead of invoking UB via transmute.
        match v {
            0 => State::Ground,
            1 => State::Escape,
            2 => State::EscapeIntermediate,
            3 => State::CsiEntry,
            4 => State::CsiParam,
            5 => State::CsiIntermediate,
            6 => State::CsiIgnore,
            7 => State::DcsEntry,
            8 => State::DcsParam,
            9 => State::DcsIntermediate,
            10 => State::DcsPassthrough,
            11 => State::DcsIgnore,
            12 => State::OscString,
            13 => State::SosPmString,
            14 => State::ApcString,
            15 => State::Anywhere,
            _ => State::Utf8Sequence,
        }
    }
}
