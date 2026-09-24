use crate::background::{BackgroundLayer, Gradient};
use crate::bell::{AudibleBell, EasingFunction, VisualBell};
use crate::color::{HsbTransform, Palette, TabBarStyle, WindowFrameConfig};
use crate::config_types::*;
use crate::daemon::DaemonOptions;
use crate::exec_domain::ExecDomain;
use crate::font::{
    AllowSquareGlyphOverflow, FontLocatorSelection, FontRasterizerSelection, FontShaperSelection,
    FreeTypeLoadFlags, FreeTypeLoadTarget, StyleRule, TextStyle,
};
use crate::frontend::FrontEndSelection;
use crate::keyassignment::SpawnCommand;
use crate::keys::{Key, LeaderKey, Mouse};
use crate::units::Dimension;
use crate::unix::UnixDomain;
use crate::{
    default_one_point_oh, default_one_point_oh_f64, default_true,
    default_win32_acrylic_accent_color, CellWidth, GpuInfo, IntegratedTitleButtonColor,
    KeyMapPreference, RgbaColor, SerialDomain, SystemBackdrop, WebGpuPowerPreference,
};
use onlyterm_bidi::ParagraphDirectionHint;
use onlyterm_config_derive::ConfigMeta;
use onlyterm_dynamic::{FromDynamic, ToDynamic};
use onlyterm_input_types::{
    IntegratedTitleButton, IntegratedTitleButtonAlignment, IntegratedTitleButtonStyle, Modifiers,
    UIKeyCapRendering, WindowDecorations,
};
use std::collections::HashMap;
use std::path::PathBuf;
use termwiz::hyperlink;

mod config_definition;

pub(crate) use config_definition::config_paths::{
    compute_cache_dir, compute_data_dir, compute_runtime_dir,
};
pub use config_definition::configuration::*;
pub(crate) use config_definition::configuration::{
    default_config_with_overrides_applied, CONFIG_FILE_OVERRIDE, CONFIG_OVERRIDES, CONFIG_SKIP,
};
pub use config_definition::defaults::*;
pub use config_definition::terminal::*;
pub use config_definition::*;
