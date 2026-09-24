use crate::windows::owned_handle::HandleType;
use crate::windows::poll::poll_impl;
use crate::{pollfd, Error, FileDescriptor, OwnedHandle, Result, POLLERR, POLLHUP, POLLIN};
use std::io::Error as IoError;
use std::os::windows::prelude::*;
use std::ptr;
use std::sync::Once;
use std::time::Duration;
use winapi::shared::ws2def::{AF_INET, INADDR_LOOPBACK, SOCKADDR_IN};
use winapi::um::winsock2::{
    accept, bind, connect, getsockname, htonl, listen, recv, send, WSASocketW, WSAStartup,
    INVALID_SOCKET, SOCK_STREAM, WSADATA, WSA_FLAG_NO_HANDLE_INHERIT,
};

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

#[cfg(test)]
mod test {
    use super::*;
    use crate::{FromRawSocketDescriptor, SocketDescriptor};
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
