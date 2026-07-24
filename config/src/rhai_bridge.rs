//! Rust-side entry points for the `wezterm.on`/`emit` runtime event-callback
//! bridge, now backed entirely by rhai (L4.6 of
//! `docs/plans/2026-07-23-lua-rhai-migration.md`).
//!
//! This module is the direct replacement for `config::lua::emit_sync_callback`/
//! `emit_event`/`emit_async_callback`, which took an `&mlua::Lua` (the companion
//! mlua context described in `RhaiEventScript`'s doc comment in
//! `config/src/rhai_engine.rs`) plus Lua-native argument/return types. The
//! functions here instead take a [`crate::rhai_engine::RhaiConfigState`] (the live,
//! main-thread-only rhai engine + compiled config-script `AST`, obtained via
//! `config::with_rhai_config_on_main_thread`/`run_immediate_with_rhai_config`) plus
//! `rhai::Dynamic` arguments/return values -- or, for callers that don't want to
//! depend on the `rhai` crate directly, `wezterm_dynamic::Value` overloads that go
//! through the existing `config::rhai_value` bridge.
//!
//! ## Async
//!
//! Just like the rhai engine construction itself (see the "Async" section of
//! `config/src/rhai_engine.rs`'s module doc comment), rhai has no native async
//! execution model, so [`emit_event`] (the async-callable replacement for the old
//! `emit_event`/`create_async_function`-based Lua path) runs the handler chain
//! synchronously under the hood and simply wraps the result in a already-ready
//! `Future`. This is a genuine, documented semantic difference from the mlua path
//! (which could `.await` other async Lua functions from within a handler), not a
//! cosmetic one -- see the migration plan's L2 risk notes, carried over unchanged
//! by this task.
use crate::rhai_engine::RhaiConfigState;
use rhai::Dynamic;

/// Look up and invoke the *first* registered handler for `name`, returning its
/// result (or `Dynamic::UNIT` if there are no handlers, or the name was never
/// registered). Mirrors `config::lua::emit_sync_callback`'s "first handler wins"
/// contract exactly (the Lua version's `for func in tbl.sequence_values(..) {
/// return func.call(args); }` only ever calls the first registered function too,
/// despite the table supporting multiple -- this is intentional pre-existing
/// behavior for the small number of callers that use the sync variant for a
/// single-answer callback, e.g. `mux-is-process-stateful`/`format-tab-title`, as
/// opposed to [`emit_event`]'s true multi-handler chain).
pub fn emit_sync_callback(
    state: &RhaiConfigState,
    name: &str,
    args: Vec<Dynamic>,
) -> anyhow::Result<Dynamic> {
    let handlers = state.engine.events.first_handler(name);
    match handlers {
        Some(handler) => handler
            .call(&state.engine.engine, &state.ast, args)
            .map_err(|e| anyhow::anyhow!("{e}")),
        None => Ok(Dynamic::UNIT),
    }
}

/// Async-callable analogue of [`emit_sync_callback`], mirroring
/// `config::lua::emit_async_callback`. rhai has no async execution model (see the
/// module doc comment), so this runs synchronously and immediately resolves; it
/// exists as `async fn` purely so call sites that `.await` it (mirroring the old
/// `emit_async_callback(...).await` shape) don't need restructuring.
pub async fn emit_async_callback(
    state: &RhaiConfigState,
    name: &str,
    args: Vec<Dynamic>,
) -> anyhow::Result<Dynamic> {
    emit_sync_callback(state, name, args)
}

/// Invoke *every* registered handler for `name`, in registration order, exactly
/// like `config::lua::emit_event`/`wezterm.emit`: if a handler returns `false`, no
/// further handlers run and this itself returns `false` ("prevent the default
/// action"); otherwise (including when there are no handlers at all) this returns
/// `true`.
///
/// `async fn` for the same call-site-compatibility reason as
/// [`emit_async_callback`] -- there is no actual suspension point.
pub async fn emit_event(state: &RhaiConfigState, name: &str, args: Vec<Dynamic>) -> anyhow::Result<bool> {
    state
        .emit_sync(name, args)
        .map_err(|e| anyhow::anyhow!("{e}"))
}
