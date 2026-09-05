//! Cache and keyboard process-discovery regressions.
use super::*;

/// Task #247: once a `CachedProcInfo` entry already exists, a
/// subsequent `divine_process_list(AllowStale)` call against an
/// *expired* entry must return immediately with the OLD (stale) data
/// and hand the refresh off to a background thread, rather than
/// recomputing inline on the caller's thread.
///
/// Honesty note (matching this session's established practice, see
/// task #241's commit for the precedent): there's no cheap,
/// deterministic way to make a *real* `with_root_pid` system-process
/// snapshot artificially slow, so this cannot directly assert "the
/// call returned in under N milliseconds while a slow refresh was
/// in flight" the way the lock-timeout tests in #244/#246 do. Instead
/// this proves the structural property that guarantees the timing
/// property holds: (1) the value returned by the stale call is
/// bit-for-bit the same `updated` timestamp as what was cached
/// before the call (i.e. it did *not* wait for a fresh recompute),
/// and (2) `updating` is left `true` immediately after the call
/// returns, proving a background thread was actually queued rather
/// than the refresh having already happened synchronously and
/// finished before `divine_process_list` returned.
#[test]
fn divine_process_list_returns_stale_data_and_backgrounds_refresh() {
    let pane = make_pane_with_real_pid();

    // Seed `proc_list` with a real `CachedProcInfo` for our own pid.
    // Task #471: a cold cache under `AllowStale` no longer fetches
    // synchronously (see `cold_cache_allow_stale_is_non_blocking` below),
    // so this setup step deliberately uses `FetchImmediate` -- still
    // synchronous by design, and exactly what this test needs to get a
    // populated cache to rewind in the next step.
    let first_updated = {
        let info = pane
            .divine_process_list(CachePolicy::FetchImmediate)
            .expect("with_root_pid(std::process::id()) must resolve for the test process");
        assert!(
            !info.updating,
            "no background refresh should be in flight right after the initial synchronous fetch"
        );
        info.updated
    };

    // Force the cache to look expired without sleeping
    // `PROC_INFO_CACHE_TTL` (300ms) in a test: reach into the cache
    // directly (same module, so `proc_list` is visible) and rewind
    // `updated`. This rewound timestamp -- not `first_updated` -- is
    // what a correctly-behaving stale read must hand back, since
    // that's what's actually sitting in the cache at the moment of
    // the next call.
    let expired_updated = {
        let mut proc_list = pane.proc_list.lock();
        let info = proc_list.as_mut().expect("seeded by the call above");
        assert_eq!(info.updated, first_updated);
        info.updated = Instant::now() - (PROC_INFO_CACHE_TTL + Duration::from_millis(50));
        info.updated
    };

    // Second call, still `AllowStale`: must return the SAME
    // `expired_updated` timestamp immediately (proving it did not
    // block on a fresh synchronous recompute -- a recompute would
    // produce a brand new `Instant::now()`, not this rewound one),
    // and must leave `updating == true` (proving a background thread
    // was queued rather than nothing happening at all).
    let stale_updated = {
        let info = pane
            .divine_process_list(CachePolicy::AllowStale)
            .expect("cache is populated, so this must return Some even while stale");
        assert_eq!(
            info.updated, expired_updated,
            "a stale-but-present cache entry must be returned as-is, not recomputed inline"
        );
        assert!(
            info.updating,
            "an expired, allow-stale call against a fresh (non-updating) cache entry must \
                 queue a background refresh"
        );
        info.updated
    };
    assert_eq!(stale_updated, expired_updated);

    // A third call while the background refresh is (very likely)
    // still marked in-flight must not spawn a second concurrent
    // refresh -- `can_update()` gates on `!updating`. We can't
    // deterministically observe "no second thread was spawned"
    // directly, but we can at least confirm this call doesn't panic
    // or deadlock and still hands back usable data.
    let _ = pane.divine_process_list(CachePolicy::AllowStale);

    // Give the background thread a bounded window to finish and
    // confirm it actually does complete and clear `updating`,
    // demonstrating the refresh is real (not a permanently stuck
    // flag) -- this part is inherently timing-sensitive against a
    // real OS snapshot, so it's a generous bound (well beyond any
    // reasonable `with_root_pid` cost) rather than a tight one.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let still_updating = pane
            .proc_list
            .lock()
            .as_ref()
            .map(|info| info.updating)
            .unwrap_or(false);
        if !still_updating {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "background refresh never cleared `updating` within 5s"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let refreshed = pane.proc_list.lock().as_ref().unwrap().updated;
    assert!(
        refreshed > first_updated,
        "background thread must have actually recomputed and stored a newer `updated` value"
    );
}

/// Task #471: the very *first* `divine_process_list(AllowStale)` call for
/// a pane (cold cache, `proc_list` still `None`) must return `None`
/// immediately rather than blocking the caller on a synchronous
/// `with_root_pid` system-wide process snapshot -- that inline fetch used
/// to be reachable from `WM_PAINT` for a newly created pane's first
/// paint. It must still kick off a background fetch that eventually
/// populates the cache, so the *next* call (once that fetch completes)
/// sees real data instead of `None` forever.
#[test]
fn cold_cache_allow_stale_is_non_blocking() {
    let pane = make_pane_with_real_pid();

    // Cache is empty (`proc_list` is `None`): confirm nothing is cached
    // yet before making the call under test.
    assert!(pane.proc_list.lock().is_none());

    let cold_result = pane.divine_process_list(CachePolicy::AllowStale);
    assert!(
        cold_result.is_none(),
        "a cold cache under AllowStale must return None immediately rather than \
         computing inline"
    );
    drop(cold_result);

    // A background fetch must have been queued: `proc_list` should
    // eventually become populated without any further calls into
    // `divine_process_list`. Bounded poll rather than a fixed sleep,
    // matching `divine_process_list_returns_stale_data_and_backgrounds_refresh`'s
    // "generous bound against a real OS snapshot" approach.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if pane.proc_list.lock().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "cold-cache background fetch never populated proc_list within 5s"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // Once populated, a subsequent `AllowStale` call must return the
    // now-cached data (still fresh, so no further background refresh is
    // queued).
    let info = pane
        .divine_process_list(CachePolicy::AllowStale)
        .expect("background cold fetch must have populated the cache by now");
    assert!(
        !info.updating,
        "a freshly-populated cache entry must not itself be mid-refresh"
    );
}

#[cfg(windows)]
#[test]
fn key_compat_names_work_even_when_cold_refresh_is_stuck() {
    let pane = make_pane_with_real_pid();
    // Reproduce an unavailable cache whose refresh never completed. The old
    // lookup returned None here forever, disabling the compatibility switch.
    pane.proc_list_cold_fetch_in_flight
        .store(true, Ordering::SeqCst);
    let names = pane
        .get_process_tree_exe_names(CachePolicy::AllowStale)
        .expect("the first key must get names without waiting for the process cache");
    let exe = std::env::current_exe().unwrap();
    assert!(names.contains(exe.file_name().unwrap().to_str().unwrap()));
    assert!(
        pane.proc_list.lock().is_none(),
        "keyboard lookup must not warm the heavyweight cache"
    );
}

#[cfg(windows)]
#[test]
fn key_compat_names_ignore_stale_positive_and_stuck_refresh() {
    let pane = make_pane_with_real_pid();
    // Cached tree incorrectly says Codex is still running. Also mark its
    // refresh stuck: the old lookup continued to serve this data forever.
    let root = LocalProcessInfo {
        pid: std::process::id(),
        ppid: 0,
        name: "codex.exe".into(),
        executable: "codex.exe".into(),
        argv: vec![],
        cwd: std::path::PathBuf::new(),
        status: procinfo::LocalProcessStatus::Run,
        start_time: 0,
        console: 0,
        children: HashMap::new(),
    };
    *pane.proc_list.lock() = Some(CachedProcInfo {
        foreground: root.clone(),
        root,
        updated: Instant::now() - Duration::from_secs(60),
        updating: true,
    });
    let names = pane
        .get_process_tree_exe_names(CachePolicy::AllowStale)
        .unwrap();
    let exe = std::env::current_exe().unwrap();
    assert!(
        names.contains(exe.file_name().unwrap().to_str().unwrap()),
        "must use the actual process, not the stale root"
    );
    assert!(
        !names.contains("codex.exe"),
        "must not retain a stale Codex match after exit"
    );
    assert!(
        pane.proc_list.lock().as_ref().unwrap().updating,
        "keyboard lookup must succeed independently of the stalled refresh"
    );
}

/// A coding agent (Claude Code, Codex, etc.) running in a pane keeps its
/// own persistent subshell alive for tool calls and `cd`s it around
/// internally as it navigates a repo. That subshell is always younger
/// than the agent, so it -- not the user's real shell -- would win the
/// `foreground` ("youngest process anywhere in the tree") pick. Before
/// this fix, `get_current_working_dir` used that pick directly, so the
/// tab title followed the agent's internal navigation instead of the
/// user's own shell. `divine_current_working_dir` must use `root` (the
/// process the pty actually spawned) instead: seed a `CachedProcInfo`
/// where `root` and `foreground` disagree, and confirm the pane reports
/// `root`'s cwd.
#[test]
fn current_working_dir_tracks_root_process_not_foreground_pick() {
    use std::path::PathBuf;

    let pane = make_pane_with_real_pid();

    let user_shell_dir = if cfg!(windows) {
        PathBuf::from("C:\\Users\\test\\project")
    } else {
        PathBuf::from("/home/test/project")
    };
    let agent_subshell_dir = if cfg!(windows) {
        PathBuf::from("C:\\Users\\test\\project\\some\\deeply\\nested\\dir")
    } else {
        PathBuf::from("/home/test/project/some/deeply/nested/dir")
    };

    let root = LocalProcessInfo {
        pid: 1,
        ppid: 0,
        name: "pwsh".into(),
        executable: PathBuf::from("pwsh.exe"),
        argv: vec![],
        cwd: user_shell_dir.clone(),
        status: procinfo::LocalProcessStatus::Run,
        start_time: 0,
        #[cfg(windows)]
        console: 0,
        children: HashMap::new(),
    };
    let foreground = LocalProcessInfo {
        pid: 2,
        ppid: 1,
        name: "bash".into(),
        executable: PathBuf::from("bash.exe"),
        argv: vec![],
        cwd: agent_subshell_dir,
        status: procinfo::LocalProcessStatus::Run,
        start_time: 1,
        #[cfg(windows)]
        console: 0,
        children: HashMap::new(),
    };

    *pane.proc_list.lock() = Some(CachedProcInfo {
        root,
        foreground,
        updated: Instant::now(),
        updating: false,
    });

    let cwd = pane
        .get_current_working_dir(CachePolicy::AllowStale)
        .expect("seeded cache must yield a cwd");
    let expected = url::Url::from_directory_path(user_shell_dir)
        .expect("test path must convert to a directory URL");
    assert_eq!(
        cwd, expected,
        "tab cwd must track the pty's own root process, not whatever \
         younger process (e.g. an agent's internal tool-call subshell) \
         currently wins the foreground pick"
    );
}
