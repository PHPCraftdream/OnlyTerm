#![warn(clippy::undocumented_unsafe_blocks)]
//! Configuration for the gui portion of the terminal

use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::DirBuilder;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod config;
mod domains;
mod process;
mod ui;
mod value;

pub(crate) use ui::{appearance, font, input};

// Preserve the original public module paths; other crates depend on them.
pub use appearance::window;
pub use input::keyassignment;
pub use process::{powershell, shell};
pub use value::{ktav_value, meta};

// Preserve the original internal module paths for in-crate callers.
pub(crate) use appearance::{background, bell, color, frontend};
pub(crate) use domains::{exec_domain, unix};
pub(crate) use input::keys;
pub(crate) use process::daemon;
pub(crate) use value::{config_types, units, version};

// Preserve the original item-level re-export surface.
pub use appearance::background::*;
pub use appearance::bell::*;
pub use appearance::color::*;
pub use appearance::frontend::*;
pub use appearance::window::*;
pub use config::*;
pub use domains::exec_domain::*;
pub use domains::serial::*;
pub use domains::unix::*;
pub use font::*;
pub use input::keyassignment::*;
pub use input::keys::*;
pub use process::daemon::*;
pub use process::powershell::*;
pub use process::shell::*;
pub use process::start_conf::*;
pub use value::config_types::*;
pub(crate) use value::dynamic_convert::*;
pub use value::ktav_value::*;
pub use value::meta::*;
pub use value::units::*;
pub use value::version::*;

use onlyterm_color_schemes_data as scheme_data;

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

lazy_static! {
    /// Memoizes the result of `lookup_default_scheme` so repeated lookups of
    /// the same name (config reloads re-resolve the scheme every time) don't
    /// re-parse its TOML. Values are leaked rather than stored inline so
    /// callers can hold a `&'static Palette` without keeping the mutex
    /// locked; there are at most `SCHEMES.len()` of them and they live for
    /// the life of the process anyway, so nothing meaningfully "leaks".
    static ref SCHEME_CACHE: Mutex<HashMap<String, Option<&'static Palette>>> =
        Mutex::new(HashMap::new());
}

/// Look up one of the built-in color schemes by name.
///
/// Deliberately does *not* go through `COLOR_SCHEMES`: that map is built by
/// TOML-parsing all ~1000 bundled schemes up front, which measured at ~2.2s
/// of startup time (debug build) for a config that names a single scheme --
/// it was the single largest contributor to the delay before the window
/// appeared (task #405). Since a config can only name one scheme, parse just
/// that one.
///
/// The linear scan over `SCHEMES` is only string comparisons against static
/// data and is negligible next to parsing even one TOML document. The
/// fallback path exists because a scheme can also be referenced by an alias,
/// which is only known after parsing that scheme's metadata; that is rare,
/// so it keeps paying the old build-everything cost rather than complicating
/// the common case.
pub fn lookup_default_scheme(name: &str) -> Option<&'static Palette> {
    if let Some(cached) = SCHEME_CACHE.lock().unwrap().get(name) {
        return *cached;
    }

    let resolved = scheme_data::SCHEMES
        .iter()
        .find(|(scheme_name, _)| *scheme_name == name)
        .and_then(|(_, data)| ColorSchemeFile::from_toml_str(data).ok())
        .map(|scheme| &*Box::leak(Box::new(scheme.colors)))
        // Not a primary scheme name: it may still be an alias, which can only
        // be discovered by parsing metadata, so fall back to the full map.
        .or_else(|| COLOR_SCHEMES.get(name));

    SCHEME_CACHE
        .lock()
        .unwrap()
        .insert(name.to_string(), resolved);
    resolved
}

pub fn create_user_owned_dirs(p: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
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
    vec![xdg_config_home()]
}

pub(crate) fn default_one_point_oh_f64() -> f64 {
    1.0
}

pub(crate) fn default_one_point_oh() -> f32 {
    1.0
}

pub(crate) fn default_true() -> bool {
    true
}
