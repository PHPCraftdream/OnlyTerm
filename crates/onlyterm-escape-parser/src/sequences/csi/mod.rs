use super::OneBased;
use crate::color::{AnsiColor, ColorSpec, RgbColor, SrgbaTuple};
use core::convert::TryInto;
use core::fmt::{Display, Error as FmtError, Formatter};
use num_traits::{FromPrimitive, ToPrimitive};
use onlyterm_input_types::Modifiers;

use crate::allocate::*;

pub use vtparse::CsiParam;

mod commands;
mod parser;
#[cfg(all(test, feature = "std"))]
mod test;

pub use self::commands::*;
use self::parser::{Cracked, EncodeCSIParam, ParamEnum};

pub use onlyterm_input_types::KittyKeyboardFlags;

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
