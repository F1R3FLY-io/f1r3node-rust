//! Canonical persistent-DAG substrate for RSpace.
//!
//! The live RSpace remains PathMap-backed. This module defines the portable,
//! content-addressed logical image used by the incremental persistence layer;
//! it deliberately does not encode PathMap's private nodes, ACT offsets,
//! process-local caches, or swizzled pointers.

mod canonical;
mod checksum;
mod closure;
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
#[cfg(test)]
pub(crate) use closure::verify_incremental_closure;
pub use closure::{
    ClosureError, IncrementalClosureLimits, StructuralClosureReport, StructuralValidationLimits,
    verify_structural_closure, verify_structural_closure_with_limits,
};
pub use durable::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CatalogPackStateV1, CatalogSelectionV1, CatalogSlotV1,
    DurableFormatError, INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexFormatError,
    IndexGenerationV1, ObjectIndexFileViewV1, ObjectIndexHeaderV1, ObjectIndexRecordIter,
    ObjectIndexRecordV1, ObjectLocationV1, PACK_HEADER_LEN_V1, PackFormatError, PackHeaderV1,
    PackTerminalIdentityV1,
};
pub use manifest::{
    AcceleratorArtifact, AcceleratorArtifactKind, AcceleratorManifestV1, AcceleratorManifestView,
    LegacyHistoryRoot, NamespaceRoot, SemanticNamespace, SemanticProfileId, SemanticRoots,
    SemanticStateManifestV1, SemanticStateManifestView,
};
pub use mutation::{
    ContentMutationEntry, ContentMutationIter, ContentTransition, CountMutationEntry,
    CountMutationIter, CountTransition, MutationBatchPlanV1, MutationBatchViewV1,
    MutationDecodeLimits, MutationError, RSpaceMutationBatchV1, StateVersionV1,
};
pub use object::{
    BranchKind, CanonicalBlob, CanonicalBlobKind, CanonicalBlobView, CanonicalDagNode,
    CanonicalDagNodeView, CanonicalObjectKind, CanonicalObjectView, DagChild, DagChildrenIter,
    DagChildrenView, EndpointValue,
};
pub use physical::{PhysicalDigest, StoreId};
pub use retention::{
    RetainedStateIter, RetainedStateV1, RetentionManifestV1, RetentionManifestView,
};
pub use rewrite::{
    CompareAndSwapFailure, DagRewriteLimits, DagRewriteReport, PreparedDagRewrite, RewriteError,
    rewrite_semantic_roots, rewrite_semantic_roots_with_limits,
};
#[cfg(test)]
pub(crate) use rewrite::{
    ExactPath, ExactPathArena, InheritedBoundaryKind, InheritedObjectRef, RewriteProvenance,
};
#[cfg(test)]
pub(crate) use rewrite::{preflight_sensitive_copy_bytes, reset_preflight_sensitive_copy_bytes};
pub(crate) use semantic::semantic_profile_seal;
pub use semantic::{
    IncrementalSemanticLimits, IncrementalSemanticReport, SemanticProfile,
    SemanticValidationLimits, SemanticValidationReport, verify_rspace_semantics,
};
pub use store::{
    CanonicalObjectStore, ImmutableObjectStore, LayeredObject, MutableObjectStore, ObjectAdmission,
    StagingObjectStore, StagingStoreError, StagingStoreLimits,
};
pub use transaction::{
    DagTransactionDiagnostics, DagTransactionError, DagTransactionLimits, OpenDagTransaction,
    SealedDagTransaction, SealedObject, SealedTransitionV1, VerifiedDagState,
};
pub use wal::{
    CommitPublicationV1, CommittedObjectComparison, CommittedObjectLookup, LogPackAddress,
    LogPackDecodeLimits, LogPackFormatError, LogPackObjectDispositionV1, LogPackObjectIter,
    LogPackObjectPlanError, LogPackObjectPlanV1, LogPackObjectView, LogPackSequence,
    LogPackWriteError, PackId, PrepareRecordIdentityV1, PreparedLogPackTransactionV1,
    TRANSACTION_COMMIT_RECORD_LEN_V1, TransactionCommitRecordV1, TransactionCommitViewV1,
    TransactionPrepareRecordViewV1,
};

#[cfg(test)]
#[path = "../../../tests/persistence/mod.rs"]
mod tests;
