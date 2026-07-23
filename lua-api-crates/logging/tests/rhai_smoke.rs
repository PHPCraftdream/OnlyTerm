//! L4b smoke test: `logging::register_rhai` exposes rhai equivalents of every
//! function `register(lua: &Lua)` registers on the mlua path (see
//! docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! ## Handling non-determinism / environment dependence
//!
//! `log_error`/`log_info`/`log_warn`/`print` all just forward to the `log`
//! crate's macros, which by default have no installed logger in a test binary
//! (so the calls are near no-ops) -- there is nothing meaningfully
//! non-deterministic here, but there is also nothing to assert *on* other than
//! "the call succeeds and returns unit". `to_string` is the only function with
//! an observable return value, so it gets the bulk of the coverage;
//! `log_error`/`log_info`/`log_warn`/`print` are covered for "callable without
//! erroring, with a variety of argument shapes" instead of output content.

use rhai::Engine;

fn make_engine() -> Engine {
    let mut engine = Engine::new();
    logging::register_rhai(&mut engine).expect("register_rhai");
    engine
}

#[test]
fn rhai_log_error_accepts_an_array_of_mixed_values() -> anyhow::Result<()> {
    let engine = make_engine();
    // Mirrors `wezterm.log_error("a", 1, true)` on the mlua path; see the
    // module-level doc comment on `register_rhai` in src/lib.rs for why this
    // takes a single Array argument on the rhai side instead of being
    // variadic.
    engine
        .eval::<()>(r#"log_error(["a", 1, true])"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    Ok(())
}

#[test]
fn rhai_log_info_accepts_an_array_of_mixed_values() -> anyhow::Result<()> {
    let engine = make_engine();
    engine
        .eval::<()>(r#"log_info(["hello", 42, 3.5])"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    Ok(())
}

#[test]
fn rhai_log_warn_accepts_an_array_of_mixed_values() -> anyhow::Result<()> {
    let engine = make_engine();
    engine
        .eval::<()>(r#"log_warn(["watch out", ()])"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    Ok(())
}

#[test]
fn rhai_print_is_redirected_through_log_info_like_the_lua_path() -> anyhow::Result<()> {
    let engine = make_engine();
    // On the mlua path, `print` shadows Lua's own global `print`
    // (`lua.globals().set("print", ...)`), routing it through `log::info!`
    // instead of stdout. rhai's `print(...)` is a special built-in engine
    // command (not an ordinary function you can override via
    // `register_fn` -- see the doc comment on `register_rhai` in
    // src/lib.rs for why), dispatched through `Engine::on_print`, which
    // `register_rhai` sets to route through `log::info!` the same way. This
    // just proves the call succeeds (there is no installed logger in this
    // test binary to assert output content against, mirroring how
    // `log_error`/`log_info`/`log_warn` are also only checked for "callable
    // without erroring" above).
    engine
        .eval::<()>(r#"print("from print")"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    Ok(())
}

#[test]
fn rhai_to_string_renders_strings() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: String = engine
        .eval(r#"to_string("hello")"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    // Matches the mlua path's `ValuePrinter`-based debug rendering: a bare
    // string renders with its surrounding quotes in debug form.
    assert!(result.contains("hello"));
    Ok(())
}

#[test]
fn rhai_to_string_renders_arrays_and_maps() -> anyhow::Result<()> {
    let engine = make_engine();
    let arr_result: String = engine
        .eval(r#"to_string([1, 2, 3])"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(arr_result.contains('1') && arr_result.contains('2') && arr_result.contains('3'));

    let map_result: String = engine
        .eval(r#"to_string(#{ key: "value" })"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(map_result.contains("key"));
    assert!(map_result.contains("value"));
    Ok(())
}

#[test]
fn rhai_to_string_renders_numbers_and_bools() -> anyhow::Result<()> {
    let engine = make_engine();
    let int_result: String = engine
        .eval(r#"to_string(42)"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(int_result.contains("42"));

    let bool_result: String = engine
        .eval(r#"to_string(true)"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(bool_result.contains("true"));
    Ok(())
}
