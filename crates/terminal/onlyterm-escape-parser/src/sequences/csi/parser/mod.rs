use super::*;

pub(super) trait EncodeCSIParam {
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
pub(super) trait ParamEnum: FromPrimitive {
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

pub(super) struct Cracked {
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

mod dispatch;
mod protocol;
mod sgr;
mod window;

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
