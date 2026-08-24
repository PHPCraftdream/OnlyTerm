//! Windows Job Object helpers for enforcing child process cleanup.
//!
//! This module provides functionality to create Windows Job Objects configured
//! with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE. When a process is assigned to such a
//! job, the kernel will force-kill all processes in the job when the last handle
//! to the job closes - even if the parent dies via TerminateProcess without
//! any userspace cleanup running.
//!
//! This is used as a defense-in-depth mechanism for non-elevated hosting children
//! (onlyterm-mux-server.exe --single-pane), ensuring they are destroyed when the
//! GUI dies even if the child is wedged in a blocking syscall and cannot respond
//! to its normal shutdown signals.

use filedescriptor::OwnedHandle;
use std::io::Error as IoError;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr;

#[cfg(windows)]
use winapi::um::handleapi::CloseHandle;
#[cfg(windows)]
use winapi::um::jobapi2::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject};
#[cfg(windows)]
use winapi::um::winnt::{
    JobObjectExtendedLimitInformation, HANDLE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// Creates a Job Object configured with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
/// and assigns the given child process to it.
///
/// Returns the job handle if successful, or None if the job object could not
/// be created or the process could not be assigned. Failure is non-fatal:
/// the caller should log a warning and proceed without job-based cleanup.
///
/// # Handle ownership and lifetime
///
/// The returned job handle is owned by the caller and must be kept alive for
/// as long as the child process should be allowed to live. When this handle
/// closes (either explicitly or when the owning Rust value is dropped), the
/// kernel will force-kill all processes still assigned to the job.
///
/// **Critical: the job handle must not be inheritable.** An inherited handle
/// would be duplicated into the child process, keeping the job alive even after
/// all handles in the parent process have closed, which would defeat the entire
/// mechanism. The handle returned by this function is non-inheritable by
/// construction (CreateJobObjectW with NULL security attributes produces a
/// non-inheritable handle).
///
/// # Multiple tabs: one job per child or one shared job?
///
/// This function creates one job per child process. This is the correct choice
/// for two reasons:
///
/// 1. **Handle lifetime semantics**: With one job per child, we can create the
///    job immediately after spawning and keep its handle tied to the lifetime
///    of the connection object that represents that specific tab. When the tab
///    closes (or the GUI dies), the handle drops and the job closes, killing
///    just that one child. A shared job would require tracking how many children
///    are still in it and deciding when to close the job handle, which adds
///    complexity and opportunities for bugs.
///
/// 2. **Isolation of failures**: If one child crashes or misbehaves in a way
///    that somehow corrupts the job state, other children are not affected.
///    Each tab gets its own cleanup guarantee independent of the others.
///
/// # Example
///
/// Shown as prose rather than a doc test: this module is private to the
/// crate, so a doc test -- which is compiled as if it were a separate crate --
/// could not reach this function to call it.
///
/// ```text
/// let child = cmd.spawn()?;
///
/// match assign_to_kill_on_close_job(&child, "child.exe") {
///     Some(job) => // keep `job` alive as long as the child should live;
///                  // dropping it has the kernel kill the child
///     None => // job setup failed: already warned, carry on without it
/// }
/// ```
#[cfg(windows)]
pub fn assign_to_kill_on_close_job(
    child: &std::process::Child,
    process_name: &str,
) -> Option<OwnedHandle> {
    // SAFETY: FFI call with NULL security attributes and NULL name;
    // returns either a valid job handle or NULL.
    let job = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };

    if job.is_null() {
        log::warn!(
            "CreateJobObjectW failed: {}; {} will not be automatically \
             cleaned up when the GUI dies",
            IoError::last_os_error(),
            process_name,
        );
        return None;
    }

    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
        // SAFETY: `repr(C)` POD struct of primitive types; valid
        // zero-initialized.
        unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    // SAFETY: `job` is a valid handle from CreateJobObjectW.
    // `info` is a properly initialized JOBOBJECT_EXTENDED_LIMIT_INFORMATION.
    let set_res = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };

    if set_res == 0 {
        log::warn!(
            "SetInformationJobObject failed: {}; {} will not be automatically \
             cleaned up when the GUI dies",
            IoError::last_os_error(),
            process_name,
        );
        // SAFETY: `job` is a valid handle; we clean it up because
        // SetInformationJobObject failed.
        unsafe {
            CloseHandle(job);
        }
        return None;
    }

    // SAFETY: Both `job` (from CreateJobObjectW) and the child's process handle
    // are valid handles.
    let assign_res = unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) };

    if assign_res == 0 {
        log::warn!(
            "AssignProcessToJobObject failed: {}; {} will not be automatically \
             cleaned up when the GUI dies",
            IoError::last_os_error(),
            process_name,
        );
        // SAFETY: `job` is a valid handle; we clean it up because
        // AssignProcessToJobObject failed.
        unsafe {
            CloseHandle(job);
        }
        return None;
    }

    // SAFETY: `job` is a valid, fully-configured job object handle.
    // We take exclusive ownership so it is closed (and thus triggers
    // KILL_ON_JOB_CLOSE) on drop.
    Some(unsafe { OwnedHandle::from_raw_handle(job as _) })
}

/// Stub implementation for non-Windows platforms.
#[cfg(not(windows))]
pub fn assign_to_kill_on_close_job(
    _child: &std::process::Child,
    _process_name: &str,
) -> Option<filedescriptor::OwnedHandle> {
    None
}
