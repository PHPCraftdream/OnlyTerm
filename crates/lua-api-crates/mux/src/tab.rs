use config::keyassignment::{PaneDirection, RotationDirection};

use super::*;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct MuxTab(pub TabId);

impl MuxTab {
    pub fn resolve<'a>(&self, mux: &'a Arc<Mux>) -> anyhow::Result<Arc<Tab>> {
        mux.get_tab(self.0)
            .ok_or_else(|| anyhow::anyhow!(format!("tab id {} not found in mux", self.0)))
    }
}

// ---------------------------------------------------------------------------
// L4d: rhai port of the `MuxTab` UserData above (see
// docs/plans/2026-07-23-lua-rhai-migration.md). Runs in parallel with the mlua
// path; does not replace or touch `impl UserData for MuxTab`.
//
// `get_pane_direction`'s `PaneDirection` argument and `get_size`'s
// `TerminalSize` return already derive `FromDynamic`/`ToDynamic` (see
// `config/src/keyassignment.rs`/`term/src/terminal.rs`), so both go through
// the same `config::rhai_value` bridge used everywhere else in this port,
// rather than a bespoke conversion.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_type_with_name::<MuxTab>("MuxTab");

    engine.register_fn("to_string", |this: &mut MuxTab| -> String {
        format!("MuxTab(tab_id:{}, pid:{})", this.0, std::process::id())
    });
    engine.register_fn("tab_id", |this: &mut MuxTab| -> rhai::INT {
        this.0 as rhai::INT
    });

    engine.register_fn(
        "window",
        |this: &mut MuxTab| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            for window_id in mux.iter_windows() {
                if let Some(window) = mux.get_window(window_id) {
                    for tab in window.iter() {
                        if tab.tab_id() == this.0 {
                            return Ok(rhai::Dynamic::from(MuxWindow(window_id)));
                        }
                    }
                }
            }
            Ok(rhai::Dynamic::UNIT)
        },
    );

    engine.register_fn(
        "get_title",
        |this: &mut MuxTab| -> Result<String, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;
            Ok(tab.get_title())
        },
    );
    engine.register_fn(
        "set_title",
        |this: &mut MuxTab, title: String| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;
            tab.set_title(&title);
            Ok(())
        },
    );

    engine.register_fn(
        "active_pane",
        |this: &mut MuxTab| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;
            Ok(
                match tab.get_active_pane().map(|pane| MuxPane(pane.pane_id())) {
                    Some(pane) => rhai::Dynamic::from(pane),
                    None => rhai::Dynamic::UNIT,
                },
            )
        },
    );

    engine.register_fn(
        "panes",
        |this: &mut MuxTab| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;
            Ok(tab
                .iter_panes_ignoring_zoom()
                .into_iter()
                .map(|info| rhai::Dynamic::from(MuxPane(info.pane.pane_id())))
                .collect())
        },
    );

    engine.register_fn(
        "get_pane_direction",
        |this: &mut MuxTab,
         direction: rhai::Dynamic|
         -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;
            let panes = tab.iter_panes_ignoring_zoom();

            let dir: PaneDirection = config::rhai_value::rhai_dynamic_to_value(&direction)
                .map_err(|err| -> Box<rhai::EvalAltResult> {
                    format!("get_pane_direction: {err}").into()
                })?;
            Ok(
                match tab
                    .get_pane_direction(dir, true)
                    .map(|pane_index| MuxPane(panes[pane_index].pane.pane_id()))
                {
                    Some(pane) => rhai::Dynamic::from(pane),
                    None => rhai::Dynamic::UNIT,
                },
            )
        },
    );

    engine.register_fn(
        "set_zoomed",
        |this: &mut MuxTab, zoomed: bool| -> Result<bool, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;
            Ok(tab.set_zoomed(zoomed))
        },
    );

    engine.register_fn(
        "panes_with_info",
        |this: &mut MuxTab| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;

            let mut result = rhai::Array::new();
            for pos in tab.iter_panes_ignoring_zoom().into_iter() {
                let info = MuxPaneInfo {
                    index: pos.index,
                    is_active: pos.is_active,
                    is_zoomed: pos.is_zoomed,
                    left: pos.left,
                    top: pos.top,
                    width: pos.width,
                    pixel_width: pos.pixel_width,
                    height: pos.height,
                    pixel_height: pos.pixel_height,
                };
                let mut map = match config::rhai_value::dynamic_to_rhai_dynamic(&info.to_dynamic())
                    .try_cast::<rhai::Map>()
                {
                    Some(m) => m,
                    None => rhai::Map::new(),
                };
                map.insert("pane".into(), rhai::Dynamic::from(MuxPane(pos.pane.pane_id())));
                result.push(rhai::Dynamic::from_map(map));
            }

            Ok(result)
        },
    );

    engine.register_fn(
        "rotate_counter_clockwise",
        |this: &mut MuxTab| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;

            let tab_id = tab.tab_id();
            let direction = RotationDirection::CounterClockwise;
            promise::spawn::spawn(async move {
                let mux = Mux::get();
                if let Err(err) = mux.rotate_panes(tab_id, direction).await {
                    log::error!("Unable to rotate panes: {:#}", err);
                }
            })
            .detach();

            Ok(())
        },
    );

    engine.register_fn(
        "rotate_clockwise",
        |this: &mut MuxTab| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;
            // Pre-existing behavior, kept identical here intentionally: this
            // binding uses the CounterClockwise direction, mirroring the
            // (likely copy/paste) bug that upstream's `rotate_clockwise` mlua
            // binding also carries. Do not "fix" it here.
            let tab_id = tab.tab_id();
            let direction = RotationDirection::CounterClockwise;
            promise::spawn::spawn(async move {
                let mux = Mux::get();
                if let Err(err) = mux.rotate_panes(tab_id, direction).await {
                    log::error!("Unable to rotate panes: {:#}", err);
                }
            })
            .detach();

            Ok(())
        },
    );

    engine.register_fn(
        "get_size",
        |this: &mut MuxTab| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;
            Ok(config::rhai_value::dynamic_to_rhai_dynamic(
                &tab.get_size().to_dynamic(),
            ))
        },
    );

    engine.register_fn(
        "activate",
        |this: &mut MuxTab| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = Mux::get();
            let tab = resolve_rhai(this, &mux)?;

            let pane = tab.get_active_pane().ok_or_else(|| -> Box<rhai::EvalAltResult> {
                format!("tab {} has no active pane!?", this.0).into()
            })?;

            let (_domain_id, window_id, tab_id) = mux.resolve_pane_id(pane.pane_id()).ok_or_else(
                || -> Box<rhai::EvalAltResult> {
                    format!("pane {} not found", pane.pane_id()).into()
                },
            )?;
            {
                let mut window = mux.get_window_mut(window_id).ok_or_else(
                    || -> Box<rhai::EvalAltResult> {
                        format!("window {window_id} not found").into()
                    },
                )?;
                let tab_idx = window.idx_by_id(tab_id).ok_or_else(
                    || -> Box<rhai::EvalAltResult> {
                        format!("tab {tab_id} isn't really in window {window_id}!?").into()
                    },
                )?;
                window.save_and_then_set_active(tab_idx);
            }
            Ok(())
        },
    );

    Ok(())
}

/// rhai-flavored equivalent of `get_mux()` (defined in `lib.rs` for the mlua
/// path).
pub(crate) fn get_mux_rhai() -> Result<Arc<Mux>, Box<rhai::EvalAltResult>> {
    Mux::try_get().ok_or_else(|| "cannot get Mux!?".into())
}

/// rhai-flavored equivalent of `MuxTab::resolve` above, also used by
/// `lib.rs`'s `mux::get_tab` binding (which lives outside this module).
pub(crate) fn resolve_rhai(
    this: &MuxTab,
    mux: &Arc<Mux>,
) -> Result<Arc<Tab>, Box<rhai::EvalAltResult>> {
    mux.get_tab(this.0)
        .ok_or_else(|| format!("tab id {} not found in mux", this.0).into())
}
