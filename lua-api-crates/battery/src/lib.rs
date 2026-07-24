use config::impl_rhai_conversion_dynamic;
use wezterm_dynamic::{FromDynamic, ToDynamic};

/// Rhai-side registration of `battery_info()` (see
/// `docs/plans/2026-07-23-lua-rhai-migration.md`).
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
impl_rhai_conversion_dynamic!(BatteryInfo);

/// rhai analogue of the former mlua `battery_info` binding. Returns a rhai
/// `Array` of `Dynamic` object maps (via `impl_rhai_conversion_dynamic!`'s
/// generated `From<BatteryInfo> for rhai::Dynamic`).
///
/// Battery presence/state is inherently non-deterministic across machines (a CI
/// runner may have zero batteries, a laptop may have one or more in varying
/// states), so the underlying `collect_battery_info` helper is exercised in
/// tests only for "does it run without panicking and return the right shape",
/// never for a specific fixed reading. See `tests/rhai_smoke.rs`.
fn battery_info_rhai() -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let info = collect_battery_info().map_err(|err| -> Box<rhai::EvalAltResult> {
        format!("battery_info: {err:#}").into()
    })?;
    Ok(info
        .into_iter()
        .map(|b| -> rhai::Dynamic { b.into() })
        .collect())
}

/// Enumerate the system's batteries via `starship_battery` and convert each
/// into a `BatteryInfo`.
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
