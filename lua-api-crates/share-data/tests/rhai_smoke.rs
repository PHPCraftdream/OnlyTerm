//! L4c smoke test: `share_data::register_rhai` exposes a rhai equivalent of
//! `wezterm.GLOBAL` (see docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! The most important property this crate must preserve across the Lua ->
//! rhai migration is called out explicitly in the migration plan: `GLOBAL` is
//! meant to share data *between* different parts of the config/event
//! pipeline, so a value written from one script engine must be visible from
//! the other -- there cannot secretly be two independent stores. Several
//! tests below therefore mix an `mlua::Lua` instance (via `register`) and a
//! `rhai::Engine` instance (via `register_rhai`) in the same test and assert
//! that writes performed through one are observable through the other.
//!
//! Because the underlying storage (`GLOBALS`) is a single process-wide
//! `lazy_static`, tests that mutate it run serially (`#[test]` functions in
//! one binary already run in threads by default, so a lock-free shared
//! mutable static would make tests interfere with each other); each test
//! here uses a distinct top-level key (namespaced by the test name) to avoid
//! cross-test interference despite sharing the same global underneath.

use config::lua::mlua::Lua;
use rhai::Engine;

fn make_rhai_engine() -> Engine {
    let mut engine = Engine::new();
    share_data::register_rhai(&mut engine).expect("register_rhai");
    engine
}

/// Builds a plain `mlua::Lua` with `share_data::register` applied, the same
/// way `config::lua::make_lua_context` does for the real config pipeline.
/// `register` only populates `package.loaded.wezterm` (see
/// `config::lua::get_or_create_module`), not a bare `wezterm` global -- real
/// `.lua` configs do `local wezterm = require 'wezterm'` themselves, so
/// scripts run against this `Lua` do the same.
fn make_lua() -> Lua {
    let lua = Lua::new();
    share_data::register(&lua).expect("register");
    lua
}

#[test]
fn rhai_global_data_set_and_get_round_trip() -> anyhow::Result<()> {
    let engine = make_rhai_engine();
    let script = r#"
        let g = global_data();
        g["rhai_round_trip_key"] = "hello from rhai";
        g["rhai_round_trip_key"]
    "#;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result, "hello from rhai");
    Ok(())
}

#[test]
fn rhai_global_data_missing_key_is_unit() -> anyhow::Result<()> {
    let engine = make_rhai_engine();
    let result: rhai::Dynamic = engine
        .eval(r#"global_data()["this_key_was_never_set_rhai"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(result.is_unit());
    Ok(())
}

#[test]
fn rhai_global_data_supports_nested_values() -> anyhow::Result<()> {
    let engine = make_rhai_engine();
    let script = r#"
        let g = global_data();
        g["rhai_nested_key"] = #{ a: 1, b: [1, 2, 3], c: "text" };
        let back = g["rhai_nested_key"];
        [back.a, back.b.len(), back.c]
    "#;
    let result: rhai::Array = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result[0].as_int().unwrap(), 1);
    assert_eq!(result[1].as_int().unwrap(), 3);
    assert_eq!(result[2].clone().into_string().unwrap(), "text");
    Ok(())
}

/// The critical cross-engine test: a value written through the *mlua* path
/// (`wezterm.GLOBAL`, via `register`) must be visible through the *rhai*
/// path (`global_data()`, via `register_rhai`), because both clone the same
/// `GLOBALS` singleton (an `Arc<Mutex<..>>`-backed `Value::Object`) rather
/// than each getting an independent store.
#[test]
fn value_written_via_mlua_is_visible_via_rhai() -> anyhow::Result<()> {
    let lua = make_lua();
    lua.load(
        r#"
        local wezterm = require 'wezterm'
        wezterm.GLOBAL.cross_engine_key_from_lua = "written by lua"
        "#,
    )
    .exec()?;

    let engine = make_rhai_engine();
    let result: String = engine
        .eval(r#"global_data()["cross_engine_key_from_lua"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result, "written by lua");
    Ok(())
}

/// The mirror image of the test above: a value written through the *rhai*
/// path must be visible through the *mlua* path.
#[test]
fn value_written_via_rhai_is_visible_via_mlua() -> anyhow::Result<()> {
    let engine = make_rhai_engine();
    // rhai's assignment syntax requires the left-hand side to be rooted in a
    // variable (a call result like `global_data()["key"] = ...` is not a
    // valid assignment target -- see the module-level "Assignment target"
    // note below), so bind the `GlobalData` handle to a local first.
    engine
        .run(r#"let g = global_data(); g["cross_engine_key_from_rhai"] = "written by rhai";"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let lua = make_lua();
    let value: String = lua
        .load(r#"local wezterm = require 'wezterm'; return wezterm.GLOBAL.cross_engine_key_from_rhai"#)
        .eval()?;
    assert_eq!(value, "written by rhai");
    Ok(())
}

/// Confirms the sharing survives *multiple independent engine instances* on
/// both sides simultaneously -- not just a single Lua + single rhai pairing --
/// since real usage may construct several `Lua`/`Engine` instances over a
/// process's lifetime (e.g. one per config reload), and all of them must
/// still observe the one shared store.
#[test]
fn multiple_engine_instances_all_share_the_same_store() -> anyhow::Result<()> {
    let lua_a = make_lua();
    let lua_b = make_lua();
    let engine_a = make_rhai_engine();
    let engine_b = make_rhai_engine();

    lua_a
        .load(r#"local wezterm = require 'wezterm'; wezterm.GLOBAL.multi_engine_key = 1"#)
        .exec()?;

    // Visible from a second, independently-constructed Lua instance.
    let via_lua_b: i64 = lua_b
        .load(r#"local wezterm = require 'wezterm'; return wezterm.GLOBAL.multi_engine_key"#)
        .eval()?;
    assert_eq!(via_lua_b, 1);

    // Visible from both independently-constructed rhai engines.
    let via_engine_a: rhai::INT = engine_a
        .eval(r#"global_data()["multi_engine_key"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(via_engine_a, 1);
    let via_engine_b: rhai::INT = engine_b
        .eval(r#"global_data()["multi_engine_key"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(via_engine_b, 1);

    // And a write from one of the rhai engines is visible everywhere else,
    // including back on the mlua side.
    engine_b
        .run(r#"let g = global_data(); g["multi_engine_key"] = 2;"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let via_lua_a: i64 = lua_a
        .load(r#"local wezterm = require 'wezterm'; return wezterm.GLOBAL.multi_engine_key"#)
        .eval()?;
    assert_eq!(via_lua_a, 2);

    Ok(())
}

#[test]
fn rhai_global_data_len_reflects_object_key_count() -> anyhow::Result<()> {
    let engine = make_rhai_engine();
    // `len` is a get-property on `GlobalData` (mirroring the mlua path's
    // `__len` metamethod), reflecting the *whole* shared object's key count,
    // so just assert it is non-negative and grows after inserting a
    // guaranteed-fresh key, rather than asserting an exact number (other
    // tests in this same binary share and mutate the same `GLOBALS` root
    // object concurrently).
    let before: rhai::INT = engine
        .eval(r#"global_data().len"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    engine
        .run(r#"let g = global_data(); g["len_probe_key_rhai"] = true;"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let after: rhai::INT = engine
        .eval(r#"global_data().len"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(after > before);
    Ok(())
}

#[test]
fn rhai_global_data_array_values_round_trip() -> anyhow::Result<()> {
    // `GlobalData["key"] = [...]` stores a whole array value under a
    // top-level key (mirroring `wezterm.GLOBAL.some_key = {10, 20, 30}` on
    // the mlua path); reading it back through the same key returns an
    // equivalent rhai `Array`. Note that -- like the mlua `Value::Array`
    // this wraps -- an array *nested inside* a GLOBAL value is itself a
    // further independently-addressable shared subtree (see
    // `dynamic_to_gvalue`'s doc comment in src/lib.rs): indexing into it via
    // `g["key"][i]` reads/writes that subtree directly, without needing to
    // round-trip the whole array back out to rhai first.
    let engine = make_rhai_engine();
    engine
        .run(r#"let g = global_data(); g["rhai_array_key"] = [10, 20, 30];"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let whole: rhai::Array = engine
        .eval(r#"global_data()["rhai_array_key"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(whole.len(), 3);
    assert_eq!(whole[1].as_int().unwrap(), 20);

    Ok(())
}
