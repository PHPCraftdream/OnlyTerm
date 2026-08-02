use super::*;
use mux::domain::{Domain, DomainId};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct MuxDomain(pub DomainId);

impl MuxDomain {
    pub fn resolve<'a>(&self, mux: &'a Arc<Mux>) -> anyhow::Result<Arc<dyn Domain>> {
        mux.get_domain(self.0)
            .ok_or_else(|| anyhow::anyhow!(format!("domain id {} not found in mux", self.0)))
    }
}

// `register_rhai` (the "L4d" rhai port of `MuxDomain`'s methods, plus its
// private `get_mux_rhai`/`resolve_rhai` helpers) used to live below this
// point. With the scripting layer removed it had no remaining caller
// anywhere in the workspace (only this crate's own `tests/rhai_smoke.rs`,
// which exercised nothing else) and has been deleted. `MuxDomain`'s
// plain-Rust `resolve` above is unaffected.
