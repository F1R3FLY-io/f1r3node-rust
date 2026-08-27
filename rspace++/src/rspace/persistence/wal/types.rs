use super::error::LogPackFormatError;

/// A nonzero transaction sequence in one store's authenticated log chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogPackSequence(u64);

impl LogPackSequence {
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
    segment: u64,
    offset: u64,
    length: u64,
}

impl LogPackAddress {
    pub fn new(segment: u64, offset: u64, length: u64) -> Result<Self, LogPackFormatError> {
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

    pub const fn segment(self) -> u64 { self.segment }

    pub const fn offset(self) -> u64 { self.offset }

    pub const fn length(self) -> u64 { self.length }

    pub fn end(self) -> u64 {
        self.offset
            .checked_add(self.length)
            .expect("validated log-pack address cannot overflow")
    }
}

/// Total work/retention limits. No limit is a traversal- or term-depth cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogPackDecodeLimits {
    pub max_record_bytes: u64,
    pub max_mutation_bytes: u64,
    pub max_objects: u64,
    pub max_object_bytes: u64,
    pub max_mutation_entries: u64,
    pub max_mutation_key_bytes: u64,
}

impl LogPackDecodeLimits {
    pub const UNBOUNDED: Self = Self {
        max_record_bytes: u64::MAX,
        max_mutation_bytes: u64::MAX,
        max_objects: u64::MAX,
        max_object_bytes: u64::MAX,
        max_mutation_entries: u64::MAX,
        max_mutation_key_bytes: u64::MAX,
    };
}

impl Default for LogPackDecodeLimits {
    fn default() -> Self { Self::UNBOUNDED }
}
