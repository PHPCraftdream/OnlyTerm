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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum HandleType {
    Char,
    Disk,
    Pipe,
    Socket,
    #[default]
    Unknown,
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

impl Pipe {
    pub fn new() -> Result<Pipe> {
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 0,
        };
        let mut read: HANDLE = INVALID_HANDLE_VALUE as _;
        let mut write: HANDLE = INVALID_HANDLE_VALUE as _;
        // SAFETY: `&mut read`/`&mut write` are valid out-pointers for the pipe
        // handles and `&mut sa` is a fully-initialized `SECURITY_ATTRIBUTES`.
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

/// Size in bytes of the random handshake token exchanged over the loopback
/// socketpair below, used to authenticate that the peer `accept()`ed by the
/// listening socket really is the `client` socket we ourselves created (and
/// not some unrelated local process that raced in and connected first; see
/// the module-level rationale in `socketpair_impl`).
const HANDSHAKE_TOKEN_LEN: usize = 16;

/// Total wall-clock budget for the `accept()` + verify retry loop in
/// `socketpair_impl`. A legitimate same-machine loopback connect completes
/// in microseconds, so this is very generous headroom against scheduling
/// jitter while still bounding how long a hostile/noisy local process can
/// make us spin.
const HANDSHAKE_TOTAL_BUDGET: Duration = Duration::from_secs(2);

/// Per-attempt timeout waiting for the handshake token to arrive on a
/// freshly accepted connection. Our own `client` writes the token
/// immediately after `connect()` succeeds, so this only needs to cover
/// scheduler latency, not real network delay.
const HANDSHAKE_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(250);

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

    {
        let addr_ptr = &in_addr as *const SOCKADDR_IN as *const winapi::shared::ws2def::SOCKADDR;
        // SAFETY: `bind` takes a `*const SOCKADDR`; SOCKADDR_IN is layout-
        // compatible (it is a superset). We use an explicit cast instead of
        // transmute to avoid the unsafe pointer reinterpretation. `addr_ptr`
        // points at the live `in_addr` local for the duration of this call.
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

    {
        let addr_ptr = &mut in_addr as *mut SOCKADDR_IN as *mut winapi::shared::ws2def::SOCKADDR;
        // SAFETY: `getsockname` takes a `*mut SOCKADDR`; same layout-
        // compatibility rationale as `bind` above. `addr_ptr` and `&mut
        // addr_len` are valid out-pointers for the duration of this call.
        if unsafe { getsockname(s.as_raw_handle() as _, addr_ptr, &mut addr_len) } != 0 {
            return Err(Error::Getsockname(IoError::last_os_error()));
        }
    }

    // Use a backlog greater than 1: on Windows, loopback listening sockets
    // are visible to any other local process, not just to us. If a rival
    // process connects before our own `client` below, a backlog of exactly
    // 1 could let that rival occupy the only queue slot and force our own
    // connection attempt to fail or wait. A small backlog gives our
    // legitimate connection room to queue even if noise/rivals get there
    // first; the handshake-token verification below is what actually
    // decides which queued connection we trust as `server`.
    // SAFETY: `s` is a bound, listening-capable socket; `4` is a valid
    // backlog.
    unsafe {
        if listen(s.as_raw_handle() as _, 4) != 0 {
            return Err(Error::Listen(IoError::last_os_error()));
        }
    }

    // Generate a random handshake token. Only our own `client` (below) will
    // ever know this value, so requiring a peer to present it back to us
    // before we trust an `accept()`ed connection as `server` closes the
    // TOCTOU window between `listen()` and `accept()` during which some
    // other local process could otherwise race in and hijack the "server"
    // half of this supposedly-private pair (the same class of bug as
    // CPython bpo-24924).
    let mut token = [0u8; HANDSHAKE_TOKEN_LEN];
    // `getrandom::Error` doesn't implement `std::error::Error` (and thus
    // isn't accepted by `IoError::new`) unless this crate's own `std`
    // feature happens to be unified on via some other dependent in the
    // workspace, so format it explicitly instead of relying on that.
    getrandom::fill(&mut token).map_err(|e| Error::Io(IoError::other(e.to_string())))?;

    let client = socket(AF_INET, SOCK_STREAM, 0)?;

    {
        let addr_ptr = &in_addr as *const SOCKADDR_IN as *const winapi::shared::ws2def::SOCKADDR;
        // SAFETY: `connect` takes a `*const SOCKADDR`; same layout-
        // compatibility rationale as `bind` above. `addr_ptr` points at the
        // live `in_addr` local for the duration of this call.
        if unsafe { connect(client.as_raw_handle() as _, addr_ptr, addr_len) } != 0 {
            return Err(Error::Connect(IoError::last_os_error()));
        }
    }

    // The connection is now established, so this is a plain blocking send
    // on our own socket; it completes immediately over loopback and never
    // blocks waiting on a remote peer.
    // SAFETY: `client.as_raw_handle()` is the just-connected client socket;
    // `token.as_ptr()` is a valid buffer of `token.len()` bytes.
    let sent = unsafe {
        send(
            client.as_raw_handle() as _,
            token.as_ptr() as *const _,
            token.len() as _,
            0,
        )
    };
    if sent != token.len() as i32 {
        return Err(Error::Connect(IoError::last_os_error()));
    }

    let deadline = std::time::Instant::now() + HANDSHAKE_TOTAL_BUDGET;
    let server = loop {
        if std::time::Instant::now() >= deadline {
            return Err(Error::SocketpairHandshake);
        }

        // SAFETY: `s` is a listening socket; NULL out-pointers are valid for
        // accept when the caller address is not needed.
        let candidate = unsafe { accept(s.as_raw_handle() as _, ptr::null_mut(), ptr::null_mut()) };
        if candidate == INVALID_SOCKET {
            return Err(Error::Accept(IoError::last_os_error()));
        }
        let candidate = FileDescriptor {
            handle: OwnedHandle {
                handle: candidate as _,
                handle_type: HandleType::Socket,
            },
        };

        if verify_handshake_token(&candidate, &token) {
            break candidate;
        }
        // Not our client: could be a rival local process that raced onto
        // the loopback port, or a legitimate connection that simply didn't
        // present a valid token in time. Either way, drop it and keep
        // waiting for our real client within the overall deadline.
    };

    Ok((server, client))
}

/// Reads exactly `token.len()` bytes from `candidate` within a short
/// per-attempt timeout and compares them against `token`. Returns `true`
/// only on an exact match; any timeout, short read, or mismatch is treated
/// as "not our peer" so the caller can discard the connection and keep
/// waiting for the real one.
fn verify_handshake_token(candidate: &FileDescriptor, token: &[u8; HANDSHAKE_TOKEN_LEN]) -> bool {
    let mut pfd = [pollfd {
        fd: candidate.as_raw_handle() as _,
        events: POLLIN,
        revents: 0,
    }];

    let mut received = [0u8; HANDSHAKE_TOKEN_LEN];
    let mut filled = 0usize;
    let attempt_deadline = std::time::Instant::now() + HANDSHAKE_ATTEMPT_TIMEOUT;

    while filled < received.len() {
        let remaining = match attempt_deadline.checked_duration_since(std::time::Instant::now()) {
            Some(d) if !d.is_zero() => d,
            _ => return false,
        };

        pfd[0].revents = 0;
        match poll_impl(&mut pfd, Some(remaining)) {
            Ok(0) | Err(_) => return false,
            Ok(_) => {}
        }
        if pfd[0].revents & (POLLERR | POLLHUP) != 0 {
            return false;
        }

        // SAFETY: `candidate.as_raw_handle()` is a valid, currently-open
        // socket; `received[filled..].as_mut_ptr()` points at valid,
        // writable memory for the remaining, un-filled portion of the
        // buffer.
        let n = unsafe {
            recv(
                candidate.as_raw_handle() as _,
                received[filled..].as_mut_ptr() as *mut _,
                (received.len() - filled) as _,
                0,
            )
        };
        if n <= 0 {
            // 0 means the peer disconnected; negative is a socket error.
            return false;
        }
        filled += n as usize;
    }

    received == *token
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
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn socketpair() {
        let (mut a, mut b) = super::socketpair_impl().unwrap();
        a.write_all(b"hello").unwrap();
        let mut buf = [0u8; 5];
        assert_eq!(b.read(&mut buf).unwrap(), 5);
        assert_eq!(&buf, b"hello");
    }

    // `socketpair_impl` itself is hard to exercise adversarially in a
    // deterministic unit test: doing so would require actually winning a
    // race against the function's own internal `connect()` from a second
    // process/thread, which is inherently timing-dependent and would make
    // the test flaky (it might pass "for the wrong reason" if our rival
    // loses the race, or need artificial delays hard-coded into
    // production code just to make it testable). Instead, these tests
    // exercise `verify_handshake_token` directly: it's the exact function
    // `socketpair_impl` relies on to decide whether an `accept()`ed
    // connection is trustworthy, so proving it correctly rejects
    // mismatched/silent/disconnecting peers and accepts only an exact
    // token match is a faithful, deterministic test of the real security
    // property, without needing to fake a live port race.

    /// Sets up a real loopback TCP pair (mirroring the shape of the
    /// production listener/connector) and returns the accepted side as a
    /// `FileDescriptor` plus the connecting `TcpStream` the test can use to
    /// play the role of either a legitimate client or a rival process.
    fn accepted_pair() -> (FileDescriptor, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let connector = TcpStream::connect(addr).unwrap();
        let (accepted, _) = listener.accept().unwrap();

        // SAFETY: `accepted` is a valid, just-accepted, owned socket
        // obtained from `std::net::TcpListener::accept`; ownership is
        // transferred into the `FileDescriptor` via `into_raw_socket`,
        // which the std type guarantees returns a live, uniquely-owned
        // socket handle.
        let accepted = unsafe {
            FileDescriptor::from_socket_descriptor(accepted.into_raw_socket() as SocketDescriptor)
        };

        (accepted, connector)
    }

    #[test]
    fn verify_handshake_token_accepts_exact_match() {
        let token = [0x42u8; HANDSHAKE_TOKEN_LEN];
        let (accepted, mut connector) = accepted_pair();
        connector.write_all(&token).unwrap();
        assert!(verify_handshake_token(&accepted, &token));
    }

    #[test]
    fn verify_handshake_token_rejects_wrong_bytes() {
        // Simulates a rival local process that connected to our listening
        // port but doesn't know our real token: it sends *some* bytes, but
        // not the ones we expect.
        let token = [0x42u8; HANDSHAKE_TOKEN_LEN];
        let wrong = [0x99u8; HANDSHAKE_TOKEN_LEN];
        let (accepted, mut connector) = accepted_pair();
        connector.write_all(&wrong).unwrap();
        assert!(!verify_handshake_token(&accepted, &token));
    }

    #[test]
    fn verify_handshake_token_rejects_silence_within_timeout() {
        // Simulates a rival that connects but never sends anything: the
        // read must time out and be treated as a rejection, not hang
        // forever or be trusted by default.
        let token = [0x42u8; HANDSHAKE_TOKEN_LEN];
        let (accepted, connector) = accepted_pair();
        let started = std::time::Instant::now();
        assert!(!verify_handshake_token(&accepted, &token));
        // Must not have blocked past the bounded per-attempt timeout
        // (with slack for scheduler jitter on a loaded CI box).
        assert!(started.elapsed() < HANDSHAKE_ATTEMPT_TIMEOUT * 4);
        drop(connector);
    }

    #[test]
    fn verify_handshake_token_rejects_early_disconnect() {
        // Simulates a rival that connects and then immediately hangs up
        // without sending the full token.
        let token = [0x42u8; HANDSHAKE_TOKEN_LEN];
        let (accepted, connector) = accepted_pair();
        drop(connector);
        assert!(!verify_handshake_token(&accepted, &token));
    }
}
