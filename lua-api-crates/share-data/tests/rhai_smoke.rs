//! Smoke test: `share_data::register_rhai` exposes a rhai equivalent of
//! `wezterm.GLOBAL` (see docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! The most important property this crate must preserve is that `GLOBAL` shares
//! data *between* different parts of the config/event pipeline: a value written
//! from one rhai engine instance must be visible from another, because there is
//! a single process-wide store underneath. The tests below assert that several
//! independently-constructed `rhai::Engine` instances (via `register_rhai`) all
//! observe the one shared store.
//!
//! Because the underlying storage (`GLOBALS`) is a single process-wide
//! `lazy_static`, tests that mutate it run serially (`#[test]` functions in
//! one binary already run in threads by default, so a lock-free shared
//! mutable static would make tests interfere with each other); each test
//! here uses a distinct top-level key (namespaced by the test name) to avoid
//! cross-test interference despite sharing the same global underneath.

use rhai::Engine;

fn make_rhai_engine() -> Engine {
    let mut engine = Engine::new();
    share_data::register_rhai(&mut engine).expect("register_rhai");
    engine
}

#[test]
fn rhai_global_data_set_and_get_round_trip() -> anyhow::Result<()> {
    let engine = make_rhai_engine();
    let script = r#"
        let g = global_data();
        g["rhai_round_trip_key"] = "hello from rhai";
        g["rhai_round_trip_key"]
    "#;
    let result: String = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result, "hello from rhai");
    Ok(())
}

#[test]
fn rhai_global_data_missing_key_is_unit() -> anyhow::Result<()> {
    let engine = make_rhai_engine();
    let result: rhai::Dynamic = engine
        .eval(r#"global_data()["this_key_was_never_set_rhai"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(result.is_unit());
    Ok(())
}

#[test]
fn rhai_global_data_supports_nested_values() -> anyhow::Result<()> {
    let engine = make_rhai_engine();
    let script = r#"
        let g = global_data();
        g["rhai_nested_key"] = #{ a: 1, b: [1, 2, 3], c: "text" };
        let back = g["rhai_nested_key"];
        [back.a, back.b.len(), back.c]
    "#;
    let result: rhai::Array = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result[0].as_int().unwrap(), 1);
    assert_eq!(result[1].as_int().unwrap(), 3);
    assert_eq!(result[2].clone().into_string().unwrap(), "text");
    Ok(())
}

/// Confirms the sharing survives *multiple independent engine instances*
/// simultaneously -- real usage may construct several `Engine` instances over a
/// process's lifetime (e.g. one per config reload), and all of them must still
/// observe the one shared store.
#[test]
fn multiple_engine_instances_all_share_the_same_store() -> anyhow::Result<()> {
    let engine_a = make_rhai_engine();
    let engine_b = make_rhai_engine();

    engine_a
        .run(r#"let g = global_data(); g["multi_engine_key"] = 1;"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    // Visible from a second, independently-constructed rhai engine.
    let via_engine_b: rhai::INT = engine_b
        .eval(r#"global_data()["multi_engine_key"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(via_engine_b, 1);

    // And a write from the second engine is visible from the first.
    engine_b
        .run(r#"let g = global_data(); g["multi_engine_key"] = 2;"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let via_engine_a: rhai::INT = engine_a
        .eval(r#"global_data()["multi_engine_key"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(via_engine_a, 2);

    Ok(())
}

#[test]
fn rhai_global_data_len_reflects_object_key_count() -> anyhow::Result<()> {
    let engine = make_rhai_engine();
    // `len` is a get-property on `GlobalData`, reflecting the *whole* shared
    // object's key count, so just assert it grows after inserting a
    // guaranteed-fresh key, rather than asserting an exact number (other
    // tests in this same binary share and mutate the same `GLOBALS` root
    // object concurrently).
    let before: rhai::INT = engine
        .eval(r#"global_data().len"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    engine
        .run(r#"let g = global_data(); g["len_probe_key_rhai"] = true;"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let after: rhai::INT = engine
        .eval(r#"global_data().len"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(after > before);
    Ok(())
}

#[test]
fn rhai_global_data_array_values_round_trip() -> anyhow::Result<()> {
    // `GlobalData["key"] = [...]` stores a whole array value under a top-level
    // key; reading it back through the same key returns an equivalent rhai
    // `Array`. Note that an array *nested inside* a GLOBAL value is itself a
    // further independently-addressable shared subtree (see
    // `dynamic_to_gvalue`'s doc comment in src/lib.rs): indexing into it via
    // `g["key"][i]` reads/writes that subtree directly, without needing to
    // round-trip the whole array back out to rhai first.
    let engine = make_rhai_engine();
    engine
        .run(r#"let g = global_data(); g["rhai_array_key"] = [10, 20, 30];"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let whole: rhai::Array = engine
        .eval(r#"global_data()["rhai_array_key"]"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(whole.len(), 3);
    assert_eq!(whole[1].as_int().unwrap(), 20);

    Ok(())
}
