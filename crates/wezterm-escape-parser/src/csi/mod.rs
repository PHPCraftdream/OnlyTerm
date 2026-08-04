use super::OneBased;
use crate::color::{AnsiColor, ColorSpec, RgbColor, SrgbaTuple};
use core::convert::TryInto;
use core::fmt::{Display, Error as FmtError, Formatter};
use num_traits::{FromPrimitive, ToPrimitive};
use wezterm_input_types::Modifiers;

use crate::allocate::*;

pub use vtparse::CsiParam;

mod cursor;
mod device;
mod edit;
mod keyboard;
mod misc;
mod mode;
mod mouse;
mod sgr;
mod style;
mod window;
#[cfg(all(test, feature = "std"))]
mod test;

pub use self::cursor::*;
pub use self::device::*;
pub use self::edit::*;
pub use self::keyboard::*;
pub use self::misc::*;
pub use self::mode::*;
pub use self::mouse::*;
pub use self::sgr::*;
pub use self::style::*;
pub use self::window::*;

pub use wezterm_input_types::KittyKeyboardFlags;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CSI {
    /// SGR: Set Graphics Rendition.
    /// These values affect how the character is rendered.
    Sgr(Sgr),

    /// CSI codes that relate to the cursor
    Cursor(Cursor),

    Edit(Edit),

    Mode(Mode),

    Device(Box<Device>),

    Mouse(MouseReport),

    Window(Box<Window>),

    Keyboard(Keyboard),

    /// ECMA-48 SCP
    SelectCharacterPath(CharacterPath, i64),

    /// Unknown or unspecified; should be rare and is rather
    /// large, so it is boxed and kept outside of the enum
    /// body to help reduce space usage in the common cases.
    Unspecified(Box<Unspecified>),
}

#[cfg(all(test, target_pointer_width = "64"))]
#[test]
fn csi_size() {
    assert_eq!(core::mem::size_of::<Sgr>(), 24);
    assert_eq!(core::mem::size_of::<Cursor>(), 12);
    assert_eq!(core::mem::size_of::<Edit>(), 8);
    assert_eq!(core::mem::size_of::<Mode>(), 24);
    assert_eq!(core::mem::size_of::<MouseReport>(), 8);
    assert_eq!(core::mem::size_of::<Window>(), 40);
    assert_eq!(core::mem::size_of::<Keyboard>(), 8);
    assert_eq!(core::mem::size_of::<CSI>(), 32);
}

impl Display for CSI {
    // TODO: data size optimization opportunity: if we could somehow know that we
    // had a run of CSI instances being encoded in sequence, we could
    // potentially collapse them together.  This is a few bytes difference in
    // practice so it may not be worthwhile with modern networks.
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        write!(f, "\x1b[")?;
        match self {
            CSI::Sgr(sgr) => sgr.fmt(f)?,
            CSI::Cursor(c) => c.fmt(f)?,
            CSI::Edit(e) => e.fmt(f)?,
            CSI::Mode(mode) => mode.fmt(f)?,
            CSI::Unspecified(unspec) => unspec.fmt(f)?,
            CSI::Mouse(mouse) => mouse.fmt(f)?,
            CSI::Device(dev) => dev.fmt(f)?,
            CSI::Window(window) => window.fmt(f)?,
            CSI::Keyboard(Keyboard::SetKittyState { flags, mode }) => {
                write!(f, "={};{}u", flags.bits(), *mode as u16)?
            }
            CSI::Keyboard(Keyboard::PushKittyState { flags, mode }) => {
                write!(f, ">{};{}u", flags.bits(), *mode as u16)?
            }
            CSI::Keyboard(Keyboard::PopKittyState(n)) => write!(f, "<{}u", *n)?,
            CSI::Keyboard(Keyboard::QueryKittySupport) => write!(f, "?u")?,
            CSI::Keyboard(Keyboard::ReportKittyState(flags)) => write!(f, "?{}u", flags.bits())?,
            CSI::SelectCharacterPath(path, n) => {
                let a = match path {
                    CharacterPath::ImplementationDefault => 0,
                    CharacterPath::LeftToRightOrTopToBottom => 1,
                    CharacterPath::RightToLeftOrBottomToTop => 2,
                };
                match (a, n) {
                    (0, 0) => write!(f, " k")?,
                    (a, 0) => write!(f, "{} k", a)?,
                    (a, n) => write!(f, "{};{} k", a, n)?,
                }
            }
        };
        Ok(())
    }
}

trait EncodeCSIParam {
    fn write_csi(&self, f: &mut Formatter, control: &str) -> Result<(), FmtError>;
}

impl<T: ParamEnum + PartialEq + ToPrimitive> EncodeCSIParam for T {
    fn write_csi(&self, f: &mut Formatter, control: &str) -> Result<(), FmtError> {
        if *self == ParamEnum::default() {
            write!(f, "{}", control)
        } else {
            let value = self.to_i64().ok_or(FmtError)?;
            write!(f, "{}{}", value, control)
        }
    }
}

impl EncodeCSIParam for u32 {
    fn write_csi(&self, f: &mut Formatter, control: &str) -> Result<(), FmtError> {
        if *self == 1 {
            write!(f, "{}", control)
        } else {
            write!(f, "{}{}", *self, control)
        }
    }
}

impl EncodeCSIParam for OneBased {
    fn write_csi(&self, f: &mut Formatter, control: &str) -> Result<(), FmtError> {
        if self.as_one_based() == 1 {
            write!(f, "{}", control)
        } else {
            write!(f, "{}{}", *self, control)
        }
    }
}

/// This trait aids in parsing escape sequences.
/// In many cases we simply want to collect integral values >= 1,
/// but in some we build out an enum.  The trait helps to generalize
/// the parser code while keeping it relatively terse.
trait ParseParams: Sized {
    fn parse_params(params: &[CsiParam]) -> Result<Self, ()>;
}

/// Parse an input parameter into a 1-based unsigned value
impl ParseParams for u32 {
    fn parse_params(params: &[CsiParam]) -> Result<u32, ()> {
        match params {
            [] => Ok(1),
            [p] => to_1b_u32(p),
            _ => Err(()),
        }
    }
}

/// Parse an input parameter into a 1-based unsigned value
impl ParseParams for OneBased {
    fn parse_params(params: &[CsiParam]) -> Result<OneBased, ()> {
        match params {
            [] => Ok(OneBased::new(1)),
            [p] => OneBased::from_esc_param(p),
            _ => Err(()),
        }
    }
}

/// Parse a pair of 1-based unsigned values into a tuple.
/// This is typically used to build a struct comprised of
/// the pair of values.
impl ParseParams for (OneBased, OneBased) {
    fn parse_params(params: &[CsiParam]) -> Result<(OneBased, OneBased), ()> {
        match params {
            [] | [CsiParam::P(b';')] => Ok((OneBased::new(1), OneBased::new(1))),
            [a] | [a, CsiParam::P(b';')] => Ok((OneBased::from_esc_param(a)?, OneBased::new(1))),
            [a, CsiParam::P(b';'), b] => {
                Ok((OneBased::from_esc_param(a)?, OneBased::from_esc_param(b)?))
            }
            [CsiParam::P(b';'), b] => Ok((OneBased::new(1), OneBased::from_esc_param(b)?)),
            _ => Err(()),
        }
    }
}

/// This is ostensibly a marker trait that is used within this module
/// to denote an enum.  It does double duty as a stand-in for Default.
/// We need separate traits for this to disambiguate from a regular
/// primitive integer.
trait ParamEnum: FromPrimitive {
    fn default() -> Self;
}

/// implement ParseParams for the enums that also implement ParamEnum.
impl<T: ParamEnum> ParseParams for T {
    fn parse_params(params: &[CsiParam]) -> Result<Self, ()> {
        match params {
            [] => Ok(ParamEnum::default()),
            [CsiParam::Integer(i)] => FromPrimitive::from_i64(*i).ok_or(()),
            _ => Err(()),
        }
    }
}


/// Constrol Sequence Initiator (CSI) Parser.
/// Since many sequences allow for composition of actions by separating
/// `;` character, we need to be able to iterate over
/// the set of parsed actions from a given CSI sequence.
/// `CSIParser` implements an Iterator that yields `CSI` instances as
/// it parses them out from the input sequence.
struct CSIParser<'a> {
    /// this flag is set when more than two intermediates
    /// arrived and subsequent characters were ignored.
    parameters_truncated: bool,
    control: char,
    /// While params is_some we have more data to consume.  The advance_by
    /// method updates the slice as we consume data.
    /// In a number of cases an empty params list is used to indicate
    /// default values, especially for SGR, so we need to be careful not
    /// to update params to an empty slice.
    params: Option<&'a [CsiParam]>,
    orig_params: &'a [CsiParam],
}

impl CSI {
    /// Parse a CSI sequence.
    /// Returns an iterator that yields individual CSI actions.
    /// Why not a single?  Because sequences like `CSI [ 1 ; 3 m`
    /// embed two separate actions but are sent as a single unit.
    /// If no semantic meaning is known for a subsequence, the remainder
    /// of the sequence is returned wrapped in a `CSI::Unspecified` container.
    pub fn parse<'a>(
        params: &'a [CsiParam],
        parameters_truncated: bool,
        control: char,
    ) -> impl Iterator<Item = CSI> + 'a {
        CSIParser {
            parameters_truncated,
            control,
            params: Some(params),
            orig_params: params,
        }
    }
}

/// A little helper to convert i64 -> u8 if safe
fn to_u8(v: &CsiParam) -> Result<u8, ()> {
    match v {
        CsiParam::P(_) => Err(()),
        CsiParam::Integer(v) => {
            if *v <= i64::from(u8::MAX) {
                Ok(*v as u8)
            } else {
                Err(())
            }
        }
    }
}

/// Convert the input value to 1-based u32.
/// The intent is to protect consumers from out of range values
/// when operating on the data, while balancing strictness with
/// practical implementation bugs.  For example, it is common
/// to see 0 values being emitted from existing libraries, and
/// we desire to see the intended output.
/// Ensures that the value is in the range 1..=max_value.
/// If the input is 0 it is treated as 1.  If the value is
/// otherwise outside that range, an error is propagated and
/// that will typically case the sequence to be reported via
/// the Unspecified placeholder.
fn to_1b_u32(v: &CsiParam) -> Result<u32, ()> {
    match v {
        CsiParam::Integer(v) if *v == 0 => Ok(1),
        CsiParam::Integer(v) if *v > 0 && *v <= i64::from(u32::MAX) => Ok(*v as u32),
        _ => Err(()),
    }
}

struct Cracked {
    pub(super) params: Vec<Option<CsiParam>>,
}

impl Cracked {
    pub fn parse(params: &[CsiParam]) -> Result<Self, ()> {
        let mut res = vec![];
        let mut iter = params.iter().peekable();
        while let Some(p) = iter.next() {
            match p {
                CsiParam::P(b';') => {
                    res.push(None);
                }
                CsiParam::Integer(_) => {
                    res.push(Some(*p));
                    if let Some(CsiParam::P(b';')) = iter.peek() {
                        iter.next();
                    }
                }
                _ => return Err(()),
            }
        }
        Ok(Self { params: res })
    }

    pub fn get(&self, idx: usize) -> Option<&CsiParam> {
        self.params.get(idx)?.as_ref()
    }

    pub fn opt_int(&self, idx: usize) -> Option<i64> {
        self.get(idx).and_then(CsiParam::as_integer)
    }

    pub fn int(&self, idx: usize) -> Result<i64, ()> {
        self.get(idx).and_then(CsiParam::as_integer).ok_or(())
    }

    pub fn len(&self) -> usize {
        self.params.len()
    }
}

macro_rules! noparams {
    ($ns:ident, $variant:ident, $params:expr) => {{
        if $params.len() != 0 {
            Err(())
        } else {
            Ok(CSI::$ns($ns::$variant))
        }
    }};
}

macro_rules! parse {
    ($ns:ident, $variant:ident, $params:expr) => {{
        let value = ParseParams::parse_params($params)?;
        Ok(CSI::$ns($ns::$variant(value)))
    }};

    ($ns:ident, $variant:ident, $first:ident, $second:ident, $params:expr) => {{
        let (p1, p2): (OneBased, OneBased) = ParseParams::parse_params($params)?;
        Ok(CSI::$ns($ns::$variant {
            $first: p1,
            $second: p2,
        }))
    }};
}

impl<'a> CSIParser<'a> {
    fn parse_next(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match (self.control, self.orig_params) {
            ('k', [.., CsiParam::P(b' ')]) => self.select_character_path(params),
            ('q', [.., CsiParam::P(b' ')]) => self.cursor_style(params),
            ('y', [.., CsiParam::P(b'*')]) => self.checksum_area(params),

            ('c', [CsiParam::P(b'='), ..]) => self
                .req_tertiary_device_attributes(params)
                .map(|dev| CSI::Device(Box::new(dev))),
            ('c', [CsiParam::P(b'>'), ..]) => self
                .req_secondary_device_attributes(params)
                .map(|dev| CSI::Device(Box::new(dev))),

            ('m', [CsiParam::P(b'<'), ..]) | ('M', [CsiParam::P(b'<'), ..]) => {
                self.mouse_sgr1006(params).map(CSI::Mouse)
            }

            ('c', [CsiParam::P(b'?'), ..]) => self
                .secondary_device_attributes(params)
                .map(|dev| CSI::Device(Box::new(dev))),

            ('S', [CsiParam::P(b'?'), ..]) => XtSmGraphics::parse(params),
            ('p', [CsiParam::Integer(_), CsiParam::P(b'$')])
            | ('p', [CsiParam::P(b'?'), CsiParam::Integer(_), CsiParam::P(b'$')]) => {
                self.decrqm(params)
            }
            ('h', [CsiParam::P(b'?'), ..]) => self
                .dec(self.focus(params, 1, 0))
                .map(|mode| CSI::Mode(Mode::SetDecPrivateMode(mode))),
            ('l', [CsiParam::P(b'?'), ..]) => self
                .dec(self.focus(params, 1, 0))
                .map(|mode| CSI::Mode(Mode::ResetDecPrivateMode(mode))),
            ('r', [CsiParam::P(b'?'), ..]) => self
                .dec(self.focus(params, 1, 0))
                .map(|mode| CSI::Mode(Mode::RestoreDecPrivateMode(mode))),
            ('q', [CsiParam::P(b'>'), ..]) => self
                .req_terminal_name_and_version(params)
                .map(|dev| CSI::Device(Box::new(dev))),
            ('s', [CsiParam::P(b'?'), ..]) => self
                .dec(self.focus(params, 1, 0))
                .map(|mode| CSI::Mode(Mode::SaveDecPrivateMode(mode))),
            ('m', [CsiParam::P(b'>'), ..]) => self.xterm_key_modifier(params),

            ('p', [CsiParam::P(b'!')]) => Ok(CSI::Device(Box::new(Device::SoftReset))),
            ('u', [CsiParam::P(b'='), CsiParam::Integer(flags)]) => {
                Ok(CSI::Keyboard(Keyboard::SetKittyState {
                    flags: KittyKeyboardFlags::from_bits_truncate(
                        (*flags).try_into().map_err(|_| ())?,
                    ),
                    mode: KittyKeyboardMode::AssignAll,
                }))
            }
            (
                'u',
                [
                    CsiParam::P(b'='),
                    CsiParam::Integer(flags),
                    CsiParam::P(b';'),
                    CsiParam::Integer(mode),
                ],
            ) => Ok(CSI::Keyboard(Keyboard::SetKittyState {
                flags: KittyKeyboardFlags::from_bits_truncate((*flags).try_into().map_err(|_| ())?),
                mode: match *mode {
                    1 => KittyKeyboardMode::AssignAll,
                    2 => KittyKeyboardMode::SetSpecified,
                    3 => KittyKeyboardMode::ClearSpecified,
                    _ => return Err(()),
                },
            })),
            ('u', [CsiParam::P(b'>')]) => Ok(CSI::Keyboard(Keyboard::PushKittyState {
                flags: KittyKeyboardFlags::NONE,
                mode: KittyKeyboardMode::AssignAll,
            })),
            ('u', [CsiParam::P(b'>'), CsiParam::Integer(flags)]) => {
                Ok(CSI::Keyboard(Keyboard::PushKittyState {
                    flags: KittyKeyboardFlags::from_bits_truncate(
                        (*flags).try_into().map_err(|_| ())?,
                    ),
                    mode: KittyKeyboardMode::AssignAll,
                }))
            }
            (
                'u',
                [
                    CsiParam::P(b'>'),
                    CsiParam::Integer(flags),
                    CsiParam::P(b';'),
                    CsiParam::Integer(mode),
                ],
            ) => Ok(CSI::Keyboard(Keyboard::PushKittyState {
                flags: KittyKeyboardFlags::from_bits_truncate((*flags).try_into().map_err(|_| ())?),
                mode: match *mode {
                    1 => KittyKeyboardMode::AssignAll,
                    2 => KittyKeyboardMode::SetSpecified,
                    3 => KittyKeyboardMode::ClearSpecified,
                    _ => return Err(()),
                },
            })),
            ('u', [CsiParam::P(b'?')]) => Ok(CSI::Keyboard(Keyboard::QueryKittySupport)),
            ('u', [CsiParam::P(b'?'), CsiParam::Integer(flags)]) => {
                Ok(CSI::Keyboard(Keyboard::ReportKittyState(
                    KittyKeyboardFlags::from_bits_truncate((*flags).try_into().map_err(|_| ())?),
                )))
            }
            ('u', [CsiParam::P(b'<'), CsiParam::Integer(how_many)]) => Ok(CSI::Keyboard(
                Keyboard::PopKittyState((*how_many).try_into().map_err(|_| ())?),
            )),
            ('u', [CsiParam::P(b'<')]) => Ok(CSI::Keyboard(Keyboard::PopKittyState(1))),

            _ => match self.control {
                'c' => self
                    .req_primary_device_attributes(params)
                    .map(|dev| CSI::Device(Box::new(dev))),

                '@' => parse!(Edit, InsertCharacter, params),
                '`' => parse!(Cursor, CharacterPositionAbsolute, params),
                'A' => parse!(Cursor, Up, params),
                'B' => parse!(Cursor, Down, params),
                'C' => parse!(Cursor, Right, params),
                'D' => parse!(Cursor, Left, params),
                'E' => parse!(Cursor, NextLine, params),
                'F' => parse!(Cursor, PrecedingLine, params),
                'G' => parse!(Cursor, CharacterAbsolute, params),
                'H' => parse!(Cursor, Position, line, col, params),
                'I' => parse!(Cursor, ForwardTabulation, params),
                'J' => parse!(Edit, EraseInDisplay, params),
                'K' => parse!(Edit, EraseInLine, params),
                'L' => parse!(Edit, InsertLine, params),
                'M' => parse!(Edit, DeleteLine, params),
                'P' => parse!(Edit, DeleteCharacter, params),
                'R' => parse!(Cursor, ActivePositionReport, line, col, params),
                'S' => parse!(Edit, ScrollUp, params),
                'T' => parse!(Edit, ScrollDown, params),
                'W' => parse!(Cursor, TabulationControl, params),
                'X' => parse!(Edit, EraseCharacter, params),
                'Y' => parse!(Cursor, LineTabulation, params),
                'Z' => parse!(Cursor, BackwardTabulation, params),

                'a' => parse!(Cursor, CharacterPositionForward, params),
                'b' => parse!(Edit, Repeat, params),
                'd' => parse!(Cursor, LinePositionAbsolute, params),
                'e' => parse!(Cursor, LinePositionForward, params),
                'f' => parse!(Cursor, CharacterAndLinePosition, line, col, params),
                'g' => parse!(Cursor, TabulationClear, params),
                'h' => self
                    .terminal_mode(params)
                    .map(|mode| CSI::Mode(Mode::SetMode(mode))),
                'j' => parse!(Cursor, CharacterPositionBackward, params),
                'k' => parse!(Cursor, LinePositionBackward, params),
                'l' => self
                    .terminal_mode(params)
                    .map(|mode| CSI::Mode(Mode::ResetMode(mode))),

                'm' => self.sgr(params).map(CSI::Sgr),
                'n' => self.dsr(params),
                'r' => self.decstbm(params),
                's' => self.decslrm(params),
                't' => self.window(params).map(|p| CSI::Window(Box::new(p))),
                'u' => noparams!(Cursor, RestoreCursor, params),
                'x' => self
                    .req_terminal_parameters(params)
                    .map(|dev| CSI::Device(Box::new(dev))),

                _ => Err(()),
            },
        }
    }

    /// Consume some number of elements from params and update it.
    /// Take care to avoid setting params back to an empty slice
    /// as this would trigger returning a default value and/or
    /// an unterminated parse loop.
    fn advance_by<T>(&mut self, n: usize, params: &'a [CsiParam], result: T) -> T {
        let n = if matches!(params.get(n), Some(CsiParam::P(b';'))) {
            n + 1
        } else {
            n
        };

        let (_, next) = params.split_at(n);
        if !next.is_empty() {
            self.params = Some(next);
        }
        result
    }

    fn focus(&self, params: &'a [CsiParam], from_start: usize, from_end: usize) -> &'a [CsiParam] {
        if params == self.orig_params {
            let len = params.len();
            &params[from_start..len - from_end]
        } else {
            params
        }
    }

    fn select_character_path(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        fn path(n: i64) -> Result<CharacterPath, ()> {
            Ok(match n {
                0 => CharacterPath::ImplementationDefault,
                1 => CharacterPath::LeftToRightOrTopToBottom,
                2 => CharacterPath::RightToLeftOrBottomToTop,
                _ => return Err(()),
            })
        }

        match params {
            [CsiParam::P(b' ')] => Ok(self.advance_by(
                1,
                params,
                CSI::SelectCharacterPath(CharacterPath::ImplementationDefault, 0),
            )),
            [CsiParam::Integer(a), CsiParam::P(b' ')] => {
                Ok(self.advance_by(2, params, CSI::SelectCharacterPath(path(*a)?, 0)))
            }
            [
                CsiParam::Integer(a),
                CsiParam::P(b';'),
                CsiParam::Integer(b),
                CsiParam::P(b' '),
            ] => Ok(self.advance_by(4, params, CSI::SelectCharacterPath(path(*a)?, *b))),
            _ => Err(()),
        }
    }

    fn cursor_style(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match params {
            [CsiParam::Integer(p), CsiParam::P(b' ')] => match FromPrimitive::from_i64(*p) {
                None => Err(()),
                Some(style) => {
                    Ok(self.advance_by(2, params, CSI::Cursor(Cursor::CursorStyle(style))))
                }
            },
            _ => Err(()),
        }
    }

    fn checksum_area(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        let params = Cracked::parse(&params[..params.len() - 1])?;

        let request_id = params.int(0)?;
        let page_number = params.int(1)?;
        let top = OneBased::from_optional_esc_param(params.get(2))?;
        let left = OneBased::from_optional_esc_param(params.get(3))?;
        let bottom = OneBased::from_optional_esc_param(params.get(4))?;
        let right = OneBased::from_optional_esc_param(params.get(5))?;
        Ok(CSI::Window(Box::new(Window::ChecksumRectangularArea {
            request_id,
            page_number,
            top,
            left,
            bottom,
            right,
        })))
    }

    fn dsr(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match params {
            [CsiParam::Integer(5)] => {
                Ok(self.advance_by(1, params, CSI::Device(Box::new(Device::StatusReport))))
            }

            [CsiParam::Integer(6)] => {
                Ok(self.advance_by(1, params, CSI::Cursor(Cursor::RequestActivePositionReport)))
            }
            _ => Err(()),
        }
    }

    fn decstbm(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match params {
            [] => Ok(CSI::Cursor(Cursor::SetTopAndBottomMargins {
                top: OneBased::new(1),
                bottom: OneBased::new(u32::MAX),
            })),
            [p] => Ok(self.advance_by(
                1,
                params,
                CSI::Cursor(Cursor::SetTopAndBottomMargins {
                    top: OneBased::from_esc_param(p)?,
                    bottom: OneBased::new(u32::MAX),
                }),
            )),
            [a, CsiParam::P(b';'), b] => Ok(self.advance_by(
                3,
                params,
                CSI::Cursor(Cursor::SetTopAndBottomMargins {
                    top: OneBased::from_esc_param(a)?,
                    bottom: OneBased::from_esc_param_with_big_default(b)?,
                }),
            )),
            [CsiParam::P(b';'), b] => Ok(self.advance_by(
                2,
                params,
                CSI::Cursor(Cursor::SetTopAndBottomMargins {
                    top: OneBased::new(1),
                    bottom: OneBased::from_esc_param_with_big_default(b)?,
                }),
            )),
            _ => Err(()),
        }
    }

    fn xterm_key_modifier(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match params {
            [CsiParam::P(b'>'), a, CsiParam::P(b';'), b] => {
                let resource =
                    XtermKeyModifierResource::parse(a.as_integer().ok_or(())?).ok_or(())?;
                Ok(self.advance_by(
                    4,
                    params,
                    CSI::Mode(Mode::XtermKeyMode {
                        resource,
                        value: Some(b.as_integer().ok_or(())?),
                    }),
                ))
            }
            [CsiParam::P(b'>'), a, CsiParam::P(b';')] => {
                let resource =
                    XtermKeyModifierResource::parse(a.as_integer().ok_or(())?).ok_or(())?;
                Ok(self.advance_by(
                    3,
                    params,
                    CSI::Mode(Mode::XtermKeyMode {
                        resource,
                        value: None,
                    }),
                ))
            }
            [CsiParam::P(b'>'), p] => {
                let resource =
                    XtermKeyModifierResource::parse(p.as_integer().ok_or(())?).ok_or(())?;
                Ok(self.advance_by(
                    2,
                    params,
                    CSI::Mode(Mode::XtermKeyMode {
                        resource,
                        value: None,
                    }),
                ))
            }
            _ => Err(()),
        }
    }

    fn decslrm(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        match params {
            [] => {
                // with no params this is a request to save the cursor
                // and is technically in conflict with SetLeftAndRightMargins.
                // The emulator needs to decide based on DECSLRM mode
                // whether this saves the cursor or is SetLeftAndRightMargins
                // with default parameters!
                Ok(CSI::Cursor(Cursor::SaveCursor))
            }
            [p] => Ok(self.advance_by(
                1,
                params,
                CSI::Cursor(Cursor::SetLeftAndRightMargins {
                    left: OneBased::from_esc_param(p)?,
                    right: OneBased::new(u32::MAX),
                }),
            )),
            [a, CsiParam::P(b';'), b] => Ok(self.advance_by(
                3,
                params,
                CSI::Cursor(Cursor::SetLeftAndRightMargins {
                    left: OneBased::from_esc_param(a)?,
                    right: OneBased::from_esc_param(b)?,
                }),
            )),
            [CsiParam::P(b';'), b] => Ok(self.advance_by(
                2,
                params,
                CSI::Cursor(Cursor::SetLeftAndRightMargins {
                    left: OneBased::new(1),
                    right: OneBased::from_esc_param(b)?,
                }),
            )),
            _ => Err(()),
        }
    }

    fn req_primary_device_attributes(&mut self, params: &'a [CsiParam]) -> Result<Device, ()> {
        match params {
            [] => Ok(Device::RequestPrimaryDeviceAttributes),
            [CsiParam::Integer(0)] => {
                Ok(self.advance_by(1, params, Device::RequestPrimaryDeviceAttributes))
            }
            _ => Err(()),
        }
    }

    fn req_terminal_name_and_version(&mut self, params: &'a [CsiParam]) -> Result<Device, ()> {
        match params {
            [_] => Ok(Device::RequestTerminalNameAndVersion),

            [_, CsiParam::Integer(0)] => {
                Ok(self.advance_by(2, params, Device::RequestTerminalNameAndVersion))
            }
            _ => Err(()),
        }
    }

    fn req_secondary_device_attributes(&mut self, params: &'a [CsiParam]) -> Result<Device, ()> {
        match params {
            [CsiParam::P(b'>')] => Ok(Device::RequestSecondaryDeviceAttributes),
            [CsiParam::P(b'>'), CsiParam::Integer(0)] => {
                Ok(self.advance_by(2, params, Device::RequestSecondaryDeviceAttributes))
            }
            _ => Err(()),
        }
    }

    fn req_tertiary_device_attributes(&mut self, params: &'a [CsiParam]) -> Result<Device, ()> {
        match params {
            [CsiParam::P(b'=')] => Ok(Device::RequestTertiaryDeviceAttributes),
            [CsiParam::P(b'='), CsiParam::Integer(0)] => {
                Ok(self.advance_by(2, params, Device::RequestTertiaryDeviceAttributes))
            }
            _ => Err(()),
        }
    }

    fn secondary_device_attributes(&mut self, params: &'a [CsiParam]) -> Result<Device, ()> {
        match params {
            [
                _,
                CsiParam::Integer(1),
                CsiParam::P(b';'),
                CsiParam::Integer(0),
            ] => Ok(self.advance_by(
                4,
                params,
                Device::DeviceAttributes(DeviceAttributes::Vt101WithNoOptions),
            )),
            [_, CsiParam::Integer(6)] => {
                Ok(self.advance_by(2, params, Device::DeviceAttributes(DeviceAttributes::Vt102)))
            }
            [
                _,
                CsiParam::Integer(1),
                CsiParam::P(b';'),
                CsiParam::Integer(2),
            ] => Ok(self.advance_by(
                4,
                params,
                Device::DeviceAttributes(DeviceAttributes::Vt100WithAdvancedVideoOption),
            )),
            [_, CsiParam::Integer(62), ..] => Ok(self.advance_by(
                params.len(),
                params,
                Device::DeviceAttributes(DeviceAttributes::Vt220(
                    DeviceAttributeFlags::from_params(&params[2..]),
                )),
            )),
            [_, CsiParam::Integer(63), ..] => Ok(self.advance_by(
                params.len(),
                params,
                Device::DeviceAttributes(DeviceAttributes::Vt320(
                    DeviceAttributeFlags::from_params(&params[2..]),
                )),
            )),
            [_, CsiParam::Integer(64), ..] => Ok(self.advance_by(
                params.len(),
                params,
                Device::DeviceAttributes(DeviceAttributes::Vt420(
                    DeviceAttributeFlags::from_params(&params[2..]),
                )),
            )),
            _ => Err(()),
        }
    }

    fn req_terminal_parameters(&mut self, params: &'a [CsiParam]) -> Result<Device, ()> {
        match params {
            [] | [CsiParam::Integer(0)] => Ok(Device::RequestTerminalParameters(0)),
            [CsiParam::Integer(1)] => Ok(Device::RequestTerminalParameters(1)),
            _ => Err(()),
        }
    }

    /// Parse extended mouse reports known as SGR 1006 mode
    fn mouse_sgr1006(&mut self, params: &'a [CsiParam]) -> Result<MouseReport, ()> {
        let (p0, p1, p2) = match params {
            [
                CsiParam::P(b'<'),
                CsiParam::Integer(p0),
                CsiParam::P(b';'),
                CsiParam::Integer(p1),
                CsiParam::P(b';'),
                CsiParam::Integer(p2),
            ] => (*p0, *p1, *p2),
            _ => return Err(()),
        };

        // 'M' encodes a press, 'm' a release.
        let button = match (self.control, p0 & 0b110_0011) {
            ('M', 0) => MouseButton::Button1Press,
            ('m', 0) => MouseButton::Button1Release,
            ('M', 1) => MouseButton::Button2Press,
            ('m', 1) => MouseButton::Button2Release,
            ('M', 2) => MouseButton::Button3Press,
            ('m', 2) => MouseButton::Button3Release,
            ('M', 64) => MouseButton::Button4Press,
            ('m', 64) => MouseButton::Button4Release,
            ('M', 65) => MouseButton::Button5Press,
            ('m', 65) => MouseButton::Button5Release,
            ('M', 66) => MouseButton::Button6Press,
            ('m', 66) => MouseButton::Button6Release,
            ('M', 67) => MouseButton::Button7Press,
            ('m', 67) => MouseButton::Button7Release,
            ('M', 32) => MouseButton::Button1Drag,
            ('M', 33) => MouseButton::Button2Drag,
            ('M', 34) => MouseButton::Button3Drag,
            // Note that there is some theoretical ambiguity with these None values.
            // The ambiguity stems from alternative encodings of the mouse protocol;
            // when set to SGR1006 mode the variants with the `3` parameter do not
            // occur.  They included here as a reminder for when support for those
            // other encodings is added and this block is likely copied and pasted
            // or refactored for re-use with them.
            ('M', 35) => MouseButton::None, // mouse motion with no buttons
            ('m', 35) => MouseButton::None, // mouse motion with no buttons (in Windows Terminal)
            ('M', 3) => MouseButton::None,  // legacy notification about button release
            ('m', 3) => MouseButton::None,  // release+press doesn't make sense
            _ => {
                return Err(());
            }
        };

        let mut modifiers = Modifiers::NONE;
        if p0 & 4 != 0 {
            modifiers |= Modifiers::SHIFT;
        }
        if p0 & 8 != 0 {
            modifiers |= Modifiers::ALT;
        }
        if p0 & 16 != 0 {
            modifiers |= Modifiers::CTRL;
        }

        Ok(self.advance_by(
            6,
            params,
            MouseReport::SGR1006 {
                x: p1 as u16,
                y: p2 as u16,
                button,
                modifiers,
            },
        ))
    }

    fn decrqm(&mut self, params: &'a [CsiParam]) -> Result<CSI, ()> {
        Ok(CSI::Mode(match params {
            [CsiParam::Integer(p), CsiParam::P(b'$')] => {
                Mode::QueryMode(match FromPrimitive::from_i64(*p) {
                    None => TerminalMode::Unspecified(p.to_u16().ok_or(())?),
                    Some(mode) => TerminalMode::Code(mode),
                })
            }
            [CsiParam::P(b'?'), CsiParam::Integer(p), CsiParam::P(b'$')] => {
                Mode::QueryDecPrivateMode(match FromPrimitive::from_i64(*p) {
                    None => DecPrivateMode::Unspecified(p.to_u16().ok_or(())?),
                    Some(mode) => DecPrivateMode::Code(mode),
                })
            }
            _ => return Err(()),
        }))
    }

    fn dec(&mut self, params: &'a [CsiParam]) -> Result<DecPrivateMode, ()> {
        match params {
            [CsiParam::Integer(p0), ..] => match FromPrimitive::from_i64(*p0) {
                None => Ok(self.advance_by(
                    1,
                    params,
                    DecPrivateMode::Unspecified(p0.to_u16().ok_or(())?),
                )),
                Some(mode) => Ok(self.advance_by(1, params, DecPrivateMode::Code(mode))),
            },
            _ => Err(()),
        }
    }

    fn terminal_mode(&mut self, params: &'a [CsiParam]) -> Result<TerminalMode, ()> {
        let p0 = params.first().and_then(CsiParam::as_integer).ok_or(())?;
        match FromPrimitive::from_i64(p0) {
            None => {
                Ok(self.advance_by(1, params, TerminalMode::Unspecified(p0.to_u16().ok_or(())?)))
            }
            Some(mode) => Ok(self.advance_by(1, params, TerminalMode::Code(mode))),
        }
    }

    fn parse_sgr_color(&mut self, params: &'a [CsiParam]) -> Result<ColorSpec, ()> {
        match params {
            // wezterm extension to support an optional alpha channel in the `:` form only
            [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(6),
                CsiParam::P(b':'),
                CsiParam::Integer(_colorspace),
                CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                CsiParam::P(b':'),
                alpha,
                ..,
            ] => {
                let res: SrgbaTuple =
                    (to_u8(red)?, to_u8(green)?, to_u8(blue)?, to_u8(alpha)?).into();
                Ok(self.advance_by(13, params, res.into()))
            }
            [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(6),
                CsiParam::P(b':'),
                /* empty colorspace */ CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                CsiParam::P(b':'),
                alpha,
                ..,
            ] => {
                let res: SrgbaTuple =
                    (to_u8(red)?, to_u8(green)?, to_u8(blue)?, to_u8(alpha)?).into();
                Ok(self.advance_by(12, params, res.into()))
            }
            [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(6),
                CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                CsiParam::P(b':'),
                alpha,
                ..,
            ] => {
                let res: SrgbaTuple =
                    (to_u8(red)?, to_u8(green)?, to_u8(blue)?, to_u8(alpha)?).into();
                Ok(self.advance_by(11, params, res.into()))
            }

            // standard sgr colors
            [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(2),
                CsiParam::P(b':'),
                CsiParam::Integer(_colorspace),
                CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                ..,
            ] => {
                let res = RgbColor::new_8bpc(to_u8(red)?, to_u8(green)?, to_u8(blue)?).into();
                Ok(self.advance_by(11, params, res))
            }

            [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(2),
                CsiParam::P(b':'),
                /* empty colorspace */ CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                ..,
            ] => {
                let res = RgbColor::new_8bpc(to_u8(red)?, to_u8(green)?, to_u8(blue)?).into();
                Ok(self.advance_by(10, params, res))
            }

            [
                _,
                CsiParam::P(b';'),
                CsiParam::Integer(2),
                CsiParam::P(b';'),
                red,
                CsiParam::P(b';'),
                green,
                CsiParam::P(b';'),
                blue,
                ..,
            ]
            | [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(2),
                CsiParam::P(b':'),
                red,
                CsiParam::P(b':'),
                green,
                CsiParam::P(b':'),
                blue,
                ..,
            ] => {
                let res = RgbColor::new_8bpc(to_u8(red)?, to_u8(green)?, to_u8(blue)?).into();
                Ok(self.advance_by(9, params, res))
            }

            [
                _,
                CsiParam::P(b';'),
                CsiParam::Integer(5),
                CsiParam::P(b';'),
                idx,
                ..,
            ]
            | [
                _,
                CsiParam::P(b':'),
                CsiParam::Integer(5),
                CsiParam::P(b':'),
                idx,
                ..,
            ] => Ok(self.advance_by(5, params, ColorSpec::PaletteIndex(to_u8(idx)?))),
            _ => Err(()),
        }
    }

    fn window(&mut self, params: &'a [CsiParam]) -> Result<Window, ()> {
        let params = Cracked::parse(params)?;

        let p = params.int(0)?;
        let arg1 = params.opt_int(1);
        let arg2 = params.opt_int(2);

        match p {
            1 => Ok(Window::DeIconify),
            2 => Ok(Window::Iconify),
            3 => Ok(Window::MoveWindow {
                x: arg1.unwrap_or(0),
                y: arg2.unwrap_or(0),
            }),
            4 => Ok(Window::ResizeWindowPixels {
                height: arg1,
                width: arg2,
            }),
            5 => Ok(Window::RaiseWindow),
            6 => match params.len() {
                1 => Ok(Window::LowerWindow),
                _ => Ok(Window::ReportCellSizePixelsResponse {
                    height: arg1,
                    width: arg2,
                }),
            },
            7 => Ok(Window::RefreshWindow),
            8 => Ok(Window::ResizeWindowCells {
                height: arg1,
                width: arg2,
            }),
            9 => match arg1 {
                Some(0) => Ok(Window::RestoreMaximizedWindow),
                Some(1) => Ok(Window::MaximizeWindow),
                Some(2) => Ok(Window::MaximizeWindowVertically),
                Some(3) => Ok(Window::MaximizeWindowHorizontally),
                _ => Err(()),
            },
            10 => match arg1 {
                Some(0) => Ok(Window::UndoFullScreenMode),
                Some(1) => Ok(Window::ChangeToFullScreenMode),
                Some(2) => Ok(Window::ToggleFullScreen),
                _ => Err(()),
            },
            11 => Ok(Window::ReportWindowState),
            13 => match arg1 {
                None => Ok(Window::ReportWindowPosition),
                Some(2) => Ok(Window::ReportTextAreaPosition),
                _ => Err(()),
            },
            14 => match arg1 {
                None => Ok(Window::ReportTextAreaSizePixels),
                Some(2) => Ok(Window::ReportWindowSizePixels),
                _ => Err(()),
            },
            15 => Ok(Window::ReportScreenSizePixels),
            16 => Ok(Window::ReportCellSizePixels),
            18 => Ok(Window::ReportTextAreaSizeCells),
            19 => Ok(Window::ReportScreenSizeCells),
            20 => Ok(Window::ReportIconLabel),
            21 => Ok(Window::ReportWindowTitle),
            22 => match arg1 {
                Some(0) => Ok(Window::PushIconAndWindowTitle),
                Some(1) => Ok(Window::PushIconTitle),
                Some(2) => Ok(Window::PushWindowTitle),
                _ => Err(()),
            },
            23 => match arg1 {
                Some(0) => Ok(Window::PopIconAndWindowTitle),
                Some(1) => Ok(Window::PopIconTitle),
                Some(2) => Ok(Window::PopWindowTitle),
                _ => Err(()),
            },
            _ => Err(()),
        }
    }

    fn underline(&mut self, params: &'a [CsiParam]) -> Result<Sgr, ()> {
        let (sgr, n) = match params {
            [_, CsiParam::P(b':'), CsiParam::Integer(0), ..] => {
                (Sgr::Underline(Underline::None), 3)
            }
            [_, CsiParam::P(b':'), CsiParam::Integer(1), ..] => {
                (Sgr::Underline(Underline::Single), 3)
            }
            [_, CsiParam::P(b':'), CsiParam::Integer(2), ..] => {
                (Sgr::Underline(Underline::Double), 3)
            }
            [_, CsiParam::P(b':'), CsiParam::Integer(3), ..] => {
                (Sgr::Underline(Underline::Curly), 3)
            }
            [_, CsiParam::P(b':'), CsiParam::Integer(4), ..] => {
                (Sgr::Underline(Underline::Dotted), 3)
            }
            [_, CsiParam::P(b':'), CsiParam::Integer(5), ..] => {
                (Sgr::Underline(Underline::Dashed), 3)
            }
            _ => (Sgr::Underline(Underline::Single), 1),
        };

        Ok(self.advance_by(n, params, sgr))
    }

    fn sgr(&mut self, params: &'a [CsiParam]) -> Result<Sgr, ()> {
        if params.is_empty() {
            // With no parameters, treat as equivalent to Reset.
            Ok(Sgr::Reset)
        } else {
            for p in params {
                match p {
                    CsiParam::P(b';')
                    | CsiParam::P(b':')
                    | CsiParam::P(b'?')
                    | CsiParam::Integer(_) => {}
                    _ => return Err(()),
                }
            }

            // Consume a single parameter and return the parsed result
            macro_rules! one {
                ($t:expr) => {
                    Ok(self.advance_by(1, params, $t))
                };
            }

            match &params[0] {
                CsiParam::P(b';') => {
                    // Starting with an empty item is equivalent to a reset
                    self.advance_by(1, params, Ok(Sgr::Reset))
                }

                // There are a small number of DEC private SGR parameters that
                // have equivalents in the normal SGR space.
                // We're simply inlining recognizing them here, and mapping them
                // to those SGR equivalents. That makes parsing "lossy" in the
                // sense that the original sequence is lost, but semantically,
                // the result is the same.
                // These codes are taken from the "SGR" section of
                // "Digital ANSI-Compliant Printing Protocol
                // Level 2 Programming Reference Manual"
                // on page 7-78.
                // <https://vaxhaven.com/images/f/f7/EK-PPLV2-PM-B01.pdf>
                /* Withdrawn because xterm introduced a conflict:
                 * <https://github.com/mintty/mintty/issues/1171#issuecomment-1336174469>
                 * <https://github.com/mintty/mintty/issues/1189>
                CsiParam::P(b'?') if params.len() > 1 => match &params[1] {
                    // Consume two parameters and return the parsed result
                    macro_rules! two {
                        ($t:expr) => {
                            Ok(self.advance_by(2, params, $t))
                        };
                    }
                    CsiParam::Integer(i) => match FromPrimitive::from_i64(*i) {
                        None => Err(()),
                        Some(code) => match code {
                            0 => two!(Sgr::Reset),
                            4 => two!(Sgr::VerticalAlign(VerticalAlign::SuperScript)),
                            5 => two!(Sgr::VerticalAlign(VerticalAlign::SubScript)),
                            6 => two!(Sgr::Overline(true)),
                            24 => two!(Sgr::VerticalAlign(VerticalAlign::BaseLine)),
                            26 => two!(Sgr::Overline(false)),
                            _ => Err(()),
                        },
                    },
                    _ => Err(()),
                },
                */
                CsiParam::P(_) => Err(()),
                CsiParam::Integer(i) => match FromPrimitive::from_i64(*i) {
                    None => Err(()),
                    Some(sgr) => match sgr {
                        SgrCode::Reset => one!(Sgr::Reset),
                        SgrCode::IntensityBold => one!(Sgr::Intensity(Intensity::Bold)),
                        SgrCode::IntensityDim => one!(Sgr::Intensity(Intensity::Half)),
                        SgrCode::NormalIntensity => one!(Sgr::Intensity(Intensity::Normal)),
                        SgrCode::UnderlineOn => {
                            self.underline(params) //.map(Sgr::Underline)
                        }
                        SgrCode::UnderlineDouble => one!(Sgr::Underline(Underline::Double)),
                        SgrCode::UnderlineOff => one!(Sgr::Underline(Underline::None)),
                        SgrCode::UnderlineColor => {
                            self.parse_sgr_color(params).map(Sgr::UnderlineColor)
                        }
                        SgrCode::ResetUnderlineColor => {
                            one!(Sgr::UnderlineColor(ColorSpec::default()))
                        }
                        SgrCode::BlinkOn => one!(Sgr::Blink(Blink::Slow)),
                        SgrCode::RapidBlinkOn => one!(Sgr::Blink(Blink::Rapid)),
                        SgrCode::BlinkOff => one!(Sgr::Blink(Blink::None)),
                        SgrCode::ItalicOn => one!(Sgr::Italic(true)),
                        SgrCode::ItalicOff => one!(Sgr::Italic(false)),
                        SgrCode::VerticalAlignSuperScript => {
                            one!(Sgr::VerticalAlign(VerticalAlign::SuperScript))
                        }
                        SgrCode::VerticalAlignSubScript => {
                            one!(Sgr::VerticalAlign(VerticalAlign::SubScript))
                        }
                        SgrCode::VerticalAlignBaseLine => {
                            one!(Sgr::VerticalAlign(VerticalAlign::BaseLine))
                        }
                        SgrCode::ForegroundColor => {
                            self.parse_sgr_color(params).map(Sgr::Foreground)
                        }
                        SgrCode::ForegroundBlack => one!(Sgr::Foreground(AnsiColor::Black.into())),
                        SgrCode::ForegroundRed => one!(Sgr::Foreground(AnsiColor::Maroon.into())),
                        SgrCode::ForegroundGreen => one!(Sgr::Foreground(AnsiColor::Green.into())),
                        SgrCode::ForegroundYellow => one!(Sgr::Foreground(AnsiColor::Olive.into())),
                        SgrCode::ForegroundBlue => one!(Sgr::Foreground(AnsiColor::Navy.into())),
                        SgrCode::ForegroundMagenta => {
                            one!(Sgr::Foreground(AnsiColor::Purple.into()))
                        }
                        SgrCode::ForegroundCyan => one!(Sgr::Foreground(AnsiColor::Teal.into())),
                        SgrCode::ForegroundWhite => one!(Sgr::Foreground(AnsiColor::Silver.into())),
                        SgrCode::ForegroundDefault => one!(Sgr::Foreground(ColorSpec::Default)),
                        SgrCode::ForegroundBrightBlack => {
                            one!(Sgr::Foreground(AnsiColor::Grey.into()))
                        }
                        SgrCode::ForegroundBrightRed => {
                            one!(Sgr::Foreground(AnsiColor::Red.into()))
                        }
                        SgrCode::ForegroundBrightGreen => {
                            one!(Sgr::Foreground(AnsiColor::Lime.into()))
                        }
                        SgrCode::ForegroundBrightYellow => {
                            one!(Sgr::Foreground(AnsiColor::Yellow.into()))
                        }
                        SgrCode::ForegroundBrightBlue => {
                            one!(Sgr::Foreground(AnsiColor::Blue.into()))
                        }
                        SgrCode::ForegroundBrightMagenta => {
                            one!(Sgr::Foreground(AnsiColor::Fuchsia.into()))
                        }
                        SgrCode::ForegroundBrightCyan => {
                            one!(Sgr::Foreground(AnsiColor::Aqua.into()))
                        }
                        SgrCode::ForegroundBrightWhite => {
                            one!(Sgr::Foreground(AnsiColor::White.into()))
                        }

                        SgrCode::BackgroundColor => {
                            self.parse_sgr_color(params).map(Sgr::Background)
                        }
                        SgrCode::BackgroundBlack => one!(Sgr::Background(AnsiColor::Black.into())),
                        SgrCode::BackgroundRed => one!(Sgr::Background(AnsiColor::Maroon.into())),
                        SgrCode::BackgroundGreen => one!(Sgr::Background(AnsiColor::Green.into())),
                        SgrCode::BackgroundYellow => one!(Sgr::Background(AnsiColor::Olive.into())),
                        SgrCode::BackgroundBlue => one!(Sgr::Background(AnsiColor::Navy.into())),
                        SgrCode::BackgroundMagenta => {
                            one!(Sgr::Background(AnsiColor::Purple.into()))
                        }
                        SgrCode::BackgroundCyan => one!(Sgr::Background(AnsiColor::Teal.into())),
                        SgrCode::BackgroundWhite => one!(Sgr::Background(AnsiColor::Silver.into())),
                        SgrCode::BackgroundDefault => one!(Sgr::Background(ColorSpec::Default)),
                        SgrCode::BackgroundBrightBlack => {
                            one!(Sgr::Background(AnsiColor::Grey.into()))
                        }
                        SgrCode::BackgroundBrightRed => {
                            one!(Sgr::Background(AnsiColor::Red.into()))
                        }
                        SgrCode::BackgroundBrightGreen => {
                            one!(Sgr::Background(AnsiColor::Lime.into()))
                        }
                        SgrCode::BackgroundBrightYellow => {
                            one!(Sgr::Background(AnsiColor::Yellow.into()))
                        }
                        SgrCode::BackgroundBrightBlue => {
                            one!(Sgr::Background(AnsiColor::Blue.into()))
                        }
                        SgrCode::BackgroundBrightMagenta => {
                            one!(Sgr::Background(AnsiColor::Fuchsia.into()))
                        }
                        SgrCode::BackgroundBrightCyan => {
                            one!(Sgr::Background(AnsiColor::Aqua.into()))
                        }
                        SgrCode::BackgroundBrightWhite => {
                            one!(Sgr::Background(AnsiColor::White.into()))
                        }

                        SgrCode::InverseOn => one!(Sgr::Inverse(true)),
                        SgrCode::InverseOff => one!(Sgr::Inverse(false)),
                        SgrCode::InvisibleOn => one!(Sgr::Invisible(true)),
                        SgrCode::InvisibleOff => one!(Sgr::Invisible(false)),
                        SgrCode::StrikeThroughOn => one!(Sgr::StrikeThrough(true)),
                        SgrCode::StrikeThroughOff => one!(Sgr::StrikeThrough(false)),
                        SgrCode::OverlineOn => one!(Sgr::Overline(true)),
                        SgrCode::OverlineOff => one!(Sgr::Overline(false)),
                        SgrCode::DefaultFont => one!(Sgr::Font(Font::Default)),
                        SgrCode::AltFont1 => one!(Sgr::Font(Font::Alternate(1))),
                        SgrCode::AltFont2 => one!(Sgr::Font(Font::Alternate(2))),
                        SgrCode::AltFont3 => one!(Sgr::Font(Font::Alternate(3))),
                        SgrCode::AltFont4 => one!(Sgr::Font(Font::Alternate(4))),
                        SgrCode::AltFont5 => one!(Sgr::Font(Font::Alternate(5))),
                        SgrCode::AltFont6 => one!(Sgr::Font(Font::Alternate(6))),
                        SgrCode::AltFont7 => one!(Sgr::Font(Font::Alternate(7))),
                        SgrCode::AltFont8 => one!(Sgr::Font(Font::Alternate(8))),
                        SgrCode::AltFont9 => one!(Sgr::Font(Font::Alternate(9))),
                    },
                },
            }
        }
    }
}

impl<'a> Iterator for CSIParser<'a> {
    type Item = CSI;

    fn next(&mut self) -> Option<CSI> {
        let params = self.params.take()?;

        match self.parse_next(params) {
            Ok(csi) => Some(csi),
            Err(()) => Some(CSI::Unspecified(Box::new(Unspecified {
                params: params.to_vec(),
                parameters_truncated: self.parameters_truncated,
                control: self.control,
            }))),
        }
    }
}


