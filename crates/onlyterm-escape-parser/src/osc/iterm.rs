use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ITermProprietary {
    /// The "Set Mark" command allows you to record a location and then jump back to it later
    SetMark,
    /// To bring iTerm2 to the foreground
    StealFocus,
    /// To erase the scrollback history
    ClearScrollback,
    /// To inform iTerm2 of the current directory to help semantic history
    CurrentDir(String),
    /// To change the session's profile on the fly
    SetProfile(String),
    /// Currently defined values for the string parameter are "rule", "find", "font"
    /// or an empty string.  iTerm2 will go into paste mode until EndCopy is received.
    CopyToClipboard(String),
    /// Ends CopyToClipboard mode in iTerm2.
    EndCopy,
    /// The boolean should be yes or no. This shows or hides the cursor guide
    HighlightCursorLine(bool),
    /// Request that the terminal send a ReportCellSize response
    RequestCellSize,
    /// The response to RequestCellSize.  The height and width are the dimensions
    /// of a cell measured in points according to the docs, but in practice, they
    /// are actually pixels.
    /// If scale is_some(), the width and height will be multiplied by scale to
    /// get the true device dimensions
    ReportCellSize {
        height_pixels: NotNan<f32>,
        width_pixels: NotNan<f32>,
        scale: Option<NotNan<f32>>,
    },
    /// Place a string in the systems pasteboard
    Copy(String),
    /// Each iTerm2 session has internal variables (as described in
    /// <https://www.iterm2.com/documentation-badges.html>). This escape sequence reports
    /// a variable's value.  The response is another ReportVariable.
    ReportVariable(String),
    /// User-defined variables may be set with the following escape sequence
    SetUserVar {
        name: String,
        value: String,
    },
    SetBadgeFormat(String),
    /// Download file data from the application.
    File(Box<ITermFileData>),

    /// Configure unicode version
    UnicodeVersion(ITermUnicodeVersionOp),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ITermUnicodeVersionOp {
    Set(u8),
    Push(Option<String>),
    Pop(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ITermFileData {
    /// file name
    pub name: Option<String>,
    /// size of the data in bytes; this is used by iterm to show progress
    /// while waiting for the rest of the payload
    pub size: Option<usize>,
    /// width to render
    pub width: ITermDimension,
    /// height to render
    pub height: ITermDimension,
    /// if true, preserve aspect ratio when fitting to width/height
    pub preserve_aspect_ratio: bool,
    /// if true, attempt to display in the terminal rather than downloading to
    /// the users download directory
    pub inline: bool,
    /// if true, do not move the cursor
    pub do_not_move_cursor: bool,
    /// The data to transfer
    pub data: Vec<u8>,
}

impl ITermFileData {
    fn parse(osc: &[&[u8]]) -> Result<Self> {
        let mut params = HashMap::new();

        // Unfortunately, the encoding for the file download data is
        // awkward to fit in the conventional OSC data that our parser
        // expects at a higher level.
        // We have a mix of '=', ';' and ':' separated keys and values,
        // and a number of them are optional.
        // ESC ] 1337 ; File = [optional arguments] : base-64 encoded file contents ^G

        let mut data = None;

        let last = osc.len() - 1;
        for (idx, s) in osc.iter().enumerate().skip(1) {
            let param = if idx == 1 {
                if s.len() >= 5 {
                    // skip over File=
                    &s[5..]
                } else {
                    bail!("failed to parse file data; File= not found");
                }
            } else {
                s
            };

            let param = if idx == last {
                // The final argument contains `:base64`, so look for that
                if let Some(colon) = param.iter().position(|c| *c == b':') {
                    data = Some(base64_decode(&param[colon + 1..])?);
                    &param[..colon]
                } else {
                    // If we don't find the colon in the last piece, we've
                    // got nothing useful
                    bail!("failed to parse file data; no colon found");
                }
            } else {
                param
            };

            // eg: `File=;size=1234` case. <https://github.com/wezterm/wezterm/issues/1291>
            if param.is_empty() {
                continue;
            }

            // look for k=v in param
            if let Some(equal) = param.iter().position(|c| *c == b'=') {
                let key = &param[..equal];
                let value = &param[equal + 1..];
                params.insert(str::from_utf8(key)?, str::from_utf8(value)?);
            } else if idx != last {
                bail!("failed to parse file data; no equals found");
            }
        }

        let name = params
            .get("name")
            .and_then(|s| base64_decode(s).ok())
            .and_then(|b| String::from_utf8(b).ok());
        let size = params.get("size").and_then(|s| s.parse().ok());
        let width = params
            .get("width")
            .and_then(|s| ITermDimension::parse(s).ok())
            .unwrap_or(ITermDimension::Automatic);
        let height = params
            .get("height")
            .and_then(|s| ITermDimension::parse(s).ok())
            .unwrap_or(ITermDimension::Automatic);
        let preserve_aspect_ratio = params
            .get("preserveAspectRatio")
            .map(|s| *s != "0")
            .unwrap_or(true);
        let inline = params.get("inline").map(|s| *s != "0").unwrap_or(false);
        let do_not_move_cursor = params
            .get("doNotMoveCursor")
            .map(|s| *s != "0")
            .unwrap_or(false);
        let data = data.ok_or_else(|| "didn't set data".to_string())?;
        Ok(Self {
            name,
            size,
            width,
            height,
            preserve_aspect_ratio,
            inline,
            do_not_move_cursor,
            data,
        })
    }
}

impl Display for ITermFileData {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "File")?;
        let mut sep = "=";
        let emit_sep = |sep, f: &mut Formatter| -> core::result::Result<&str, FmtError> {
            write!(f, "{}", sep)?;
            Ok(";")
        };
        if let Some(size) = self.size {
            sep = emit_sep(sep, f)?;
            write!(f, "size={}", size)?;
        }
        if let Some(ref name) = self.name {
            sep = emit_sep(sep, f)?;
            write!(f, "name={}", base64_encode(name))?;
        }
        if self.width != ITermDimension::Automatic {
            sep = emit_sep(sep, f)?;
            write!(f, "width={}", self.width)?;
        }
        if self.height != ITermDimension::Automatic {
            sep = emit_sep(sep, f)?;
            write!(f, "height={}", self.height)?;
        }
        if !self.preserve_aspect_ratio {
            sep = emit_sep(sep, f)?;
            write!(f, "preserveAspectRatio=0")?;
        }
        if self.inline {
            sep = emit_sep(sep, f)?;
            write!(f, "inline=1")?;
        }
        if self.do_not_move_cursor {
            sep = emit_sep(sep, f)?;
            write!(f, "doNotMoveCursor=1")?;
        }
        // Ensure that we emit a sep if we didn't already.
        // It will still be set to '=' in that case.
        if sep == "=" {
            write!(f, "=")?;
        }
        write!(f, ":{}", base64_encode(&self.data))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ITermDimension {
    #[default]
    Automatic,
    Cells(i64),
    Pixels(i64),
    Percent(i64),
}

impl Display for ITermDimension {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        use self::ITermDimension::*;
        match self {
            Automatic => write!(f, "auto"),
            Cells(n) => write!(f, "{}", n),
            Pixels(n) => write!(f, "{}px", n),
            Percent(n) => write!(f, "{}%", n),
        }
    }
}

impl core::str::FromStr for ITermDimension {
    type Err = crate::Error;
    fn from_str(s: &str) -> Result<Self> {
        ITermDimension::parse(s)
    }
}

impl ITermDimension {
    fn parse(s: &str) -> Result<Self> {
        if s == "auto" {
            Ok(ITermDimension::Automatic)
        } else if let Some(s) = s.strip_suffix("px") {
            let num = s.parse()?;
            Ok(ITermDimension::Pixels(num))
        } else if let Some(s) = s.strip_suffix('%') {
            let num = s.parse()?;
            Ok(ITermDimension::Percent(num))
        } else {
            let num = s.parse()?;
            Ok(ITermDimension::Cells(num))
        }
    }

    /// Convert the dimension into a number of pixels based on the provided
    /// size of a cell and number of cells in that dimension.
    /// Returns None for the Automatic variant.
    pub fn to_pixels(&self, cell_size: usize, num_cells: usize) -> Option<usize> {
        match self {
            ITermDimension::Automatic => None,
            ITermDimension::Cells(n) => Some((*n).max(0) as usize * cell_size),
            ITermDimension::Pixels(n) => Some((*n).max(0) as usize),
            ITermDimension::Percent(n) => Some(
                (((*n).clamp(0, 100) as f32 / 100.0) * num_cells as f32 * cell_size as f32)
                    as usize,
            ),
        }
    }
}

impl ITermProprietary {
    #[allow(clippy::cognitive_complexity)]
    pub(super) fn parse(osc: &[&[u8]]) -> Result<Self> {
        // iTerm has a number of different styles of OSC parameter
        // encodings, which makes this section of code a bit gnarly.
        ensure!(osc.len() > 1, "not enough args");

        let param = String::from_utf8_lossy(osc[1]);

        let mut iter = param.splitn(2, '=');
        let keyword = iter.next().ok_or_else(|| "bad params".to_string())?;
        let p1 = iter.next();

        macro_rules! single {
            ($variant:ident, $text:expr) => {
                if osc.len() == 2 && keyword == $text && p1.is_none() {
                    return Ok(ITermProprietary::$variant);
                }
            };
        }

        macro_rules! one_str {
            ($variant:ident, $text:expr) => {
                if osc.len() == 2 && keyword == $text {
                    if let Some(p1) = p1 {
                        return Ok(ITermProprietary::$variant(p1.into()));
                    }
                }
            };
        }
        macro_rules! const_arg {
            ($variant:ident, $text:expr, $value:expr, $res:expr) => {
                if osc.len() == 2 && keyword == $text {
                    if let Some(p1) = p1 {
                        if p1 == $value {
                            return Ok(ITermProprietary::$variant($res));
                        }
                    }
                }
            };
        }

        single!(SetMark, "SetMark");
        single!(StealFocus, "StealFocus");
        single!(ClearScrollback, "ClearScrollback");
        single!(EndCopy, "EndCopy");
        single!(RequestCellSize, "ReportCellSize");
        const_arg!(HighlightCursorLine, "HighlightCursorLine", "yes", true);
        const_arg!(HighlightCursorLine, "HighlightCursorLine", "no", false);
        one_str!(CurrentDir, "CurrentDir");
        one_str!(SetProfile, "SetProfile");
        one_str!(CopyToClipboard, "CopyToClipboard");

        let p1_empty = matches!(p1, Some("") | None);

        if osc.len() == 3 && keyword == "Copy" && p1_empty {
            return Ok(ITermProprietary::Copy(String::from_utf8(base64_decode(
                osc[2],
            )?)?));
        }
        if osc.len() == 3 && keyword == "SetBadgeFormat" && p1_empty {
            return Ok(ITermProprietary::SetBadgeFormat(String::from_utf8(
                base64_decode(osc[2])?,
            )?));
        }

        if osc.len() == 3
            && keyword == "ReportCellSize"
            && p1.is_some()
            && let Some(p1) = p1
        {
            return Ok(ITermProprietary::ReportCellSize {
                height_pixels: NotNan::new(p1.parse()?).map_err(not_nan_err)?,
                width_pixels: NotNan::new(String::from_utf8_lossy(osc[2]).parse()?)
                    .map_err(not_nan_err)?,
                scale: None,
            });
        }
        if osc.len() == 4
            && keyword == "ReportCellSize"
            && p1.is_some()
            && let Some(p1) = p1
        {
            return Ok(ITermProprietary::ReportCellSize {
                height_pixels: NotNan::new(p1.parse()?).map_err(not_nan_err)?,
                width_pixels: NotNan::new(String::from_utf8_lossy(osc[2]).parse()?)
                    .map_err(not_nan_err)?,
                scale: Some(
                    NotNan::new(String::from_utf8_lossy(osc[3]).parse()?).map_err(not_nan_err)?,
                ),
            });
        }

        if osc.len() == 2
            && keyword == "SetUserVar"
            && let Some(p1) = p1
        {
            let mut iter = p1.splitn(2, '=');
            let p1 = iter.next();
            let p2 = iter.next();

            if let (Some(k), Some(v)) = (p1, p2) {
                return Ok(ITermProprietary::SetUserVar {
                    name: k.to_string(),
                    value: String::from_utf8(base64_decode(v)?)?,
                });
            }
        }

        if osc.len() == 2
            && keyword == "UnicodeVersion"
            && let Some(p1) = p1
        {
            let mut iter = p1.splitn(2, ' ');
            let keyword = iter.next();
            let label = iter.next();

            if let Some("push") = keyword {
                return Ok(ITermProprietary::UnicodeVersion(
                    ITermUnicodeVersionOp::Push(label.map(|s| s.to_string())),
                ));
            }
            if let Some("pop") = keyword {
                return Ok(ITermProprietary::UnicodeVersion(
                    ITermUnicodeVersionOp::Pop(label.map(|s| s.to_string())),
                ));
            }

            if let Ok(n) = p1.parse::<u8>() {
                return Ok(ITermProprietary::UnicodeVersion(
                    ITermUnicodeVersionOp::Set(n),
                ));
            }
        }

        if keyword == "File" {
            return Ok(ITermProprietary::File(Box::new(ITermFileData::parse(osc)?)));
        }

        bail!("ITermProprietary {:?}", osc);
    }
}

impl Display for ITermProprietary {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "1337;")?;
        use self::ITermProprietary::*;
        match self {
            SetMark => write!(f, "SetMark")?,
            StealFocus => write!(f, "StealFocus")?,
            ClearScrollback => write!(f, "ClearScrollback")?,
            CurrentDir(s) => write!(f, "CurrentDir={}", s)?,
            SetProfile(s) => write!(f, "SetProfile={}", s)?,
            CopyToClipboard(s) => write!(f, "CopyToClipboard={}", s)?,
            EndCopy => write!(f, "EndCopy")?,
            HighlightCursorLine(yes) => {
                write!(f, "HighlightCursorLine={}", if *yes { "yes" } else { "no" })?
            }
            RequestCellSize => write!(f, "ReportCellSize")?,
            ReportCellSize {
                height_pixels,
                width_pixels,
                scale: None,
            } => write!(f, "ReportCellSize={height_pixels:.1};{width_pixels:.1}")?,
            ReportCellSize {
                height_pixels,
                width_pixels,
                scale: Some(scale),
            } => write!(
                f,
                "ReportCellSize={height_pixels:.1};{width_pixels:.1};{scale:.1}",
            )?,
            Copy(s) => write!(f, "Copy=;{}", base64_encode(s))?,
            ReportVariable(s) => write!(f, "ReportVariable={}", base64_encode(s))?,
            SetUserVar { name, value } => {
                write!(f, "SetUserVar={}={}", name, base64_encode(value))?
            }
            SetBadgeFormat(s) => write!(f, "SetBadgeFormat={}", base64_encode(s))?,
            File(file) => file.fmt(f)?,
            UnicodeVersion(ITermUnicodeVersionOp::Set(n)) => write!(f, "UnicodeVersion={}", n)?,
            UnicodeVersion(ITermUnicodeVersionOp::Push(Some(label))) => {
                write!(f, "UnicodeVersion=push {}", label)?
            }
            UnicodeVersion(ITermUnicodeVersionOp::Push(None)) => write!(f, "UnicodeVersion=push")?,
            UnicodeVersion(ITermUnicodeVersionOp::Pop(Some(label))) => {
                write!(f, "UnicodeVersion=pop {}", label)?
            }
            UnicodeVersion(ITermUnicodeVersionOp::Pop(None)) => write!(f, "UnicodeVersion=pop")?,
        }
        Ok(())
    }
}

fn not_nan_err(err: ordered_float::FloatIsNan) -> crate::Error {
    format_err!("{:#}", err)
}
