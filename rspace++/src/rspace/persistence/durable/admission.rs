//! Profile-typed semantic admission of physically recovered durable state.
//!
//! Physical WAL, catalog, and index recovery is deliberately an internal
//! phase. Production callers receive authority only after the catalog-named
//! retention policy, every retained structural closure, the current RSpace
//! semantics, and both authenticated byte inventories have been recomputed
//! through the recovered digest index.

use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

use super::io::{DurableDirectoryV1, DurableFileV1, DurableIoError, DurableWriterLeaseV1};
use super::startup::{
    DurableStartupErrorV1, DurableStartupWorkV1, IndexBindingDispositionV1,
    IndexBoundPhysicalRecoveryV1, PhysicallyRecoveredDurableWriterV1, read_catalog_token,
    recover_physical_from_lease_v1,
};
use super::{CommittedObjectIndexV1, INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexFormatError};
use crate::rspace::persistence::canonical::{
    CanonicalFormatError, CanonicalSink, ObjectDigest, ObjectRefV1, checked_len,
};
use crate::rspace::persistence::checkpoint::{
    CheckpointReferenceVisitErrorV1, GenerationCheckpointDescriptorV1, GenerationCheckpointErrorV1,
    try_visit_checkpoint_references, try_visit_checkpoint_references_v2,
};
use crate::rspace::persistence::closure::ledger_trial::{
    LedgerTrialAxisV1, LedgerTrialLimitsV1, LedgerTrialRefusalV1, RootDeltaV1, run_domain_trials,
    run_ledger_trial,
};
use crate::rspace::persistence::closure::{
    ClosureCreditV1, ClosureDomainV1, ClosureError, ClosureInventoryV1,
    RetainedUnionClosureLimitsV1, RetainedUnionClosureReportV1, StructuralClosureLedgerV1,
    StructuralClosureReport, StructuralValidationLimits,
    verify_structural_closure_in_ledger_with_credit,
    verify_structural_closure_in_ledger_with_limits,
};
use crate::rspace::persistence::mutation::MutationBatchViewV1;
use crate::rspace::persistence::retention::v2_format::{
    RetentionAnchorV2, RetentionFormat, RetentionReader,
};
use crate::rspace::persistence::retention::v2_reader::{
    AdmittedRetention, AdmittedRetentionMetadataV2, AppointedRetention, RetentionRadixErrorV2,
    RetentionRadixRefusalV2, admit_retention_history_v2,
};
use crate::rspace::persistence::retention::v2_writer::{commit_retention_v2, migrate_retention_v1};
use crate::rspace::persistence::retention::{
    RetainedStateIter, RetainedStateV1, RetentionManifestV1,
};
use crate::rspace::persistence::rewrite::{DagRewriteLimits, RewriteError};
use crate::rspace::persistence::semantic::{
    CurrentShapeSummaryV1, SemanticProfile, SemanticValidationLimits, SemanticValidationReport,
    SemanticallyAdmittedStateV1, verify_rspace_semantics_after_structural_with_shape,
};
use crate::rspace::persistence::store::{
    ImmutableObjectStore, StagingObjectStore, StagingStoreError,
};
#[cfg(test)]
use crate::rspace::persistence::transaction::SealedObject;
use crate::rspace::persistence::transaction::{
    DagTransactionError, DagTransactionLimits, OpenDagTransaction, SealedDagTransaction,
    SealedTransitionV1, SemanticValidityV1, ValidityOnlyV1,
};
use crate::rspace::persistence::wal::{
    CommittedObjectComparison, CommittedObjectLookup, LogPackObjectPlanError, LogPackObjectPlanV1,
    LogPackSequence, PreparedLogPackTransactionV1, TerminalCommitViewV1,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CatalogAuthorityTokenV1, CatalogSlotV1, DurableLimits, GenerationLeaseError,
    GenerationLeaseRegistryV1, LogPackDecodeLimits, ObjectIndexRecordV1, PackId, SemanticProfileId,
    StateVersionV1, VerifiedDagState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DurableAdmissionLimitsV1 {
    pub(crate) current_semantics: SemanticValidationLimits,
    pub(crate) per_state_structural: StructuralValidationLimits,
    pub(crate) max_retained_states: u64,
    pub(crate) max_union_objects: u64,
    pub(crate) max_union_canonical_bytes: u64,
    pub(crate) max_aggregate_root_visits: u64,
    pub(crate) max_aggregate_references: u64,
    pub(crate) max_object_reads: u64,
    pub(crate) max_object_read_bytes: u64,
}

impl DurableAdmissionLimitsV1 {
    pub(crate) const UNBOUNDED: Self = Self {
        current_semantics: SemanticValidationLimits::unbounded(),
        per_state_structural: StructuralValidationLimits::unbounded(),
        max_retained_states: u64::MAX,
        max_union_objects: u64::MAX,
        max_union_canonical_bytes: u64::MAX,
        max_aggregate_root_visits: u64::MAX,
        max_aggregate_references: u64::MAX,
        max_object_reads: u64::MAX,
        max_object_read_bytes: u64::MAX,
    };
}

impl Default for DurableAdmissionLimitsV1 {
    fn default() -> Self { Self::UNBOUNDED }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TrustedStateCeilingsV1 {
    pub(crate) max_state_objects: u64,
    pub(crate) max_state_canonical_bytes: u64,
    pub(crate) max_retained_states: u64,
    pub(crate) max_union_objects: u64,
    pub(crate) max_union_canonical_bytes: u64,
    pub(crate) max_metadata_objects: u64,
    pub(crate) max_metadata_canonical_bytes: u64,
    pub(crate) max_logical_nodes: u64,
    pub(crate) max_endpoints: u64,
    pub(crate) max_logical_path_bytes: u64,
    pub(crate) max_payload_bytes: u64,
}

impl TrustedStateCeilingsV1 {
    pub(crate) const UNBOUNDED: Self = Self {
        max_state_objects: u64::MAX,
        max_state_canonical_bytes: u64::MAX,
        max_retained_states: u64::MAX,
        max_union_objects: u64::MAX,
        max_union_canonical_bytes: u64::MAX,
        max_metadata_objects: u64::MAX,
        max_metadata_canonical_bytes: u64::MAX,
        max_logical_nodes: u64::MAX,
        max_endpoints: u64::MAX,
        max_logical_path_bytes: u64::MAX,
        max_payload_bytes: u64::MAX,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TrustedTrialWorkV1 {
    pub(crate) count: u64,
    pub(crate) union_objects: u64,
    pub(crate) union_bytes: u64,
    pub(crate) scratch: u64,
    pub(crate) references: u64,
    pub(crate) expansions: u64,
    pub(crate) quoted_bytes: u64,
}

impl TrustedTrialWorkV1 {
    pub(crate) const UNBOUNDED: Self = Self {
        count: u64::MAX,
        union_objects: u64::MAX,
        union_bytes: u64::MAX,
        scratch: u64::MAX,
        references: u64::MAX,
        expansions: u64::MAX,
        quoted_bytes: u64::MAX,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TrustedSuccessorPolicyV1 {
    pub(crate) state: TrustedStateCeilingsV1,
    pub(crate) work: [TrustedTrialWorkV1; 3],
}

impl TrustedSuccessorPolicyV1 {
    pub(crate) const UNBOUNDED: Self = Self {
        state: TrustedStateCeilingsV1::UNBOUNDED,
        work: [TrustedTrialWorkV1::UNBOUNDED; 3],
    };

    const fn trial_limits(&self, domain: ClosureDomainV1) -> LedgerTrialLimitsV1 {
        let (work, final_objects, final_bytes) = match domain {
            ClosureDomainV1::Current => {
                (self.work[0], self.state.max_state_objects, self.state.max_state_canonical_bytes)
            }
            ClosureDomainV1::Retained => {
                (self.work[1], self.state.max_union_objects, self.state.max_union_canonical_bytes)
            }
            ClosureDomainV1::Metadata => (
                self.work[2],
                self.state.max_metadata_objects,
                self.state.max_metadata_canonical_bytes,
            ),
        };
        LedgerTrialLimitsV1::UNBOUNDED
            .with(LedgerTrialAxisV1::Count, work.count)
            .with(LedgerTrialAxisV1::UnionObjects, work.union_objects)
            .with(LedgerTrialAxisV1::UnionBytes, work.union_bytes)
            .with(LedgerTrialAxisV1::Scratch, work.scratch)
            .with(LedgerTrialAxisV1::References, work.references)
            .with(LedgerTrialAxisV1::Expansions, work.expansions)
            .with(LedgerTrialAxisV1::QuotedBytes, work.quoted_bytes)
            .with(LedgerTrialAxisV1::FinalObjects, final_objects)
            .with(LedgerTrialAxisV1::FinalBytes, final_bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrustedCeilingAxisV1 {
    StateObjects,
    StateBytes,
    RetainedStates,
    LogicalNodes,
    Endpoints,
    LogicalPathBytes,
    PayloadBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DurableAdmissionLimitKindV1 {
    PerStateDistinctObjects,
    PerStateCanonicalBytes,
    PerStateReferences,
    PerStateWorklist,
    RetainedStates,
    UnionObjects,
    UnionCanonicalBytes,
    AggregateRootVisits,
    AggregateReferences,
    ObjectReads,
    ObjectReadBytes,
    LogicalNodes,
    Endpoints,
    LogicalPathBytes,
    PayloadBytes,
    JoinLookups,
    JoinLookupSteps,
    SemanticWorklist,
}

impl DurableAdmissionLimitKindV1 {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::PerStateDistinctObjects => "per-state distinct objects",
            Self::PerStateCanonicalBytes => "per-state canonical bytes",
            Self::PerStateReferences => "per-state references",
            Self::PerStateWorklist => "per-state structural worklist",
            Self::RetainedStates => "retained states",
            Self::UnionObjects => "retained-union objects",
            Self::UnionCanonicalBytes => "retained-union canonical bytes",
            Self::AggregateRootVisits => "aggregate retained-root visits",
            Self::AggregateReferences => "aggregate retained references",
            Self::ObjectReads => "durable object reads",
            Self::ObjectReadBytes => "durable object read bytes",
            Self::LogicalNodes => "semantic logical nodes",
            Self::Endpoints => "semantic endpoints",
            Self::LogicalPathBytes => "semantic logical path bytes",
            Self::PayloadBytes => "semantic source payload bytes",
            Self::JoinLookups => "semantic join lookups",
            Self::JoinLookupSteps => "semantic join lookup steps",
            Self::SemanticWorklist => "semantic worklist",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DurableAdmissionPreflightErrorV1 {
    pub(crate) resource: DurableAdmissionLimitKindV1,
    pub(crate) limit: u64,
    pub(crate) attempted: u64,
}

impl Display for DurableAdmissionPreflightErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} limit exceeded before generation publication: limit {}, required {}",
            self.resource.name(),
            self.limit,
            self.attempted,
        )
    }
}

impl std::error::Error for DurableAdmissionPreflightErrorV1 {}

/// Exact, nonserialized replay demand derived by successful admission.
///
/// This certificate is kept inside affine writer authority. Callers can
/// choose ceilings but cannot forge the work required to re-admit the exact
/// checkpoint state after publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DurableAdmissionRequirementsV1 {
    per_state_structural: StructuralClosureReport,
    current_semantics: SemanticValidationReport,
    retained_states: u64,
    union_objects: u64,
    union_canonical_bytes: u64,
    aggregate_root_visits: u64,
    aggregate_references: u64,
    object_reads: u64,
    object_read_bytes: u64,
}

impl DurableAdmissionRequirementsV1 {
    fn require(
        resource: DurableAdmissionLimitKindV1,
        limit: u64,
        attempted: u64,
    ) -> Result<(), DurableAdmissionPreflightErrorV1> {
        if attempted > limit {
            return Err(DurableAdmissionPreflightErrorV1 {
                resource,
                limit,
                attempted,
            });
        }
        Ok(())
    }

    pub(crate) fn preflight(
        self,
        limits: DurableAdmissionLimitsV1,
    ) -> Result<(), DurableAdmissionPreflightErrorV1> {
        let structural = self.per_state_structural;
        Self::require(
            DurableAdmissionLimitKindV1::PerStateDistinctObjects,
            limits.per_state_structural.max_distinct_objects,
            structural.distinct_objects,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::PerStateCanonicalBytes,
            limits.per_state_structural.max_canonical_bytes,
            structural.canonical_bytes,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::PerStateReferences,
            limits.per_state_structural.max_references,
            structural.references,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::PerStateWorklist,
            limits.per_state_structural.max_worklist,
            structural.maximum_worklist,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::RetainedStates,
            limits.max_retained_states,
            self.retained_states,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::UnionObjects,
            limits.max_union_objects,
            self.union_objects,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::UnionCanonicalBytes,
            limits.max_union_canonical_bytes,
            self.union_canonical_bytes,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::AggregateRootVisits,
            limits.max_aggregate_root_visits,
            self.aggregate_root_visits,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::AggregateReferences,
            limits.max_aggregate_references,
            self.aggregate_references,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::ObjectReads,
            limits.max_object_reads,
            self.object_reads,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::ObjectReadBytes,
            limits.max_object_read_bytes,
            self.object_read_bytes,
        )?;

        // Semantic completion consumes an already verified structural token;
        // its nested structural limits are deliberately inapplicable here.
        let semantic = self.current_semantics;
        Self::require(
            DurableAdmissionLimitKindV1::LogicalNodes,
            limits.current_semantics.max_logical_nodes,
            semantic.logical_nodes,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::Endpoints,
            limits.current_semantics.max_endpoints,
            semantic.endpoints,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::LogicalPathBytes,
            limits.current_semantics.max_logical_path_bytes,
            semantic.maximum_logical_path_bytes,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::PayloadBytes,
            limits.current_semantics.max_payload_bytes,
            semantic.source_payload_bytes,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::JoinLookups,
            limits.current_semantics.max_join_lookups,
            semantic.join_lookups,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::JoinLookupSteps,
            limits.current_semantics.max_join_lookup_steps,
            semantic.join_lookup_steps,
        )?;
        Self::require(
            DurableAdmissionLimitKindV1::SemanticWorklist,
            limits.current_semantics.max_worklist,
            semantic.maximum_worklist,
        )
    }
}

fn componentwise_max_structural(
    maximum: &mut StructuralClosureReport,
    observed: StructuralClosureReport,
) {
    maximum.distinct_objects = maximum.distinct_objects.max(observed.distinct_objects);
    maximum.node_objects = maximum.node_objects.max(observed.node_objects);
    maximum.payload_objects = maximum.payload_objects.max(observed.payload_objects);
    maximum.canonical_bytes = maximum.canonical_bytes.max(observed.canonical_bytes);
    maximum.references = maximum.references.max(observed.references);
    maximum.maximum_worklist = maximum.maximum_worklist.max(observed.maximum_worklist);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InventoryMismatchKindV1 {
    CurrentState,
    CheckpointCurrentStateObjects,
    RetainedUnion,
    CheckpointRetainedUnionObjects,
    RetainedState { generation: u64 },
    CheckpointRetentionMetadataObjects,
    CheckpointRetentionMetadataBytes,
}

#[derive(Debug)]
pub(crate) enum DurableObjectReadErrorV1 {
    Io(DurableIoError),
    MaterializationLimit {
        digest: ObjectDigest,
        limit: u64,
        attempted: u64,
    },
    MaterializationAddressSpace {
        digest: ObjectDigest,
        attempted: u64,
    },
    MaterializationAllocation {
        digest: ObjectDigest,
        attempted: u64,
    },
    PackOutsideAuthority {
        digest: ObjectDigest,
        pack: PackId,
        first: PackId,
        active: PackId,
    },
    ObjectExtentOverflow {
        digest: ObjectDigest,
        offset: u64,
        length: u64,
    },
    ObjectBeyondAuthorizedTail {
        digest: ObjectDigest,
        pack: PackId,
        end: u64,
        tail: u64,
    },
    AdmissionReadLimit {
        resource: &'static str,
        limit: u64,
        attempted: u64,
    },
    MissingObject(ObjectDigest),
    ObjectReferenceMismatch {
        requested: ObjectRefV1,
        indexed: ObjectRefV1,
    },
    CopiedDigestMismatch {
        expected: ObjectDigest,
        actual: ObjectDigest,
    },
}

impl Display for DurableObjectReadErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::MaterializationLimit {
                digest,
                limit,
                attempted,
            } => write!(
                formatter,
                "durable object {digest:?} advertises {attempted} bytes above the caller's \
                 {limit}-byte materialization bound"
            ),
            Self::MaterializationAddressSpace { digest, attempted } => write!(
                formatter,
                "durable object {digest:?} length {attempted} does not fit this platform's \
                 address space"
            ),
            Self::MaterializationAllocation { digest, attempted } => {
                write!(formatter, "cannot reserve {attempted} bytes for durable object {digest:?}")
            }
            Self::PackOutsideAuthority {
                digest,
                pack,
                first,
                active,
            } => write!(
                formatter,
                "durable object {digest:?} points to pack {}, outside catalog range {}..={}",
                pack.get(),
                first.get(),
                active.get()
            ),
            Self::ObjectExtentOverflow {
                digest,
                offset,
                length,
            } => write!(formatter, "durable object {digest:?} extent {offset}+{length} overflows"),
            Self::ObjectBeyondAuthorizedTail {
                digest,
                pack,
                end,
                tail,
            } => write!(
                formatter,
                "durable object {digest:?} ends at {end} in pack {}, beyond authorized tail {tail}",
                pack.get()
            ),
            Self::AdmissionReadLimit {
                resource,
                limit,
                attempted,
            } => write!(
                formatter,
                "durable semantic admission {resource} limit {limit} exceeded by {attempted}"
            ),
            Self::MissingObject(digest) => {
                write!(formatter, "durable object {digest:?} is absent from the committed index")
            }
            Self::ObjectReferenceMismatch { requested, indexed } => write!(
                formatter,
                "durable object reference {requested:?} conflicts with indexed reference \
                 {indexed:?}"
            ),
            Self::CopiedDigestMismatch { expected, actual } => write!(
                formatter,
                "durable object changed while streaming: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for DurableObjectReadErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DurableIoError> for DurableObjectReadErrorV1 {
    fn from(error: DurableIoError) -> Self { Self::Io(error) }
}

#[derive(Debug)]
pub(crate) enum DurableObjectCopyErrorV1<E> {
    Object(DurableObjectReadErrorV1),
    Sink(E),
}

impl<E: Display> Display for DurableObjectCopyErrorV1<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Object(error) => Display::fmt(error, formatter),
            Self::Sink(error) => write!(formatter, "durable object sink failed: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for DurableObjectCopyErrorV1<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Object(error) => Some(error),
            Self::Sink(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub(crate) enum DurableAdmissionErrorV1 {
    Startup(DurableStartupErrorV1),
    GenerationLease(GenerationLeaseError),
    ObjectStore(DurableObjectReadErrorV1),
    Canonical(CanonicalFormatError),
    Closure(ClosureError<DurableObjectReadErrorV1>),
    ProfileMismatch {
        expected: SemanticProfileId,
        actual: SemanticProfileId,
    },
    RetentionCurrentMismatch {
        catalog: StateVersionV1,
        manifest: StateVersionV1,
    },
    NonHistoricalState {
        current_generation: u64,
        retained_generation: u64,
    },
    InventoryMismatch {
        kind: InventoryMismatchKindV1,
        declared: u64,
        computed: u64,
    },
    RetentionRadix(RetentionRadixRefusalV2),
    RetentionWriterUnreadable {
        writer: RetentionFormat,
        reader: RetentionReader,
    },
    #[cfg(test)]
    InjectedFailure(&'static str),
    AuthorityChanged,
    PackTail {
        pack: PackId,
        authorized: u64,
        available: u64,
    },
}

impl Display for DurableAdmissionErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Startup(error) => Display::fmt(error, formatter),
            Self::GenerationLease(error) => Display::fmt(error, formatter),
            Self::ObjectStore(error) => Display::fmt(error, formatter),
            Self::Canonical(error) => Display::fmt(error, formatter),
            Self::Closure(error) => Display::fmt(error, formatter),
            Self::ProfileMismatch { expected, actual } => write!(
                formatter,
                "durable catalog semantic profile {:?} does not equal requested profile {:?}",
                actual.as_bytes(),
                expected.as_bytes()
            ),
            Self::RetentionCurrentMismatch { catalog, manifest } => write!(
                formatter,
                "retention current state {manifest:?} does not equal catalog state {catalog:?}"
            ),
            Self::NonHistoricalState {
                current_generation,
                retained_generation,
            } => write!(
                formatter,
                "retained generation {retained_generation} does not precede current generation \
                 {current_generation}"
            ),
            Self::InventoryMismatch {
                kind,
                declared,
                computed,
            } => write!(
                formatter,
                "authenticated {kind:?} inventory declares {declared}, but admission computed \
                 {computed}"
            ),
            Self::RetentionRadix(refusal) => Display::fmt(refusal, formatter),
            Self::RetentionWriterUnreadable { writer, reader } => write!(
                formatter,
                "a {writer:?} retention writer would seal successors that its {reader:?} \
                 retention reader refuses"
            ),
            #[cfg(test)]
            Self::InjectedFailure(point) => {
                write!(formatter, "injected durable semantic-admission failure at {point}")
            }
            Self::AuthorityChanged => {
                formatter.write_str("durable catalog authority changed during semantic admission")
            }
            Self::PackTail {
                pack,
                authorized,
                available,
            } => write!(
                formatter,
                "catalog authorizes {authorized} bytes in pack {}, but only {available} exist",
                pack.get()
            ),
        }
    }
}

impl std::error::Error for DurableAdmissionErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Startup(error) => Some(error),
            Self::GenerationLease(error) => Some(error),
            Self::ObjectStore(error) => Some(error),
            Self::Canonical(error) => Some(error),
            Self::Closure(error) => Some(error),
            Self::RetentionRadix(refusal) => Some(refusal),
            _ => None,
        }
    }
}

impl From<RetentionRadixErrorV2<DurableObjectReadErrorV1>> for DurableAdmissionErrorV1 {
    fn from(error: RetentionRadixErrorV2<DurableObjectReadErrorV1>) -> Self {
        match error {
            RetentionRadixErrorV2::Store(error) => Self::ObjectStore(error),
            RetentionRadixErrorV2::Canonical(error) => Self::Canonical(error),
            RetentionRadixErrorV2::Refusal(refusal) => Self::RetentionRadix(refusal),
        }
    }
}

impl From<DurableStartupErrorV1> for DurableAdmissionErrorV1 {
    fn from(error: DurableStartupErrorV1) -> Self { Self::Startup(error) }
}

impl From<GenerationLeaseError> for DurableAdmissionErrorV1 {
    fn from(error: GenerationLeaseError) -> Self { Self::GenerationLease(error) }
}

impl From<DurableObjectReadErrorV1> for DurableAdmissionErrorV1 {
    fn from(error: DurableObjectReadErrorV1) -> Self { Self::ObjectStore(error) }
}

impl From<CanonicalFormatError> for DurableAdmissionErrorV1 {
    fn from(error: CanonicalFormatError) -> Self { Self::Canonical(error) }
}

impl From<ClosureError<DurableObjectReadErrorV1>> for DurableAdmissionErrorV1 {
    fn from(error: ClosureError<DurableObjectReadErrorV1>) -> Self { Self::Closure(error) }
}

struct PinnedPackV1 {
    id: PackId,
    file: DurableFileV1,
    authorized_tail: u64,
}

/// Immutable object reads bound to the one physically recovered index and to
/// already-open handles for every catalog-named pack. The pack vector is
/// addressed by `pack_id - first_live_pack`; no directory enumeration or
/// second location map participates in lookup.
pub(crate) struct DurableIndexedObjectStoreV1 {
    index: CommittedObjectIndexV1,
    first_live_pack: PackId,
    active_pack: PackId,
    packs: Box<[PinnedPackV1]>,
}

impl DurableIndexedObjectStoreV1 {
    fn pin(
        directory: &DurableDirectoryV1,
        authority: &CatalogAuthorityTokenV1,
        index: CommittedObjectIndexV1,
    ) -> Result<Self, DurableAdmissionErrorV1> {
        let pack_state = authority.slot().pack();
        let first_live_pack = PackId::new(pack_state.first_live_pack()).map_err(|error| {
            CanonicalFormatError::SemanticCoherence {
                detail: error.to_string(),
            }
        })?;
        let active_pack = PackId::new(pack_state.active_pack()).map_err(|error| {
            CanonicalFormatError::SemanticCoherence {
                detail: error.to_string(),
            }
        })?;
        let count = active_pack
            .get()
            .checked_sub(first_live_pack.get())
            .and_then(|distance| distance.checked_add(1))
            .ok_or(CanonicalFormatError::CounterOverflow("live pack handles"))?;
        let count_usize =
            usize::try_from(count).map_err(|_| CanonicalFormatError::LengthOutOfRange {
                field: "live pack handles",
                value: count,
            })?;
        let mut packs = Vec::new();
        packs
            .try_reserve_exact(count_usize)
            .map_err(|_| CanonicalFormatError::AllocationFailure { requested: count })?;
        let mut raw = first_live_pack.get();
        loop {
            let id = PackId::new(raw).map_err(|error| CanonicalFormatError::SemanticCoherence {
                detail: error.to_string(),
            })?;
            let file = directory
                .open_pack(id)
                .map_err(DurableStartupErrorV1::from)?;
            let available = file.len().map_err(DurableStartupErrorV1::from)?;
            let authorized_tail = if id == active_pack {
                pack_state.durable_pack_tail()
            } else {
                available
            };
            if available < authorized_tail {
                return Err(DurableAdmissionErrorV1::PackTail {
                    pack: id,
                    authorized: authorized_tail,
                    available,
                });
            }
            packs.push(PinnedPackV1 {
                id,
                file,
                authorized_tail,
            });
            if id == active_pack {
                break;
            }
            raw = raw
                .checked_add(1)
                .ok_or(CanonicalFormatError::CounterOverflow("live pack identifier"))?;
        }
        Ok(Self {
            index,
            first_live_pack,
            active_pack,
            packs: packs.into_boxed_slice(),
        })
    }

    fn lookup_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<ObjectIndexRecordV1>, DurableObjectReadErrorV1> {
        let Some(record) = self.index.get(*digest) else {
            return Ok(None);
        };
        let attempted = record.reference().encoded_len();
        if attempted > max_encoded_len {
            return Err(DurableObjectReadErrorV1::MaterializationLimit {
                digest: *digest,
                limit: max_encoded_len,
                attempted,
            });
        }
        Ok(Some(record))
    }

    fn lookup_exact(
        &self,
        reference: ObjectRefV1,
    ) -> Result<Option<ObjectIndexRecordV1>, DurableObjectReadErrorV1> {
        let Some(record) = self.index.get(reference.digest()) else {
            return Ok(None);
        };
        if record.reference() != reference {
            return Err(DurableObjectReadErrorV1::ObjectReferenceMismatch {
                requested: reference,
                indexed: record.reference(),
            });
        }
        Ok(Some(record))
    }

    fn locate_record(
        &self,
        record: ObjectIndexRecordV1,
    ) -> Result<(&PinnedPackV1, u64), DurableObjectReadErrorV1> {
        let reference = record.reference();
        let location = record.location();
        let pack = location.pack_id();
        let relative = pack.get().checked_sub(self.first_live_pack.get()).ok_or(
            DurableObjectReadErrorV1::PackOutsideAuthority {
                digest: reference.digest(),
                pack,
                first: self.first_live_pack,
                active: self.active_pack,
            },
        )?;
        let index = usize::try_from(relative).map_err(|_| {
            DurableObjectReadErrorV1::PackOutsideAuthority {
                digest: reference.digest(),
                pack,
                first: self.first_live_pack,
                active: self.active_pack,
            }
        })?;
        let pinned =
            self.packs
                .get(index)
                .ok_or(DurableObjectReadErrorV1::PackOutsideAuthority {
                    digest: reference.digest(),
                    pack,
                    first: self.first_live_pack,
                    active: self.active_pack,
                })?;
        debug_assert_eq!(pinned.id, pack);
        let offset = location.canonical_offset();
        let end = offset.checked_add(reference.encoded_len()).ok_or(
            DurableObjectReadErrorV1::ObjectExtentOverflow {
                digest: reference.digest(),
                offset,
                length: reference.encoded_len(),
            },
        )?;
        if end > pinned.authorized_tail {
            return Err(DurableObjectReadErrorV1::ObjectBeyondAuthorizedTail {
                digest: reference.digest(),
                pack,
                end,
                tail: pinned.authorized_tail,
            });
        }
        Ok((pinned, offset))
    }

    fn read_record(
        &self,
        record: ObjectIndexRecordV1,
    ) -> Result<Box<[u8]>, DurableObjectReadErrorV1> {
        let reference = record.reference();
        let (pinned, offset) = self.locate_record(record)?;
        let length = reference.encoded_len();
        let length_usize = usize::try_from(length).map_err(|_| {
            DurableObjectReadErrorV1::MaterializationAddressSpace {
                digest: reference.digest(),
                attempted: length,
            }
        })?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length_usize).map_err(|_| {
            DurableObjectReadErrorV1::MaterializationAllocation {
                digest: reference.digest(),
                attempted: length,
            }
        })?;
        bytes.resize(length_usize, 0);
        pinned.file.read_exact_at(offset, &mut bytes)?;
        Ok(bytes.into_boxed_slice())
    }

    pub(crate) fn copy_verified_canonical_with_scratch<S: CanonicalSink>(
        &self,
        reference: ObjectRefV1,
        sink: &mut S,
        scratch: &mut Vec<u8>,
    ) -> Result<(), DurableObjectCopyErrorV1<S::Error>> {
        let record = self
            .lookup_exact(reference)
            .map_err(DurableObjectCopyErrorV1::Object)?
            .ok_or(DurableObjectCopyErrorV1::Object(DurableObjectReadErrorV1::MissingObject(
                reference.digest(),
            )))?;
        let (pinned, offset) = self
            .locate_record(record)
            .map_err(DurableObjectCopyErrorV1::Object)?;
        copy_verified_canonical_with_scratch(reference, sink, scratch, |relative, destination| {
            let absolute = offset.checked_add(relative).ok_or(
                DurableObjectReadErrorV1::ObjectExtentOverflow {
                    digest: reference.digest(),
                    offset,
                    length: relative,
                },
            )?;
            pinned.file.read_exact_at(absolute, destination)?;
            Ok(())
        })
    }
}

const DURABLE_COPY_BUFFER_LEN: usize = 64 * 1024;

pub(crate) fn copy_verified_canonical_with<S: CanonicalSink>(
    reference: ObjectRefV1,
    sink: &mut S,
    read: impl FnMut(u64, &mut [u8]) -> Result<(), DurableObjectReadErrorV1>,
) -> Result<(), DurableObjectCopyErrorV1<S::Error>> {
    copy_verified_canonical_with_scratch(reference, sink, &mut Vec::new(), read)
}

pub(crate) fn copy_verified_canonical_with_scratch<S: CanonicalSink>(
    reference: ObjectRefV1,
    sink: &mut S,
    scratch: &mut Vec<u8>,
    mut read: impl FnMut(u64, &mut [u8]) -> Result<(), DurableObjectReadErrorV1>,
) -> Result<(), DurableObjectCopyErrorV1<S::Error>> {
    let capacity = usize::try_from(reference.encoded_len().min(DURABLE_COPY_BUFFER_LEN as u64))
        .expect("bounded durable copy scratch fits usize");
    if scratch.capacity() < capacity {
        let additional = capacity.saturating_sub(scratch.len());
        scratch.try_reserve_exact(additional).map_err(|_| {
            DurableObjectCopyErrorV1::Object(DurableObjectReadErrorV1::MaterializationAllocation {
                digest: reference.digest(),
                attempted: capacity as u64,
            })
        })?;
    }
    scratch.resize(capacity, 0);
    let mut hasher = Blake2b::<U32>::new();
    let mut copied = 0u64;
    while copied < reference.encoded_len() {
        let remaining = reference.encoded_len() - copied;
        let chunk_len = usize::try_from(remaining.min(scratch.len() as u64))
            .expect("bounded durable copy chunk fits usize");
        let chunk = &mut scratch[..chunk_len];
        read(copied, chunk).map_err(DurableObjectCopyErrorV1::Object)?;
        hasher.update(&*chunk);
        sink.write_all(chunk)
            .map_err(DurableObjectCopyErrorV1::Sink)?;
        copied = copied.checked_add(chunk_len as u64).ok_or_else(|| {
            DurableObjectCopyErrorV1::Object(DurableObjectReadErrorV1::ObjectExtentOverflow {
                digest: reference.digest(),
                offset: 0,
                length: reference.encoded_len(),
            })
        })?;
    }
    let hash = hasher.finalize();
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&hash);
    let actual = ObjectDigest::from_array(digest);
    if actual != reference.digest() {
        return Err(DurableObjectCopyErrorV1::Object(
            DurableObjectReadErrorV1::CopiedDigestMismatch {
                expected: reference.digest(),
                actual,
            },
        ));
    }
    Ok(())
}

impl ImmutableObjectStore for DurableIndexedObjectStoreV1 {
    type Error = DurableObjectReadErrorV1;
    type Object = Box<[u8]>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        self.lookup_bounded(digest, max_encoded_len)?
            .map(|record| self.read_record(record))
            .transpose()
    }
}

/// Compare a sealed object's complete canonical bytes with the committed
/// digest index.  Classification deliberately reads the indexed body and
/// compares its full extent; a digest hit alone is never treated as equality.
impl CommittedObjectLookup for DurableIndexedObjectStoreV1 {
    type Error = DurableObjectReadErrorV1;

    fn compare_committed_object(
        &self,
        reference: ObjectRefV1,
        sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error> {
        let Some(record) = self.lookup_exact(reference)? else {
            return Ok(CommittedObjectComparison::Missing);
        };
        let committed = self.read_record(record)?;
        Ok(if committed.as_ref() == sealed_canonical_bytes {
            CommittedObjectComparison::Equal
        } else {
            CommittedObjectComparison::Collision
        })
    }
}

struct AdmissionReadBudgetV1 {
    reads: AtomicU64,
    bytes: AtomicU64,
    max_reads: u64,
    max_bytes: u64,
}

impl AdmissionReadBudgetV1 {
    fn new(max_reads: u64, max_bytes: u64) -> Self {
        Self {
            reads: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            max_reads,
            max_bytes,
        }
    }

    fn charge(&self, bytes: u64) -> Result<(), DurableObjectReadErrorV1> {
        charge_atomic(&self.bytes, bytes, self.max_bytes, "object-read bytes")?;
        if let Err(error) = charge_atomic(&self.reads, 1, self.max_reads, "object reads") {
            self.bytes.fetch_sub(bytes, Ordering::Relaxed);
            return Err(error);
        }
        Ok(())
    }

    fn reads(&self) -> u64 { self.reads.load(Ordering::Relaxed) }

    fn bytes(&self) -> u64 { self.bytes.load(Ordering::Relaxed) }
}

fn charge_atomic(
    counter: &AtomicU64,
    amount: u64,
    limit: u64,
    resource: &'static str,
) -> Result<(), DurableObjectReadErrorV1> {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let attempted =
            current
                .checked_add(amount)
                .ok_or(DurableObjectReadErrorV1::AdmissionReadLimit {
                    resource,
                    limit,
                    attempted: u64::MAX,
                })?;
        if attempted > limit {
            return Err(DurableObjectReadErrorV1::AdmissionReadLimit {
                resource,
                limit,
                attempted,
            });
        }
        match counter.compare_exchange_weak(
            current,
            attempted,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Ok(()),
            Err(observed) => current = observed,
        }
    }
}

struct BoundedAdmissionStoreV1<'store> {
    base: &'store DurableIndexedObjectStoreV1,
    budget: AdmissionReadBudgetV1,
}

impl<'store> BoundedAdmissionStoreV1<'store> {
    fn new(base: &'store DurableIndexedObjectStoreV1, limits: DurableAdmissionLimitsV1) -> Self {
        Self {
            base,
            budget: AdmissionReadBudgetV1::new(
                limits.max_object_reads,
                limits.max_object_read_bytes,
            ),
        }
    }
}

impl ImmutableObjectStore for BoundedAdmissionStoreV1<'_> {
    type Error = DurableObjectReadErrorV1;
    type Object = Box<[u8]>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        let Some(record) = self.base.lookup_bounded(digest, max_encoded_len)? else {
            return Ok(None);
        };
        self.budget.charge(record.reference().encoded_len())?;
        self.base.read_record(record).map(Some)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DurableAdmissionReportV1 {
    pub(crate) startup_disposition: IndexBindingDispositionV1,
    pub(crate) startup_work: DurableStartupWorkV1,
    pub(crate) current_structural: StructuralClosureReport,
    pub(crate) current_semantics: SemanticValidationReport,
    pub(crate) retained_states: u64,
    pub(crate) aggregate_root_visits: u64,
    pub(crate) aggregate_references: u64,
    pub(crate) retained_union: RetainedUnionClosureReportV1,
    pub(crate) retention_manifest_canonical_bytes: u64,
    pub(crate) retention_metadata_objects: u64,
    pub(crate) retention_metadata_canonical_bytes: u64,
    pub(crate) object_reads: u64,
    pub(crate) object_read_bytes: u64,
}

pub(crate) struct DurableAuthorityCoreV1<P: SemanticProfile> {
    store: DurableIndexedObjectStoreV1,
    registry: GenerationLeaseRegistryV1,
    authority: CatalogAuthorityTokenV1,
    version: StateVersionV1,
    retention_manifest_bytes: Box<[u8]>,
    retention_format: RetentionFormat,
    validity: SemanticValidityV1,
    shape: CurrentShapeSummaryV1,
    retained_closure: StructuralClosureLedgerV1,
    retained_states: u64,
    retained_state_maxima: ClosureInventoryV1,
    /// The retention format in which this writer seals successors and
    /// checkpoints, fixed by its directory at admission.
    retention_writer: RetentionFormat,
    profile: PhantomData<fn() -> P>,
}

impl<P: SemanticProfile> DurableAuthorityCoreV1<P> {
    pub(super) fn preflight_writer(
        &self,
        lease: &DurableWriterLeaseV1,
    ) -> Result<(), DurableTransactionPreparationError> {
        if !self.registry.shares_registry(&lease.generation_leases()) {
            return Err(DurableTransactionPreparationError::AuthorityMismatch);
        }
        Ok(())
    }

    pub(crate) fn verified_state(
        &self,
    ) -> VerifiedDagState<'_, DurableIndexedObjectStoreV1, P, ValidityOnlyV1> {
        VerifiedDagState::from_validity(&self.store, self.version, self.validity)
            .expect("durable validity token is bound to its exact catalog state")
    }

    pub(crate) const fn validity(&self) -> SemanticValidityV1 { self.validity }

    pub(crate) const fn shape(&self) -> &CurrentShapeSummaryV1 { &self.shape }

    pub(crate) const fn authority(&self) -> &CatalogAuthorityTokenV1 { &self.authority }

    pub(crate) const fn retention_manifest_bytes(&self) -> &[u8] { &self.retention_manifest_bytes }

    pub(crate) const fn retention_format(&self) -> RetentionFormat { self.retention_format }

    pub(crate) const fn retained_closure(&self) -> &StructuralClosureLedgerV1 {
        &self.retained_closure
    }

    pub(crate) const fn retained_states(&self) -> u64 { self.retained_states }

    pub(crate) const fn retained_state_maxima(&self) -> ClosureInventoryV1 {
        self.retained_state_maxima
    }

    /// Refuse a successor of a V2 source under the V1 writer, which would
    /// replace the V2 metadata with a stale V1 manifest.
    fn require_writable_retention(&self) -> Result<(), DurableTransactionPreparationError> {
        match (self.retention_format(), self.retention_writer) {
            (format @ RetentionFormat::V2, RetentionFormat::V1) => {
                Err(DurableTransactionPreparationError::RetentionFormatNotWritable(format))
            }
            (RetentionFormat::V1, _) | (RetentionFormat::V2, RetentionFormat::V2) => Ok(()),
        }
    }

    /// The private overlay a retention V2 write stages into: it reads the
    /// pinned store and admits the retention metadata domains.
    fn retention_staging(&self) -> StagingObjectStore<'_, DurableIndexedObjectStoreV1> {
        StagingObjectStore::new(&self.store).with_retention_reader(RetentionReader::Dual)
    }

    pub(crate) fn get_object_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Box<[u8]>>, DurableObjectReadErrorV1> {
        self.store.get_bounded(digest, max_encoded_len)
    }

    /// Classify sealed canonical objects against the authenticated pinned
    /// index. Complete bytes are compared after every digest hit, so the plan
    /// is safe for direct WAL writing without a digest-only shortcut or a
    /// second semantic projection.
    pub(crate) fn classify_transaction_objects(
        &self,
        transaction: &crate::rspace::persistence::SealedDagTransaction<'_, P>,
    ) -> Result<LogPackObjectPlanV1, LogPackObjectPlanError<DurableObjectReadErrorV1>> {
        LogPackObjectPlanV1::classify(transaction, &self.store)
    }

    /// Build the checked, zero-copy WAL prepare plan for one sealed
    /// transaction. Object dispositions are classified exactly once against
    /// the admitted index, then bound to its authenticated receipt and limits.
    pub(crate) fn prepare_transaction<'transaction, 'mutation>(
        &self,
        lease: &DurableWriterLeaseV1,
        transaction: &'transaction SealedDurableTransactionV1<'mutation, P>,
        sequence: LogPackSequence,
        limits: crate::rspace::persistence::LogPackDecodeLimits,
    ) -> Result<
        PreparedDurableLogPackTransactionV1<'transaction, 'mutation, P>,
        DurableTransactionPreparationError,
    > {
        self.preflight_writer(lease)?;
        if transaction.predecessor != *self.authority() ||
            !transaction.registry.shares_registry(&self.registry) ||
            sequence !=
                self.authority()
                    .slot()
                    .commit_sequence()
                    .successor()
                    .map_err(DurableTransactionPreparationError::Format)?
        {
            return Err(DurableTransactionPreparationError::AuthorityMismatch);
        }
        let plan = self
            .classify_transaction_objects(&transaction.sealed)
            .map_err(DurableTransactionPreparationError::ObjectPlan)?;
        let prepared = PreparedLogPackTransactionV1::new(
            &transaction.sealed,
            plan,
            self.authority().slot().store_id(),
            sequence,
            limits,
        )
        .map_err(DurableTransactionPreparationError::Format)?;
        Ok(PreparedDurableLogPackTransactionV1 {
            prepared,
            sealed: transaction,
        })
    }

    pub(crate) fn bind_trusted_transaction<'mutation>(
        &self,
        lease: &DurableWriterLeaseV1,
        sealed: SealedDagTransaction<'mutation, P>,
        policy: TrustedSuccessorPolicyV1,
    ) -> Result<SealedDurableTransactionV1<'mutation, P>, DurableTransactionPreparationError> {
        self.preflight_writer(lease)?;
        let receipt = sealed.receipt();
        if receipt.base() != self.authority.slot().state() || receipt.profile_id() != P::ID {
            return Err(DurableTransactionPreparationError::AuthorityMismatch);
        }
        self.require_writable_retention()?;
        let retained_states = self
            .retained_states
            .checked_add(1)
            .ok_or(CanonicalFormatError::CounterOverflow("successor retained states"))?;
        let staging = StagingObjectStore::new(&self.store);
        for object in sealed.objects() {
            staging
                .stage(object.canonical_bytes_arc())
                .map_err(|error| inventory_error(SuccessorInventoryError::Staging(error)))?;
        }
        let target = receipt.target();
        let trials = run_domain_trials(
            &staging,
            &self.retained_closure,
            RootDeltaV1 {
                added: &[target.manifest()],
                removed: &[receipt.base().manifest()],
            },
            RootDeltaV1 {
                added: &[target.manifest()],
                removed: &[],
            },
            [
                policy.trial_limits(ClosureDomainV1::Current),
                policy.trial_limits(ClosureDomainV1::Retained),
            ],
        )
        .map_err(|(domain, refusal)| DurableTransactionPreparationError::TrustedTrial {
            domain,
            refusal: Box::new(refusal),
        })?;
        let current = trials.successor.inventory(ClosureDomainV1::Current);
        let retained_union = trials.successor.retained_union_report();
        require_current_within_retained_union(current, retained_union).map_err(inventory_error)?;
        let maxima = ClosureInventoryV1 {
            distinct_objects: self
                .retained_state_maxima
                .distinct_objects
                .max(current.distinct_objects),
            canonical_bytes: self
                .retained_state_maxima
                .canonical_bytes
                .max(current.canonical_bytes),
        };
        let mutation = MutationBatchViewV1::parse(sealed.mutation_bytes())
            .map_err(|error| DurableTransactionPreparationError::Format(error.into()))?;
        let shape = self
            .shape
            .successor(mutation, sealed.diagnostics().rewrite)?;
        let target_state = RetainedStateV1::new(target, current.canonical_bytes)?;
        let appointed =
            AppointedRetention::parse(&self.retention_manifest_bytes, RetentionReader::Dual)?;
        let (sealed, retention, ledger, metadata) = match (appointed, self.retention_writer) {
            (AppointedRetention::V1(appointed), RetentionFormat::V1) => {
                let mut historical = appointed.historical().collect::<Result<Vec<_>, _>>()?;
                historical.push(appointed.current());
                let manifest =
                    RetentionManifestV1::new(target_state, historical.into_boxed_slice())?;
                let retention = manifest.object_ref();
                (sealed.with_retention_manifest(manifest)?, retention, trials.successor, None)
            }
            (AppointedRetention::V1(appointed), RetentionFormat::V2) => {
                let staging = self.retention_staging();
                let step = migrate_retention_v1(
                    &staging,
                    appointed,
                    target_state,
                    DagRewriteLimits::UNBOUNDED,
                )
                .map_err(RetentionWriteErrorV2::Edit)?;
                let (sealed, successor) = attach_retention_v2(
                    sealed,
                    staging,
                    &trials.successor,
                    step.anchor(),
                    &[],
                    policy.trial_limits(ClosureDomainV1::Metadata),
                )
                .map_err(trusted_retention_error)?;
                let metadata = successor.metadata_inventory();
                (sealed, step.anchor().object_ref(), successor, Some(metadata))
            }
            (AppointedRetention::V2(predecessor), RetentionFormat::V2) => {
                let staging = self.retention_staging();
                let step = commit_retention_v2(
                    &staging,
                    predecessor,
                    target_state,
                    DagRewriteLimits::UNBOUNDED,
                )
                .map_err(RetentionWriteErrorV2::Edit)?;
                let (sealed, successor) = attach_retention_v2(
                    sealed,
                    staging,
                    &trials.successor,
                    step.anchor(),
                    &[predecessor.object_ref()],
                    policy.trial_limits(ClosureDomainV1::Metadata),
                )
                .map_err(trusted_retention_error)?;
                let metadata = successor.metadata_inventory();
                (sealed, step.anchor().object_ref(), successor, Some(metadata))
            }
            (AppointedRetention::V2(_), RetentionFormat::V1) => {
                return Err(DurableTransactionPreparationError::RetentionFormatNotWritable(
                    RetentionFormat::V2,
                ));
            }
        };
        let ceilings = [
            (
                TrustedCeilingAxisV1::StateObjects,
                policy.state.max_state_objects,
                maxima.distinct_objects,
            ),
            (
                TrustedCeilingAxisV1::StateBytes,
                policy.state.max_state_canonical_bytes,
                maxima.canonical_bytes,
            ),
            (
                TrustedCeilingAxisV1::RetainedStates,
                policy.state.max_retained_states,
                retained_states,
            ),
            (
                TrustedCeilingAxisV1::LogicalNodes,
                policy.state.max_logical_nodes,
                shape.logical_nodes(),
            ),
            (TrustedCeilingAxisV1::Endpoints, policy.state.max_endpoints, shape.endpoints()),
            (
                TrustedCeilingAxisV1::LogicalPathBytes,
                policy.state.max_logical_path_bytes,
                shape.maximum_logical_path_bytes()?,
            ),
            (
                TrustedCeilingAxisV1::PayloadBytes,
                policy.state.max_payload_bytes,
                shape.source_payload_bytes(),
            ),
        ];
        if let Some(&(axis, limit, attempted)) = ceilings
            .iter()
            .find(|(_, limit, attempted)| attempted > limit)
        {
            return Err(DurableTransactionPreparationError::TrustedCeiling {
                axis,
                limit,
                attempted,
            });
        }
        let retention_manifest_bytes = sealed_object_bytes(&sealed, retention)?;
        let validity = self.validity.successor(&sealed)?;
        Ok(SealedDurableTransactionV1 {
            sealed,
            predecessor: self.authority.clone(),
            registry: self.registry.clone(),
            inventory: DurableSuccessorInventoryV1 {
                current,
                retained_union,
                metadata,
            },
            binding: SuccessorBindingV1::Trusted {
                policy,
                successor: Box::new(PreparedTrustedSuccessorV1 {
                    retention_manifest_bytes,
                    validity,
                    shape,
                    retained_closure: ledger,
                    retained_states,
                    retained_state_maxima: maxima,
                }),
            },
        })
    }

    pub(crate) fn trusted_successor_index(
        &self,
        records: impl Iterator<Item = Result<ObjectIndexRecordV1, IndexFormatError>>,
        new_tail: u64,
    ) -> Result<CommittedObjectIndexV1, IndexFormatError> {
        let mut index = self.store.index.clone();
        for record in records {
            index.insert(record?)?;
        }
        let derived = index
            .record_count()
            .checked_mul(INDEX_RECORD_LEN_V1)
            .and_then(|bytes| bytes.checked_add(INDEX_HEADER_LEN_V1))
            .ok_or(IndexFormatError::RecordCountOverflow)?;
        if derived != new_tail {
            return Err(IndexFormatError::CanonicalStreamTail {
                expected: derived,
                appointed: new_tail,
            });
        }
        Ok(index)
    }

    pub(crate) fn assemble_trusted_successor(
        &self,
        lease: &DurableWriterLeaseV1,
        successor: &PreparedTrustedSuccessorV1,
        slot: CatalogSlotV1,
        image: [u8; CATALOG_IMAGE_LEN],
        index: CommittedObjectIndexV1,
    ) -> Result<Self, DurableAdmissionErrorV1> {
        let authority = CatalogAuthorityTokenV1::select(image)
            .map_err(DurableStartupErrorV1::Catalog)?
            .ok_or(DurableStartupErrorV1::NoCatalogAuthority)?;
        if authority.slot() != slot {
            return Err(DurableAdmissionErrorV1::AuthorityChanged);
        }
        let store = DurableIndexedObjectStoreV1::pin(lease.directory(), &authority, index)?;
        VerifiedDagState::<_, P, ValidityOnlyV1>::from_validity(
            &store,
            slot.state(),
            successor.validity,
        )?;
        let source = &successor.retention_manifest_bytes;
        let mut retention_manifest_bytes = Vec::new();
        retention_manifest_bytes
            .try_reserve_exact(source.len())
            .map_err(|_| CanonicalFormatError::AllocationFailure {
                requested: u64::try_from(source.len()).unwrap_or(u64::MAX),
            })?;
        retention_manifest_bytes.extend_from_slice(source);
        Ok(Self {
            store,
            registry: lease.generation_leases(),
            authority,
            version: slot.state(),
            retention_manifest_bytes: retention_manifest_bytes.into_boxed_slice(),
            retention_format: self.retention_writer,
            validity: successor.validity,
            shape: successor.shape.clone(),
            retained_closure: successor.retained_closure.clone(),
            retained_states: successor.retained_states,
            retained_state_maxima: successor.retained_state_maxima,
            retention_writer: self.retention_writer,
            profile: PhantomData,
        })
    }

    #[cfg(test)]
    pub(crate) const fn committed_index(&self) -> &CommittedObjectIndexV1 { &self.store.index }
}

/// Immutable checkpoint source pinned to one exact catalog authority.
///
/// Keeping this authority separate from the mutable writer lease lets a
/// generation publisher borrow the complete admitted source while mutating
/// new pack/index files, without reopening the store, cloning the closure, or
/// introducing aliasing escape hatches.
pub(crate) struct AdmittedCheckpointSourceV1<P: SemanticProfile> {
    core: DurableAuthorityCoreV1<P>,
    retention: AdmittedRetention,
    semantic: SemanticallyAdmittedStateV1,
    report: DurableAdmissionReportV1,
    admission_requirements: DurableAdmissionRequirementsV1,
}

impl<P: SemanticProfile> AdmittedCheckpointSourceV1<P> {
    pub(crate) const fn core(&self) -> &DurableAuthorityCoreV1<P> { &self.core }

    pub(crate) fn into_core(self) -> DurableAuthorityCoreV1<P> { self.core }

    pub(super) fn preflight_writer(
        &self,
        lease: &DurableWriterLeaseV1,
    ) -> Result<(), DurableTransactionPreparationError> {
        self.core.preflight_writer(lease)
    }

    pub(crate) fn verified_state(&self) -> VerifiedDagState<'_, DurableIndexedObjectStoreV1, P> {
        VerifiedDagState::from_completed_admission(
            &self.core.store,
            self.core.version,
            self.semantic,
        )
        .expect("durable admission token is bound to its exact catalog state")
    }

    pub(crate) const fn report(&self) -> DurableAdmissionReportV1 { self.report }

    pub(crate) const fn shape(&self) -> &CurrentShapeSummaryV1 { self.core.shape() }

    pub(crate) fn preflight_checkpoint_admission(
        &self,
        limits: DurableAdmissionLimitsV1,
    ) -> Result<(), DurableAdmissionPreflightErrorV1> {
        self.admission_requirements.preflight(limits)
    }

    pub(crate) const fn authority(&self) -> &CatalogAuthorityTokenV1 { self.core.authority() }

    pub(crate) const fn retention_manifest_bytes(&self) -> &[u8] {
        self.core.retention_manifest_bytes()
    }

    pub(crate) const fn retention_format(&self) -> RetentionFormat { self.core.retention_format() }

    pub(crate) const fn retention_metadata_v2(&self) -> Option<&AdmittedRetentionMetadataV2> {
        self.retention.metadata_v2()
    }

    pub(crate) const fn retained_closure(&self) -> &StructuralClosureLedgerV1 {
        self.core.retained_closure()
    }

    /// Derive fresh-generation checkpoint authority from the exact closure
    /// and byte inventories admitted under this still-current catalog token.
    ///
    /// A V1 source copies its manifest beside the retained union under either
    /// writer. A V2 source copies its metadata closure, which holds the
    /// appointed anchor, and only the V2 writer may derive it.
    pub(crate) fn checkpoint_descriptor(
        &self,
    ) -> Result<GenerationCheckpointDescriptorV1, GenerationCheckpointErrorV1> {
        let pack = self.core.authority.slot().pack();
        match (self.retention_format(), self.core.retention_writer) {
            (RetentionFormat::V1, _) => GenerationCheckpointDescriptorV1::from_retained_union(
                P::ID,
                self.core.version,
                pack.retention_manifest(),
                self.report.current_structural.distinct_objects,
                pack.current_state_canonical_bytes(),
                &self.core.retained_closure,
            ),
            (RetentionFormat::V2, RetentionFormat::V2) => {
                GenerationCheckpointDescriptorV1::from_retained_union_and_metadata(
                    P::ID,
                    self.core.version,
                    pack.retention_manifest(),
                    self.report.current_structural.distinct_objects,
                    pack.current_state_canonical_bytes(),
                    &self.core.retained_closure,
                )
            }
            (format @ RetentionFormat::V2, RetentionFormat::V1) => {
                Err(GenerationCheckpointErrorV1::SourceRetentionFormatNotWritable(format))
            }
        }
    }

    pub(crate) fn try_visit_checkpoint_references<E>(
        &self,
        visitor: impl FnMut(ObjectRefV1) -> Result<(), E>,
    ) -> Result<(), CheckpointReferenceVisitErrorV1<E>> {
        match (self.retention_format(), self.core.retention_writer) {
            (RetentionFormat::V1, _) => try_visit_checkpoint_references(
                &self.core.retained_closure,
                self.core.authority.slot().pack().retention_manifest(),
                visitor,
            ),
            (RetentionFormat::V2, RetentionFormat::V2) => {
                try_visit_checkpoint_references_v2(&self.core.retained_closure, visitor)
            }
            (format @ RetentionFormat::V2, RetentionFormat::V1) => {
                Err(CheckpointReferenceVisitErrorV1::Checkpoint(
                    GenerationCheckpointErrorV1::SourceRetentionFormatNotWritable(format),
                ))
            }
        }
    }

    pub(crate) fn copy_checkpoint_object<S: CanonicalSink>(
        &self,
        reference: ObjectRefV1,
        sink: &mut S,
        scratch: &mut Vec<u8>,
    ) -> Result<(), DurableObjectCopyErrorV1<S::Error>> {
        self.core
            .store
            .copy_verified_canonical_with_scratch(reference, sink, scratch)
    }
}

/// Affine authority for constructing and publishing one fresh generation.
///
/// Publication must destructure this value into its disjoint fields. The
/// source remains immutably borrowed by checkpoint/index plans while the lease
/// alone owns all filesystem mutation authority.
pub(crate) struct CheckpointPublicationAuthorityV1<P: SemanticProfile> {
    lease: DurableWriterLeaseV1,
    source: AdmittedCheckpointSourceV1<P>,
}

impl<P: SemanticProfile> CheckpointPublicationAuthorityV1<P> {
    pub(crate) fn into_parts(self) -> (DurableWriterLeaseV1, AdmittedCheckpointSourceV1<P>) {
        (self.lease, self.source)
    }
}

pub(crate) struct SemanticallyAdmittedDurableWriterV1<P: SemanticProfile> {
    lease: DurableWriterLeaseV1,
    source: AdmittedCheckpointSourceV1<P>,
}

#[derive(Debug)]
#[expect(
    dead_code,
    reason = "the variant payloads are diagnostic context read only by the derived Debug impl"
)]
pub(crate) enum SuccessorInventoryError {
    Staging(StagingStoreError<DurableObjectReadErrorV1>),
    Closure(ClosureError<StagingStoreError<DurableObjectReadErrorV1>>),
    CurrentOutsideRetainedUnion {
        current: ClosureInventoryV1,
        retained_union: RetainedUnionClosureReportV1,
    },
}

fn require_current_within_retained_union(
    current: ClosureInventoryV1,
    retained_union: RetainedUnionClosureReportV1,
) -> Result<(), SuccessorInventoryError> {
    if current.distinct_objects > retained_union.distinct_objects ||
        current.canonical_bytes > retained_union.canonical_bytes
    {
        return Err(SuccessorInventoryError::CurrentOutsideRetainedUnion {
            current,
            retained_union,
        });
    }
    Ok(())
}

fn inventory_error(error: SuccessorInventoryError) -> DurableTransactionPreparationError {
    DurableTransactionPreparationError::Inventory(Box::new(error))
}

#[derive(Debug)]
pub(crate) enum DurableTransactionPreparationError {
    ObjectPlan(LogPackObjectPlanError<DurableObjectReadErrorV1>),
    Format(crate::rspace::persistence::LogPackFormatError),
    Transaction(Box<DagTransactionError<DurableObjectReadErrorV1>>),
    Canonical(CanonicalFormatError),
    Inventory(Box<SuccessorInventoryError>),
    AdmissionPreflight(DurableAdmissionPreflightErrorV1),
    AuthorityMismatch,
    AdmissionPolicyMismatch,
    RetentionFormatNotWritable(RetentionFormat),
    RetentionWrite(Box<RetentionWriteErrorV2>),
    TrustedTrial {
        domain: ClosureDomainV1,
        refusal: Box<LedgerTrialRefusalV1<StagingStoreError<DurableObjectReadErrorV1>>>,
    },
    TrustedCeiling {
        axis: TrustedCeilingAxisV1,
        limit: u64,
        attempted: u64,
    },
    PublicationModeMismatch,
    TrustedPolicyMismatch,
}

impl std::fmt::Display for DurableTransactionPreparationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ObjectPlan(error) => write!(formatter, "WAL object planning failed: {error}"),
            Self::Format(error) => write!(formatter, "WAL transaction preflight failed: {error}"),
            Self::Transaction(error) => write!(formatter, "transaction sealing failed: {error}"),
            Self::Canonical(error) => Display::fmt(error, formatter),
            Self::Inventory(error) => write!(formatter, "successor inventory failed: {error:?}"),
            Self::AdmissionPreflight(error) => Display::fmt(error, formatter),
            Self::AuthorityMismatch => {
                formatter.write_str("durable transaction authority mismatch")
            }
            Self::AdmissionPolicyMismatch => {
                formatter.write_str("durable transaction admission policy mismatch")
            }
            Self::RetentionFormatNotWritable(format) => write!(
                formatter,
                "an admitted {format:?} retention source cannot seal a successor under the V1 \
                 retention writer"
            ),
            Self::RetentionWrite(error) => Display::fmt(error, formatter),
            Self::TrustedTrial { domain, refusal } => write!(
                formatter,
                "the trusted {domain:?} ledger trial refused the successor: {refusal:?}"
            ),
            Self::TrustedCeiling {
                axis,
                limit,
                attempted,
            } => write!(
                formatter,
                "trusted successor {axis:?} ceiling exceeded: limit {limit}, required {attempted}"
            ),
            Self::PublicationModeMismatch => {
                formatter.write_str("durable transaction publication mode mismatch")
            }
            Self::TrustedPolicyMismatch => {
                formatter.write_str("durable transaction trusted policy mismatch")
            }
        }
    }
}

impl std::error::Error for DurableTransactionPreparationError {}

impl From<CanonicalFormatError> for DurableTransactionPreparationError {
    fn from(error: CanonicalFormatError) -> Self { Self::Canonical(error) }
}

impl From<RetentionWriteErrorV2> for DurableTransactionPreparationError {
    fn from(error: RetentionWriteErrorV2) -> Self { Self::RetentionWrite(Box::new(error)) }
}

/// A refused retention V2 successor write. Nothing was staged outside the
/// write's private overlay.
#[derive(Debug)]
pub(crate) enum RetentionWriteErrorV2 {
    Edit(RewriteError<StagingStoreError<DurableObjectReadErrorV1>>),
    Trial(LedgerTrialRefusalV1<StagingStoreError<DurableObjectReadErrorV1>>),
    Staging(StagingStoreError<DurableObjectReadErrorV1>),
}

impl Display for RetentionWriteErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Edit(error) => write!(formatter, "retention history edit failed: {error}"),
            Self::Trial(refusal) => {
                write!(formatter, "retention metadata trial refused the successor: {refusal:?}")
            }
            Self::Staging(error) => {
                write!(formatter, "retention metadata could not be sealed: {error}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DurableSuccessorInventoryV1 {
    pub(super) current: ClosureInventoryV1,
    pub(super) retained_union: RetainedUnionClosureReportV1,
    /// The successor's retention V2 metadata closure, as the private ledger
    /// trial derived it; none under the V1 writer.
    pub(super) metadata: Option<ClosureInventoryV1>,
}

pub(crate) struct PreparedTrustedSuccessorV1 {
    retention_manifest_bytes: Box<[u8]>,
    validity: SemanticValidityV1,
    shape: CurrentShapeSummaryV1,
    retained_closure: StructuralClosureLedgerV1,
    retained_states: u64,
    retained_state_maxima: ClosureInventoryV1,
}

#[cfg(test)]
impl PreparedTrustedSuccessorV1 {
    pub(crate) fn retention_manifest_bytes(&self) -> &[u8] { &self.retention_manifest_bytes }

    pub(crate) const fn validity(&self) -> SemanticValidityV1 { self.validity }

    pub(crate) const fn shape(&self) -> &CurrentShapeSummaryV1 { &self.shape }

    pub(crate) const fn retained_closure(&self) -> &StructuralClosureLedgerV1 {
        &self.retained_closure
    }

    pub(crate) const fn retained_states(&self) -> u64 { self.retained_states }

    pub(crate) const fn retained_state_maxima(&self) -> ClosureInventoryV1 {
        self.retained_state_maxima
    }
}

pub(crate) enum SuccessorBindingV1 {
    ColdChecked {
        limits: DurableAdmissionLimitsV1,
    },
    Trusted {
        policy: TrustedSuccessorPolicyV1,
        successor: Box<PreparedTrustedSuccessorV1>,
    },
}

pub(crate) struct SealedDurableTransactionV1<'mutation, P: SemanticProfile> {
    sealed: SealedDagTransaction<'mutation, P>,
    predecessor: CatalogAuthorityTokenV1,
    registry: GenerationLeaseRegistryV1,
    inventory: DurableSuccessorInventoryV1,
    binding: SuccessorBindingV1,
}

impl<P: SemanticProfile> SealedDurableTransactionV1<'_, P> {
    pub(crate) fn receipt(&self) -> SealedTransitionV1 { self.sealed.receipt() }

    #[cfg(test)]
    pub(crate) fn objects(&self) -> &[SealedObject] { self.sealed.objects() }

    #[cfg(test)]
    pub(crate) fn retention_manifest(&self) -> Option<ObjectRefV1> {
        self.sealed.retention_manifest()
    }

    #[cfg(test)]
    pub(crate) const fn binding(&self) -> &SuccessorBindingV1 { &self.binding }

    #[cfg(test)]
    pub(crate) const fn successor_inventory(
        &self,
    ) -> (ClosureInventoryV1, RetainedUnionClosureReportV1, Option<ClosureInventoryV1>) {
        (self.inventory.current, self.inventory.retained_union, self.inventory.metadata)
    }
}

pub(crate) struct PreparedDurableLogPackTransactionV1<'transaction, 'mutation, P: SemanticProfile> {
    prepared: PreparedLogPackTransactionV1<'transaction, 'mutation, P>,
    sealed: &'transaction SealedDurableTransactionV1<'mutation, P>,
}

impl<'transaction, 'mutation, P: SemanticProfile>
    PreparedDurableLogPackTransactionV1<'transaction, 'mutation, P>
{
    pub(crate) fn preflight(
        &self,
        authority: &CatalogAuthorityTokenV1,
        registry: &GenerationLeaseRegistryV1,
        limits: DurableAdmissionLimitsV1,
    ) -> Result<(), DurableTransactionPreparationError> {
        self.preflight_authority(authority, registry)?;
        match &self.sealed.binding {
            SuccessorBindingV1::ColdChecked { limits: sealed } if *sealed == limits => Ok(()),
            SuccessorBindingV1::ColdChecked { .. } => {
                Err(DurableTransactionPreparationError::AdmissionPolicyMismatch)
            }
            SuccessorBindingV1::Trusted { .. } => {
                Err(DurableTransactionPreparationError::PublicationModeMismatch)
            }
        }
    }

    pub(crate) fn preflight_trusted(
        &self,
        authority: &CatalogAuthorityTokenV1,
        registry: &GenerationLeaseRegistryV1,
        policy: TrustedSuccessorPolicyV1,
    ) -> Result<&'transaction PreparedTrustedSuccessorV1, DurableTransactionPreparationError> {
        self.preflight_authority(authority, registry)?;
        match &self.sealed.binding {
            SuccessorBindingV1::ColdChecked { .. } => {
                Err(DurableTransactionPreparationError::PublicationModeMismatch)
            }
            SuccessorBindingV1::Trusted {
                policy: sealed,
                successor,
            } if *sealed == policy => Ok(successor),
            SuccessorBindingV1::Trusted { .. } => {
                Err(DurableTransactionPreparationError::TrustedPolicyMismatch)
            }
        }
    }

    fn preflight_authority(
        &self,
        authority: &CatalogAuthorityTokenV1,
        registry: &GenerationLeaseRegistryV1,
    ) -> Result<(), DurableTransactionPreparationError> {
        if self.sealed.predecessor != *authority || !self.sealed.registry.shares_registry(registry)
        {
            return Err(DurableTransactionPreparationError::AuthorityMismatch);
        }
        Ok(())
    }

    pub(super) fn prepared(&self) -> &PreparedLogPackTransactionV1<'transaction, 'mutation, P> {
        &self.prepared
    }

    pub(super) fn inventory(&self) -> DurableSuccessorInventoryV1 { self.sealed.inventory }
}

struct SuccessorInventoryStoreV1<'store> {
    staging: StagingObjectStore<'store, DurableIndexedObjectStoreV1>,
    budget: AdmissionReadBudgetV1,
}

impl ImmutableObjectStore for SuccessorInventoryStoreV1<'_> {
    type Error = StagingStoreError<DurableObjectReadErrorV1>;
    type Object = crate::rspace::persistence::LayeredObject<Box<[u8]>>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        self.budget
            .charge(max_encoded_len)
            .map_err(StagingStoreError::Base)?;
        self.staging.get_bounded(digest, max_encoded_len)
    }
}

impl<P: SemanticProfile> SemanticallyAdmittedDurableWriterV1<P> {
    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this path accessor yet"
    )]
    pub(crate) fn directory_path(&self) -> std::path::PathBuf {
        self.lease.directory().path().to_path_buf()
    }

    pub(crate) fn into_parts(self) -> (DurableWriterLeaseV1, AdmittedCheckpointSourceV1<P>) {
        (self.lease, self.source)
    }

    pub(crate) fn from_parts(
        lease: DurableWriterLeaseV1,
        source: AdmittedCheckpointSourceV1<P>,
    ) -> Result<Self, DurableTransactionPreparationError> {
        source.preflight_writer(&lease)?;
        Ok(Self { lease, source })
    }

    /// Return the immutable catalog image governing this admitted writer.
    /// Compaction and reader-retention layers must compare this exact image,
    /// rather than re-open the catalog by pathname, before treating a
    /// generation as still authoritative.
    pub(crate) const fn catalog(&self) -> CatalogSlotV1 { self.source.authority().slot() }

    pub(crate) fn publish_incremental_transaction_v1(
        self,
        prepared: &PreparedDurableLogPackTransactionV1<'_, '_, P>,
        recovery_limits: DurableLimits,
        wal_limits: crate::rspace::persistence::LogPackDecodeLimits,
        admission_limits: DurableAdmissionLimitsV1,
    ) -> Result<Self, super::publication::IncrementalPublicationErrorV1> {
        let Self { mut lease, source } = self;
        let source = super::publication::publish_incremental_from_lease_v1(
            &mut lease,
            &source,
            prepared,
            recovery_limits,
            wal_limits,
            admission_limits,
        )?;
        Ok(Self { lease, source })
    }

    pub(crate) fn generation_leases(&self) -> GenerationLeaseRegistryV1 {
        self.lease.generation_leases()
    }

    pub(crate) fn verified_state(&self) -> VerifiedDagState<'_, DurableIndexedObjectStoreV1, P> {
        self.source.verified_state()
    }

    pub(crate) const fn report(&self) -> DurableAdmissionReportV1 { self.source.report() }

    pub(crate) const fn shape(&self) -> &CurrentShapeSummaryV1 { self.source.shape() }

    pub(crate) const fn authority(&self) -> &CatalogAuthorityTokenV1 { self.source.authority() }

    pub(crate) const fn retention_manifest_bytes(&self) -> &[u8] {
        self.source.retention_manifest_bytes()
    }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this closure accessor yet"
    )]
    pub(crate) const fn retained_closure(&self) -> &StructuralClosureLedgerV1 {
        self.source.retained_closure()
    }

    pub(crate) fn open_transaction<'writer, 'mutation>(
        &'writer self,
        mutation: MutationBatchViewV1<'mutation>,
        limits: DagTransactionLimits,
    ) -> Result<
        OpenDagTransaction<'writer, 'mutation, DurableIndexedObjectStoreV1, P>,
        DagTransactionError<DurableObjectReadErrorV1>,
    > {
        self.source.open_transaction(mutation, limits)
    }

    pub(crate) fn seal_transaction<'mutation>(
        &self,
        mutation: MutationBatchViewV1<'mutation>,
        limits: DagTransactionLimits,
        admission_limits: DurableAdmissionLimitsV1,
    ) -> Result<SealedDurableTransactionV1<'mutation, P>, DurableTransactionPreparationError> {
        self.source
            .seal_transaction(&self.lease, mutation, limits, admission_limits)
    }

    #[expect(
        dead_code,
        reason = "callers bind through AdmittedCheckpointSourceV1 with an explicit lease"
    )]
    pub(crate) fn bind_durable_transaction<'mutation>(
        &self,
        sealed: SealedDagTransaction<'mutation, P>,
        limits: DurableAdmissionLimitsV1,
    ) -> Result<SealedDurableTransactionV1<'mutation, P>, DurableTransactionPreparationError> {
        self.source
            .bind_durable_transaction(&self.lease, sealed, limits)
    }

    pub(crate) fn get_object_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Box<[u8]>>, DurableObjectReadErrorV1> {
        self.source.get_object_bounded(digest, max_encoded_len)
    }

    pub(crate) fn classify_transaction_objects(
        &self,
        transaction: &SealedDagTransaction<'_, P>,
    ) -> Result<LogPackObjectPlanV1, LogPackObjectPlanError<DurableObjectReadErrorV1>> {
        self.source.classify_transaction_objects(transaction)
    }

    pub(crate) fn prepare_transaction<'transaction, 'mutation>(
        &self,
        transaction: &'transaction SealedDurableTransactionV1<'mutation, P>,
        sequence: LogPackSequence,
        limits: LogPackDecodeLimits,
    ) -> Result<
        PreparedDurableLogPackTransactionV1<'transaction, 'mutation, P>,
        DurableTransactionPreparationError,
    > {
        self.source
            .prepare_transaction(&self.lease, transaction, sequence, limits)
    }

    pub(crate) fn into_checkpoint_publication(self) -> CheckpointPublicationAuthorityV1<P> {
        CheckpointPublicationAuthorityV1 {
            lease: self.lease,
            source: self.source,
        }
    }
}

impl<P: SemanticProfile> AdmittedCheckpointSourceV1<P> {
    /// Open the checked transaction adapter over this exact admitted
    /// authority. The returned transaction borrows the pinned indexed store,
    /// so callers cannot detach it from the catalog token that authenticated
    /// its base state.
    pub(crate) fn open_transaction<'writer, 'mutation>(
        &'writer self,
        mutation: MutationBatchViewV1<'mutation>,
        limits: DagTransactionLimits,
    ) -> Result<
        OpenDagTransaction<'writer, 'mutation, DurableIndexedObjectStoreV1, P>,
        DagTransactionError<DurableObjectReadErrorV1>,
    > {
        OpenDagTransaction::new(self.verified_state(), mutation, limits)
    }

    /// Seal a durable transaction together with its successor retention
    /// manifest.  This is the production boundary: callers cannot publish a
    /// target state whose retention policy was omitted from the sealed object
    /// set.  The low-level `OpenDagTransaction::seal` remains available for
    /// generic in-memory users that do not own durable retention authority.
    pub(crate) fn seal_transaction<'mutation>(
        &self,
        lease: &DurableWriterLeaseV1,
        mutation: MutationBatchViewV1<'mutation>,
        limits: DagTransactionLimits,
        admission_limits: DurableAdmissionLimitsV1,
    ) -> Result<SealedDurableTransactionV1<'mutation, P>, DurableTransactionPreparationError> {
        self.preflight_writer(lease)?;
        self.core.require_writable_retention()?;
        let sealed = self
            .open_transaction(mutation, limits)
            .and_then(OpenDagTransaction::seal)
            .map_err(|error| DurableTransactionPreparationError::Transaction(Box::new(error)))?;
        self.bind_durable_transaction(lease, sealed, admission_limits)
    }

    /// Attach the target retention metadata to a sealed transaction. The
    /// previous current state becomes historical; no durable bytes are
    /// published until this metadata is included in the prepared object set.
    ///
    /// The V1 writer seals a complete successor manifest. The V2 writer edits
    /// only the history radix path of the predecessor's current state, or
    /// migrates a V1 source once.
    pub(crate) fn bind_durable_transaction<'mutation>(
        &self,
        lease: &DurableWriterLeaseV1,
        sealed: SealedDagTransaction<'mutation, P>,
        limits: DurableAdmissionLimitsV1,
    ) -> Result<SealedDurableTransactionV1<'mutation, P>, DurableTransactionPreparationError> {
        self.preflight_writer(lease)?;
        if sealed.receipt().base() != self.authority().slot().state() ||
            sealed.receipt().profile_id() != P::ID
        {
            return Err(DurableTransactionPreparationError::AuthorityMismatch);
        }
        self.core.require_writable_retention()?;
        let appointed =
            AppointedRetention::parse(self.retention_manifest_bytes(), RetentionReader::Dual)?;
        let retained_states = appointed
            .historical_count()?
            .checked_add(2)
            .ok_or(CanonicalFormatError::CounterOverflow("successor retained states"))?;
        DurableAdmissionRequirementsV1::require(
            DurableAdmissionLimitKindV1::RetainedStates,
            limits.max_retained_states,
            retained_states,
        )
        .map_err(DurableTransactionPreparationError::AdmissionPreflight)?;
        let inventory = self
            .successor_inventory(&sealed, limits)
            .map_err(|error| DurableTransactionPreparationError::Inventory(Box::new(error)))?;
        let target = sealed.receipt().target();
        let target_state = RetainedStateV1::new(target, inventory.current.canonical_bytes)?;
        let (sealed, metadata) = match (appointed, self.core.retention_writer) {
            (AppointedRetention::V1(current), RetentionFormat::V1) => {
                let mut historical = current.historical().collect::<Result<Vec<_>, _>>()?;
                historical.push(current.current());
                let manifest =
                    RetentionManifestV1::new(target_state, historical.into_boxed_slice())?;
                (sealed.with_retention_manifest(manifest)?, None)
            }
            (AppointedRetention::V1(current), RetentionFormat::V2) => {
                let staging = self.core.retention_staging();
                let step = migrate_retention_v1(
                    &staging,
                    current,
                    target_state,
                    DagRewriteLimits::UNBOUNDED,
                )
                .map_err(RetentionWriteErrorV2::Edit)?;
                let (sealed, successor) = attach_retention_v2(
                    sealed,
                    staging,
                    &self.core.retained_closure,
                    step.anchor(),
                    &[],
                    metadata_trial_limits(limits),
                )?;
                (sealed, Some(successor.metadata_inventory()))
            }
            (AppointedRetention::V2(predecessor), RetentionFormat::V2) => {
                let staging = self.core.retention_staging();
                let step = commit_retention_v2(
                    &staging,
                    predecessor,
                    target_state,
                    DagRewriteLimits::UNBOUNDED,
                )
                .map_err(RetentionWriteErrorV2::Edit)?;
                let (sealed, successor) = attach_retention_v2(
                    sealed,
                    staging,
                    &self.core.retained_closure,
                    step.anchor(),
                    &[predecessor.object_ref()],
                    metadata_trial_limits(limits),
                )?;
                (sealed, Some(successor.metadata_inventory()))
            }
            (AppointedRetention::V2(_), RetentionFormat::V1) => {
                return Err(DurableTransactionPreparationError::RetentionFormatNotWritable(
                    RetentionFormat::V2,
                ));
            }
        };
        Ok(SealedDurableTransactionV1 {
            sealed,
            predecessor: self.authority().clone(),
            registry: self.core.registry.clone(),
            inventory: DurableSuccessorInventoryV1 {
                metadata,
                ..inventory
            },
            binding: SuccessorBindingV1::ColdChecked { limits },
        })
    }

    pub(crate) fn get_object_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Box<[u8]>>, DurableObjectReadErrorV1> {
        self.core.get_object_bounded(digest, max_encoded_len)
    }

    pub(crate) fn classify_transaction_objects(
        &self,
        transaction: &crate::rspace::persistence::SealedDagTransaction<'_, P>,
    ) -> Result<LogPackObjectPlanV1, LogPackObjectPlanError<DurableObjectReadErrorV1>> {
        self.core.classify_transaction_objects(transaction)
    }

    pub(crate) fn prepare_transaction<'transaction, 'mutation>(
        &self,
        lease: &DurableWriterLeaseV1,
        transaction: &'transaction SealedDurableTransactionV1<'mutation, P>,
        sequence: LogPackSequence,
        limits: crate::rspace::persistence::LogPackDecodeLimits,
    ) -> Result<
        PreparedDurableLogPackTransactionV1<'transaction, 'mutation, P>,
        DurableTransactionPreparationError,
    > {
        self.core
            .prepare_transaction(lease, transaction, sequence, limits)
    }

    /// Derive exact current and retained-union reports from one staged
    /// traversal.
    fn successor_inventory(
        &self,
        sealed: &SealedDagTransaction<'_, P>,
        limits: DurableAdmissionLimitsV1,
    ) -> Result<DurableSuccessorInventoryV1, SuccessorInventoryError> {
        let staging = StagingObjectStore::new(&self.core.store);
        for object in sealed.objects() {
            staging
                .stage(object.canonical_bytes_arc())
                .map_err(SuccessorInventoryError::Staging)?;
        }
        let store = SuccessorInventoryStoreV1 {
            staging,
            budget: AdmissionReadBudgetV1::new(
                limits.max_object_reads,
                limits.max_object_read_bytes,
            ),
        };
        let mut ledger = self.core.retained_closure.clone();
        let current = verify_structural_closure_in_ledger_with_limits(
            &store,
            sealed.receipt().target().manifest(),
            limits.per_state_structural,
            &mut ledger,
            RetainedUnionClosureLimitsV1 {
                max_distinct_objects: limits.max_union_objects,
                max_canonical_bytes: limits.max_union_canonical_bytes,
            },
        )
        .map_err(SuccessorInventoryError::Closure)?;
        let current = ClosureInventoryV1 {
            distinct_objects: current.report().distinct_objects,
            canonical_bytes: current.report().canonical_bytes,
        };
        let retained_union = ledger.retained_union_report();
        require_current_within_retained_union(current, retained_union)?;
        Ok(DurableSuccessorInventoryV1 {
            current,
            retained_union,
            metadata: None,
        })
    }
}

impl DurableDirectoryV1 {
    pub(crate) fn recover_and_admit_v1<P: SemanticProfile>(
        self,
        recovery_limits: DurableLimits,
        wal_limits: LogPackDecodeLimits,
        admission_limits: DurableAdmissionLimitsV1,
    ) -> Result<SemanticallyAdmittedDurableWriterV1<P>, DurableAdmissionErrorV1> {
        let physical = self.recover_physical_writer_v1(recovery_limits, wal_limits)?;
        admit_physical_writer::<P>(physical, admission_limits)
    }
}

pub(crate) fn admit_physical_writer<P: SemanticProfile>(
    physical: PhysicallyRecoveredDurableWriterV1,
    limits: DurableAdmissionLimitsV1,
) -> Result<SemanticallyAdmittedDurableWriterV1<P>, DurableAdmissionErrorV1> {
    let (lease, recovered) = physical.into_admission_parts();
    let source = admit_physical_source::<P>(&lease, recovered, limits)?;
    Ok(SemanticallyAdmittedDurableWriterV1 { lease, source })
}

pub(crate) fn acquire_cold_checked<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    core: &DurableAuthorityCoreV1<P>,
    recovery_limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    admission_limits: DurableAdmissionLimitsV1,
) -> Result<AdmittedCheckpointSourceV1<P>, DurableAdmissionErrorV1> {
    let physical = recover_physical_from_lease_v1(lease, recovery_limits, wal_limits)?;
    let source = admit_physical_source::<P>(lease, physical, admission_limits)?;
    if !core.authority().matches_exactly(source.authority().image()) {
        return Err(DurableAdmissionErrorV1::AuthorityChanged);
    }
    Ok(source)
}

#[cfg(test)]
std::thread_local! {
    static COLD_ADMISSIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_cold_admissions() { COLD_ADMISSIONS.set(0); }

#[cfg(test)]
pub(crate) fn cold_admissions() -> u64 { COLD_ADMISSIONS.get() }

pub(crate) fn admit_physical_source<P: SemanticProfile>(
    lease: &DurableWriterLeaseV1,
    recovered: IndexBoundPhysicalRecoveryV1,
    limits: DurableAdmissionLimitsV1,
) -> Result<AdmittedCheckpointSourceV1<P>, DurableAdmissionErrorV1> {
    #[cfg(test)]
    COLD_ADMISSIONS.set(COLD_ADMISSIONS.get().saturating_add(1));
    let (authority, index, terminal_commit, startup_disposition, startup_work) =
        recovered.into_admission_parts();
    let catalog = authority.slot();
    if catalog.profile_id() != P::ID {
        return Err(DurableAdmissionErrorV1::ProfileMismatch {
            expected: P::ID,
            actual: catalog.profile_id(),
        });
    }

    let retention_writer = lease.directory().retention_writer();
    if let (RetentionFormat::V2, reader @ RetentionReader::V1Only) =
        (retention_writer, lease.directory().retention_reader())
    {
        return Err(DurableAdmissionErrorV1::RetentionWriterUnreadable {
            writer: retention_writer,
            reader,
        });
    }

    let store = DurableIndexedObjectStoreV1::pin(lease.directory(), &authority, index)?;
    require_same_authority(lease.directory(), &authority)?;
    let bounded = BoundedAdmissionStoreV1::new(&store, limits);
    let retention_ref = catalog.pack().retention_manifest();
    let retention_manifest_bytes = load_exact(&bounded, retention_ref)?;
    let retention =
        AppointedRetention::parse(&retention_manifest_bytes, lease.directory().retention_reader())?;
    let historical_count = retention.historical_count()?;
    let retained_states = historical_count
        .checked_add(1)
        .ok_or(CanonicalFormatError::CounterOverflow("retained states"))?;
    require_limit("retained states", limits.max_retained_states, retained_states)?;

    let retained_current = retention.current();
    if retained_current.state() != catalog.state() {
        return Err(DurableAdmissionErrorV1::RetentionCurrentMismatch {
            catalog: catalog.state(),
            manifest: retained_current.state(),
        });
    }
    let (historical, admitted_retention) = match retention {
        AppointedRetention::V1(manifest) => {
            (HistoricalEntriesV1::Manifest(manifest.historical()), AdmittedRetention::V1)
        }
        AppointedRetention::V2(anchor) => {
            let (entries, metadata) = admit_retention_history_v2(&bounded, anchor)?.into_parts();
            (
                HistoricalEntriesV1::Radix(entries.into_iter()),
                AdmittedRetention::V2(AdmittedRetentionMetadataV2::new(anchor, metadata)),
            )
        }
    };
    let (retention_metadata_objects, retention_metadata_canonical_bytes) =
        match admitted_retention.metadata_v2() {
            None => (1, retention_ref.encoded_len()),
            Some(admitted) => {
                (admitted.metadata().object_count(), admitted.metadata().canonical_bytes())
            }
        };
    if let TerminalCommitViewV1::GenerationCheckpoint(commit) = terminal_commit {
        let descriptor = commit.descriptor();
        compare_inventory(
            InventoryMismatchKindV1::CheckpointRetentionMetadataObjects,
            descriptor
                .data_objects()
                .checked_sub(descriptor.retained_union_objects())
                .expect("a parsed checkpoint descriptor's DATA objects cover its retained union"),
            retention_metadata_objects,
        )?;
        compare_inventory(
            InventoryMismatchKindV1::CheckpointRetentionMetadataBytes,
            descriptor
                .data_canonical_bytes()
                .get()
                .checked_sub(descriptor.retained_union_canonical_bytes().get())
                .expect("a parsed checkpoint descriptor's DATA bytes cover its retained union"),
            retention_metadata_canonical_bytes,
        )?;
    }

    let union_limits = RetainedUnionClosureLimitsV1 {
        max_distinct_objects: limits.max_union_objects,
        max_canonical_bytes: limits.max_union_canonical_bytes,
    };
    let mut ledger = StructuralClosureLedgerV1::new();
    let current_structural = verify_structural_closure_in_ledger_with_credit(
        &bounded,
        catalog.state().manifest(),
        limits.per_state_structural,
        &mut ledger,
        union_limits,
        ClosureCreditV1::CURRENT_AND_RETAINED,
    )?;
    let mut maximum_per_state_structural = current_structural.report();
    compare_inventory(
        InventoryMismatchKindV1::CurrentState,
        retained_current.canonical_bytes(),
        current_structural.report().canonical_bytes,
    )?;
    compare_inventory(
        InventoryMismatchKindV1::CurrentState,
        catalog.pack().current_state_canonical_bytes(),
        current_structural.report().canonical_bytes,
    )?;
    if let TerminalCommitViewV1::GenerationCheckpoint(commit) = terminal_commit {
        compare_inventory(
            InventoryMismatchKindV1::CheckpointCurrentStateObjects,
            commit.descriptor().current_state_objects(),
            current_structural.report().distinct_objects,
        )?;
    }

    let mut aggregate_root_visits = current_structural.report().distinct_objects;
    let mut aggregate_references = current_structural.report().references;
    require_limit(
        "aggregate retained-root visits",
        limits.max_aggregate_root_visits,
        aggregate_root_visits,
    )?;
    require_limit(
        "aggregate retained references",
        limits.max_aggregate_references,
        aggregate_references,
    )?;

    for retained in historical {
        let retained = retained?;
        if retained.state().generation() >= catalog.state().generation() {
            return Err(DurableAdmissionErrorV1::NonHistoricalState {
                current_generation: catalog.state().generation(),
                retained_generation: retained.state().generation(),
            });
        }
        let admitted = verify_structural_closure_in_ledger_with_limits(
            &bounded,
            retained.state().manifest(),
            limits.per_state_structural,
            &mut ledger,
            union_limits,
        )?;
        componentwise_max_structural(&mut maximum_per_state_structural, admitted.report());
        compare_inventory(
            InventoryMismatchKindV1::RetainedState {
                generation: retained.state().generation(),
            },
            retained.canonical_bytes(),
            admitted.report().canonical_bytes,
        )?;
        aggregate_root_visits = aggregate_root_visits
            .checked_add(admitted.report().distinct_objects)
            .ok_or(CanonicalFormatError::CounterOverflow("aggregate retained-root visits"))?;
        aggregate_references = aggregate_references
            .checked_add(admitted.report().references)
            .ok_or(CanonicalFormatError::CounterOverflow("aggregate retained references"))?;
        require_limit(
            "aggregate retained-root visits",
            limits.max_aggregate_root_visits,
            aggregate_root_visits,
        )?;
        require_limit(
            "aggregate retained references",
            limits.max_aggregate_references,
            aggregate_references,
        )?;
    }

    if let Some(admitted) = admitted_retention.metadata_v2() {
        ledger.seed_retention_metadata(admitted.anchor().object_ref(), admitted.metadata())?;
    }

    let retained_union = ledger.retained_union_report();
    if let TerminalCommitViewV1::GenerationCheckpoint(commit) = terminal_commit {
        compare_inventory(
            InventoryMismatchKindV1::CheckpointRetainedUnionObjects,
            commit.descriptor().retained_union_objects(),
            retained_union.distinct_objects,
        )?;
    }
    compare_inventory(
        InventoryMismatchKindV1::RetainedUnion,
        catalog.pack().retained_canonical_bytes(),
        retained_union.canonical_bytes,
    )?;
    let (semantic, shape) = verify_rspace_semantics_after_structural_with_shape::<P, _>(
        &bounded,
        current_structural,
        limits.current_semantics,
    )?;
    require_same_authority(lease.directory(), &authority)?;

    let report = DurableAdmissionReportV1 {
        startup_disposition,
        startup_work,
        current_structural: current_structural.report(),
        current_semantics: semantic.report(),
        retained_states,
        aggregate_root_visits,
        aggregate_references,
        retained_union,
        retention_manifest_canonical_bytes: retention_ref.encoded_len(),
        retention_metadata_objects,
        retention_metadata_canonical_bytes,
        object_reads: bounded.budget.reads(),
        object_read_bytes: bounded.budget.bytes(),
    };
    let admission_requirements = DurableAdmissionRequirementsV1 {
        per_state_structural: maximum_per_state_structural,
        current_semantics: report.current_semantics,
        retained_states: report.retained_states,
        union_objects: report.retained_union.distinct_objects,
        union_canonical_bytes: report.retained_union.canonical_bytes,
        aggregate_root_visits: report.aggregate_root_visits,
        aggregate_references: report.aggregate_references,
        object_reads: report.object_reads,
        object_read_bytes: report.object_read_bytes,
    };
    Ok(AdmittedCheckpointSourceV1 {
        core: DurableAuthorityCoreV1 {
            store,
            registry: lease.generation_leases(),
            authority,
            version: catalog.state(),
            retention_manifest_bytes,
            retention_format: admitted_retention.format(),
            validity: SemanticValidityV1::from_admission(semantic),
            shape,
            retained_closure: ledger,
            retained_states,
            retained_state_maxima: ClosureInventoryV1 {
                distinct_objects: maximum_per_state_structural.distinct_objects,
                canonical_bytes: maximum_per_state_structural.canonical_bytes,
            },
            retention_writer,
            profile: PhantomData,
        },
        retention: admitted_retention,
        semantic,
        report,
        admission_requirements,
    })
}

/// Account a staged retention V2 successor in the metadata closure with
/// one private ledger trial, which activates the successor anchor before
/// it deactivates the predecessor's, then attach the staged metadata.
/// The trial visits only the objects whose metadata count crosses zero.
fn attach_retention_v2<'mutation, P: SemanticProfile>(
    sealed: SealedDagTransaction<'mutation, P>,
    staging: StagingObjectStore<'_, DurableIndexedObjectStoreV1>,
    ledger: &StructuralClosureLedgerV1,
    anchor: RetentionAnchorV2,
    predecessor: &[ObjectRefV1],
    limits: LedgerTrialLimitsV1,
) -> Result<(SealedDagTransaction<'mutation, P>, StructuralClosureLedgerV1), RetentionWriteErrorV2>
{
    let trial = run_ledger_trial(
        &staging,
        ledger,
        ClosureDomainV1::Metadata,
        &[anchor.object_ref()],
        predecessor,
        limits,
    )
    .map_err(RetentionWriteErrorV2::Trial)?;
    let sealed = sealed
        .with_retention_metadata(anchor, staging)
        .map_err(RetentionWriteErrorV2::Staging)?;
    Ok((sealed, trial.successor))
}

fn trusted_retention_error(error: RetentionWriteErrorV2) -> DurableTransactionPreparationError {
    match error {
        RetentionWriteErrorV2::Trial(refusal) => DurableTransactionPreparationError::TrustedTrial {
            domain: ClosureDomainV1::Metadata,
            refusal: Box::new(refusal),
        },
        error @ (RetentionWriteErrorV2::Edit(_) | RetentionWriteErrorV2::Staging(_)) => {
            error.into()
        }
    }
}

fn sealed_object_bytes<P: SemanticProfile>(
    sealed: &SealedDagTransaction<'_, P>,
    reference: ObjectRefV1,
) -> Result<Box<[u8]>, CanonicalFormatError> {
    let objects = sealed.objects();
    let source = match objects
        .binary_search_by_key(&reference.digest(), |object| object.reference().digest())
    {
        Ok(index) => objects[index].canonical_bytes(),
        Err(_) => return Err(CanonicalFormatError::MissingObject(reference.digest())),
    };
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(source.len())
        .map_err(|_| CanonicalFormatError::AllocationFailure {
            requested: u64::try_from(source.len()).unwrap_or(u64::MAX),
        })?;
    bytes.extend_from_slice(source);
    Ok(bytes.into_boxed_slice())
}

/// The metadata trial's budgets, drawn from the admission limits that bound
/// the cold reader's walk of the same closure: no new limit is introduced.
const fn metadata_trial_limits(limits: DurableAdmissionLimitsV1) -> LedgerTrialLimitsV1 {
    LedgerTrialLimitsV1::UNBOUNDED
        .with(LedgerTrialAxisV1::UnionObjects, limits.max_object_reads)
        .with(LedgerTrialAxisV1::UnionBytes, limits.max_object_read_bytes)
        .with(LedgerTrialAxisV1::Scratch, limits.per_state_structural.max_worklist)
        .with(LedgerTrialAxisV1::References, limits.max_aggregate_references)
        .with(LedgerTrialAxisV1::Expansions, limits.max_object_reads)
        .with(LedgerTrialAxisV1::QuotedBytes, limits.max_object_read_bytes)
        .with(LedgerTrialAxisV1::FinalObjects, limits.max_object_reads)
        .with(LedgerTrialAxisV1::FinalBytes, limits.max_object_read_bytes)
}

/// The historical entries of either retention format, in strict state order.
enum HistoricalEntriesV1<'a> {
    Manifest(RetainedStateIter<'a>),
    Radix(std::vec::IntoIter<RetainedStateV1>),
}

impl Iterator for HistoricalEntriesV1<'_> {
    type Item = Result<RetainedStateV1, CanonicalFormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Manifest(entries) => entries.next(),
            Self::Radix(entries) => entries.next().map(Ok),
        }
    }
}

fn require_same_authority(
    directory: &DurableDirectoryV1,
    authority: &CatalogAuthorityTokenV1,
) -> Result<(), DurableAdmissionErrorV1> {
    let current = read_catalog_token(directory)?;
    if current.matches_exactly(authority.image()) {
        Ok(())
    } else {
        Err(DurableAdmissionErrorV1::AuthorityChanged)
    }
}

fn load_exact<S: ImmutableObjectStore>(
    store: &S,
    reference: ObjectRefV1,
) -> Result<Box<[u8]>, DurableAdmissionErrorV1>
where
    S::Error: Into<DurableObjectReadErrorV1>,
    S::Object: Into<Box<[u8]>>,
{
    let bytes = store
        .get_bounded(&reference.digest(), reference.encoded_len())
        .map_err(Into::into)?
        .ok_or(CanonicalFormatError::MissingObject(reference.digest()))?
        .into();
    let actual_len = checked_len(bytes.len(), "retention manifest object")?;
    if actual_len != reference.encoded_len() {
        return Err(CanonicalFormatError::ObjectLengthMismatch {
            expected: reference.encoded_len(),
            actual: actual_len,
        }
        .into());
    }
    let actual = ObjectDigest::of_canonical_bytes(&bytes);
    if actual != reference.digest() {
        return Err(CanonicalFormatError::DigestMismatch {
            expected: reference.digest(),
            actual,
        }
        .into());
    }
    Ok(bytes)
}

fn compare_inventory(
    kind: InventoryMismatchKindV1,
    declared: u64,
    computed: u64,
) -> Result<(), DurableAdmissionErrorV1> {
    if declared == computed {
        Ok(())
    } else {
        Err(DurableAdmissionErrorV1::InventoryMismatch {
            kind,
            declared,
            computed,
        })
    }
}

fn require_limit(
    resource: &'static str,
    limit: u64,
    attempted: u64,
) -> Result<(), DurableAdmissionErrorV1> {
    if attempted <= limit {
        Ok(())
    } else {
        Err(CanonicalFormatError::ResourceLimit {
            resource,
            limit,
            attempted,
        }
        .into())
    }
}
