//! Cheap idle probes; full discovery still runs after activity or a bounded idle interval.
use super::{CachedProcInfo, LocalPane, LocalProcessInfo};
use procinfo::ProcessActivityStamp;
use std::sync::Arc;
use std::time::{Duration, Instant};

const IDLE_FULL_REFRESH: Duration = Duration::from_secs(5);
type Stamps = Vec<(u32, ProcessActivityStamp)>;

#[derive(Clone)]
pub(super) struct ActivityState {
    epoch: Arc<()>,
    stamps: Option<Stamps>,
    quiet_since: Instant,
    full_refresh: Instant,
}

impl ActivityState {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            epoch: Arc::new(()),
            stamps: None,
            quiet_since: now,
            full_refresh: now,
        }
    }

    pub(super) fn snapshot_started(&mut self, at: Instant) {
        self.full_refresh = at;
    }
}

pub(super) struct RefreshPlan {
    state: ActivityState,
    pids: Vec<u32>,
}

fn process_ids(root: &LocalProcessInfo) -> Vec<u32> {
    let mut result = Vec::new();
    let mut pending = vec![root];
    while let Some(process) = pending.pop() {
        result.push(process.pid);
        pending.extend(process.children.values());
    }
    result.sort_unstable();
    result
}

impl RefreshPlan {
    pub(super) fn new(info: &CachedProcInfo) -> Self {
        Self {
            state: info.activity.clone(),
            pids: process_ids(&info.root),
        }
    }

    pub(super) fn owns(&self, info: &CachedProcInfo) -> bool {
        Arc::ptr_eq(&self.state.epoch, &info.activity.epoch)
    }

    fn unchanged(&self, stamps: &Option<Stamps>) -> bool {
        stamps.is_some() && *stamps == self.state.stamps
    }

    fn can_reuse(&self, stamps: &Option<Stamps>, now: Instant) -> bool {
        self.unchanged(stamps)
            // A cached system snapshot may predate the first quiet probe; do not bless it.
            && self.state.full_refresh >= self.state.quiet_since
            && now.saturating_duration_since(self.state.full_refresh) < IDLE_FULL_REFRESH
    }

    fn finish_full(
        &self,
        fresh: &mut CachedProcInfo,
        stamps: Option<Stamps>,
        observed_at: Instant,
    ) {
        fresh.activity.quiet_since = observed_at;
        if process_ids(&fresh.root) == self.pids {
            if self.unchanged(&stamps) {
                fresh.activity.quiet_since = self.state.quiet_since;
            }
            fresh.activity.stamps = stamps;
        }
    }

    pub(super) fn run(&self, pid: u32) -> RefreshResult {
        self.run_with(
            pid,
            Instant::now,
            LocalProcessInfo::activity_stamp,
            LocalPane::compute_proc_info,
        )
    }

    fn run_with(
        &self,
        pid: u32,
        now: impl Fn() -> Instant,
        probe: impl Fn(u32) -> Option<ProcessActivityStamp>,
        fetch: impl FnOnce(u32) -> Option<CachedProcInfo>,
    ) -> RefreshResult {
        let stamps = self
            .pids
            .iter()
            .map(|&pid| probe(pid).map(|stamp| (pid, stamp)))
            .collect();
        let observed_at = now();
        if self.can_reuse(&stamps, observed_at) {
            return RefreshResult::Unchanged;
        }
        match fetch(pid) {
            Some(mut fresh) => {
                self.finish_full(&mut fresh, stamps, observed_at);
                RefreshResult::Fresh(Box::new(fresh))
            }
            None => RefreshResult::Failed,
        }
    }
}

pub(super) enum RefreshResult {
    Unchanged,
    Fresh(Box<CachedProcInfo>),
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashMap;

    fn root() -> LocalProcessInfo {
        LocalProcessInfo {
            pid: 10,
            ppid: 0,
            name: "shell".into(),
            executable: "shell.exe".into(),
            argv: vec![],
            cwd: "work".into(),
            status: procinfo::LocalProcessStatus::Run,
            start_time: 7,
            console: 1,
            children: HashMap::new(),
        }
    }

    fn cached(at: Instant) -> CachedProcInfo {
        let root = root();
        CachedProcInfo {
            foreground: root.clone_without_children(),
            root,
            updated: at,
            updating: false,
            activity: ActivityState::new(at),
        }
    }

    fn stamp() -> ProcessActivityStamp {
        ProcessActivityStamp {
            creation_time: 7,
            cycles: 100,
        }
    }

    fn quiet(at: Instant) -> CachedProcInfo {
        let mut info = cached(at);
        info.activity.stamps = Some(vec![(10, stamp())]);
        info
    }

    #[test]
    fn idle_refresh_does_not_enumerate_system_processes() {
        let at = Instant::now();
        let info = quiet(at);
        let plan = RefreshPlan::new(&info);
        let result = plan.run_with(
            10,
            || at + Duration::from_secs(1),
            |_| Some(stamp()),
            |_| {
                panic!("idle probes must not take a system snapshot");
            },
        );
        assert!(matches!(result, RefreshResult::Unchanged));
    }

    #[test]
    fn activity_pid_reuse_and_inaccessible_processes_force_real_refresh() {
        let at = Instant::now();
        for observed in [
            Some(ProcessActivityStamp {
                cycles: 101,
                ..stamp()
            }),
            Some(ProcessActivityStamp {
                creation_time: 8,
                ..stamp()
            }),
            None,
        ] {
            let plan = RefreshPlan::new(&quiet(at));
            let calls = Cell::new(0);
            let result = plan.run_with(
                10,
                || at + Duration::from_secs(1),
                |_| observed,
                |_| {
                    calls.set(calls.get() + 1);
                    let mut fresh = cached(at + Duration::from_secs(1));
                    fresh.root.cwd = "changed".into();
                    Some(fresh)
                },
            );
            match result {
                RefreshResult::Fresh(info) => {
                    assert_eq!(info.root.cwd, std::path::PathBuf::from("changed"))
                }
                _ => panic!("changed or uncertain activity must refresh metadata"),
            }
            assert_eq!(calls.get(), 1);
        }
    }

    #[test]
    fn snapshot_older_than_activity_probe_cannot_be_blessed_as_idle() {
        let at = Instant::now();
        let mut info = quiet(at);
        info.activity.quiet_since = at + Duration::from_millis(100);
        let plan = RefreshPlan::new(&info);
        let now = at + Duration::from_secs(1);
        let result = plan.run_with(10, || now, |_| Some(stamp()), |_| Some(cached(now)));
        let fresh = match result {
            RefreshResult::Fresh(info) => info,
            _ => panic!("shared snapshot predates the activity sample"),
        };
        let next = RefreshPlan::new(&fresh);
        assert!(matches!(
            next.run_with(
                10,
                || now,
                |_| Some(stamp()),
                |_| panic!("snapshot is now newer")
            ),
            RefreshResult::Unchanged
        ));
    }

    #[test]
    fn idle_has_a_hard_full_refresh_deadline() {
        let at = Instant::now();
        let plan = RefreshPlan::new(&quiet(at));
        let result = plan.run_with(
            10,
            || at + IDLE_FULL_REFRESH,
            |_| Some(stamp()),
            |_| Some(cached(at + IDLE_FULL_REFRESH)),
        );
        assert!(matches!(result, RefreshResult::Fresh(_)));
    }

    #[test]
    fn new_descendants_need_their_own_activity_baseline() {
        let at = Instant::now();
        let plan = RefreshPlan::new(&quiet(at));
        let result = plan.run_with(
            10,
            || at + IDLE_FULL_REFRESH,
            |_| Some(stamp()),
            |_| {
                let mut fresh = cached(at + IDLE_FULL_REFRESH);
                let mut child = root();
                child.pid = 11;
                child.ppid = 10;
                fresh.root.children.insert(11, child);
                Some(fresh)
            },
        );
        match result {
            RefreshResult::Fresh(info) => assert!(info.activity.stamps.is_none()),
            _ => panic!("full refresh was required"),
        }
    }

    #[test]
    fn older_background_refresh_cannot_overwrite_an_immediate_fetch() {
        let at = Instant::now();
        let old = quiet(at);
        let plan = RefreshPlan::new(&old);
        assert!(plan.owns(&old));
        assert!(!plan.owns(&cached(at)));
    }

    #[test]
    fn failed_full_refresh_is_not_reported_as_unchanged() {
        let at = Instant::now();
        let plan = RefreshPlan::new(&quiet(at));
        assert!(matches!(
            plan.run_with(10, || at + IDLE_FULL_REFRESH, |_| Some(stamp()), |_| None),
            RefreshResult::Failed
        ));
    }
}
