use config::impl_rhai_conversion_dynamic;
use config::lua::get_or_create_module;
use config::lua::mlua::{self, Lua};
use luahelper::impl_lua_conversion_dynamic;
use wezterm_dynamic::{FromDynamic, ToDynamic};

pub fn register(lua: &Lua) -> anyhow::Result<()> {
    let wezterm_mod = get_or_create_module(lua, "wezterm")?;
    wezterm_mod.set("battery_info", lua.create_function(battery_info)?)?;
    Ok(())
}

/// Rhai-side analogue of `register` above (L4a, see
/// `docs/plans/2026-07-23-lua-rhai-migration.md`). Registers `battery_info()` as a
/// top-level function on the engine, mirroring `wezterm.battery_info()` on the Lua
/// path (there is no sub-module here on the Lua side either -- `battery_info` is
/// set directly on the `wezterm` table, not e.g. `wezterm.battery.info()` -- so this
/// registers a bare global function rather than using `register_static_module`,
/// matching the mlua call-site shape as closely as possible).
///
/// Runs in parallel with `register(lua: &Lua)`; does not replace or touch it. Both
/// remain valid until L5 removes the mlua pipeline.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_fn("battery_info", battery_info_rhai);
    Ok(())
}

#[derive(FromDynamic, ToDynamic, Debug, Clone)]
struct BatteryInfo {
    state_of_charge: f32,
    vendor: String,
    model: String,
    state: String,
    serial: String,
    time_to_full: Option<f32>,
    time_to_empty: Option<f32>,
}
impl_lua_conversion_dynamic!(BatteryInfo);
impl_rhai_conversion_dynamic!(BatteryInfo);

fn battery_info<'lua>(_: &'lua Lua, _: ()) -> mlua::Result<Vec<BatteryInfo>> {
    collect_battery_info().map_err(mlua::Error::external)
}

/// rhai analogue of `battery_info` above. Returns a rhai `Array` of `Dynamic`
/// object maps (via `impl_rhai_conversion_dynamic!`'s generated `From<BatteryInfo>
/// for rhai::Dynamic`), which is the closest rhai equivalent of the
/// `Vec<BatteryInfo>` (converted to a Lua table of tables by `impl_lua_conversion_
/// dynamic!`) that the mlua path returns.
///
/// Battery presence/state is inherently non-deterministic across machines (a CI
/// runner may have zero batteries, a laptop may have one or more in varying
/// states), so the underlying `collect_battery_info` helper -- shared with the mlua
/// path above -- is exercised in tests only for "does it run without panicking and
/// return the right shape", never for a specific fixed reading. See
/// `tests/rhai_smoke.rs` for how this is verified.
fn battery_info_rhai() -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let info = collect_battery_info().map_err(|err| -> Box<rhai::EvalAltResult> {
        format!("battery_info: {err:#}").into()
    })?;
    Ok(info
        .into_iter()
        .map(|b| -> rhai::Dynamic { b.into() })
        .collect())
}

/// Shared implementation used by both `battery_info` (mlua) and `battery_info_rhai`
/// (rhai): enumerate the system's batteries via `starship_battery` and convert each
/// into a `BatteryInfo`. Factored out so the two engine bindings can't drift in
/// behavior.
fn collect_battery_info() -> anyhow::Result<Vec<BatteryInfo>> {
    use starship_battery::{Manager, State};
    let manager = Manager::new()?;
    let mut result = vec![];
    for b in manager.batteries()? {
        let bat = b?;
        result.push(BatteryInfo {
            state_of_charge: bat.state_of_charge().value,
            vendor: opt_string(bat.vendor()),
            model: opt_string(bat.model()),
            serial: opt_string(bat.serial_number()),
            state: match bat.state() {
                State::Charging => "Charging",
                State::Discharging => "Discharging",
                State::Empty => "Empty",
                State::Full => "Full",
                State::Unknown => "Unknown",
            }
            .to_string(),
            time_to_full: bat.time_to_full().map(|q| q.value),
            time_to_empty: bat.time_to_empty().map(|q| q.value),
        })
    }
    Ok(result)
}

fn opt_string(s: Option<&str>) -> String {
    match s {
        Some(s) => s,
        None => "unknown",
    }
    .to_string()
}
