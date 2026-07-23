use anyhow::anyhow;
use config::lua::get_or_create_module;
use config::lua::mlua::{self, Lua};
use smol::prelude::*;

pub fn register(lua: &Lua) -> anyhow::Result<()> {
    let wezterm_mod = get_or_create_module(lua, "wezterm")?;
    wezterm_mod.set("read_dir", lua.create_async_function(read_dir)?)?;
    wezterm_mod.set("glob", lua.create_async_function(glob)?)?;
    Ok(())
}

/// Rhai-side analogue of `register` above (L4a, see
/// `docs/plans/2026-07-23-lua-rhai-migration.md`). Registers `read_dir`/`glob` as
/// top-level functions on the engine, mirroring `wezterm.read_dir(...)`/
/// `wezterm.glob(...)` on the mlua path (both are set directly on the `wezterm`
/// table there too, not under a sub-module, so this registers bare global
/// functions rather than using `register_static_module`).
///
/// ## Async -> sync
///
/// The mlua bindings are `create_async_function`s (`smol::fs::read_dir`,
/// `smol::unblock`), because Lua callbacks in wezterm's config pipeline can
/// themselves be `async`. rhai has no async execution model at all -- see the
/// module-level doc comment in `config/src/rhai_engine.rs` -- so there is no
/// async counterpart to port here; instead each rhai-facing function calls
/// `smol::block_on` around the exact same shared async logic
/// (`read_dir_impl`/`glob_impl`) that the mlua path awaits, so the actual
/// filesystem-walking behavior can't drift between the two bindings even though
/// their execution model differs.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_fn("read_dir", read_dir_rhai);
    engine.register_fn("glob", glob_rhai);
    engine.register_fn("glob", glob_rhai_no_path);
    Ok(())
}

async fn read_dir<'lua>(_: &'lua Lua, path: String) -> mlua::Result<Vec<String>> {
    read_dir_impl(path).await.map_err(mlua::Error::external)
}

async fn read_dir_impl(path: String) -> anyhow::Result<Vec<String>> {
    let mut dir = smol::fs::read_dir(path).await?;
    let mut entries = vec![];
    while let Some(entry) = dir.next().await {
        let entry = entry?;
        if let Some(utf8) = entry.path().to_str() {
            entries.push(utf8.to_string());
        } else {
            return Err(anyhow!(
                "path entry {} is not representable as utf8",
                entry.path().display()
            ));
        }
    }
    Ok(entries)
}

/// rhai analogue of `read_dir` above: same underlying async implementation
/// (`read_dir_impl`), driven to completion synchronously via `smol::block_on`
/// since rhai-registered functions must be plain synchronous Rust functions.
fn read_dir_rhai(path: String) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let entries = smol::block_on(read_dir_impl(path))
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("read_dir: {err:#}").into() })?;
    Ok(entries.into_iter().map(rhai::Dynamic::from).collect())
}

async fn glob<'lua>(
    _: &'lua Lua,
    (pattern, path): (String, Option<String>),
) -> mlua::Result<Vec<String>> {
    glob_impl(pattern, path).await.map_err(mlua::Error::external)
}

async fn glob_impl(pattern: String, path: Option<String>) -> anyhow::Result<Vec<String>> {
    let entries = smol::unblock(move || {
        let mut entries = vec![];
        let glob = filenamegen::Glob::new(&pattern)?;
        for path in glob.walk(path.as_deref().unwrap_or(".")) {
            if let Some(utf8) = path.to_str() {
                entries.push(utf8.to_string());
            } else {
                return Err(anyhow!(
                    "path entry {} is not representable as utf8",
                    path.display()
                ));
            }
        }
        Ok(entries)
    })
    .await?;
    Ok(entries)
}

/// rhai analogue of `glob` above (two-argument form: `glob(pattern, path)`), same
/// shared implementation (`glob_impl`) as the mlua path, driven synchronously via
/// `smol::block_on`.
fn glob_rhai(
    pattern: String,
    path: String,
) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let entries = smol::block_on(glob_impl(pattern, Some(path)))
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("glob: {err:#}").into() })?;
    Ok(entries.into_iter().map(rhai::Dynamic::from).collect())
}

/// One-argument overload of `glob(pattern)` (path defaults to `"."`), mirroring
/// the mlua binding's `Option<String>` parameter -- rhai's `register_fn` has no
/// direct analogue of an optional trailing Lua argument, so instead this
/// registers a second overload under the same name, which rhai resolves by
/// arity at the call site (`glob("*.txt")` vs `glob("*.txt", "/some/dir")`).
fn glob_rhai_no_path(pattern: String) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let entries = smol::block_on(glob_impl(pattern, None))
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("glob: {err:#}").into() })?;
    Ok(entries.into_iter().map(rhai::Dynamic::from).collect())
}
