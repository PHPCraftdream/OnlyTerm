#![warn(clippy::undocumented_unsafe_blocks)]
//! encode and decode the frames for the mux protocol.
//! The frames include the length of a PDU as well as an identifier
//! that informs us how to decode it.  The length, ident and serial
//! number are encoded using a variable length integer encoding.
//! Rather than rely solely on serde to serialize and deserialize an
//! enum, we encode the enum variants with a version/identifier tag
//! for ourselves.  This will make it a little easier to manage
//! client and server instances that are built from different versions
//! of this code; in this way the client and server can more gracefully
//! manage unknown enum variants.
#![allow(dead_code)]
#![allow(clippy::range_plus_one)]

mod framing;
mod input_serial;
mod lines;
mod messages;
mod pdu;

pub use framing::CorruptResponse;
pub use input_serial::InputSerial;
pub use lines::SerializedLines;
pub use messages::*;
pub use pdu::{DecodedPdu, Pdu, CODEC_VERSION};

#[cfg(test)]
mod test {
    use super::*;
    use crate::framing::{decode_raw, decode_raw_async, deserialize, encode_raw, serialize};
    use mux::pane::PaneId;
    use mux::renderable::{RenderableDimensions, StableCursorPosition};
    use mux::tab::SerdeUrl;
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::ops::Range;
    use termwiz::surface::{Line, SequenceNo};
    use wezterm_term::TerminalSize;
    use wezterm_term::StableRowIndex;

    #[test]
    fn test_frame() {
        let mut encoded = Vec::new();
        encode_raw(0x81, 0x42, b"hello", false, &mut encoded).unwrap();
        assert_eq!(&encoded, b"\x08\x42\x81\x01hello");
        let decoded = decode_raw(encoded.as_slice()).unwrap();
        assert_eq!(decoded.ident, 0x81);
        assert_eq!(decoded.serial, 0x42);
        assert_eq!(decoded.data, b"hello");
    }

    #[test]
    fn test_frame_lengths() {
        for (serial, target_len) in (1..).zip([128, 247, 256, 65536, 16777216].iter()) {
            let mut payload = Vec::with_capacity(*target_len);
            payload.resize(*target_len, b'a');
            let mut encoded = Vec::new();
            encode_raw(0x42, serial, payload.as_slice(), false, &mut encoded).unwrap();
            let decoded = decode_raw(encoded.as_slice()).unwrap();
            assert_eq!(decoded.ident, 0x42);
            assert_eq!(decoded.serial, serial);
            assert_eq!(decoded.data, payload);
        }
    }

    #[test]
    fn test_pdu_ping() {
        let mut encoded = Vec::new();
        Pdu::Ping(Ping {}).encode(&mut encoded, 0x40).unwrap();
        assert_eq!(&encoded, &[2, 0x40, 1]);
        assert_eq!(
            DecodedPdu {
                serial: 0x40,
                pdu: Pdu::Ping(Ping {})
            },
            Pdu::decode(encoded.as_slice()).unwrap()
        );
    }

    #[test]
    fn stream_decode() {
        let mut encoded = Vec::new();
        Pdu::Ping(Ping {}).encode(&mut encoded, 0x1).unwrap();
        Pdu::Pong(Pong {}).encode(&mut encoded, 0x2).unwrap();
        assert_eq!(encoded.len(), 6);

        let mut cursor = Cursor::new(encoded.as_slice());
        let mut read_buffer = Vec::new();

        assert_eq!(
            Pdu::try_read_and_decode(&mut cursor, &mut read_buffer).unwrap(),
            Some(DecodedPdu {
                serial: 1,
                pdu: Pdu::Ping(Ping {})
            })
        );
        assert_eq!(
            Pdu::try_read_and_decode(&mut cursor, &mut read_buffer).unwrap(),
            Some(DecodedPdu {
                serial: 2,
                pdu: Pdu::Pong(Pong {})
            })
        );
        let err = Pdu::try_read_and_decode(&mut cursor, &mut read_buffer).unwrap_err();
        assert_eq!(
            err.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn test_pdu_ping_base91() {
        let mut encoded = Vec::new();
        {
            let mut encoder = base91::Base91Encoder::new(&mut encoded);
            Pdu::Ping(Ping {}).encode(&mut encoder, 0x41).unwrap();
        }
        assert_eq!(&encoded, &[60, 67, 75, 65]);
        let decoded = base91::decode(&encoded);
        assert_eq!(
            DecodedPdu {
                serial: 0x41,
                pdu: Pdu::Ping(Ping {})
            },
            Pdu::decode(decoded.as_slice()).unwrap()
        );
    }

    #[test]
    fn test_pdu_pong() {
        let mut encoded = Vec::new();
        Pdu::Pong(Pong {}).encode(&mut encoded, 0x42).unwrap();
        assert_eq!(&encoded, &[2, 0x42, 2]);
        assert_eq!(
            DecodedPdu {
                serial: 0x42,
                pdu: Pdu::Pong(Pong {})
            },
            Pdu::decode(encoded.as_slice()).unwrap()
        );
    }

    #[test]
    fn test_bogus_pdu() {
        let mut encoded = Vec::new();
        encode_raw(0xdeadbeef, 0x42, b"hello", false, &mut encoded).unwrap();
        assert_eq!(
            DecodedPdu {
                serial: 0x42,
                pdu: Pdu::Invalid { ident: 0xdeadbeef }
            },
            Pdu::decode(encoded.as_slice()).unwrap()
        );
    }

    /// A declared payload length well above MAX_PDU_SIZE (256 MiB) so that the
    /// size guard fires before any large allocation or body read is attempted.
    const TEST_OVERSIZED_DATA_LEN: u64 = 4 * 1024 * 1024 * 1024; // 4 GiB

    /// Build a PDU frame whose header advertises `declared_data_len` bytes of
    /// payload, but whose body contains no actual bytes. Used to verify the
    /// size guard rejects oversized/corrupt frames *before* allocating.
    fn make_oversized_frame(declared_data_len: u64) -> Vec<u8> {
        // serial and ident are chosen so their leb128 encodings are a single
        // byte each; only the declared length is inflated.
        let serial: u64 = 0;
        let ident: u64 = 1;
        let len =
            declared_data_len + crate::framing::encoded_length(serial) as u64
                + crate::framing::encoded_length(ident) as u64;
        let mut buf = Vec::new();
        leb128::write::unsigned(&mut buf, len).unwrap();
        leb128::write::unsigned(&mut buf, serial).unwrap();
        leb128::write::unsigned(&mut buf, ident).unwrap();
        buf
    }

    #[test]
    fn oversized_pdu_is_rejected_without_allocating() {
        // A corrupt/malicious frame declaring ~4 GiB of payload but carrying no
        // body bytes must be rejected by the size guard rather than triggering a
        // multi-gigabyte allocation.
        let frame = make_oversized_frame(TEST_OVERSIZED_DATA_LEN);
        let err = decode_raw(frame.as_slice()).unwrap_err().to_string();
        assert!(
            err.contains("exceeds the maximum"),
            "expected the size guard to reject the oversized PDU, got: {}",
            err
        );
    }

    #[test]
    fn oversized_pdu_is_rejected_by_async_decoder() {
        let frame = make_oversized_frame(TEST_OVERSIZED_DATA_LEN);
        let mut cursor = smol::io::Cursor::new(frame);
        let err = smol::block_on(decode_raw_async(&mut cursor, None))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("exceeds the maximum"),
            "expected the async size guard to reject the oversized PDU, got: {}",
            err
        );
    }

    #[test]
    fn valid_pdu_still_decodes_after_size_guard() {
        // The size guard must not interfere with the normal decode path.
        let mut encoded = Vec::new();
        encode_raw(0x42, 0x10, b"small payload", false, &mut encoded).unwrap();
        let decoded = decode_raw(encoded.as_slice()).unwrap();
        assert_eq!(decoded.ident, 0x42);
        assert_eq!(decoded.serial, 0x10);
        assert_eq!(decoded.data, b"small payload");
    }

    fn sample_render_changes_response() -> GetPaneRenderChangesResponse {
        let mut user_vars = HashMap::new();
        user_vars.insert("foo".to_string(), "bar".to_string());
        user_vars.insert("baz".to_string(), "qux".to_string());
        GetPaneRenderChangesResponse {
            pane_id: 42,
            mouse_grabbed: false,
            cursor_position: StableCursorPosition::default(),
            dimensions: RenderableDimensions::default(),
            dirty_lines: vec![],
            title: "test pane".to_string(),
            working_dir: None,
            bonus_lines: SerializedLines::default(),
            input_serial: None,
            seqno: 7,
            user_vars,
        }
    }

    #[test]
    fn test_render_changes_user_vars_round_trip() {
        // user_vars must survive a PDU encode/decode round-trip through the
        // real mux wire format (varbincode). Regression for upstream #7610:
        // GetPaneRenderChangesResponse previously carried no user_vars field,
        // so a freshly-reattached ClientPane always started with an empty map
        // and lost the OSC 1337 SetUserVar state until the next change.
        let original = sample_render_changes_response();
        let expected_user_vars = original.user_vars.clone();
        let expected_pane_id = original.pane_id;
        let expected_seqno = original.seqno;
        let mut encoded = Vec::new();
        Pdu::GetPaneRenderChangesResponse(original)
            .encode(&mut encoded, 0x10)
            .unwrap();
        let decoded = Pdu::decode(encoded.as_slice()).unwrap();
        match decoded.pdu {
            Pdu::GetPaneRenderChangesResponse(resp) => {
                assert_eq!(resp.user_vars, expected_user_vars);
                assert_eq!(resp.pane_id, expected_pane_id);
                assert_eq!(resp.seqno, expected_seqno);
            }
            other => panic!(
                "expected GetPaneRenderChangesResponse, got {}",
                other.pdu_name()
            ),
        }
    }

    /// Mirrors the on-wire shape of `GetPaneRenderChangesResponse` as it was
    /// *before* upstream #7610 added the trailing `user_vars` field, so we can
    /// verify the backward-compatibility properties of that addition.
    #[derive(Serialize, Deserialize)]
    struct LegacyRenderChangesResponse {
        pane_id: PaneId,
        mouse_grabbed: bool,
        cursor_position: StableCursorPosition,
        dimensions: RenderableDimensions,
        dirty_lines: Vec<Range<StableRowIndex>>,
        title: String,
        working_dir: Option<SerdeUrl>,
        bonus_lines: SerializedLines,
        input_serial: Option<InputSerial>,
        seqno: SequenceNo,
    }

    #[test]
    fn test_render_changes_new_payload_decodes_as_legacy_shape() {
        // Adding the trailing `user_vars` field must not break a decoder built
        // from the older struct shape (new server -> old client): the legacy
        // decoder reads its fields and ignores the trailing user_vars bytes.
        // This is the backward-compatible direction for the varbincode wire
        // format, which serializes structs as an ordered field sequence.
        //
        // NOTE: the reverse direction (old server -> new client) is *not*
        // covered, because varbincode is a sequential format: when the new
        // struct tries to read the trailing user_vars field from a payload that
        // lacks it, it hits EOF ("failed to fill whole buffer") regardless of
        // `#[serde(default)]`. That attribute is therefore defensive / for
        // self-describing formats only; both ends of a mux connection must run
        // compatible versions.
        let original = sample_render_changes_response();
        let (data, is_compressed) = serialize(&original).unwrap();
        let legacy: LegacyRenderChangesResponse =
            deserialize(data.as_slice(), is_compressed).unwrap();
        assert_eq!(legacy.pane_id, original.pane_id);
        assert_eq!(legacy.mouse_grabbed, original.mouse_grabbed);
        assert_eq!(legacy.seqno, original.seqno);
        assert_eq!(legacy.title, original.title);
    }

    fn sample_spawn_v2(attach: bool) -> SpawnV2 {
        SpawnV2 {
            domain: config::keyassignment::SpawnTabDomain::DefaultDomain,
            window_id: None,
            command: None,
            command_dir: None,
            size: TerminalSize::default(),
            workspace: "default".to_string(),
            attach,
        }
    }

    #[test]
    fn test_spawn_v2_attach_round_trip() {
        // The `attach` field must survive a PDU encode/decode round-trip through
        // the real mux wire format (varbincode). Regression for upstream #7583:
        // SpawnV2 previously carried no `attach` field, so the --attach flag of
        // `wezterm start`/`wezterm connect` was silently dropped when the client
        // delegated spawn to an already-running GUI process via the SpawnV2 PDU,
        // causing the server to unconditionally spawn a new tab instead of
        // reusing an existing pane of the domain.
        for attach in [false, true] {
            let original = sample_spawn_v2(attach);
            let mut encoded = Vec::new();
            Pdu::SpawnV2(original).encode(&mut encoded, 0x10).unwrap();
            let decoded = Pdu::decode(encoded.as_slice()).unwrap();
            match decoded.pdu {
                Pdu::SpawnV2(resp) => {
                    assert_eq!(
                        resp.attach, attach,
                        "attach field did not survive round-trip"
                    );
                }
                other => panic!("expected SpawnV2, got {}", other.pdu_name()),
            }
        }

        // The two attach values must also produce distinct on-wire payloads, so
        // that a server cannot mistake one for the other.
        let mut enc_false = Vec::new();
        Pdu::SpawnV2(sample_spawn_v2(false))
            .encode(&mut enc_false, 0x10)
            .unwrap();
        let mut enc_true = Vec::new();
        Pdu::SpawnV2(sample_spawn_v2(true))
            .encode(&mut enc_true, 0x10)
            .unwrap();
        assert_ne!(
            enc_false, enc_true,
            "attach value must influence the wire encoding"
        );
    }

    /// End-to-end round-trip benchmark over a real Unix domain socket (the
    /// same transport `wezterm-mux-server`/`wezterm-client` actually use),
    /// comparing sending a `GetLinesResponse` (the PDU a scroll/redraw
    /// fetches) always-raw versus letting it auto-compress the way
    /// `Pdu::encode` does in production. Not a correctness test -- it just
    /// prints timings; run with `cargo test --release -p codec
    /// bench_socket_roundtrip -- --nocapture` to see the numbers.
    #[test]
    fn bench_socket_roundtrip_raw_vs_compressed() {
        use std::time::Instant;
        use wezterm_uds::{UnixListener, UnixStream};

        let attrs = termwiz::cell::CellAttributes::default();
        let mut lines = Vec::new();
        for i in 0..500u32 {
            let text = format!(
                "{:04} drwxr-xr-x  2 user user 4096 Jan  1 00:00 some/typical/path/example-{} \
                 -- repeated repeated repeated words words words",
                i,
                i % 37
            );
            lines.push((i as StableRowIndex, Line::from_text(&text, &attrs, 1, None)));
        }
        let response = GetLinesResponse {
            pane_id: 1,
            lines: SerializedLines::from(lines),
        };

        const ITERATIONS: u64 = 200;
        const GET_LINES_RESPONSE_IDENT: u64 = 23;

        fn run_variant(name: &str, response: &GetLinesResponse, force_raw: bool) {
            let sock_path = std::env::temp_dir().join(format!(
                "onlyterm-codec-bench-{}-{name}.sock",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&sock_path);
            let listener = UnixListener::bind(&sock_path).expect("bind unix socket");

            let server = std::thread::spawn(move || {
                let (mut stream, _addr) = listener.accept().expect("accept");
                for serial in 0..ITERATIONS {
                    let decoded = decode_raw(&mut stream).expect("server decode");
                    assert_eq!(decoded.serial, serial);
                    let _resp: GetLinesResponse =
                        deserialize(std::io::Cursor::new(&decoded.data), decoded.is_compressed)
                            .expect("server deserialize");
                    encode_raw(0, serial, b"", false, &mut stream).expect("server ack");
                }
            });

            let mut client = UnixStream::connect(&sock_path).expect("connect unix socket");
            let mut total_bytes = 0usize;
            let start = Instant::now();
            for serial in 0..ITERATIONS {
                let (data, is_compressed) = if force_raw {
                    let mut buf = Vec::new();
                    let mut encode = varbincode::Serializer::new(&mut buf);
                    response.serialize(&mut encode).expect("raw serialize");
                    (buf, false)
                } else {
                    serialize(response).expect("auto serialize")
                };
                total_bytes += data.len();
                encode_raw(
                    GET_LINES_RESPONSE_IDENT,
                    serial,
                    &data,
                    is_compressed,
                    &mut client,
                )
                .expect("client encode");
                let _ack = decode_raw(&mut client).expect("client decode ack");
            }
            let elapsed = start.elapsed();

            server.join().expect("server thread");
            let _ = std::fs::remove_file(&sock_path);

            println!(
                "{name}: {} round trips over a real unix socket in {:?} ({:?}/iter), \
                 avg {} bytes/msg",
                ITERATIONS,
                elapsed,
                elapsed / ITERATIONS as u32,
                total_bytes / ITERATIONS as usize,
            );
        }

        run_variant("raw", &response, true);
        run_variant("auto-compress", &response, false);
    }
}
