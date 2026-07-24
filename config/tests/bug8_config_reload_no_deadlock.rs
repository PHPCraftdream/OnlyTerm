//! Regression test for BUG8: `wezterm-gui -n ls-fonts ...` (and in fact any
//! wezterm process, since this is on the very first config load of any
//! process, not specific to `ls-fonts` or fonts at all) hung forever with
//! zero output.
//!
//! Root cause: `Configuration::reload()` locks `Configuration::inner` (a
//! plain `std::sync::Mutex`, non-reentrant) and, while holding that lock,
//! calls `Config::load()`. `Config::load()` -> `Config::try_default()` /
//! `try_load()` synchronously builds a companion mlua context via
//! `crate::lua::make_lua_context`, which runs every registered
//! `SETUP_FUNCS` callback. In the real binaries (`wezterm`, `wezterm-gui`,
//! `wezterm-mux-server`), one of those callbacks is
//! `lua-api-crates/time-funcs::register`, which calls
//! `config::subscribe_to_config_reload` on its very first invocation --
//! and that function locks the *same* `Configuration::inner` mutex again,
//! from the same thread, before the outer lock is ever released. That is
//! an immediate, guaranteed self-deadlock the first time any process loads
//! its configuration (not a race -- it reproduces every single time).
//!
//! This test reproduces the exact shape of that hazard without depending
//! on `lua-api-crates/time-funcs` (a separate crate `config` does not
//! depend on): it registers a `SETUP_FUNCS` callback, via the same public
//! `config::lua::add_context_setup_func` API real callers use, that calls
//! `config::subscribe_to_config_reload` -- mirroring exactly what
//! `time-funcs::register` does. It then triggers a config load
//! (`config::common_init` with `skip_config: true`, matching `wezterm-gui
//! -n ls-fonts`) on a background thread and asserts that it completes
//! within a generous timeout, rather than trusting the real
//! `wezterm-gui.exe` process (slower to build, and harder to diagnose a
//! hang in than a plain `#[test]`).
//!
//! Uses a fixed-duration `mpsc` `recv_timeout` (rather than
//! `thread::spawn(..).join()`, which would itself block forever on a
//! deadlock) so that a regression here fails fast with a clear panic
//! message instead of hanging the test binary.

use std::sync::mpsc;
use std::time::Duration;

/// Mirrors `lua-api-crates/time-funcs::register`'s `subscribe_to_config_reload`
/// call: the one `SETUP_FUNCS` callback (out of the many registered by real
/// wezterm binaries) that is reachable from `Config::load()` itself and that
/// locks the global `Configuration` a second time.
fn setup_func_that_subscribes(_lua: &config::lua::mlua::Lua) -> anyhow::Result<()> {
    // Leak the subscription: we only care that this call itself does not
    // deadlock, matching `time-funcs::register`'s
    // `CONFIG_SUBSCRIPTION.lock().unwrap().replace(..)` which likewise keeps
    // the subscription alive for the life of the process.
    let sub = config::subscribe_to_config_reload(|| true);
    std::mem::forget(sub);
    Ok(())
}

#[test]
fn config_reload_does_not_deadlock_when_a_setup_func_subscribes() {
    config::lua::add_context_setup_func(setup_func_that_subscribes);

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // Matches `wezterm-gui -n ls-fonts`'s call in `run()`: `skip_config:
        // true`, no config file override, no --config overrides.
        let result = config::common_init(None, &[], true);
        // The receiver may already have timed out and been dropped if this
        // deadlocks past the timeout below; ignore the send failure in that
        // case (there is nothing left to report to).
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(result) => {
            result.expect("common_init(skip_config: true) should succeed with defaults");
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!(
                "config::common_init(.., skip_config: true) did not return within 20s: \
                 this is BUG8, a self-deadlock in Configuration::reload() when a \
                 SETUP_FUNCS callback (e.g. time-funcs::register) calls back into \
                 config::subscribe_to_config_reload while Config::load() is still \
                 running under Configuration::inner's lock"
            );
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("worker thread panicked before completing common_init");
        }
    }
}
