//! GuiWin represents a Gui TermWindow (as opposed to a Mux window) in lua code
use crate::termwindow::TermWindowNotif;
use crate::TermWindow;
use config::keyassignment::{ClipboardCopyDestination, KeyAssignment};
use mux::pane::PaneId;
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
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
        format!("GuiWin(mux_window_id:{}, pid:{})", this.mux_window_id, std::process::id())
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

