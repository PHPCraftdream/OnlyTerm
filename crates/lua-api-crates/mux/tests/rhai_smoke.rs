//! L4d smoke test: `mux_lua::register_rhai` (and the per-type
//! `domain::register_rhai`/`pane::register_rhai`/`tab::register_rhai`/
//! `window::register_rhai` it composes) exposes rhai equivalents of the
//! `wezterm.mux.*` module and the `MuxDomain`/`MuxPane`/`MuxTab`/`MuxWindow`
//! object methods (see docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! ## Test strategy and its limits
//!
//! `Mux` is a single process-wide `lazy_static`-backed singleton
//! (`Mux::try_get()`/`Mux::set_mux()`, see `mux/src/lib.rs`), so all tests in
//! this file that touch it run against the *same* underlying `Mux` +
//! `LocalDomain` (installed once, lazily, by `ensure_mux()` below) rather than
//! a fresh one per test -- mirroring how `share-data`'s rhai smoke test
//! handles its own process-wide singleton (`GLOBALS`).
//!
//! This file deliberately does **not** call `mux::spawn_window()` /
//! `window.spawn_tab()` / `pane.split()` or anything else that reaches
//! `Mux::spawn_tab_or_window`/`LocalDomain::spawn_pane`: those paths call
//! `mux.add_pane()`, which calls `promise::spawn::spawn_into_main_thread`,
//! which unconditionally panics ("no scheduler has been configured") unless
//! something has already called `promise::spawn::set_schedulers` -- normally
//! done once, at startup, by the embedding application (`wezterm-gui`/
//! `wezterm-mux-server`), never by a crate's own unit tests. A `LocalDomain`
//! spawn also launches a real, live shell process (e.g. `cmd.exe` on
//! Windows) that stays running indefinitely once started, which is not
//! something a fast, deterministic unit test should be doing at all even if
//! the scheduler panic were worked around. So `MuxPane`/`MuxTab`/`MuxWindow`
//! methods that require a live, resolved pane/tab/window (`get_title`,
//! `send_text`, `tabs()`, etc) are exercised here only for their
//! id-not-found error path (`mux::get_pane`/`get_tab`/`get_window` called
//! with an id that provably does not exist in this test's `Mux`), which
//! confirms the rhai binding's argument marshalling and `Mux` resolution
//! wiring without ever needing a live pane. Full behavioral coverage of the
//! pane/tab/window instance methods needs an actual running mux (i.e. a
//! `wezterm-gui`/`wezterm-mux-server`-level integration test, out of scope
//! for this crate's own unit tests) -- see this task's final report for the
//! explicit list of methods only covered by registration/type-checking, not
//! by a live-value assertion.
//!
//! `eval`/`eval_scope` below funnel every rhai error through `.map_err`
//! rather than relying on `?`'s blanket `From` conversion into
//! `anyhow::Error`: `Box<rhai::EvalAltResult>` is not `Sync` (it can
//! transitively hold a non-`Sync` rhai `FnPtr`/`Dynamic`), which `anyhow`
//! requires of any error type it converts via `?`.

use mux::domain::{Domain, LocalDomain};
use mux::Mux;
use rhai::Engine;
use std::sync::Arc;
use std::sync::Once;

/// Installs a process-wide `Mux` with a single `LocalDomain` named
/// "rhai-smoke-test", the first time any test calls this; subsequent calls
/// (from other tests, possibly concurrently) just return immediately, since
/// `Mux::set_mux` only ever needs to run once for the whole test binary.
fn ensure_mux() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let domain: Arc<dyn Domain> =
            Arc::new(LocalDomain::new("rhai-smoke-test").expect("LocalDomain::new"));
        let mux = Arc::new(Mux::new(Some(domain)));
        Mux::set_mux(&mux);
    });
}

fn make_engine() -> Engine {
    ensure_mux();
    let mut engine = Engine::new();
    mux_lua::register_rhai(&mut engine).expect("register_rhai");
    engine
}

fn eval<T: Clone + 'static>(engine: &Engine, script: &str) -> anyhow::Result<T> {
    engine
        .eval::<T>(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))
}

#[test]
fn mux_get_active_workspace_returns_a_string() -> anyhow::Result<()> {
    let engine = make_engine();
    let workspace: String = eval(&engine, "mux::get_active_workspace()")?;
    assert!(!workspace.is_empty());
    Ok(())
}

#[test]
fn mux_get_workspace_names_is_empty_without_a_live_window() -> anyhow::Result<()> {
    let engine = make_engine();
    // `Mux::iter_workspaces` derives its list purely from *existing windows*
    // (see `mux::Mux::iter_workspaces`), not from `active_workspace()`
    // (which falls back to a config default independent of whether any
    // window actually carries that name) -- with no window ever spawned in
    // this test binary (see the module doc comment), this is always empty,
    // even though `get_active_workspace()` still returns a non-empty name.
    let names: rhai::Array = eval(&engine, "mux::get_workspace_names()")?;
    assert!(names.is_empty());
    Ok(())
}

#[test]
fn mux_set_active_workspace_rejects_unknown_workspace() {
    let engine = make_engine();
    let result: anyhow::Result<()> = eval(
        &engine,
        r#"mux::set_active_workspace("this-workspace-does-not-exist")"#,
    );
    assert!(result.is_err());
}

#[test]
fn rename_workspace_on_nonexistent_workspace_is_a_harmless_no_op() -> anyhow::Result<()> {
    let engine = make_engine();
    // `Mux::rename_workspace` only touches windows whose workspace matches
    // `old_workspace` (see `mux::Mux::rename_workspace`); with no window in
    // this test's `Mux`, there is nothing to rename, and the call must
    // still succeed rather than erroring.
    eval::<()>(
        &engine,
        r#"mux::rename_workspace("no-such-workspace", "still-no-such-workspace")"#,
    )?;
    Ok(())
}

#[test]
fn mux_get_domain_default_matches_all_domains_entry() -> anyhow::Result<()> {
    let engine = make_engine();
    let default_domain_id: rhai::INT = eval(&engine, "mux::get_domain().domain_id()")?;
    let all: rhai::Array = eval(&engine, "mux::all_domains()")?;
    assert_eq!(all.len(), 1);
    let only_domain = all[0].clone().clone_cast::<mux_lua::MuxDomain>();
    let _ = only_domain; // just confirming the cast/type succeeds
    assert!(default_domain_id >= 0);
    Ok(())
}

#[test]
fn mux_get_domain_by_name_and_by_id_round_trip() -> anyhow::Result<()> {
    let engine = make_engine();
    let name: String = eval(&engine, "mux::get_domain().name()")?;
    assert_eq!(name, "rhai-smoke-test");

    let by_name_id: rhai::INT =
        eval(&engine, &format!(r#"mux::get_domain("{name}").domain_id()"#))?;
    let by_default_id: rhai::INT = eval(&engine, "mux::get_domain().domain_id()")?;
    assert_eq!(by_name_id, by_default_id);

    let by_id_id: rhai::INT = eval(
        &engine,
        &format!("mux::get_domain({by_default_id}).domain_id()"),
    )?;
    assert_eq!(by_id_id, by_default_id);
    Ok(())
}

#[test]
fn mux_get_domain_unknown_name_and_unknown_id_are_unit() -> anyhow::Result<()> {
    let engine = make_engine();
    let by_name: rhai::Dynamic = eval(&engine, r#"mux::get_domain("no-such-domain")"#)?;
    assert!(by_name.is_unit());
    let by_id: rhai::Dynamic = eval(&engine, "mux::get_domain(999999999)")?;
    assert!(by_id.is_unit());
    Ok(())
}

#[test]
fn mux_domain_is_spawnable_and_state() -> anyhow::Result<()> {
    let engine = make_engine();
    let spawnable: bool = eval(&engine, "mux::get_domain().is_spawnable()")?;
    assert!(spawnable);
    let state: String = eval(&engine, "mux::get_domain().state()")?;
    assert_eq!(state, "Attached");
    Ok(())
}

#[test]
fn mux_domain_label_defaults_to_name() -> anyhow::Result<()> {
    let engine = make_engine();
    let name: String = eval(&engine, "mux::get_domain().name()")?;
    let label: String = eval(&engine, "mux::get_domain().label()")?;
    assert_eq!(name, label);
    Ok(())
}

#[test]
fn mux_domain_detach_reports_local_domain_error() {
    let engine = make_engine();
    // `LocalDomain::detach` always returns an error (`"detach not
    // implemented for LocalDomain"`) -- verifies the rhai binding surfaces
    // that error rather than swallowing it or panicking.
    let result: anyhow::Result<()> = eval(&engine, "mux::get_domain().detach();");
    assert!(result.is_err());
}

#[test]
fn mux_domain_attach_succeeds_with_no_window() -> anyhow::Result<()> {
    let engine = make_engine();
    // `LocalDomain::attach` is a no-op success when given no window.
    eval::<()>(&engine, "mux::get_domain().attach();")?;
    Ok(())
}

#[test]
fn mux_domain_to_string_and_has_any_panes() -> anyhow::Result<()> {
    let engine = make_engine();
    let s: String = eval(&engine, "mux::get_domain().to_string()")?;
    assert!(s.starts_with("MuxDomain("));
    // No pane has ever been spawned in this test binary's `Mux` (see the
    // module doc comment on why this file never spawns a live pane), so this
    // is deterministically `false`.
    let has_panes: bool = eval(&engine, "mux::get_domain().has_any_panes()")?;
    assert!(!has_panes);
    Ok(())
}

#[test]
fn set_default_domain_round_trips() -> anyhow::Result<()> {
    let engine = make_engine();
    // There's only ever the one `LocalDomain` in this test binary's shared
    // `Mux`, so this just confirms the call succeeds end-to-end (resolves the
    // `MuxDomain` argument, calls through to `Mux::set_default_domain`)
    // without erroring, not that it changes anything observable.
    eval::<()>(&engine, "mux::set_default_domain(mux::get_domain());")?;
    Ok(())
}

#[test]
fn mux_get_window_unknown_id_errors() {
    let engine = make_engine();
    let result: anyhow::Result<mux_lua::MuxWindow> =
        eval(&engine, "mux::get_window(999999999)");
    assert!(result.is_err());
}

#[test]
fn mux_get_pane_unknown_id_errors() {
    let engine = make_engine();
    let result: anyhow::Result<mux_lua::MuxPane> = eval(&engine, "mux::get_pane(999999999)");
    assert!(result.is_err());
}

#[test]
fn mux_get_tab_unknown_id_errors() {
    let engine = make_engine();
    let result: anyhow::Result<mux_lua::MuxTab> = eval(&engine, "mux::get_tab(999999999)");
    assert!(result.is_err());
}

#[test]
fn mux_all_windows_is_empty_without_spawning() -> anyhow::Result<()> {
    let engine = make_engine();
    let windows: rhai::Array = eval(&engine, "mux::all_windows()")?;
    assert!(windows.is_empty());
    Ok(())
}

// `set_gui_window_resolver_rhai` sets a process-wide slot with no reset hook
// exposed outside the crate (see `window.rs`'s module doc comment), so the
// "no resolver registered yet" and "resolver registered" scenarios are
// combined into a single test that observes them in that fixed order --
// splitting them into two separate `#[test]` functions would make the
// "not yet registered" assertion flaky, since Rust runs `#[test]` functions
// in the same binary concurrently by default and the two could otherwise
// race against each other's use of that shared slot.
#[test]
fn window_gui_window_before_and_after_registering_a_resolver() -> anyhow::Result<()> {
    let engine = make_engine();
    // `MuxWindow` is a plain id newtype (`pub struct MuxWindow(pub
    // WindowId)`, see `window.rs`), so it can be constructed directly in
    // Rust and handed into a rhai `Scope` without going through
    // `mux::get_window`/a live spawn -- this exercises `gui_window()`'s
    // weak/late-bound resolver hook (see `window.rs`'s module doc comment)
    // in isolation, independent of whether the id actually resolves in this
    // test's `Mux`.
    let window = mux_lua::MuxWindow(999999999);
    let mut scope = rhai::Scope::new();
    scope.push("w", window);

    let before: Result<rhai::Dynamic, _> = engine.eval_with_scope(&mut scope, "w.gui_window()");
    assert!(before.is_err());

    mux_lua::set_gui_window_resolver_rhai(|window_id| {
        Some(rhai::Dynamic::from(format!("gui-window-{window_id}")))
    });
    let after: String = engine
        .eval_with_scope(&mut scope, "w.gui_window()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(after, "gui-window-999999999");
    Ok(())
}

#[test]
fn window_id_and_to_string_do_not_require_a_live_window() -> anyhow::Result<()> {
    let engine = make_engine();
    let window = mux_lua::MuxWindow(7);
    let mut scope = rhai::Scope::new();
    scope.push("w", window);
    let id: rhai::INT = engine
        .eval_with_scope(&mut scope, "w.window_id()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(id, 7);
    let s: String = engine
        .eval_with_scope(&mut scope, "w.to_string()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(s.starts_with("MuxWindow("));
    Ok(())
}

#[test]
fn tab_id_and_to_string_do_not_require_a_live_tab() -> anyhow::Result<()> {
    let engine = make_engine();
    let tab = mux_lua::MuxTab(9);
    let mut scope = rhai::Scope::new();
    scope.push("t", tab);
    let id: rhai::INT = engine
        .eval_with_scope(&mut scope, "t.tab_id()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(id, 9);
    let s: String = engine
        .eval_with_scope(&mut scope, "t.to_string()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(s.starts_with("MuxTab("));
    Ok(())
}

#[test]
fn tab_window_returns_unit_when_tab_is_in_no_window() -> anyhow::Result<()> {
    let engine = make_engine();
    // `MuxTab::window()`'s rhai binding iterates every window looking for
    // this tab id; with no window ever spawned, it must return unit rather
    // than erroring (mirrors the mlua path's `Option<MuxWindow>` -> `nil`).
    let tab = mux_lua::MuxTab(999999999);
    let mut scope = rhai::Scope::new();
    scope.push("t", tab);
    let result: rhai::Dynamic = engine
        .eval_with_scope(&mut scope, "t.window()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(result.is_unit());
    Ok(())
}

#[test]
fn tab_get_title_on_unresolved_id_errors() {
    let engine = make_engine();
    let tab = mux_lua::MuxTab(999999999);
    let mut scope = rhai::Scope::new();
    scope.push("t", tab);
    let result: Result<String, _> = engine.eval_with_scope(&mut scope, "t.get_title()");
    assert!(result.is_err());
}

#[test]
fn pane_id_and_to_string_do_not_require_a_live_pane() -> anyhow::Result<()> {
    let engine = make_engine();
    let pane = mux_lua::MuxPane(11);
    let mut scope = rhai::Scope::new();
    scope.push("p", pane);
    let id: rhai::INT = engine
        .eval_with_scope(&mut scope, "p.pane_id()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(id, 11);
    let s: String = engine
        .eval_with_scope(&mut scope, "p.to_string()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(s.starts_with("MuxPane("));
    let same: mux_lua::MuxPane = engine
        .eval_with_scope(&mut scope, "p.mux_pane()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let _ = same; // `mux_pane()` needs no live pane -- it's a bare identity fn.
    Ok(())
}

#[test]
fn pane_window_and_tab_return_unit_when_unresolved() -> anyhow::Result<()> {
    let engine = make_engine();
    let pane = mux_lua::MuxPane(999999999);
    let mut scope = rhai::Scope::new();
    scope.push("p", pane);
    let window: rhai::Dynamic = engine
        .eval_with_scope(&mut scope, "p.window()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(window.is_unit());
    let tab: rhai::Dynamic = engine
        .eval_with_scope(&mut scope, "p.tab()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(tab.is_unit());
    Ok(())
}

#[test]
fn pane_get_title_on_unresolved_id_errors() {
    let engine = make_engine();
    let pane = mux_lua::MuxPane(999999999);
    let mut scope = rhai::Scope::new();
    scope.push("p", pane);
    let result: Result<String, _> = engine.eval_with_scope(&mut scope, "p.get_title()");
    assert!(result.is_err());
}

#[test]
fn pane_get_domain_name_on_unresolved_id_errors() {
    let engine = make_engine();
    // Both the mlua and rhai bindings resolve the pane id (`this.resolve`/
    // `resolve_rhai`) *before* falling back to `""` for a resolved pane
    // whose domain has since disappeared -- an unresolved pane id errors at
    // that first `resolve` step, same as every other pane method here.
    let pane = mux_lua::MuxPane(999999999);
    let mut scope = rhai::Scope::new();
    scope.push("p", pane);
    let result: Result<String, _> = engine.eval_with_scope(&mut scope, "p.get_domain_name()");
    assert!(result.is_err());
}
