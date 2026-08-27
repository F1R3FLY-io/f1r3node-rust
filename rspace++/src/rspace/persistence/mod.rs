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
mod semantic;
mod store;

pub use canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, ObjectDigest, ObjectRefV1,
};
pub use closure::{
    ClosureError, StructuralClosureReport, StructuralValidationLimits, verify_structural_closure,
    verify_structural_closure_with_limits,
};
pub use manifest::{
    AcceleratorArtifact, AcceleratorArtifactKind, AcceleratorManifestV1, AcceleratorManifestView,
    LegacyHistoryRoot, NamespaceRoot, SemanticNamespace, SemanticRoots, SemanticStateManifestV1,
    SemanticStateManifestView,
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
pub use semantic::{SemanticValidationLimits, SemanticValidationReport, verify_rspace_semantics};
pub use store::{CanonicalObjectStore, ImmutableObjectStore, ObjectAdmission};

#[cfg(test)]
#[path = "../../../tests/persistence/mod.rs"]
mod tests;
