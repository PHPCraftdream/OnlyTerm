use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharacterPath {
    /// 0
    ImplementationDefault,
    /// 1
    LeftToRightOrTopToBottom,
    /// 2
    RightToLeftOrBottomToTop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unspecified {
    pub params: Vec<CsiParam>,
    /// if true, more than two intermediates arrived and the
    /// remaining data was ignored
    pub parameters_truncated: bool,
    /// The final character in the CSI sequence; this typically
    /// defines how to interpret the other parameters.
    pub control: char,
}

impl Display for Unspecified {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        for p in &self.params {
            write!(f, "{}", p)?;
        }
        write!(f, "{}", self.control)
    }
}
