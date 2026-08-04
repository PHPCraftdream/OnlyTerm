use super::*;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct MuxPane(pub PaneId);

impl MuxPane {
    pub fn resolve(&self, mux: &Arc<Mux>) -> anyhow::Result<Arc<dyn Pane>> {
        mux.get_pane(self.0)
            .ok_or_else(|| anyhow::anyhow!(format!("pane id {} not found in mux", self.0)))
    }
}

// `register_rhai` (the "L4d" rhai port of `MuxPane`'s methods, plus its
// private `get_mux_rhai`/`resolve_rhai`/`SplitPane`/`find_zone`/`*_rhai`
// helpers) used to live below this point. With the scripting layer removed
// it had no remaining caller anywhere in the workspace (only this crate's
// own `tests/rhai_smoke.rs`, which exercised nothing else) and has been
// deleted. Its private `get_text_from_semantic_zone` helper (used only by
// the deleted `get_text_from_semantic_zone_rhai`/`get_text_from_region`
// bindings) and this crate's own `get_mux()` (used only by that helper and
// the other now-deleted `Spawn*::spawn` methods) were dead in turn and have
// been removed too. `MuxPane`'s plain-Rust `resolve` above is unaffected and
// continues to be used directly by `wezterm-gui` (see e.g.
// `crates/wezterm-gui/src/termwindow/mod.rs`).
