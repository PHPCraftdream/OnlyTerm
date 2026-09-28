//! Tests for `pane_reports_password_input` (see `mod.rs`), split into its
//! own file to keep `mod.rs` at or below the repo's 1000-line-per-file cap.
use super::*;
use onlyterm_dynamic::Object;
use std::collections::BTreeMap;

fn object_value(entries: &[(&str, Value)]) -> Value {
    let mut map: BTreeMap<Value, Value> = BTreeMap::new();
    for (k, v) in entries {
        map.insert(Value::String((*k).to_string()), v.clone());
    }
    Value::Object(Object::from(map))
}

#[test]
fn true_when_metadata_declares_password_input_true() {
    let metadata = object_value(&[("password_input", Value::Bool(true))]);
    assert!(pane_reports_password_input(&metadata));
}

#[test]
fn false_when_metadata_declares_password_input_false() {
    let metadata = object_value(&[("password_input", Value::Bool(false))]);
    assert!(!pane_reports_password_input(&metadata));
}

#[test]
fn false_when_key_is_absent() {
    // `LocalPane::get_metadata` always returns an empty object.
    let metadata = object_value(&[]);
    assert!(!pane_reports_password_input(&metadata));
}

#[test]
fn false_when_value_is_not_an_object() {
    assert!(!pane_reports_password_input(&Value::Null));
}

#[test]
fn false_when_key_has_the_wrong_type() {
    let metadata = object_value(&[("password_input", Value::String("true".to_string()))]);
    assert!(!pane_reports_password_input(&metadata));
}
