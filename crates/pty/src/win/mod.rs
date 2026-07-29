use crate::{Child, ChildKiller, ExitStatus};
use anyhow::Context as _;
use std::io::{Error as IoError, Result as IoResult};
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use winapi::shared::minwindef::DWORD;
use winapi::um::handleapi::CloseHandle;
use winapi::um::minwinbase::STILL_ACTIVE;
use winapi::um::processthreadsapi::*;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::INFINITE;

pub mod conpty;
mod procthreadattr;
mod pseudocon;

use filedescriptor::OwnedHandle;

/// How long to wait for the child to exit on its own (eg. because the pty
/// was already closed, or the process is shutting down for an unrelated
/// reason) before forcefully terminating it (and any surviving descendants
/// via the Job Object).
///
/// This used to also send CTRL_BREAK_EVENT as a "polite" first step, which
/// required creating the child with CREATE_NEW_PROCESS_GROUP so the signal
/// could be scoped to just this child. That flag has a Windows-documented
/// side effect: processes created with it stop receiving CTRL_C_EVENT (ie.
/// the user's physical Ctrl+C) entirely, only CTRL_BREAK_EVENT reaches them.
/// That silently broke Ctrl+C for everything running in the pane, which is
/// far more important than a slightly gentler kill sequence, so the
/// CTRL_BREAK_EVENT step and CREATE_NEW_PROCESS_GROUP were both removed.
const GRACEFUL_KILL_TIMEOUT_MS: DWORD = 5000;

/// Handle to the Windows Job Object that a child (and any descendants it
/// spawns) is assigned to, shared between a `WinChild` and any
/// `WinChildKiller`s split out from it via `clone_killer()`. The job is
/// configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so closing this
/// handle force-kills every process still assigned to it. This lets us
/// clean up grandchild processes that a direct `TerminateProcess` on the
/// main process handle alone would not reach.
///
/// This is shared (`Arc<Mutex<..>>>`) rather than owned outright by
/// `WinChild` because the actual kill in normal pane-close operation goes
/// through a `WinChildKiller` split out via `clone_killer()` (see
/// `mux::localpane::split_child`), not through `WinChild::kill()` directly.
/// Whichever kill path runs takes the handle out of the `Option` so the
/// job is only ever closed once.
type SharedJobHandle = Arc<Mutex<Option<OwnedHandle>>>;

#[derive(Debug)]
pub struct WinChild {
    proc: Mutex<OwnedHandle>,
    job: SharedJobHandle,
}

impl WinChild {
    fn is_complete(&mut self) -> IoResult<Option<ExitStatus>> {
        let mut status: DWORD = 0;
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        // SAFETY: `proc` is a duplicated, valid process handle. `&mut status`
        // is a valid `*mut DWORD`.
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            if status == STILL_ACTIVE {
                Ok(None)
            } else {
                Ok(Some(ExitStatus::with_exit_code(status)))
            }
        } else {
            Ok(None)
        }
    }

    fn do_kill(&mut self) -> IoResult<()> {
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        // Take the job handle out so that we own its lifetime for the
        // duration of the background thread below; there's nothing else
        // that needs it once a kill has been requested.
        let job = self.job.lock().unwrap().take();
        std::thread::spawn(move || {
            kill_gracefully_then_forcefully(proc, job);
        });
        Ok(())
    }
}

/// Give the child up to `GRACEFUL_KILL_TIMEOUT_MS` to exit on its own, then
/// forcefully terminate it and close the Job Object handle (if we have one)
/// to force-kill any surviving descendant processes via
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
///
/// Even if the process exits on its own within the timeout, we still close
/// the job handle afterwards: the user wants ALL descendant processes
/// gone, not just the direct child, and any grandchildren that are still
/// alive in the job at that point are cleaned up as a result.
///
/// This runs on a background thread (spawned by `do_kill`/`WinChildKiller::kill`)
/// so blocking here for up to 5 seconds does not stall the GUI/mux thread.
fn kill_gracefully_then_forcefully(proc: OwnedHandle, job: Option<OwnedHandle>) {
    // SAFETY: `proc` is a duplicated, valid process handle.
    let wait_result = unsafe { WaitForSingleObject(proc.as_raw_handle() as _, GRACEFUL_KILL_TIMEOUT_MS) };
    const WAIT_OBJECT_0: DWORD = 0;
    if wait_result != WAIT_OBJECT_0 {
        // Still running after the grace period: escalate to a forceful
        // kill of the direct child.
        // SAFETY: `proc` is a valid process handle.
        unsafe { TerminateProcess(proc.as_raw_handle() as _, 1) };
    }

    // Whether the process exited on its own or we just forced it, close
    // the Job Object handle (if any) so that JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    // cleans up any descendant processes still assigned to it.
    if let Some(job) = job {
        // SAFETY: `job` is a valid Job Object handle from CreateJobObjectW.
        unsafe {
            CloseHandle(job.as_raw_handle() as _);
        }
    }
}

impl ChildKiller for WinChild {
    fn kill(&mut self) -> IoResult<()> {
        self.do_kill().ok();
        Ok(())
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        // Duplicating the process handle is a fallible kernel operation
        // (eg: handle table exhaustion). `clone_killer` has an infallible
        // signature, so rather than panicking here (which used to bring
        // down the whole process just because a `ChildKiller` was cloned;
        // see wezterm/wezterm#5107) we degrade gracefully: produce a
        // killer with no handle, whose `kill()` becomes a harmless no-op.
        let proc = self.proc.lock().unwrap().try_clone().ok();
        if proc.is_none() {
            log::warn!(
                "WinChild::clone_killer: failed to duplicate the process \
                 handle; the returned killer will be unable to terminate \
                 the process"
            );
        }
        Box::new(WinChildKiller {
            proc,
            job: Arc::clone(&self.job),
        })
    }
}

#[derive(Debug)]
pub struct WinChildKiller {
    proc: Option<OwnedHandle>,
    job: SharedJobHandle,
}

impl ChildKiller for WinChildKiller {
    fn kill(&mut self) -> IoResult<()> {
        let proc = match &self.proc {
            Some(proc) => proc.try_clone().map_err(|e| {
                IoError::new(std::io::ErrorKind::Other, format!("Failed to clone handle: {}", e))
            })?,
            // No handle available (eg: because an earlier duplication
            // attempt failed); treat this as a no-op rather than panicking
            // or erroring out.
            None => return Ok(()),
        };
        // Take the job handle out so that we own its lifetime for the
        // duration of the background thread below; there's nothing else
        // that needs it once a kill has been requested. This also ensures
        // the job is only ever closed once, even if `kill()` is called
        // more than once or from multiple cloned killers.
        let job = self.job.lock().unwrap().take();
        std::thread::spawn(move || {
            kill_gracefully_then_forcefully(proc, job);
        });
        Ok(())
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.as_ref().and_then(|proc| proc.try_clone().ok());
        Box::new(WinChildKiller {
            proc,
            job: Arc::clone(&self.job),
        })
    }
}

impl Child for WinChild {
    fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
        self.is_complete()
    }

    fn wait(&mut self) -> IoResult<ExitStatus> {
        if let Ok(Some(status)) = self.try_wait() {
            return Ok(status);
        }
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        // SAFETY: `proc` is a duplicated, valid process handle.
        unsafe {
            WaitForSingleObject(proc.as_raw_handle() as _, INFINITE);
        }
        let mut status: DWORD = 0;
        // SAFETY: `proc` is a valid process handle; `&mut status` is valid.
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            Ok(ExitStatus::with_exit_code(status))
        } else {
            Err(IoError::last_os_error())
        }
    }

    fn process_id(&self) -> Option<u32> {
        // SAFETY: The handle is valid for the lifetime of `WinChild`.
        let res = unsafe { GetProcessId(self.proc.lock().unwrap().as_raw_handle() as _) };
        if res == 0 {
            None
        } else {
            Some(res)
        }
    }

    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        let proc = self.proc.lock().unwrap();
        Some(proc.as_raw_handle())
    }
}

impl std::future::Future for WinChild {
    type Output = anyhow::Result<ExitStatus>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<anyhow::Result<ExitStatus>> {
        match self.is_complete() {
            Ok(Some(status)) => Poll::Ready(Ok(status)),
            Err(err) => Poll::Ready(Err(err).context("Failed to retrieve process exit status")),
            Ok(None) => {
                struct PassRawHandleToWaiterThread(pub RawHandle);
                // SAFETY: `RawHandle` is a process-global Windows handle with no
                // thread affinity. The handle was obtained via `try_clone`
                // (DuplicateHandle), so it is independently owned and safe to send
                // to the waiter thread.
                unsafe impl Send for PassRawHandleToWaiterThread {}

                let proc = self.proc.lock().unwrap().try_clone()?;
                let handle = PassRawHandleToWaiterThread(proc.as_raw_handle());

                let waker = cx.waker().clone();
                std::thread::spawn(move || {
                    // SAFETY: `handle.0` is a duplicated, valid process handle.
                    unsafe {
                        WaitForSingleObject(handle.0 as _, INFINITE);
                    }
                    waker.wake();
                });
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression test for wezterm/wezterm#5107: `clone_killer()` used to
    // `.unwrap()` the result of duplicating the process handle, which
    // panics if `DuplicateHandle` ever fails (eg: handle table exhaustion,
    // or the underlying process handle having become otherwise unusable).
    // A `ChildKiller` may be cloned and used from an independent thread
    // specifically so that callers can signal a process while another
    // thread is blocked in `.wait()`, so a panic here can bring down an
    // otherwise-healthy process. `WinChildKiller::kill()` must instead
    // degrade to a harmless no-op when it holds no handle.
    #[test]
    fn clone_killer_with_no_handle_does_not_panic() {
        let mut killer = WinChildKiller {
            proc: None,
            job: Arc::new(Mutex::new(None)),
        };

        // kill() on a handle-less killer must be a harmless no-op, not a
        // panic or a hard error.
        assert!(killer.kill().is_ok());

        // clone_killer() must likewise not panic when there is no handle
        // to duplicate, and the clone must itself still be inert.
        let mut cloned = killer.clone_killer();
        assert!(cloned.kill().is_ok());
    }
}
