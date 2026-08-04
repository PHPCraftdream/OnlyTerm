use super::*;
use num_derive::{FromPrimitive, ToPrimitive};

#[derive(Debug, Clone, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum DeviceAttributeCodes {
    Columns132 = 1,
    Printer = 2,
    RegisGraphics = 3,
    SixelGraphics = 4,
    SelectiveErase = 6,
    UserDefinedKeys = 8,
    NationalReplacementCharsets = 9,
    TechnicalCharacters = 15,
    UserWindows = 18,
    HorizontalScrolling = 21,
    AnsiColor = 22,
    AnsiTextLocator = 29,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceAttribute {
    Code(DeviceAttributeCodes),
    Unspecified(CsiParam),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAttributeFlags {
    pub attributes: Vec<DeviceAttribute>,
}

impl DeviceAttributeFlags {
    fn emit(&self, f: &mut Formatter, leader: &str) -> Result<(), FmtError> {
        write!(f, "{}", leader)?;
        for item in &self.attributes {
            match item {
                DeviceAttribute::Code(c) => write!(f, ";{}", c.to_u16().ok_or(FmtError)?)?,
                DeviceAttribute::Unspecified(param) => write!(f, ";{}", param)?,
            }
        }
        write!(f, "c")?;
        Ok(())
    }

    pub fn new(attributes: Vec<DeviceAttribute>) -> Self {
        Self { attributes }
    }

    pub(super) fn from_params(params: &[CsiParam]) -> Self {
        let mut attributes = Vec::new();
        for i in params {
            match i {
                CsiParam::Integer(p) => match FromPrimitive::from_i64(*p) {
                    Some(c) => attributes.push(DeviceAttribute::Code(c)),
                    None => attributes.push(DeviceAttribute::Unspecified(*i)),
                },
                CsiParam::P(b';') => {}
                _ => attributes.push(DeviceAttribute::Unspecified(*i)),
            }
        }
        Self { attributes }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceAttributes {
    Vt100WithAdvancedVideoOption,
    Vt101WithNoOptions,
    Vt102,
    Vt220(DeviceAttributeFlags),
    Vt320(DeviceAttributeFlags),
    Vt420(DeviceAttributeFlags),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XtSmGraphicsItem {
    NumberOfColorRegisters,
    SixelGraphicsGeometry,
    RegisGraphicsGeometry,
    Unspecified(i64),
}

impl Display for XtSmGraphicsItem {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        match self {
            Self::NumberOfColorRegisters => write!(f, "1"),
            Self::SixelGraphicsGeometry => write!(f, "2"),
            Self::RegisGraphicsGeometry => write!(f, "3"),
            Self::Unspecified(n) => write!(f, "{}", n),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XtSmGraphicsAction {
    ReadAttribute,
    ResetToDefault,
    SetToValue,
    ReadMaximumAllowedValue,
}

impl XtSmGraphicsAction {
    pub fn to_i64(&self) -> i64 {
        match self {
            Self::ReadAttribute => 1,
            Self::ResetToDefault => 2,
            Self::SetToValue => 3,
            Self::ReadMaximumAllowedValue => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XtSmGraphicsStatus {
    Success,
    InvalidItem,
    InvalidAction,
    Failure,
}

impl XtSmGraphicsStatus {
    pub fn to_i64(&self) -> i64 {
        match self {
            Self::Success => 0,
            Self::InvalidItem => 1,
            Self::InvalidAction => 2,
            Self::Failure => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XtSmGraphics {
    pub item: XtSmGraphicsItem,
    pub action_or_status: i64,
    pub value: Vec<i64>,
}

impl XtSmGraphics {
    pub fn action(&self) -> Option<XtSmGraphicsAction> {
        match self.action_or_status {
            1 => Some(XtSmGraphicsAction::ReadAttribute),
            2 => Some(XtSmGraphicsAction::ResetToDefault),
            3 => Some(XtSmGraphicsAction::SetToValue),
            4 => Some(XtSmGraphicsAction::ReadMaximumAllowedValue),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<XtSmGraphicsStatus> {
        match self.action_or_status {
            0 => Some(XtSmGraphicsStatus::Success),
            1 => Some(XtSmGraphicsStatus::InvalidItem),
            2 => Some(XtSmGraphicsStatus::InvalidAction),
            3 => Some(XtSmGraphicsStatus::Failure),
            _ => None,
        }
    }

    // The unit error type is part of the public API; a typed error would be a breaking change.
    #[allow(clippy::result_unit_err)]
    pub fn parse(params: &[CsiParam]) -> Result<CSI, ()> {
        let params = Cracked::parse(&params[1..])?;
        Ok(CSI::Device(Box::new(Device::XtSmGraphics(XtSmGraphics {
            item: match params.get(0).ok_or(())? {
                CsiParam::Integer(1) => XtSmGraphicsItem::NumberOfColorRegisters,
                CsiParam::Integer(2) => XtSmGraphicsItem::SixelGraphicsGeometry,
                CsiParam::Integer(3) => XtSmGraphicsItem::RegisGraphicsGeometry,
                CsiParam::Integer(n) => XtSmGraphicsItem::Unspecified(*n),
                _ => return Err(()),
            },
            action_or_status: match params.get(1).ok_or(())? {
                CsiParam::Integer(n) => *n,
                _ => return Err(()),
            },
            value: params.params[2..]
                .iter()
                .filter_map(|p| match p {
                    Some(CsiParam::Integer(n)) => Some(*n),
                    _ => None,
                })
                .collect(),
        }))))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Device {
    DeviceAttributes(DeviceAttributes),
    /// DECSTR - https://vt100.net/docs/vt510-rm/DECSTR.html
    SoftReset,
    RequestPrimaryDeviceAttributes,
    RequestSecondaryDeviceAttributes,
    RequestTertiaryDeviceAttributes,
    StatusReport,
    /// https://github.com/mintty/mintty/issues/881
    /// https://gitlab.gnome.org/GNOME/vte/-/issues/235
    RequestTerminalNameAndVersion,
    RequestTerminalParameters(i64),
    XtSmGraphics(XtSmGraphics),
}

impl Display for Device {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        match self {
            Device::DeviceAttributes(DeviceAttributes::Vt100WithAdvancedVideoOption) => {
                write!(f, "?1;2c")?
            }
            Device::DeviceAttributes(DeviceAttributes::Vt101WithNoOptions) => write!(f, "?1;0c")?,
            Device::DeviceAttributes(DeviceAttributes::Vt102) => write!(f, "?6c")?,
            Device::DeviceAttributes(DeviceAttributes::Vt220(attr)) => attr.emit(f, "?62")?,
            Device::DeviceAttributes(DeviceAttributes::Vt320(attr)) => attr.emit(f, "?63")?,
            Device::DeviceAttributes(DeviceAttributes::Vt420(attr)) => attr.emit(f, "?64")?,
            Device::SoftReset => write!(f, "!p")?,
            Device::RequestPrimaryDeviceAttributes => write!(f, "c")?,
            Device::RequestSecondaryDeviceAttributes => write!(f, ">c")?,
            Device::RequestTertiaryDeviceAttributes => write!(f, "=c")?,
            Device::RequestTerminalNameAndVersion => write!(f, ">q")?,
            Device::RequestTerminalParameters(n) => write!(f, "{};1;1;128;128;1;0x", n + 2)?,
            Device::StatusReport => write!(f, "5n")?,
            Device::XtSmGraphics(g) => {
                write!(f, "?{};{}", g.item, g.action_or_status)?;
                for v in &g.value {
                    write!(f, ";{}", v)?;
                }
                write!(f, "S")?;
            }
        };
        Ok(())
    }
}
