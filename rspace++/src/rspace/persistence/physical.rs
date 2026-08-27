//! Fixed-width physical-store identities.

use std::fmt::{Display, Formatter};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

pub const PHYSICAL_DIGEST_LEN: usize = 32;
pub const STORE_ID_LEN: usize = 16;

/// A durable-store identity, never a path, file descriptor, or process-local
/// pointer. Copying files between stores cannot silently splice their logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoreId([u8; STORE_ID_LEN]);

impl StoreId {
    pub const fn from_array(bytes: [u8; STORE_ID_LEN]) -> Self { Self(bytes) }

    pub fn random() -> Self { Self(*uuid::Uuid::new_v4().as_bytes()) }

    pub const fn as_bytes(&self) -> &[u8; STORE_ID_LEN] { &self.0 }
}

impl Display for StoreId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

/// BLAKE2b-256 identity of an authenticated physical record image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysicalDigest([u8; PHYSICAL_DIGEST_LEN]);

impl PhysicalDigest {
    pub const ZERO: Self = Self([0; PHYSICAL_DIGEST_LEN]);

    pub const fn from_array(bytes: [u8; PHYSICAL_DIGEST_LEN]) -> Self { Self(bytes) }

    pub const fn as_bytes(&self) -> &[u8; PHYSICAL_DIGEST_LEN] { &self.0 }

    pub fn of_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(bytes);
        let hash = hasher.finalize();
        let mut digest = [0; PHYSICAL_DIGEST_LEN];
        digest.copy_from_slice(&hash);
        Self(digest)
    }
}

impl Display for PhysicalDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}
