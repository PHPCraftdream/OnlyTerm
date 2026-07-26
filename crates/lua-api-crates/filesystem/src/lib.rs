use anyhow::anyhow;
use smol::prelude::*;

/// Rhai-side registration of `read_dir`/`glob` (see
/// `docs/plans/2026-07-23-lua-rhai-migration.md`).
///
/// ## Async -> sync
///
/// rhai has no async execution model, so each rhai-facing function calls
/// `smol::block_on` around the exact same shared async logic
/// (`read_dir_impl`/`glob_impl`).
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_fn("read_dir", read_dir_rhai);
    engine.register_fn("glob", glob_rhai);
    engine.register_fn("glob", glob_rhai_no_path);
    Ok(())
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

/// rhai analogue of `read_dir`: same underlying async implementation
/// (`read_dir_impl`), driven to completion synchronously via `smol::block_on`.
fn read_dir_rhai(path: String) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let entries = smol::block_on(read_dir_impl(path))
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("read_dir: {err:#}").into() })?;
    Ok(entries.into_iter().map(rhai::Dynamic::from).collect())
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

/// two-argument form: `glob(pattern, path)`.
fn glob_rhai(
    pattern: String,
    path: String,
) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let entries = smol::block_on(glob_impl(pattern, Some(path)))
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("glob: {err:#}").into() })?;
    Ok(entries.into_iter().map(rhai::Dynamic::from).collect())
}

/// One-argument overload of `glob(pattern)` (path defaults to `"."`).
fn glob_rhai_no_path(pattern: String) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let entries = smol::block_on(glob_impl(pattern, None))
        .map_err(|err| -> Box<rhai::EvalAltResult> { format!("glob: {err:#}").into() })?;
    Ok(entries.into_iter().map(rhai::Dynamic::from).collect())
}
