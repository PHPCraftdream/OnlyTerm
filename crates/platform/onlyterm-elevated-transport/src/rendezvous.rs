use crate::bridge::spawn_bridge_to_local_stream;
use anyhow::Context;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::server::{
    Callback, ErrorResponse, Request as HandshakeRequest, Response as HandshakeResponse,
};
use tungstenite::protocol::WebSocket;

/// HTTP header carrying the rendezvous token during the WebSocket
/// handshake. Checked by the server (the GUI process) before completing
/// the handshake -- see `TokenCallback::on_request`.
pub(crate) const TOKEN_HEADER: &str = "X-OnlyTerm-Token";

/// Base58 alphabet (Bitcoin/IPFS convention): the 58 alphanumeric
/// characters with the visually-ambiguous `0`/`O`/`I`/`l` removed. Only
/// used here for its density (no non-alphanumeric characters to worry
/// about quoting when this token later travels as a `ShellExecuteExW`
/// command-line argument), not for any Bitcoin-specific reason.
pub(crate) const BASE58_ALPHABET: &[u8] =
    b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

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
    ) -> anyhow::Result<onlyterm_uds::UnixStream> {
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

pub(crate) struct TokenCallback {
    pub(crate) expected: String,
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
/// (`onlyterm-mux-server --single-pane --connect-ws ...`), never from the
/// GUI.
pub fn connect_and_bridge(port: u16, token: &str) -> anyhow::Result<onlyterm_uds::UnixStream> {
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
