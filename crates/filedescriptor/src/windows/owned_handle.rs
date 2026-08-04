use crate::{AsRawFileDescriptor, OwnedHandle, Result};
use std::io::Error as IoError;
use std::os::windows::prelude::*;
use winapi::um::fileapi::*;
use winapi::um::handleapi::*;
use winapi::um::namedpipeapi::GetNamedPipeInfo;
use winapi::um::processthreadsapi::*;
use winapi::um::winbase::{FILE_TYPE_CHAR, FILE_TYPE_DISK, FILE_TYPE_PIPE};
use winapi::um::winnt::HANDLE;
use winapi::um::winsock2::{
    closesocket, getsockopt, WSAGetLastError, SOL_SOCKET, SO_ERROR, WSAENOTSOCK,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum HandleType {
    Char,
    Disk,
    Pipe,
    Socket,
    #[default]
    Unknown,
}

// SAFETY: `OwnedHandle` wraps a Windows kernel `HANDLE`, which is a
// process-global, thread-independent value (it is an integer/pointer into
// the process handle table, not a thread-local resource). Moving or sharing
// it across threads is sound; the handle carries no thread affinity.
// Mutation of the underlying kernel object is governed by the kernel, not by
// Rust's `&mut` exclusivity.
unsafe impl Send for OwnedHandle {}
// SAFETY: same rationale as the `Send` impl above - the handle has no thread
// affinity and the kernel, not `&mut` exclusivity, governs mutation safety.
unsafe impl Sync for OwnedHandle {}

impl OwnedHandle {
    fn probe_handle_type_if_unknown(handle: RawHandle, handle_type: HandleType) -> HandleType {
        match handle_type {
            HandleType::Unknown => Self::probe_handle_type(handle),
            t => t,
        }
    }

    pub(crate) fn probe_handle_type(handle: RawHandle) -> HandleType {
        let handle = handle as HANDLE;
        match
            // SAFETY: `handle` is a valid HANDLE (or INVALID_HANDLE_VALUE).
            unsafe { GetFileType(handle) }
        {
            FILE_TYPE_CHAR => HandleType::Char,
            FILE_TYPE_DISK => HandleType::Disk,
            FILE_TYPE_PIPE => {
                // Could be a pipe or a socket.  Test if for pipeness
                let mut flags = 0;
                let mut out_buf = 0;
                let mut in_buf = 0;
                let mut inst = 0;
                // SAFETY: `handle` is a valid HANDLE; all out-pointers are
                // valid `*mut DWORD`.
                if unsafe {
                    GetNamedPipeInfo(handle, &mut flags, &mut out_buf, &mut in_buf, &mut inst)
                } != 0
                {
                    HandleType::Pipe
                } else {
                    // It's probably a socket, but it may be a special device used
                    // when piping between WSL and native win32 apps.
                    let mut err = 0;
                    let mut errsize = std::mem::size_of_val(&err) as _;
                    // SAFETY: `handle` is a valid HANDLE cast to a SOCKET;
                    // all out-pointers are valid.
                    if unsafe {
                        getsockopt(
                            handle as _,
                            SOL_SOCKET,
                            SO_ERROR,
                            &mut err as *mut _ as *mut i8,
                            &mut errsize,
                        ) != 0
                            && WSAGetLastError() == WSAENOTSOCK
                    } {
                        HandleType::Pipe
                    } else {
                        HandleType::Socket
                    }
                }
            }
            _ => HandleType::Unknown,
        }
    }

    pub(crate) fn is_socket_handle(&self) -> bool {
        match self.handle_type {
            HandleType::Socket => true,
            HandleType::Unknown => Self::probe_handle_type(self.handle) == HandleType::Socket,
            _ => false,
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !std::ptr::eq(self.handle, INVALID_HANDLE_VALUE as _) && !self.handle.is_null() {
            // SAFETY: `self.handle` was checked non-null and non-INVALID;
            // `is_socket_handle` determines the correct close function.
            unsafe {
                if self.is_socket_handle() {
                    closesocket(self.handle as _);
                } else {
                    CloseHandle(self.handle as _);
                }
            };
        }
    }
}

impl FromRawHandle for OwnedHandle {
    unsafe fn from_raw_handle(handle: RawHandle) -> Self {
        // SAFETY: forwarded from the trait contract — the caller guarantees
        // `handle` is a valid, owned HANDLE (or INVALID_HANDLE_VALUE).
        OwnedHandle {
            handle,
            handle_type: Self::probe_handle_type(handle),
        }
    }
}

impl OwnedHandle {
    #[inline]
    pub(crate) fn dup_impl<F: AsRawFileDescriptor>(f: &F, handle_type: HandleType) -> Result<Self> {
        let handle = f.as_raw_file_descriptor();
        if std::ptr::eq(handle, INVALID_HANDLE_VALUE as _) || handle.is_null() {
            return Ok(OwnedHandle {
                handle,
                handle_type,
            });
        }

        let handle_type = Self::probe_handle_type_if_unknown(handle, handle_type);

        // SAFETY: GetCurrentProcess returns a pseudo-handle with no
        // preconditions.
        let proc = unsafe { GetCurrentProcess() };
        let mut duped = INVALID_HANDLE_VALUE;
        // SAFETY: `proc` is a valid pseudo-handle; `handle` is a valid source;
        // `&mut duped` is a valid out-pointer.
        let ok = unsafe {
            DuplicateHandle(
                proc,
                handle as *mut _,
                proc,
                &mut duped,
                0,
                0, // not inheritable
                winapi::um::winnt::DUPLICATE_SAME_ACCESS,
            )
        };
        if ok == 0 {
            Err(IoError::last_os_error().into())
        } else {
            Ok(OwnedHandle {
                handle: duped as *mut _,
                handle_type,
            })
        }
    }
}

impl AsRawHandle for OwnedHandle {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle
    }
}

impl IntoRawHandle for OwnedHandle {
    fn into_raw_handle(self) -> RawHandle {
        let handle = self.handle;
        std::mem::forget(self);
        handle
    }
}
