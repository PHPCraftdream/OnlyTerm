use bstr::BString;
use smol::prelude::*;

/// Rhai-side registration of `open_with`/`run_child_process`/
/// `background_child_process` (see `docs/plans/2026-07-23-lua-rhai-migration.md`).
///
/// ## Async -> sync
///
/// `run_child_process`/`background_child_process` call `smol::block_on` around
/// the exact same shared async logic (`run_child_process_impl`/
/// `background_child_process_impl`), since rhai has no async execution model.
pub fn register_rhai(engine: &mut rhai::Engine) -> anyhow::Result<()> {
    engine.register_fn("open_with", open_with_rhai);
    engine.register_fn("open_with", open_with_rhai_no_app);
    engine.register_fn("run_child_process", run_child_process_rhai);
    engine.register_fn("background_child_process", background_child_process_rhai);
    Ok(())
}

/// Shared implementation of `open_with`.
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

/// One-argument overload of `open_with(url)` (no explicit app); rhai resolves
/// by arity at the call site.
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

/// rhai analogue of `run_child_process`: same underlying async implementation
/// (`run_child_process_impl`), driven synchronously via `smol::block_on`.
/// Returns a 3-element `Array` `[success: bool, stdout: String, stderr: String]`
/// since rhai has no native multi-return; stdout/stderr are converted lossily
/// to UTF-8 since rhai strings, unlike `BString`, cannot hold arbitrary bytes.
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

/// rhai analogue of `background_child_process`, same shared implementation
/// (`background_child_process_impl`), driven synchronously via `smol::block_on`.
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

/// Converts a rhai `Array` of strings into a `Vec<String>`. The rhai call site
/// takes a single `Array` argument instead: `run_child_process(["ls", "-la"])`.
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
