#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use sha2::Digest;
use uuid::Uuid;

/// Identifies data within the store.
/// This is an (unspecified) hash of the content
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ContentId([u8; 32]);

impl ContentId {
    pub fn for_bytes(bytes: &[u8]) -> Self {
        let mut hasher = sha2::Sha256::new();
        hasher.update(bytes);
        Self(hasher.finalize().into())
    }

    pub fn as_hash_bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl std::fmt::Display for ContentId {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "sha256-")?;
        for byte in &self.0 {
            write!(fmt, "{byte:x}")?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for ContentId {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "ContentId({self})")
    }
}

/// Represents an individual lease
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct LeaseId {
    uuid: Uuid,
    pid: u32,
}

impl std::fmt::Display for LeaseId {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "lease:pid={},{}", self.pid, self.uuid.hyphenated())
    }
}

impl Default for LeaseId {
    fn default() -> Self {
        Self::new()
    }
}

impl LeaseId {
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        let pid = std::process::id();
        Self { uuid, pid }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }
}
