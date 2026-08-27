//! Physical tear detection for append-only persistence records.
//!
//! CRC32C is deliberately not an identity or authenticity primitive. Logical
//! identity remains BLAKE2b-256 over canonical bytes; this checksum only lets
//! recovery reject physically incomplete or damaged records cheaply.

use crc::{CRC_32_ISCSI, Crc, Digest, Table};

const CRC32C: Crc<u32, Table<16>> = Crc::<u32, Table<16>>::new(&CRC_32_ISCSI);

pub(crate) const CHECKSUM_FIELD_LEN: usize = 8;

pub(crate) struct Crc32c(Digest<'static, u32, Table<16>>);

impl Crc32c {
    pub(crate) fn new() -> Self { Self(CRC32C.digest()) }

    pub(crate) fn update(&mut self, bytes: &[u8]) { self.0.update(bytes); }

    pub(crate) fn finish(self) -> u32 { self.0.finalize() }
}

pub(crate) fn crc32c(bytes: &[u8]) -> u32 { CRC32C.checksum(bytes) }

/// Encode CRC32C in the persistence protocol's checked `u64` field.
///
/// CRC32C is intrinsically 32 bits. The high half is reserved and readers
/// reject it when nonzero, so a future checksum regime cannot be confused
/// with V1 merely because its low 32 bits happen to agree.
pub(crate) fn encode_crc32c(checksum: u32) -> [u8; CHECKSUM_FIELD_LEN] {
    u64::from(checksum).to_le_bytes()
}

pub(crate) fn decode_crc32c(bytes: [u8; CHECKSUM_FIELD_LEN]) -> Result<u32, u64> {
    let encoded = u64::from_le_bytes(bytes);
    u32::try_from(encoded).map_err(|_| encoded)
}

#[cfg(test)]
mod tests {
    use super::{Crc32c, crc32c, decode_crc32c, encode_crc32c};

    #[test]
    fn crc32c_matches_the_iscsi_check_value_incrementally() {
        let mut digest = Crc32c::new();
        digest.update(b"1234");
        digest.update(b"56789");
        assert_eq!(digest.finish(), 0xe306_9283);
        assert_eq!(crc32c(b"123456789"), 0xe306_9283);
    }

    #[test]
    fn persisted_crc32c_is_a_checked_u64() {
        assert_eq!(decode_crc32c(encode_crc32c(0xe306_9283)), Ok(0xe306_9283));
        assert_eq!(decode_crc32c((1u64 << 32).to_le_bytes()), Err(1u64 << 32));
    }
}
