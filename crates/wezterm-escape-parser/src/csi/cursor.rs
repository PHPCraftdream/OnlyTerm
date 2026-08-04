use super::*;
use num_derive::{FromPrimitive, ToPrimitive};

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive, ToPrimitive, Default)]
pub enum CursorStyle {
    #[default]
    Default = 0,
    BlinkingBlock = 1,
    SteadyBlock = 2,
    BlinkingUnderline = 3,
    SteadyUnderline = 4,
    BlinkingBar = 5,
    SteadyBar = 6,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cursor {
    /// CBT Moves cursor to the Ps tabs backward. The default value of Ps is 1.
    BackwardTabulation(u32),

    /// TBC - TABULATION CLEAR
    TabulationClear(TabulationClear),

    /// CHA: Moves cursor to the Ps-th column of the active line. The default
    /// value of Ps is 1.
    CharacterAbsolute(OneBased),

    /// HPA CHARACTER POSITION ABSOLUTE
    /// HPA Moves cursor to the Ps-th column of the active line. The default
    /// value of Ps is 1.
    CharacterPositionAbsolute(OneBased),

    /// HPB - CHARACTER POSITION BACKWARD
    /// HPB Moves cursor to the left Ps columns. The default value of Ps is 1.
    CharacterPositionBackward(u32),

    /// HPR - CHARACTER POSITION FORWARD
    /// HPR Moves cursor to the right Ps columns. The default value of Ps is 1.
    CharacterPositionForward(u32),

    /// HVP - CHARACTER AND LINE POSITION
    /// HVP Moves cursor to the Ps1-th line and to the Ps2-th column. The
    /// default value of Ps1 and Ps2 is 1.
    CharacterAndLinePosition {
        line: OneBased,
        col: OneBased,
    },

    /// VPA - LINE POSITION ABSOLUTE
    /// Move to the corresponding vertical position (line Ps) of the current
    /// column. The default value of Ps is 1.
    LinePositionAbsolute(u32),

    /// VPB - LINE POSITION BACKWARD
    /// Moves cursor up Ps lines in the same column. The default value of Ps is
    /// 1.
    LinePositionBackward(u32),

    /// VPR - LINE POSITION FORWARD
    /// Moves cursor down Ps lines in the same column. The default value of Ps
    /// is 1.
    LinePositionForward(u32),

    /// CHT
    /// Moves cursor to the Ps tabs forward. The default value of Ps is 1.
    ForwardTabulation(u32),

    /// CNL Moves cursor to the first column of Ps-th following line. The
    /// default value of Ps is 1.
    NextLine(u32),

    /// CPL Moves cursor to the first column of Ps-th preceding line. The
    /// default value of Ps is 1.
    PrecedingLine(u32),

    /// CPR - ACTIVE POSITION REPORT
    /// If the DEVICE COMPONENT SELECT MODE (DCSM)
    /// is set to PRESENTATION, CPR is used to report the active presentation
    /// position of the sending device as residing in the presentation
    /// component at the n-th line position according to the line progression
    /// and at the m-th character position according to the character path,
    /// where n equals the value of Pn1 and m equal s the value of Pn2.
    /// If the DEVICE COMPONENT SELECT MODE (DCSM) is set to DATA, CPR is used
    /// to report the active data position of the sending device as
    /// residing in the data component at the n-th line position according
    /// to the line progression and at the m-th character position
    /// according to the character progression, where n equals the value of
    /// Pn1 and m equals the value of Pn2. CPR may be solicited by a DEVICE
    /// STATUS REPORT (DSR) or be sent unsolicited .
    ActivePositionReport {
        line: OneBased,
        col: OneBased,
    },

    /// CPR: this is the request from the client.
    /// The terminal will respond with ActivePositionReport.
    RequestActivePositionReport,

    /// SCP - Save Cursor Position.
    /// Only works when DECLRMM is disabled
    SaveCursor,
    RestoreCursor,

    /// CTC - CURSOR TABULATION CONTROL
    /// CTC causes one or more tabulation stops to be set or cleared in the
    /// presentation component, depending on the parameter values.
    /// In the case of parameter values 0, 2 or 4 the number of lines affected
    /// depends on the setting of the TABULATION STOP MODE (TSM).
    TabulationControl(CursorTabulationControl),

    /// CUB - Cursor Left
    /// Moves cursor to the left Ps columns. The default value of Ps is 1.
    Left(u32),

    /// CUD - Cursor Down
    Down(u32),

    /// CUF - Cursor Right
    Right(u32),

    /// CUP - Cursor Position
    /// Moves cursor to the Ps1-th line and to the Ps2-th column. The default
    /// value of Ps1 and Ps2 is 1.
    Position {
        line: OneBased,
        col: OneBased,
    },

    /// CUU - Cursor Up
    Up(u32),

    /// CVT - Cursor Line Tabulation
    /// CVT causes the active presentation position to be moved to the
    /// corresponding character position of the line corresponding to the n-th
    /// following line tabulation stop in the presentation component, where n
    /// equals the value of Pn.
    LineTabulation(u32),

    /// DECSTBM - Set top and bottom margins.
    SetTopAndBottomMargins {
        top: OneBased,
        bottom: OneBased,
    },

    /// https://vt100.net/docs/vt510-rm/DECSLRM.html
    SetLeftAndRightMargins {
        left: OneBased,
        right: OneBased,
    },

    CursorStyle(CursorStyle),
}

impl Display for Cursor {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        match self {
            Cursor::BackwardTabulation(n) => n.write_csi(f, "Z")?,
            Cursor::CharacterAbsolute(col) => col.write_csi(f, "G")?,
            Cursor::ForwardTabulation(n) => n.write_csi(f, "I")?,
            Cursor::NextLine(n) => n.write_csi(f, "E")?,
            Cursor::PrecedingLine(n) => n.write_csi(f, "F")?,
            Cursor::ActivePositionReport { line, col } => write!(f, "{};{}R", line, col)?,
            Cursor::Left(n) => n.write_csi(f, "D")?,
            Cursor::Down(n) => n.write_csi(f, "B")?,
            Cursor::Right(n) => n.write_csi(f, "C")?,
            Cursor::Up(n) => n.write_csi(f, "A")?,
            Cursor::Position { line, col } => write!(f, "{};{}H", line, col)?,
            Cursor::LineTabulation(n) => n.write_csi(f, "Y")?,
            Cursor::TabulationControl(n) => n.write_csi(f, "W")?,
            Cursor::TabulationClear(n) => n.write_csi(f, "g")?,
            Cursor::CharacterPositionAbsolute(n) => n.write_csi(f, "`")?,
            Cursor::CharacterPositionBackward(n) => n.write_csi(f, "j")?,
            Cursor::CharacterPositionForward(n) => n.write_csi(f, "a")?,
            Cursor::CharacterAndLinePosition { line, col } => write!(f, "{};{}f", line, col)?,
            Cursor::LinePositionAbsolute(n) => n.write_csi(f, "d")?,
            Cursor::LinePositionBackward(n) => n.write_csi(f, "k")?,
            Cursor::LinePositionForward(n) => n.write_csi(f, "e")?,
            Cursor::SetTopAndBottomMargins { top, bottom } => {
                if top.as_one_based() == 1 && bottom.as_one_based() == u32::MAX {
                    write!(f, "r")?;
                } else {
                    write!(f, "{};{}r", top, bottom)?;
                }
            }
            Cursor::SetLeftAndRightMargins { left, right } => {
                if left.as_one_based() == 1 && right.as_one_based() == u32::MAX {
                    write!(f, "s")?;
                } else {
                    write!(f, "{};{}s", left, right)?;
                }
            }
            Cursor::RequestActivePositionReport => write!(f, "6n")?,
            Cursor::SaveCursor => write!(f, "s")?,
            Cursor::RestoreCursor => write!(f, "u")?,
            Cursor::CursorStyle(style) => write!(f, "{} q", *style as u8)?,
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, FromPrimitive, Copy, ToPrimitive)]
pub enum CursorTabulationControl {
    SetCharacterTabStopAtActivePosition = 0,
    SetLineTabStopAtActiveLine = 1,
    ClearCharacterTabStopAtActivePosition = 2,
    ClearLineTabstopAtActiveLine = 3,
    ClearAllCharacterTabStopsAtActiveLine = 4,
    ClearAllCharacterTabStops = 5,
    ClearAllLineTabStops = 6,
}

impl ParamEnum for CursorTabulationControl {
    fn default() -> Self {
        CursorTabulationControl::SetCharacterTabStopAtActivePosition
    }
}

#[derive(Debug, Clone, PartialEq, Eq, FromPrimitive, Copy, ToPrimitive)]
pub enum TabulationClear {
    ClearCharacterTabStopAtActivePosition = 0,
    ClearLineTabStopAtActiveLine = 1,
    ClearCharacterTabStopsAtActiveLine = 2,
    ClearAllCharacterTabStops = 3,
    ClearAllLineTabStops = 4,
    ClearAllTabStops = 5,
}

impl ParamEnum for TabulationClear {
    fn default() -> Self {
        TabulationClear::ClearCharacterTabStopAtActivePosition
    }
}
