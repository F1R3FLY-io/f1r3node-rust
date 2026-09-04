//! Authenticated, restart-safe superseded-generation reclamation records.

use super::catalog::CatalogSlotV1;
use crate::rspace::persistence::checksum::{crc32c, decode_crc32c, encode_crc32c};
use crate::rspace::persistence::retention::GenerationLeaseKeyV1;

const MAGIC: &[u8; 8] = b"RSPRECL1";
const VERSION: u64 = 1;
const BODY_LEN: usize = 8 + 8 + (8 * 8);
pub(crate) const RECLAMATION_RECORD_LEN_V1: usize = BODY_LEN + 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReclamationCandidateV1 {
    old: GenerationLeaseKeyV1,
    successor: GenerationLeaseKeyV1,
}

impl ReclamationCandidateV1 {
    pub(crate) fn new(
        old: GenerationLeaseKeyV1,
        successor: GenerationLeaseKeyV1,
    ) -> Result<Self, &'static str> {
        if old.epoch() >= successor.epoch() || old.active_pack() >= successor.first_live_pack() {
            return Err("reclamation successor does not strictly supersede old generation");
        }
        Ok(Self { old, successor })
    }

    pub(crate) const fn old(self) -> GenerationLeaseKeyV1 { self.old }
    pub(crate) const fn successor(self) -> GenerationLeaseKeyV1 { self.successor }

    pub(crate) fn from_catalogs(
        old: CatalogSlotV1,
        successor: CatalogSlotV1,
    ) -> Result<Self, &'static str> {
        Self::new(
            GenerationLeaseKeyV1::from_catalog(old),
            GenerationLeaseKeyV1::from_catalog(successor),
        )
    }

    pub(crate) fn encode(self) -> [u8; RECLAMATION_RECORD_LEN_V1] {
        let mut bytes = [0u8; RECLAMATION_RECORD_LEN_V1];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..16].copy_from_slice(&VERSION.to_le_bytes());
        let fields = [
            self.old.epoch(),
            self.old.first_live_pack(),
            self.old.active_pack(),
            self.old.index_generation(),
            self.successor.epoch(),
            self.successor.first_live_pack(),
            self.successor.active_pack(),
            self.successor.index_generation(),
        ];
        for (index, value) in fields.into_iter().enumerate() {
            let start = 16 + index * 8;
            bytes[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
        let checksum = crc32c(&bytes[..BODY_LEN]);
        bytes[BODY_LEN..].copy_from_slice(&encode_crc32c(checksum));
        bytes
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != RECLAMATION_RECORD_LEN_V1 || &bytes[..8] != MAGIC {
            return Err("invalid reclamation record header");
        }
        if u64::from_le_bytes(bytes[8..16].try_into().unwrap()) != VERSION {
            return Err("unsupported reclamation record version");
        }
        let stored = decode_crc32c(bytes[BODY_LEN..].try_into().unwrap())
            .map_err(|_| "reclamation record checksum is out of range")?;
        if stored != crc32c(&bytes[..BODY_LEN]) {
            return Err("reclamation record checksum mismatch");
        }
        let mut fields = [0u64; 8];
        for (index, field) in fields.iter_mut().enumerate() {
            let start = 16 + index * 8;
            *field = u64::from_le_bytes(bytes[start..start + 8].try_into().unwrap());
        }
        Self::new(
            GenerationLeaseKeyV1::new(fields[0], fields[1], fields[2], fields[3]),
            GenerationLeaseKeyV1::new(fields[4], fields[5], fields[6], fields[7]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(epoch: u64, first: u64, active: u64, index: u64) -> GenerationLeaseKeyV1 {
        GenerationLeaseKeyV1::new(epoch, first, active, index)
    }

    #[test]
    fn record_round_trips_and_rejects_tampering() {
        let candidate = ReclamationCandidateV1::new(key(1, 2, 3, 4), key(2, 4, 5, 6)).unwrap();
        let bytes = candidate.encode();
        assert_eq!(ReclamationCandidateV1::decode(&bytes), Ok(candidate));
        let mut altered = bytes;
        altered[24] ^= 1;
        assert_eq!(
            ReclamationCandidateV1::decode(&altered),
            Err("reclamation record checksum mismatch")
        );
    }

    #[test]
    fn record_rejects_non_superseding_generation() {
        assert_eq!(
            ReclamationCandidateV1::new(key(2, 2, 3, 4), key(2, 4, 5, 6)),
            Err("reclamation successor does not strictly supersede old generation")
        );
    }
}
