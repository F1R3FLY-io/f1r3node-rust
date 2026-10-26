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
use crate::rspace::persistence::canonical::{CanonicalFormatError, OBJECT_DOMAIN_LEN, ReadCursor};
use crate::rspace::persistence::checkpoint::OrderedObjectInventoryBuilderV1;
use crate::rspace::persistence::checksum::{CHECKSUM_FIELD_LEN, Crc32c, decode_crc32c};
use crate::rspace::persistence::mutation::{
    MUTATION_STREAM_MAX_SCRATCH_BYTES_V1, MutationBatchReadAtV1, MutationDecodeLimits,
    MutationReadClassV1, MutationStreamErrorV1, stream_mutation_batch_v1,
};
use crate::rspace::persistence::object::CanonicalObjectStreamValidatorV1;
use crate::rspace::persistence::physical::PHYSICAL_DIGEST_LEN;
use crate::rspace::persistence::retention::v2_format::RetentionReader;
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1,
    GenerationCheckpointCommitViewV1, GenerationCheckpointPrepareIdentityV1,
    GenerationCheckpointPreparePreflightV1, PrepareRecordPreflightV1, TerminalCommitViewV1,
    designated_retention_object, preflight_generation_checkpoint_prepare_for,
    validate_streamed_mutation_receipt_v1,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, COMMIT_DISCRIMINATOR_LEN_V1, CanonicalObjectKind, CatalogAuthorityTokenV1,
    CommitRecordKindV1, LogPackAddress, LogPackDecodeLimits, LogPackFormatError,
    LogPackObjectDispositionV1, LogPackObjectHeaderV1, LogPackSequence,
    OBJECT_RECORD_HEADER_LEN_V1, ObjectDigest, ObjectRefV1, PREPARE_DISCRIMINATOR_LEN_V1, PackId,
    PhysicalDigest, PrepareRecordKindV1, SemanticProfileId, StateVersionV1, StoreId,
    TRANSACTION_COMMIT_RECORD_LEN_V1, TRANSACTION_PREPARE_HEADER_LEN_V1, TransactionCommitViewV1,
    decode_commit_kind_v1, decode_prepare_kind_v1, preflight_prepare_record_v1,
};

const CHECKPOINT_OBJECT_REFERENCE_LEN: u64 = 40;
const CHECKPOINT_STREAM_SCRATCH_LEN: usize = 64 * 1024;
const INCREMENTAL_LAYOUT_TRANSCRIPT_DOMAIN: &[u8] = b"RSpaceK1LayoutTranscriptV1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IncrementalLayoutDigestV1([u8; 32]);

impl IncrementalLayoutDigestV1 {
    pub(crate) const fn as_bytes(&self) -> &[u8; 32] { &self.0 }
}

pub(crate) struct IncrementalLayoutTranscriptV1 {
    hasher: Blake2b<U32>,
    object_count: u64,
    data_count: u64,
    logical_object_bytes: u64,
}

impl IncrementalLayoutTranscriptV1 {
    pub(crate) fn new() -> Self {
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(INCREMENTAL_LAYOUT_TRANSCRIPT_DOMAIN);
        Self {
            hasher,
            object_count: 0,
            data_count: 0,
            logical_object_bytes: 0,
        }
    }

    pub(crate) fn push(
        &mut self,
        disposition: LogPackObjectDispositionV1,
        reference: ObjectRefV1,
        relative_record_offset: u64,
        relative_canonical_offset: Option<u64>,
        relative_final_extent: u64,
    ) -> Result<(), &'static str> {
        self.hasher.update(self.object_count.to_le_bytes());
        self.hasher.update(disposition.kind().to_le_bytes());
        self.hasher.update(reference.digest().as_bytes());
        self.hasher.update(reference.encoded_len().to_le_bytes());
        self.hasher.update(relative_record_offset.to_le_bytes());
        match relative_canonical_offset {
            Some(offset) => {
                self.hasher.update([1]);
                self.hasher.update(offset.to_le_bytes());
                self.data_count = self.data_count.checked_add(1).ok_or("layout DATA count")?;
            }
            None => {
                self.hasher.update([0]);
                self.hasher.update(0u64.to_le_bytes());
            }
        }
        self.hasher.update(relative_final_extent.to_le_bytes());
        self.object_count = self
            .object_count
            .checked_add(1)
            .ok_or("layout object count")?;
        self.logical_object_bytes = self
            .logical_object_bytes
            .checked_add(reference.encoded_len())
            .ok_or("layout logical object bytes")?;
        Ok(())
    }

    pub(crate) fn finish(self, relative_final_extent: u64) -> IncrementalLayoutSummaryV1 {
        let mut hasher = self.hasher;
        hasher.update(self.object_count.to_le_bytes());
        hasher.update(self.data_count.to_le_bytes());
        hasher.update(self.logical_object_bytes.to_le_bytes());
        hasher.update(relative_final_extent.to_le_bytes());
        let hash = hasher.finalize();
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&hash);
        IncrementalLayoutSummaryV1 {
            digest: IncrementalLayoutDigestV1(digest),
            object_count: self.object_count,
            data_count: self.data_count,
            logical_object_bytes: self.logical_object_bytes,
            relative_final_extent,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IncrementalLayoutSummaryV1 {
    pub(crate) digest: IncrementalLayoutDigestV1,
    pub(crate) object_count: u64,
    pub(crate) data_count: u64,
    pub(crate) logical_object_bytes: u64,
    pub(crate) relative_final_extent: u64,
}

struct IncrementalPrimaryScanV1 {
    identity: crate::rspace::persistence::PrepareRecordIdentityV1,
    layout: IncrementalLayoutSummaryV1,
    objects_offset: u64,
    trailer_offset: u64,
    projected_index_records: u64,
    projected_index_tail: u64,
}

struct CommitBoundIncrementalV1 {
    primary: IncrementalPrimaryScanV1,
}

struct ChainBoundIncrementalV1 {
    primary: IncrementalPrimaryScanV1,
}

impl CommitBoundIncrementalV1 {
    fn bind_chain(self) -> ChainBoundIncrementalV1 {
        ChainBoundIncrementalV1 {
            primary: self.primary,
        }
    }
}

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

    /// The retention formats recovery accepts from this provider's packs.
    /// A provider that does not opt in stays a strict V1 reader.
    fn retention_reader(&self) -> RetentionReader { RetentionReader::V1Only }
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
    read_work: RecoveryReadWorkV1,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RecoveryReadWorkV1 {
    authorized_pack_bytes: u64,
    physical_read_bytes: u64,
    primary_authenticated_bytes: u64,
    prior_key_reread_bytes: u64,
    authenticated_source_replay_bytes: u64,
    layout_application_replay_bytes: u64,
}

impl RecoveryReadWorkV1 {
    pub(crate) const fn authorized_pack_bytes(self) -> u64 { self.authorized_pack_bytes }

    pub(crate) const fn physical_read_bytes(self) -> u64 { self.physical_read_bytes }

    pub(crate) const fn primary_authenticated_bytes(self) -> u64 {
        self.primary_authenticated_bytes
    }

    pub(crate) const fn prior_key_reread_bytes(self) -> u64 { self.prior_key_reread_bytes }

    pub(crate) const fn authenticated_source_replay_bytes(self) -> u64 {
        self.authenticated_source_replay_bytes
    }

    pub(crate) const fn layout_application_replay_bytes(self) -> u64 {
        self.layout_application_replay_bytes
    }

    pub(crate) const fn layout_replay_bytes(self) -> u64 { self.layout_application_replay_bytes }
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

    pub(crate) const fn read_work(&self) -> RecoveryReadWorkV1 { self.scan.read_work }

    pub(crate) const fn terminal_commit(&self) -> TerminalCommitViewV1 { self.scan.terminal_commit }

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
    IncrementalReplayTranscript {
        expected: IncrementalLayoutSummaryV1,
        actual: IncrementalLayoutSummaryV1,
    },
    IncrementalReplaySourceChanged {
        expected_digest: PhysicalDigest,
        actual_digest: PhysicalDigest,
        expected_crc32c: u32,
        actual_crc32c: u32,
        expected_bytes: u64,
        actual_bytes: u64,
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
    #[expect(
        dead_code,
        reason = "Display prints a fixed message, so only the derived Debug reads these fields"
    )]
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
            Self::IncrementalReplayTranscript { expected, actual } => write!(
                formatter,
                "incremental PREPARE replay layout {actual:?} does not equal authenticated \
                 first-pass layout {expected:?}"
            ),
            Self::IncrementalReplaySourceChanged {
                expected_digest,
                actual_digest,
                expected_crc32c,
                actual_crc32c,
                expected_bytes,
                actual_bytes,
            } => write!(
                formatter,
                "incremental PREPARE authenticated source changed between passes: expected \
                 {expected_bytes} bytes, digest {expected_digest}, CRC32C \
                 {expected_crc32c:#010x}; replayed {actual_bytes} bytes, digest {actual_digest}, \
                 CRC32C {actual_crc32c:#010x}"
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

#[derive(Clone, Copy)]
enum RecoveryReadClassV1 {
    PrimaryAuthenticated,
    PriorKeyReread,
    SourceAuthenticationReplay,
    LayoutApplicationReplay,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RecoveryReadAccountingV1 {
    physical_read_bytes: u64,
    primary_authenticated_bytes: u64,
    prior_key_reread_bytes: u64,
    authenticated_source_replay_bytes: u64,
    layout_application_replay_bytes: u64,
}

impl RecoveryReadAccountingV1 {
    const fn report(self, authorized_pack_bytes: u64) -> RecoveryReadWorkV1 {
        RecoveryReadWorkV1 {
            authorized_pack_bytes,
            physical_read_bytes: self.physical_read_bytes,
            primary_authenticated_bytes: self.primary_authenticated_bytes,
            prior_key_reread_bytes: self.prior_key_reread_bytes,
            authenticated_source_replay_bytes: self.authenticated_source_replay_bytes,
            layout_application_replay_bytes: self.layout_application_replay_bytes,
        }
    }
}

fn read_exact_accounted<R: DurableReadAt>(
    reader: &R,
    offset: u64,
    destination: &mut [u8],
    limits: DurableLimits,
    accounting: &mut RecoveryReadAccountingV1,
    class: RecoveryReadClassV1,
) -> Result<(), RecoveryError<R::Error>> {
    let width = u64::try_from(destination.len())
        .map_err(|_| RecoveryError::Arithmetic("physical read width"))?;
    let next_physical = limits.checked_charge(
        DurableLimitKind::TotalPhysicalReadBytes,
        accounting.physical_read_bytes,
        width,
    )?;
    let current_class = match class {
        RecoveryReadClassV1::PrimaryAuthenticated => accounting.primary_authenticated_bytes,
        RecoveryReadClassV1::PriorKeyReread => accounting.prior_key_reread_bytes,
        RecoveryReadClassV1::SourceAuthenticationReplay => {
            accounting.authenticated_source_replay_bytes
        }
        RecoveryReadClassV1::LayoutApplicationReplay => accounting.layout_application_replay_bytes,
    };
    let next_class = current_class
        .checked_add(width)
        .ok_or(RecoveryError::Arithmetic("classified physical read bytes"))?;
    accounting.physical_read_bytes = next_physical;
    match class {
        RecoveryReadClassV1::PrimaryAuthenticated => {
            accounting.primary_authenticated_bytes = next_class;
        }
        RecoveryReadClassV1::PriorKeyReread => accounting.prior_key_reread_bytes = next_class,
        RecoveryReadClassV1::SourceAuthenticationReplay => {
            accounting.authenticated_source_replay_bytes = next_class;
        }
        RecoveryReadClassV1::LayoutApplicationReplay => {
            accounting.layout_application_replay_bytes = next_class;
        }
    }
    reader
        .read_exact_at(offset, destination)
        .map_err(RecoveryError::Read)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn drain_primary_record_extent<R: DurableReadAt>(
    reader: &R,
    mut cursor: u64,
    end: u64,
    scratch: &mut [u8],
    limits: DurableLimits,
    accounting: &mut RecoveryReadAccountingV1,
    crc: &mut Crc32c,
    hasher: &mut Blake2b<U32>,
) -> Result<(), RecoveryError<R::Error>> {
    debug_assert!(!scratch.is_empty());
    while cursor < end {
        let remaining = end - cursor;
        let width = usize::try_from(remaining.min(scratch.len() as u64))
            .map_err(|_| RecoveryError::Arithmetic("primary drain width"))?;
        let chunk = &mut scratch[..width];
        read_exact_accounted(
            reader,
            cursor,
            chunk,
            limits,
            accounting,
            RecoveryReadClassV1::PrimaryAuthenticated,
        )?;
        crc.update(chunk);
        hasher.update(&*chunk);
        cursor = cursor
            .checked_add(width as u64)
            .ok_or(RecoveryError::Arithmetic("primary drain cursor"))?;
    }
    Ok(())
}

struct RecoveryMutationSourceV1<'reader, 'accounting, R> {
    reader: &'reader R,
    limits: DurableLimits,
    accounting: &'accounting mut RecoveryReadAccountingV1,
}

impl<R: DurableReadAt> MutationBatchReadAtV1 for RecoveryMutationSourceV1<'_, '_, R> {
    type Error = RecoveryError<R::Error>;

    fn read_exact_at(
        &mut self,
        class: MutationReadClassV1,
        offset: u64,
        destination: &mut [u8],
    ) -> Result<(), Self::Error> {
        let class = match class {
            MutationReadClassV1::Primary => RecoveryReadClassV1::PrimaryAuthenticated,
            MutationReadClassV1::PriorKeyReread => RecoveryReadClassV1::PriorKeyReread,
        };
        read_exact_accounted(self.reader, offset, destination, self.limits, self.accounting, class)
    }
}

fn canonical_truncation(
    offset: u64,
    needed: u64,
    available: u64,
) -> Result<LogPackFormatError, &'static str> {
    let offset = usize::try_from(offset).map_err(|_| "canonical truncation offset")?;
    let needed = usize::try_from(needed).map_err(|_| "canonical truncation width")?;
    let available =
        usize::try_from(available).map_err(|_| "canonical truncation available bytes")?;
    Ok(CanonicalFormatError::Truncated {
        offset,
        needed,
        available,
    }
    .into())
}

struct IncrementalPrimaryStateV1<E> {
    retention_reader: RetentionReader,
    cursor: u64,
    crc: Crc32c,
    hasher: Blake2b<U32>,
    pending_error: Option<RecoveryError<E>>,
    transcript: IncrementalLayoutTranscriptV1,
    previous: Option<ObjectDigest>,
    observed_object_bytes: u64,
    projected_index_records: u64,
    projected_index_tail: u64,
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn scan_incremental_mutation_primary<R: DurableReadAt>(
    reader: &R,
    mutation_offset: u64,
    trailer_offset: u64,
    preflight: PrepareRecordPreflightV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    scratch: &mut [u8],
    accounting: &mut RecoveryReadAccountingV1,
    state: &mut IncrementalPrimaryStateV1<R::Error>,
) -> Result<(), RecoveryError<R::Error>> {
    let available_payload = trailer_offset.saturating_sub(mutation_offset);
    if mutation_offset > trailer_offset {
        state.pending_error = Some(RecoveryError::Wal(
            canonical_truncation(0, preflight.mutation_len(), 0)
                .map_err(RecoveryError::Arithmetic)?,
        ));
        return Ok(());
    }
    if preflight.mutation_len() > available_payload {
        state.pending_error = Some(RecoveryError::Wal(
            canonical_truncation(0, preflight.mutation_len(), available_payload)
                .map_err(RecoveryError::Arithmetic)?,
        ));
        return Ok(());
    }

    let mut source = RecoveryMutationSourceV1 {
        reader,
        limits,
        accounting,
    };
    let streamed = stream_mutation_batch_v1(
        &mut source,
        mutation_offset,
        preflight.mutation_len(),
        MutationDecodeLimits {
            max_entries: wal_limits.max_mutation_entries,
            max_key_bytes: wal_limits.max_mutation_key_bytes,
        },
        u64::MAX,
        scratch,
        |bytes| {
            state.crc.update(bytes);
            state.hasher.update(bytes);
        },
    );
    state.cursor = mutation_offset
        .checked_add(preflight.mutation_len())
        .ok_or(RecoveryError::Arithmetic("incremental mutation end"))?;
    match streamed {
        Ok(streamed) => {
            if let Err(error) = validate_streamed_mutation_receipt_v1(
                streamed.payload_digest(),
                streamed.base(),
                streamed.target_generation(),
                preflight.receipt(),
            ) {
                state.pending_error = Some(RecoveryError::Wal(error));
            }
        }
        Err(failure) => match failure.into_error() {
            MutationStreamErrorV1::Read(error) => return Err(error),
            MutationStreamErrorV1::Mutation(error) => {
                state.pending_error = Some(RecoveryError::Wal(error.into()));
            }
            MutationStreamErrorV1::SourceChanged { .. } => {
                state.pending_error = Some(RecoveryError::Arithmetic(
                    "incremental mutation source changed during primary validation",
                ));
            }
            MutationStreamErrorV1::ScratchBytes { .. } => {
                return Err(RecoveryError::Arithmetic("incremental mutation scratch contract"));
            }
            MutationStreamErrorV1::PhysicalReadLimit { .. } => {
                return Err(RecoveryError::Arithmetic(
                    "incremental mutation nested physical-read limit",
                ));
            }
        },
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn scan_one_incremental_object_primary<R, S>(
    reader: &R,
    pack_id: PackId,
    prepare_offset: u64,
    mutation_offset: u64,
    trailer_offset: u64,
    preflight: PrepareRecordPreflightV1,
    generation_start: bool,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    index_sink: &S,
    scratch: &mut [u8],
    accounting: &mut RecoveryReadAccountingV1,
    state: &mut IncrementalPrimaryStateV1<R::Error>,
) -> Result<(), RecoveryError<R::Error>>
where
    R: DurableReadAt,
    S: RecoveryIndexSinkV1<R::Error>,
{
    let available = trailer_offset.saturating_sub(state.cursor);
    if state.cursor > trailer_offset || available < OBJECT_RECORD_HEADER_LEN_V1 {
        let offset = state.cursor.saturating_sub(mutation_offset);
        state.pending_error = Some(RecoveryError::Wal(
            canonical_truncation(offset, OBJECT_RECORD_HEADER_LEN_V1, available)
                .map_err(RecoveryError::Arithmetic)?,
        ));
        return Ok(());
    }
    let relative_record_offset = state
        .cursor
        .checked_sub(prepare_offset)
        .ok_or(RecoveryError::Arithmetic("relative object record offset"))?;
    let mut raw_header = [0u8; OBJECT_RECORD_HEADER_LEN_V1 as usize];
    read_exact_accounted(
        reader,
        state.cursor,
        &mut raw_header,
        limits,
        accounting,
        RecoveryReadClassV1::PrimaryAuthenticated,
    )?;
    state.crc.update(&raw_header);
    state.hasher.update(raw_header);
    state.cursor = state
        .cursor
        .checked_add(OBJECT_RECORD_HEADER_LEN_V1)
        .ok_or(RecoveryError::Arithmetic("incremental object body offset"))?;
    let header = match LogPackObjectHeaderV1::parse_exact(&raw_header) {
        Ok(header) => header,
        Err(error) => {
            state.pending_error = Some(RecoveryError::Wal(error));
            return Ok(());
        }
    };
    let disposition = header.disposition();
    let reference = header.reference();
    if let Some(previous) = state.previous {
        if previous >= reference.digest() {
            state.pending_error = Some(RecoveryError::Wal(LogPackFormatError::ObjectsNotStrict {
                previous,
                current: reference.digest(),
            }));
            return Ok(());
        }
    }
    if reference.encoded_len() > wal_limits.max_single_object_bytes {
        state.pending_error = Some(RecoveryError::Wal(LogPackFormatError::ResourceLimit {
            resource: "single referenced object bytes",
            limit: wal_limits.max_single_object_bytes,
            attempted: reference.encoded_len(),
        }));
        return Ok(());
    }
    state.observed_object_bytes = match state
        .observed_object_bytes
        .checked_add(reference.encoded_len())
    {
        Some(bytes) => bytes,
        None => {
            state.pending_error = Some(RecoveryError::Wal(
                CanonicalFormatError::LengthOverflow {
                    field: "WAL canonical object bytes",
                }
                .into(),
            ));
            return Ok(());
        }
    };

    let relative_canonical_offset = match disposition {
        LogPackObjectDispositionV1::Data => {
            let available = trailer_offset.saturating_sub(state.cursor);
            if state.cursor > trailer_offset || reference.encoded_len() > available {
                let offset = state.cursor.saturating_sub(mutation_offset);
                state.pending_error = Some(RecoveryError::Wal(
                    canonical_truncation(offset, reference.encoded_len(), available)
                        .map_err(RecoveryError::Arithmetic)?,
                ));
                return Ok(());
            }
            let object_len = match usize::try_from(reference.encoded_len()) {
                Ok(length) => length,
                Err(_) => {
                    state.pending_error = Some(RecoveryError::Wal(
                        CanonicalFormatError::LengthOutOfRange {
                            field: "WAL canonical object",
                            value: reference.encoded_len(),
                        }
                        .into(),
                    ));
                    return Ok(());
                }
            };
            let relative = state
                .cursor
                .checked_sub(prepare_offset)
                .ok_or(RecoveryError::Arithmetic("relative canonical object offset"))?;
            let mut validator =
                CanonicalObjectStreamValidatorV1::new_for(object_len, state.retention_reader);
            let body_end = state
                .cursor
                .checked_add(reference.encoded_len())
                .ok_or(RecoveryError::Arithmetic("incremental object body end"))?;
            let mut object_error = None;
            while state.cursor < body_end {
                let width = usize::try_from((body_end - state.cursor).min(scratch.len() as u64))
                    .map_err(|_| RecoveryError::Arithmetic("incremental object chunk"))?;
                let chunk = &mut scratch[..width];
                read_exact_accounted(
                    reader,
                    state.cursor,
                    chunk,
                    limits,
                    accounting,
                    RecoveryReadClassV1::PrimaryAuthenticated,
                )?;
                state.crc.update(chunk);
                state.hasher.update(&*chunk);
                if object_error.is_none() {
                    if let Err(error) = validator.feed(chunk) {
                        object_error = Some(error);
                    }
                }
                state.cursor = state
                    .cursor
                    .checked_add(width as u64)
                    .ok_or(RecoveryError::Arithmetic("incremental object cursor"))?;
            }
            if let Some(error) = object_error {
                state.pending_error = Some(RecoveryError::Wal(error.into()));
                return Ok(());
            }
            let actual = match validator.finish() {
                Ok(identity) => identity.reference(),
                Err(error) => {
                    state.pending_error = Some(RecoveryError::Wal(error.into()));
                    return Ok(());
                }
            };
            if actual != reference {
                state.pending_error =
                    Some(RecoveryError::Wal(LogPackFormatError::ObjectReference {
                        declared: reference,
                        actual,
                    }));
                return Ok(());
            }
            state.projected_index_records = state
                .projected_index_records
                .checked_add(1)
                .ok_or(RecoveryError::Arithmetic("index record count"))?;
            limits.require(DurableLimitKind::IndexRecords, state.projected_index_records)?;
            state.projected_index_tail = state
                .projected_index_tail
                .checked_add(INDEX_RECORD_LEN_V1)
                .ok_or(RecoveryError::Arithmetic("index tail"))?;
            limits.require(DurableLimitKind::IndexBytes, state.projected_index_tail)?;
            let record = ObjectIndexRecordV1::from_streamed_data(
                pack_id,
                prepare_offset,
                preflight.sequence(),
                reference,
                relative,
            )?;
            if let Err(error) = index_sink.validate_data(record) {
                state.pending_error = Some(error.into_recovery());
                return Ok(());
            }
            Some(relative)
        }
        LogPackObjectDispositionV1::Reuse if generation_start => {
            state.pending_error = Some(RecoveryError::GenerationCheckpointReuse(reference));
            return Ok(());
        }
        LogPackObjectDispositionV1::Reuse => {
            if let Err(error) = index_sink.validate_reuse(reference, preflight.sequence()) {
                state.pending_error = Some(error.into_recovery());
                return Ok(());
            }
            None
        }
    };
    let relative_final_extent = state
        .cursor
        .checked_sub(prepare_offset)
        .ok_or(RecoveryError::Arithmetic("relative object final extent"))?;
    state
        .transcript
        .push(
            disposition,
            reference,
            relative_record_offset,
            relative_canonical_offset,
            relative_final_extent,
        )
        .map_err(RecoveryError::Arithmetic)?;
    state.previous = Some(reference.digest());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn scan_incremental_objects_primary<R, S>(
    reader: &R,
    pack_id: PackId,
    prepare_offset: u64,
    mutation_offset: u64,
    trailer_offset: u64,
    preflight: PrepareRecordPreflightV1,
    generation_start: bool,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    index_sink: &S,
    scratch: &mut [u8],
    accounting: &mut RecoveryReadAccountingV1,
    state: &mut IncrementalPrimaryStateV1<R::Error>,
) -> Result<(), RecoveryError<R::Error>>
where
    R: DurableReadAt,
    S: RecoveryIndexSinkV1<R::Error>,
{
    for _ in 0..preflight.object_count() {
        if state.pending_error.is_some() {
            break;
        }
        scan_one_incremental_object_primary(
            reader,
            pack_id,
            prepare_offset,
            mutation_offset,
            trailer_offset,
            preflight,
            generation_start,
            limits,
            wal_limits,
            index_sink,
            scratch,
            accounting,
            state,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[expect(
    clippy::boxed_local,
    reason = "the phased caller boxes the CRC, Blake2b and error state to keep the stack flat"
)]
#[inline(never)]
fn finish_incremental_prepare_primary<R: DurableReadAt>(
    reader: &R,
    prepare_offset: u64,
    mutation_offset: u64,
    trailer_offset: u64,
    preflight: PrepareRecordPreflightV1,
    limits: DurableLimits,
    scratch: &mut [u8],
    accounting: &mut RecoveryReadAccountingV1,
    mut state: Box<IncrementalPrimaryStateV1<R::Error>>,
    objects_offset: u64,
) -> Result<IncrementalPrimaryScanV1, RecoveryError<R::Error>> {
    if state.pending_error.is_none() && state.cursor != trailer_offset {
        let relative = state
            .cursor
            .checked_sub(mutation_offset)
            .ok_or(RecoveryError::Arithmetic("incremental payload cursor"))?;
        let trailing = trailer_offset.saturating_sub(state.cursor);
        let offset = usize::try_from(relative)
            .map_err(|_| RecoveryError::Arithmetic("incremental trailing offset"))?;
        let trailing = usize::try_from(trailing)
            .map_err(|_| RecoveryError::Arithmetic("incremental trailing bytes"))?;
        state.pending_error = Some(RecoveryError::Wal(
            CanonicalFormatError::TrailingBytes { offset, trailing }.into(),
        ));
    }
    if state.pending_error.is_none() && state.observed_object_bytes != preflight.object_bytes() {
        state.pending_error = Some(RecoveryError::Wal(LogPackFormatError::LengthMismatch {
            declared: preflight.object_bytes(),
            actual: state.observed_object_bytes,
        }));
    }
    if state.cursor < trailer_offset {
        drain_primary_record_extent(
            reader,
            state.cursor,
            trailer_offset,
            scratch,
            limits,
            accounting,
            &mut state.crc,
            &mut state.hasher,
        )?;
    }
    let IncrementalPrimaryStateV1 {
        crc,
        hasher,
        pending_error,
        transcript,
        projected_index_records,
        projected_index_tail,
        ..
    } = *state;
    let mut trailer = [0u8; CHECKSUM_FIELD_LEN];
    read_exact_accounted(
        reader,
        trailer_offset,
        &mut trailer,
        limits,
        accounting,
        RecoveryReadClassV1::PrimaryAuthenticated,
    )?;
    let declared_crc = decode_crc32c(trailer).map_err(|encoded| {
        RecoveryError::Wal(LogPackFormatError::ChecksumOutOfRange {
            field: "prepare record",
            encoded,
        })
    })?;
    let actual_crc = crc.finish();
    if declared_crc != actual_crc {
        return Err(RecoveryError::Wal(LogPackFormatError::RecordChecksum {
            expected: declared_crc,
            actual: actual_crc,
        }));
    }
    if let Some(error) = pending_error {
        return Err(error);
    }
    let digest = finish_physical_digest(hasher);
    let relative_final_extent = trailer_offset
        .checked_sub(prepare_offset)
        .ok_or(RecoveryError::Arithmetic("incremental primary final extent"))?;
    let layout = transcript.finish(relative_final_extent);
    Ok(IncrementalPrimaryScanV1 {
        identity: crate::rspace::persistence::PrepareRecordIdentityV1::from_streamed(
            preflight,
            digest,
            declared_crc,
        ),
        layout,
        objects_offset,
        trailer_offset,
        projected_index_records,
        projected_index_tail,
    })
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn scan_incremental_prepare_primary_phased<R, S>(
    reader: &R,
    pack_id: PackId,
    prepare_offset: u64,
    prefix: &[u8; TRANSACTION_PREPARE_HEADER_LEN_V1 as usize],
    preflight: PrepareRecordPreflightV1,
    generation_start: bool,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    retention_reader: RetentionReader,
    index_sink: &S,
    current_index_records: u64,
    current_index_tail: u64,
    scratch: &mut [u8],
    accounting: &mut RecoveryReadAccountingV1,
) -> Result<IncrementalPrimaryScanV1, RecoveryError<R::Error>>
where
    R: DurableReadAt,
    S: RecoveryIndexSinkV1<R::Error>,
{
    debug_assert!(!scratch.is_empty());
    let prepare_end = prepare_offset
        .checked_add(preflight.record_len())
        .ok_or(RecoveryError::Arithmetic("incremental prepare end"))?;
    let trailer_offset = prepare_end
        .checked_sub(CHECKSUM_FIELD_LEN as u64)
        .ok_or(RecoveryError::Arithmetic("incremental prepare trailer"))?;
    let mutation_offset = prepare_offset
        .checked_add(TRANSACTION_PREPARE_HEADER_LEN_V1)
        .ok_or(RecoveryError::Arithmetic("incremental mutation offset"))?;
    let mut crc = Crc32c::new();
    crc.update(prefix);
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(prefix);
    let mut state = Box::new(IncrementalPrimaryStateV1 {
        retention_reader,
        cursor: mutation_offset,
        crc,
        hasher,
        pending_error: None,
        transcript: IncrementalLayoutTranscriptV1::new(),
        previous: None,
        observed_object_bytes: 0,
        projected_index_records: current_index_records,
        projected_index_tail: current_index_tail,
    });
    scan_incremental_mutation_primary(
        reader,
        mutation_offset,
        trailer_offset,
        preflight,
        limits,
        wal_limits,
        scratch,
        accounting,
        &mut state,
    )?;
    let objects_offset = state.cursor;
    scan_incremental_objects_primary(
        reader,
        pack_id,
        prepare_offset,
        mutation_offset,
        trailer_offset,
        preflight,
        generation_start,
        limits,
        wal_limits,
        index_sink,
        scratch,
        accounting,
        &mut state,
    )?;
    finish_incremental_prepare_primary(
        reader,
        prepare_offset,
        mutation_offset,
        trailer_offset,
        preflight,
        limits,
        scratch,
        accounting,
        state,
        objects_offset,
    )
}

fn verify_incremental_source_replay<R: DurableReadAt>(
    reader: &R,
    prepare_offset: u64,
    primary: &IncrementalPrimaryScanV1,
    limits: DurableLimits,
    scratch: &mut [u8],
    accounting: &mut RecoveryReadAccountingV1,
) -> Result<(), RecoveryError<R::Error>> {
    let expected_bytes = primary
        .identity
        .record_len()
        .checked_sub(CHECKSUM_FIELD_LEN as u64)
        .ok_or(RecoveryError::Arithmetic("incremental replay extent"))?;
    let replay_end = prepare_offset
        .checked_add(expected_bytes)
        .ok_or(RecoveryError::Arithmetic("incremental replay end"))?;
    let mut cursor = prepare_offset;
    let mut crc = Crc32c::new();
    let mut hasher = Blake2b::<U32>::new();
    while cursor < replay_end {
        let width = usize::try_from((replay_end - cursor).min(scratch.len() as u64))
            .map_err(|_| RecoveryError::Arithmetic("incremental source replay chunk"))?;
        let chunk = &mut scratch[..width];
        read_exact_accounted(
            reader,
            cursor,
            chunk,
            limits,
            accounting,
            RecoveryReadClassV1::SourceAuthenticationReplay,
        )?;
        crc.update(chunk);
        hasher.update(&*chunk);
        cursor = cursor
            .checked_add(width as u64)
            .ok_or(RecoveryError::Arithmetic("incremental source replay cursor"))?;
    }
    let actual_bytes = cursor - prepare_offset;
    let actual_crc32c = crc.finish();
    let actual_digest = finish_physical_digest(hasher);
    if actual_bytes != expected_bytes ||
        actual_crc32c != primary.identity.crc32c() ||
        actual_digest != primary.identity.digest()
    {
        return Err(RecoveryError::IncrementalReplaySourceChanged {
            expected_digest: primary.identity.digest(),
            actual_digest,
            expected_crc32c: primary.identity.crc32c(),
            actual_crc32c,
            expected_bytes,
            actual_bytes,
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_incremental_layout<R, S>(
    reader: &R,
    pack_id: PackId,
    prepare_offset: u64,
    sequence: LogPackSequence,
    generation_start: bool,
    primary: &IncrementalPrimaryScanV1,
    limits: DurableLimits,
    accounting: &mut RecoveryReadAccountingV1,
    index_sink: &mut S,
    index_records: &mut u64,
    expected_index_tail: &mut u64,
) -> Result<(), RecoveryError<R::Error>>
where
    R: DurableReadAt,
    S: RecoveryIndexSinkV1<R::Error>,
{
    let mut cursor = primary.objects_offset;
    let mut transcript = IncrementalLayoutTranscriptV1::new();
    for _ in 0..primary.layout.object_count {
        let relative_record_offset = cursor
            .checked_sub(prepare_offset)
            .ok_or(RecoveryError::Arithmetic("layout relative record offset"))?;
        let mut raw_header = [0u8; OBJECT_RECORD_HEADER_LEN_V1 as usize];
        read_exact_accounted(
            reader,
            cursor,
            &mut raw_header,
            limits,
            accounting,
            RecoveryReadClassV1::LayoutApplicationReplay,
        )?;
        cursor = cursor
            .checked_add(OBJECT_RECORD_HEADER_LEN_V1)
            .ok_or(RecoveryError::Arithmetic("layout body offset"))?;
        let header = LogPackObjectHeaderV1::parse_exact(&raw_header)?;
        let disposition = header.disposition();
        let reference = header.reference();
        let relative_canonical_offset = match disposition {
            LogPackObjectDispositionV1::Data => {
                let relative = cursor
                    .checked_sub(prepare_offset)
                    .ok_or(RecoveryError::Arithmetic("layout relative canonical offset"))?;
                let record = ObjectIndexRecordV1::from_streamed_data(
                    pack_id,
                    prepare_offset,
                    sequence,
                    reference,
                    relative,
                )?;
                index_sink.data(record)?;
                *index_records = index_records
                    .checked_add(1)
                    .ok_or(RecoveryError::Arithmetic("applied index record count"))?;
                *expected_index_tail = expected_index_tail
                    .checked_add(INDEX_RECORD_LEN_V1)
                    .ok_or(RecoveryError::Arithmetic("applied index tail"))?;
                cursor = cursor
                    .checked_add(reference.encoded_len())
                    .ok_or(RecoveryError::Arithmetic("layout DATA extent"))?;
                Some(relative)
            }
            LogPackObjectDispositionV1::Reuse if generation_start => {
                return Err(RecoveryError::GenerationCheckpointReuse(reference));
            }
            LogPackObjectDispositionV1::Reuse => {
                index_sink.reuse(reference, sequence)?;
                None
            }
        };
        let relative_final_extent = cursor
            .checked_sub(prepare_offset)
            .ok_or(RecoveryError::Arithmetic("layout relative final extent"))?;
        transcript
            .push(
                disposition,
                reference,
                relative_record_offset,
                relative_canonical_offset,
                relative_final_extent,
            )
            .map_err(RecoveryError::Arithmetic)?;
    }
    if cursor != primary.trailer_offset {
        return Err(RecoveryError::IncrementalReplayTranscript {
            expected: primary.layout,
            actual: transcript.finish(
                cursor
                    .checked_sub(prepare_offset)
                    .ok_or(RecoveryError::Arithmetic("layout replay final extent"))?,
            ),
        });
    }
    let actual = transcript.finish(
        cursor
            .checked_sub(prepare_offset)
            .ok_or(RecoveryError::Arithmetic("layout replay final extent"))?,
    );
    if actual != primary.layout {
        return Err(RecoveryError::IncrementalReplayTranscript {
            expected: primary.layout,
            actual,
        });
    }
    debug_assert_eq!(*index_records, primary.projected_index_records);
    debug_assert_eq!(*expected_index_tail, primary.projected_index_tail);
    Ok(())
}

trait RecoveryIndexSinkV1<E> {
    type FinalIndex;

    fn validate_data(
        &self,
        record: ObjectIndexRecordV1,
    ) -> Result<(), RecoveryIndexValidationErrorV1>;

    fn validate_reuse(
        &self,
        reference: crate::rspace::persistence::ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryIndexValidationErrorV1>;

    fn data(&mut self, record: ObjectIndexRecordV1) -> Result<(), RecoveryError<E>>;

    fn reuse(
        &mut self,
        reference: crate::rspace::persistence::ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryError<E>>;

    fn finish(self) -> Result<Self::FinalIndex, RecoveryError<E>>;
}

#[derive(Debug)]
enum RecoveryIndexValidationErrorV1 {
    Index(IndexFormatError),
    MissingReuse(ObjectRefV1),
    ReuseReference {
        indexed: ObjectRefV1,
        reused: ObjectRefV1,
    },
    ReuseNotPrior {
        reference: ObjectRefV1,
        indexed_sequence: LogPackSequence,
        current_sequence: LogPackSequence,
    },
    ReplayDataRecord {
        expected: Option<ObjectIndexRecordV1>,
        actual: ObjectIndexRecordV1,
    },
}

impl RecoveryIndexValidationErrorV1 {
    fn into_recovery<E>(self) -> RecoveryError<E> {
        match self {
            Self::Index(error) => RecoveryError::Index(error),
            Self::MissingReuse(reference) => RecoveryError::MissingReuse(reference),
            Self::ReuseReference { indexed, reused } => {
                RecoveryError::ReuseReference { indexed, reused }
            }
            Self::ReuseNotPrior {
                reference,
                indexed_sequence,
                current_sequence,
            } => RecoveryError::ReuseNotPrior {
                reference,
                indexed_sequence,
                current_sequence,
            },
            Self::ReplayDataRecord { expected, actual } => {
                RecoveryError::ReplayDataRecord { expected, actual }
            }
        }
    }
}

fn verify_prior_reuse(
    indexed: Option<ObjectIndexRecordV1>,
    reference: crate::rspace::persistence::ObjectRefV1,
    current_sequence: LogPackSequence,
) -> Result<(), RecoveryIndexValidationErrorV1> {
    let Some(indexed) = indexed else {
        return Err(RecoveryIndexValidationErrorV1::MissingReuse(reference));
    };
    if indexed.reference() != reference {
        return Err(RecoveryIndexValidationErrorV1::ReuseReference {
            indexed: indexed.reference(),
            reused: reference,
        });
    }
    let indexed_sequence = indexed.location().sequence();
    if indexed_sequence >= current_sequence {
        return Err(RecoveryIndexValidationErrorV1::ReuseNotPrior {
            reference,
            indexed_sequence,
            current_sequence,
        });
    }
    Ok(())
}

struct BuildingIndexSinkV1 {
    index: CommittedObjectIndexV1,
}

impl<E> RecoveryIndexSinkV1<E> for BuildingIndexSinkV1 {
    type FinalIndex = CommittedObjectIndexV1;

    fn validate_data(
        &self,
        record: ObjectIndexRecordV1,
    ) -> Result<(), RecoveryIndexValidationErrorV1> {
        self.index
            .validate_insert(record)
            .map_err(RecoveryIndexValidationErrorV1::Index)
    }

    fn validate_reuse(
        &self,
        reference: ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryIndexValidationErrorV1> {
        verify_prior_reuse(self.index.get(reference.digest()), reference, current_sequence)
    }

    fn data(&mut self, record: ObjectIndexRecordV1) -> Result<(), RecoveryError<E>> {
        self.index.insert(record)?;
        Ok(())
    }

    fn reuse(
        &mut self,
        reference: crate::rspace::persistence::ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryError<E>> {
        verify_prior_reuse(self.index.get(reference.digest()), reference, current_sequence)
            .map_err(RecoveryIndexValidationErrorV1::into_recovery)
    }

    fn finish(self) -> Result<Self::FinalIndex, RecoveryError<E>> { Ok(self.index) }
}

struct ReplayingIndexSinkV1<'authority> {
    authority: &'authority CommittedObjectIndexV1,
    data_records: u64,
}

struct StagingIndexSinkV1<'authority, F> {
    authority: &'authority CommittedObjectIndexV1,
    data_records: u64,
    stage_data: F,
}

impl<E, F> RecoveryIndexSinkV1<E> for StagingIndexSinkV1<'_, F>
where F: FnMut(ObjectIndexRecordV1) -> Result<(), RecoveryError<E>>
{
    type FinalIndex = ();

    fn validate_data(
        &self,
        record: ObjectIndexRecordV1,
    ) -> Result<(), RecoveryIndexValidationErrorV1> {
        let expected = self.authority.get(record.reference().digest());
        if expected != Some(record) {
            return Err(RecoveryIndexValidationErrorV1::ReplayDataRecord {
                expected,
                actual: record,
            });
        }
        Ok(())
    }

    fn validate_reuse(
        &self,
        reference: ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryIndexValidationErrorV1> {
        verify_prior_reuse(self.authority.get(reference.digest()), reference, current_sequence)
    }

    fn data(&mut self, record: ObjectIndexRecordV1) -> Result<(), RecoveryError<E>> {
        let expected = self.authority.get(record.reference().digest());
        if expected != Some(record) {
            return Err(RecoveryIndexValidationErrorV1::ReplayDataRecord {
                expected,
                actual: record,
            }
            .into_recovery());
        }
        self.data_records = self
            .data_records
            .checked_add(1)
            .ok_or(RecoveryError::Arithmetic("staged DATA record count"))?;
        (self.stage_data)(record)
    }

    fn reuse(
        &mut self,
        reference: ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryError<E>> {
        verify_prior_reuse(self.authority.get(reference.digest()), reference, current_sequence)
            .map_err(RecoveryIndexValidationErrorV1::into_recovery)
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

impl<E> RecoveryIndexSinkV1<E> for ReplayingIndexSinkV1<'_> {
    type FinalIndex = ();

    fn validate_data(
        &self,
        record: ObjectIndexRecordV1,
    ) -> Result<(), RecoveryIndexValidationErrorV1> {
        let expected = self.authority.get(record.reference().digest());
        if expected != Some(record) {
            return Err(RecoveryIndexValidationErrorV1::ReplayDataRecord {
                expected,
                actual: record,
            });
        }
        Ok(())
    }

    fn validate_reuse(
        &self,
        reference: ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryIndexValidationErrorV1> {
        verify_prior_reuse(self.authority.get(reference.digest()), reference, current_sequence)
    }

    fn data(&mut self, record: ObjectIndexRecordV1) -> Result<(), RecoveryError<E>> {
        let expected = self.authority.get(record.reference().digest());
        if expected != Some(record) {
            return Err(RecoveryIndexValidationErrorV1::ReplayDataRecord {
                expected,
                actual: record,
            }
            .into_recovery());
        }
        self.data_records = self
            .data_records
            .checked_add(1)
            .ok_or(RecoveryError::Arithmetic("replayed DATA record count"))?;
        Ok(())
    }

    fn reuse(
        &mut self,
        reference: crate::rspace::persistence::ObjectRefV1,
        current_sequence: LogPackSequence,
    ) -> Result<(), RecoveryError<E>> {
        verify_prior_reuse(self.authority.get(reference.digest()), reference, current_sequence)
            .map_err(RecoveryIndexValidationErrorV1::into_recovery)
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
    retention_reader: RetentionReader,
    object_records: &mut u64,
    index_records: &mut u64,
    expected_index_tail: &mut u64,
    index_sink: &mut S,
    scratch: &mut [u8],
    accounting: &mut RecoveryReadAccountingV1,
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
    let (retention_role, retention_kind) = designated_retention_object(descriptor.format());
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
        read_exact_accounted(
            reader,
            cursor,
            &mut reference_bytes,
            limits,
            accounting,
            RecoveryReadClassV1::PrimaryAuthenticated,
        )?;
        record_crc.update(&reference_bytes);
        record_hasher.update(reference_bytes);
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
            read_exact_accounted(
                reader,
                body_cursor,
                chunk,
                limits,
                accounting,
                RecoveryReadClassV1::PrimaryAuthenticated,
            )?;
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
        let kind = CanonicalObjectKind::from_domain_prefix_for(
            &domain[..captured_domain_len],
            retention_reader,
        )
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
            require_checkpoint_kind(retention_role, reference, kind, retention_kind)?;
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
    read_exact_accounted(
        reader,
        trailer_offset,
        &mut trailer,
        limits,
        accounting,
        RecoveryReadClassV1::PrimaryAuthenticated,
    )?;
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
            role: retention_role,
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
    limits: DurableLimits,
    accounting: &mut RecoveryReadAccountingV1,
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
    read_exact_accounted(
        reader,
        commit_offset,
        &mut discriminator,
        limits,
        accounting,
        RecoveryReadClassV1::PrimaryAuthenticated,
    )?;
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
pub(crate) fn scan_catalog_authorized_pack_chain<P>(
    provider: &mut P,
    authority: &CatalogAuthorityTokenV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
) -> Result<ScannedPackChainV1, RecoveryError<P::Error>>
where
    P: CatalogPackProviderV1,
{
    scan_catalog_authorized_pack_chain_with_sink(
        provider,
        authority,
        limits,
        wal_limits,
        BuildingIndexSinkV1 {
            index: CommittedObjectIndexV1::new(),
        },
    )
}

/// Re-scan an exact catalog-authorized chain against the first pass's
/// recovered index without allocating another PathMap. DATA records must
/// reproduce the authoritative locations exactly and REUSE may only resolve
/// strictly backward to an earlier committed transaction.
pub(crate) fn replay_catalog_authorized_pack_chain<P>(
    provider: &mut P,
    authority: &CatalogAuthorityTokenV1,
    authoritative_index: &CommittedObjectIndexV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
) -> Result<ScannedPackChainV1<()>, RecoveryError<P::Error>>
where
    P: CatalogPackProviderV1,
{
    scan_catalog_authorized_pack_chain_with_sink(
        provider,
        authority,
        limits,
        wal_limits,
        ReplayingIndexSinkV1 {
            authority: authoritative_index,
            data_records: 0,
        },
    )
}

/// Revalidate a previously catalog-verified chain and stage its DATA records
/// in canonical source order into unpublished, rollback-capable state.
///
/// The callback is intentionally unavailable on the initial authority scan.
/// It may only mutate private state whose publication occurs after the
/// returned scan is checked against a freshly read catalog token. A callback
/// failure or any later replay failure may leave a staged prefix, which the
/// caller must discard.
pub(crate) fn stage_catalog_verified_data_in_source_order<P, F>(
    provider: &mut P,
    recovery: &CatalogVerifiedRecoveryV1,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    stage_data: F,
) -> Result<ScannedPackChainV1<()>, RecoveryError<P::Error>>
where
    P: CatalogPackProviderV1,
    F: FnMut(ObjectIndexRecordV1) -> Result<(), RecoveryError<P::Error>>,
{
    scan_catalog_authorized_pack_chain_with_sink(
        provider,
        recovery.authority(),
        limits,
        wal_limits,
        StagingIndexSinkV1 {
            authority: recovery.object_index(),
            data_records: 0,
            stage_data,
        },
    )
}

struct ScannedRecordV1 {
    prepare_len: u64,
    prepare_sequence: LogPackSequence,
    prepare_store: StoreId,
    prepare_profile: SemanticProfileId,
    prepare_base: Option<StateVersionV1>,
    target_state: StateVersionV1,
    commit: TerminalCommitViewV1,
    commit_len: u64,
    commit_expected_index_tail: u64,
    pending_incremental: Option<CommitBoundIncrementalV1>,
}

struct ChainBoundRecordV1 {
    scanned: ScannedRecordV1,
    prepare_end: u64,
}

#[derive(Clone, Copy)]
struct RecordChainExpectationV1 {
    expected_sequence: LogPackSequence,
    expected_store: StoreId,
    expected_profile: SemanticProfileId,
    previous_state: Option<StateVersionV1>,
    previous_commit: PhysicalDigest,
    first_live_pack: PackId,
    index_generation: u64,
}

#[inline(never)]
fn bind_record_to_chain_v1<E>(
    pack_id: PackId,
    offset: u64,
    expectation: RecordChainExpectationV1,
    scanned: ScannedRecordV1,
) -> Result<ChainBoundRecordV1, RecoveryError<E>> {
    if scanned.prepare_sequence != expectation.expected_sequence {
        return Err(RecoveryError::Sequence {
            expected: expectation.expected_sequence,
            actual: scanned.prepare_sequence,
        });
    }
    if scanned.prepare_store != expectation.expected_store {
        return Err(RecoveryError::Store {
            expected: expectation.expected_store,
            actual: scanned.prepare_store,
        });
    }
    if scanned.prepare_profile != expectation.expected_profile {
        return Err(RecoveryError::Profile {
            expected: expectation.expected_profile,
            actual: scanned.prepare_profile,
        });
    }
    if let (Some(expected_base), Some(actual_base)) =
        (expectation.previous_state, scanned.prepare_base)
    {
        if actual_base != expected_base {
            return Err(RecoveryError::StateContinuity {
                expected: expected_base,
                actual: actual_base,
            });
        }
    }
    if scanned.commit.previous_commit() != expectation.previous_commit {
        return Err(RecoveryError::PreviousCommit {
            expected: expectation.previous_commit,
            actual: scanned.commit.previous_commit(),
        });
    }
    let prepare_end = offset
        .checked_add(scanned.prepare_len)
        .ok_or(RecoveryError::Arithmetic("prepare end"))?;
    let expected_prepare_address = LogPackAddress::from_pack(pack_id, offset, scanned.prepare_len)?;
    if scanned.commit.prepare_address() != expected_prepare_address {
        return Err(RecoveryError::PrepareAddress {
            expected: expected_prepare_address,
            actual: scanned.commit.prepare_address(),
        });
    }
    if scanned.commit.publication().first_live_pack() != expectation.first_live_pack.get() {
        return Err(RecoveryError::PublicationFirstLive {
            expected: expectation.first_live_pack.get(),
            actual: scanned.commit.publication().first_live_pack(),
        });
    }
    if scanned.commit.publication().index_generation() != expectation.index_generation {
        return Err(RecoveryError::PublicationIndexGeneration {
            expected: expectation.index_generation,
            actual: scanned.commit.publication().index_generation(),
        });
    }
    if scanned.commit.durable_index_tail() != scanned.commit_expected_index_tail {
        return Err(RecoveryError::IndexTail {
            expected: scanned.commit_expected_index_tail,
            actual: scanned.commit.durable_index_tail(),
        });
    }
    Ok(ChainBoundRecordV1 {
        scanned,
        prepare_end,
    })
}

#[allow(clippy::too_many_arguments)]
fn scan_incremental_record_v1<R, S>(
    reader: &R,
    pack_id: PackId,
    offset: u64,
    durable_tail: u64,
    discriminator_end: u64,
    discriminator: &[u8; PREPARE_DISCRIMINATOR_LEN_V1 as usize],
    generation_start: bool,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    retention_reader: RetentionReader,
    index_sink: &S,
    index_records: u64,
    expected_index_tail: u64,
    scratch: &mut Option<Vec<u8>>,
    accounting: &mut RecoveryReadAccountingV1,
    transactions: &mut u64,
    object_records: &mut u64,
) -> Result<ScannedRecordV1, RecoveryError<R::Error>>
where
    R: DurableReadAt,
    S: RecoveryIndexSinkV1<R::Error>,
{
    *transactions = limits.checked_charge(DurableLimitKind::Transactions, *transactions, 1)?;
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
    prefix[..PREPARE_DISCRIMINATOR_LEN_V1 as usize].copy_from_slice(discriminator);
    read_exact_accounted(
        reader,
        discriminator_end,
        &mut prefix[PREPARE_DISCRIMINATOR_LEN_V1 as usize..],
        limits,
        accounting,
        RecoveryReadClassV1::PrimaryAuthenticated,
    )?;
    let preflight = preflight_prepare_record_v1(&prefix, wal_limits)?;
    *object_records = limits.checked_charge(
        DurableLimitKind::ObjectRecords,
        *object_records,
        preflight.object_count(),
    )?;
    let prepare_len = preflight.record_len();
    let prepare_end = offset
        .checked_add(prepare_len)
        .ok_or(RecoveryError::Arithmetic("incremental prepare end"))?;
    let commit_discriminator = require_commit_pair(
        reader,
        pack_id,
        prepare_end,
        durable_tail,
        PrepareRecordKindV1::Incremental,
        CommitRecordKindV1::Incremental,
        limits,
        accounting,
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

    if scratch.is_none() {
        let mut allocated = Vec::new();
        allocated
            .try_reserve_exact(MUTATION_STREAM_MAX_SCRATCH_BYTES_V1)
            .map_err(|_| RecoveryError::CheckpointScratchAllocation {
                bytes: MUTATION_STREAM_MAX_SCRATCH_BYTES_V1 as u64,
            })?;
        allocated.resize(MUTATION_STREAM_MAX_SCRATCH_BYTES_V1, 0);
        *scratch = Some(allocated);
    }
    let primary = scan_incremental_prepare_primary_phased(
        reader,
        pack_id,
        offset,
        &prefix,
        preflight,
        generation_start,
        limits,
        wal_limits,
        retention_reader,
        index_sink,
        index_records,
        expected_index_tail,
        scratch
            .as_deref_mut()
            .expect("incremental branch initialized its bounded scratch"),
        accounting,
    )?;
    let mut raw_commit = [0u8; TRANSACTION_COMMIT_RECORD_LEN_V1 as usize];
    raw_commit[..COMMIT_DISCRIMINATOR_LEN_V1 as usize].copy_from_slice(&commit_discriminator);
    let commit_body_offset = prepare_end
        .checked_add(COMMIT_DISCRIMINATOR_LEN_V1)
        .ok_or(RecoveryError::Arithmetic("incremental commit body offset"))?;
    read_exact_accounted(
        reader,
        commit_body_offset,
        &mut raw_commit[COMMIT_DISCRIMINATOR_LEN_V1 as usize..],
        limits,
        accounting,
        RecoveryReadClassV1::PrimaryAuthenticated,
    )?;
    let incremental_commit = TransactionCommitViewV1::parse(&raw_commit)?;
    incremental_commit.verify_prepare_identity(primary.identity)?;
    let receipt = primary.identity.receipt();
    Ok(ScannedRecordV1 {
        prepare_len,
        prepare_sequence: primary.identity.sequence(),
        prepare_store: primary.identity.store_id(),
        prepare_profile: receipt.profile_id(),
        prepare_base: Some(receipt.base()),
        target_state: receipt.target(),
        commit: TerminalCommitViewV1::Incremental(incremental_commit),
        commit_len: TRANSACTION_COMMIT_RECORD_LEN_V1,
        commit_expected_index_tail: primary.projected_index_tail,
        pending_incremental: Some(CommitBoundIncrementalV1 { primary }),
    })
}

#[allow(clippy::too_many_arguments)]
fn scan_generation_checkpoint_record_v1<R, S>(
    reader: &R,
    pack_id: PackId,
    offset: u64,
    durable_tail: u64,
    discriminator_end: u64,
    discriminator: &[u8; PREPARE_DISCRIMINATOR_LEN_V1 as usize],
    first_live_pack: PackId,
    expected_sequence: LogPackSequence,
    expected_store: StoreId,
    expected_profile: SemanticProfileId,
    index_generation: u64,
    previous_commit: PhysicalDigest,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    retention_reader: RetentionReader,
    scratch: &mut Option<Vec<u8>>,
    accounting: &mut RecoveryReadAccountingV1,
    index_sink: &mut S,
    checkpoints: &mut u64,
    object_records: &mut u64,
    index_records: &mut u64,
    expected_index_tail: &mut u64,
) -> Result<ScannedRecordV1, RecoveryError<R::Error>>
where
    R: DurableReadAt,
    S: RecoveryIndexSinkV1<R::Error>,
{
    *checkpoints = checkpoints
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
    prefix[..PREPARE_DISCRIMINATOR_LEN_V1 as usize].copy_from_slice(discriminator);
    read_exact_accounted(
        reader,
        discriminator_end,
        &mut prefix[PREPARE_DISCRIMINATOR_LEN_V1 as usize..],
        limits,
        accounting,
        RecoveryReadClassV1::PrimaryAuthenticated,
    )?;
    let preflight =
        preflight_generation_checkpoint_prepare_for(&prefix, wal_limits, retention_reader)?;
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
        reader,
        pack_id,
        prepare_end,
        durable_tail,
        PrepareRecordKindV1::GenerationCheckpoint,
        CommitRecordKindV1::GenerationCheckpoint,
        limits,
        accounting,
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
    raw_commit[..COMMIT_DISCRIMINATOR_LEN_V1 as usize].copy_from_slice(&commit_discriminator);
    let commit_body_offset = prepare_end
        .checked_add(COMMIT_DISCRIMINATOR_LEN_V1)
        .ok_or(RecoveryError::Arithmetic("checkpoint commit body offset"))?;
    read_exact_accounted(
        reader,
        commit_body_offset,
        &mut raw_commit[COMMIT_DISCRIMINATOR_LEN_V1 as usize..],
        limits,
        accounting,
        RecoveryReadClassV1::PrimaryAuthenticated,
    )?;
    let checkpoint_commit =
        GenerationCheckpointCommitViewV1::parse_for(&raw_commit, retention_reader)?;
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
        return Err(RecoveryError::Wal(LogPackFormatError::CheckpointDescriptorMismatch));
    }
    if checkpoint_commit.previous_commit() != previous_commit {
        return Err(RecoveryError::PreviousCommit {
            expected: previous_commit,
            actual: checkpoint_commit.previous_commit(),
        });
    }
    let expected_prepare_address = LogPackAddress::from_pack(pack_id, offset, prepare_len)?;
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
        *object_records,
        preflight.descriptor().data_objects(),
    )?;
    let checkpoint_index_records = index_records
        .checked_add(preflight.descriptor().data_objects())
        .ok_or(RecoveryError::Arithmetic("checkpoint projected index record count"))?;
    limits.require(DurableLimitKind::IndexRecords, checkpoint_index_records)?;
    limits.require(DurableLimitKind::IndexBytes, checkpoint_index_tail)?;
    if checkpoint_commit.publication().durable_index_tail() != checkpoint_index_tail {
        return Err(RecoveryError::IndexTail {
            expected: checkpoint_index_tail,
            actual: checkpoint_commit.publication().durable_index_tail(),
        });
    }
    if scratch.is_none() {
        let mut allocated = Vec::new();
        allocated
            .try_reserve_exact(CHECKPOINT_STREAM_SCRATCH_LEN)
            .map_err(|_| RecoveryError::CheckpointScratchAllocation {
                bytes: CHECKPOINT_STREAM_SCRATCH_LEN as u64,
            })?;
        allocated.resize(CHECKPOINT_STREAM_SCRATCH_LEN, 0);
        *scratch = Some(allocated);
    }
    let identity = stream_generation_checkpoint_prepare(
        reader,
        pack_id,
        offset,
        &prefix,
        preflight,
        limits,
        wal_limits,
        retention_reader,
        object_records,
        index_records,
        expected_index_tail,
        index_sink,
        scratch
            .as_deref_mut()
            .expect("checkpoint branch initialized its bounded scratch"),
        accounting,
    )?;
    checkpoint_commit.verify_prepare(identity)?;
    Ok(ScannedRecordV1 {
        prepare_len,
        prepare_sequence: identity.sequence(),
        prepare_store: identity.store_id(),
        prepare_profile: identity.descriptor().profile_id(),
        prepare_base: None,
        target_state: identity.descriptor().state(),
        commit: TerminalCommitViewV1::GenerationCheckpoint(checkpoint_commit),
        commit_len: GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
        commit_expected_index_tail: *expected_index_tail,
        pending_incremental: None,
    })
}

struct DispatchedRecordV1 {
    scanned: ScannedRecordV1,
    generation_start: bool,
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn scan_commit_bound_record_v1<R, S>(
    reader: &R,
    pack_id: PackId,
    offset: u64,
    durable_tail: u64,
    first_live_pack: PackId,
    expected_sequence: LogPackSequence,
    expected_store: StoreId,
    expected_profile: SemanticProfileId,
    index_generation: u64,
    previous_commit: PhysicalDigest,
    limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    retention_reader: RetentionReader,
    scratch: &mut Option<Vec<u8>>,
    accounting: &mut RecoveryReadAccountingV1,
    index_sink: &mut S,
    transactions: &mut u64,
    checkpoints: &mut u64,
    object_records: &mut u64,
    index_records: &mut u64,
    expected_index_tail: &mut u64,
) -> Result<DispatchedRecordV1, RecoveryError<R::Error>>
where
    R: DurableReadAt,
    S: RecoveryIndexSinkV1<R::Error>,
{
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
    read_exact_accounted(
        reader,
        offset,
        &mut discriminator,
        limits,
        accounting,
        RecoveryReadClassV1::PrimaryAuthenticated,
    )?;
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
    let scanned = match prepare_kind {
        PrepareRecordKindV1::Incremental => scan_incremental_record_v1(
            reader,
            pack_id,
            offset,
            durable_tail,
            discriminator_end,
            &discriminator,
            generation_start,
            limits,
            wal_limits,
            retention_reader,
            &*index_sink,
            *index_records,
            *expected_index_tail,
            scratch,
            accounting,
            transactions,
            object_records,
        )?,
        PrepareRecordKindV1::GenerationCheckpoint => scan_generation_checkpoint_record_v1(
            reader,
            pack_id,
            offset,
            durable_tail,
            discriminator_end,
            &discriminator,
            first_live_pack,
            expected_sequence,
            expected_store,
            expected_profile,
            index_generation,
            previous_commit,
            limits,
            wal_limits,
            retention_reader,
            scratch,
            accounting,
            index_sink,
            checkpoints,
            object_records,
            index_records,
            expected_index_tail,
        )?,
    };
    Ok(DispatchedRecordV1 {
        scanned,
        generation_start,
    })
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
    let retention_reader = provider.retention_reader();
    let mut transactions = 0u64;
    let mut checkpoints = 0u64;
    let mut object_records = 0u64;
    let mut index_records = 0u64;
    let mut scanned_bytes = 0u64;
    let mut accounting = RecoveryReadAccountingV1::default();
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
        read_exact_accounted(
            &reader,
            0,
            &mut header_bytes,
            limits,
            &mut accounting,
            RecoveryReadClassV1::PrimaryAuthenticated,
        )?;
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
            let DispatchedRecordV1 {
                scanned,
                generation_start,
            } = scan_commit_bound_record_v1(
                &reader,
                pack_id,
                offset,
                durable_tail,
                first_live_pack,
                expected_sequence,
                expected_store,
                expected_profile,
                index_generation,
                previous_commit,
                limits,
                wal_limits,
                retention_reader,
                &mut checkpoint_scratch,
                &mut accounting,
                &mut index_sink,
                &mut transactions,
                &mut checkpoints,
                &mut object_records,
                &mut index_records,
                &mut expected_index_tail,
            )?;

            let ChainBoundRecordV1 {
                scanned,
                prepare_end,
            } = bind_record_to_chain_v1(
                pack_id,
                offset,
                RecordChainExpectationV1 {
                    expected_sequence,
                    expected_store,
                    expected_profile,
                    previous_state,
                    previous_commit,
                    first_live_pack,
                    index_generation,
                },
                scanned,
            )?;
            let ScannedRecordV1 {
                prepare_sequence,
                target_state,
                commit,
                commit_len,
                pending_incremental,
                ..
            } = scanned;

            if let Some(pending) = pending_incremental {
                let bound = pending.bind_chain();
                verify_incremental_source_replay(
                    &reader,
                    offset,
                    &bound.primary,
                    limits,
                    checkpoint_scratch
                        .as_deref_mut()
                        .expect("incremental branch retained its bounded scratch"),
                    &mut accounting,
                )?;
                apply_incremental_layout(
                    &reader,
                    pack_id,
                    offset,
                    prepare_sequence,
                    generation_start,
                    &bound.primary,
                    limits,
                    &mut accounting,
                    &mut index_sink,
                    &mut index_records,
                    &mut expected_index_tail,
                )?;
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
        read_work: accounting.report(scanned_bytes),
    })
}
