use std::io::Error as IoError;
use std::ptr::{null, null_mut};
use winapi::um::handleapi::CloseHandle;
use winapi::um::synchapi::{CreateEventW, ResetEvent, SetEvent};
// Only `is_signalled` uses these, and it is itself `#[cfg(test)]`.
#[cfg(test)]
use winapi::um::synchapi::WaitForSingleObject;
#[cfg(test)]
use winapi::um::winbase::WAIT_OBJECT_0;
use winapi::um::winnt::HANDLE;

pub struct EventHandle(pub HANDLE);
// SAFETY: `EventHandle` owns a HANDLE to a manual-reset event object. Windows
// event objects are kernel synchronization primitives that are explicitly
// designed to be signaled/waited on from any thread, so sharing the handle
// across threads (Send + Sync) is sound. The handle is only ever passed to
// the event APIs below and is closed exactly once in `Drop`.
unsafe impl Send for EventHandle {}
// SAFETY: same rationale as the `Send` impl above.
unsafe impl Sync for EventHandle {}

impl Drop for EventHandle {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a valid, owned HANDLE created by `CreateEventW`
        // and not closed elsewhere; `CloseHandle` releases it exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

impl EventHandle {
    pub fn new_manual_reset() -> anyhow::Result<Self> {
        // SAFETY: all arguments are valid constants: no security attributes,
        // a manual-reset event, initially non-signaled, and an unnamed object.
        let handle = unsafe { CreateEventW(null_mut(), 1, 0, null()) };
        if handle.is_null() {
            return Err(IoError::last_os_error().into());
        }
        Ok(Self(handle))
    }

    pub fn set_event(&self) {
        // SAFETY: `self.0` is a valid event HANDLE owned by this `EventHandle`.
        unsafe {
            SetEvent(self.0);
        }
    }

    pub fn reset_event(&self) {
        // SAFETY: `self.0` is a valid event HANDLE owned by this `EventHandle`.
        unsafe {
            ResetEvent(self.0);
        }
    }

    /// Returns true if the event is currently signalled, without blocking.
    #[cfg(test)]
    pub fn is_signalled(&self) -> bool {
        // SAFETY: `self.0` is a valid event HANDLE; a zero timeout polls without
        // blocking.
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
    }
}
