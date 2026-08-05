use super::*;
// See `crate::test::MUX_TEST_GUARD`: `allocate` reaches into the
// process-global `Mux` singleton (`Mux::set_mux`/`Mux::get`), so tests
// that install one must run serially with every other such test in the
// crate, not just within this module.
use crate::test::MUX_TEST_GUARD;

fn test_term_config() -> Arc<dyn TerminalConfiguration + Send + Sync> {
    Arc::new(config::TermConfig::new())
}

#[test]
fn allocate_succeeds_under_normal_conditions() {
    let _guard = MUX_TEST_GUARD.lock();
    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let size = TerminalSize::default();
    let result = allocate(size, test_term_config());
    assert!(
        result.is_ok(),
        "allocate() should succeed when fds/pipes are available: {:?}",
        result.err()
    );

    Mux::shutdown();
}

// Regression test for https://github.com/wez/wezterm/issues/3107: opening
// the debug overlay (or any termwiz overlay/pane) used to call
// `Pipe::new().expect(...)`, which panicked and took down the *entire*
// process whenever the OS ran out of file descriptors (a condition users
// hit in practice by rapidly opening overlays/panes, exhausting
// RLIMIT_NOFILE). `allocate` must instead return an `Err` so the caller
// can log the failure and keep the rest of wezterm running.
#[cfg(unix)]
#[test]
fn allocate_reports_error_instead_of_panicking_on_fd_exhaustion() {
    let _guard = MUX_TEST_GUARD.lock();
    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    // Save the current fd limit so we can restore it even if an
    // assertion below fails.
    // SAFETY: `libc::rlimit` is a POD (two `rlim_t` fields) with no
    // validity invariants, so `mem::zeroed()` produces a valid value.
    let mut saved: libc::rlimit = unsafe { std::mem::zeroed() };
    // SAFETY: `RLIMIT_NOFILE` is a valid resource constant and `&mut saved`
    // is a valid out-pointer that outlives the call.
    let got = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut saved) };
    assert_eq!(got, 0, "getrlimit failed");

    // Open files until we're right at the edge of the current soft
    // limit, then lower the soft limit to the current fd count so that
    // the very next fd allocation (the pipe/socketpair in `allocate`)
    // fails with EMFILE, exactly as reported in #3107.
    let mut keepalive = Vec::new();
    loop {
        match std::fs::File::open("/dev/null") {
            Ok(f) => keepalive.push(f),
            Err(_) => break,
        }
        if keepalive.len() > 100_000 {
            // Safety valve: don't loop forever if something is wrong
            // with the environment.
            break;
        }
    }

    let current_fd_count = keepalive.len() as libc::rlim_t;
    let tight = libc::rlimit {
        rlim_cur: current_fd_count.saturating_sub(1).max(3),
        rlim_max: saved.rlim_max,
    };
    // SAFETY: `&tight` is a valid pointer to an `rlimit` that outlives the
    // call; lowering the soft RLIMIT_NOFILE limit is a supported operation
    // and stays within the existing hard limit (`tight.rlim_max = saved.rlim_max`).
    let set = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &tight) };

    let result = if set == 0 {
        allocate(TerminalSize::default(), test_term_config())
    } else {
        // If we couldn't lower the limit (eg: insufficient privilege in
        // this environment) fall back to asserting on the error message
        // path is at least well-formed by closing all of our extra fds;
        // there's nothing further we can assert here.
        Err(anyhow::anyhow!(
            "could not lower RLIMIT_NOFILE in this environment"
        ))
    };

    // Restore the fd limit and release our held-open files before making
    // any assertions, so a failing assertion doesn't leave the test
    // process (and subsequent tests) starved of file descriptors.
    // SAFETY: `&saved` is a valid pointer to the previously captured
    // `rlimit`; this restores the original limit, which was valid on entry.
    unsafe {
        libc::setrlimit(libc::RLIMIT_NOFILE, &saved);
    }
    drop(keepalive);

    if set == 0 {
        assert!(
            result.is_err(),
            "allocate() should return Err on fd exhaustion instead of panicking"
        );
    }

    Mux::shutdown();
}
