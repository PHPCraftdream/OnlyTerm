use config::keyassignment::PaneDirection;

use super::*;
use luahelper::mlua::Value;
use luahelper::{from_lua, to_lua};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct MuxTab(pub TabId);

impl MuxTab {
    pub fn resolve<'a>(&self, mux: &'a Arc<Mux>) -> mlua::Result<Arc<Tab>> {
        mux.get_tab(self.0)
            .ok_or_else(|| mlua::Error::external(format!("tab id {} not found in mux", self.0)))
    }
}

impl UserData for MuxTab {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, _: ()| {
            Ok(format!("MuxTab(tab_id:{}, pid:{})", this.0, unsafe {
                libc::getpid()
            }))
        });
        methods.add_method("tab_id", |_, this, _: ()| Ok(this.0));
        methods.add_method("window", |_, this, _: ()| {
            let mux = get_mux()?;
            for window_id in mux.iter_windows() {
                if let Some(window) = mux.get_window(window_id) {
                    for tab in window.iter() {
                        if tab.tab_id() == this.0 {
                            return Ok(Some(MuxWindow(window_id)));
                        }
                    }
                }
            }
            Ok(None)
        });
        methods.add_method("get_title", |_, this, _: ()| {
            let mux = get_mux()?;
            let tab = this.resolve(&mux)?;
            Ok(tab.get_title())
        });
        methods.add_method("set_title", |_, this, title: String| {
            let mux = get_mux()?;
            let tab = this.resolve(&mux)?;
            Ok(tab.set_title(&title))
        });
        methods.add_method("active_pane", |_, this, _: ()| {
            let mux = get_mux()?;
            let tab = this.resolve(&mux)?;
            Ok(tab.get_active_pane().map(|pane| MuxPane(pane.pane_id())))
        });
        methods.add_method("panes", |_, this, _: ()| {
            let mux = get_mux()?;
            let tab = this.resolve(&mux)?;
            Ok(tab
                .iter_panes_ignoring_zoom()
                .into_iter()
                .map(|info| MuxPane(info.pane.pane_id()))
                .collect::<Vec<MuxPane>>())
        });

        methods.add_method("get_pane_direction", |_, this, direction: Value| {
            let mux = get_mux()?;
            let tab = this.resolve(&mux)?;
            let panes = tab.iter_panes_ignoring_zoom();

            let dir: PaneDirection = from_lua(direction)?;
            let pane = tab
                .get_pane_direction(dir, true)
                .map(|pane_index| MuxPane(panes[pane_index].pane.pane_id()));
            Ok(pane)
        });

        methods.add_method("set_zoomed", |_, this, zoomed: bool| {
            let mux = get_mux()?;
            let tab = this.resolve(&mux)?;
            let was_zoomed = tab.set_zoomed(zoomed);
            Ok(was_zoomed)
        });

        methods.add_method("panes_with_info", |lua, this, _: ()| {
            let mux = get_mux()?;
            let tab = this.resolve(&mux)?;

            let result = lua.create_table()?;
            for (idx, pos) in tab.iter_panes_ignoring_zoom().into_iter().enumerate() {
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
                let info = luahelper::dynamic_to_lua_value(lua, info.to_dynamic())?;
                match &info {
                    LuaValue::Table(t) => {
                        t.set("pane", MuxPane(pos.pane.pane_id()))?;
                    }
                    _ => {}
                }
                result.set(idx + 1, info)?;
            }

            Ok(result)
        });

        methods.add_method("rotate_counter_clockwise", |_, this, _: ()| {
            let mux = get_mux()?;
            let tab = this.resolve(&mux)?;
            tab.rotate_counter_clockwise();
            Ok(())
        });

        methods.add_method("rotate_clockwise", |_, this, _: ()| {
            let mux = get_mux()?;
            let tab = this.resolve(&mux)?;
            tab.rotate_counter_clockwise();
            Ok(())
        });

        methods.add_method("get_size", |lua, this, _: ()| {
            let mux = get_mux()?;
            let tab = this.resolve(&mux)?;
            to_lua(lua, tab.get_size())
        });

        methods.add_method("activate", move |_lua, this, ()| {
            let mux = Mux::get();
            let tab = this.resolve(&mux)?;

            let pane = tab.get_active_pane().ok_or_else(|| {
                mlua::Error::external(format!("tab {} has no active pane!?", this.0))
            })?;

            let (_domain_id, window_id, tab_id) =
                mux.resolve_pane_id(pane.pane_id()).ok_or_else(|| {
                    mlua::Error::external(format!("pane {} not found", pane.pane_id()))
                })?;
            {
                let mut window = mux.get_window_mut(window_id).ok_or_else(|| {
                    mlua::Error::external(format!("window {window_id} not found"))
                })?;
                let tab_idx = window.idx_by_id(tab_id).ok_or_else(|| {
                    mlua::Error::external(format!(
                        "tab {tab_id} isn't really in window {window_id}!?"
                    ))
                })?;
                window.save_and_then_set_active(tab_idx);
            }
            Ok(())
        });
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
        format!("MuxTab(tab_id:{}, pid:{})", this.0, unsafe {
            libc::getpid()
        })
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
            tab.rotate_counter_clockwise();
            Ok(())
        },
    );

    engine.register_fn(
        "rotate_clockwise",
        |this: &mut MuxTab| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let tab = resolve_rhai(this, &mux)?;
            // Mirrors the mlua path's own `rotate_clockwise` binding above,
            // which (pre-existing behavior, not introduced by this port)
            // itself calls `rotate_counter_clockwise` -- kept identical here
            // so this rhai port's behavior can't drift from the mlua path it
            // mirrors, even though that looks like a copy/paste bug in the
            // original.
            tab.rotate_counter_clockwise();
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
