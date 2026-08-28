use std::fmt::{Display, Formatter};

use super::types::LogPackAddress;
use crate::rspace::persistence::checkpoint::GenerationCheckpointErrorV1;
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
    ZeroPackId,
    PackIdOverflow(u64),
    ZeroSequence,
    SequenceOverflow(u64),
    InvalidDomain,
    UnsupportedVersion(u64),
    UnexpectedRecordKind {
        expected: u64,
        actual: u64,
    },
    UnsupportedPrepareKind(u64),
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
    ChecksumOutOfRange {
        field: &'static str,
        encoded: u64,
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
    AddressBeforePackHeader {
        field: &'static str,
        offset: u64,
        header_len: u64,
    },
    ZeroAddressLength,
    AddressOverflow {
        offset: u64,
        length: u64,
    },
    ZeroFirstLivePack,
    ZeroIndexGeneration,
    IndexTailBeforeHeader(u64),
    IndexTailUnaligned(u64),
    ByteInventory {
        current_state: u64,
        retained: u64,
    },
    CurrentStateBytes {
        minimum: u64,
        actual: u64,
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
    ObjectPlanCount {
        expected: u64,
        actual: u64,
    },
    ObjectPlanInventory,
    UnsupportedObjectDisposition(u64),
    ObjectReference {
        declared: ObjectRefV1,
        actual: ObjectRefV1,
    },
    CheckpointInventoryDigest {
        expected: [u8; 32],
        actual: [u8; 32],
    },
    CheckpointDesignatedObjectMissing {
        role: &'static str,
        reference: ObjectRefV1,
    },
    CheckpointDesignatedObjectKind {
        role: &'static str,
        reference: ObjectRefV1,
        expected: crate::rspace::persistence::CanonicalObjectKind,
        actual: crate::rspace::persistence::CanonicalObjectKind,
    },
    Checkpoint(GenerationCheckpointErrorV1),
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

impl From<GenerationCheckpointErrorV1> for LogPackFormatError {
    fn from(error: GenerationCheckpointErrorV1) -> Self { Self::Checkpoint(error) }
}

impl Display for LogPackFormatError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canonical(error) => Display::fmt(error, formatter),
            Self::Mutation(error) => Display::fmt(error, formatter),
            Self::ZeroPackId => formatter.write_str("log-pack ID zero is reserved"),
            Self::PackIdOverflow(value) => {
                write!(formatter, "log-pack ID {value} has no successor")
            }
            Self::ZeroSequence => formatter.write_str("log-pack sequence zero is reserved"),
            Self::SequenceOverflow(value) => {
                write!(formatter, "log-pack sequence {value} has no successor")
            }
            Self::InvalidDomain => formatter.write_str("invalid log-pack record domain"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported log-pack version {version}")
            }
            Self::UnexpectedRecordKind { expected, actual } => {
                write!(formatter, "unexpected log-pack record kind {actual}; expected {expected}")
            }
            Self::UnsupportedPrepareKind(kind) => {
                write!(formatter, "unsupported log-pack prepare kind {kind}")
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
            Self::ChecksumOutOfRange { field, encoded } => write!(
                formatter,
                "log-pack {field} checksum {encoded:#018x} is not a zero-extended CRC32C"
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
            Self::AddressBeforePackHeader {
                field,
                offset,
                header_len,
            } => write!(
                formatter,
                "log-pack {field} offset {offset} lies inside the {header_len}-byte pack header"
            ),
            Self::ZeroAddressLength => {
                formatter.write_str("log-pack address length must be nonzero")
            }
            Self::AddressOverflow { offset, length } => write!(
                formatter,
                "log-pack address offset {offset} plus length {length} overflows u64"
            ),
            Self::ZeroFirstLivePack => {
                formatter.write_str("log-pack first-live pack zero is reserved")
            }
            Self::ZeroIndexGeneration => {
                formatter.write_str("log-pack index generation zero is reserved")
            }
            Self::IndexTailBeforeHeader(tail) => {
                write!(formatter, "log-pack durable index tail {tail} precedes its header")
            }
            Self::IndexTailUnaligned(tail) => write!(
                formatter,
                "log-pack durable index tail {tail} is not aligned to a complete index record"
            ),
            Self::ByteInventory {
                current_state,
                retained,
            } => write!(
                formatter,
                "log-pack current-state canonical bytes {current_state} exceed retained canonical \
                 bytes {retained}"
            ),
            Self::CurrentStateBytes { minimum, actual } => write!(
                formatter,
                "log-pack current-state canonical bytes {actual} are below target manifest length \
                 {minimum}"
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
            Self::ObjectPlanCount { expected, actual } => write!(
                formatter,
                "log-pack object plan has {actual} records for {expected} sealed objects"
            ),
            Self::ObjectPlanInventory => formatter
                .write_str("log-pack object plan inventories do not match the sealed transaction"),
            Self::UnsupportedObjectDisposition(kind) => {
                write!(formatter, "unsupported log-pack object disposition {kind}")
            }
            Self::ObjectReference { declared, actual } => write!(
                formatter,
                "canonical object does not match declared reference {declared:?}; actual \
                 {actual:?}"
            ),
            Self::CheckpointInventoryDigest { expected, actual } => write!(
                formatter,
                "checkpoint object inventory digest mismatch: expected {expected:02x?}, got \
                 {actual:02x?}"
            ),
            Self::CheckpointDesignatedObjectMissing { role, reference } => write!(
                formatter,
                "checkpoint {role} object {reference:?} is absent from its DATA inventory"
            ),
            Self::CheckpointDesignatedObjectKind {
                role,
                reference,
                expected,
                actual,
            } => write!(
                formatter,
                "checkpoint {role} object {reference:?} has kind {}; expected {}",
                actual.name(),
                expected.name()
            ),
            Self::Checkpoint(error) => Display::fmt(error, formatter),
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

impl std::error::Error for LogPackFormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Canonical(error) => Some(error),
            Self::Mutation(error) => Some(error),
            Self::Checkpoint(error) => Some(error),
            _ => None,
        }
    }
}
