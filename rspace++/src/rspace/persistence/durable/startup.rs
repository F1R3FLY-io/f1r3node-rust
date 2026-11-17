//! Typed durable startup and mandatory physical-index binding.
//!
//! No recovered state leaves this module until the exact catalog-authorized
//! WAL chain has been scanned and its rebuildable physical index has either
//! been verified byte-for-byte or atomically rebuilt, synchronized, reopened,
//! and verified again.

use std::convert::Infallible;
use std::fmt::{Display, Formatter};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use super::catalog::CatalogPackStateV1;
use super::index::{
    CanonicalIndexStreamV1, ObjectIndexPrefixVerifierV1, PreparedGenerationCheckpointIndexV1,
    PreparedGenerationCheckpointIndexVisitErrorV1,
};
#[cfg(test)]
use super::io::DurableIoOperation;
use super::io::{
    CatalogSlotIndexV1, DurableDirectoryV1, DurableFileV1, DurableIoError, DurableWriterLeaseV1,
    FileLengthExpectation,
};
use super::pack::PackHeaderV1;
use super::publication::preflight_recovery_limits;
use super::reclaim::{ReclamationCandidateV1, ReclamationCatalogRelationV1};
use super::recovery::{
    CatalogPackProviderV1, CatalogVerifiedRecoveryV1, RecoveryError, ScannedPackChainV1,
    scan_catalog_authorized_pack_chain, stage_catalog_verified_data_in_source_order,
};
use super::{
    CommittedObjectIndexV1, DurableFormatError, DurableLimitConfig, DurableLimitError,
    DurableLimitKind, DurableLimits, INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexFormatError,
    IndexGenerationV1, ObjectIndexHeaderV1, ObjectIndexRecordV1, PACK_HEADER_LEN_V1,
};
use crate::rspace::persistence::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, ObjectRefV1,
};
use crate::rspace::persistence::checkpoint::{
    CheckpointReferenceVisitErrorV1, GenerationCheckpointDescriptorV1, GenerationCheckpointErrorV1,
    try_visit_checkpoint_references_v2,
};
use crate::rspace::persistence::closure::{
    ClosureCreditV1, ClosureError, RetainedUnionClosureLimitsV1, StructuralClosureLedgerV1,
    StructuralValidationLimits, verify_structural_closure_in_ledger_with_credit,
};
use crate::rspace::persistence::manifest::{SEMANTIC_SCHEMA_EPOCH_V1, SemanticStateManifestV1};
use crate::rspace::persistence::object::CanonicalDagNode;
use crate::rspace::persistence::retention::RetainedStateV1;
use crate::rspace::persistence::retention::v2_format::{
    EMPTY_RETENTION_RADIX_NODE, RetentionAnchorV2, RetentionReader, empty_retention_radix_ref,
};
use crate::rspace::persistence::retention::v2_reader::{
    RetentionRadixErrorV2, RetentionRadixRefusalV2, admit_retention_history_v2,
};
use crate::rspace::persistence::semantic::SemanticProfile;
use crate::rspace::persistence::store::{
    CanonicalObjectStore, StagingObjectStore, StagingStoreError,
};
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, GenerationCheckpointCommitRecordV1,
    GenerationCheckpointPrepareWriteErrorV1, GenerationCheckpointSourceCopyErrorV1,
    GenerationCheckpointSourceV1, PreparedGenerationCheckpointV1, TerminalCommitViewV1,
};
use crate::rspace::persistence::{
    CatalogAuthorityTokenV1, CatalogSlotV1, LogPackAddress, LogPackDecodeLimits,
    LogPackFormatError, PackId, PackTerminalIdentityV1, PhysicalDigest, SemanticProfileId,
    SemanticRoots, StateVersionV1, StoreId,
};

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DurableStartupHookPointV1 {
    BeforeFirstCatalogVerification,
    BeforeSecondCatalogVerification,
    BeforePostInstallIndexVerification,
    BeforeFinalCatalogVerification,
}

#[cfg(test)]
type DurableStartupHookV1 = (DurableStartupHookPointV1, Box<dyn FnOnce() + 'static>);

#[cfg(test)]
thread_local! {
    static DURABLE_STARTUP_HOOK: std::cell::RefCell<Option<DurableStartupHookV1>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn arm_durable_startup_hook(
    point: DurableStartupHookPointV1,
    hook: impl FnOnce() + 'static,
) {
    DURABLE_STARTUP_HOOK.with(|armed| {
        let previous = armed.borrow_mut().replace((point, Box::new(hook)));
        assert!(previous.is_none(), "durable startup hook already armed");
    });
}

#[cfg(test)]
fn run_durable_startup_hook(point: DurableStartupHookPointV1) {
    DURABLE_STARTUP_HOOK.with(|armed| {
        let should_run = armed
            .borrow()
            .as_ref()
            .is_some_and(|(armed_point, _)| *armed_point == point);
        if should_run {
            let (_, hook) = armed.borrow_mut().take().expect("checked startup hook");
            hook();
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IndexBindingDispositionV1 {
    VerifiedExisting,
    RebuiltAndVerified,
}

/// Exact startup work, deliberately separate from both logical state-size
/// inventories and retained physical storage. These counters describe I/O
/// performed by this recovery attempt; they are diagnostics, never consensus
/// authority or a persisted-byte ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DurableStartupWorkV1 {
    wal_scan_passes: u8,
    wal_bytes_read: u64,
    index_bytes_read: u64,
    index_bytes_written: u64,
    file_syncs: u64,
    directory_syncs: u64,
}

impl DurableStartupWorkV1 {
    pub(crate) const fn wal_scan_passes(self) -> u8 { self.wal_scan_passes }

    pub(crate) const fn wal_bytes_read(self) -> u64 { self.wal_bytes_read }

    pub(crate) const fn index_bytes_read(self) -> u64 { self.index_bytes_read }

    pub(crate) const fn index_bytes_written(self) -> u64 { self.index_bytes_written }

    pub(crate) const fn file_syncs(self) -> u64 { self.file_syncs }

    pub(crate) const fn directory_syncs(self) -> u64 { self.directory_syncs }
}

/// The only startup result that exposes recovered RSpace state.
pub(crate) struct IndexBoundPhysicalRecoveryV1 {
    recovery: CatalogVerifiedRecoveryV1,
    disposition: IndexBindingDispositionV1,
    work: DurableStartupWorkV1,
}

impl IndexBoundPhysicalRecoveryV1 {
    pub(crate) const fn store_id(&self) -> StoreId { self.recovery.store_id() }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this profile accessor yet"
    )]
    pub(crate) const fn profile_id(&self) -> SemanticProfileId { self.recovery.profile_id() }

    pub(crate) const fn state(&self) -> StateVersionV1 { self.recovery.state() }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this terminal accessor yet"
    )]
    pub(crate) const fn terminal(&self) -> PackTerminalIdentityV1 { self.recovery.terminal() }

    pub(crate) const fn terminal_address(&self) -> LogPackAddress {
        self.recovery.terminal_address()
    }

    pub(crate) const fn expected_index_tail(&self) -> u64 { self.recovery.expected_index_tail() }

    pub(crate) const fn transactions(&self) -> u64 { self.recovery.transactions() }

    pub(crate) const fn checkpoints(&self) -> u64 { self.recovery.checkpoints() }

    pub(crate) const fn object_records(&self) -> u64 { self.recovery.object_records() }

    pub(crate) const fn scanned_bytes(&self) -> u64 { self.recovery.scanned_bytes() }

    pub(crate) const fn recovery_window(&self) -> super::recovery::RecoveryWindowDemandV1 {
        self.recovery.window_demand()
    }

    pub(crate) fn object_index(&self) -> &CommittedObjectIndexV1 { self.recovery.object_index() }

    pub(crate) const fn catalog_authority(&self) -> &CatalogAuthorityTokenV1 {
        self.recovery.authority()
    }

    pub(crate) const fn disposition(&self) -> IndexBindingDispositionV1 { self.disposition }

    pub(crate) const fn startup_work(&self) -> DurableStartupWorkV1 { self.work }

    pub(super) fn into_admission_parts(
        self,
    ) -> (
        CatalogAuthorityTokenV1,
        CommittedObjectIndexV1,
        TerminalCommitViewV1,
        IndexBindingDispositionV1,
        DurableStartupWorkV1,
    ) {
        let terminal_commit = self.recovery.terminal_commit();
        let (authority, index) = self.recovery.into_authority_and_index();
        (authority, index, terminal_commit, self.disposition, self.work)
    }
}

/// A recovered writer retains the process-wide writer lease for as long as
/// the recovered authority is usable.
pub(crate) struct PhysicallyRecoveredDurableWriterV1 {
    lease: DurableWriterLeaseV1,
    recovered: IndexBoundPhysicalRecoveryV1,
}

impl PhysicallyRecoveredDurableWriterV1 {
    #[cfg(test)]
    pub(crate) const fn recovered(&self) -> &IndexBoundPhysicalRecoveryV1 { &self.recovered }

    pub(super) fn into_admission_parts(
        self,
    ) -> (DurableWriterLeaseV1, IndexBoundPhysicalRecoveryV1) {
        (self.lease, self.recovered)
    }
}

#[derive(Debug)]
pub(crate) enum DurableStartupErrorV1 {
    Io(DurableIoError),
    Catalog(DurableFormatError),
    NoCatalogAuthority,
    Recovery(RecoveryError<DurableIoError>),
    CatalogVerification(RecoveryError<Infallible>),
    Index(IndexFormatError),
    Limit(DurableLimitError),
    ReplayMismatch,
    PostInstallIndexMismatch,
}

impl Display for DurableStartupErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Catalog(error) => Display::fmt(error, formatter),
            Self::NoCatalogAuthority => formatter.write_str("durable catalog has no current slot"),
            Self::Recovery(error) => Display::fmt(error, formatter),
            Self::CatalogVerification(error) => Display::fmt(error, formatter),
            Self::Index(error) => Display::fmt(error, formatter),
            Self::Limit(error) => Display::fmt(error, formatter),
            Self::ReplayMismatch => formatter.write_str(
                "index-repair WAL replay did not reproduce the authoritative recovery summary",
            ),
            Self::PostInstallIndexMismatch => formatter.write_str(
                "atomically installed physical index failed its mandatory WAL re-verification",
            ),
        }
    }
}

impl std::error::Error for DurableStartupErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Catalog(error) => Some(error),
            Self::Recovery(error) => Some(error),
            Self::CatalogVerification(error) => Some(error),
            Self::Index(error) => Some(error),
            Self::Limit(error) => Some(error),
            Self::NoCatalogAuthority | Self::ReplayMismatch | Self::PostInstallIndexMismatch => {
                None
            }
        }
    }
}

impl From<DurableIoError> for DurableStartupErrorV1 {
    fn from(error: DurableIoError) -> Self { Self::Io(error) }
}

impl From<DurableFormatError> for DurableStartupErrorV1 {
    fn from(error: DurableFormatError) -> Self { Self::Catalog(error) }
}

impl From<IndexFormatError> for DurableStartupErrorV1 {
    fn from(error: IndexFormatError) -> Self { Self::Index(error) }
}

impl From<DurableLimitError> for DurableStartupErrorV1 {
    fn from(error: DurableLimitError) -> Self { Self::Limit(error) }
}

impl DurableDirectoryV1 {
    /// Internal physical phase of profile-typed durable admission.
    pub(super) fn recover_physical_writer_v1(
        self,
        limits: DurableLimits,
        wal_limits: LogPackDecodeLimits,
    ) -> Result<PhysicallyRecoveredDurableWriterV1, DurableStartupErrorV1> {
        recover_physical_with_lease_v1(self.try_acquire_writer()?, limits, wal_limits)
    }

    /// Focused physical-recovery tests exercise this phase independently.
    /// Production code cannot name it and must use `recover_and_admit_v1`.
    #[cfg(test)]
    pub(crate) fn recover_writer_v1(
        self,
        limits: DurableLimits,
        wal_limits: LogPackDecodeLimits,
    ) -> Result<PhysicallyRecoveredDurableWriterV1, DurableStartupErrorV1> {
        self.recover_physical_writer_v1(limits, wal_limits)
    }
}

/// Run the exact restart-grade physical recovery path while retaining an
/// already-held writer lease.
///
/// Generation publication cannot reacquire its own process lock. Consuming
/// the affine lease here keeps the lock continuously held while deliberately
/// discarding every stale in-memory index/store view and reconstructing
/// authority from the catalog-appointed durable bytes.
pub(crate) fn recover_physical_with_lease_v1(
    mut lease: DurableWriterLeaseV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
) -> Result<PhysicallyRecoveredDurableWriterV1, DurableStartupErrorV1> {
    let recovered = recover_physical_from_lease_v1(&mut lease, limits, wal_limits)?;
    Ok(PhysicallyRecoveredDurableWriterV1 { lease, recovered })
}

pub(crate) fn recover_physical_from_lease_v1(
    lease: &mut DurableWriterLeaseV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
) -> Result<IndexBoundPhysicalRecoveryV1, DurableStartupErrorV1> {
    lease.remove_reclamation_temporary()?;
    let candidate = lease.read_reclamation_candidate()?;
    let reader = lease.directory().try_clone_reader()?;
    let recovered = recover_index_bound_v1(&reader, lease, limits, wal_limits)?;
    resolve_reclamation_candidate_v1(lease, candidate, recovered.catalog_authority().slot())?;
    Ok(recovered)
}

pub(super) fn resolve_reclamation_candidate_v1(
    lease: &mut DurableWriterLeaseV1,
    candidate: Option<ReclamationCandidateV1>,
    selected: CatalogSlotV1,
) -> Result<(), DurableStartupErrorV1> {
    if let Some(candidate) = candidate {
        match candidate.relation_to_catalog(selected) {
            ReclamationCatalogRelationV1::PreCas => {
                lease.remove_reclamation_candidate()?;
            }
            ReclamationCatalogRelationV1::SelectedSuccessor |
            ReclamationCatalogRelationV1::SelectedDescendant => {
                lease.reclaim_reclamation_candidate(candidate, selected)?;
            }
            ReclamationCatalogRelationV1::Unrelated => {
                return Err(DurableStartupErrorV1::Io(DurableIoError::CatalogAuthorityChanged {
                    path: lease.directory().path().to_path_buf(),
                }));
            }
        }
    }
    Ok(())
}

struct DirectoryPackProviderV1<'directory>(&'directory DurableDirectoryV1);

impl CatalogPackProviderV1 for DirectoryPackProviderV1<'_> {
    type Reader = DurableFileV1;
    type Error = DurableIoError;

    fn open_pack(&mut self, pack_id: PackId) -> Result<Self::Reader, Self::Error> {
        self.0.open_pack(pack_id)
    }

    fn retention_reader(&self) -> RetentionReader { self.0.retention_reader() }
}

enum PhysicalIndexProbeV1 {
    Rebuild {
        bytes_read: u64,
    },
    Verifying {
        file: DurableFileV1,
        verifier: ObjectIndexPrefixVerifierV1,
        bytes_read: u64,
    },
}

#[derive(Clone, Copy)]
struct PhysicalIndexProbeOutcomeV1 {
    valid: bool,
    bytes_read: u64,
}

impl PhysicalIndexProbeV1 {
    fn open(
        directory: &DurableDirectoryV1,
        header: ObjectIndexHeaderV1,
        appointed_tail: u64,
    ) -> Result<Self, DurableStartupErrorV1> {
        let file = match directory.open_index(header.generation()) {
            Ok(file) => file,
            Err(error) if error.is_missing_or_short_file() => {
                return Ok(Self::Rebuild { bytes_read: 0 });
            }
            Err(error) => return Err(error.into()),
        };
        let available = file.len()?;
        // A suffix beyond the catalog frontier is recoverable, but retaining
        // it would make physical accounting and the final typed invariant
        // ambiguous. Rebuild to the one canonical exact length.
        if available != appointed_tail {
            return Ok(Self::Rebuild { bytes_read: 0 });
        }
        let mut verifier = match ObjectIndexPrefixVerifierV1::new(header, appointed_tail, available)
        {
            Ok(verifier) => verifier,
            Err(IndexFormatError::DurableTailBeyondFile { .. }) => {
                return Ok(Self::Rebuild { bytes_read: 0 });
            }
            Err(error) => return Err(error.into()),
        };
        let mut header_bytes = [0u8; INDEX_HEADER_LEN_V1 as usize];
        match file.read_exact_at(0, &mut header_bytes) {
            Ok(()) => {}
            Err(error) if error.is_missing_or_short_file() => {
                return Ok(Self::Rebuild { bytes_read: 0 });
            }
            Err(error) => return Err(error.into()),
        }
        if verifier.verify_header(&header_bytes).is_err() {
            return Ok(Self::Rebuild {
                bytes_read: INDEX_HEADER_LEN_V1,
            });
        }
        Ok(Self::Verifying {
            file,
            verifier,
            bytes_read: INDEX_HEADER_LEN_V1,
        })
    }

    fn verify_against_authority(
        self,
        authority: &CommittedObjectIndexV1,
    ) -> Result<PhysicalIndexProbeOutcomeV1, RecoveryError<DurableIoError>> {
        match self {
            Self::Rebuild { bytes_read } => Ok(PhysicalIndexProbeOutcomeV1 {
                valid: false,
                bytes_read,
            }),
            Self::Verifying {
                file,
                mut verifier,
                mut bytes_read,
            } => {
                let mut previous_position = None;
                let mut records_seen = 0u64;
                while records_seen < authority.record_count() {
                    let offset = verifier.next_record_offset();
                    let mut bytes = [0u8; INDEX_RECORD_LEN_V1 as usize];
                    match file.read_exact_at(offset, &mut bytes) {
                        Ok(()) => {}
                        Err(error) if error.is_missing_or_short_file() => {
                            return Ok(PhysicalIndexProbeOutcomeV1 {
                                valid: false,
                                bytes_read,
                            });
                        }
                        Err(error) => return Err(RecoveryError::Read(error)),
                    }
                    bytes_read = bytes_read
                        .checked_add(INDEX_RECORD_LEN_V1)
                        .ok_or(RecoveryError::Arithmetic("physical index bytes read"))?;
                    let record = match ObjectIndexRecordV1::parse_exact(&bytes) {
                        Ok(record) => record,
                        Err(_) => {
                            return Ok(PhysicalIndexProbeOutcomeV1 {
                                valid: false,
                                bytes_read,
                            });
                        }
                    };
                    if authority.get(record.reference().digest()) != Some(record) {
                        return Ok(PhysicalIndexProbeOutcomeV1 {
                            valid: false,
                            bytes_read,
                        });
                    }
                    let position =
                        (record.location().pack_id().get(), record.location().canonical_offset());
                    if previous_position.is_some_and(|previous| previous >= position) {
                        return Ok(PhysicalIndexProbeOutcomeV1 {
                            valid: false,
                            bytes_read,
                        });
                    }
                    previous_position = Some(position);
                    if verifier.accept_next_record(record).is_err() {
                        return Ok(PhysicalIndexProbeOutcomeV1 {
                            valid: false,
                            bytes_read,
                        });
                    }
                    records_seen = records_seen
                        .checked_add(1)
                        .ok_or(RecoveryError::Arithmetic("physical index records read"))?;
                }
                Ok(PhysicalIndexProbeOutcomeV1 {
                    valid: verifier.finish().is_ok(),
                    bytes_read,
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RecoverySummaryV1 {
    store_id: StoreId,
    profile_id: SemanticProfileId,
    state: StateVersionV1,
    terminal: PackTerminalIdentityV1,
    terminal_address: LogPackAddress,
    index_tail: u64,
    transactions: u64,
    checkpoints: u64,
    object_records: u64,
    scanned_bytes: u64,
}

impl RecoverySummaryV1 {
    fn of<I>(recovery: &CatalogVerifiedRecoveryV1<I>) -> Self {
        Self {
            store_id: recovery.store_id(),
            profile_id: recovery.profile_id(),
            state: recovery.state(),
            terminal: recovery.terminal(),
            terminal_address: recovery.terminal_address(),
            index_tail: recovery.expected_index_tail(),
            transactions: recovery.transactions(),
            checkpoints: recovery.checkpoints(),
            object_records: recovery.object_records(),
            scanned_bytes: recovery.scanned_bytes(),
        }
    }
}

pub(super) fn read_catalog_token(
    directory: &DurableDirectoryV1,
) -> Result<CatalogAuthorityTokenV1, DurableStartupErrorV1> {
    let image = directory.open_catalog()?.read_catalog_image()?;
    CatalogAuthorityTokenV1::select(image)?.ok_or(DurableStartupErrorV1::NoCatalogAuthority)
}

fn verify_catalog_scan<I>(
    directory: &DurableDirectoryV1,
    scan: ScannedPackChainV1<I>,
) -> Result<CatalogVerifiedRecoveryV1<I>, DurableStartupErrorV1> {
    let current = read_catalog_token(directory)?;
    scan.verify_catalog(current)
        .map_err(DurableStartupErrorV1::CatalogVerification)
}

fn scan_authorized_chain(
    directory: &DurableDirectoryV1,
    authority: &CatalogAuthorityTokenV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
) -> Result<ScannedPackChainV1, DurableStartupErrorV1> {
    let mut provider = DirectoryPackProviderV1(directory);
    scan_catalog_authorized_pack_chain(&mut provider, authority, limits, wal_limits)
        .map_err(DurableStartupErrorV1::Recovery)
}

fn remaining_scan_limits(
    limits: DurableLimits,
    consumed_scan: u64,
    consumed_physical: u64,
) -> Result<DurableLimits, DurableStartupErrorV1> {
    let scan_maximum = limits.maximum(DurableLimitKind::TotalScanBytes).get();
    let remaining_scan = scan_maximum
        .checked_sub(consumed_scan)
        .filter(|remaining| *remaining != 0)
        .ok_or(DurableLimitError::Exceeded {
            resource: DurableLimitKind::TotalScanBytes,
            limit: scan_maximum,
            attempted: consumed_scan.saturating_add(1),
        })?;
    let physical_maximum = limits
        .maximum(DurableLimitKind::TotalPhysicalReadBytes)
        .get();
    let remaining_physical = physical_maximum
        .checked_sub(consumed_physical)
        .filter(|remaining| *remaining != 0)
        .ok_or(DurableLimitError::Exceeded {
            resource: DurableLimitKind::TotalPhysicalReadBytes,
            limit: physical_maximum,
            attempted: consumed_physical.saturating_add(1),
        })?;
    DurableLimits::new(DurableLimitConfig {
        max_live_packs: limits.maximum(DurableLimitKind::LivePacks).get(),
        max_pack_bytes: limits.maximum(DurableLimitKind::PackBytes).get(),
        max_transactions: limits.maximum(DurableLimitKind::Transactions).get(),
        max_object_records: limits.maximum(DurableLimitKind::ObjectRecords).get(),
        max_total_scan_bytes: remaining_scan,
        max_total_physical_read_bytes: remaining_physical,
        max_index_bytes: limits.maximum(DurableLimitKind::IndexBytes).get(),
        max_index_records: limits.maximum(DurableLimitKind::IndexRecords).get(),
        max_full_byte_comparison_bytes: limits
            .maximum(DurableLimitKind::FullByteComparisonBytes)
            .get(),
        max_aggregate_full_byte_comparison_bytes: limits
            .maximum(DurableLimitKind::AggregateFullByteComparisonBytes)
            .get(),
    })
    .map_err(Into::into)
}

fn rebuild_index_pass(
    directory: &DurableDirectoryV1,
    lease: &mut DurableWriterLeaseV1,
    authoritative_recovery: &CatalogVerifiedRecoveryV1,
    header: ObjectIndexHeaderV1,
    appointed_tail: u64,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
) -> Result<(CatalogVerifiedRecoveryV1<()>, DurableFileV1), DurableStartupErrorV1> {
    let mut candidate = lease.create_rebuilt_index(header.generation())?;
    let mut stream = CanonicalIndexStreamV1::new(header);
    candidate.write_all_at(0, &stream.header_image())?;
    let mut provider = DirectoryPackProviderV1(directory);
    let scan = stage_catalog_verified_data_in_source_order(
        &mut provider,
        authoritative_recovery,
        limits,
        wal_limits,
        |record| {
            let image = stream.next_record(record).map_err(RecoveryError::Index)?;
            candidate
                .write_all_at(image.offset(), image.bytes())
                .map_err(RecoveryError::Read)
        },
    )
    .map_err(DurableStartupErrorV1::Recovery)?;
    stream.verify_appointed_tail(appointed_tail)?;
    #[cfg(test)]
    run_durable_startup_hook(DurableStartupHookPointV1::BeforeSecondCatalogVerification);
    let recovery = verify_catalog_scan(directory, scan)?;
    if !recovery
        .authority()
        .matches_exactly(authoritative_recovery.authority().image())
    {
        return Err(DurableStartupErrorV1::CatalogVerification(
            RecoveryError::CatalogAuthorityChanged,
        ));
    }
    let installed = candidate.sync_and_seal(appointed_tail)?.publish_replace()?;
    Ok((recovery, installed))
}

fn recover_index_bound_v1(
    directory: &DurableDirectoryV1,
    lease: &mut DurableWriterLeaseV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
) -> Result<IndexBoundPhysicalRecoveryV1, DurableStartupErrorV1> {
    let authority = read_catalog_token(directory)?;
    let catalog = authority.slot();
    let generation = IndexGenerationV1::new(catalog.pack().index_generation())?;
    let header = ObjectIndexHeaderV1::new(catalog.store_id(), generation);
    let appointed_tail = catalog.durable_index_tail();
    let first_scan = scan_authorized_chain(directory, &authority, limits, wal_limits)?;
    #[cfg(test)]
    run_durable_startup_hook(DurableStartupHookPointV1::BeforeFirstCatalogVerification);
    let first = verify_catalog_scan(directory, first_scan)?;
    if !first.authority().matches_exactly(authority.image()) {
        return Err(DurableStartupErrorV1::CatalogVerification(
            RecoveryError::CatalogAuthorityChanged,
        ));
    }
    let first_probe = PhysicalIndexProbeV1::open(directory, header, appointed_tail)?
        .verify_against_authority(first.object_index())
        .map_err(DurableStartupErrorV1::Recovery)?;
    if first_probe.valid {
        let wal_bytes_read = first.read_work().physical_read_bytes();
        return Ok(IndexBoundPhysicalRecoveryV1 {
            recovery: first,
            disposition: IndexBindingDispositionV1::VerifiedExisting,
            work: DurableStartupWorkV1 {
                wal_scan_passes: 1,
                wal_bytes_read,
                index_bytes_read: first_probe.bytes_read,
                index_bytes_written: 0,
                file_syncs: 0,
                directory_syncs: 0,
            },
        });
    }

    // Index repair requires one additional source-order scan of the same
    // catalog-appointed pack extents. Admit both scans' aggregate I/O before the
    // temporary index is created, so a resource-limit refusal cannot mutate
    // durable state or replace the official accelerator halfway through.
    let repair_scan_bytes =
        first
            .scanned_bytes()
            .checked_mul(2)
            .ok_or(DurableStartupErrorV1::Recovery(RecoveryError::Arithmetic(
                "two-pass repair scan bytes",
            )))?;
    limits.require(DurableLimitKind::TotalScanBytes, repair_scan_bytes)?;
    let projected_physical_read_bytes = first
        .read_work()
        .physical_read_bytes()
        .checked_mul(2)
        .ok_or(DurableStartupErrorV1::Recovery(RecoveryError::Arithmetic(
            "two-pass physical read bytes",
        )))?;
    limits.require(DurableLimitKind::TotalPhysicalReadBytes, projected_physical_read_bytes)?;

    let expected = RecoverySummaryV1::of(&first);
    let consumed_scan_bytes = first.scanned_bytes();
    let consumed_physical_read_bytes = first.read_work().physical_read_bytes();
    let second_limits =
        remaining_scan_limits(limits, consumed_scan_bytes, consumed_physical_read_bytes)?;
    let (second, installed) = rebuild_index_pass(
        directory,
        lease,
        &first,
        header,
        appointed_tail,
        second_limits,
        wal_limits,
    )?;
    if RecoverySummaryV1::of(&second) != expected {
        return Err(DurableStartupErrorV1::ReplayMismatch);
    }
    let _total_scan_bytes = limits.checked_charge(
        DurableLimitKind::TotalScanBytes,
        consumed_scan_bytes,
        second.scanned_bytes(),
    )?;
    let total_physical_read_bytes = limits.checked_charge(
        DurableLimitKind::TotalPhysicalReadBytes,
        consumed_physical_read_bytes,
        second.read_work().physical_read_bytes(),
    )?;
    drop(installed);

    #[cfg(test)]
    run_durable_startup_hook(DurableStartupHookPointV1::BeforePostInstallIndexVerification);
    let post_install_probe = PhysicalIndexProbeV1::open(directory, header, appointed_tail)?
        .verify_against_authority(first.object_index())
        .map_err(DurableStartupErrorV1::Recovery)?;
    if !post_install_probe.valid {
        return Err(DurableStartupErrorV1::PostInstallIndexMismatch);
    }
    #[cfg(test)]
    run_durable_startup_hook(DurableStartupHookPointV1::BeforeFinalCatalogVerification);
    let final_authority = read_catalog_token(directory)?;
    if !final_authority.matches_exactly(authority.image()) {
        return Err(DurableStartupErrorV1::CatalogVerification(
            RecoveryError::CatalogAuthorityChanged,
        ));
    }
    let index_bytes_read = first_probe
        .bytes_read
        .checked_add(post_install_probe.bytes_read)
        .ok_or(DurableStartupErrorV1::Recovery(RecoveryError::Arithmetic(
            "startup index bytes read",
        )))?;
    Ok(IndexBoundPhysicalRecoveryV1 {
        recovery: first,
        disposition: IndexBindingDispositionV1::RebuiltAndVerified,
        work: DurableStartupWorkV1 {
            wal_scan_passes: 2,
            wal_bytes_read: total_physical_read_bytes,
            index_bytes_read,
            index_bytes_written: appointed_tail,
            file_syncs: 1,
            directory_syncs: 1,
        },
    })
}

const EMPTY_STORE_GENERATION_V1: u64 = 1;
const EMPTY_STORE_CATALOG_EPOCH_V1: u64 = 1;

type EmptyStateStagingErrorV1 = StagingStoreError<CanonicalFormatError>;

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DurableStoreGenesisStepV1 {
    CreatePack,
    WritePackHeader,
    WritePrepare,
    SyncPrepare,
    CreateIndex,
    WriteIndex,
    SyncIndex,
    WriteCommit,
    SyncCommit,
    SyncArtifactDirectory,
    CreateCatalog,
    WriteCatalogSlot,
    SyncCatalog,
    SyncCatalogDirectory,
}

#[cfg(test)]
impl DurableStoreGenesisStepV1 {
    const fn operation(self) -> DurableIoOperation {
        match self {
            Self::CreatePack | Self::CreateIndex | Self::CreateCatalog => {
                DurableIoOperation::CreateFile
            }
            Self::WritePackHeader |
            Self::WritePrepare |
            Self::WriteIndex |
            Self::WriteCommit |
            Self::WriteCatalogSlot => DurableIoOperation::PositionalWrite,
            Self::SyncPrepare | Self::SyncIndex | Self::SyncCommit | Self::SyncCatalog => {
                DurableIoOperation::SyncFile
            }
            Self::SyncArtifactDirectory | Self::SyncCatalogDirectory => {
                DurableIoOperation::SyncDirectory
            }
        }
    }
}

#[cfg(test)]
type DurableStoreGenesisTraceV1 = Option<Vec<DurableStoreGenesisStepV1>>;

#[cfg(test)]
thread_local! {
    static DURABLE_STORE_GENESIS_FAILPOINT: std::cell::Cell<Option<DurableStoreGenesisStepV1>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
thread_local! {
    static DURABLE_STORE_GENESIS_TRACE: std::cell::RefCell<DurableStoreGenesisTraceV1> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn start_durable_store_genesis_trace() {
    DURABLE_STORE_GENESIS_TRACE.with(|trace| {
        let previous = trace.borrow_mut().replace(Vec::new());
        assert!(previous.is_none(), "durable store genesis trace already started");
    });
}

#[cfg(test)]
pub(crate) fn take_durable_store_genesis_trace() -> Vec<DurableStoreGenesisStepV1> {
    DURABLE_STORE_GENESIS_TRACE.with(|trace| {
        trace
            .borrow_mut()
            .take()
            .expect("a durable store genesis trace was started")
    })
}

#[cfg(test)]
pub(crate) fn arm_durable_store_genesis_failpoint(step: DurableStoreGenesisStepV1) {
    DURABLE_STORE_GENESIS_FAILPOINT.with(|armed| {
        let previous = armed.replace(Some(step));
        assert!(previous.is_none(), "durable store genesis failpoint already armed");
    });
}

#[cfg(test)]
fn reach_durable_store_genesis_step(
    step: DurableStoreGenesisStepV1,
    path: &Path,
) -> Result<(), DurableIoError> {
    DURABLE_STORE_GENESIS_TRACE.with(|trace| {
        if let Some(steps) = trace.borrow_mut().as_mut() {
            steps.push(step);
        }
    });
    DURABLE_STORE_GENESIS_FAILPOINT.with(|armed| match armed.get() {
        Some(armed_step) if armed_step == step => {
            armed.set(None);
            Err(DurableIoError::Io {
                operation: step.operation(),
                path: path.to_path_buf(),
                source: std::io::Error::other(format!(
                    "injected durable store genesis failure at {step:?}"
                )),
            })
        }
        _ => Ok(()),
    })
}

#[derive(Debug)]
pub(crate) enum DurableStoreGenesisErrorV1 {
    Io(DurableIoError),
    CatalogIndeterminate(DurableIoError),
    DirectoryHoldsStoreFiles {
        directory: PathBuf,
        files: Box<[String]>,
    },
    Canonical(CanonicalFormatError),
    RetentionRadix(RetentionRadixRefusalV2),
    Checkpoint(GenerationCheckpointErrorV1),
    LogPack(Box<LogPackFormatError>),
    Index(Box<IndexFormatError>),
    Catalog(Box<DurableFormatError>),
    Limit(DurableLimitError),
}

impl Display for DurableStoreGenesisErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::CatalogIndeterminate(error) => write!(
                formatter,
                "the empty store's catalog slot may already be authoritative: {error}"
            ),
            Self::DirectoryHoldsStoreFiles { directory, files } => write!(
                formatter,
                "{} already holds durable store files, so no empty store is created there: {}",
                directory.display(),
                files.join(", ")
            ),
            Self::Canonical(error) => Display::fmt(error, formatter),
            Self::RetentionRadix(error) => Display::fmt(error, formatter),
            Self::Checkpoint(error) => Display::fmt(error, formatter),
            Self::LogPack(error) => Display::fmt(error, formatter),
            Self::Index(error) => Display::fmt(error, formatter),
            Self::Catalog(error) => Display::fmt(error, formatter),
            Self::Limit(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for DurableStoreGenesisErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) | Self::CatalogIndeterminate(error) => Some(error),
            Self::DirectoryHoldsStoreFiles { .. } => None,
            Self::Canonical(error) => Some(error),
            Self::RetentionRadix(error) => Some(error),
            Self::Checkpoint(error) => Some(error),
            Self::LogPack(error) => Some(&**error),
            Self::Index(error) => Some(&**error),
            Self::Catalog(error) => Some(&**error),
            Self::Limit(error) => Some(error),
        }
    }
}

impl From<DurableIoError> for DurableStoreGenesisErrorV1 {
    fn from(error: DurableIoError) -> Self { Self::Io(error) }
}

impl From<CanonicalFormatError> for DurableStoreGenesisErrorV1 {
    fn from(error: CanonicalFormatError) -> Self { Self::Canonical(error) }
}

impl From<EmptyStateStagingErrorV1> for DurableStoreGenesisErrorV1 {
    fn from(error: EmptyStateStagingErrorV1) -> Self {
        match error {
            StagingStoreError::Base(error) | StagingStoreError::Format(error) => {
                Self::Canonical(error)
            }
        }
    }
}

impl From<ClosureError<EmptyStateStagingErrorV1>> for DurableStoreGenesisErrorV1 {
    fn from(error: ClosureError<EmptyStateStagingErrorV1>) -> Self {
        match error {
            ClosureError::Format(error) => Self::Canonical(error),
            ClosureError::Store(error) => error.into(),
        }
    }
}

impl From<RetentionRadixErrorV2<EmptyStateStagingErrorV1>> for DurableStoreGenesisErrorV1 {
    fn from(error: RetentionRadixErrorV2<EmptyStateStagingErrorV1>) -> Self {
        match error {
            RetentionRadixErrorV2::Store(error) => error.into(),
            RetentionRadixErrorV2::Canonical(error) => Self::Canonical(error),
            RetentionRadixErrorV2::Refusal(refusal) => Self::RetentionRadix(refusal),
        }
    }
}

impl From<GenerationCheckpointErrorV1> for DurableStoreGenesisErrorV1 {
    fn from(error: GenerationCheckpointErrorV1) -> Self { Self::Checkpoint(error) }
}

impl From<GenerationCheckpointPrepareWriteErrorV1<CanonicalFormatError, Infallible>>
    for DurableStoreGenesisErrorV1
{
    fn from(
        error: GenerationCheckpointPrepareWriteErrorV1<CanonicalFormatError, Infallible>,
    ) -> Self {
        match error {
            GenerationCheckpointPrepareWriteErrorV1::Source(error) => Self::Canonical(error),
            GenerationCheckpointPrepareWriteErrorV1::Sink(never) => match never {},
            GenerationCheckpointPrepareWriteErrorV1::Checkpoint(error) => Self::Checkpoint(error),
            GenerationCheckpointPrepareWriteErrorV1::Invariant(error) => {
                Self::LogPack(Box::new(error))
            }
        }
    }
}

impl From<PreparedGenerationCheckpointIndexVisitErrorV1<IndexFormatError>>
    for DurableStoreGenesisErrorV1
{
    fn from(error: PreparedGenerationCheckpointIndexVisitErrorV1<IndexFormatError>) -> Self {
        match error {
            PreparedGenerationCheckpointIndexVisitErrorV1::Checkpoint(error) => {
                Self::Checkpoint(error)
            }
            PreparedGenerationCheckpointIndexVisitErrorV1::Invariant(error) => {
                Self::LogPack(Box::new(error))
            }
            PreparedGenerationCheckpointIndexVisitErrorV1::Index(error) |
            PreparedGenerationCheckpointIndexVisitErrorV1::Visitor(error) => {
                Self::Index(Box::new(error))
            }
        }
    }
}

impl From<LogPackFormatError> for DurableStoreGenesisErrorV1 {
    fn from(error: LogPackFormatError) -> Self { Self::LogPack(Box::new(error)) }
}

impl From<IndexFormatError> for DurableStoreGenesisErrorV1 {
    fn from(error: IndexFormatError) -> Self { Self::Index(Box::new(error)) }
}

impl From<DurableFormatError> for DurableStoreGenesisErrorV1 {
    fn from(error: DurableFormatError) -> Self { Self::Catalog(Box::new(error)) }
}

impl From<DurableLimitError> for DurableStoreGenesisErrorV1 {
    fn from(error: DurableLimitError) -> Self { Self::Limit(error) }
}

pub(crate) struct EmptyStateCheckpointSourceV1 {
    descriptor: GenerationCheckpointDescriptorV1,
    ledger: StructuralClosureLedgerV1,
    objects: Box<[(ObjectRefV1, Arc<[u8]>)]>,
}

impl EmptyStateCheckpointSourceV1 {
    fn new<P: SemanticProfile>() -> Result<Self, DurableStoreGenesisErrorV1> {
        let base = CanonicalObjectStore::new();
        let staging = StagingObjectStore::new(&base).with_retention_reader(RetentionReader::Dual);
        let (empty_root, _) =
            staging.stage(CanonicalDagNode::empty().try_canonical_bytes()?.into())?;
        let roots = SemanticRoots {
            data_bag: empty_root,
            continuation_map: empty_root,
            join_bag: empty_root,
        };
        let manifest = SemanticStateManifestV1::new(SEMANTIC_SCHEMA_EPOCH_V1, P::ID, roots, None)?;
        let (manifest_ref, _) = staging.stage(manifest.try_canonical_bytes()?.into())?;
        let state = StateVersionV1::new(EMPTY_STORE_GENERATION_V1, manifest_ref);
        let mut ledger = StructuralClosureLedgerV1::new();
        let current = verify_structural_closure_in_ledger_with_credit(
            &staging,
            manifest_ref,
            StructuralValidationLimits::unbounded(),
            &mut ledger,
            RetainedUnionClosureLimitsV1::UNBOUNDED,
            ClosureCreditV1::CURRENT_AND_RETAINED,
        )?
        .report();
        let anchor = RetentionAnchorV2::new(
            RetainedStateV1::new(state, current.canonical_bytes)?,
            0,
            empty_retention_radix_ref(),
        )?;
        staging.stage(anchor.try_canonical_bytes()?.into())?;
        staging.stage(Arc::from(&EMPTY_RETENTION_RADIX_NODE[..]))?;
        let (_, metadata) = admit_retention_history_v2(&staging, anchor)?.into_parts();
        ledger.seed_retention_metadata(anchor.object_ref(), &metadata)?;
        let descriptor = GenerationCheckpointDescriptorV1::from_retained_union_and_metadata(
            P::ID,
            state,
            anchor.object_ref(),
            current.distinct_objects,
            current.canonical_bytes,
            &ledger,
        )?;
        let objects = staging.into_objects(|reference, bytes| (reference, bytes))?;
        Ok(Self {
            descriptor,
            ledger,
            objects,
        })
    }
}

impl GenerationCheckpointSourceV1 for EmptyStateCheckpointSourceV1 {
    type ReadError = CanonicalFormatError;

    fn checkpoint_descriptor(
        &self,
    ) -> Result<GenerationCheckpointDescriptorV1, GenerationCheckpointErrorV1> {
        Ok(self.descriptor)
    }

    fn try_visit_checkpoint_references<E>(
        &self,
        visitor: impl FnMut(ObjectRefV1) -> Result<(), E>,
    ) -> Result<(), CheckpointReferenceVisitErrorV1<E>> {
        try_visit_checkpoint_references_v2(&self.ledger, visitor)
    }

    fn copy_checkpoint_object<S: CanonicalSink>(
        &self,
        reference: ObjectRefV1,
        sink: &mut S,
        _scratch: &mut Vec<u8>,
    ) -> Result<(), GenerationCheckpointSourceCopyErrorV1<Self::ReadError, S::Error>> {
        match self
            .objects
            .binary_search_by_key(&reference.digest(), |(stored, _)| stored.digest())
        {
            Ok(position) => match &self.objects[position] {
                (stored, bytes) if *stored == reference => sink
                    .write_all(bytes)
                    .map_err(GenerationCheckpointSourceCopyErrorV1::Sink),
                (stored, _) => Err(GenerationCheckpointSourceCopyErrorV1::Source(
                    CanonicalFormatError::ObjectLengthMismatch {
                        expected: reference.encoded_len(),
                        actual: stored.encoded_len(),
                    },
                )),
            },
            Err(_) => Err(GenerationCheckpointSourceCopyErrorV1::Source(
                CanonicalFormatError::MissingObject(reference.digest()),
            )),
        }
    }
}

struct EmptyStoreImageV1 {
    pack_header: PackHeaderV1,
    prepare: Vec<u8>,
    prepare_address: LogPackAddress,
    index_generation: IndexGenerationV1,
    index: Vec<u8>,
    index_tail: u64,
    commit: GenerationCheckpointCommitRecordV1,
    commit_address: LogPackAddress,
    slot: CatalogSlotV1,
}

impl EmptyStoreImageV1 {
    fn new<P: SemanticProfile>(
        source: &EmptyStateCheckpointSourceV1,
        limits: DurableLimits,
    ) -> Result<Self, DurableStoreGenesisErrorV1> {
        let store_id = StoreId::random();
        let pack_header = PackHeaderV1::genesis(store_id);
        let prepared = PreparedGenerationCheckpointV1::<P, _>::new(
            source,
            store_id,
            pack_header.first_sequence(),
            LogPackDecodeLimits::UNBOUNDED,
        )?;
        let prepare_address = LogPackAddress::from_pack(
            pack_header.pack_id(),
            PACK_HEADER_LEN_V1,
            prepared.record_len(),
        )?;
        let mut prepare = reserved_image_buffer(prepared.record_len(), "empty-store PREPARE")?;
        let identity = prepared.write_to(&mut prepare)?;
        let index_header = ObjectIndexHeaderV1::new(store_id, IndexGenerationV1::FIRST);
        let index_plan =
            PreparedGenerationCheckpointIndexV1::new(index_header, prepare_address, &prepared)?;
        let mut index = reserved_image_buffer(index_plan.exact_tail(), "empty-store index")?;
        let mut stream = CanonicalIndexStreamV1::new(index_header);
        index.extend_from_slice(&stream.header_image());
        index_plan.try_visit_records(|record| {
            index.extend_from_slice(stream.next_record(record)?.bytes());
            Ok::<(), IndexFormatError>(())
        })?;
        stream.verify_appointed_tail(index_plan.exact_tail())?;
        let commit_address = LogPackAddress::from_pack(
            pack_header.pack_id(),
            prepare_address.end(),
            GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
        )?;
        preflight_recovery_limits(
            limits,
            commit_address.end(),
            index_plan.record_count(),
            index_plan.exact_tail(),
        )?;
        let publication = index_plan.publication();
        let commit = GenerationCheckpointCommitRecordV1::new(
            store_id,
            PhysicalDigest::ZERO,
            prepare_address,
            identity,
            publication,
        )?;
        let descriptor = prepared.descriptor();
        let pack = CatalogPackStateV1::new(
            commit_address.segment(),
            commit_address.segment(),
            commit_address.end(),
            publication.index_generation(),
            index_plan.exact_tail(),
            descriptor.retention_manifest(),
            descriptor.current_state_canonical_bytes().get(),
            descriptor
                .inventory()
                .retained_union_canonical_bytes()
                .get(),
        )?;
        let slot = CatalogSlotV1::new(
            store_id,
            EMPTY_STORE_CATALOG_EPOCH_V1,
            descriptor.state(),
            P::ID,
            commit.digest(),
            prepared.sequence(),
            commit_address,
            pack,
        )?;
        Ok(Self {
            pack_header,
            prepare,
            prepare_address,
            index_generation: index_header.generation(),
            index,
            index_tail: index_plan.exact_tail(),
            commit,
            commit_address,
            slot,
        })
    }
}

fn reserved_image_buffer(len: u64, field: &'static str) -> Result<Vec<u8>, CanonicalFormatError> {
    let capacity = usize::try_from(len)
        .map_err(|_| CanonicalFormatError::LengthOutOfRange { field, value: len })?;
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(capacity)
        .map_err(|_| CanonicalFormatError::AllocationFailure { requested: len })?;
    Ok(buffer)
}

fn write_empty_store_image(
    lease: &mut DurableWriterLeaseV1,
    image: &EmptyStoreImageV1,
) -> Result<(), DurableStoreGenesisErrorV1> {
    {
        #[cfg(test)]
        reach_durable_store_genesis_step(
            DurableStoreGenesisStepV1::CreatePack,
            lease.directory().path(),
        )?;
        let mut pack = lease.create_pack(image.pack_header.pack_id())?;
        #[cfg(test)]
        reach_durable_store_genesis_step(DurableStoreGenesisStepV1::WritePackHeader, pack.path())?;
        pack.write_all_at(0, &image.pack_header.encode())?;
        #[cfg(test)]
        reach_durable_store_genesis_step(DurableStoreGenesisStepV1::WritePrepare, pack.path())?;
        pack.write_all_at(image.prepare_address.offset(), &image.prepare)?;
        pack.require_length(FileLengthExpectation::Exact(image.prepare_address.end()))?;
        #[cfg(test)]
        reach_durable_store_genesis_step(DurableStoreGenesisStepV1::SyncPrepare, pack.path())?;
        pack.sync_all()?;
    }
    {
        #[cfg(test)]
        reach_durable_store_genesis_step(
            DurableStoreGenesisStepV1::CreateIndex,
            lease.directory().path(),
        )?;
        let mut index = lease.create_index(image.index_generation)?;
        #[cfg(test)]
        reach_durable_store_genesis_step(DurableStoreGenesisStepV1::WriteIndex, index.path())?;
        index.write_all_at(0, &image.index)?;
        index.require_length(FileLengthExpectation::Exact(image.index_tail))?;
        #[cfg(test)]
        reach_durable_store_genesis_step(DurableStoreGenesisStepV1::SyncIndex, index.path())?;
        index.sync_all()?;
    }
    {
        let mut pack = lease.open_pack(image.commit_address.pack_id())?;
        pack.require_length(FileLengthExpectation::Exact(image.commit_address.offset()))?;
        #[cfg(test)]
        reach_durable_store_genesis_step(DurableStoreGenesisStepV1::WriteCommit, pack.path())?;
        pack.write_all_at(image.commit_address.offset(), image.commit.canonical_bytes())?;
        pack.require_length(FileLengthExpectation::Exact(image.commit_address.end()))?;
        #[cfg(test)]
        reach_durable_store_genesis_step(DurableStoreGenesisStepV1::SyncCommit, pack.path())?;
        pack.sync_all()?;
    }
    #[cfg(test)]
    reach_durable_store_genesis_step(
        DurableStoreGenesisStepV1::SyncArtifactDirectory,
        lease.directory().path(),
    )?;
    lease.sync_directory()?;
    {
        #[cfg(test)]
        reach_durable_store_genesis_step(
            DurableStoreGenesisStepV1::CreateCatalog,
            lease.directory().path(),
        )?;
        let mut catalog = lease.create_catalog()?;
        #[cfg(test)]
        reach_durable_store_genesis_step(
            DurableStoreGenesisStepV1::WriteCatalogSlot,
            catalog.path(),
        )
        .map_err(DurableStoreGenesisErrorV1::CatalogIndeterminate)?;
        catalog
            .write_catalog_slot(CatalogSlotIndexV1::First, &image.slot.encode())
            .map_err(DurableStoreGenesisErrorV1::CatalogIndeterminate)?;
        #[cfg(test)]
        reach_durable_store_genesis_step(DurableStoreGenesisStepV1::SyncCatalog, catalog.path())
            .map_err(DurableStoreGenesisErrorV1::CatalogIndeterminate)?;
        catalog
            .sync_catalog_all()
            .map_err(DurableStoreGenesisErrorV1::CatalogIndeterminate)?;
    }
    #[cfg(test)]
    reach_durable_store_genesis_step(
        DurableStoreGenesisStepV1::SyncCatalogDirectory,
        lease.directory().path(),
    )
    .map_err(DurableStoreGenesisErrorV1::CatalogIndeterminate)?;
    lease
        .sync_catalog_directory()
        .map_err(DurableStoreGenesisErrorV1::CatalogIndeterminate)
}

pub(crate) fn create_empty_store_v1<P: SemanticProfile>(
    directory: impl Into<PathBuf>,
    limits: DurableLimits,
) -> Result<CatalogSlotV1, DurableStoreGenesisErrorV1> {
    let source = EmptyStateCheckpointSourceV1::new::<P>()?;
    let image = EmptyStoreImageV1::new::<P>(&source, limits)?;
    let mut lease = DurableDirectoryV1::open(directory)?.try_acquire_writer()?;
    let files = lease.directory().store_file_names()?;
    if !files.is_empty() {
        return Err(DurableStoreGenesisErrorV1::DirectoryHoldsStoreFiles {
            directory: lease.directory().path().to_path_buf(),
            files: files.into_boxed_slice(),
        });
    }
    write_empty_store_image(&mut lease, &image)?;
    Ok(image.slot)
}
