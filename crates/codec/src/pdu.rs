//! Defines the `Pdu` enum, its encode/decode machinery, and the
//! version negotiation constant for the mux protocol.
//! Rather than rely solely on serde to serialize and deserialize an
//! enum, we encode the enum variants with a version/identifier tag
//! for ourselves.  This will make it a little easier to manage
//! client and server instances that are built from different versions
//! of this code; in this way the client and server can more gracefully
//! manage unknown enum variants.
use crate::framing::{decode_raw, decode_raw_async, deserialize, encode_raw, encode_raw_async, serialize};
use crate::messages::*;
use anyhow::{bail, Context as _, Error};
use mux::pane::PaneId;
use smol::io::AsyncWriteExt;
use smol::prelude::*;
use std::io::Cursor;

#[derive(Debug, PartialEq)]
pub struct DecodedPdu {
    pub serial: u64,
    pub pdu: Pdu,
}

macro_rules! pdu {
    ($( $name:ident:$vers:expr),* $(,)?) => {
        // `Pdu` is the wire-protocol enum for the mux client/server RPC
        // codec; each variant tuple-wraps a distinct PDU payload type
        // by value. Boxing the largest variant would change the public
        // `Pdu` type (every match arm and every construction site across
        // the client and server crates would need updating to box/unbox),
        // and would also change the wire-serialization shape produced by
        // `serde`/`varbincode` for that variant. Not worth the churn just
        // to shrink the enum's stack footprint.
        #[allow(clippy::large_enum_variant)]
        #[derive(PartialEq, Debug)]
        pub enum Pdu {
            Invalid{ident: u64},
            $(
                $name($name)
            ,)*
        }

        impl Pdu {
            pub fn encode<W: std::io::Write>(&self, w: W, serial: u64) -> Result<(), Error> {
                match self {
                    Pdu::Invalid{..} => bail!("attempted to serialize Pdu::Invalid"),
                    $(
                        Pdu::$name(s) => {
                            let (data, is_compressed) = serialize(s)?;
                            let encoded_size = encode_raw($vers, serial, &data, is_compressed, w)?;
                            log::debug!("encode {} size={encoded_size}", stringify!($name));
                            metrics::histogram!("pdu.size", "pdu" => stringify!($name)).record(encoded_size as f64);
                            metrics::histogram!("pdu.size.rate", "pdu" => stringify!($name)).record(encoded_size as f64);
                            Ok(())
                        }
                    ,)*
                }
            }

            pub async fn encode_async<W: Unpin + AsyncWriteExt>(&self, w: &mut W, serial: u64) -> Result<(), Error> {
                match self {
                    Pdu::Invalid{..} => bail!("attempted to serialize Pdu::Invalid"),
                    $(
                        Pdu::$name(s) => {
                            let (data, is_compressed) = serialize(s)?;
                            let encoded_size = encode_raw_async($vers, serial, &data, is_compressed, w).await?;
                            log::debug!("encode_async {} size={encoded_size}", stringify!($name));
                            metrics::histogram!("pdu.size", "pdu" => stringify!($name)).record(encoded_size as f64);
                            metrics::histogram!("pdu.size.rate", "pdu" => stringify!($name)).record(encoded_size as f64);
                            Ok(())
                        }
                    ,)*
                }
            }

            pub fn pdu_name(&self) -> &'static str {
                match self {
                    Pdu::Invalid{..} => "Invalid",
                    $(
                        Pdu::$name(_) => {
                            stringify!($name)
                        }
                    ,)*
                }
            }

            pub fn decode<R: std::io::Read>(r: R) -> Result<DecodedPdu, Error> {
                let decoded = decode_raw(r).context("decoding a PDU")?;
                match decoded.ident {
                    $(
                        $vers => {
                            metrics::histogram!("pdu.size", "pdu" => stringify!($name)).record(decoded.data.len() as f64);
                            metrics::histogram!("pdu.size.rate", "pdu" => stringify!($name)).record(decoded.data.len() as f64);
                            Ok(DecodedPdu {
                                serial: decoded.serial,
                                pdu: Pdu::$name(deserialize(decoded.data.as_slice(), decoded.is_compressed)?)
                            })
                        }
                    ,)*
                    _ => {
                        metrics::histogram!("pdu.size", "pdu" => "??").record(decoded.data.len() as f64);
                        metrics::histogram!("pdu.size.rate", "pdu" => "??").record(decoded.data.len() as f64);
                        Ok(DecodedPdu {
                            serial: decoded.serial,
                            pdu: Pdu::Invalid{ident:decoded.ident}
                        })
                    }
                }
            }

            pub async fn decode_async<R>(r: &mut R, max_serial: Option<u64>) -> Result<DecodedPdu, Error>
                where R: std::marker::Unpin,
                      R: AsyncRead,
                      R: std::fmt::Debug
            {
                let decoded = decode_raw_async(r, max_serial).await.context("decoding a PDU")?;
                match decoded.ident {
                    $(
                        $vers => {
                            metrics::histogram!("pdu.size", "pdu" => stringify!($name)).record(decoded.data.len() as f64);
                            Ok(DecodedPdu {
                                serial: decoded.serial,
                                pdu: Pdu::$name(deserialize(decoded.data.as_slice(), decoded.is_compressed)?)
                            })
                        }
                    ,)*
                    _ => {
                        metrics::histogram!("pdu.size", "pdu" => "??").record(decoded.data.len() as f64);
                        Ok(DecodedPdu {
                            serial: decoded.serial,
                            pdu: Pdu::Invalid{ident:decoded.ident}
                        })
                    }
                }
            }
        }
    }
}

/// The overall version of the codec.
/// This must be bumped when backwards incompatible changes
/// are made to the types and protocol.
pub const CODEC_VERSION: usize = 46;

// Defines the Pdu enum.
// Each struct has an explicit identifying number.
// This allows removal of obsolete structs,
// and defining newer structs as the protocol evolves.
pdu! {
    ErrorResponse: 0,
    Ping: 1,
    Pong: 2,
    ListPanes: 3,
    ListPanesResponse: 4,
    SpawnResponse: 8,
    WriteToPane: 9,
    UnitResponse: 10,
    SendKeyDown: 11,
    SendMouseEvent: 12,
    SendPaste: 13,
    Resize: 14,
    SetClipboard: 20,
    GetLines: 22,
    GetLinesResponse: 23,
    GetPaneRenderChanges: 24,
    GetPaneRenderChangesResponse: 25,
    GetCodecVersion: 26,
    GetCodecVersionResponse: 27,
    GetTlsCreds: 28,
    GetTlsCredsResponse: 29,
    LivenessResponse: 30,
    SearchScrollbackRequest: 31,
    SearchScrollbackResponse: 32,
    SetPaneZoomed: 33,
    SplitPane: 34,
    KillPane: 35,
    SpawnV2: 36,
    PaneRemoved: 37,
    SetPalette: 38,
    NotifyAlert: 39,
    SetClientId: 40,
    GetClientList: 41,
    GetClientListResponse: 42,
    SetWindowWorkspace: 43,
    WindowWorkspaceChanged: 44,
    SetFocusedPane: 45,
    GetImageCell: 46,
    GetImageCellResponse: 47,
    MovePaneToNewTab: 48,
    MovePaneToNewTabResponse: 49,
    ActivatePaneDirection: 50,
    GetPaneRenderableDimensions: 51,
    GetPaneRenderableDimensionsResponse: 52,
    PaneFocused: 53,
    TabReflowed: 54,
    TabAddedToWindow: 55,
    TabTitleChanged: 56,
    WindowTitleChanged: 57,
    RenameWorkspace: 58,
    EraseScrollbackRequest: 59,
    GetPaneDirection: 60,
    GetPaneDirectionResponse: 61,
    AdjustPaneSize: 62,
    RotatePanes: 63,
    SwapActivePaneWithIndex: 64,
}

impl Pdu {
    /// Returns true if this type of Pdu represents action taken
    /// directly by a user, rather than background traffic on
    /// a live connection
    pub fn is_user_input(&self) -> bool {
        matches!(
            self,
            Self::WriteToPane(_)
                | Self::SendKeyDown(_)
                | Self::SendMouseEvent(_)
                | Self::SendPaste(_)
                | Self::Resize(_)
                | Self::SetClipboard(_)
                | Self::SetPaneZoomed(_)
                | Self::SpawnV2(_)
        )
    }

    pub fn stream_decode(buffer: &mut Vec<u8>) -> anyhow::Result<Option<DecodedPdu>> {
        let mut cursor = Cursor::new(buffer.as_slice());
        match Self::decode(&mut cursor) {
            Ok(decoded) => {
                let consumed = cursor.position() as usize;
                let remain = buffer.len() - consumed;
                // Remove `consumed` bytes from the start of the vec. `copy_within`
                // uses memmove semantics and correctly handles the overlapping
                // shift here; the previous ptr::copy_nonoverlapping was in fact
                // unsound whenever consumed < remain (overlapping src/dest).
                buffer.copy_within(consumed.., 0);
                buffer.truncate(remain);
                Ok(Some(decoded))
            }
            Err(err) => {
                if let Some(ioerr) = err.root_cause().downcast_ref::<std::io::Error>() {
                    match ioerr.kind() {
                        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::WouldBlock => {
                            return Ok(None);
                        }
                        _ => {}
                    }
                } else {
                    log::error!("not an ioerror in stream_decode: {:?}", err);
                }
                Err(err)
            }
        }
    }

    pub fn try_read_and_decode<R: std::io::Read>(
        r: &mut R,
        buffer: &mut Vec<u8>,
    ) -> anyhow::Result<Option<DecodedPdu>> {
        loop {
            if let Some(decoded) =
                Self::stream_decode(buffer).context("stream_decode of buffer for PDU")?
            {
                return Ok(Some(decoded));
            }

            let mut buf = [0u8; 4096];
            let size = match r.read(&mut buf) {
                Ok(size) => size,
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::WouldBlock {
                        return Ok(None);
                    }
                    return Err(err.into());
                }
            };
            if size == 0 {
                return Err(
                    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "End Of File").into(),
                );
            }

            buffer.extend_from_slice(&buf[0..size]);
        }
    }

    pub fn pane_id(&self) -> Option<PaneId> {
        match self {
            Pdu::GetPaneRenderChangesResponse(GetPaneRenderChangesResponse { pane_id, .. })
            | Pdu::SetPalette(SetPalette { pane_id, .. })
            | Pdu::NotifyAlert(NotifyAlert { pane_id, .. })
            | Pdu::SetClipboard(SetClipboard { pane_id, .. })
            | Pdu::PaneFocused(PaneFocused { pane_id })
            | Pdu::PaneRemoved(PaneRemoved { pane_id }) => Some(*pane_id),
            _ => None,
        }
    }
}
