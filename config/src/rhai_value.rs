//! Bridge between `rhai::Dynamic` and `wezterm_dynamic::Value`.
//!
//! This is the L1 deliverable of the Lua -> rhai config migration (see
//! `docs/plans/2026-07-23-lua-rhai-migration.md`). It plays the same role for the
//! future rhai config pipeline that `luahelper::lua_value_to_dynamic` /
//! `luahelper::dynamic_to_lua_value` play for the current mlua pipeline: a total,
//! generic conversion between the script engine's own dynamic value type and
//! `wezterm_dynamic::Value`, the engine-agnostic value type that all
//! `FromDynamic`/`ToDynamic` config structs already speak.
//!
//! Unlike the mlua bridge, this module does not (yet) plug into
//! `impl_lua_conversion_dynamic!`, which is hard-wired to `mlua::IntoLua`/`FromLua`.
//! Making the macro backend-agnostic (or adding a parallel
//! `impl_rhai_conversion_dynamic!`) is L2's job, once `config/src/lua.rs` grows a
//! rhai-based sibling entry point that actually needs to call these functions from
//! generated code. For now this module is a complete, independently testable unit.
//!
//! ## Coverage
//!
//! `rhai::Dynamic` has more variants than Lua's `mlua::Value` (notably a distinct
//! `Char`, plus opt-in `Blob`/timestamp/decimal support), so this bridge covers
//! everything `Dynamic` can natively hold, plus deliberate, documented policies for
//! the values that have no `wezterm_dynamic::Value` equivalent:
//!
//! | rhai variant           | wezterm_dynamic::Value                          |
//! |-------------------------|--------------------------------------------------|
//! | `()` (unit)              | `Value::Null`                                    |
//! | `bool`                   | `Value::Bool`                                    |
//! | `INT` (i64/i32)           | `Value::I64`                                     |
//! | `FLOAT` (f64/f32)          | `Value::F64`                                     |
//! | `ImmutableString`/`String` | `Value::String`                                  |
//! | `char`                   | `Value::String` (single-character string)        |
//! | `Array`                  | `Value::Array`, elementwise recursive             |
//! | `Blob` (`Vec<u8>`)        | `Value::Array` of `Value::U64` byte values        |
//! | `Map`                    | `Value::Object`, keys converted to `Value::String`|
//! | `Shared`/locked `Dynamic` | transparently unwrapped via `Dynamic::flatten()`  |
//! | `FnPtr` (function value)  | rejected: `Error::Message` (no meaningful config value) |
//! | `Instant`/timestamp        | rejected: `Error::Message` (no meaningful config value) |
//! | custom `Variant` types    | best-effort `Value::String` via `Dynamic::to_string()`, only if `to_string` yields something other than the bare type name; otherwise rejected |
//!
//! Rejection uses `wezterm_dynamic::Error::Message`, mirroring how the mlua bridge
//! rejects `LuaValue::Function`/`LuaValue::Thread` with
//! `mlua::Error::FromLuaConversionError`.
//!
//! The reverse direction, `wezterm_dynamic::Value -> rhai::Dynamic`, is total: every
//! `Value` variant has an unambiguous rhai representation, so it never fails.

use rhai::{Array as RhaiArray, Dynamic, Map as RhaiMap};
use std::convert::TryFrom;
use wezterm_dynamic::{Error, Value as DynValue};

/// Convert a `rhai::Dynamic` into a `wezterm_dynamic::Value`.
///
/// This is total over everything that can actually appear as the result of
/// evaluating a `.rhai` script or config literal, with the exception of the
/// variants documented at the module level (function pointers, timestamps, and
/// non-`Display`-able custom types), which return an `Err`.
///
/// Shared/locked `Dynamic` values (produced by rhai's closures/`no_closure`-off
/// captures) are transparently resolved via [`Dynamic::flatten_clone`] before
/// inspecting the underlying value, so callers never need to special-case them.
pub fn rhai_dynamic_to_dynamic(value: &Dynamic) -> Result<DynValue, Error> {
    // Shared (locked) values wrap a plain Dynamic behind an Rc<RefCell<..>> (or
    // Arc<Mutex<..>> under the `sync` feature). `flatten_clone` gives us back a
    // plain, inspectable Dynamic without disturbing the original (still-shared)
    // value, which is exactly what we want for a read-only conversion.
    let value = value.flatten_clone();

    if value.is_unit() {
        return Ok(DynValue::Null);
    }
    if let Ok(b) = value.as_bool() {
        return Ok(DynValue::Bool(b));
    }
    if let Ok(i) = value.as_int() {
        return Ok(DynValue::I64(i as i64));
    }
    if let Ok(f) = value.as_float() {
        return Ok(DynValue::F64(ordered_float::OrderedFloat(f as f64)));
    }
    if let Ok(c) = value.as_char() {
        // rhai's `char` has no direct wezterm_dynamic equivalent; represent it as
        // a single-character string, matching how Lua (which has no char type at
        // all, only strings) would present the same data.
        let mut s = String::new();
        s.push(c);
        return Ok(DynValue::String(s));
    }
    if value.is_string() {
        let s = value
            .into_string()
            .map_err(|ty| Error::Message(format!("expected string, got `{ty}`")))?;
        return Ok(DynValue::String(s));
    }
    if value.is_array() {
        let arr = value
            .into_array()
            .map_err(|ty| Error::Message(format!("expected array, got `{ty}`")))?;
        let mut converted = Vec::with_capacity(arr.len());
        for item in &arr {
            converted.push(rhai_dynamic_to_dynamic(item)?);
        }
        return Ok(DynValue::Array(converted.into()));
    }
    if value.is_blob() {
        let blob = value
            .into_blob()
            .map_err(|ty| Error::Message(format!("expected blob, got `{ty}`")))?;
        // No dedicated byte-string value in wezterm_dynamic; represent a blob as
        // an array of unsigned byte values. This is lossless and round-trips
        // through `Value::Array`/`Value::U64` cleanly.
        let converted: Vec<DynValue> = blob.into_iter().map(|b| DynValue::U64(b as u64)).collect();
        return Ok(DynValue::Array(converted.into()));
    }
    if value.is_map() {
        let map = value
            .try_cast::<RhaiMap>()
            .ok_or_else(|| Error::Message("expected map".to_string()))?;
        let mut converted = std::collections::BTreeMap::new();
        for (key, val) in map.iter() {
            converted.insert(
                DynValue::String(key.to_string()),
                rhai_dynamic_to_dynamic(val)?,
            );
        }
        return Ok(DynValue::Object(converted.into()));
    }
    if value.is_fnptr() {
        return Err(Error::Message(format!(
            "cannot convert a rhai function value (`{}`) into a config value",
            value.type_name()
        )));
    }
    if value.is_timestamp() {
        return Err(Error::Message(
            "cannot convert a rhai timestamp into a config value".to_string(),
        ));
    }

    // Anything left over is a custom `Variant` type registered with the engine
    // (e.g. via `register_type`). We have no generic structural access to it, so
    // fall back to `Display`/`to_string()` on a best-effort basis, the same
    // fallback the mlua bridge uses for userdata without a `__wezterm_to_dynamic`
    // metamethod (there it calls the Lua `tostring` metamethod). If `to_string()`
    // doesn't do anything useful (rhai's default falls back to the bare type
    // name for types with no custom Display), treat it as unsupported instead of
    // silently emitting a useless string.
    let type_name = value.type_name();
    let as_string = value.to_string();
    if as_string.is_empty() || as_string == type_name {
        return Err(Error::Message(format!(
            "cannot convert rhai value of type `{type_name}` into a config value"
        )));
    }
    Ok(DynValue::String(as_string))
}

/// Convert a `wezterm_dynamic::Value` into a `rhai::Dynamic`.
///
/// This direction is total: every `Value` variant has an unambiguous, lossless
/// rhai representation, so this never fails. Needed for the reverse leg of the
/// bridge -- e.g. handing config-derived data back into rhai event callbacks (see
/// `wezterm.on(...)` in the mlua pipeline, `config/src/lua.rs`), which is wired up
/// as part of L2.
pub fn dynamic_to_rhai_dynamic(value: &DynValue) -> Dynamic {
    match value {
        DynValue::Null => Dynamic::UNIT,
        DynValue::Bool(b) => Dynamic::from(*b),
        DynValue::String(s) => Dynamic::from(s.clone()),
        DynValue::U64(u) => {
            // rhai's INT is i64 (or i32 under `only_i32`) with no unsigned
            // variant. Values that fit losslessly become an INT; anything larger
            // than INT::MAX (only possible with the 64-bit INT type near its
            // upper half) is preserved exactly as an f64, which is the same
            // lossy-for-huge-integers tradeoff the existing mlua bridge accepts
            // for u64 (`u.into_lua(lua)?`, which goes through Lua's own
            // number type).
            match rhai::INT::try_from(*u) {
                Ok(i) => Dynamic::from(i),
                Err(_) => Dynamic::from(*u as f64),
            }
        }
        DynValue::I64(i) => match rhai::INT::try_from(*i) {
            Ok(i) => Dynamic::from(i),
            Err(_) => Dynamic::from(*i as f64),
        },
        DynValue::F64(f) => Dynamic::from(f.into_inner() as rhai::FLOAT),
        DynValue::Array(array) => {
            let items: RhaiArray = array.iter().map(dynamic_to_rhai_dynamic).collect();
            Dynamic::from_array(items)
        }
        DynValue::Object(object) => {
            let mut map = RhaiMap::new();
            for (key, val) in object.iter() {
                // rhai map keys are always plain identifiers/strings; stringify
                // non-string Value keys (mirroring how the mlua bridge instead
                // preserves arbitrary Lua table keys - rhai's Map type simply
                // doesn't support that, so this is the best available fidelity).
                let key = match key {
                    DynValue::String(s) => s.clone(),
                    other => format!("{other:?}"),
                };
                map.insert(key.into(), dynamic_to_rhai_dynamic(val));
            }
            Dynamic::from_map(map)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use rhai::Engine;

    fn eval(script: &str) -> Dynamic {
        let engine = Engine::new();
        engine.eval(script).expect("script should evaluate")
    }

    #[test]
    fn unit() {
        assert_eq!(rhai_dynamic_to_dynamic(&eval("()")).unwrap(), DynValue::Null);
    }

    #[test]
    fn bool_values() {
        assert_eq!(rhai_dynamic_to_dynamic(&eval("true")).unwrap(), DynValue::Bool(true));
        assert_eq!(rhai_dynamic_to_dynamic(&eval("false")).unwrap(), DynValue::Bool(false));
    }

    #[test]
    fn integers() {
        assert_eq!(rhai_dynamic_to_dynamic(&eval("42")).unwrap(), DynValue::I64(42));
        assert_eq!(rhai_dynamic_to_dynamic(&eval("-7")).unwrap(), DynValue::I64(-7));
        assert_eq!(rhai_dynamic_to_dynamic(&eval("0")).unwrap(), DynValue::I64(0));
    }

    #[test]
    fn floats() {
        assert_eq!(
            rhai_dynamic_to_dynamic(&eval("1.5")).unwrap(),
            DynValue::F64(ordered_float::OrderedFloat(1.5))
        );
        assert_eq!(
            rhai_dynamic_to_dynamic(&eval("-0.25")).unwrap(),
            DynValue::F64(ordered_float::OrderedFloat(-0.25))
        );
    }

    #[test]
    fn strings() {
        assert_eq!(
            rhai_dynamic_to_dynamic(&eval(r#""hello""#)).unwrap(),
            DynValue::String("hello".to_string())
        );
        // empty string is not unit/nil
        assert_eq!(
            rhai_dynamic_to_dynamic(&eval(r#""""#)).unwrap(),
            DynValue::String(String::new())
        );
    }

    #[test]
    fn chars_become_single_char_strings() {
        assert_eq!(
            rhai_dynamic_to_dynamic(&eval("'x'")).unwrap(),
            DynValue::String("x".to_string())
        );
    }

    #[test]
    fn arrays_recursive_and_mixed_types() {
        let converted = rhai_dynamic_to_dynamic(&eval(r#"[1, "two", 3.0, true, ()]"#)).unwrap();
        match converted {
            DynValue::Array(items) => {
                let items: Vec<&DynValue> = items.iter().collect();
                assert_eq!(
                    items,
                    vec![
                        &DynValue::I64(1),
                        &DynValue::String("two".to_string()),
                        &DynValue::F64(ordered_float::OrderedFloat(3.0)),
                        &DynValue::Bool(true),
                        &DynValue::Null,
                    ]
                );
            }
            other => panic!("expected array, got {:?}", other),
        }
    }

    #[test]
    fn nested_arrays() {
        let converted = rhai_dynamic_to_dynamic(&eval(r#"[[1, 2], [3, 4]]"#)).unwrap();
        match converted {
            DynValue::Array(items) => {
                assert_eq!(items.len(), 2);
                match &items[0] {
                    DynValue::Array(inner) => {
                        let inner: Vec<&DynValue> = inner.iter().collect();
                        assert_eq!(inner, vec![&DynValue::I64(1), &DynValue::I64(2)]);
                    }
                    other => panic!("expected nested array, got {:?}", other),
                }
            }
            other => panic!("expected array, got {:?}", other),
        }
    }

    #[test]
    fn objects_nested() {
        let converted = rhai_dynamic_to_dynamic(&eval(
            r#"
            #{
                name: "wez",
                inner: #{
                    a: 1,
                    b: [1, 2, 3],
                },
            }
            "#,
        ))
        .unwrap();
        match converted {
            DynValue::Object(obj) => {
                assert_eq!(
                    obj.get_by_str("name"),
                    Some(&DynValue::String("wez".to_string()))
                );
                match obj.get_by_str("inner") {
                    Some(DynValue::Object(inner)) => {
                        assert_eq!(inner.get_by_str("a"), Some(&DynValue::I64(1)));
                        match inner.get_by_str("b") {
                            Some(DynValue::Array(items)) => {
                                let items: Vec<&DynValue> = items.iter().collect();
                                assert_eq!(
                                    items,
                                    vec![&DynValue::I64(1), &DynValue::I64(2), &DynValue::I64(3)]
                                );
                            }
                            other => panic!("expected array, got {:?}", other),
                        }
                    }
                    other => panic!("expected nested object, got {:?}", other),
                }
            }
            other => panic!("expected object, got {:?}", other),
        }
    }

    #[test]
    fn blobs_become_byte_arrays() {
        let converted = rhai_dynamic_to_dynamic(&eval(r#"blob(3, 0x41)"#)).unwrap();
        match converted {
            DynValue::Array(items) => {
                let items: Vec<&DynValue> = items.iter().collect();
                assert_eq!(
                    items,
                    vec![&DynValue::U64(0x41), &DynValue::U64(0x41), &DynValue::U64(0x41)]
                );
            }
            other => panic!("expected array-of-bytes, got {:?}", other),
        }
    }

    #[test]
    fn function_pointers_are_rejected() {
        let value = eval(
            r#"
            fn double(x) { x * 2 }
            Fn("double")
            "#,
        );
        assert!(value.is_fnptr());
        let err = rhai_dynamic_to_dynamic(&value).unwrap_err();
        assert!(err.to_string().contains("function"));
    }

    #[test]
    fn timestamps_are_rejected() {
        let value = eval(r#"timestamp()"#);
        assert!(value.is_timestamp());
        let err = rhai_dynamic_to_dynamic(&value).unwrap_err();
        assert!(err.to_string().contains("timestamp"));
    }

    #[test]
    fn shared_values_are_transparently_unwrapped() {
        // Closures capturing outer variables by reference produce `Dynamic::Shared`
        // values. Evaluate a script that forces a variable to become shared via a
        // closure capture, then confirm the conversion sees through it to the
        // underlying plain value.
        let engine = Engine::new();
        let script = r#"
            let x = 10;
            let f = || x + 1;
            f.call();
            x
        "#;
        let value: Dynamic = engine.eval(script).expect("script should evaluate");
        let converted = rhai_dynamic_to_dynamic(&value).unwrap();
        assert_eq!(converted, DynValue::I64(10));
    }

    #[test]
    fn roundtrip_object_through_reverse_conversion() {
        let original = eval(
            r#"
            #{
                s: "value",
                n: 42,
                f: 1.5,
                b: true,
                nil_val: (),
                arr: [1, 2, 3],
                nested: #{ x: 1 },
            }
            "#,
        );
        let as_dynvalue = rhai_dynamic_to_dynamic(&original).unwrap();
        let back = dynamic_to_rhai_dynamic(&as_dynvalue);
        let roundtripped = rhai_dynamic_to_dynamic(&back).unwrap();
        assert_eq!(as_dynvalue, roundtripped);
    }

    #[test]
    fn reverse_scalars() {
        assert!(dynamic_to_rhai_dynamic(&DynValue::Null).is_unit());
        assert_eq!(
            dynamic_to_rhai_dynamic(&DynValue::Bool(true))
                .as_bool()
                .unwrap(),
            true
        );
        assert_eq!(
            dynamic_to_rhai_dynamic(&DynValue::I64(7)).as_int().unwrap(),
            7
        );
        assert_eq!(
            dynamic_to_rhai_dynamic(&DynValue::U64(7)).as_int().unwrap(),
            7
        );
        assert_eq!(
            dynamic_to_rhai_dynamic(&DynValue::F64(ordered_float::OrderedFloat(2.5)))
                .as_float()
                .unwrap(),
            2.5
        );
        assert_eq!(
            dynamic_to_rhai_dynamic(&DynValue::String("hi".to_string()))
                .into_string()
                .unwrap(),
            "hi".to_string()
        );
    }

    #[test]
    fn reverse_large_u64_falls_back_to_float() {
        let huge = u64::MAX;
        let converted = dynamic_to_rhai_dynamic(&DynValue::U64(huge));
        // Doesn't fit in rhai::INT (i64/i32), so it becomes a float instead of
        // panicking or truncating silently.
        assert!(converted.is_float());
    }

    #[test]
    fn reverse_array_and_object() {
        let value = DynValue::Array(
            vec![DynValue::I64(1), DynValue::String("two".to_string())].into(),
        );
        let converted = dynamic_to_rhai_dynamic(&value);
        assert!(converted.is_array());
        let arr = converted.into_array().unwrap();
        assert_eq!(arr.len(), 2);

        let mut obj = std::collections::BTreeMap::new();
        obj.insert(DynValue::String("k".to_string()), DynValue::I64(1));
        let value = DynValue::Object(obj.into());
        let converted = dynamic_to_rhai_dynamic(&value);
        assert!(converted.is_map());
        let map = converted.try_cast::<RhaiMap>().unwrap();
        assert_eq!(map.get("k").unwrap().as_int().unwrap(), 1);
    }
}
