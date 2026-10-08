//! Typed durable startup and mandatory physical-index binding.
//!
//! No recovered state leaves this module until the exact catalog-authorized
//! WAL chain has been scanned and its rebuildable physical index has either
//! been verified byte-for-byte or atomically rebuilt, synchronized, reopened,
//! and verified again.

use std::convert::Infallible;
use std::fmt::{Display, Formatter};

use super::index::{CanonicalIndexStreamV1, ObjectIndexPrefixVerifierV1};
use super::io::{DurableDirectoryV1, DurableFileV1, DurableIoError, DurableWriterLeaseV1};
use super::reclaim::ReclamationCatalogRelationV1;
use super::recovery::{
    CatalogPackProviderV1, CatalogVerifiedRecoveryV1, RecoveryError, ScannedPackChainV1,
    scan_catalog_authorized_pack_chain, stage_catalog_verified_data_in_source_order,
};
use super::{
    CommittedObjectIndexV1, DurableFormatError, DurableLimitConfig, DurableLimitError,
    DurableLimitKind, DurableLimits, INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexFormatError,
    IndexGenerationV1, ObjectIndexHeaderV1, ObjectIndexRecordV1,
};
use crate::rspace::persistence::wal::TerminalCommitViewV1;
use crate::rspace::persistence::{
    CatalogAuthorityTokenV1, LogPackAddress, LogPackDecodeLimits, PackId, PackTerminalIdentityV1,
    SemanticProfileId, StateVersionV1, StoreId,
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

    pub(crate) const fn profile_id(&self) -> SemanticProfileId { self.recovery.profile_id() }

    pub(crate) const fn state(&self) -> StateVersionV1 { self.recovery.state() }

    pub(crate) const fn terminal(&self) -> PackTerminalIdentityV1 { self.recovery.terminal() }

    pub(crate) const fn terminal_address(&self) -> LogPackAddress {
        self.recovery.terminal_address()
    }

    pub(crate) const fn expected_index_tail(&self) -> u64 { self.recovery.expected_index_tail() }

    pub(crate) const fn transactions(&self) -> u64 { self.recovery.transactions() }

    pub(crate) const fn checkpoints(&self) -> u64 { self.recovery.checkpoints() }

    pub(crate) const fn object_records(&self) -> u64 { self.recovery.object_records() }

    pub(crate) const fn scanned_bytes(&self) -> u64 { self.recovery.scanned_bytes() }

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
    let candidate = lease.read_reclamation_candidate()?;
    let reader = lease.directory().try_clone_reader()?;
    let recovered = recover_index_bound_v1(&reader, lease, limits, wal_limits)?;
    if let Some(candidate) = candidate {
        let selected = recovered.catalog_authority().slot();
        match candidate.relation_to_catalog(selected) {
            ReclamationCatalogRelationV1::PreCas => {
                lease.remove_reclamation_candidate()?;
            }
            ReclamationCatalogRelationV1::SelectedSuccessor => {
                lease.reclaim_reclamation_candidate(candidate, selected)?;
            }
            ReclamationCatalogRelationV1::Unrelated => {
                return Err(DurableStartupErrorV1::Io(DurableIoError::CatalogAuthorityChanged {
                    path: lease.directory().path().to_path_buf(),
                }));
            }
        }
    }
    Ok(recovered)
}

struct DirectoryPackProviderV1<'directory>(&'directory DurableDirectoryV1);

impl CatalogPackProviderV1 for DirectoryPackProviderV1<'_> {
    type Reader = DurableFileV1;
    type Error = DurableIoError;

    fn open_pack(&mut self, pack_id: PackId) -> Result<Self::Reader, Self::Error> {
        self.0.open_pack(pack_id)
    }
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
    drop(second);
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
