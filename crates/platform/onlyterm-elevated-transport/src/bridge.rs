use anyhow::Context;
use filedescriptor::{poll, pollfd, AsRawSocketDescriptor, POLLERR, POLLHUP, POLLIN, POLLOUT};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tungstenite::protocol::WebSocket;
use tungstenite::Message;

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
pub(crate) fn spawn_bridge_to_local_stream(
    mut ws: WebSocket<TcpStream>,
) -> anyhow::Result<onlyterm_uds::UnixStream> {
    let (local_end, bridge_end) =
        filedescriptor::socketpair().context("creating local bridge socketpair")?;

    // SAFETY: `local_end` was just created by `filedescriptor::
    // socketpair()` immediately above and is uniquely owned at this point;
    // `into_raw_socket()` transfers that ownership into the `UnixStream`
    // constructed here, which becomes its sole owner.
    let local_stream = unsafe {
        use std::os::windows::io::{FromRawSocket, IntoRawSocket};
        onlyterm_uds::UnixStream::from_raw_socket(local_end.into_raw_socket())
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

/// Upper bound on how long to drain WebSocket data after the local side
/// closes. This bounds the teardown time when a peer has vanished or is
/// wedged, preventing indefinite hangs while still giving well-behaved
/// peers a chance to deliver their final bytes.
///
/// CHOSEN VALUE: 2 seconds is long enough for a 16 MiB payload (the
/// regression test case) to drain through the WebSocket bridge at the
/// measured throughput of ~50-100 MiB/s, but short enough that a genuinely
/// wedged peer doesn't stall teardown indefinitely. 2 seconds at 50 MiB/s
/// allows ~100 MiB to drain - far more than the 16 MiB test payload.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Settles the WebSocket before closing it, once the local side has gone
/// away (`bridge_end` returned EOF).
///
/// # Which half actually fixes the tail loss
///
/// Worth being precise, because the obvious reading is the wrong one.
///
/// The defect is on the *sending* side: the application writes its last
/// bytes and drops its end of the socketpair. Those bytes reached
/// `ws.write()`, which only queues them in tungstenite's outgoing buffer --
/// and a single `flush()` on a non-blocking socket can return `WouldBlock`
/// with frames still pending. The old code closed and returned anyway, so
/// the queued tail died with the WebSocket; the receiver saw EOF in about
/// three runs out of four, around PDU 61 of 64.
///
/// So it is the **bounded flush loop** that carries the fix. In that
/// scenario nothing is arriving, so draining reads cannot be what helps --
/// an earlier version of this function claimed exactly that in its comments
/// and was wrong. Before deleting either loop as redundant, re-run
/// `tests/tail_loss_regression.rs` with this function's body reverted:
/// `test_websocket_bridge_no_tail_loss_on_sender_close` is the test that
/// fails against the bug (its `..._on_receiver_close` sibling passes either
/// way and proves nothing about this).
///
/// The read loop still earns its keep: it keeps consuming while we wait, so
/// a peer that is mid-send does not stall against its own full write buffer
/// while we are trying to flush into the same connection.
///
/// Both loops share one deadline. An unbounded settle would re-introduce the
/// hang this crate has already shipped once, merely moved to teardown.
///
/// Returns the number of bytes drained from the peer (for logging).
fn drain_websocket_before_close(
    ws: &mut WebSocket<TcpStream>,
    bridge_end: &mut filedescriptor::FileDescriptor,
) -> usize {
    let deadline = Instant::now() + DRAIN_TIMEOUT;
    let mut bytes_drained = 0;

    loop {
        // Check timeout first
        if Instant::now() >= deadline {
            if bytes_drained > 0 {
                log::warn!(
                    "elevated tab rendezvous bridge: drain timeout after {} bytes (may have lost data)",
                    bytes_drained
                );
            } else {
                log::debug!("elevated tab rendezvous bridge: drain timeout, no data to drain");
            }
            break;
        }

        // Try to read from WebSocket
        match ws.read() {
            Ok(Message::Binary(data)) => {
                bytes_drained += data.len();
                // Write to local side; if this fails, we can't deliver the data anyway
                // (the local side is already closed), so log and continue draining.
                if let Err(err) =
                    write_all_to_local(bridge_end, ws.get_ref().as_socket_descriptor(), &data)
                {
                    log::debug!(
                        "elevated tab rendezvous bridge: failed to write drained data: {err:#}"
                    );
                    // Keep trying to drain even if we can't deliver; we want to
                    // read everything we can from the WebSocket to avoid hanging
                    // the peer on their own write buffer.
                }
            }
            // Text/Ping/Pong/Frame: not part of this protocol; ignore and keep draining.
            Ok(_) => {}
            // No more data available
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Could poll here for more data, but we're in shutdown - give
                // the peer a brief window and then exit. This avoids waiting on
                // a peer that may never come back.
                std::thread::sleep(Duration::from_millis(10));
                // Continue looping to check deadline and try again
                continue;
            }
            // WebSocket is properly closed - we're done
            Err(tungstenite::Error::ConnectionClosed) | Err(tungstenite::Error::AlreadyClosed) => {
                if bytes_drained > 0 {
                    log::debug!(
                        "elevated tab rendezvous bridge: drained {} bytes before WebSocket close",
                        bytes_drained
                    );
                }
                break;
            }
            // Other errors - log and break
            Err(err) => {
                log::debug!(
                    "elevated tab rendezvous bridge: WebSocket read error during drain: {err:#}"
                );
                break;
            }
        }
    }

    // Now close the WebSocket cleanly, and -- the part that actually fixes
    // the tail loss -- keep flushing until the outgoing buffer is empty or
    // the shared deadline expires. `close` itself only queues the close
    // frame; a single `flush` on a non-blocking socket may leave both it and
    // any preceding data frames pending.
    let _ = ws.close(None);
    loop {
        match ws.flush() {
            Ok(()) => break,
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    log::warn!(
                        "elevated tab rendezvous bridge: could not flush the outgoing buffer \
                         within {:?} of the peer closing; the tail of this stream is lost",
                        DRAIN_TIMEOUT
                    );
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            // ConnectionClosed/AlreadyClosed here mean the close completed;
            // anything else is not something a retry would help with.
            Err(_) => break,
        }
    }

    bytes_drained
}

/// The actual pump loop: drains everything currently available from the
/// WebSocket (forwarding binary payloads to `bridge_end`) and from
/// `bridge_end` (forwarding as WebSocket binary messages), then parks in
/// `poll()` until either side is readable again. Returns (ending the
/// thread) once either side closes or errors -- dropping `bridge_end`
/// then surfaces as EOF to whatever's reading the other end of the
/// socketpair, which is exactly what a real connection close should look
/// like to the mux protocol layer.
pub(crate) fn pump_bridge(
    ws: &mut WebSocket<TcpStream>,
    bridge_end: &mut filedescriptor::FileDescriptor,
) {
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
                    if let Err(err) =
                        write_all_to_local(bridge_end, ws.get_ref().as_socket_descriptor(), &data)
                    {
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
                    log::debug!(
                        "elevated tab rendezvous bridge: local side closed, draining WebSocket"
                    );
                    drain_websocket_before_close(ws, bridge_end);
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
///
/// The write must not outlive the connection it belongs to: even when the
/// local send buffer is full, we must still detect WebSocket-side closure.
/// If the WebSocket dies while `dest` is backed up, polling only `dest`
/// would block forever -- the write would never complete, and the pump
/// thread would never return to its main loop to notice the dead peer.
/// So we poll both `dest` and `ws_fd` simultaneously, and POLLHUP/POLLERR on
/// the WebSocket fd terminates the write as an error -- the same invariant
/// `pump_bridge` already enforces for its own poll loop.
///
/// The WebSocket entry asks for *no* events at all rather than POLLIN, which
/// matters: we only ever reach this poll because `dest` is backed up, and the
/// reason it is backed up is that the WebSocket is delivering faster than the
/// local side drains -- so there is almost always unread data waiting on it.
/// Asking for POLLIN would therefore return immediately every time, turning
/// this wait into a busy loop that pegs a core for as long as the backpressure
/// lasts. Measured on this platform (`WSAPoll`): with `events: 0` a readable
/// socket does not wake the poll (revents 0), while a closed peer still
/// reports POLLHUP (revents 0x2). That is exactly the pair of properties this
/// needs, and it is why the hangup flags are read from `filedescriptor`'s
/// re-exports rather than written as literals -- the POSIX values differ from
/// the Windows ones (POLLHUP is 0x10 there but 0x2 here, where 0x10 is
/// POLLOUT), so a hardcoded constant silently disables this check.
///
/// We don't check POLLHUP on `dest` because a socketpair end doesn't poll
/// POLLHUP on the peer's closure; we detect that via EOF when the pump loop
/// returns to reading from `bridge_end`.
fn write_all_to_local(
    dest: &mut filedescriptor::FileDescriptor,
    ws_fd: usize,
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
                // Poll both descriptors: we need writability on `dest`, but
                // we must also detect hangup on the WebSocket side. See the
                // doc comment for why the WebSocket entry asks for no events.
                let mut pfd = [
                    pollfd {
                        fd: ws_fd,
                        events: 0,
                        revents: 0,
                    },
                    pollfd {
                        fd: dest.as_socket_descriptor(),
                        events: POLLOUT,
                        revents: 0,
                    },
                ];
                poll(&mut pfd, Some(PUMP_POLL_TIMEOUT))
                    .map_err(|err| std::io::Error::other(format!("{err:#}")))?;

                // If the WebSocket side is closed (POLLHUP/POLLERR), this write
                // cannot succeed and the pump should exit. We only check the
                // WebSocket fd, not `dest`, because socketpair ends don't poll
                // POLLHUP on peer closure (that's detected via EOF on read).
                if pfd[0].revents & (POLLERR | POLLHUP) != 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "connection closed while waiting for send buffer space",
                    ));
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
