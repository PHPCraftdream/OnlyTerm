//! GuiWin represents a Gui TermWindow (as opposed to a Mux window) in lua code
use super::luaerr;
use crate::termwindow::TermWindowNotif;
use crate::TermWindow;
use config::keyassignment::{ClipboardCopyDestination, KeyAssignment};
use luahelper::*;
use mlua::{UserData, UserDataMethods, UserDataRef};
use mux::pane::PaneId;
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use mux_lua::MuxPane;
use termwiz_funcs::lines_to_escapes;
use wezterm_dynamic::{FromDynamic, ToDynamic};
use wezterm_toast_notification::ToastNotification;
use window::{Connection, ConnectionOps, DeadKeyStatus, WindowOps, WindowState};

/// L4.6 rhai binding for `GuiWin`, mirroring the `impl UserData for GuiWin` block
/// below one method at a time (see that impl's methods for the semantics each of
/// these forwards to -- this is deliberately *not* a from-scratch reimplementation,
/// just a second front door onto the same underlying `TermWindow`/`window::Window`
/// operations).
///
/// ## Why only a subset of methods
///
/// About half of `GuiWin`'s methods (`get_dimensions`, `effective_config`,
/// `active_pane`, `perform_action`, etc) are `add_async_method`s on the mlua side:
/// they send a `TermWindowNotif::Apply`/similar message to the GUI event loop via
/// `Window::notify` (which just enqueues a future onto the main-thread executor,
/// see `window::os::*::connection::Connection::with_window_inner`) and then
/// `.await` a channel response from that enqueued work. That `.await` is safe on
/// the mlua side because Lua callbacks are executed via `create_async_function`/
/// `call_async`, which suspends and lets the *same* executor drain its queue
/// (including the just-enqueued `Apply` future) before resuming.
///
/// rhai has no async execution model at all (see `config/src/rhai_engine.rs`'s
/// module doc comment): every rhai-registered function is a plain synchronous
/// `Fn`. Emulating the mlua behavior with `smol::block_on(rx.recv())` would be
/// **unsound** here specifically because `wezterm.on(...)` handlers run *on the
/// main GUI thread itself* (see `config::with_rhai_config_on_main_thread`) -- the
/// same thread that would need to drain its executor queue to ever deliver the
/// `Apply` response. Blocking that thread synchronously waiting on a message that
/// only that same thread's queue-drain can produce is a guaranteed self-deadlock,
/// not a rare race.
///
/// So this binding only exposes the methods that are genuinely synchronous
/// end-to-end: either pure computation (`window_id`, `active_workspace`) or a
/// fire-and-forget `.notify()` with no response to wait for (`set_position`,
/// `maximize`, ...). The async, response-waiting methods are deliberately left
/// unbound; a script that needs one of them from rhai today has no working
/// substitute, and closing that gap requires either a redesigned non-blocking
/// notify/response mechanism or a different threading model for event-handler
/// dispatch -- flagged as follow-up work, not solved here.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_type_with_name::<GuiWin>("GuiWin");

    engine.register_fn("to_string", |this: &mut GuiWin| -> String {
        format!("GuiWin(mux_window_id:{}, pid:{})", this.mux_window_id, unsafe {
            libc::getpid()
        })
    });

    engine.register_fn("window_id", |this: &mut GuiWin| -> rhai::INT {
        this.mux_window_id as rhai::INT
    });

    engine.register_fn("mux_window", |this: &mut GuiWin| -> mux_lua::MuxWindow {
        mux_lua::MuxWindow(this.mux_window_id)
    });

    engine.register_fn(
        "active_tab",
        |this: &mut GuiWin| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let mux = Mux::try_get().ok_or_else(|| -> Box<rhai::EvalAltResult> {
                "cannot get Mux!?".into()
            })?;
            let window = mux.get_window(this.mux_window_id).ok_or_else(
                || -> Box<rhai::EvalAltResult> {
                    format!("invalid window {}", this.mux_window_id).into()
                },
            )?;
            Ok(match window.get_active() {
                Some(tab) => rhai::Dynamic::from(mux_lua::MuxTab(tab.tab_id())),
                None => rhai::Dynamic::UNIT,
            })
        },
    );

    engine.register_fn(
        "set_inner_size",
        |this: &mut GuiWin, width: rhai::INT, height: rhai::INT| {
            this.window.notify(TermWindowNotif::SetInnerSize {
                width: width as usize,
                height: height as usize,
            });
        },
    );
    engine.register_fn("set_position", |this: &mut GuiWin, x: rhai::INT, y: rhai::INT| {
        this.window
            .set_window_position(euclid::point2(x as isize, y as isize));
    });
    engine.register_fn("maximize", |this: &mut GuiWin| {
        this.window.maximize();
    });
    engine.register_fn("restore", |this: &mut GuiWin| {
        this.window.restore();
    });
    engine.register_fn("toggle_fullscreen", |this: &mut GuiWin| {
        this.window.toggle_fullscreen();
    });
    engine.register_fn("focus", |this: &mut GuiWin| {
        this.window.focus();
    });
    engine.register_fn(
        "toast_notification",
        |this: &mut GuiWin,
         title: &str,
         message: &str,
         url: rhai::Dynamic,
         timeout_ms: rhai::Dynamic| {
            let _ = this;
            wezterm_toast_notification::show(ToastNotification {
                title: title.to_string(),
                message: message.to_string(),
                url: url.into_string().ok(),
                timeout: timeout_ms
                    .as_int()
                    .ok()
                    .map(|ms| std::time::Duration::from_millis(ms as u64)),
            });
        },
    );
    engine.register_fn("get_appearance", |this: &mut GuiWin| -> String {
        let _ = this;
        Connection::get().unwrap().get_appearance().to_string()
    });
    engine.register_fn("set_right_status", |this: &mut GuiWin, status: &str| {
        this.window
            .notify(TermWindowNotif::SetRightStatus(status.to_string()));
    });
    engine.register_fn("set_left_status", |this: &mut GuiWin, status: &str| {
        this.window
            .notify(TermWindowNotif::SetLeftStatus(status.to_string()));
    });
    engine.register_fn(
        "set_config_overrides",
        |this: &mut GuiWin, value: rhai::Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            let value = config::rhai_value::rhai_dynamic_to_dynamic(&value)
                .map_err(|e| -> Box<rhai::EvalAltResult> { format!("{e}").into() })?;
            this.window
                .notify(TermWindowNotif::SetConfigOverrides(value));
            Ok(())
        },
    );
    engine.register_fn(
        "active_workspace",
        |this: &mut GuiWin| -> Result<String, Box<rhai::EvalAltResult>> {
            let _ = this;
            let mux = Mux::try_get().ok_or_else(|| -> Box<rhai::EvalAltResult> {
                "no mux?".into()
            })?;
            Ok(mux.active_workspace().to_string())
        },
    );
    engine.register_fn(
        "copy_to_clipboard",
        |this: &mut GuiWin, text: &str| {
            let text = text.to_string();
            this.window
                .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    term_window.copy_to_clipboard(ClipboardCopyDestination::Clipboard, text);
                })));
        },
    );

    Ok(())
}

#[derive(Clone)]
pub struct GuiWin {
    pub mux_window_id: MuxWindowId,
    pub window: ::window::Window,
}

impl GuiWin {
    pub fn new(term_window: &TermWindow) -> Self {
        let window = term_window.window.clone().unwrap();
        let mux_window_id = term_window.mux_window_id;
        Self {
            window,
            mux_window_id,
        }
    }
}

impl UserData for GuiWin {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, _: ()| {
            Ok(format!(
                "GuiWin(mux_window_id:{}, pid:{})",
                this.mux_window_id,
                unsafe { libc::getpid() }
            ))
        });

        methods.add_method("window_id", |_, this, _: ()| Ok(this.mux_window_id));
        methods.add_method("mux_window", |_, this, _: ()| {
            Ok(mux_lua::MuxWindow(this.mux_window_id))
        });
        methods.add_method("active_tab", |_, this, _: ()| {
            let mux = Mux::try_get().ok_or_else(|| mlua::Error::external("cannot get Mux!?"))?;
            let window = mux.get_window(this.mux_window_id).ok_or_else(|| {
                mlua::Error::external(format!("invalid window {}", this.mux_window_id))
            })?;
            Ok(window.get_active().map(|tab| mux_lua::MuxTab(tab.tab_id())))
        });

        methods.add_method(
            "set_inner_size",
            |_, this, (width, height): (usize, usize)| {
                this.window
                    .notify(TermWindowNotif::SetInnerSize { width, height });
                Ok(())
            },
        );
        methods.add_method("set_position", |_, this, (x, y): (isize, isize)| {
            this.window.set_window_position(euclid::point2(x, y));
            Ok(())
        });
        methods.add_method("maximize", |_, this, _: ()| {
            this.window.maximize();
            Ok(())
        });
        methods.add_method("restore", |_, this, _: ()| {
            this.window.restore();
            Ok(())
        });
        methods.add_method("toggle_fullscreen", |_, this, _: ()| {
            this.window.toggle_fullscreen();
            Ok(())
        });
        methods.add_method("focus", |_, this, _: ()| {
            this.window.focus();
            Ok(())
        });
        methods.add_method(
            "toast_notification",
            |_, _, (title, message, url, timeout): (String, String, Option<String>, Option<u64>)| {
                wezterm_toast_notification::show(ToastNotification {
                    title,
                    message,
                    url,
                    timeout: timeout.map(std::time::Duration::from_millis)
                });
                Ok(())
            },
        );
        methods.add_method("get_appearance", |_, _, _: ()| {
            Ok(Connection::get().unwrap().get_appearance().to_string())
        });
        methods.add_method("set_right_status", |_, this, status: String| {
            this.window.notify(TermWindowNotif::SetRightStatus(status));
            Ok(())
        });
        methods.add_method("set_left_status", |_, this, status: String| {
            this.window.notify(TermWindowNotif::SetLeftStatus(status));
            Ok(())
        });
        methods.add_async_method("get_dimensions", |_, this, _: ()| async move {
            let (tx, rx) = smol::channel::bounded(1);
            this.window.notify(TermWindowNotif::GetDimensions(tx));
            let (dims, window_state) = rx
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!("{:#}", e))
                .map_err(luaerr)?;

            #[derive(FromDynamic, ToDynamic)]
            struct Dims {
                pixel_width: usize,
                pixel_height: usize,
                dpi: usize,
                is_full_screen: bool,
            }
            impl_lua_conversion_dynamic!(Dims);

            let dims = Dims {
                pixel_width: dims.pixel_width,
                pixel_height: dims.pixel_height,
                dpi: dims.dpi,
                is_full_screen: window_state.contains(WindowState::FULL_SCREEN),
                // FIXME: expose other states here
            };
            Ok(dims)
        });
        methods.add_async_method(
            "get_selection_text_for_pane",
            |_, this, pane: UserDataRef<MuxPane>| async move {
                let (tx, rx) = smol::channel::bounded(1);
                this.window.notify(TermWindowNotif::GetSelectionForPane {
                    pane_id: pane.0,
                    tx,
                });
                let text = rx
                    .recv()
                    .await
                    .map_err(|e| anyhow::anyhow!("{:#}", e))
                    .map_err(luaerr)?;

                Ok(text)
            },
        );
        methods.add_async_method("current_event", |lua, this, _: ()| async move {
            let (tx, rx) = smol::channel::bounded(1);
            this.window
                .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    tx.try_send(term_window.current_event.to_dynamic()).ok();
                })));
            let result = rx.recv().await.map_err(mlua::Error::external)?;
            luahelper::dynamic_to_lua_value(lua, result)
        });
        methods.add_async_method(
            "perform_action",
            |_, this, (assignment, pane): (KeyAssignment, UserDataRef<MuxPane>)| async move {
                let (tx, rx) = smol::channel::bounded(1);
                this.window.notify(TermWindowNotif::PerformAssignment {
                    pane_id: pane.0,
                    assignment,
                    tx: Some(tx),
                });
                let result = rx.recv().await.map_err(mlua::Error::external)?;
                result.map_err(mlua::Error::external)
            },
        );
        methods.add_async_method("effective_config", |_, this, _: ()| async move {
            let (tx, rx) = smol::channel::bounded(1);
            this.window.notify(TermWindowNotif::GetEffectiveConfig(tx));
            let config = rx
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!("{:#}", e))
                .map_err(luaerr)?;

            Ok((*config).clone())
        });
        methods.add_async_method("get_config_overrides", |lua, this, _: ()| async move {
            let (tx, rx) = smol::channel::bounded(1);
            this.window.notify(TermWindowNotif::GetConfigOverrides(tx));
            let overrides = rx
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!("{:#}", e))
                .map_err(luaerr)?;

            dynamic_to_lua_value(lua, overrides)
        });
        methods.add_method("set_config_overrides", |_, this, value: mlua::Value| {
            let value = lua_value_to_dynamic(value)?;
            this.window
                .notify(TermWindowNotif::SetConfigOverrides(value));
            Ok(())
        });
        methods.add_async_method("is_focused", |_, this, _: ()| async move {
            let (tx, rx) = smol::channel::bounded(1);
            this.window
                .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    tx.try_send(term_window.focused.is_some()).ok();
                })));
            let result = rx
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!("{:#}", e))
                .map_err(luaerr)?;

            Ok(result)
        });
        methods.add_async_method("leader_is_active", |_, this, _: ()| async move {
            let (tx, rx) = smol::channel::bounded(1);
            this.window
                .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    tx.try_send(term_window.leader_is_active()).ok();
                })));
            let result = rx
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!("{:#}", e))
                .map_err(luaerr)?;

            Ok(result)
        });
        methods.add_async_method("composition_status", |_, this, _: ()| async move {
            let (tx, rx) = smol::channel::bounded(1);
            this.window
                .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    tx.try_send(match term_window.composition_status() {
                        DeadKeyStatus::None => None,
                        DeadKeyStatus::Composing(s) => Some(s.clone()),
                    })
                    .ok();
                })));
            let result = rx
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!("{:#}", e))
                .map_err(luaerr)?;

            Ok(result)
        });
        methods.add_async_method("active_key_table", |_, this, _: ()| async move {
            let (tx, rx) = smol::channel::bounded(1);
            this.window
                .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    tx.try_send(term_window.current_key_table_name()).ok();
                })));
            let result = rx
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!("{:#}", e))
                .map_err(luaerr)?;

            Ok(result)
        });
        methods.add_async_method("keyboard_modifiers", |_, this, _: ()| async move {
            let (tx, rx) = smol::channel::bounded(1);
            this.window
                .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    tx.try_send(term_window.current_modifier_and_led_state())
                        .ok();
                })));
            let (mods, leds) = rx
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!("{:#}", e))
                .map_err(luaerr)?;

            Ok((mods.to_string(), leds.to_string()))
        });
        methods.add_async_method("active_pane", |_, this, _: ()| async move {
            let (tx, rx) = smol::channel::bounded(1);
            this.window
                .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    tx.try_send(
                        term_window
                            .get_active_pane_or_overlay()
                            .map(|pane| MuxPane(pane.pane_id())),
                    )
                    .ok();
                })));
            let result = rx
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!("{:#}", e))
                .map_err(luaerr)?;

            Ok(result)
        });
        methods.add_method("active_workspace", |_, _, _: ()| {
            let mux = Mux::try_get()
                .ok_or_else(|| anyhow::anyhow!("no mux?"))
                .map_err(luaerr)?;
            Ok(mux.active_workspace().to_string())
        });
        methods.add_method(
            "copy_to_clipboard",
            |_, this, (text, clipboard): (String, Option<ClipboardCopyDestination>)| {
                let clipboard = clipboard.unwrap_or_default();
                this.window
                    .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                        term_window.copy_to_clipboard(clipboard, text);
                    })));
                Ok(())
            },
        );
        methods.add_async_method(
            "get_selection_escapes_for_pane",
            |_, this, pane: UserDataRef<MuxPane>| async move {
                let (tx, rx) = smol::channel::bounded(1);
                let pane_id = pane.0;
                this.window
                    .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                        fn do_it(
                            pane_id: PaneId,
                            term_window: &mut TermWindow,
                        ) -> anyhow::Result<String> {
                            let mux = Mux::try_get().ok_or_else(|| anyhow::anyhow!("no mux"))?;
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow::anyhow!("invalid pane {pane_id}"))?;
                            let lines = term_window.selection_lines(&pane);
                            lines_to_escapes(lines)
                        }
                        tx.try_send(do_it(pane_id, term_window).map_err(|err| format!("{err:#}")))
                            .ok();
                    })));
                let result = rx.recv().await.map_err(mlua::Error::external)?;

                Ok(result)
            },
        );
    }
}
