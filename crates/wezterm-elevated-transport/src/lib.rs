//! WebSocket-based rendezvous transport for elevated single-pane tabs.
//!
//! `ShellExecuteExW("runas")` cannot pass an inheritable handle to the
//! elevated child the way ordinary `CreateProcess`-based spawning can --
//! there is no `STARTUPINFO`/handle-inheritance equivalent anywhere in
//! `SHELLEXECUTEINFOW`, and the actual elevated `CreateProcess` call is
//! made by the Application Information service after the UAC consent UI,
//! not by the calling process, so there is no handle table to project into
//! the child in the first place. So the `proxy_command`/anonymous-
//! socketpair transport the non-elevated single-pane path uses
//! (`wezterm-gui`'s `spawn_single_pane_tab`) cannot be reused for an
//! elevated child.
//!
//! This crate implements the alternative: the GUI (non-elevated) opens a
//! loopback TCP listener and generates a random token
//! (`generate_rendezvous_token`); the elevated child is launched with that
//! port and token as CLI arguments and connects *out* to the GUI
//! (`connect_and_bridge`), authenticating with the token during the
//! WebSocket handshake (`RendezvousListener::accept` on the server side).
//! Once connected, the WebSocket carries the same mux PDU byte stream the
//! proxy_command transport carries, bridged via a background pump thread
//! onto a local `filedescriptor::socketpair()` end that the existing
//! `wezterm-client`/`wezterm-mux-server-impl` machinery consumes
//! unchanged on both sides (`Reconnectable` already accepts a
//! pre-connected stream on the client side; `dispatch::process` already
//! accepts any `AsRawDesc` stream on the server side).
//!
//! Both sides of this crate exist together deliberately: it is meant to be
//! the *only* place that understands the WebSocket framing/bridging, so
//! `wezterm-gui` (the rendezvous server) and `wezterm-mux-server` (the
//! rendezvous client) each depend on it without depending on each other --
//! `wezterm-gui` is a binary crate and cannot be a library dependency of
//! anything else.

use anyhow::Context;
use filedescriptor::{poll, pollfd, AsRawSocketDescriptor, POLLIN, POLLOUT};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::server::{
    Callback, ErrorResponse, Request as HandshakeRequest, Response as HandshakeResponse,
};
use tungstenite::protocol::WebSocket;
use tungstenite::Message;

/// HTTP header carrying the rendezvous token during the WebSocket
/// handshake. Checked by the server (the GUI process) before completing
/// the handshake -- see `TokenCallback::on_request`.
const TOKEN_HEADER: &str = "X-OnlyTerm-Token";

/// Base58 alphabet (Bitcoin/IPFS convention): the 58 alphanumeric
/// characters with the visually-ambiguous `0`/`O`/`I`/`l` removed. Only
/// used here for its density (no non-alphanumeric characters to worry
/// about quoting when this token later travels as a `ShellExecuteExW`
/// command-line argument), not for any Bitcoin-specific reason.
const BASE58_ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Generates a cryptographically random 64-character Base58 token: used to
/// authenticate the WebSocket rendezvous connection between the
/// (non-elevated) GUI process and an elevated single-pane child it spawns.
/// 64 Base58 characters is ~375 bits of entropy (64 * log2(58)), far more
/// than needed for a random label used exactly once.
///
/// Deliberately does NOT base58-encode a fixed-size random byte buffer
/// (e.g. via a `bs58`-style encoder): that approach's output length varies
/// with the buffer's leading-zero bytes, and getting a guaranteed-64-char
/// token out of it means padding/truncation logic that's easy to get
/// subtly wrong. Instead this draws random alphabet indices directly, via
/// rejection sampling (`byte % 58` alone would be biased toward the lower
/// symbols, since 256 isn't a multiple of 58 -- rejecting any byte >= 232,
/// the largest multiple of 58 that's <= 256, removes that bias).
pub fn generate_rendezvous_token() -> anyhow::Result<String> {
    const TOKEN_LEN: usize = 64;
    // Largest multiple of 58 not exceeding 256: 58 * 4 = 232.
    const REJECT_AT_OR_ABOVE: u8 = 232;

    let mut token = String::with_capacity(TOKEN_LEN);
    let mut buf = [0u8; 1];
    while token.len() < TOKEN_LEN {
        // `getrandom::Error` doesn't implement `std::error::Error` in this
        // version, same workaround already used in
        // `filedescriptor::windows::socketpair`.
        getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("getrandom::fill failed: {e}"))?;
        let byte = buf[0];
        if byte >= REJECT_AT_OR_ABOVE {
            continue;
        }
        token.push(BASE58_ALPHABET[(byte % 58) as usize] as char);
    }
    Ok(token)
}

/// A loopback TCP listener plus a freshly generated token, ready to be
/// handed (as `127.0.0.1:<port>` and the token string) to an elevated
/// single-pane child process as CLI arguments.
pub struct RendezvousListener {
    listener: TcpListener,
    port: u16,
    token: String,
}

impl RendezvousListener {
    /// Binds an ephemeral loopback port and generates a fresh token. Must
    /// be called *before* spawning the elevated child, so the endpoint
    /// exists and is claimed the moment the child is told about it -- no
    /// window where the port is known but unclaimed.
    pub fn bind() -> anyhow::Result<Self> {
        let listener =
            TcpListener::bind("127.0.0.1:0").context("binding loopback rendezvous listener")?;
        let port = listener
            .local_addr()
            .context("reading rendezvous listener's local address")?
            .port();
        let token = generate_rendezvous_token().context("generating rendezvous token")?;
        Ok(Self {
            listener,
            port,
            token,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Blocks (on the calling thread -- callers must be on a dedicated
    /// background thread, never the GUI thread) until either: a client
    /// connects and completes the WebSocket handshake with the correct
    /// token (returns the connected, bridged local stream), `deadline`
    /// elapses, or `child_exited` starts reporting `true` (so a crashed
    /// child doesn't make this wait out the full deadline for a
    /// connection that will never come).
    ///
    /// Connection attempts with a missing/incorrect token are logged and
    /// rejected (403), and this keeps waiting for the *real* child rather
    /// than treating a bad attempt as fatal -- some other local process
    /// could in principle guess the port before the real child connects
    /// (the token, not the port, is what actually authenticates).
    pub fn accept(
        &self,
        deadline: Instant,
        mut child_exited: impl FnMut() -> bool,
    ) -> anyhow::Result<wezterm_uds::UnixStream> {
        self.listener
            .set_nonblocking(true)
            .context("setting rendezvous listener non-blocking")?;

        loop {
            if Instant::now() >= deadline {
                anyhow::bail!("timed out waiting for the elevated process to connect");
            }
            if child_exited() {
                anyhow::bail!("the elevated process exited before connecting");
            }

            match self.listener.accept() {
                Ok((stream, _addr)) => match self.complete_handshake(stream) {
                    Ok(ws) => return spawn_bridge_to_local_stream(ws),
                    Err(err) => {
                        log::warn!(
                            "elevated tab rendezvous: rejected a connection attempt: {:#}",
                            err
                        );
                        continue;
                    }
                },
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) => return Err(e).context("accepting rendezvous connection"),
            }
        }
    }

    fn complete_handshake(&self, stream: TcpStream) -> anyhow::Result<WebSocket<TcpStream>> {
        // The handshake itself is done with blocking I/O (simpler, and
        // this whole function already runs on a dedicated background
        // thread); only the outer accept-loop above needs non-blocking
        // polling.
        stream
            .set_nonblocking(false)
            .context("setting accepted rendezvous stream blocking for handshake")?;
        // A blocking read with no timeout can hang forever if the peer
        // never sends the rest of the handshake (a stalled/malicious
        // connection, or simply a bug on either side) -- bound it, so a
        // broken handshake fails fast with a clear error instead of
        // wedging this accept loop (and the caller waiting on it) forever.
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .context("setting rendezvous handshake read timeout")?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .context("setting rendezvous handshake write timeout")?;
        tungstenite::accept_hdr(
            stream,
            TokenCallback {
                expected: self.token.clone(),
            },
        )
        .map_err(|e| anyhow::anyhow!("WebSocket handshake failed: {e}"))
    }
}

struct TokenCallback {
    expected: String,
}

impl Callback for TokenCallback {
    fn on_request(
        self,
        request: &HandshakeRequest,
        response: HandshakeResponse,
    ) -> Result<HandshakeResponse, ErrorResponse> {
        let got = request
            .headers()
            .get(TOKEN_HEADER)
            .and_then(|v| v.to_str().ok());
        if got == Some(self.expected.as_str()) {
            Ok(response)
        } else {
            log::warn!("elevated tab rendezvous: connection attempt with missing/wrong token");
            let rejection = http::Response::builder()
                .status(http::StatusCode::FORBIDDEN)
                .body(None)
                .expect("building a static 403 response cannot fail");
            Err(rejection)
        }
    }
}

/// Client side: connects out to `127.0.0.1:<port>` and completes the
/// WebSocket handshake, presenting `token` via the same header the server
/// checks. Called from the elevated single-pane child process
/// (`wezterm-mux-server --single-pane --connect-ws ...`), never from the
/// GUI.
pub fn connect_and_bridge(port: u16, token: &str) -> anyhow::Result<wezterm_uds::UnixStream> {
    let stream = TcpStream::connect(("127.0.0.1", port))
        .with_context(|| format!("connecting to rendezvous server on port {port}"))?;
    // See the matching timeouts in `RendezvousListener::complete_handshake`:
    // bounds the blocking handshake read/write so a stalled/broken
    // handshake fails fast instead of hanging this call forever.
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .context("setting rendezvous client handshake read timeout")?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .context("setting rendezvous client handshake write timeout")?;
    let mut request = format!("ws://127.0.0.1:{port}/onlyterm-elev")
        .into_client_request()
        .context("building WebSocket client request")?;
    request.headers_mut().insert(
        TOKEN_HEADER,
        token
            .parse()
            .context("rendezvous token is not a valid header value")?,
    );
    let (ws, _response) = tungstenite::client::client(request, stream)
        .map_err(|e| anyhow::anyhow!("WebSocket client handshake failed: {e}"))?;
    spawn_bridge_to_local_stream(ws)
}

/// Bridges an established `WebSocket<TcpStream>` onto one end of a fresh
/// `filedescriptor::socketpair()`, via a background pump thread, and
/// returns the *other* end as a plain byte stream -- from that point on,
/// the caller (either the GUI's `ClientDomain`/`Client` machinery, or the
/// elevated child's `dispatch::process`) only ever sees ordinary stream
/// I/O, unaware that a WebSocket sits in between. Needed because
/// `async-io`'s `Async<T>` (which both those consumers are built on, on
/// Windows) only supports `AsSocket` types -- a message-oriented
/// `WebSocket<TcpStream>` isn't one, so this bridge is what makes the rest
/// of the mux client/server code reusable unchanged.
fn spawn_bridge_to_local_stream(
    mut ws: WebSocket<TcpStream>,
) -> anyhow::Result<wezterm_uds::UnixStream> {
    let (local_end, bridge_end) =
        filedescriptor::socketpair().context("creating local bridge socketpair")?;

    // SAFETY: `local_end` was just created by `filedescriptor::
    // socketpair()` immediately above and is uniquely owned at this point;
    // `into_raw_socket()` transfers that ownership into the `UnixStream`
    // constructed here, which becomes its sole owner.
    let local_stream = unsafe {
        use std::os::windows::io::{FromRawSocket, IntoRawSocket};
        wezterm_uds::UnixStream::from_raw_socket(local_end.into_raw_socket())
    };

    ws.get_ref()
        .set_nonblocking(true)
        .context("setting rendezvous WebSocket stream non-blocking for the bridge pump")?;
    let mut bridge_end = bridge_end;
    bridge_end
        .set_non_blocking(true)
        .context("setting bridge socketpair end non-blocking")?;

    std::thread::Builder::new()
        .name("onlyterm-elev-rendezvous-bridge".to_string())
        .spawn(move || {
            pump_bridge(&mut ws, &mut bridge_end);
        })
        .context("spawning rendezvous bridge pump thread")?;

    Ok(local_stream)
}

/// Upper bound on how long the pump parks in `poll()` before looping
/// around anyway. Correctness does not depend on it: every iteration
/// drains both directions all the way to `WouldBlock` before waiting, so
/// `poll()` reporting readability is what actually drives data movement.
/// It exists purely as a liveness backstop.
const PUMP_POLL_TIMEOUT: Duration = Duration::from_millis(250);

/// The actual pump loop: drains everything currently available from the
/// WebSocket (forwarding binary payloads to `bridge_end`) and from
/// `bridge_end` (forwarding as WebSocket binary messages), then parks in
/// `poll()` until either side is readable again. Returns (ending the
/// thread) once either side closes or errors -- dropping `bridge_end`
/// then surfaces as EOF to whatever's reading the other end of the
/// socketpair, which is exactly what a real connection close should look
/// like to the mux protocol layer.
fn pump_bridge(ws: &mut WebSocket<TcpStream>, bridge_end: &mut filedescriptor::FileDescriptor) {
    let mut buf = [0u8; 32 * 1024];
    // True while tungstenite still holds frames it could not hand to the
    // socket because the peer isn't draining fast enough. While that's the
    // case we stop pulling more from the local side, so the congestion
    // propagates backwards as a full socketpair buffer (which is what the
    // local writer is prepared for) rather than as unbounded growth of
    // tungstenite's in-memory out-buffer.
    let mut ws_write_pending = false;

    loop {
        // WebSocket -> local. Drained all the way to `WouldBlock` rather
        // than one message per iteration: a single `ws.read()` can leave
        // further *complete* messages sitting in tungstenite's internal
        // read buffer, and those do not make the underlying socket look
        // readable to `poll()` below -- so stopping after one message
        // could park this thread with undelivered data already in hand.
        loop {
            match ws.read() {
                Ok(Message::Binary(data)) => {
                    if let Err(err) = write_all_to_local(bridge_end, &data) {
                        log::debug!("elevated tab rendezvous bridge: local write failed: {err:#}");
                        return;
                    }
                }
                // Text/Ping/Pong/Frame: not part of this protocol (only
                // ever binary messages are sent by either side of this
                // bridge); tungstenite already auto-answers Ping/Close for
                // us. Ignore and keep pumping.
                Ok(_) => {}
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    break
                }
                Err(tungstenite::Error::ConnectionClosed)
                | Err(tungstenite::Error::AlreadyClosed) => {
                    log::debug!("elevated tab rendezvous bridge: WebSocket closed");
                    return;
                }
                Err(err) => {
                    log::debug!("elevated tab rendezvous bridge: WebSocket read failed: {err:#}");
                    return;
                }
            }
        }

        // local -> WebSocket, likewise drained to `WouldBlock`.
        while !ws_write_pending {
            match bridge_end.read(&mut buf) {
                Ok(0) => {
                    log::debug!("elevated tab rendezvous bridge: local side closed");
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return;
                }
                Ok(n) => match ws.write(Message::Binary(buf[..n].to_vec().into())) {
                    Ok(()) => {}
                    // Not a failure and not data loss: the frame is already
                    // formatted into tungstenite's out-buffer, and only the
                    // opportunistic push of that buffer to the socket hit a
                    // full send buffer. Stop pulling more from the local
                    // side and let the flush below retry it.
                    Err(tungstenite::Error::Io(ref e))
                        if e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        ws_write_pending = true;
                    }
                    Err(err) => {
                        log::debug!(
                            "elevated tab rendezvous bridge: WebSocket write failed: {err:#}"
                        );
                        return;
                    }
                },
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) => {
                    log::debug!("elevated tab rendezvous bridge: local read failed: {err:#}");
                    return;
                }
            }
        }

        ws_write_pending = match ws.flush() {
            Ok(()) => false,
            // Same as above: whatever couldn't be pushed stays buffered and
            // is retried on the next iteration, once `poll()` says the
            // socket is writable again. Treating this as fatal (as an
            // earlier version did) tore the connection down -- and silently
            // truncated the stream -- the first time a peer fell behind.
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                true
            }
            Err(err) => {
                log::debug!("elevated tab rendezvous bridge: WebSocket flush failed: {err:#}");
                return;
            }
        };

        let mut pfd = [
            pollfd {
                fd: ws.get_ref().as_socket_descriptor(),
                events: POLLIN | if ws_write_pending { POLLOUT } else { 0 },
                revents: 0,
            },
            pollfd {
                fd: bridge_end.as_socket_descriptor(),
                events: POLLIN,
                revents: 0,
            },
        ];
        // Always poll both file descriptors: even when output is backed up,
        // we must still detect local-side closure (EOF on bridge_end), which
        // happens when the mux server or client crashes. Previously we only
        // watched the WebSocket when ws_write_pending was true, which meant
        // the pump could hang indefinitely if the local side closed while
        // the WebSocket send buffer was full.
        if let Err(err) = poll(&mut pfd, Some(PUMP_POLL_TIMEOUT)) {
            log::debug!("elevated tab rendezvous bridge: poll failed: {err:#}");
            return;
        }
    }
}

/// `std::io::Write::write_all` retries only on `Interrupted`. On the
/// non-blocking socketpair end this bridge writes to, a full send buffer
/// surfaces as `WouldBlock`, which `write_all` reports as a hard error
/// *after* having already written part of the buffer -- silently
/// truncating the byte stream the mux protocol is carrying. So retry
/// `WouldBlock` here, waiting for writability rather than spinning.
fn write_all_to_local(
    dest: &mut filedescriptor::FileDescriptor,
    mut data: &[u8],
) -> std::io::Result<()> {
    while !data.is_empty() {
        match dest.write(data) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "bridge socketpair accepted 0 bytes",
                ))
            }
            Ok(n) => data = &data[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let mut pfd = [pollfd {
                    fd: dest.as_socket_descriptor(),
                    events: POLLOUT,
                    revents: 0,
                }];
                poll(&mut pfd, Some(PUMP_POLL_TIMEOUT))
                    .map_err(|err| std::io::Error::other(format!("{err:#}")))?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::{FromRawSocket, IntoRawSocket};

    /// Like `spawn_bridge_to_local_stream`, but test-only and returns the
    /// pump thread handle. Used by peer-death tests to verify the pump exits cleanly.
    fn spawn_bridge_for_test(
        mut ws: WebSocket<TcpStream>,
    ) -> (wezterm_uds::UnixStream, std::thread::JoinHandle<()>) {
        let (local_end, bridge_end) =
            filedescriptor::socketpair().expect("creating local bridge socketpair should succeed");

        // SAFETY: `local_end` was just created by `filedescriptor::
        // socketpair()` immediately above and is uniquely owned at this point;
        // `into_raw_socket()` transfers that ownership into the `UnixStream`
        // constructed here, which becomes its sole owner.
        let local_stream =
            unsafe { wezterm_uds::UnixStream::from_raw_socket(local_end.into_raw_socket()) };

        ws.get_ref().set_nonblocking(true).expect(
            "setting rendezvous WebSocket stream non-blocking for the bridge pump should succeed",
        );
        let mut bridge_end = bridge_end;
        bridge_end
            .set_non_blocking(true)
            .expect("setting bridge socketpair end non-blocking should succeed");

        let handle = std::thread::Builder::new()
            .name("onlyterm-elev-rendezvous-bridge".to_string())
            .spawn(move || {
                pump_bridge(&mut ws, &mut bridge_end);
            })
            .expect("spawning rendezvous bridge pump thread should succeed");

        (local_stream, handle)
    }

    /// Test helper that creates a connected WebSocket pair and returns
    /// both local streams plus both pump thread handles and TcpStream handles.
    /// Unlike the normal `connected_bridge_pair()`, this exposes the pump thread
    /// handles and TcpStream handles so tests can verify pumps exit cleanly on peer death.
    fn connected_bridge_pair_for_test() -> (
        wezterm_uds::UnixStream,
        wezterm_uds::UnixStream,
        TcpStream,
        TcpStream,
        std::thread::JoinHandle<()>,
        std::thread::JoinHandle<()>,
    ) {
        // Create a listener and bind to an ephemeral port.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind should succeed");
        let port = listener
            .local_addr()
            .expect("getting local addr should succeed")
            .port();
        let token = generate_rendezvous_token().expect("token generation should succeed");

        // Server side: accept and set up the WebSocket bridge.
        let token_for_server = token.clone();
        let server_thread = std::thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("setting listener non-blocking should succeed");

            let deadline = Instant::now() + Duration::from_secs(5);
            let tcp_stream = loop {
                if Instant::now() >= deadline {
                    panic!("server accept timed out");
                }

                match listener.accept() {
                    Ok((stream, _addr)) => {
                        break stream;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("server accept failed: {e}"),
                }
            };
            tcp_stream
                .set_nonblocking(false)
                .expect("setting stream blocking should succeed");
            tcp_stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("setting read timeout should succeed");
            tcp_stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .expect("setting write timeout should succeed");

            let ws = tungstenite::accept_hdr(
                tcp_stream
                    .try_clone()
                    .expect("cloning stream should succeed"),
                TokenCallback {
                    expected: token_for_server,
                },
            )
            .expect("WebSocket handshake should succeed");

            // Clone the TcpStream so we can return it for shutdown later.
            // The WebSocket will own one copy, we'll own another.
            let tcp_for_shutdown = tcp_stream
                .try_clone()
                .expect("cloning stream should succeed");

            let (local_stream, handle) = spawn_bridge_for_test(ws);

            (local_stream, tcp_for_shutdown, handle)
        });

        // Client side: connect and set up the WebSocket bridge.
        let client_thread = std::thread::spawn(move || {
            let stream =
                TcpStream::connect(("127.0.0.1", port)).expect("client connect should succeed");
            let tcp_for_shutdown = stream.try_clone().expect("cloning stream should succeed");

            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("setting read timeout should succeed");
            stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .expect("setting write timeout should succeed");

            let mut request = format!("ws://127.0.0.1:{port}/onlyterm-elev")
                .into_client_request()
                .expect("building request should succeed");
            request.headers_mut().insert(
                TOKEN_HEADER,
                token.parse().expect("parsing token should succeed"),
            );

            let (ws, _response) = tungstenite::client::client(request, stream)
                .expect("client handshake should succeed");

            let (local_stream, handle) = spawn_bridge_for_test(ws);

            (local_stream, tcp_for_shutdown, handle)
        });

        // Wait for both sides to complete setup.
        let (server_stream, server_tcp, server_handle) = server_thread
            .join()
            .expect("server setup thread should not panic");
        let (client_stream, client_tcp, client_handle) = client_thread
            .join()
            .expect("client setup thread should not panic");

        (
            server_stream,
            client_stream,
            server_tcp,
            client_tcp,
            server_handle,
            client_handle,
        )
    }

    #[test]
    fn test_generate_rendezvous_token_length_and_alphabet() {
        let token = generate_rendezvous_token().expect("token generation should not fail");
        assert_eq!(token.len(), 64, "token must be exactly 64 characters");
        for c in token.chars() {
            assert!(
                BASE58_ALPHABET.contains(&(c as u8)),
                "token character {:?} is outside the Base58 alphabet",
                c
            );
        }
    }

    #[test]
    fn test_generate_rendezvous_token_is_random() {
        // Not a rigorous randomness test -- just a sanity check that this
        // isn't accidentally returning a constant/degenerate value.
        let a = generate_rendezvous_token().unwrap();
        let b = generate_rendezvous_token().unwrap();
        assert_ne!(a, b, "two calls produced the same token");
    }

    /// End-to-end regression test: bind a listener, connect a real client
    /// with the correct token, confirm bytes written on one side of the
    /// bridge arrive on the other in both directions. This is the actual
    /// WebSocket handshake + bridge pump running against localhost, not a
    /// mock -- it doesn't touch elevation/ShellExecuteExW at all, which is
    /// exactly the part of this transport that CAN be exercised in a
    /// normal test (the UAC-crossing part cannot, see docs referenced in
    /// this crate's own doc comment).
    #[test]
    fn test_accept_and_connect_round_trip() {
        const FROM_CLIENT: &[u8] = b"hello from client";
        const FROM_SERVER: &[u8] = b"hello from server";

        let (mut server_stream, mut client_stream) = connected_bridge_pair();

        client_stream
            .write_all(FROM_CLIENT)
            .expect("client write should succeed");
        let mut buf = [0u8; 32];
        read_exactly(&mut server_stream, &mut buf[..FROM_CLIENT.len()]);
        assert_eq!(&buf[..FROM_CLIENT.len()], FROM_CLIENT);

        server_stream
            .write_all(FROM_SERVER)
            .expect("server write should succeed");
        let mut buf2 = [0u8; 32];
        read_exactly(&mut client_stream, &mut buf2[..FROM_SERVER.len()]);
        assert_eq!(&buf2[..FROM_SERVER.len()], FROM_SERVER);
    }

    /// Regression test for the bridge's congestion handling. A payload far
    /// larger than any socket buffer guarantees that both the pump's write
    /// to its local socketpair end and tungstenite's flush of its
    /// out-buffer hit `WouldBlock` part way through. Neither is a failure
    /// -- but treating them as one (or handing them to `write_all`, which
    /// gives up on `WouldBlock` after a partial write) silently truncates
    /// the byte stream, which for the mux protocol riding on top of this
    /// means an unparseable PDU rather than a clean error.
    #[test]
    fn test_bridge_survives_a_payload_larger_than_the_socket_buffers() {
        // 4 MiB: two orders of magnitude past the default loopback socket
        // buffer, so congestion is certain rather than timing-dependent.
        const LEN: usize = 4 * 1024 * 1024;

        let (server_stream, client_stream) = connected_bridge_pair();

        // Two different patterns, so a direction that echoed the wrong
        // buffer back would be caught rather than silently matching.
        let to_server: Vec<u8> = (0..LEN).map(|i| (i % 251) as u8).collect();
        let to_client: Vec<u8> = (0..LEN).map(|i| (i % 241) as u8).collect();

        let (client_stream, server_stream) =
            assert_transfers(client_stream, server_stream, &to_server);
        let (_server_stream, _client_stream) =
            assert_transfers(server_stream, client_stream, &to_client);
    }

    /// Binds a listener, connects a real client to it with the correct
    /// token, and returns the two bridged local streams as
    /// `(server_side, client_side)`.
    fn connected_bridge_pair() -> (wezterm_uds::UnixStream, wezterm_uds::UnixStream) {
        let listener = RendezvousListener::bind().expect("bind should succeed");
        let port = listener.port();
        let token = listener.token().to_string();

        let client_thread = std::thread::spawn(move || {
            connect_and_bridge(port, &token).expect("client connect should succeed")
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        let server_stream = listener
            .accept(deadline, || false)
            .expect("accept should succeed with the correct token");
        let client_stream = client_thread
            .join()
            .expect("client thread should not panic");
        (server_stream, client_stream)
    }

    /// Writes `payload` into `from` while a second thread reads it back
    /// out of `to`, and asserts it arrives byte for byte. The reader has
    /// to be on its own thread: a payload bigger than the socket buffers
    /// cannot be fully written before anyone reads it, so doing both from
    /// one thread would deadlock the test itself rather than test the
    /// bridge. Both streams are handed back so callers can reuse them.
    fn assert_transfers(
        mut from: wezterm_uds::UnixStream,
        mut to: wezterm_uds::UnixStream,
        payload: &[u8],
    ) -> (wezterm_uds::UnixStream, wezterm_uds::UnixStream) {
        let len = payload.len();
        let reader = std::thread::spawn(move || {
            let mut got = vec![0u8; len];
            read_exactly(&mut to, &mut got);
            (to, got)
        });
        from.write_all(payload).expect("write should succeed");
        let (to, got) = reader.join().expect("reader thread should not panic");

        let mismatch = got.iter().zip(payload).position(|(a, b)| a != b);
        assert!(
            mismatch.is_none(),
            "payload differs starting at byte {:?} of {}",
            mismatch,
            len
        );
        (from, to)
    }

    /// Fills `buf` completely from `stream`. The bridge pump thread is
    /// asynchronous relative to the reader, so a single `read()` can
    /// legitimately return fewer bytes than were written (it only has to
    /// return "at least one byte") -- hence the loop.
    ///
    /// The `SO_RCVTIMEO` is what makes a stalled bridge *observable*: this
    /// is a blocking socket, so without it a `read()` waiting for bytes
    /// that will never arrive parks forever and no amount of deadline
    /// checking around the call ever runs again. That is exactly how the
    /// original version of this test -- which asked for 18 bytes of a
    /// 17-byte message -- turned an off-by-one into a silent, permanent
    /// hang with no test output at all.
    fn read_exactly(stream: &mut wezterm_uds::UnixStream, buf: &mut [u8]) {
        let want = buf.len();
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .expect("setting a read timeout should succeed");
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut got = 0;
        while got < want {
            if Instant::now() >= deadline {
                panic!("timed out waiting for {want} bytes, only got {got}");
            }
            match stream.read(&mut buf[got..]) {
                Ok(0) => panic!("stream closed after {got} of {want} bytes"),
                Ok(n) => got += n,
                // `WouldBlock` for a non-blocking socket, `TimedOut` for
                // the `SO_RCVTIMEO` above -- both just mean "nothing yet".
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => panic!("read failed after {got} of {want} bytes: {e}"),
            }
        }
    }

    /// Waits for EOF on `stream` (i.e., `read()` returns `Ok(0)`), failing
    /// with a clear panic message if the deadline elapses first. Used by
    /// peer-death tests to verify that the pump thread exits cleanly when
    /// one side of the bridge dies.
    fn wait_for_eof(stream: &mut wezterm_uds::UnixStream, deadline: Instant) {
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("setting a read timeout should succeed");
        let mut buf = [0u8; 1];
        loop {
            if Instant::now() >= deadline {
                panic!("timed out waiting for EOF");
            }
            match stream.read(&mut buf) {
                Ok(0) => return, // EOF: pump thread exited
                Ok(_) => {
                    // Got a byte but we expect EOF. This means the pump is
                    // still running and forwarding data, which is wrong for
                    // a peer-death test. Consume and continue waiting.
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => panic!("read failed while waiting for EOF: {e}"),
            }
        }
    }

    /// Waits for a pump thread to exit, with a deadline. Returns `true` if
    /// the thread exited before the deadline, panics otherwise.
    fn wait_for_pump_exit(handle: std::thread::JoinHandle<()>, deadline: Instant) -> bool {
        loop {
            if Instant::now() >= deadline {
                panic!("timed out waiting for pump thread to exit");
            }
            if handle.is_finished() {
                // Join to propagate any panic from the thread.
                let _ = handle.join();
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Regression test for abrupt WebSocket-side death. Simulates a scenario
    /// where the underlying TCP connection dies (e.g., network error, process
    /// crash) and verifies that the pump thread exits cleanly without hanging
    /// or spinning, and that the local side observes EOF.
    #[test]
    fn test_pump_exits_on_websocket_side_death() {
        let (
            mut server_stream,
            _client_stream,
            _server_tcp,
            client_tcp,
            server_handle,
            client_handle,
        ) = connected_bridge_pair_for_test();

        // Give the pump threads a moment to start up and settle.
        std::thread::sleep(Duration::from_millis(100));

        // Force-close the client-side TCP connection to simulate
        // WebSocket-side death (as seen from the server).
        client_tcp
            .shutdown(std::net::Shutdown::Both)
            .expect("TCP shutdown should succeed");

        // The server-side local stream should observe EOF within a bounded time.
        let deadline = Instant::now() + Duration::from_secs(5);
        wait_for_eof(&mut server_stream, deadline);

        // The server pump thread should have exited.
        let deadline = Instant::now() + Duration::from_secs(1);
        wait_for_pump_exit(server_handle, deadline);

        // The client pump thread should also have exited (it detected
        // the shutdown on its own side).
        let deadline = Instant::now() + Duration::from_secs(1);
        wait_for_pump_exit(client_handle, deadline);
    }

    /// Regression test for abrupt local-side death. Verifies that the pump
    /// thread detects EOF on the local socketpair end and exits cleanly.
    #[test]
    fn test_pump_exits_on_local_side_death() {
        let (
            mut server_stream,
            _client_stream,
            _server_tcp,
            _client_tcp,
            server_handle,
            _client_handle,
        ) = connected_bridge_pair_for_test();

        // Give the pump threads a moment to start up.
        std::thread::sleep(Duration::from_millis(100));

        // Write to the local stream to ensure the pump thread wakes up.
        server_stream
            .write_all(b"test")
            .expect("write should succeed");

        // Drop the local stream to simulate local-side death.
        // This closes the socketpair, so the pump thread's bridge_end
        // will return EOF when it tries to read.
        drop(server_stream);

        // The server pump thread should detect EOF on bridge_end and exit.
        // With our fix to always poll both file descriptors, this should
        // happen even if the WebSocket send buffer is full.
        let deadline = Instant::now() + Duration::from_secs(5);
        wait_for_pump_exit(server_handle, deadline);
    }
}
