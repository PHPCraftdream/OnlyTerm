use std::collections::HashMap;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub struct ScreenInfo {
    pub name: String,
    pub x: isize,
    pub y: isize,
    pub width: isize,
    pub height: isize,
    pub scale: f64,
    pub max_fps: Option<usize>,
    pub effective_dpi: Option<f64>,
}

#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub struct Screens {
    pub main: ScreenInfo,
    pub active: ScreenInfo,
    pub by_name: HashMap<String, ScreenInfo>,
    pub origin_x: isize,
    pub origin_y: isize,
    pub virtual_width: isize,
    pub virtual_height: isize,
}

impl From<window::screen::ScreenInfo> for ScreenInfo {
    fn from(info: window::screen::ScreenInfo) -> Self {
        Self {
            name: info.name,
            x: info.rect.min_x(),
            y: info.rect.min_y(),
            width: info.rect.width(),
            height: info.rect.height(),
            scale: info.scale,
            max_fps: info.max_fps,
            effective_dpi: info.effective_dpi,
        }
    }
}

impl From<window::screen::Screens> for Screens {
    fn from(screens: window::screen::Screens) -> Self {
        let origin_x = screens.virtual_rect.min_x();
        let origin_y = screens.virtual_rect.min_y();
        let virtual_width = screens.virtual_rect.width();
        let virtual_height = screens.virtual_rect.height();
        Self {
            main: screens.main.into(),
            active: screens.active.into(),
            by_name: screens
                .by_name
                .into_iter()
                .map(|(k, info)| (k, info.into()))
                .collect(),
            origin_x,
            origin_y,
            virtual_width,
            virtual_height,
        }
    }
}
