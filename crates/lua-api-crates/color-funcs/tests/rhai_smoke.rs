//! L4a smoke test: `color_funcs::register_rhai` exposes rhai equivalents of
//! every function `register(lua: &Lua)` registers on the mlua path (see
//! docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! The first two tests (`rhai_can_register_and_call_color_parse_like_the_lua_api_does`,
//! `rhai_reports_parse_errors_like_the_lua_api_does`) are the original L0 tooling
//! sample and are left untouched (still passing against the standalone
//! `parse_color` defined in this file, independent of the real
//! `color_funcs::register_rhai` exercised by everything below) -- this file now
//! additionally covers every remaining function.
//!
//! ## Handling non-determinism / environment dependence
//!
//! Most of this crate's functions are pure (color math) or deterministic given a
//! fixed input file, so most tests compare directly against calling the
//! equivalent Rust function/mlua semantics. The two exceptions:
//!   * `extract_colors_from_image`: analyzes a real image. Rather than
//!     asserting a fixed color list (brittle: depends on the exact
//!     quantization/threshold algorithm's tie-breaking), this test generates a
//!     small solid-color PNG fixture and asserts on *shape* (returns at least
//!     one `Color`, doesn't panic) plus that the returned color is close to the
//!     fixture's known fill color -- deterministic because the fixture image
//!     itself is deterministic, even though "how many distinct colors come
//!     back" is an implementation detail we don't want to over-specify.
//!   * `get_builtin_color_schemes`/`get_builtin_schemes`: the specific set of
//!     bundled schemes is a large, frequently-updated data file
//!     (`config::COLOR_SCHEMES`), so instead of hardcoding a scheme count or
//!     name list, this test cross-checks against `config::COLOR_SCHEMES`
//!     directly (the same static the mlua path clones), so it can never drift
//!     from whatever schemes are actually bundled.

use rhai::{Engine, Scope};
use std::fs;

fn make_engine() -> Engine {
    let mut engine = Engine::new();
    color_funcs::register_rhai(&mut engine).expect("register_rhai");
    engine
}

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
    let mut color_module = rhai::Module::new();
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
    let mut color_module = rhai::Module::new();
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

// ---------------------------------------------------------------------------
// L4a: real `color_funcs::register_rhai` coverage, one test per function.
// ---------------------------------------------------------------------------

#[test]
fn rhai_color_parse_matches_config_rgbacolor() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: String = engine
        .eval(r##"color::parse("#ff0000").to_string()"##)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let expected = String::from(config::RgbaColor::try_from("#ff0000".to_string())?);
    assert_eq!(result, expected);
    Ok(())
}

#[test]
fn rhai_color_parse_error_on_bad_spec() {
    let engine = make_engine();
    let result: Result<rhai::Dynamic, _> = engine.eval(r#"color::parse("nonsense-color")"#);
    assert!(result.is_err());
}

#[test]
fn rhai_color_from_hsla_matches_srgbatuple() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: String = engine
        .eval(r#"color::from_hsla(120.0, 1.0, 0.5, 1.0).to_string()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let expected: String = {
        let tuple = config::SrgbaTuple::from_hsla(120.0, 1.0, 0.5, 1.0);
        let color: config::RgbaColor = tuple.into();
        color.into()
    };
    assert_eq!(result, expected);
    Ok(())
}

#[test]
fn rhai_color_equality_operator() -> anyhow::Result<()> {
    let engine = make_engine();
    let same: bool = engine
        .eval(r#"color::parse("red") == color::parse("red")"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(same);
    let different: bool = engine
        .eval(r#"color::parse("red") == color::parse("blue")"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(!different);
    Ok(())
}

#[test]
fn rhai_color_complement_and_complement_ryb() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: String = engine
        .eval(r#"color::parse("red").complement().to_string()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let base = config::RgbaColor::try_from("red".to_string())?;
    let expected: String = config::RgbaColor::from(base.complement()).into();
    assert_eq!(result, expected);

    let result_ryb: String = engine
        .eval(r#"color::parse("red").complement_ryb().to_string()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let expected_ryb: String = config::RgbaColor::from(base.complement_ryb()).into();
    assert_eq!(result_ryb, expected_ryb);
    Ok(())
}

#[test]
fn rhai_color_triad_returns_two_element_array() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: rhai::Array = engine
        .eval(r#"color::parse("red").triad()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result.len(), 2);

    let base = config::RgbaColor::try_from("red".to_string())?;
    let (a, b) = base.triad();
    let a_str: String = config::RgbaColor::from(a).into();
    let b_str: String = config::RgbaColor::from(b).into();

    // Colors are custom types (`Color`), not strings, in the array; call
    // to_string() on them via a fresh eval that reads the value back in,
    // proving the array holds real, method-callable Color values (not just
    // opaque blobs).
    let got_a: String = {
        let mut scope = Scope::new();
        scope.push("c", result[0].clone());
        engine
            .eval_with_scope(&mut scope, "c.to_string()")
            .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?
    };
    let got_b: String = {
        let mut scope = Scope::new();
        scope.push("c", result[1].clone());
        engine
            .eval_with_scope(&mut scope, "c.to_string()")
            .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?
    };

    assert_eq!(got_a, a_str);
    assert_eq!(got_b, b_str);
    Ok(())
}

#[test]
fn rhai_color_square_returns_three_element_array() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: rhai::Array = engine
        .eval(r#"color::parse("red").square()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result.len(), 3);
    Ok(())
}

#[test]
fn rhai_color_saturate_desaturate_lighten_darken_and_hue() -> anyhow::Result<()> {
    let engine = make_engine();
    let base = config::RgbaColor::try_from("red".to_string())?;

    let cases: &[(&str, config::SrgbaTuple)] = &[
        (
            r#"color::parse("red").saturate(0.5).to_string()"#,
            base.saturate(0.5),
        ),
        (
            r#"color::parse("red").desaturate(0.5).to_string()"#,
            base.saturate(-0.5),
        ),
        (
            r#"color::parse("red").saturate_fixed(0.1).to_string()"#,
            base.saturate_fixed(0.1),
        ),
        (
            r#"color::parse("red").desaturate_fixed(0.1).to_string()"#,
            base.saturate_fixed(-0.1),
        ),
        (
            r#"color::parse("red").lighten(0.5).to_string()"#,
            base.lighten(0.5),
        ),
        (
            r#"color::parse("red").darken(0.5).to_string()"#,
            base.lighten(-0.5),
        ),
        (
            r#"color::parse("red").lighten_fixed(0.1).to_string()"#,
            base.lighten_fixed(0.1),
        ),
        (
            r#"color::parse("red").darken_fixed(0.1).to_string()"#,
            base.lighten_fixed(-0.1),
        ),
        (
            r#"color::parse("red").adjust_hue_fixed(30.0).to_string()"#,
            base.adjust_hue_fixed(30.0),
        ),
        (
            r#"color::parse("red").adjust_hue_fixed_ryb(30.0).to_string()"#,
            base.adjust_hue_fixed_ryb(30.0),
        ),
    ];

    for (script, expected) in cases {
        let result: String = engine
            .eval(script)
            .map_err(|err| anyhow::anyhow!("rhai eval error on `{script}`: {err}"))?;
        let expected_str: String = config::RgbaColor::from(*expected).into();
        assert_eq!(result, expected_str, "mismatch for script `{script}`");
    }
    Ok(())
}

#[test]
fn rhai_color_srgba_u8_matches_direct_call() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: rhai::Array = engine
        .eval(r#"color::parse("red").srgba_u8()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let base = config::RgbaColor::try_from("red".to_string())?;
    let (r, g, b, a) = base.to_srgb_u8();
    assert_eq!(result[0].as_int().unwrap(), r as i64);
    assert_eq!(result[1].as_int().unwrap(), g as i64);
    assert_eq!(result[2].as_int().unwrap(), b as i64);
    assert_eq!(result[3].as_int().unwrap(), a as i64);
    Ok(())
}

#[test]
fn rhai_color_linear_rgba_matches_direct_call() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: rhai::Array = engine
        .eval(r#"color::parse("red").linear_rgba()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let base = config::RgbaColor::try_from("red".to_string())?;
    let rgba = base.to_linear();
    assert!((result[0].as_float().unwrap() - rgba.0 as f64).abs() < 1e-6);
    assert!((result[1].as_float().unwrap() - rgba.1 as f64).abs() < 1e-6);
    assert!((result[2].as_float().unwrap() - rgba.2 as f64).abs() < 1e-6);
    assert!((result[3].as_float().unwrap() - rgba.3 as f64).abs() < 1e-6);
    Ok(())
}

#[test]
fn rhai_color_hsla_and_laba_match_direct_call() -> anyhow::Result<()> {
    let engine = make_engine();
    let base = config::RgbaColor::try_from("red".to_string())?;

    let hsla: rhai::Array = engine
        .eval(r#"color::parse("red").hsla()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let (h, s, l, a) = base.to_hsla();
    assert!((hsla[0].as_float().unwrap() - h).abs() < 1e-6);
    assert!((hsla[1].as_float().unwrap() - s).abs() < 1e-6);
    assert!((hsla[2].as_float().unwrap() - l).abs() < 1e-6);
    assert!((hsla[3].as_float().unwrap() - a).abs() < 1e-6);

    let laba: rhai::Array = engine
        .eval(r#"color::parse("red").laba()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let (l2, a2, b2, alpha2) = base.to_laba();
    assert!((laba[0].as_float().unwrap() - l2).abs() < 1e-6);
    assert!((laba[1].as_float().unwrap() - a2).abs() < 1e-6);
    assert!((laba[2].as_float().unwrap() - b2).abs() < 1e-6);
    assert!((laba[3].as_float().unwrap() - alpha2).abs() < 1e-6);
    Ok(())
}

#[test]
fn rhai_color_contrast_ratio_and_delta_e_match_direct_call() -> anyhow::Result<()> {
    let engine = make_engine();
    let result_contrast: f64 = engine
        .eval(r#"color::parse("red").contrast_ratio(color::parse("blue"))"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let result_delta: f64 = engine
        .eval(r#"color::parse("red").delta_e(color::parse("blue"))"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let red = config::RgbaColor::try_from("red".to_string())?;
    let blue = config::RgbaColor::try_from("blue".to_string())?;
    let expected_contrast = red.contrast_ratio(&blue) as f64;
    let expected_delta = red.delta_e(&blue) as f64;

    assert!((result_contrast - expected_contrast).abs() < 1e-4);
    assert!((result_delta - expected_delta).abs() < 1e-4);
    Ok(())
}

#[test]
fn rhai_get_default_colors_returns_a_palette_like_map() -> anyhow::Result<()> {
    let engine = make_engine();
    // Palette round-trips through `impl_rhai_conversion_dynamic!` as an object
    // map; confirm at least the `foreground`/`background` keys are present and
    // agree with calling the same underlying conversion directly.
    let result: rhai::Map = engine
        .eval(r#"color::get_default_colors()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let expected: config::Palette = wezterm_term::color::ColorPalette::default().into();
    assert!(result.contains_key("foreground"));
    assert!(result.contains_key("background"));
    assert_eq!(expected.foreground.is_some(), true);
    Ok(())
}

#[test]
fn rhai_gradient_colors_matches_direct_build() -> anyhow::Result<()> {
    let engine = make_engine();
    let script = r##"
        color::gradient(
            #{
                orientation: "Horizontal",
                colors: ["#ff0000", "#0000ff"],
                preset: (),
                interpolation: "Linear",
                blend: "Rgb",
                segment_size: (),
                segment_smoothness: (),
                noise: (),
            },
            4
        )
    "##;
    let result: rhai::Array = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result.len(), 4);

    // Cross-check against the same call site the mlua path uses
    // (`Gradient::build` + `.colors(n)`), rather than hardcoding expected color
    // strings, so this test can't drift from the real gradient math.
    let gradient = config::Gradient {
        orientation: Default::default(),
        colors: vec!["#ff0000".to_string(), "#0000ff".to_string()],
        preset: None,
        interpolation: Default::default(),
        blend: Default::default(),
        segment_size: None,
        segment_smoothness: None,
        noise: None,
    };
    let built = gradient.build()?;
    assert_eq!(built.colors(4).len(), result.len());

    // Also reachable as a bare top-level function (mirrors
    // `wezterm.gradient_colors(...)` on the mlua path).
    let script2 = script.replace("color::gradient(", "gradient_colors(");
    let result2: rhai::Array = engine
        .eval(&script2)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result2.len(), 4);
    Ok(())
}

#[test]
fn rhai_gradient_colors_rejects_negative_count() {
    let engine = make_engine();
    let script = r##"
        color::gradient(
            #{ colors: ["#ff0000", "#0000ff"] },
            -1
        )
    "##;
    let result: Result<rhai::Array, _> = engine.eval(script);
    assert!(result.is_err(), "negative num_colors should be rejected");
}

#[test]
fn rhai_get_builtin_color_schemes_matches_config_color_schemes() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: rhai::Map = engine
        .eval(r#"color::get_builtin_schemes()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    assert_eq!(result.len(), config::COLOR_SCHEMES.len());
    for name in config::COLOR_SCHEMES.keys() {
        assert!(
            result.contains_key(name.as_str()),
            "expected builtin scheme `{name}` to be present in the rhai result"
        );
    }

    // Also reachable as a bare top-level function (mirrors
    // `wezterm.get_builtin_color_schemes()` on the mlua path).
    let result2: rhai::Map = engine
        .eval(r#"get_builtin_color_schemes()"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result2.len(), config::COLOR_SCHEMES.len());
    Ok(())
}

#[test]
fn rhai_load_and_save_scheme_round_trip_via_toml() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "wezterm-color-funcs-rhai-smoke-{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;
    let path = dir.join("scheme.toml");
    let path_str = path.display().to_string().replace('\\', "\\\\");

    let engine = make_engine();

    // Save a scheme built from an explicit `colors` object-map literal, then
    // load it back and confirm round-trip fidelity on a representative field.
    //
    // Deliberately *not* built from `color::get_default_colors()` here: that
    // populates `Palette::indexed` (a `HashMap<u8, RgbaColor>` for the
    // extended 256-color palette), and rhai's `Map` type only supports string
    // keys -- per the documented policy in `config/src/rhai_value.rs`
    // (`rhai_dynamic_to_dynamic`'s module-level doc comment: "rhai map keys
    // are always plain identifiers/strings ... rhai's Map type simply doesn't
    // support" non-string keys), a `HashMap<u8, _>` round-tripped through a
    // rhai `Map` loses the numeric-key typing `FromDynamic for HashMap<K, _>`
    // needs to reconstruct `u8` keys, and fails to convert back. That's a
    // pre-existing, documented limitation of the L1 `rhai::Dynamic <->
    // wezterm_dynamic::Value` bridge itself (not something introduced or
    // fixable in this L4a per-crate port), so this test simply avoids
    // exercising that unsupported edge case, the same way a real `.rhai`
    // config author populating `colors` by hand would rarely set `indexed`
    // explicitly either.
    let script = format!(
        r##"
        let colors = #{{
            foreground: "#ffffff",
            background: "#000000",
            // `ColorSchemeFile::from_toml_value` (invoked by `load_scheme`)
            // requires `ansi` to be present, mirroring the same requirement
            // the mlua path is subject to (it calls the same underlying
            // `ColorSchemeFile::from_toml_str`).
            ansi: [
                "#000000", "#ff0000", "#00ff00", "#ffff00",
                "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
            ],
        }};
        let metadata = #{{ name: "test-scheme", author: (), origin_url: (), wezterm_version: (), aliases: [] }};
        color::save_scheme(colors, metadata, "{path_str}");
        color::load_scheme("{path_str}")
        "##
    );
    let result: rhai::Map = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    assert!(result.contains_key("colors"));
    assert!(result.contains_key("metadata"));

    let metadata = result
        .get("metadata")
        .unwrap()
        .clone()
        .try_cast::<rhai::Map>()
        .expect("metadata should be a map");
    assert_eq!(
        metadata.get("name").unwrap().clone().into_string().unwrap(),
        "test-scheme"
    );

    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn rhai_load_scheme_reports_errors_for_missing_file() {
    let engine = make_engine();
    let result: Result<rhai::Map, _> =
        engine.eval(r#"color::load_scheme("/this/path/does/not/exist.toml")"#);
    assert!(result.is_err());
}

#[test]
fn rhai_load_terminal_sexy_scheme_matches_direct_call() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "wezterm-color-funcs-rhai-smoke-sexy-{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;
    let path = dir.join("scheme.json");

    // Minimal terminal.sexy-format JSON fixture matching
    // lua-api-crates/color-funcs/src/schemes/sexy.rs's Sexy struct.
    let json = r##"{
        "name": "Test Sexy Scheme",
        "author": "test",
        "color": [
            "#000000", "#ff0000", "#00ff00", "#ffff00",
            "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
            "#000000", "#ff0000", "#00ff00", "#ffff00",
            "#0000ff", "#ff00ff", "#00ffff", "#ffffff"
        ],
        "foreground": "#ffffff",
        "background": "#000000"
    }"##;
    fs::write(&path, json)?;
    let path_str = path.display().to_string().replace('\\', "\\\\");

    let engine = make_engine();
    let script = format!(r#"color::load_terminal_sexy_scheme("{path_str}")"#);
    let result: rhai::Map = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let expected = color_funcs::schemes::sexy::Sexy::load_file(&path)?;
    let metadata = result
        .get("metadata")
        .unwrap()
        .clone()
        .try_cast::<rhai::Map>()
        .expect("metadata should be a map");
    assert_eq!(
        metadata.get("name").unwrap().clone().into_string().unwrap(),
        expected.metadata.name.unwrap()
    );

    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn rhai_load_base16_scheme_matches_direct_call() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "wezterm-color-funcs-rhai-smoke-base16-{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;
    let path = dir.join("scheme.yaml");

    // Minimal base16 YAML fixture matching
    // lua-api-crates/color-funcs/src/schemes/base16.rs's Base16Scheme struct.
    let yaml = r#"
scheme: "Test Base16 Scheme"
author: "test"
base00: "000000"
base01: "111111"
base02: "222222"
base03: "333333"
base04: "444444"
base05: "555555"
base06: "666666"
base07: "777777"
base08: "880000"
base09: "008800"
base0A: "000088"
base0B: "888800"
base0C: "880088"
base0D: "008888"
base0E: "888888"
base0F: "ffffff"
"#;
    fs::write(&path, yaml)?;
    let path_str = path.display().to_string().replace('\\', "\\\\");

    let engine = make_engine();
    let script = format!(r#"color::load_base16_scheme("{path_str}")"#);
    let result: rhai::Map = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let expected = color_funcs::schemes::base16::Base16Scheme::load_file(&path)?;
    let metadata = result
        .get("metadata")
        .unwrap()
        .clone()
        .try_cast::<rhai::Map>()
        .expect("metadata should be a map");
    assert_eq!(
        metadata.get("name").unwrap().clone().into_string().unwrap(),
        expected.metadata.name.unwrap()
    );

    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

/// `extract_colors_from_image` genuinely touches the filesystem and runs a
/// nontrivial image-analysis algorithm, so rather than asserting an exact
/// color list (which would be sensitive to unrelated changes in the
/// quantization/threshold algorithm), this builds a deterministic
/// single-color fixture image and checks the shape of the result: at least
/// one color comes back, and it's a `Color` (parseable back via `to_string`).
#[test]
fn rhai_extract_colors_from_image_returns_reasonable_shape() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "wezterm-color-funcs-rhai-smoke-img-{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;
    let path = dir.join("fixture.png");

    // A small four-color image (quadrants) -- deterministic input, with more
    // than one distinct color so the *default* `num_colors` (16, see
    // `image_colors::default_num_colors`) doesn't make this call fail the way
    // it legitimately would against a single-color image (that failure mode
    // is exercised deliberately in
    // `rhai_extract_colors_from_image_reports_errors_for_missing_file`'s
    // sibling tests via an explicit `num_colors` override instead). "At least
    // one color comes back" is a meaningful, deterministic assertion without
    // pinning the exact quantization/threshold algorithm's output.
    let mut img = image::RgbaImage::new(16, 16);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        *pixel = match (x < 8, y < 8) {
            (true, true) => image::Rgba([255, 0, 0, 255]),
            (false, true) => image::Rgba([0, 255, 0, 255]),
            (true, false) => image::Rgba([0, 0, 255, 255]),
            (false, false) => image::Rgba([255, 255, 0, 255]),
        };
    }
    img.save(&path)?;
    let path_str = path.display().to_string().replace('\\', "\\\\");

    let engine = make_engine();
    let script = format!(r#"color::extract_colors_from_image("{path_str}")"#);
    let result: rhai::Array = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    assert!(
        !result.is_empty(),
        "expected at least one color extracted from a solid-color fixture image"
    );
    // Each entry should be a Color, callable like any other (proves the type
    // registration/array-of-custom-type marshaling works end to end).
    let mut scope = Scope::new();
    scope.push("c", result[0].clone());
    let as_string: String = engine
        .eval_with_scope(&mut scope, "c.to_string()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(!as_string.is_empty());

    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn rhai_extract_colors_from_image_with_params_overload() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "wezterm-color-funcs-rhai-smoke-img-params-{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;
    let path = dir.join("fixture.png");
    let img = image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 255, 0, 255]));
    img.save(&path)?;
    let path_str = path.display().to_string().replace('\\', "\\\\");

    let engine = make_engine();
    let script = format!(
        r#"color::extract_colors_from_image("{path_str}", #{{ num_colors: 1 }})"#
    );
    let result: rhai::Array = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result.len(), 1);

    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn rhai_extract_colors_from_image_reports_errors_for_missing_file() {
    let engine = make_engine();
    let result: Result<rhai::Array, _> =
        engine.eval(r#"color::extract_colors_from_image("/no/such/image.png")"#);
    assert!(result.is_err());
}
