use super::*;
use mux::domain::{Domain, DomainId, DomainState};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct MuxDomain(pub DomainId);

impl MuxDomain {
    pub fn resolve<'a>(&self, mux: &'a Arc<Mux>) -> anyhow::Result<Arc<dyn Domain>> {
        mux.get_domain(self.0)
            .ok_or_else(|| anyhow::anyhow!(format!("domain id {} not found in mux", self.0)))
    }
}

// ---------------------------------------------------------------------------
// L4d: rhai port of the `MuxDomain` UserData above (see
// docs/plans/2026-07-23-lua-rhai-migration.md). Runs in parallel with the mlua
// path; does not replace or touch `impl UserData for MuxDomain`.
//
// `MuxDomain` itself (`#[derive(Clone, Copy, Debug)] pub struct MuxDomain(pub
// DomainId)`) is already engine-agnostic -- only its `UserData` impl above is
// mlua-specific -- so it is reused as-is here, registered as a rhai custom
// type via `register_type_with_name` (the same mechanism every other L4
// crate's `register_rhai` uses, e.g. `share-data::GlobalData`,
// `termwiz-funcs::NerdFonts`), with one `register_fn` per
// `methods.add_method`/`add_async_method` call above. `attach`/`label` are
// async on the mlua side; mirroring the same async -> sync adaptation
// already used for `spawn-funcs`/`filesystem`, they are driven to completion
// via `smol::block_on` here since rhai has no async execution model at all
// (see `config/src/rhai_engine.rs` module doc comment).
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_type_with_name::<MuxDomain>("MuxDomain");

    engine.register_fn("to_string", |this: &mut MuxDomain| -> String {
        format!("MuxDomain(pane_id:{}, pid:{})", this.0, unsafe {
            libc::getpid()
        })
    });
    engine.register_fn(
        "domain_id",
        |this: &mut MuxDomain| -> rhai::INT { this.0 as rhai::INT },
    );

    engine.register_fn(
        "is_spawnable",
        |this: &mut MuxDomain| -> Result<bool, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let domain = resolve_rhai(this, &mux)?;
            Ok(domain.spawnable())
        },
    );

    engine.register_fn(
        "attach",
        |this: &mut MuxDomain, window: MuxWindow| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let domain = resolve_rhai(this, &mux)?;
            smol::block_on(domain.attach(Some(window.0))).map_err(
                |err| -> Box<rhai::EvalAltResult> {
                    format!("failed to attach domain {}: {err:#}", domain.domain_name()).into()
                },
            )
        },
    );
    // Zero-arg overload: `domain.attach()` with no window, mirroring the mlua
    // path's `Option<UserDataRef<MuxWindow>>` parameter -- rhai's
    // `register_fn` has no direct analogue of an optional trailing argument,
    // so instead this registers a second overload under the same name, which
    // rhai resolves by arity at the call site.
    engine.register_fn(
        "attach",
        |this: &mut MuxDomain| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let domain = resolve_rhai(this, &mux)?;
            smol::block_on(domain.attach(None)).map_err(|err| -> Box<rhai::EvalAltResult> {
                format!("failed to attach domain {}: {err:#}", domain.domain_name()).into()
            })
        },
    );

    engine.register_fn(
        "detach",
        |this: &mut MuxDomain| -> Result<(), Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let domain = resolve_rhai(this, &mux)?;
            domain.detach().map_err(|err| -> Box<rhai::EvalAltResult> {
                format!("failed to detach domain {}: {err:#}", domain.domain_name()).into()
            })
        },
    );

    engine.register_fn(
        "state",
        |this: &mut MuxDomain| -> Result<String, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let domain = resolve_rhai(this, &mux)?;
            Ok(match domain.state() {
                DomainState::Attached => "Attached",
                DomainState::Detached => "Detached",
            }
            .to_string())
        },
    );

    engine.register_fn(
        "name",
        |this: &mut MuxDomain| -> Result<String, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let domain = resolve_rhai(this, &mux)?;
            Ok(domain.domain_name().to_string())
        },
    );

    engine.register_fn(
        "label",
        |this: &mut MuxDomain| -> Result<String, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let domain = resolve_rhai(this, &mux)?;
            Ok(smol::block_on(domain.domain_label()))
        },
    );

    engine.register_fn(
        "has_any_panes",
        |this: &mut MuxDomain| -> Result<bool, Box<rhai::EvalAltResult>> {
            let mux = get_mux_rhai()?;
            let domain = resolve_rhai(this, &mux)?;
            let have_panes_in_domain = mux
                .iter_panes()
                .iter()
                .any(|p| p.domain_id() == domain.domain_id());
            Ok(have_panes_in_domain)
        },
    );

    Ok(())
}

/// rhai-flavored equivalent of `get_mux()` (defined in `lib.rs` for the mlua
/// path): same underlying `Mux::try_get()`, just returning a rhai-compatible
/// error type instead of `mlua::Error`.
pub(crate) fn get_mux_rhai() -> Result<Arc<Mux>, Box<rhai::EvalAltResult>> {
    Mux::try_get().ok_or_else(|| "cannot get Mux!?".into())
}

/// rhai-flavored equivalent of `MuxDomain::resolve`, used by every method
/// registered above, and also by `lib.rs`'s `set_default_domain` binding
/// (which lives outside this module).
pub(crate) fn resolve_rhai(
    this: &MuxDomain,
    mux: &Arc<Mux>,
) -> Result<Arc<dyn Domain>, Box<rhai::EvalAltResult>> {
    mux.get_domain(this.0)
        .ok_or_else(|| format!("domain id {} not found in mux", this.0).into())
}
