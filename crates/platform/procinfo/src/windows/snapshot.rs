use super::*;
/// How long the machine-wide process snapshot shared by all
/// `with_root_pid` callers stays fresh. Matches the per-pane
/// `PROC_INFO_CACHE_TTL` used by mux's `divine_process_list`, so each
/// pane's background refresh almost always lands on an already-warm
/// shared snapshot instead of taking its own.
const PROC_SNAPSHOT_TTL: Duration = Duration::from_millis(300);

/// Identity and activity, without enumerating unrelated system processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessActivityStamp {
    pub creation_time: u64,
    pub cycles: u64,
}

/// How long to stop attempting a fresh walk after one fails. A failed
/// snapshot means the OS refused the handle or the memory the walk needs;
/// retrying at the `PROC_SNAPSHOT_TTL` cadence under sustained memory
/// pressure only spends more allocations on the same failure, and every
/// attempt is another chance for an allocation to abort the whole process
/// (Rust's global allocator aborts on failure -- that is exactly how the
/// 2026-09-14 crashes ended, inside this walk, immediately after a
/// "process snapshot failed" warning). Serving the last complete snapshot
/// for a few seconds is well inside the staleness every caller already
/// tolerates.
const PROC_SNAPSHOT_FAILURE_BACKOFF: Duration = Duration::from_secs(5);

/// The one field of `PROCESSENTRY32W` that `with_root_pid`'s tree
/// building actually needs, copied out of the raw Win32 struct so the
/// shared cache doesn't hand out Win32 handles or keep snapshot-shaped
/// memory alive.
#[derive(Clone)]
struct ProcessEntry {
    pid: u32,
    ppid: u32,
    /// Range into `ProcessEntries::names` rather than an owned path: this
    /// walk covers *every* process on the machine, but only the handful in
    /// a caller's requested subtree ever have their path read, so
    /// materializing a `PathBuf` here allocated thousands of strings per
    /// refresh that nothing looked at.
    exe: std::ops::Range<usize>,
}

/// Lightweight snapshot entry used only by fresh keyboard compatibility
/// checks. Keeping the Toolhelp executable name in its source UTF-16 shape
/// avoids a PathBuf allocation for every process in the machine-wide snapshot.
#[derive(Clone)]
struct SnapshotExeEntry {
    pid: u32,
    ppid: u32,
    exe: std::ops::Range<usize>,
}

#[derive(Default)]
struct SnapshotExeEntries {
    entries: Vec<SnapshotExeEntry>,
    names: Vec<u16>,
}

impl SnapshotExeEntries {
    fn push(&mut self, pid: u32, ppid: u32, exe: &[u16]) {
        let len = exe.iter().position(|&unit| unit == 0).unwrap_or(exe.len());
        let start = self.names.len();
        self.names.extend_from_slice(&exe[..len]);
        self.entries.push(SnapshotExeEntry {
            pid,
            ppid,
            exe: start..self.names.len(),
        });
    }
}

/// Machine-wide process list shared by every `with_root_pid` caller.
/// `with_root_pid` is called once per pane per `PROC_INFO_CACHE_TTL`
/// (bidi foreground-process polling, title updates, cwd lookups); without
/// this cache each caller took its own `CreateToolhelp32Snapshot` walk
/// over every process on the machine, so the cost scaled with
/// pane-count x machine process-count. Callers already treat the data as
/// eventually-fresh (mux wraps it in its own 300ms stale-while-revalidate
/// cache), so a shared snapshot up to `PROC_SNAPSHOT_TTL` old is within
/// the staleness every caller tolerates. Concurrent refresher threads
/// serialize on the mutex; the first one past expiry refreshes and the
/// rest find it warm.
struct ProcessEntries {
    started_at: Instant,
    entries: Vec<ProcessEntry>,
    /// All executable names of `entries`, concatenated and NUL-free. One
    /// allocation for the whole machine instead of one per process.
    names: Vec<u16>,
}

impl ProcessEntries {
    fn new(started_at: Instant) -> Self {
        Self {
            started_at,
            entries: Vec::new(),
            names: Vec::new(),
        }
    }

    /// Empties the contents but keeps both allocations, so a refresh that
    /// reuses this buffer does not have to grow them again from zero.
    fn reset(&mut self, started_at: Instant) {
        self.started_at = started_at;
        self.entries.clear();
        self.names.clear();
    }

    fn push(&mut self, pid: u32, ppid: u32, exe: &[u16]) {
        let len = exe.iter().position(|&unit| unit == 0).unwrap_or(exe.len());
        let start = self.names.len();
        self.names.extend_from_slice(&exe[..len]);
        self.entries.push(ProcessEntry {
            pid,
            ppid,
            exe: start..self.names.len(),
        });
    }

    /// Materializes one entry's executable path. Callers reach this only
    /// for the processes they actually walk, which is the point of storing
    /// the names packed.
    fn exe_path(&self, entry: &ProcessEntry) -> PathBuf {
        // `push` already stripped the NUL, so the range is exactly the name.
        OsString::from_wide(&self.names[entry.exe.clone()]).into()
    }
}

struct CachedSnapshot {
    updated: Instant,
    entries: Arc<ProcessEntries>,
}

struct ProcessSnapshotState {
    cached: Option<CachedSnapshot>,
    /// Backing storage reclaimed from a superseded snapshot, refilled in
    /// place by the next refresh instead of allocating a fresh pair of
    /// `Vec`s. Never handed to callers, so it may also hold the remains of
    /// a failed (partial) walk -- `reset` clears that before reuse.
    spare: Option<ProcessEntries>,
    /// Set when a walk fails, cleared when one succeeds; gates
    /// `PROC_SNAPSHOT_FAILURE_BACKOFF`.
    failed_at: Option<Instant>,
}

impl ProcessSnapshotState {
    const fn new() -> Self {
        Self {
            cached: None,
            spare: None,
            failed_at: None,
        }
    }
}

type ProcessSnapshotCache = Mutex<ProcessSnapshotState>;
static SNAPSHOT_CACHE: ProcessSnapshotCache = Mutex::new(ProcessSnapshotState::new());

fn shared_snapshot_entries() -> Arc<ProcessEntries> {
    snapshot_entries_with(&SNAPSHOT_CACHE, Instant::now, refill_snapshot_entries)
}

/// The last complete snapshot if there is one, or an empty stand-in. Used
/// on every path that declines to walk (failed or backed off): callers
/// tolerate staleness, but must never see a partially filled walk.
fn stale_or_empty(cached: &Option<CachedSnapshot>, started_at: Instant) -> Arc<ProcessEntries> {
    match cached {
        Some(cached) => Arc::clone(&cached.entries),
        None => Arc::new(ProcessEntries::new(started_at)),
    }
}

fn snapshot_entries_with(
    cache: &ProcessSnapshotCache,
    now: impl Fn() -> Instant,
    refill: impl FnOnce(&mut ProcessEntries) -> io::Result<()>,
) -> Arc<ProcessEntries> {
    // Deliberately poison-tolerant: the cache holds only plain data, so a
    // panicking refresher must not permanently break process lookups.
    let mut state = cache.lock().unwrap_or_else(|e| e.into_inner());

    if let Some(cached) = state.cached.as_ref() {
        if now().saturating_duration_since(cached.updated) < PROC_SNAPSHOT_TTL {
            return Arc::clone(&cached.entries);
        }
    }

    // Checked whether or not anything is cached: with nothing cached and
    // the machine out of memory, hammering the walk on every call is the
    // worst thing to do.
    if let Some(failed_at) = state.failed_at {
        if now().saturating_duration_since(failed_at) < PROC_SNAPSHOT_FAILURE_BACKOFF {
            return stale_or_empty(&state.cached, now());
        }
    }

    let started_at = now();
    let mut buf = match state.spare.take() {
        Some(mut spare) => {
            spare.reset(started_at);
            spare
        }
        None => ProcessEntries::new(started_at),
    };

    if let Err(err) = refill(&mut buf) {
        log::warn!("process snapshot failed: {err}");
        state.failed_at = Some(now());
        // The partial walk stays private as the spare: its capacity is
        // still worth keeping, and the previously published snapshot --
        // which is complete -- remains what callers get.
        state.spare = Some(buf);
        return stale_or_empty(&state.cached, started_at);
    }

    state.failed_at = None;
    let entries = Arc::new(buf);
    let superseded = state.cached.replace(CachedSnapshot {
        updated: now(),
        entries: Arc::clone(&entries),
    });
    // Reclaim the superseded generation's allocations once no caller still
    // holds it, which is the common case: callers drop their `Arc` as soon
    // as their tree walk returns.
    if let Some(superseded) = superseded {
        state.spare = Arc::try_unwrap(superseded.entries).ok();
    }
    entries
}

fn refill_snapshot_entries(dest: &mut ProcessEntries) -> io::Result<()> {
    for info in Snapshot::new()?.iter() {
        let info = info?;
        dest.push(
            info.th32ProcessID,
            info.th32ParentProcessID,
            &info.szExeFile,
        );
    }
    Ok(())
}

fn fresh_snapshot_exe_entries() -> io::Result<SnapshotExeEntries> {
    let mut entries = SnapshotExeEntries::default();
    for info in Snapshot::new()?.iter() {
        let info = info?;
        entries.push(
            info.th32ProcessID,
            info.th32ParentProcessID,
            &info.szExeFile,
        );
    }
    Ok(entries)
}

fn exe_names_from_entries(
    snapshot: &SnapshotExeEntries,
    root_pid: u32,
) -> io::Result<HashSet<String>> {
    let entries = &snapshot.entries;
    let root = entries
        .iter()
        .find(|entry| entry.pid == root_pid)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "pane root PID missing from process snapshot",
            )
        })?;
    let mut children: HashMap<u32, Vec<&SnapshotExeEntry>> = HashMap::new();
    for entry in entries {
        children.entry(entry.ppid).or_default().push(entry);
    }
    let mut names = HashSet::new();
    let mut visited = HashSet::new();
    let mut stack = vec![root];
    while let Some(entry) = stack.pop() {
        if !visited.insert(entry.pid) {
            continue;
        }
        if let Some(name) = entry_exe_name(&snapshot.names[entry.exe.clone()]) {
            names.insert(name);
        }
        if let Some(children) = children.get(&entry.pid) {
            stack.extend(children.iter().copied());
        }
    }
    Ok(names)
}

fn entry_exe_name(exe: &[u16]) -> Option<String> {
    let end = exe.iter().position(|&c| c == 0).unwrap_or(exe.len());
    if end == 0 {
        None
    } else {
        Some(String::from_utf16_lossy(&exe[..end]))
    }
}

impl LocalProcessInfo {
    /// Returns None for exited/inaccessible processes; never treats uncertainty as idle.
    pub fn activity_stamp(pid: u32) -> Option<ProcessActivityStamp> {
        // SAFETY: read/wait-only access; OpenProcess transfers ownership of a non-null handle.
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
        if raw.is_null() {
            return None;
        }
        // SAFETY: the non-null OpenProcess result is exclusively owned and closed exactly once.
        let handle = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let mut cycles = 0;
        // SAFETY: owned live handle has query/SYNCHRONIZE rights; all outputs are initialized.
        // Zero-time wait never blocks. QueryProcessCycleTime includes user and kernel execution.
        unsafe {
            if WaitForSingleObject(handle.as_raw_handle().cast(), 0)
                != winapi::shared::winerror::WAIT_TIMEOUT
                || GetProcessTimes(
                    handle.as_raw_handle().cast(),
                    &mut creation,
                    &mut exit,
                    &mut kernel,
                    &mut user,
                ) == 0
                || QueryProcessCycleTime(handle.as_raw_handle().cast(), &mut cycles) == 0
            {
                return None;
            }
        }
        Some(ProcessActivityStamp {
            creation_time: (u64::from(creation.dwHighDateTime) << 32)
                | u64::from(creation.dwLowDateTime),
            cycles,
        })
    }

    /// Fresh executable base names for a pane's process tree. Unlike
    /// `with_root_pid`, this only reads Toolhelp's PID/PPID/name records:
    /// no OpenProcess, remote memory reads, or asynchronous cache refresh.
    /// Keyboard compatibility decisions must see programs started/stopped
    /// since the last title/render refresh, including on the very first key.
    pub fn fresh_process_tree_exe_names(pid: u32) -> io::Result<HashSet<String>> {
        exe_names_from_entries(&fresh_snapshot_exe_entries()?, pid)
    }

    pub fn current_working_dir(pid: u32) -> Option<PathBuf> {
        log::trace!("current_working_dir({})", pid);
        let proc = ProcHandle::new(pid)?;
        let params = proc.get_params_impl(false)?;
        Some(params.cwd)
    }

    pub fn executable_path(pid: u32) -> Option<PathBuf> {
        log::trace!("executable_path({})", pid);
        let proc = ProcHandle::new(pid)?;
        proc.executable()
    }

    pub fn with_root_pid(pid: u32) -> Option<Self> {
        Self::with_root_pid_and_snapshot(pid).map(|(root, _)| root)
    }

    /// Snapshot start is conservative: data cannot predate this instant.
    pub fn with_root_pid_and_snapshot(pid: u32) -> Option<(Self, Instant)> {
        log::trace!("LocalProcessInfo::with_root_pid({}), getting snapshot", pid);
        // Shared machine-wide snapshot; may be up to PROC_SNAPSHOT_TTL
        // old and is shared across all panes/callers.
        let procs = shared_snapshot_entries();
        log::trace!("Got snapshot");

        // A closure, not a plain `fn`, because the fallback executable name
        // now lives in the snapshot's packed `names` buffer rather than in
        // the entry itself.
        let make_leaf = |info: &ProcessEntry| -> LocalProcessInfo {
            let mut executable = None;
            let mut start_time = 0;
            let mut cwd = PathBuf::new();
            let mut argv = vec![];
            let mut console = 0;

            if let Some(proc) = ProcHandle::new(info.pid) {
                if let Some(exe) = proc.executable() {
                    executable.replace(exe);
                }
                if let Some(params) = proc.get_params() {
                    cwd = params.cwd;
                    argv = params.argv;
                    console = params.console as _;
                }
                if let Some(start) = proc.start_time() {
                    start_time = start;
                }
            }

            let executable = executable.unwrap_or_else(|| procs.exe_path(info));
            let name = match executable.file_name() {
                Some(name) => name.to_string_lossy().into_owned(),
                None => String::new(),
            };

            LocalProcessInfo {
                pid: info.pid,
                ppid: info.ppid,
                name,
                executable,
                cwd,
                argv,
                start_time,
                status: LocalProcessStatus::Run,
                children: HashMap::new(),
                console,
            }
        };

        crate::build_tree_iterative(
            &procs.entries,
            pid,
            |info| info.pid,
            |info| info.ppid,
            make_leaf,
        )
        .map(|root| (root, procs.started_at))
    }

    /// Aggregate CPU time and working-set memory across `pid` and all of
    /// its descendants, reusing the same shared, cached Toolhelp snapshot
    /// as `with_root_pid`. Unlike `with_root_pid`, this never reads a
    /// process's PEB/command line -- only `GetProcessTimes` and
    /// `GetProcessMemoryInfo` on a `PROCESS_QUERY_INFORMATION` handle -- so
    /// it is cheaper per process and, unlike `ProcHandle`, does not skip
    /// `pid` when it equals our own (self-querying via `GetCurrentProcess`'s
    /// pseudo-handle is safe for these two calls; `ProcHandle::new` only
    /// avoids self because of the PEB/`ReadProcessMemory` deadlock risk).
    /// Processes that can no longer be opened (exited between the snapshot
    /// and this call, or access denied) are silently skipped, same as
    /// `with_root_pid`'s per-leaf `ProcHandle::new`.
    pub fn process_tree_resource_usage(pid: u32) -> io::Result<ProcessTreeUsage> {
        let procs = shared_snapshot_entries();
        let root = procs
            .entries
            .iter()
            .find(|entry| entry.pid == pid)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "root pid missing from process snapshot",
                )
            })?;

        let mut children: HashMap<u32, Vec<&ProcessEntry>> = HashMap::new();
        for entry in procs.entries.iter() {
            children.entry(entry.ppid).or_default().push(entry);
        }

        // SAFETY: GetCurrentProcessId has no preconditions and no UB.
        let my_pid = unsafe { GetCurrentProcessId() };
        let mut usage = ProcessTreeUsage::default();
        let mut visited = HashSet::new();
        let mut stack = vec![root];
        while let Some(entry) = stack.pop() {
            if !visited.insert(entry.pid) {
                continue;
            }
            if let Some((cpu_time_100ns, working_set_bytes)) =
                process_cpu_and_memory(entry.pid, my_pid)
            {
                usage.total_cpu_time_100ns += cpu_time_100ns;
                usage.total_working_set_bytes += working_set_bytes;
                usage.process_count += 1;
            }
            if let Some(kids) = children.get(&entry.pid) {
                stack.extend(kids.iter().copied());
            }
        }
        Ok(usage)
    }
}

/// Combined CPU time (kernel+user) and physical RAM footprint of a process
/// tree, as sampled by `LocalProcessInfo::process_tree_resource_usage`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessTreeUsage {
    /// Number of processes actually opened and read (may be less than the
    /// tree's true size if some could not be opened).
    pub process_count: usize,
    /// Sum, across every opened process, of kernel+user CPU time ever
    /// consumed (100-nanosecond units, matching `FILETIME`). This is a
    /// cumulative counter, not a rate -- compute a percentage from the
    /// delta between two samples and the wall-clock time between them.
    pub total_cpu_time_100ns: u64,
    /// Sum, across every opened process, of the process's current working
    /// set size in bytes.
    pub total_working_set_bytes: u64,
}

/// Reads (kernel+user CPU time in 100ns units, working-set bytes) for a
/// single process. `pid == my_pid` uses `GetCurrentProcess()`'s
/// pseudo-handle (always valid, needs no `CloseHandle`); every other pid is
/// opened and closed here.
fn process_cpu_and_memory(pid: u32, my_pid: u32) -> Option<(u64, u64)> {
    if pid == my_pid {
        // SAFETY: GetCurrentProcess returns a pseudo-handle that is always
        // valid and must not be closed.
        read_cpu_and_memory(unsafe { GetCurrentProcess() })
    } else {
        // SAFETY: `PROCESS_QUERY_INFORMATION` and `pid` are valid arguments;
        // `FALSE` is a valid BOOL. The returned handle (or NULL) is closed
        // below before returning.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, FALSE as _, pid) };
        if handle.is_null() {
            return None;
        }
        let result = read_cpu_and_memory(handle);
        // SAFETY: `handle` was just obtained from `OpenProcess` above and is
        // not used again after this point.
        unsafe { CloseHandle(handle) };
        result
    }
}

fn read_cpu_and_memory(handle: HANDLE) -> Option<(u64, u64)> {
    const fn empty() -> FILETIME {
        FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }
    }
    fn filetime_to_100ns(ft: FILETIME) -> u64 {
        (ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64
    }

    let mut create = empty();
    let mut exit = empty();
    let mut kernel = empty();
    let mut user = empty();
    // SAFETY: `handle` is a valid process handle (real or the current-process
    // pseudo-handle); all out-pointers are valid `*mut FILETIME`.
    let res = unsafe { GetProcessTimes(handle, &mut create, &mut exit, &mut kernel, &mut user) };
    if res == 0 {
        return None;
    }
    let cpu_time_100ns = filetime_to_100ns(kernel) + filetime_to_100ns(user);

    // SAFETY: PROCESS_MEMORY_COUNTERS is a plain POD struct of integers; the
    // all-zero bit pattern is a valid value, immediately overwritten below.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: `handle` is valid; `&mut counters` is a valid out-pointer sized
    // by the `cb` field set just above, as the API requires.
    let mem_res = unsafe { GetProcessMemoryInfo(handle, &mut counters, counters.cb) };
    let working_set_bytes = if mem_res == 0 {
        0
    } else {
        counters.WorkingSetSize as u64
    };

    Some((cpu_time_100ns, working_set_bytes))
}

#[cfg(test)]
#[path = "key_compat_tests.rs"]
mod key_compat_tests;

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod resource_usage_tests;
