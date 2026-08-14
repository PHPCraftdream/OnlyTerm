// Integration tests for the rendezvous WebSocket bridge driving real mux protocol traffic.
//
// These tests use the public API (RendezvousListener::bind, accept, connect_and_bridge)
// and real codec::Pdu values, establishing that:
// 1. PDUs survive the bridge byte-exactly, in both directions.
// 2. Large payloads exceeding socket buffers arrive complete and in order.
// 3. Back-to-back PDUs arrive as the same sequence, none dropped/reordered/merged.
//
// All tests bind to port 0 (ephemeral) to avoid collisions with user OnlyTerm instances.

use anyhow::Context as _;
use codec::{Pdu, Ping, Pong, WriteToPane};
use std::io::{Read, Write};
use std::thread;
use std::time::{Duration, Instant};

/// Helper: write exactly n bytes to a stream, retrying on WouldBlock with timeout.
fn write_all_timeout<W: Write>(
    mut writer: W,
    mut data: &[u8],
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    while !data.is_empty() {
        if Instant::now() >= deadline {
            anyhow::bail!("write_all_timeout: timed out after {:?}", timeout);
        }
        match writer.write(data) {
            Ok(0) => anyhow::bail!("write_all_timeout: wrote 0 bytes"),
            Ok(n) => data = &data[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => return Err(e).context("write_all_timeout: write failed"),
        }
    }
    Ok(())
}

/// Helper: decode a single Pdu from a stream with timeout.
///
/// `read_buffer` belongs to the *caller* and must be carried across every
/// decode from the same stream. `Pdu::try_read_and_decode` reads up to 4 KiB
/// at a time into it and consumes exactly one PDU, deliberately leaving
/// whatever else that read pulled in for the next call. Allocating a fresh
/// buffer per PDU therefore discards every PDU that shared a read with the
/// one returned, and the following call finds the socket empty and fails with
/// `UnexpectedEof` -- which reads exactly like the bridge dropping the rest of
/// the stream, when nothing was ever transmitted twice.
fn decode_pdu_timeout<R: Read>(
    mut reader: R,
    read_buffer: &mut Vec<u8>,
    timeout: Duration,
) -> anyhow::Result<codec::DecodedPdu> {
    let deadline = Instant::now() + timeout;

    loop {
        if Instant::now() >= deadline {
            anyhow::bail!("decode_pdu_timeout: timed out waiting for PDU");
        }

        match Pdu::try_read_and_decode(&mut reader, read_buffer) {
            Ok(Some(decoded)) => return Ok(decoded),
            Ok(None) => {
                // Need more data - wait a bit then try again
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e).context("decode_pdu_timeout: decode failed"),
        }
    }
}

/// Test: small PDUs (Ping/Pong) survive the bridge byte-exactly in both directions.
#[test]
fn test_small_pdu_roundtrip_both_directions() {
    let listener = wezterm_elevated_transport::RendezvousListener::bind()
        .expect("RendezvousListener::bind should succeed");
    let port = listener.port();
    let token = listener.token().to_string();

    // Server side: accept connection
    let server_handle = thread::spawn(move || -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut server_stream = listener
            .accept(deadline, || false)
            .expect("listener.accept should succeed");

        // Send Ping from server -> client
        let ping_pdu = Pdu::Ping(Ping {});
        let mut encoded_ping = Vec::new();
        ping_pdu
            .encode(&mut encoded_ping, 100)
            .expect("ping.encode should succeed");
        server_stream.set_nonblocking(false).unwrap();
        server_stream
            .write_all(&encoded_ping)
            .expect("server write ping should succeed");

        // Receive Pong from client -> server
        let mut read_buffer = Vec::new();
        let decoded_pong =
            decode_pdu_timeout(&mut server_stream, &mut read_buffer, Duration::from_secs(5))
                .expect("server should receive Pong");
        assert_eq!(decoded_pong.serial, 101);
        assert_eq!(decoded_pong.pdu, Pdu::Pong(Pong {}));

        Ok(())
    });

    // Client side: connect
    thread::sleep(Duration::from_millis(100)); // Give server time to start accepting
    let mut client_stream = wezterm_elevated_transport::connect_and_bridge(port, &token)
        .expect("connect_and_bridge should succeed");

    // Receive Ping from server -> client
    let mut read_buffer = Vec::new();
    let decoded_ping =
        decode_pdu_timeout(&mut client_stream, &mut read_buffer, Duration::from_secs(5))
            .expect("client should receive Ping");
    assert_eq!(decoded_ping.serial, 100);
    assert_eq!(decoded_ping.pdu, Pdu::Ping(Ping {}));

    // Send Pong from client -> server
    let pong_pdu = Pdu::Pong(Pong {});
    let mut encoded_pong = Vec::new();
    pong_pdu
        .encode(&mut encoded_pong, 101)
        .expect("pong.encode should succeed");
    client_stream.set_nonblocking(false).unwrap();
    client_stream
        .write_all(&encoded_pong)
        .expect("client write pong should succeed");

    // Wait for server to complete
    server_handle
        .join()
        .expect("server thread should complete successfully")
        .unwrap();
}

/// Test: payload large enough to exceed socket buffers arrives complete and in order.
///
/// The payload size is chosen to exceed typical socket send/receive buffers
/// (which are often 64-128 KB on Windows). A 2 MB payload ensures multiple
/// buffer writes on the sender side and multiple reads on the receiver side.
#[test]
fn test_large_payload_exceeds_socket_buffers() {
    const PAYLOAD_SIZE: usize = 2 * 1024 * 1024; // 2 MB

    let listener = wezterm_elevated_transport::RendezvousListener::bind()
        .expect("RendezvousListener::bind should succeed");
    let port = listener.port();
    let token = listener.token().to_string();

    // Create a large payload with identifiable pattern
    let mut large_data = Vec::with_capacity(PAYLOAD_SIZE);
    for i in 0..PAYLOAD_SIZE {
        large_data.push((i % 256) as u8);
    }

    // Server side: send large WriteToPane PDU
    let server_handle = thread::spawn(move || -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(30); // Large payload needs more time
        let mut server_stream = listener
            .accept(deadline, || false)
            .expect("listener.accept should succeed");

        // Send large WriteToPane from server -> client
        let write_pdu = Pdu::WriteToPane(WriteToPane {
            pane_id: 42,
            data: large_data.clone(),
        });
        let mut encoded_write = Vec::new();
        write_pdu
            .encode(&mut encoded_write, 200)
            .expect("large write_pdu.encode should succeed");

        server_stream.set_nonblocking(false).unwrap();
        write_all_timeout(&mut server_stream, &encoded_write, Duration::from_secs(30))
            .expect("server write large payload should succeed");

        Ok(())
    });

    // Client side: receive and verify large payload
    thread::sleep(Duration::from_millis(100));
    let mut client_stream = wezterm_elevated_transport::connect_and_bridge(port, &token)
        .expect("connect_and_bridge should succeed");

    // Receive large WriteToPane from server -> client
    let mut read_buffer = Vec::new();
    let decoded_write = decode_pdu_timeout(
        &mut client_stream,
        &mut read_buffer,
        Duration::from_secs(30),
    )
    .expect("client should receive large WriteToPane");

    assert_eq!(decoded_write.serial, 200);
    match &decoded_write.pdu {
        Pdu::WriteToPane(WriteToPane { pane_id, data }) => {
            assert_eq!(*pane_id, 42);
            assert_eq!(data.len(), PAYLOAD_SIZE, "payload should be complete");
            // Verify pattern
            for (i, &byte) in data.iter().enumerate() {
                assert_eq!(
                    byte,
                    (i % 256) as u8,
                    "payload byte at position {} mismatch",
                    i
                );
            }
        }
        other => panic!("expected WriteToPane, got {}", other.pdu_name()),
    }

    server_handle
        .join()
        .expect("server thread should complete successfully")
        .unwrap();
}

/// Test: a stream of many PDUs back to back arrives in the same sequence.
///
/// Tests the boundary conditions: ensures the bridge doesn't drop PDUs,
/// doesn't reorder them, and doesn't merge neighboring PDUs.
///
/// This was briefly disabled on the theory that the bridge dropped everything
/// after the first PDU. It does not: the decode helper was allocating a fresh
/// read buffer per PDU and discarding whatever else the same read had pulled
/// in. See `decode_pdu_timeout`.
#[test]
fn test_many_pdus_back_to_back_arrive_in_order() {
    const NUM_PDUS: usize = 100;

    let listener = wezterm_elevated_transport::RendezvousListener::bind()
        .expect("RendezvousListener::bind should succeed");
    let port = listener.port();
    let token = listener.token().to_string();

    // Create alternating Ping/Pong PDUs with sequential serial numbers
    let make_sequence = || -> Vec<Pdu> {
        (0..NUM_PDUS)
            .map(|i| {
                if i % 2 == 0 {
                    Pdu::Ping(Ping {})
                } else {
                    Pdu::Pong(Pong {})
                }
            })
            .collect()
    };

    let expected_sequence = make_sequence();

    // Server side: send all PDUs back to back
    let server_sequence = make_sequence();
    let server_handle = thread::spawn(move || -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut server_stream = listener
            .accept(deadline, || false)
            .expect("listener.accept should succeed");

        server_stream.set_nonblocking(false).unwrap();

        // Send all PDUs as one stream
        for (serial, pdu) in server_sequence.iter().enumerate() {
            let mut encoded = Vec::new();
            pdu.encode(&mut encoded, serial as u64)
                .expect("pdu.encode should succeed");
            server_stream
                .write_all(&encoded)
                .expect("server write pdu should succeed");
        }

        // Flush to ensure all data is sent
        server_stream.flush().expect("server flush should succeed");

        // Don't close - let the bridge handle cleanup when client is done
        // Keep this thread alive while client reads
        thread::sleep(Duration::from_secs(5));

        Ok(())
    });

    // Give the server time to start accepting
    thread::sleep(Duration::from_millis(200));

    // Client side: receive and verify sequence
    let mut client_stream = wezterm_elevated_transport::connect_and_bridge(port, &token)
        .expect("connect_and_bridge should succeed");

    client_stream.set_nonblocking(false).unwrap();

    // One buffer for the whole stream: see `decode_pdu_timeout`.
    let mut read_buffer = Vec::new();

    // Receive all PDUs in order
    for (serial, expected_pdu) in expected_sequence.iter().enumerate() {
        let decoded = decode_pdu_timeout(
            &mut client_stream,
            &mut read_buffer,
            Duration::from_secs(10),
        )
        .unwrap_or_else(|_| panic!("client should receive PDU {}", serial));

        assert_eq!(decoded.serial, serial as u64, "serial number should match");
        assert_eq!(
            decoded.pdu, *expected_pdu,
            "PDU type at position {} should match",
            serial
        );
    }

    server_handle
        .join()
        .expect("server thread should complete successfully")
        .unwrap();
}
