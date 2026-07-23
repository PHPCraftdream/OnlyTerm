//! L4c smoke test: `termwiz_funcs::register_rhai` exposes rhai equivalents of
//! every function `register(lua: &Lua)` registers on the mlua path (see
//! docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! ## Handling non-determinism / environment dependence
//!
//! All of these functions are pure (string/formatting math, a static glyph
//! lookup table) and deterministic given fixed input, so every test compares
//! directly against calling the equivalent Rust function.

use rhai::Engine;

fn make_engine() -> Engine {
    let mut engine = Engine::new();
    termwiz_funcs::register_rhai(&mut engine).expect("register_rhai");
    engine
}

#[test]
fn rhai_nerdfonts_indexer_and_function_match_direct_lookup() -> anyhow::Result<()> {
    let engine = make_engine();

    // Indexer form: `new_nerd_fonts()["cod_account"]`.
    let result: String = engine
        .eval(r#"new_nerd_fonts()["cod_account"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let expected = termwiz::nerdfonts::NERD_FONTS
        .get("cod_account")
        .unwrap()
        .to_string();
    assert_eq!(result, expected);

    // Convenience function form: `nerdfonts("cod_account")`.
    let result2: String = engine
        .eval(r#"nerdfonts("cod_account")"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result2, expected);

    Ok(())
}

#[test]
fn rhai_nerdfonts_missing_key_returns_unit() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: rhai::Dynamic = engine
        .eval(r#"new_nerd_fonts()["this-key-does-not-exist"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(result.is_unit());
    Ok(())
}

#[test]
fn rhai_column_width_matches_unicode_column_width() -> anyhow::Result<()> {
    let engine = make_engine();
    for s in ["hello", "", "a b c", "wide-\u{1F600}-emoji"] {
        let script = format!("column_width(\"{}\")", s.replace('"', "\\\""));
        let result: rhai::INT = engine
            .eval(&script)
            .map_err(|err| anyhow::anyhow!("rhai eval error on `{script}`: {err}"))?;
        let expected = termwiz::cell::unicode_column_width(s, None) as rhai::INT;
        assert_eq!(result, expected, "mismatch for {s:?}");
    }
    Ok(())
}

#[test]
fn rhai_pad_right_and_pad_left_match_direct_call() -> anyhow::Result<()> {
    let engine = make_engine();

    let result: String = engine
        .eval(r#"pad_right("hi", 5)"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result, termwiz_funcs::pad_right("hi".to_string(), 5));

    let result: String = engine
        .eval(r#"pad_left("hi", 5)"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result, termwiz_funcs::pad_left("hi".to_string(), 5));

    Ok(())
}

#[test]
fn rhai_truncate_right_and_truncate_left_match_direct_call() -> anyhow::Result<()> {
    let engine = make_engine();

    let result: String = engine
        .eval(r#"truncate_right("hello world", 5)"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result, termwiz_funcs::truncate_right("hello world", 5));

    let result: String = engine
        .eval(r#"truncate_left("hello world", 5)"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result, termwiz_funcs::truncate_left("hello world", 5));

    Ok(())
}

#[test]
fn rhai_format_renders_text_and_reset_attributes() -> anyhow::Result<()> {
    let engine = make_engine();
    let script = r#"
        format([
            #{ Text: "hello" },
            "ResetAttributes",
        ])
    "#;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let expected = termwiz_funcs::format_as_escapes(vec![
        termwiz_funcs::FormatItem::Text("hello".to_string()),
        termwiz_funcs::FormatItem::ResetAttributes,
    ])?;
    assert_eq!(result, expected);
    assert!(result.contains("hello"));
    Ok(())
}

#[test]
fn rhai_format_renders_foreground_ansi_color() -> anyhow::Result<()> {
    let engine = make_engine();
    let script = r#"
        format([
            #{ Foreground: #{ AnsiColor: "Red" } },
            #{ Text: "colored" },
        ])
    "#;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let expected = termwiz_funcs::format_as_escapes(vec![
        termwiz_funcs::FormatItem::Foreground(termwiz_funcs::FormatColor::AnsiColor(
            termwiz::color::AnsiColor::Red,
        )),
        termwiz_funcs::FormatItem::Text("colored".to_string()),
    ])?;
    assert_eq!(result, expected);
    Ok(())
}

#[test]
fn rhai_format_rejects_malformed_item() {
    let engine = make_engine();
    let result: Result<String, _> = engine.eval(r#"format([#{ NotARealVariant: "x" }])"#);
    assert!(result.is_err(), "an unrecognized FormatItem shape should be rejected");
}

#[test]
fn rhai_permute_any_mods_excludes_none_and_matches_count() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: rhai::Array = engine
        .eval(r#"permute_any_mods(#{ key: "a" })"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    // 2^4 combinations minus the all-NONE combination.
    assert_eq!(result.len(), 15);

    for entry in &result {
        let map = entry.clone().try_cast::<rhai::Map>().expect("entry should be a map");
        assert_eq!(map.get("key").unwrap().clone().into_string().unwrap(), "a");
        assert!(map.contains_key("mods"));
        let mods = map.get("mods").unwrap().clone().into_string().unwrap();
        assert_ne!(mods, wezterm_input_types::Modifiers::NONE.to_string());
    }
    Ok(())
}

#[test]
fn rhai_permute_any_or_no_mods_includes_none_and_matches_count() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: rhai::Array = engine
        .eval(r#"permute_any_or_no_mods(#{ key: "a" })"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    // Full 2^4 combinations, including all-NONE.
    assert_eq!(result.len(), 16);

    let none_str = wezterm_input_types::Modifiers::NONE.to_string();
    let mut saw_none = false;
    for entry in &result {
        let map = entry.clone().try_cast::<rhai::Map>().expect("entry should be a map");
        let mods = map.get("mods").unwrap().clone().into_string().unwrap();
        if mods == none_str {
            saw_none = true;
        }
    }
    assert!(saw_none, "expected the all-NONE combination to be present");
    Ok(())
}

#[test]
fn rhai_permute_mods_preserves_other_keys() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: rhai::Array = engine
        .eval(r#"permute_any_mods(#{ key: "x", action: "SendKey" })"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    for entry in &result {
        let map = entry.clone().try_cast::<rhai::Map>().expect("entry should be a map");
        assert_eq!(map.get("key").unwrap().clone().into_string().unwrap(), "x");
        assert_eq!(
            map.get("action").unwrap().clone().into_string().unwrap(),
            "SendKey"
        );
    }
    Ok(())
}
