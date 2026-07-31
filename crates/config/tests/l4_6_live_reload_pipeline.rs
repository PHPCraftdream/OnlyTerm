//! L4.6 live check: exercises the *actual* background-thread-reload ->
//! main-thread-pipe handoff (`RHAI_PIPE`/`RhaiConfigCell` in
//! `config/src/lib.rs`), which `l4_6_rhai_event_bridge.rs` does not cover (that
//! file builds a `RhaiConfigState` directly from a `RhaiEventScript` descriptor,
//! bypassing the pipe). This is the part of the pipeline where `mlua::Lua`
//! being `Send` used to matter and rhai's `Engine`/`AST` not being `Send`
//! forced the redesign in the first place, so it's worth its own end-to-end
//! test using the exact same public entry points the real `wezterm-gui`/
//! `wezterm-mux-server` binaries call (`config::designate_this_as_the_main_thread`,
//! `config::common_init`, `config::reload`, `config::with_rhai_config_on_main_thread`).

use std::sync::Mutex;

/// Serializes against the same process-wide config globals as the other L4.6
/// test file (`CONFIG_FILE_OVERRIDE`, `CONFIG_SKIP`, plus here also `CONFIG`'s
/// generation counter and the `RHAI_PIPE`/`RHAI_CONFIG` thread-local, which
/// only usefully exists on whichever thread calls
/// `designate_this_as_the_main_thread` -- must be *this* test thread, and only
/// one test in this binary may claim that role.
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
#[ignore = "task #275: Config::try_load no longer evaluates .rhai files (config loading \
            now goes through ktav; see config::ktav_value/Config::from_ktav_dynamic), so \
            writing a .rhai fixture and loading it via config::reload() can no longer \
            populate font_size or produce a RHAI_PIPE event_script. The pipe/event-bridge \
            mechanism this test exercises is still present and unchanged in isolation -- \
            only its .rhai-based fixture setup is now stale. Task #280 \
            (\"Удалить/переписать rhai-специфичные тесты в crates/config\") owns rewriting \
            or removing this test as part of retiring the rhai event-hook machinery \
            (task #276/#278)."]
fn background_reload_handoff_reaches_main_thread_event_bridge() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // This test thread plays the role of "the main thread" in a real
    // wezterm-gui/wezterm-mux-server process.
    config::designate_this_as_the_main_thread();

    let dir = tempfile::tempdir().unwrap();

    // `on(...)`'s handler here uses only what the real production engine
    // (built by `make_rhai_engine`, with no test-only extras) exposes: it
    // calls the *other* real event-bridge function,
    // `add_to_config_reload_watch_list`, as its observable side effect. This
    // keeps the test entirely within the pipe-delivered `RhaiConfigState` as
    // received (no re-registration of a custom `record` function, which
    // would require re-evaluating the script against a different engine and
    // double-registering the handler -- see `l4_6_rhai_event_bridge.rs`'s
    // `window_config_reloaded_handler_fires_end_to_end` for that fuller,
    // externally-observable-file-write version of this same proof).
    let script = r#"
        on("window-config-reloaded", |window_id, pane_id| {
            add_to_config_reload_watch_list("handler-ran-with-window-" + window_id);
        });

        #{
            font_size: 13.0,
        }
    "#;
    let config_path = dir.path().join("onlyterm.rhai");
    std::fs::write(&config_path, script).unwrap();

    // Exactly what `wezterm-gui`/`wezterm-mux-server`'s `common_init` does:
    // set the override, then perform an initial synchronous `reload()`. The
    // "background thread" framing in the doc comment above refers to
    // `ConfigInner::watch_path`'s filesystem-watcher thread, which also calls
    // `reload()` -- `common_init`'s own call happens on this thread, but
    // reaches the identical `Configuration::reload` -> `Config::load()` ->
    // `RHAI_PIPE.sender.try_send(event_script)` path that a background
    // watcher-thread-triggered reload would.
    config::set_config_file_override(&config_path);
    config::reload();

    let cfg = config::configuration();
    assert_eq!(cfg.font_size, 13.0, "sanity: the .rhai config actually loaded");

    // Drain RHAI_PIPE and act on the resulting RhaiConfigState on "the main
    // thread" (this test thread), exactly like a real event dispatch call
    // site (e.g. `TermWindow::schedule_window_event` in
    // `wezterm-gui/src/termwindow/mod.rs`) would via
    // `config::with_rhai_config_on_main_thread`.
    let (handler_count, default_action, watch_paths) =
        smol::block_on(config::with_rhai_config_on_main_thread(|state| async move {
            let state =
                state.ok_or_else(|| anyhow::anyhow!("no rhai config state available"))?;

            // Proof #1: the on(...) call in the script (evaluated by
            // `RhaiConfigState::from_script`, which `RhaiConfigCell::update_to_latest`
            // called after draining RHAI_PIPE) actually registered a handler.
            let handler_count = state.engine.events.handler_count("window-config-reloaded");

            // Proof #2: emitting the event through the exact function
            // mux/wezterm-gui call sites use actually invokes that handler.
            let args = vec![rhai::Dynamic::from(1_i64), rhai::Dynamic::from(2_i64)];
            let default_action =
                config::rhai_bridge::emit_event(&state, "window-config-reloaded", args).await?;

            // Proof #3: the handler's own side effect (calling
            // `add_to_config_reload_watch_list`, a *different* real
            // production rhai-engine function) took place, and used the
            // argument passed to `emit_event` (`window_id` = 1) to build its
            // string -- confirming args flow through end to end, not just
            // that some handler fired.
            let watch_paths = state.engine.watch_list.paths();

            anyhow::Ok((handler_count, default_action, watch_paths))
        }))
        .expect("with_rhai_config_on_main_thread should succeed");

    // The mere fact that this closure received `Some(state)` at all (the
    // `ok_or_else` above did not fire) proves the background-reload ->
    // RHAI_PIPE -> main-thread handoff worked: this is the specific mechanism
    // that replaced sending an `mlua::Lua` (which was `Send`) across the same
    // channel with sending a `RhaiEventScript` descriptor (plain data)
    // instead, then rebuilding the non-`Send` `rhai::Engine`/`AST` here on
    // the main thread.
    assert_eq!(handler_count, 1, "on(...) must have registered exactly one handler");
    assert_eq!(default_action, true, "no handler returned false");
    assert_eq!(
        watch_paths,
        vec!["handler-ran-with-window-1".to_string()],
        "the handler must have actually executed with the args emit_event passed it"
    );
}
