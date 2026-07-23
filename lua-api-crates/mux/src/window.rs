use super::*;
use parking_lot::{MappedRwLockReadGuard, MappedRwLockWriteGuard};

#[derive(Clone, Copy, Debug)]
pub struct MuxWindow(pub WindowId);

impl MuxWindow {
    pub fn resolve<'a>(
        &self,
        mux: &'a Arc<Mux>,
    ) -> mlua::Result<MappedRwLockReadGuard<'a, Window>> {
        mux.get_window(self.0)
            .ok_or_else(|| mlua::Error::external(format!("window id {} not found in mux", self.0)))
    }

    pub fn resolve_mut<'a>(
        &self,
        mux: &'a Arc<Mux>,
    ) -> mlua::Result<MappedRwLockWriteGuard<'a, Window>> {
        mux.get_window_mut(self.0)
            .ok_or_else(|| mlua::Error::external(format!("window id {} not found in mux", self.0)))
    }
}

impl UserData for MuxWindow {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, _: ()| {
            Ok(format!(
                "MuxWindow(mux_window_id:{}, pid:{})",
                this.0,
                unsafe { libc::getpid() }
            ))
        });
        methods.add_method("window_id", |_, this, _: ()| Ok(this.0));
        methods.add_async_method("gui_window", |lua, this, _: ()| async move {
            // Weakly bound to the gui module; mux cannot hard-depend
            // on wezterm-gui, but we can runtime resolve the appropriate module
            let wezterm_mod = get_or_create_module(lua, "wezterm")
                .map_err(|err| mlua::Error::external(format!("{err:#}")))?;
            let gui: mlua::Table = wezterm_mod.get("gui")?;
            let func: mlua::Function = gui.get("gui_window_for_mux_window")?;
            func.call_async::<_, mlua::Value>(this.0).await
        });
        methods.add_method("get_workspace", |_, this, _: ()| {
            let mux = get_mux()?;
            let window = this.resolve(&mux)?;
            Ok(window.get_workspace().to_string())
        });
        methods.add_method("set_workspace", |_, this, new_name: String| {
            let mux = get_mux()?;
            let mut window = this.resolve_mut(&mux)?;
            Ok(window.set_workspace(&new_name))
        });
        methods.add_async_method("spawn_tab", |_, this, spawn: SpawnTab| async move {
            spawn.spawn(this).await
        });
        methods.add_method("get_title", |_, this, _: ()| {
            let mux = get_mux()?;
            let window = this.resolve(&mux)?;
            Ok(window.get_title().to_string())
        });
        methods.add_method("set_title", |_, this, title: String| {
            let mux = get_mux()?;
            let mut window = this.resolve_mut(&mux)?;
            Ok(window.set_title(&title))
        });
        methods.add_method("tabs", |_, this, _: ()| {
            let mux = get_mux()?;
            let window = this.resolve(&mux)?;
            Ok(window
                .iter()
                .map(|tab| MuxTab(tab.tab_id()))
                .collect::<Vec<MuxTab>>())
        });
        methods.add_method("tabs_with_info", |lua, this, _: ()| {
            let mux = get_mux()?;
            let window = this.resolve(&mux)?;
            let result = lua.create_table()?;
            let active_idx = window.get_active_idx();
            for (index, tab) in window.iter().enumerate() {
                let info = MuxTabInfo {
                    index,
                    is_active: index == active_idx,
                };
                let info = luahelper::dynamic_to_lua_value(lua, info.to_dynamic())?;
                match &info {
                    LuaValue::Table(t) => {
                        t.set("tab", MuxTab(tab.tab_id()))?;
                    }
                    _ => {}
                }
                result.set(index + 1, info)?;
            }
            Ok(result)
        });
        methods.add_method("active_tab", |_, this, _: ()| {
            let mux = get_mux()?;
            let window = this.resolve(&mux)?;
            Ok(window.get_active().map(|tab| MuxTab(tab.tab_id())))
        });
        methods.add_method("active_pane", |_, this, _: ()| {
            let mux = get_mux()?;
            let window = this.resolve(&mux)?;
            Ok(window
                .get_active()
                .and_then(|tab| tab.get_active_pane().map(|pane| MuxPane(pane.pane_id()))))
        });
    }
}

// ---------------------------------------------------------------------------
// L4d: rhai port of the `MuxWindow` UserData above (see
// docs/plans/2026-07-23-lua-rhai-migration.md). Runs in parallel with the mlua
// path; does not replace or touch `impl UserData for MuxWindow`.
//
// `spawn_tab` reuses `SpawnTab::spawn` (defined in `lib.rs`) via the same
// `smol::block_on` async->sync adaptation used throughout this port -- the
// underlying async implementation is not duplicated, only driven differently.
//
// `gui_window` is the one method here with no direct rhai analogue available
// yet: on the mlua side it does a *runtime*, not compile-time, lookup of
// `wezterm.gui.gui_window_for_mux_window` -- a function that a completely
// separate crate (`wezterm-gui`, which cannot be a dependency of `mux-lua`;
// the dependency arrow points the other way) registers into the *same* `Lua`
// instance at startup. rhai's engine has no equivalent of "reach into a
// module living in a different namespace of the same interpreter instance and
// call whatever function some other, later-loaded crate happened to stash
// there" via `register_fn`/`register_static_module` alone. Rather than skip
// this method, it is ported via the same kind of weak/late-bound global hook
// this codebase already uses for `Mux` itself (`Mux::try_get()`/
// `Mux::set_mux`, see `mux/src/lib.rs`): see [`set_gui_window_resolver_rhai`]
// below, which `wezterm-gui` is expected to populate once it wires up its own
// rhai registration -- out of scope for this task (mux-lua alone), but the
// hook exists and is unit-tested here so that follow-up work has a
// ready-made, already-proven sink to call into.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_type_with_name::<MuxWindow>("MuxWindow");

    engine.register_fn("to_string", |this: &mut MuxWindow| -> String {
        format!("MuxWindow(mux_window_id:{}, pid:{})", this.0, unsafe {
            libc::getpid()
        })
    });
    engine.register_fn(
        "window_id",
        |this: &mut MuxWindow| -> rhai::INT { this.0 as rhai::INT },
    );

    engine.register_fn(
        "gui_window",
        |this: &mut MuxWindow| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            match gui_window_resolver_rhai(this.0) {
                Some(value) => Ok(value),
                None => Err(
                    "gui_window: no gui window resolver has been registered (this rhai engine \
                     is not running inside wezterm-gui, or wezterm-gui has not wired up its \
                     rhai bindings yet)"
                        .into(),
                ),
            }
        },
    );

    engine.register_fn(
        "get_workspace",
        |this: &mut MuxWindow| -> Result<String, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let window = resolve_rhai(this, &mux)?;
            Ok(window.get_workspace().to_string())
        },
    );
    engine.register_fn(
        "set_workspace",
        |this: &mut MuxWindow, new_name: String| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let mut window = resolve_mut_rhai(this, &mux)?;
            window.set_workspace(&new_name);
            Ok(())
        },
    );

    engine.register_fn(
        "spawn_tab",
        |this: &mut MuxWindow,
         spawn: rhai::Map|
         -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let spawn = SpawnTab::try_from(rhai::Dynamic::from_map(spawn)).map_err(
                |err| -> Box<rhai::EvalAltResult> { format!("spawn_tab: {err}").into() },
            )?;
            let window = *this;
            let (tab, pane, window) = smol::block_on(spawn.spawn(&window))
                .map_err(|err| -> Box<rhai::EvalAltResult> { format!("{err:#}").into() })?;
            Ok(rhai::Dynamic::from(vec![
                rhai::Dynamic::from(tab),
                rhai::Dynamic::from(pane),
                rhai::Dynamic::from(window),
            ]))
        },
    );
    // Zero-arg overload: `window.spawn_tab()` with defaults.
    engine.register_fn(
        "spawn_tab",
        |this: &mut MuxWindow| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let spawn = SpawnTab::default();
            let window = *this;
            let (tab, pane, window) = smol::block_on(spawn.spawn(&window))
                .map_err(|err| -> Box<rhai::EvalAltResult> { format!("{err:#}").into() })?;
            Ok(rhai::Dynamic::from(vec![
                rhai::Dynamic::from(tab),
                rhai::Dynamic::from(pane),
                rhai::Dynamic::from(window),
            ]))
        },
    );

    engine.register_fn(
        "get_title",
        |this: &mut MuxWindow| -> Result<String, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let window = resolve_rhai(this, &mux)?;
            Ok(window.get_title().to_string())
        },
    );
    engine.register_fn(
        "set_title",
        |this: &mut MuxWindow, title: String| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let mut window = resolve_mut_rhai(this, &mux)?;
            window.set_title(&title);
            Ok(())
        },
    );

    engine.register_fn(
        "tabs",
        |this: &mut MuxWindow| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let window = resolve_rhai(this, &mux)?;
            Ok(window
                .iter()
                .map(|tab| rhai::Dynamic::from(MuxTab(tab.tab_id())))
                .collect())
        },
    );

    engine.register_fn(
        "tabs_with_info",
        |this: &mut MuxWindow| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let window = resolve_rhai(this, &mux)?;
            let active_idx = window.get_active_idx();
            let mut result = rhai::Array::new();
            for (index, tab) in window.iter().enumerate() {
                let info = MuxTabInfo {
                    index,
                    is_active: index == active_idx,
                };
                let mut map = match config::rhai_value::dynamic_to_rhai_dynamic(&info.to_dynamic())
                    .try_cast::<rhai::Map>()
                {
                    Some(m) => m,
                    None => rhai::Map::new(),
                };
                map.insert("tab".into(), rhai::Dynamic::from(MuxTab(tab.tab_id())));
                result.push(rhai::Dynamic::from_map(map));
            }
            Ok(result)
        },
    );

    engine.register_fn(
        "active_tab",
        |this: &mut MuxWindow| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let window = resolve_rhai(this, &mux)?;
            Ok(match window.get_active() {
                Some(tab) => rhai::Dynamic::from(MuxTab(tab.tab_id())),
                None => rhai::Dynamic::UNIT,
            })
        },
    );

    engine.register_fn(
        "active_pane",
        |this: &mut MuxWindow| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let window = resolve_rhai(this, &mux)?;
            Ok(
                match window
                    .get_active()
                    .and_then(|tab| tab.get_active_pane().map(|pane| MuxPane(pane.pane_id())))
                {
                    Some(pane) => rhai::Dynamic::from(pane),
                    None => rhai::Dynamic::UNIT,
                },
            )
        },
    );

    Ok(())
}

/// rhai-flavored equivalent of `MuxWindow::resolve` above, also used by
/// `lib.rs`'s `mux::get_window` binding (which lives outside this module) to
/// validate a window id exists before handing back a `MuxWindow` handle.
pub(crate) fn resolve_rhai<'a>(
    this: &MuxWindow,
    mux: &'a Arc<Mux>,
) -> Result<MappedRwLockReadGuard<'a, Window>, Box<rhai::EvalAltResult>> {
    mux.get_window(this.0)
        .ok_or_else(|| format!("window id {} not found in mux", this.0).into())
}

/// rhai-flavored equivalent of `MuxWindow::resolve_mut` above.
fn resolve_mut_rhai<'a>(
    this: &MuxWindow,
    mux: &'a Arc<Mux>,
) -> Result<MappedRwLockWriteGuard<'a, Window>, Box<rhai::EvalAltResult>> {
    mux.get_window_mut(this.0)
        .ok_or_else(|| format!("window id {} not found in mux", this.0).into())
}

/// Weak/late-bound hook so that `wezterm-gui` (a downstream crate that
/// depends on `mux-lua`, never the other way around -- see the module-level
/// doc comment above) can register a resolver from `MuxWindowId` to whatever
/// rhai representation of its own GUI window type it wants to hand back from
/// `gui_window()`. Mirrors the shape of `Mux::set_mux`/`Mux::try_get`
/// (`mux/src/lib.rs`): a process-wide slot, set once at startup, read on
/// every call.
type GuiWindowResolverFn = dyn Fn(WindowId) -> Option<rhai::Dynamic> + Send + Sync;

lazy_static::lazy_static! {
    static ref GUI_WINDOW_RESOLVER_RHAI: std::sync::Mutex<Option<Box<GuiWindowResolverFn>>> =
        std::sync::Mutex::new(None);
}

/// Register the resolver used by `gui_window()` above. Intended to be called
/// once, at startup, by whichever crate owns the concept of a "gui window"
/// (currently `wezterm-gui`); not yet wired up anywhere, as no rhai-side gui
/// window type exists yet -- see the module-level doc comment.
pub fn set_gui_window_resolver_rhai(
    resolver: impl Fn(WindowId) -> Option<rhai::Dynamic> + Send + Sync + 'static,
) {
    *GUI_WINDOW_RESOLVER_RHAI.lock().unwrap() = Some(Box::new(resolver));
}

fn gui_window_resolver_rhai(window_id: WindowId) -> Option<rhai::Dynamic> {
    let resolver = GUI_WINDOW_RESOLVER_RHAI.lock().unwrap();
    resolver.as_ref().and_then(|f| f(window_id))
}
