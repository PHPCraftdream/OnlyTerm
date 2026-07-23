//! L0 tooling smoke-test for the Lua -> rhai config migration.
//!
//! See `docs/plans/2026-07-23-lua-rhai-migration.md` for the overall plan. This test
//! does NOT wire rhai into the real config pipeline (that is L1 - "the Dynamic
//! bridge" - and L2 - "the engine entry point"). Its only job is to prove, before any
//! real porting work starts, that:
//!
//!   1. `rhai` actually builds and runs inside this workspace as a plain dev-dependency
//!      of `config` (pure Rust, no vendored C, unlike `mlua` - see config/Cargo.toml).
//!   2. A `.rhai` script using an object-map literal (the natural rhai analogue of a
//!      Lua table used as a config value) parses into `rhai::Dynamic`.
//!   3. A hand-written, deliberately partial conversion from `rhai::Dynamic` into
//!      `wezterm_dynamic::Value` (the crate's existing engine-agnostic value type,
//!      see wezterm-dynamic/src/value.rs) is possible using only public APIs on both
//!      sides.
//!
//! What this test intentionally does NOT do (left for L1):
//!   - A complete, generic `rhai::Dynamic -> wezterm_dynamic::Value` conversion
//!     covering every rhai type (timestamps, blobs, function pointers, custom
//!     types, shared/locked values behind `Dynamic::from_ref`, etc).
//!   - Hooking this conversion into `FromDynamic`/`ToDynamic` derive so that
//!     `Config`, `SshDomain`-style structs, etc. can be built from it.
//!   - Any replacement of `impl_lua_conversion_dynamic!` (config/src/*.rs).
//!
//! The conversion below covers exactly the value shapes that appear in the sample
//! script: unit, bool, integer, float, string, array, and nested object map. That
//! is the "contour" L1 needs to fill in generically.

use rhai::{Dynamic, Engine, Map};
use wezterm_dynamic::Value as DynValue;

/// Sketch of the future `rhai::Dynamic -> wezterm_dynamic::Value` bridge.
///
/// L1 will need to turn this into a proper, total, generic conversion (probably
/// living next to `luahelper::lua_value_to_dynamic`, e.g. as
/// `rhai_value_to_dynamic` in a small helper crate/module mirroring `luahelper`).
/// Here we only handle the handful of `rhai` variants that a plain object-map
/// literal can produce, using rhai's public `is_*`/`as_*`/`try_cast` accessors -
/// deliberately not exhaustive.
fn rhai_dynamic_to_wezterm_dynamic(value: &Dynamic) -> DynValue {
    if value.is_unit() {
        return DynValue::Null;
    }
    if let Some(b) = value.clone().try_cast::<bool>() {
        return DynValue::Bool(b);
    }
    if let Some(i) = value.clone().try_cast::<rhai::INT>() {
        return DynValue::I64(i as i64);
    }
    if let Some(f) = value.clone().try_cast::<rhai::FLOAT>() {
        return DynValue::F64(ordered_float::OrderedFloat(f as f64));
    }
    if let Some(s) = value.clone().try_cast::<rhai::ImmutableString>() {
        return DynValue::String(s.to_string());
    }
    if value.is_array() {
        let arr = value
            .clone()
            .into_array()
            .expect("checked is_array() above");
        let converted: Vec<DynValue> = arr.iter().map(rhai_dynamic_to_wezterm_dynamic).collect();
        return DynValue::Array(converted.into());
    }
    if value.is_map() {
        let map = value.clone().try_cast::<Map>().expect("checked is_map()");
        let converted: std::collections::BTreeMap<DynValue, DynValue> = map
            .iter()
            .map(|(k, v)| {
                (
                    DynValue::String(k.to_string()),
                    rhai_dynamic_to_wezterm_dynamic(v),
                )
            })
            .collect();
        return DynValue::Object(converted.into());
    }

    panic!(
        "rhai_dynamic_to_wezterm_dynamic: unhandled rhai type `{}` - this is exactly \
         the kind of gap L1 must close with a total conversion",
        value.type_name()
    );
}

#[test]
fn rhai_engine_builds_and_evaluates_object_map_literal() -> anyhow::Result<()> {
    // Proves point (1) and (2): rhai actually builds in this workspace and can
    // evaluate an object-map literal script - the rhai analogue of a Lua table
    // used as config data - into a `rhai::Dynamic`.
    let engine = Engine::new();

    let script = r#"
        #{
            prop1: "value",
            nested: #{
                x: 1,
                y: 2,
            },
        }
    "#;

    let result: Dynamic = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(result.is_map(), "expected top-level object map literal");

    let map = result.clone().try_cast::<Map>().expect("is_map() checked");
    assert_eq!(map.len(), 2);
    assert_eq!(
        map.get("prop1")
            .and_then(|v| v.clone().try_cast::<rhai::ImmutableString>())
            .as_deref(),
        Some("value")
    );

    Ok(())
}

#[test]
fn rhai_dynamic_bridges_to_wezterm_dynamic_for_simple_values() -> anyhow::Result<()> {
    // Proves point (3): a hand-rolled, partial bridge from rhai::Dynamic into
    // wezterm_dynamic::Value (the type the rest of `config` already speaks via
    // FromDynamic/ToDynamic) is mechanically straightforward for the common
    // scalar/array/object shapes. This is the "contour" of the real L1 bridge,
    // not the bridge itself.
    let engine = Engine::new();

    let script = r#"
        #{
            prop1: "value",
            enabled: true,
            count: 42,
            ratio: 1.5,
            nested: #{
                x: 1,
                y: 2,
            },
            items: [1, 2, 3],
        }
    "#;

    let rhai_value: Dynamic = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let converted = rhai_dynamic_to_wezterm_dynamic(&rhai_value);

    let obj = match &converted {
        DynValue::Object(obj) => obj,
        other => panic!("expected DynValue::Object, got {:?}", other),
    };

    assert_eq!(
        obj.get_by_str("prop1"),
        Some(&DynValue::String("value".to_string()))
    );
    assert_eq!(obj.get_by_str("enabled"), Some(&DynValue::Bool(true)));
    assert_eq!(obj.get_by_str("count"), Some(&DynValue::I64(42)));
    assert_eq!(
        obj.get_by_str("ratio"),
        Some(&DynValue::F64(ordered_float::OrderedFloat(1.5)))
    );

    match obj.get_by_str("nested") {
        Some(DynValue::Object(nested)) => {
            assert_eq!(nested.get_by_str("x"), Some(&DynValue::I64(1)));
            assert_eq!(nested.get_by_str("y"), Some(&DynValue::I64(2)));
        }
        other => panic!("expected nested DynValue::Object, got {:?}", other),
    }

    match obj.get_by_str("items") {
        Some(DynValue::Array(items)) => {
            let items: Vec<&DynValue> = items.iter().collect();
            assert_eq!(
                items,
                vec![&DynValue::I64(1), &DynValue::I64(2), &DynValue::I64(3)]
            );
        }
        other => panic!("expected DynValue::Array, got {:?}", other),
    }

    Ok(())
}
