use super::*;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct MuxTab(pub TabId);

impl MuxTab {
    pub fn resolve(&self, mux: &Arc<Mux>) -> anyhow::Result<Arc<Tab>> {
        mux.get_tab(self.0)
            .ok_or_else(|| anyhow::anyhow!(format!("tab id {} not found in mux", self.0)))
    }
}

// `register_rhai` (the "L4d" rhai port of `MuxTab`'s methods, plus its
// private `get_mux_rhai`/`resolve_rhai` helpers) used to live below this
// point. With the scripting layer removed it had no remaining caller
// anywhere in the workspace (only this crate's own `tests/rhai_smoke.rs`,
// which exercised nothing else) and has been deleted. `MuxTab`'s plain-Rust
// `resolve` above is unaffected.
