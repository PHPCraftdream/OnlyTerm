use serde_json::Value as JValue;

// ---------------------------------------------------------------------------
// Rhai port of the serde functions (see
// docs/plans/2026-07-23-lua-rhai-migration.md).
//
// Uses rhai's own serde integration (`rhai::serde`) instead of hand-rolling a
// recursive value-tree walk: with the `serde` cargo feature enabled on `rhai`,
// `rhai::serde::to_dynamic`/`from_dynamic` serialize/deserialize through
// `Dynamic` exactly the way `serde_json` serializes through `JValue`, so any
// `Serialize`/`Deserialize` type (which `serde_json::Value` already is) converts
// to/from `rhai::Dynamic` for free.
//   * *_encode(value: Dynamic) -> String: `from_dynamic::<JValue>` turns the
//     incoming rhai value into a `serde_json::Value`, then the same
//     `serde_json`/`serde_yaml`/`toml` serializer.
//   * *_decode(text: String) -> Dynamic: the same deserializers parse into a
//     `serde_json::Value`, then `to_dynamic` turns that into the `Dynamic`
//     handed back to the calling `.rhai` script.
// ---------------------------------------------------------------------------
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    let mut serde_module = rhai::Module::new();

    // Decoders:
    serde_module.set_native_fn("json_decode", json_decode_rhai);
    serde_module.set_native_fn("yaml_decode", yaml_decode_rhai);
    serde_module.set_native_fn("toml_decode", toml_decode_rhai);

    // Encoders:
    serde_module.set_native_fn("json_encode", json_encode_rhai);
    serde_module.set_native_fn("yaml_encode", yaml_encode_rhai);
    serde_module.set_native_fn("toml_encode", toml_encode_rhai);
    // Pretty ones:
    serde_module.set_native_fn("json_encode_pretty", json_encode_pretty_rhai);
    serde_module.set_native_fn("toml_encode_pretty", toml_encode_pretty_rhai);
    // Note there is no pretty encoder for yaml, because the default one is
    // pretty already. See https://github.com/dtolnay/serde-yaml/issues/226

    engine.register_static_module("serde", serde_module.into());

    // For backward compatibility, mirroring `wezterm.json_parse`/
    // `wezterm.json_encode` registered directly as top-level functions.
    engine.register_fn("json_parse", json_decode_rhai);
    engine.register_fn("json_encode", json_encode_rhai);

    Ok(())
}

fn to_eval_err(err: impl std::fmt::Display) -> Box<rhai::EvalAltResult> {
    format!("{err:#}").into()
}

fn json_encode_rhai(value: rhai::Dynamic) -> Result<String, Box<rhai::EvalAltResult>> {
    let json: JValue = rhai::serde::from_dynamic(&value)?;
    serde_json::to_string(&json).map_err(to_eval_err)
}

fn json_encode_pretty_rhai(value: rhai::Dynamic) -> Result<String, Box<rhai::EvalAltResult>> {
    let json: JValue = rhai::serde::from_dynamic(&value)?;
    serde_json::to_string_pretty(&json).map_err(to_eval_err)
}

fn yaml_encode_rhai(value: rhai::Dynamic) -> Result<String, Box<rhai::EvalAltResult>> {
    let json: JValue = rhai::serde::from_dynamic(&value)?;
    serde_yaml::to_string(&json).map_err(to_eval_err)
}

fn toml_encode_rhai(value: rhai::Dynamic) -> Result<String, Box<rhai::EvalAltResult>> {
    let json: JValue = rhai::serde::from_dynamic(&value)?;
    toml::to_string(&json).map_err(to_eval_err)
}

fn toml_encode_pretty_rhai(value: rhai::Dynamic) -> Result<String, Box<rhai::EvalAltResult>> {
    let json: JValue = rhai::serde::from_dynamic(&value)?;
    toml::to_string_pretty(&json).map_err(to_eval_err)
}

fn json_decode_rhai(text: String) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    let value: JValue = serde_json::from_str(&text).map_err(to_eval_err)?;
    rhai::serde::to_dynamic(value)
}

fn yaml_decode_rhai(text: String) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    let value: JValue = serde_yaml::from_str(&text).map_err(to_eval_err)?;
    rhai::serde::to_dynamic(value)
}

fn toml_decode_rhai(text: String) -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
    let value: JValue = toml::from_str(&text).map_err(to_eval_err)?;
    rhai::serde::to_dynamic(value)
}
