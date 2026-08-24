use super::{Config, PathPossibility};
use crate::background::BackgroundLayer;
use crate::color::{ColorSchemeFile, Palette, TabBarColor, TabBarColors};
use crate::font::StyleRule;
use crate::keyassignment::{KeyAssignment, KeyTable, KeyTableEntry, KeyTables, MouseEventTrigger};
use crate::{
    default_config_with_overrides_applied, LoadedConfig, MouseEventTriggerMods, RgbaColor,
    CONFIG_DIRS, CONFIG_FILE_OVERRIDE, CONFIG_OVERRIDES, CONFIG_SKIP, HOME_DIR,
};
use anyhow::Context;
use ktav::value::Value as KtavValue;
use onlyterm_dynamic::FromDynamic;
use onlyterm_term::TerminalSize;
use portable_pty::CommandBuilder;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// Distinguishes "no `.ktav` config exists at this candidate path, but a
/// legacy `.rhai`/`.lua` sibling does" from any other load error (I/O error,
/// a `.ktav` file that exists but fails to parse, etc). `load_with_overrides`
/// downcasts to this type so that a legacy sibling found next to an
/// earlier-searched candidate doesn't prevent it from continuing on to a
/// later candidate that might have a genuine, loadable `.ktav` config (task
/// #298 / bug F9): this case is deferred and only surfaced as a hard error if
/// no valid `.ktav` config is found anywhere in the whole search order.
#[derive(Debug)]
struct LegacyScriptSiblingError {
    script_path: PathBuf,
    expected_path: PathBuf,
}

impl std::fmt::Display for LegacyScriptSiblingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Found a legacy scripted configuration file at {} but \
             scripted configs (rhai/Lua) are no longer supported: \
             the config-scripting engine has been removed from \
             onlyterm's live config-loading path in favor of the \
             static `ktav` format. Please migrate {} to the ktav \
             format and save it as {}. See the migration guide for \
             details.",
            self.script_path.display(),
            self.script_path.display(),
            self.expected_path.display()
        )
    }
}

impl std::error::Error for LegacyScriptSiblingError {}

impl Config {
    pub fn load() -> LoadedConfig {
        Self::load_with_overrides(&onlyterm_dynamic::Value::default())
    }

    /// `ulimit_nofile`/`ulimit_nproc` configure Unix `RLIMIT_NOFILE`/
    /// `RLIMIT_NPROC`, which have no Windows equivalent, so there is
    /// nothing to apply here.
    pub fn update_ulimit(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// The `.ktav` locations we look in, in priority order. Split out of
    /// `load_with_overrides` so that `config_file_path` can answer "which
    /// file *is* (or would be) my config?" using the exact same search
    /// order, rather than a second copy of it that could drift.
    fn config_file_candidates() -> Vec<PathPossibility> {
        // Note that the directories crate has methods for locating project
        // specific config directories, but only returns one of them, not
        // multiple.  In addition, it spawns a lot of subprocesses,
        // so we do this bit "by-hand"

        let mut paths = vec![PathPossibility::optional(HOME_DIR.join(".onlyterm.ktav"))];
        for dir in CONFIG_DIRS.iter() {
            paths.push(PathPossibility::optional(dir.join("onlyterm.ktav")))
        }

        if cfg!(windows) {
            // On Windows, a common use case is to maintain a thumb drive
            // with a set of portable tools that don't need to be installed
            // to run on a target system.  In that scenario, the user would
            // like to run with the config from their thumbdrive because
            // either the target system won't have any config, or will have
            // the config of another user.
            // So we prioritize that here: if there is a config in the same
            // dir as the executable that will take precedence.
            if let Ok(exe_name) = std::env::current_exe() {
                if let Some(exe_dir) = exe_name.parent() {
                    paths.insert(0, PathPossibility::optional(exe_dir.join("onlyterm.ktav")));
                }
            }
        }
        if let Some(path) = std::env::var_os("ONLYTERM_CONFIG_FILE") {
            log::trace!("Note: ONLYTERM_CONFIG_FILE is set in the environment");
            paths.insert(0, PathPossibility::required(path.into()));
        }

        if let Some(path) = CONFIG_FILE_OVERRIDE.lock().unwrap().as_ref() {
            log::trace!("Note: config file override is set");
            paths.insert(0, PathPossibility::required(path.clone()));
        }

        paths
    }

    /// The config file to show the user when they ask to open their
    /// settings: the highest-priority candidate that actually exists, or --
    /// when they have no config at all yet -- the path we recommend they
    /// create, `$HOME/.onlyterm.ktav`.
    ///
    /// Deliberately independent of whether the config *loaded*: a file that
    /// exists but fails to parse is precisely the one the user needs to
    /// open, and at that point `configuration()` is serving built-in
    /// defaults that know nothing about it.
    pub fn config_file_path() -> PathBuf {
        for candidate in Self::config_file_candidates() {
            if candidate.path.exists() {
                return candidate.path;
            }
        }
        HOME_DIR.join(".onlyterm.ktav")
    }

    pub fn load_with_overrides(overrides: &onlyterm_dynamic::Value) -> LoadedConfig {
        let paths = Self::config_file_candidates();

        if let Some(found) = Self::search_paths_for_config(&paths, overrides) {
            return found;
        }

        // We didn't find (or were asked to skip) a onlyterm.ktav file, so
        // update the environment to make it simpler to understand this
        // state.
        std::env::remove_var("ONLYTERM_CONFIG_FILE");
        std::env::remove_var("ONLYTERM_CONFIG_DIR");

        match Self::try_default() {
            Err(err) => LoadedConfig {
                config: Err(err),
                file_name: None,
                warnings: vec![],
            },
            Ok(cfg) => cfg,
        }
    }

    /// Walks `paths` (candidate `.ktav` config locations, in priority order)
    /// and returns `Some(loaded)` as soon as a candidate either loads
    /// successfully or hits a hard error; returns `None` if none of the
    /// candidates exist/apply at all (the caller should then fall back to
    /// `try_default`).
    ///
    /// A legacy `.rhai`/`.lua` sibling found next to one candidate path must
    /// not stop the search: a *later* candidate path may still have a valid,
    /// already-migrated `.ktav` config (task #298 / bug F9), and that should
    /// win outright with no error at all. So a `LegacyScriptSiblingError`
    /// from `try_load` is stashed here instead of being returned
    /// immediately, and only surfaced -- using the first one found, i.e. the
    /// highest-priority candidate that had a legacy sibling -- if no
    /// candidate anywhere in the whole search order produces a real config.
    /// Any other error (a real I/O error, or a `.ktav` file that exists at
    /// this candidate but fails to parse/validate) still bails out
    /// immediately: those indicate an actual problem with a config file the
    /// user is actively using, not just an absent candidate, so hiding them
    /// in favor of a lower-priority candidate would risk silently skipping a
    /// real typo in the user's active config.
    pub(super) fn search_paths_for_config(
        paths: &[PathPossibility],
        overrides: &onlyterm_dynamic::Value,
    ) -> Option<LoadedConfig> {
        let mut deferred_legacy_error: Option<(anyhow::Error, PathBuf)> = None;

        for path_item in paths {
            if CONFIG_SKIP.load(Ordering::Relaxed) {
                break;
            }

            match Self::try_load(path_item, overrides) {
                Err(err) => {
                    if let Some(sibling_err) = err.downcast_ref::<LegacyScriptSiblingError>() {
                        if deferred_legacy_error.is_none() {
                            let expected_path = sibling_err.expected_path.clone();
                            deferred_legacy_error = Some((err, expected_path));
                        }
                        continue;
                    }
                    return Some(LoadedConfig {
                        config: Err(err),
                        file_name: Some(path_item.path.clone()),
                        warnings: vec![],
                    });
                }
                Ok(None) => continue,
                Ok(Some(loaded)) => return Some(loaded),
            }
        }

        // Even though no `.ktav` file exists yet at `expected_path`, we still
        // report it as `file_name`: `ConfigInner::reload` (see
        // `crates/config/src/lib.rs`) builds its filesystem-watch list from
        // `file_name` alone, and watching `expected_path`'s parent directory
        // (which does exist) means that if the user migrates their legacy
        // script to `expected_path` while OnlyTerm is still running (showing
        // this very error), the new file's creation is picked up by the
        // `notify` watcher and triggers a live reload -- instead of leaving
        // the user stuck with this error until they manually restart.
        deferred_legacy_error.map(|(err, expected_path)| LoadedConfig {
            config: Err(err),
            file_name: Some(expected_path),
            warnings: vec![],
        })
    }

    pub fn try_default() -> anyhow::Result<LoadedConfig> {
        let (config, warnings) =
            onlyterm_dynamic::Error::capture_warnings(|| -> anyhow::Result<Config> {
                Ok(default_config_with_overrides_applied()?.compute_extra_defaults(None))
            });

        Ok(LoadedConfig {
            config: Ok(config?),
            file_name: None,
            warnings,
        })
    }

    /// If `p` (a `.ktav` candidate path) doesn't exist, but a legacy
    /// `.rhai`- or `.lua`-suffixed sibling does, produce a clear, actionable
    /// error explaining that scripted configs are no longer supported at
    /// runtime: the rhai/mlua config-scripting engines have been retired
    /// from the live config-loading path (task #275 onward) in favor of the
    /// static `ktav` format, and users must migrate their config and rename
    /// the file.
    ///
    /// This is purely a diagnostic: we never evaluate the `.rhai`/`.lua`
    /// file here, we only check for its existence on disk so that the error
    /// message can point the user at the specific file that needs
    /// migrating instead of a generic "file not found".
    pub(super) fn legacy_script_sibling(p: &Path) -> Option<PathBuf> {
        let file_name = p.file_name()?.to_str()?;
        let stem = if file_name == "onlyterm.ktav" {
            "onlyterm"
        } else if file_name == ".onlyterm.ktav" {
            ".onlyterm"
        } else {
            file_name.strip_suffix(".ktav")?
        };

        for ext in ["rhai", "lua"] {
            let candidate = p.with_file_name(format!("{stem}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    pub(super) fn try_load(
        path_item: &PathPossibility,
        overrides: &onlyterm_dynamic::Value,
    ) -> anyhow::Result<Option<LoadedConfig>> {
        let p = path_item.path.as_path();
        log::trace!("consider config: {}", p.display());
        let mut file = match std::fs::File::open(p) {
            Ok(file) => file,
            Err(err) => match err.kind() {
                std::io::ErrorKind::NotFound if !path_item.is_required => {
                    if let Some(script_path) = Self::legacy_script_sibling(p) {
                        // Note: this is deliberately a distinguishable error
                        // type (`LegacyScriptSiblingError`), not a bare
                        // `anyhow::bail!` string. `load_with_overrides` walks
                        // multiple candidate paths in priority order, and a
                        // legacy `.rhai`/`.lua` sibling found next to an
                        // *earlier* candidate must not prevent a later
                        // candidate's valid `.ktav` config from loading (see
                        // task #298 / bug F9): the caller downcasts this
                        // error to tell "no ktav here, but there's a legacy
                        // script sibling" (keep searching, only report if
                        // nothing better turns up) apart from any other error
                        // (which should still bail out immediately).
                        return Err(LegacyScriptSiblingError {
                            script_path,
                            expected_path: p.to_path_buf(),
                        }
                        .into());
                    }
                    return Ok(None);
                }
                _ => anyhow::bail!("Error opening {}: {}", p.display(), err),
            },
        };

        let mut s = String::new();
        file.read_to_string(&mut s)?;

        // Skip a potential BOM that Windows software may have placed in the
        // file.
        let text = s.trim_start_matches('\u{FEFF}');

        let (config, warnings) =
            onlyterm_dynamic::Error::capture_warnings(|| -> anyhow::Result<Config> {
                let cfg: Config;

                let parsed = ktav::parse(text)
                    .map_err(|e| anyhow::anyhow!("Error parsing {}: {}", p.display(), e))?;

                // `ktav::parse` succeeds as long as `text` is *syntactically*
                // valid ktav, but a config file is additionally expected to
                // be a top-level `Object` (`key: value` pairs). Some classes
                // of typo -- e.g. a missing space after the first `:`, which
                // ktav treats as one bare unquoted string rather than a
                // `key: value` pair, so a whole document of them parses as a
                // top-level `Array` of strings -- are still syntactically
                // valid ktav, just not shaped like a config file. Left
                // unchecked, that non-Object value flows into
                // `Config::from_ktav_dynamic` below and fails with an opaque
                // `NoConversion Array`-style error with no line number and no
                // hint about what's actually wrong. Catch it here instead,
                // mirroring the same check `apply_overrides_to_ktav` already
                // does for the `--config key=value` override path.
                let kind = match &parsed {
                    KtavValue::Object(_) => None,
                    KtavValue::Array(_) => Some("an array"),
                    KtavValue::String(_) => Some("a single string"),
                    KtavValue::Integer(_) => Some("a single integer"),
                    KtavValue::Float(_) => Some("a single float"),
                    KtavValue::Bool(_) => Some("a single boolean"),
                    KtavValue::Null => Some("a single null"),
                };
                if let Some(kind) = kind {
                    anyhow::bail!(
                        "Error parsing {}: the config file must be a top-level ktav \
                         object (`key: value` pairs), but it parsed successfully as \
                         {kind} instead. This usually means a line that was meant to \
                         be a `key: value` pair is missing the space after the `:` \
                         (so ktav reads it as one bare string rather than a key/value \
                         pair), or the document has some other unexpected top-level \
                         structure. Please check the file for a typo like that.",
                        p.display()
                    );
                }

                let config_value = crate::ktav_value::ktav_value_to_dynamic(&parsed);

                let config_value = Config::apply_overrides_to_ktav(config_value)?;
                let config_value = Config::apply_overrides_obj_to(config_value, overrides)?;

                cfg = Config::from_ktav_dynamic(config_value).with_context(|| {
                    format!(
                        "Error converting ktav value parsed from {} to Config struct",
                        p.display()
                    )
                })?;
                cfg.check_consistency()?;

                // Compute but discard the key bindings here so that we raise any
                // problems earlier than we use them.
                let _ = cfg.key_bindings();

                std::env::set_var("ONLYTERM_CONFIG_FILE", p);
                if let Some(dir) = p.parent() {
                    std::env::set_var("ONLYTERM_CONFIG_DIR", dir);
                }
                Ok(cfg)
            });
        let cfg = config?;

        Ok(Some(LoadedConfig {
            config: Ok(cfg.compute_extra_defaults(Some(p))),
            file_name: Some(p.to_path_buf()),
            warnings,
        }))
    }

    /// Convert a `onlyterm_dynamic::Value` (the result of parsing a `.ktav`
    /// config document, see `crate::ktav_value::ktav_value_to_dynamic`) into
    /// a `Config`, in the same "strict: deny unknown fields" mode that the
    /// rhai and (before it) mlua config-builder paths enforced.
    fn from_ktav_dynamic(dyn_value: onlyterm_dynamic::Value) -> anyhow::Result<Config> {
        Config::from_dynamic(
            &dyn_value,
            onlyterm_dynamic::FromDynamicOptions {
                unknown_fields: onlyterm_dynamic::UnknownFieldAction::Deny,
                deprecated_fields: onlyterm_dynamic::UnknownFieldAction::Warn,
            },
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Apply an overrides object (as used by `overridden_config`/palette
    /// previews) directly onto the `onlyterm_dynamic::Value` parsed from the
    /// `.ktav` config document. Ktav is a static data format with no
    /// running engine/callbacks involved, so this is a plain structural
    /// merge (one level deep, matching the previous rhai/mlua behavior).
    pub(crate) fn apply_overrides_obj_to(
        mut config: onlyterm_dynamic::Value,
        overrides: &onlyterm_dynamic::Value,
    ) -> anyhow::Result<onlyterm_dynamic::Value> {
        match overrides {
            onlyterm_dynamic::Value::Object(obj) => {
                if obj.is_empty() {
                    return Ok(config);
                }
                let map = match &mut config {
                    onlyterm_dynamic::Value::Object(map) => map,
                    _ => anyhow::bail!(
                        "expected the config document to be an object, \
                         so that overrides could be applied"
                    ),
                };
                for (key, value) in obj {
                    map.insert(key.clone(), value.clone());
                }
                Ok(config)
            }
            _ => Ok(config),
        }
    }

    /// Apply the `--config key=value` command line overrides on top of the
    /// `onlyterm_dynamic::Value` parsed from the `.ktav` config document.
    ///
    /// Each `value` is itself parsed as a standalone ktav scalar/value
    /// fragment (via `ktav::parse`, wrapped in a single-key document so that
    /// the existing ktav grammar -- which always parses to a top-level
    /// object -- can parse a bare scalar/compound the same way it would
    /// inside a real config file), then spliced into `config[key]`. This
    /// replaces the previous rhai-expression-evaluation behavior
    /// (`apply_overrides_to_rhai`) now that config values are plain,
    /// engine-free data.
    ///
    /// Caveats from wrapping `value` as a single `v: <value>` line rather
    /// than parsing it in the context of a real multi-line document:
    ///
    /// - Inline arrays split elements on top-level commas, not whitespace,
    ///   so `--config 'default_prog=[bash -l]'` does NOT produce the
    ///   2-element array `["bash", "-l"]` -- ktav parses `bash -l` as a
    ///   single bareword/string segment, yielding the 1-element array
    ///   `["bash -l"]`. Use commas instead, e.g.
    ///   `--config 'default_prog=[bash, -l]'`.
    /// - Compound values that span multiple lines (e.g. an object literal
    ///   written across several lines the way it would appear in a config
    ///   file) are not supported through this flag at all, since `value`
    ///   is always wrapped as a single line; such overrides fail to parse.
    ///   Keep `--config` values to a single line (inline `{...}`/`[...]`
    ///   syntax is fine as long as it's all on one line).
    pub(crate) fn apply_overrides_to_ktav(
        mut config: onlyterm_dynamic::Value,
    ) -> anyhow::Result<onlyterm_dynamic::Value> {
        let overrides = CONFIG_OVERRIDES.lock().unwrap();
        for (key, value) in &*overrides {
            if value == "nil" || value == "()" || value == "null" {
                // Literal nil/unit/null as the value is the same as not
                // specifying the value: skip it rather than trying to parse
                // it as a document fragment.
                continue;
            }

            // Wrap `value` as the RHS of a single synthetic key so ktav's
            // "document is always a top-level object" parser can be reused
            // to parse a single bare scalar/compound value.
            let wrapped = format!("v: {value}\n");
            let parsed = ktav::parse(&wrapped).map_err(|e| {
                anyhow::anyhow!("--config {}={}: error parsing value: {}", key, value, e)
            })?;
            let evaluated = match &parsed {
                KtavValue::Object(obj) => obj.get("v").cloned().ok_or_else(|| {
                    anyhow::anyhow!("--config {}={}: internal parse error", key, value)
                })?,
                other => anyhow::bail!(
                    "--config {}={}: internal parse error (unexpected top-level {other:?})",
                    key,
                    value
                ),
            };
            let evaluated = crate::ktav_value::ktav_value_to_dynamic(&evaluated);

            let map = match &mut config {
                onlyterm_dynamic::Value::Object(map) => map,
                _ => anyhow::bail!(
                    "expected the config document to be an object, \
                     so that --config {}={} could be applied",
                    key,
                    value
                ),
            };
            log::debug!("Apply {}={} to config", key, value);
            map.insert(onlyterm_dynamic::Value::String(key.clone()), evaluated);
        }
        Ok(config)
    }

    /// Check for logical conflicts in the config
    pub fn check_consistency(&self) -> anyhow::Result<()> {
        self.check_domain_consistency()?;
        self.check_exec_domains_are_usable()?;
        Ok(())
    }

    /// `exec_domains` entries exist purely to wrap a spawned command --
    /// `ExecDomain::fixup_command` used to name a rhai function
    /// (dispatched via `with_rhai_config_on_main_thread`/
    /// `emit_async_callback`, see `crates/mux/src/domain.rs`) that rewrote
    /// the command before it was spawned, e.g. to route it into `docker
    /// exec`, `ssh`, or some other wrapper. Now that the rhai/Lua scripting
    /// engines have been removed entirely, there is no callback mechanism
    /// left for `fixup_command` to invoke, and ktav (a static data format)
    /// has no expression/function syntax that could express one either.
    ///
    /// Silently ignoring `fixup_command` and spawning the un-wrapped
    /// command directly on the host would be actively wrong/dangerous for
    /// anyone relying on an `ExecDomain` to sandbox or redirect spawns (see
    /// `LocalDomain::fixup_command` in `crates/mux/src/domain.rs`), so
    /// instead of loading quietly, refuse to load at all and explain why:
    /// this mirrors `legacy_script_sibling` above, which does the same
    /// "used to be scriptable, scripting is gone, tell the user clearly"
    /// check for legacy `.rhai`/`.lua` config files.
    fn check_exec_domains_are_usable(&self) -> anyhow::Result<()> {
        if let Some(d) = self.exec_domains.first() {
            anyhow::bail!(
                "exec_domains contains an entry named \"{}\", but ExecDomain \
                 command-wrapping relied on the (now-removed) rhai scripting \
                 engine to implement `fixup_command`, and ktav (a static data \
                 format) has no callback/expression mechanism that could \
                 replace it. As a result, `exec_domains` can no longer wrap, \
                 redirect, or sandbox spawned commands and is no longer \
                 usable: loading it would silently spawn commands unwrapped, \
                 directly on the host, instead of doing whatever the domain \
                 used to do (e.g. `docker exec`, `ssh`, or similar). Please \
                 remove the exec_domains entries from your config; there is \
                 currently no built-in replacement (WSL domain support has \
                 also been removed from this fork).",
                d.name
            );
        }
        Ok(())
    }

    fn check_domain_consistency(&self) -> anyhow::Result<()> {
        let mut domains = HashMap::new();

        let mut check_domain = |name: &str, kind: &str| {
            if let Some(exists) = domains.get(name) {
                anyhow::bail!(
                    "{kind} with name \"{name}\" conflicts with \
                     another existing {exists} with the same name"
                );
            }
            domains.insert(name.to_string(), kind.to_string());
            Ok(())
        };

        for d in &self.unix_domains {
            check_domain(&d.name, "unix domain")?;
        }
        for d in &self.exec_domains {
            check_domain(&d.name, "exec domain")?;
        }
        Ok(())
    }

    pub fn default_config() -> Self {
        Self::default().compute_extra_defaults(None)
    }

    pub fn key_bindings(&self) -> KeyTables {
        let mut tables = KeyTables::default();

        for k in &self.keys {
            let (key, mods) = k
                .key
                .key
                .resolve(self.key_map_preference)
                .normalize_shift(k.key.mods);
            tables.default.insert(
                (key, mods),
                KeyTableEntry {
                    action: k.action.clone(),
                },
            );
        }

        for (name, keys) in &self.key_tables {
            let mut table = KeyTable::default();
            for k in keys {
                let (key, mods) = k
                    .key
                    .key
                    .resolve(self.key_map_preference)
                    .normalize_shift(k.key.mods);
                table.insert(
                    (key, mods),
                    KeyTableEntry {
                        action: k.action.clone(),
                    },
                );
            }
            tables.by_name.insert(name.to_string(), table);
        }

        tables
    }

    pub fn mouse_bindings(
        &self,
    ) -> HashMap<(MouseEventTrigger, MouseEventTriggerMods), KeyAssignment> {
        let mut map = HashMap::new();

        for m in &self.mouse_bindings {
            map.insert((m.event.clone(), m.mods), m.action.clone());
        }

        map
    }

    /// In some cases we need to compute expanded values based
    /// on those provided by the user.  This is where we do that.
    pub fn compute_extra_defaults(&self, config_path: Option<&Path>) -> Self {
        let mut cfg = self.clone();

        // Convert any relative font dirs to their config file relative locations
        if let Some(config_dir) = config_path.as_ref().and_then(|p| p.parent()) {
            for font_dir in &mut cfg.font_dirs {
                if !font_dir.is_absolute() {
                    let dir = config_dir.join(&font_dir);
                    *font_dir = dir;
                }
            }

            if let Some(path) = &self.window_background_image {
                if !path.is_absolute() {
                    cfg.window_background_image.replace(config_dir.join(path));
                }
            }
        }

        // Add some reasonable default font rules
        let reduced = self.font.reduce_first_font_to_family();

        let italic = reduced.make_italic();

        let bold = reduced.make_bold();
        let bold_italic = bold.make_italic();

        let half_bright = reduced.make_half_bright();
        let half_bright_italic = half_bright.make_italic();

        cfg.font_rules.push(StyleRule {
            italic: Some(true),
            intensity: Some(onlyterm_term::Intensity::Half),
            font: half_bright_italic,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(false),
            intensity: Some(onlyterm_term::Intensity::Half),
            font: half_bright,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(false),
            intensity: Some(onlyterm_term::Intensity::Bold),
            font: bold,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(true),
            intensity: Some(onlyterm_term::Intensity::Bold),
            font: bold_italic,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(true),
            intensity: Some(onlyterm_term::Intensity::Normal),
            font: italic,
            ..Default::default()
        });

        // Load any additional color schemes into the color_schemes map
        cfg.load_color_schemes(&cfg.compute_color_scheme_dirs())
            .ok();

        if let Some(scheme) = cfg.color_scheme.as_ref() {
            match cfg.resolve_color_scheme() {
                None => {
                    log::error!(
                        "Your configuration specifies color_scheme=\"{}\" \
                        but that scheme was not found",
                        scheme
                    );
                }
                Some(p) => {
                    cfg.resolved_palette = p.clone();
                }
            }
        }

        if let Some(colors) = &cfg.colors {
            cfg.resolved_palette = cfg.resolved_palette.overlay_with(colors);
        } else if cfg.color_scheme.is_none() {
            // Neither an explicit palette nor a scheme: fall back to this
            // fork's light default. This has to be an `else` branch rather
            // than a default value on the `colors` field -- as a default it
            // was overlaid on top of every resolved scheme too, so setting
            // `color_scheme` had no visible effect at all.
            if let Some(colors) = default_colors() {
                cfg.resolved_palette = cfg.resolved_palette.overlay_with(&colors);
            }
        }

        if let Some(bg) = BackgroundLayer::with_legacy(self) {
            cfg.background.insert(0, bg);
        }

        cfg
    }

    fn compute_color_scheme_dirs(&self) -> Vec<PathBuf> {
        let mut paths = self.color_scheme_dirs.clone();
        for dir in CONFIG_DIRS.iter() {
            paths.push(dir.join("colors"));
        }
        if cfg!(windows) {
            // See commentary re: portable tools above!
            if let Ok(exe_name) = std::env::current_exe() {
                if let Some(exe_dir) = exe_name.parent() {
                    paths.insert(0, exe_dir.join("colors"));
                }
            }
        }
        paths
    }

    fn load_color_schemes(&mut self, paths: &[PathBuf]) -> anyhow::Result<()> {
        fn extract_scheme_name(name: &str) -> Option<&str> {
            if name.ends_with(".toml") {
                let len = name.len();
                Some(&name[..len - 5])
            } else {
                None
            }
        }

        fn load_scheme(path: &Path) -> anyhow::Result<ColorSchemeFile> {
            let s = std::fs::read_to_string(path)?;
            ColorSchemeFile::from_toml_str(&s).context("parsing TOML")
        }

        for colors_dir in paths {
            if let Ok(dir) = std::fs::read_dir(colors_dir) {
                for entry in dir {
                    let Ok(entry) = entry else {
                        continue;
                    };
                    if let Some(name) = entry.file_name().to_str() {
                        if let Some(scheme_name) = extract_scheme_name(name) {
                            if self.color_schemes.contains_key(scheme_name) {
                                // This scheme has already been defined
                                continue;
                            }

                            let path = entry.path();
                            match load_scheme(&path) {
                                Ok(scheme) => {
                                    let name = scheme
                                        .metadata
                                        .name
                                        .unwrap_or_else(|| scheme_name.to_string());
                                    log::trace!(
                                        "Loaded color scheme `{}` from {}",
                                        name,
                                        path.display()
                                    );
                                    self.color_schemes.insert(name, scheme.colors);
                                }
                                Err(err) => {
                                    log::error!(
                                        "Color scheme in `{}` failed to load: {:#}",
                                        path.display(),
                                        err
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn resolve_color_scheme(&self) -> Option<&Palette> {
        let scheme_name = self.color_scheme.as_ref()?;

        if let Some(palette) = self.color_schemes.get(scheme_name) {
            Some(palette)
        } else {
            // Parses just the named scheme rather than all ~1000 bundled
            // ones; see `lookup_default_scheme` for why (task #405).
            crate::lookup_default_scheme(scheme_name)
        }
    }

    pub fn initial_size(&self, dpi: u32, cell_pixel_dims: Option<(usize, usize)>) -> TerminalSize {
        // If we aren't passed the actual values, guess at a plausible
        // default set of pixel dimensions.
        // This is based on "typical" 10 point font at "normal"
        // pixel density.
        // This will get filled in by the gui layer, but there is
        // an edge case where we emit an iTerm image escape in
        // the software update banner through the mux layer before
        // the GUI has had a chance to update the pixel dimensions
        // when running under X11.
        // This is a bit gross.
        let (cell_pixel_width, cell_pixel_height) = cell_pixel_dims.unwrap_or((8, 16));

        TerminalSize {
            rows: self.initial_rows as usize,
            cols: self.initial_cols as usize,
            pixel_width: cell_pixel_width * self.initial_cols as usize,
            pixel_height: cell_pixel_height * self.initial_rows as usize,
            dpi,
        }
    }

    pub fn build_prog(
        &self,
        prog: Option<Vec<&OsStr>>,
        default_prog: Option<&Vec<String>>,
        default_cwd: Option<&PathBuf>,
    ) -> anyhow::Result<CommandBuilder> {
        let mut cmd = match prog {
            Some(args) => {
                let mut args = args.iter();
                let mut cmd = CommandBuilder::new(args.next().expect("executable name"));
                cmd.args(args);
                cmd
            }
            None => {
                if let Some(prog) = default_prog {
                    let mut args = prog.iter();
                    let mut cmd = CommandBuilder::new(args.next().expect("executable name"));
                    cmd.args(args);
                    cmd
                } else {
                    CommandBuilder::new_default_prog()
                }
            }
        };

        self.apply_cmd_defaults(&mut cmd, None, default_cwd);

        Ok(cmd)
    }

    pub fn apply_cmd_defaults(
        &self,
        cmd: &mut CommandBuilder,
        default_prog: Option<&Vec<String>>,
        default_cwd: Option<&PathBuf>,
    ) {
        // Apply `default_cwd` only if `cwd` is not already set, allows `--cwd`
        // option to take precedence
        if let (None, Some(cwd)) = (cmd.get_cwd(), default_cwd) {
            cmd.cwd(cwd);
        }

        if let Some(default_prog) = default_prog {
            if cmd.is_default_prog() {
                cmd.replace_default_prog(default_prog);
            }
        }

        // Has to come after `replace_default_prog` above, since that is what
        // decides which program is actually being launched.
        crate::powershell::ensure_powershell_utf8(cmd);

        // Augment WSLENV so that TERM related environment propagates
        // across the win32/wsl boundary
        let mut wsl_env = std::env::var("WSLENV").ok();

        // If we are running as an appimage, we will have "$APPIMAGE"
        // and "$APPDIR" set in the onlyterm process. These will be
        // propagated to the child processes. Since some apps (including
        // onlyterm) use these variables to detect if they are running in
        // an appimage, those child processes will be misconfigured.
        // Ensure that they are unset.
        // https://docs.appimage.org/packaging-guide/environment-variables.html#id2
        cmd.env_remove("APPIMAGE");
        cmd.env_remove("APPDIR");
        cmd.env_remove("OWD");

        for (k, v) in &self.set_environment_variables {
            if k == "WSLENV" {
                wsl_env.replace(v.clone());
            } else {
                cmd.env(k, v);
            }
        }

        if wsl_env.is_some() || cfg!(windows) || crate::version::running_under_wsl() {
            let mut wsl_env = wsl_env.unwrap_or_default();
            if !wsl_env.is_empty() {
                wsl_env.push(':');
            }
            wsl_env.push_str("TERM:COLORTERM:TERM_PROGRAM:TERM_PROGRAM_VERSION");
            cmd.env("WSLENV", wsl_env);
        }

        cmd.env("TERM", &self.term);
        cmd.env("COLORTERM", "truecolor");
        // TERM_PROGRAM and TERM_PROGRAM_VERSION are an emerging
        // de-facto standard for identifying the terminal.
        cmd.env("TERM_PROGRAM", "OnlyTerm");
        cmd.env("TERM_PROGRAM_VERSION", crate::onlyterm_version());
    }
}

fn rgba(hex: &str) -> RgbaColor {
    <RgbaColor as std::convert::TryFrom<String>>::try_from(hex.to_string())
        .expect("built-in default color literal must be valid")
}

/// OnlyTerm defaults to a light, GitHub-style palette rather than
/// upstream wezterm's unset (effectively dark) palette.
fn default_colors() -> Option<Palette> {
    let tab_bar_color = |bg: &str, fg: &str| TabBarColor {
        bg_color: rgba(bg),
        fg_color: rgba(fg),
        ..Default::default()
    };

    Some(Palette {
        foreground: Some(rgba("#1f2328")),
        background: Some(rgba("#ffffff")),
        cursor_fg: Some(rgba("#ffffff")),
        cursor_bg: Some(rgba("#1f2328")),
        cursor_border: Some(rgba("#1f2328")),
        selection_fg: Some(rgba("#1f2328")),
        selection_bg: Some(rgba("#d0d7de")),
        ansi: Some([
            rgba("#f6f8fa"),
            rgba("#cf222e"),
            rgba("#116329"),
            rgba("#4d2d00"),
            rgba("#0969da"),
            rgba("#8250df"),
            rgba("#1b7c83"),
            rgba("#f6f8fa"),
        ]),
        brights: Some([
            rgba("#24292f"),
            rgba("#a40e26"),
            rgba("#1a7f37"),
            rgba("#633c01"),
            rgba("#0550ae"),
            rgba("#6f42c1"),
            rgba("#3192aa"),
            rgba("#ffffff"),
        ]),
        scrollbar_thumb: Some(rgba("#b6b6b6")),
        tab_bar: Some(TabBarColors {
            background: Some(rgba("#e8edf2")),
            active_tab: Some(tab_bar_color("#ffffff", "#1f2328")),
            inactive_tab: Some(tab_bar_color("#d0d7de", "#57606a")),
            inactive_tab_hover: Some(tab_bar_color("#c8d1da", "#24292f")),
            new_tab: Some(tab_bar_color("#e8edf2", "#57606a")),
            ..Default::default()
        }),
        ..Default::default()
    })
}
