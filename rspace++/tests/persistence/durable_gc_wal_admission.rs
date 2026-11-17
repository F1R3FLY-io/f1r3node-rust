use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;

use super::checkpoint_wal::fixture as checkpoint_source_fixture;
use super::durable_publication::{
    FORMAL_PUBLICATION_PHASE_ORDER, directory_snapshot, install_checkpoint_image,
    publish_data_successor,
};
use super::durable_recovery::{
    catalog_image, checkpoint_image_from_source, limit_config, limits, valid_two_pack_image,
};
use super::durable_rspace_authority::{
    assert_empty, assert_journals_unchanged, fixture as journal_owner_fixture,
    poll_checkpoint_with, populate, publication_limits, store_files,
};
use super::support::{PersistenceTrialDir, TestSemanticProfile, encode, trial_scratch};
use super::transaction_sealing::{base_state, seal_data};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::internal::Datum;
use crate::rspace::persistence::durable::{
    AdmittedCheckpointSourceV1, DurableAdmissionLimitsV1, DurableAuthorityCoreV1,
    DurableDirectoryV1, DurableIoError, DurableIoOperation, DurableRSpacePublicationLimitsV1,
    DurableRSpaceTransactionError, DurableStartupErrorV1, DurableStorageFaultV1,
    DurableWriterLeaseV1, GenerationCheckpointPublicationErrorV1,
    GenerationCheckpointPublicationHookPointV1, GenerationCheckpointPublicationOutcomeV1,
    IncrementalAppendErrorV1, IncrementalPublicationErrorV1, IndexBindingDispositionV1,
    JournalPublicationPhaseV1, RecoveryError, SemanticallyAdmittedDurableWriterV1,
    TrustedSuccessorPolicyV1, admit_physical_source, arm_durable_storage_fault,
    arm_generation_checkpoint_publication_hook, clear_durable_storage_fault, index_file_name,
    pack_file_name, publish_incremental_from_lease_v1, publish_trusted_incremental_v1,
    recover_physical_from_lease_v1,
};
use crate::rspace::persistence::retention::RetainedRootSnapshotV1;
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CanonicalObjectStore, CatalogSelectionV1, CatalogSlotV1,
    CommittedObjectComparison, CommittedObjectLookup, DagTransactionLimits, DurableLimitConfig,
    DurableLimitError, DurableLimitKind, DurableLimits, GenerationLeaseKeyV1,
    GenerationLeaseRegistryV1, INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexGenerationV1,
    LogPackDecodeLimits, LogPackObjectPlanV1, MutationBatchViewV1, ObjectRefV1, OpenDagTransaction,
    PackId, PreparedLogPackTransactionV1, RSpaceMutationBatchV1, StateVersionV1,
    TRANSACTION_COMMIT_RECORD_LEN_V1,
};

type Source = AdmittedCheckpointSourceV1<TestSemanticProfile>;
type Core = DurableAuthorityCoreV1<TestSemanticProfile>;
type Writer = SemanticallyAdmittedDurableWriterV1<TestSemanticProfile>;
type Point = GenerationCheckpointPublicationHookPointV1;
type Outcome = GenerationCheckpointPublicationOutcomeV1;
type Files = BTreeMap<String, Vec<u8>>;

const CATALOG_FILE: &str = "catalog.v1";
const RECLAMATION_INTENT_TEMPORARY_FILE: &str = "reclaim.v1.tmp";
const APPEND_MODES: [AppendMode; 2] = [AppendMode::Trusted, AppendMode::ColdChecked];
const STORAGE_FAULTS: [DurableStorageFaultV1; 2] =
    [DurableStorageFaultV1::StorageFull, DurableStorageFaultV1::ShortWrite];
const STORAGE_IDENTITIES: [&str; 2] = ["storage-a", "storage-b"];
const WINDOW_KINDS: [DurableLimitKind; 8] = [
    DurableLimitKind::LivePacks,
    DurableLimitKind::PackBytes,
    DurableLimitKind::Transactions,
    DurableLimitKind::ObjectRecords,
    DurableLimitKind::TotalScanBytes,
    DurableLimitKind::TotalPhysicalReadBytes,
    DurableLimitKind::IndexBytes,
    DurableLimitKind::IndexRecords,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppendMode {
    ColdChecked,
    Trusted,
}

enum AppendAuthority {
    ColdChecked(Box<Source>),
    Trusted(Arc<Core>),
}

impl AppendAuthority {
    fn of(mode: AppendMode, source: Source) -> Self {
        match mode {
            AppendMode::ColdChecked => Self::ColdChecked(Box::new(source)),
            AppendMode::Trusted => Self::Trusted(source.into_core()),
        }
    }

    fn core(&self) -> &Core {
        match self {
            Self::ColdChecked(source) => source.core(),
            Self::Trusted(core) => core,
        }
    }

    fn slot(&self) -> CatalogSlotV1 { self.core().authority().slot() }
}

struct AppendStore {
    authority: AppendAuthority,
    lease: DurableWriterLeaseV1,
    scratch: PersistenceTrialDir,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowDemand {
    live_packs: u64,
    pack_bytes: u64,
    transactions: u64,
    object_records: u64,
    total_scan_bytes: u64,
    total_physical_read_bytes: u64,
    index_bytes: u64,
    index_records: u64,
}

impl WindowDemand {
    fn of(self, kind: DurableLimitKind) -> u64 {
        match kind {
            DurableLimitKind::LivePacks => self.live_packs,
            DurableLimitKind::PackBytes => self.pack_bytes,
            DurableLimitKind::Transactions => self.transactions,
            DurableLimitKind::ObjectRecords => self.object_records,
            DurableLimitKind::TotalScanBytes => self.total_scan_bytes,
            DurableLimitKind::TotalPhysicalReadBytes => self.total_physical_read_bytes,
            DurableLimitKind::IndexBytes => self.index_bytes,
            DurableLimitKind::IndexRecords => self.index_records,
            DurableLimitKind::FullByteComparisonBytes |
            DurableLimitKind::AggregateFullByteComparisonBytes => {
                panic!("the recovery window does not bound {kind:?}")
            }
        }
    }

    fn limits(self, lowered: Option<DurableLimitKind>) -> DurableLimits {
        let bound = |kind: DurableLimitKind| match lowered {
            Some(lowered) if lowered == kind => self.of(kind) - 1,
            Some(_) | None => self.of(kind),
        };
        DurableLimits::new(DurableLimitConfig {
            max_live_packs: bound(DurableLimitKind::LivePacks),
            max_pack_bytes: bound(DurableLimitKind::PackBytes),
            max_transactions: bound(DurableLimitKind::Transactions),
            max_object_records: bound(DurableLimitKind::ObjectRecords),
            max_total_scan_bytes: bound(DurableLimitKind::TotalScanBytes),
            max_total_physical_read_bytes: bound(DurableLimitKind::TotalPhysicalReadBytes),
            max_index_bytes: bound(DurableLimitKind::IndexBytes),
            max_index_records: bound(DurableLimitKind::IndexRecords),
            ..limit_config()
        })
        .expect("every bound of the recovery window is positive")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultPlacement {
    BeforeTheFirstWrite,
    At(Point),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultedFile {
    Catalog,
    Pack,
    Index,
    Directory,
    ReclamationIntent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StorageRefusal {
    StorageFull(DurableIoOperation),
    ZeroWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TypedRefusal {
    outcome: Outcome,
    storage: StorageRefusal,
    file: FaultedFile,
}

struct EveryObjectMissing;

impl CommittedObjectLookup for EveryObjectMissing {
    type Error = Infallible;

    fn compare_committed_object(
        &self,
        _reference: ObjectRefV1,
        _sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error> {
        Ok(CommittedObjectComparison::Missing)
    }
}

fn write_owner_only(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .expect("create an owner-only durable trial file");
    file.write_all(bytes).expect("write the durable trial file");
    file.sync_all().expect("sync the durable trial file");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .expect("restrict the durable trial file to its owner");
}

fn admit(root: &Path, recovery_limits: DurableLimits) -> Writer {
    DurableDirectoryV1::open(root)
        .expect("open the durable trial directory")
        .recover_and_admit_v1::<TestSemanticProfile>(
            recovery_limits,
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("recover and admit the durable trial directory")
}

fn open_store(mode: AppendMode, prefix: &str) -> AppendStore {
    let scratch = trial_scratch(prefix);
    install_checkpoint_image(
        scratch.path(),
        &checkpoint_image_from_source(checkpoint_source_fixture()),
    );
    let (lease, source) = admit(scratch.path(), limits()).into_parts();
    AppendStore {
        authority: AppendAuthority::of(mode, source),
        lease,
        scratch,
    }
}

fn data_mutation(base: StateVersionV1, identities: &[&str]) -> Vec<u8> {
    let channel = "channel".to_owned();
    let mut batch = RSpaceMutationBatchV1::new(base).expect("the base state has a successor");
    for identity in identities {
        let datum = Datum::create(&channel, (*identity).to_owned(), false);
        batch
            .record_data(&RSpaceDataIndex::datum_path(&channel, &datum), 0, 1)
            .expect("record one new datum");
    }
    encode(&batch.plan().expect("plan the data mutation"))
}

fn reread_key_bytes(mutation: &[u8]) -> u64 {
    let view = MutationBatchViewV1::parse(mutation).expect("the appended mutation parses");
    let mut key_lengths = view
        .data()
        .map(|entry| entry.key.len() as u64)
        .collect::<Vec<_>>();
    key_lengths.pop();
    key_lengths.iter().sum()
}

fn append(
    lease: &mut DurableWriterLeaseV1,
    authority: &AppendAuthority,
    mutation: &[u8],
    recovery_limits: DurableLimits,
) -> Result<AppendAuthority, IncrementalPublicationErrorV1> {
    let view = MutationBatchViewV1::parse(mutation).expect("the appended mutation parses");
    let sealed = match authority {
        AppendAuthority::ColdChecked(source) => source.seal_transaction(
            lease,
            view,
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        ),
        AppendAuthority::Trusted(core) => core.bind_trusted_transaction(
            lease,
            OpenDagTransaction::new(core.verified_state(), view, DagTransactionLimits::UNBOUNDED)
                .and_then(OpenDagTransaction::seal)
                .expect("seal the appended mutation on the authority core"),
            TrustedSuccessorPolicyV1::UNBOUNDED,
        ),
    }
    .expect("bind the appended transaction to the current authority");
    let sequence = authority
        .slot()
        .commit_sequence()
        .successor()
        .expect("the commit sequence has a successor");
    let prepared = authority
        .core()
        .prepare_transaction(lease, &sealed, sequence, LogPackDecodeLimits::UNBOUNDED)
        .expect("prepare the appended transaction");
    match authority {
        AppendAuthority::ColdChecked(source) => publish_incremental_from_lease_v1(
            lease,
            source,
            &prepared,
            recovery_limits,
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map(|successor| AppendAuthority::ColdChecked(Box::new(successor))),
        AppendAuthority::Trusted(core) => publish_trusted_incremental_v1(
            lease,
            core,
            &prepared,
            TrustedSuccessorPolicyV1::UNBOUNDED,
            recovery_limits,
            |_| {},
        )
        .map(|successor| AppendAuthority::Trusted(Arc::new(successor))),
    }
}

fn limit_refusal<Recovered>(
    recovery: Result<Recovered, DurableStartupErrorV1>,
) -> Option<DurableLimitError> {
    match recovery {
        Ok(_) => None,
        Err(
            DurableStartupErrorV1::Limit(refusal) |
            DurableStartupErrorV1::Recovery(RecoveryError::Limit(refusal)),
        ) => Some(refusal),
        Err(error) => panic!("recovery failed for a reason other than a recovery limit: {error:?}"),
    }
}

fn measured_demand(lease: &mut DurableWriterLeaseV1) -> WindowDemand {
    let recovered = recover_physical_from_lease_v1(lease, limits(), LogPackDecodeLimits::UNBOUNDED)
        .expect("recover the published successor under ample limits");
    let work = recovered.startup_work();
    assert_eq!(
        (recovered.disposition(), work.wal_scan_passes()),
        (IndexBindingDispositionV1::VerifiedExisting, 1),
        "a published successor is recovered by one verifying scan"
    );
    let pack = recovered.catalog_authority().slot().pack();
    WindowDemand {
        live_packs: pack.active_pack() - pack.first_live_pack() + 1,
        pack_bytes: pack.durable_pack_tail(),
        transactions: recovered.transactions(),
        object_records: recovered.object_records(),
        total_scan_bytes: recovered.scanned_bytes(),
        total_physical_read_bytes: work.wal_bytes_read(),
        index_bytes: pack.durable_index_tail(),
        index_records: (pack.durable_index_tail() - INDEX_HEADER_LEN_V1) / INDEX_RECORD_LEN_V1,
    }
}

fn recover_source(lease: &mut DurableWriterLeaseV1) -> Source {
    let physical = recover_physical_from_lease_v1(lease, limits(), LogPackDecodeLimits::UNBOUNDED)
        .expect("recover the store once the storage fault is cleared");
    admit_physical_source::<TestSemanticProfile>(
        lease,
        physical,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("admit the recovered store")
}

fn selected_slot(root: &Path) -> CatalogSlotV1 {
    let image: [u8; CATALOG_IMAGE_LEN] = fs::read(root.join(CATALOG_FILE))
        .expect("read the catalog file")
        .try_into()
        .expect("the catalog file has its exact image length");
    CatalogSelectionV1::select(&image)
        .expect("the catalog image decodes")
        .expect("the catalog image selects a slot")
        .slot()
}

fn retention_picture(registry: &GenerationLeaseRegistryV1) -> (usize, RetainedRootSnapshotV1) {
    (
        registry.active_lease_count_for_test(),
        registry
            .retained_roots()
            .expect("reach the retained root registry")
            .snapshot()
            .expect("snapshot the retained roots"),
    )
}

fn pre_swap_placements(first: Point) -> Vec<FaultPlacement> {
    let mut placements = Vec::with_capacity(FORMAL_PUBLICATION_PHASE_ORDER.len());
    placements.push(FaultPlacement::BeforeTheFirstWrite);
    placements.extend(
        FORMAL_PUBLICATION_PHASE_ORDER
            .into_iter()
            .skip_while(|point| *point != first)
            .take_while(|point| *point != Point::AfterCatalogSlotWrite)
            .map(FaultPlacement::At),
    );
    placements
}

fn arm_storage_fault(fault: DurableStorageFaultV1, placement: FaultPlacement) {
    match placement {
        FaultPlacement::BeforeTheFirstWrite => arm_durable_storage_fault(fault),
        FaultPlacement::At(point) => {
            arm_generation_checkpoint_publication_hook(point, move || {
                arm_durable_storage_fault(fault)
            });
        }
    }
}

const fn persisting_fault(fault: DurableStorageFaultV1) -> DurableStorageFaultV1 {
    match fault {
        DurableStorageFaultV1::StorageFull => DurableStorageFaultV1::StorageFull,
        DurableStorageFaultV1::ShortWrite | DurableStorageFaultV1::NoProgress => {
            DurableStorageFaultV1::NoProgress
        }
    }
}

fn faulted_file(root: &Path, path: &Path) -> FaultedFile {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a durable path ends in a UTF-8 name");
    match name {
        CATALOG_FILE => FaultedFile::Catalog,
        RECLAMATION_INTENT_TEMPORARY_FILE => FaultedFile::ReclamationIntent,
        name if name.starts_with("pack-") => FaultedFile::Pack,
        name if name.starts_with("index-") => FaultedFile::Index,
        name if root.file_name().and_then(|root| root.to_str()) == Some(name) => {
            FaultedFile::Directory
        }
        other => panic!("a storage refusal names the unexpected file {other}"),
    }
}

fn storage_refusal(root: &Path, error: &DurableIoError) -> (StorageRefusal, FaultedFile) {
    match error {
        DurableIoError::Io {
            operation,
            path,
            source,
        } if source.kind() == std::io::ErrorKind::StorageFull => {
            (StorageRefusal::StorageFull(*operation), faulted_file(root, path))
        }
        DurableIoError::ZeroWrite {
            operation: DurableIoOperation::PositionalWrite,
            path,
            ..
        } => (StorageRefusal::ZeroWrite, faulted_file(root, path)),
        other => panic!("the refusal is not a typed storage error: {other:?}"),
    }
}

fn incremental_refusal(
    root: &Path,
    attempt: Result<AppendAuthority, IncrementalPublicationErrorV1>,
) -> TypedRefusal {
    let error = match attempt {
        Ok(_) => panic!("the append was acknowledged through the storage fault"),
        Err(error) => error,
    };
    let (storage, file) = match &error {
        IncrementalPublicationErrorV1::Append(IncrementalAppendErrorV1::Io(io)) |
        IncrementalPublicationErrorV1::Io(io) |
        IncrementalPublicationErrorV1::CatalogCommitIndeterminate(io) => storage_refusal(root, io),
        other => panic!("the storage fault was not refused with a typed storage error: {other:?}"),
    };
    TypedRefusal {
        outcome: error.publication_outcome(),
        storage,
        file,
    }
}

fn checkpoint_refusal(root: &Path, error: &GenerationCheckpointPublicationErrorV1) -> TypedRefusal {
    let (storage, file) = match error {
        GenerationCheckpointPublicationErrorV1::Io(io) |
        GenerationCheckpointPublicationErrorV1::CatalogCommitIndeterminate(io) => {
            storage_refusal(root, io)
        }
        other => panic!("the storage fault was not refused with a typed storage error: {other:?}"),
    };
    TypedRefusal {
        outcome: error.publication_outcome(),
        storage,
        file,
    }
}

fn expected_incremental_refusal(
    fault: DurableStorageFaultV1,
    placement: FaultPlacement,
) -> TypedRefusal {
    let (outcome, storage, file) = match (fault, placement) {
        (DurableStorageFaultV1::StorageFull, FaultPlacement::BeforeTheFirstWrite) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::PositionalWrite),
            FaultedFile::Pack,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterPrepareWrite)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::SyncFile),
            FaultedFile::Pack,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterPrepareSync)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::PositionalWrite),
            FaultedFile::Index,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterIndexWrite)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::SyncFile),
            FaultedFile::Index,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterIndexSync)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::PositionalWrite),
            FaultedFile::Pack,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterCommitWrite)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::SyncFile),
            FaultedFile::Pack,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterCommitSync)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::SyncDirectory),
            FaultedFile::Directory,
        ),
        (
            DurableStorageFaultV1::StorageFull,
            FaultPlacement::At(
                Point::AfterArtifactDirectorySync |
                Point::BeforeCatalogCompareAndSwap |
                Point::AfterCatalogCasVerifiedBeforeSlotWrite,
            ),
        ) => (
            Outcome::Indeterminate,
            StorageRefusal::StorageFull(DurableIoOperation::PositionalWrite),
            FaultedFile::Catalog,
        ),
        (DurableStorageFaultV1::ShortWrite, FaultPlacement::BeforeTheFirstWrite) => {
            (Outcome::DefinitelyNotPublished, StorageRefusal::ZeroWrite, FaultedFile::Pack)
        }
        (
            DurableStorageFaultV1::ShortWrite,
            FaultPlacement::At(Point::AfterPrepareWrite | Point::AfterPrepareSync),
        ) => (Outcome::DefinitelyNotPublished, StorageRefusal::ZeroWrite, FaultedFile::Index),
        (
            DurableStorageFaultV1::ShortWrite,
            FaultPlacement::At(Point::AfterIndexWrite | Point::AfterIndexSync),
        ) => (Outcome::DefinitelyNotPublished, StorageRefusal::ZeroWrite, FaultedFile::Pack),
        (
            DurableStorageFaultV1::ShortWrite,
            FaultPlacement::At(
                Point::AfterCommitWrite |
                Point::AfterCommitSync |
                Point::AfterArtifactDirectorySync |
                Point::BeforeCatalogCompareAndSwap |
                Point::AfterCatalogCasVerifiedBeforeSlotWrite,
            ),
        ) => (Outcome::Indeterminate, StorageRefusal::ZeroWrite, FaultedFile::Catalog),
        (fault, placement) => {
            panic!("no incremental publication places {fault:?} at {placement:?}")
        }
    };
    TypedRefusal {
        outcome,
        storage,
        file,
    }
}

fn expected_persisting_refusal(fault: DurableStorageFaultV1, pack_residue: bool) -> TypedRefusal {
    let storage = match (fault, pack_residue) {
        (DurableStorageFaultV1::StorageFull, true) => {
            StorageRefusal::StorageFull(DurableIoOperation::SyncFile)
        }
        (DurableStorageFaultV1::StorageFull, false) => {
            StorageRefusal::StorageFull(DurableIoOperation::PositionalWrite)
        }
        (DurableStorageFaultV1::ShortWrite | DurableStorageFaultV1::NoProgress, _) => {
            StorageRefusal::ZeroWrite
        }
    };
    TypedRefusal {
        outcome: Outcome::DefinitelyNotPublished,
        storage,
        file: FaultedFile::Pack,
    }
}

fn expected_checkpoint_refusal(
    fault: DurableStorageFaultV1,
    placement: FaultPlacement,
) -> TypedRefusal {
    let (outcome, storage, file) = match (fault, placement) {
        (
            DurableStorageFaultV1::StorageFull,
            FaultPlacement::BeforeTheFirstWrite |
            FaultPlacement::At(Point::AfterPreflightBeforeCleanup | Point::AfterCleanup),
        ) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::PositionalWrite),
            FaultedFile::Pack,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterPrepareWrite)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::SyncFile),
            FaultedFile::Pack,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterPrepareSync)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::PositionalWrite),
            FaultedFile::Index,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterIndexWrite)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::SyncFile),
            FaultedFile::Index,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterIndexSync)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::PositionalWrite),
            FaultedFile::Pack,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterCommitWrite)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::SyncFile),
            FaultedFile::Pack,
        ),
        (DurableStorageFaultV1::StorageFull, FaultPlacement::At(Point::AfterCommitSync)) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::StorageFull(DurableIoOperation::SyncDirectory),
            FaultedFile::Directory,
        ),
        (
            DurableStorageFaultV1::StorageFull,
            FaultPlacement::At(
                Point::AfterArtifactDirectorySync |
                Point::BeforeCatalogCompareAndSwap |
                Point::AfterCatalogCasVerifiedBeforeSlotWrite,
            ),
        ) => (
            Outcome::Indeterminate,
            StorageRefusal::StorageFull(DurableIoOperation::PositionalWrite),
            FaultedFile::Catalog,
        ),
        (
            DurableStorageFaultV1::ShortWrite,
            FaultPlacement::BeforeTheFirstWrite |
            FaultPlacement::At(Point::AfterPreflightBeforeCleanup | Point::AfterCleanup),
        ) => (Outcome::DefinitelyNotPublished, StorageRefusal::ZeroWrite, FaultedFile::Pack),
        (
            DurableStorageFaultV1::ShortWrite,
            FaultPlacement::At(Point::AfterPrepareWrite | Point::AfterPrepareSync),
        ) => (Outcome::DefinitelyNotPublished, StorageRefusal::ZeroWrite, FaultedFile::Index),
        (
            DurableStorageFaultV1::ShortWrite,
            FaultPlacement::At(Point::AfterIndexWrite | Point::AfterIndexSync),
        ) => (Outcome::DefinitelyNotPublished, StorageRefusal::ZeroWrite, FaultedFile::Pack),
        (
            DurableStorageFaultV1::ShortWrite,
            FaultPlacement::At(Point::AfterCommitWrite | Point::AfterCommitSync),
        ) => (
            Outcome::DefinitelyNotPublished,
            StorageRefusal::ZeroWrite,
            FaultedFile::ReclamationIntent,
        ),
        (
            DurableStorageFaultV1::ShortWrite,
            FaultPlacement::At(
                Point::AfterArtifactDirectorySync |
                Point::BeforeCatalogCompareAndSwap |
                Point::AfterCatalogCasVerifiedBeforeSlotWrite,
            ),
        ) => (Outcome::Indeterminate, StorageRefusal::ZeroWrite, FaultedFile::Catalog),
        (fault, placement) => {
            panic!("no generation checkpoint places {fault:?} at {placement:?}")
        }
    };
    TypedRefusal {
        outcome,
        storage,
        file,
    }
}

fn assert_old_authority_on_disk(root: &Path, old: CatalogSlotV1, old_files: &Files, context: &str) {
    assert_eq!(selected_slot(root), old, "{context}: the catalog selects the old slot");
    let files = directory_snapshot(root);
    assert!(files.keys().eq(old_files.keys()), "{context}: the directory keeps its file names");
    for name in [pack_file_name(PackId::GENESIS), index_file_name(IndexGenerationV1::FIRST)] {
        assert!(
            files[&name].starts_with(&old_files[&name]),
            "{context}: {name} keeps every byte the old slot appoints"
        );
    }
}

fn incremental_control(mode: AppendMode) -> Files {
    let mut store = open_store(mode, "gc-wal-storage-control-");
    let mutation = data_mutation(store.authority.slot().state(), &STORAGE_IDENTITIES);
    store.authority = append(&mut store.lease, &store.authority, &mutation, limits())
        .unwrap_or_else(|error| panic!("{mode:?}: the control append failed: {error:?}"));
    directory_snapshot(store.scratch.path())
}

fn incremental_storage_case(
    mode: AppendMode,
    fault: DurableStorageFaultV1,
    placement: FaultPlacement,
    control: &Files,
) {
    let context = format!("{mode:?}, {fault:?} at {placement:?}");
    let mut store = open_store(mode, "gc-wal-storage-");
    let root = store.scratch.path().to_path_buf();
    let old = store.authority.slot();
    let old_files = directory_snapshot(&root);
    let mutation = data_mutation(old.state(), &STORAGE_IDENTITIES);
    let registry = store.lease.generation_leases();
    let generation = GenerationLeaseKeyV1::from_catalog(old);
    let known = registry
        .register_known_root(old.state().manifest(), generation)
        .expect("register a Known root on the store under test");
    let retention = retention_picture(&registry);
    assert_eq!((retention.0, retention.1.roots().len()), (1, 1), "{context}");

    arm_storage_fault(fault, placement);
    let refusal =
        incremental_refusal(&root, append(&mut store.lease, &store.authority, &mutation, limits()));
    assert_eq!(refusal, expected_incremental_refusal(fault, placement), "{context}");
    assert_old_authority_on_disk(&root, old, &old_files, &context);
    let after_refusal = directory_snapshot(&root);
    let pack_name = pack_file_name(PackId::GENESIS);
    let pack_residue = after_refusal[&pack_name].len() > old_files[&pack_name].len();
    let image_changed = after_refusal[CATALOG_FILE] != old_files[CATALOG_FILE];
    assert_eq!(
        image_changed,
        refusal ==
            TypedRefusal {
                outcome: Outcome::Indeterminate,
                storage: StorageRefusal::ZeroWrite,
                file: FaultedFile::Catalog,
            },
        "{context}: only a short slot write changes a byte of the catalog"
    );

    match (image_changed, append(&mut store.lease, &store.authority, &mutation, limits())) {
        (true, Err(IncrementalPublicationErrorV1::CatalogCompareAndSwap)) => {}
        (true, Ok(_)) => panic!("{context}: an append over a changed catalog was acknowledged"),
        (true, Err(error)) => {
            panic!("{context}: an append over a changed catalog ended with {error:?}")
        }
        (false, attempt) => assert_eq!(
            incremental_refusal(&root, attempt),
            expected_persisting_refusal(fault, pack_residue),
            "{context}: the persisting fault refuses the second attempt"
        ),
    }
    assert_old_authority_on_disk(&root, old, &old_files, &context);
    assert_eq!(retention_picture(&registry), retention, "{context}");
    assert!(
        registry
            .has_active(generation)
            .expect("read the generation lease registry"),
        "{context}"
    );

    assert_eq!(clear_durable_storage_fault(), Some(persisting_fault(fault)), "{context}");
    let recovered = recover_source(&mut store.lease);
    assert_eq!(recovered.authority().slot(), old, "{context}: recovery admits the old slot");
    store.authority = AppendAuthority::of(mode, recovered);
    store.authority = append(&mut store.lease, &store.authority, &mutation, limits())
        .unwrap_or_else(|error| panic!("{context}: the retry on freed storage failed: {error:?}"));
    assert_eq!(directory_snapshot(&root), *control, "{context}: the retry publishes the control");
    assert_eq!(retention_picture(&registry), retention, "{context}");
    drop(known);
}

fn checkpoint_store(prefix: &str) -> (PersistenceTrialDir, Writer) {
    let scratch = trial_scratch(prefix);
    install_checkpoint_image(
        scratch.path(),
        &checkpoint_image_from_source(checkpoint_source_fixture()),
    );
    let writer = publish_data_successor(admit(scratch.path(), limits()), "checkpoint-storage");
    (scratch, writer)
}

fn checkpoint_control() -> Files {
    let (scratch, writer) = checkpoint_store("gc-wal-storage-checkpoint-control-");
    let old = writer.catalog();
    let old_files = directory_snapshot(scratch.path());
    let known = writer
        .register_known_root_v1(old.state().manifest())
        .expect("the current state manifest is a present object");
    let published = writer
        .publish_generation_checkpoint_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("the control checkpoint publishes");
    assert_ne!(published.catalog().pack().active_pack(), old.pack().active_pack());
    let files = directory_snapshot(scratch.path());
    for name in [pack_file_name(PackId::GENESIS), index_file_name(IndexGenerationV1::FIRST)] {
        assert_eq!(
            files.get(&name),
            old_files.get(&name),
            "the Known lease keeps {name} of the superseded generation"
        );
    }
    drop(published);
    drop(known);
    files
}

fn checkpoint_storage_case(
    fault: DurableStorageFaultV1,
    placement: FaultPlacement,
    control: &Files,
) {
    let context = format!("generation checkpoint, {fault:?} at {placement:?}");
    let (scratch, writer) = checkpoint_store("gc-wal-storage-checkpoint-");
    let old = writer.catalog();
    let old_files = directory_snapshot(scratch.path());
    let registry = writer.generation_leases();
    let known = writer
        .register_known_root_v1(old.state().manifest())
        .expect("the current state manifest is a present object");
    let retention = retention_picture(&registry);
    assert_eq!((retention.0, retention.1.roots().len()), (1, 1), "{context}");

    arm_storage_fault(fault, placement);
    let refusal = match writer.publish_generation_checkpoint_v1(
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    ) {
        Ok(_) => panic!("{context}: the checkpoint was acknowledged through the storage fault"),
        Err(error) => checkpoint_refusal(scratch.path(), &error),
    };
    assert_eq!(clear_durable_storage_fault(), Some(persisting_fault(fault)), "{context}");
    assert_eq!(refusal, expected_checkpoint_refusal(fault, placement), "{context}");
    assert_eq!(selected_slot(scratch.path()), old, "{context}: the catalog selects the old slot");
    let after_refusal = directory_snapshot(scratch.path());
    for name in [pack_file_name(PackId::GENESIS), index_file_name(IndexGenerationV1::FIRST)] {
        assert_eq!(
            after_refusal.get(&name),
            old_files.get(&name),
            "{context}: {name} of the old generation is unchanged"
        );
    }
    assert_eq!(retention_picture(&registry), retention, "{context}");
    assert!(
        registry
            .has_active(known.generation())
            .expect("read the generation lease registry"),
        "{context}"
    );

    let reopened = admit(scratch.path(), limits());
    assert_eq!(reopened.catalog(), old, "{context}: the reopened store has the old authority");
    assert!(registry.shares_registry(&reopened.generation_leases()), "{context}");
    assert_eq!(retention_picture(&registry), retention, "{context}");
    let published = reopened
        .publish_generation_checkpoint_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap_or_else(|error| panic!("{context}: the retry on freed storage failed: {error:?}"));
    assert_eq!(published.catalog().state(), old.state(), "{context}");
    assert_eq!(
        directory_snapshot(scratch.path()),
        *control,
        "{context}: the retry publishes the control"
    );
    assert!(
        registry
            .has_active(known.generation())
            .expect("read the generation lease registry"),
        "{context}"
    );
    drop(published);
    drop(known);
}

#[test]
fn an_append_is_acknowledged_only_inside_the_recovery_window() {
    const HISTORY: [&[&str]; 3] =
        [&["window-a"], &["window-b", "window-c"], &["window-d", "window-e", "window-f"]];
    const LOWERED_ON_THE_FIRST_APPEND: [DurableLimitKind; 6] = [
        DurableLimitKind::PackBytes,
        DurableLimitKind::ObjectRecords,
        DurableLimitKind::TotalScanBytes,
        DurableLimitKind::TotalPhysicalReadBytes,
        DurableLimitKind::IndexBytes,
        DurableLimitKind::IndexRecords,
    ];
    const LOWERED_ON_A_LATER_APPEND: [DurableLimitKind; 7] = [
        DurableLimitKind::PackBytes,
        DurableLimitKind::Transactions,
        DurableLimitKind::ObjectRecords,
        DurableLimitKind::TotalScanBytes,
        DurableLimitKind::TotalPhysicalReadBytes,
        DurableLimitKind::IndexBytes,
        DurableLimitKind::IndexRecords,
    ];
    let refusals_per_mode =
        LOWERED_ON_THE_FIRST_APPEND.len() + (HISTORY.len() - 1) * LOWERED_ON_A_LATER_APPEND.len();
    let mut refused = Vec::with_capacity(APPEND_MODES.len() * refusals_per_mode);
    let mut expected_refused = Vec::with_capacity(APPEND_MODES.len() * refusals_per_mode);
    for mode in APPEND_MODES {
        let mut store = open_store(mode, "gc-wal-admission-");
        let mut twin = open_store(mode, "gc-wal-admission-twin-");
        let registry = store.lease.generation_leases();
        let genesis = store.authority.slot();
        let known = registry
            .register_known_root(
                genesis.state().manifest(),
                GenerationLeaseKeyV1::from_catalog(genesis),
            )
            .expect("register a Known root on the store under test");
        for (step, identities) in HISTORY.iter().enumerate() {
            let context = format!("{mode:?}, append {step}");
            let lowered: &[DurableLimitKind] = match step {
                0 => &LOWERED_ON_THE_FIRST_APPEND,
                _ => &LOWERED_ON_A_LATER_APPEND,
            };
            expected_refused.extend(lowered.iter().map(|kind| (mode, step, *kind)));
            let base = store.authority.slot();
            assert_eq!(twin.authority.slot(), base, "{context}: the twin is one append ahead");
            let mutation = data_mutation(base.state(), identities);
            assert_eq!(
                reread_key_bytes(&mutation) != 0,
                identities.len() > 1,
                "{context}: recovery re-reads a prior key exactly when the batch has several"
            );
            twin.authority = append(&mut twin.lease, &twin.authority, &mutation, limits())
                .unwrap_or_else(|error| panic!("{context}: the twin append failed: {error:?}"));
            let demand = measured_demand(&mut twin.lease);
            let generation = GenerationLeaseKeyV1::from_catalog(base);
            let pin = registry
                .acquire(generation)
                .expect("pin the generation the append starts from");
            let retention = retention_picture(&registry);
            assert_eq!((retention.0, retention.1.roots().len()), (2, 1), "{context}");
            let files = directory_snapshot(store.scratch.path());

            for kind in WINDOW_KINDS {
                let required = demand.of(kind);
                let limit = match required {
                    0 => panic!("{context}: the successor demands nothing of {kind:?}"),
                    1 => continue,
                    required => required - 1,
                };
                let tight = demand.limits(Some(kind));
                let expected = DurableLimitError::Exceeded {
                    resource: kind,
                    limit,
                    attempted: required,
                };
                assert_eq!(
                    limit_refusal(recover_physical_from_lease_v1(
                        &mut twin.lease,
                        tight,
                        LogPackDecodeLimits::UNBOUNDED,
                    )),
                    Some(expected),
                    "{context}: recovery of the published successor under {kind:?} = {limit}"
                );
                let attempt = append(&mut store.lease, &store.authority, &mutation, tight);
                let outcome = attempt
                    .as_ref()
                    .err()
                    .map(IncrementalPublicationErrorV1::publication_outcome);
                match attempt {
                    Err(IncrementalPublicationErrorV1::Limit(refusal)) => assert_eq!(
                        (refusal, outcome),
                        (expected, Some(Outcome::DefinitelyNotPublished)),
                        "{context}: {kind:?}"
                    ),
                    Ok(_) => panic!(
                        "{context}: the append under {kind:?} = {limit} was acknowledged, and \
                         recovery under the same limits returns {:?}",
                        limit_refusal(recover_physical_from_lease_v1(
                            &mut store.lease,
                            tight,
                            LogPackDecodeLimits::UNBOUNDED,
                        ))
                    ),
                    Err(error) => panic!(
                        "{context}: the append under {kind:?} = {limit} was not refused by the \
                         recovery window; it ended as {outcome:?} with {error:?}"
                    ),
                }
                assert_eq!(
                    directory_snapshot(store.scratch.path()),
                    files,
                    "{context}: the refusal under {kind:?} changes no byte"
                );
                assert_eq!(retention_picture(&registry), retention, "{context}: {kind:?}");
                assert!(
                    registry
                        .has_active(generation)
                        .expect("read the generation lease registry"),
                    "{context}: {kind:?}"
                );
                refused.push((mode, step, kind));
            }

            assert_eq!(selected_slot(store.scratch.path()), base, "{context}");
            let exact = demand.limits(None);
            store.authority = append(&mut store.lease, &store.authority, &mutation, exact)
                .unwrap_or_else(|error| {
                    panic!("{context}: the append at its exact demand {demand:?} failed: {error:?}")
                });
            assert_eq!(
                directory_snapshot(store.scratch.path()),
                directory_snapshot(twin.scratch.path()),
                "{context}: the acknowledged store is the twin"
            );
            let carried = store.authority.core().recovery_window();
            assert_eq!(
                (
                    carried.transactions(),
                    carried.object_records(),
                    carried.scanned_bytes(),
                    carried.physical_read_bytes(),
                    carried.sealed_pack_bytes(),
                ),
                (
                    demand.transactions,
                    demand.object_records,
                    demand.total_scan_bytes,
                    demand.total_physical_read_bytes,
                    0,
                ),
                "{context}: the successor carries the demand that recovery measures"
            );
            let recovered = recover_physical_from_lease_v1(
                &mut store.lease,
                exact,
                LogPackDecodeLimits::UNBOUNDED,
            )
            .unwrap_or_else(|error| {
                panic!("{context}: recovery at the exact demand refused the store: {error:?}")
            });
            assert_eq!(recovered.catalog_authority().slot(), store.authority.slot(), "{context}");
            assert_eq!(recovered.recovery_window(), carried, "{context}");
            assert_eq!(retention_picture(&registry), retention, "{context}");
            drop(pin);
        }
        drop(known);
    }
    assert_eq!(refused, expected_refused);
    assert_eq!(refused.len(), APPEND_MODES.len() * refusals_per_mode);
}

#[test]
fn a_generation_checkpoint_reopens_a_full_transaction_window() {
    let window = DurableLimits::new(DurableLimitConfig {
        max_transactions: 2,
        ..limit_config()
    })
    .expect("a window of two transactions is a valid limit");
    let full = DurableLimitError::Exceeded {
        resource: DurableLimitKind::Transactions,
        limit: 2,
        attempted: 3,
    };
    for mode in APPEND_MODES {
        let scratch = trial_scratch("gc-wal-window-");
        install_checkpoint_image(
            scratch.path(),
            &checkpoint_image_from_source(checkpoint_source_fixture()),
        );
        let (mut lease, source) = admit(scratch.path(), window).into_parts();
        let mut authority = AppendAuthority::of(mode, source);
        for identity in ["window-first", "window-second"] {
            let mutation = data_mutation(authority.slot().state(), &[identity]);
            authority = append(&mut lease, &authority, &mutation, window).unwrap_or_else(|error| {
                panic!("{mode:?}: the append of {identity} inside the window failed: {error:?}")
            });
        }
        assert_eq!(authority.core().recovery_window().transactions(), 2, "{mode:?}");
        let filled = authority.slot();
        let files = directory_snapshot(scratch.path());
        let mutation = data_mutation(filled.state(), &["window-third"]);
        match append(&mut lease, &authority, &mutation, window) {
            Err(IncrementalPublicationErrorV1::Limit(refusal)) => {
                assert_eq!(refusal, full, "{mode:?}")
            }
            Ok(_) => panic!("{mode:?}: the third append was acknowledged past a window of two"),
            Err(error) => panic!("{mode:?}: the third append ended with {error:?}"),
        }
        assert_eq!(directory_snapshot(scratch.path()), files, "{mode:?}");
        drop(authority);
        drop(lease);

        let checkpointed = admit(scratch.path(), window)
            .publish_generation_checkpoint_v1(
                window,
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .unwrap_or_else(|error| {
                panic!("{mode:?}: the checkpoint of the full window failed: {error:?}")
            });
        let rotated = checkpointed.catalog();
        assert_eq!(rotated.state(), filled.state(), "{mode:?}");
        assert!(rotated.pack().first_live_pack() > filled.pack().active_pack(), "{mode:?}");
        let (mut lease, source) = checkpointed.into_parts();
        let recovered =
            recover_physical_from_lease_v1(&mut lease, window, LogPackDecodeLimits::UNBOUNDED)
                .unwrap_or_else(|error| {
                    panic!("{mode:?}: recovery refused the rotated generation: {error:?}")
                });
        assert_eq!((recovered.transactions(), recovered.checkpoints()), (0, 1), "{mode:?}");
        let authority = AppendAuthority::of(mode, source);
        assert_eq!(authority.core().recovery_window().transactions(), 0, "{mode:?}");
        let authority = append(&mut lease, &authority, &mutation, window).unwrap_or_else(|error| {
            panic!("{mode:?}: the append after the checkpoint failed: {error:?}")
        });
        assert_eq!(authority.slot().state().generation(), filled.state().generation() + 1);
        assert_eq!(authority.core().recovery_window().transactions(), 1, "{mode:?}");
    }
}

#[test]
fn the_journal_owner_keeps_its_journals_when_the_recovery_window_refuses_the_publication() {
    let (twin_scratch, twin_hot, twin_gate, mut twin_owner) = journal_owner_fixture();
    populate(&twin_hot);
    let target = poll_checkpoint_with(&mut twin_owner, &twin_gate, &twin_hot, publication_limits())
        .expect("the twin owner publishes under ample limits");
    let published = store_files(twin_scratch.path());
    let demand = selected_slot(twin_scratch.path()).durable_pack_tail();

    let (scratch, hot, gate, mut owner) = journal_owner_fixture();
    let old = owner.state(&hot).expect("a fresh owner is ready");
    populate(&hot);
    let registry = owner.generation_leases();
    let old_pin = owner.generation_key();
    let files = store_files(scratch.path());
    let journals = hot
        .pending_deltas()
        .expect("the populated journals are readable");
    let tight = DurableRSpacePublicationLimitsV1 {
        recovery: DurableLimits::new(DurableLimitConfig {
            max_pack_bytes: demand - 1,
            ..limit_config()
        })
        .expect("one byte below the successor's pack tail is a valid limit"),
        ..publication_limits()
    };
    let expected = DurableLimitError::Exceeded {
        resource: DurableLimitKind::PackBytes,
        limit: demand - 1,
        attempted: demand,
    };
    for attempt in 0..2 {
        match poll_checkpoint_with(&mut owner, &gate, &hot, tight) {
            Err(DurableRSpaceTransactionError::Publication(
                IncrementalPublicationErrorV1::Limit(refusal),
            )) => assert_eq!(refusal, expected, "attempt {attempt}"),
            Ok(state) => panic!(
                "attempt {attempt}: the owner acknowledged {state:?} past the recovery window"
            ),
            Err(error) => panic!("attempt {attempt}: the owner's checkpoint ended with {error:?}"),
        }
        assert_eq!(owner.phase(&hot), JournalPublicationPhaseV1::RecoveryRequired);
        assert!(owner.state(&hot).is_err(), "attempt {attempt}");
        assert!(owner.pending_terminal().is_none(), "attempt {attempt}");
        assert!(gate.try_read().is_err(), "attempt {attempt}");
        assert!(
            registry
                .has_active(old_pin)
                .expect("read the generation lease registry"),
            "attempt {attempt}"
        );
        assert_eq!(store_files(scratch.path()), files, "attempt {attempt}");
        assert_journals_unchanged(&hot, &journals);
    }
    assert_eq!(
        owner
            .reconcile(tight)
            .expect("the owner recovers its predecessor under the tight limits"),
        old
    );
    assert_eq!(owner.phase(&hot), JournalPublicationPhaseV1::Ready);
    assert!(gate.try_read().is_ok());
    assert_eq!(store_files(scratch.path()), files);
    assert_journals_unchanged(&hot, &journals);
    assert_eq!(
        poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
            .expect("the owner publishes once the window admits the successor"),
        target
    );
    assert_eq!(store_files(scratch.path()), published);
    assert_empty(&hot);
}

#[test]
fn the_preflight_bounds_the_live_packs_and_the_sealed_packs_that_recovery_reads() {
    let image = valid_two_pack_image();
    let scratch = trial_scratch("gc-wal-admission-two-packs-");
    let active_pack = PackId::GENESIS
        .successor()
        .expect("the genesis pack has a successor");
    write_owner_only(&scratch.path().join(CATALOG_FILE), &catalog_image(image.catalog));
    write_owner_only(&scratch.path().join(pack_file_name(PackId::GENESIS)), &image.first);
    write_owner_only(&scratch.path().join(pack_file_name(active_pack)), &image.second);
    let startup_refusal = |recovery_limits: DurableLimits| {
        limit_refusal(
            DurableDirectoryV1::open(scratch.path())
                .expect("open the two-pack trial directory")
                .recover_writer_v1(recovery_limits, LogPackDecodeLimits::UNBOUNDED),
        )
    };
    let limits_with = |config: DurableLimitConfig| {
        DurableLimits::new(config).expect("the lowered limit stays positive")
    };

    let (window, slot) = {
        let physical = DurableDirectoryV1::open(scratch.path())
            .expect("open the two-pack trial directory")
            .recover_writer_v1(limits(), LogPackDecodeLimits::UNBOUNDED)
            .expect("recover the two-pack store under ample limits");
        let recovered = physical.recovered();
        (recovered.recovery_window(), recovered.catalog_authority().slot())
    };
    let sealed_pack_bytes = image.first.len() as u64;
    assert_eq!(slot, image.catalog);
    assert_eq!(
        (slot.pack().first_live_pack(), slot.pack().active_pack()),
        (PackId::GENESIS.get(), active_pack.get())
    );
    assert_eq!(
        (window.transactions(), window.sealed_pack_bytes()),
        (image.transactions, sealed_pack_bytes)
    );

    let object_store = CanonicalObjectStore::new();
    let mutation =
        data_mutation(base_state(&object_store, 41, None), &["sealed-a", "sealed-b", "sealed-c"]);
    let sealed = seal_data(&object_store, &mutation);
    let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &EveryObjectMissing)
            .expect("classify every object of the record as inline"),
        slot.store_id(),
        slot.commit_sequence()
            .successor()
            .expect("the commit sequence has a successor"),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .expect("prepare a record for the two-pack store");
    assert_eq!(prepared.prior_key_reread_bytes(), reread_key_bytes(&mutation));
    assert_ne!(prepared.prior_key_reread_bytes(), 0);
    let appended = prepared.record_len() + TRANSACTION_COMMIT_RECORD_LEN_V1;

    let one_live_pack = limits_with(DurableLimitConfig {
        max_live_packs: 1,
        ..limit_config()
    });
    let live_packs = DurableLimitError::Exceeded {
        resource: DurableLimitKind::LivePacks,
        limit: 1,
        attempted: 2,
    };
    assert_eq!(window.admit_incremental_append(one_live_pack, slot, &prepared), Err(live_packs));
    assert_eq!(startup_refusal(one_live_pack), Some(live_packs));

    let below_the_sealed_pack = limits_with(DurableLimitConfig {
        max_pack_bytes: sealed_pack_bytes - 1,
        ..limit_config()
    });
    let sealed_pack = DurableLimitError::Exceeded {
        resource: DurableLimitKind::PackBytes,
        limit: sealed_pack_bytes - 1,
        attempted: sealed_pack_bytes,
    };
    assert_eq!(
        window.admit_incremental_append(below_the_sealed_pack, slot, &prepared),
        Err(sealed_pack)
    );
    assert_eq!(startup_refusal(below_the_sealed_pack), Some(sealed_pack));

    let pack_demand = sealed_pack_bytes.max(slot.durable_pack_tail() + appended);
    assert_eq!(
        window.admit_incremental_append(
            limits_with(DurableLimitConfig {
                max_pack_bytes: pack_demand - 1,
                ..limit_config()
            }),
            slot,
            &prepared,
        ),
        Err(DurableLimitError::Exceeded {
            resource: DurableLimitKind::PackBytes,
            limit: pack_demand - 1,
            attempted: pack_demand,
        })
    );
    let successor = window
        .admit_incremental_append(
            limits_with(DurableLimitConfig {
                max_live_packs: 2,
                max_pack_bytes: pack_demand,
                ..limit_config()
            }),
            slot,
            &prepared,
        )
        .expect("the preflight admits the append at its exact pack demand");
    assert_eq!(
        (
            successor.transactions(),
            successor.object_records(),
            successor.scanned_bytes(),
            successor.sealed_pack_bytes(),
        ),
        (
            window.transactions() + 1,
            window.object_records() + prepared.object_count(),
            window.scanned_bytes() + appended,
            sealed_pack_bytes,
        )
    );
    assert!(successor.physical_read_bytes() > window.physical_read_bytes() + appended);
    assert_eq!(startup_refusal(limits()), None);
}

#[test]
fn storage_faults_before_the_incremental_catalog_swap_refuse_typed_and_retry_once_space_is_freed() {
    let placements = pre_swap_placements(Point::AfterPrepareWrite);
    let mut cases = 0usize;
    for mode in APPEND_MODES {
        let control = incremental_control(mode);
        for fault in STORAGE_FAULTS {
            for placement in placements.iter().copied() {
                incremental_storage_case(mode, fault, placement, &control);
                cases += 1;
            }
        }
    }
    assert_eq!((placements.len(), cases), (10, 40));
}

#[test]
fn storage_faults_before_the_checkpoint_catalog_swap_refuse_typed_and_retry_once_space_is_freed() {
    let placements = pre_swap_placements(Point::AfterPreflightBeforeCleanup);
    let control = checkpoint_control();
    let mut cases = 0usize;
    for fault in STORAGE_FAULTS {
        for placement in placements.iter().copied() {
            checkpoint_storage_case(fault, placement, &control);
            cases += 1;
        }
    }
    assert_eq!((placements.len(), cases), (12, 24));
}
