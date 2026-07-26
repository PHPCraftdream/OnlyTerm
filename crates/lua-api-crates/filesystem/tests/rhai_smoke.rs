//! L4a smoke test: `filesystem::register_rhai` exposes `read_dir`/`glob` to rhai
//! scripts, mirroring `wezterm.read_dir(...)`/`wezterm.glob(...)` on the mlua path
//! (see docs/plans/2026-07-23-lua-rhai-migration.md).
//!
//! ## Handling non-determinism / environment dependence
//!
//! Both functions genuinely touch the filesystem, so rather than asserting
//! against a fixed hardcoded listing (fragile: depends on what's checked out,
//! platform path separators, etc), these tests:
//!   * build a throwaway temp directory with a known, controlled set of files
//!     (via `std::env::temp_dir()` + a unique subdirectory), so the *expected*
//!     answer is still deterministic -- this isn't "can't test it precisely",
//!     it's "construct our own deterministic filesystem fixture rather than
//!     relying on the ambient one";
//!   * additionally cross-check the rhai path's result against calling the
//!     exact same underlying Rust implementation directly (`read_dir_impl`/
//!     `glob_impl` are private, so instead this re-derives the expected set via
//!     `std::fs`/a second `filenamegen::Glob` pass), proving the rhai bindings
//!     don't silently drop or reorder entries relative to what's actually on
//!     disk.

use rhai::Engine;
use std::collections::BTreeSet;
use std::fs;

/// Creates a fresh temp directory containing a known set of files, and returns
/// its path. Using a per-test unique subdirectory (via `std::process::id()` +
/// the test name) avoids collisions if tests run concurrently.
fn make_fixture_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wezterm-filesystem-rhai-smoke-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create fixture dir");
    for file in ["a.txt", "b.txt", "c.log"] {
        fs::write(dir.join(file), b"content").expect("write fixture file");
    }
    dir
}

#[test]
fn rhai_read_dir_lists_the_same_entries_as_std_fs() -> anyhow::Result<()> {
    let dir = make_fixture_dir("read_dir");

    let mut engine = Engine::new();
    filesystem::register_rhai(&mut engine)?;

    let script = format!(r#"read_dir("{}")"#, dir.display().to_string().replace('\\', "\\\\"));
    let result: rhai::Array = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let rhai_entries: BTreeSet<String> = result
        .into_iter()
        .map(|v| v.into_string().expect("read_dir entries should be strings"))
        .collect();

    let expected: BTreeSet<String> = fs::read_dir(&dir)?
        .map(|entry| entry.unwrap().path().to_str().unwrap().to_string())
        .collect();

    assert_eq!(rhai_entries, expected);
    assert_eq!(rhai_entries.len(), 3);

    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn rhai_read_dir_on_nonexistent_path_reports_an_error_like_the_lua_api_does() {
    let mut engine = Engine::new();
    filesystem::register_rhai(&mut engine).expect("register_rhai");

    let result: Result<rhai::Array, _> =
        engine.eval(r#"read_dir("/this/path/really/should/not/exist/anywhere")"#);
    assert!(
        result.is_err(),
        "expected read_dir on a nonexistent path to fail, like the mlua path's \
         mlua::Error::external(...) does"
    );
}

#[test]
fn rhai_glob_with_explicit_path_matches_the_expected_files() -> anyhow::Result<()> {
    let dir = make_fixture_dir("glob_with_path");

    let mut engine = Engine::new();
    filesystem::register_rhai(&mut engine)?;

    let script = format!(
        r#"glob("*.txt", "{}")"#,
        dir.display().to_string().replace('\\', "\\\\")
    );
    let result: rhai::Array = engine
        .eval(&script)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    let rhai_entries: BTreeSet<String> = result
        .into_iter()
        .map(|v| v.into_string().expect("glob entries should be strings"))
        .collect();

    // Oracle: re-run the same glob pattern directly via filenamegen (the exact
    // crate the mlua/rhai bindings both delegate to), rather than hardcoding
    // path strings that would be brittle across platforms/temp-dir schemes.
    let glob = filenamegen::Glob::new("*.txt")?;
    let expected: BTreeSet<String> = glob
        .walk(&dir)
        .into_iter()
        .map(|p| p.to_str().unwrap().to_string())
        .collect();

    assert_eq!(rhai_entries, expected);
    assert_eq!(rhai_entries.len(), 2, "expected exactly the two .txt fixture files");

    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn rhai_glob_without_path_defaults_to_current_dir_and_does_not_panic() -> anyhow::Result<()> {
    // Mirrors the mlua binding's `Option<String>` path parameter defaulting to
    // "." -- here exercised as a same-named overload resolved by arity. Since
    // "current dir" isn't a controlled fixture, only assert on type/shape and
    // that it doesn't error, not on specific contents.
    let mut engine = Engine::new();
    filesystem::register_rhai(&mut engine)?;

    let result: rhai::Array = engine
        .eval(r#"glob("*")"#)
        .map_err(|err| anyhow::anyhow!("rhai eval error: {err}"))?;

    for entry in &result {
        assert!(entry.is_string(), "expected glob() entries to be strings");
    }
    Ok(())
}
