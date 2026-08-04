use crate::{
    AsRawFileDescriptor, AsRawSocketDescriptor, Error, FileDescriptor, OwnedHandle, Result,
    StdioDescriptor,
};
use std::io::{self, Error as IoError};
use std::os::windows::prelude::*;
use std::ptr;
use winapi::um::fileapi::*;
use winapi::um::processenv::{GetStdHandle, SetStdHandle};
use winapi::um::winsock2::{ioctlsocket, recv, send, SOCKET};

const STD_INPUT_HANDLE: u32 = 4294967286; // -10
const STD_OUTPUT_HANDLE: u32 = 4294967285; // -11
const STD_ERROR_HANDLE: u32 = 4294967284; // -12

impl FileDescriptor {
    #[inline]
    pub(crate) fn as_stdio_impl(&self) -> Result<std::process::Stdio> {
        let duped = self.handle.try_clone()?;
        let handle = duped.into_raw_handle();
        // SAFETY: `handle` is a duplicated, valid HANDLE obtained via
        // DuplicateHandle; ownership is transferred to `Stdio`.
        let stdio = unsafe { std::process::Stdio::from_raw_handle(handle) };
        Ok(stdio)
    }

    #[inline]
    pub(crate) fn as_file_impl(&self) -> Result<std::fs::File> {
        let duped = self.handle.try_clone()?;
        let handle = duped.into_raw_handle();
        // SAFETY: `handle` is a duplicated, valid HANDLE obtained via
        // DuplicateHandle; ownership is transferred to `File`.
        let stdio = unsafe { std::fs::File::from_raw_handle(handle) };
        Ok(stdio)
    }

    #[inline]
    pub(crate) fn set_non_blocking_impl(&mut self, non_blocking: bool) -> Result<()> {
        if !self.handle.is_socket_handle() {
            return Err(Error::OnlySocketsNonBlocking);
        }

        let mut on = if non_blocking { 1 } else { 0 };
        // SAFETY: The handle was verified to be a socket; FIONBIO is a
        // standard ioctl.
        let res = unsafe {
            ioctlsocket(
                self.as_raw_socket() as SOCKET,
                winapi::um::winsock2::FIONBIO,
                &mut on,
            )
        };
        if res != 0 {
            Err(Error::FionBio(std::io::Error::last_os_error()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn redirect_stdio_impl<F: AsRawFileDescriptor>(
        f: &F,
        stdio: StdioDescriptor,
    ) -> Result<Self> {
        let std_handle = match stdio {
            StdioDescriptor::Stdin => STD_INPUT_HANDLE,
            StdioDescriptor::Stdout => STD_OUTPUT_HANDLE,
            StdioDescriptor::Stderr => STD_ERROR_HANDLE,
        };

        // SAFETY: `std_handle` is a valid STD_* constant.
        let raw_std_handle = unsafe { GetStdHandle(std_handle) } as *mut _;
        // SAFETY: `raw_std_handle` is a valid (possibly NULL) stdio handle;
        // ownership is transferred to the returned `FileDescriptor`.
        let std_original = unsafe { FileDescriptor::from_raw_handle(raw_std_handle) };

        let cloned_handle = OwnedHandle::dup(f)?;
        // SAFETY: `std_handle` is a valid STD_* constant; `cloned_handle`
        // is a valid duplicated handle.
        if unsafe { SetStdHandle(std_handle, cloned_handle.into_raw_handle() as *mut _) } == 0 {
            Err(Error::SetStdHandle(std::io::Error::last_os_error()))
        } else {
            Ok(std_original)
        }
    }
}

impl IntoRawHandle for FileDescriptor {
    fn into_raw_handle(self) -> RawHandle {
        self.handle.into_raw_handle()
    }
}

impl AsRawHandle for FileDescriptor {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}

impl FromRawHandle for FileDescriptor {
    // SAFETY: forwarded from the trait contract — the caller guarantees
    // `handle` is a valid, owned HANDLE.
    unsafe fn from_raw_handle(handle: RawHandle) -> FileDescriptor {
        Self {
            handle: OwnedHandle::from_raw_handle(handle),
        }
    }
}

impl IntoRawSocket for FileDescriptor {
    fn into_raw_socket(self) -> RawSocket {
        // FIXME: this isn't a guaranteed conversion!
        debug_assert!(self.handle.is_socket_handle());
        self.handle.into_raw_handle() as RawSocket
    }
}

impl AsRawSocket for FileDescriptor {
    fn as_raw_socket(&self) -> RawSocket {
        // FIXME: this isn't a guaranteed conversion!
        debug_assert!(self.handle.is_socket_handle());
        self.handle.as_raw_handle() as RawSocket
    }
}

impl AsSocket for FileDescriptor {
    fn as_socket(&self) -> BorrowedSocket<'_> {
        // SAFETY: `self.as_raw_socket()` returns a valid socket handle; the
        // `BorrowedSocket` borrows it for the lifetime of `self`.
        unsafe { BorrowedSocket::borrow_raw(self.as_raw_socket()) }
    }
}

impl FromRawSocket for FileDescriptor {
    // SAFETY: forwarded from the trait contract — the caller guarantees
    // `handle` is a valid, owned socket.
    unsafe fn from_raw_socket(handle: RawSocket) -> FileDescriptor {
        Self {
            handle: OwnedHandle::from_raw_handle(handle as RawHandle),
        }
    }
}

impl io::Read for FileDescriptor {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.handle.is_socket_handle() {
            // It's important to use the winsock functions to read/write
            // even though ReadFile and WriteFile technically work; only
            // the winsock functions respect non-blocking mode.
            // SAFETY: `self.as_socket_descriptor()` is a valid socket;
            // `buf.as_mut_ptr()` is a valid buffer of `buf.len()` bytes.
            let num_read = unsafe {
                recv(
                    self.as_socket_descriptor(),
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as _,
                    0,
                )
            };
            if num_read < 0 {
                Err(IoError::last_os_error())
            } else {
                Ok(num_read as usize)
            }
        } else {
            let mut num_read = 0;
            // SAFETY: `self.handle.as_raw_handle()` is a valid file handle;
            // `buf.as_mut_ptr()` and `&mut num_read` are valid out-pointers.
            let ok = unsafe {
                ReadFile(
                    self.handle.as_raw_handle() as *mut _,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as _,
                    &mut num_read,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                let err = IoError::last_os_error();
                if err.kind() == std::io::ErrorKind::BrokenPipe {
                    Ok(0)
                } else {
                    Err(err)
                }
            } else {
                Ok(num_read as usize)
            }
        }
    }
}

impl io::Write for FileDescriptor {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.handle.is_socket_handle() {
            // SAFETY: `self.as_socket_descriptor()` is a valid socket;
            // `buf.as_ptr()` is a valid buffer of `buf.len()` bytes.
            let num_wrote = unsafe {
                send(
                    self.as_socket_descriptor(),
                    buf.as_ptr() as *const _,
                    buf.len() as _,
                    0,
                )
            };
            if num_wrote < 0 {
                Err(IoError::last_os_error())
            } else {
                Ok(num_wrote as usize)
            }
        } else {
            let mut num_wrote = 0;
            // SAFETY: `self.handle.as_raw_handle()` is a valid file handle;
            // `buf.as_ptr()` and `&mut num_wrote` are valid.
            let ok = unsafe {
                WriteFile(
                    self.handle.as_raw_handle() as *mut _,
                    buf.as_ptr() as *const _,
                    buf.len() as u32,
                    &mut num_wrote,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                Err(IoError::last_os_error())
            } else {
                Ok(num_wrote as usize)
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
