use crate::{
    AsRawFileDescriptor, AsRawSocketDescriptor, Error, FileDescriptor, FromRawFileDescriptor,
    FromRawSocketDescriptor, IntoRawFileDescriptor, IntoRawSocketDescriptor, OwnedHandle, Pipe,
    Result, StdioDescriptor,
};
use std::io::{self, Error as IoError};
use std::os::windows::prelude::*;
use std::ptr;
use std::sync::Once;
use std::time::Duration;
use winapi::shared::ws2def::{AF_INET, INADDR_LOOPBACK, SOCKADDR_IN};
use winapi::um::fileapi::*;
use winapi::um::handleapi::*;
use winapi::um::minwinbase::SECURITY_ATTRIBUTES;
use winapi::um::namedpipeapi::{CreatePipe, GetNamedPipeInfo};
use winapi::um::processenv::{GetStdHandle, SetStdHandle};
use winapi::um::processthreadsapi::*;
use winapi::um::winbase::{FILE_TYPE_CHAR, FILE_TYPE_DISK, FILE_TYPE_PIPE};
use winapi::um::winnt::HANDLE;
use winapi::um::winsock2::{
    accept, bind, closesocket, connect, getsockname, getsockopt, htonl, ioctlsocket, listen, recv,
    send, WSAGetLastError, WSAPoll, WSASocketW, WSAStartup, INVALID_SOCKET, SOCKET, SOCK_STREAM,
    SOL_SOCKET, SO_ERROR, WSADATA, WSAENOTSOCK, WSA_FLAG_NO_HANDLE_INHERIT,
};
pub use winapi::um::winsock2::{POLLERR, POLLHUP, POLLIN, POLLOUT, WSAPOLLFD as pollfd};

/// `RawFileDescriptor` is a platform independent type alias for the
/// underlying platform file descriptor type.  It is primarily useful
/// for avoiding using `cfg` blocks in platform independent code.
pub type RawFileDescriptor = RawHandle;

/// `SocketDescriptor` is a platform independent type alias for the
/// underlying platform socket descriptor type.  It is primarily useful
/// for avoiding using `cfg` blocks in platform independent code.
pub type SocketDescriptor = SOCKET;

const STD_INPUT_HANDLE: u32 = 4294967286; // -10
const STD_OUTPUT_HANDLE: u32 = 4294967285; // -11
const STD_ERROR_HANDLE: u32 = 4294967284; // -12

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HandleType {
    Char,
    Disk,
    Pipe,
    Socket,
    Unknown,
}

impl Default for HandleType {
    fn default() -> Self {
        HandleType::Unknown
    }
}

impl<T: AsRawHandle> AsRawFileDescriptor for T {
    fn as_raw_file_descriptor(&self) -> RawFileDescriptor {
        self.as_raw_handle()
    }
}

impl<T: IntoRawHandle> IntoRawFileDescriptor for T {
    fn into_raw_file_descriptor(self) -> RawFileDescriptor {
        self.into_raw_handle()
    }
}

impl<T: FromRawHandle> FromRawFileDescriptor for T {
    unsafe fn from_raw_file_descriptor(handle: RawHandle) -> Self {
        Self::from_raw_handle(handle)
    }
}

impl<T: AsRawSocket> AsRawSocketDescriptor for T {
    fn as_socket_descriptor(&self) -> SocketDescriptor {
        self.as_raw_socket() as SocketDescriptor
    }
}

impl<T: IntoRawSocket> IntoRawSocketDescriptor for T {
    fn into_socket_descriptor(self) -> SocketDescriptor {
        self.into_raw_socket() as SocketDescriptor
    }
}

impl<T: FromRawSocket> FromRawSocketDescriptor for T {
    unsafe fn from_socket_descriptor(handle: SocketDescriptor) -> Self {
        Self::from_raw_socket(handle as _)
    }
}

// SAFETY: `OwnedHandle` wraps a Windows kernel `HANDLE`, which is a
// process-global, thread-independent value (it is an integer/pointer into
// the process handle table, not a thread-local resource). Moving or sharing
// it across threads is sound; the handle carries no thread affinity.
// Mutation of the underlying kernel object is governed by the kernel, not by
// Rust's `&mut` exclusivity.
unsafe impl Send for OwnedHandle {}
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

    fn is_socket_handle(&self) -> bool {
        match self.handle_type {
            HandleType::Socket => true,
            HandleType::Unknown => Self::probe_handle_type(self.handle) == HandleType::Socket,
            _ => false,
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE as _ && !self.handle.is_null() {
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
        if handle == INVALID_HANDLE_VALUE as _ || handle.is_null() {
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
    fn as_socket(&self) -> BorrowedSocket {
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

impl Pipe {
    pub fn new() -> Result<Pipe> {
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 0,
        };
        let mut read: HANDLE = INVALID_HANDLE_VALUE as _;
        let mut write: HANDLE = INVALID_HANDLE_VALUE as _;
        if unsafe { CreatePipe(&mut read, &mut write, &mut sa, 0) } == 0 {
            Err(Error::Pipe(IoError::last_os_error()))
        } else {
            Ok(Pipe {
                read: FileDescriptor {
                    handle: OwnedHandle {
                        handle: read as _,
                        handle_type: HandleType::Pipe,
                    },
                },
                write: FileDescriptor {
                    handle: OwnedHandle {
                        handle: write as _,
                        handle_type: HandleType::Pipe,
                    },
                },
            })
        }
    }
}

fn init_winsock() {
    static START: Once = Once::new();
    START.call_once(||
        // SAFETY: WSAStartup is the standard winsock initialization call;
        // `0x202` requests version 2.2; `&mut data` is a valid out-pointer.
        // WSADATA is a `repr(C)` struct valid when zero-initialized.
        unsafe {
            let mut data: WSADATA = std::mem::zeroed();
            let ret = WSAStartup(0x202, &mut data); // version 2.2
            assert_eq!(ret, 0, "failed to initialize winsock");
        });
}

fn socket(af: i32, sock_type: i32, proto: i32) -> Result<FileDescriptor> {
    // SAFETY: `af`, `sock_type`, `proto` are valid socket parameters;
    // WSA_FLAG_NO_HANDLE_INHERIT is a valid flag.
    let s = unsafe {
        WSASocketW(
            af,
            sock_type,
            proto,
            ptr::null_mut(),
            0,
            WSA_FLAG_NO_HANDLE_INHERIT,
        )
    };
    if s == INVALID_SOCKET {
        Err(Error::Socket(IoError::last_os_error()))
    } else {
        Ok(FileDescriptor {
            handle: OwnedHandle {
                handle: s as _,
                handle_type: HandleType::Socket,
            },
        })
    }
}

#[doc(hidden)]
pub fn socketpair_impl() -> Result<(FileDescriptor, FileDescriptor)> {
    init_winsock();

    let s = socket(AF_INET, SOCK_STREAM, 0)?;

    // SAFETY: SOCKADDR_IN is a `repr(C)` struct of primitive types;
    // zero-initialization is valid.
    let mut in_addr: SOCKADDR_IN = unsafe { std::mem::zeroed() };
    in_addr.sin_family = AF_INET as _;
    // SAFETY: `S_un.S_addr_mut()` returns a valid `*mut u32` inside the
    // union; `htonl` converts the loopback address to network byte order.
    unsafe {
        *in_addr.sin_addr.S_un.S_addr_mut() = htonl(INADDR_LOOPBACK);
    }

    // SAFETY: `bind` takes a `*const SOCKADDR`; SOCKADDR_IN is layout-
    // compatible (it is a superset). We use an explicit cast instead of
    // transmute to avoid the unsafe pointer reinterpretation.
    {
        let addr_ptr = &in_addr as *const SOCKADDR_IN as *const winapi::shared::ws2def::SOCKADDR;
        if unsafe {
            bind(
                s.as_raw_handle() as _,
                addr_ptr,
                std::mem::size_of_val(&in_addr) as _,
            )
        } != 0
        {
            return Err(Error::Bind(IoError::last_os_error()));
        }
    }

    let mut addr_len = std::mem::size_of_val(&in_addr) as i32;

    // SAFETY: `getsockname` takes a `*mut SOCKADDR`; same layout-
    // compatibility rationale as `bind` above.
    {
        let addr_ptr = &mut in_addr as *mut SOCKADDR_IN as *mut winapi::shared::ws2def::SOCKADDR;
        if unsafe {
            getsockname(
                s.as_raw_handle() as _,
                addr_ptr,
                &mut addr_len,
            )
        } != 0
        {
            return Err(Error::Getsockname(IoError::last_os_error()));
        }
    }

    // SAFETY: `s` is a bound, listening-capable socket; `1` is a valid
    // backlog.
    unsafe {
        if listen(s.as_raw_handle() as _, 1) != 0 {
            return Err(Error::Listen(IoError::last_os_error()));
        }
    }

    let client = socket(AF_INET, SOCK_STREAM, 0)?;

    // SAFETY: `connect` takes a `*const SOCKADDR`; same layout-
    // compatibility rationale as `bind` above.
    {
        let addr_ptr = &in_addr as *const SOCKADDR_IN as *const winapi::shared::ws2def::SOCKADDR;
        if unsafe {
            connect(
                client.as_raw_handle() as _,
                addr_ptr,
                addr_len,
            )
        } != 0
        {
            return Err(Error::Connect(IoError::last_os_error()));
        }
    }

    // SAFETY: `s` is a listening socket; NULL out-pointers are valid for
    // accept when the caller address is not needed.
    let server = unsafe { accept(s.as_raw_handle() as _, ptr::null_mut(), ptr::null_mut()) };
    if server == INVALID_SOCKET {
        return Err(Error::Accept(IoError::last_os_error()));
    }
    let server = FileDescriptor {
        handle: OwnedHandle {
            handle: server as _,
            handle_type: HandleType::Socket,
        },
    };

    Ok((server, client))
}

#[doc(hidden)]
pub fn poll_impl(pfd: &mut [pollfd], duration: Option<Duration>) -> Result<usize> {
    // SAFETY: WSAPoll is the winsock polling function; `pfd.as_mut_ptr()` is
    // a valid array of `pfd.len()` pollfd entries.
    let poll_result = unsafe {
        WSAPoll(
            pfd.as_mut_ptr(),
            pfd.len() as _,
            duration
                .map(|wait| wait.as_millis() as libc::c_int)
                .unwrap_or(-1),
        )
    };
    if poll_result < 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(poll_result as usize)
    }
}

#[cfg(test)]
mod test {
    use std::io::{Read, Write};

    #[test]
    fn socketpair() {
        let (mut a, mut b) = super::socketpair_impl().unwrap();
        a.write_all(b"hello").unwrap();
        let mut buf = [0u8; 5];
        assert_eq!(b.read(&mut buf).unwrap(), 5);
        assert_eq!(&buf, b"hello");
    }
}
