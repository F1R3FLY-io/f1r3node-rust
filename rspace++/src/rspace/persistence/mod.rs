//! Canonical persistent-DAG substrate for RSpace.
//!
//! The live RSpace remains PathMap-backed. This module defines the portable,
//! content-addressed logical image used by the incremental persistence layer;
//! it deliberately does not encode PathMap's private nodes, ACT offsets,
//! process-local caches, or swizzled pointers.

mod canonical;
mod checkpoint;
mod checksum;
mod closure;
#[expect(
    clippy::result_large_err,
    reason = "largest Err: DurableRSpaceTransactionError (752+ B), from journal publication"
)]
mod durable;
mod manifest;
mod mutation;
mod object;
mod physical;
mod retention;
mod rewrite;
mod semantic;
mod store;
mod transaction;
mod wal;

pub use canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, ObjectDigest, ObjectRefV1,
};
pub use closure::ClosureError;
#[cfg(test)]
pub(crate) use closure::{
    IncrementalClosureLimits, StructuralClosureReport, StructuralValidationLimits,
    verify_structural_closure, verify_structural_closure_with_limits,
};
#[cfg(test)]
pub(crate) use closure::{
    RetainedUnionClosureLimitsV1, StructuralClosureLedgerV1, verify_incremental_closure,
    verify_structural_closure_in_ledger_with_limits,
};
pub use durable::{
    CATALOG_IMAGE_LEN, CatalogAuthorityTokenV1, CatalogSlotV1, DurableLimits, ObjectIndexRecordV1,
    PackTerminalIdentityV1,
};
#[cfg(test)]
pub(crate) use durable::{
    CATALOG_SLOT_LEN, CatalogPackStateV1, CatalogSelectionV1, DurableFormatError,
    DurableLimitConfig, DurableLimitError, DurableLimitKind, INDEX_HEADER_LEN_V1,
    INDEX_RECORD_LEN_V1, IndexFormatError, IndexGenerationV1, ObjectIndexFileViewV1,
    ObjectIndexHeaderV1, ObjectLocationV1, PACK_HEADER_LEN_V1, PackFormatError, PackHeaderV1,
};
pub(crate) use durable::{DurableAuthorityViewV1, DurableObjectReadErrorV1};
#[cfg(test)]
pub(crate) use manifest::{
    AcceleratorArtifact, AcceleratorArtifactKind, AcceleratorManifestV1, AcceleratorManifestView,
    LegacyHistoryRoot, SemanticStateManifestV1,
};
pub use manifest::{
    SemanticNamespace, SemanticProfileId, SemanticRoots, SemanticStateManifestView,
};
pub use mutation::{ContentTransition, MutationError, RSpaceMutationBatchV1, StateVersionV1};
#[cfg(test)]
pub(crate) use mutation::{CountTransition, MutationBatchViewV1, MutationDecodeLimits};
#[cfg(test)]
pub(crate) use object::{
    BranchKind, CanonicalBlobView, CanonicalDagNode, CanonicalDagNodeView, CanonicalObjectView,
    DagChild,
};
pub use object::{CanonicalBlob, CanonicalBlobKind, CanonicalObjectKind, EndpointValue};
pub use physical::{PhysicalDigest, StoreId};
pub use retention::{
    GenerationLeaseError, GenerationLeaseKeyV1, GenerationLeaseRegistryV1, GenerationLeaseV1,
};
#[cfg(test)]
pub(crate) use retention::{RetainedStateV1, RetentionManifestV1, RetentionManifestView};
#[cfg(test)]
pub(crate) use rewrite::{
    DagRewriteLimits, PreparedDagRewrite, RewriteError, rewrite_semantic_roots,
    rewrite_semantic_roots_with_limits,
};
#[cfg(test)]
pub(crate) use rewrite::{
    ExactPath, ExactPathArena, InheritedBoundaryKind, InheritedObjectRef, RewriteProvenance,
};
#[cfg(test)]
pub(crate) use rewrite::{preflight_sensitive_copy_bytes, reset_preflight_sensitive_copy_bytes};
#[cfg(test)]
pub(crate) use semantic::semantic_profile_seal;
#[cfg(test)]
pub(crate) use semantic::verify_rspace_semantics_after_structural;
#[cfg(test)]
pub(crate) use semantic::{
    IncrementalSemanticLimits, SemanticValidationLimits, SemanticValidationReport,
    verify_rspace_semantics,
};
pub use semantic::{RSpaceSemanticProfile, SemanticProfile};
#[cfg(test)]
pub(crate) use store::{
    CanonicalObjectStore, MutableObjectStore, ObjectAdmission, StagingObjectStore,
    StagingStoreError, StagingStoreLimits,
};
pub use store::{ImmutableObjectStore, LayeredObject};
#[cfg(test)]
pub(crate) use transaction::SealedTransitionV1;
pub use transaction::{
    DagTransactionError, DagTransactionLimits, OpenDagTransaction, SealedDagTransaction,
    SealedObject, VerifiedDagState,
};
pub use wal::{
    COMMIT_DISCRIMINATOR_LEN_V1, CommitPublicationV1, CommitRecordKindV1, LogPackAddress,
    LogPackDecodeLimits, LogPackFormatError, LogPackObjectDispositionV1, LogPackSequence,
    PREPARE_DISCRIMINATOR_LEN_V1, PackId, PrepareRecordIdentityV1, PrepareRecordKindV1,
    TRANSACTION_COMMIT_RECORD_LEN_V1, TRANSACTION_PREPARE_HEADER_LEN_V1, TransactionCommitViewV1,
    decode_commit_kind_v1, decode_prepare_kind_v1, preflight_prepare_record_v1,
};
#[cfg(test)]
pub(crate) use wal::{
    CommittedObjectComparison, CommittedObjectLookup, LogPackObjectPlanError, LogPackObjectPlanV1,
    LogPackObjectView, LogPackWriteError, PreparedLogPackTransactionV1, TransactionCommitRecordV1,
    TransactionPrepareRecordViewV1, preflight_prepare_record_len_v1,
};
pub(crate) use wal::{LogPackObjectHeaderV1, OBJECT_RECORD_HEADER_LEN_V1};

#[cfg(test)]
#[path = "../../../tests/persistence/mod.rs"]
mod tests;
