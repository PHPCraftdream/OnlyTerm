use crate::color::{RgbColor, RgbaColor};
use crate::font::TextStyle;
use crate::units::Dimension;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub struct WindowFrameConfig {
    #[dynamic(default = "default_inactive_titlebar_bg")]
    pub inactive_titlebar_bg: RgbaColor,
    #[dynamic(default = "default_active_titlebar_bg")]
    pub active_titlebar_bg: RgbaColor,
    #[dynamic(default = "default_inactive_titlebar_fg")]
    pub inactive_titlebar_fg: RgbaColor,
    #[dynamic(default = "default_active_titlebar_fg")]
    pub active_titlebar_fg: RgbaColor,
    #[dynamic(default = "default_inactive_titlebar_border_bottom")]
    pub inactive_titlebar_border_bottom: RgbaColor,
    #[dynamic(default = "default_active_titlebar_border_bottom")]
    pub active_titlebar_border_bottom: RgbaColor,
    #[dynamic(default = "default_button_fg")]
    pub button_fg: RgbaColor,
    #[dynamic(default = "default_button_bg")]
    pub button_bg: RgbaColor,
    #[dynamic(default = "default_button_hover_fg")]
    pub button_hover_fg: RgbaColor,
    #[dynamic(default = "default_button_hover_bg")]
    pub button_hover_bg: RgbaColor,

    #[dynamic(default)]
    pub font: Option<TextStyle>,
    #[dynamic(default)]
    pub font_size: Option<f64>,

    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_zero_pixel")]
    pub border_left_width: Dimension,
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_zero_pixel")]
    pub border_right_width: Dimension,
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_zero_pixel")]
    pub border_top_height: Dimension,
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_zero_pixel")]
    pub border_bottom_height: Dimension,

    pub border_left_color: Option<RgbaColor>,
    pub border_right_color: Option<RgbaColor>,
    pub border_top_color: Option<RgbaColor>,
    pub border_bottom_color: Option<RgbaColor>,
}

const fn default_zero_pixel() -> Dimension {
    Dimension::Pixels(0.)
}

impl Default for WindowFrameConfig {
    fn default() -> Self {
        Self {
            inactive_titlebar_bg: default_inactive_titlebar_bg(),
            active_titlebar_bg: default_active_titlebar_bg(),
            inactive_titlebar_fg: default_inactive_titlebar_fg(),
            active_titlebar_fg: default_active_titlebar_fg(),
            inactive_titlebar_border_bottom: default_inactive_titlebar_border_bottom(),
            active_titlebar_border_bottom: default_active_titlebar_border_bottom(),
            button_fg: default_button_fg(),
            button_bg: default_button_bg(),
            button_hover_fg: default_button_hover_fg(),
            button_hover_bg: default_button_hover_bg(),
            font: None,
            font_size: None,
            border_left_width: default_zero_pixel(),
            border_right_width: default_zero_pixel(),
            border_top_height: default_zero_pixel(),
            border_bottom_height: default_zero_pixel(),
            border_left_color: None,
            border_right_color: None,
            border_top_color: None,
            border_bottom_color: None,
        }
    }
}

// OnlyTerm defaults the window frame (titlebar/tab-bar chrome in
// integrated-title-bar mode) to a light, GitHub-style palette matching
// the light `colors` default in config.rs's `default_colors()`, instead
// of upstream's dark titlebar - this is what was showing as a solid
// black/near-black stripe above/around the tab bar on an otherwise
// light theme.
fn default_inactive_titlebar_bg() -> RgbaColor {
    RgbColor::new_8bpc(0xee, 0xf1, 0xf4).into()
}

fn default_active_titlebar_bg() -> RgbaColor {
    RgbColor::new_8bpc(0xe8, 0xed, 0xf2).into()
}

fn default_inactive_titlebar_fg() -> RgbaColor {
    RgbColor::new_8bpc(0x57, 0x60, 0x6a).into()
}

fn default_active_titlebar_fg() -> RgbaColor {
    RgbColor::new_8bpc(0x1f, 0x23, 0x28).into()
}

fn default_inactive_titlebar_border_bottom() -> RgbaColor {
    RgbColor::new_8bpc(0xd0, 0xd7, 0xde).into()
}

fn default_active_titlebar_border_bottom() -> RgbaColor {
    RgbColor::new_8bpc(0xd0, 0xd7, 0xde).into()
}

fn default_button_hover_fg() -> RgbaColor {
    RgbColor::new_8bpc(0x1f, 0x23, 0x28).into()
}

fn default_button_fg() -> RgbaColor {
    RgbColor::new_8bpc(0x1f, 0x23, 0x28).into()
}

fn default_button_hover_bg() -> RgbaColor {
    RgbColor::new_8bpc(0xd0, 0xd7, 0xde).into()
}

fn default_button_bg() -> RgbaColor {
    RgbColor::new_8bpc(0xe8, 0xed, 0xf2).into()
}
