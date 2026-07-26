//! L4b smoke test: `procinfo_funcs::register_rhai` exposes rhai equivalents of
//! every function `register(lua: &Lua)` registers on the mlua path (see
//! docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! ## Handling non-determinism
//!
//! Everything in this crate reflects live OS process state (the calling
//! process's own pid, its cwd, its executable path, and -- for arbitrary pids
//! -- whatever happens to be running on the machine at test time). None of
//! that is safe to hardcode:
//!   * `procinfo::pid()` is only meaningfully comparable to
//!     `std::process::id()` (this test binary's own real pid) -- not to a
//!     fixed literal.
//!   * `procinfo::get_info_for_pid(pid)`/`current_working_dir_for_pid`/
//!     `executable_path_for_pid`, called with this process's own pid, are
//!     asserted on *shape* (right field types/non-empty where meaningful)
//!     rather than fixed values, since the exact cwd/executable path is
//!     wherever `cargo test` happens to run from -- consistent with how L4a's
//!     `battery`/`filesystem` crates already handle environment-dependent
//!     results (see their smoke-test doc comments) for the same reason.
//!   * a deliberately-implausible pid is used to exercise the "no such
//!     process" `None`/`()` path, which is deterministic (that pid should
//!     never legitimately exist), without asserting on any specific
//!     "not found" value's content.

use rhai::Engine;

fn make_engine() -> Engine {
    let mut engine = Engine::new();
    procinfo_funcs::register_rhai(&mut engine).expect("register_rhai");
    engine
}

#[test]
fn rhai_procinfo_pid_matches_current_process_id() -> anyhow::Result<()> {
    let engine = make_engine();
    let result: rhai::INT = engine
        .eval("procinfo::pid()")
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;
    assert_eq!(result as u32, std::process::id());
    Ok(())
}

#[test]
fn rhai_procinfo_get_info_for_pid_returns_reasonable_shape_for_self() -> anyhow::Result<()> {
    let engine = make_engine();
    let pid = std::process::id();
    let script = format!("procinfo::get_info_for_pid({pid})");
    let result: rhai::Map = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    // Cross-check against calling the exact same underlying Rust function
    // directly, rather than hardcoding expected field values (which would be
    // brittle across machines/CI/working directories).
    let expected =
        procinfo::LocalProcessInfo::with_root_pid(pid).expect("current process should be found");

    assert_eq!(
        result.get("pid").unwrap().as_int().unwrap() as u32,
        expected.pid
    );
    assert_eq!(
        result
            .get("name")
            .unwrap()
            .clone()
            .into_string()
            .unwrap(),
        expected.name
    );
    assert!(result.contains_key("executable"));
    assert!(result.contains_key("argv"));
    assert!(result.contains_key("status"));
    assert!(result.contains_key("children"));
    Ok(())
}

#[test]
fn rhai_procinfo_get_info_for_pid_returns_unit_for_a_nonexistent_pid() -> anyhow::Result<()> {
    let engine = make_engine();
    // Picking a pid that is guaranteed to not resolve to a process is
    // platform-dependent in a way this test shouldn't hardcode: pid 0 looks
    // like an obviously-unused/reserved value, but on Windows it is the
    // (enumerable) System Idle Process, so `with_root_pid(0)` legitimately
    // returns `Some(...)` there while returning `None` on Linux/macOS. Rather
    // than special-case pid 0 per-platform, this cross-checks the rhai
    // binding against calling the exact same underlying Rust function
    // directly for a pid deliberately chosen to be implausible everywhere
    // (`u32::MAX - 1`, far outside any real OS's live pid range), so the test
    // asserts "rhai agrees with Rust" rather than "pid X is definitely free",
    // and can't spuriously fail on a platform where a low reserved pid
    // happens to resolve.
    let pid = u32::MAX - 1;
    let script = format!("procinfo::get_info_for_pid({pid})");
    let result: rhai::Dynamic = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let expected = procinfo::LocalProcessInfo::with_root_pid(pid);
    match expected {
        None => assert!(
            result.is_unit(),
            "expected `()` for a pid with no matching process, like the mlua path's `nil`"
        ),
        Some(_) => {
            // Implausible on any real machine, but if it somehow does exist,
            // just confirm the rhai side agrees it found *something* rather
            // than asserting a specific shape (already covered by the `self`
            // pid test above).
            assert!(!result.is_unit());
        }
    }
    Ok(())
}

#[test]
fn rhai_procinfo_current_working_dir_for_pid_matches_direct_call() -> anyhow::Result<()> {
    let engine = make_engine();
    let pid = std::process::id();
    let script = format!("procinfo::current_working_dir_for_pid({pid})");
    let result: rhai::Dynamic = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let expected = procinfo::LocalProcessInfo::current_working_dir(pid)
        .and_then(|p| p.to_str().map(|s| s.to_string()));

    match expected {
        Some(expected_str) => {
            assert!(result.is_string());
            assert_eq!(result.into_string().unwrap(), expected_str);
        }
        None => {
            assert!(
                result.is_unit(),
                "expected `()` to mirror the mlua path's `nil` when cwd is unavailable"
            );
        }
    }
    Ok(())
}

#[test]
fn rhai_procinfo_executable_path_for_pid_matches_direct_call() -> anyhow::Result<()> {
    let engine = make_engine();
    let pid = std::process::id();
    let script = format!("procinfo::executable_path_for_pid({pid})");
    let result: rhai::Dynamic = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let expected = procinfo::LocalProcessInfo::executable_path(pid)
        .and_then(|p| p.to_str().map(|s| s.to_string()));

    match expected {
        Some(expected_str) => {
            assert!(result.is_string());
            assert_eq!(result.into_string().unwrap(), expected_str);
        }
        None => {
            assert!(
                result.is_unit(),
                "expected `()` to mirror the mlua path's `nil` when the executable path is unavailable"
            );
        }
    }
    Ok(())
}

#[test]
fn rhai_procinfo_get_info_for_pid_rejects_out_of_range_pid() {
    let engine = make_engine();
    // -1 doesn't fit in a u32; the rhai binding should reject it with a
    // proper error rather than silently truncating/wrapping.
    let result: Result<rhai::Dynamic, _> = engine.eval("procinfo::get_info_for_pid(-1)");
    assert!(result.is_err());
}
