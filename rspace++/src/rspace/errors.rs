/* RSpace Error */

use shared::rust::store::key_value_store::KvStoreError;

#[derive(Debug, Clone, PartialEq)]
pub enum RSpaceError {
    ProduceCommitDenied,
    ProduceCommitProtocolViolation,
    InterpreterError(String),
    HistoryError(HistoryError),
    RadixTreeError(RadixTreeError),
    KvStoreError(KvStoreError),
    BugFoundError(String),
    ReportingError(String),
}

impl std::fmt::Display for RSpaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            RSpaceError::ProduceCommitDenied => write!(f, "Produce commit denied"),
            RSpaceError::ProduceCommitProtocolViolation => {
                write!(f, "Produce commit guard violated its invocation contract")
            }
            RSpaceError::InterpreterError(err) => write!(f, "Interpreter Error: {}", err),
            RSpaceError::HistoryError(err) => write!(f, "History Error: {}", err),
            RSpaceError::RadixTreeError(err) => write!(f, "Radix Tree Error: {}", err),
            RSpaceError::KvStoreError(err) => write!(f, "Key Value Store Error: {}", err),
            RSpaceError::BugFoundError(err) => write!(f, "RSpace Bug Found Error: {}", err),
            RSpaceError::ReportingError(err) => write!(f, "Reporting Error: {}", err),
        }
    }
}

impl From<RadixTreeError> for RSpaceError {
    fn from(error: RadixTreeError) -> Self { RSpaceError::RadixTreeError(error) }
}

impl From<KvStoreError> for RSpaceError {
    fn from(error: KvStoreError) -> Self { RSpaceError::KvStoreError(error) }
}

impl From<HistoryError> for RSpaceError {
    fn from(error: HistoryError) -> Self { RSpaceError::HistoryError(error) }
}

/* History Error */

#[derive(Debug, Clone, PartialEq)]
pub enum HistoryError {
    ActionError(String),
    /// A cold-store leaf could not be decoded.
    ///
    /// ⚠ DELIBERATELY NOT `ActionError`. The cold-store decode path
    /// (`serializers::decode_datums` / `decode_continuations` / `decode_joins`,
    /// bounded by `ColdStoreDecode`) is the one place where the node reads
    /// bytes it did not necessarily write: `rspace_importer` commits peer
    /// bytes to LMDB without ever deep-decoding them. Before this variant
    /// existed, a malformed or too-deep leaf had no failure channel at all
    /// — the decoders `.expect(..)`d, so the read aborted the PROCESS,
    /// permanently and on every restart. Folding it into `ActionError`
    /// would make "the state we synced is not decodable" indistinguishable
    /// from "the action we just performed was invalid", which are different
    /// problems with different operator responses.
    DecodeError(String),
    RadixTreeError(RadixTreeError),
    KvStoreError(KvStoreError),
    RootError(RootError),
    MergeError(String),
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            HistoryError::ActionError(err) => write!(f, "Actions Error: {}", err),
            HistoryError::DecodeError(err) => write!(f, "Cold Store Decode Error: {}", err),
            HistoryError::RadixTreeError(err) => write!(f, "Radix Tree Error: {}", err),
            HistoryError::KvStoreError(err) => write!(f, "Key Value Store Error: {}", err),
            HistoryError::RootError(err) => write!(f, "Root Error: {}", err),
            HistoryError::MergeError(err) => write!(f, "Merge Error: {}", err),
        }
    }
}

impl From<RadixTreeError> for HistoryError {
    fn from(error: RadixTreeError) -> Self { HistoryError::RadixTreeError(error) }
}

impl From<KvStoreError> for HistoryError {
    fn from(error: KvStoreError) -> Self { HistoryError::KvStoreError(error) }
}

impl From<RootError> for HistoryError {
    fn from(error: RootError) -> Self { HistoryError::RootError(error) }
}

/* History Repository Error */

#[derive(Debug)]
pub enum HistoryRepositoryError {
    RootError(RootError),
    HistoryError(HistoryError),
}

impl std::fmt::Display for HistoryRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            HistoryRepositoryError::RootError(err) => write!(f, "Root Error: {}", err),
            HistoryRepositoryError::HistoryError(err) => write!(f, "History Error: {}", err),
        }
    }
}

impl From<RootError> for HistoryRepositoryError {
    fn from(error: RootError) -> Self { HistoryRepositoryError::RootError(error) }
}

impl From<HistoryError> for HistoryRepositoryError {
    fn from(error: HistoryError) -> Self { HistoryRepositoryError::HistoryError(error) }
}

/* Radix Tree Error */

#[derive(Debug, Clone, PartialEq)]
pub enum RadixTreeError {
    KeyNotFound(String),
    KvStoreError(KvStoreError),
    PrefixError(String),
    CollisionError(String),
    SerializationError(String),
}

impl std::fmt::Display for RadixTreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            RadixTreeError::KeyNotFound(err) => write!(f, "Key Not Found Error: {}", err),
            RadixTreeError::KvStoreError(err) => write!(f, "Key Value Store Error: {}", err),
            RadixTreeError::PrefixError(err) => write!(f, "Prefix Error: {}", err),
            RadixTreeError::CollisionError(err) => write!(f, "Collision Error: {}", err),
            RadixTreeError::SerializationError(err) => write!(f, "Serialization Error: {}", err),
        }
    }
}

impl From<KvStoreError> for RadixTreeError {
    fn from(error: KvStoreError) -> Self { RadixTreeError::KvStoreError(error) }
}

/* Root Error */

#[derive(Debug, Clone, PartialEq)]
pub enum RootError {
    KvStoreError(String),
    UnknownRootError(String),
}

impl std::fmt::Display for RootError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            RootError::KvStoreError(err) => write!(f, "Key Value Store Error: {}", err),
            RootError::UnknownRootError(err) => write!(f, "Unknown root: {}", err),
        }
    }
}

impl From<KvStoreError> for RootError {
    fn from(error: KvStoreError) -> Self { RootError::KvStoreError(error.to_string()) }
}
