//! L4a smoke test: `battery::register_rhai` exposes `battery_info()` to rhai
//! scripts, mirroring `wezterm.battery_info()` on the mlua path (see
//! `docs/plans/2026-07-23-lua-rhai-migration.md`).
//!
//! ## Handling non-determinism
//!
//! Battery presence and state (`Charging`/`Discharging`/`Full`/... , charge
//! percentage, vendor/model/serial strings) are properties of whatever real
//! hardware the test happens to run on -- a CI runner or desktop machine
//! typically has zero batteries, a laptop has one or more in an arbitrary state
//! at any given moment. There is nothing here to assert a fixed value against
//! without faking the underlying `starship_battery::Manager`, which is out of
//! scope for this port. So these tests assert only what's true regardless of
//! the host's hardware:
//!   * the call succeeds (doesn't panic, doesn't return a rhai eval error) --
//!     confirms the registration + argument/return marshalling is wired up
//!     correctly end to end, independent of what batteries (if any) exist;
//!   * the result has the right rhai *type* (an `Array`), and if non-empty,
//!     each element is a map with the expected field names, i.e. the shape
//!     produced by `impl_rhai_conversion_dynamic!`'s `From<BatteryInfo> for
//!     rhai::Dynamic` is what a `.rhai` script actually observes;
//!   * calling the same underlying Rust code both directly (as a sanity oracle)
//!     and through the rhai engine produces results of the same length --
//!     this is the strongest equality check possible without controlling the
//!     hardware, since two calls milliseconds apart on a real battery could in
//!     principle observe a different charge reading (though not a different
//!     battery *count*).

use rhai::Engine;

#[test]
fn rhai_battery_info_returns_an_array_without_panicking() -> anyhow::Result<()> {
    let mut engine = Engine::new();
    battery::register_rhai(&mut engine)?;

    let result: rhai::Array = engine
        .eval("battery_info()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    // Whatever the host's hardware situation, each entry (if any) must be a
    // rhai map with the fields BatteryInfo defines, mirroring the Lua-table
    // shape `impl_lua_conversion_dynamic!` produces for the same struct.
    for entry in &result {
        assert!(
            entry.is_map(),
            "expected each battery_info() entry to be a map, got {}",
            entry.type_name()
        );
        let map = entry.clone_cast::<rhai::Map>();
        for field in [
            "state_of_charge",
            "vendor",
            "model",
            "state",
            "serial",
            "time_to_full",
            "time_to_empty",
        ] {
            assert!(
                map.contains_key(field),
                "expected battery_info() entry to have a `{field}` field, got {map:?}"
            );
        }
    }

    Ok(())
}

#[test]
fn rhai_battery_info_agrees_with_direct_manager_enumeration_on_count() -> anyhow::Result<()> {
    let mut engine = Engine::new();
    battery::register_rhai(&mut engine)?;

    let result: rhai::Array = engine
        .eval("battery_info()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    // Oracle: call starship_battery directly (independent of both the mlua and
    // rhai bindings) and confirm the battery *count* agrees. We don't compare
    // individual field values since e.g. state_of_charge can legitimately
    // change between the two enumerations on real hardware.
    let manager = starship_battery::Manager::new()?;
    let direct_count = manager.batteries()?.count();

    assert_eq!(
        result.len(),
        direct_count,
        "battery_info() via rhai should enumerate the same number of \
         batteries as calling starship_battery::Manager directly"
    );

    Ok(())
}

#[test]
fn rhai_battery_info_is_callable_from_a_script_like_wezterm_battery_info() -> anyhow::Result<()> {
    // Slightly more script-shaped than a bare `.eval`, proving the function is
    // usable the way a real .rhai config would use it (e.g. iterating over the
    // result), not just callable in isolation.
    let mut engine = Engine::new();
    battery::register_rhai(&mut engine)?;

    let script = r#"
        let batteries = battery_info();
        batteries.len()
    "#;
    let count: i64 = engine
        .eval(script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert!(count >= 0);
    Ok(())
}
