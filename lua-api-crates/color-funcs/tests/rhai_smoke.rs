//! L0 tooling: rhai registration-pattern smoke-test, sample for the future
//! L4a-L4e per-crate ports (see docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! This is a parallel, INDEPENDENT test - it does not replace or touch the
//! existing `register(lua: &Lua)` in `lua-api-crates/color-funcs/src/lib.rs`,
//! which keeps registering `wezterm.color.parse` etc. for the current mlua
//! pipeline. This test only proves that the same underlying Rust function
//! (`config::color::RgbaColor::try_from(String)`, the guts of the existing
//! `parse_color` in src/lib.rs) can be exposed to a `.rhai` script with an
//! equivalent call-site shape (`color::parse("red")`), using rhai's own APIs.
//!
//! ## register_fn vs #[export_module]: chosen mechanism for L4a-L4e
//!
//! rhai offers two main ways to expose a group of native functions:
//!   - `Engine::register_fn` (+ `Engine::register_static_module` to namespace it)
//!   - the `#[export_module]` proc-macro, which turns a whole Rust module's `pub
//!     fn`s into a rhai module in one shot.
//!
//! We pick **`register_fn`** (with `register_static_module` for the
//! `wezterm.color`-style namespace) as the default mechanism for all future
//! L4 per-crate ports, for three reasons:
//!   1. It mirrors the current lua-api-crates pattern almost line-for-line:
//!      `color.set("parse", lua.create_function(parse_color)?)` becomes
//!      `module.set_native_fn("parse", parse_color)`. `#[export_module]`
//!      requires restructuring every crate's free functions into an inline
//!      module annotated with the macro, which is a bigger, riskier diff for
//!      a mechanical 13-crate port.
//!   2. Explicit registration keeps full control over the exposed name
//!      independent of the Rust fn name (needed since several lua-api-crates
//!      register the same Rust helper under multiple names/paths, e.g.
//!      `color.set("gradient", ...)` and `wezterm_mod.set("gradient_colors", ...)`
//!      both wrapping `gradient_colors` - see color-funcs/src/lib.rs::register).
//!   3. `#[export_module]` shines when exporting a large, stable, self-contained
//!      API surface as a single unit; our 13 crates instead each expose a
//!      handful of loosely related functions attached to differently-named
//!      sub-modules of a single `wezterm` namespace, which maps more directly
//!      onto imperative `register_fn`/`set_native_fn` calls than onto one
//!      `#[export_module]` per crate.
//!
//! L4 implementers: keep using `register_fn`/`register_static_module` unless a
//! specific crate's function count/complexity makes `#[export_module]` clearly
//! better - this is a default, not an absolute rule.

use rhai::{Engine, Module, Scope};

/// Mirrors `lua-api-crates/color-funcs/src/lib.rs::parse_color`, which has the
/// signature `fn(&Lua, String) -> mlua::Result<ColorWrap>` and is registered as
/// `wezterm.color.parse`. Here we drop the `ColorWrap` `UserData` wrapper (that
/// is an L4a concern - registering a custom rhai type - not an L0 concern) and
/// return the canonical color string directly, which is enough to prove the
/// registration + call-site pattern end-to-end.
fn parse_color(spec: String) -> Result<String, Box<rhai::EvalAltResult>> {
    let color = config::RgbaColor::try_from(spec.clone()).map_err(
        |err| -> Box<rhai::EvalAltResult> {
            format!("failed to parse `{spec}` as RgbaColor: {err:#}").into()
        },
    )?;
    Ok(String::from(color))
}

#[test]
fn rhai_can_register_and_call_color_parse_like_the_lua_api_does() -> anyhow::Result<()> {
    let mut engine = Engine::new();

    // `register_static_module` gives us the `color::parse(...)` call-site shape,
    // which is the closest rhai analogue of `wezterm.color.parse(...)`. A real L4a
    // port would register this as a sub-module of a `wezterm` module rather than a
    // bare global module, but that's the L2/L4 integration concern, not L0's.
    let mut color_module = Module::new();
    color_module.set_native_fn("parse", parse_color);
    engine.register_static_module("color", color_module.into());

    let mut scope = Scope::new();
    let result: String = engine
        .eval_with_scope(&mut scope, r#"color::parse("red")"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    // Compare against calling the exact same underlying Rust function directly,
    // rather than hard-coding rgba float values here.
    let expected = String::from(config::RgbaColor::try_from("red".to_string())?);
    assert_eq!(result, expected);

    Ok(())
}

#[test]
fn rhai_reports_parse_errors_like_the_lua_api_does() {
    let mut engine = Engine::new();
    let mut color_module = Module::new();
    color_module.set_native_fn("parse", parse_color);
    engine.register_static_module("color", color_module.into());

    let mut scope = Scope::new();
    let result: Result<String, _> =
        engine.eval_with_scope(&mut scope, r#"color::parse("not-a-color")"#);

    assert!(
        result.is_err(),
        "expected parsing an invalid color spec to fail, like the existing \
         mlua::Error::external(...) path in lua-api-crates/color-funcs does"
    );
}
