#![warn(clippy::undocumented_unsafe_blocks)]
//! Configuration for the gui portion of the terminal

use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::DirBuilder;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

mod background;
mod bell;
mod cell;
mod color;
mod config;
mod config_types;
mod daemon;
mod dynamic_convert;
mod exec_domain;
mod font;
mod font_weight;
mod text_style;
mod frontend;
pub mod keyassignment;
mod keys;
pub mod ktav_value;
pub mod meta;
mod scheme_data;
mod serial;
mod terminal;
mod units;
mod unix;
mod version;
pub mod window;
mod configuration;

pub use crate::config::*;
pub use background::*;
pub use bell::*;
pub use cell::*;
pub use color::*;
pub use config_types::*;
pub use daemon::*;
pub use exec_domain::*;
pub use font::*;
pub use frontend::*;
pub use keys::*;
pub use serial::*;
pub use terminal::*;
pub use units::*;
pub use unix::*;
pub use version::*;
pub(crate) use dynamic_convert::*;
pub use configuration::*;

pub(crate) use configuration::{default_config_with_overrides_applied};
pub(crate) use configuration::{CONFIG_FILE_OVERRIDE, CONFIG_OVERRIDES, CONFIG_SKIP};

lazy_static! {
    pub static ref HOME_DIR: PathBuf = dirs_next::home_dir().expect("can't find HOME dir");
    pub static ref CONFIG_DIRS: Vec<PathBuf> = config_dirs();
    pub static ref RUNTIME_DIR: PathBuf = compute_runtime_dir().unwrap();
    pub static ref DATA_DIR: PathBuf = compute_data_dir().unwrap();
    pub static ref CACHE_DIR: PathBuf = compute_cache_dir().unwrap();
    pub static ref COLOR_SCHEMES: HashMap<String, Palette> = build_default_schemes();
}

pub fn build_default_schemes() -> HashMap<String, Palette> {
    let mut color_schemes = HashMap::new();
    for (scheme_name, data) in scheme_data::SCHEMES.iter() {
        let scheme_name = scheme_name.to_string();
        let scheme = ColorSchemeFile::from_toml_str(data).unwrap();
        color_schemes.insert(scheme_name, scheme.colors.clone());
        for alias in scheme.metadata.aliases {
            color_schemes.insert(alias, scheme.colors.clone());
        }
    }
    color_schemes
}

pub fn create_user_owned_dirs(p: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);

    #[cfg(unix)]
    {
        builder.mode(0o700);
    }

    builder.create(p)?;
    Ok(())
}

fn xdg_config_home() -> PathBuf {
    match std::env::var_os("XDG_CONFIG_HOME").map(|s| PathBuf::from(s).join("onlyterm")) {
        Some(p) => p,
        None => HOME_DIR.join(".onlyterm"),
    }
}

fn config_dirs() -> Vec<PathBuf> {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut dirs = vec![xdg_config_home()];

    #[cfg(unix)]
    if let Some(d) = std::env::var_os("XDG_CONFIG_DIRS") {
        dirs.extend(std::env::split_paths(&d).map(|s| PathBuf::from(s).join("onlyterm")));
    }

    dirs
}

fn default_one_point_oh_f64() -> f64 {
    1.0
}

fn default_one_point_oh() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}