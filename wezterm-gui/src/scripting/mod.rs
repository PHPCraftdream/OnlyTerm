/// L4.6: registers the rhai-side types this crate exposes to the event-callback
/// bridge (`wezterm.on`/`emit`, see `config::rhai_bridge`). Wired up via
/// `config::rhai_engine::add_rhai_setup_func` in `wezterm-gui/src/main.rs`.
pub mod guiwin;

pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    guiwin::register_rhai(engine)
}
