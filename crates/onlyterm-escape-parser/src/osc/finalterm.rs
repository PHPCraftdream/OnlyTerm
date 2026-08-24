use super::*;

/// https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalTermClick {
    /// Allow motion only within the single input line using left/right arrow keys
    Line,
    /// Allow moving between multiple lines of input using left/right arrow keys
    MultipleLine,
    /// Allow left/right and conservative up/down arrow motion
    ConservativeVertical,
    /// Allow left/right and up/down motion, and the line editor ensures that
    /// there are no spurious trailing spaces at ends of lines and that vertical
    /// motion across shorter lines causes some horizontal cursor motion.
    SmartVertical,
}

impl core::convert::TryFrom<&str> for FinalTermClick {
    type Error = crate::Error;
    fn try_from(s: &str) -> Result<Self> {
        match s {
            "line" => Ok(Self::Line),
            "m" => Ok(Self::MultipleLine),
            "v" => Ok(Self::ConservativeVertical),
            "w" => Ok(Self::SmartVertical),
            _ => bail!("invalid FinalTermClick {}", s),
        }
    }
}

impl Display for FinalTermClick {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            Self::Line => write!(f, "line"),
            Self::MultipleLine => write!(f, "m"),
            Self::ConservativeVertical => write!(f, "v"),
            Self::SmartVertical => write!(f, "w"),
        }
    }
}

/// https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FinalTermPromptKind {
    /// A normal left side primary prompt
    #[default]
    Initial,
    /// A right-aligned prompt
    RightSide,
    /// A continuation prompt for an input that can be edited
    Continuation,
    /// A continuation prompt where the input cannot be edited
    Secondary,
}

impl core::convert::TryFrom<&str> for FinalTermPromptKind {
    type Error = crate::Error;
    fn try_from(s: &str) -> Result<Self> {
        match s {
            "i" => Ok(Self::Initial),
            "r" => Ok(Self::RightSide),
            "c" => Ok(Self::Continuation),
            "s" => Ok(Self::Secondary),
            _ => bail!("invalid FinalTermPromptKind {}", s),
        }
    }
}

impl Display for FinalTermPromptKind {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            Self::Initial => write!(f, "i"),
            Self::RightSide => write!(f, "r"),
            Self::Continuation => write!(f, "c"),
            Self::Secondary => write!(f, "s"),
        }
    }
}

/// https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalTermSemanticPrompt {
    /// Do a "fresh line"; if the cursor is at the left margin then
    /// do nothing, otherwise perform the equivalent of "\r\n"
    FreshLine,

    /// Do a "fresh line" as above and then place the terminal into
    /// prompt mode; the output between now and the next marker is
    /// considered part of the prompt.
    FreshLineAndStartPrompt {
        aid: Option<String>,
        cl: Option<FinalTermClick>,
    },

    /// Denote the end of a command output and then perform FreshLine
    MarkEndOfCommandWithFreshLine {
        aid: Option<String>,
        cl: Option<FinalTermClick>,
    },

    /// Start a prompt
    StartPrompt(FinalTermPromptKind),

    /// Mark the end of a prompt and the start of the user input.
    /// The terminal considers all subsequent output to be "user input"
    /// until the next semantic marker.
    MarkEndOfPromptAndStartOfInputUntilNextMarker,

    /// Mark the end of a prompt and the start of the user input.
    /// The terminal considers all subsequent output to be "user input"
    /// until the end of the line.
    MarkEndOfPromptAndStartOfInputUntilEndOfLine,

    MarkEndOfInputAndStartOfOutput {
        aid: Option<String>,
    },

    /// Indicates the result of the command
    CommandStatus {
        status: i32,
        aid: Option<String>,
    },
}

impl FinalTermSemanticPrompt {
    pub(super) fn parse(osc: &[&[u8]]) -> Result<Self> {
        ensure!(osc.len() > 1, "not enough args");
        let param = String::from_utf8_lossy(osc[1]);

        macro_rules! single {
            ($variant:ident, $text:expr) => {
                if osc.len() == 2 && param == $text {
                    return Ok(FinalTermSemanticPrompt::$variant);
                }
            };
        }

        single!(FreshLine, "L");
        single!(MarkEndOfPromptAndStartOfInputUntilNextMarker, "B");
        single!(MarkEndOfPromptAndStartOfInputUntilEndOfLine, "I");

        let mut params = HashMap::new();
        use core::convert::TryInto;

        for s in osc.iter().skip(if param == "D" { 3 } else { 2 }) {
            if let Some(equal) = s.iter().position(|c| *c == b'=') {
                let key = &s[..equal];
                let value = &s[equal + 1..];
                params.insert(str::from_utf8(key)?, str::from_utf8(value)?);
            } else if !s.is_empty() {
                bail!("malformed FinalTermSemanticPrompt");
            }
        }

        if param == "A" {
            return Ok(Self::FreshLineAndStartPrompt {
                aid: params.get("aid").map(|&s| s.to_owned()),
                cl: match params.get("cl") {
                    Some(&cl) => Some(cl.try_into()?),
                    None => None,
                },
            });
        }

        if param == "C" {
            return Ok(Self::MarkEndOfInputAndStartOfOutput {
                aid: params.get("aid").map(|&s| s.to_owned()),
            });
        }

        if param == "D" {
            let status = match osc.get(2).copied() {
                Some(s) => match str::from_utf8(s) {
                    Ok(s) => s.parse().unwrap_or(0),
                    _ => 0,
                },
                _ => 0,
            };

            return Ok(Self::CommandStatus {
                status,
                aid: params.get("aid").map(|&s| s.to_owned()),
            });
        }

        if param == "N" {
            return Ok(Self::MarkEndOfCommandWithFreshLine {
                aid: params.get("aid").map(|&s| s.to_owned()),
                cl: match params.get("cl") {
                    Some(&cl) => Some(cl.try_into()?),
                    None => None,
                },
            });
        }

        if param == "P" {
            return Ok(Self::StartPrompt(match params.get("k") {
                Some(&cl) => cl.try_into()?,
                None => FinalTermPromptKind::default(),
            }));
        }

        bail!(
            "invalid FinalTermSemanticPrompt p1:{:?}, params:{:?}",
            param,
            params
        );
    }
}

impl Display for FinalTermSemanticPrompt {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "133;")?;
        match self {
            Self::FreshLine => write!(f, "L")?,
            Self::FreshLineAndStartPrompt { aid, cl } => {
                write!(f, "A")?;
                if let Some(aid) = aid {
                    write!(f, ";aid={}", aid)?;
                }
                if let Some(cl) = cl {
                    write!(f, ";cl={}", cl)?;
                }
            }
            Self::MarkEndOfCommandWithFreshLine { aid, cl } => {
                write!(f, "N")?;
                if let Some(aid) = aid {
                    write!(f, ";aid={}", aid)?;
                }
                if let Some(cl) = cl {
                    write!(f, ";cl={}", cl)?;
                }
            }
            Self::StartPrompt(kind) => {
                write!(f, "P;k={}", kind)?;
            }
            Self::MarkEndOfPromptAndStartOfInputUntilNextMarker => write!(f, "B")?,
            Self::MarkEndOfPromptAndStartOfInputUntilEndOfLine => write!(f, "I")?,
            Self::MarkEndOfInputAndStartOfOutput { aid } => {
                write!(f, "C")?;
                if let Some(aid) = aid {
                    write!(f, ";aid={}", aid)?;
                }
            }
            Self::CommandStatus {
                status,
                aid: Some(aid),
            } => {
                write!(f, "D;{};err={};aid={}", status, status, aid)?;
            }
            Self::CommandStatus { status, aid: None } => {
                write!(f, "D;{}", status)?;
            }
        }
        Ok(())
    }
}
