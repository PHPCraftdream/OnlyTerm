//! Process discovery for keyboard compatibility and cached title/cwd queries.
use super::*;

impl LocalPane {
    pub(super) fn process_tree_exe_names(
        &self,
        _policy: CachePolicy,
    ) -> Option<std::collections::HashSet<String>> {
        #[cfg(windows)]
        {
            let pid = {
                let process = self.process.lock();
                match &*process {
                    ProcessState::Running { pid, .. } => *pid,
                    _ => None,
                }
            }?;
            // This lookup selects keyboard bytes, unlike title/cwd polling:
            // a stale negative silently disables Codex's Ctrl-letter fix,
            // and a stale positive corrupts input after returning to a shell.
            // Use only lightweight snapshot names, outside the process lock.
            let start = Instant::now();
            let result = LocalProcessInfo::fresh_process_tree_exe_names(pid);
            log::info!(
                "diag: key-compat LocalPane pane={} process_pid={} source=fresh-snapshot elapsed_us={}",
                self.pane_id,
                pid,
                start.elapsed().as_micros(),
            );
            match result {
                Ok(names) => Some(names),
                Err(err) => {
                    log::warn!(
                        "key-compat pane={} process_pid={}: process lookup failed: {}",
                        self.pane_id,
                        pid,
                        err,
                    );
                    None
                }
            }
        }
        #[cfg(not(windows))]
        self.divine_process_list(_policy)
            .map(|info| info.root.flatten_to_exe_names())
    }

    pub(super) fn process_tree_allows_close(&self) -> bool {
        if let Some(info) = self.divine_process_list(CachePolicy::FetchImmediate) {
            log::trace!(
                "can_close_without_prompting? procs in pane {:#?}",
                info.root
            );

            // `mux-is-process-stateful` used to be a rhai event-callback
            // hook consulted here; with the scripting layer removed there
            // is no handler left to ask, so this always takes the same
            // default-behavior fallback the old code took when no rhai
            // config was loaded.
            fn default_stateful_check(proc_list: &LocalProcessInfo) -> bool {
                // Fig uses `figterm` a pseudo terminal for a lot of functionality, it runs between
                // the shell and terminal. Unfortunately it is typically named `<shell> (figterm)`,
                // which prevents the statuful check from passing. This strips the suffix from the
                // process name to allow the check to pass.
                let names = proc_list
                    .flatten_to_exe_names()
                    .into_iter()
                    .map(|s| match s.strip_suffix(" (figterm)") {
                        Some(s) => s.into(),
                        None => s,
                    })
                    .collect::<HashSet<_>>();

                let skip = configuration()
                    .skip_close_confirmation_for_processes_named
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>();

                if !names.is_subset(&skip) {
                    // There are other processes running than are listed,
                    // so we consider this to be stateful
                    return true;
                }
                false
            }

            let is_stateful = default_stateful_check(&info.root);

            !is_stateful
        } else {
            false
        }
    }

    pub(super) fn divine_current_working_dir(&self, policy: CachePolicy) -> Option<Url> {
        // Deliberately `root`, not `foreground`: `foreground` is "youngest
        // process anywhere in the tree", which is exactly right for "what
        // program is the user interacting with" (see
        // `get_foreground_process_name`/`get_process_tree_exe_names`) but
        // wrong for "what directory is the tab about". A coding agent
        // running in this pane (Claude Code, Codex, etc.) keeps a
        // persistent subshell alive for its own tool calls and `cd`s it
        // around internally as it reads different parts of a repo; that
        // subshell is always younger than the agent itself, so it would
        // permanently win the `foreground` pick and its cwd would follow
        // the agent's *internal* navigation, renaming the tab on every
        // such move. `root` is the actual process the pty spawned (the
        // user's real interactive shell) -- its own cwd only changes when
        // the user runs `cd` at that shell's own prompt, which is exactly
        // the tab-rename trigger the user actually wants.
        if let Some(root) = self.divine_root_process(policy) {
            return Url::from_directory_path(root.cwd.clone()).ok();
        }

        None
    }

    /// Does the actual, expensive work of `divine_process_list`'s refresh:
    /// walks a fresh, live system-wide process snapshot rooted at `pid`
    /// and picks out the "foreground" process. This is the part that's
    /// slow (see the doc comment on `divine_process_list`) and that task
    /// #247 moves off the caller's thread whenever there's already a
    /// stale value it's safe to return instead.
    pub(super) fn compute_proc_info(pid: u32) -> Option<CachedProcInfo> {
        log::trace!("CachedProcInfo expired, refresh");
        let root = LocalProcessInfo::with_root_pid(pid)?;

        // Windows doesn't have any job control or session concept,
        // so we infer that the equivalent to the process group
        // leader is the most recently spawned program running
        // in the console
        let mut youngest = &root;

        // Walk the process tree with an explicit stack rather
        // than recursion: this tree is rebuilt from a live
        // system-wide process snapshot every time the cache
        // expires, and its depth is not bounded by anything
        // onlyterm controls, so a recursive walk here could in
        // principle overflow the stack for a sufficiently deep
        // process tree.
        let mut stack: Vec<&LocalProcessInfo> = vec![&root];
        while let Some(proc) = stack.pop() {
            if proc.start_time >= youngest.start_time {
                youngest = proc;
            }

            for child in proc.children.values() {
                #[cfg(windows)]
                if child.console == 0 {
                    continue;
                }
                stack.push(child);
            }
        }
        let mut foreground = youngest.clone();
        foreground.children.clear();

        log::trace!("CachedProcInfo updated");
        Some(CachedProcInfo {
            root,
            foreground,
            updated: Instant::now(),
            updating: false,
        })
    }

    /// Task #247: `divine_process_list` used to call `with_root_pid`
    /// (a `CreateToolhelp32Snapshot` walk over *every* process on the
    /// machine, plus a `ProcHandle::new` per process on Windows)
    /// synchronously, inline, on the calling thread whenever the cache
    /// was expired -- even under `CachePolicy::AllowStale`. Since this is
    /// reachable from the GUI thread on essentially every key/mouse event
    /// (`get_tab_information` -> `get_current_working_dir(AllowStale)` ->
    /// `divine_current_working_dir` -> `divine_foreground_process` on
    /// Windows), that inline snapshot -- whose cost scales with total
    /// system process count, not anything onlyterm controls -- could stall
    /// input/rendering on every cache expiry (every `PROC_INFO_CACHE_TTL`
    /// = 300ms).
    ///
    /// This now follows a stale-return-plus-background-refresh pattern:
    /// when the caller allows stale data and a cached value already exists (even
    /// if expired), return it immediately and kick off a background
    /// fetch to refresh the cache for next time, guarded by
    /// `CachedProcInfo::updating` against spawning a duplicate concurrent
    /// refresh.
    ///
    /// Task #471: the very first call for a pane (`proc_list` still
    /// `None`, nothing cached at all yet) used to be the one case that
    /// stayed synchronous even under `AllowStale`, on the theory that
    /// there's no stale value to fall back to. In practice this made the
    /// very first paint of a new pane (inside `WM_PAINT` on the GUI
    /// thread) pay for a full system-wide process snapshot inline. Every
    /// `AllowStale` caller of the methods that bottom out here
    /// (`get_foreground_process_name`, `get_current_working_dir`) already
    /// treats `None` as an expected, gracefully-handled case --
    /// `bidi_disabled_by_foreground_process` just returns `false`,
    /// `get_title` falls back to the existing title, `PaneNode`
    /// serialization already models the field as `Option` -- so a cold
    /// `AllowStale` call now returns `None` immediately and kicks off the
    /// same kind of background fetch as the warm-refresh path, guarded by
    /// `proc_list_cold_fetch_in_flight` (there's no existing
    /// `CachedProcInfo` yet to stash an `updating` flag on). Only
    /// `CachePolicy::FetchImmediate` (an explicit "I need a fresh answer
    /// right now" request, e.g. `can_close_without_prompting`'s
    /// close-tab-time stateful-process check) still fetches inline on a
    /// cold cache, since that policy is a deliberate opt-out of the
    /// stale/async contract this cache otherwise provides.
    ///
    /// Both background-fetch paths below use `smol::unblock` (the same
    /// mechanism task #469 used for `Mux::resolve_cwd`'s
    /// `FetchImmediate` case) rather than `std::thread::spawn`: this is a
    /// periodic, independent, one-shot-per-refresh workload (one snapshot
    /// per `PROC_INFO_CACHE_TTL` expiry per pane), not a persistent
    /// request/response channel, so there's no long-lived state worth
    /// building a dedicated worker thread for (contrast
    /// `renderthread.rs`, which owns a GPU context that must stay pinned
    /// to one thread for its whole lifetime). `smol::unblock` schedules
    /// the closure onto its own pooled, reused blocking-thread executor
    /// the moment it's called -- not lazily on first `.await` -- so
    /// `.detach()`-ing the returned `Task` immediately below is a
    /// fire-and-forget spawn with the same semantics as the
    /// `std::thread::spawn` it replaces, minus the fresh ~1MB-stack OS
    /// thread every 300ms per pane.
    pub(super) fn divine_process_list(
        &self,
        policy: CachePolicy,
    ) -> Option<MappedMutexGuard<'_, CachedProcInfo>> {
        if let ProcessState::Running { pid: Some(pid), .. } = &*self.process.lock() {
            let pid = *pid;
            let mut proc_list = self.proc_list.lock();

            match proc_list.as_mut() {
                None if policy == CachePolicy::FetchImmediate => {
                    // Caller explicitly wants a fresh answer right now
                    // and there's nothing cached yet: no way to avoid
                    // doing this one fetch synchronously.
                    let info = Self::compute_proc_info(pid)?;
                    proc_list.replace(info);
                }
                None => {
                    // Cold cache, but the caller tolerates a stale (here:
                    // entirely absent) answer: return `None` now and
                    // queue a background fetch to populate the cache for
                    // next time, rather than blocking the caller on a
                    // full system-wide process snapshot.
                    if !self
                        .proc_list_cold_fetch_in_flight
                        .swap(true, Ordering::SeqCst)
                    {
                        let proc_list_ref = Arc::clone(&self.proc_list);
                        let in_flight_ref = Arc::clone(&self.proc_list_cold_fetch_in_flight);
                        smol::unblock(move || {
                            let result = Self::compute_proc_info(pid);
                            let mut proc_list = proc_list_ref.lock();
                            if let Some(info) = result {
                                proc_list.replace(info);
                            }
                            in_flight_ref.store(false, Ordering::SeqCst);
                        })
                        .detach();
                    }
                    return None;
                }
                Some(info) if policy == CachePolicy::FetchImmediate => {
                    // Caller explicitly wants a fresh answer right now
                    // (e.g. an explicit close-tab process check): keep
                    // doing the synchronous refresh inline, matching the
                    // previous behavior for this policy.
                    let fresh = Self::compute_proc_info(pid)?;
                    *info = fresh;
                }
                Some(info) if info.expired() && info.can_update() => {
                    // Stale, but there's already something to return, and
                    // policy allows it: hand back the stale data now and
                    // queue up a background refresh for next time.
                    info.updating = true;
                    let proc_list_ref = Arc::clone(&self.proc_list);
                    smol::unblock(move || {
                        if let Some(fresh) = Self::compute_proc_info(pid) {
                            let mut proc_list = proc_list_ref.lock();
                            if let Some(info) = proc_list.as_mut() {
                                *info = fresh;
                            }
                        } else if let Some(info) = proc_list_ref.lock().as_mut() {
                            // Refresh failed (e.g. the process has since
                            // exited): stop claiming an update is
                            // in-flight so a later call can try again,
                            // but keep serving the last-known-good data
                            // in the meantime.
                            info.updating = false;
                        }
                    })
                    .detach();
                }
                Some(_) => {
                    // Either still fresh, or already being refreshed by
                    // another in-flight background thread -- either way,
                    // just return what's cached.
                }
            }

            return Some(MutexGuard::map(proc_list, |info| info.as_mut().unwrap()));
        }
        None
    }

    #[allow(dead_code)]
    pub(super) fn divine_foreground_process(
        &self,
        policy: CachePolicy,
    ) -> Option<LocalProcessInfo> {
        self.divine_process_list(policy)
            .map(|info| info.foreground.clone())
    }

    /// The process the pty itself spawned for this pane -- the user's
    /// actual interactive shell -- as opposed to `divine_foreground_process`'s
    /// "youngest process anywhere in the tree". See `divine_current_working_dir`
    /// for why cwd tracking needs this one instead of the foreground pick.
    pub(super) fn divine_root_process(&self, policy: CachePolicy) -> Option<LocalProcessInfo> {
        self.divine_process_list(policy)
            .map(|info| info.root.clone())
    }
}
