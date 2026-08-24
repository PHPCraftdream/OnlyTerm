use crate::color::{RgbColor, RgbaColor};
use onlyterm_dynamic::{FromDynamic, ToDynamic};
use onlyterm_term::{Intensity, Underline};
use std::convert::TryFrom;
use termwiz::cell::CellAttributes;
use termwiz::color::ColorSpec as TWColorSpec;

/// Specify the text styling for a tab in the tab bar
#[derive(Debug, Clone, Default, PartialEq, FromDynamic, ToDynamic)]
pub struct TabBarColor {
    /// Specifies the intensity attribute for the tab title text
    #[dynamic(default)]
    pub intensity: Intensity,
    /// Specifies the underline attribute for the tab title text
    #[dynamic(default)]
    pub underline: Underline,
    /// Specifies the italic attribute for the tab title text
    #[dynamic(default)]
    pub italic: bool,
    /// Specifies the strikethrough attribute for the tab title text
    #[dynamic(default)]
    pub strikethrough: bool,
    /// The background color for the tab
    pub bg_color: RgbaColor,
    /// The forgeground/text color for the tab
    pub fg_color: RgbaColor,
}

impl TabBarColor {
    pub fn as_cell_attributes(&self) -> CellAttributes {
        let mut attr = CellAttributes::default();
        attr.set_intensity(self.intensity)
            .set_underline(self.underline)
            .set_italic(self.italic)
            .set_strikethrough(self.strikethrough)
            .set_background(TWColorSpec::TrueColor(*self.bg_color))
            .set_foreground(TWColorSpec::TrueColor(*self.fg_color));
        attr
    }
}

/// Specifies the colors to use for the tab bar portion of the UI.
/// These are not part of the terminal model and cannot be updated
/// in the same way that the dynamic color schemes are.
#[derive(Default, Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct TabBarColors {
    /// The background color for the tab bar
    #[dynamic(default)]
    pub background: Option<RgbaColor>,

    /// Styling for the active tab
    #[dynamic(default)]
    pub active_tab: Option<TabBarColor>,

    /// Styling for other inactive tabs
    #[dynamic(default)]
    pub inactive_tab: Option<TabBarColor>,

    /// Styling for an inactive tab with a mouse hovering
    #[dynamic(default)]
    pub inactive_tab_hover: Option<TabBarColor>,

    /// Styling for the new tab button
    #[dynamic(default)]
    pub new_tab: Option<TabBarColor>,

    /// Styling for the new tab button with a mouse hovering
    #[dynamic(default)]
    pub new_tab_hover: Option<TabBarColor>,

    #[dynamic(default)]
    pub inactive_tab_edge: Option<RgbaColor>,

    #[dynamic(default)]
    pub inactive_tab_edge_hover: Option<RgbaColor>,
}

impl TabBarColors {
    pub fn background(&self) -> RgbaColor {
        self.background.unwrap_or_else(default_background)
    }

    pub fn active_tab(&self) -> TabBarColor {
        self.active_tab.clone().unwrap_or_else(default_active_tab)
    }

    pub fn inactive_tab(&self) -> TabBarColor {
        self.inactive_tab
            .clone()
            .unwrap_or_else(default_inactive_tab)
    }

    pub fn inactive_tab_hover(&self) -> TabBarColor {
        self.inactive_tab_hover
            .clone()
            .unwrap_or_else(default_inactive_tab_hover)
    }

    pub fn new_tab(&self) -> TabBarColor {
        self.new_tab.clone().unwrap_or_else(default_inactive_tab)
    }

    pub fn new_tab_hover(&self) -> TabBarColor {
        self.new_tab_hover
            .clone()
            .unwrap_or_else(default_inactive_tab_hover)
    }

    pub fn inactive_tab_edge(&self) -> RgbaColor {
        self.inactive_tab_edge
            .unwrap_or_else(default_inactive_tab_edge)
    }

    pub fn inactive_tab_edge_hover(&self) -> RgbaColor {
        self.inactive_tab_edge_hover
            .unwrap_or_else(default_inactive_tab_edge_hover)
    }

    pub fn overlay_with(&self, other: &Self) -> Self {
        macro_rules! overlay {
            ($name:ident) => {
                if let Some(c) = &other.$name {
                    Some(c.clone())
                } else {
                    self.$name.clone()
                }
            };
        }
        Self {
            active_tab: overlay!(active_tab),
            background: overlay!(background),
            inactive_tab: overlay!(inactive_tab),
            inactive_tab_hover: overlay!(inactive_tab_hover),
            inactive_tab_edge: overlay!(inactive_tab_edge),
            inactive_tab_edge_hover: overlay!(inactive_tab_edge_hover),
            new_tab: overlay!(new_tab),
            new_tab_hover: overlay!(new_tab_hover),
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
#[dynamic(try_from = "String")]
pub enum IntegratedTitleButtonColor {
    #[default]
    Auto,
    Custom(RgbaColor),
}

impl From<IntegratedTitleButtonColor> for String {
    fn from(value: IntegratedTitleButtonColor) -> String {
        match value {
            IntegratedTitleButtonColor::Auto => "auto".to_string(),
            IntegratedTitleButtonColor::Custom(color) => color.into(),
        }
    }
}

impl TryFrom<String> for IntegratedTitleButtonColor {
    type Error = <RgbaColor as TryFrom<String>>::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.eq_ignore_ascii_case("auto") {
            Ok(Self::Auto)
        } else {
            Ok(Self::Custom(RgbaColor::try_from(value)?))
        }
    }
}

fn default_background() -> RgbaColor {
    (0x33, 0x33, 0x33).into()
}

fn default_inactive_tab_edge() -> RgbaColor {
    RgbColor::new_8bpc(0x57, 0x57, 0x57).into()
}

fn default_inactive_tab_edge_hover() -> RgbaColor {
    RgbColor::new_8bpc(0x36, 0x36, 0x36).into()
}

fn default_inactive_tab() -> TabBarColor {
    TabBarColor {
        bg_color: (0x33, 0x33, 0x33).into(),
        fg_color: (0x80, 0x80, 0x80).into(),
        ..TabBarColor::default()
    }
}
fn default_inactive_tab_hover() -> TabBarColor {
    TabBarColor {
        bg_color: (0x1f, 0x1f, 0x1f).into(),
        fg_color: (0x90, 0x90, 0x90).into(),
        italic: true,
        ..TabBarColor::default()
    }
}
fn default_active_tab() -> TabBarColor {
    TabBarColor {
        bg_color: (0x00, 0x00, 0x00).into(),
        fg_color: (0xc0, 0xc0, 0xc0).into(),
        ..TabBarColor::default()
    }
}

#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub struct TabBarStyle {
    #[dynamic(default = "default_new_tab")]
    pub new_tab: String,
    #[dynamic(default = "default_new_tab")]
    pub new_tab_hover: String,
    #[dynamic(default = "default_window_hide")]
    pub window_hide: String,
    #[dynamic(default = "default_window_hide")]
    pub window_hide_hover: String,
    #[dynamic(default = "default_window_maximize")]
    pub window_maximize: String,
    #[dynamic(default = "default_window_maximize")]
    pub window_maximize_hover: String,
    #[dynamic(default = "default_window_close")]
    pub window_close: String,
    #[dynamic(default = "default_window_close")]
    pub window_close_hover: String,
}

impl Default for TabBarStyle {
    fn default() -> Self {
        Self {
            new_tab: default_new_tab(),
            new_tab_hover: default_new_tab(),
            window_hide: default_window_hide(),
            window_hide_hover: default_window_hide(),
            window_maximize: default_window_maximize(),
            window_maximize_hover: default_window_maximize(),
            window_close: default_window_close(),
            window_close_hover: default_window_close(),
        }
    }
}

fn default_new_tab() -> String {
    " + ".to_string()
}

fn default_window_hide() -> String {
    " . ".to_string()
}

fn default_window_maximize() -> String {
    " - ".to_string()
}

fn default_window_close() -> String {
    " X ".to_string()
}
