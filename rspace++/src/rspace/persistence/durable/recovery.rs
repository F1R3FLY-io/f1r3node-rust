//! Iterative recovery of catalog-authorized pack prefixes.
//!
//! The scanner never searches for a later valid record after an error. Every
//! pack is consumed as one exact header followed by adjacent prepare/commit
//! pairs, and every variable read is preceded by authenticated fixed-prefix
//! preflight.

use std::fmt::{Display, Formatter};

use super::{
    CommittedObjectIndexV1, DurableFormatError, DurableLimitError, DurableLimitKind, DurableLimits,
    INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexFormatError, ObjectIndexRecordV1,
    PACK_HEADER_LEN_V1, PackFormatError, PackHeaderV1, PackTerminalIdentityV1,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CatalogAuthorityTokenV1, LogPackAddress, LogPackDecodeLimits,
    LogPackFormatError, LogPackObjectDispositionV1, LogPackSequence, PackId, PhysicalDigest,
    SemanticProfileId, StateVersionV1, StoreId, TRANSACTION_COMMIT_RECORD_LEN_V1,
    TRANSACTION_PREPARE_HEADER_LEN_V1, TransactionCommitViewV1, TransactionPrepareRecordViewV1,
    preflight_prepare_record_v1,
};

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
    terminal_commit_bytes: [u8; TRANSACTION_COMMIT_RECORD_LEN_V1 as usize],
    index: I,
    expected_index_tail: u64,
    transactions: u64,
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
        let commit = TransactionCommitViewV1::parse(&self.terminal_commit_bytes)?;
        catalog
            .verify_commit(commit)
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

    pub(crate) const fn object_records(&self) -> u64 { self.scan.object_records }

    pub(crate) const fn scanned_bytes(&self) -> u64 { self.scan.scanned_bytes }

    pub(crate) const fn authority(&self) -> &CatalogAuthorityTokenV1 { &self.authority }
}

impl CatalogVerifiedRecoveryV1<CommittedObjectIndexV1> {
    pub(crate) fn object_index(&self) -> &CommittedObjectIndexV1 { &self.scan.index }
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
    let mut object_records = 0u64;
    let mut index_records = 0u64;
    let mut scanned_bytes = 0u64;
    let mut expected_index_tail = INDEX_HEADER_LEN_V1;
    limits.require(DurableLimitKind::IndexBytes, expected_index_tail)?;
    let mut previous_terminal = None;
    let mut previous_state = None;
    let mut terminal_address = None;
    let mut terminal_commit_bytes = None;
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
            transactions =
                limits.checked_charge(DurableLimitKind::Transactions, transactions, 1)?;
            let prefix_end = offset
                .checked_add(TRANSACTION_PREPARE_HEADER_LEN_V1)
                .ok_or(RecoveryError::Arithmetic("prepare prefix end"))?;
            if prefix_end > durable_tail {
                return Err(RecoveryError::AuthorizedTailCutsRecord {
                    pack: pack_id,
                    record_offset: offset,
                    required_end: prefix_end,
                    durable_tail,
                });
            }
            let mut prefix = [0u8; TRANSACTION_PREPARE_HEADER_LEN_V1 as usize];
            reader
                .read_exact_at(offset, &mut prefix)
                .map_err(RecoveryError::Read)?;
            let preflight = preflight_prepare_record_v1(&prefix, wal_limits)?;
            // Prefix authentication precedes this charge; allocation and the
            // remaining read follow it. No body work occurs if the aggregate
            // budget rejects the declared count.
            object_records = limits.checked_charge(
                DurableLimitKind::ObjectRecords,
                object_records,
                preflight.object_count(),
            )?;
            let prepare_len = preflight.record_len();
            let prepare_end = offset
                .checked_add(prepare_len)
                .ok_or(RecoveryError::Arithmetic("prepare end"))?;
            let commit_end = prepare_end
                .checked_add(TRANSACTION_COMMIT_RECORD_LEN_V1)
                .ok_or(RecoveryError::Arithmetic("commit end"))?;
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
            prepare_bytes[..TRANSACTION_PREPARE_HEADER_LEN_V1 as usize].copy_from_slice(&prefix);
            reader
                .read_exact_at(
                    prefix_end,
                    &mut prepare_bytes[TRANSACTION_PREPARE_HEADER_LEN_V1 as usize..],
                )
                .map_err(RecoveryError::Read)?;
            let prepare = TransactionPrepareRecordViewV1::parse(&prepare_bytes, wal_limits)?;
            if prepare.sequence() != expected_sequence {
                return Err(RecoveryError::Sequence {
                    expected: expected_sequence,
                    actual: prepare.sequence(),
                });
            }
            if prepare.store_id() != expected_store {
                return Err(RecoveryError::Store {
                    expected: expected_store,
                    actual: prepare.store_id(),
                });
            }
            if prepare.receipt().profile_id() != expected_profile {
                return Err(RecoveryError::Profile {
                    expected: expected_profile,
                    actual: prepare.receipt().profile_id(),
                });
            }
            if let Some(expected_base) = previous_state {
                if prepare.receipt().base() != expected_base {
                    return Err(RecoveryError::StateContinuity {
                        expected: expected_base,
                        actual: prepare.receipt().base(),
                    });
                }
            }

            let mut commit_bytes = [0u8; TRANSACTION_COMMIT_RECORD_LEN_V1 as usize];
            reader
                .read_exact_at(prepare_end, &mut commit_bytes)
                .map_err(RecoveryError::Read)?;
            let commit = TransactionCommitViewV1::parse(&commit_bytes)?;
            commit.verify_prepare(prepare)?;
            if commit.previous_commit() != previous_commit {
                return Err(RecoveryError::PreviousCommit {
                    expected: previous_commit,
                    actual: commit.previous_commit(),
                });
            }
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

            // A generation begins with a self-contained checkpoint in its
            // first live pack. Because the rebuilt index is empty here, REUSE
            // would depend on a retired generation and is rejected explicitly.
            let generation_checkpoint = pack_id == first_live_pack && offset == PACK_HEADER_LEN_V1;
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
                        limits.require(DurableLimitKind::IndexBytes, expected_index_tail)?;
                        index_sink.data(record)?;
                    }
                    LogPackObjectDispositionV1::Reuse if generation_checkpoint => {
                        return Err(RecoveryError::GenerationCheckpointReuse(object.reference()));
                    }
                    LogPackObjectDispositionV1::Reuse => {
                        index_sink.reuse(object.reference(), prepare.sequence())?;
                    }
                }
            }
            if commit.durable_index_tail() != expected_index_tail {
                return Err(RecoveryError::IndexTail {
                    expected: expected_index_tail,
                    actual: commit.durable_index_tail(),
                });
            }

            previous_commit = commit.digest();
            previous_state = Some(commit.receipt().target());
            last_sequence = Some(expected_sequence);
            terminal_address = Some(LogPackAddress::from_pack(
                pack_id,
                prepare_end,
                TRANSACTION_COMMIT_RECORD_LEN_V1,
            )?);
            terminal_commit_bytes = Some(commit_bytes);
            offset = commit_end;
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
    let terminal_commit_bytes = terminal_commit_bytes.ok_or(RecoveryError::EmptyPackSet)?;
    let state = previous_state.ok_or(RecoveryError::EmptyPackSet)?;
    let index = index_sink.finish()?;
    Ok(ScannedPackChainV1 {
        catalog_image: *authority.image(),
        store_id: expected_store,
        profile_id: expected_profile,
        state,
        terminal,
        terminal_address,
        terminal_commit_bytes,
        index,
        expected_index_tail,
        transactions,
        object_records,
        scanned_bytes,
    })
}
