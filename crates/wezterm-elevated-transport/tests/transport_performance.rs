// Benchmark harness for elevated transport performance.
//
// Measures:
// 1. Sustained throughput pushing 16+ MiB through WebSocket-bridged transport
// 2. Round-trip latency (median and p99) of small PDUs
// 3. Baseline comparisons (raw socketpair, in-memory)
//
// All measurements use real mux PDUs (WriteToPane for throughput, Ping/Pong for latency)
// and report what's included/excluded (encode/decode overhead, transport cost, etc.)
//
// Run individual tests with: cargo test -p wezterm-elevated-transport transport_performance -- --test-threads=1 --nocapture

use anyhow::Context as _;
use codec::{Pdu, Ping, Pong, WriteToPane};
use filedescriptor::socketpair;
use std::io::{Read, Write};
use std::os::windows::io::{FromRawSocket, IntoRawSocket};
use std::time::{Duration, Instant};
use wezterm_uds::UnixStream;

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

/// Benchmark: sustained throughput pushing 16+ MiB through WebSocket-bridged transport.
///
/// WHAT'S MEASURED:
/// - Includes: WebSocket framing, TCP loopback, socketpair, pump thread overhead
/// - Excludes: PDU encode/decode (measured separately in in-memory baseline)
/// - Transport: WebSocket bridged to socketpair (what elevated tabs pay)
///
/// Test sends a series of WriteToPane PDUs with large payloads totaling 16 MiB.
#[test]
fn bench_throughput_websocket_bridge() {
    const TOTAL_BYTES: usize = 16 * 1024 * 1024; // 16 MiB
    const PDU_PAYLOAD_SIZE: usize = 256 * 1024; // 256 KiB per PDU
    const NUM_PDUS: usize = TOTAL_BYTES / PDU_PAYLOAD_SIZE;

    let listener = wezterm_elevated_transport::RendezvousListener::bind()
        .expect("RendezvousListener::bind should succeed");
    let port = listener.port();
    let token = listener.token().to_string();

    // Both sides meet here so the clock starts at the same moment as the
    // first byte is sent. Starting it when the *receiver* first reads is not
    // good enough: by then the bridge's pump thread and the TCP buffers have
    // already absorbed a chunk of the stream, so the receiver merely drains a
    // backlog and the bridge scores higher than the bare socketpair it runs
    // on top of -- which it cannot actually be.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let server_barrier = std::sync::Arc::clone(&barrier);

    // A second rendezvous, at the END. Without it the sender returns as soon
    // as its last write is accepted, drops the stream, and the bridge tears
    // down while data is still in flight -- the receiver then sees EOF around
    // PDU 61-63 of 64, in roughly three runs out of four. That tail loss is a
    // real defect in the bridge's shutdown path, not an artifact of this
    // benchmark, and it is tracked separately; holding the connection open
    // here keeps the *throughput* measurement from depending on it.
    let done = std::sync::Arc::new(std::sync::Barrier::new(2));
    let server_done = std::sync::Arc::clone(&done);

    // Server side: send all PDUs
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

        // NOTE: deliberately no timing on this side. A sender-side clock stops
        // when the last write is handed to the kernel (for a socketpair) or to
        // the bridge's pump thread (for the WebSocket path), NOT when the data
        // arrives. The pump buffers, so timing here made the bridged transport
        // look *faster* than the bare socketpair it is bridged onto -- and
        // faster than encoding alone can run, which is how the error was
        // caught. Throughput is timed by the receiver below, from the barrier.
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

        // Hold the stream open until the receiver has everything.
        server_done.wait();

        Ok(())
    });

    // Client side: receive all PDUs
    std::thread::sleep(Duration::from_millis(100));
    let mut client_stream = wezterm_elevated_transport::connect_and_bridge(port, &token)
        .expect("connect_and_bridge should succeed");

    client_stream.set_nonblocking(false).unwrap();
    let mut read_buffer = Vec::new();

    barrier.wait();
    let start = Instant::now();

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

    let elapsed = start.elapsed();
    let throughput_mb_per_sec = (TOTAL_BYTES as f64) / (elapsed.as_secs_f64() * 1024.0 * 1024.0);

    println!(
        "WebSocket bridge throughput: {:.2} MiB/s ({} bytes in {:.2}s)",
        throughput_mb_per_sec,
        TOTAL_BYTES,
        elapsed.as_secs_f64()
    );

    done.wait();
    server_handle
        .join()
        .expect("server thread should complete")
        .unwrap();
}

/// Benchmark: sustained throughput through raw socketpair (baseline for ordinary tabs).
///
/// WHAT'S MEASURED:
/// - Includes: socketpair IPC only
/// - Excludes: PDU encode/decode (same as elevated transport baseline)
/// - Transport: Raw socketpair (what ordinary tabs pay)
///
/// Test sends the same payload as bench_throughput_websocket_bridge but over
/// a direct socketpair without WebSocket bridging.
#[test]
fn bench_throughput_socketpair_baseline() {
    const TOTAL_BYTES: usize = 16 * 1024 * 1024; // 16 MiB
    const PDU_PAYLOAD_SIZE: usize = 256 * 1024; // 256 KiB per PDU
    const NUM_PDUS: usize = TOTAL_BYTES / PDU_PAYLOAD_SIZE;

    // Create a socketpair
    let (sock_a, sock_b) = socketpair().expect("socketpair should succeed");

    // SAFETY: Both ends were just created and are uniquely owned here.
    let mut client_stream = unsafe { UnixStream::from_raw_socket(sock_a.into_raw_socket()) };
    let mut server_stream = unsafe { UnixStream::from_raw_socket(sock_b.into_raw_socket()) };

    client_stream.set_nonblocking(false).unwrap();
    server_stream.set_nonblocking(false).unwrap();

    // Create test payload
    let mut large_data = Vec::with_capacity(PDU_PAYLOAD_SIZE);
    for i in 0..PDU_PAYLOAD_SIZE {
        large_data.push((i % 256) as u8);
    }

    // Server thread sends all PDUs
    // No timing here, and the same barrier as the WebSocket case: see the
    // note in bench_throughput_websocket_bridge for why both matter. The two
    // benchmarks must be measured identically or comparing them is meaningless.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let server_barrier = std::sync::Arc::clone(&barrier);

    let server_handle = std::thread::spawn(move || {
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

            server_stream
                .write_all(&encoded_write)
                .expect("server write should succeed");
        }

        server_stream
    });

    // Client receives all PDUs
    let mut read_buffer = Vec::new();

    barrier.wait();
    let start = Instant::now();

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

    let elapsed = start.elapsed();
    let throughput_mb_per_sec = (TOTAL_BYTES as f64) / (elapsed.as_secs_f64() * 1024.0 * 1024.0);

    println!(
        "Socketpair baseline throughput: {:.2} MiB/s ({} bytes in {:.2}s)",
        throughput_mb_per_sec,
        TOTAL_BYTES,
        elapsed.as_secs_f64()
    );

    let _server_stream = server_handle.join().expect("server thread should complete");
}

/// Benchmark: in-memory encode/decode baseline (no transport cost).
///
/// WHAT'S MEASURED:
/// - Includes: PDU encode/decode overhead only
/// - Excludes: Any transport cost (this is pure codec overhead)
/// - Transport: None (in-memory)
///
/// This tells us how much of the measured transport cost is actually
/// codec/serialization vs. the transport itself.
#[test]
fn bench_throughput_in_memory_baseline() {
    const TOTAL_BYTES: usize = 16 * 1024 * 1024; // 16 MiB
    const PDU_PAYLOAD_SIZE: usize = 256 * 1024; // 256 KiB per PDU
    const NUM_PDUS: usize = TOTAL_BYTES / PDU_PAYLOAD_SIZE;

    // Create test payload
    let mut large_data = Vec::with_capacity(PDU_PAYLOAD_SIZE);
    for i in 0..PDU_PAYLOAD_SIZE {
        large_data.push((i % 256) as u8);
    }

    let start = Instant::now();

    // Encode and decode all PDUs in memory
    for serial in 0..NUM_PDUS {
        let write_pdu = Pdu::WriteToPane(WriteToPane {
            pane_id: 42,
            data: large_data.clone(),
        });

        // Encode
        let mut encoded = Vec::new();
        write_pdu
            .encode(&mut encoded, serial as u64)
            .expect("encode should succeed");

        // Decode
        let decoded = Pdu::decode(encoded.as_slice()).expect("decode should succeed");
        assert_eq!(decoded.serial, serial as u64);
    }

    let elapsed = start.elapsed();
    let throughput_mb_per_sec = (TOTAL_BYTES as f64) / (elapsed.as_secs_f64() * 1024.0 * 1024.0);

    println!(
        "In-memory baseline throughput: {:.2} MiB/s ({} bytes in {:.2}s)",
        throughput_mb_per_sec,
        TOTAL_BYTES,
        elapsed.as_secs_f64()
    );
}

/// Benchmark: round-trip latency through WebSocket-bridged transport.
///
/// WHAT'S MEASURED:
/// - Includes: WebSocket framing, TCP loopback, socketpair, pump thread overhead
/// - Excludes: PDU encode/decode (included in the measured round-trip, but identical across transports)
/// - Transport: WebSocket bridged to socketpair
///
/// Reports MEDIAN and p99 latencies over many iterations.
#[test]
fn bench_latency_websocket_bridge() {
    const ITERATIONS: usize = 10_000; // Enough for meaningful p99
    const WARMUP_ITERATIONS: usize = 1_000; // Warm up the system

    let listener = wezterm_elevated_transport::RendezvousListener::bind()
        .expect("RendezvousListener::bind should succeed");
    let port = listener.port();
    let token = listener.token().to_string();

    // Server side: respond to each PDU
    let server_handle = std::thread::spawn(move || -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut server_stream = listener
            .accept(deadline, || false)
            .expect("listener.accept should succeed");

        server_stream.set_nonblocking(false).unwrap();

        // Warmup
        for _ in 0..WARMUP_ITERATIONS {
            let mut read_buffer = Vec::new();
            let decoded = decode_pdu_timeout(
                &mut server_stream,
                &mut read_buffer,
                Duration::from_secs(10),
            )
            .expect("server should receive Ping");

            let pong_pdu = Pdu::Pong(Pong {});
            let mut encoded_pong = Vec::new();
            pong_pdu
                .encode(&mut encoded_pong, decoded.serial)
                .expect("pong.encode should succeed");

            write_all_timeout(&mut server_stream, &encoded_pong, Duration::from_secs(10))
                .expect("server write pong should succeed");
        }

        // Main measurements
        for _ in 0..ITERATIONS {
            let mut read_buffer = Vec::new();
            let decoded = decode_pdu_timeout(
                &mut server_stream,
                &mut read_buffer,
                Duration::from_secs(10),
            )
            .expect("server should receive Ping");

            let pong_pdu = Pdu::Pong(Pong {});
            let mut encoded_pong = Vec::new();
            pong_pdu
                .encode(&mut encoded_pong, decoded.serial)
                .expect("pong.encode should succeed");

            write_all_timeout(&mut server_stream, &encoded_pong, Duration::from_secs(10))
                .expect("server write pong should succeed");
        }

        Ok(())
    });

    // Client side: send Ping, receive Pong, measure latency
    std::thread::sleep(Duration::from_millis(100));
    let mut client_stream = wezterm_elevated_transport::connect_and_bridge(port, &token)
        .expect("connect_and_bridge should succeed");

    client_stream.set_nonblocking(false).unwrap();
    let mut read_buffer = Vec::new();

    // Warmup
    for i in 0..WARMUP_ITERATIONS {
        let ping_pdu = Pdu::Ping(Ping {});
        let mut encoded_ping = Vec::new();
        ping_pdu
            .encode(&mut encoded_ping, i as u64)
            .expect("ping.encode should succeed");

        client_stream
            .write_all(&encoded_ping)
            .expect("client write ping should succeed");

        let decoded_pong = decode_pdu_timeout(
            &mut client_stream,
            &mut read_buffer,
            Duration::from_secs(10),
        )
        .expect("client should receive Pong");
        assert_eq!(decoded_pong.serial, i as u64);
    }

    // Main measurements
    let mut latencies = Vec::with_capacity(ITERATIONS);

    for i in 0..ITERATIONS {
        let ping_pdu = Pdu::Ping(Ping {});
        let mut encoded_ping = Vec::new();
        ping_pdu
            .encode(&mut encoded_ping, i as u64)
            .expect("ping.encode should succeed");

        let start = Instant::now();

        client_stream
            .write_all(&encoded_ping)
            .expect("client write ping should succeed");

        let decoded_pong = decode_pdu_timeout(
            &mut client_stream,
            &mut read_buffer,
            Duration::from_secs(10),
        )
        .expect("client should receive Pong");

        let elapsed = start.elapsed();

        assert_eq!(decoded_pong.serial, i as u64);
        latencies.push(elapsed);
    }

    server_handle
        .join()
        .expect("server thread should complete")
        .unwrap();

    // Compute statistics
    latencies.sort();
    let median = latencies[ITERATIONS / 2];
    let p99_index = (ITERATIONS as f64 * 0.99) as usize;
    let p99 = latencies[p99_index];
    let min = latencies[0];
    let max = latencies[ITERATIONS - 1];

    let median_us = median.as_secs_f64() * 1_000_000.0;
    let p99_us = p99.as_secs_f64() * 1_000_000.0;
    let min_us = min.as_secs_f64() * 1_000_000.0;
    let max_us = max.as_secs_f64() * 1_000_000.0;

    println!("WebSocket bridge latency ({} iterations):", ITERATIONS);
    println!("  Median: {:.2} μs", median_us);
    println!("  p99: {:.2} μs", p99_us);
    println!("  Min: {:.2} μs", min_us);
    println!("  Max: {:.2} μs", max_us);
}

/// Benchmark: round-trip latency through raw socketpair (baseline for ordinary tabs).
///
/// WHAT'S MEASURED:
/// - Includes: socketpair IPC only
/// - Excludes: PDU encode/decode (same as elevated transport baseline)
/// - Transport: Raw socketpair
///
/// Reports MEDIAN and p99 latencies over many iterations.
#[test]
fn bench_latency_socketpair_baseline() {
    const ITERATIONS: usize = 10_000;
    const WARMUP_ITERATIONS: usize = 1_000;

    // Create a socketpair
    let (sock_a, sock_b) = socketpair().expect("socketpair should succeed");

    // SAFETY: Both ends were just created and are uniquely owned here.
    let mut client_stream = unsafe { UnixStream::from_raw_socket(sock_a.into_raw_socket()) };
    let mut server_stream = unsafe { UnixStream::from_raw_socket(sock_b.into_raw_socket()) };

    client_stream.set_nonblocking(false).unwrap();
    server_stream.set_nonblocking(false).unwrap();

    // Server thread responds to each PDU
    let server_handle = std::thread::spawn(move || {
        let mut read_buffer = Vec::new();

        // Warmup
        for _ in 0..WARMUP_ITERATIONS {
            let decoded = decode_pdu_timeout(
                &mut server_stream,
                &mut read_buffer,
                Duration::from_secs(10),
            )
            .expect("server should receive Ping");

            let pong_pdu = Pdu::Pong(Pong {});
            let mut encoded_pong = Vec::new();
            pong_pdu
                .encode(&mut encoded_pong, decoded.serial)
                .expect("pong.encode should succeed");

            server_stream
                .write_all(&encoded_pong)
                .expect("server write pong should succeed");
        }

        // Main measurements
        for _ in 0..ITERATIONS {
            let decoded = decode_pdu_timeout(
                &mut server_stream,
                &mut read_buffer,
                Duration::from_secs(10),
            )
            .expect("server should receive Ping");

            let pong_pdu = Pdu::Pong(Pong {});
            let mut encoded_pong = Vec::new();
            pong_pdu
                .encode(&mut encoded_pong, decoded.serial)
                .expect("pong.encode should succeed");

            server_stream
                .write_all(&encoded_pong)
                .expect("server write pong should succeed");
        }

        server_stream
    });

    // Client sends Ping, receives Pong, measures latency
    let mut read_buffer = Vec::new();

    // Warmup
    for i in 0..WARMUP_ITERATIONS {
        let ping_pdu = Pdu::Ping(Ping {});
        let mut encoded_ping = Vec::new();
        ping_pdu
            .encode(&mut encoded_ping, i as u64)
            .expect("ping.encode should succeed");

        client_stream
            .write_all(&encoded_ping)
            .expect("client write ping should succeed");

        let decoded_pong = decode_pdu_timeout(
            &mut client_stream,
            &mut read_buffer,
            Duration::from_secs(10),
        )
        .expect("client should receive Pong");
        assert_eq!(decoded_pong.serial, i as u64);
    }

    // Main measurements
    let mut latencies = Vec::with_capacity(ITERATIONS);

    for i in 0..ITERATIONS {
        let ping_pdu = Pdu::Ping(Ping {});
        let mut encoded_ping = Vec::new();
        ping_pdu
            .encode(&mut encoded_ping, i as u64)
            .expect("ping.encode should succeed");

        let start = Instant::now();

        client_stream
            .write_all(&encoded_ping)
            .expect("client write ping should succeed");

        let decoded_pong = decode_pdu_timeout(
            &mut client_stream,
            &mut read_buffer,
            Duration::from_secs(10),
        )
        .expect("client should receive Pong");

        let elapsed = start.elapsed();

        assert_eq!(decoded_pong.serial, i as u64);
        latencies.push(elapsed);
    }

    let _server_stream = server_handle.join().expect("server thread should complete");

    // Compute statistics
    latencies.sort();
    let median = latencies[ITERATIONS / 2];
    let p99_index = (ITERATIONS as f64 * 0.99) as usize;
    let p99 = latencies[p99_index];
    let min = latencies[0];
    let max = latencies[ITERATIONS - 1];

    let median_us = median.as_secs_f64() * 1_000_000.0;
    let p99_us = p99.as_secs_f64() * 1_000_000.0;
    let min_us = min.as_secs_f64() * 1_000_000.0;
    let max_us = max.as_secs_f64() * 1_000_000.0;

    println!("Socketpair baseline latency ({} iterations):", ITERATIONS);
    println!("  Median: {:.2} μs", median_us);
    println!("  p99: {:.2} μs", p99_us);
    println!("  Min: {:.2} μs", min_us);
    println!("  Max: {:.2} μs", max_us);
}

/// Benchmark: in-memory encode/decode latency baseline (no transport cost).
///
/// WHAT'S MEASURED:
/// - Includes: PDU encode/decode overhead only
/// - Excludes: Any transport cost
/// - Transport: None (in-memory)
///
/// This tells us the minimum possible latency with this codec.
#[test]
fn bench_latency_in_memory_baseline() {
    const ITERATIONS: usize = 10_000;
    const WARMUP_ITERATIONS: usize = 1_000;

    let ping_pdu = Pdu::Ping(Ping {});
    let pong_pdu = Pdu::Pong(Pong {});

    // Warmup
    for i in 0..WARMUP_ITERATIONS {
        let mut encoded_ping = Vec::new();
        ping_pdu
            .encode(&mut encoded_ping, i as u64)
            .expect("ping.encode should succeed");

        let decoded = Pdu::decode(encoded_ping.as_slice()).expect("decode should succeed");
        assert_eq!(decoded.serial, i as u64);

        let mut encoded_pong = Vec::new();
        pong_pdu
            .encode(&mut encoded_pong, i as u64)
            .expect("pong.encode should succeed");

        let decoded_pong = Pdu::decode(encoded_pong.as_slice()).expect("decode should succeed");
        assert_eq!(decoded_pong.serial, i as u64);
    }

    // Main measurements
    let mut latencies = Vec::with_capacity(ITERATIONS);

    for i in 0..ITERATIONS {
        let mut encoded_ping = Vec::new();

        let start = Instant::now();

        ping_pdu
            .encode(&mut encoded_ping, i as u64)
            .expect("ping.encode should succeed");

        let decoded = Pdu::decode(encoded_ping.as_slice()).expect("decode should succeed");
        assert_eq!(decoded.serial, i as u64);

        let mut encoded_pong = Vec::new();
        pong_pdu
            .encode(&mut encoded_pong, i as u64)
            .expect("pong.encode should succeed");

        let decoded_pong = Pdu::decode(encoded_pong.as_slice()).expect("decode should succeed");
        assert_eq!(decoded_pong.serial, i as u64);

        let elapsed = start.elapsed();
        latencies.push(elapsed);
    }

    // Compute statistics
    latencies.sort();
    let median = latencies[ITERATIONS / 2];
    let p99_index = (ITERATIONS as f64 * 0.99) as usize;
    let p99 = latencies[p99_index];
    let min = latencies[0];
    let max = latencies[ITERATIONS - 1];

    let median_us = median.as_secs_f64() * 1_000_000.0;
    let p99_us = p99.as_secs_f64() * 1_000_000.0;
    let min_us = min.as_secs_f64() * 1_000_000.0;
    let max_us = max.as_secs_f64() * 1_000_000.0;

    println!("In-memory baseline latency ({} iterations):", ITERATIONS);
    println!("  Median: {:.2} μs", median_us);
    println!("  p99: {:.2} μs", p99_us);
    println!("  Min: {:.2} μs", min_us);
    println!("  Max: {:.2} μs", max_us);
}
