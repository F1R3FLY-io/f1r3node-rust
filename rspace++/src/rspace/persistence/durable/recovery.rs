//! Iterative recovery of catalog-authorized pack prefixes.
//!
//! The scanner never searches for a later valid record after an error. Every
//! pack is consumed as one exact header followed by adjacent prepare/commit
//! pairs, and every variable read is preceded by authenticated fixed-prefix
//! preflight.

use std::fmt::{Display, Formatter};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

use super::{
    CommittedObjectIndexV1, DurableFormatError, DurableLimitError, DurableLimitKind, DurableLimits,
    INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexFormatError, ObjectIndexRecordV1,
    PACK_HEADER_LEN_V1, PackFormatError, PackHeaderV1, PackTerminalIdentityV1,
};
use crate::rspace::persistence::canonical::{OBJECT_DOMAIN_LEN, ReadCursor};
use crate::rspace::persistence::checkpoint::OrderedObjectInventoryBuilderV1;
use crate::rspace::persistence::checksum::{CHECKSUM_FIELD_LEN, Crc32c, decode_crc32c};
use crate::rspace::persistence::physical::PHYSICAL_DIGEST_LEN;
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1,
    GenerationCheckpointCommitViewV1, GenerationCheckpointPrepareIdentityV1,
    GenerationCheckpointPreparePreflightV1, TerminalCommitViewV1,
    preflight_generation_checkpoint_prepare_v1,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, COMMIT_DISCRIMINATOR_LEN_V1, CanonicalObjectKind, CatalogAuthorityTokenV1,
    CommitRecordKindV1, LogPackAddress, LogPackDecodeLimits, LogPackFormatError,
    LogPackObjectDispositionV1, LogPackSequence, ObjectDigest, ObjectRefV1,
    PREPARE_DISCRIMINATOR_LEN_V1, PackId, PhysicalDigest, PrepareRecordKindV1, SemanticProfileId,
    StateVersionV1, StoreId, TRANSACTION_COMMIT_RECORD_LEN_V1, TRANSACTION_PREPARE_HEADER_LEN_V1,
    TransactionCommitViewV1, TransactionPrepareRecordViewV1, decode_commit_kind_v1,
    decode_prepare_kind_v1, preflight_prepare_record_v1,
};

const CHECKPOINT_OBJECT_REFERENCE_LEN: u64 = 40;
const CHECKPOINT_STREAM_SCRATCH_LEN: usize = 64 * 1024;

/// Minimal positional-read contract used by both production files and exact
/// in-memory crash images. Implementations must fill the complete destination
/// or return an error; short reads are never successful.
pub(crate) trait DurableReadAt {
    type Error;

    fn durable_len(&self) -> Result<u64, Self::Error>;

    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> Result<(), Self::Error>;
}

/// Opens one catalog-named pack as an owned reader.
///
/// The scanner requests packs sequentially and drops each reader before it
/// requests the successor, so recovery holds O(1) descriptors independent of
/// the number of live packs. Implementations must not enumerate a directory:
/// the selected catalog is the sole source of pack identifiers.
pub(crate) trait CatalogPackProviderV1 {
    type Reader: DurableReadAt<Error = Self::Error>;
    type Error;

    fn open_pack(&mut self, pack_id: PackId) -> Result<Self::Reader, Self::Error>;
}

pub(crate) struct ScannedPackChainV1<I = CommittedObjectIndexV1> {
    catalog_image: [u8; CATALOG_IMAGE_LEN],
    store_id: StoreId,
    profile_id: SemanticProfileId,
    state: StateVersionV1,
    terminal: PackTerminalIdentityV1,
    terminal_address: LogPackAddress,
    terminal_commit: TerminalCommitViewV1,
    index: I,
    expected_index_tail: u64,
    transactions: u64,
    checkpoints: u64,
    object_records: u64,
    scanned_bytes: u64,
}

/// Catalog-authoritative recovery result. This type can only be constructed by
/// consuming a scan together with the exact token that authorized its reads.
pub(crate) struct CatalogVerifiedRecoveryV1<I = CommittedObjectIndexV1> {
    scan: ScannedPackChainV1<I>,
    authority: CatalogAuthorityTokenV1,
}

impl<I> ScannedPackChainV1<I> {
    pub(crate) fn verify_catalog(
        self,
        authority: CatalogAuthorityTokenV1,
    ) -> Result<CatalogVerifiedRecoveryV1<I>, RecoveryError<std::convert::Infallible>> {
        if self.catalog_image != *authority.image() {
            return Err(RecoveryError::CatalogAuthorityChanged);
        }
        let catalog = authority.slot();
        let commit = self.terminal_commit;
        catalog
            .verify_terminal_commit(commit)
            .map_err(RecoveryError::Catalog)?;
        if self.terminal_address != catalog.commit_address() {
            return Err(RecoveryError::CatalogTerminal {
                expected_sequence: catalog.commit_sequence(),
                actual_sequence: self.terminal.last_sequence(),
                expected_digest: catalog.commit_digest(),
                actual_digest: self.terminal.commit_digest(),
                expected_address: catalog.commit_address(),
                actual_address: self.terminal_address,
            });
        }
        Ok(CatalogVerifiedRecoveryV1 {
            scan: self,
            authority,
        })
    }
}

impl<I> CatalogVerifiedRecoveryV1<I> {
    pub(crate) const fn store_id(&self) -> StoreId { self.scan.store_id }

    pub(crate) const fn profile_id(&self) -> SemanticProfileId { self.scan.profile_id }

    pub(crate) const fn state(&self) -> StateVersionV1 { self.scan.state }

    pub(crate) const fn terminal(&self) -> PackTerminalIdentityV1 { self.scan.terminal }

    pub(crate) const fn terminal_address(&self) -> LogPackAddress { self.scan.terminal_address }

    pub(crate) const fn expected_index_tail(&self) -> u64 { self.scan.expected_index_tail }

    pub(crate) const fn transactions(&self) -> u64 { self.scan.transactions }

    pub(crate) const fn checkpoints(&self) -> u64 { self.scan.checkpoints }

    pub(crate) const fn object_records(&self) -> u64 { self.scan.object_records }

    pub(crate) const fn scanned_bytes(&self) -> u64 { self.scan.scanned_bytes }

    pub(crate) const fn authority(&self) -> &CatalogAuthorityTokenV1 { &self.authority }
}

impl CatalogVerifiedRecoveryV1<CommittedObjectIndexV1> {
    pub(crate) fn object_index(&self) -> &CommittedObjectIndexV1 { &self.scan.index }

    pub(super) fn into_authority_and_index(
        self,
    ) -> (CatalogAuthorityTokenV1, CommittedObjectIndexV1) {
        (self.authority, self.scan.index)
    }
}

#[derive(Debug)]
pub(crate) enum RecoveryError<E> {
    Read(E),
    Limit(DurableLimitError),
    Pack(PackFormatError),
    Wal(LogPackFormatError),
    Index(IndexFormatError),
    Catalog(DurableFormatError),
    CatalogAuthorityChanged,
    EmptyPackSet,
    PackHasNoTransactions(PackId),
    DurableTail {
        pack: PackId,
        tail: u64,
        available: u64,
    },
    AuthorizedTailCutsRecord {
        pack: PackId,
        record_offset: u64,
        required_end: u64,
        durable_tail: u64,
    },
    ParsedTailMismatch {
        pack: PackId,
        parsed_end: u64,
        durable_tail: u64,
    },
    Arithmetic(&'static str),
    Sequence {
        expected: LogPackSequence,
        actual: LogPackSequence,
    },
    Store {
        expected: StoreId,
        actual: StoreId,
    },
    Profile {
        expected: SemanticProfileId,
        actual: SemanticProfileId,
    },
    StateContinuity {
        expected: StateVersionV1,
        actual: StateVersionV1,
    },
    PreviousCommit {
        expected: PhysicalDigest,
        actual: PhysicalDigest,
    },
    PrepareAddress {
        expected: LogPackAddress,
        actual: LogPackAddress,
    },
    MissingReuse(crate::rspace::persistence::ObjectRefV1),
    GenerationCheckpointReuse(crate::rspace::persistence::ObjectRefV1),
    GenerationCheckpointPosition {
        expected_pack: PackId,
        expected_offset: u64,
        actual_pack: PackId,
        actual_offset: u64,
    },
    CheckpointObjectExtent {
        reference: ObjectRefV1,
        body_offset: u64,
        body_end: u64,
        trailer_offset: u64,
    },
    CheckpointPrepareTail {
        expected: u64,
        actual: u64,
    },
    CheckpointScratchAllocation {
        bytes: u64,
    },
    CommitPairing {
        prepare: PrepareRecordKindV1,
        commit: CommitRecordKindV1,
    },
    ReuseReference {
        indexed: crate::rspace::persistence::ObjectRefV1,
        reused: crate::rspace::persistence::ObjectRefV1,
    },
    ReuseNotPrior {
        reference: crate::rspace::persistence::ObjectRefV1,
        indexed_sequence: LogPackSequence,
        current_sequence: LogPackSequence,
    },
    ReplayDataRecord {
        expected: Option<ObjectIndexRecordV1>,
        actual: ObjectIndexRecordV1,
    },
    ReplayIndexRecordCount {
        expected: u64,
        actual: u64,
    },
    PublicationFirstLive {
        expected: u64,
        actual: u64,
    },
    PublicationIndexGeneration {
        expected: u64,
        actual: u64,
    },
    IndexTail {
        expected: u64,
        actual: u64,
    },
    CatalogTerminal {
        expected_sequence: LogPackSequence,
        actual_sequence: LogPackSequence,
        expected_digest: PhysicalDigest,
        actual_digest: PhysicalDigest,
        expected_address: LogPackAddress,
        actual_address: LogPackAddress,
    },
}

impl<E> From<DurableLimitError> for RecoveryError<E> {
    fn from(error: DurableLimitError) -> Self { Self::Limit(error) }
}

impl<E> From<PackFormatError> for RecoveryError<E> {
    fn from(error: PackFormatError) -> Self { Self::Pack(error) }
}

impl<E> From<LogPackFormatError> for RecoveryError<E> {
    fn from(error: LogPackFormatError) -> Self { Self::Wal(error) }
}

impl<E> From<IndexFormatError> for RecoveryError<E> {
    fn from(error: IndexFormatError) -> Self { Self::Index(error) }
}

impl<E> From<DurableFormatError> for RecoveryError<E> {
    fn from(error: DurableFormatError) -> Self { Self::Catalog(error) }
}

impl<E: Display> Display for RecoveryError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(error) => write!(formatter, "durable positional read failed: {error}"),
            Self::Limit(error) => Display::fmt(error, formatter),
            Self::Pack(error) => Display::fmt(error, formatter),
            Self::Wal(error) => Display::fmt(error, formatter),
            Self::Index(error) => Display::fmt(error, formatter),
            Self::Catalog(error) => Display::fmt(error, formatter),
            Self::CatalogAuthorityChanged => formatter.write_str(
                "catalog authority token changed between scan admission and terminal verification",
            ),
            Self::EmptyPackSet => formatter.write_str("catalog names no live pack source"),
            Self::PackHasNoTransactions(pack) => {
                write!(formatter, "live pack {} contains no committed transaction", pack.get())
            }
            Self::DurableTail {
                pack,
                tail,
                available,
            } => write!(
                formatter,
                "pack {} durable tail {tail} is invalid for {available} available bytes",
                pack.get()
            ),
            Self::AuthorizedTailCutsRecord {
                pack,
                record_offset,
                required_end,
                durable_tail,
            } => write!(
                formatter,
                "pack {} catalog-authorized tail {durable_tail} cuts the record at \
                 {record_offset}; the complete record ends at {required_end}",
                pack.get()
            ),
            Self::ParsedTailMismatch {
                pack,
                parsed_end,
                durable_tail,
            } => write!(
                formatter,
                "pack {} parser stopped at {parsed_end}, not at catalog-authorized tail \
                 {durable_tail}",
                pack.get()
            ),
            Self::Arithmetic(field) => write!(formatter, "recovery {field} arithmetic overflow"),
            Self::Sequence { expected, actual } => write!(
                formatter,
                "expected transaction sequence {}, got {}",
                expected.get(),
                actual.get()
            ),
            Self::Store { expected, actual } => {
                write!(formatter, "expected recovery store {expected}, got {actual}")
            }
            Self::Profile { expected, actual } => {
                write!(formatter, "expected recovery semantic profile {expected:?}, got {actual:?}")
            }
            Self::StateContinuity { expected, actual } => write!(
                formatter,
                "transaction base {actual:?} does not continue recovered state {expected:?}"
            ),
            Self::PreviousCommit { expected, actual } => write!(
                formatter,
                "transaction previous commit {actual} does not equal recovered terminal {expected}"
            ),
            Self::PrepareAddress { expected, actual } => write!(
                formatter,
                "commit prepare address {actual:?} does not equal adjacent record {expected:?}"
            ),
            Self::MissingReuse(reference) => write!(
                formatter,
                "REUSE object {} has no earlier committed DATA location",
                reference.digest()
            ),
            Self::GenerationCheckpointReuse(reference) => write!(
                formatter,
                "generation checkpoint object {} is REUSE, but a fresh generation must carry its \
                 complete retained closure as DATA",
                reference.digest()
            ),
            Self::GenerationCheckpointPosition {
                expected_pack,
                expected_offset,
                actual_pack,
                actual_offset,
            } => write!(
                formatter,
                "generation checkpoint at pack {} offset {actual_offset} does not equal required \
                 first-live position pack {} offset {expected_offset}",
                actual_pack.get(),
                expected_pack.get(),
            ),
            Self::CheckpointObjectExtent {
                reference,
                body_offset,
                body_end,
                trailer_offset,
            } => write!(
                formatter,
                "checkpoint object {} body [{body_offset}, {body_end}) crosses its record trailer \
                 at {trailer_offset}",
                reference.digest(),
            ),
            Self::CheckpointPrepareTail { expected, actual } => write!(
                formatter,
                "checkpoint object stream ended at {actual}, not at authenticated trailer offset \
                 {expected}"
            ),
            Self::CheckpointScratchAllocation { bytes } => write!(
                formatter,
                "cannot allocate the reusable {bytes}-byte checkpoint recovery scratch buffer"
            ),
            Self::CommitPairing { prepare, commit } => {
                write!(formatter, "WAL {prepare:?} PREPARE is paired with {commit:?} COMMIT")
            }
            Self::ReuseReference { indexed, reused } => write!(
                formatter,
                "REUSE reference {reused:?} disagrees with indexed DATA {indexed:?}"
            ),
            Self::ReuseNotPrior {
                reference,
                indexed_sequence,
                current_sequence,
            } => write!(
                formatter,
                "REUSE object {} resolves to sequence {}, which is not earlier than current \
                 sequence {}",
                reference.digest(),
                indexed_sequence.get(),
                current_sequence.get()
            ),
            Self::ReplayDataRecord { expected, actual } => write!(
                formatter,
                "replayed DATA index record {actual:?} does not equal first-pass authority \
                 {expected:?}"
            ),
            Self::ReplayIndexRecordCount { expected, actual } => write!(
                formatter,
                "replayed DATA record count {actual} does not equal authoritative index count \
                 {expected}"
            ),
            Self::PublicationFirstLive { expected, actual } => write!(
                formatter,
                "commit first-live pack {actual} does not equal catalog generation {expected}"
            ),
            Self::PublicationIndexGeneration { expected, actual } => write!(
                formatter,
                "commit index generation {actual} does not equal catalog generation {expected}"
            ),
            Self::IndexTail { expected, actual } => {
                write!(formatter, "index tail {actual} does not equal expected {expected}")
            }
            Self::CatalogTerminal { .. } => formatter
                .write_str("catalog terminal identity/address does not equal recovered tip"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RecoveryError<E> {}

trait RecoveryIndexSinkV1<E> {
    type FinalIndex;

    fn data(&mut self, record: ObjectIndexRecordV1) -> Result<(), RecoveryError<E>>;

    fn reuse(
        &mut self,
        reference: crate::rspace::persistence::ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryError<E>>;

    fn finish(self) -> Result<Self::FinalIndex, RecoveryError<E>>;
}

fn verify_prior_reuse<E>(
    indexed: Option<ObjectIndexRecordV1>,
    reference: crate::rspace::persistence::ObjectRefV1,
    current_sequence: LogPackSequence,
) -> Result<(), RecoveryError<E>> {
    let Some(indexed) = indexed else {
        return Err(RecoveryError::MissingReuse(reference));
    };
    if indexed.reference() != reference {
        return Err(RecoveryError::ReuseReference {
            indexed: indexed.reference(),
            reused: reference,
        });
    }
    let indexed_sequence = indexed.location().sequence();
    if indexed_sequence >= current_sequence {
        return Err(RecoveryError::ReuseNotPrior {
            reference,
            indexed_sequence,
            current_sequence,
        });
    }
    Ok(())
}

struct BuildingIndexSinkV1<F> {
    index: CommittedObjectIndexV1,
    observe_data: F,
}

impl<E, F> RecoveryIndexSinkV1<E> for BuildingIndexSinkV1<F>
where F: FnMut(ObjectIndexRecordV1) -> Result<(), RecoveryError<E>>
{
    type FinalIndex = CommittedObjectIndexV1;

    fn data(&mut self, record: ObjectIndexRecordV1) -> Result<(), RecoveryError<E>> {
        self.index.insert(record)?;
        (self.observe_data)(record)
    }

    fn reuse(
        &mut self,
        reference: crate::rspace::persistence::ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryError<E>> {
        verify_prior_reuse(self.index.get(reference.digest()), reference, current_sequence)
    }

    fn finish(self) -> Result<Self::FinalIndex, RecoveryError<E>> { Ok(self.index) }
}

struct ReplayingIndexSinkV1<'authority, F> {
    authority: &'authority CommittedObjectIndexV1,
    data_records: u64,
    observe_data: F,
}

impl<E, F> RecoveryIndexSinkV1<E> for ReplayingIndexSinkV1<'_, F>
where F: FnMut(ObjectIndexRecordV1) -> Result<(), RecoveryError<E>>
{
    type FinalIndex = ();

    fn data(&mut self, record: ObjectIndexRecordV1) -> Result<(), RecoveryError<E>> {
        let expected = self.authority.get(record.reference().digest());
        if expected != Some(record) {
            return Err(RecoveryError::ReplayDataRecord {
                expected,
                actual: record,
            });
        }
        self.data_records = self
            .data_records
            .checked_add(1)
            .ok_or(RecoveryError::Arithmetic("replayed DATA record count"))?;
        (self.observe_data)(record)
    }

    fn reuse(
        &mut self,
        reference: crate::rspace::persistence::ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryError<E>> {
        verify_prior_reuse(self.authority.get(reference.digest()), reference, current_sequence)
    }

    fn finish(self) -> Result<Self::FinalIndex, RecoveryError<E>> {
        let expected = self.authority.record_count();
        if self.data_records != expected {
            return Err(RecoveryError::ReplayIndexRecordCount {
                expected,
                actual: self.data_records,
            });
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn stream_generation_checkpoint_prepare<R, S>(
    reader: &R,
    pack_id: PackId,
    prepare_offset: u64,
    prefix: &[u8; GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize],
    preflight: GenerationCheckpointPreparePreflightV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    object_records: &mut u64,
    index_records: &mut u64,
    expected_index_tail: &mut u64,
    index_sink: &mut S,
    scratch: &mut [u8],
) -> Result<GenerationCheckpointPrepareIdentityV1, RecoveryError<R::Error>>
where
    R: DurableReadAt,
    S: RecoveryIndexSinkV1<R::Error>,
{
    debug_assert!(!scratch.is_empty());
    let descriptor = preflight.descriptor();
    *object_records = limits.checked_charge(
        DurableLimitKind::ObjectRecords,
        *object_records,
        descriptor.data_objects(),
    )?;
    let prepare_end = prepare_offset
        .checked_add(preflight.record_len())
        .ok_or(RecoveryError::Arithmetic("checkpoint prepare end"))?;
    let trailer_offset = prepare_end
        .checked_sub(CHECKSUM_FIELD_LEN as u64)
        .ok_or(RecoveryError::Arithmetic("checkpoint trailer offset"))?;

    let mut record_crc = Crc32c::new();
    let mut record_hasher = Blake2b::<U32>::new();
    record_crc.update(prefix);
    record_hasher.update(prefix);
    let mut inventory = OrderedObjectInventoryBuilderV1::new(
        descriptor.data_objects(),
        descriptor.data_canonical_bytes().get(),
    );
    let state_manifest = descriptor.state().manifest();
    let retention_manifest = descriptor.retention_manifest();
    let mut saw_state_manifest = false;
    let mut saw_retention_manifest = false;
    let mut cursor = prepare_offset
        .checked_add(GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1)
        .ok_or(RecoveryError::Arithmetic("checkpoint object stream offset"))?;

    for _ in 0..descriptor.data_objects() {
        let reference_end = cursor
            .checked_add(CHECKPOINT_OBJECT_REFERENCE_LEN)
            .ok_or(RecoveryError::Arithmetic("checkpoint object-reference end"))?;
        if reference_end > trailer_offset {
            return Err(RecoveryError::CheckpointPrepareTail {
                expected: trailer_offset,
                actual: reference_end,
            });
        }
        let mut reference_bytes = [0u8; CHECKPOINT_OBJECT_REFERENCE_LEN as usize];
        reader
            .read_exact_at(cursor, &mut reference_bytes)
            .map_err(RecoveryError::Read)?;
        record_crc.update(&reference_bytes);
        record_hasher.update(&reference_bytes);
        let mut reference_cursor = ReadCursor::new(&reference_bytes);
        let reference =
            ObjectRefV1::parse(&mut reference_cursor).map_err(LogPackFormatError::from)?;
        reference_cursor
            .finish()
            .map_err(LogPackFormatError::from)?;
        if reference.encoded_len() > wal_limits.max_single_object_bytes {
            return Err(RecoveryError::Wal(LogPackFormatError::ResourceLimit {
                resource: "single canonical object bytes",
                limit: wal_limits.max_single_object_bytes,
                attempted: reference.encoded_len(),
            }));
        }
        inventory
            .push(reference)
            .map_err(LogPackFormatError::from)?;
        let body_offset = reference_end;
        let body_end = body_offset
            .checked_add(reference.encoded_len())
            .ok_or(RecoveryError::Arithmetic("checkpoint object body end"))?;
        if body_end > trailer_offset {
            return Err(RecoveryError::CheckpointObjectExtent {
                reference,
                body_offset,
                body_end,
                trailer_offset,
            });
        }

        let mut object_hasher = Blake2b::<U32>::new();
        let mut domain = [0u8; OBJECT_DOMAIN_LEN];
        let mut body_cursor = body_offset;
        let mut body_consumed = 0u64;
        while body_cursor < body_end {
            let remaining = body_end - body_cursor;
            let take = usize::try_from(remaining.min(scratch.len() as u64))
                .expect("bounded checkpoint scratch length fits usize");
            let chunk = &mut scratch[..take];
            reader
                .read_exact_at(body_cursor, chunk)
                .map_err(RecoveryError::Read)?;
            if body_consumed < OBJECT_DOMAIN_LEN as u64 {
                let domain_start = body_consumed as usize;
                let copy_len = (OBJECT_DOMAIN_LEN - domain_start).min(take);
                domain[domain_start..domain_start + copy_len].copy_from_slice(&chunk[..copy_len]);
            }
            record_crc.update(chunk);
            record_hasher.update(&*chunk);
            object_hasher.update(&*chunk);
            body_cursor = body_cursor
                .checked_add(take as u64)
                .ok_or(RecoveryError::Arithmetic("checkpoint object stream cursor"))?;
            body_consumed = body_consumed
                .checked_add(take as u64)
                .ok_or(RecoveryError::Arithmetic("checkpoint object bytes read"))?;
        }
        let object_digest = finish_object_digest(object_hasher);
        if object_digest != reference.digest() {
            let actual = ObjectRefV1::new(object_digest, reference.encoded_len())
                .map_err(LogPackFormatError::from)?;
            return Err(RecoveryError::Wal(LogPackFormatError::ObjectReference {
                declared: reference,
                actual,
            }));
        }
        let captured_domain_len = reference.encoded_len().min(OBJECT_DOMAIN_LEN as u64) as usize;
        let kind = CanonicalObjectKind::from_domain_prefix(&domain[..captured_domain_len])
            .map_err(LogPackFormatError::from)?;
        if reference == state_manifest {
            require_checkpoint_kind(
                "state manifest",
                reference,
                kind,
                CanonicalObjectKind::SemanticManifest,
            )?;
            saw_state_manifest = true;
        }
        if reference == retention_manifest {
            require_checkpoint_kind(
                "retention manifest",
                reference,
                kind,
                CanonicalObjectKind::RetentionManifest,
            )?;
            saw_retention_manifest = true;
        }

        *index_records = index_records
            .checked_add(1)
            .ok_or(RecoveryError::Arithmetic("checkpoint index record count"))?;
        limits.require(DurableLimitKind::IndexRecords, *index_records)?;
        let relative_canonical_offset = body_offset
            .checked_sub(prepare_offset)
            .ok_or(RecoveryError::Arithmetic("checkpoint relative object offset"))?;
        let record = ObjectIndexRecordV1::from_checkpoint_reference(
            pack_id,
            prepare_offset,
            preflight.sequence(),
            reference,
            relative_canonical_offset,
        )?;
        *expected_index_tail = expected_index_tail
            .checked_add(INDEX_RECORD_LEN_V1)
            .ok_or(RecoveryError::Arithmetic("checkpoint index tail"))?;
        limits.require(DurableLimitKind::IndexBytes, *expected_index_tail)?;
        index_sink.data(record)?;
        cursor = body_end;
    }

    if cursor != trailer_offset {
        return Err(RecoveryError::CheckpointPrepareTail {
            expected: trailer_offset,
            actual: cursor,
        });
    }
    let mut trailer = [0u8; CHECKSUM_FIELD_LEN];
    reader
        .read_exact_at(trailer_offset, &mut trailer)
        .map_err(RecoveryError::Read)?;
    let declared_crc = decode_crc32c(trailer).map_err(|encoded| {
        RecoveryError::Wal(LogPackFormatError::ChecksumOutOfRange {
            field: "checkpoint prepare record",
            encoded,
        })
    })?;
    let actual_crc = record_crc.finish();
    if declared_crc != actual_crc {
        return Err(RecoveryError::Wal(LogPackFormatError::RecordChecksum {
            expected: declared_crc,
            actual: actual_crc,
        }));
    }
    if !saw_state_manifest {
        return Err(RecoveryError::Wal(LogPackFormatError::CheckpointDesignatedObjectMissing {
            role: "state manifest",
            reference: state_manifest,
        }));
    }
    if !saw_retention_manifest {
        return Err(RecoveryError::Wal(LogPackFormatError::CheckpointDesignatedObjectMissing {
            role: "retention manifest",
            reference: retention_manifest,
        }));
    }
    let observed = inventory.finish().map_err(LogPackFormatError::from)?;
    if observed.digest() != descriptor.ordered_digest() {
        return Err(RecoveryError::Wal(LogPackFormatError::CheckpointInventoryDigest {
            expected: *descriptor.ordered_digest().as_bytes(),
            actual: *observed.digest().as_bytes(),
        }));
    }
    Ok(GenerationCheckpointPrepareIdentityV1::from_streamed(
        preflight,
        finish_physical_digest(record_hasher),
        declared_crc,
    ))
}

fn require_checkpoint_kind<E>(
    role: &'static str,
    reference: ObjectRefV1,
    actual: CanonicalObjectKind,
    expected: CanonicalObjectKind,
) -> Result<(), RecoveryError<E>> {
    if actual != expected {
        return Err(RecoveryError::Wal(LogPackFormatError::CheckpointDesignatedObjectKind {
            role,
            reference,
            expected,
            actual,
        }));
    }
    Ok(())
}

fn finish_object_digest(hasher: Blake2b<U32>) -> ObjectDigest {
    let hash = hasher.finalize();
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&hash);
    ObjectDigest::from_array(digest)
}

fn finish_physical_digest(hasher: Blake2b<U32>) -> PhysicalDigest {
    let hash = hasher.finalize();
    let mut digest = [0u8; PHYSICAL_DIGEST_LEN];
    digest.copy_from_slice(&hash);
    PhysicalDigest::from_array(digest)
}

fn require_commit_pair<R: DurableReadAt>(
    reader: &R,
    pack_id: PackId,
    commit_offset: u64,
    durable_tail: u64,
    prepare: PrepareRecordKindV1,
    expected: CommitRecordKindV1,
) -> Result<[u8; COMMIT_DISCRIMINATOR_LEN_V1 as usize], RecoveryError<R::Error>> {
    let discriminator_end = commit_offset
        .checked_add(COMMIT_DISCRIMINATOR_LEN_V1)
        .ok_or(RecoveryError::Arithmetic("commit discriminator end"))?;
    if discriminator_end > durable_tail {
        return Err(RecoveryError::AuthorizedTailCutsRecord {
            pack: pack_id,
            record_offset: commit_offset,
            required_end: discriminator_end,
            durable_tail,
        });
    }
    let mut discriminator = [0u8; COMMIT_DISCRIMINATOR_LEN_V1 as usize];
    reader
        .read_exact_at(commit_offset, &mut discriminator)
        .map_err(RecoveryError::Read)?;
    let actual = decode_commit_kind_v1(&discriminator)?;
    if actual != expected {
        return Err(RecoveryError::CommitPairing {
            prepare,
            commit: actual,
        });
    }
    Ok(discriminator)
}

/// Scan the exact pack range named by a selected catalog authority.
///
/// Range cardinality is admitted before the provider is called. Packs are
/// then opened one at a time, and each complete authorized pack extent is
/// charged before its first byte is read. An authenticated prepare prefix
/// similarly charges its declared object count before the record allocation
/// and variable body read. The returned value is deliberately non-authority;
/// callers must consume it with [`ScannedPackChainV1::verify_catalog`].
pub(crate) fn scan_catalog_authorized_pack_chain<P, F>(
    provider: &mut P,
    authority: &CatalogAuthorityTokenV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    observe_data: F,
) -> Result<ScannedPackChainV1, RecoveryError<P::Error>>
where
    P: CatalogPackProviderV1,
    F: FnMut(ObjectIndexRecordV1) -> Result<(), RecoveryError<P::Error>>,
{
    scan_catalog_authorized_pack_chain_with_sink(
        provider,
        authority,
        limits,
        wal_limits,
        BuildingIndexSinkV1 {
            index: CommittedObjectIndexV1::new(),
            observe_data,
        },
    )
}

/// Re-scan an exact catalog-authorized chain against the first pass's
/// recovered index without allocating another PathMap. DATA records must
/// reproduce the authoritative locations exactly and REUSE may only resolve
/// strictly backward to an earlier committed transaction.
pub(crate) fn replay_catalog_authorized_pack_chain<P, F>(
    provider: &mut P,
    authority: &CatalogAuthorityTokenV1,
    authoritative_index: &CommittedObjectIndexV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    observe_data: F,
) -> Result<ScannedPackChainV1<()>, RecoveryError<P::Error>>
where
    P: CatalogPackProviderV1,
    F: FnMut(ObjectIndexRecordV1) -> Result<(), RecoveryError<P::Error>>,
{
    scan_catalog_authorized_pack_chain_with_sink(
        provider,
        authority,
        limits,
        wal_limits,
        ReplayingIndexSinkV1 {
            authority: authoritative_index,
            data_records: 0,
            observe_data,
        },
    )
}

fn scan_catalog_authorized_pack_chain_with_sink<P, S>(
    provider: &mut P,
    authority: &CatalogAuthorityTokenV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    mut index_sink: S,
) -> Result<ScannedPackChainV1<S::FinalIndex>, RecoveryError<P::Error>>
where
    P: CatalogPackProviderV1,
    S: RecoveryIndexSinkV1<P::Error>,
{
    let catalog = authority.slot();
    let pack_state = catalog.pack();
    let first_live_pack = PackId::new(pack_state.first_live_pack())?;
    let active_pack = PackId::new(pack_state.active_pack())?;
    let live_pack_count = active_pack
        .get()
        .checked_sub(first_live_pack.get())
        .and_then(|distance| distance.checked_add(1))
        .ok_or(RecoveryError::Arithmetic("live-pack count"))?;
    // This is intentionally before the first provider call: a catalog cannot
    // force recovery to open an unbounded number of descriptors in sequence.
    limits.require(DurableLimitKind::LivePacks, live_pack_count)?;

    let expected_store = catalog.store_id();
    let expected_profile = catalog.profile_id();
    let index_generation = pack_state.index_generation();
    let mut transactions = 0u64;
    let mut checkpoints = 0u64;
    let mut object_records = 0u64;
    let mut index_records = 0u64;
    let mut scanned_bytes = 0u64;
    let mut expected_index_tail = INDEX_HEADER_LEN_V1;
    limits.require(DurableLimitKind::IndexBytes, expected_index_tail)?;
    let mut previous_terminal = None;
    let mut previous_state = None;
    let mut terminal_address = None;
    let mut terminal_commit = None;
    let mut checkpoint_scratch = None;
    let mut pack_raw = first_live_pack.get();

    loop {
        let pack_id = PackId::new(pack_raw)?;
        let reader = provider.open_pack(pack_id).map_err(RecoveryError::Read)?;
        let available = reader.durable_len().map_err(RecoveryError::Read)?;
        let durable_tail = if pack_id == active_pack {
            pack_state.durable_pack_tail()
        } else {
            // A non-active live pack is immutable and was sealed before the
            // catalog advanced. Consequently its physical length is its exact
            // committed extent; only the active pack may retain a crash suffix.
            available
        };
        limits.require(DurableLimitKind::PackBytes, durable_tail)?;
        scanned_bytes =
            limits.checked_charge(DurableLimitKind::TotalScanBytes, scanned_bytes, durable_tail)?;
        if durable_tail < PACK_HEADER_LEN_V1 || durable_tail > available {
            return Err(RecoveryError::DurableTail {
                pack: pack_id,
                tail: durable_tail,
                available,
            });
        }

        let mut header_bytes = [0u8; PACK_HEADER_LEN_V1 as usize];
        reader
            .read_exact_at(0, &mut header_bytes)
            .map_err(RecoveryError::Read)?;
        let header = PackHeaderV1::parse_exact(&header_bytes)?;
        header.verify_identity(expected_store, pack_id)?;
        if let Some(previous) = previous_terminal {
            header.verify_predecessor(previous)?;
        } else if pack_id == PackId::GENESIS {
            // The parser already proved the genesis sentinel/digest rules.
        } else if header.previous_pack_id().is_none() {
            return Err(RecoveryError::Pack(PackFormatError::GenesisHasNoPredecessor));
        }

        let mut expected_sequence = header.first_sequence();
        let mut last_sequence = None;
        let mut previous_commit = header.previous_terminal_commit();
        let mut offset = PACK_HEADER_LEN_V1;
        while offset < durable_tail {
            let discriminator_end = offset
                .checked_add(PREPARE_DISCRIMINATOR_LEN_V1)
                .ok_or(RecoveryError::Arithmetic("prepare discriminator end"))?;
            if discriminator_end > durable_tail {
                return Err(RecoveryError::AuthorizedTailCutsRecord {
                    pack: pack_id,
                    record_offset: offset,
                    required_end: discriminator_end,
                    durable_tail,
                });
            }
            let mut discriminator = [0u8; PREPARE_DISCRIMINATOR_LEN_V1 as usize];
            reader
                .read_exact_at(offset, &mut discriminator)
                .map_err(RecoveryError::Read)?;
            let prepare_kind = decode_prepare_kind_v1(&discriminator)?;
            let generation_start = pack_id == first_live_pack && offset == PACK_HEADER_LEN_V1;
            if prepare_kind == PrepareRecordKindV1::GenerationCheckpoint && !generation_start {
                return Err(RecoveryError::GenerationCheckpointPosition {
                    expected_pack: first_live_pack,
                    expected_offset: PACK_HEADER_LEN_V1,
                    actual_pack: pack_id,
                    actual_offset: offset,
                });
            }

            let (
                prepare_len,
                prepare_sequence,
                prepare_store,
                prepare_profile,
                prepare_base,
                target_state,
                commit,
                commit_len,
            ) = match prepare_kind {
                PrepareRecordKindV1::Incremental => {
                    transactions =
                        limits.checked_charge(DurableLimitKind::Transactions, transactions, 1)?;
                    let prefix_end = offset
                        .checked_add(TRANSACTION_PREPARE_HEADER_LEN_V1)
                        .ok_or(RecoveryError::Arithmetic("incremental prepare prefix end"))?;
                    if prefix_end > durable_tail {
                        return Err(RecoveryError::AuthorizedTailCutsRecord {
                            pack: pack_id,
                            record_offset: offset,
                            required_end: prefix_end,
                            durable_tail,
                        });
                    }
                    let mut prefix = [0u8; TRANSACTION_PREPARE_HEADER_LEN_V1 as usize];
                    prefix[..PREPARE_DISCRIMINATOR_LEN_V1 as usize].copy_from_slice(&discriminator);
                    reader
                        .read_exact_at(
                            discriminator_end,
                            &mut prefix[PREPARE_DISCRIMINATOR_LEN_V1 as usize..],
                        )
                        .map_err(RecoveryError::Read)?;
                    let preflight = preflight_prepare_record_v1(&prefix, wal_limits)?;
                    object_records = limits.checked_charge(
                        DurableLimitKind::ObjectRecords,
                        object_records,
                        preflight.object_count(),
                    )?;
                    let prepare_len = preflight.record_len();
                    let prepare_end = offset
                        .checked_add(prepare_len)
                        .ok_or(RecoveryError::Arithmetic("incremental prepare end"))?;
                    let commit_discriminator = require_commit_pair(
                        &reader,
                        pack_id,
                        prepare_end,
                        durable_tail,
                        prepare_kind,
                        CommitRecordKindV1::Incremental,
                    )?;
                    let commit_end = prepare_end
                        .checked_add(TRANSACTION_COMMIT_RECORD_LEN_V1)
                        .ok_or(RecoveryError::Arithmetic("incremental commit end"))?;
                    if commit_end > durable_tail {
                        return Err(RecoveryError::AuthorizedTailCutsRecord {
                            pack: pack_id,
                            record_offset: offset,
                            required_end: commit_end,
                            durable_tail,
                        });
                    }

                    let prepare_size = usize::try_from(prepare_len)
                        .map_err(|_| RecoveryError::Arithmetic("prepare allocation"))?;
                    let mut prepare_bytes = Vec::new();
                    prepare_bytes.try_reserve_exact(prepare_size).map_err(|_| {
                        RecoveryError::Wal(LogPackFormatError::ResourceLimit {
                            resource: "prepare allocation bytes",
                            limit: wal_limits.max_record_bytes,
                            attempted: prepare_len,
                        })
                    })?;
                    prepare_bytes.resize(prepare_size, 0);
                    prepare_bytes[..TRANSACTION_PREPARE_HEADER_LEN_V1 as usize]
                        .copy_from_slice(&prefix);
                    reader
                        .read_exact_at(
                            prefix_end,
                            &mut prepare_bytes[TRANSACTION_PREPARE_HEADER_LEN_V1 as usize..],
                        )
                        .map_err(RecoveryError::Read)?;
                    let prepare =
                        TransactionPrepareRecordViewV1::parse(&prepare_bytes, wal_limits)?;
                    let mut raw_commit = [0u8; TRANSACTION_COMMIT_RECORD_LEN_V1 as usize];
                    raw_commit[..COMMIT_DISCRIMINATOR_LEN_V1 as usize]
                        .copy_from_slice(&commit_discriminator);
                    let commit_body_offset =
                        prepare_end
                            .checked_add(COMMIT_DISCRIMINATOR_LEN_V1)
                            .ok_or(RecoveryError::Arithmetic("incremental commit body offset"))?;
                    reader
                        .read_exact_at(
                            commit_body_offset,
                            &mut raw_commit[COMMIT_DISCRIMINATOR_LEN_V1 as usize..],
                        )
                        .map_err(RecoveryError::Read)?;
                    let incremental_commit = TransactionCommitViewV1::parse(&raw_commit)?;
                    incremental_commit.verify_prepare(prepare)?;

                    for object in prepare.objects() {
                        match object.disposition() {
                            LogPackObjectDispositionV1::Data => {
                                index_records = index_records
                                    .checked_add(1)
                                    .ok_or(RecoveryError::Arithmetic("index record count"))?;
                                limits.require(DurableLimitKind::IndexRecords, index_records)?;
                                let record = ObjectIndexRecordV1::from_data_view(
                                    pack_id,
                                    offset,
                                    prepare.sequence(),
                                    object,
                                )?;
                                expected_index_tail = expected_index_tail
                                    .checked_add(INDEX_RECORD_LEN_V1)
                                    .ok_or(RecoveryError::Arithmetic("index tail"))?;
                                limits
                                    .require(DurableLimitKind::IndexBytes, expected_index_tail)?;
                                index_sink.data(record)?;
                            }
                            LogPackObjectDispositionV1::Reuse if generation_start => {
                                return Err(RecoveryError::GenerationCheckpointReuse(
                                    object.reference(),
                                ));
                            }
                            LogPackObjectDispositionV1::Reuse => {
                                index_sink.reuse(object.reference(), prepare.sequence())?;
                            }
                        }
                    }
                    (
                        prepare_len,
                        prepare.sequence(),
                        prepare.store_id(),
                        prepare.receipt().profile_id(),
                        Some(prepare.receipt().base()),
                        prepare.receipt().target(),
                        TerminalCommitViewV1::Incremental(incremental_commit),
                        TRANSACTION_COMMIT_RECORD_LEN_V1,
                    )
                }
                PrepareRecordKindV1::GenerationCheckpoint => {
                    checkpoints = checkpoints
                        .checked_add(1)
                        .ok_or(RecoveryError::Arithmetic("checkpoint record count"))?;
                    let prefix_end = offset
                        .checked_add(GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1)
                        .ok_or(RecoveryError::Arithmetic("checkpoint prepare prefix end"))?;
                    if prefix_end > durable_tail {
                        return Err(RecoveryError::AuthorizedTailCutsRecord {
                            pack: pack_id,
                            record_offset: offset,
                            required_end: prefix_end,
                            durable_tail,
                        });
                    }
                    let mut prefix = [0u8; GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize];
                    prefix[..PREPARE_DISCRIMINATOR_LEN_V1 as usize].copy_from_slice(&discriminator);
                    reader
                        .read_exact_at(
                            discriminator_end,
                            &mut prefix[PREPARE_DISCRIMINATOR_LEN_V1 as usize..],
                        )
                        .map_err(RecoveryError::Read)?;
                    let preflight =
                        preflight_generation_checkpoint_prepare_v1(&prefix, wal_limits)?;
                    if preflight.sequence() != expected_sequence {
                        return Err(RecoveryError::Sequence {
                            expected: expected_sequence,
                            actual: preflight.sequence(),
                        });
                    }
                    if preflight.store_id() != expected_store {
                        return Err(RecoveryError::Store {
                            expected: expected_store,
                            actual: preflight.store_id(),
                        });
                    }
                    if preflight.descriptor().profile_id() != expected_profile {
                        return Err(RecoveryError::Profile {
                            expected: expected_profile,
                            actual: preflight.descriptor().profile_id(),
                        });
                    }
                    let prepare_len = preflight.record_len();
                    let prepare_end = offset
                        .checked_add(prepare_len)
                        .ok_or(RecoveryError::Arithmetic("checkpoint prepare end"))?;
                    let commit_discriminator = require_commit_pair(
                        &reader,
                        pack_id,
                        prepare_end,
                        durable_tail,
                        prepare_kind,
                        CommitRecordKindV1::GenerationCheckpoint,
                    )?;
                    let commit_end = prepare_end
                        .checked_add(GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1)
                        .ok_or(RecoveryError::Arithmetic("checkpoint commit end"))?;
                    if commit_end > durable_tail {
                        return Err(RecoveryError::AuthorizedTailCutsRecord {
                            pack: pack_id,
                            record_offset: offset,
                            required_end: commit_end,
                            durable_tail,
                        });
                    }
                    let mut raw_commit = [0u8; GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1 as usize];
                    raw_commit[..COMMIT_DISCRIMINATOR_LEN_V1 as usize]
                        .copy_from_slice(&commit_discriminator);
                    let commit_body_offset =
                        prepare_end
                            .checked_add(COMMIT_DISCRIMINATOR_LEN_V1)
                            .ok_or(RecoveryError::Arithmetic("checkpoint commit body offset"))?;
                    reader
                        .read_exact_at(
                            commit_body_offset,
                            &mut raw_commit[COMMIT_DISCRIMINATOR_LEN_V1 as usize..],
                        )
                        .map_err(RecoveryError::Read)?;
                    let checkpoint_commit = GenerationCheckpointCommitViewV1::parse(&raw_commit)?;
                    if checkpoint_commit.sequence() != expected_sequence {
                        return Err(RecoveryError::Sequence {
                            expected: expected_sequence,
                            actual: checkpoint_commit.sequence(),
                        });
                    }
                    if checkpoint_commit.store_id() != expected_store {
                        return Err(RecoveryError::Store {
                            expected: expected_store,
                            actual: checkpoint_commit.store_id(),
                        });
                    }
                    if checkpoint_commit.descriptor() != preflight.descriptor() {
                        return Err(RecoveryError::Wal(
                            LogPackFormatError::CheckpointDescriptorMismatch,
                        ));
                    }
                    if checkpoint_commit.previous_commit() != previous_commit {
                        return Err(RecoveryError::PreviousCommit {
                            expected: previous_commit,
                            actual: checkpoint_commit.previous_commit(),
                        });
                    }
                    let expected_prepare_address =
                        LogPackAddress::from_pack(pack_id, offset, prepare_len)?;
                    if checkpoint_commit.prepare_address() != expected_prepare_address {
                        return Err(RecoveryError::PrepareAddress {
                            expected: expected_prepare_address,
                            actual: checkpoint_commit.prepare_address(),
                        });
                    }
                    if checkpoint_commit.publication().first_live_pack() != first_live_pack.get() {
                        return Err(RecoveryError::PublicationFirstLive {
                            expected: first_live_pack.get(),
                            actual: checkpoint_commit.publication().first_live_pack(),
                        });
                    }
                    if checkpoint_commit.publication().index_generation() != index_generation {
                        return Err(RecoveryError::PublicationIndexGeneration {
                            expected: index_generation,
                            actual: checkpoint_commit.publication().index_generation(),
                        });
                    }
                    let checkpoint_index_bytes = preflight
                        .descriptor()
                        .data_objects()
                        .checked_mul(INDEX_RECORD_LEN_V1)
                        .ok_or(RecoveryError::Arithmetic("checkpoint exact index record bytes"))?;
                    let checkpoint_index_tail = expected_index_tail
                        .checked_add(checkpoint_index_bytes)
                        .ok_or(RecoveryError::Arithmetic("checkpoint exact index tail"))?;
                    let _checkpoint_object_records = limits.checked_charge(
                        DurableLimitKind::ObjectRecords,
                        object_records,
                        preflight.descriptor().data_objects(),
                    )?;
                    let checkpoint_index_records = index_records
                        .checked_add(preflight.descriptor().data_objects())
                        .ok_or(RecoveryError::Arithmetic(
                            "checkpoint projected index record count",
                        ))?;
                    limits.require(DurableLimitKind::IndexRecords, checkpoint_index_records)?;
                    limits.require(DurableLimitKind::IndexBytes, checkpoint_index_tail)?;
                    if checkpoint_commit.publication().durable_index_tail() != checkpoint_index_tail
                    {
                        return Err(RecoveryError::IndexTail {
                            expected: checkpoint_index_tail,
                            actual: checkpoint_commit.publication().durable_index_tail(),
                        });
                    }
                    if checkpoint_scratch.is_none() {
                        let mut scratch = Vec::new();
                        scratch
                            .try_reserve_exact(CHECKPOINT_STREAM_SCRATCH_LEN)
                            .map_err(|_| RecoveryError::CheckpointScratchAllocation {
                                bytes: CHECKPOINT_STREAM_SCRATCH_LEN as u64,
                            })?;
                        scratch.resize(CHECKPOINT_STREAM_SCRATCH_LEN, 0);
                        checkpoint_scratch = Some(scratch);
                    }
                    let identity = stream_generation_checkpoint_prepare(
                        &reader,
                        pack_id,
                        offset,
                        &prefix,
                        preflight,
                        limits,
                        wal_limits,
                        &mut object_records,
                        &mut index_records,
                        &mut expected_index_tail,
                        &mut index_sink,
                        checkpoint_scratch
                            .as_deref_mut()
                            .expect("checkpoint branch initialized its bounded scratch"),
                    )?;
                    checkpoint_commit.verify_prepare(identity)?;
                    (
                        prepare_len,
                        identity.sequence(),
                        identity.store_id(),
                        identity.descriptor().profile_id(),
                        None,
                        identity.descriptor().state(),
                        TerminalCommitViewV1::GenerationCheckpoint(checkpoint_commit),
                        GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
                    )
                }
            };

            if prepare_sequence != expected_sequence {
                return Err(RecoveryError::Sequence {
                    expected: expected_sequence,
                    actual: prepare_sequence,
                });
            }
            if prepare_store != expected_store {
                return Err(RecoveryError::Store {
                    expected: expected_store,
                    actual: prepare_store,
                });
            }
            if prepare_profile != expected_profile {
                return Err(RecoveryError::Profile {
                    expected: expected_profile,
                    actual: prepare_profile,
                });
            }
            if let (Some(expected_base), Some(actual_base)) = (previous_state, prepare_base) {
                if actual_base != expected_base {
                    return Err(RecoveryError::StateContinuity {
                        expected: expected_base,
                        actual: actual_base,
                    });
                }
            }
            if commit.previous_commit() != previous_commit {
                return Err(RecoveryError::PreviousCommit {
                    expected: previous_commit,
                    actual: commit.previous_commit(),
                });
            }
            let prepare_end = offset
                .checked_add(prepare_len)
                .ok_or(RecoveryError::Arithmetic("prepare end"))?;
            let expected_prepare_address = LogPackAddress::from_pack(pack_id, offset, prepare_len)?;
            if commit.prepare_address() != expected_prepare_address {
                return Err(RecoveryError::PrepareAddress {
                    expected: expected_prepare_address,
                    actual: commit.prepare_address(),
                });
            }
            if commit.publication().first_live_pack() != first_live_pack.get() {
                return Err(RecoveryError::PublicationFirstLive {
                    expected: first_live_pack.get(),
                    actual: commit.publication().first_live_pack(),
                });
            }
            if commit.publication().index_generation() != index_generation {
                return Err(RecoveryError::PublicationIndexGeneration {
                    expected: index_generation,
                    actual: commit.publication().index_generation(),
                });
            }

            if commit.durable_index_tail() != expected_index_tail {
                return Err(RecoveryError::IndexTail {
                    expected: expected_index_tail,
                    actual: commit.durable_index_tail(),
                });
            }

            previous_commit = commit.digest();
            previous_state = Some(target_state);
            last_sequence = Some(expected_sequence);
            terminal_address = Some(LogPackAddress::from_pack(pack_id, prepare_end, commit_len)?);
            terminal_commit = Some(commit);
            offset = prepare_end
                .checked_add(commit_len)
                .ok_or(RecoveryError::Arithmetic("commit end"))?;
            if offset < durable_tail {
                expected_sequence = expected_sequence.successor()?;
            }
        }
        if offset != durable_tail {
            return Err(RecoveryError::ParsedTailMismatch {
                pack: pack_id,
                parsed_end: offset,
                durable_tail,
            });
        }
        let last_sequence = last_sequence.ok_or(RecoveryError::PackHasNoTransactions(pack_id))?;
        previous_terminal =
            Some(PackTerminalIdentityV1::new(pack_id, last_sequence, previous_commit));

        if pack_id == active_pack {
            break;
        }
        pack_raw = pack_raw
            .checked_add(1)
            .ok_or(RecoveryError::Arithmetic("pack identifier"))?;
    }

    let terminal = previous_terminal.ok_or(RecoveryError::EmptyPackSet)?;
    let terminal_address = terminal_address.ok_or(RecoveryError::EmptyPackSet)?;
    let terminal_commit = terminal_commit.ok_or(RecoveryError::EmptyPackSet)?;
    let state = previous_state.ok_or(RecoveryError::EmptyPackSet)?;
    let index = index_sink.finish()?;
    Ok(ScannedPackChainV1 {
        catalog_image: *authority.image(),
        store_id: expected_store,
        profile_id: expected_profile,
        state,
        terminal,
        terminal_address,
        terminal_commit,
        index,
        expected_index_tail,
        transactions,
        checkpoints,
        object_records,
        scanned_bytes,
    })
}
