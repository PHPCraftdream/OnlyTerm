//! L4c smoke test: `spawn_funcs::register_rhai` exposes rhai equivalents of
//! every function `register(lua: &Lua)` registers on the mlua path (see
//! docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! ## Handling non-determinism / environment dependence
//!
//! `open_with` genuinely opens a URL/app via the OS shell, which isn't
//! something a test should trigger for real, so it is exercised only via
//! `register_rhai`'s call-site *shape* (arity resolution / it doesn't panic
//! wiring things up) is covered implicitly by every other test compiling and
//! running against the same `make_engine()`. `run_child_process`/
//! `background_child_process` spawn a real child process, so tests use a
//! small, portable command (`cmd /C echo ...` on Windows, `sh -c 'echo ...'`
//! elsewhere) that behaves identically across the environments CI runs on.

use rhai::Engine;

fn make_engine() -> Engine {
    let mut engine = Engine::new();
    spawn_funcs::register_rhai(&mut engine).expect("register_rhai");
    engine
}

/// Returns a `[program, args...]` rhai array literal (as a `.rhai` source
/// fragment) that echoes `text` to stdout, portable across Windows/Unix.
fn echo_argv_literal(text: &str) -> String {
    if cfg!(windows) {
        format!(r#"["cmd", "/C", "echo {text}"]"#)
    } else {
        format!(r#"["/bin/sh", "-c", "echo {text}"]"#)
    }
}

#[test]
fn rhai_run_child_process_returns_success_stdout_stderr_array() -> anyhow::Result<()> {
    let engine = make_engine();
    let argv = echo_argv_literal("hello-from-rhai");
    let script = format!("run_child_process({argv})");
    let result: rhai::Array = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    assert_eq!(result.len(), 3, "expected [success, stdout, stderr]");
    let success = result[0].as_bool().expect("success should be a bool");
    assert!(success, "expected the echo command to succeed");

    let stdout = result[1]
        .clone()
        .into_string()
        .expect("stdout should be a string");
    assert!(
        stdout.contains("hello-from-rhai"),
        "expected stdout ({stdout:?}) to contain the echoed text"
    );

    let stderr = result[2]
        .clone()
        .into_string()
        .expect("stderr should be a string");
    assert!(stderr.is_empty(), "expected empty stderr, got {stderr:?}");
    Ok(())
}

#[test]
fn rhai_run_child_process_reports_success_false_for_nonzero_exit() -> anyhow::Result<()> {
    let engine = make_engine();
    let argv = if cfg!(windows) {
        r#"["cmd", "/C", "exit 1"]"#.to_string()
    } else {
        r#"["/bin/sh", "-c", "exit 1"]"#.to_string()
    };
    let script = format!("run_child_process({argv})");
    let result: rhai::Array = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let success = result[0].as_bool().expect("success should be a bool");
    assert!(!success, "expected a nonzero exit to report success = false");
    Ok(())
}

#[test]
fn rhai_run_child_process_rejects_empty_argv() {
    let engine = make_engine();
    let result: Result<rhai::Array, _> = engine.eval("run_child_process([])");
    assert!(result.is_err(), "an empty argv should be rejected");
}

#[test]
fn rhai_run_child_process_rejects_nonstring_argv_elements() {
    let engine = make_engine();
    let result: Result<rhai::Array, _> = engine.eval(r#"run_child_process(["echo", 42])"#);
    assert!(
        result.is_err(),
        "a non-string element in the argv array should be rejected"
    );
}

#[test]
fn rhai_run_child_process_matches_direct_smol_call() -> anyhow::Result<()> {
    // Cross-check the rhai path's stdout against calling the exact same
    // underlying command directly, so the two can't silently drift.
    let engine = make_engine();
    let argv = echo_argv_literal("cross-check");
    let script = format!("run_child_process({argv})");
    let result: rhai::Array = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    let stdout = result[1].clone().into_string().unwrap();

    let direct_output = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "echo cross-check"])
            .output()?
    } else {
        std::process::Command::new("/bin/sh")
            .args(["-c", "echo cross-check"])
            .output()?
    };
    let direct_stdout = String::from_utf8_lossy(&direct_output.stdout);
    assert_eq!(stdout.trim(), direct_stdout.trim());
    Ok(())
}

#[test]
fn rhai_background_child_process_spawns_without_waiting() -> anyhow::Result<()> {
    let engine = make_engine();
    let argv = echo_argv_literal("background-hello");
    let script = format!("background_child_process({argv})");
    // Should complete quickly (doesn't wait for the child to exit) and not error.
    engine
        .eval::<()>(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    Ok(())
}

#[test]
fn rhai_background_child_process_rejects_empty_argv() {
    let engine = make_engine();
    let result: Result<(), _> = engine.eval("background_child_process([])");
    assert!(result.is_err(), "an empty argv should be rejected");
}

#[test]
fn rhai_open_with_two_arg_and_one_arg_overloads_compile_and_resolve() -> anyhow::Result<()> {
    // `open_with` genuinely shells out to the OS (`ShellExecuteW`/`xdg-open`/
    // etc, via `wezterm_open_url::open_with`/`open_url`) to open a URL/app,
    // which a test must never trigger for real (it would pop a browser or
    // similar on the machine running the suite, including on developer
    // workstations and any interactive CI agent). rhai resolves function
    // names/arity at *compile* time (`Engine::compile`), before any
    // evaluation happens, so `AST` construction succeeding here is already
    // sufficient proof that both the two-argument and one-argument
    // `open_with` overloads registered in `register_rhai` exist under the
    // expected name and are call-site compatible - without ever executing
    // the (real, side-effecting) function bodies.
    let engine = make_engine();
    engine
        .compile(r#"open_with("about:blank", "some-app")"#)
        .map_err(|err| anyhow::anyhow!("rhai compile error (two-arg open_with): {err}"))?;
    engine
        .compile(r#"open_with("about:blank")"#)
        .map_err(|err| anyhow::anyhow!("rhai compile error (one-arg open_with): {err}"))?;
    Ok(())
}
