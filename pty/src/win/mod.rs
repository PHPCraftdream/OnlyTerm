use crate::{Child, ChildKiller, ExitStatus};
use anyhow::Context as _;
use std::io::{Error as IoError, Result as IoResult};
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use winapi::shared::minwindef::DWORD;
use winapi::um::minwinbase::STILL_ACTIVE;
use winapi::um::processthreadsapi::*;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::INFINITE;

pub mod conpty;
mod procthreadattr;
mod pseudocon;

use filedescriptor::OwnedHandle;

#[derive(Debug)]
pub struct WinChild {
    proc: Mutex<OwnedHandle>,
}

impl WinChild {
    fn is_complete(&mut self) -> IoResult<Option<ExitStatus>> {
        let mut status: DWORD = 0;
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
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
        std::thread::spawn(move || {
            unsafe { TerminateProcess(proc.as_raw_handle() as _, 1) };
        });
        Ok(())
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
        Box::new(WinChildKiller { proc })
    }
}

#[derive(Debug)]
pub struct WinChildKiller {
    proc: Option<OwnedHandle>,
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
        std::thread::spawn(move || {
            unsafe { TerminateProcess(proc.as_raw_handle() as _, 1) };
        });
        Ok(())
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.as_ref().and_then(|proc| proc.try_clone().ok());
        Box::new(WinChildKiller { proc })
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
        unsafe {
            WaitForSingleObject(proc.as_raw_handle() as _, INFINITE);
        }
        let mut status: DWORD = 0;
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            Ok(ExitStatus::with_exit_code(status))
        } else {
            Err(IoError::last_os_error())
        }
    }

    fn process_id(&self) -> Option<u32> {
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
                unsafe impl Send for PassRawHandleToWaiterThread {}

                let proc = self.proc.lock().unwrap().try_clone()?;
                let handle = PassRawHandleToWaiterThread(proc.as_raw_handle());

                let waker = cx.waker().clone();
                std::thread::spawn(move || {
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
        let mut killer = WinChildKiller { proc: None };

        // kill() on a handle-less killer must be a harmless no-op, not a
        // panic or a hard error.
        assert!(killer.kill().is_ok());

        // clone_killer() must likewise not panic when there is no handle
        // to duplicate, and the clone must itself still be inert.
        let mut cloned = killer.clone_killer();
        assert!(cloned.kill().is_ok());
    }
}
