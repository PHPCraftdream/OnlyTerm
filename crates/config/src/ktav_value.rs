//! Bridge between `ktav::value::Value` (the dynamic value type produced by
//! parsing a `.ktav` config document) and `onlyterm_dynamic::Value`, the
//! engine-agnostic value type that `Config` and friends speak via
//! `FromDynamic`/`ToDynamic` (see `crate::config::Config`).
//!
//! This plays the same role for the ktav config-loading path (task #275 of
//! the rhai-removal sequence) that `rhai_value::rhai_dynamic_to_dynamic`
//! played for the (now superseded) rhai config-loading path: a total,
//! generic conversion from the config-format engine's own dynamic value type
//! into `onlyterm_dynamic::Value`, so that `Config::from_dynamic` can build a
//! `Config` directly from a parsed `.ktav` document without `Config` (or any
//! of the many types it references) needing to derive `serde::Deserialize`.
//!
//! ## Why not `ktav::from_str::<Config>` directly?
//!
//! `ktav::from_str<T: serde::de::DeserializeOwned>` is the crate's preferred
//! entry point, and would be the most direct path *if* `Config` derived
//! `serde::Deserialize`. It doesn't -- `Config` (and the large tree of types
//! it's built from: `color.rs`, `font.rs`, `keyassignment.rs`, etc.) derives
//! `onlyterm_dynamic::{FromDynamic, ToDynamic}` instead, which is a distinct,
//! independently-hand-rolled derive (see `onlyterm-dynamic-derive`) with its
//! own attribute surface (`#[dynamic(default)]`, rename rules, deprecated
//! fields, "deny unknown fields" mode, etc.) that config-loading elsewhere
//! depends on. Retrofitting `serde::Deserialize` onto that whole type tree
//! is out of scope for this task (see the task's own file-touch boundary),
//! so instead this module goes through ktav's dynamic `Value` (via
//! `ktav::parse`) and bridges it to `onlyterm_dynamic::Value`, exactly the
//! way the rhai path bridged `rhai::Dynamic`.
//!
//! ## Coverage
//!
//! `ktav::value::Value` has exactly the variants documented at
//! <https://docs.rs/ktav/0.6.1/ktav/value/value/enum.Value.html>; the
//! mapping below is total (this conversion never fails):
//!
//! | ktav variant           | onlyterm_dynamic::Value                    |
//! |-------------------------|--------------------------------------------|
//! | `Null`                   | `Value::Null`                              |
//! | `Bool`                   | `Value::Bool`                              |
//! | `Integer` (canonical text)| `Value::I64` (parsed) or `Value::String` (fallback if it doesn't fit an `i64`) |
//! | `Float` (canonical text)  | `Value::F64` (parsed) or `Value::String` (fallback, should not happen for ktav's own canonical `ryu` output) |
//! | `String`                  | `Value::String`                            |
//! | `Array`                   | `Value::Array`, elementwise recursive       |
//! | `Object`                  | `Value::Object`, keys converted to `Value::String` |

use ktav::value::Value as KtavValue;
use onlyterm_dynamic::Value as DynValue;

/// Convert a `ktav::value::Value` (the result of `ktav::parse`-ing a `.ktav`
/// config document) into a `onlyterm_dynamic::Value`.
///
/// This is total: every `ktav::value::Value` variant has an unambiguous
/// `onlyterm_dynamic::Value` representation.
pub fn ktav_value_to_dynamic(value: &KtavValue) -> DynValue {
    match value {
        KtavValue::Null => DynValue::Null,
        KtavValue::Bool(b) => DynValue::Bool(*b),
        KtavValue::Integer(s) => {
            // ktav stores the canonical base-10 decimal form as text; parse
            // it into an i64 for a native numeric `Value`. Fall back to a
            // string on the (should-not-happen-for-a-canonical-integer)
            // chance it doesn't fit, rather than panicking or losing data.
            match s.parse::<i64>() {
                Ok(i) => DynValue::I64(i),
                Err(_) => DynValue::String(s.to_string()),
            }
        }
        KtavValue::Float(s) => match s.parse::<f64>() {
            Ok(f) => DynValue::F64(ordered_float::OrderedFloat(f)),
            Err(_) => DynValue::String(s.to_string()),
        },
        KtavValue::String(s) => DynValue::String(s.to_string()),
        KtavValue::Array(items) => {
            let converted: Vec<DynValue> = items.iter().map(ktav_value_to_dynamic).collect();
            DynValue::Array(converted.into())
        }
        KtavValue::Object(obj) => {
            let mut converted = std::collections::BTreeMap::new();
            for (key, val) in obj.iter() {
                converted.insert(
                    DynValue::String(key.to_string()),
                    ktav_value_to_dynamic(val),
                );
            }
            DynValue::Object(converted.into())
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn parse(src: &str) -> KtavValue {
        ktav::parse(src).expect("ktav source should parse")
    }

    #[test]
    fn null_and_bool() {
        assert_eq!(
            ktav_value_to_dynamic(&parse("a: null\n")),
            DynValue::Object(
                vec![(DynValue::String("a".to_string()), DynValue::Null)]
                    .into_iter()
                    .collect()
            )
        );
        let v = parse("a: true\nb: false\n");
        match ktav_value_to_dynamic(&v) {
            DynValue::Object(obj) => {
                assert_eq!(obj.get_by_str("a"), Some(&DynValue::Bool(true)));
                assert_eq!(obj.get_by_str("b"), Some(&DynValue::Bool(false)));
            }
            other => panic!("expected object, got {:?}", other),
        }
    }

    #[test]
    fn integers_and_floats() {
        let v = parse("port: 8080\nratio: 3.5\nneg: -7\n");
        match ktav_value_to_dynamic(&v) {
            DynValue::Object(obj) => {
                assert_eq!(obj.get_by_str("port"), Some(&DynValue::I64(8080)));
                assert_eq!(
                    obj.get_by_str("ratio"),
                    Some(&DynValue::F64(ordered_float::OrderedFloat(3.5)))
                );
                assert_eq!(obj.get_by_str("neg"), Some(&DynValue::I64(-7)));
            }
            other => panic!("expected object, got {:?}", other),
        }
    }

    #[test]
    fn strings() {
        let v = parse("name: onlyterm\n");
        match ktav_value_to_dynamic(&v) {
            DynValue::Object(obj) => {
                assert_eq!(
                    obj.get_by_str("name"),
                    Some(&DynValue::String("onlyterm".to_string()))
                );
            }
            other => panic!("expected object, got {:?}", other),
        }
    }

    #[test]
    fn arrays_and_nested_objects() {
        let v = parse(
            "tags: [\n    prod\n    eu\n]\ndb: {\n    host: primary.internal\n    timeout: 30\n}\n",
        );
        match ktav_value_to_dynamic(&v) {
            DynValue::Object(obj) => {
                match obj.get_by_str("tags") {
                    Some(DynValue::Array(items)) => {
                        let items: Vec<&DynValue> = items.iter().collect();
                        assert_eq!(
                            items,
                            vec![
                                &DynValue::String("prod".to_string()),
                                &DynValue::String("eu".to_string())
                            ]
                        );
                    }
                    other => panic!("expected array, got {:?}", other),
                }
                match obj.get_by_str("db") {
                    Some(DynValue::Object(inner)) => {
                        assert_eq!(
                            inner.get_by_str("host"),
                            Some(&DynValue::String("primary.internal".to_string()))
                        );
                        assert_eq!(inner.get_by_str("timeout"), Some(&DynValue::I64(30)));
                    }
                    other => panic!("expected nested object, got {:?}", other),
                }
            }
            other => panic!("expected object, got {:?}", other),
        }
    }
}
