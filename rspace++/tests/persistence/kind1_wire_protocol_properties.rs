use std::collections::BTreeSet;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Mutex};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use proptest::prelude::*;

use super::support::{TestSemanticProfile, encode, synthetic_ref};
use super::transaction_sealing::{base_state, data_batch, seal_data};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::internal::Datum;
use crate::rspace::persistence::durable::{
    CatalogPackProviderV1, DurableReadAt, RecoveryError, scan_catalog_authorized_pack_chain,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, COMMIT_DISCRIMINATOR_LEN_V1, CanonicalFormatError,
    CanonicalObjectStore, CanonicalSink, CatalogAuthorityTokenV1, CatalogPackStateV1,
    CatalogSlotV1, CommitPublicationV1, CommitRecordKindV1, CommittedObjectComparison,
    CommittedObjectLookup, DurableLimitConfig, DurableLimitError, DurableLimitKind, DurableLimits,
    INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, LogPackAddress, LogPackDecodeLimits,
    LogPackFormatError, LogPackObjectPlanV1, LogPackSequence, MutationError, ObjectRefV1,
    PACK_HEADER_LEN_V1, PREPARE_DISCRIMINATOR_LEN_V1, PackHeaderV1, PackId, PhysicalDigest,
    PrepareRecordIdentityV1, PrepareRecordKindV1, PreparedLogPackTransactionV1,
    RSpaceMutationBatchV1, StateVersionV1, StoreId, TRANSACTION_COMMIT_RECORD_LEN_V1,
    TRANSACTION_PREPARE_HEADER_LEN_V1, TransactionCommitRecordV1, TransactionCommitViewV1,
    TransactionPrepareRecordViewV1, decode_commit_kind_v1, decode_prepare_kind_v1,
    preflight_prepare_record_v1,
};

const PREPARE_AUTH_HEADER_LEN: usize = 232;
const CHECKSUM_LEN: usize = 8;
const COMMIT_AUTH_LEN: usize = 376;
const OBJECT_RECORD_HEADER_LEN: u64 = 48;

struct MissingLookup;

impl CommittedObjectLookup for MissingLookup {
    type Error = Infallible;

    fn compare_committed_object(
        &self,
        _reference: ObjectRefV1,
        _sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error> {
        Ok(CommittedObjectComparison::Missing)
    }
}

struct FragmentSink {
    bytes: Vec<u8>,
    width: usize,
}

impl CanonicalSink for FragmentSink {
    type Error = Infallible;

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        for chunk in bytes.chunks(self.width) {
            self.bytes.extend_from_slice(chunk);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReadFragment {
    offset: u64,
    len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TraceReadError {
    Injected(u64),
    OutOfRange,
}

impl Display for TraceReadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Injected(offset) => write!(formatter, "injected read failure at {offset}"),
            Self::OutOfRange => formatter.write_str("read outside the synthetic durable image"),
        }
    }
}

impl std::error::Error for TraceReadError {}

#[derive(Clone)]
struct TraceReader {
    bytes: Arc<[u8]>,
    fragments: Arc<Mutex<Vec<ReadFragment>>>,
    partition_width: usize,
    fail_at: Option<u64>,
}

impl DurableReadAt for TraceReader {
    type Error = TraceReadError;

    fn durable_len(&self) -> Result<u64, Self::Error> { Ok(self.bytes.len() as u64) }

    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> Result<(), Self::Error> {
        let start = usize::try_from(offset).map_err(|_| TraceReadError::OutOfRange)?;
        start
            .checked_add(destination.len())
            .filter(|end| *end <= self.bytes.len())
            .ok_or(TraceReadError::OutOfRange)?;
        let mut copied = 0usize;
        while copied < destination.len() {
            let width = self.partition_width.min(destination.len() - copied);
            let fragment_offset = offset
                .checked_add(copied as u64)
                .ok_or(TraceReadError::OutOfRange)?;
            let fragment_end = fragment_offset
                .checked_add(width as u64)
                .ok_or(TraceReadError::OutOfRange)?;
            if let Some(fail_at) = self.fail_at {
                if fragment_offset <= fail_at && fail_at < fragment_end {
                    return Err(TraceReadError::Injected(fail_at));
                }
            }
            self.fragments.lock().unwrap().push(ReadFragment {
                offset: fragment_offset,
                len: width,
            });
            destination[copied..copied + width]
                .copy_from_slice(&self.bytes[start + copied..start + copied + width]);
            copied += width;
        }
        Ok(())
    }
}

struct TraceProvider(TraceReader);

impl CatalogPackProviderV1 for TraceProvider {
    type Reader = TraceReader;
    type Error = TraceReadError;

    fn open_pack(&mut self, pack_id: PackId) -> Result<Self::Reader, Self::Error> {
        if pack_id != PackId::GENESIS {
            return Err(TraceReadError::OutOfRange);
        }
        Ok(self.0.clone())
    }
}

#[derive(Clone)]
struct RecoveryFixture {
    pack: Arc<[u8]>,
    catalog: CatalogSlotV1,
    prepare_offset: u64,
    prepare_len: u64,
    mutation_len: u64,
    object_count: u64,
    object_bytes: u64,
    maximum_object_len: u64,
    mutation_entries: u64,
    mutation_key_bytes: u64,
    expected_prior_key_reread_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReadWorkSnapshot {
    authorized: u64,
    physical: u64,
    prior_key: u64,
    layout: u64,
}

fn mutation_bytes(store: &CanonicalObjectStore, entries: usize, identity: &str) -> Vec<u8> {
    let base = base_state(store, 41, None);
    let channel = "wire-property-channel".to_owned();
    let mut batch = RSpaceMutationBatchV1::new(base).unwrap();
    for index in 0..entries {
        let datum = Datum::create(&channel, format!("{identity}-{index:04}"), false);
        let path = RSpaceDataIndex::datum_path(&channel, &datum);
        batch.record_data(&path, 0, 1).unwrap();
    }
    encode(&batch.plan().unwrap())
}

fn prepare_image(
    entries: usize,
    identity: &str,
    store_id: StoreId,
    sequence: LogPackSequence,
) -> (Vec<u8>, crate::rspace::persistence::SealedTransitionV1, u64) {
    let store = CanonicalObjectStore::new();
    let mutation = mutation_bytes(&store, entries, identity);
    let sealed = seal_data(&store, &mutation);
    let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &MissingLookup).unwrap(),
        store_id,
        sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let inline_count = prepared.inline_count();
    let receipt = sealed.receipt();
    let mut bytes = Vec::new();
    prepared.write_to(&mut bytes).unwrap();
    (bytes, receipt, inline_count)
}

fn frozen_prepare_image() -> (Vec<u8>, crate::rspace::persistence::SealedTransitionV1, StoreId) {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 41, None);
    let mutation = data_batch(base, "WAL-round-trip");
    let sealed = seal_data(&store, &mutation);
    let store_id = StoreId::from_array([0x5a; 16]);
    let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, &MissingLookup).unwrap(),
        store_id,
        LogPackSequence::new(7).unwrap(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let receipt = sealed.receipt();
    let mut bytes = Vec::new();
    prepared.write_to(&mut bytes).unwrap();
    (bytes, receipt, store_id)
}

fn recovery_fixture(entries: usize) -> RecoveryFixture {
    let store_id = StoreId::from_array([0x5a; 16]);
    let sequence = LogPackSequence::FIRST;
    let (prepare, receipt, inline_count) =
        prepare_image(entries, "kind1-recovery", store_id, sequence);
    let prepare_view =
        TransactionPrepareRecordViewV1::parse(&prepare, LogPackDecodeLimits::UNBOUNDED).unwrap();
    let preflight = preflight_prepare_record_v1(
        &prepare[..TRANSACTION_PREPARE_HEADER_LEN_V1 as usize],
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let keys = prepare_view
        .mutation()
        .data()
        .map(|entry| entry.key.len() as u64)
        .chain(
            prepare_view
                .mutation()
                .continuations()
                .map(|entry| entry.key.len() as u64),
        )
        .chain(
            prepare_view
                .mutation()
                .joins()
                .map(|entry| entry.key.len() as u64),
        )
        .collect::<Vec<_>>();
    let maximum_object_len = prepare_view
        .objects()
        .map(|object| object.reference().encoded_len())
        .max()
        .unwrap();
    let prepare_offset = PACK_HEADER_LEN_V1;
    let prepare_address =
        LogPackAddress::from_pack(PackId::GENESIS, prepare_offset, prepare_view.record_len())
            .unwrap();
    let index_tail = INDEX_HEADER_LEN_V1 + inline_count * INDEX_RECORD_LEN_V1;
    let publication =
        CommitPublicationV1::new(1, 1, index_tail, synthetic_ref(0xd1), 4096, 8192).unwrap();
    let commit = TransactionCommitRecordV1::new(
        receipt,
        store_id,
        PhysicalDigest::ZERO,
        prepare_address,
        PrepareRecordIdentityV1::from_record_view(prepare_view),
        publication,
    )
    .unwrap();
    let mut pack = PackHeaderV1::genesis(store_id).encode().to_vec();
    pack.extend_from_slice(&prepare);
    let commit_offset = pack.len() as u64;
    pack.extend_from_slice(commit.canonical_bytes());
    let commit_address =
        LogPackAddress::from_pack(PackId::GENESIS, commit_offset, TRANSACTION_COMMIT_RECORD_LEN_V1)
            .unwrap();
    let pack_state = CatalogPackStateV1::new(
        1,
        1,
        pack.len() as u64,
        1,
        index_tail,
        publication.retention_manifest(),
        publication.current_state_canonical_bytes(),
        publication.retained_canonical_bytes(),
    )
    .unwrap();
    let catalog = CatalogSlotV1::new(
        store_id,
        1,
        receipt.target(),
        receipt.profile_id(),
        commit.digest(),
        sequence,
        commit_address,
        pack_state,
    )
    .unwrap();
    RecoveryFixture {
        pack: pack.into(),
        catalog,
        prepare_offset,
        prepare_len: preflight.record_len(),
        mutation_len: preflight.mutation_len(),
        object_count: preflight.object_count(),
        object_bytes: preflight.object_bytes(),
        maximum_object_len,
        mutation_entries: keys.len() as u64,
        mutation_key_bytes: keys.iter().sum(),
        expected_prior_key_reread_bytes: keys.iter().take(keys.len().saturating_sub(1)).sum(),
    }
}

fn authority(catalog: CatalogSlotV1) -> CatalogAuthorityTokenV1 {
    let mut image = [0u8; CATALOG_IMAGE_LEN];
    image[..CATALOG_SLOT_LEN].copy_from_slice(&catalog.encode());
    CatalogAuthorityTokenV1::select(image).unwrap().unwrap()
}

fn durable_limits(physical: u64) -> DurableLimits {
    DurableLimits::new(DurableLimitConfig {
        max_live_packs: 8,
        max_pack_bytes: 16 * 1024 * 1024,
        max_transactions: 1024,
        max_object_records: 65_536,
        max_total_scan_bytes: 64 * 1024 * 1024,
        max_total_physical_read_bytes: physical,
        max_index_bytes: 16 * 1024 * 1024,
        max_index_records: 65_536,
        max_full_byte_comparison_bytes: 16 * 1024 * 1024,
        max_aggregate_full_byte_comparison_bytes: 64 * 1024 * 1024,
    })
    .unwrap()
}

fn scan(
    fixture: &RecoveryFixture,
    partition_width: usize,
    fail_at: Option<u64>,
    wal_limits: LogPackDecodeLimits,
) -> (Result<ReadWorkSnapshot, RecoveryError<TraceReadError>>, Vec<ReadFragment>) {
    let fragments = Arc::new(Mutex::new(Vec::new()));
    let mut provider = TraceProvider(TraceReader {
        bytes: Arc::clone(&fixture.pack),
        fragments: Arc::clone(&fragments),
        partition_width,
        fail_at,
    });
    let result = scan_catalog_authorized_pack_chain(
        &mut provider,
        &authority(fixture.catalog),
        durable_limits(64 * 1024 * 1024),
        wal_limits,
    )
    .map(|scanned| {
        let recovered = scanned
            .verify_catalog(authority(fixture.catalog))
            .expect("the unchanged catalog token verifies a successful scan");
        let work = recovered.read_work();
        ReadWorkSnapshot {
            authorized: work.authorized_pack_bytes(),
            physical: work.physical_read_bytes(),
            prior_key: work.prior_key_reread_bytes(),
            layout: work.layout_replay_bytes(),
        }
    });
    let trace = fragments.lock().unwrap().clone();
    (result, trace)
}

fn scan_with_physical_read_limit(
    fixture: &RecoveryFixture,
    physical_read_limit: u64,
) -> (Result<(), RecoveryError<TraceReadError>>, Vec<ReadFragment>) {
    let fragments = Arc::new(Mutex::new(Vec::new()));
    let mut provider = TraceProvider(TraceReader {
        bytes: Arc::clone(&fixture.pack),
        fragments: Arc::clone(&fragments),
        partition_width: usize::MAX,
        fail_at: None,
    });
    let result = scan_catalog_authorized_pack_chain(
        &mut provider,
        &authority(fixture.catalog),
        durable_limits(physical_read_limit),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .map(|_| ());
    let trace = fragments.lock().unwrap().clone();
    (result, trace)
}

fn primary_trace_boundaries(trace: &[ReadFragment], authorized_extent: u64) -> (usize, Vec<usize>) {
    let extent = usize::try_from(authorized_extent).unwrap();
    let mut seen = vec![false; extent];
    let mut seen_count = 0usize;
    let mut rereads = Vec::new();
    for (index, read) in trace.iter().enumerate() {
        let start = usize::try_from(read.offset).unwrap();
        let end = start.checked_add(read.len).unwrap();
        assert!(end <= extent);
        if seen[start..end].iter().all(|observed| *observed) {
            rereads.push(index);
        }
        for observed in &mut seen[start..end] {
            if !*observed {
                *observed = true;
                seen_count += 1;
            }
        }
        if seen_count == extent {
            return (index + 1, rereads);
        }
    }
    panic!("the successful trace did not cover the complete authorized extent")
}

fn exact_contiguous_replay_end(
    trace: &[ReadFragment],
    start_index: usize,
    range: std::ops::Range<u64>,
) -> usize {
    let mut expected = range.start;
    for (index, read) in trace.iter().enumerate().skip(start_index) {
        assert_eq!(read.offset, expected);
        expected = expected.checked_add(read.len as u64).unwrap();
        assert!(expected <= range.end);
        if expected == range.end {
            return index + 1;
        }
    }
    panic!("the successful trace did not replay the complete authenticated source")
}

fn assert_physical_limit_rejects_before_trace_event(
    fixture: &RecoveryFixture,
    successful_trace: &[ReadFragment],
    rejected_event: usize,
) {
    let admitted_prefix = successful_trace[..rejected_event]
        .iter()
        .map(|read| read.len as u64)
        .sum::<u64>();
    let attempted = admitted_prefix
        .checked_add(successful_trace[rejected_event].len as u64)
        .unwrap();
    let limit = attempted - 1;
    let (result, rejected_trace) = scan_with_physical_read_limit(fixture, limit);
    assert!(matches!(
        result,
        Err(RecoveryError::Limit(DurableLimitError::Exceeded {
            resource: DurableLimitKind::TotalPhysicalReadBytes,
            limit: actual_limit,
            attempted: actual_attempted,
        })) if actual_limit == limit && actual_attempted == attempted
    ));
    assert_eq!(rejected_trace, successful_trace[..rejected_event]);
    assert_eq!(
        rejected_trace
            .iter()
            .map(|read| read.len as u64)
            .sum::<u64>(),
        admitted_prefix,
    );
    assert!(admitted_prefix <= limit);
}

fn first_observation_coverage(
    fragments: &[ReadFragment],
    range: std::ops::Range<u64>,
) -> (Vec<u64>, u64) {
    let mut seen = BTreeSet::new();
    let mut first = Vec::new();
    let mut duplicates = 0u64;
    for fragment in fragments {
        for offset in fragment.offset..fragment.offset + fragment.len as u64 {
            if range.contains(&offset) {
                if seen.insert(offset) {
                    first.push(offset);
                } else {
                    duplicates += 1;
                }
            }
        }
    }
    (first, duplicates)
}

fn repair_prepare_header_crc(bytes: &mut [u8]) {
    let crc = crate::rspace::persistence::checksum::crc32c(&bytes[..PREPARE_AUTH_HEADER_LEN]);
    bytes[PREPARE_AUTH_HEADER_LEN..PREPARE_AUTH_HEADER_LEN + CHECKSUM_LEN]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
}

fn repair_prepare_record_crc(bytes: &mut [u8]) {
    let payload_end = bytes.len() - CHECKSUM_LEN;
    let crc = crate::rspace::persistence::checksum::crc32c(&bytes[..payload_end]);
    bytes[payload_end..].copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
}

fn repair_commit_crc(bytes: &mut [u8]) {
    let crc = crate::rspace::persistence::checksum::crc32c(&bytes[..COMMIT_AUTH_LEN]);
    bytes[COMMIT_AUTH_LEN..]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(crc));
}

fn catalog_for_commit(template: CatalogSlotV1, commit: TransactionCommitViewV1) -> CatalogSlotV1 {
    CatalogSlotV1::new(
        template.store_id(),
        template.epoch(),
        template.state(),
        template.profile_id(),
        commit.digest(),
        template.commit_sequence(),
        template.commit_address(),
        template.pack(),
    )
    .unwrap()
}

fn semantic_fault_fixture(valid_crc: bool) -> RecoveryFixture {
    let fixture = recovery_fixture(2);
    let mut pack = fixture.pack.to_vec();
    let start = fixture.prepare_offset as usize;
    let end = start + fixture.prepare_len as usize;
    pack[start + TRANSACTION_PREPARE_HEADER_LEN_V1 as usize] ^= 0x80;
    if valid_crc {
        repair_prepare_record_crc(&mut pack[start..end]);
        let digest = PhysicalDigest::of_bytes(&pack[start..end - CHECKSUM_LEN]);
        pack[end + 120..end + 152].copy_from_slice(digest.as_bytes());
        repair_commit_crc(&mut pack[end..end + TRANSACTION_COMMIT_RECORD_LEN_V1 as usize]);
    }
    let catalog = if valid_crc {
        let commit = TransactionCommitViewV1::parse(
            &pack[end..end + TRANSACTION_COMMIT_RECORD_LEN_V1 as usize],
        )
        .unwrap();
        catalog_for_commit(fixture.catalog, commit)
    } else {
        fixture.catalog
    };
    RecoveryFixture {
        pack: pack.into(),
        catalog,
        ..fixture
    }
}

fn assert_wal_resource(
    result: Result<TransactionPrepareRecordViewV1<'_>, LogPackFormatError>,
    resource: &'static str,
    limit: u64,
    attempted: u64,
) {
    let observed = match result {
        Err(LogPackFormatError::ResourceLimit {
            resource,
            limit,
            attempted,
        }) => (resource, limit, attempted),
        Err(LogPackFormatError::Mutation(MutationError::Canonical(
            CanonicalFormatError::ResourceLimit {
                resource,
                limit,
                attempted,
            },
        ))) => (resource, limit, attempted),
        other => panic!("expected {resource} resource refusal, got {other:?}"),
    };
    assert_eq!(observed, (resource, limit, attempted));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn kind1_v1_wire_images_are_frozen_and_stream_identical(width in 1usize..257) {
        let (prepare, receipt, store_id) = frozen_prepare_image();
        let view = TransactionPrepareRecordViewV1::parse(
            &prepare,
            LogPackDecodeLimits::UNBOUNDED,
        ).unwrap();
        let store = CanonicalObjectStore::new();
        let base = base_state(&store, 41, None);
        let mutation = data_batch(base, "WAL-round-trip");
        let sealed = seal_data(&store, &mutation);
        let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
            &sealed,
            LogPackObjectPlanV1::classify(&sealed, &MissingLookup).unwrap(),
            store_id,
            LogPackSequence::new(7).unwrap(),
            LogPackDecodeLimits::UNBOUNDED,
        ).unwrap();
        let mut streamed = FragmentSink { bytes: Vec::new(), width };
        let identity = prepared.write_to(&mut streamed).unwrap();
        prop_assert_eq!(&streamed.bytes, &prepare);
        prop_assert_eq!(identity, PrepareRecordIdentityV1::from_record_view(view));
        prop_assert_eq!(prepare.len(), 1123);
        prop_assert_eq!(
            hex::encode(&prepare[..PREPARE_AUTH_HEADER_LEN]),
            "52537061636557414c2e50726570000001000000000000000100000000000000630400000000000007000000000000005a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a290000000000000008a2bad5d01cd13ce6c39000aa50c93509172aebb94fed4f47858b8414404bcfd1000000000000002a0000000000000024b1f6c14483eeb76ebb2e72dcad8846267740bde2cc0ce364cef24b98131dd0d1000000000000005253706163652e546573742e56310000cd05aa8ec69624595dd5c844dbc40f03d891154be925157f4ce25c95c8d6cba458010000000000000200000000000000b301000000000000"
        );
        prop_assert_eq!(view.crc32c(), 0xe620_2329);
        prop_assert_eq!(
            view.digest().to_string(),
            "9b459e919000bfaa225df9b9bf521d79f4070aa98f8727af6b28b418714c842f",
        );
        let commit = TransactionCommitRecordV1::new(
            receipt,
            store_id,
            PhysicalDigest::of_bytes(b"frozen previous commit"),
            LogPackAddress::new(7, 16_384, view.record_len()).unwrap(),
            PrepareRecordIdentityV1::from_record_view(view),
            CommitPublicationV1::new(1, 3, 32_752, synthetic_ref(0xe0), 2048, 4096).unwrap(),
        ).unwrap();
        prop_assert_eq!(commit.canonical_bytes().len() as u64, TRANSACTION_COMMIT_RECORD_LEN_V1);
        prop_assert_eq!(
            hex::encode(commit.canonical_bytes()),
            "52537061636557414c2e436f6d6d000001000000000000000200000000000000800100000000000007000000000000005a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a80f5dfe8cb9b5cec6d10570022f58c7d18aa77808b16ac31784bcdfee68ac6a20700000000000000004000000000000063040000000000009b459e919000bfaa225df9b9bf521d79f4070aa98f8727af6b28b418714c842f290000000000000008a2bad5d01cd13ce6c39000aa50c93509172aebb94fed4f47858b8414404bcfd1000000000000002a0000000000000024b1f6c14483eeb76ebb2e72dcad8846267740bde2cc0ce364cef24b98131dd0d1000000000000005253706163652e546573742e56310000cd05aa8ec69624595dd5c844dbc40f03d891154be925157f4ce25c95c8d6cba4f07f00000000000001000000000000000300000000000000e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0320000000000000000080000000000000010000000000000b9f2d03400000000"
        );
    }

    #[test]
    fn primary_read_trace_covers_authenticated_extent_once_in_order_for_every_partition(
        partition_width in 1usize..1025,
    ) {
        let fixture = recovery_fixture(4);
        let (result, trace) = scan(
            &fixture,
            partition_width,
            None,
            LogPackDecodeLimits::UNBOUNDED,
        );
        result.unwrap();
        let range = fixture.prepare_offset
            ..fixture.prepare_offset + fixture.prepare_len - CHECKSUM_LEN as u64;
        let (first, _) = first_observation_coverage(&trace, range.clone());
        prop_assert_eq!(first, range.collect::<Vec<_>>());
    }
}

#[test]
fn kind1_discriminator_and_adjacent_pair_matrix_is_exact() {
    let fixture = recovery_fixture(2);
    let prepare_start = fixture.prepare_offset as usize;
    let commit_start = prepare_start + fixture.prepare_len as usize;
    let prepare_discriminator =
        &fixture.pack[prepare_start..prepare_start + PREPARE_DISCRIMINATOR_LEN_V1 as usize];
    let commit_discriminator =
        &fixture.pack[commit_start..commit_start + COMMIT_DISCRIMINATOR_LEN_V1 as usize];
    assert_eq!(
        decode_prepare_kind_v1(prepare_discriminator).unwrap(),
        PrepareRecordKindV1::Incremental,
    );
    assert_eq!(
        decode_commit_kind_v1(commit_discriminator).unwrap(),
        CommitRecordKindV1::Incremental,
    );
    for length in 0..prepare_discriminator.len() {
        assert!(decode_prepare_kind_v1(&prepare_discriminator[..length]).is_err());
    }
    for length in 0..commit_discriminator.len() {
        assert!(decode_commit_kind_v1(&commit_discriminator[..length]).is_err());
    }
    assert!(decode_commit_kind_v1(&fixture.pack[commit_start + 1..commit_start + 33]).is_err());
    let mut mismatch = fixture.pack.to_vec();
    mismatch[commit_start + 24..commit_start + 32].copy_from_slice(&4u64.to_le_bytes());
    let mismatch = RecoveryFixture {
        pack: mismatch.into(),
        ..fixture
    };
    assert!(matches!(
        scan(&mismatch, 31, None, LogPackDecodeLimits::UNBOUNDED).0,
        Err(RecoveryError::CommitPairing {
            prepare: PrepareRecordKindV1::Incremental,
            commit: CommitRecordKindV1::GenerationCheckpoint,
        })
    ));

    let checkpoint = super::durable_recovery::valid_checkpoint_image();
    let prepare_start = checkpoint.prepare_address.offset() as usize;
    let commit_start = checkpoint.commit_address.offset() as usize;
    assert_eq!(
        decode_prepare_kind_v1(
            &checkpoint.pack[prepare_start..prepare_start + PREPARE_DISCRIMINATOR_LEN_V1 as usize],
        )
        .unwrap(),
        PrepareRecordKindV1::GenerationCheckpoint,
    );
    assert_eq!(
        decode_commit_kind_v1(
            &checkpoint.pack[commit_start..commit_start + COMMIT_DISCRIMINATOR_LEN_V1 as usize],
        )
        .unwrap(),
        CommitRecordKindV1::GenerationCheckpoint,
    );
    let mut mismatch = checkpoint.pack.to_vec();
    mismatch[commit_start + 24..commit_start + 32].copy_from_slice(&2u64.to_le_bytes());
    let mismatch = RecoveryFixture {
        pack: mismatch.into(),
        catalog: checkpoint.catalog,
        prepare_offset: checkpoint.prepare_address.offset(),
        prepare_len: checkpoint.prepare_address.length(),
        mutation_len: 0,
        object_count: checkpoint.records.len() as u64,
        object_bytes: 0,
        maximum_object_len: 0,
        mutation_entries: 0,
        mutation_key_bytes: 0,
        expected_prior_key_reread_bytes: 0,
    };
    assert!(matches!(
        scan(&mismatch, 31, None, LogPackDecodeLimits::UNBOUNDED).0,
        Err(RecoveryError::CommitPairing {
            prepare: PrepareRecordKindV1::GenerationCheckpoint,
            commit: CommitRecordKindV1::Incremental,
        })
    ));
}

#[test]
fn first_semantic_error_is_retained_while_declared_extent_is_raw_drained() {
    let fixture = semantic_fault_fixture(true);
    let (result, trace) = scan(&fixture, 37, None, LogPackDecodeLimits::UNBOUNDED);
    assert!(matches!(
        result,
        Err(RecoveryError::Wal(LogPackFormatError::Mutation(MutationError::Canonical(
            CanonicalFormatError::InvalidDomain { .. }
        ))))
    ));
    let range = fixture.prepare_offset..fixture.prepare_offset + fixture.prepare_len;
    let (first, _) = first_observation_coverage(&trace, range.clone());
    assert_eq!(first, range.collect::<Vec<_>>());
}

#[test]
fn every_stream_transition_strictly_decreases_formal_progress_measure() {
    let fixture = recovery_fixture(5);
    let (result, trace) = scan(&fixture, 29, None, LogPackDecodeLimits::UNBOUNDED);
    let work = result.unwrap();
    let mut primary = work.authorized;
    let mut auxiliary = work
        .physical
        .checked_sub(work.authorized)
        .expect("physical work contains the authorized primary extent");
    let mut seen = BTreeSet::new();
    for fragment in trace {
        assert_ne!(fragment.len, 0);
        let before = (primary, auxiliary);
        let range = fragment.offset..fragment.offset + fragment.len as u64;
        let unseen = range
            .clone()
            .filter(|offset| !seen.contains(offset))
            .count() as u64;
        if unseen == fragment.len as u64 {
            seen.extend(range);
            primary = primary.checked_sub(unseen).unwrap();
        } else {
            assert_eq!(unseen, 0);
            auxiliary = auxiliary.checked_sub(fragment.len as u64).unwrap();
        }
        assert!((primary, auxiliary) < before);
    }
    assert_eq!((primary, auxiliary), (0, 0));
}

#[test]
fn all_kind1_extent_count_and_sum_arithmetic_fails_closed_at_boundaries() {
    assert!(matches!(
        LogPackAddress::new(1, u64::MAX, 1),
        Err(LogPackFormatError::AddressOverflow { .. })
    ));
    assert!(matches!(
        LogPackSequence::new(u64::MAX).unwrap().successor(),
        Err(LogPackFormatError::SequenceOverflow(u64::MAX))
    ));
    assert!(matches!(
        PackId::new(u64::MAX).unwrap().successor(),
        Err(LogPackFormatError::PackIdOverflow(u64::MAX))
    ));
    assert!(matches!(
        RSpaceMutationBatchV1::new(StateVersionV1::new(u64::MAX, synthetic_ref(1))),
        Err(MutationError::GenerationOverflow(u64::MAX))
    ));
    let limits = durable_limits(u64::MAX);
    for resource in DurableLimitKind::ALL {
        assert!(matches!(
            limits.checked_charge(resource, u64::MAX, 1),
            Err(DurableLimitError::CounterOverflow {
                resource: actual,
                current: u64::MAX,
                additional: 1,
            }) if actual == resource
        ));
    }
    let fixture = recovery_fixture(2);
    let start = fixture.prepare_offset as usize;
    let original = &fixture.pack[start..start + fixture.prepare_len as usize];
    for offset in [32usize, 208, 216, 224] {
        let mut record = original.to_vec();
        record[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        repair_prepare_header_crc(&mut record);
        repair_prepare_record_crc(&mut record);
        assert!(
            TransactionPrepareRecordViewV1::parse(&record, LogPackDecodeLimits::UNBOUNDED).is_err(),
            "boundary declaration at offset {offset} must fail closed when consumed",
        );
    }
}

#[test]
fn kind1_variable_work_is_silent_until_header_pair_and_aggregate_preadmission() {
    let fixture = recovery_fixture(3);
    let prefix_end = fixture.prepare_offset + TRANSACTION_PREPARE_HEADER_LEN_V1;
    let prepare_end = fixture.prepare_offset + fixture.prepare_len;

    let mut bad_header = fixture.pack.to_vec();
    bad_header[fixture.prepare_offset as usize + 80] ^= 1;
    let bad_header = RecoveryFixture {
        pack: bad_header.into(),
        ..fixture.clone()
    };
    let (_, reads) = scan(&bad_header, 17, None, LogPackDecodeLimits::UNBOUNDED);
    assert!(reads.iter().all(|read| read.offset < prefix_end));

    let mut bad_pair = fixture.pack.to_vec();
    bad_pair[prepare_end as usize + 24..prepare_end as usize + 32]
        .copy_from_slice(&4u64.to_le_bytes());
    let bad_pair = RecoveryFixture {
        pack: bad_pair.into(),
        ..fixture.clone()
    };
    let (_, reads) = scan(&bad_pair, 17, None, LogPackDecodeLimits::UNBOUNDED);
    assert!(reads.iter().all(|read| {
        read.offset < prefix_end ||
            (prepare_end..prepare_end + COMMIT_DISCRIMINATOR_LEN_V1).contains(&read.offset)
    }));

    let limits = LogPackDecodeLimits {
        max_objects: fixture.object_count - 1,
        ..LogPackDecodeLimits::UNBOUNDED
    };
    let (_, reads) = scan(&fixture, 17, None, limits);
    assert!(reads.iter().all(|read| read.offset < prefix_end));

    let (_, reads) = scan(&fixture, 17, None, LogPackDecodeLimits::UNBOUNDED);
    let pair = reads
        .iter()
        .position(|read| read.offset == prepare_end)
        .unwrap();
    let body = reads
        .iter()
        .position(|read| (prefix_end..prepare_end).contains(&read.offset))
        .unwrap();
    assert!(pair < body);
}

#[test]
fn every_kind1_limit_accepts_exact_and_rejects_minus_one_before_corresponding_work() {
    let fixture = recovery_fixture(3);
    let start = fixture.prepare_offset as usize;
    let bytes = &fixture.pack[start..start + fixture.prepare_len as usize];
    TransactionPrepareRecordViewV1::parse(bytes, LogPackDecodeLimits {
        max_record_bytes: fixture.prepare_len,
        max_mutation_bytes: fixture.mutation_len,
        max_objects: fixture.object_count,
        max_object_bytes: fixture.object_bytes,
        max_single_object_bytes: fixture.maximum_object_len,
        max_mutation_entries: fixture.mutation_entries,
        max_mutation_key_bytes: fixture.mutation_key_bytes,
    })
    .unwrap();
    let cases = [
        ("record bytes", fixture.prepare_len, LogPackDecodeLimits {
            max_record_bytes: fixture.prepare_len - 1,
            ..LogPackDecodeLimits::UNBOUNDED
        }),
        ("mutation bytes", fixture.mutation_len, LogPackDecodeLimits {
            max_mutation_bytes: fixture.mutation_len - 1,
            ..LogPackDecodeLimits::UNBOUNDED
        }),
        ("objects", fixture.object_count, LogPackDecodeLimits {
            max_objects: fixture.object_count - 1,
            ..LogPackDecodeLimits::UNBOUNDED
        }),
        ("canonical object bytes", fixture.object_bytes, LogPackDecodeLimits {
            max_object_bytes: fixture.object_bytes - 1,
            ..LogPackDecodeLimits::UNBOUNDED
        }),
        ("single referenced object bytes", fixture.maximum_object_len, LogPackDecodeLimits {
            max_single_object_bytes: fixture.maximum_object_len - 1,
            ..LogPackDecodeLimits::UNBOUNDED
        }),
        ("mutation entries", fixture.mutation_entries, LogPackDecodeLimits {
            max_mutation_entries: fixture.mutation_entries - 1,
            ..LogPackDecodeLimits::UNBOUNDED
        }),
        ("mutation key bytes", fixture.mutation_key_bytes, LogPackDecodeLimits {
            max_mutation_key_bytes: fixture.mutation_key_bytes - 1,
            ..LogPackDecodeLimits::UNBOUNDED
        }),
    ];
    for (resource, demand, limits) in cases {
        assert_wal_resource(
            TransactionPrepareRecordViewV1::parse(bytes, limits),
            resource,
            demand - 1,
            demand,
        );
        let (result, _) = scan(&fixture, 61, None, limits);
        assert!(result.is_err(), "recovery accepted underprovisioned {resource}");
    }

    let demand = 4096;
    let limits = DurableLimits::new(DurableLimitConfig {
        max_live_packs: demand,
        max_pack_bytes: demand,
        max_transactions: demand,
        max_object_records: demand,
        max_total_scan_bytes: demand,
        max_total_physical_read_bytes: demand,
        max_index_bytes: demand,
        max_index_records: demand,
        max_full_byte_comparison_bytes: demand,
        max_aggregate_full_byte_comparison_bytes: demand,
    })
    .unwrap();
    for resource in DurableLimitKind::ALL {
        limits.require(resource, demand).unwrap();
        assert!(matches!(
            limits.require(resource, demand + 1),
            Err(DurableLimitError::Exceeded {
                resource: actual,
                limit,
                attempted,
            }) if actual == resource && limit == demand && attempted == demand + 1
        ));
    }
}

#[test]
fn recovery_read_work_separates_authorized_primary_prior_key_and_layout_bytes_exactly() {
    let fixture = recovery_fixture(5);
    let authenticated_source_replay_bytes = fixture.prepare_len - CHECKSUM_LEN as u64;
    let layout_application_replay_bytes = fixture.object_count * OBJECT_RECORD_HEADER_LEN;
    assert_eq!(
        authenticated_source_replay_bytes,
        TRANSACTION_PREPARE_HEADER_LEN_V1 +
            fixture.mutation_len +
            layout_application_replay_bytes +
            fixture.object_bytes,
    );
    let fragments = Arc::new(Mutex::new(Vec::new()));
    let mut provider = TraceProvider(TraceReader {
        bytes: Arc::clone(&fixture.pack),
        fragments: Arc::clone(&fragments),
        partition_width: usize::MAX,
        fail_at: None,
    });
    let token = authority(fixture.catalog);
    let scanned = scan_catalog_authorized_pack_chain(
        &mut provider,
        &token,
        durable_limits(64 * 1024 * 1024),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let work = scanned.verify_catalog(token).unwrap().read_work();
    let physical = fragments
        .lock()
        .unwrap()
        .iter()
        .map(|read| read.len as u64)
        .sum::<u64>();
    assert_eq!(work.authorized_pack_bytes(), fixture.pack.len() as u64);
    assert_eq!(work.primary_authenticated_bytes(), work.authorized_pack_bytes(),);
    assert_eq!(work.physical_read_bytes(), physical);
    assert_eq!(work.prior_key_reread_bytes(), fixture.expected_prior_key_reread_bytes,);
    assert_eq!(work.authenticated_source_replay_bytes(), authenticated_source_replay_bytes,);
    assert_eq!(work.layout_application_replay_bytes(), layout_application_replay_bytes,);
    assert_eq!(
        work.physical_read_bytes(),
        work.primary_authenticated_bytes() +
            work.prior_key_reread_bytes() +
            work.authenticated_source_replay_bytes() +
            work.layout_application_replay_bytes(),
    );

    let (exact_result, exact_trace) =
        scan_with_physical_read_limit(&fixture, work.physical_read_bytes());
    exact_result.unwrap();
    let successful_trace = fragments.lock().unwrap().clone();
    assert_eq!(exact_trace, successful_trace);
    let (source_replay_start, prior_key_reads) =
        primary_trace_boundaries(&successful_trace, work.authorized_pack_bytes());
    assert_eq!(
        prior_key_reads
            .iter()
            .map(|index| successful_trace[*index].len as u64)
            .sum::<u64>(),
        work.prior_key_reread_bytes(),
    );
    assert_eq!(
        successful_trace[..source_replay_start]
            .iter()
            .map(|read| read.len as u64)
            .sum::<u64>(),
        work.primary_authenticated_bytes() + work.prior_key_reread_bytes(),
    );
    let last_prior_key_read = *prior_key_reads
        .last()
        .expect("the multi-entry fixture must exercise a prior-key reread");
    let source_replay_end = exact_contiguous_replay_end(
        &successful_trace,
        source_replay_start,
        fixture.prepare_offset..fixture.prepare_offset + authenticated_source_replay_bytes,
    );
    assert!(source_replay_end < successful_trace.len());
    assert_eq!(
        successful_trace[source_replay_end..]
            .iter()
            .map(|read| read.len as u64)
            .sum::<u64>(),
        layout_application_replay_bytes,
    );
    assert_physical_limit_rejects_before_trace_event(
        &fixture,
        &successful_trace,
        last_prior_key_read,
    );
    assert_physical_limit_rejects_before_trace_event(
        &fixture,
        &successful_trace,
        source_replay_end - 1,
    );
    assert_physical_limit_rejects_before_trace_event(
        &fixture,
        &successful_trace,
        successful_trace.len() - 1,
    );
}

#[test]
fn recovery_physical_work_is_semantic_comm_neutral() {
    let source = include_str!("../../src/rspace/persistence/durable/recovery.rs");
    for capability in
        ["RuntimeBudget", "BillableKind", "reserve_comm", "cost_trace_digest", "storage_charge"]
    {
        assert!(!source.contains(capability));
    }
    let manifest = include_str!("../../Cargo.toml");
    assert!(
        manifest
            .lines()
            .map(str::trim)
            .all(|line| { !line.starts_with("rholang =") && line != "package = \"rholang\"" })
    );
    let fixture = recovery_fixture(3);
    scan(&fixture, 47, None, LogPackDecodeLimits::UNBOUNDED)
        .0
        .unwrap();
}

#[test]
fn prepare_header_checksum_encoding_and_232_byte_domain_are_exact() {
    let fixture = recovery_fixture(2);
    let start = fixture.prepare_offset as usize;
    let prefix = &fixture.pack[start..start + TRANSACTION_PREPARE_HEADER_LEN_V1 as usize];
    let expected = crate::rspace::persistence::checksum::crc32c(&prefix[..PREPARE_AUTH_HEADER_LEN]);
    assert_eq!(&prefix[PREPARE_AUTH_HEADER_LEN..], &u64::from(expected).to_le_bytes(),);
    for bit in 32..64 {
        let mut corrupt = prefix.to_vec();
        corrupt[PREPARE_AUTH_HEADER_LEN..]
            .copy_from_slice(&(u64::from(expected) | (1u64 << bit)).to_le_bytes());
        assert!(matches!(
            preflight_prepare_record_v1(&corrupt, LogPackDecodeLimits::UNBOUNDED),
            Err(LogPackFormatError::ChecksumOutOfRange {
                field: "prepare header",
                ..
            })
        ));
    }
    for offset in [0usize, 15, 16, 31, 32, 231] {
        let mut corrupt = prefix.to_vec();
        corrupt[offset] ^= 1;
        assert!(matches!(
            preflight_prepare_record_v1(&corrupt, LogPackDecodeLimits::UNBOUNDED),
            Err(LogPackFormatError::HeaderChecksum { .. })
        ));
    }
}

#[test]
fn prepare_record_checksum_encoding_and_prefix_payload_domain_are_exact() {
    let fixture = recovery_fixture(2);
    let start = fixture.prepare_offset as usize;
    let end = start + fixture.prepare_len as usize;
    let prepare = &fixture.pack[start..end];
    let payload_end = prepare.len() - CHECKSUM_LEN;
    let expected = crate::rspace::persistence::checksum::crc32c(&prepare[..payload_end]);
    assert_eq!(&prepare[payload_end..], &u64::from(expected).to_le_bytes());
    for bit in 32..64 {
        let mut corrupt = prepare.to_vec();
        corrupt[payload_end..]
            .copy_from_slice(&(u64::from(expected) | (1u64 << bit)).to_le_bytes());
        assert!(matches!(
            TransactionPrepareRecordViewV1::parse(&corrupt, LogPackDecodeLimits::UNBOUNDED),
            Err(LogPackFormatError::ChecksumOutOfRange {
                field: "prepare record",
                ..
            })
        ));
    }
    for offset in [TRANSACTION_PREPARE_HEADER_LEN_V1 as usize, payload_end / 2, payload_end - 1] {
        let mut corrupt = prepare.to_vec();
        corrupt[offset] ^= 1;
        assert!(matches!(
            TransactionPrepareRecordViewV1::parse(&corrupt, LogPackDecodeLimits::UNBOUNDED),
            Err(LogPackFormatError::RecordChecksum { .. })
        ));
    }
    let mut corrupt_prefix = prepare.to_vec();
    corrupt_prefix[80] ^= 1;
    repair_prepare_header_crc(&mut corrupt_prefix);
    let result =
        TransactionPrepareRecordViewV1::parse(&corrupt_prefix, LogPackDecodeLimits::UNBOUNDED);
    assert!(
        matches!(result, Err(LogPackFormatError::MutationReceipt { .. })),
        "repaired header mutation produced {result:?} instead of the exact semantic binding error"
    );
}

#[test]
fn kind1_error_precedence_is_io_then_crc_then_first_semantic() {
    let semantic = semantic_fault_fixture(true);
    let fail_at = semantic.prepare_offset + TRANSACTION_PREPARE_HEADER_LEN_V1 + 1;
    assert!(matches!(
        scan(
            &semantic,
            1,
            Some(fail_at),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .0,
        Err(RecoveryError::Read(TraceReadError::Injected(offset))) if offset == fail_at
    ));

    let crc = semantic_fault_fixture(false);
    assert!(matches!(
        scan(&crc, 41, None, LogPackDecodeLimits::UNBOUNDED).0,
        Err(RecoveryError::Wal(LogPackFormatError::RecordChecksum { .. }))
    ));

    assert!(matches!(
        scan(&semantic, 41, None, LogPackDecodeLimits::UNBOUNDED).0,
        Err(RecoveryError::Wal(LogPackFormatError::Mutation(MutationError::Canonical(
            CanonicalFormatError::InvalidDomain { .. }
        ))))
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn kind1_physical_digest_is_exact_chunk_fold_over_crc_domain(width in 1usize..1025) {
        let fixture = recovery_fixture(3);
        let start = fixture.prepare_offset as usize;
        let end = start + fixture.prepare_len as usize - CHECKSUM_LEN;
        let domain = &fixture.pack[start..end];
        let prepare = TransactionPrepareRecordViewV1::parse(
            &fixture.pack[start..end + CHECKSUM_LEN],
            LogPackDecodeLimits::UNBOUNDED,
        ).unwrap();
        let mut hasher = Blake2b::<U32>::new();
        for chunk in domain.chunks(width) {
            hasher.update(chunk);
        }
        let folded: [u8; 32] = hasher.finalize().into();
        prop_assert_eq!(*prepare.digest().as_bytes(), folded);
        prop_assert_eq!(prepare.digest(), PhysicalDigest::of_bytes(domain));
    }
}

#[test]
fn computed_prepare_digest_has_no_authority_until_exact_commit_binding() {
    let fixture = recovery_fixture(2);
    let start = fixture.prepare_offset as usize;
    let end = start + fixture.prepare_len as usize;
    let prepare = TransactionPrepareRecordViewV1::parse(
        &fixture.pack[start..end],
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    assert_eq!(
        prepare.digest(),
        PhysicalDigest::of_bytes(&fixture.pack[start..end - CHECKSUM_LEN]),
    );
    let commit_end = end + TRANSACTION_COMMIT_RECORD_LEN_V1 as usize;
    TransactionCommitViewV1::parse(&fixture.pack[end..commit_end])
        .unwrap()
        .verify_prepare(prepare)
        .unwrap();

    for (offset, field) in
        [(40usize, "sequence"), (48, "store"), (112, "length"), (120, "digest"), (200, "receipt")]
    {
        let mut commit = fixture.pack[end..commit_end].to_vec();
        if offset == 40 {
            commit[offset..offset + 8].copy_from_slice(&2_u64.to_le_bytes());
        } else {
            commit[offset] ^= 1;
        }
        repair_commit_crc(&mut commit);
        assert!(
            TransactionCommitViewV1::parse(&commit)
                .unwrap()
                .verify_prepare(prepare)
                .is_err(),
            "computed PREPARE digest acquired authority despite changed {field}",
        );
    }

    let mut rebound = fixture.pack.to_vec();
    rebound[end + 120] ^= 1;
    repair_commit_crc(&mut rebound[end..commit_end]);
    let rebound_commit = TransactionCommitViewV1::parse(&rebound[end..commit_end]).unwrap();
    let rebound = RecoveryFixture {
        pack: rebound.into(),
        catalog: catalog_for_commit(fixture.catalog, rebound_commit),
        ..fixture
    };
    assert!(matches!(
        scan(&rebound, 53, None, LogPackDecodeLimits::UNBOUNDED).0,
        Err(RecoveryError::Wal(LogPackFormatError::RecordDigest { .. }))
    ));

    let fixture = recovery_fixture(2);
    let end = fixture.prepare_offset as usize + fixture.prepare_len as usize;
    let commit_end = end + TRANSACTION_COMMIT_RECORD_LEN_V1 as usize;
    let mut shifted = fixture.pack.to_vec();
    shifted[end + 104..end + 112].copy_from_slice(&(fixture.prepare_offset + 1).to_le_bytes());
    repair_commit_crc(&mut shifted[end..commit_end]);
    let shifted_commit = TransactionCommitViewV1::parse(&shifted[end..commit_end]).unwrap();
    let shifted = RecoveryFixture {
        pack: shifted.into(),
        catalog: catalog_for_commit(fixture.catalog, shifted_commit),
        ..fixture
    };
    assert!(matches!(
        scan(&shifted, 53, None, LogPackDecodeLimits::UNBOUNDED).0,
        Err(RecoveryError::PrepareAddress { .. })
    ));
}
