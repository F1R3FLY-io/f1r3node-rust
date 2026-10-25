use super::error::LogPackFormatError;
use crate::rspace::persistence::canonical::ObjectRefV1;
use crate::rspace::persistence::durable::{INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1};

/// A nonzero immutable log-pack identifier.
///
/// Pack zero is reserved as the on-disk sentinel for "no predecessor" and
/// cannot name an address. The wrapper prevents that sentinel from escaping
/// into publisher or recovery arithmetic after parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackId(u64);

impl PackId {
    pub const GENESIS: Self = Self(1);

    pub fn new(value: u64) -> Result<Self, LogPackFormatError> {
        if value == 0 {
            return Err(LogPackFormatError::ZeroPackId);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 { self.0 }

    pub fn successor(self) -> Result<Self, LogPackFormatError> {
        let value = self
            .0
            .checked_add(1)
            .ok_or(LogPackFormatError::PackIdOverflow(self.0))?;
        Ok(Self(value))
    }
}

/// A nonzero transaction sequence in one store's authenticated log chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogPackSequence(u64);

impl LogPackSequence {
    pub const FIRST: Self = Self(1);

    pub fn new(value: u64) -> Result<Self, LogPackFormatError> {
        if value == 0 {
            return Err(LogPackFormatError::ZeroSequence);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 { self.0 }

    pub fn successor(self) -> Result<Self, LogPackFormatError> {
        let value = self
            .0
            .checked_add(1)
            .ok_or(LogPackFormatError::SequenceOverflow(self.0))?;
        Ok(Self(value))
    }
}

/// An exact record extent within an immutable log-pack segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogPackAddress {
    segment: PackId,
    offset: u64,
    length: u64,
}

impl LogPackAddress {
    pub fn new(segment: u64, offset: u64, length: u64) -> Result<Self, LogPackFormatError> {
        Self::from_pack(PackId::new(segment)?, offset, length)
    }

    pub fn from_pack(
        segment: PackId,
        offset: u64,
        length: u64,
    ) -> Result<Self, LogPackFormatError> {
        if length == 0 {
            return Err(LogPackFormatError::ZeroAddressLength);
        }
        offset
            .checked_add(length)
            .ok_or(LogPackFormatError::AddressOverflow { offset, length })?;
        Ok(Self {
            segment,
            offset,
            length,
        })
    }

    pub const fn pack_id(self) -> PackId { self.segment }

    pub const fn segment(self) -> u64 { self.segment.get() }

    pub const fn offset(self) -> u64 { self.offset }

    pub const fn length(self) -> u64 { self.length }

    pub fn end(self) -> u64 {
        self.offset
            .checked_add(self.length)
            .expect("validated log-pack address cannot overflow")
    }
}

/// Catalog-governing state authenticated by a terminal commit.
///
/// The active pack and durable pack tail are already determined by the
/// commit record's own address and extent. The catalog epoch is the two-slot
/// compare-and-swap coordinate and therefore is intentionally not part of the
/// transaction's semantic publication identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitPublicationV1 {
    first_live_pack: u64,
    index_generation: u64,
    durable_index_tail: u64,
    retention_manifest: ObjectRefV1,
    current_state_canonical_bytes: u64,
    retained_canonical_bytes: u64,
}

impl CommitPublicationV1 {
    pub fn new(
        first_live_pack: u64,
        index_generation: u64,
        durable_index_tail: u64,
        retention_manifest: ObjectRefV1,
        current_state_canonical_bytes: u64,
        retained_canonical_bytes: u64,
    ) -> Result<Self, LogPackFormatError> {
        if first_live_pack == 0 {
            return Err(LogPackFormatError::ZeroFirstLivePack);
        }
        if index_generation == 0 {
            return Err(LogPackFormatError::ZeroIndexGeneration);
        }
        if durable_index_tail < INDEX_HEADER_LEN_V1 {
            return Err(LogPackFormatError::IndexTailBeforeHeader(durable_index_tail));
        }
        if !(durable_index_tail - INDEX_HEADER_LEN_V1).is_multiple_of(INDEX_RECORD_LEN_V1) {
            return Err(LogPackFormatError::IndexTailUnaligned(durable_index_tail));
        }
        if current_state_canonical_bytes > retained_canonical_bytes {
            return Err(LogPackFormatError::ByteInventory {
                current_state: current_state_canonical_bytes,
                retained: retained_canonical_bytes,
            });
        }
        Ok(Self {
            first_live_pack,
            index_generation,
            durable_index_tail,
            retention_manifest,
            current_state_canonical_bytes,
            retained_canonical_bytes,
        })
    }

    pub const fn first_live_pack(self) -> u64 { self.first_live_pack }

    pub const fn index_generation(self) -> u64 { self.index_generation }

    pub const fn durable_index_tail(self) -> u64 { self.durable_index_tail }

    pub const fn retention_manifest(self) -> ObjectRefV1 { self.retention_manifest }

    pub const fn current_state_canonical_bytes(self) -> u64 { self.current_state_canonical_bytes }

    pub const fn retained_canonical_bytes(self) -> u64 { self.retained_canonical_bytes }
}

/// Total work/retention limits. No limit is a traversal- or term-depth cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogPackDecodeLimits {
    pub max_record_bytes: u64,
    pub max_mutation_bytes: u64,
    pub max_objects: u64,
    pub max_object_bytes: u64,
    pub max_single_object_bytes: u64,
    pub max_mutation_entries: u64,
    pub max_mutation_key_bytes: u64,
}

impl LogPackDecodeLimits {
    pub const UNBOUNDED: Self = Self {
        max_record_bytes: u64::MAX,
        max_mutation_bytes: u64::MAX,
        max_objects: u64::MAX,
        max_object_bytes: u64::MAX,
        max_single_object_bytes: u64::MAX,
        max_mutation_entries: u64::MAX,
        max_mutation_key_bytes: u64::MAX,
    };
}

impl Default for LogPackDecodeLimits {
    fn default() -> Self { Self::UNBOUNDED }
}
