use crate::allocate::*;
use core::fmt::{Display, Error as FmtError, Formatter};

mod deletion;
mod frames;
mod placement;
mod transmit;

pub use self::deletion::*;
pub use self::frames::*;
pub use self::placement::*;
pub use self::transmit::*;
use self::transmit::{get, geti};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KittyImage {
    /// a='t'
    TransmitData {
        transmit: KittyImageTransmit,
        verbosity: KittyImageVerbosity,
    },
    /// a='T'
    TransmitDataAndDisplay {
        transmit: KittyImageTransmit,
        placement: KittyImagePlacement,
        verbosity: KittyImageVerbosity,
    },
    /// a='p'
    Display {
        image_id: Option<u32>,
        image_number: Option<u32>,
        placement: KittyImagePlacement,
        verbosity: KittyImageVerbosity,
    },
    /// a='d'
    Delete {
        what: KittyImageDelete,
        verbosity: KittyImageVerbosity,
    },
    /// a='q'
    Query { transmit: KittyImageTransmit },
    /// a='f'
    TransmitFrame {
        transmit: KittyImageTransmit,
        frame: KittyImageFrame,
        verbosity: KittyImageVerbosity,
    },
    /// a='c'
    ComposeFrame {
        frame: KittyImageFrameCompose,
        verbosity: KittyImageVerbosity,
    },
}

impl KittyImage {
    pub fn verbosity(&self) -> KittyImageVerbosity {
        match self {
            Self::TransmitData { verbosity, .. } => *verbosity,
            Self::Query { .. } => KittyImageVerbosity::Verbose,
            Self::TransmitDataAndDisplay { verbosity, .. } => *verbosity,
            Self::Display { verbosity, .. } => *verbosity,
            Self::Delete { verbosity, .. } => *verbosity,
            Self::TransmitFrame { verbosity, .. } => *verbosity,
            Self::ComposeFrame { verbosity, .. } => *verbosity,
        }
    }

    pub fn parse_apc(data: &[u8]) -> Option<Self> {
        if data.is_empty() || data[0] != b'G' {
            return None;
        }
        let mut keys_payload_iter = data[1..].splitn(2, |&d| d == b';');
        let keys = keys_payload_iter.next()?;
        let key_string = core::str::from_utf8(keys).ok()?;
        let mut keys: BTreeMap<&str, &str> = BTreeMap::new();
        for k_v in key_string.split(',') {
            let (k, v) = k_v.split_once('=')?;

            keys.insert(k, v);
        }

        let payload = keys_payload_iter.next().unwrap_or(b"");
        let action = get(&keys, "a").unwrap_or("t");
        let verbosity = KittyImageVerbosity::from_keys(&keys)?;
        match action {
            "t" => Some(Self::TransmitData {
                transmit: KittyImageTransmit::from_keys(&keys, payload)?,
                verbosity,
            }),
            "q" => Some(Self::Query {
                transmit: KittyImageTransmit::from_keys(&keys, payload)?,
            }),
            "T" => Some(Self::TransmitDataAndDisplay {
                transmit: KittyImageTransmit::from_keys(&keys, payload)?,
                placement: KittyImagePlacement::from_keys(&keys)?,
                verbosity,
            }),
            "p" => Some(Self::Display {
                placement: KittyImagePlacement::from_keys(&keys)?,
                image_id: geti(&keys, "i"),
                image_number: geti(&keys, "I"),
                verbosity,
            }),
            "d" => Some(Self::Delete {
                what: KittyImageDelete::from_keys(&keys)?,
                verbosity,
            }),
            "f" => Some(Self::TransmitFrame {
                transmit: KittyImageTransmit::from_keys(&keys, payload)?,
                frame: KittyImageFrame::from_keys(&keys)?,
                verbosity,
            }),
            "c" => Some(Self::ComposeFrame {
                frame: KittyImageFrameCompose::from_keys(&keys)?,
                verbosity,
            }),
            _ => None,
        }
    }

    fn to_keys(&self, keys: &mut BTreeMap<&'static str, String>) {
        match self {
            Self::TransmitData {
                transmit,
                verbosity,
            } => {
                // Implied: keys.insert("a", "t".to_string());
                verbosity.to_keys(keys);
                transmit.to_keys(keys);
            }
            Self::Query { transmit } => {
                keys.insert("a", "q".to_string());
                transmit.to_keys(keys);
            }
            Self::TransmitDataAndDisplay {
                transmit,
                verbosity,
                placement,
            } => {
                keys.insert("a", "Q".to_string());
                verbosity.to_keys(keys);
                placement.to_keys(keys);
                transmit.to_keys(keys);
            }
            Self::Display {
                image_id,
                image_number,
                placement,
                verbosity,
            } => {
                keys.insert("a", "p".to_string());
                verbosity.to_keys(keys);
                placement.to_keys(keys);
                if let Some(image_id) = image_id {
                    keys.insert("i", image_id.to_string());
                }
                if let Some(image_number) = image_number {
                    keys.insert("I", image_number.to_string());
                }
            }
            Self::Delete { what, verbosity } => {
                keys.insert("a", "d".to_string());
                verbosity.to_keys(keys);
                what.to_keys(keys);
            }
            Self::TransmitFrame {
                transmit,
                verbosity,
                frame,
            } => {
                keys.insert("a", "f".to_string());
                transmit.to_keys(keys);
                frame.to_keys(keys);
                verbosity.to_keys(keys);
            }
            Self::ComposeFrame { frame, verbosity } => {
                keys.insert("a", "c".to_string());
                frame.to_keys(keys);
                verbosity.to_keys(keys);
            }
        }
    }
}

impl Display for KittyImage {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        write!(f, "\x1b_G")?;
        let mut keys = BTreeMap::new();
        self.to_keys(&mut keys);
        let mut payload = None;
        let mut first = true;
        for (k, v) in keys {
            if k == "payload" {
                payload = Some(v);
            } else {
                if first {
                    first = false;
                } else {
                    write!(f, ",")?;
                }

                write!(f, "{}={}", k, v)?;
            }
        }

        if let Some(p) = payload {
            write!(f, ";{}", p)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test;
