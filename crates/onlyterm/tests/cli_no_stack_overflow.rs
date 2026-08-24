//! Regression test for a Windows-only debug-build bug where `onlyterm cli
//! <subcommand>` overflowed the process main thread's stack.
//!
//! Root cause: `onlyterm::cli::run_cli` drove a single large async state
//! machine (covering every `CliSubCommand` variant) directly on the calling
//! thread via `promise::spawn::block_on`. In debug builds, without
//! optimizations collapsing the generated future's size, that state machine
//! was large enough to overflow the small (~1MB) default stack that Windows
//! gives a process's main thread. `RUST_MIN_STACK` does not help here because
//! it only affects threads spawned via `std::thread`, not the main thread.
//!
//! The fix runs the CLI's async dispatch on a scoped thread with an 8MB
//! stack. This test does not attempt to reproduce the overflow directly
//! (that depended on debug-profile codegen specifics and this environment's
//! default stack limits); instead it asserts the invariant that must hold
//! either way: invoking the CLI never aborts the process. A stack overflow
//! on Windows terminates the process abnormally (not a normal `Result::Err`
//! exit), so any such regression will fail this test regardless of which
//! exit code the "no server available" error path happens to produce.
use std::process::Command;

fn onlyterm_exe() -> &'static str {
    env!("CARGO_BIN_EXE_onlyterm")
}

#[test]
fn cli_help_does_not_crash() {
    let output = Command::new(onlyterm_exe())
        .args(["cli", "--help"])
        .output()
        .expect("failed to run onlyterm cli --help");

    assert!(
        output.status.success(),
        "onlyterm cli --help did not exit successfully: {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_list_does_not_crash_without_server() {
    // `--no-auto-start` keeps this hermetic: it avoids spawning a real mux
    // server process, so the client's connection attempt is expected to
    // fail. What we're checking is *how* it fails: a clean, non-zero exit
    // with an error message is fine; an abnormal process termination (as
    // caused by a stack overflow) is the regression this test guards
    // against.
    let output = Command::new(onlyterm_exe())
        .args(["cli", "--no-auto-start", "list"])
        .output()
        .expect("failed to run onlyterm cli --no-auto-start list");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("overflowed its stack"),
        "onlyterm cli list overflowed its stack: {}",
        stderr
    );

    #[cfg(windows)]
    {
        // On Windows a stack overflow terminates the process via
        // `__fastfail`/an unhandled exception, which shows up as a status
        // code without the normal "process exited with success/failure"
        // shape that `ExitStatus::code()` returns for a graceful exit.
        // Rust's std reports it as no exit code being available in some
        // configurations, or a very distinctive NTSTATUS-derived code
        // (commonly 0xC00000FD, STATUS_STACK_OVERFLOW). Ensure we didn't
        // hit that.
        const STATUS_STACK_OVERFLOW: i32 = 0xC00000FDu32 as i32;
        assert_ne!(
            output.status.code(),
            Some(STATUS_STACK_OVERFLOW),
            "onlyterm cli list crashed with STATUS_STACK_OVERFLOW: {:?}",
            output.status
        );
    }
}
