// Regression test for WebSocket bridge tail-loss bug.
//
// BUG: The WebSocket bridge loses the tail of the stream when the writing side closes.
//
// REPRODUCTION: Push 64 WriteToPane PDUs of 256 KiB each (16 MiB) through
// connect_and_bridge(), and let the sender drop its stream as soon as its last
// write_all returns. The receiver then fails with "decode failed: End Of File"
// at PDU 61-63 of 64, in roughly THREE RUNS OUT OF FOUR.
//
// This test deliberately avoids the end-barrier that bench_throughput_websocket_bridge
// uses to sidestep this bug, so it can catch the actual defect.

use anyhow::Context as _;
use codec::{Pdu, WriteToPane};
use std::io::Read;
use std::time::{Duration, Instant};

/// Helper: write exactly n bytes to a stream, retrying on WouldBlock with timeout.
fn write_all_timeout<W: std::io::Write>(
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
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => return Err(e).context("write_all_timeout: write failed"),
        }
    }
    Ok(())
}

/// Helper: decode a single Pdu from a stream with timeout.
/// `read_buffer` belongs to the caller and must be carried across every decode
/// from the same stream (see integration.rs for detailed explanation).
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
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e).context("decode_pdu_timeout: decode failed"),
        }
    }
}

/// Regression test: WebSocket bridge loses tail when sender closes immediately.
///
/// This test reproduces the bug where the WebSocket bridge loses the tail of
/// the stream when the writing side closes. The sender writes 64 PDUs (16 MiB)
/// and immediately drops its stream, without waiting for the receiver to finish.
///
/// Against the buggy code, this fails ~75% of the time (EOF at PDU 61-63 of 64).
/// After the fix, it should pass 100% of the time.
#[test]
fn test_websocket_bridge_no_tail_loss_on_sender_close() {
    const TOTAL_BYTES: usize = 16 * 1024 * 1024; // 16 MiB
    const PDU_PAYLOAD_SIZE: usize = 256 * 1024; // 256 KiB per PDU
    const NUM_PDUS: usize = TOTAL_BYTES / PDU_PAYLOAD_SIZE;

    let listener = wezterm_elevated_transport::RendezvousListener::bind()
        .expect("RendezvousListener::bind should succeed");
    let port = listener.port();
    let token = listener.token().to_string();

    // A barrier so both sides start at the same moment (for consistent timing).
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let server_barrier = std::sync::Arc::clone(&barrier);

    // Server side: send all PDUs and immediately drop the stream (no done barrier).
    let server_handle = std::thread::spawn(move || -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut server_stream = listener
            .accept(deadline, || false)
            .expect("listener.accept should succeed");

        server_stream.set_nonblocking(false).unwrap();

        // Create test payload
        let mut large_data = Vec::with_capacity(PDU_PAYLOAD_SIZE);
        for i in 0..PDU_PAYLOAD_SIZE {
            large_data.push((i % 256) as u8);
        }

        server_barrier.wait();

        for serial in 0..NUM_PDUS {
            let write_pdu = Pdu::WriteToPane(WriteToPane {
                pane_id: 42,
                data: large_data.clone(),
            });
            let mut encoded_write = Vec::new();
            write_pdu
                .encode(&mut encoded_write, serial as u64)
                .expect("write_pdu.encode should succeed");

            write_all_timeout(&mut server_stream, &encoded_write, Duration::from_secs(30))
                .expect("server write should succeed");
        }

        // CRITICAL: Drop the stream immediately after the last write.
        // This is what triggers the bug: the pump thread may still have data
        // buffered that hasn't been forwarded to the WebSocket yet.
        drop(server_stream);

        Ok(())
    });

    // Client side: receive all PDUs
    std::thread::sleep(Duration::from_millis(100));
    let mut client_stream = wezterm_elevated_transport::connect_and_bridge(port, &token)
        .expect("connect_and_bridge should succeed");

    client_stream.set_nonblocking(false).unwrap();
    let mut read_buffer = Vec::new();

    barrier.wait();

    // Try to receive all NUM_PDUS PDUs. This will fail with EOF if the bug occurs.
    for serial in 0..NUM_PDUS {
        let decoded = decode_pdu_timeout(
            &mut client_stream,
            &mut read_buffer,
            Duration::from_secs(30),
        )
        .unwrap_or_else(|err| {
            panic!(
                "client should receive PDU {} of {}: {:#}",
                serial, NUM_PDUS, err
            )
        });

        assert_eq!(decoded.serial, serial as u64);
    }

    server_handle
        .join()
        .expect("server thread should complete")
        .unwrap();
}

/// Regression test: WebSocket bridge loses tail when receiver closes immediately.
///
/// This is the symmetric case: the receiver closes while data is still arriving
/// from the WebSocket. The same bug can manifest here if the pump thread doesn't
/// drain its buffers before exiting.
#[test]
fn test_websocket_bridge_no_tail_loss_on_receiver_close() {
    const TOTAL_BYTES: usize = 16 * 1024 * 1024; // 16 MiB
    const PDU_PAYLOAD_SIZE: usize = 256 * 1024; // 256 KiB per PDU
    const NUM_PDUS: usize = TOTAL_BYTES / PDU_PAYLOAD_SIZE;

    let listener = wezterm_elevated_transport::RendezvousListener::bind()
        .expect("RendezvousListener::bind should succeed");
    let port = listener.port();
    let token = listener.token().to_string();

    // A barrier so both sides start at the same moment.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let server_barrier = std::sync::Arc::clone(&barrier);

    // Server side: send all PDUs and keep the stream open.
    let server_stream_holder = std::sync::Arc::new(std::sync::Mutex::new(None));
    let server_stream_holder_clone = std::sync::Arc::clone(&server_stream_holder);

    let server_handle = std::thread::spawn(move || -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut server_stream = listener
            .accept(deadline, || false)
            .expect("listener.accept should succeed");

        server_stream.set_nonblocking(false).unwrap();

        // Store the stream so we can keep it alive
        *server_stream_holder_clone.lock().unwrap() = Some(server_stream.try_clone().unwrap());

        // Create test payload
        let mut large_data = Vec::with_capacity(PDU_PAYLOAD_SIZE);
        for i in 0..PDU_PAYLOAD_SIZE {
            large_data.push((i % 256) as u8);
        }

        server_barrier.wait();

        for serial in 0..NUM_PDUS {
            let write_pdu = Pdu::WriteToPane(WriteToPane {
                pane_id: 42,
                data: large_data.clone(),
            });
            let mut encoded_write = Vec::new();
            write_pdu
                .encode(&mut encoded_write, serial as u64)
                .expect("write_pdu.encode should succeed");

            write_all_timeout(&mut server_stream, &encoded_write, Duration::from_secs(30))
                .expect("server write should succeed");
        }

        // Keep the stream open until the test is done
        std::thread::sleep(Duration::from_secs(5));

        Ok(())
    });

    // Client side: receive all PDUs and immediately drop the stream.
    std::thread::sleep(Duration::from_millis(100));
    let mut client_stream = wezterm_elevated_transport::connect_and_bridge(port, &token)
        .expect("connect_and_bridge should succeed");

    client_stream.set_nonblocking(false).unwrap();
    let mut read_buffer = Vec::new();

    barrier.wait();

    // Receive all NUM_PDUS PDUs.
    for serial in 0..NUM_PDUS {
        let decoded = decode_pdu_timeout(
            &mut client_stream,
            &mut read_buffer,
            Duration::from_secs(30),
        )
        .unwrap_or_else(|err| {
            panic!(
                "client should receive PDU {} of {}: {:#}",
                serial, NUM_PDUS, err
            )
        });

        assert_eq!(decoded.serial, serial as u64);
    }

    // CRITICAL: Drop the client stream immediately after receiving the last PDU.
    // The server is still sending (or has data in flight), and the pump thread
    // needs to handle this gracefully without losing data that was already read
    // from the WebSocket but not yet written to the local socketpair.
    drop(client_stream);

    // Wait for the server to finish
    server_handle
        .join()
        .expect("server thread should complete")
        .unwrap();

    // Clean up the server stream
    drop(server_stream_holder);
}
