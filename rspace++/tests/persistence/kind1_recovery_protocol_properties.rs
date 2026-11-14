use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::process::Command;
use std::sync::Arc;

use super::durable_recovery::limits;
use super::support::{TEST_SEMANTIC_PROFILE_ID, TestSemanticProfile, encode, synthetic_ref};
use super::transaction_sealing::{base_state, data_batch, seal_data};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::internal::Datum;
use crate::rspace::persistence::checksum::{crc32c, encode_crc32c};
use crate::rspace::persistence::durable::{
    CatalogPackProviderV1, CommittedObjectIndexV1, DurableReadAt, RecoveryError,
    replay_catalog_authorized_pack_chain, scan_catalog_authorized_pack_chain,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CanonicalObjectStore, CatalogAuthorityTokenV1,
    CatalogPackStateV1, CatalogSlotV1, CommitPublicationV1, CommittedObjectComparison,
    CommittedObjectLookup, INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexGenerationV1,
    LogPackAddress, LogPackDecodeLimits, LogPackFormatError, LogPackObjectDispositionV1,
    LogPackObjectPlanV1, LogPackSequence, MutableObjectStore, ObjectAdmission, ObjectDigest,
    ObjectIndexRecordV1, ObjectLocationV1, ObjectRefV1, PACK_HEADER_LEN_V1, PackHeaderV1, PackId,
    PhysicalDigest, RSpaceMutationBatchV1, StateVersionV1, StoreId,
    TRANSACTION_COMMIT_RECORD_LEN_V1, TRANSACTION_PREPARE_HEADER_LEN_V1, TransactionCommitRecordV1,
    TransactionCommitViewV1, TransactionPrepareRecordViewV1, preflight_prepare_record_v1,
};

const PREPARE_AUTH_LEN: usize = 232;
const PREPARE_CRC_OFFSET: usize = 232;
const PREPARE_SEQUENCE_OFFSET: usize = 40;
const PREPARE_STORE_OFFSET: usize = 48;
const PREPARE_BASE_OFFSET: usize = 64;
const PREPARE_TARGET_OFFSET: usize = 112;
const PREPARE_PROFILE_OFFSET: usize = 160;
const PREPARE_MUTATION_DIGEST_OFFSET: usize = 176;
const PREPARE_MUTATION_LEN_OFFSET: usize = 208;
const PREPARE_OBJECT_COUNT_OFFSET: usize = 216;
const PREPARE_OBJECT_BYTES_OFFSET: usize = 224;
const COMMIT_AUTH_LEN: usize = 376;
const COMMIT_SEQUENCE_OFFSET: usize = 40;
const COMMIT_STORE_OFFSET: usize = 48;
const COMMIT_PREVIOUS_OFFSET: usize = 64;
const COMMIT_PREPARE_OFFSET_OFFSET: usize = 104;
const COMMIT_PREPARE_LENGTH_OFFSET: usize = 112;
const COMMIT_PREPARE_DIGEST_OFFSET: usize = 120;
const COMMIT_BASE_OFFSET: usize = 152;
const COMMIT_TARGET_OFFSET: usize = 200;
const COMMIT_PROFILE_OFFSET: usize = 248;
const COMMIT_MUTATION_DIGEST_OFFSET: usize = 264;
const COMMIT_INDEX_TAIL_OFFSET: usize = 296;
const COMMIT_FIRST_LIVE_OFFSET: usize = 304;
const COMMIT_INDEX_GENERATION_OFFSET: usize = 312;
const MUTATION_BASE_GENERATION_OFFSET: usize = 40;
const MUTATION_TARGET_GENERATION_OFFSET: usize = 48;
const MUTATION_BASE_REFERENCE_OFFSET: usize = 56;
const OBJECT_RECORD_HEADER_LEN: usize = 48;

thread_local! {
    static COUNT_ALLOCATIONS: Cell<bool> = const { Cell::new(false) };
    static ALLOCATION_COUNT: Cell<u64> = const { Cell::new(0) };
    static ALLOCATION_BYTES: Cell<u64> = const { Cell::new(0) };
    static ALLOCATION_MAX_SINGLE_BYTES: Cell<u64> = const { Cell::new(0) };
    static ALLOCATION_LIVE_BYTES: Cell<u64> = const { Cell::new(0) };
    static ALLOCATION_PEAK_LIVE_BYTES: Cell<u64> = const { Cell::new(0) };
}

fn record_allocation(bytes: usize) {
    let bytes = bytes as u64;
    ALLOCATION_COUNT.with(|count| count.set(count.get().saturating_add(1)));
    ALLOCATION_BYTES.with(|total| total.set(total.get().saturating_add(bytes)));
    ALLOCATION_MAX_SINGLE_BYTES.with(|maximum| maximum.set(maximum.get().max(bytes)));
    ALLOCATION_LIVE_BYTES.with(|live| {
        let next = live.get().saturating_add(bytes);
        live.set(next);
        ALLOCATION_PEAK_LIVE_BYTES.with(|peak| peak.set(peak.get().max(next)));
    });
}

fn record_deallocation(bytes: usize) {
    ALLOCATION_LIVE_BYTES.with(|live| live.set(live.get().saturating_sub(bytes as u64)));
}

struct RecoveryCountingAllocator;

unsafe impl GlobalAlloc for RecoveryCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let _ = COUNT_ALLOCATIONS.try_with(|enabled| {
                if enabled.get() {
                    record_allocation(layout.size());
                }
            });
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let _ = COUNT_ALLOCATIONS.try_with(|enabled| {
            if enabled.get() {
                record_deallocation(layout.size());
            }
        });
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            let _ = COUNT_ALLOCATIONS.try_with(|enabled| {
                if enabled.get() {
                    record_allocation(layout.size());
                }
            });
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !pointer.is_null() {
            let _ = COUNT_ALLOCATIONS.try_with(|enabled| {
                if enabled.get() {
                    record_deallocation(layout.size());
                    record_allocation(new_size);
                }
            });
        }
        pointer
    }
}

#[global_allocator]
static RECOVERY_ALLOCATOR: RecoveryCountingAllocator = RecoveryCountingAllocator;

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

struct EqualLookup;

impl CommittedObjectLookup for EqualLookup {
    type Error = Infallible;

    fn compare_committed_object(
        &self,
        _reference: ObjectRefV1,
        _sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error> {
        Ok(CommittedObjectComparison::Equal)
    }
}

struct KnownObjects(BTreeMap<ObjectDigest, Arc<[u8]>>);

impl CommittedObjectLookup for KnownObjects {
    type Error = Infallible;

    fn compare_committed_object(
        &self,
        reference: ObjectRefV1,
        sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error> {
        Ok(match self.0.get(&reference.digest()) {
            None => CommittedObjectComparison::Missing,
            Some(committed) if committed.as_ref() == sealed_canonical_bytes => {
                CommittedObjectComparison::Equal
            }
            Some(_) => CommittedObjectComparison::Collision,
        })
    }
}

#[derive(Clone)]
struct MemoryReader {
    bytes: Arc<[u8]>,
}

impl DurableReadAt for MemoryReader {
    type Error = Infallible;

    fn durable_len(&self) -> Result<u64, Self::Error> { Ok(self.bytes.len() as u64) }

    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> Result<(), Self::Error> {
        let offset = offset as usize;
        destination.copy_from_slice(&self.bytes[offset..offset + destination.len()]);
        Ok(())
    }
}

struct MemoryProvider {
    bytes: Arc<[u8]>,
}

impl CatalogPackProviderV1 for MemoryProvider {
    type Reader = MemoryReader;
    type Error = Infallible;

    fn open_pack(&mut self, pack_id: PackId) -> Result<Self::Reader, Self::Error> {
        assert_eq!(pack_id, PackId::GENESIS);
        Ok(MemoryReader {
            bytes: Arc::clone(&self.bytes),
        })
    }
}

struct Kind1Image {
    pack: Arc<[u8]>,
    catalog: CatalogSlotV1,
    first_prepare_offset: usize,
    first_commit_offset: usize,
    middle_prepare_offset: usize,
    second_prepare_offset: usize,
    second_commit_offset: usize,
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn write_reference(bytes: &mut [u8], offset: usize, reference: ObjectRefV1) {
    bytes[offset..offset + 32].copy_from_slice(reference.digest().as_bytes());
    write_u64(bytes, offset + 32, reference.encoded_len());
}

fn write_state(bytes: &mut [u8], offset: usize, state: StateVersionV1) {
    write_u64(bytes, offset, state.generation());
    write_reference(bytes, offset + 8, state.manifest());
}

fn token(slot: CatalogSlotV1, second: bool) -> CatalogAuthorityTokenV1 {
    let mut image = [0u8; CATALOG_IMAGE_LEN];
    let offset = usize::from(second) * CATALOG_SLOT_LEN;
    image[offset..offset + CATALOG_SLOT_LEN].copy_from_slice(&slot.encode());
    CatalogAuthorityTokenV1::select(image).unwrap().unwrap()
}

fn prepare_len(pack: &[u8], prepare_offset: usize) -> usize {
    read_u64(pack, prepare_offset + 32) as usize
}

fn repair_commit_crc(pack: &mut [u8], commit_offset: usize) {
    let crc = crc32c(&pack[commit_offset..commit_offset + COMMIT_AUTH_LEN]);
    pack[commit_offset + COMMIT_AUTH_LEN..commit_offset + COMMIT_AUTH_LEN + 8]
        .copy_from_slice(&encode_crc32c(crc));
}

fn repair_prepare_and_commit_binding(pack: &mut [u8], prepare_offset: usize, commit_offset: usize) {
    let mutation_len = read_u64(pack, prepare_offset + PREPARE_MUTATION_LEN_OFFSET) as usize;
    let mutation_offset = prepare_offset + TRANSACTION_PREPARE_HEADER_LEN_V1 as usize;
    let mutation_digest =
        ObjectDigest::of_canonical_bytes(&pack[mutation_offset..mutation_offset + mutation_len]);
    pack[prepare_offset + PREPARE_MUTATION_DIGEST_OFFSET..
        prepare_offset + PREPARE_MUTATION_DIGEST_OFFSET + 32]
        .copy_from_slice(mutation_digest.as_bytes());
    pack[commit_offset + COMMIT_MUTATION_DIGEST_OFFSET..
        commit_offset + COMMIT_MUTATION_DIGEST_OFFSET + 32]
        .copy_from_slice(mutation_digest.as_bytes());

    let header_crc = crc32c(&pack[prepare_offset..prepare_offset + PREPARE_AUTH_LEN]);
    pack[prepare_offset + PREPARE_CRC_OFFSET..prepare_offset + PREPARE_CRC_OFFSET + 8]
        .copy_from_slice(&encode_crc32c(header_crc));
    let length = prepare_len(pack, prepare_offset);
    let trailer = prepare_offset + length - 8;
    let record_crc = crc32c(&pack[prepare_offset..trailer]);
    pack[trailer..trailer + 8].copy_from_slice(&encode_crc32c(record_crc));
    let digest = PhysicalDigest::of_bytes(&pack[prepare_offset..trailer]);
    pack[commit_offset + COMMIT_PREPARE_DIGEST_OFFSET..
        commit_offset + COMMIT_PREPARE_DIGEST_OFFSET + 32]
        .copy_from_slice(digest.as_bytes());
    repair_commit_crc(pack, commit_offset);
}

fn repair_prepare_physical_binding(pack: &mut [u8], prepare_offset: usize, commit_offset: usize) {
    let header_crc = crc32c(&pack[prepare_offset..prepare_offset + PREPARE_AUTH_LEN]);
    pack[prepare_offset + PREPARE_CRC_OFFSET..prepare_offset + PREPARE_CRC_OFFSET + 8]
        .copy_from_slice(&encode_crc32c(header_crc));
    let length = prepare_len(pack, prepare_offset);
    let trailer = prepare_offset + length - 8;
    let record_crc = crc32c(&pack[prepare_offset..trailer]);
    pack[trailer..trailer + 8].copy_from_slice(&encode_crc32c(record_crc));
    let digest = PhysicalDigest::of_bytes(&pack[prepare_offset..trailer]);
    pack[commit_offset + COMMIT_PREPARE_DIGEST_OFFSET..
        commit_offset + COMMIT_PREPARE_DIGEST_OFFSET + 32]
        .copy_from_slice(digest.as_bytes());
    repair_commit_crc(pack, commit_offset);
}

fn patch_second_transition_base(pack: &mut [u8], image: &Kind1Image, base: StateVersionV1) {
    let target = StateVersionV1::new(base.generation() + 1, base.manifest());
    let prepare = image.second_prepare_offset;
    let commit = image.second_commit_offset;
    let mutation = prepare + TRANSACTION_PREPARE_HEADER_LEN_V1 as usize;
    write_u64(pack, mutation + MUTATION_BASE_GENERATION_OFFSET, base.generation());
    write_u64(pack, mutation + MUTATION_TARGET_GENERATION_OFFSET, target.generation());
    write_reference(pack, mutation + MUTATION_BASE_REFERENCE_OFFSET, base.manifest());
    write_state(pack, prepare + PREPARE_BASE_OFFSET, base);
    write_state(pack, prepare + PREPARE_TARGET_OFFSET, target);
    write_state(pack, commit + COMMIT_BASE_OFFSET, base);
    write_state(pack, commit + COMMIT_TARGET_OFFSET, target);
    repair_prepare_and_commit_binding(pack, prepare, commit);
}

fn kind1_image(identity: &str) -> Kind1Image { kind1_image_with_cardinality(identity, 1) }

fn data_batch_with_cardinality(
    base: StateVersionV1,
    identity: &str,
    cardinality: usize,
) -> Vec<u8> {
    assert!(cardinality > 0);
    let channel = "channel".to_owned();
    let mut batch = RSpaceMutationBatchV1::new(base).unwrap();
    for ordinal in 0..cardinality {
        let datum = Datum::create(&channel, format!("{identity}-{ordinal:08}"), false);
        let path = RSpaceDataIndex::datum_path(&channel, &datum);
        batch.record_data(&path, 0, 1).unwrap();
    }
    encode(&batch.plan().unwrap())
}

fn kind1_image_with_cardinality(identity: &str, cardinality: usize) -> Kind1Image {
    let object_store = CanonicalObjectStore::new();
    let base = base_state(&object_store, 17, None);
    let mutation = data_batch_with_cardinality(base, identity, cardinality + 1);
    let sealed = seal_data(&object_store, &mutation);
    let store_id = StoreId::from_array([0x4b; 16]);
    let first_sequence = LogPackSequence::FIRST;
    let first =
        crate::rspace::persistence::PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
            &sealed,
            LogPackObjectPlanV1::classify(&sealed, &MissingLookup).unwrap(),
            store_id,
            first_sequence,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap();
    let mut pack = PackHeaderV1::genesis(store_id).encode().to_vec();
    let first_prepare_offset = pack.len();
    let first_identity = first.write_to(&mut pack).unwrap();
    let first_prepare_address = LogPackAddress::from_pack(
        PackId::GENESIS,
        first_prepare_offset as u64,
        first_identity.record_len(),
    )
    .unwrap();
    let first_index_tail = INDEX_HEADER_LEN_V1 + first.inline_count() * INDEX_RECORD_LEN_V1;
    let first_publication = CommitPublicationV1::new(
        PackId::GENESIS.get(),
        IndexGenerationV1::FIRST.get(),
        first_index_tail,
        synthetic_ref(0xa1),
        4096,
        8192,
    )
    .unwrap();
    let first_commit = TransactionCommitRecordV1::new(
        sealed.receipt(),
        store_id,
        PhysicalDigest::ZERO,
        first_prepare_address,
        first_identity,
        first_publication,
    )
    .unwrap();
    let first_commit_offset = pack.len();
    pack.extend_from_slice(first_commit.canonical_bytes());

    let mut known = BTreeMap::new();
    for object in sealed.objects() {
        let bytes: Arc<[u8]> = object.canonical_bytes().into();
        assert!(matches!(
            object_store
                .put_if_absent_or_equal(object.reference(), Arc::clone(&bytes))
                .unwrap(),
            ObjectAdmission::Inserted | ObjectAdmission::AlreadyPresent
        ));
        known.insert(object.reference().digest(), bytes);
    }
    let channel = "channel".to_owned();
    let removed = Datum::create(&channel, format!("{identity}-{cardinality:08}"), false);
    let removed_path = RSpaceDataIndex::datum_path(&channel, &removed);
    let mut second_batch = RSpaceMutationBatchV1::new(sealed.receipt().target()).unwrap();
    second_batch.record_data(&removed_path, 1, 0).unwrap();
    let second_mutation = encode(&second_batch.plan().unwrap());
    let second_sealed = seal_data(&object_store, &second_mutation);
    let second_plan =
        LogPackObjectPlanV1::classify(&second_sealed, &KnownObjects(known.clone())).unwrap();
    assert!(second_plan.inline_count() > 0);
    for object in second_sealed.objects() {
        let bytes: Arc<[u8]> = object.canonical_bytes().into();
        let _ = object_store
            .put_if_absent_or_equal(object.reference(), Arc::clone(&bytes))
            .unwrap();
        known.insert(object.reference().digest(), bytes);
    }
    let mut third_batch = RSpaceMutationBatchV1::new(second_sealed.receipt().target()).unwrap();
    third_batch.record_data(&removed_path, 0, 1).unwrap();
    let successor = Datum::create(&channel, format!("{identity}-successor"), false);
    let successor_path = RSpaceDataIndex::datum_path(&channel, &successor);
    third_batch.record_data(&successor_path, 0, 1).unwrap();
    let third_mutation = encode(&third_batch.plan().unwrap());
    let third_sealed = seal_data(&object_store, &third_mutation);
    let third_plan = LogPackObjectPlanV1::classify(&third_sealed, &KnownObjects(known)).unwrap();
    assert!(third_plan.inline_count() > 0);
    assert!(
        third_plan.inline_count() < third_plan.len() as u64,
        "third plan inline {} of {}",
        third_plan.inline_count(),
        third_plan.len()
    );

    let middle_sequence = first_sequence.successor().unwrap();
    let middle =
        crate::rspace::persistence::PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
            &second_sealed,
            second_plan,
            store_id,
            middle_sequence,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap();
    let middle_prepare_offset = pack.len();
    let middle_identity = middle.write_to(&mut pack).unwrap();
    let middle_prepare_address = LogPackAddress::from_pack(
        PackId::GENESIS,
        middle_prepare_offset as u64,
        middle_identity.record_len(),
    )
    .unwrap();
    let middle_index_tail = first_index_tail + middle.inline_count() * INDEX_RECORD_LEN_V1;
    let middle_publication = CommitPublicationV1::new(
        PackId::GENESIS.get(),
        IndexGenerationV1::FIRST.get(),
        middle_index_tail,
        synthetic_ref(0xa2),
        4096,
        8192,
    )
    .unwrap();
    let middle_commit = TransactionCommitRecordV1::new(
        second_sealed.receipt(),
        store_id,
        first_commit.digest(),
        middle_prepare_address,
        middle_identity,
        middle_publication,
    )
    .unwrap();
    pack.extend_from_slice(middle_commit.canonical_bytes());

    let second_sequence = middle_sequence.successor().unwrap();
    let second =
        crate::rspace::persistence::PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
            &third_sealed,
            third_plan,
            store_id,
            second_sequence,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap();
    let second_prepare_offset = pack.len();
    let second_identity = second.write_to(&mut pack).unwrap();
    let second_prepare_address = LogPackAddress::from_pack(
        PackId::GENESIS,
        second_prepare_offset as u64,
        second_identity.record_len(),
    )
    .unwrap();
    let second_index_tail = middle_index_tail + second.inline_count() * INDEX_RECORD_LEN_V1;
    let second_publication = CommitPublicationV1::new(
        PackId::GENESIS.get(),
        IndexGenerationV1::FIRST.get(),
        second_index_tail,
        synthetic_ref(0xa3),
        4096,
        8192,
    )
    .unwrap();
    let second_commit = TransactionCommitRecordV1::new(
        third_sealed.receipt(),
        store_id,
        middle_commit.digest(),
        second_prepare_address,
        second_identity,
        second_publication,
    )
    .unwrap();
    let second_commit_offset = pack.len();
    pack.extend_from_slice(second_commit.canonical_bytes());

    let terminal = TransactionCommitViewV1::parse(
        &pack[second_commit_offset..
            second_commit_offset + TRANSACTION_COMMIT_RECORD_LEN_V1 as usize],
    )
    .unwrap();
    let terminal_address = LogPackAddress::from_pack(
        PackId::GENESIS,
        second_commit_offset as u64,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )
    .unwrap();
    let pack_state = CatalogPackStateV1::new(
        PackId::GENESIS.get(),
        PackId::GENESIS.get(),
        terminal_address.end(),
        IndexGenerationV1::FIRST.get(),
        second_index_tail,
        terminal.publication().retention_manifest(),
        terminal.publication().current_state_canonical_bytes(),
        terminal.publication().retained_canonical_bytes(),
    )
    .unwrap();
    let catalog = CatalogSlotV1::new(
        store_id,
        1,
        terminal.receipt().target(),
        TEST_SEMANTIC_PROFILE_ID,
        terminal.digest(),
        second_sequence,
        terminal_address,
        pack_state,
    )
    .unwrap();
    Kind1Image {
        pack: pack.into(),
        catalog,
        first_prepare_offset,
        first_commit_offset,
        middle_prepare_offset,
        second_prepare_offset,
        second_commit_offset,
    }
}

fn generation_start_reuse_image(identity: &str) -> (Arc<[u8]>, CatalogSlotV1) {
    let object_store = CanonicalObjectStore::new();
    let base = base_state(&object_store, 5, None);
    let mutation = data_batch(base, identity);
    let sealed = seal_data(&object_store, &mutation);
    let store_id = StoreId::from_array([0x6d; 16]);
    let prepared =
        crate::rspace::persistence::PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
            &sealed,
            LogPackObjectPlanV1::classify(&sealed, &EqualLookup).unwrap(),
            store_id,
            LogPackSequence::FIRST,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap();
    let mut pack = PackHeaderV1::genesis(store_id).encode().to_vec();
    let prepare_offset = pack.len() as u64;
    let identity = prepared.write_to(&mut pack).unwrap();
    let prepare_address =
        LogPackAddress::from_pack(PackId::GENESIS, prepare_offset, identity.record_len()).unwrap();
    let publication =
        CommitPublicationV1::new(1, 1, INDEX_HEADER_LEN_V1, synthetic_ref(0xb1), 4096, 8192)
            .unwrap();
    let commit = TransactionCommitRecordV1::new(
        sealed.receipt(),
        store_id,
        PhysicalDigest::ZERO,
        prepare_address,
        identity,
        publication,
    )
    .unwrap();
    let commit_address = LogPackAddress::from_pack(
        PackId::GENESIS,
        pack.len() as u64,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )
    .unwrap();
    pack.extend_from_slice(commit.canonical_bytes());
    let pack_state = CatalogPackStateV1::new(
        1,
        1,
        commit_address.end(),
        1,
        INDEX_HEADER_LEN_V1,
        publication.retention_manifest(),
        publication.current_state_canonical_bytes(),
        publication.retained_canonical_bytes(),
    )
    .unwrap();
    let catalog = CatalogSlotV1::new(
        store_id,
        1,
        sealed.receipt().target(),
        TEST_SEMANTIC_PROFILE_ID,
        commit.digest(),
        LogPackSequence::FIRST,
        commit_address,
        pack_state,
    )
    .unwrap();
    (pack.into(), catalog)
}

fn prepare_view(pack: &[u8], offset: usize) -> TransactionPrepareRecordViewV1<'_> {
    let length = prepare_len(pack, offset);
    TransactionPrepareRecordViewV1::parse(
        &pack[offset..offset + length],
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap()
}

fn recovery_error(pack: Arc<[u8]>, catalog: CatalogSlotV1) -> RecoveryError<Infallible> {
    let mut provider = MemoryProvider { bytes: pack };
    let result = scan_catalog_authorized_pack_chain(
        &mut provider,
        &token(catalog, false),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    );
    match result {
        Err(error) => error,
        Ok(_) => panic!("malformed kind-1 image was accepted"),
    }
}

fn exact_contiguous_prepare_error(pack: &[u8], prepare_offset: usize) -> LogPackFormatError {
    let length = prepare_len(pack, prepare_offset);
    TransactionPrepareRecordViewV1::parse(
        &pack[prepare_offset..prepare_offset + length],
        LogPackDecodeLimits::UNBOUNDED,
    )
    .expect_err("the malformed PREPARE must be rejected by the contiguous oracle")
}

fn assert_streaming_matches_contiguous_prepare_error(
    pack: Vec<u8>,
    image: &Kind1Image,
    prepare_offset: usize,
) {
    let expected = exact_contiguous_prepare_error(&pack, prepare_offset);
    match recovery_error(pack.into(), image.catalog) {
        RecoveryError::Wal(actual) => assert_eq!(actual, expected),
        _ => panic!("streaming recovery did not preserve the contiguous PREPARE error class"),
    }
}

fn strict_order_preserving_replacement(
    objects: &[crate::rspace::persistence::LogPackObjectView<'_>],
    ordinal: usize,
) -> ObjectDigest {
    let original = objects[ordinal].reference().digest();
    let previous = ordinal
        .checked_sub(1)
        .map(|index| objects[index].reference().digest());
    let next = objects
        .get(ordinal + 1)
        .map(|object| object.reference().digest());
    for byte_index in (0..32).rev() {
        for bit in 0..8 {
            let mut bytes: [u8; 32] = original.as_bytes().try_into().unwrap();
            bytes[byte_index] ^= 1 << bit;
            let candidate = ObjectDigest::from_array(bytes);
            if candidate != original &&
                previous.is_none_or(|digest| digest < candidate) &&
                next.is_none_or(|digest| candidate < digest) &&
                objects
                    .iter()
                    .all(|object| object.reference().digest() != candidate)
            {
                return candidate;
            }
        }
    }
    panic!("an order-preserving replacement digest must exist")
}

#[test]
fn streamed_mutation_identity_matches_receipt_or_rejects_each_binding() {
    let image = kind1_image("streamed-mutation-receipt-binding");
    let cases = [
        (PREPARE_MUTATION_DIGEST_OFFSET, COMMIT_MUTATION_DIGEST_OFFSET),
        (PREPARE_BASE_OFFSET, COMMIT_BASE_OFFSET),
        (PREPARE_TARGET_OFFSET, COMMIT_TARGET_OFFSET),
    ];
    for (prepare_field, commit_field) in cases {
        let mut pack = image.pack.to_vec();
        pack[image.first_prepare_offset + prepare_field] ^= 1;
        pack[image.first_commit_offset + commit_field] ^= 1;
        repair_prepare_physical_binding(
            &mut pack,
            image.first_prepare_offset,
            image.first_commit_offset,
        );
        let mut provider = MemoryProvider { bytes: pack.into() };
        let result = scan_catalog_authorized_pack_chain(
            &mut provider,
            &token(image.catalog, false),
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("mismatched streamed mutation identity was accepted"),
        };
        if prepare_field == PREPARE_MUTATION_DIGEST_OFFSET {
            assert!(matches!(error, RecoveryError::Wal(LogPackFormatError::MutationDigest { .. })));
        } else {
            assert!(matches!(
                error,
                RecoveryError::Wal(LogPackFormatError::MutationReceipt { .. })
            ));
        }
    }
}

#[test]
fn kind1_object_layout_parser_enforces_count_disposition_reference_order_and_body_shape() {
    let image = kind1_image("object-layout-pda");
    let second = prepare_view(&image.pack, image.second_prepare_offset);
    let objects = second.objects().collect::<Vec<_>>();
    let declared_count =
        read_u64(&image.pack, image.second_prepare_offset + PREPARE_OBJECT_COUNT_OFFSET);
    assert_eq!(objects.len() as u64, declared_count);
    assert!(
        objects
            .windows(2)
            .all(|pair| { pair[0].reference().digest() < pair[1].reference().digest() })
    );
    let first_record = image.second_prepare_offset + objects[0].record_offset() as usize;

    // The disposition is an encoded u64, but every non-DATA/non-REUSE tag in
    // the complete one-byte generator domain must preserve the exact oracle
    // error. Boundary values cover the remaining wider encodings.
    for kind in (0u64..=u8::MAX as u64)
        .chain([u64::from(u32::MAX), u64::MAX])
        .filter(|kind| {
            *kind != LogPackObjectDispositionV1::Data.kind() &&
                *kind != LogPackObjectDispositionV1::Reuse.kind()
        })
    {
        let mut malformed = image.pack.to_vec();
        write_u64(&mut malformed, first_record, kind);
        repair_prepare_physical_binding(
            &mut malformed,
            image.second_prepare_offset,
            image.second_commit_offset,
        );
        assert_streaming_matches_contiguous_prepare_error(
            malformed,
            &image,
            image.second_prepare_offset,
        );
    }

    for count in [0, declared_count - 1, declared_count + 1] {
        let mut malformed = image.pack.to_vec();
        write_u64(&mut malformed, image.second_prepare_offset + PREPARE_OBJECT_COUNT_OFFSET, count);
        repair_prepare_physical_binding(
            &mut malformed,
            image.second_prepare_offset,
            image.second_commit_offset,
        );
        assert_streaming_matches_contiguous_prepare_error(
            malformed,
            &image,
            image.second_prepare_offset,
        );
    }

    let second_record = image.second_prepare_offset + objects[1].record_offset() as usize;
    let mut duplicate = image.pack.to_vec();
    duplicate[second_record + 8..second_record + 40]
        .copy_from_slice(objects[0].reference().digest().as_bytes());
    repair_prepare_physical_binding(
        &mut duplicate,
        image.second_prepare_offset,
        image.second_commit_offset,
    );
    assert_streaming_matches_contiguous_prepare_error(
        duplicate,
        &image,
        image.second_prepare_offset,
    );

    let mut swapped = image.pack.to_vec();
    let first_header = swapped[first_record..first_record + OBJECT_RECORD_HEADER_LEN].to_vec();
    let second_header = swapped[second_record..second_record + OBJECT_RECORD_HEADER_LEN].to_vec();
    swapped[first_record..first_record + OBJECT_RECORD_HEADER_LEN].copy_from_slice(&second_header);
    swapped[second_record..second_record + OBJECT_RECORD_HEADER_LEN].copy_from_slice(&first_header);
    repair_prepare_physical_binding(
        &mut swapped,
        image.second_prepare_offset,
        image.second_commit_offset,
    );
    assert_streaming_matches_contiguous_prepare_error(swapped, &image, image.second_prepare_offset);

    let data = objects
        .iter()
        .find(|object| object.disposition() == LogPackObjectDispositionV1::Data)
        .unwrap();
    let data_record = image.second_prepare_offset + data.record_offset() as usize;
    let body = image.second_prepare_offset + data.canonical_offset().unwrap() as usize;
    let mut malformed_body = image.pack.to_vec();
    malformed_body[body] ^= 0xff;
    repair_prepare_physical_binding(
        &mut malformed_body,
        image.second_prepare_offset,
        image.second_commit_offset,
    );
    assert_streaming_matches_contiguous_prepare_error(
        malformed_body,
        &image,
        image.second_prepare_offset,
    );

    for delta in [-1i64, 1] {
        let mut malformed = image.pack.to_vec();
        let old = data.reference().encoded_len();
        let changed = if delta < 0 { old - 1 } else { old + 1 };
        write_u64(&mut malformed, data_record + 40, changed);
        let logical =
            read_u64(&malformed, image.second_prepare_offset + PREPARE_OBJECT_BYTES_OFFSET);
        write_u64(
            &mut malformed,
            image.second_prepare_offset + PREPARE_OBJECT_BYTES_OFFSET,
            if delta < 0 { logical - 1 } else { logical + 1 },
        );
        repair_prepare_physical_binding(
            &mut malformed,
            image.second_prepare_offset,
            image.second_commit_offset,
        );
        assert_streaming_matches_contiguous_prepare_error(
            malformed,
            &image,
            image.second_prepare_offset,
        );
    }

    let reuse = objects
        .iter()
        .find(|object| object.disposition() == LogPackObjectDispositionV1::Reuse)
        .unwrap();
    let reuse_record = image.second_prepare_offset + reuse.record_offset() as usize;
    let mut reuse_with_body = image.pack.to_vec();
    write_u64(&mut reuse_with_body, reuse_record, LogPackObjectDispositionV1::Data.kind());
    repair_prepare_physical_binding(
        &mut reuse_with_body,
        image.second_prepare_offset,
        image.second_commit_offset,
    );
    assert_streaming_matches_contiguous_prepare_error(
        reuse_with_body,
        &image,
        image.second_prepare_offset,
    );
}

#[test]
fn declared_object_reference_must_equal_streamed_canonical_digest_and_length() {
    let image = kind1_image("declared-reference-binding");
    let second = prepare_view(&image.pack, image.second_prepare_offset);
    let objects = second.objects().collect::<Vec<_>>();
    let (ordinal, data) = objects
        .iter()
        .enumerate()
        .find(|(_, object)| object.disposition() == LogPackObjectDispositionV1::Data)
        .unwrap();
    let record = image.second_prepare_offset + data.record_offset() as usize;

    let previous = ordinal
        .checked_sub(1)
        .map(|index| objects[index].reference().digest());
    let next = objects
        .get(ordinal + 1)
        .map(|object| object.reference().digest());
    let mut replacement = None;
    for byte_index in (0..32).rev() {
        for bit in 0..8 {
            let mut bytes: [u8; 32] = data.reference().digest().as_bytes().try_into().unwrap();
            bytes[byte_index] ^= 1 << bit;
            let candidate = ObjectDigest::from_array(bytes);
            if previous.is_none_or(|digest| digest < candidate) &&
                next.is_none_or(|digest| candidate < digest)
            {
                replacement = Some(candidate);
                break;
            }
        }
        if replacement.is_some() {
            break;
        }
    }
    let replacement = replacement.expect("a strict-order-preserving digest mutation exists");
    let mut digest_mismatch = image.pack.to_vec();
    digest_mismatch[record + 8..record + 40].copy_from_slice(replacement.as_bytes());
    repair_prepare_physical_binding(
        &mut digest_mismatch,
        image.second_prepare_offset,
        image.second_commit_offset,
    );
    assert!(matches!(
        recovery_error(digest_mismatch.into(), image.catalog),
        RecoveryError::Wal(LogPackFormatError::ObjectReference { .. })
    ));

    let mut length_mismatch = image.pack.to_vec();
    write_u64(&mut length_mismatch, record + 40, data.reference().encoded_len() + 1);
    let declared =
        read_u64(&length_mismatch, image.second_prepare_offset + PREPARE_OBJECT_BYTES_OFFSET);
    write_u64(
        &mut length_mismatch,
        image.second_prepare_offset + PREPARE_OBJECT_BYTES_OFFSET,
        declared + 1,
    );
    repair_prepare_physical_binding(
        &mut length_mismatch,
        image.second_prepare_offset,
        image.second_commit_offset,
    );
    let expected = exact_contiguous_prepare_error(&length_mismatch, image.second_prepare_offset);
    assert!(matches!(
        &expected,
        LogPackFormatError::Canonical(_) | LogPackFormatError::ObjectReference { .. }
    ));
    match recovery_error(length_mismatch.into(), image.catalog) {
        RecoveryError::Wal(actual) => assert_eq!(actual, expected),
        _ => panic!("declared object-length mismatch did not preserve its exact error class"),
    }
}

#[test]
fn object_byte_sum_includes_data_and_reuse_and_objects_end_at_trailer() {
    let image = kind1_image("mixed-object-byte-sum");
    let prepare = prepare_view(&image.pack, image.second_prepare_offset);
    let preflight = preflight_prepare_record_v1(
        &image.pack[image.second_prepare_offset..
            image.second_prepare_offset + TRANSACTION_PREPARE_HEADER_LEN_V1 as usize],
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let objects = prepare.objects().collect::<Vec<_>>();
    assert!(
        objects
            .iter()
            .any(|object| { object.disposition() == LogPackObjectDispositionV1::Data })
    );
    assert!(
        objects
            .iter()
            .any(|object| { object.disposition() == LogPackObjectDispositionV1::Reuse })
    );
    let logical_bytes = objects
        .iter()
        .try_fold(0u64, |sum, object| sum.checked_add(object.reference().encoded_len()))
        .unwrap();
    assert_eq!(logical_bytes, preflight.object_bytes());
    let final_extent = objects
        .iter()
        .try_fold(prepare.objects_offset(), |cursor, object| {
            assert_eq!(object.record_offset(), cursor, "object records must be adjacent");
            let body_start = cursor.checked_add(OBJECT_RECORD_HEADER_LEN as u64).unwrap();
            match object.disposition() {
                LogPackObjectDispositionV1::Data => {
                    assert_eq!(object.canonical_offset(), Some(body_start));
                    assert_eq!(
                        object.canonical_bytes().unwrap().len() as u64,
                        object.reference().encoded_len()
                    );
                    body_start.checked_add(object.reference().encoded_len())
                }
                LogPackObjectDispositionV1::Reuse => {
                    assert_eq!(object.canonical_offset(), None);
                    assert_eq!(object.canonical_bytes(), None);
                    Some(body_start)
                }
            }
        })
        .unwrap();
    let trailer_offset = prepare.record_len() - 8;
    assert_eq!(final_extent, trailer_offset);

    let reused_bytes = objects
        .iter()
        .filter(|object| object.disposition() == LogPackObjectDispositionV1::Reuse)
        .map(|object| object.reference().encoded_len())
        .sum::<u64>();
    assert!(reused_bytes > 0);
    let mut data_only_sum = image.pack.to_vec();
    write_u64(
        &mut data_only_sum,
        image.second_prepare_offset + PREPARE_OBJECT_BYTES_OFFSET,
        logical_bytes - reused_bytes,
    );
    repair_prepare_physical_binding(
        &mut data_only_sum,
        image.second_prepare_offset,
        image.second_commit_offset,
    );
    assert_eq!(
        exact_contiguous_prepare_error(&data_only_sum, image.second_prepare_offset),
        LogPackFormatError::LengthMismatch {
            declared: logical_bytes - reused_bytes,
            actual: logical_bytes,
        }
    );
    assert_streaming_matches_contiguous_prepare_error(
        data_only_sum,
        &image,
        image.second_prepare_offset,
    );
}

#[test]
fn incremental_commit_fixed_image_crc_domain_width_and_all_truncations_are_exact() {
    let image = kind1_image("fixed-commit");
    let bytes = &image.pack[image.second_commit_offset..
        image.second_commit_offset + TRANSACTION_COMMIT_RECORD_LEN_V1 as usize];
    assert_eq!(bytes.len(), 384);
    assert_eq!(&bytes[COMMIT_AUTH_LEN..], &encode_crc32c(crc32c(&bytes[..COMMIT_AUTH_LEN])));
    assert!(TransactionCommitViewV1::parse(bytes).is_ok());
    for length in 0..bytes.len() {
        assert!(
            TransactionCommitViewV1::parse(&bytes[..length]).is_err(),
            "commit truncation at {length} was accepted",
        );
    }
    let mut high_crc = bytes.to_vec();
    let encoded = u64::from(crc32c(&bytes[..COMMIT_AUTH_LEN])) | (1u64 << 32);
    high_crc[COMMIT_AUTH_LEN..].copy_from_slice(&encoded.to_le_bytes());
    assert!(matches!(
        TransactionCommitViewV1::parse(&high_crc),
        Err(LogPackFormatError::ChecksumOutOfRange { .. })
    ));
    for offset in [0, 15, 16, 31, 32, COMMIT_AUTH_LEN - 1] {
        let mut corrupt = bytes.to_vec();
        corrupt[offset] ^= 1;
        assert!(matches!(
            TransactionCommitViewV1::parse(&corrupt),
            Err(LogPackFormatError::RecordChecksum { .. })
        ));
    }
}

#[test]
fn incremental_commit_binding_rejects_each_of_six_identity_fields_before_replay() {
    let image = kind1_image("commit-binding-matrix");
    for field in 0..6 {
        let mut pack = image.pack.to_vec();
        let commit = image.first_commit_offset;
        match field {
            0 => {
                let value = read_u64(&pack, commit + COMMIT_SEQUENCE_OFFSET) + 1;
                write_u64(&mut pack, commit + COMMIT_SEQUENCE_OFFSET, value);
            }
            1 => pack[commit + COMMIT_STORE_OFFSET] ^= 1,
            2 => pack[commit + COMMIT_PREPARE_DIGEST_OFFSET] ^= 1,
            3 => {
                let value = read_u64(&pack, commit + COMMIT_PREPARE_LENGTH_OFFSET) + 1;
                write_u64(&mut pack, commit + COMMIT_PREPARE_LENGTH_OFFSET, value);
            }
            4 => {
                let value = read_u64(&pack, commit + COMMIT_TARGET_OFFSET) + 1;
                write_u64(&mut pack, commit + COMMIT_TARGET_OFFSET, value);
            }
            5 => {
                let value = read_u64(&pack, commit + COMMIT_PREPARE_OFFSET_OFFSET) + 1;
                write_u64(&mut pack, commit + COMMIT_PREPARE_OFFSET_OFFSET, value);
            }
            _ => unreachable!(),
        }
        repair_commit_crc(&mut pack, image.first_commit_offset);
        let error = recovery_error(pack.into(), image.catalog);
        match field {
            0 => assert!(matches!(error, RecoveryError::Wal(LogPackFormatError::Sequence { .. }))),
            1 => assert!(matches!(error, RecoveryError::Wal(LogPackFormatError::Store { .. }))),
            2 => assert!(matches!(
                error,
                RecoveryError::Wal(LogPackFormatError::RecordDigest { .. })
            )),
            3 => assert!(matches!(
                error,
                RecoveryError::Wal(LogPackFormatError::AddressLength { .. })
            )),
            4 => assert!(matches!(error, RecoveryError::Wal(LogPackFormatError::ReceiptMismatch))),
            5 => assert!(matches!(error, RecoveryError::PrepareAddress { .. })),
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum OuterFault {
    Sequence,
    Store,
    Profile,
    StateBase,
    PreviousCommit,
    PrepareAddress,
    FirstLivePack,
    IndexGeneration,
    IndexTail,
}

fn inject_outer_fault(pack: &mut [u8], image: &Kind1Image, fault: OuterFault) {
    let prepare = image.second_prepare_offset;
    let commit = image.second_commit_offset;
    match fault {
        OuterFault::Sequence => {
            let sequence = read_u64(pack, prepare + PREPARE_SEQUENCE_OFFSET) + 1;
            write_u64(pack, prepare + PREPARE_SEQUENCE_OFFSET, sequence);
            write_u64(pack, commit + COMMIT_SEQUENCE_OFFSET, sequence);
            repair_prepare_and_commit_binding(pack, prepare, commit);
        }
        OuterFault::Store => {
            pack[prepare + PREPARE_STORE_OFFSET] ^= 1;
            pack[commit + COMMIT_STORE_OFFSET] ^= 1;
            repair_prepare_and_commit_binding(pack, prepare, commit);
        }
        OuterFault::Profile => {
            pack[prepare + PREPARE_PROFILE_OFFSET] ^= 1;
            pack[commit + COMMIT_PROFILE_OFFSET] ^= 1;
            repair_prepare_and_commit_binding(pack, prepare, commit);
        }
        OuterFault::StateBase => {
            let current = prepare_view(pack, image.first_prepare_offset)
                .receipt()
                .target();
            let changed = StateVersionV1::new(current.generation() - 1, current.manifest());
            patch_second_transition_base(pack, image, changed);
        }
        OuterFault::PreviousCommit => {
            pack[commit + COMMIT_PREVIOUS_OFFSET] ^= 1;
            repair_commit_crc(pack, commit);
        }
        OuterFault::PrepareAddress => {
            let offset = read_u64(pack, commit + COMMIT_PREPARE_OFFSET_OFFSET) + 1;
            write_u64(pack, commit + COMMIT_PREPARE_OFFSET_OFFSET, offset);
            repair_commit_crc(pack, commit);
        }
        OuterFault::FirstLivePack => {
            write_u64(pack, commit + COMMIT_FIRST_LIVE_OFFSET, 2);
            repair_commit_crc(pack, commit);
        }
        OuterFault::IndexGeneration => {
            write_u64(pack, commit + COMMIT_INDEX_GENERATION_OFFSET, 2);
            repair_commit_crc(pack, commit);
        }
        OuterFault::IndexTail => {
            let tail = read_u64(pack, commit + COMMIT_INDEX_TAIL_OFFSET) + INDEX_RECORD_LEN_V1;
            write_u64(pack, commit + COMMIT_INDEX_TAIL_OFFSET, tail);
            repair_commit_crc(pack, commit);
        }
    }
}

#[test]
fn outer_chain_binding_rejects_each_of_nine_fields_before_replay() {
    let image = kind1_image("outer-binding-matrix");
    for fault in [
        OuterFault::Sequence,
        OuterFault::Store,
        OuterFault::Profile,
        OuterFault::StateBase,
        OuterFault::PreviousCommit,
        OuterFault::PrepareAddress,
        OuterFault::FirstLivePack,
        OuterFault::IndexGeneration,
        OuterFault::IndexTail,
    ] {
        let mut pack = image.pack.to_vec();
        inject_outer_fault(&mut pack, &image, fault);
        let error = recovery_error(pack.into(), image.catalog);
        let exact = match fault {
            OuterFault::Sequence => matches!(error, RecoveryError::Sequence { .. }),
            OuterFault::Store => matches!(error, RecoveryError::Store { .. }),
            OuterFault::Profile => matches!(error, RecoveryError::Profile { .. }),
            OuterFault::StateBase => matches!(error, RecoveryError::StateContinuity { .. }),
            OuterFault::PreviousCommit => matches!(error, RecoveryError::PreviousCommit { .. }),
            OuterFault::PrepareAddress => matches!(error, RecoveryError::PrepareAddress { .. }),
            OuterFault::FirstLivePack => {
                matches!(error, RecoveryError::PublicationFirstLive { .. })
            }
            OuterFault::IndexGeneration => {
                matches!(error, RecoveryError::PublicationIndexGeneration { .. })
            }
            OuterFault::IndexTail => matches!(error, RecoveryError::IndexTail { .. }),
        };
        assert!(exact, "outer fault {fault:?} did not preserve its exact error variant");
    }
}

fn expected_data_records(image: &Kind1Image) -> Vec<ObjectIndexRecordV1> {
    [image.first_prepare_offset, image.middle_prepare_offset, image.second_prepare_offset]
        .into_iter()
        .flat_map(|offset| {
            let prepare = prepare_view(&image.pack, offset);
            prepare
                .objects()
                .filter(|object| object.disposition() == LogPackObjectDispositionV1::Data)
                .map(move |object| {
                    ObjectIndexRecordV1::from_data_view(
                        PackId::GENESIS,
                        offset as u64,
                        prepare.sequence(),
                        object,
                    )
                    .unwrap()
                })
        })
        .collect()
}

#[test]
fn authenticated_layout_replay_emits_exact_private_index_and_enforces_reuse_rules() {
    let image = kind1_image("private-index-and-reuse");
    let expected = expected_data_records(&image);
    let authority = token(image.catalog, false);
    let mut provider = MemoryProvider {
        bytes: Arc::clone(&image.pack),
    };
    let recovered = scan_catalog_authorized_pack_chain(
        &mut provider,
        &authority,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap()
    .verify_catalog(authority)
    .unwrap();
    assert_eq!(recovered.object_index().record_count(), expected.len() as u64);
    for record in expected.iter().copied() {
        assert_eq!(recovered.object_index().get(record.reference().digest()), Some(record));
    }

    let second = prepare_view(&image.pack, image.second_prepare_offset);
    let objects = second.objects().collect::<Vec<_>>();
    let (reuse_ordinal, reused) = objects
        .iter()
        .enumerate()
        .find(|(_, object)| object.disposition() == LogPackObjectDispositionV1::Reuse)
        .unwrap();
    let prior = recovered
        .object_index()
        .get(reused.reference().digest())
        .expect("valid REUSE resolves to a DATA location");
    assert_eq!(prior.reference(), reused.reference());
    assert!(prior.location().sequence() < second.sequence());
    let first_data = prepare_view(&image.pack, image.first_prepare_offset)
        .objects()
        .filter(|object| object.disposition() == LogPackObjectDispositionV1::Data)
        .count();
    let middle_data = prepare_view(&image.pack, image.middle_prepare_offset)
        .objects()
        .filter(|object| object.disposition() == LogPackObjectDispositionV1::Data)
        .count();
    let second_data = objects
        .iter()
        .filter(|object| object.disposition() == LogPackObjectDispositionV1::Data)
        .count();
    assert_eq!(
        recovered.object_index().record_count(),
        (first_data + middle_data + second_data) as u64,
        "REUSE must emit no private index location",
    );

    let replacement_digest = strict_order_preserving_replacement(&objects, reuse_ordinal);
    let replacement =
        ObjectRefV1::new(replacement_digest, reused.reference().encoded_len()).unwrap();
    let reuse_record = image.second_prepare_offset + reused.record_offset() as usize;

    let mut missing = image.pack.to_vec();
    write_reference(&mut missing, reuse_record + 8, replacement);
    repair_prepare_physical_binding(
        &mut missing,
        image.second_prepare_offset,
        image.second_commit_offset,
    );
    assert!(matches!(
        recovery_error(missing.into(), image.catalog),
        RecoveryError::MissingReuse(reference) if reference == replacement
    ));

    let mut mismatched = image.pack.to_vec();
    let length_offset = image.second_prepare_offset + reused.record_offset() as usize + 40;
    write_u64(&mut mismatched, length_offset, reused.reference().encoded_len() + 1);
    let declared = read_u64(&mismatched, image.second_prepare_offset + PREPARE_OBJECT_BYTES_OFFSET);
    write_u64(
        &mut mismatched,
        image.second_prepare_offset + PREPARE_OBJECT_BYTES_OFFSET,
        declared + 1,
    );
    repair_prepare_and_commit_binding(
        &mut mismatched,
        image.second_prepare_offset,
        image.second_commit_offset,
    );
    assert!(matches!(
        recovery_error(mismatched.into(), image.catalog),
        RecoveryError::ReuseReference { indexed, reused: actual }
            if indexed == reused.reference() &&
                actual.digest() == reused.reference().digest() &&
                actual.encoded_len() == reused.reference().encoded_len() + 1
    ));

    let mut same_generation_pack = image.pack.to_vec();
    write_reference(&mut same_generation_pack, reuse_record + 8, replacement);
    repair_prepare_physical_binding(
        &mut same_generation_pack,
        image.second_prepare_offset,
        image.second_commit_offset,
    );
    let mut same_generation_authority = CommittedObjectIndexV1::new();
    for record in expected.iter().copied() {
        same_generation_authority.insert(record).unwrap();
    }
    let same_generation_record = ObjectIndexRecordV1::new(
        replacement,
        ObjectLocationV1::new(PackId::GENESIS, PACK_HEADER_LEN_V1, second.sequence()).unwrap(),
    )
    .unwrap();
    same_generation_authority
        .insert(same_generation_record)
        .unwrap();
    let mut provider = MemoryProvider {
        bytes: same_generation_pack.into(),
    };
    let error = match replay_catalog_authorized_pack_chain(
        &mut provider,
        &token(image.catalog, false),
        &same_generation_authority,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    ) {
        Err(error) => error,
        Ok(_) => panic!("same-generation REUSE was accepted"),
    };
    assert!(matches!(
        error,
        RecoveryError::ReuseNotPrior {
            reference,
            indexed_sequence,
            current_sequence,
        } if reference == replacement &&
            indexed_sequence == second.sequence() &&
            current_sequence == second.sequence()
    ));

    let (reuse_pack, reuse_catalog) = generation_start_reuse_image("generation-start-reuse");
    assert!(matches!(
        recovery_error(reuse_pack, reuse_catalog),
        RecoveryError::GenerationCheckpointReuse(_)
    ));
}

#[derive(Clone, Copy)]
enum ReplayMutation {
    Substitution,
    Omission,
    Duplication,
    Reordering,
    OffsetShift,
}

#[derive(Clone)]
struct UnstableLayoutReader {
    bytes: Arc<[u8]>,
    target: usize,
    target_len: usize,
    replacement: Arc<[u8]>,
    primary_seen: Arc<Cell<bool>>,
}

impl DurableReadAt for UnstableLayoutReader {
    type Error = Infallible;

    fn durable_len(&self) -> Result<u64, Self::Error> { Ok(self.bytes.len() as u64) }

    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> Result<(), Self::Error> {
        let start = offset as usize;
        let end = start + destination.len();
        destination.copy_from_slice(&self.bytes[start..end]);
        let target_end = self.target + self.target_len;
        let overlaps = start < target_end && end > self.target;
        if overlaps && self.primary_seen.get() {
            let overlap_start = start.max(self.target);
            let overlap_end = end.min(target_end);
            destination[overlap_start - start..overlap_end - start].copy_from_slice(
                &self.replacement[overlap_start - self.target..overlap_end - self.target],
            );
        } else if overlaps {
            self.primary_seen.set(true);
        }
        Ok(())
    }
}

struct UnstableLayoutProvider {
    reader: UnstableLayoutReader,
}

impl CatalogPackProviderV1 for UnstableLayoutProvider {
    type Reader = UnstableLayoutReader;
    type Error = Infallible;

    fn open_pack(&mut self, pack_id: PackId) -> Result<Self::Reader, Self::Error> {
        assert_eq!(pack_id, PackId::GENESIS);
        Ok(self.reader.clone())
    }
}

fn replay_replacement(
    pack: &[u8],
    records: &[usize],
    target_index: usize,
    mutation: ReplayMutation,
) -> Vec<u8> {
    let target = records[target_index];
    let other = if target_index == 0 { 1 } else { 0 };
    match mutation {
        ReplayMutation::Substitution => {
            let mut replacement = pack[target..target + OBJECT_RECORD_HEADER_LEN].to_vec();
            replacement[8] ^= 1;
            replacement
        }
        ReplayMutation::Omission => vec![0; OBJECT_RECORD_HEADER_LEN],
        ReplayMutation::Duplication | ReplayMutation::Reordering => {
            pack[records[other]..records[other] + OBJECT_RECORD_HEADER_LEN].to_vec()
        }
        ReplayMutation::OffsetShift => {
            let mut replacement = vec![0; OBJECT_RECORD_HEADER_LEN];
            replacement[..OBJECT_RECORD_HEADER_LEN - 1]
                .copy_from_slice(&pack[target + 1..target + OBJECT_RECORD_HEADER_LEN]);
            replacement
        }
    }
}

#[test]
fn layout_transcript_and_replay_accept_iff_structurally_identical() {
    let image = kind1_image("typed-layout-replay");
    let authority = token(image.catalog, false);
    let mut stable = MemoryProvider {
        bytes: Arc::clone(&image.pack),
    };
    assert!(
        scan_catalog_authorized_pack_chain(
            &mut stable,
            &authority,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap()
        .verify_catalog(authority.clone())
        .is_ok()
    );

    let second = prepare_view(&image.pack, image.second_prepare_offset);
    let records = second
        .objects()
        .map(|object| image.second_prepare_offset + object.record_offset() as usize)
        .collect::<Vec<_>>();
    assert!(records.len() >= 2);
    let target_index = 0;
    for mutation in [
        ReplayMutation::Substitution,
        ReplayMutation::Omission,
        ReplayMutation::Duplication,
        ReplayMutation::Reordering,
        ReplayMutation::OffsetShift,
    ] {
        let replacement = replay_replacement(&image.pack, &records, target_index, mutation);
        let mut provider = UnstableLayoutProvider {
            reader: UnstableLayoutReader {
                bytes: Arc::clone(&image.pack),
                target: records[target_index],
                target_len: OBJECT_RECORD_HEADER_LEN,
                replacement: replacement.into(),
                primary_seen: Arc::new(Cell::new(false)),
            },
        };
        let error = match scan_catalog_authorized_pack_chain(
            &mut provider,
            &authority,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
        ) {
            Err(error) => error,
            Ok(_) => panic!("typed replay mutation was accepted"),
        };
        assert!(matches!(
            error,
            RecoveryError::IncrementalReplayTranscript { .. } |
                RecoveryError::IncrementalReplaySourceChanged { .. }
        ));
    }

    // A header-only replay cannot detect this: the declared layout remains
    // byte-identical while one already-authenticated DATA body byte changes.
    let data = second
        .objects()
        .find(|object| object.disposition() == LogPackObjectDispositionV1::Data)
        .unwrap();
    let body_offset = image.second_prepare_offset + data.canonical_offset().unwrap() as usize;
    let mut changed_byte = image.pack[body_offset];
    changed_byte ^= 1;
    let mut provider = UnstableLayoutProvider {
        reader: UnstableLayoutReader {
            bytes: Arc::clone(&image.pack),
            target: body_offset,
            target_len: 1,
            replacement: Arc::from([changed_byte]),
            primary_seen: Arc::new(Cell::new(false)),
        },
    };
    let error = match scan_catalog_authorized_pack_chain(
        &mut provider,
        &authority,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    ) {
        Err(error) => error,
        Ok(_) => panic!("DATA-body-only source instability was accepted"),
    };
    assert!(matches!(error, RecoveryError::IncrementalReplaySourceChanged { .. }));
}

#[test]
fn no_validation_failure_exposes_recovery_authority_or_mutates_durable_index() {
    let image = kind1_image("authority-side-effects");
    let authority = token(image.catalog, false);
    let mut provider = MemoryProvider {
        bytes: Arc::clone(&image.pack),
    };
    let recovered = scan_catalog_authorized_pack_chain(
        &mut provider,
        &authority,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap()
    .verify_catalog(authority.clone())
    .unwrap();
    let record_count = recovered.object_index().record_count();

    let mut mutation_fault = image.pack.to_vec();
    mutation_fault[image.first_prepare_offset + PREPARE_MUTATION_DIGEST_OFFSET] ^= 1;
    mutation_fault[image.first_commit_offset + COMMIT_MUTATION_DIGEST_OFFSET] ^= 1;
    repair_prepare_physical_binding(
        &mut mutation_fault,
        image.first_prepare_offset,
        image.first_commit_offset,
    );
    assert!(matches!(
        recovery_error(mutation_fault.into(), image.catalog),
        RecoveryError::Wal(LogPackFormatError::MutationDigest { .. })
    ));

    let first = prepare_view(&image.pack, image.first_prepare_offset);
    let first_data = first
        .objects()
        .find(|object| object.disposition() == LogPackObjectDispositionV1::Data)
        .unwrap();
    let mut object_fault = image.pack.to_vec();
    let body = image.first_prepare_offset + first_data.canonical_offset().unwrap() as usize;
    object_fault[body] ^= 1;
    repair_prepare_physical_binding(
        &mut object_fault,
        image.first_prepare_offset,
        image.first_commit_offset,
    );
    let _ = recovery_error(object_fault.into(), image.catalog);

    for field in 0..6 {
        let mut commit_fault = image.pack.to_vec();
        match field {
            0 => write_u64(
                &mut commit_fault,
                image.first_commit_offset + COMMIT_SEQUENCE_OFFSET,
                LogPackSequence::FIRST.get() + 1,
            ),
            1 => commit_fault[image.first_commit_offset + COMMIT_STORE_OFFSET] ^= 1,
            2 => commit_fault[image.first_commit_offset + COMMIT_PREPARE_DIGEST_OFFSET] ^= 1,
            3 => {
                let length = read_u64(
                    &commit_fault,
                    image.first_commit_offset + COMMIT_PREPARE_LENGTH_OFFSET,
                );
                write_u64(
                    &mut commit_fault,
                    image.first_commit_offset + COMMIT_PREPARE_LENGTH_OFFSET,
                    length + 1,
                );
            }
            4 => {
                let target =
                    read_u64(&commit_fault, image.first_commit_offset + COMMIT_TARGET_OFFSET);
                write_u64(
                    &mut commit_fault,
                    image.first_commit_offset + COMMIT_TARGET_OFFSET,
                    target + 1,
                );
            }
            5 => {
                let offset = read_u64(
                    &commit_fault,
                    image.first_commit_offset + COMMIT_PREPARE_OFFSET_OFFSET,
                );
                write_u64(
                    &mut commit_fault,
                    image.first_commit_offset + COMMIT_PREPARE_OFFSET_OFFSET,
                    offset + 1,
                );
            }
            _ => unreachable!(),
        }
        repair_commit_crc(&mut commit_fault, image.first_commit_offset);
        let _ = recovery_error(commit_fault.into(), image.catalog);
    }

    for fault in [
        OuterFault::Sequence,
        OuterFault::Store,
        OuterFault::Profile,
        OuterFault::StateBase,
        OuterFault::PreviousCommit,
        OuterFault::PrepareAddress,
        OuterFault::FirstLivePack,
        OuterFault::IndexGeneration,
        OuterFault::IndexTail,
    ] {
        let mut chain_fault = image.pack.to_vec();
        inject_outer_fault(&mut chain_fault, &image, fault);
        let _ = recovery_error(chain_fault.into(), image.catalog);
    }

    let mut commit_fault = image.pack.to_vec();
    commit_fault[image.first_commit_offset + COMMIT_PREPARE_DIGEST_OFFSET] ^= 1;
    repair_commit_crc(&mut commit_fault, image.first_commit_offset);
    let mut provider = MemoryProvider {
        bytes: commit_fault.into(),
    };
    let replay_result = replay_catalog_authorized_pack_chain(
        &mut provider,
        &token(image.catalog, false),
        recovered.object_index(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    );
    assert!(replay_result.is_err());
    assert_eq!(recovered.object_index().record_count(), record_count);

    // Catalog failure is the final gate. The initial scan API has no observer
    // or durable publication capability, so a rejected candidate stays private.
    let mut provider = MemoryProvider {
        bytes: Arc::clone(&image.pack),
    };
    let scanned = scan_catalog_authorized_pack_chain(
        &mut provider,
        &authority,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    assert!(matches!(
        scanned.verify_catalog(token(image.catalog, true)),
        Err(RecoveryError::CatalogAuthorityChanged)
    ));
}

struct StructuredOracle {
    transactions: u64,
    state: StateVersionV1,
    terminal_address: LogPackAddress,
    terminal: TransactionCommitViewV1,
    records: Vec<ObjectIndexRecordV1>,
    object_records: u64,
    index_tail: u64,
}

fn contiguous_oracle(image: &Kind1Image) -> StructuredOracle {
    let mut offset = PACK_HEADER_LEN_V1 as usize;
    let mut transactions = 0u64;
    let mut object_records = 0u64;
    let mut records = Vec::new();
    let mut terminal = None;
    let mut terminal_address = None;
    while offset < image.pack.len() {
        let prepare = prepare_view(&image.pack, offset);
        object_records += prepare.objects().len() as u64;
        for object in prepare.objects() {
            if object.disposition() == LogPackObjectDispositionV1::Data {
                records.push(
                    ObjectIndexRecordV1::from_data_view(
                        PackId::GENESIS,
                        offset as u64,
                        prepare.sequence(),
                        object,
                    )
                    .unwrap(),
                );
            }
        }
        let commit_offset = offset + prepare.record_len() as usize;
        let commit = TransactionCommitViewV1::parse(
            &image.pack[commit_offset..commit_offset + TRANSACTION_COMMIT_RECORD_LEN_V1 as usize],
        )
        .unwrap();
        commit.verify_prepare(prepare).unwrap();
        terminal_address = Some(
            LogPackAddress::from_pack(
                PackId::GENESIS,
                commit_offset as u64,
                TRANSACTION_COMMIT_RECORD_LEN_V1,
            )
            .unwrap(),
        );
        terminal = Some(commit);
        transactions += 1;
        offset = commit_offset + TRANSACTION_COMMIT_RECORD_LEN_V1 as usize;
    }
    let terminal = terminal.unwrap();
    StructuredOracle {
        transactions,
        state: terminal.receipt().target(),
        terminal_address: terminal_address.unwrap(),
        terminal,
        object_records,
        index_tail: INDEX_HEADER_LEN_V1 + records.len() as u64 * INDEX_RECORD_LEN_V1,
        records,
    }
}

#[test]
fn kind1_streaming_recovery_is_a_complete_structured_differential_of_contiguous_oracle() {
    for identity in ["x".to_owned(), "q".repeat(4097), "z".repeat(65_537)] {
        let image = kind1_image(&identity);
        let oracle = contiguous_oracle(&image);
        let authority = token(image.catalog, false);
        let mut provider = MemoryProvider {
            bytes: Arc::clone(&image.pack),
        };
        let recovered = scan_catalog_authorized_pack_chain(
            &mut provider,
            &authority,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap()
        .verify_catalog(authority)
        .unwrap();
        for record in oracle.records.iter().copied() {
            assert_eq!(recovered.object_index().get(record.reference().digest()), Some(record));
        }
        assert_eq!(recovered.transactions(), oracle.transactions);
        assert_eq!(recovered.checkpoints(), 0);
        assert_eq!(recovered.state(), oracle.state);
        assert_eq!(recovered.terminal_address(), oracle.terminal_address);
        assert_eq!(recovered.terminal_commit().sequence(), oracle.terminal.sequence());
        assert_eq!(recovered.terminal_commit().store_id(), oracle.terminal.store_id());
        assert_eq!(recovered.terminal_commit().digest(), oracle.terminal.digest());
        assert_eq!(recovered.terminal_commit().state(), oracle.terminal.receipt().target());
        assert_eq!(
            recovered.terminal_commit().profile_id(),
            oracle.terminal.receipt().profile_id()
        );
        assert_eq!(
            recovered.terminal_commit().prepare_address(),
            oracle.terminal.prepare_address()
        );
        assert_eq!(recovered.expected_index_tail(), oracle.index_tail);
        assert_eq!(recovered.object_records(), oracle.object_records);
        assert_eq!(recovered.object_index().record_count(), oracle.records.len() as u64);
        assert_eq!(recovered.scanned_bytes(), image.pack.len() as u64);
        assert_eq!(recovered.read_work().authorized_pack_bytes(), image.pack.len() as u64);
        assert!(recovered.read_work().physical_read_bytes() >= image.pack.len() as u64);
    }

    let image = kind1_image("structured-malformed-differential");
    let first = prepare_view(&image.pack, image.first_prepare_offset);
    let first_object = first.objects().next().unwrap();
    let first_record = image.first_prepare_offset + first_object.record_offset() as usize;

    let mut semantic_with_bad_crc = image.pack.to_vec();
    write_u64(&mut semantic_with_bad_crc, first_record, 0);
    assert!(matches!(
        recovery_error(semantic_with_bad_crc.into(), image.catalog),
        RecoveryError::Wal(LogPackFormatError::RecordChecksum { .. })
    ));

    let mut semantic_with_valid_crc = image.pack.to_vec();
    write_u64(&mut semantic_with_valid_crc, first_record, 0);
    repair_prepare_physical_binding(
        &mut semantic_with_valid_crc,
        image.first_prepare_offset,
        image.first_commit_offset,
    );
    assert_streaming_matches_contiguous_prepare_error(
        semantic_with_valid_crc,
        &image,
        image.first_prepare_offset,
    );

    let mut malformed_prepare_corpus = Vec::new();
    let mut wrong_count = image.pack.to_vec();
    let count = read_u64(&wrong_count, image.first_prepare_offset + PREPARE_OBJECT_COUNT_OFFSET);
    write_u64(
        &mut wrong_count,
        image.first_prepare_offset + PREPARE_OBJECT_COUNT_OFFSET,
        count + 1,
    );
    malformed_prepare_corpus.push(wrong_count);

    let first_objects = first.objects().collect::<Vec<_>>();
    if first_objects.len() >= 2 {
        let mut duplicate = image.pack.to_vec();
        let second_record = image.first_prepare_offset + first_objects[1].record_offset() as usize;
        duplicate[second_record + 8..second_record + 40]
            .copy_from_slice(first_objects[0].reference().digest().as_bytes());
        malformed_prepare_corpus.push(duplicate);
    }

    let mut body_fault = image.pack.to_vec();
    let data = first_objects
        .iter()
        .find(|object| object.disposition() == LogPackObjectDispositionV1::Data)
        .unwrap();
    body_fault[image.first_prepare_offset + data.canonical_offset().unwrap() as usize] ^= 1;
    malformed_prepare_corpus.push(body_fault);

    let mut logical_sum = image.pack.to_vec();
    let object_bytes =
        read_u64(&logical_sum, image.first_prepare_offset + PREPARE_OBJECT_BYTES_OFFSET);
    write_u64(
        &mut logical_sum,
        image.first_prepare_offset + PREPARE_OBJECT_BYTES_OFFSET,
        object_bytes + 1,
    );
    malformed_prepare_corpus.push(logical_sum);

    for mut malformed in malformed_prepare_corpus {
        repair_prepare_physical_binding(
            &mut malformed,
            image.first_prepare_offset,
            image.first_commit_offset,
        );
        assert_streaming_matches_contiguous_prepare_error(
            malformed,
            &image,
            image.first_prepare_offset,
        );
    }

    let mut mutation_receipt = image.pack.to_vec();
    mutation_receipt[image.first_prepare_offset + PREPARE_MUTATION_DIGEST_OFFSET] ^= 1;
    mutation_receipt[image.first_commit_offset + COMMIT_MUTATION_DIGEST_OFFSET] ^= 1;
    repair_prepare_physical_binding(
        &mut mutation_receipt,
        image.first_prepare_offset,
        image.first_commit_offset,
    );
    assert!(matches!(
        recovery_error(mutation_receipt.into(), image.catalog),
        RecoveryError::Wal(LogPackFormatError::MutationDigest { .. })
    ));

    let mut commit_before_catalog = image.pack.to_vec();
    commit_before_catalog[image.first_commit_offset + COMMIT_PREPARE_DIGEST_OFFSET] ^= 1;
    repair_commit_crc(&mut commit_before_catalog, image.first_commit_offset);
    assert!(matches!(
        recovery_error(commit_before_catalog.into(), image.catalog),
        RecoveryError::Wal(LogPackFormatError::RecordDigest { .. })
    ));

    let mut chain_before_catalog = image.pack.to_vec();
    inject_outer_fault(&mut chain_before_catalog, &image, OuterFault::PreviousCommit);
    assert!(matches!(
        recovery_error(chain_before_catalog.into(), image.catalog),
        RecoveryError::PreviousCommit { .. }
    ));
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AllocationMeasurement {
    pub(crate) count: u64,
    pub(super) total_bytes: u64,
    pub(super) max_single_bytes: u64,
    pub(super) peak_live_bytes: u64,
    pub(super) live_at_end: u64,
}

fn reset_allocation_measurement() {
    ALLOCATION_COUNT.with(|count| count.set(0));
    ALLOCATION_BYTES.with(|bytes| bytes.set(0));
    ALLOCATION_MAX_SINGLE_BYTES.with(|bytes| bytes.set(0));
    ALLOCATION_LIVE_BYTES.with(|bytes| bytes.set(0));
    ALLOCATION_PEAK_LIVE_BYTES.with(|bytes| bytes.set(0));
}

pub(crate) fn measure_allocations<T>(operation: impl FnOnce() -> T) -> (T, AllocationMeasurement) {
    reset_allocation_measurement();
    COUNT_ALLOCATIONS.with(|enabled| enabled.set(true));
    let result = operation();
    COUNT_ALLOCATIONS.with(|enabled| enabled.set(false));
    let measurement = AllocationMeasurement {
        count: ALLOCATION_COUNT.with(Cell::get),
        total_bytes: ALLOCATION_BYTES.with(Cell::get),
        max_single_bytes: ALLOCATION_MAX_SINGLE_BYTES.with(Cell::get),
        peak_live_bytes: ALLOCATION_PEAK_LIVE_BYTES.with(Cell::get),
        live_at_end: ALLOCATION_LIVE_BYTES.with(Cell::get),
    };
    (result, measurement)
}

fn measured_recovery_allocations(image: &Kind1Image) -> AllocationMeasurement {
    let authority = token(image.catalog, false);
    let mut build = MemoryProvider {
        bytes: Arc::clone(&image.pack),
    };
    let authoritative = scan_catalog_authorized_pack_chain(
        &mut build,
        &authority,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap()
    .verify_catalog(authority.clone())
    .unwrap();
    let mut warm = MemoryProvider {
        bytes: Arc::clone(&image.pack),
    };
    replay_catalog_authorized_pack_chain(
        &mut warm,
        &authority,
        authoritative.object_index(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    reset_allocation_measurement();
    COUNT_ALLOCATIONS.with(|enabled| enabled.set(true));
    let mut measured = MemoryProvider {
        bytes: Arc::clone(&image.pack),
    };
    let result = replay_catalog_authorized_pack_chain(
        &mut measured,
        &authority,
        authoritative.object_index(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    );
    COUNT_ALLOCATIONS.with(|enabled| enabled.set(false));
    result.unwrap();
    AllocationMeasurement {
        count: ALLOCATION_COUNT.with(Cell::get),
        total_bytes: ALLOCATION_BYTES.with(Cell::get),
        max_single_bytes: ALLOCATION_MAX_SINGLE_BYTES.with(Cell::get),
        peak_live_bytes: ALLOCATION_PEAK_LIVE_BYTES.with(Cell::get),
        live_at_end: ALLOCATION_LIVE_BYTES.with(Cell::get),
    }
}

#[test]
fn kind1_decoder_owned_heap_has_zero_input_slope() {
    let extent_measurements = [64usize, 4096, 65_536, 262_144].map(|length| {
        let image = kind1_image(&"h".repeat(length));
        measured_recovery_allocations(&image)
    });
    let cardinality_measurements = [1usize, 4, 16, 64].map(|cardinality| {
        let image = kind1_image_with_cardinality("cardinality", cardinality);
        measured_recovery_allocations(&image)
    });

    for (dimension, measurements) in [
        ("record/key/body extent", extent_measurements),
        ("entry/object cardinality", cardinality_measurements),
    ] {
        assert!(
            measurements
                .iter()
                .all(|measurement| measurement.live_at_end == 0),
            "{dimension} retained decoder-owned allocations: {measurements:?}",
        );
        let minimum_peak = measurements
            .iter()
            .map(|measurement| measurement.peak_live_bytes)
            .min()
            .unwrap();
        let maximum_peak = measurements
            .iter()
            .map(|measurement| measurement.peak_live_bytes)
            .max()
            .unwrap();
        let minimum_count = measurements
            .iter()
            .map(|measurement| measurement.count)
            .min()
            .unwrap();
        let maximum_count = measurements
            .iter()
            .map(|measurement| measurement.count)
            .max()
            .unwrap();
        assert!(
            maximum_peak <= minimum_peak + 8 * 1024,
            "{dimension} has input-sloped peak live heap: {measurements:?}",
        );
        assert!(
            maximum_count <= minimum_count + 8,
            "{dimension} has input-sloped allocation count: {measurements:?}",
        );
        assert!(
            measurements
                .iter()
                .all(|measurement| measurement.total_bytes >= measurement.peak_live_bytes),
        );
    }

    // Non-vacuity: the same phase-resetting allocator must detect the retired
    // whole-record strategy immediately.
    let control = kind1_image(&"control".repeat(32 * 1024));
    let prepare_bytes = prepare_len(&control.pack, control.first_prepare_offset);
    reset_allocation_measurement();
    COUNT_ALLOCATIONS.with(|enabled| enabled.set(true));
    let whole_record = vec![0u8; prepare_bytes];
    COUNT_ALLOCATIONS.with(|enabled| enabled.set(false));
    let peak = ALLOCATION_PEAK_LIVE_BYTES.with(Cell::get);
    assert!(peak >= prepare_bytes as u64);
    assert!(ALLOCATION_MAX_SINGLE_BYTES.with(Cell::get) >= prepare_bytes as u64);
    drop(whole_record);
}

#[test]
fn kind1_native_stack_high_water_has_zero_input_slope() {
    const CHILD_ENV: &str = "RSPACE_KIND1_STACK_CHILD";
    if let Some(specification) = std::env::var_os(CHILD_ENV) {
        let values = specification
            .to_str()
            .unwrap()
            .split(':')
            .map(|value| value.parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 3);
        let (stack_bytes, identity_len, cardinality) = (values[0], values[1], values[2]);
        let image = kind1_image_with_cardinality(&"s".repeat(identity_len), cardinality);
        std::thread::Builder::new()
            .name(format!("kind1-stack-{stack_bytes}-{identity_len}-{cardinality}"))
            .stack_size(stack_bytes)
            .spawn(move || {
                let authority = token(image.catalog, false);
                let mut provider = MemoryProvider { bytes: image.pack };
                let recovered = scan_catalog_authorized_pack_chain(
                    &mut provider,
                    &authority,
                    limits(),
                    LogPackDecodeLimits::UNBOUNDED,
                )
                .unwrap()
                .verify_catalog(authority)
                .unwrap();
                assert_eq!(recovered.transactions(), 3);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }

    // Each case is process-isolated because a native stack overflow may abort
    // the process. Passing the same 128 KiB envelope at every geometric scale
    // is the executable high-water zero-slope criterion; 256 KiB repeats the
    // matrix to distinguish a genuine decoder regression from platform guard
    // and test-harness overhead.
    for stack_bytes in [128 * 1024usize, 256 * 1024] {
        for (identity_len, cardinality) in
            [(64usize, 1usize), (4096, 1), (65_536, 1), (64, 4), (64, 16), (64, 64)]
        {
            let output = Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg(
                    "rspace::persistence::tests::kind1_recovery_protocol_properties::kind1_native_stack_high_water_has_zero_input_slope",
                )
                .arg("--nocapture")
                .env(
                    CHILD_ENV,
                    format!("{stack_bytes}:{identity_len}:{cardinality}"),
                )
                .output()
                .expect("isolated kind-1 stack child starts");
            assert!(
                output.status.success(),
                "kind-1 stack child failed at stack={stack_bytes}, identity={identity_len}, \
                 cardinality={cardinality}:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }
}
