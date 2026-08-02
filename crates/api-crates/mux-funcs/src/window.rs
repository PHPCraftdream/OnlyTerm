use super::*;
use parking_lot::{MappedRwLockReadGuard, MappedRwLockWriteGuard};

#[derive(Clone, Copy, Debug)]
pub struct MuxWindow(pub WindowId);

impl MuxWindow {
    pub fn resolve<'a>(
        &self,
        mux: &'a Arc<Mux>,
    ) -> anyhow::Result<MappedRwLockReadGuard<'a, Window>> {
        mux.get_window(self.0)
            .ok_or_else(|| anyhow::anyhow!(format!("window id {} not found in mux", self.0)))
    }

    pub fn resolve_mut<'a>(
        &self,
        mux: &'a Arc<Mux>,
    ) -> anyhow::Result<MappedRwLockWriteGuard<'a, Window>> {
        mux.get_window_mut(self.0)
            .ok_or_else(|| anyhow::anyhow!(format!("window id {} not found in mux", self.0)))
    }
}

// `register_rhai` (the "L4d" rhai port of `MuxWindow`'s methods, plus its
// private `resolve_rhai`/`resolve_mut_rhai` helpers and the
// `set_gui_window_resolver_rhai`/`GUI_WINDOW_RESOLVER_RHAI` weak-hook
// mechanism that only existed to back its `gui_window` binding) used to live
// below this point. `set_gui_window_resolver_rhai` was never actually called
// by `wezterm-gui` (confirmed by grep across the workspace) -- only
// re-exported and exercised by this crate's own `tests/rhai_smoke.rs`. With
// the scripting layer removed, none of it had a remaining caller anywhere in
// the workspace, so it has been deleted. `MuxWindow`'s plain-Rust
// `resolve`/`resolve_mut` above are unaffected.
