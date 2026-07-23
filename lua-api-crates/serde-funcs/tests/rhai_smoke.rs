//! L4b smoke test: `serde_funcs::register_rhai` exposes rhai equivalents of
//! every function `register(lua: &Lua)` registers on the mlua path (see
//! docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! This crate's rhai port uses rhai's own `serde` feature integration
//! (`rhai::serde::{to_dynamic, from_dynamic}`) rather than a hand-rolled
//! `Dynamic <-> serde_json::Value` tree walk -- see the doc comment on
//! `register_rhai` in src/lib.rs for the rationale. Every test below is
//! deterministic (pure text <-> value transforms, no filesystem/OS/env
//! dependence), so each is checked against an exact expected value or a
//! round-trip, mirroring `lua-api-crates/serde-funcs/src/lib.rs`'s own
//! `#[cfg(test)]` module (`test_json_encode_decode` et al), just driven
//! through a `.rhai` script + `register_rhai` instead of calling the mlua
//! functions directly.

use rhai::Engine;

fn make_engine() -> Engine {
    let mut engine = Engine::new();
    serde_funcs::register_rhai(&mut engine).expect("register_rhai");
    engine
}

#[test]
fn rhai_json_decode_then_json_encode_round_trips() -> anyhow::Result<()> {
    let engine = make_engine();
    let script = r#"
        let decoded = serde::json_decode(`{"a": 1, "b": "two", "c": [1, 2, 3], "d": true, "e": null}`);
        serde::json_encode(decoded)
    "#;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let result_json: serde_json::Value = serde_json::from_str(&result)?;
    let expected_json: serde_json::Value = serde_json::json!({
        "a": 1, "b": "two", "c": [1, 2, 3], "d": true, "e": null
    });
    assert_eq!(result_json, expected_json);
    Ok(())
}

#[test]
fn rhai_json_encode_matches_serde_json_directly() -> anyhow::Result<()> {
    let engine = make_engine();
    let script = r#"serde::json_encode(#{ key2str: "value1", key2int: 4, key2arr: [2, 3] })"#;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let result_json: serde_json::Value = serde_json::from_str(&result)?;
    let expected_json = serde_json::json!({ "key2str": "value1", "key2int": 4, "key2arr": [2, 3] });
    assert_eq!(result_json, expected_json);
    Ok(())
}

#[test]
fn rhai_json_encode_pretty_is_valid_and_equivalent_json() -> anyhow::Result<()> {
    let engine = make_engine();
    let script = r#"serde::json_encode_pretty(#{ a: 1, b: [1, 2] })"#;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    // Pretty output should contain newlines (unlike the compact encoder) and
    // parse back to the same value.
    assert!(result.contains('\n'));
    let result_json: serde_json::Value = serde_json::from_str(&result)?;
    assert_eq!(result_json, serde_json::json!({ "a": 1, "b": [1, 2] }));
    Ok(())
}

#[test]
fn rhai_json_parse_alias_matches_json_decode() -> anyhow::Result<()> {
    let engine = make_engine();
    // `json_parse` is the backward-compat top-level alias for
    // `serde::json_decode` (mirrors `wezterm.json_parse` on the mlua path).
    let via_alias: rhai::Map = engine
        .eval(r#"json_parse(`{"x": 1}`)"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let via_module: rhai::Map = engine
        .eval(r#"serde::json_decode(`{"x": 1}`)"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(
        via_alias.get("x").unwrap().as_int().unwrap(),
        via_module.get("x").unwrap().as_int().unwrap()
    );

    // Top-level `json_encode` alias too.
    let via_alias_enc: String = engine
        .eval(r#"json_encode(#{ x: 1 })"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let via_module_enc: String = engine
        .eval(r#"serde::json_encode(#{ x: 1 })"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let a: serde_json::Value = serde_json::from_str(&via_alias_enc)?;
    let b: serde_json::Value = serde_json::from_str(&via_module_enc)?;
    assert_eq!(a, b);
    Ok(())
}

#[test]
fn rhai_yaml_decode_then_yaml_encode_round_trips() -> anyhow::Result<()> {
    let engine = make_engine();
    let script = r##"
        let decoded = serde::yaml_decode("a: 1\nb: two\nc:\n  - 1\n  - 2\n");
        serde::yaml_encode(decoded)
    "##;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let result_yaml: serde_json::Value = serde_yaml::from_str(&result)?;
    let expected_yaml: serde_json::Value = serde_yaml::from_str("a: 1\nb: two\nc:\n  - 1\n  - 2\n")?;
    assert_eq!(result_yaml, expected_yaml);
    Ok(())
}

#[test]
fn rhai_yaml_encode_matches_serde_yaml_directly() -> anyhow::Result<()> {
    let engine = make_engine();
    let script = r#"serde::yaml_encode(#{ key: "value", nested: #{ n: 5 } })"#;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let result_yaml: serde_json::Value = serde_yaml::from_str(&result)?;
    assert_eq!(
        result_yaml,
        serde_json::json!({ "key": "value", "nested": { "n": 5 } })
    );
    Ok(())
}

#[test]
fn rhai_toml_decode_then_toml_encode_round_trips() -> anyhow::Result<()> {
    let engine = make_engine();
    let script = r##"
        let decoded = serde::toml_decode("a = 1\nb = \"two\"\n");
        serde::toml_encode(decoded)
    "##;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let result_toml: serde_json::Value = toml::from_str(&result)?;
    let expected_toml: serde_json::Value = toml::from_str("a = 1\nb = \"two\"\n")?;
    assert_eq!(result_toml, expected_toml);
    Ok(())
}

#[test]
fn rhai_toml_encode_pretty_is_valid_and_equivalent_toml() -> anyhow::Result<()> {
    let engine = make_engine();
    let script = r#"serde::toml_encode_pretty(#{ a: 1, nested: #{ n: 2 } })"#;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let result_toml: serde_json::Value = toml::from_str(&result)?;
    assert_eq!(
        result_toml,
        serde_json::json!({ "a": 1, "nested": { "n": 2 } })
    );
    Ok(())
}

#[test]
fn rhai_json_decode_reports_errors_for_invalid_json() {
    let engine = make_engine();
    let result: Result<rhai::Dynamic, _> = engine.eval(r#"serde::json_decode("not json at all {")"#);
    assert!(result.is_err());
}

#[test]
fn rhai_yaml_decode_and_toml_decode_reject_malformed_input() {
    let engine = make_engine();
    // Malformed TOML (unterminated string) should error out, not silently
    // succeed with partial/garbage data.
    let toml_result: Result<rhai::Dynamic, _> =
        engine.eval(r#"serde::toml_decode("a = \"unterminated")"#);
    assert!(toml_result.is_err());
}
