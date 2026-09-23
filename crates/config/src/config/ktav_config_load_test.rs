use super::config_impl::PathPossibility;
use super::*;
use std::io::Write;

/// `CONFIG_OVERRIDES` is a process-wide `lazy_static` `Mutex`, and Rust test
/// binaries run tests concurrently on multiple threads by default. Any test
/// that reads or writes `CONFIG_OVERRIDES` must serialize against every other
/// such test via this mutex, or a test elsewhere in this module can observe
/// another test's overrides mid-flight (which is exactly what happened before
/// this guard existed: `loads_a_ktav_config_file`, which never touches
/// `CONFIG_OVERRIDES` itself, intermittently failed with `font_size=22.5`
/// leaking in from `applies_config_overrides_to_ktav_config` running
/// concurrently on another thread).
static CONFIG_OVERRIDES_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// End to end proof that the real, public config-loading path (`Config::load`,
/// via `Config::try_load`) parses a `.ktav` config file: this is the
/// keystone task (#275) of the rhai -> ktav config-format migration,
/// replacing the previous rhai-script-evaluation loading path with a
/// direct `ktav::parse` -> `onlyterm_dynamic::Value` ->
/// `Config::from_dynamic` pipeline (see `ktav_value::ktav_value_to_dynamic`
/// and `Config::from_ktav_dynamic`). Exercises a font size override, a
/// color scheme name, and a keybinding, so this proves the new load path
/// actually populates real `Config` fields end-to-end, not just compiles.
#[test]
fn loads_a_ktav_config_file() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`: this test doesn't set any
    // overrides itself, but must still serialize against
    // `applies_config_overrides_to_ktav_config` (which does), since both
    // call through `Config::try_load` -> `apply_overrides_to_ktav`,
    // which reads the same global `CONFIG_OVERRIDES`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("onlyterm.ktav");
    std::fs::write(
        &config_path,
        "\
font_size: 14
term: screen-256color
color_scheme: Builtin Solarized Dark
keys: [
    {
        key: t
        mods: CTRL|SHIFT
        action: ToggleFullScreen
    }
]
",
    )
    .unwrap();

    let path_item = PathPossibility::required(config_path.clone());
    let loaded = Config::try_load(&path_item, &onlyterm_dynamic::Value::default())
        .expect("try_load should succeed")
        .expect("a config was found at the required path");

    let cfg = loaded.config.expect("config should parse");
    assert_eq!(cfg.font_size, 14.0);
    assert_eq!(cfg.term, "screen-256color");
    assert_eq!(cfg.color_scheme.as_deref(), Some("Builtin Solarized Dark"));
    assert_eq!(cfg.keys.len(), 1);
    let key = &cfg.keys[0];
    assert_eq!(key.key.mods, Modifiers::CTRL | Modifiers::SHIFT);
    assert!(matches!(key.action, KeyAssignment::ToggleFullScreen));
    assert_eq!(loaded.file_name.as_deref(), Some(config_path.as_path()));
}

/// `--config key=value`-style overrides (see `CONFIG_OVERRIDES`) are
/// parsed as standalone ktav value fragments and spliced on top of the
/// parsed config, replacing the previous rhai-expression-evaluation
/// behavior now that config values are plain, engine-free data.
#[test]
fn applies_config_overrides_to_ktav_config() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("onlyterm.ktav");
    std::fs::write(&config_path, "font_size: 10\n").unwrap();

    *CONFIG_OVERRIDES.lock().unwrap() = vec![("font_size".to_string(), "22.5".to_string())];

    let path_item = PathPossibility::required(config_path);
    let loaded = Config::try_load(&path_item, &onlyterm_dynamic::Value::default())
        .expect("try_load should succeed")
        .expect("a config was found");
    let cfg = loaded.config.expect("config should parse");
    assert_eq!(cfg.font_size, 22.5);

    // Clean up global state so other tests in this process aren't affected.
    CONFIG_OVERRIDES.lock().unwrap().clear();
}

/// Task #413 (OpenGL removal, config layer): a real `.ktav` config file
/// left over from before the OpenGL renderer was removed may still say
/// `front_end: OpenGL` (the historical default) or `front_end: Software`
/// (the Mesa/SWRAST mode). Neither backend exists anymore, but loading
/// such a config must not fail -- that would turn a previously-working
/// setup into a hard error on upgrade. `FrontEndSelection::from_dynamic`
/// maps both legacy values onto `WebGpu` (see `frontend.rs`); this test
/// exercises that mapping through the full, real
/// `Config::try_load` -> `ktav::parse` -> `Config::from_dynamic` pipeline,
/// not just the unit-level `FromDynamic` impl.
#[test]
fn legacy_front_end_values_migrate_to_web_gpu() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let load = |body: &str| {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.ktav");
        std::fs::write(&config_path, body).unwrap();
        let path_item = PathPossibility::required(config_path);
        Config::try_load(&path_item, &onlyterm_dynamic::Value::default())
            .expect("try_load should succeed")
            .expect("a config was found")
            .config
            .expect("config should parse despite the removed front_end value")
    };

    let cfg = load("front_end: OpenGL\n");
    assert_eq!(cfg.front_end, FrontEndSelection::WebGpu);

    let cfg = load("front_end: Software\n");
    assert_eq!(cfg.front_end, FrontEndSelection::WebGpu);

    // The still-supported value keeps working normally.
    let cfg = load("front_end: WebGpu\n");
    assert_eq!(cfg.front_end, FrontEndSelection::WebGpu);
}

/// Regression test for task #336: `color_scheme` used to have no effect
/// whatsoever. `colors` carried a non-empty default (this fork's light
/// `default_colors` palette), and `compute_extra_defaults` overlays
/// `colors` on top of the resolved scheme -- so the default silently
/// overwrote every scheme the user asked for. The three cases below
/// pin the intended layering.
#[test]
fn color_scheme_is_not_clobbered_by_the_default_palette() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let load = |body: &str| {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.ktav");
        std::fs::write(&config_path, body).unwrap();
        let path_item = PathPossibility::required(config_path);
        Config::try_load(&path_item, &onlyterm_dynamic::Value::default())
            .expect("try_load should succeed")
            .expect("a config was found")
            .config
            .expect("config should parse")
    };
    let rgba = |hex: &str| {
        <crate::color::RgbaColor as std::convert::TryFrom<String>>::try_from(hex.to_string())
            .unwrap()
    };

    // A scheme alone must actually reach the palette. Batman's
    // background is #1b1d1e; before the fix this came back white.
    let cfg = load(
        "color_scheme: Batman
",
    );
    assert_eq!(
        cfg.resolved_palette.background,
        Some(rgba("#1b1d1e")),
        "color_scheme must decide the background when `colors` is unset"
    );

    // An explicit `colors` still wins over the scheme -- that is the
    // documented override direction.
    let cfg = load(
        "color_scheme: Batman
colors: { background: #123456 }
",
    );
    assert_eq!(
        cfg.resolved_palette.background,
        Some(rgba("#123456")),
        "explicit colors must override the scheme"
    );

    // With neither set, the fork's light default still applies.
    let cfg = load(
        "font_size: 12
",
    );
    assert_eq!(
        cfg.resolved_palette.background,
        Some(rgba("#ffffff")),
        "the light default palette must survive when nothing is configured"
    );
}

/// Regression test for task #294: ktav object keys are unconditionally
/// strings (see `crate::ktav_value::ktav_value_to_dynamic`), so a
/// numeric-looking `colors.indexed` key like `136` arrives as
/// `Value::String("136")`, not `Value::I64(136)`. `Palette::indexed` is
/// `HashMap<u8, RgbaColor>`, and before the `map_key_from_dynamic`
/// fallback in `onlyterm_dynamic::fromdynamic` (see
/// `crates/onlyterm-dynamic/src/fromdynamic.rs`), `u8::from_dynamic`
/// rejected a `Value::String` key outright with an opaque
/// `NoConversion { source_type: "String", dest_type: "u8" }`, so any user
/// customizing `colors.indexed` -- documented as usable at
/// `docs/config/appearance.md` -- got a config load failure. This proves
/// the documented syntax loads end-to-end and lands at the expected
/// palette index.
#[test]
fn loads_colors_indexed_with_string_keys() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("onlyterm.ktav");
    std::fs::write(
        &config_path,
        "\
colors: {
    indexed: { 136: #af8700 }
}
",
    )
    .unwrap();

    let path_item = PathPossibility::required(config_path);
    let loaded = Config::try_load(&path_item, &onlyterm_dynamic::Value::default())
        .expect("try_load should succeed")
        .expect("a config was found");
    let cfg = loaded.config.expect("config should parse");

    let palette = cfg.colors.expect("colors table should be present");
    let expected =
        <crate::color::RgbaColor as std::convert::TryFrom<String>>::try_from("#af8700".to_string())
            .unwrap();
    assert_eq!(
        palette.indexed.get(&136u8),
        Some(&expected),
        "colors.indexed.136 should have loaded from the string key \"136\""
    );
}

/// If a legacy `onlyterm.rhai`/`onlyterm.lua` file exists but there is no
/// `.ktav` sibling, `try_load` must not silently ignore it (which would
/// look to the user like "onlyterm forgot my config"); it must fail with
/// an actionable message telling them scripted configs are no longer
/// supported and pointing at the specific file to rename/migrate.
#[test]
fn legacy_rhai_only_config_produces_actionable_error() {
    let dir = tempfile::tempdir().unwrap();
    let rhai_path = dir.path().join("onlyterm.rhai");
    let mut f = std::fs::File::create(&rhai_path).unwrap();
    writeln!(f, "#{{}}").unwrap();
    drop(f);

    let ktav_path = dir.path().join("onlyterm.ktav");
    let path_item = PathPossibility::optional(ktav_path);
    let err = match Config::try_load(&path_item, &onlyterm_dynamic::Value::default()) {
        Err(err) => err,
        Ok(_) => panic!("a legacy .rhai-only directory must error, not silently skip"),
    };
    let message = format!("{err:#}");
    assert!(
        message.contains("no longer supported"),
        "unexpected error message: {}",
        message
    );
    assert!(
        message.contains("onlyterm.ktav"),
        "error should mention the expected new filename: {}",
        message
    );
}

/// Regression test for task #298 / bug F9: a legacy `.rhai`/`.lua`
/// sibling sitting next to an *earlier* candidate in the config search
/// order must not prevent a *later* candidate's valid, already-migrated
/// `.ktav` config from loading. Concretely: a user who migrated from
/// `$HOME/.onlyterm.rhai` to the more advanced `<config-dir>/onlyterm.ktav`
/// location, but left the old `.rhai` file sitting on disk, must still
/// start successfully from the `.ktav` file found later in the search
/// order -- not be blocked by a legacy-script error pointing at the old,
/// irrelevant file. This exercises the actual multi-candidate search loop
/// (`Config::search_paths_for_config`), not just a single `try_load`
/// call, since the bug was specifically in how the search loop reacted
/// to an error from an earlier candidate.
#[test]
fn legacy_sibling_at_earlier_candidate_does_not_block_later_valid_ktav() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    // First (higher-priority) candidate directory: has an old `.rhai`
    // file but no `.ktav` replacement -- simulates a stale legacy file
    // left behind after migrating elsewhere.
    let old_dir = tempfile::tempdir().unwrap();
    std::fs::write(old_dir.path().join(".onlyterm.rhai"), "#{}").unwrap();
    let old_ktav_path = old_dir.path().join(".onlyterm.ktav");

    // Second (lower-priority) candidate directory: has a genuine, valid,
    // already-migrated `.ktav` config.
    let new_dir = tempfile::tempdir().unwrap();
    let new_ktav_path = new_dir.path().join("onlyterm.ktav");
    std::fs::write(&new_ktav_path, "font_size: 18\nterm: screen\n").unwrap();

    let paths = vec![
        PathPossibility::optional(old_ktav_path),
        PathPossibility::optional(new_ktav_path.clone()),
    ];

    let loaded = Config::search_paths_for_config(&paths, &onlyterm_dynamic::Value::default())
        .expect("the later candidate's valid .ktav config should be found");
    let cfg = loaded
            .config
            .expect("a valid .ktav config later in the search order must load successfully, not be blocked by an earlier legacy .rhai sibling");
    assert_eq!(cfg.font_size, 18.0);
    assert_eq!(cfg.term, "screen");
    assert_eq!(loaded.file_name.as_deref(), Some(new_ktav_path.as_path()));
}

/// Companion to the test above: confirms the existing legacy-detection
/// behavior is unchanged for the simple case where NO candidate anywhere
/// in the search order has a valid `.ktav` config -- only a legacy
/// `.rhai`/`.lua` file. This must still produce the same clear,
/// actionable "found a legacy scripted configuration file" error as
/// before, not be silently swallowed now that the search loop defers
/// legacy-sibling errors.
#[test]
fn legacy_sibling_with_no_ktav_anywhere_still_errors() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let old_dir = tempfile::tempdir().unwrap();
    std::fs::write(old_dir.path().join(".onlyterm.rhai"), "#{}").unwrap();
    let old_ktav_path = old_dir.path().join(".onlyterm.ktav");

    // A second candidate directory that has neither a `.ktav` file nor
    // any legacy sibling at all -- just a plain "nothing here".
    let empty_dir = tempfile::tempdir().unwrap();
    let empty_ktav_path = empty_dir.path().join("onlyterm.ktav");

    let paths = vec![
        PathPossibility::optional(old_ktav_path),
        PathPossibility::optional(empty_ktav_path),
    ];

    let loaded = Config::search_paths_for_config(&paths, &onlyterm_dynamic::Value::default())
        .expect("a deferred legacy-sibling error must still be surfaced eventually");
    let err = match loaded.config {
        Err(err) => err,
        Ok(_) => panic!(
            "a legacy .rhai-only search order (no .ktav found anywhere) must still \
                 error, not silently fall through"
        ),
    };
    let message = format!("{err:#}");
    assert!(
        message.contains("no longer supported"),
        "unexpected error message: {}",
        message
    );
    assert!(
        message.contains(".onlyterm.rhai") || message.contains("onlyterm.rhai"),
        "error should mention the specific legacy file to migrate: {}",
        message
    );
}

/// Regression test for task #301 (second-round review of task #298's
/// refactor): when no candidate anywhere in the search order produces a
/// real config and the deferred legacy-sibling error is finally
/// surfaced, `LoadedConfig.file_name` must be `Some(expected_path)` --
/// the not-yet-existing `.ktav` path the user is expected to migrate
/// to -- not `None`. `ConfigInner::reload` (see
/// `crates/config/src/lib.rs`) builds its filesystem-watch list solely
/// from `file_name`; with `None` nothing gets watched in this error
/// state, so migrating the legacy file while OnlyTerm is still running
/// would never be picked up by the live-reload watcher until the user
/// manually restarts. Before task #298's refactor this was
/// `Some(path_item.path.clone())`; the refactor accidentally dropped it
/// to `None` on the deferred-error path specifically.
#[test]
fn deferred_legacy_error_still_reports_expected_ktav_path_as_file_name() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let old_dir = tempfile::tempdir().unwrap();
    std::fs::write(old_dir.path().join(".onlyterm.rhai"), "#{}").unwrap();
    let old_ktav_path = old_dir.path().join(".onlyterm.ktav");

    // A second candidate directory that has neither a `.ktav` file nor
    // any legacy sibling at all -- just a plain "nothing here" -- so no
    // candidate in the whole search order produces a real config.
    let empty_dir = tempfile::tempdir().unwrap();
    let empty_ktav_path = empty_dir.path().join("onlyterm.ktav");

    let paths = vec![
        PathPossibility::optional(old_ktav_path.clone()),
        PathPossibility::optional(empty_ktav_path),
    ];

    let loaded = Config::search_paths_for_config(&paths, &onlyterm_dynamic::Value::default())
        .expect("a deferred legacy-sibling error must still be surfaced eventually");
    assert!(
        loaded.config.is_err(),
        "no valid .ktav config exists anywhere, so this must still be an error"
    );
    assert_eq!(
        loaded.file_name.as_deref(),
        Some(old_ktav_path.as_path()),
        "file_name must be the expected (not-yet-existing) .ktav path, so that its \
             parent directory -- which does exist -- ends up on the config-reload \
             watch list; otherwise migrating the legacy file while OnlyTerm is still \
             running is never picked up by the live-reload watcher"
    );
    // The expected path's parent directory is `old_dir`, which does
    // exist on disk: this is exactly the directory that
    // `ConfigInner::reload` in `crates/config/src/lib.rs` would add to
    // its `notify` watch list from `file_name.parent()`, since
    // `file_name` is now `Some(old_ktav_path)` rather than `None`.
    assert!(
        loaded.file_name.as_ref().unwrap().parent() == Some(old_dir.path()),
        "the watched parent directory must be the real, existing directory the user \
             would migrate their config into"
    );
}

/// Regression test for task #295: `ExecDomain::fixup_command` used to
/// name a rhai function that rewrote a spawned command (e.g. to route
/// it into `docker exec`, `ssh`, etc. -- see
/// `LocalDomain::fixup_command` in `crates/mux/src/domain.rs`). With
/// the rhai/Lua scripting engines removed there is no callback
/// mechanism left to run, and a mechanical removal of the rhai
/// callsite left the rest of the spawn path proceeding with the
/// command completely unwrapped -- silently spawning on the host
/// instead of erroring. A non-empty `exec_domains` must now fail to
/// load, loudly, rather than let that silent behavior change through.
#[test]
fn exec_domains_are_rejected_at_load_time() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("onlyterm.ktav");
    std::fs::write(
        &config_path,
        "\
exec_domains: [
    {
        name: mydomain
        fixup_command: wrap_in_docker
    }
]
",
    )
    .unwrap();

    let path_item = PathPossibility::required(config_path);
    let err = match Config::try_load(&path_item, &onlyterm_dynamic::Value::default()) {
        Err(err) => err,
        Ok(_) => panic!(
            "a config with a non-empty exec_domains must fail to load, \
                 not silently spawn commands unwrapped"
        ),
    };
    let message = format!("{err:#}");
    assert!(
        message.contains("rhai scripting engine")
            && message.contains("fixup_command")
            && message.contains("no longer usable"),
        "error should clearly explain that exec_domains can no longer \
             wrap commands now that the rhai scripting engine is gone: {}",
        message
    );
}

/// The common case -- no `exec_domains` at all -- must be completely
/// unaffected by the task #295 check: no new error, no behavior change.
#[test]
fn config_with_no_exec_domains_is_unaffected() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("onlyterm.ktav");
    std::fs::write(&config_path, "font_size: 12\n").unwrap();

    let path_item = PathPossibility::required(config_path);
    let loaded = Config::try_load(&path_item, &onlyterm_dynamic::Value::default())
        .expect("try_load should succeed")
        .expect("a config was found");
    let cfg = loaded
        .config
        .expect("config with no exec_domains should load fine");
    assert!(cfg.exec_domains.is_empty());
    assert_eq!(cfg.font_size, 12.0);
}

/// Regression test for task #296: a config file that parses
/// successfully as *syntactically valid ktav* but not as a top-level
/// `Object` -- e.g. because a `key: value` line is missing the space
/// after the `:`, so ktav reads the whole line as one bare unquoted
/// string rather than a `key`/`value` pair, and the whole document
/// parses as a top-level `Array` of strings -- must fail with a clear
/// error naming the actual parsed shape and hinting at the likely
/// cause, not the old opaque `NoConversion Array`-style error from deep
/// inside `onlyterm_dynamic`'s `Config::from_dynamic` conversion.
#[test]
fn malformed_top_level_array_config_produces_actionable_error() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("onlyterm.ktav");
    // Missing space after the first `:` on each line: ktav parses each
    // line as one bare string rather than a `key: value` pair, so the
    // whole document parses as a top-level `Array`, not an `Object`.
    std::fs::write(&config_path, "font_size:14\nterm: screen\n").unwrap();

    let path_item = PathPossibility::required(config_path);
    let err = match Config::try_load(&path_item, &onlyterm_dynamic::Value::default()) {
        Err(err) => err,
        Ok(_) => panic!(
            "a config document that parses as a top-level array, not an \
                 object, must fail to load, not be silently accepted"
        ),
    };
    let message = format!("{err:#}");
    assert!(
        message.contains("top-level ktav object")
            && message.contains("an array")
            && message.contains("missing the space after the `:`"),
        "error should clearly explain the config parsed as a non-Object \
             shape and hint at the likely typo, not just show the old opaque \
             `NoConversion Array` error: {}",
        message
    );
    assert!(
        !message.contains("NoConversion"),
        "the new, clearer error should be raised before ever reaching \
             the opaque onlyterm_dynamic NoConversion error: {}",
        message
    );
}

/// The common case -- a normal, correctly-formed ktav config file that
/// parses as a top-level object -- must be completely unaffected by the
/// task #296 shape check: no new error, no false-positive rejection.
#[test]
fn normal_config_is_unaffected_by_top_level_shape_check() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("onlyterm.ktav");
    std::fs::write(&config_path, "font_size: 14\nterm: screen\n").unwrap();

    let path_item = PathPossibility::required(config_path);
    let loaded = Config::try_load(&path_item, &onlyterm_dynamic::Value::default())
        .expect("try_load should succeed")
        .expect("a config was found");
    let cfg = loaded
        .config
        .expect("a normal top-level-object ktav config should load fine");
    assert_eq!(cfg.font_size, 14.0);
    assert_eq!(cfg.term, "screen");
}

/// Sanity check for the pure path-diagnostics helper itself: only exact
/// `<stem>.ktav` -> `<stem>.rhai`/`<stem>.lua` siblings are detected, and
/// only when the legacy file actually exists on disk (we must never
/// claim a legacy file exists when it doesn't, else every fresh/no-config
/// user would see the migration error instead of falling through to
/// defaults).
#[test]
fn legacy_script_sibling_detection() {
    let dir = tempfile::tempdir().unwrap();
    let ktav_path = dir.path().join("onlyterm.ktav");
    assert_eq!(Config::legacy_script_sibling(&ktav_path), None);

    std::fs::write(dir.path().join("onlyterm.rhai"), "#{}").unwrap();
    assert_eq!(
        Config::legacy_script_sibling(&ktav_path),
        Some(dir.path().join("onlyterm.rhai"))
    );

    let dot_ktav_path = dir.path().join(".onlyterm.ktav");
    assert_eq!(Config::legacy_script_sibling(&dot_ktav_path), None);
    std::fs::write(dir.path().join(".onlyterm.lua"), "return {}").unwrap();
    assert_eq!(
        Config::legacy_script_sibling(&dot_ktav_path),
        Some(dir.path().join(".onlyterm.lua"))
    );
}

/// Task #420 (OS-portable config paths). A relative `font_dirs` entry
/// must resolve to a path under the directory that holds the config
/// file itself, using whatever `PathBuf`/`Path::join` produces on the
/// host OS -- there is no hardcoded separator or platform assumption in
/// `compute_extra_defaults`. This is what makes a relative `font_dirs`
/// entry (e.g. `font_dirs: [fonts]`) safe to sync between Windows and
/// Linux/macOS: the join happens the same way regardless of platform,
/// unlike a hardcoded absolute path such as `C:/Windows/Fonts`, which
/// only ever makes sense on one OS (see `docs/config/reference/config/
/// font_dirs.md`).
#[test]
fn relative_font_dirs_resolve_against_the_config_file_directory() {
    // See `CONFIG_OVERRIDES_TEST_LOCK`.
    let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("onlyterm.ktav");
    std::fs::write(&config_path, "font_dirs: [fonts, ../shared-fonts]\n").unwrap();

    let path_item = PathPossibility::required(config_path);
    let loaded = Config::try_load(&path_item, &onlyterm_dynamic::Value::default())
        .expect("try_load should succeed")
        .expect("a config was found");
    let cfg = loaded.config.expect("config should parse");

    assert_eq!(
        cfg.font_dirs,
        vec![dir.path().join("fonts"), dir.path().join("../shared-fonts"),],
        "relative font_dirs entries must resolve relative to the config \
             file's own directory on every OS, with no hardcoded platform path"
    );
    for font_dir in &cfg.font_dirs {
        assert!(
            font_dir.is_absolute(),
            "resolved font_dirs entries must be absolute: {:?}",
            font_dir
        );
    }
}
