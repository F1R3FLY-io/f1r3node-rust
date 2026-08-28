use std::fmt::{Display, Formatter};

use crate::rspace::persistence::{
    CanonicalFormatError, CommitPublicationV1, LogPackFormatError, PhysicalDigest,
    SemanticProfileId, StateVersionV1, StoreId,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DurableFormatError {
    Canonical(CanonicalFormatError),
    LogPack(LogPackFormatError),
    InvalidCatalogDomain,
    UnsupportedCatalogVersion(u64),
    CatalogChecksum {
        expected: u32,
        actual: u32,
    },
    CatalogChecksumOutOfRange(u64),
    NonzeroCatalogReserved,
    ZeroCatalogEpoch,
    CatalogEpochOverflow(u64),
    CatalogEpochConflict(u64),
    CatalogEpochGap {
        lower: u64,
        upper: u64,
    },
    CatalogStoreMismatch {
        lower: StoreId,
        upper: StoreId,
    },
    CatalogProfileMismatch {
        lower: SemanticProfileId,
        upper: SemanticProfileId,
    },
    CatalogCommitSegment {
        commit: u64,
        active: u64,
    },
    CatalogCommitLength {
        incremental: u64,
        checkpoint: u64,
        actual: u64,
    },
    CatalogCommitBeforePackHeader {
        offset: u64,
        header_len: u64,
    },
    CatalogPackTail {
        commit_end: u64,
        durable_tail: u64,
    },
    CatalogPackRange {
        first: u64,
        active: u64,
    },
    ZeroCatalogIndexGeneration,
    CatalogIndexTail(u64),
    CatalogByteInventory {
        current_state: u64,
        retained: u64,
    },
    CatalogCurrentStateBytes {
        minimum: u64,
        actual: u64,
    },
    UnsupportedCatalogFlags(u64),
    NoValidCatalog {
        first: Option<Box<DurableFormatError>>,
        second: Option<Box<DurableFormatError>>,
    },
    CatalogCompareAndSwap {
        expected: StateVersionV1,
        actual: StateVersionV1,
    },
    CatalogSuccessorEpoch {
        expected: u64,
        actual: u64,
    },
    CatalogSuccessorStore {
        expected: StoreId,
        actual: StoreId,
    },
    CatalogSuccessorProfile {
        expected: SemanticProfileId,
        actual: SemanticProfileId,
    },
    CatalogCommitStore {
        catalog: StoreId,
        commit: StoreId,
    },
    CatalogCommitSequence {
        catalog: u64,
        commit: u64,
    },
    CatalogCommitDigest {
        catalog: PhysicalDigest,
        commit: PhysicalDigest,
    },
    CatalogCommitState {
        catalog: StateVersionV1,
        commit: StateVersionV1,
    },
    CatalogCommitProfile {
        catalog: SemanticProfileId,
        commit: SemanticProfileId,
    },
    CatalogCommitPublication {
        catalog: CommitPublicationV1,
        commit: CommitPublicationV1,
    },
}

impl From<CanonicalFormatError> for DurableFormatError {
    fn from(error: CanonicalFormatError) -> Self { Self::Canonical(error) }
}

impl From<LogPackFormatError> for DurableFormatError {
    fn from(error: LogPackFormatError) -> Self { Self::LogPack(error) }
}

impl Display for DurableFormatError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canonical(error) => Display::fmt(error, formatter),
            Self::LogPack(error) => Display::fmt(error, formatter),
            Self::InvalidCatalogDomain => formatter.write_str("invalid RSpace catalog domain"),
            Self::UnsupportedCatalogVersion(version) => {
                write!(formatter, "unsupported RSpace catalog version {version}")
            }
            Self::CatalogChecksum { expected, actual } => write!(
                formatter,
                "catalog CRC32C mismatch: expected {expected:#010x}, got {actual:#010x}"
            ),
            Self::CatalogChecksumOutOfRange(encoded) => {
                write!(formatter, "catalog checksum {encoded:#018x} is not a zero-extended CRC32C")
            }
            Self::NonzeroCatalogReserved => {
                formatter.write_str("catalog reserved bytes must be zero")
            }
            Self::ZeroCatalogEpoch => formatter.write_str("catalog epoch zero is reserved"),
            Self::CatalogEpochOverflow(epoch) => {
                write!(formatter, "catalog epoch {epoch} has no successor")
            }
            Self::CatalogEpochConflict(epoch) => {
                write!(formatter, "two unequal catalog slots claim the same epoch {epoch}")
            }
            Self::CatalogEpochGap { lower, upper } => {
                write!(formatter, "catalog epochs are not adjacent: lower {lower}, upper {upper}")
            }
            Self::CatalogStoreMismatch { lower, upper } => {
                write!(formatter, "catalog slots name different stores: {lower} and {upper}")
            }
            Self::CatalogProfileMismatch { lower, upper } => write!(
                formatter,
                "catalog slots name different semantic profiles: {lower:?} and {upper:?}"
            ),
            Self::CatalogCommitSegment { commit, active } => {
                write!(formatter, "catalog commit segment {commit} is not active pack {active}")
            }
            Self::CatalogCommitLength {
                incremental,
                checkpoint,
                actual,
            } => write!(
                formatter,
                "catalog commit length {actual} is neither V1 incremental length {incremental} \
                 nor V1 checkpoint length {checkpoint}"
            ),
            Self::CatalogCommitBeforePackHeader { offset, header_len } => write!(
                formatter,
                "catalog commit offset {offset} lies inside the {header_len}-byte pack header"
            ),
            Self::CatalogPackTail {
                commit_end,
                durable_tail,
            } => write!(
                formatter,
                "catalog commit ends at {commit_end}, but durable pack tail is {durable_tail}"
            ),
            Self::CatalogPackRange { first, active } => write!(
                formatter,
                "catalog live pack range is invalid: first {first}, active {active}"
            ),
            Self::ZeroCatalogIndexGeneration => {
                formatter.write_str("catalog index generation zero is reserved")
            }
            Self::CatalogIndexTail(tail) => {
                write!(formatter, "catalog durable index tail {tail} precedes its header")
            }
            Self::CatalogByteInventory {
                current_state,
                retained,
            } => write!(
                formatter,
                "catalog current-state canonical bytes {current_state} exceed retained canonical \
                 bytes {retained}"
            ),
            Self::CatalogCurrentStateBytes { minimum, actual } => write!(
                formatter,
                "catalog current-state canonical bytes {actual} are below its manifest length \
                 {minimum}"
            ),
            Self::UnsupportedCatalogFlags(flags) => {
                write!(formatter, "unsupported catalog flags {flags:#x}")
            }
            Self::NoValidCatalog { first, second } => write!(
                formatter,
                "catalog has no valid slot (first: {first:?}; second: {second:?})"
            ),
            Self::CatalogCompareAndSwap { expected, actual } => {
                write!(formatter, "catalog state changed: expected {expected:?}, actual {actual:?}")
            }
            Self::CatalogSuccessorEpoch { expected, actual } => write!(
                formatter,
                "catalog successor epoch {actual} does not immediately follow {expected}"
            ),
            Self::CatalogSuccessorStore { expected, actual } => {
                write!(formatter, "catalog successor store {actual} does not preserve {expected}")
            }
            Self::CatalogSuccessorProfile { expected, actual } => write!(
                formatter,
                "catalog successor profile {actual:?} does not preserve {expected:?}"
            ),
            Self::CatalogCommitStore { catalog, commit } => write!(
                formatter,
                "catalog store {catalog} does not match terminal commit store {commit}"
            ),
            Self::CatalogCommitSequence { catalog, commit } => write!(
                formatter,
                "catalog commit sequence {catalog} does not match terminal commit sequence \
                 {commit}"
            ),
            Self::CatalogCommitDigest { catalog, commit } => write!(
                formatter,
                "catalog commit digest {catalog} does not match terminal commit digest {commit}"
            ),
            Self::CatalogCommitState { catalog, commit } => write!(
                formatter,
                "catalog state {catalog:?} does not match terminal commit target {commit:?}"
            ),
            Self::CatalogCommitProfile { catalog, commit } => write!(
                formatter,
                "catalog semantic profile {catalog:?} does not match terminal commit profile \
                 {commit:?}"
            ),
            Self::CatalogCommitPublication { catalog, commit } => write!(
                formatter,
                "catalog pack publication {catalog:?} does not match terminal commit publication \
                 {commit:?}"
            ),
        }
    }
}

impl std::error::Error for DurableFormatError {}
