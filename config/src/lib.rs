//! Configuration for the gui portion of the terminal

use anyhow::{anyhow, bail, Context, Error};
use lazy_static::lazy_static;
use ordered_float::NotNan;
use smol::channel::{Receiver, Sender};
use smol::prelude::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::fs::DirBuilder;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wezterm_dynamic::{FromDynamic, FromDynamicOptions, ToDynamic, UnknownFieldAction, Value};
use wezterm_term::UnicodeVersion;

mod background;
mod bell;
mod cell;
mod color;
mod config;
mod daemon;
mod exec_domain;
mod font;
mod frontend;
pub mod keyassignment;
mod keys;
pub mod meta;
pub mod rhai_bridge;
pub mod rhai_engine;
pub mod rhai_value;
mod scheme_data;
mod serial;
mod terminal;
mod units;
mod unix;
mod version;
pub mod window;
mod wsl;

pub use crate::config::*;
pub use background::*;
pub use bell::*;
pub use cell::*;
pub use color::*;
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
pub use wsl::*;

type ErrorCallback = fn(&str);

lazy_static! {
    pub static ref HOME_DIR: PathBuf = dirs_next::home_dir().expect("can't find HOME dir");
    pub static ref CONFIG_DIRS: Vec<PathBuf> = config_dirs();
    pub static ref RUNTIME_DIR: PathBuf = compute_runtime_dir().unwrap();
    pub static ref DATA_DIR: PathBuf = compute_data_dir().unwrap();
    pub static ref CACHE_DIR: PathBuf = compute_cache_dir().unwrap();
    static ref CONFIG: Configuration = Configuration::new();
    static ref CONFIG_FILE_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);
    static ref CONFIG_SKIP: AtomicBool = AtomicBool::new(false);
    static ref CONFIG_OVERRIDES: Mutex<Vec<(String, String)>> = Mutex::new(vec![]);
    static ref SHOW_ERROR: Mutex<Option<ErrorCallback>> =
        Mutex::new(Some(|e| log::error!("{}", e)));
    static ref RHAI_PIPE: RhaiPipe = RhaiPipe::new();
    pub static ref COLOR_SCHEMES: HashMap<String, Palette> = build_default_schemes();
}

thread_local! {
    static RHAI_CONFIG: RefCell<Option<RhaiConfigCell>> = RefCell::new(None);
}

fn toml_table_has_numeric_keys(t: &toml::value::Table) -> bool {
    t.keys().all(|k| k.parse::<isize>().is_ok())
}

fn json_object_has_numeric_keys(t: &serde_json::Map<String, serde_json::Value>) -> bool {
    t.keys().all(|k| k.parse::<isize>().is_ok())
}

fn toml_to_dynamic(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(s) => s.to_dynamic(),
        toml::Value::Integer(n) => n.to_dynamic(),
        toml::Value::Float(n) => n.to_dynamic(),
        toml::Value::Boolean(b) => b.to_dynamic(),
        toml::Value::Datetime(d) => d.to_string().to_dynamic(),
        toml::Value::Array(a) => a
            .iter()
            .map(toml_to_dynamic)
            .collect::<Vec<_>>()
            .to_dynamic(),
        // Allow `colors.indexed` to be passed through with actual integer keys
        toml::Value::Table(t) if toml_table_has_numeric_keys(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (k.parse::<isize>().unwrap().to_dynamic(), toml_to_dynamic(v)))
                .collect::<BTreeMap<_, _>>()
                .into(),
        ),
        toml::Value::Table(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (Value::String(k.to_string()), toml_to_dynamic(v)))
                .collect::<BTreeMap<_, _>>()
                .into(),
        ),
    }
}

fn json_to_dynamic(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => b.to_dynamic(),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_dynamic()
            } else if let Some(i) = n.as_u64() {
                i.to_dynamic()
            } else if let Some(f) = n.as_f64() {
                f.to_dynamic()
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => s.to_dynamic(),
        serde_json::Value::Array(a) => a
            .iter()
            .map(json_to_dynamic)
            .collect::<Vec<_>>()
            .to_dynamic(),
        // Allow `colors.indexed` to be passed through with actual integer keys
        serde_json::Value::Object(t) if json_object_has_numeric_keys(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (k.parse::<isize>().unwrap().to_dynamic(), json_to_dynamic(v)))
                .collect::<BTreeMap<_, _>>()
                .into(),
        ),
        serde_json::Value::Object(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (Value::String(k.to_string()), json_to_dynamic(v)))
                .collect::<BTreeMap<_, _>>()
                .into(),
        ),
    }
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

struct RhaiPipe {
    sender: Sender<rhai_engine::RhaiEventScript>,
    receiver: Receiver<rhai_engine::RhaiEventScript>,
}
impl RhaiPipe {
    pub fn new() -> Self {
        let (sender, receiver) = smol::channel::unbounded();
        Self { sender, receiver }
    }
}

/// The implementation is only slightly crazy...
/// `rhai::Engine`/`rhai::AST` are neither `Send` nor `Sync` in this workspace (the
/// `sync` cargo feature is not enabled; see `config/src/rhai_engine.rs`'s
/// module-level doc comment and the L4.6 migration note below). We take care to
/// build and reference the live engine only from the main thread of the
/// application.
/// We also need to take care to keep this engine alive if a long running
/// future is outstanding while a config reload happens.
/// We have to use `Rc` to manage its lifetime, but due to some issues
/// with rust's async lifetime tracking we need to indirectly schedule
/// some of the futures to avoid it thinking that the generated future
/// in the async block needs to be Send.
///
/// A further complication is that config reloading tends to happen in
/// a background filesystem watching thread.
///
/// The result of all these constraints is that the RhaiPipe struct above
/// is used as a channel to transport newly loaded configs' *event-script
/// descriptor* (`RhaiEventScript`, plain `Send` data: just the script source text
/// and its path) to the main thread, which is where the actual (non-`Send`)
/// `rhai::Engine`+`AST` gets (re)built, via `RhaiConfigState::from_script`. This
/// replaces the pre-L4.6 design, which sent an already-built `mlua::Lua` (which
/// *was* `Send`) across an equivalent `LuaPipe`; rhai's engine can't follow that
/// exact shape, so the message that crosses the channel had to change from "the
/// engine itself" to "what's needed to rebuild the engine where it will be used".
///
/// The main thread pops the loaded descriptors to obtain the latest one
/// and updates RhaiConfigCell
struct RhaiConfigCell {
    state: Option<Rc<rhai_engine::RhaiConfigState>>,
}

impl RhaiConfigCell {
    /// Consume any event-script descriptors sent to us via the
    /// config loader until we end up with the most
    /// recent one being referenced by RHAI_CONFIG.
    fn update_to_latest(&mut self) {
        let mut latest = None;
        while let Ok(script) = RHAI_PIPE.receiver.try_recv() {
            latest = Some(script);
        }
        if let Some(script) = latest {
            match rhai_engine::RhaiConfigState::from_script(&script) {
                Ok(state) => {
                    self.state.replace(Rc::new(state));
                }
                Err(err) => {
                    // Keep serving the previous generation's handlers rather than
                    // losing the event bridge entirely on a transient rebuild
                    // failure (e.g. a script that parsed fine on the background
                    // thread encountering a resource limit differently here --
                    // shouldn't normally happen, since it's the same source text
                    // and engine construction, but fail soft rather than panic).
                    log::error!("Failed to rebuild rhai event-callback engine: {:#}", err);
                }
            }
        }
    }

    /// Take a reference on the latest generation of the rhai config state
    fn get_state(&self) -> Option<Rc<rhai_engine::RhaiConfigState>> {
        self.state.as_ref().map(Rc::clone)
    }
}

pub fn designate_this_as_the_main_thread() {
    RHAI_CONFIG.with(|lc| {
        let mut lc = lc.borrow_mut();
        if lc.is_none() {
            lc.replace(RhaiConfigCell { state: None });
        }
    });
}

#[must_use = "Cancels the subscription when dropped"]
pub struct ConfigSubscription(usize);

impl Drop for ConfigSubscription {
    fn drop(&mut self) {
        CONFIG.unsub(self.0);
    }
}

pub fn subscribe_to_config_reload<F>(subscriber: F) -> ConfigSubscription
where
    F: Fn() -> bool + 'static + Send,
{
    ConfigSubscription(CONFIG.subscribe(subscriber))
}

/// Spawn a future that will run with an optional rhai event-callback state from
/// the most recently loaded configuration.
/// The `func` argument is passed the state and must return a Future.
///
/// This function MUST only be called from the main thread.
/// In exchange for the caller checking for this, the parameters to
/// this method are not required to be Send.
///
/// Calling this function from a secondary thread will panic.
/// You should use `with_rhai_config` if you are triggering a
/// call from a secondary thread.
pub async fn with_rhai_config_on_main_thread<F, RETF, RET>(func: F) -> anyhow::Result<RET>
where
    F: FnOnce(Option<Rc<rhai_engine::RhaiConfigState>>) -> RETF,
    RETF: Future<Output = anyhow::Result<RET>>,
{
    let state = RHAI_CONFIG.with(|lc| {
        let mut lc = lc.borrow_mut();
        let lc = lc.as_mut().expect(
            "with_rhai_config_on_main_thread not called
             from main thread, use with_rhai_config instead!",
        );
        lc.update_to_latest();
        lc.get_state()
    });

    func(state).await
}

pub fn run_immediate_with_rhai_config<F, RET>(func: F) -> anyhow::Result<RET>
where
    F: FnOnce(Option<Rc<rhai_engine::RhaiConfigState>>) -> anyhow::Result<RET>,
{
    let state = RHAI_CONFIG.with(|lc| {
        let mut lc = lc.borrow_mut();
        let lc = lc.as_mut().expect(
            "with_rhai_config_on_main_thread not called
             from main thread, use with_rhai_config instead!",
        );
        lc.update_to_latest();
        lc.get_state()
    });

    func(state)
}

fn schedule_with_rhai<F, RETF, RET>(func: F) -> promise::spawn::Task<anyhow::Result<RET>>
where
    F: 'static,
    RET: 'static,
    F: Fn(Option<Rc<rhai_engine::RhaiConfigState>>) -> RETF,
    RETF: Future<Output = anyhow::Result<RET>>,
{
    promise::spawn::spawn(async move { with_rhai_config_on_main_thread(func).await })
}

/// Spawn a future that will run with an optional rhai event-callback state from
/// the most recently loaded configuration.
/// The `func` argument is passed the state and must return a Future.
pub async fn with_rhai_config<F, RETF, RET>(func: F) -> anyhow::Result<RET>
where
    F: Fn(Option<Rc<rhai_engine::RhaiConfigState>>) -> RETF,
    RETF: Future<Output = anyhow::Result<RET>> + Send + 'static,
    F: Send + 'static,
    RET: Send + 'static,
{
    promise::spawn::spawn_into_main_thread(async move { schedule_with_rhai(func).await }).await
}

fn default_config_with_overrides_applied() -> anyhow::Result<Config> {
    // Cause the default config to be re-evaluated with the overrides applied
    let rhai_engine =
        rhai_engine::make_rhai_engine(Path::new("override")).context("make_rhai_engine")?;
    let table = rhai::Dynamic::from_map(rhai::Map::new());
    let config =
        Config::apply_overrides_to_rhai(&rhai_engine, table).context("apply_overrides_to_rhai")?;

    let dyn_config =
        rhai_value::rhai_dynamic_to_dynamic(&config).map_err(|e| anyhow!("{e}"))?;

    let cfg: Config = Config::from_dynamic(
        &dyn_config,
        FromDynamicOptions {
            unknown_fields: UnknownFieldAction::Deny,
            deprecated_fields: UnknownFieldAction::Warn,
        },
    )
    .context("Error converting rhai value from overrides to Config struct")?;
    // Compute but discard the key bindings here so that we raise any
    // problems earlier than we use them.
    let _ = cfg.key_bindings();

    cfg.check_consistency().context("check_consistency")?;

    Ok(cfg)
}

pub fn common_init(
    config_file: Option<&OsString>,
    overrides: &[(String, String)],
    skip_config: bool,
) -> anyhow::Result<()> {
    if let Some(config_file) = config_file {
        set_config_file_override(Path::new(config_file));
    } else if skip_config {
        CONFIG_SKIP.store(true, Ordering::Relaxed);
    }

    set_config_overrides(overrides).context("common_init: set_config_overrides")?;
    reload();
    Ok(())
}

pub fn assign_error_callback(cb: ErrorCallback) {
    let mut factory = SHOW_ERROR.lock().unwrap();
    factory.replace(cb);
}

pub fn show_error(err: &str) {
    let factory = SHOW_ERROR.lock().unwrap();
    if let Some(cb) = factory.as_ref() {
        cb(err)
    }
}

pub fn create_user_owned_dirs(p: &Path) -> anyhow::Result<()> {
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
    match std::env::var_os("XDG_CONFIG_HOME").map(|s| PathBuf::from(s).join("wezterm")) {
        Some(p) => p,
        None => HOME_DIR.join(".config").join("wezterm"),
    }
}

fn config_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.push(xdg_config_home());

    #[cfg(unix)]
    if let Some(d) = std::env::var_os("XDG_CONFIG_DIRS") {
        dirs.extend(std::env::split_paths(&d).map(|s| PathBuf::from(s).join("wezterm")));
    }

    dirs
}

pub fn set_config_file_override(path: &Path) {
    CONFIG_FILE_OVERRIDE
        .lock()
        .unwrap()
        .replace(path.to_path_buf());
}

pub fn set_config_overrides(items: &[(String, String)]) -> anyhow::Result<()> {
    *CONFIG_OVERRIDES.lock().unwrap() = items.to_vec();

    let _ = default_config_with_overrides_applied()?;
    Ok(())
}

pub fn is_config_overridden() -> bool {
    CONFIG_SKIP.load(Ordering::Relaxed)
        || !CONFIG_OVERRIDES.lock().unwrap().is_empty()
        || CONFIG_FILE_OVERRIDE.lock().unwrap().is_some()
}

/// Discard the current configuration and replace it with
/// the default configuration
pub fn use_default_configuration() {
    CONFIG.use_defaults();
}

/// Use a config that doesn't depend on the user's
/// environment and is suitable for unit testing
pub fn use_test_configuration() {
    CONFIG.use_test();
}

pub fn use_this_configuration(config: Config) {
    CONFIG.use_this_config(config);
}

/// Returns a handle to the current configuration
pub fn configuration() -> ConfigHandle {
    CONFIG.get()
}

/// Returns a version of the config (loaded from the config file)
/// with some field overridden based on the supplied overrides object.
pub fn overridden_config(overrides: &wezterm_dynamic::Value) -> Result<ConfigHandle, Error> {
    CONFIG.overridden(overrides)
}

pub fn reload() {
    CONFIG.reload();
}

/// If there was an error loading the preferred configuration,
/// return it, otherwise return the current configuration
pub fn configuration_result() -> Result<ConfigHandle, Error> {
    if let Some(error) = CONFIG.get_error() {
        bail!("{}", error);
    }
    Ok(CONFIG.get())
}

/// Returns the combined set of errors + warnings encountered
/// while loading the preferred configuration
pub fn configuration_warnings_and_errors() -> Vec<String> {
    CONFIG.get_warnings_and_errors()
}

struct ConfigInner {
    config: Arc<Config>,
    error: Option<String>,
    warnings: Vec<String>,
    generation: usize,
    watcher: Option<notify::RecommendedWatcher>,
    subscribers: HashMap<usize, Box<dyn Fn() -> bool + Send>>,
}

impl ConfigInner {
    fn new() -> Self {
        Self {
            config: Arc::new(Config::default_config()),
            error: None,
            warnings: vec![],
            generation: 0,
            watcher: None,
            subscribers: HashMap::new(),
        }
    }

    fn subscribe<F>(&mut self, subscriber: F) -> usize
    where
        F: Fn() -> bool + 'static + Send,
    {
        static SUB_ID: AtomicUsize = AtomicUsize::new(0);
        let sub_id = SUB_ID.fetch_add(1, Ordering::Relaxed);
        self.subscribers.insert(sub_id, Box::new(subscriber));
        sub_id
    }

    fn unsub(&mut self, sub_id: usize) {
        self.subscribers.remove(&sub_id);
    }

    fn notify(&mut self) {
        self.subscribers.retain(|_, notify| notify());
    }

    fn watch_path(&mut self, path: PathBuf) {
        if self.watcher.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            const DELAY: Duration = Duration::from_millis(200);
            let watcher = notify::recommended_watcher(tx).unwrap();
            let path = path.clone();

            std::thread::spawn(move || {
                // block until we get an event
                use notify::EventKind;

                fn extract_path(event: notify::Event) -> Vec<PathBuf> {
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                            event.paths
                        }
                        _ => vec![],
                    }
                }

                while let Ok(event) = rx.recv() {
                    log::debug!("event:{:?}", event);
                    match event {
                        Ok(event) => {
                            let mut paths = extract_path(event);
                            if !paths.is_empty() {
                                // Grace period to allow events to settle
                                std::thread::sleep(DELAY);
                                // Drain any other immediately ready events
                                while let Ok(Ok(event)) = rx.try_recv() {
                                    paths.append(&mut extract_path(event));
                                }
                                paths.sort();
                                paths.dedup();
                                log::debug!("paths {:?} changed, reload config", path);
                                reload();
                            }
                        }
                        Err(_) => {
                            reload();
                        }
                    }
                }
            });
            self.watcher.replace(watcher);
        }
        if let Some(watcher) = self.watcher.as_mut() {
            use notify::Watcher;
            watcher
                .watch(&path, notify::RecursiveMode::NonRecursive)
                .ok();
        }
    }

    /// Attempt to load the user's configuration.
    /// On success, clear any error and replace the current
    /// configuration.
    /// On failure, retain the existing configuration but
    /// replace any captured error message.
    ///
    /// `loaded` must have been produced by a call to `Config::load()` made
    /// *before* the `Configuration`'s mutex was locked (see
    /// `Configuration::reload`): `Config::load()` runs arbitrary
    /// `SETUP_FUNCS` (via `make_lua_context`, itself reached from
    /// `Config::try_default`/`try_load`) as a side effect of parsing the
    /// config, and at least one of those (`time-funcs::register`) calls back
    /// into `config::subscribe_to_config_reload`, which locks this same
    /// `Configuration`'s mutex. Calling `Config::load()` while already
    /// holding that mutex self-deadlocks the very first time a process
    /// loads its configuration (`std::sync::Mutex` is not reentrant) -- see
    /// BUG8. Accepting the already-loaded result here keeps that
    /// computation entirely outside of the lock.
    fn reload(&mut self, loaded: LoadedConfig) {
        let LoadedConfig {
            config,
            file_name,
            event_script,
            warnings,
            rhai_watch_paths,
        } = loaded;

        self.warnings = warnings;

        // Before we process the success/failure, extract and update
        // any paths that we should be watching
        let mut watch_paths = vec![];
        if let Some(path) = file_name {
            // Let's also watch the parent directory for folks that do
            // things with symlinks:
            if let Some(parent) = path.parent() {
                // But avoid watching the home dir itself, so that we
                // don't keep reloading every time something in the
                // home dir changes!
                // <https://github.com/wezterm/wezterm/issues/1895>
                if parent != &*HOME_DIR {
                    watch_paths.push(parent.to_path_buf());
                }
            }
            watch_paths.push(path);
        }
        // Note: the pre-L4.6 companion mlua context also merged in a
        // `"wezterm-watch-paths"` Lua-registry-backed watch list here
        // (`ConfigInner::accumulate_watch_paths`), but that companion context
        // never actually executed the user's config script (see the L4.6
        // migration note on `RhaiEventScript`), so that registry value was
        // always empty in practice; `rhai_watch_paths` below (populated from
        // the rhai engine that *does* evaluate the script, in
        // `Config::try_load`) was always the real source of watched paths
        // added via `add_to_config_reload_watch_list`.
        for path in rhai_watch_paths {
            watch_paths.push(PathBuf::from(path));
        }

        match config {
            Ok(config) => {
                self.config = Arc::new(config);
                self.error.take();
                self.generation += 1;

                // If we loaded a user config, publish this latest version of
                // the event-script descriptor to the RHAI_PIPE. This allows a
                // subsequent call to `with_rhai_config` to reference a live rhai
                // engine built from this script even though we are (probably)
                // resolving this from a background reloading thread.
                if let Some(event_script) = event_script {
                    RHAI_PIPE.sender.try_send(event_script).ok();
                }
                log::debug!("Reloaded configuration! generation={}", self.generation);
            }
            Err(err) => {
                let err = format!("{:#}", err);
                if self.generation > 0 {
                    // Only generate the message for an actual reload
                    show_error(&err);
                }
                self.error.replace(err);
            }
        }

        self.notify();
        if self.config.automatically_reload_config {
            for path in watch_paths {
                self.watch_path(path);
            }
        }
    }

    /// Discard the current configuration and any recorded
    /// error message; replace them with the default
    /// configuration
    fn use_defaults(&mut self) {
        self.config = Arc::new(Config::default_config());
        self.error.take();
        self.generation += 1;
    }

    fn use_this_config(&mut self, cfg: Config) {
        self.config = Arc::new(cfg);
        self.error.take();
        self.generation += 1;
    }

    fn overridden(&mut self, overrides: &wezterm_dynamic::Value) -> Result<ConfigHandle, Error> {
        let config = Config::load_with_overrides(overrides);
        Ok(ConfigHandle {
            config: Arc::new(config.config?),
            generation: self.generation,
        })
    }

    fn use_test(&mut self) {
        let mut config = Config::default_config();
        config.font_locator = FontLocatorSelection::ConfigDirsOnly;
        let exe_name = std::env::current_exe().unwrap();
        let exe_dir = exe_name.parent().unwrap();
        config.font_dirs.push(exe_dir.join("../../../assets/fonts"));
        // If we're building for a specific target, the dir
        // level is one deeper.
        #[cfg(target_os = "macos")]
        config
            .font_dirs
            .push(exe_dir.join("../../../../assets/fonts"));
        // Specify the same DPI used on non-mac systems so
        // that we have consistent values regardless of the
        // operating system that we're running tests on
        config.dpi.replace(96.0);
        self.config = Arc::new(config);
        self.error.take();
        self.generation += 1;
    }
}

pub struct Configuration {
    inner: Mutex<ConfigInner>,
}

impl Configuration {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ConfigInner::new()),
        }
    }

    /// Returns the effective configuration.
    pub fn get(&self) -> ConfigHandle {
        let inner = self.inner.lock().unwrap();
        ConfigHandle {
            config: Arc::clone(&inner.config),
            generation: inner.generation,
        }
    }

    /// Subscribe to config reload events
    fn subscribe<F>(&self, subscriber: F) -> usize
    where
        F: Fn() -> bool + 'static + Send,
    {
        let mut inner = self.inner.lock().unwrap();
        inner.subscribe(subscriber)
    }

    fn unsub(&self, sub_id: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner.unsub(sub_id);
    }

    /// Reset the configuration to defaults
    pub fn use_defaults(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.use_defaults();
    }

    fn use_this_config(&self, cfg: Config) {
        let mut inner = self.inner.lock().unwrap();
        inner.use_this_config(cfg);
    }

    fn overridden(&self, overrides: &wezterm_dynamic::Value) -> Result<ConfigHandle, Error> {
        let mut inner = self.inner.lock().unwrap();
        inner.overridden(overrides)
    }

    /// Use a config that doesn't depend on the user's
    /// environment and is suitable for unit testing
    pub fn use_test(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.use_test();
    }

    /// Reload the configuration
    pub fn reload(&self) {
        // Deliberately computed *before* taking the lock below: `Config::load()`
        // runs arbitrary config-setup code (via rhai's `RHAI_SETUP_FUNCS`)
        // that can call back into this `Configuration` (e.g.
        // `time-funcs::register` -> `subscribe_to_config_reload` -> `subscribe`,
        // below) to lock the same mutex. Since `std::sync::Mutex` is not
        // reentrant, doing the load while holding the lock would deadlock the
        // very first time any process loads its config. See BUG8 and the doc
        // comment on `ConfigInner::reload`.
        let loaded = Config::load();
        let mut inner = self.inner.lock().unwrap();
        inner.reload(loaded);
    }

    /// Returns a copy of any captured error message.
    /// The error message is not cleared.
    pub fn get_error(&self) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        inner.error.as_ref().cloned()
    }

    pub fn get_warnings_and_errors(&self) -> Vec<String> {
        let mut result = vec![];
        let inner = self.inner.lock().unwrap();
        if let Some(error) = &inner.error {
            result.push(error.clone());
        }
        for warning in &inner.warnings {
            result.push(warning.clone());
        }
        result
    }

    /// Returns any captured error message, and clears
    /// it from the config state.
    #[allow(dead_code)]
    pub fn clear_error(&self) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        inner.error.take()
    }
}

#[derive(Clone, Debug)]
pub struct ConfigHandle {
    config: Arc<Config>,
    generation: usize,
}

impl ConfigHandle {
    /// Returns the generation number for the configuration,
    /// allowing consuming code to know whether the config
    /// has been reloading since they last derived some
    /// information from the configuration
    pub fn generation(&self) -> usize {
        self.generation
    }

    pub fn default_config() -> Self {
        Self {
            config: Arc::new(Config::default_config()),
            generation: 0,
        }
    }

    pub fn unicode_version(&self) -> UnicodeVersion {
        UnicodeVersion {
            version: self.config.unicode_version,
            ambiguous_are_wide: self.config.treat_east_asian_ambiguous_width_as_wide,
            cell_widths: CellWidth::compile_to_map(self.config.cell_widths.clone()),
        }
    }
}

impl std::ops::Deref for ConfigHandle {
    type Target = Config;
    fn deref(&self) -> &Config {
        &*self.config
    }
}

pub struct LoadedConfig {
    pub config: anyhow::Result<Config>,
    pub file_name: Option<PathBuf>,
    /// `Send`-safe descriptor of the config script's event-callback surface
    /// (`wezterm.on`/`emit`, consumed by `mux`/`wezterm-gui` via
    /// `config::rhai::emit_sync_callback`/`emit_event`/`emit_async_callback`).
    ///
    /// Before L4.6 this field held a companion `mlua::Lua` context (`lua:
    /// Option<mlua::Lua>`), built by `crate::lua::make_lua_context` purely to
    /// give the runtime event bridge a `Send`-able value to pass across the
    /// background-reload-thread -> main-thread channel (`mlua::Lua` is `Send`,
    /// unlike `rhai::Engine`/`rhai::AST` in this workspace's non-`sync`
    /// build). See `RhaiEventScript`'s doc comment in
    /// `config/src/rhai_engine.rs` for why a plain-data descriptor plus a
    /// main-thread rebuild replaces that approach.
    pub event_script: Option<rhai_engine::RhaiEventScript>,
    pub warnings: Vec<String>,
    /// Paths accumulated via `add_to_config_reload_watch_list` while
    /// evaluating a `.rhai` config script (see `rhai_engine::ConfigReloadWatchList`).
    pub rhai_watch_paths: Vec<String>,
}

#[cfg(test)]
mod reload_notify_test {
    use super::*;
    use std::sync::atomic::AtomicUsize as StdAtomicUsize;

    /// Build a minimal `LoadedConfig` carrying the default `Config`, as if
    /// `Config::load()` had succeeded, without touching the filesystem or the
    /// process-wide `CONFIG`/`CONFIG_OVERRIDES` singletons that other tests in
    /// this crate serialize on (see `CONFIG_OVERRIDES_TEST_LOCK` in
    /// `config.rs`). This lets the notification-fanout behavior below be
    /// exercised against a private `Configuration` instance in complete
    /// isolation.
    fn loaded_default() -> LoadedConfig {
        LoadedConfig {
            config: Ok(Config::default_config()),
            file_name: None,
            event_script: None,
            warnings: vec![],
            rhai_watch_paths: vec![],
        }
    }

    /// UP-46 investigation: every live TermWindow subscribes to config
    /// reloads via `subscribe_to_config_reload` (see
    /// `wezterm-gui/src/termwindow/mod.rs`'s `TermWindow::new_window`), and
    /// *every* reload trigger -- the config-file watcher, the OS
    /// `AppearanceChanged` notification handler, and manual
    /// Ctrl+Shift+R -- funnels through this same
    /// `Configuration::reload`/`ConfigInner::reload` -> `ConfigInner::notify`
    /// path (`config::reload()` at the top of this module just calls
    /// `CONFIG.reload()`, which is this method).
    ///
    /// The reported symptom for UP-46 (upstream #3328/#5451/#6607/#2446/#4437)
    /// is that a live theme/appearance change doesn't reach every open
    /// window. This test locks in that the shared fanout mechanism itself is
    /// not the culprit: a single `reload()` call synchronously invokes every
    /// subscriber registered so far -- there is no "notify only the window
    /// that triggered the reload" shortcut, no skip based on subscription
    /// order, and no silently dropped subscriber. If this ever regresses (for
    /// example, someone reintroduces an early-return once `find` on the
    /// map hits one match, or a `HashMap` iteration accidentally gets bounded)
    /// this test fails immediately, without needing multiple live OS windows.
    ///
    /// What this test deliberately does NOT and CANNOT establish (see the
    /// UP-46 session notes): whether every OS-level window HWND actually
    /// receives its own `WM_SETTINGCHANGE`/`AppearanceChanged` notification in
    /// the same tick, and whether each window's own
    /// `wezterm.on('window-config-reloaded', ...)` handler (which
    /// independently calls `window:get_appearance()` and
    /// `window:set_config_overrides()`, per
    /// `docs/config/lua/window/get_appearance.md`) observes a consistent
    /// appearance value across windows. That requires multiple live native
    /// windows and a real OS theme toggle, which is outside what a unit test
    /// in this crate can exercise.
    #[test]
    fn reload_notifies_every_subscriber_not_just_one() {
        let configuration = Configuration::new();

        const N: usize = 5;
        let counters: Vec<Arc<StdAtomicUsize>> =
            (0..N).map(|_| Arc::new(StdAtomicUsize::new(0))).collect();

        let _subs: Vec<ConfigSubscription> = counters
            .iter()
            .map(|counter| {
                let counter = Arc::clone(counter);
                ConfigSubscription(configuration.subscribe(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    true
                }))
            })
            .collect();

        {
            let mut inner = configuration.inner.lock().unwrap();
            inner.reload(loaded_default());
        }

        for (idx, counter) in counters.iter().enumerate() {
            assert_eq!(
                counter.load(Ordering::SeqCst),
                1,
                "subscriber {idx} of {N} was not notified by a single reload() call \
                 (per-window updates should all fire together, not a subset)"
            );
        }

        // A second reload should notify all of them again -- confirms no
        // subscriber gets silently dropped after the first cycle (which
        // would manifest as "the first appearance change works, later ones
        // don't reach every window").
        {
            let mut inner = configuration.inner.lock().unwrap();
            inner.reload(loaded_default());
        }

        for (idx, counter) in counters.iter().enumerate() {
            assert_eq!(
                counter.load(Ordering::SeqCst),
                2,
                "subscriber {idx} of {N} stopped receiving notifications after the first reload"
            );
        }
    }
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
