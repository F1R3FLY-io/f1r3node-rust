use std::cell::Cell;
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::support::{TEST_SEMANTIC_PROFILE_ID, TestSemanticProfile, synthetic_ref};
use super::transaction_sealing::{base_state, data_batch, seal_data};
use crate::rspace::persistence::durable::{
    CatalogPackProviderV1, DurableReadAt, RecoveryError, scan_catalog_authorized_pack_chain,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CanonicalObjectStore, CatalogAuthorityTokenV1,
    CatalogPackStateV1, CatalogSlotV1, CommittedObjectComparison, CommittedObjectLookup,
    DurableFormatError, DurableLimitConfig, DurableLimitKind, DurableLimits, INDEX_HEADER_LEN_V1,
    INDEX_RECORD_LEN_V1, IndexGenerationV1, LogPackAddress, LogPackDecodeLimits,
    LogPackObjectPlanV1, LogPackSequence, MutableObjectStore, ObjectAdmission, ObjectDigest,
    PACK_HEADER_LEN_V1, PackHeaderV1, PackId, PackTerminalIdentityV1, PhysicalDigest,
    PreparedLogPackTransactionV1, StoreId, TRANSACTION_COMMIT_RECORD_LEN_V1,
    TRANSACTION_PREPARE_HEADER_LEN_V1, TransactionCommitRecordV1,
};

#[derive(Clone)]
struct MemoryReader {
    bytes: Arc<[u8]>,
    reads: Arc<Mutex<Vec<(u64, usize)>>>,
}

impl DurableReadAt for MemoryReader {
    type Error = Infallible;

    fn durable_len(&self) -> Result<u64, Self::Error> { Ok(self.bytes.len() as u64) }

    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> Result<(), Self::Error> {
        self.reads.lock().unwrap().push((offset, destination.len()));
        let offset = offset as usize;
        destination.copy_from_slice(&self.bytes[offset..offset + destination.len()]);
        Ok(())
    }
}

struct MemoryProvider {
    pack: PackId,
    reader: MemoryReader,
    opens: u64,
}

impl MemoryProvider {
    fn new(pack: PackId, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            pack,
            reader: MemoryReader {
                bytes: bytes.into(),
                reads: Arc::new(Mutex::new(Vec::new())),
            },
            opens: 0,
        }
    }
}

impl CatalogPackProviderV1 for MemoryProvider {
    type Reader = MemoryReader;
    type Error = Infallible;

    fn open_pack(&mut self, pack_id: PackId) -> Result<Self::Reader, Self::Error> {
        assert_eq!(pack_id, self.pack);
        self.opens += 1;
        Ok(self.reader.clone())
    }
}

struct TrackedReader {
    bytes: Arc<[u8]>,
    live: Rc<Cell<u64>>,
}

impl DurableReadAt for TrackedReader {
    type Error = Infallible;

    fn durable_len(&self) -> Result<u64, Self::Error> { Ok(self.bytes.len() as u64) }

    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> Result<(), Self::Error> {
        let offset = offset as usize;
        destination.copy_from_slice(&self.bytes[offset..offset + destination.len()]);
        Ok(())
    }
}

impl Drop for TrackedReader {
    fn drop(&mut self) { self.live.set(self.live.get() - 1); }
}

struct MultiPackProvider {
    packs: BTreeMap<PackId, Arc<[u8]>>,
    opens: u64,
    live: Rc<Cell<u64>>,
    maximum_live: Rc<Cell<u64>>,
}

impl MultiPackProvider {
    fn new(packs: impl IntoIterator<Item = (PackId, Arc<[u8]>)>) -> Self {
        Self {
            packs: packs.into_iter().collect(),
            opens: 0,
            live: Rc::new(Cell::new(0)),
            maximum_live: Rc::new(Cell::new(0)),
        }
    }
}

impl CatalogPackProviderV1 for MultiPackProvider {
    type Reader = TrackedReader;
    type Error = Infallible;

    fn open_pack(&mut self, pack_id: PackId) -> Result<Self::Reader, Self::Error> {
        self.opens += 1;
        let live = self.live.get() + 1;
        self.live.set(live);
        self.maximum_live.set(self.maximum_live.get().max(live));
        Ok(TrackedReader {
            bytes: Arc::clone(
                self.packs
                    .get(&pack_id)
                    .expect("catalog pack fixture exists"),
            ),
            live: Rc::clone(&self.live),
        })
    }
}

struct MissingLookup;

impl CommittedObjectLookup for MissingLookup {
    type Error = Infallible;

    fn compare_committed_object(
        &self,
        _reference: crate::rspace::persistence::ObjectRefV1,
        _sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error> {
        Ok(CommittedObjectComparison::Missing)
    }
}

struct EqualFirst;

impl CommittedObjectLookup for EqualFirst {
    type Error = Infallible;

    fn compare_committed_object(
        &self,
        _reference: crate::rspace::persistence::ObjectRefV1,
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
        reference: crate::rspace::persistence::ObjectRefV1,
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

#[derive(Clone, Copy)]
enum SecondPackFault {
    None,
    HeaderSequence,
    StateBase,
    PreviousCommit,
}

pub(super) struct TwoPackImage {
    pub(super) first: Arc<[u8]>,
    pub(super) second: Arc<[u8]>,
    pub(super) catalog: CatalogSlotV1,
    pub(super) transactions: u64,
}

fn two_pack_image(fault: SecondPackFault) -> TwoPackImage {
    let object_store = CanonicalObjectStore::new();
    let base = base_state(&object_store, 30, None);
    let first_mutation = data_batch(base, "durable-recovery-pack-one");
    let first_sealed = seal_data(&object_store, &first_mutation);
    let store_id = StoreId::from_array([0x71; 16]);
    let first_prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &first_sealed,
        LogPackObjectPlanV1::classify(&first_sealed, &MissingLookup).unwrap(),
        store_id,
        LogPackSequence::FIRST,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let first_index_tail =
        INDEX_HEADER_LEN_V1 + first_prepared.inline_count() * INDEX_RECORD_LEN_V1;
    let mut first_bytes = PackHeaderV1::genesis(store_id).encode().to_vec();
    let first_prepare_offset = first_bytes.len() as u64;
    let first_prepare = first_prepared.write_to(&mut first_bytes).unwrap();
    let first_prepare_address = LogPackAddress::new(
        PackId::GENESIS.get(),
        first_prepare_offset,
        first_prepare.record_len(),
    )
    .unwrap();
    let first_commit_address = LogPackAddress::new(
        PackId::GENESIS.get(),
        first_bytes.len() as u64,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )
    .unwrap();
    let first_pack_state = CatalogPackStateV1::new(
        PackId::GENESIS.get(),
        PackId::GENESIS.get(),
        first_commit_address.end(),
        IndexGenerationV1::FIRST.get(),
        first_index_tail,
        synthetic_ref(0xe1),
        8192,
        8192,
    )
    .unwrap();
    let first_commit = TransactionCommitRecordV1::new(
        first_sealed.receipt(),
        store_id,
        PhysicalDigest::ZERO,
        first_prepare_address,
        first_prepare,
        first_pack_state.commit_publication(),
    )
    .unwrap();
    first_bytes.extend_from_slice(first_commit.canonical_bytes());

    let mut known = BTreeMap::new();
    for object in first_sealed.objects() {
        let bytes: Arc<[u8]> = object.canonical_bytes().into();
        assert!(matches!(
            object_store
                .put_if_absent_or_equal(object.reference(), Arc::clone(&bytes))
                .unwrap(),
            ObjectAdmission::Inserted | ObjectAdmission::AlreadyPresent
        ));
        known.insert(object.reference().digest(), bytes);
    }

    let second_base = match fault {
        SecondPackFault::StateBase => base,
        _ => first_sealed.receipt().target(),
    };
    let second_mutation = data_batch(second_base, "durable-recovery-pack-two");
    let second_sealed = seal_data(&object_store, &second_mutation);
    let actual_terminal =
        PackTerminalIdentityV1::new(PackId::GENESIS, LogPackSequence::FIRST, first_commit.digest());
    let header_terminal = match fault {
        SecondPackFault::HeaderSequence => PackTerminalIdentityV1::new(
            PackId::GENESIS,
            LogPackSequence::new(2).unwrap(),
            first_commit.digest(),
        ),
        _ => actual_terminal,
    };
    let second_header = PackHeaderV1::successor(store_id, header_terminal).unwrap();
    let second_sequence = second_header.first_sequence();
    let second_prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &second_sealed,
        LogPackObjectPlanV1::classify(&second_sealed, &KnownObjects(known)).unwrap(),
        store_id,
        second_sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let second_index_tail = first_index_tail + second_prepared.inline_count() * INDEX_RECORD_LEN_V1;
    let second_pack = PackId::GENESIS.successor().unwrap();
    let mut second_bytes = second_header.encode().to_vec();
    let second_prepare_offset = second_bytes.len() as u64;
    let second_prepare = second_prepared.write_to(&mut second_bytes).unwrap();
    let second_prepare_address =
        LogPackAddress::from_pack(second_pack, second_prepare_offset, second_prepare.record_len())
            .unwrap();
    let second_commit_address = LogPackAddress::from_pack(
        second_pack,
        second_bytes.len() as u64,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )
    .unwrap();
    let second_pack_state = CatalogPackStateV1::new(
        PackId::GENESIS.get(),
        second_pack.get(),
        second_commit_address.end(),
        IndexGenerationV1::FIRST.get(),
        second_index_tail,
        synthetic_ref(0xe2),
        12_288,
        12_288,
    )
    .unwrap();
    let previous_commit = match fault {
        SecondPackFault::PreviousCommit => PhysicalDigest::of_bytes(b"wrong previous commit"),
        _ => first_commit.digest(),
    };
    let second_commit = TransactionCommitRecordV1::new(
        second_sealed.receipt(),
        store_id,
        previous_commit,
        second_prepare_address,
        second_prepare,
        second_pack_state.commit_publication(),
    )
    .unwrap();
    second_bytes.extend_from_slice(second_commit.canonical_bytes());
    let catalog = CatalogSlotV1::new(
        store_id,
        2,
        second_sealed.receipt().target(),
        TEST_SEMANTIC_PROFILE_ID,
        second_commit.digest(),
        second_sequence,
        second_commit_address,
        second_pack_state,
    )
    .unwrap();
    TwoPackImage {
        first: first_bytes.into(),
        second: second_bytes.into(),
        catalog,
        transactions: 2,
    }
}

pub(super) fn valid_two_pack_image() -> TwoPackImage { two_pack_image(SecondPackFault::None) }

fn limit_config() -> DurableLimitConfig {
    DurableLimitConfig {
        max_live_packs: 8,
        max_pack_bytes: 16 * 1024 * 1024,
        max_transactions: 1024,
        max_object_records: 65_536,
        max_total_scan_bytes: 64 * 1024 * 1024,
        max_index_bytes: 16 * 1024 * 1024,
        max_index_records: 65_536,
        max_full_byte_comparison_bytes: 16 * 1024 * 1024,
        max_aggregate_full_byte_comparison_bytes: 64 * 1024 * 1024,
    }
}

pub(super) fn limits() -> DurableLimits { DurableLimits::new(limit_config()).unwrap() }

fn token_with_slot(slot: CatalogSlotV1, second: bool) -> CatalogAuthorityTokenV1 {
    let mut image = [0u8; CATALOG_IMAGE_LEN];
    let start = usize::from(second) * CATALOG_SLOT_LEN;
    image[start..start + CATALOG_SLOT_LEN].copy_from_slice(&slot.encode());
    CatalogAuthorityTokenV1::select(image).unwrap().unwrap()
}

pub(super) fn catalog_image(slot: CatalogSlotV1) -> [u8; CATALOG_IMAGE_LEN] {
    let mut image = [0u8; CATALOG_IMAGE_LEN];
    image[..CATALOG_SLOT_LEN].copy_from_slice(&slot.encode());
    image
}

fn image_with_lookup<L: CommittedObjectLookup<Error = Infallible>>(
    lookup: &L,
) -> (Arc<[u8]>, CatalogSlotV1, u64) {
    let object_store = CanonicalObjectStore::new();
    let base = base_state(&object_store, 7, None);
    let mutation = data_batch(base, "durable-recovery");
    let sealed = seal_data(&object_store, &mutation);
    let store_id = StoreId::from_array([0x51; 16]);
    let sequence = LogPackSequence::FIRST;
    let prepared = PreparedLogPackTransactionV1::<TestSemanticProfile>::new(
        &sealed,
        LogPackObjectPlanV1::classify(&sealed, lookup).unwrap(),
        store_id,
        sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let expected_index_tail = INDEX_HEADER_LEN_V1 + prepared.inline_count() * INDEX_RECORD_LEN_V1;
    let mut bytes = PackHeaderV1::genesis(store_id).encode().to_vec();
    let prepare_offset = bytes.len() as u64;
    let prepare = prepared.write_to(&mut bytes).unwrap();
    let prepare_address =
        LogPackAddress::new(PackId::GENESIS.get(), prepare_offset, prepare.record_len()).unwrap();
    let commit_offset = bytes.len() as u64;
    let commit_address =
        LogPackAddress::new(PackId::GENESIS.get(), commit_offset, TRANSACTION_COMMIT_RECORD_LEN_V1)
            .unwrap();
    let pack_state = CatalogPackStateV1::new(
        PackId::GENESIS.get(),
        PackId::GENESIS.get(),
        commit_address.end(),
        IndexGenerationV1::FIRST.get(),
        expected_index_tail,
        synthetic_ref(0xd7),
        4096,
        4096,
    )
    .unwrap();
    let commit = TransactionCommitRecordV1::new(
        sealed.receipt(),
        store_id,
        PhysicalDigest::ZERO,
        prepare_address,
        prepare,
        pack_state.commit_publication(),
    )
    .unwrap();
    bytes.extend_from_slice(commit.canonical_bytes());
    let catalog = CatalogSlotV1::new(
        store_id,
        1,
        sealed.receipt().target(),
        TEST_SEMANTIC_PROFILE_ID,
        commit.digest(),
        sequence,
        commit_address,
        pack_state,
    )
    .unwrap();
    (bytes.into(), catalog, prepared.inline_count())
}

#[test]
fn exact_pack_chain_scans_iteratively_and_consumes_the_catalog_tip() {
    let (bytes, catalog, data_count) = image_with_lookup(&MissingLookup);
    let mut provider = MemoryProvider::new(PackId::GENESIS, Arc::clone(&bytes));
    let token = token_with_slot(catalog, false);
    let mut observed = Vec::new();
    let scanned = scan_catalog_authorized_pack_chain(
        &mut provider,
        &token,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        |record| {
            observed.push(record);
            Ok(())
        },
    )
    .unwrap();
    let recovered = scanned.verify_catalog(token).unwrap();
    assert_eq!(provider.opens, 1);
    assert_eq!(recovered.transactions(), 1);
    assert_eq!(recovered.object_records(), data_count);
    assert_eq!(recovered.object_index().record_count(), data_count);
    assert_eq!(observed.len() as u64, data_count);
    assert_eq!(recovered.expected_index_tail(), catalog.durable_index_tail());
    assert_eq!(recovered.scanned_bytes(), bytes.len() as u64);
}

#[test]
fn generation_checkpoint_reuse_and_short_physical_pack_fail_closed() {
    let (reuse_bytes, reuse_catalog, _) = image_with_lookup(&EqualFirst);
    let reuse_token = token_with_slot(reuse_catalog, false);
    let mut reuse_provider = MemoryProvider::new(PackId::GENESIS, reuse_bytes);
    assert!(matches!(
        scan_catalog_authorized_pack_chain(
            &mut reuse_provider,
            &reuse_token,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            |_| Ok(()),
        ),
        Err(RecoveryError::GenerationCheckpointReuse(_))
    ));

    let (bytes, catalog, _) = image_with_lookup(&MissingLookup);
    let token = token_with_slot(catalog, false);
    let mut truncated = bytes.to_vec();
    truncated.pop();
    let mut provider = MemoryProvider::new(PackId::GENESIS, truncated);
    assert!(matches!(
        scan_catalog_authorized_pack_chain(
            &mut provider,
            &token,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            |_| Ok(()),
        ),
        Err(RecoveryError::DurableTail {
            pack: PackId::GENESIS,
            tail,
            available,
        }) if tail == catalog.durable_pack_tail() && available + 1 == tail
    ));
}

#[test]
fn exact_catalog_image_is_consumed_before_recovered_authority_is_exposed() {
    let (bytes, catalog, _) = image_with_lookup(&MissingLookup);
    let scan_token = token_with_slot(catalog, false);
    let mut provider = MemoryProvider::new(PackId::GENESIS, bytes);
    let scanned = scan_catalog_authorized_pack_chain(
        &mut provider,
        &scan_token,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        |_| Ok(()),
    )
    .unwrap();
    assert!(matches!(
        scanned.verify_catalog(token_with_slot(catalog, true)),
        Err(RecoveryError::CatalogAuthorityChanged)
    ));
}

#[test]
fn range_and_object_work_are_admitted_before_open_or_variable_body_read() {
    let (bytes, catalog, data_count) = image_with_lookup(&MissingLookup);
    let commit_address =
        LogPackAddress::new(2, PACK_HEADER_LEN_V1, TRANSACTION_COMMIT_RECORD_LEN_V1).unwrap();
    let wide_pack = CatalogPackStateV1::new(
        1,
        2,
        commit_address.end(),
        1,
        catalog.durable_index_tail(),
        catalog.pack().retention_manifest(),
        catalog.pack().current_state_canonical_bytes(),
        catalog.pack().retained_canonical_bytes(),
    )
    .unwrap();
    let wide_catalog = CatalogSlotV1::new(
        catalog.store_id(),
        catalog.epoch(),
        catalog.state(),
        catalog.profile_id(),
        catalog.commit_digest(),
        catalog.commit_sequence(),
        commit_address,
        wide_pack,
    )
    .unwrap();
    let wide_token = token_with_slot(wide_catalog, false);
    let mut provider = MemoryProvider::new(PackId::GENESIS, Arc::clone(&bytes));
    let mut config = limit_config();
    config.max_live_packs = 1;
    assert!(matches!(
        scan_catalog_authorized_pack_chain(
            &mut provider,
            &wide_token,
            DurableLimits::new(config).unwrap(),
            LogPackDecodeLimits::UNBOUNDED,
            |_| Ok(()),
        ),
        Err(RecoveryError::Limit(error))
            if error == crate::rspace::persistence::DurableLimitError::Exceeded {
                resource: DurableLimitKind::LivePacks,
                limit: 1,
                attempted: 2,
            }
    ));
    assert_eq!(provider.opens, 0);

    let token = token_with_slot(catalog, false);
    let mut provider = MemoryProvider::new(PackId::GENESIS, bytes);
    let reads = Arc::clone(&provider.reader.reads);
    let mut config = limit_config();
    config.max_object_records = data_count.saturating_sub(1).max(1);
    assert!(matches!(
        scan_catalog_authorized_pack_chain(
            &mut provider,
            &token,
            DurableLimits::new(config).unwrap(),
            LogPackDecodeLimits::UNBOUNDED,
            |_| Ok(()),
        ),
        Err(RecoveryError::Limit(_))
    ));
    let variable_body_offset = PACK_HEADER_LEN_V1 + TRANSACTION_PREPARE_HEADER_LEN_V1;
    assert!(
        reads
            .lock()
            .unwrap()
            .iter()
            .all(|(offset, _)| *offset != variable_body_offset)
    );
}

#[test]
fn two_pack_chain_preserves_exact_linkage_and_holds_one_reader_at_a_time() {
    let image = two_pack_image(SecondPackFault::None);
    let second_pack = PackId::GENESIS.successor().unwrap();
    let mut provider = MultiPackProvider::new([
        (PackId::GENESIS, Arc::clone(&image.first)),
        (second_pack, Arc::clone(&image.second)),
    ]);
    let token = token_with_slot(image.catalog, false);
    let scanned = scan_catalog_authorized_pack_chain(
        &mut provider,
        &token,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        |_| Ok(()),
    )
    .unwrap();
    let recovered = scanned.verify_catalog(token).unwrap();
    assert_eq!(recovered.transactions(), image.transactions);
    assert_eq!(recovered.terminal().pack_id(), second_pack);
    assert_eq!(recovered.terminal_address(), image.catalog.commit_address());
    assert_eq!(provider.opens, 2);
    assert_eq!(provider.maximum_live.get(), 1);
    assert_eq!(provider.live.get(), 0);
}

#[test]
fn predecessor_suffix_is_refused_but_active_crash_suffix_is_excluded() {
    let image = two_pack_image(SecondPackFault::None);
    let second_pack = PackId::GENESIS.successor().unwrap();

    let mut first_with_suffix = image.first.to_vec();
    first_with_suffix.push(0xa5);
    let mut provider = MultiPackProvider::new([
        (PackId::GENESIS, first_with_suffix.into()),
        (second_pack, Arc::clone(&image.second)),
    ]);
    let token = token_with_slot(image.catalog, false);
    assert!(matches!(
        scan_catalog_authorized_pack_chain(
            &mut provider,
            &token,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            |_| Ok(()),
        ),
        Err(RecoveryError::AuthorizedTailCutsRecord {
            pack: PackId::GENESIS,
            ..
        })
    ));
    assert_eq!(provider.opens, 1);
    assert_eq!(provider.live.get(), 0);

    let mut active_with_suffix = image.second.to_vec();
    active_with_suffix.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    let mut provider = MultiPackProvider::new([
        (PackId::GENESIS, Arc::clone(&image.first)),
        (second_pack, active_with_suffix.into()),
    ]);
    let token = token_with_slot(image.catalog, false);
    let scanned = scan_catalog_authorized_pack_chain(
        &mut provider,
        &token,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        |_| Ok(()),
    )
    .unwrap();
    let recovered = scanned.verify_catalog(token).unwrap();
    assert_eq!(
        recovered.scanned_bytes(),
        image.first.len() as u64 + image.catalog.durable_pack_tail()
    );
    assert_eq!(provider.maximum_live.get(), 1);
    assert_eq!(provider.live.get(), 0);
}

#[test]
fn terminal_publication_digest_and_address_mismatches_fail_before_authority() {
    let image = two_pack_image(SecondPackFault::None);
    let second_pack = PackId::GENESIS.successor().unwrap();
    let changed_pack = CatalogPackStateV1::new(
        image.catalog.pack().first_live_pack(),
        image.catalog.pack().active_pack(),
        image.catalog.pack().durable_pack_tail(),
        image.catalog.pack().index_generation(),
        image.catalog.pack().durable_index_tail(),
        synthetic_ref(0xef),
        image.catalog.pack().current_state_canonical_bytes(),
        image.catalog.pack().retained_canonical_bytes(),
    )
    .unwrap();
    let changed_publication = CatalogSlotV1::new(
        image.catalog.store_id(),
        image.catalog.epoch(),
        image.catalog.state(),
        image.catalog.profile_id(),
        image.catalog.commit_digest(),
        image.catalog.commit_sequence(),
        image.catalog.commit_address(),
        changed_pack,
    )
    .unwrap();
    let token = token_with_slot(changed_publication, false);
    let mut provider = MultiPackProvider::new([
        (PackId::GENESIS, Arc::clone(&image.first)),
        (second_pack, Arc::clone(&image.second)),
    ]);
    let scanned = scan_catalog_authorized_pack_chain(
        &mut provider,
        &token,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        |_| Ok(()),
    )
    .unwrap();
    assert!(matches!(
        scanned.verify_catalog(token),
        Err(RecoveryError::Catalog(DurableFormatError::CatalogCommitPublication { .. }))
    ));

    let changed_digest = CatalogSlotV1::new(
        image.catalog.store_id(),
        image.catalog.epoch(),
        image.catalog.state(),
        image.catalog.profile_id(),
        PhysicalDigest::of_bytes(b"catalog points at a different commit"),
        image.catalog.commit_sequence(),
        image.catalog.commit_address(),
        image.catalog.pack(),
    )
    .unwrap();
    let token = token_with_slot(changed_digest, false);
    let mut provider = MultiPackProvider::new([
        (PackId::GENESIS, Arc::clone(&image.first)),
        (second_pack, Arc::clone(&image.second)),
    ]);
    let scanned = scan_catalog_authorized_pack_chain(
        &mut provider,
        &token,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        |_| Ok(()),
    )
    .unwrap();
    assert!(matches!(
        scanned.verify_catalog(token),
        Err(RecoveryError::Catalog(DurableFormatError::CatalogCommitDigest { .. }))
    ));

    let wrong_segment = LogPackAddress::new(
        PackId::GENESIS.get(),
        image.catalog.commit_address().offset(),
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )
    .unwrap();
    assert!(matches!(
        CatalogSlotV1::new(
            image.catalog.store_id(),
            image.catalog.epoch(),
            image.catalog.state(),
            image.catalog.profile_id(),
            image.catalog.commit_digest(),
            image.catalog.commit_sequence(),
            wrong_segment,
            image.catalog.pack(),
        ),
        Err(DurableFormatError::CatalogCommitSegment { .. })
    ));
}

#[test]
fn cross_pack_sequence_state_and_previous_commit_discontinuities_fail_closed() {
    for fault in [
        SecondPackFault::HeaderSequence,
        SecondPackFault::StateBase,
        SecondPackFault::PreviousCommit,
    ] {
        let image = two_pack_image(fault);
        let second_pack = PackId::GENESIS.successor().unwrap();
        let mut provider =
            MultiPackProvider::new([(PackId::GENESIS, image.first), (second_pack, image.second)]);
        let token = token_with_slot(image.catalog, false);
        let result = scan_catalog_authorized_pack_chain(
            &mut provider,
            &token,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            |_| Ok(()),
        );
        match fault {
            SecondPackFault::HeaderSequence => {
                assert!(matches!(result, Err(RecoveryError::Pack(_))))
            }
            SecondPackFault::StateBase => {
                assert!(matches!(result, Err(RecoveryError::StateContinuity { .. })))
            }
            SecondPackFault::PreviousCommit => {
                assert!(matches!(result, Err(RecoveryError::PreviousCommit { .. })))
            }
            SecondPackFault::None => unreachable!(),
        }
        assert_eq!(provider.maximum_live.get(), 1);
        assert_eq!(provider.live.get(), 0);
    }
}
