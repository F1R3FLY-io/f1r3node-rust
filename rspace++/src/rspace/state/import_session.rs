use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use shared::rust::store::key_value_store::KvStoreError;

use crate::rspace::errors::{AuthorityBackendMismatch, RadixTreeError, RootError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{ColdLeafAddress, ColdLeafStoreError};
use crate::rspace::shared::trie_exporter::{NodePath, StateExportError, Value};

/// Determines how a sealed state import changes the verified-root catalog.
///
/// Forward-horizon synchronization publishes several historical roots without
/// moving the repository's current-root pointer.  Initial LFS synchronization
/// publishes one verified root and activates it in the same atomic catalog
/// transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateImportPublicationMode {
    PreserveCurrent,
    Activate(Blake2b256Hash),
}

/// Immutable authority describing the roots an import session must prove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateImportSpec {
    roots: Vec<Blake2b256Hash>,
    publication_mode: StateImportPublicationMode,
}

impl StateImportSpec {
    pub fn new(
        roots: impl IntoIterator<Item = Blake2b256Hash>,
        publication_mode: StateImportPublicationMode,
    ) -> Result<Self, StateImportError> {
        let roots = roots.into_iter().collect::<BTreeSet<_>>();
        if roots.is_empty() {
            return Err(StateImportError::NoTargetRoots);
        }
        for root in &roots {
            validate_digest_width(root, "target root")?;
        }
        if let StateImportPublicationMode::Activate(target) = &publication_mode {
            validate_digest_width(target, "activation target")?;
            if !roots.contains(target) {
                return Err(StateImportError::ActivationTargetOutsideImport(target.clone()));
            }
        }
        Ok(Self {
            roots: roots.into_iter().collect(),
            publication_mode,
        })
    }

    pub fn roots(&self) -> &[Blake2b256Hash] { &self.roots }

    pub fn publication_mode(&self) -> &StateImportPublicationMode { &self.publication_mode }
}

/// One transport page supplied to an affine state-import session.
///
/// The page owns its bytes so validation can finish before any object becomes
/// durable.  The session records neither the transport cursor nor its objects
/// until all history hashes, typed leaves, record boundaries, and the expected
/// export projection have been checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateImportPage {
    pub start_path: NodePath,
    /// Cursor advertised by the peer for the next page.  The importer derives
    /// the authoritative cursor from the authenticated radix traversal and
    /// rejects a response when the two differ.
    pub advertised_next_path: NodePath,
    pub skip: i32,
    pub take: i32,
    pub history_items: Vec<(Blake2b256Hash, Value)>,
    pub data_items: Vec<(Blake2b256Hash, Value)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateImportPageReceipt {
    /// Authenticated continuation cursor. Terminal pages are normalized to
    /// repeat the page's start cursor; this field is never an empty sentinel.
    pub next_path: NodePath,
    pub terminal: bool,
    pub history_objects_staged: usize,
    pub cold_leaves_staged: usize,
}

/// Result of consuming a sealed session's one-shot publication capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateImportPublication {
    pub verified_roots: Vec<Blake2b256Hash>,
    /// Present only when this session atomically activated a root.
    pub activation: Option<StateImportActivation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateImportActivation {
    pub root: Blake2b256Hash,
    pub generation: u64,
}

/// Monotone witness for the authority-effect boundary of one publication call.
///
/// The caller creates a fresh witness for each affine publication attempt. A
/// [`SealedStateImport`] implementation records the witness immediately after
/// its root-catalog transaction becomes authoritative, before constructing or
/// encoding a receipt. This lets an FFI boundary distinguish a pre-publication
/// unwind from an unwind after authority has already changed. The witness does
/// not claim filesystem durability; persistent backends establish that
/// separately.
#[derive(Debug, Default)]
pub struct StateImportPublicationWitness {
    authoritative: AtomicBool,
}

impl StateImportPublicationWitness {
    pub const fn new() -> Self {
        Self {
            authoritative: AtomicBool::new(false),
        }
    }

    /// Record that this call crossed its authoritative publication point.
    ///
    /// The transition is monotone and intentionally non-panicking so it can be
    /// placed directly after a storage commit.
    pub fn record_authoritative_publication(&self) {
        self.authoritative.store(true, Ordering::Release);
    }

    pub fn authoritative_publication_observed(&self) -> bool {
        self.authoritative.load(Ordering::Acquire)
    }
}

/// Mutable but unpublished import capability.
///
/// `seal(self: Box<Self>)` consumes the only staging capability.  Code holding
/// the resulting [`SealedStateImport`] can no longer add pages, which closes
/// the validation/publication time-of-check/time-of-use gap at the type
/// boundary.
pub trait StateImportSession: Send {
    fn spec(&self) -> &StateImportSpec;

    fn validate_and_stage_page(
        &mut self,
        page: StateImportPage,
    ) -> Result<StateImportPageReceipt, StateImportError>;

    fn seal(self: Box<Self>) -> Result<Box<dyn SealedStateImport>, StateImportError>;
}

/// Fully verified, one-shot root-publication capability.
pub trait SealedStateImport: Send {
    fn spec(&self) -> &StateImportSpec;

    /// Publish the verified roots and mark `witness` at the authority-effect
    /// boundary before doing any fallible receipt construction.
    fn publish(
        self: Box<Self>,
        witness: &StateImportPublicationWitness,
    ) -> Result<StateImportPublication, StateImportError>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum StateImportError {
    NoTargetRoots,
    ActivationTargetOutsideImport(Blake2b256Hash),
    InvalidDigestWidth {
        field: &'static str,
        available: usize,
    },
    InvalidPagination(String),
    InvalidStartPath(String),
    PageRootOutsideImport(Blake2b256Hash),
    UnexpectedStartPath(NodePath),
    IncompletePagination(Vec<NodePath>),
    AdvertisedCursorMismatch {
        advertised: NodePath,
        authenticated: NodePath,
    },
    InvalidHistoryKeyPath {
        available: usize,
    },
    InvalidHistoryPageSize {
        take: i32,
        received: usize,
    },
    AccountingOverflow(&'static str),
    InternalInvariant(String),
    DuplicateHistoryObject(Blake2b256Hash),
    DuplicateColdLeaf(ColdLeafAddress),
    HistoryDigestMismatch {
        expected: Blake2b256Hash,
        actual: Blake2b256Hash,
    },
    DataDigestMismatch {
        expected: Blake2b256Hash,
        actual: Blake2b256Hash,
    },
    HistoryProjectionMismatch,
    DataProjectionMismatch,
    MissingHistoryObject(Blake2b256Hash),
    MissingColdLeaf(ColdLeafAddress),
    MalformedRecord {
        address: ColdLeafAddress,
        index: usize,
        error: String,
    },
    ExportProjection(StateExportError),
    RadixTree(RadixTreeError),
    ColdLeaf(ColdLeafStoreError),
    Store(KvStoreError),
    Root(RootError),
    BackendMismatch(AuthorityBackendMismatch),
}

impl std::fmt::Display for StateImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTargetRoots => write!(f, "state import has no target roots"),
            Self::ActivationTargetOutsideImport(root) => {
                write!(f, "state import activation target {root} is not one of the target roots")
            }
            Self::InvalidDigestWidth { field, available } => {
                write!(f, "state import {field} must contain 32 bytes, found {available}")
            }
            Self::InvalidPagination(error) => {
                write!(f, "state import pagination is invalid: {error}")
            }
            Self::InvalidStartPath(error) => {
                write!(f, "state import start path is invalid: {error}")
            }
            Self::PageRootOutsideImport(root) => {
                write!(f, "state import page root {root} is outside the session authority")
            }
            Self::UnexpectedStartPath(path) => write!(
                f,
                "state import page starts at an unrequested or already-consumed {}-component \
                 cursor",
                path.len()
            ),
            Self::IncompletePagination(paths) => write!(
                f,
                "state import cannot seal while {} authenticated cursor(s) remain",
                paths.len()
            ),
            Self::AdvertisedCursorMismatch {
                advertised,
                authenticated,
            } => write!(
                f,
                "state import peer cursor does not match authenticated traversal: advertised {} \
                 components, authenticated {} components",
                advertised.len(),
                authenticated.len()
            ),
            Self::InvalidHistoryKeyPath { available } => {
                write!(f, "state import history key must contain 33 bytes, found {available}")
            }
            Self::InvalidHistoryPageSize { take, received } => write!(
                f,
                "state import page requested at most {take} history objects but received \
                 {received}"
            ),
            Self::AccountingOverflow(field) => {
                write!(f, "state import {field} accounting overflowed u64")
            }
            Self::InternalInvariant(error) => {
                write!(f, "state import internal invariant failed: {error}")
            }
            Self::DuplicateHistoryObject(hash) => {
                write!(f, "state import page repeats history object {hash}")
            }
            Self::DuplicateColdLeaf(address) => write!(
                f,
                "state import page repeats cold leaf {:?} at {}",
                address.kind(),
                address.digest()
            ),
            Self::HistoryDigestMismatch { expected, actual } => write!(
                f,
                "state import history digest mismatch: expected {expected}, computed {actual}"
            ),
            Self::DataDigestMismatch { expected, actual } => write!(
                f,
                "state import cold-leaf digest mismatch: expected {expected}, computed {actual}"
            ),
            Self::HistoryProjectionMismatch => write!(
                f,
                "state import history objects do not equal the requested radix projection"
            ),
            Self::DataProjectionMismatch => {
                write!(f, "state import cold leaves do not equal the requested radix projection")
            }
            Self::MissingHistoryObject(hash) => {
                write!(f, "state import closure is missing history object {hash}")
            }
            Self::MissingColdLeaf(address) => write!(
                f,
                "state import closure is missing cold leaf {:?} at {}",
                address.kind(),
                address.digest()
            ),
            Self::MalformedRecord {
                address,
                index,
                error,
            } => write!(
                f,
                "state import cold leaf {:?} at {} has malformed record {index}: {error}",
                address.kind(),
                address.digest()
            ),
            Self::ExportProjection(error) => {
                write!(f, "state import authenticated projection failed: {error}")
            }
            Self::RadixTree(error) => write!(f, "state import radix-tree error: {error}"),
            Self::ColdLeaf(error) => write!(f, "state import cold-leaf error: {error}"),
            Self::Store(error) => write!(f, "state import storage error: {error}"),
            Self::Root(error) => write!(f, "state import root-catalog error: {error}"),
            Self::BackendMismatch(error) => {
                write!(f, "state import authority backend mismatch: {error}")
            }
        }
    }
}

impl std::error::Error for StateImportError {}

impl From<RadixTreeError> for StateImportError {
    fn from(error: RadixTreeError) -> Self { Self::RadixTree(error) }
}

impl From<ColdLeafStoreError> for StateImportError {
    fn from(error: ColdLeafStoreError) -> Self { Self::ColdLeaf(error) }
}

impl From<KvStoreError> for StateImportError {
    fn from(error: KvStoreError) -> Self { Self::Store(error) }
}

impl From<RootError> for StateImportError {
    fn from(error: RootError) -> Self { Self::Root(error) }
}

impl From<StateExportError> for StateImportError {
    fn from(error: StateExportError) -> Self { Self::ExportProjection(error) }
}

impl From<AuthorityBackendMismatch> for StateImportError {
    fn from(error: AuthorityBackendMismatch) -> Self { Self::BackendMismatch(error) }
}

fn validate_digest_width(
    digest: &Blake2b256Hash,
    field: &'static str,
) -> Result<(), StateImportError> {
    if digest.0.len() == 32 {
        Ok(())
    } else {
        Err(StateImportError::InvalidDigestWidth {
            field,
            available: digest.0.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(seed: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![seed; 32]) }

    #[test]
    fn import_spec_canonicalizes_roots_and_requires_activation_membership() {
        let first = hash(1);
        let second = hash(2);
        let spec = StateImportSpec::new(
            [second.clone(), first.clone(), second],
            StateImportPublicationMode::Activate(first.clone()),
        )
        .expect("valid import authority");
        assert_eq!(spec.roots(), &[first.clone(), hash(2)]);
        assert_eq!(spec.publication_mode(), &StateImportPublicationMode::Activate(first));

        assert_eq!(
            StateImportSpec::new([hash(3)], StateImportPublicationMode::Activate(hash(4))),
            Err(StateImportError::ActivationTargetOutsideImport(hash(4)))
        );
    }

    #[test]
    fn import_spec_rejects_empty_or_non_digest_authority() {
        assert_eq!(
            StateImportSpec::new([], StateImportPublicationMode::PreserveCurrent),
            Err(StateImportError::NoTargetRoots)
        );
        assert_eq!(
            StateImportSpec::new(
                [Blake2b256Hash::from_bytes(vec![0; 31])],
                StateImportPublicationMode::PreserveCurrent
            ),
            Err(StateImportError::InvalidDigestWidth {
                field: "target root",
                available: 31,
            })
        );
    }

    #[test]
    fn publication_witness_is_false_until_marked_and_then_monotone() {
        let witness = StateImportPublicationWitness::new();
        assert!(!witness.authoritative_publication_observed());
        witness.record_authoritative_publication();
        assert!(witness.authoritative_publication_observed());
        witness.record_authoritative_publication();
        assert!(witness.authoritative_publication_observed());
    }
}
