//! Windows parent-process watcher: exits the current process when a given
//! parent PID dies.
//!
//! Adapted from `wezterm-mux-server`'s `--single-pane`/`--supervise-pid`
//! watcher (`crates/wezterm-mux-server/src/main.rs`), which is a separate
//! binary crate and so cannot be called into directly. Kept here, in a
//! crate both `wezterm-mux-server` callers and `wezterm-gui`'s
//! `--gpu-tab-host` mode (task #650) can depend on, as a fallback layered on
//! top of a Job Object (`windows_job::assign_to_kill_on_close_job`) rather
//! than a replacement for it -- see that module's doc comment for why both
//! exist.

#[cfg(windows)]
pub fn spawn_parent_watcher(parent_pid: u32) {
    use winapi::shared::minwindef::DWORD;
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winbase::{INFINITE, WAIT_OBJECT_0};
    use winapi::um::winnt::SYNCHRONIZE;

    /// Carries the parent's process handle onto the watcher thread.
    ///
    /// A Windows HANDLE is an index into a process-wide table, not something
    /// owned by the thread that obtained it, so using it from another thread
    /// is sound -- but it is spelled as a raw pointer, which the compiler
    /// must assume is not `Send`.
    struct ParentHandle(winapi::um::winnt::HANDLE);
    // SAFETY: see above -- the handle is valid process-wide, and exactly one
    // thread owns this value at a time because it is moved, not shared.
    unsafe impl Send for ParentHandle {}

    // Resolve the pid to a handle HERE, on the calling thread, before the
    // watcher thread even exists. Windows recycles process ids, so every
    // moment between the parent handing us its pid and us pinning it down is
    // a window in which that pid could come to mean some unrelated process.
    //
    // SAFETY: OpenProcess is a standard Windows API call; SYNCHRONIZE is the
    // minimal access right needed to wait on the handle.
    let h_parent = unsafe { OpenProcess(SYNCHRONIZE, false as i32, parent_pid as DWORD) };

    if h_parent.is_null() {
        // SAFETY: GetLastError takes no arguments and has no preconditions;
        // it just reads the calling thread's last-error TLS slot.
        let error_code = unsafe { GetLastError() };
        log::warn!(
            "Parent-watcher: OpenProcess({}) failed with error {}. \
             This typically means the parent has already exited.",
            parent_pid,
            error_code
        );
        // Parent already gone before we finished starting: exit rather than
        // carry on hosting a tab nothing can reach.
        std::process::exit(1);
    }

    let h_parent = ParentHandle(h_parent);

    std::thread::spawn(move || {
        let h_parent = h_parent.0;

        log::info!(
            "Parent-watcher: opened handle to parent PID {}, waiting...",
            parent_pid
        );

        // SAFETY: h_parent is a valid process handle returned by OpenProcess.
        let wait_result = unsafe { WaitForSingleObject(h_parent, INFINITE) };

        // SAFETY: h_parent is a valid process handle and we have sole
        // ownership of it; closing it here is correct after we're done
        // waiting.
        unsafe { CloseHandle(h_parent) };

        match wait_result {
            WAIT_OBJECT_0 => {
                log::info!(
                    "Parent-watcher: parent PID {} has terminated, exiting",
                    parent_pid
                );
                std::process::exit(0);
            }
            _ => {
                // SAFETY: GetLastError takes no arguments and has no
                // preconditions.
                let error_code = unsafe { GetLastError() };
                log::error!(
                    "Parent-watcher: WaitForSingleObject returned unexpected result {} \
                     (GetLastError={}). Continuing without parent supervision.",
                    wait_result,
                    error_code
                );
            }
        }
    });
}

/// Stub implementation for non-Windows platforms.
#[cfg(not(windows))]
pub fn spawn_parent_watcher(_parent_pid: u32) {}
