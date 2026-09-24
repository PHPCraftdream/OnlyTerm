#[cfg(test)]
mod test_cases {
    use crate::bridge::pump_bridge;
    use crate::rendezvous::{
        connect_and_bridge, generate_rendezvous_token, RendezvousListener, TokenCallback,
        BASE58_ALPHABET, TOKEN_HEADER,
    };
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::os::windows::io::{FromRawSocket, IntoRawSocket};
    use std::time::{Duration, Instant};
    use tungstenite::client::IntoClientRequest;
    use tungstenite::protocol::WebSocket;

    /// Like `spawn_bridge_to_local_stream`, but test-only and returns the
    /// pump thread handle. Used by peer-death tests to verify the pump exits cleanly.
    fn spawn_bridge_for_test(
        mut ws: WebSocket<TcpStream>,
    ) -> (onlyterm_uds::UnixStream, std::thread::JoinHandle<()>) {
        let (local_end, bridge_end) =
            filedescriptor::socketpair().expect("creating local bridge socketpair should succeed");

        // SAFETY: `local_end` was just created by `filedescriptor::
        // socketpair()` immediately above and is uniquely owned at this point;
        // `into_raw_socket()` transfers that ownership into the `UnixStream`
        // constructed here, which becomes its sole owner.
        let local_stream =
            unsafe { onlyterm_uds::UnixStream::from_raw_socket(local_end.into_raw_socket()) };

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
        onlyterm_uds::UnixStream,
        onlyterm_uds::UnixStream,
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
    fn connected_bridge_pair() -> (onlyterm_uds::UnixStream, onlyterm_uds::UnixStream) {
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
        mut from: onlyterm_uds::UnixStream,
        mut to: onlyterm_uds::UnixStream,
        payload: &[u8],
    ) -> (onlyterm_uds::UnixStream, onlyterm_uds::UnixStream) {
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
    fn read_exactly(stream: &mut onlyterm_uds::UnixStream, buf: &mut [u8]) {
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
    fn wait_for_eof(stream: &mut onlyterm_uds::UnixStream, deadline: Instant) {
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
