use std::fmt::Display;
use wezterm_dynamic::{FromDynamic, FromDynamicOptions, ToDynamic, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontWeight(u16);

enum FontWeightOrLabel {
    Weight(u16),
    Label(&'static str),
}

impl FontWeight {
    fn categorize_weight(&self) -> FontWeightOrLabel {
        let label = if *self == Self::EXTRABLACK {
            "ExtraBlack"
        } else if *self == Self::BLACK {
            "Black"
        } else if *self == Self::EXTRABOLD {
            "ExtraBold"
        } else if *self == Self::BOLD {
            "Bold"
        } else if *self == Self::DEMIBOLD {
            "DemiBold"
        } else if *self == Self::MEDIUM {
            "Medium"
        } else if *self == Self::REGULAR {
            "Regular"
        } else if *self == Self::BOOK {
            "Book"
        } else if *self == Self::DEMILIGHT {
            "DemiLight"
        } else if *self == Self::LIGHT {
            "Light"
        } else if *self == Self::EXTRALIGHT {
            "ExtraLight"
        } else if *self == Self::THIN {
            "Thin"
        } else {
            return FontWeightOrLabel::Weight(self.0);
        };
        FontWeightOrLabel::Label(label)
    }

    fn from_str(s: &str) -> Option<FontWeight> {
        Some(match s {
            "ExtraBlack" => Self::EXTRABLACK,
            "Black" => Self::BLACK,
            "ExtraBold" => Self::EXTRABOLD,
            "Bold" => Self::BOLD,
            "DemiBold" => Self::DEMIBOLD,
            "Medium" => Self::MEDIUM,
            "Regular" => Self::REGULAR,
            "Book" => Self::BOOK,
            "DemiLight" => Self::DEMILIGHT,
            "Light" => Self::LIGHT,
            "ExtraLight" => Self::EXTRALIGHT,
            "Thin" => Self::THIN,
            _ => return None,
        })
    }
}

impl ToDynamic for FontWeight {
    fn to_dynamic(&self) -> Value {
        match self.categorize_weight() {
            FontWeightOrLabel::Weight(n) => Value::U64(n as u64),
            FontWeightOrLabel::Label(l) => Value::String(l.to_string()),
        }
    }
}

impl FromDynamic for FontWeight {
    fn from_dynamic(
        value: &Value,
        _options: FromDynamicOptions,
    ) -> Result<Self, wezterm_dynamic::Error> {
        match value {
            Value::String(s) => {
                Ok(Self::from_str(s).ok_or_else(|| format!("invalid font weight {}", s))?)
            }
            other => {
                if let Some(value) = value.coerce_unsigned() {
                    if value > 0 && value <= (u16::MAX as u64) {
                        Ok(FontWeight(value as u16))
                    } else {
                        Err(format!("invalid font weight {}", value).into())
                    }
                } else {
                    Err(wezterm_dynamic::Error::NoConversion {
                        source_type: other.variant_name().to_string(),
                        dest_type: "FontWeight",
                    })
                }
            }
        }
    }
}

impl Display for FontWeight {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.categorize_weight() {
            // ktav has no quoting syntax: a label is written as a bare
            // identifier (e.g. `weight: Bold`), not a quoted string.
            FontWeightOrLabel::Weight(n) => write!(fmt, "{}", n),
            FontWeightOrLabel::Label(l) => write!(fmt, "{}", l),
        }
    }
}

impl FontWeight {
    pub const THIN: FontWeight = FontWeight(100);
    pub const EXTRALIGHT: FontWeight = FontWeight(200);
    pub const LIGHT: FontWeight = FontWeight(300);
    pub const DEMILIGHT: FontWeight = FontWeight(350);
    pub const BOOK: FontWeight = FontWeight(380);
    pub const REGULAR: FontWeight = FontWeight(400);
    pub const MEDIUM: FontWeight = FontWeight(500);
    pub const DEMIBOLD: FontWeight = FontWeight(600);
    pub const BOLD: FontWeight = FontWeight(700);
    pub const EXTRABOLD: FontWeight = FontWeight(800);
    pub const BLACK: FontWeight = FontWeight(900);
    pub const EXTRABLACK: FontWeight = FontWeight(1000);
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::REGULAR
    }
}

impl FontWeight {
    pub const fn from_opentype_weight(w: u16) -> Self {
        Self(w)
    }

    pub fn to_opentype_weight(self) -> u16 {
        self.0
    }

    pub fn lighter(self) -> Self {
        Self::from_opentype_weight(self.to_opentype_weight().saturating_sub(200))
    }

    pub fn bolder(self) -> Self {
        Self::from_opentype_weight(self.to_opentype_weight() + 200)
    }
}
