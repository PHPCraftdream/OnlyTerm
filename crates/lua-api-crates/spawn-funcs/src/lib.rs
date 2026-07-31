use bstr::BString;
use smol::prelude::*;
use std::time::Duration;

/// Rhai-side registration of `open_with`/`run_child_process`/
/// `background_child_process` (see `docs/plans/2026-07-23-lua-rhai-migration.md`).
///
/// ## Async -> sync
///
/// `run_child_process`/`background_child_process` call `smol::block_on` around
/// the exact same shared async logic (`run_child_process_impl`/
/// `background_child_process_impl`), since rhai has no async execution model.
///
/// ## Timeout
///
/// `run_child_process` is reachable synchronously on the GUI thread from
/// `format-tab-title`/`format-window-title`/`update-status` event callbacks
/// (the classic "shell out to git/kubectl for the status bar" recipe), so a
/// hung child process would otherwise freeze every window forever. Its
/// `smol::block_on` call is bounded by racing the child-process future
/// against a `smol::Timer` (see `run_with_timeout`), configured via
/// `child_process_timeout_ms`. On timeout, the child is killed via
/// `Command::kill_on_drop(true)` (dropping the losing future's `Child`/
/// `ChildGuard` forcibly kills the still-running process rather than
/// leaving it as an orphan) and an error is returned to the script instead
/// of hanging.
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

    // If this future is dropped before it completes (see `run_with_timeout`),
    // the child's `Child`/`ChildGuard` gets dropped along with it, which
    // forcibly kills the process rather than leaving it running as an
    // orphan.
    cmd.kill_on_drop(true);

    let output = cmd.output().await?;

    Ok((
        output.status.success(),
        output.stdout.into(),
        output.stderr.into(),
    ))
}

/// Races `fut` against a timer of `timeout_ms` milliseconds. If the timer
/// wins, `fut` is dropped (killing any child process it owns, per
/// `Command::kill_on_drop`) and `Err` is returned instead of blocking
/// forever. A `timeout_ms` of 0 disables the timeout and waits on `fut`
/// indefinitely.
///
/// This exists because `run_child_process` is reachable synchronously on the
/// GUI thread (via `format-tab-title`/`format-window-title`/`update-status`
/// callbacks, which have no async execution model of their own), so an
/// unbounded wait on a hung child process (a stuck `git`, a wedged
/// `wsl.exe`, a command touching a stalled network mount, etc.) would
/// otherwise freeze every window in the process forever.
///
/// Split out from `run_with_timeout` (which reads the `timeout_ms` value
/// from `child_process_timeout_ms` in the live config) so tests can drive
/// this with an explicit short duration instead of depending on/mutating
/// global config state.
async fn race_with_timeout<T>(
    what: &str,
    timeout_ms: u64,
    fut: impl Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    if timeout_ms == 0 {
        return fut.await;
    }

    let timed_out = async {
        smol::Timer::after(Duration::from_millis(timeout_ms)).await;
        anyhow::bail!("{what}: timed out after {timeout_ms}ms (see child_process_timeout_ms)")
    };

    smol::future::or(fut, timed_out).await
}

/// Convenience wrapper around `race_with_timeout` that reads the timeout
/// from the live `child_process_timeout_ms` config value.
async fn run_with_timeout<T>(
    what: &str,
    fut: impl Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    let timeout_ms = config::configuration().child_process_timeout_ms;
    race_with_timeout(what, timeout_ms, fut).await
}

/// rhai analogue of `run_child_process`: same underlying async implementation
/// (`run_child_process_impl`), driven synchronously via `smol::block_on` and
/// bounded by `run_with_timeout`/`child_process_timeout_ms`.
/// Returns a 3-element `Array` `[success: bool, stdout: String, stderr: String]`
/// since rhai has no native multi-return; stdout/stderr are converted lossily
/// to UTF-8 since rhai strings, unlike `BString`, cannot hold arbitrary bytes.
fn run_child_process_rhai(args: rhai::Array) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    let args = array_to_string_vec(args, "run_child_process")?;
    if args.is_empty() {
        return Err("run_child_process: expected at least one argument (the command)".into());
    }
    let (success, stdout, stderr) = smol::block_on(run_with_timeout(
        "run_child_process",
        run_child_process_impl(args),
    ))
    .map_err(|err| -> Box<rhai::EvalAltResult> { format!("run_child_process: {err:#}").into() })?;
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

#[cfg(test)]
mod timeout_tests {
    use super::*;
    use std::time::Instant;

    /// Builds a command that deliberately blocks for several seconds:
    /// `ping -n 20 127.0.0.1` on Windows (~19s), `sleep 20` elsewhere.
    /// Either way, comfortably longer than the short timeout used below, so
    /// a real (not simulated) hang is exercised end-to-end through
    /// `run_child_process_impl`/`race_with_timeout`, including the
    /// `kill_on_drop` cleanup path.
    fn hang_forever_args() -> Vec<String> {
        #[cfg(windows)]
        {
            vec![
                "ping".to_string(),
                "-n".to_string(),
                "20".to_string(),
                "127.0.0.1".to_string(),
            ]
        }
        #[cfg(not(windows))]
        {
            vec!["sleep".to_string(), "20".to_string()]
        }
    }

    /// A hung child process must not be allowed to block
    /// `run_child_process` forever: with a short timeout configured, the
    /// call should return an error within roughly that timeout (not the
    /// full ~19-20s the child would otherwise run for), and the error
    /// should say it was a timeout.
    #[test]
    fn run_child_process_impl_times_out_instead_of_hanging() {
        let timeout_ms = 500u64;
        let started = Instant::now();

        let result = smol::block_on(race_with_timeout(
            "run_child_process",
            timeout_ms,
            run_child_process_impl(hang_forever_args()),
        ));

        let elapsed = started.elapsed();

        assert!(
            result.is_err(),
            "expected a timeout error, got success: {result:?}"
        );
        let message = format!("{:#}", result.unwrap_err());
        assert!(
            message.contains("timed out"),
            "expected a timeout error message, got: {message}"
        );

        // Generous upper bound to absorb CI/test-machine scheduling jitter
        // while still being nowhere near the ~19-20s the child would run
        // for if it weren't killed on timeout.
        assert!(
            elapsed < Duration::from_secs(10),
            "run_child_process took {elapsed:?}, expected it to return promptly after the \
             {timeout_ms}ms timeout instead of waiting for the full hang duration"
        );
    }

    /// With no timeout configured (0 disables it), a quick command still
    /// completes normally and returns its real output -- the timeout
    /// plumbing must not interfere with the non-hanging, common case.
    #[test]
    fn race_with_timeout_disabled_still_returns_real_result() {
        #[cfg(windows)]
        let args = vec!["cmd".to_string(), "/C".to_string(), "echo hello".to_string()];
        #[cfg(not(windows))]
        let args = vec!["echo".to_string(), "hello".to_string()];

        let (success, stdout, _stderr) = smol::block_on(race_with_timeout(
            "run_child_process",
            0,
            run_child_process_impl(args),
        ))
        .expect("quick command should succeed");

        assert!(success);
        assert!(
            String::from_utf8_lossy(&stdout).contains("hello"),
            "stdout: {stdout:?}"
        );
    }

    /// A command that finishes well within the timeout should succeed
    /// normally, with the timeout race not affecting its result.
    #[test]
    fn race_with_timeout_lets_fast_command_through() {
        #[cfg(windows)]
        let args = vec!["cmd".to_string(), "/C".to_string(), "echo hello".to_string()];
        #[cfg(not(windows))]
        let args = vec!["echo".to_string(), "hello".to_string()];

        let (success, stdout, _stderr) = smol::block_on(race_with_timeout(
            "run_child_process",
            5_000,
            run_child_process_impl(args),
        ))
        .expect("quick command should succeed within a generous timeout");

        assert!(success);
        assert!(
            String::from_utf8_lossy(&stdout).contains("hello"),
            "stdout: {stdout:?}"
        );
    }
}
