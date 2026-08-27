//! Canonical persistent-DAG substrate for RSpace.
//!
//! The live RSpace remains PathMap-backed. This module defines the portable,
//! content-addressed logical image used by the incremental persistence layer;
//! it deliberately does not encode PathMap's private nodes, ACT offsets,
//! process-local caches, or swizzled pointers.

mod canonical;
mod closure;
mod manifest;
mod mutation;
mod object;
mod rewrite;
mod semantic;
mod store;
mod transaction;

pub use canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, ObjectDigest, ObjectRefV1,
};
#[cfg(test)]
pub(crate) use closure::verify_incremental_closure;
pub use closure::{
    ClosureError, IncrementalClosureLimits, StructuralClosureReport, StructuralValidationLimits,
    verify_structural_closure, verify_structural_closure_with_limits,
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
    CanonicalObjectStore, ImmutableObjectStore, LayeredObject, ObjectAdmission, StagingObjectStore,
    StagingStoreError, StagingStoreLimits,
};
pub use transaction::{
    DagTransactionDiagnostics, DagTransactionError, DagTransactionLimits, OpenDagTransaction,
    SealedDagTransaction, SealedObject, SealedTransitionV1, VerifiedDagState,
};

#[cfg(test)]
#[path = "../../../tests/persistence/mod.rs"]
mod tests;
