use super::*;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};
use bitflags::bitflags;

bitflags! {
    #[cfg_attr(feature="use_serde", derive(Serialize, Deserialize))]
    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    pub struct MouseButtons: u8 {
        const NONE = 0;
        const LEFT = 1<<1;
        const RIGHT = 1<<2;
        const MIDDLE = 1<<3;
        const VERT_WHEEL = 1<<4;
        const HORZ_WHEEL = 1<<5;
        /// if set then the wheel movement was in the positive
        /// direction, else the negative direction
        const WHEEL_POSITIVE = 1<<6;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseButton {
    Button1Press,
    Button2Press,
    Button3Press,
    Button4Press,
    Button5Press,
    Button6Press,
    Button7Press,
    Button1Release,
    Button2Release,
    Button3Release,
    Button4Release,
    Button5Release,
    Button6Release,
    Button7Release,
    Button1Drag,
    Button2Drag,
    Button3Drag,
    None,
}

impl From<MouseButton> for MouseButtons {
    fn from(button: MouseButton) -> MouseButtons {
        match button {
            MouseButton::Button1Press | MouseButton::Button1Drag => MouseButtons::LEFT,
            MouseButton::Button2Press | MouseButton::Button2Drag => MouseButtons::MIDDLE,
            MouseButton::Button3Press | MouseButton::Button3Drag => MouseButtons::RIGHT,
            MouseButton::Button4Press => MouseButtons::VERT_WHEEL | MouseButtons::WHEEL_POSITIVE,
            MouseButton::Button5Press => MouseButtons::VERT_WHEEL,
            MouseButton::Button6Press => MouseButtons::HORZ_WHEEL | MouseButtons::WHEEL_POSITIVE,
            MouseButton::Button7Press => MouseButtons::HORZ_WHEEL,
            _ => MouseButtons::NONE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseReport {
    SGR1006 {
        x: u16,
        y: u16,
        button: MouseButton,
        modifiers: Modifiers,
    },
    SGR1016 {
        x_pixels: u16,
        y_pixels: u16,
        button: MouseButton,
        modifiers: Modifiers,
    },
}

impl Display for MouseReport {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        match self {
            MouseReport::SGR1006 {
                x,
                y,
                button,
                modifiers,
            } => {
                let mut b = 0;
                if (*modifiers & Modifiers::SHIFT) != Modifiers::NONE {
                    b |= 4;
                }
                if (*modifiers & Modifiers::ALT) != Modifiers::NONE {
                    b |= 8;
                }
                if (*modifiers & Modifiers::CTRL) != Modifiers::NONE {
                    b |= 16;
                }
                b |= match button {
                    MouseButton::Button1Press | MouseButton::Button1Release => 0,
                    MouseButton::Button2Press | MouseButton::Button2Release => 1,
                    MouseButton::Button3Press | MouseButton::Button3Release => 2,
                    MouseButton::Button4Press | MouseButton::Button4Release => 64,
                    MouseButton::Button5Press | MouseButton::Button5Release => 65,
                    MouseButton::Button6Press | MouseButton::Button6Release => 66,
                    MouseButton::Button7Press | MouseButton::Button7Release => 67,
                    MouseButton::Button1Drag => 32,
                    MouseButton::Button2Drag => 33,
                    MouseButton::Button3Drag => 34,
                    MouseButton::None => 35,
                };
                let trailer = match button {
                    MouseButton::Button1Press
                    | MouseButton::Button2Press
                    | MouseButton::Button3Press
                    | MouseButton::Button4Press
                    | MouseButton::Button5Press
                    | MouseButton::Button1Drag
                    | MouseButton::Button2Drag
                    | MouseButton::Button3Drag
                    | MouseButton::None => 'M',
                    _ => 'm',
                };
                write!(f, "<{};{};{}{}", b, x, y, trailer)
            }
            MouseReport::SGR1016 {
                x_pixels,
                y_pixels,
                button,
                modifiers,
            } => {
                let mut b = 0;
                if (*modifiers & Modifiers::SHIFT) != Modifiers::NONE {
                    b |= 4;
                }
                if (*modifiers & Modifiers::ALT) != Modifiers::NONE {
                    b |= 8;
                }
                if (*modifiers & Modifiers::CTRL) != Modifiers::NONE {
                    b |= 16;
                }
                b |= match button {
                    MouseButton::Button1Press | MouseButton::Button1Release => 0,
                    MouseButton::Button2Press | MouseButton::Button2Release => 1,
                    MouseButton::Button3Press | MouseButton::Button3Release => 2,
                    MouseButton::Button4Press | MouseButton::Button4Release => 64,
                    MouseButton::Button5Press | MouseButton::Button5Release => 65,
                    MouseButton::Button6Press | MouseButton::Button6Release => 66,
                    MouseButton::Button7Press | MouseButton::Button7Release => 67,
                    MouseButton::Button1Drag => 32,
                    MouseButton::Button2Drag => 33,
                    MouseButton::Button3Drag => 34,
                    MouseButton::None => 35,
                };
                let trailer = match button {
                    MouseButton::Button1Press
                    | MouseButton::Button2Press
                    | MouseButton::Button3Press
                    | MouseButton::Button4Press
                    | MouseButton::Button5Press
                    | MouseButton::Button1Drag
                    | MouseButton::Button2Drag
                    | MouseButton::Button3Drag
                    | MouseButton::None => 'M',
                    _ => 'm',
                };
                write!(f, "<{};{};{}{}", b, x_pixels, y_pixels, trailer)
            }
        }
    }
}
