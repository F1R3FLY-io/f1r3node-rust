//! Affine publication of one self-contained durable RSpace generation.
//!
//! The publisher consumes the old semantically admitted writer authority. It
//! first makes an exact successor pack and index durable, then performs the
//! two-slot catalog compare-and-swap, and finally reconstructs authority by
//! running the ordinary restart path through semantic admission. No success
//! or error path can return the stale pre-publication writer.

use std::fmt::{Display, Formatter};
#[cfg(test)]
use std::path::Path;

use super::admission::{
    AdmittedCheckpointSourceV1, CheckpointPublicationAuthorityV1, DurableAdmissionErrorV1,
    DurableAdmissionLimitsV1, DurableAdmissionPreflightErrorV1, DurableAuthorityCoreV1,
    DurableObjectReadErrorV1, DurableSuccessorInventoryV1, DurableTransactionPreparationError,
    PreparedDurableLogPackTransactionV1, SemanticallyAdmittedDurableWriterV1,
    TrustedSuccessorPolicyV1, admit_physical_source, admit_physical_writer,
};
use super::catalog::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CatalogAuthorityTokenV1, CatalogPackStateV1, CatalogSlotV1,
};
use super::index::{
    CanonicalIndexStreamV1, IndexFormatError, IndexGenerationV1,
    PreparedGenerationCheckpointIndexV1, PreparedGenerationCheckpointIndexVisitErrorV1,
    PreparedIndexAppendV1,
};
#[cfg(test)]
use super::io::DurableIoOperation;
use super::io::{
    CatalogSlotIndexV1, DurableCanonicalAppendSinkV1, DurableIoError, DurableWriterLeaseV1,
    FileLengthExpectation,
};
use super::limits::{DurableLimitError, DurableLimitKind, DurableLimits};
use super::pack::{PackFormatError, PackHeaderV1, PackTerminalIdentityV1};
use super::startup::{
    DurableStartupErrorV1, recover_physical_from_lease_v1, recover_physical_with_lease_v1,
    resolve_reclamation_candidate_v1,
};
use crate::rspace::persistence::canonical::CanonicalSink;
use crate::rspace::persistence::closure::{
    ClosureDomainV1, ClosureInventoryV1, StructuralClosureLedgerV1,
};
use crate::rspace::persistence::durable::PACK_HEADER_LEN_V1;
use crate::rspace::persistence::retention::v2_reader::admit_retention_history_v2;
use crate::rspace::persistence::retention::{
    KnownRootLeaseError, KnownRootLeaseV1, RetainedRootRegistryV1, RetainedRootSealV1,
    RetainedRootSnapshotV1, RetainedRootV1, RetentionManifestV1, RetentionManifestView,
    RetentionRegistryError,
};
use crate::rspace::persistence::semantic::SemanticProfile;
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, GenerationCheckpointCommitRecordV1,
    GenerationCheckpointPrepareIdentityV1, GenerationCheckpointPrepareWriteErrorV1, LogPackAddress,
    LogPackDecodeLimits, LogPackFormatError, LogPackWriteError, PackId, PrepareRecordIdentityV1,
    PreparedGenerationCheckpointV1, PreparedLogPackTransactionV1, TRANSACTION_COMMIT_RECORD_LEN_V1,
    TransactionCommitRecordV1,
};
use crate::rspace::persistence::{GenerationLeaseKeyV1, ObjectDigest, ObjectRefV1};

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GenerationCheckpointPublicationHookPointV1 {
    AfterPreflightBeforeCleanup,
    AfterCleanup,
    AfterPrepareWrite,
    BeforeCatalogCompareAndSwap,
    AfterPrepareSync,
    AfterIndexWrite,
    AfterIndexSync,
    AfterCommitWrite,
    AfterCommitSync,
    AfterArtifactDirectorySync,
    AfterCatalogCasVerifiedBeforeSlotWrite,
    AfterCatalogSlotWrite,
    AfterCatalogFileSyncBeforeDirectorySync,
    AfterCatalogDirectorySync,
    AfterCatalogSyncBeforeRecovery,
    AfterPhysicalRecovery,
    BeforeSemanticAdmission,
    AfterSemanticAdmission,
}

#[cfg(test)]
type GenerationCheckpointPublicationHookV1 =
    (GenerationCheckpointPublicationHookPointV1, Box<dyn FnOnce() + 'static>);

#[cfg(test)]
thread_local! {
    static GENERATION_CHECKPOINT_PUBLICATION_HOOK: std::cell::RefCell<
        Option<GenerationCheckpointPublicationHookV1>,
    > = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    static GENERATION_CHECKPOINT_PUBLICATION_FAILPOINT: std::cell::Cell<
        Option<GenerationCheckpointPublicationHookPointV1>,
    > = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
thread_local! {
    static GENERATION_CHECKPOINT_PUBLICATION_TRACE: std::cell::RefCell<
        Option<Vec<GenerationCheckpointPublicationHookPointV1>>,
    > = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn start_generation_checkpoint_publication_trace() {
    GENERATION_CHECKPOINT_PUBLICATION_TRACE.with(|trace| {
        let previous = trace.borrow_mut().replace(Vec::new());
        assert!(previous.is_none(), "generation-checkpoint publication trace already active");
    });
}

#[cfg(test)]
pub(crate) fn take_generation_checkpoint_publication_trace()
-> Vec<GenerationCheckpointPublicationHookPointV1> {
    GENERATION_CHECKPOINT_PUBLICATION_TRACE.with(|trace| {
        trace
            .borrow_mut()
            .take()
            .expect("generation-checkpoint publication trace is active")
    })
}

#[cfg(test)]
pub(crate) fn arm_generation_checkpoint_publication_hook(
    point: GenerationCheckpointPublicationHookPointV1,
    hook: impl FnOnce() + 'static,
) {
    GENERATION_CHECKPOINT_PUBLICATION_HOOK.with(|armed| {
        let previous = armed.borrow_mut().replace((point, Box::new(hook)));
        assert!(previous.is_none(), "generation-checkpoint publication hook already armed");
    });
}

#[cfg(test)]
pub(crate) fn arm_generation_checkpoint_publication_failpoint(
    point: GenerationCheckpointPublicationHookPointV1,
) {
    GENERATION_CHECKPOINT_PUBLICATION_FAILPOINT.with(|armed| {
        assert!(armed.replace(Some(point)).is_none(), "publication failpoint already armed");
    });
}

#[cfg(test)]
fn trip_generation_checkpoint_publication_failpoint(
    point: GenerationCheckpointPublicationHookPointV1,
    path: &Path,
) -> Result<(), GenerationCheckpointPublicationErrorV1> {
    GENERATION_CHECKPOINT_PUBLICATION_FAILPOINT.with(|armed| {
        if armed.get() != Some(point) {
            return Ok(());
        }
        armed.set(None);
        let io = DurableIoError::Io {
            operation: DurableIoOperation::SyncFile,
            path: path.to_path_buf(),
            source: std::io::Error::other(format!(
                "injected generation-checkpoint publication failure at {point:?}"
            )),
        };
        let error = match point {
            GenerationCheckpointPublicationHookPointV1::AfterPreflightBeforeCleanup |
            GenerationCheckpointPublicationHookPointV1::AfterCleanup |
            GenerationCheckpointPublicationHookPointV1::AfterPrepareWrite |
            GenerationCheckpointPublicationHookPointV1::AfterPrepareSync |
            GenerationCheckpointPublicationHookPointV1::AfterIndexWrite |
            GenerationCheckpointPublicationHookPointV1::AfterIndexSync |
            GenerationCheckpointPublicationHookPointV1::AfterCommitWrite |
            GenerationCheckpointPublicationHookPointV1::AfterCommitSync |
            GenerationCheckpointPublicationHookPointV1::AfterArtifactDirectorySync |
            GenerationCheckpointPublicationHookPointV1::BeforeCatalogCompareAndSwap |
            GenerationCheckpointPublicationHookPointV1::AfterCatalogCasVerifiedBeforeSlotWrite => {
                GenerationCheckpointPublicationErrorV1::Io(io)
            }
            GenerationCheckpointPublicationHookPointV1::AfterCatalogSlotWrite => {
                GenerationCheckpointPublicationErrorV1::CatalogCommitIndeterminate(io)
            }
            GenerationCheckpointPublicationHookPointV1::AfterCatalogFileSyncBeforeDirectorySync => {
                GenerationCheckpointPublicationErrorV1::PublishedCatalogDirectory(io)
            }
            GenerationCheckpointPublicationHookPointV1::AfterCatalogDirectorySync |
            GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery => {
                GenerationCheckpointPublicationErrorV1::PublishedRecovery(Box::new(
                    DurableStartupErrorV1::Io(io),
                ))
            }
            GenerationCheckpointPublicationHookPointV1::AfterPhysicalRecovery |
            GenerationCheckpointPublicationHookPointV1::BeforeSemanticAdmission |
            GenerationCheckpointPublicationHookPointV1::AfterSemanticAdmission => {
                GenerationCheckpointPublicationErrorV1::PublishedAdmission(Box::new(
                    DurableAdmissionErrorV1::InjectedFailure(
                        "generation-checkpoint publication phase",
                    ),
                ))
            }
        };
        Err(error)
    })
}

#[cfg(test)]
fn run_generation_checkpoint_publication_hook(point: GenerationCheckpointPublicationHookPointV1) {
    GENERATION_CHECKPOINT_PUBLICATION_TRACE.with(|trace| {
        if let Some(trace) = trace.borrow_mut().as_mut() {
            trace.push(point);
        }
    });
    GENERATION_CHECKPOINT_PUBLICATION_HOOK.with(|armed| {
        let should_run = armed
            .borrow()
            .as_ref()
            .is_some_and(|(armed_point, _)| *armed_point == point);
        if should_run {
            let (_, hook) = armed.borrow_mut().take().expect("checked publication hook");
            hook();
        }
    });
}

#[derive(Debug)]
pub(crate) enum GenerationCheckpointPublicationErrorV1 {
    Io(DurableIoError),
    LogPack(Box<LogPackFormatError>),
    Pack(Box<PackFormatError>),
    Index(Box<IndexFormatError>),
    Durable(Box<super::DurableFormatError>),
    Limit(DurableLimitError),
    AdmissionPreflight(DurableAdmissionPreflightErrorV1),
    CheckpointWrite(
        Box<GenerationCheckpointPrepareWriteErrorV1<DurableObjectReadErrorV1, DurableIoError>>,
    ),
    CheckpointIndex(
        Box<PreparedGenerationCheckpointIndexVisitErrorV1<CheckpointIndexWriteErrorV1>>,
    ),
    SuccessorIdentityMismatch {
        planned_pack: PackId,
        prepared_pack: PackId,
        planned_index: IndexGenerationV1,
        prepared_index: IndexGenerationV1,
    },
    CatalogCompareAndSwap,
    CatalogCommitIndeterminate(DurableIoError),
    PublishedCatalogDirectory(DurableIoError),
    PublishedRecovery(Box<DurableStartupErrorV1>),
    PublishedAdmission(Box<DurableAdmissionErrorV1>),
    RetainedRootRegistry(RetentionRegistryError),
    RetainedRootManifest(Box<DurableAdmissionErrorV1>),
    RetainedRootNotCarried {
        root: RetainedRootV1,
    },
}

/// Errors from the append-only portion of one incremental transaction.
/// Catalog publication is intentionally a later phase so a caller can retain
/// the old authority until PREPARE, index, and commit ordering is complete.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "the variant payloads are diagnostic context read only by the derived Debug impl"
)]
pub(crate) enum IncrementalAppendErrorV1 {
    Io(DurableIoError),
    LogPack(Box<LogPackFormatError>),
    Index(Box<IndexFormatError>),
}

#[derive(Debug)]
#[expect(
    dead_code,
    reason = "most variant payloads are diagnostic context read only by the derived Debug impl"
)]
pub(crate) enum IncrementalPublicationErrorV1 {
    Append(IncrementalAppendErrorV1),
    Io(DurableIoError),
    LogPack(Box<LogPackFormatError>),
    Durable(Box<super::DurableFormatError>),
    CatalogCompareAndSwap,
    CatalogCommitIndeterminate(DurableIoError),
    PublishedCatalogDirectory(DurableIoError),
    PublishedRecovery(Box<DurableStartupErrorV1>),
    PublishedAdmission(Box<DurableAdmissionErrorV1>),
    /// The published successor's cold-admitted retention V2 metadata closure
    /// differs from the one the private ledger trial derived.
    PublishedMetadataInventory {
        trial: ClosureInventoryV1,
        admitted: ClosureInventoryV1,
    },
    BaseStateMismatch {
        expected: crate::rspace::persistence::StateVersionV1,
        actual: crate::rspace::persistence::StateVersionV1,
    },
    ProfileMismatch,
    MissingRetentionManifest,
    Preparation(DurableTransactionPreparationError),
    TrustedSuccessorAssembly(Box<DurableAdmissionErrorV1>),
}

impl From<IncrementalAppendErrorV1> for IncrementalPublicationErrorV1 {
    fn from(error: IncrementalAppendErrorV1) -> Self { Self::Append(error) }
}

impl IncrementalPublicationErrorV1 {
    pub(crate) const fn publication_outcome(&self) -> GenerationCheckpointPublicationOutcomeV1 {
        use GenerationCheckpointPublicationOutcomeV1 as Outcome;
        match self {
            Self::CatalogCommitIndeterminate(_) => Outcome::Indeterminate,
            Self::PublishedCatalogDirectory(_) |
            Self::PublishedRecovery(_) |
            Self::PublishedAdmission(_) |
            Self::PublishedMetadataInventory { .. } => Outcome::Published,
            Self::Append(_) |
            Self::Io(_) |
            Self::LogPack(_) |
            Self::Durable(_) |
            Self::CatalogCompareAndSwap |
            Self::BaseStateMismatch { .. } |
            Self::ProfileMismatch |
            Self::MissingRetentionManifest |
            Self::Preparation(_) |
            Self::TrustedSuccessorAssembly(_) => Outcome::DefinitelyNotPublished,
        }
    }
}

#[cfg(test)]
fn incremental_publication_test_point(
    point: GenerationCheckpointPublicationHookPointV1,
    path: &Path,
) -> Result<(), IncrementalPublicationErrorV1> {
    run_generation_checkpoint_publication_hook(point);
    trip_generation_checkpoint_publication_failpoint(point, path).map_err(|error| match error {
        GenerationCheckpointPublicationErrorV1::Io(error) => {
            IncrementalPublicationErrorV1::Io(error)
        }
        GenerationCheckpointPublicationErrorV1::CatalogCommitIndeterminate(error) => {
            IncrementalPublicationErrorV1::CatalogCommitIndeterminate(error)
        }
        GenerationCheckpointPublicationErrorV1::PublishedCatalogDirectory(error) => {
            IncrementalPublicationErrorV1::PublishedCatalogDirectory(error)
        }
        GenerationCheckpointPublicationErrorV1::PublishedRecovery(error) => {
            IncrementalPublicationErrorV1::PublishedRecovery(error)
        }
        GenerationCheckpointPublicationErrorV1::PublishedAdmission(error) => {
            IncrementalPublicationErrorV1::PublishedAdmission(error)
        }
        other => panic!("unexpected publication test fault: {other:?}"),
    })
}

impl From<DurableIoError> for IncrementalPublicationErrorV1 {
    fn from(error: DurableIoError) -> Self { Self::Io(error) }
}

impl From<LogPackFormatError> for IncrementalPublicationErrorV1 {
    fn from(error: LogPackFormatError) -> Self { Self::LogPack(Box::new(error)) }
}

impl From<super::DurableFormatError> for IncrementalPublicationErrorV1 {
    fn from(error: super::DurableFormatError) -> Self { Self::Durable(Box::new(error)) }
}

impl From<DurableIoError> for IncrementalAppendErrorV1 {
    fn from(error: DurableIoError) -> Self { Self::Io(error) }
}

impl From<IndexFormatError> for IncrementalAppendErrorV1 {
    fn from(error: IndexFormatError) -> Self { Self::Index(Box::new(error)) }
}

impl From<LogPackFormatError> for IncrementalAppendErrorV1 {
    fn from(error: LogPackFormatError) -> Self { Self::LogPack(Box::new(error)) }
}

impl From<LogPackWriteError<DurableIoError>> for IncrementalAppendErrorV1 {
    fn from(error: LogPackWriteError<DurableIoError>) -> Self {
        match error {
            LogPackWriteError::Sink(error) => Self::Io(error),
            LogPackWriteError::Invariant(error) => Self::LogPack(Box::new(error)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GenerationCheckpointPublicationOutcomeV1 {
    DefinitelyNotPublished,
    Indeterminate,
    Published,
}

impl GenerationCheckpointPublicationErrorV1 {
    /// Classify whether the catalog publication boundary was crossed without
    /// requiring callers to maintain a fragile list of error variants.
    pub(crate) const fn publication_outcome(&self) -> GenerationCheckpointPublicationOutcomeV1 {
        match self {
            Self::CatalogCommitIndeterminate(_) => {
                GenerationCheckpointPublicationOutcomeV1::Indeterminate
            }
            Self::PublishedCatalogDirectory(_) |
            Self::PublishedRecovery(_) |
            Self::PublishedAdmission(_) => GenerationCheckpointPublicationOutcomeV1::Published,
            Self::Io(_) |
            Self::LogPack(_) |
            Self::Pack(_) |
            Self::Index(_) |
            Self::Durable(_) |
            Self::Limit(_) |
            Self::AdmissionPreflight(_) |
            Self::CheckpointWrite(_) |
            Self::CheckpointIndex(_) |
            Self::SuccessorIdentityMismatch { .. } |
            Self::CatalogCompareAndSwap => {
                GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
            }
            Self::RetainedRootRegistry(_) |
            Self::RetainedRootManifest(_) |
            Self::RetainedRootNotCarried { .. } => {
                GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
            }
        }
    }
}

impl Display for GenerationCheckpointPublicationErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::LogPack(error) => Display::fmt(error, formatter),
            Self::Pack(error) => Display::fmt(error, formatter),
            Self::Index(error) => Display::fmt(error, formatter),
            Self::Durable(error) => Display::fmt(error, formatter),
            Self::Limit(error) => Display::fmt(error, formatter),
            Self::AdmissionPreflight(error) => Display::fmt(error, formatter),
            Self::CheckpointWrite(error) => Display::fmt(error, formatter),
            Self::CheckpointIndex(error) => Display::fmt(error, formatter),
            Self::SuccessorIdentityMismatch {
                planned_pack,
                prepared_pack,
                planned_index,
                prepared_index,
            } => write!(
                formatter,
                "successor cleanup derived pack/index ({}, {}) but publication preflight derived \
                 ({}, {})",
                prepared_pack.get(),
                prepared_index.get(),
                planned_pack.get(),
                planned_index.get(),
            ),
            Self::CatalogCompareAndSwap => formatter.write_str(
                "durable catalog changed after successor files were synchronized but before \
                 publication began",
            ),
            Self::CatalogCommitIndeterminate(error) => write!(
                formatter,
                "durable catalog publication may have selected either generation: {error}"
            ),
            Self::PublishedCatalogDirectory(error) => write!(
                formatter,
                "published generation failed the post-catalog directory durability barrier: \
                 {error}"
            ),
            Self::PublishedRecovery(error) => write!(
                formatter,
                "published generation failed restart-grade physical recovery: {error}"
            ),
            Self::PublishedAdmission(error) => write!(
                formatter,
                "published generation failed mandatory semantic admission: {error}"
            ),
            Self::RetainedRootRegistry(error) => Display::fmt(error, formatter),
            Self::RetainedRootManifest(error) => write!(
                formatter,
                "the appointed retention entries cannot seed the retained-root snapshot: {error}"
            ),
            Self::RetainedRootNotCarried { root } => write!(
                formatter,
                "a generation checkpoint of the admitted source would not carry retained root \
                 {root:?}"
            ),
        }
    }
}

impl std::error::Error for GenerationCheckpointPublicationErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::LogPack(error) => Some(error.as_ref()),
            Self::Pack(error) => Some(error.as_ref()),
            Self::Index(error) => Some(error.as_ref()),
            Self::Durable(error) => Some(error.as_ref()),
            Self::Limit(error) => Some(error),
            Self::AdmissionPreflight(error) => Some(error),
            Self::CheckpointWrite(error) => Some(error.as_ref()),
            Self::CheckpointIndex(error) => Some(error.as_ref()),
            Self::CatalogCommitIndeterminate(error) => Some(error),
            Self::PublishedCatalogDirectory(error) => Some(error),
            Self::PublishedRecovery(error) => Some(error.as_ref()),
            Self::PublishedAdmission(error) => Some(error.as_ref()),
            Self::SuccessorIdentityMismatch { .. } | Self::CatalogCompareAndSwap => None,
            Self::RetainedRootRegistry(error) => Some(error),
            Self::RetainedRootManifest(error) => Some(error.as_ref()),
            Self::RetainedRootNotCarried { .. } => None,
        }
    }
}

impl From<RetentionRegistryError> for GenerationCheckpointPublicationErrorV1 {
    fn from(error: RetentionRegistryError) -> Self { Self::RetainedRootRegistry(error) }
}

impl From<DurableIoError> for GenerationCheckpointPublicationErrorV1 {
    fn from(error: DurableIoError) -> Self { Self::Io(error) }
}

impl From<LogPackFormatError> for GenerationCheckpointPublicationErrorV1 {
    fn from(error: LogPackFormatError) -> Self { Self::LogPack(Box::new(error)) }
}

impl From<PackFormatError> for GenerationCheckpointPublicationErrorV1 {
    fn from(error: PackFormatError) -> Self { Self::Pack(Box::new(error)) }
}

impl From<IndexFormatError> for GenerationCheckpointPublicationErrorV1 {
    fn from(error: IndexFormatError) -> Self { Self::Index(Box::new(error)) }
}

impl From<super::DurableFormatError> for GenerationCheckpointPublicationErrorV1 {
    fn from(error: super::DurableFormatError) -> Self { Self::Durable(Box::new(error)) }
}

impl From<DurableLimitError> for GenerationCheckpointPublicationErrorV1 {
    fn from(error: DurableLimitError) -> Self { Self::Limit(error) }
}

#[derive(Debug)]
pub(crate) enum CheckpointIndexWriteErrorV1 {
    Index(Box<IndexFormatError>),
    Io(DurableIoError),
    Cursor { expected: u64, actual: u64 },
}

impl Display for CheckpointIndexWriteErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Index(error) => Display::fmt(error, formatter),
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Cursor { expected, actual } => write!(
                formatter,
                "canonical checkpoint index record expected offset {expected}, sink is at {actual}"
            ),
        }
    }
}

impl std::error::Error for CheckpointIndexWriteErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Index(error) => Some(error.as_ref()),
            Self::Io(error) => Some(error),
            Self::Cursor { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CheckpointPrepareDurableV1 {
    address: LogPackAddress,
    identity: GenerationCheckpointPrepareIdentityV1,
}

#[derive(Clone, Copy, Debug)]
struct FreshIndexDurableV1 {
    prepare: CheckpointPrepareDurableV1,
    exact_tail: u64,
    publication: crate::rspace::persistence::CommitPublicationV1,
}

#[derive(Clone, Copy, Debug)]
struct CheckpointCommitDurableV1 {
    index: FreshIndexDurableV1,
    address: LogPackAddress,
    digest: crate::rspace::persistence::PhysicalDigest,
}

#[derive(Clone, Debug)]
struct GenerationFilesDurableV1 {
    commit: CheckpointCommitDurableV1,
    successor: CatalogSlotV1,
    successor_image: [u8; CATALOG_IMAGE_LEN],
    retained_roots: RetainedRootHandoffV1,
}

#[derive(Clone)]
struct CheckpointCarriedObjectsV1 {
    closure: StructuralClosureLedgerV1,
    retention_manifest: Option<ObjectDigest>,
}

impl std::fmt::Debug for CheckpointCarriedObjectsV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CheckpointCarriedObjectsV1")
            .field("retention_manifest", &self.retention_manifest)
            .finish_non_exhaustive()
    }
}

impl CheckpointCarriedObjectsV1 {
    fn of<P: SemanticProfile>(source: &AdmittedCheckpointSourceV1<P>) -> Self {
        let retention_manifest = match source.retention_metadata_v2() {
            None => Some(
                source
                    .authority()
                    .slot()
                    .pack()
                    .retention_manifest()
                    .digest(),
            ),
            Some(_) => None,
        };
        Self {
            closure: source.retained_closure().clone(),
            retention_manifest,
        }
    }

    fn carries(&self, digest: ObjectDigest) -> bool {
        match (self.retention_manifest, self.closure.count(ClosureDomainV1::Retained, digest)) {
            (_, 1..) => true,
            (Some(manifest), 0) => manifest == digest,
            (None, 0) => self.closure.count(ClosureDomainV1::Metadata, digest) != 0,
        }
    }

    fn require(
        &self,
        roots: &[RetainedRootV1],
    ) -> Result<(), GenerationCheckpointPublicationErrorV1> {
        match roots
            .iter()
            .find(|root| !self.carries(root.root().digest()))
        {
            Some(root) => {
                Err(GenerationCheckpointPublicationErrorV1::RetainedRootNotCarried { root: *root })
            }
            None => Ok(()),
        }
    }
}

#[derive(Clone, Debug)]
struct RetainedRootHandoffV1 {
    registry: RetainedRootRegistryV1,
    carried: CheckpointCarriedObjectsV1,
    snapshot: RetainedRootSnapshotV1,
}

impl RetainedRootHandoffV1 {
    fn begin<P: SemanticProfile>(
        lease: &DurableWriterLeaseV1,
        source: &AdmittedCheckpointSourceV1<P>,
    ) -> Result<Self, GenerationCheckpointPublicationErrorV1> {
        let registry = lease.generation_leases().retained_roots()?;
        let manifest = appointed_retention_manifest(source).map_err(|error| {
            GenerationCheckpointPublicationErrorV1::RetainedRootManifest(Box::new(error))
        })?;
        let snapshot = registry.snapshot_with_manifest(&manifest)?;
        let carried = CheckpointCarriedObjectsV1::of(source);
        carried.require(snapshot.roots())?;
        Ok(Self {
            registry,
            carried,
            snapshot,
        })
    }

    fn seal(&self) -> Result<RetainedRootSealV1, GenerationCheckpointPublicationErrorV1> {
        let seal = self.registry.seal()?;
        match self.registry.is_snapshot_current(&self.snapshot)? {
            true => {}
            false => self.carried.require(self.registry.snapshot()?.roots())?,
        }
        Ok(seal)
    }
}

fn appointed_retention_manifest<P: SemanticProfile>(
    source: &AdmittedCheckpointSourceV1<P>,
) -> Result<RetentionManifestV1, DurableAdmissionErrorV1> {
    match source.retention_metadata_v2() {
        None => {
            let view = RetentionManifestView::parse(source.retention_manifest_bytes())?;
            let mut historical = Vec::with_capacity(view.historical().len());
            for retained in view.historical() {
                historical.push(retained?);
            }
            Ok(RetentionManifestV1::new(view.current(), historical)?)
        }
        Some(admitted) => {
            let anchor = admitted.anchor();
            let (historical, _) =
                admit_retention_history_v2(source.verified_state().object_store(), anchor)?
                    .into_parts();
            Ok(RetentionManifestV1::new(anchor.current(), historical)?)
        }
    }
}

impl<P: SemanticProfile> CheckpointPublicationAuthorityV1<P> {
    /// Rewrite the exact admitted retained closure into one fresh immutable
    /// generation and return authority reconstructed from its durable bytes.
    pub(crate) fn publish_generation_checkpoint_v1(
        self,
        recovery_limits: DurableLimits,
        wal_limits: LogPackDecodeLimits,
        admission_limits: DurableAdmissionLimitsV1,
    ) -> Result<SemanticallyAdmittedDurableWriterV1<P>, GenerationCheckpointPublicationErrorV1>
    {
        let (mut lease, authority, durable) = {
            let (mut lease, source) = self.into_parts();
            let authority = source.authority().clone();
            let durable = prepare_and_write_successor_generation::<P>(
                &mut lease,
                &source,
                &authority,
                recovery_limits,
                wal_limits,
                admission_limits,
            )?;
            // The admitted old-generation store and all plans borrowing it
            // leave scope here. Only the lease, exact old catalog token, and
            // fully synchronized successor receipts cross the CAS boundary.
            (lease, authority, durable)
        };

        // Both final successor names and their complete metadata must be
        // durable before the catalog is allowed to make them reachable.
        lease.sync_directory()?;

        // Persist the authenticated cleanup intent before catalog CAS.  A
        // pre-CAS crash leaves the old catalog authoritative; a post-CAS
        // restart can validate this exact candidate before reclaiming files.
        let reclamation =
            super::ReclamationCandidateV1::from_catalogs(authority.slot(), durable.successor)
                .expect("fresh generation must strictly supersede its captured authority");
        lease.write_reclamation_candidate(reclamation)?;

        #[cfg(test)]
        run_generation_checkpoint_publication_hook(
            GenerationCheckpointPublicationHookPointV1::AfterArtifactDirectorySync,
        );
        #[cfg(test)]
        trip_generation_checkpoint_publication_failpoint(
            GenerationCheckpointPublicationHookPointV1::AfterArtifactDirectorySync,
            lease.directory().path(),
        )?;

        #[cfg(test)]
        run_generation_checkpoint_publication_hook(
            GenerationCheckpointPublicationHookPointV1::BeforeCatalogCompareAndSwap,
        );
        #[cfg(test)]
        trip_generation_checkpoint_publication_failpoint(
            GenerationCheckpointPublicationHookPointV1::BeforeCatalogCompareAndSwap,
            lease.directory().path(),
        )?;

        let retained_root_seal = durable.retained_roots.seal()?;
        publish_catalog_compare_and_swap(&mut lease, &authority, &durable)?;
        drop((retained_root_seal, durable.retained_roots));

        #[cfg(test)]
        run_generation_checkpoint_publication_hook(
            GenerationCheckpointPublicationHookPointV1::AfterCatalogFileSyncBeforeDirectorySync,
        );
        #[cfg(test)]
        trip_generation_checkpoint_publication_failpoint(
            GenerationCheckpointPublicationHookPointV1::AfterCatalogFileSyncBeforeDirectorySync,
            lease.directory().path(),
        )?;

        lease
            .sync_catalog_directory()
            .map_err(GenerationCheckpointPublicationErrorV1::PublishedCatalogDirectory)?;

        // The catalog is now durable and authoritative. Reclaim only the
        // exact superseded generation named by the synchronized candidate;
        // active readers defer cleanup while retaining the sidecar for retry.
        if let Some(candidate) = lease.read_reclamation_candidate().map_err(|error| {
            GenerationCheckpointPublicationErrorV1::PublishedRecovery(Box::new(
                DurableStartupErrorV1::Io(error),
            ))
        })? {
            lease
                .reclaim_reclamation_candidate(candidate, durable.successor)
                .map_err(|error| {
                    GenerationCheckpointPublicationErrorV1::PublishedRecovery(Box::new(
                        DurableStartupErrorV1::Io(error),
                    ))
                })?;
        }

        #[cfg(test)]
        run_generation_checkpoint_publication_hook(
            GenerationCheckpointPublicationHookPointV1::AfterCatalogDirectorySync,
        );
        #[cfg(test)]
        trip_generation_checkpoint_publication_failpoint(
            GenerationCheckpointPublicationHookPointV1::AfterCatalogDirectorySync,
            lease.directory().path(),
        )?;

        #[cfg(test)]
        run_generation_checkpoint_publication_hook(
            GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
        );
        #[cfg(test)]
        trip_generation_checkpoint_publication_failpoint(
            GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
            lease.directory().path(),
        )?;

        // The only authority returned to the caller is reconstructed by the
        // same path used after an operating-system process restart.
        let physical = recover_physical_with_lease_v1(lease, recovery_limits, wal_limits).map_err(
            |error| GenerationCheckpointPublicationErrorV1::PublishedRecovery(Box::new(error)),
        )?;
        #[cfg(test)]
        run_generation_checkpoint_publication_hook(
            GenerationCheckpointPublicationHookPointV1::AfterPhysicalRecovery,
        );
        #[cfg(test)]
        trip_generation_checkpoint_publication_failpoint(
            GenerationCheckpointPublicationHookPointV1::AfterPhysicalRecovery,
            Path::new("generation-checkpoint-publication"),
        )?;
        #[cfg(test)]
        run_generation_checkpoint_publication_hook(
            GenerationCheckpointPublicationHookPointV1::BeforeSemanticAdmission,
        );
        #[cfg(test)]
        trip_generation_checkpoint_publication_failpoint(
            GenerationCheckpointPublicationHookPointV1::BeforeSemanticAdmission,
            Path::new("generation-checkpoint-publication"),
        )?;
        let admitted = admit_physical_writer::<P>(physical, admission_limits).map_err(|error| {
            GenerationCheckpointPublicationErrorV1::PublishedAdmission(Box::new(error))
        })?;
        #[cfg(test)]
        run_generation_checkpoint_publication_hook(
            GenerationCheckpointPublicationHookPointV1::AfterSemanticAdmission,
        );
        #[cfg(test)]
        trip_generation_checkpoint_publication_failpoint(
            GenerationCheckpointPublicationHookPointV1::AfterSemanticAdmission,
            Path::new("generation-checkpoint-publication"),
        )?;
        Ok(admitted)
    }

    /// Compact the admitted retained closure into a fresh generation. This
    /// named façade intentionally delegates to the same authenticated
    /// checkpoint publication protocol; compaction never changes semantic
    /// object bytes or Merkle identities.
    #[expect(
        dead_code,
        reason = "unit tests compact through the writer's compact_generation_v1 instead"
    )]
    pub(crate) fn compact_generation_v1(
        self,
        recovery_limits: DurableLimits,
        wal_limits: LogPackDecodeLimits,
        admission_limits: DurableAdmissionLimitsV1,
    ) -> Result<SemanticallyAdmittedDurableWriterV1<P>, GenerationCheckpointPublicationErrorV1>
    {
        self.publish_generation_checkpoint_v1(recovery_limits, wal_limits, admission_limits)
    }
}

fn prepare_and_write_successor_generation<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    source: &AdmittedCheckpointSourceV1<P>,
    authority: &CatalogAuthorityTokenV1,
    recovery_limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    admission_limits: DurableAdmissionLimitsV1,
) -> Result<GenerationFilesDurableV1, GenerationCheckpointPublicationErrorV1> {
    let current = authority.slot();
    let current_pack = PackId::new(current.pack().active_pack())?;
    let previous_terminal = PackTerminalIdentityV1::new(
        current_pack,
        current.commit_sequence(),
        current.commit_digest(),
    );
    let pack_header = PackHeaderV1::successor(current.store_id(), previous_terminal)?;
    let index_generation =
        IndexGenerationV1::new(current.pack().index_generation())?.successor()?;
    let retained_roots = RetainedRootHandoffV1::begin(lease, source)?;
    let prepared = PreparedGenerationCheckpointV1::<P, _>::new(
        source,
        current.store_id(),
        pack_header.first_sequence(),
        wal_limits,
    )?;
    let prepare_address = LogPackAddress::from_pack(
        pack_header.pack_id(),
        PACK_HEADER_LEN_V1,
        prepared.record_len(),
    )?;
    let index_header = super::ObjectIndexHeaderV1::new(current.store_id(), index_generation);
    let index_plan =
        PreparedGenerationCheckpointIndexV1::new(index_header, prepare_address, &prepared)?;
    let commit_address = LogPackAddress::from_pack(
        pack_header.pack_id(),
        prepare_address.end(),
        GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
    )?;
    preflight_recovery_limits(
        recovery_limits,
        commit_address.end(),
        index_plan.record_count(),
        index_plan.exact_tail(),
    )?;
    source
        .preflight_checkpoint_admission(admission_limits)
        .map_err(GenerationCheckpointPublicationErrorV1::AdmissionPreflight)?;

    #[cfg(test)]
    run_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterPreflightBeforeCleanup,
    );
    #[cfg(test)]
    trip_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterPreflightBeforeCleanup,
        lease.directory().path(),
    )?;

    let artifacts = lease.prepare_unpublished_successor_artifacts(authority)?;
    #[cfg(test)]
    run_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterCleanup,
    );
    #[cfg(test)]
    trip_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterCleanup,
        lease.directory().path(),
    )?;
    if artifacts.pack() != pack_header.pack_id() || artifacts.index() != index_generation {
        return Err(GenerationCheckpointPublicationErrorV1::SuccessorIdentityMismatch {
            planned_pack: pack_header.pack_id(),
            prepared_pack: artifacts.pack(),
            planned_index: index_generation,
            prepared_index: artifacts.index(),
        });
    }

    lease.resolve_reclamation_before_checkpoint(current)?;

    let prepare_durable = write_and_sync_prepare(lease, pack_header, prepare_address, &prepared)?;
    let index_durable = write_and_sync_index(lease, &index_plan, prepare_durable)?;
    let commit_durable = append_and_sync_commit(lease, current, commit_address, index_durable)?;
    prepare_successor_catalog(authority, current, &prepared, commit_durable, retained_roots)
}

pub(crate) fn require_incremental_retention_manifest<P: SemanticProfile>(
    prepared: &PreparedLogPackTransactionV1<'_, '_, P>,
) -> Result<crate::rspace::persistence::ObjectRefV1, IncrementalPublicationErrorV1> {
    prepared
        .retention_manifest()
        .ok_or(IncrementalPublicationErrorV1::MissingRetentionManifest)
}

pub(crate) fn publish_incremental_from_lease_v1<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    source: &AdmittedCheckpointSourceV1<P>,
    prepared: &PreparedDurableLogPackTransactionV1<'_, '_, P>,
    recovery_limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    admission_limits: DurableAdmissionLimitsV1,
) -> Result<AdmittedCheckpointSourceV1<P>, IncrementalPublicationErrorV1> {
    publish_incremental_with_terminal_v1(
        lease,
        source,
        prepared,
        recovery_limits,
        wal_limits,
        admission_limits,
        |_| {},
    )
}

pub(crate) fn publish_incremental_with_terminal_v1<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    source: &AdmittedCheckpointSourceV1<P>,
    prepared: &PreparedDurableLogPackTransactionV1<'_, '_, P>,
    recovery_limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    admission_limits: DurableAdmissionLimitsV1,
    retain_terminal: impl FnOnce(CatalogSlotV1),
) -> Result<AdmittedCheckpointSourceV1<P>, IncrementalPublicationErrorV1> {
    source
        .preflight_writer(lease)
        .map_err(IncrementalPublicationErrorV1::Preparation)?;
    let authority = source.authority().clone();
    prepared
        .preflight(&authority, &lease.generation_leases(), admission_limits)
        .map_err(IncrementalPublicationErrorV1::Preparation)?;
    let inventory = prepared.inventory();
    let prepared = prepared.prepared();
    let current = authority.slot();
    let retention_manifest = require_incremental_base(lease, &authority, prepared)?;
    truncate_incremental_pack(lease, current)?;
    let appended = append_incremental_prepare_and_index(lease, current, prepared)?;
    let (_, successor_image) = commit_incremental_successor(
        lease,
        &authority,
        prepared,
        inventory,
        retention_manifest,
        appended,
        retain_terminal,
    )?;
    swap_incremental_catalog(lease, &authority, successor_image)?;
    let physical = recover_physical_from_lease_v1(lease, recovery_limits, wal_limits)
        .map_err(|error| IncrementalPublicationErrorV1::PublishedRecovery(Box::new(error)))?;
    #[cfg(test)]
    incremental_publication_test_point(
        GenerationCheckpointPublicationHookPointV1::AfterPhysicalRecovery,
        lease.directory().path(),
    )?;
    #[cfg(test)]
    incremental_publication_test_point(
        GenerationCheckpointPublicationHookPointV1::BeforeSemanticAdmission,
        lease.directory().path(),
    )?;
    let successor = admit_physical_source::<P>(lease, physical, admission_limits)
        .map_err(|error| IncrementalPublicationErrorV1::PublishedAdmission(Box::new(error)))?;
    let admitted = ClosureInventoryV1 {
        distinct_objects: successor.report().retention_metadata_objects,
        canonical_bytes: successor.report().retention_metadata_canonical_bytes,
    };
    match inventory.metadata {
        Some(trial) if trial != admitted => {
            return Err(IncrementalPublicationErrorV1::PublishedMetadataInventory {
                trial,
                admitted,
            });
        }
        Some(_) | None => {}
    }
    #[cfg(test)]
    incremental_publication_test_point(
        GenerationCheckpointPublicationHookPointV1::AfterSemanticAdmission,
        lease.directory().path(),
    )?;
    Ok(successor)
}

pub(crate) fn publish_trusted_incremental_v1<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    source: &DurableAuthorityCoreV1<P>,
    prepared: &PreparedDurableLogPackTransactionV1<'_, '_, P>,
    policy: TrustedSuccessorPolicyV1,
    retain_terminal: impl FnOnce(CatalogSlotV1),
) -> Result<DurableAuthorityCoreV1<P>, IncrementalPublicationErrorV1> {
    source
        .preflight_writer(lease)
        .map_err(IncrementalPublicationErrorV1::Preparation)?;
    let authority = source.authority().clone();
    let trusted = prepared
        .preflight_trusted(&authority, &lease.generation_leases(), policy)
        .map_err(IncrementalPublicationErrorV1::Preparation)?;
    let inventory = prepared.inventory();
    let prepared = prepared.prepared();
    let current = authority.slot();
    let retention_manifest = require_incremental_base(lease, &authority, prepared)?;
    let plan = IncrementalAppendPlanV1::new(current, prepared)?;
    let index = source
        .trusted_successor_index(plan.index_append.records(), plan.index_append.new_tail())
        .map_err(|error| {
            IncrementalPublicationErrorV1::TrustedSuccessorAssembly(Box::new(
                DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Index(error)),
            ))
        })?;
    truncate_incremental_pack(lease, current)?;
    let appended = append_planned_incremental_prepare_and_index(lease, plan)?;
    let (successor, successor_image) = commit_incremental_successor(
        lease,
        &authority,
        prepared,
        inventory,
        retention_manifest,
        appended,
        retain_terminal,
    )?;
    let core = source
        .assemble_trusted_successor(lease, trusted, successor, successor_image, index)
        .map_err(|error| {
            IncrementalPublicationErrorV1::TrustedSuccessorAssembly(Box::new(error))
        })?;
    swap_incremental_catalog(lease, &authority, successor_image)?;
    let published = |error| IncrementalPublicationErrorV1::PublishedRecovery(Box::new(error));
    let candidate = lease
        .read_reclamation_candidate()
        .map_err(|error| published(DurableStartupErrorV1::Io(error)))?;
    resolve_reclamation_candidate_v1(lease, candidate, successor).map_err(published)?;
    let observed = lease
        .directory()
        .open_catalog()
        .and_then(|catalog| catalog.read_catalog_image())
        .map_err(|error| published(DurableStartupErrorV1::Io(error)))?;
    if !core.authority().matches_exactly(&observed) {
        return Err(published(DurableStartupErrorV1::Io(
            DurableIoError::CatalogAuthorityChanged {
                path: lease.directory().path().to_path_buf(),
            },
        )));
    }
    Ok(core)
}

fn require_incremental_base<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    authority: &CatalogAuthorityTokenV1,
    prepared: &PreparedLogPackTransactionV1<'_, '_, P>,
) -> Result<crate::rspace::persistence::ObjectRefV1, IncrementalPublicationErrorV1> {
    let receipt = prepared.receipt();
    let current = authority.slot();
    if receipt.base() != current.state() {
        return Err(IncrementalPublicationErrorV1::BaseStateMismatch {
            expected: current.state(),
            actual: receipt.base(),
        });
    }
    if receipt.profile_id() != P::ID {
        return Err(IncrementalPublicationErrorV1::ProfileMismatch);
    }
    let retention_manifest = require_incremental_retention_manifest(prepared)?;
    let observed = lease.open_catalog()?.read_catalog_image()?;
    if !authority.matches_exactly(&observed) {
        return Err(IncrementalPublicationErrorV1::CatalogCompareAndSwap);
    }
    Ok(retention_manifest)
}

fn truncate_incremental_pack(
    lease: &mut DurableWriterLeaseV1,
    current: CatalogSlotV1,
) -> Result<(), IncrementalPublicationErrorV1> {
    let mut pack = lease.open_pack(PackId::new(current.pack().active_pack())?)?;
    let appointed = current.pack().durable_pack_tail();
    let available = pack.require_length(FileLengthExpectation::AtLeast(appointed))?;
    if available > appointed {
        pack.set_len(appointed)?;
        pack.sync_all()?;
    }
    Ok(())
}

fn commit_incremental_successor<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    authority: &CatalogAuthorityTokenV1,
    prepared: &PreparedLogPackTransactionV1<'_, '_, P>,
    inventory: DurableSuccessorInventoryV1,
    retention_manifest: crate::rspace::persistence::ObjectRefV1,
    (prepare_address, prepare_identity, index_tail): (LogPackAddress, PrepareRecordIdentityV1, u64),
    retain_terminal: impl FnOnce(CatalogSlotV1),
) -> Result<(CatalogSlotV1, [u8; CATALOG_IMAGE_LEN]), IncrementalPublicationErrorV1> {
    let receipt = prepared.receipt();
    let current = authority.slot();
    let commit_address = LogPackAddress::from_pack(
        prepare_address.pack_id(),
        prepare_address.end(),
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )?;
    let retained_bytes = inventory.retained_union.canonical_bytes;
    let current_state_bytes = inventory.current.canonical_bytes;
    let publication = crate::rspace::persistence::CommitPublicationV1::new(
        current.pack().first_live_pack(),
        current.pack().index_generation(),
        index_tail,
        retention_manifest,
        current_state_bytes,
        retained_bytes,
    )?;
    let commit = TransactionCommitRecordV1::new(
        receipt,
        current.store_id(),
        current.commit_digest(),
        prepare_address,
        prepare_identity,
        publication,
    )?;
    let pack = CatalogPackStateV1::new(
        current.pack().first_live_pack(),
        current.pack().active_pack(),
        commit_address.end(),
        current.pack().index_generation(),
        index_tail,
        publication.retention_manifest(),
        publication.current_state_canonical_bytes(),
        publication.retained_canonical_bytes(),
    )?;
    let successor = current.successor(
        current.state(),
        receipt.target(),
        commit.digest(),
        prepare_identity.sequence(),
        commit_address,
        pack,
    )?;
    let successor_image = authority.successor_image(successor)?;
    retain_terminal(successor);
    {
        let mut pack = lease.open_pack(prepare_address.pack_id())?;
        pack.require_length(FileLengthExpectation::Exact(prepare_address.end()))?;
        pack.write_all_at(prepare_address.end(), commit.canonical_bytes())?;
        pack.require_length(FileLengthExpectation::Exact(commit_address.end()))?;
        #[cfg(test)]
        incremental_publication_test_point(
            GenerationCheckpointPublicationHookPointV1::AfterCommitWrite,
            pack.path(),
        )?;
        pack.sync_all()?;
        #[cfg(test)]
        incremental_publication_test_point(
            GenerationCheckpointPublicationHookPointV1::AfterCommitSync,
            pack.path(),
        )?;
    }
    lease.sync_directory()?;
    #[cfg(test)]
    incremental_publication_test_point(
        GenerationCheckpointPublicationHookPointV1::AfterArtifactDirectorySync,
        lease.directory().path(),
    )?;
    Ok((successor, successor_image))
}

fn swap_incremental_catalog(
    lease: &mut DurableWriterLeaseV1,
    authority: &CatalogAuthorityTokenV1,
    successor_image: [u8; CATALOG_IMAGE_LEN],
) -> Result<(), IncrementalPublicationErrorV1> {
    publish_catalog_slot_compare_and_swap(lease, authority, successor_image)?;
    #[cfg(test)]
    incremental_publication_test_point(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogFileSyncBeforeDirectorySync,
        lease.directory().path(),
    )?;
    lease
        .sync_catalog_directory()
        .map_err(IncrementalPublicationErrorV1::PublishedCatalogDirectory)?;
    #[cfg(test)]
    incremental_publication_test_point(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogDirectorySync,
        lease.directory().path(),
    )?;
    #[cfg(test)]
    incremental_publication_test_point(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
        lease.directory().path(),
    )?;
    Ok(())
}

impl<P: SemanticProfile> SemanticallyAdmittedDurableWriterV1<P> {
    pub(crate) fn publish_generation_checkpoint_v1(
        self,
        recovery_limits: DurableLimits,
        wal_limits: LogPackDecodeLimits,
        admission_limits: DurableAdmissionLimitsV1,
    ) -> Result<SemanticallyAdmittedDurableWriterV1<P>, GenerationCheckpointPublicationErrorV1>
    {
        self.into_checkpoint_publication()
            .publish_generation_checkpoint_v1(recovery_limits, wal_limits, admission_limits)
    }

    pub(crate) fn compact_generation_v1(
        self,
        recovery_limits: DurableLimits,
        wal_limits: LogPackDecodeLimits,
        admission_limits: DurableAdmissionLimitsV1,
    ) -> Result<Self, GenerationCheckpointPublicationErrorV1> {
        self.publish_generation_checkpoint_v1(recovery_limits, wal_limits, admission_limits)
    }

    pub(crate) fn register_known_root_v1(
        &self,
        root: ObjectRefV1,
    ) -> Result<KnownRootLeaseV1, KnownRootLeaseError> {
        let generation = GenerationLeaseKeyV1::from_catalog(self.catalog());
        match self.get_object_bounded(&root.digest(), root.encoded_len())? {
            Some(bytes) if bytes.len() as u64 == root.encoded_len() => self
                .generation_leases()
                .register_known_root(root, generation),
            Some(_) | None => Err(KnownRootLeaseError::Absent { root, generation }),
        }
    }
}

pub(super) fn preflight_recovery_limits(
    limits: DurableLimits,
    pack_tail: u64,
    object_records: u64,
    index_tail: u64,
) -> Result<(), DurableLimitError> {
    limits.require(DurableLimitKind::LivePacks, 1)?;
    limits.require(DurableLimitKind::PackBytes, pack_tail)?;
    limits.require(DurableLimitKind::TotalScanBytes, pack_tail)?;
    limits.require(DurableLimitKind::TotalPhysicalReadBytes, pack_tail)?;
    limits.require(DurableLimitKind::ObjectRecords, object_records)?;
    limits.require(DurableLimitKind::IndexRecords, object_records)?;
    limits.require(DurableLimitKind::IndexBytes, index_tail)?;
    Ok(())
}

struct IncrementalAppendPlanV1<'prepared, 'transaction, 'mutation, P: SemanticProfile> {
    prepared: &'prepared PreparedLogPackTransactionV1<'transaction, 'mutation, P>,
    pack_id: PackId,
    prepare_offset: u64,
    prepare_address: LogPackAddress,
    index_generation: IndexGenerationV1,
    index_append: PreparedIndexAppendV1<'prepared, 'transaction, 'mutation, P>,
}

impl<'prepared, 'transaction, 'mutation, P: SemanticProfile>
    IncrementalAppendPlanV1<'prepared, 'transaction, 'mutation, P>
{
    fn new(
        current: CatalogSlotV1,
        prepared: &'prepared PreparedLogPackTransactionV1<'transaction, 'mutation, P>,
    ) -> Result<Self, IncrementalAppendErrorV1> {
        let pack_id = PackId::new(current.pack().active_pack())?;
        let prepare_offset = current.pack().durable_pack_tail();
        let prepare_address =
            LogPackAddress::from_pack(pack_id, prepare_offset, prepared.record_len())?;
        let index_generation = IndexGenerationV1::new(current.pack().index_generation())?;
        let index_header = super::ObjectIndexHeaderV1::new(current.store_id(), index_generation);
        let index_append = PreparedIndexAppendV1::new(
            index_header,
            current.pack().durable_index_tail(),
            pack_id,
            prepare_offset,
            prepared,
        )?;
        Ok(Self {
            prepared,
            pack_id,
            prepare_offset,
            prepare_address,
            index_generation,
            index_append,
        })
    }
}

/// Append one preflighted transaction and its DATA index records to the
/// currently active generation. This function performs no catalog write: the
/// caller must sync the commit and then publish the successor slot only after
/// all bytes are durable.
pub(crate) fn append_incremental_prepare_and_index<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    current: CatalogSlotV1,
    prepared: &PreparedLogPackTransactionV1<'_, '_, P>,
) -> Result<(LogPackAddress, PrepareRecordIdentityV1, u64), IncrementalAppendErrorV1> {
    append_planned_incremental_prepare_and_index(
        lease,
        IncrementalAppendPlanV1::new(current, prepared)?,
    )
}

fn append_planned_incremental_prepare_and_index<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    plan: IncrementalAppendPlanV1<'_, '_, '_, P>,
) -> Result<(LogPackAddress, PrepareRecordIdentityV1, u64), IncrementalAppendErrorV1> {
    let IncrementalAppendPlanV1 {
        prepared,
        pack_id,
        prepare_offset,
        prepare_address,
        index_generation,
        index_append,
    } = plan;

    let prepare_identity = {
        let mut pack = lease.open_pack(pack_id)?;
        pack.require_length(FileLengthExpectation::Exact(prepare_offset))?;
        let mut sink =
            DurableCanonicalAppendSinkV1::new(&mut pack, prepare_offset, prepare_address.end())?;
        let identity = prepared.write_to(&mut sink)?;
        sink.finish()?;
        #[cfg(test)]
        incremental_publication_test_point(
            GenerationCheckpointPublicationHookPointV1::AfterPrepareWrite,
            pack.path(),
        )
        .map_err(|error| match error {
            IncrementalPublicationErrorV1::Io(error) => IncrementalAppendErrorV1::Io(error),
            other => panic!("unexpected append test fault: {other:?}"),
        })?;
        pack.sync_all()?;
        #[cfg(test)]
        incremental_publication_test_point(
            GenerationCheckpointPublicationHookPointV1::AfterPrepareSync,
            pack.path(),
        )
        .map_err(|error| match error {
            IncrementalPublicationErrorV1::Io(error) => IncrementalAppendErrorV1::Io(error),
            other => panic!("unexpected append test fault: {other:?}"),
        })?;
        identity
    };

    {
        let mut index = lease.open_index(index_generation)?;
        index.require_length(FileLengthExpectation::Exact(index_append.old_tail()))?;
        let mut sink = DurableCanonicalAppendSinkV1::new(
            &mut index,
            index_append.old_tail(),
            index_append.new_tail(),
        )?;
        for record in index_append.records() {
            sink.write_all(&record?.encode())?;
        }
        sink.finish()?;
        #[cfg(test)]
        incremental_publication_test_point(
            GenerationCheckpointPublicationHookPointV1::AfterIndexWrite,
            index.path(),
        )
        .map_err(|error| match error {
            IncrementalPublicationErrorV1::Io(error) => IncrementalAppendErrorV1::Io(error),
            other => panic!("unexpected append test fault: {other:?}"),
        })?;
        index.sync_all()?;
        #[cfg(test)]
        incremental_publication_test_point(
            GenerationCheckpointPublicationHookPointV1::AfterIndexSync,
            index.path(),
        )
        .map_err(|error| match error {
            IncrementalPublicationErrorV1::Io(error) => IncrementalAppendErrorV1::Io(error),
            other => panic!("unexpected append test fault: {other:?}"),
        })?;
    }

    Ok((prepare_address, prepare_identity, index_append.new_tail()))
}

fn write_and_sync_prepare<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    header: PackHeaderV1,
    address: LogPackAddress,
    prepared: &PreparedGenerationCheckpointV1<'_, P, AdmittedCheckpointSourceV1<P>>,
) -> Result<CheckpointPrepareDurableV1, GenerationCheckpointPublicationErrorV1> {
    let mut pack = lease.create_pack(header.pack_id())?;
    pack.write_all_at(0, &header.encode())?;
    let mut sink = DurableCanonicalAppendSinkV1::new(&mut pack, address.offset(), address.end())?;
    let identity = prepared.write_to(&mut sink).map_err(|error| {
        GenerationCheckpointPublicationErrorV1::CheckpointWrite(Box::new(error))
    })?;
    sink.finish()?;
    #[cfg(test)]
    run_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterPrepareWrite,
    );
    #[cfg(test)]
    trip_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterPrepareWrite,
        pack.path(),
    )?;
    pack.sync_all()?;
    #[cfg(test)]
    run_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterPrepareSync,
    );
    #[cfg(test)]
    trip_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterPrepareSync,
        pack.path(),
    )?;
    Ok(CheckpointPrepareDurableV1 { address, identity })
}

fn write_and_sync_index<P: SemanticProfile>(
    lease: &mut DurableWriterLeaseV1,
    plan: &PreparedGenerationCheckpointIndexV1<'_, '_, P, AdmittedCheckpointSourceV1<P>>,
    prepare: CheckpointPrepareDurableV1,
) -> Result<FreshIndexDurableV1, GenerationCheckpointPublicationErrorV1> {
    let mut index = lease.create_index(plan.header().generation())?;
    let mut sink = DurableCanonicalAppendSinkV1::new(&mut index, 0, plan.exact_tail())?;
    let mut stream = CanonicalIndexStreamV1::new(plan.header());
    sink.write_all(&stream.header_image())?;
    plan.try_visit_records(|record| {
        let image = stream
            .next_record(record)
            .map_err(|error| CheckpointIndexWriteErrorV1::Index(Box::new(error)))?;
        let actual = sink.cursor();
        if image.offset() != actual {
            return Err(CheckpointIndexWriteErrorV1::Cursor {
                expected: image.offset(),
                actual,
            });
        }
        sink.write_all(image.bytes())
            .map_err(CheckpointIndexWriteErrorV1::Io)
    })
    .map_err(|error| GenerationCheckpointPublicationErrorV1::CheckpointIndex(Box::new(error)))?;
    stream.verify_appointed_tail(plan.exact_tail())?;
    sink.finish()?;
    #[cfg(test)]
    run_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterIndexWrite,
    );
    #[cfg(test)]
    trip_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterIndexWrite,
        index.path(),
    )?;
    index.sync_all()?;
    #[cfg(test)]
    run_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterIndexSync,
    );
    #[cfg(test)]
    trip_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterIndexSync,
        index.path(),
    )?;
    Ok(FreshIndexDurableV1 {
        prepare,
        exact_tail: plan.exact_tail(),
        publication: plan.publication(),
    })
}

fn append_and_sync_commit(
    lease: &mut DurableWriterLeaseV1,
    current: CatalogSlotV1,
    address: LogPackAddress,
    index: FreshIndexDurableV1,
) -> Result<CheckpointCommitDurableV1, GenerationCheckpointPublicationErrorV1> {
    let commit = GenerationCheckpointCommitRecordV1::new(
        current.store_id(),
        current.commit_digest(),
        index.prepare.address,
        index.prepare.identity,
        index.publication,
    )?;
    let mut pack = lease.open_pack(address.pack_id())?;
    pack.require_length(FileLengthExpectation::Exact(address.offset()))?;
    pack.write_all_at(address.offset(), commit.canonical_bytes())?;
    pack.require_length(FileLengthExpectation::Exact(address.end()))?;
    #[cfg(test)]
    run_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterCommitWrite,
    );
    #[cfg(test)]
    trip_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterCommitWrite,
        pack.path(),
    )?;
    pack.sync_all()?;
    #[cfg(test)]
    run_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterCommitSync,
    );
    #[cfg(test)]
    trip_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterCommitSync,
        pack.path(),
    )?;
    Ok(CheckpointCommitDurableV1 {
        index,
        address,
        digest: commit.digest(),
    })
}

fn prepare_successor_catalog<P: SemanticProfile>(
    authority: &CatalogAuthorityTokenV1,
    current: CatalogSlotV1,
    prepared: &PreparedGenerationCheckpointV1<'_, P, AdmittedCheckpointSourceV1<P>>,
    commit: CheckpointCommitDurableV1,
    retained_roots: RetainedRootHandoffV1,
) -> Result<GenerationFilesDurableV1, GenerationCheckpointPublicationErrorV1> {
    let descriptor = prepared.descriptor();
    let pack = CatalogPackStateV1::new(
        commit.address.segment(),
        commit.address.segment(),
        commit.address.end(),
        commit.index.publication.index_generation(),
        commit.index.exact_tail,
        descriptor.retention_manifest(),
        descriptor.current_state_canonical_bytes().get(),
        descriptor
            .inventory()
            .retained_union_canonical_bytes()
            .get(),
    )?;
    let successor = current.successor(
        current.state(),
        descriptor.state(),
        commit.digest,
        prepared.sequence(),
        commit.address,
        pack,
    )?;
    let successor_image = authority.successor_image(successor)?;
    Ok(GenerationFilesDurableV1 {
        commit,
        successor,
        successor_image,
        retained_roots,
    })
}

fn publish_catalog_compare_and_swap(
    lease: &mut DurableWriterLeaseV1,
    authority: &CatalogAuthorityTokenV1,
    durable: &GenerationFilesDurableV1,
) -> Result<(), GenerationCheckpointPublicationErrorV1> {
    let mut catalog = lease.open_catalog()?;
    let observed = catalog.read_catalog_image()?;
    if !authority.matches_exactly(&observed) {
        return Err(GenerationCheckpointPublicationErrorV1::CatalogCompareAndSwap);
    }

    #[cfg(test)]
    run_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogCasVerifiedBeforeSlotWrite,
    );
    #[cfg(test)]
    trip_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogCasVerifiedBeforeSlotWrite,
        catalog.path(),
    )?;

    let inactive = match authority.selection().inactive_slot_index() {
        0 => CatalogSlotIndexV1::First,
        1 => CatalogSlotIndexV1::Second,
        _ => unreachable!("a two-slot catalog has exactly two slot indices"),
    };
    let offset = authority.selection().inactive_slot_index() * CATALOG_SLOT_LEN;
    let successor_bytes: &[u8; CATALOG_SLOT_LEN] = durable.successor_image
        [offset..offset + CATALOG_SLOT_LEN]
        .try_into()
        .expect("successor image contains one exact inactive catalog slot");
    debug_assert_eq!(*successor_bytes, durable.successor.encode());
    debug_assert_eq!(durable.commit.address.end(), durable.successor.durable_pack_tail());

    // The uncertainty boundary starts immediately before this positional
    // write. A short/torn write can leave either old or new authority valid;
    // neither outcome permits the stale writer to escape.
    catalog
        .write_catalog_slot(inactive, successor_bytes)
        .map_err(GenerationCheckpointPublicationErrorV1::CatalogCommitIndeterminate)?;
    #[cfg(test)]
    run_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSlotWrite,
    );
    #[cfg(test)]
    trip_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSlotWrite,
        catalog.path(),
    )?;
    catalog
        .sync_catalog_all()
        .map_err(GenerationCheckpointPublicationErrorV1::CatalogCommitIndeterminate)
}

fn publish_catalog_slot_compare_and_swap(
    lease: &mut DurableWriterLeaseV1,
    authority: &CatalogAuthorityTokenV1,
    successor_image: [u8; CATALOG_IMAGE_LEN],
) -> Result<(), IncrementalPublicationErrorV1> {
    #[cfg(test)]
    incremental_publication_test_point(
        GenerationCheckpointPublicationHookPointV1::BeforeCatalogCompareAndSwap,
        lease.directory().path(),
    )?;
    let mut catalog = lease.open_catalog()?;
    let observed = catalog.read_catalog_image()?;
    if !authority.matches_exactly(&observed) {
        return Err(IncrementalPublicationErrorV1::CatalogCompareAndSwap);
    }
    let inactive = match authority.selection().inactive_slot_index() {
        0 => CatalogSlotIndexV1::First,
        1 => CatalogSlotIndexV1::Second,
        _ => unreachable!("a two-slot catalog has exactly two slot indices"),
    };
    let offset = authority.selection().inactive_slot_index() * CATALOG_SLOT_LEN;
    let slot: &[u8; CATALOG_SLOT_LEN] = successor_image[offset..offset + CATALOG_SLOT_LEN]
        .try_into()
        .expect("successor image contains one exact inactive catalog slot");
    #[cfg(test)]
    incremental_publication_test_point(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogCasVerifiedBeforeSlotWrite,
        catalog.path(),
    )?;
    catalog
        .write_catalog_slot(inactive, slot)
        .map_err(IncrementalPublicationErrorV1::CatalogCommitIndeterminate)?;
    #[cfg(test)]
    incremental_publication_test_point(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSlotWrite,
        catalog.path(),
    )?;
    catalog
        .sync_catalog_all()
        .map_err(IncrementalPublicationErrorV1::CatalogCommitIndeterminate)
}
