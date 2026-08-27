use std::fmt::{Display, Formatter};

use super::types::LogPackAddress;
use crate::rspace::persistence::{
    CanonicalFormatError, MutationError, ObjectDigest, ObjectRefV1, PhysicalDigest, StateVersionV1,
    StoreId,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogPackWriteError<E> {
    Sink(E),
    Invariant(LogPackFormatError),
}

impl<E: Display> Display for LogPackWriteError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sink(error) => write!(formatter, "log-pack sink failed: {error}"),
            Self::Invariant(error) => {
                write!(formatter, "log-pack writer invariant failed: {error}")
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for LogPackWriteError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sink(error) => Some(error),
            Self::Invariant(error) => Some(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogPackFormatError {
    Canonical(CanonicalFormatError),
    Mutation(MutationError),
    ZeroSequence,
    SequenceOverflow(u64),
    InvalidDomain,
    UnsupportedVersion(u64),
    LengthMismatch {
        declared: u64,
        actual: u64,
    },
    HeaderChecksum {
        expected: u32,
        actual: u32,
    },
    RecordChecksum {
        expected: u32,
        actual: u32,
    },
    RecordDigest {
        expected: PhysicalDigest,
        actual: PhysicalDigest,
    },
    Store {
        expected: StoreId,
        actual: StoreId,
    },
    Sequence {
        expected: u64,
        actual: u64,
    },
    AddressLength {
        address: LogPackAddress,
        declared: u64,
    },
    ZeroAddressLength,
    AddressOverflow {
        offset: u64,
        length: u64,
    },
    MutationDigest {
        expected: ObjectDigest,
        actual: ObjectDigest,
    },
    MutationReceipt {
        expected: StateVersionV1,
        actual: StateVersionV1,
    },
    ReceiptMismatch,
    ObjectsNotStrict {
        previous: ObjectDigest,
        current: ObjectDigest,
    },
    ObjectReference {
        declared: ObjectRefV1,
        actual: ObjectRefV1,
    },
    ResourceLimit {
        resource: &'static str,
        limit: u64,
        attempted: u64,
    },
}

impl From<CanonicalFormatError> for LogPackFormatError {
    fn from(error: CanonicalFormatError) -> Self { Self::Canonical(error) }
}

impl From<MutationError> for LogPackFormatError {
    fn from(error: MutationError) -> Self { Self::Mutation(error) }
}

impl Display for LogPackFormatError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canonical(error) => Display::fmt(error, formatter),
            Self::Mutation(error) => Display::fmt(error, formatter),
            Self::ZeroSequence => formatter.write_str("log-pack sequence zero is reserved"),
            Self::SequenceOverflow(value) => {
                write!(formatter, "log-pack sequence {value} has no successor")
            }
            Self::InvalidDomain => formatter.write_str("invalid log-pack record domain"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported log-pack version {version}")
            }
            Self::LengthMismatch { declared, actual } => {
                write!(formatter, "log-pack length mismatch: declared {declared}, actual {actual}")
            }
            Self::HeaderChecksum { expected, actual } => write!(
                formatter,
                "log-pack header CRC32C mismatch: expected {expected:#010x}, got {actual:#010x}"
            ),
            Self::RecordChecksum { expected, actual } => write!(
                formatter,
                "log-pack record CRC32C mismatch: expected {expected:#010x}, got {actual:#010x}"
            ),
            Self::RecordDigest { expected, actual } => {
                write!(formatter, "log-pack digest mismatch: expected {expected}, got {actual}")
            }
            Self::Store { expected, actual } => {
                write!(formatter, "log-pack store mismatch: expected {expected}, got {actual}")
            }
            Self::Sequence { expected, actual } => {
                write!(formatter, "log-pack sequence mismatch: expected {expected}, got {actual}")
            }
            Self::AddressLength { address, declared } => write!(
                formatter,
                "log-pack address {address:?} does not match declared length {declared}"
            ),
            Self::ZeroAddressLength => {
                formatter.write_str("log-pack address length must be nonzero")
            }
            Self::AddressOverflow { offset, length } => write!(
                formatter,
                "log-pack address offset {offset} plus length {length} overflows u64"
            ),
            Self::MutationDigest { expected, actual } => {
                write!(formatter, "mutation digest mismatch: expected {expected}, got {actual}")
            }
            Self::MutationReceipt { expected, actual } => write!(
                formatter,
                "mutation state {actual:?} does not match receipt state {expected:?}"
            ),
            Self::ReceiptMismatch => {
                formatter.write_str("prepare and commit records bind different transition receipts")
            }
            Self::ObjectsNotStrict { previous, current } => write!(
                formatter,
                "log-pack objects are not strictly digest ordered: {previous} then {current}"
            ),
            Self::ObjectReference { declared, actual } => write!(
                formatter,
                "canonical object does not match declared reference {declared:?}; actual \
                 {actual:?}"
            ),
            Self::ResourceLimit {
                resource,
                limit,
                attempted,
            } => write!(
                formatter,
                "log-pack {resource} budget exceeded: limit {limit}, attempted {attempted}"
            ),
        }
    }
}

impl std::error::Error for LogPackFormatError {}
