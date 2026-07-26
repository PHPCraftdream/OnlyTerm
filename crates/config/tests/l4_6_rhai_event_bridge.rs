//! L4.6: end-to-end proof that the `wezterm.on`/`emit` runtime event-callback
//! bridge works entirely through rhai now, with no companion `mlua::Lua` context
//! anywhere in the path.
//!
//! This exercises the *real* production pipeline pieces, not a standalone
//! `rhai::Engine` built directly by the test:
//!
//! 1. `Config::try_load` (the same function `Configuration::reload` calls, see
//!    `config/src/lib.rs`) parses a `.rhai` config file and returns a
//!    `LoadedConfig` whose `event_script` field is a `RhaiEventScript` -- the
//!    `Send`-safe descriptor (script source + path) that crosses the
//!    background-reload-thread -> main-thread channel in the real binaries
//!    (`RHAI_PIPE`/`RhaiConfigCell` in `config/src/lib.rs`).
//! 2. `RhaiConfigState::from_script` rebuilds a live engine from that
//!    descriptor and re-evaluates the script -- exactly what
//!    `RhaiConfigCell::update_to_latest` does on the main thread in the real
//!    reload pipeline. Any top-level `on(...)` call in the script registers a
//!    handler as a side effect of this evaluation.
//! 3. `config::rhai_bridge::emit_event`/`emit_sync_callback` (the direct
//!    replacements for `config::lua::emit_event`/`emit_sync_callback`, used by
//!    the ~10 mux/wezterm-gui call sites this task ported) invoke the
//!    registered handler(s).
//!
//! The side effect used to observe that the handler actually ran is a write to
//! a temp file from within the rhai script's handler closure, matching the
//! task's "verify via a side effect like a file/log write" instruction. The
//! handler's arguments stand in for the real `GuiWin`/`MuxPane` objects that
//! `wezterm-gui`/`mux` pass in production (see
//! `wezterm-gui/src/scripting/guiwin.rs::register_rhai` /
//! `lua-api-crates/mux/src/pane.rs::register_rhai`); `config` itself cannot
//! depend on those crates (they depend on `config`, not the reverse), so this
//! test uses plain rhai integers instead, which is representative of the
//! bridge mechanics being tested (registration, dispatch, arg-passing,
//! return-value contract) even though the real objects are richer.

use config::rhai_engine::RhaiConfigState;
use config::Config;

/// Serializes tests in this file against `config::set_config_file_override`,
/// a process-wide `Mutex<Option<PathBuf>>` (see `CONFIG_FILE_OVERRIDE` in
/// `config/src/lib.rs`) that `Config::load()` reads. `Config::try_load` (the
/// function that actually parses a `.rhai` file and is exercised by
/// `config/src/config.rs`'s own inline `rhai_config_load_test` module) is
/// crate-private, so this external integration test goes through the public
/// `Config::load()` entry point instead, pointed at a temp file via the
/// override -- the same mechanism `common_init`/`--config-file` use in the
/// real binaries. Since the override is global mutable state, and Rust test
/// binaries run `#[test]` functions concurrently by default, every test here
/// must hold this lock for its whole body.
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn write_rhai_config(dir: &std::path::Path, script: &str) -> std::path::PathBuf {
    let config_path = dir.join("wezterm.rhai");
    std::fs::write(&config_path, script).unwrap();
    config_path
}

/// Point `Config::load()` at `path` and load. `set_config_file_override` has
/// no public "clear" counterpart, but that's fine here: this file is its own
/// separate test binary (Rust's `tests/*.rs` integration-test convention, one
/// process per file), `TEST_LOCK` serializes every test within it, and each
/// test that needs a specific config file calls this again with its own path
/// immediately before using it -- nothing in this file ever depends on the
/// override being unset.
fn load_rhai_config(path: &std::path::Path) -> config::LoadedConfig {
    config::set_config_file_override(path);
    Config::load()
}

/// The core scenario from the task: a `.wezterm.rhai` config registers an
/// `on("window-config-reloaded", ...)` handler (the same event name
/// `TermWindow::schedule_window_event`/`emit_window_event` in
/// `wezterm-gui/src/termwindow/mod.rs` actually emits in the live GUI) that
/// writes a marker file, and emitting that event from the Rust side (mirroring
/// what `config::rhai_bridge::emit_event` callers do) really invokes it.
#[test]
fn window_config_reloaded_handler_fires_end_to_end() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let dir = tempfile::tempdir().unwrap();
    let marker_path = dir.path().join("handler-ran.marker");
    // rhai string literals don't support Windows path separators directly;
    // pass the marker path in via a global instead of interpolating it into
    // the script text.
    let marker_path_str = marker_path.to_string_lossy().into_owned();

    let script = r#"
        on("window-config-reloaded", |window_id, pane_id| {
            record(window_id, pane_id);
        });

        #{
            font_size: 12.0,
        }
    "#;
    let config_path = write_rhai_config(dir.path(), script);

    // Step 1: the real production parse path.
    let loaded = load_rhai_config(&config_path);
    assert!(
        loaded.config.is_ok(),
        "config should have parsed: {:?}",
        loaded.config.as_ref().err()
    );

    let event_script = loaded
        .event_script
        .expect("L4.6: LoadedConfig must carry an event_script descriptor");
    assert_eq!(
        event_script.source.as_deref().map(str::trim),
        Some(script.trim()),
        "RhaiEventScript must carry the script's actual source text, not a blank stand-in \
         (this is the specific L4.6 fix: the pre-existing companion mlua context never \
         executed the user's script at all)"
    );

    // Step 2: rebuild the live engine from the descriptor, exactly like
    // `RhaiConfigCell::update_to_latest` does on the main thread -- except we
    // need the `record` function available *before* the script's `on(...)`
    // call resolves it, and `RhaiConfigState::from_script` already evaluates
    // the script once as part of rebuilding (mirroring production, where the
    // background thread's `Config::try_load` has already evaluated it once to
    // derive the `Config` value, and the main thread evaluates it again to
    // populate a fresh `EventRegistry`). So rather than calling
    // `RhaiConfigState::from_script` (which would register the `on(...)`
    // handler once against an engine that doesn't have `record` yet, then a
    // second time once we registered `record` and re-evaluated -- double
    // registration, not what production does), build the engine directly via
    // `make_rhai_engine` + register `record` + compile-and-eval exactly once,
    // matching how `make_rhai_engine` folds in all `RHAI_SETUP_FUNCS`
    // registrations *before* the config script itself is ever compiled in
    // production (`Config::try_load`/`RhaiConfigState::from_script`).
    let mut engine = config::rhai_engine::make_rhai_engine(&event_script.config_file)
        .expect("engine should build");
    engine.engine.register_fn(
        "record",
        move |window_id: rhai::INT, pane_id: rhai::INT| {
            std::fs::write(&marker_path_str, format!("window={window_id} pane={pane_id}"))
                .expect("write marker file");
        },
    );
    let (ast, _) = engine
        .compile_and_eval(script)
        .expect("script should evaluate with record() available");
    let state = RhaiConfigState { engine, ast };

    assert_eq!(
        state.engine.events.handler_count("window-config-reloaded"),
        1,
        "on(\"window-config-reloaded\", ...) must have registered exactly one handler"
    );

    // Step 3: the same call mux/wezterm-gui call sites make.
    let args = vec![rhai::Dynamic::from(1_i64), rhai::Dynamic::from(2_i64)];
    let default_action = smol::block_on(config::rhai_bridge::emit_event(
        &state,
        "window-config-reloaded",
        args,
    ))
    .expect("emit_event should succeed");
    assert_eq!(
        default_action, true,
        "no handler returned false, so default action should proceed"
    );

    let marker_contents = std::fs::read_to_string(&marker_path)
        .expect("handler should have written the marker file, proving it actually ran");
    assert_eq!(marker_contents, "window=1 pane=2");
}

/// Smoke test for `emit_sync_callback` (the replacement for the "first handler
/// wins" mlua `emit_sync_callback`, used by e.g. `mux-is-process-stateful`/
/// `format-tab-title` call sites) via the same real `try_load` ->
/// `RhaiConfigState::from_script` path.
#[test]
fn emit_sync_callback_returns_first_handler_result_end_to_end() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let dir = tempfile::tempdir().unwrap();
    let script = r#"
        on("mux-is-process-stateful", |proc_info| {
            proc_info.name == "keep-me-alive"
        });

        #{}
    "#;
    let config_path = write_rhai_config(dir.path(), script);

    let loaded = load_rhai_config(&config_path);
    assert!(loaded.config.is_ok());
    let event_script = loaded.event_script.unwrap();
    let state = RhaiConfigState::from_script(&event_script).unwrap();

    let mut proc_info = rhai::Map::new();
    proc_info.insert("name".into(), "keep-me-alive".into());
    let arg = rhai::Dynamic::from_map(proc_info);

    let result =
        config::rhai_bridge::emit_sync_callback(&state, "mux-is-process-stateful", vec![arg])
            .expect("emit_sync_callback should succeed");
    assert_eq!(result.as_bool(), Ok(true));

    // Different process name: handler returns false.
    let mut proc_info2 = rhai::Map::new();
    proc_info2.insert("name".into(), "something-else".into());
    let arg2 = rhai::Dynamic::from_map(proc_info2);
    let result2 =
        config::rhai_bridge::emit_sync_callback(&state, "mux-is-process-stateful", vec![arg2])
            .expect("emit_sync_callback should succeed");
    assert_eq!(result2.as_bool(), Ok(false));
}

/// No handler registered at all: `emit_event` must return `true` (default
/// action proceeds) and must not error, mirroring the Lua path's behavior for
/// an event nobody subscribed to.
#[test]
fn emit_event_with_no_handler_returns_true() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let dir = tempfile::tempdir().unwrap();
    let config_path = write_rhai_config(dir.path(), "#{}");

    let loaded = load_rhai_config(&config_path);
    assert!(loaded.config.is_ok());
    let event_script = loaded.event_script.unwrap();
    let state = RhaiConfigState::from_script(&event_script).unwrap();

    let default_action = smol::block_on(config::rhai_bridge::emit_event(
        &state,
        "never-registered-by-anyone",
        vec![],
    ))
    .expect("emit_event should succeed even with no handlers");
    assert_eq!(default_action, true);
}

/// `Config::try_default()` (no config file at all) still hands back an
/// `event_script`, so `with_rhai_config_on_main_thread`/
/// `run_immediate_with_rhai_config` callers always have a `RhaiConfigState` to
/// build, matching the pre-L4.6 companion mlua context's "always exists, even
/// with no config file" behavior.
#[test]
fn try_default_provides_a_handler_less_event_script() {
    let loaded = Config::try_default().expect("try_default should succeed");
    let event_script = loaded
        .event_script
        .expect("try_default must still provide an event_script descriptor");
    assert!(event_script.source.is_none());

    let state = RhaiConfigState::from_script(&event_script)
        .expect("a handler-less RhaiConfigState must still build successfully");
    assert_eq!(state.engine.events.event_count(), 0);

    let default_action =
        smol::block_on(config::rhai_bridge::emit_event(&state, "anything", vec![])).unwrap();
    assert_eq!(default_action, true);
}
