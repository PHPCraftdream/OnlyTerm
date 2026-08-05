use crate::*;
use std::convert::TryFrom;
use std::str::FromStr;
use termwiz::color::ColorSpec as TWColorSpec;
pub use termwiz::color::{AnsiColor, ColorAttribute, RgbColor, SrgbaTuple};
use wezterm_dynamic::{FromDynamic, ToDynamic};
use wezterm_term::color::ColorPalette;

mod color_scheme;
mod tabbar;
mod window_frame;

pub use color_scheme::*;
pub use tabbar::*;
pub use window_frame::*;

#[derive(Debug, Copy, Clone, FromDynamic, ToDynamic)]
pub struct HsbTransform {
    #[dynamic(default = "default_one_point_oh")]
    pub hue: f32,
    #[dynamic(default = "default_one_point_oh")]
    pub saturation: f32,
    #[dynamic(default = "default_one_point_oh")]
    pub brightness: f32,
}

impl Default for HsbTransform {
    fn default() -> Self {
        Self {
            hue: 1.,
            saturation: 1.,
            brightness: 1.,
        }
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, FromDynamic, ToDynamic)]
#[dynamic(try_from = "String", into = "String")]
pub struct RgbaColor {
    #[dynamic(flatten)]
    color: SrgbaTuple,
}

impl From<RgbColor> for RgbaColor {
    fn from(color: RgbColor) -> Self {
        Self {
            color: color.into(),
        }
    }
}

impl From<SrgbaTuple> for RgbaColor {
    fn from(color: SrgbaTuple) -> Self {
        Self { color }
    }
}

impl From<(u8, u8, u8)> for RgbaColor {
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        let color: SrgbaTuple = (r, g, b).into();
        Self { color }
    }
}

impl std::ops::Deref for RgbaColor {
    type Target = SrgbaTuple;
    fn deref(&self) -> &SrgbaTuple {
        &self.color
    }
}

impl From<&RgbaColor> for String {
    fn from(val: &RgbaColor) -> Self {
        val.color.to_string()
    }
}

impl From<RgbaColor> for String {
    fn from(val: RgbaColor) -> Self {
        val.color.to_string()
    }
}

impl From<RgbaColor> for SrgbaTuple {
    fn from(val: RgbaColor) -> Self {
        val.color
    }
}

impl TryFrom<String> for RgbaColor {
    type Error = anyhow::Error;
    fn try_from(s: String) -> anyhow::Result<RgbaColor> {
        Ok(RgbaColor {
            color: SrgbaTuple::from_str(&s)
                .map_err(|_| anyhow::anyhow!("failed to parse {} as RgbaColor", s))?,
        })
    }
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpec {
    AnsiColor(AnsiColor),
    Color(RgbaColor),
    Default,
}

impl From<AnsiColor> for ColorSpec {
    fn from(color: AnsiColor) -> ColorSpec {
        Self::AnsiColor(color)
    }
}

impl From<ColorSpec> for ColorAttribute {
    fn from(val: ColorSpec) -> Self {
        match val {
            ColorSpec::AnsiColor(c) => ColorAttribute::PaletteIndex(c.into()),
            ColorSpec::Color(RgbaColor { color }) => {
                ColorAttribute::TrueColorWithDefaultFallback(color)
            }
            ColorSpec::Default => ColorAttribute::Default,
        }
    }
}

impl From<ColorSpec> for TWColorSpec {
    fn from(val: ColorSpec) -> Self {
        match val {
            ColorSpec::AnsiColor(c) => c.into(),
            ColorSpec::Color(RgbaColor { color }) => TWColorSpec::TrueColor(color),
            ColorSpec::Default => TWColorSpec::Default,
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct Palette {
    /// The text color to use when the attributes are reset to default
    pub foreground: Option<RgbaColor>,
    /// The background color to use when the attributes are reset to default
    pub background: Option<RgbaColor>,
    /// The color of the cursor
    pub cursor_fg: Option<RgbaColor>,
    pub cursor_bg: Option<RgbaColor>,
    pub cursor_border: Option<RgbaColor>,
    /// The color of selected text
    pub selection_fg: Option<RgbaColor>,
    pub selection_bg: Option<RgbaColor>,
    /// A list of 8 colors corresponding to the basic ANSI palette
    pub ansi: Option<[RgbaColor; 8]>,
    /// A list of 8 colors corresponding to bright versions of the
    /// ANSI palette
    pub brights: Option<[RgbaColor; 8]>,
    /// A map for setting arbitrary colors ranging from 16 to 256 in the color
    /// palette
    #[dynamic(default)]
    pub indexed: HashMap<u8, RgbaColor>,
    /// Configure the colors and styling of the tab bar
    pub tab_bar: Option<TabBarColors>,
    /// The color of the "thumb" of the scrollbar; the segment that
    /// represents the current viewable area
    pub scrollbar_thumb: Option<RgbaColor>,
    /// The color of the split line between panes
    pub split: Option<RgbaColor>,
    /// The color of the visual bell. If unspecified, the foreground
    /// color is used instead.
    pub visual_bell: Option<RgbaColor>,
    /// The color to use for the cursor when a dead key or leader state is active
    pub compose_cursor: Option<RgbaColor>,

    pub copy_mode_active_highlight_fg: Option<ColorSpec>,
    pub copy_mode_active_highlight_bg: Option<ColorSpec>,
    pub copy_mode_inactive_highlight_fg: Option<ColorSpec>,
    pub copy_mode_inactive_highlight_bg: Option<ColorSpec>,

    pub quick_select_label_fg: Option<ColorSpec>,
    pub quick_select_label_bg: Option<ColorSpec>,
    pub quick_select_match_fg: Option<ColorSpec>,
    pub quick_select_match_bg: Option<ColorSpec>,

    pub input_selector_label_fg: Option<ColorSpec>,
    pub input_selector_label_bg: Option<ColorSpec>,

    pub launcher_label_fg: Option<ColorSpec>,
    pub launcher_label_bg: Option<ColorSpec>,
}

impl Palette {
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
            foreground: overlay!(foreground),
            background: overlay!(background),
            cursor_fg: overlay!(cursor_fg),
            cursor_bg: overlay!(cursor_bg),
            cursor_border: overlay!(cursor_border),
            selection_fg: overlay!(selection_fg),
            selection_bg: overlay!(selection_bg),
            ansi: overlay!(ansi),
            brights: overlay!(brights),
            tab_bar: match (&self.tab_bar, &other.tab_bar) {
                (Some(a), Some(b)) => Some(a.overlay_with(b)),
                (None, Some(b)) => Some(b.clone()),
                (Some(a), None) => Some(a.clone()),
                (None, None) => None,
            },
            indexed: {
                let mut map = self.indexed.clone();
                for (k, v) in &other.indexed {
                    map.insert(*k, *v);
                }
                map
            },
            scrollbar_thumb: overlay!(scrollbar_thumb),
            split: overlay!(split),
            visual_bell: overlay!(visual_bell),
            compose_cursor: overlay!(compose_cursor),
            copy_mode_active_highlight_fg: overlay!(copy_mode_active_highlight_fg),
            copy_mode_active_highlight_bg: overlay!(copy_mode_active_highlight_bg),
            copy_mode_inactive_highlight_fg: overlay!(copy_mode_inactive_highlight_fg),
            copy_mode_inactive_highlight_bg: overlay!(copy_mode_inactive_highlight_bg),
            quick_select_label_fg: overlay!(quick_select_label_fg),
            quick_select_label_bg: overlay!(quick_select_label_bg),
            quick_select_match_fg: overlay!(quick_select_match_fg),
            quick_select_match_bg: overlay!(quick_select_match_bg),
            input_selector_label_fg: overlay!(input_selector_label_fg),
            input_selector_label_bg: overlay!(input_selector_label_bg),
            launcher_label_fg: overlay!(launcher_label_fg),
            launcher_label_bg: overlay!(launcher_label_bg),
        }
    }
}

impl From<ColorPalette> for Palette {
    fn from(cp: ColorPalette) -> Palette {
        let mut p = Palette::default();
        macro_rules! apply_color {
            ($name:ident) => {
                p.$name = Some(cp.$name.into());
            };
        }
        apply_color!(foreground);
        apply_color!(background);
        apply_color!(cursor_fg);
        apply_color!(cursor_bg);
        apply_color!(cursor_border);
        apply_color!(selection_fg);
        apply_color!(selection_bg);
        apply_color!(scrollbar_thumb);
        apply_color!(split);

        let mut ansi = [RgbaColor::default(); 8];
        for (idx, col) in cp.colors.0[0..8].iter().enumerate() {
            ansi[idx] = (*col).into();
        }
        p.ansi = Some(ansi);

        let mut brights = [RgbaColor::default(); 8];
        for (idx, col) in cp.colors.0[8..16].iter().enumerate() {
            brights[idx] = (*col).into();
        }
        p.brights = Some(brights);

        for (idx, col) in cp.colors.0.iter().enumerate().skip(16) {
            p.indexed.insert(idx as u8, (*col).into());
        }

        p
    }
}

impl From<Palette> for ColorPalette {
    fn from(cfg: Palette) -> ColorPalette {
        let mut p = ColorPalette::default();
        macro_rules! apply_color {
            ($name:ident) => {
                if let Some($name) = cfg.$name {
                    p.$name = $name.into();
                }
            };
        }
        apply_color!(foreground);
        apply_color!(background);
        apply_color!(cursor_fg);
        apply_color!(cursor_bg);
        apply_color!(cursor_border);
        apply_color!(selection_fg);
        apply_color!(selection_bg);
        apply_color!(scrollbar_thumb);
        apply_color!(split);

        if let Some(ansi) = cfg.ansi {
            for (idx, col) in ansi.iter().enumerate() {
                p.colors.0[idx] = (*col).into();
            }
        }
        if let Some(brights) = cfg.brights {
            for (idx, col) in brights.iter().enumerate() {
                p.colors.0[idx + 8] = (*col).into();
            }
        }
        for (&idx, &col) in &cfg.indexed {
            if idx < 16 {
                log::warn!(
                    "Ignoring invalid colors.indexed index {}; \
                           use `ansi` or `brights` to specify lower indices",
                    idx
                );
                continue;
            }
            p.colors.0[idx as usize] = col.into();
        }
        p
    }
}

#[cfg(test)]
#[test]
fn test_indexed_colors() {
    let scheme = r##"
[colors]
foreground = "#005661"
background = "#fef8ec"
cursor_bg = "#005661"
cursor_border = "#005661"
cursor_fg = "#ffffff"
selection_bg = "#cfe7f0"
selection_fg = "#005661"

ansi = [ "#8ca6a6" ,"#e64100" ,"#00b368" ,"#fa8900" ,"#0095a8" ,"#ff5792" ,"#00bdd6" ,"#005661" ]
brights = [ "#8ca6a6" ,"#e5164a" ,"#00b368" ,"#b3694d" ,"#0094f0" ,"#ff5792" ,"#00bdd6" ,"#004d57" ]

[colors.indexed]
52 = "#fbdada" # minus
88 = "#f6b6b6" # minus emph
22 = "#d6ffd6" # plus
28 = "#adffad" # plus emph
53 = "#feecf7" # purple
17 = "#e5dff6" # blue
23 = "#d8fdf6" # cyan
58 = "#f4ffe0" # yellow
"##;
    let scheme = ColorSchemeFile::from_toml_str(scheme).unwrap();
    assert_eq!(
        scheme.colors.indexed.get(&52),
        Some(&RgbColor::new_8bpc(0xfb, 0xda, 0xda).into())
    );
}
