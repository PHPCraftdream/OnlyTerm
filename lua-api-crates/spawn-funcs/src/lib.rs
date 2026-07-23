use bstr::BString;
use config::lua::get_or_create_module;
use config::lua::mlua::{self, Lua};

pub fn register(lua: &Lua) -> anyhow::Result<()> {
    let wezterm_mod = get_or_create_module(lua, "wezterm")?;
    wezterm_mod.set("open_with", lua.create_function(open_with)?)?;
    wezterm_mod.set(
        "run_child_process",
        lua.create_async_function(run_child_process)?,
    )?;
    wezterm_mod.set(
        "background_child_process",
        lua.create_async_function(background_child_process)?,
    )?;
    Ok(())
}

fn open_with<'lua>(_: &'lua Lua, (url, app): (String, Option<String>)) -> mlua::Result<()> {
    if let Some(app) = app {
        wezterm_open_url::open_with(&url, &app);
    } else {
        wezterm_open_url::open_url(&url);
    }
    Ok(())
}

async fn run_child_process<'lua>(
    _: &'lua Lua,
    args: Vec<String>,
) -> mlua::Result<(bool, BString, BString)> {
    let mut cmd = smol::process::Command::new(&args[0]);

    if args.len() > 1 {
        cmd.args(&args[1..]);
    }

    #[cfg(windows)]
    {
        use smol::process::windows::CommandExt;
        cmd.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
    }

    let output = cmd.output().await.map_err(mlua::Error::external)?;

    Ok((
        output.status.success(),
        output.stdout.into(),
        output.stderr.into(),
    ))
}

async fn background_child_process<'lua>(_: &'lua Lua, args: Vec<String>) -> mlua::Result<()> {
    let mut cmd = smol::process::Command::new(&args[0]);

    if args.len() > 1 {
        cmd.args(&args[1..]);
    }

    #[cfg(windows)]
    {
        use smol::process::windows::CommandExt;
        cmd.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
    }

    cmd.stdin(smol::process::Stdio::null())
        .spawn()
        .map_err(mlua::Error::external)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// L4c: rhai port of `register` above (see
// docs/plans/2026-07-23-lua-rhai-migration.md). Runs in parallel with the mlua
// path; does not replace or touch `register(lua: &Lua)`.
//
// ## Async -> sync
//
// `run_child_process`/`background_child_process` are `create_async_function`s
// on the mlua path because Lua callbacks in wezterm's config pipeline can
// themselves be `async`. rhai has no async execution model at all (see the
// module-level doc comment in `config/src/rhai_engine.rs`), so -- mirroring
// the same pattern already used for `lua-api-crates/filesystem`'s
// `read_dir`/`glob` -- each rhai-facing function here calls `smol::block_on`
// around the exact same shared async logic (`run_child_process_impl`/
// `background_child_process_impl`) that the mlua path awaits, so process
// spawning behavior can't drift between the two bindings even though their
// execution model differs.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_fn("open_with", open_with_rhai);
    engine.register_fn("open_with", open_with_rhai_no_app);
    engine.register_fn("run_child_process", run_child_process_rhai);
    engine.register_fn("background_child_process", background_child_process_rhai);
    Ok(())
}

/// Shared, engine-agnostic implementation of `open_with`, used by both the
/// mlua binding above and the rhai binding below so the two engines can't
/// drift in behavior.
fn open_with_impl(url: &str, app: Option<&str>) {
    if let Some(app) = app {
        wezterm_open_url::open_with(url, app);
    } else {
        wezterm_open_url::open_url(url);
    }
}

fn open_with_rhai(url: String, app: String) {
    open_with_impl(&url, Some(&app));
}

/// One-argument overload of `open_with(url)` (no explicit app), mirroring the
/// mlua binding's `Option<String>` parameter -- rhai's `register_fn` has no
/// direct analogue of an optional trailing Lua argument, so instead this
/// registers a second overload under the same name, which rhai resolves by
/// arity at the call site (`open_with("https://...")` vs
/// `open_with("https://...", "firefox")`).
fn open_with_rhai_no_app(url: String) {
    open_with_impl(&url, None);
}

async fn run_child_process_impl(args: Vec<String>) -> anyhow::Result<(bool, BString, BString)> {
    let mut cmd = smol::process::Command::new(&args[0]);

    if args.len() > 1 {
        cmd.args(&args[1..]);
    }

    #[cfg(windows)]
    {
        use smol::process::windows::CommandExt;
        cmd.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
    }

    let output = cmd.output().await?;

    Ok((
        output.status.success(),
        output.stdout.into(),
        output.stderr.into(),
    ))
}

/// rhai analogue of `run_child_process` above: same underlying async
/// implementation (`run_child_process_impl`), driven to completion
/// synchronously via `smol::block_on` since rhai-registered functions must be
/// plain synchronous Rust functions. Returns a 3-element `Array`
/// `[success: bool, stdout: String, stderr: String]` since rhai has no native
/// multi-return (mirroring how `color-funcs` handles multi-value mlua
/// returns); stdout/stderr are converted lossily to UTF-8 (`String::from_utf8_lossy`)
/// since rhai strings, unlike `BString`, cannot hold arbitrary bytes.
fn run_child_process_rhai(args: rhai::Array) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let args = array_to_string_vec(args, "run_child_process")?;
    if args.is_empty() {
        return Err("run_child_process: expected at least one argument (the command)".into());
    }
    let (success, stdout, stderr) = smol::block_on(run_child_process_impl(args))
        .map_err(|err| -> Box<rhai::EvalAltResult> {
            format!("run_child_process: {err:#}").into()
        })?;
    Ok(vec![
        rhai::Dynamic::from_bool(success),
        rhai::Dynamic::from(String::from_utf8_lossy(&stdout).into_owned()),
        rhai::Dynamic::from(String::from_utf8_lossy(&stderr).into_owned()),
    ])
}

async fn background_child_process_impl(args: Vec<String>) -> anyhow::Result<()> {
    let mut cmd = smol::process::Command::new(&args[0]);

    if args.len() > 1 {
        cmd.args(&args[1..]);
    }

    #[cfg(windows)]
    {
        use smol::process::windows::CommandExt;
        cmd.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
    }

    cmd.stdin(smol::process::Stdio::null()).spawn()?;

    Ok(())
}

/// rhai analogue of `background_child_process` above, same shared
/// implementation (`background_child_process_impl`) as the mlua path, driven
/// synchronously via `smol::block_on`.
fn background_child_process_rhai(args: rhai::Array) -> Result<(), Box<rhai::EvalAltResult>> {
    let args = array_to_string_vec(args, "background_child_process")?;
    if args.is_empty() {
        return Err(
            "background_child_process: expected at least one argument (the command)".into(),
        );
    }
    smol::block_on(background_child_process_impl(args)).map_err(
        |err| -> Box<rhai::EvalAltResult> {
            format!("background_child_process: {err:#}").into()
        },
    )
}

/// Converts a rhai `Array` of strings into a `Vec<String>`, since
/// `run_child_process`/`background_child_process` take a variadic-ish arg
/// list (`Vec<String>` on the mlua side, filled in by Lua's own multi-arg
/// call convention). rhai's `register_fn` only supports fixed arity, so -
/// mirroring the same choice already made in `lua-api-crates/logging` for
/// `log_error`/`log_info`/`log_warn` - the rhai call site takes a single
/// `Array` argument instead: `run_child_process(["ls", "-la"])`.
fn array_to_string_vec(
    args: rhai::Array,
    what: &str,
) -> Result<Vec<String>, Box<rhai::EvalAltResult>> {
    args.into_iter()
        .map(|item| {
            item.into_string().map_err(|ty| -> Box<rhai::EvalAltResult> {
                format!("{what}: expected an array of strings, found `{ty}`").into()
            })
        })
        .collect()
}
