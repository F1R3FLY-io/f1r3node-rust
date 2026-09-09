use std::collections::BTreeMap;

use models::rust::casper::protocol::casper_message::{ApprovedBlock, ApprovedBlockCandidate};
use proptest::prelude::*;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::history::cold_store::{
    ContinuationsLeaf, DataLeaf, JoinsLeaf, PersistedData,
};
use rspace_plus_plus::rspace::history::history_reader::HistoryReader;
use rspace_plus_plus::rspace::history::history_repository::PREFIX_JOINS;
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
use rspace_plus_plus::rspace::history::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;
use rspace_plus_plus::rspace::history::radix_tree::{
    empty_node, hash_node, sequential_export, ExportDataSettings, Item,
};
use rspace_plus_plus::rspace::history::roots_store::{RootsStore, RootsStoreInstances};
use rspace_plus_plus::rspace::serializers::serializers::encode_joins;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::instances::rspace_exporter_store::RSpaceExporterStore;
use rspace_plus_plus::rspace::state::instances::rspace_importer_store::RSpaceImporterStore;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporterInstance;
use shared::rust::store::key_value_store::{
    strict_atomic_mutate, AtomicStoreMutation, AtomicStoreOperation, EntryReader, KeyValueStore,
    KvStoreError, ValueReader,
};

use super::*;

struct Stores {
    history: Arc<dyn KeyValueStore>,
    cold: Arc<dyn KeyValueStore>,
    roots: Arc<dyn KeyValueStore>,
}

#[derive(Debug, PartialEq, Eq)]
struct StoreSnapshot {
    history: BTreeMap<Vec<u8>, Vec<u8>>,
    cold: BTreeMap<Vec<u8>, Vec<u8>>,
    roots: BTreeMap<Vec<u8>, Vec<u8>>,
}

#[derive(Clone)]
struct FaultingKeyStore {
    inner: Arc<dyn KeyValueStore>,
    key: Vec<u8>,
    fault: KeyStoreFault,
    failures: Arc<std::sync::atomic::AtomicUsize>,
    reads: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyStoreFault {
    Read,
    BeforeWrite,
    AfterWrite,
}

impl FaultingKeyStore {
    fn reject_operation(&self) -> KvStoreError {
        self.failures
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        KvStoreError::IoError(match self.fault {
            KeyStoreFault::Read => "injected store read failure".to_owned(),
            KeyStoreFault::BeforeWrite => "injected failure before write".to_owned(),
            KeyStoreFault::AfterWrite => "injected failure after write".to_owned(),
        })
    }
}

impl KeyValueStore for FaultingKeyStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<Vec<u8>>) -> Result<Vec<Option<Vec<u8>>>, KvStoreError> {
        self.reads.lock().unwrap().extend(keys.iter().cloned());
        if self.fault == KeyStoreFault::Read && keys.contains(&self.key) {
            Err(self.reject_operation())
        } else {
            self.inner.get(keys)
        }
    }

    fn with_value(&self, key: &Vec<u8>, reader: &mut ValueReader<'_>) -> Result<(), KvStoreError> {
        self.reads.lock().unwrap().push(key.clone());
        if self.fault == KeyStoreFault::Read && key == &self.key {
            Err(self.reject_operation())
        } else {
            self.inner.with_value(key, reader)
        }
    }

    fn visit_entries(&self, reader: &mut EntryReader<'_>) -> Result<(), KvStoreError> {
        self.inner.visit_entries(reader)
    }

    fn put(&self, rows: Vec<(Vec<u8>, Vec<u8>)>) -> Result<(), KvStoreError> {
        let matches = rows.iter().any(|(key, _)| key == &self.key);
        if matches && self.fault == KeyStoreFault::BeforeWrite {
            return Err(self.reject_operation());
        }
        self.inner.put(rows)?;
        if matches && self.fault == KeyStoreFault::AfterWrite {
            return Err(self.reject_operation());
        }
        Ok(())
    }

    fn put_one_if_absent(&self, key: Vec<u8>, value: Vec<u8>) -> Result<bool, KvStoreError> {
        self.inner.put_one_if_absent(key, value)
    }

    fn delete(&self, keys: Vec<Vec<u8>>) -> Result<usize, KvStoreError> { self.inner.delete(keys) }

    fn iterate(&self, visitor: fn(Vec<u8>, Vec<u8>)) -> Result<(), KvStoreError> {
        self.inner.iterate(visitor)
    }

    fn iterate_while(
        &self,
        visitor: &mut dyn FnMut(Vec<u8>, Vec<u8>) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(visitor)
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, KvStoreError> { self.inner.to_map() }

    fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

    fn size_bytes(&self) -> usize { self.inner.size_bytes() }
}

impl Stores {
    async fn new() -> Self {
        let mut manager = InMemoryStoreManager::new();
        let stores = Self {
            history: manager.store("history".to_owned()).await.unwrap(),
            cold: manager.store("cold".to_owned()).await.unwrap(),
            roots: manager.store("roots".to_owned()).await.unwrap(),
        };
        let (root, encoded) = hash_node(&empty_node());
        stores.history.put_one(root.clone(), encoded).unwrap();
        RootsStoreInstances::roots_store(stores.roots.clone())
            .record_root(&Blake2b256Hash::from_bytes(root))
            .unwrap();
        stores
    }

    fn snapshot(&self) -> StoreSnapshot {
        StoreSnapshot {
            history: self.history.to_map().unwrap(),
            cold: self.cold.to_map().unwrap(),
            roots: self.roots.to_map().unwrap(),
        }
    }

    fn core(&self) -> Core {
        let importer = Arc::new(RSpaceImporterStore::create(
            self.history.clone(),
            self.cold.clone(),
            self.roots.clone(),
        ));
        let roots = RootsStoreInstances::roots_store(self.roots.clone());
        Core::new(
            importer,
            Arc::new(move |root| {
                roots
                    .contains_root(root)
                    .map_err(|error| CasperError::Other(error.to_string()))
            }),
            Duration::from_millis(10),
        )
    }

    fn read_joins(&self, root: &Blake2b256Hash) -> Vec<Vec<String>> {
        let history = RadixHistory::create(root.clone(), self.history.clone()).unwrap();
        let reader = RSpaceHistoryReaderImpl::<String, String, String, String>::new(
            Box::new(history),
            self.cold.clone(),
        );
        let channel = "channel".to_owned();
        let joins = reader.base().get_joins(&channel);
        assert_eq!(reader.get_joins(&hash(&channel)).unwrap(), joins);
        joins
    }

    fn export(&self, root: &Blake2b256Hash) -> StoreItemsMessage {
        let start_path = vec![(root.clone(), None)];
        let exporter = Arc::new(RSpaceExporterStore::create(
            self.history.clone(),
            self.cold.clone(),
            self.roots.clone(),
        ));
        let (history, data) =
            RSpaceExporterItems::get_history_and_data(exporter, start_path.clone(), 0, PAGE_SIZE);
        assert_eq!(history.last_path, data.last_path);
        StoreItemsMessage {
            start_path,
            last_path: history.last_path,
            history_items: history
                .items
                .into_iter()
                .map(|(key, value)| (key, Bytes::from(value)))
                .collect(),
            data_items: data
                .items
                .into_iter()
                .map(|(key, value)| (key, Bytes::from(value)))
                .collect(),
        }
    }
}

async fn exported_joins_trie(leaf: JoinsLeaf) -> (Stores, Blake2b256Hash, StoreItemsMessage) {
    let source = Stores::new().await;
    let leaf_hash = Blake2b256Hash::new(&bincode::serialize(&leaf).unwrap());
    source
        .cold
        .put_one(
            bincode::serialize(&leaf_hash).unwrap(),
            bincode::serialize(&PersistedData::Joins(leaf)).unwrap(),
        )
        .unwrap();
    let mut node = empty_node();
    node[usize::from(PREFIX_JOINS)] = Item::Leaf {
        prefix: hash(&"channel".to_owned()).bytes(),
        value: leaf_hash.bytes(),
    };
    let (root_bytes, encoded) = hash_node(&node);
    source.history.put_one(root_bytes.clone(), encoded).unwrap();
    let root = Blake2b256Hash::from_bytes(root_bytes);
    RootsStoreInstances::roots_store(source.roots.clone())
        .record_root(&root)
        .unwrap();
    let page = source.export(&root);
    assert_eq!(page.history_items.len(), 1);
    assert_eq!(page.history_items[0].0, root);
    assert_eq!(page.data_items.len(), 1);
    (source, root, page)
}

async fn exported_nonempty_trie() -> (Blake2b256Hash, StoreItemsMessage) {
    let (source, root, page) = exported_joins_trie(JoinsLeaf {
        bytes: encode_joins(&vec![vec!["channel".to_owned()]]),
    })
    .await;
    assert_eq!(source.read_joins(&root), vec![vec!["channel".to_owned()]]);
    (root, page)
}

fn validate_page(core: &Core, page: &StoreItemsMessage) -> Result<(), String> {
    RSpaceImporterInstance::validate_state_items(
        page.history_items
            .iter()
            .map(|(key, value)| (key.clone(), value.to_vec()))
            .collect(),
        page.data_items
            .iter()
            .map(|(key, value)| (key.clone(), value.to_vec()))
            .collect(),
        page.start_path.clone(),
        PAGE_SIZE,
        0,
        core.importer.clone(),
    )
}

fn raw_history_page(bytes: Vec<u8>) -> StoreItemsMessage {
    let root = Blake2b256Hash::new(&bytes);
    let path = vec![(root.clone(), None)];
    StoreItemsMessage {
        start_path: path.clone(),
        last_path: path,
        history_items: vec![(root, Bytes::from(bytes))],
        data_items: Vec::new(),
    }
}

#[tokio::test]
async fn canonical_history_overlay_accepts_empty_node_encoding() {
    let destination = Stores::new().await;
    let before = destination.snapshot();
    validate_page(&destination.core(), &raw_history_page(Vec::new())).unwrap();
    assert_eq!(destination.snapshot(), before);
}

#[tokio::test]
async fn canonical_history_overlay_rejects_conflicting_duplicate_rows_in_either_order() {
    let (_, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let before = destination.snapshot();
    for invalid_first in [false, true] {
        let mut altered = page.clone();
        let invalid = (page.history_items[0].0.clone(), Bytes::from(vec![99]));
        assert_ne!(Blake2b256Hash::new(&invalid.1), invalid.0);
        if invalid_first {
            altered.history_items.insert(0, invalid);
        } else {
            altered.history_items.push(invalid);
        }
        assert!(validate_page(&destination.core(), &altered).is_err());
        assert_eq!(destination.snapshot(), before);
    }
}

#[tokio::test]
async fn canonical_history_overlay_rejects_conflicting_stored_bytes() {
    let (root, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let conflict = vec![99];
    assert_ne!(Blake2b256Hash::new(&conflict), root);
    destination.history.put_one(root.bytes(), conflict).unwrap();
    let before = destination.snapshot();
    let result = validate_page(&destination.core(), &page);
    assert_eq!(destination.snapshot(), before);
    assert!(
        result.is_err(),
        "received rows must not hide a stored-byte conflict"
    );
}

#[tokio::test]
async fn canonical_history_overlay_rejects_local_read_failure() {
    let (root, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let before = destination.snapshot();
    let failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let history = FaultingKeyStore {
        fault: KeyStoreFault::Read,
        inner: destination.history.clone(),
        key: root.bytes(),
        failures: failures.clone(),
        reads: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    assert!(matches!(
        history.get_one(&root.bytes()),
        Err(KvStoreError::IoError(_))
    ));
    failures.store(0, std::sync::atomic::Ordering::SeqCst);
    let mut core = destination.core();
    core.importer = Arc::new(RSpaceImporterStore::create(
        Arc::new(history),
        destination.cold.clone(),
        destination.roots.clone(),
    ));
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| validate_page(&core, &page)));
    assert_eq!(destination.snapshot(), before);
    assert!(
        matches!(result, Ok(Err(_))),
        "local read failure must reject without a panic"
    );
    assert!(failures.load(std::sync::atomic::Ordering::SeqCst) > 0);
}

async fn exported_history_fallback_page() -> (Stores, Blake2b256Hash, StoreItemsMessage) {
    let (source, child, _) = exported_joins_trie(JoinsLeaf {
        bytes: encode_joins(&vec![vec!["channel".to_owned()]]),
    })
    .await;
    let mut root_node = empty_node();
    root_node[0] = Item::NodePtr {
        prefix: Vec::new(),
        ptr: child.bytes(),
    };
    let (root_bytes, root_encoded) = hash_node(&root_node);
    source
        .history
        .put_one(root_bytes.clone(), root_encoded)
        .unwrap();
    let root = Blake2b256Hash::from_bytes(root_bytes);
    let start_path = vec![(root.clone(), None)];
    let exporter = Arc::new(RSpaceExporterStore::create(
        source.history.clone(),
        source.cold.clone(),
        source.roots.clone(),
    ));
    let (history, data) =
        RSpaceExporterItems::get_history_and_data(exporter, start_path.clone(), 1, PAGE_SIZE);
    assert_eq!(history.last_path, data.last_path);
    assert_eq!(history.items.len(), 1);
    assert_eq!(history.items[0].0, child);
    assert_eq!(data.items.len(), 1);
    let page = StoreItemsMessage {
        start_path,
        last_path: history.last_path,
        history_items: history
            .items
            .into_iter()
            .map(|(key, value)| (key, Bytes::from(value)))
            .collect(),
        data_items: data
            .items
            .into_iter()
            .map(|(key, value)| (key, Bytes::from(value)))
            .collect(),
    };
    (source, root, page)
}

fn validate_history_fallback_page(core: &Core, page: &StoreItemsMessage) -> Result<(), String> {
    RSpaceImporterInstance::validate_state_items(
        page.history_items
            .iter()
            .map(|(key, value)| (key.clone(), value.to_vec()))
            .collect(),
        page.data_items
            .iter()
            .map(|(key, value)| (key.clone(), value.to_vec()))
            .collect(),
        page.start_path.clone(),
        PAGE_SIZE,
        1,
        core.importer.clone(),
    )
}

#[tokio::test]
async fn physical_history_observation_returns_fallback_read_error_without_panicking() {
    let (destination, root, page) = exported_history_fallback_page().await;
    validate_history_fallback_page(&destination.core(), &page)
        .expect("the real exporter supplies a valid fallback page");
    let before = destination.snapshot();
    let failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let reads = Arc::new(std::sync::Mutex::new(Vec::new()));
    let history = FaultingKeyStore {
        fault: KeyStoreFault::Read,
        inner: destination.history.clone(),
        key: root.bytes(),
        failures: failures.clone(),
        reads: reads.clone(),
    };
    assert!(matches!(
        history.get_one(&root.bytes()),
        Err(KvStoreError::IoError(_))
    ));
    failures.store(0, std::sync::atomic::Ordering::SeqCst);
    reads.lock().unwrap().clear();
    let mut core = destination.core();
    core.importer = Arc::new(RSpaceImporterStore::create(
        Arc::new(history),
        destination.cold.clone(),
        destination.roots.clone(),
    ));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        validate_history_fallback_page(&core, &page)
    }));
    assert_eq!(destination.snapshot(), before);
    assert_eq!(failures.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        reads
            .lock()
            .unwrap()
            .iter()
            .filter(|key| **key == root.bytes())
            .count(),
        1
    );
    assert!(
        matches!(result, Ok(Err(_))),
        "a required fallback read failure must return an error without a panic"
    );
}

#[tokio::test]
async fn physical_history_observation_reuses_duplicate_root_lookups() {
    let (destination, root, page) = exported_history_fallback_page().await;
    let before = destination.snapshot();
    let failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let reads = Arc::new(std::sync::Mutex::new(Vec::new()));
    let history = FaultingKeyStore {
        fault: KeyStoreFault::Read,
        inner: destination.history.clone(),
        key: Vec::new(),
        failures: failures.clone(),
        reads: reads.clone(),
    };
    let mut core = destination.core();
    core.importer = Arc::new(RSpaceImporterStore::create(
        Arc::new(history),
        destination.cold.clone(),
        destination.roots.clone(),
    ));
    validate_history_fallback_page(&core, &page).unwrap();
    assert_eq!(destination.snapshot(), before);
    assert_eq!(failures.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        reads
            .lock()
            .unwrap()
            .iter()
            .filter(|key| **key == root.bytes())
            .count(),
        1,
        "repeated logical root lookups must reuse the attempt observation"
    );
}

async fn exported_unsorted_history() -> StoreItemsMessage {
    let source = Stores::new().await;
    let mut child_records = Vec::new();
    let mut slots = HashSet::new();
    for channel in ["unsorted-first".to_owned(), "unsorted-second".to_owned()] {
        let channel_hash = hash(&channel).bytes();
        assert!(slots.insert(channel_hash[0]));
        let leaf = JoinsLeaf {
            bytes: encode_joins(&vec![vec![channel]]),
        };
        let leaf_hash = Blake2b256Hash::new(&bincode::serialize(&leaf).unwrap());
        source
            .cold
            .put_one(
                bincode::serialize(&leaf_hash).unwrap(),
                bincode::serialize(&PersistedData::Joins(leaf)).unwrap(),
            )
            .unwrap();
        let mut record = vec![channel_hash[0], 31];
        record.extend_from_slice(&channel_hash[1..]);
        record.extend(leaf_hash.bytes());
        child_records.push(record);
    }
    child_records.sort_by(|left, right| right[0].cmp(&left[0]));
    assert!(child_records[0][0] > child_records[1][0]);
    let child_bytes = child_records.concat();
    let child_hash = Blake2b256Hash::new(&child_bytes);
    source
        .history
        .put_one(child_hash.bytes(), child_bytes)
        .unwrap();
    let mut root_node = empty_node();
    root_node[usize::from(PREFIX_JOINS)] = Item::NodePtr {
        prefix: Vec::new(),
        ptr: child_hash.bytes(),
    };
    let (root_bytes, root_encoded) = hash_node(&root_node);
    source
        .history
        .put_one(root_bytes.clone(), root_encoded)
        .unwrap();
    let root = Blake2b256Hash::from_bytes(root_bytes);
    let page = source.export(&root);
    assert_eq!(page.history_items.len(), 2);
    assert_eq!(page.data_items.len(), 2);
    page
}

#[tokio::test]
async fn canonical_history_overlay_accepts_unsorted_records_and_identical_rows() {
    let original = exported_unsorted_history().await;
    let destination = Stores::new().await;
    let before = destination.snapshot();
    for order in [[0, 0, 1], [0, 1, 0], [1, 0, 0], [1, 1, 0], [1, 0, 1], [
        0, 1, 1,
    ]] {
        let mut page = original.clone();
        page.history_items = order
            .map(|index| original.history_items[index].clone())
            .to_vec();
        validate_page(&destination.core(), &page).unwrap();
        assert_eq!(destination.snapshot(), before);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        max_shrink_iters: 2_048,
        ..ProptestConfig::default()
    })]

    #[test]
    fn history_guard_native_batch_matches_the_transaction_state(
        initial in prop::collection::vec((0u8..8, prop::collection::vec(any::<u8>(), 0..33)), 0..17),
        intervening in prop::collection::vec((0u8..8, prop::collection::vec(any::<u8>(), 0..33)), 0..17),
        incoming in prop::collection::vec((0u8..8, prop::collection::vec(any::<u8>(), 0..33)), 0..17),
        unrelated in prop::collection::vec(any::<u8>(), 0..33),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let (actual, observed, expected, compatible) = runtime.block_on(async {
            let stores = Stores::new().await;
            for (key, bytes) in initial {
                stores.history.put_one(vec![key; 32], bytes).unwrap();
            }
            let keys = incoming.iter().map(|(key, _)| vec![*key; 32]).collect();
            let preflight = stores.history.get(&keys).unwrap();
            assert_eq!(preflight.len(), incoming.len());
            for (key, bytes) in intervening {
                stores.history.put_one(vec![key; 32], bytes).unwrap();
            }
            stores.cold.put_one(vec![33; 32], unrelated.clone()).unwrap();
            stores.history.put_one(vec![34; 32], unrelated).unwrap();
            let mut expected = stores.snapshot();
            let mut candidate = expected.history.clone();
            let mut compatible = true;
            for (key, bytes) in &incoming {
                let key = vec![*key; 32];
                if candidate.get(&key).is_some_and(|existing| existing != bytes) {
                    compatible = false;
                    break;
                }
                candidate.insert(key, bytes.clone());
            }
            if compatible {
                expected.history = candidate;
            }
            let mutations: Vec<_> = incoming.into_iter().map(|(key, bytes)| AtomicStoreMutation {
                store: stores.history.as_ref(),
                key: vec![key; 32],
                operation: AtomicStoreOperation::PutIfAbsentOrEqual(bytes),
            }).collect();
            let actual = strict_atomic_mutate(&mutations);
            (actual, stores.snapshot(), expected, compatible)
        });
        prop_assert_eq!(actual.is_ok(), compatible);
        if !compatible {
            prop_assert!(matches!(actual, Err(KvStoreError::TransactionConflict(_))));
        }
        prop_assert_eq!(observed, expected);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        max_shrink_iters: 512,
        ..ProptestConfig::default()
    })]

    #[test]
    fn history_guard_native_concurrent_batches_preserve_whole_commits(
        value in prop::collection::vec(any::<u8>(), 0..33),
        conflict in any::<bool>(),
        first_private in prop::collection::vec(any::<u8>(), 0..33),
        second_private in prop::collection::vec(any::<u8>(), 0..33),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let stores = runtime.block_on(Stores::new());
        let mut expected = stores.snapshot();
        let first_value = value.clone();
        let mut second_value = value;
        if conflict {
            second_value.push(0);
        }
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let batches = [
            (41u8, first_private, first_value),
            (42u8, second_private, second_value),
        ];
        let workers: Vec<_> = batches.iter().cloned().map(|(key, private, shared)| {
            let history = stores.history.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let observed = history.get_one(&vec![40; 32]);
                barrier.wait();
                assert_eq!(observed.unwrap(), None);
                strict_atomic_mutate(&[
                    AtomicStoreMutation {
                        store: history.as_ref(),
                        key: vec![key; 32],
                        operation: AtomicStoreOperation::PutIfAbsentOrEqual(private),
                    },
                    AtomicStoreMutation {
                        store: history.as_ref(),
                        key: vec![40; 32],
                        operation: AtomicStoreOperation::PutIfAbsentOrEqual(shared),
                    },
                ])
            })
        }).collect();
        let outcomes: Vec<_> = workers.into_iter().map(|worker| worker.join().unwrap()).collect();
        prop_assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), if conflict { 1 } else { 2 });
        for ((key, private, shared), result) in batches.into_iter().zip(outcomes) {
            if result.is_ok() {
                expected.history.insert(vec![key; 32], private);
                expected.history.insert(vec![40; 32], shared);
            } else {
                prop_assert!(matches!(result, Err(KvStoreError::TransactionConflict(_))));
            }
        }
        prop_assert_eq!(stores.snapshot(), expected);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        max_shrink_iters: 32,
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_history_overlay_rejects_duplicate_slots_without_panicking(
        slot in any::<u8>(),
        prefix in prop::collection::vec(any::<u8>(), 0..9),
        key in prop::array::uniform32(any::<u8>()),
    ) {
        let mut record = vec![slot, prefix.len() as u8];
        record.extend(prefix);
        record.extend(key);
        let bytes = [record.clone(), record].concat();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let destination = runtime.block_on(Stores::new());
        let before = destination.snapshot();
        let core = destination.core();
        let page = raw_history_page(bytes);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| validate_page(&core, &page)));
        prop_assert_eq!(destination.snapshot(), before);
        prop_assert!(matches!(result, Ok(Err(_))), "duplicate slots must return an error without unwinding");
    }
}

fn read_tape_export_settings() -> ExportDataSettings {
    ExportDataSettings {
        flag_node_prefixes: true,
        flag_node_keys: true,
        flag_node_values: true,
        flag_leaf_prefixes: true,
        flag_leaf_values: true,
    }
}

struct UnpolledStartupOps;

#[async_trait::async_trait]
impl crate::rust::engine::lfs_tuple_space_requester::TupleSpaceRequesterOps for UnpolledStartupOps {
    async fn request_for_store_item(
        &self,
        _path: &StatePartPath,
        _page_size: i32,
    ) -> Result<(), CasperError> {
        panic!("an unpolled startup stream must not dispatch network requests")
    }

    fn validate_tuple_space_items(
        &self,
        _history_items: Vec<(Blake2b256Hash, Vec<u8>)>,
        _data_items: Vec<(Blake2b256Hash, Vec<u8>)>,
        _start_path: StatePartPath,
        _page_size: i32,
        _skip: i32,
        _get_from_history: Arc<dyn RSpaceImporter>,
    ) -> Result<(), CasperError> {
        panic!("an unpolled startup stream has no received pages to validate")
    }
}

#[tokio::test]
async fn startup_import_construction_must_not_publish_unreceived_state() {
    let destination = Stores::new().await;
    let (target, _) = exported_nonempty_trie().await;
    let roots = RootsStoreInstances::roots_store(destination.roots.clone());
    let previous_root = roots.current_root().unwrap();
    let before = destination.snapshot();
    assert_ne!(previous_root.as_ref(), Some(&target));
    assert!(!roots.contains_root(&target).unwrap());
    assert!(destination
        .history
        .get_one(&target.bytes())
        .unwrap()
        .is_none());
    let block = models::rust::block_implicits::get_random_block(
        Some(0),
        Some(0),
        None,
        Some(target.to_bytes_prost()),
        None,
        Some(0),
        Some(0),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some("root".to_owned()),
        None,
    );
    let approved = ApprovedBlock {
        candidate: ApprovedBlockCandidate {
            block,
            required_sigs: 0,
        },
        floor_seed: None,
        sigs: Vec::new(),
    };
    let importer = Arc::new(RSpaceImporterStore::create(
        destination.history.clone(),
        destination.cold.clone(),
        destination.roots.clone(),
    ));
    let (_sender, receiver) = mpsc::channel(1);
    let queued = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (stream, error) = crate::rust::engine::lfs_tuple_space_requester::stream(
        &approved,
        receiver,
        queued.clone(),
        Duration::from_secs(1),
        UnpolledStartupOps,
        importer,
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    let after_construction = destination.snapshot();
    let observed_current = roots.current_root().unwrap();
    let observed_tag = roots.contains_root(&target).unwrap();
    drop(stream);
    assert!(error.lock().unwrap().is_none());
    assert_eq!(queued.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(after_construction.history, before.history);
    assert_eq!(after_construction.cold, before.cold);
    assert!(destination
        .history
        .get_one(&target.bytes())
        .unwrap()
        .is_none());
    assert_eq!(
        destination.snapshot(),
        after_construction,
        "dropping an unpolled stream does not undo its constructor writes"
    );
    assert_eq!(
        (observed_current, observed_tag), (previous_root, false),
        "startup construction must preserve current-root and must not mark unreceived state as available",
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        max_shrink_iters: 256,
        ..ProptestConfig::default()
    })]

    #[test]
    fn read_tape_native_shared_occurrences_preserve_callback_order(
        slots in prop::collection::btree_set(any::<u8>(), 2..17),
        prefix in prop::collection::vec(any::<u8>(), 0..8),
        leaf_slot in any::<u8>(),
        leaf_key in prop::array::uniform32(any::<u8>()),
        budget in 1usize..20,
    ) {
        let mut child = empty_node();
        child[usize::from(leaf_slot)] = Item::Leaf { prefix: Vec::new(), value: leaf_key.to_vec() };
        let (child_key, child_bytes) = hash_node(&child);
        let mut root = empty_node();
        for slot in &slots {
            root[usize::from(*slot)] = Item::NodePtr { prefix: prefix.clone(), ptr: child_key.clone() };
        }
        let (root_key, root_bytes) = hash_node(&root);
        let stored = BTreeMap::from([(root_key.clone(), root_bytes), (child_key.clone(), child_bytes)]);
        let reads = Arc::new(std::sync::Mutex::new(Vec::new()));
        let callback_reads = reads.clone();
        let callback = Arc::new(move |key: &Vec<u8>| {
            callback_reads.lock().unwrap().push(key.clone());
            stored.get(key).cloned()
        });
        let (page, next) = sequential_export(
            root_key.clone(), None, 0, budget as i32, callback.clone(), read_tape_export_settings(),
        ).unwrap();
        let visited = slots.len().min(budget - 1);
        let mut expected_reads = vec![root_key.clone(), root_key.clone()];
        expected_reads.extend(std::iter::repeat_n(child_key.clone(), visited));
        prop_assert_eq!(&*reads.lock().unwrap(), &expected_reads);
        let mut expected_keys = vec![root_key.clone()];
        expected_keys.extend(std::iter::repeat_n(child_key.clone(), visited));
        prop_assert_eq!(&page.node_keys, &expected_keys);
        let mut expected_prefixes = vec![Vec::new()];
        expected_prefixes.extend(slots.iter().take(visited).map(|slot| {
            let mut path = vec![*slot];
            path.extend(&prefix);
            path
        }));
        prop_assert_eq!(&page.node_prefixes, &expected_prefixes);
        if budget <= slots.len() + 1 {
            let path = next.expect("budget stops at the last visited history occurrence");
            prop_assert_eq!(&path, expected_prefixes.last().unwrap());
            reads.lock().unwrap().clear();
            let (resumed, terminal) = sequential_export(
                root_key.clone(), Some(path), 0, 32, callback, read_tape_export_settings(),
            ).unwrap();
            let mut expected_resume_reads = vec![root_key.clone(), root_key];
            let carrier_reads = usize::from(visited > 0);
            expected_resume_reads.extend(std::iter::repeat_n(child_key.clone(), carrier_reads + slots.len() - visited));
            prop_assert_eq!(&*reads.lock().unwrap(), &expected_resume_reads);
            prop_assert_eq!(resumed.node_keys, vec![child_key; slots.len() - visited]);
            prop_assert_eq!(terminal, None);
        } else {
            prop_assert_eq!(next, None);
        }
    }

    #[test]
    fn read_tape_native_annotations_preserve_reads_budgets_and_failures(
        slots in prop::collection::btree_set(any::<u8>(), 2..17),
        prefix in prop::collection::vec(any::<u8>(), 0..8),
        leaf_slot in any::<u8>(),
        leaf_key in prop::array::uniform32(any::<u8>()),
        skip in 0usize..20,
        budget in prop_oneof![1usize..20, Just(750usize), Just(1024usize)],
        failed_callback in prop::option::of(0usize..20),
    ) {
        let mut child = empty_node();
        child[usize::from(leaf_slot)] = Item::Leaf { prefix: Vec::new(), value: leaf_key.to_vec() };
        let (child_key, child_bytes) = hash_node(&child);
        let mut root = empty_node();
        for slot in slots {
            root[usize::from(slot)] = Item::NodePtr { prefix: prefix.clone(), ptr: child_key.clone() };
        }
        let (root_key, root_bytes) = hash_node(&root);
        let stored = Arc::new(BTreeMap::from([(root_key.clone(), root_bytes), (child_key, child_bytes)]));
        let export = |annotated| {
            let reads = Arc::new(std::sync::Mutex::new(Vec::new()));
            let callback_reads = reads.clone();
            let stored = stored.clone();
            let callback = Arc::new(move |key: &Vec<u8>| {
                let mut reads = callback_reads.lock().unwrap();
                let fail = failed_callback == Some(reads.len());
                reads.push(key.clone());
                if fail { None } else { stored.get(key).cloned() }
            });
            let mut settings = read_tape_export_settings();
            settings.flag_node_prefixes = annotated;
            settings.flag_leaf_prefixes = annotated;
            let result = sequential_export(root_key.clone(), None, skip as i32, budget as i32, callback, settings);
            let trace = reads.lock().unwrap().clone();
            (result, trace)
        };
        let (annotated, annotated_reads) = export(true);
        let (plain, plain_reads) = export(false);
        prop_assert_eq!(annotated_reads, plain_reads);
        match (annotated, plain) {
            (Ok((annotated, annotated_next)), Ok((plain, plain_next))) => {
                prop_assert_eq!(annotated.node_prefixes.len(), annotated.node_keys.len());
                prop_assert_eq!(annotated.leaf_prefixes.len(), annotated.leaf_values.len());
                prop_assert!(plain.node_prefixes.is_empty());
                prop_assert!(plain.leaf_prefixes.is_empty());
                prop_assert_eq!(annotated.node_keys, plain.node_keys);
                prop_assert_eq!(annotated.node_values, plain.node_values);
                prop_assert_eq!(annotated.leaf_values, plain.leaf_values);
                prop_assert_eq!(annotated_next, plain_next);
            }
            (Err(annotated), Err(plain)) => prop_assert_eq!(annotated.to_string(), plain.to_string()),
            _ => prop_assert!(false, "occurrence annotations must not change success or failure"),
        }
    }

    #[test]
    fn read_tape_native_shared_leaf_hash_retains_every_expected_kind_path(
        prefix in prop::collection::vec(any::<u8>(), 0..8),
        leaf_prefix in prop::collection::vec(any::<u8>(), 0..8),
        leaf_slot in any::<u8>(),
        leaf_key in prop::array::uniform32(any::<u8>()),
        budget in prop_oneof![Just(5i32), Just(750i32), Just(1024i32)],
    ) {
        let mut child = empty_node();
        child[usize::from(leaf_slot)] = Item::Leaf { prefix: leaf_prefix.clone(), value: leaf_key.to_vec() };
        let (child_key, child_bytes) = hash_node(&child);
        let mut root = empty_node();
        for slot in [0, 1, 2] {
            root[slot] = Item::NodePtr { prefix: prefix.clone(), ptr: child_key.clone() };
        }
        let (root_key, root_bytes) = hash_node(&root);
        let stored = BTreeMap::from([(root_key.clone(), root_bytes), (child_key.clone(), child_bytes)]);
        let callback = Arc::new(move |key: &Vec<u8>| stored.get(key).cloned());
        let (page, terminal) = sequential_export(root_key.clone(), None, 0, budget, callback, read_tape_export_settings()).unwrap();
        prop_assert_eq!(terminal, None);
        prop_assert_eq!(page.node_keys, vec![root_key, child_key.clone(), child_key.clone(), child_key]);
        prop_assert_eq!(page.leaf_values, vec![leaf_key.to_vec(); 3]);
        let expected_paths: Vec<Vec<u8>> = [0u8, 1, 2].into_iter().map(|kind| {
            let mut path = vec![kind];
            path.extend(&prefix);
            path.push(leaf_slot);
            path.extend(&leaf_prefix);
            path
        }).collect();
        prop_assert_eq!(page.leaf_prefixes, expected_paths);
    }

    #[test]
    fn read_tape_native_required_failure_stops_before_later_callbacks(
        slots in prop::collection::btree_set(any::<u8>(), 2..17),
        fail_at in 0usize..20,
    ) {
        let (child_key, child_bytes) = hash_node(&empty_node());
        let mut root = empty_node();
        for slot in &slots {
            root[usize::from(*slot)] = Item::NodePtr { prefix: Vec::new(), ptr: child_key.clone() };
        }
        let (root_key, root_bytes) = hash_node(&root);
        let stored = BTreeMap::from([(root_key.clone(), root_bytes), (child_key.clone(), child_bytes)]);
        let mut expected = vec![root_key.clone(), root_key.clone()];
        expected.extend(std::iter::repeat_n(child_key, slots.len()));
        let fail_at = fail_at % expected.len();
        let reads = Arc::new(std::sync::Mutex::new(Vec::new()));
        let callback_reads = reads.clone();
        let callback = Arc::new(move |key: &Vec<u8>| {
            let mut reads = callback_reads.lock().unwrap();
            let fail = reads.len() == fail_at;
            reads.push(key.clone());
            if fail { None } else { stored.get(key).cloned() }
        });
        let result = sequential_export(root_key, None, 0, 32, callback, read_tape_export_settings());
        prop_assert_eq!(&*reads.lock().unwrap(), &expected[..=fail_at]);
        if fail_at == 0 {
            let (page, next) = result.expect("initial unavailability has a distinct exporter response");
            prop_assert!(page.node_keys.is_empty());
            prop_assert!(page.leaf_values.is_empty());
            prop_assert_eq!(next, None);
        } else {
            prop_assert!(result.is_err(), "a required stack or traversal read cannot become successful completion");
        }
    }
}

fn acquire(core: &mut Core, root: &Blake2b256Hash, now: Instant) -> StatePartPath {
    let dispatch = core
        .on_fetch(root.clone(), Bytes::from(vec![7; 32]), now)
        .expect("fresh root dispatch");
    let path = dispatch.path.clone();
    core.finish_dispatch(dispatch, now, false);
    assert_eq!(core.recovery.attempts(root), Some(1));
    path
}

fn assert_rejected_without_effects(
    core: &mut Core,
    stores: &Stores,
    before: &StoreSnapshot,
    root: &Blake2b256Hash,
    path: &StatePartPath,
    now: Instant,
) {
    let after = stores.snapshot();
    let mut violations = Vec::new();
    if after.history != before.history {
        violations.push("history changed");
    }
    if after.cold != before.cold {
        violations.push("cold data changed");
    }
    if after.roots != before.roots {
        violations.push("root markers or current-root changed");
    }
    match core.pending.get(root) {
        Some(pending) => {
            if pending.owners != HashSet::from([Bytes::from(vec![7; 32])]) {
                violations.push("retry owner changed");
            }
            if pending.outstanding != HashSet::from([path.clone()]) {
                violations.push("outstanding path changed");
            }
            if pending.chunks_imported != 0 {
                violations.push("unvalidated page counted as imported");
            }
        }
        None => violations.push("pending root retired"),
    }
    if core.path_to_root != HashMap::from([(path.clone(), root.clone())]) {
        violations.push("path-to-root ownership changed");
    }
    if !core.recovery.contains(root) {
        violations.push("recovery obligation retired");
    }
    if core.recovery.attempts(root) == Some(0) {
        violations.push("invalid page reset retry progress");
    }
    assert!(
        violations.is_empty(),
        "invalid page effects: {violations:?}"
    );
    let retry_time = now + MAX_RETRY_DELAY + RESEND_INTERVAL;
    let mut requests = core.on_tick(retry_time);
    assert_eq!(requests.len(), 1, "rejected page remains retryable");
    let retry = requests.pop().unwrap();
    assert_eq!(retry.path, *path);
    assert_eq!(retry.dispatch.key(), root);
    core.finish_dispatch(retry, retry_time, false);
}

#[tokio::test]
async fn valid_real_exporter_page_imports_nonempty_state() {
    let (root, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let mut core = destination.core();
    let now = Instant::now();
    let path = acquire(&mut core, &root, now);
    assert_eq!(page.start_path, path);
    assert_eq!(page.last_path, path, "fixture is a canonical terminal page");
    validate_page(&core, &page).expect("existing validator accepts the canonical exported page");
    assert!(core.on_items(page.clone(), now).is_none());
    for (key, bytes) in page.history_items {
        assert_eq!(
            destination.history.get_one(&key.bytes()).unwrap(),
            Some(bytes.to_vec())
        );
    }
    for (key, bytes) in page.data_items {
        assert_eq!(
            destination.cold.get_one(&key.bytes()).unwrap(),
            Some(bytes.to_vec())
        );
    }
    assert!((core.has_root)(&root).unwrap());
    assert!(!core.pending.contains_key(&root));
    assert!(!core.recovery.contains(&root));
    assert_eq!(destination.read_joins(&root), vec![vec![
        "channel".to_owned()
    ]]);
}

#[tokio::test]
async fn unknown_response_path_preserves_real_stores_and_retry_owner() {
    let (root, mut page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let mut core = destination.core();
    let now = Instant::now();
    let path = acquire(&mut core, &root, now);
    let before = destination.snapshot();
    page.start_path = vec![(Blake2b256Hash::from_bytes(vec![91; 32]), None)];
    assert!(core
        .on_items(page, now + Duration::from_millis(1))
        .is_none());
    assert_rejected_without_effects(&mut core, &destination, &before, &root, &path, now);
    assert_eq!(core.pending[&root].last_progress, now);
}

#[tokio::test]
async fn mismatched_history_hash_preserves_real_stores_and_retry_owner() {
    let (root, mut page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let original = page.history_items[0].1.to_vec();
    destination.history.put_one(root.bytes(), original).unwrap();
    let mut core = destination.core();
    let now = Instant::now();
    let path = acquire(&mut core, &root, now);
    let before = destination.snapshot();
    page.history_items[0].1 = Bytes::from_static(b"invalid-history");
    assert_ne!(
        page.history_items[0].0,
        Blake2b256Hash::new(&page.history_items[0].1)
    );
    drop(core.on_items(page, now + Duration::from_millis(1)));
    assert_rejected_without_effects(&mut core, &destination, &before, &root, &path, now);
}

#[tokio::test]
async fn corrupt_cold_data_preserves_real_stores_and_retry_owner() {
    let (root, mut page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    destination
        .cold
        .put_one(page.data_items[0].0.bytes(), page.data_items[0].1.to_vec())
        .unwrap();
    let mut core = destination.core();
    let now = Instant::now();
    let path = acquire(&mut core, &root, now);
    let before = destination.snapshot();
    page.data_items[0].1 = Bytes::from_static(b"invalid-cold-data");
    drop(core.on_items(page, now + Duration::from_millis(1)));
    assert_rejected_without_effects(&mut core, &destination, &before, &root, &path, now);
}

#[tokio::test]
async fn canonical_validator_rejects_wrong_kind_with_unchanged_leaf_hash() {
    let (_, mut page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let core = destination.core();
    let before = destination.snapshot();
    let PersistedData::Joins(leaf) = bincode::deserialize(&page.data_items[0].1).unwrap() else {
        panic!("fixture must contain a joins leaf");
    };
    let wrong_kind = DataLeaf { bytes: leaf.bytes };
    assert_eq!(
        page.data_items[0].0,
        Blake2b256Hash::new(&bincode::serialize(&wrong_kind).unwrap()),
        "the existing leaf hash does not include the outer kind"
    );
    page.data_items[0].1 =
        Bytes::from(bincode::serialize(&PersistedData::Data(wrong_kind)).unwrap());
    let validation = validate_page(&core, &page);
    assert_eq!(destination.snapshot(), before);
    assert!(
        validation.is_err(),
        "a valid leaf hash cannot authorize Data at a Joins trie occurrence"
    );
}

#[tokio::test]
async fn canonical_validator_rejects_hash_valid_malformed_inner_collection() {
    let invalid_inner = vec![1, 0, 0, 0, 0, 0, 0, 0];
    assert!(bincode::deserialize::<Vec<Vec<u8>>>(&invalid_inner).is_err());
    let (_, _, page) = exported_joins_trie(JoinsLeaf {
        bytes: invalid_inner,
    })
    .await;
    let destination = Stores::new().await;
    let core = destination.core();
    let before = destination.snapshot();
    let PersistedData::Joins(leaf) = bincode::deserialize(&page.data_items[0].1).unwrap() else {
        panic!("fixture must contain a joins leaf");
    };
    assert_eq!(
        page.data_items[0].0,
        Blake2b256Hash::new(&bincode::serialize(&leaf).unwrap())
    );
    let validation = validate_page(&core, &page);
    assert_eq!(destination.snapshot(), before);
    assert!(
        validation.is_err(),
        "matching hashes and leaf kinds cannot authorize an undecodable inner collection"
    );
}

#[tokio::test]
async fn canonical_validator_rejects_hash_valid_malformed_nested_join() {
    let invalid_join = vec![1, 0, 0, 0, 0, 0, 0, 0];
    assert!(bincode::deserialize::<Vec<String>>(&invalid_join).is_err());
    let collection = bincode::serialize(&vec![invalid_join.clone()]).unwrap();
    assert_eq!(
        bincode::deserialize::<Vec<Vec<u8>>>(&collection).unwrap(),
        vec![invalid_join]
    );
    let (_, _, page) = exported_joins_trie(JoinsLeaf { bytes: collection }).await;
    let destination = Stores::new().await;
    let core = destination.core();
    let before = destination.snapshot();
    let validation = validate_page(&core, &page);
    assert_eq!(destination.snapshot(), before);
    assert!(
        validation.is_err(),
        "valid outer framing cannot authorize a malformed nested join"
    );
}

#[tokio::test]
async fn valid_cold_payload_retains_existing_trailing_byte_acceptance() {
    let expected = vec![vec!["channel".to_owned()]];
    let mut encoded_join = bincode::serialize(&expected[0]).unwrap();
    encoded_join.extend([91, 92]);
    let mut encoded_collection = bincode::serialize(&vec![encoded_join]).unwrap();
    encoded_collection.extend([93, 94]);
    let (source, root, mut page) = exported_joins_trie(JoinsLeaf {
        bytes: encoded_collection,
    })
    .await;
    assert_eq!(source.read_joins(&root), expected);
    let mut envelope = page.data_items[0].1.to_vec();
    envelope.extend([95, 96]);
    page.data_items[0].1 = Bytes::from(envelope);
    let destination = Stores::new().await;
    let mut core = destination.core();
    validate_page(&core, &page)
        .expect("the existing consumer accepts trailing bytes at all layers");
    let now = Instant::now();
    acquire(&mut core, &root, now);
    assert!(core.on_items(page, now).is_none());
    assert_eq!(destination.read_joins(&root), expected);
}

#[tokio::test]
async fn received_cold_rows_accept_compatible_duplicate_envelopes_in_either_order() {
    let (_, page) = exported_nonempty_trie().await;
    let (key, original) = page.data_items[0].clone();
    let mut extended = original.to_vec();
    extended.extend([71, 72]);
    let PersistedData::Joins(original_leaf) = bincode::deserialize(&original).unwrap() else {
        panic!("fixture must contain a joins leaf");
    };
    let PersistedData::Joins(extended_leaf) = bincode::deserialize(&extended).unwrap() else {
        panic!("a trailer must not change the joins kind");
    };
    assert_eq!(original_leaf.bytes, extended_leaf.bytes);
    for reverse in [false, true] {
        let destination = Stores::new().await;
        let core = destination.core();
        let before = destination.snapshot();
        let mut candidate = page.clone();
        candidate
            .data_items
            .push((key.clone(), Bytes::from(extended.clone())));
        if reverse {
            candidate.data_items.reverse();
        }
        validate_page(&core, &candidate).expect("compatible repeated envelopes remain valid");
        assert_eq!(destination.snapshot(), before);
    }
}

#[tokio::test]
async fn received_cold_rows_retain_first_compatible_encoding_during_import() {
    let (root, page) = exported_nonempty_trie().await;
    let (key, original) = page.data_items[0].clone();
    let mut extended = original.to_vec();
    extended.extend([71, 72]);
    for reverse in [false, true] {
        let destination = Stores::new().await;
        let mut core = destination.core();
        let mut candidate = page.clone();
        candidate
            .data_items
            .push((key.clone(), Bytes::from(extended.clone())));
        if reverse {
            candidate.data_items.reverse();
        }
        let expected = candidate.data_items[0].1.to_vec();
        let now = Instant::now();
        acquire(&mut core, &root, now);
        assert!(core.on_items(candidate, now).is_none());
        assert_eq!(destination.read_joins(&root), vec![vec![
            "channel".to_owned()
        ]]);
        assert_eq!(
            destination.cold.get_one(&key.bytes()).unwrap(),
            Some(expected),
            "compatible duplicate rows must not replace the first accepted encoding"
        );
    }
}

#[tokio::test]
async fn received_cold_rows_reject_malformed_duplicates_in_either_order() {
    let (_, page) = exported_nonempty_trie().await;
    let key = page.data_items[0].0.clone();
    for reverse in [false, true] {
        let destination = Stores::new().await;
        let core = destination.core();
        let before = destination.snapshot();
        let mut candidate = page.clone();
        candidate
            .data_items
            .push((key.clone(), Bytes::from_static(b"bad")));
        if reverse {
            candidate.data_items.reverse();
        }
        assert!(validate_page(&core, &candidate).is_err());
        assert_eq!(destination.snapshot(), before);
    }
}

#[tokio::test]
async fn received_cold_rows_reject_wrong_kind_duplicates_in_either_order() {
    let (_, _, page) = exported_joins_trie(JoinsLeaf {
        bytes: bincode::serialize(&Vec::<Vec<u8>>::new()).unwrap(),
    })
    .await;
    let (key, original) = page.data_items[0].clone();
    let PersistedData::Joins(leaf) = bincode::deserialize(&original).unwrap() else {
        panic!("fixture must contain a joins leaf");
    };
    let wrong_kind = DataLeaf { bytes: leaf.bytes };
    assert_eq!(
        key,
        Blake2b256Hash::new(&bincode::serialize(&wrong_kind).unwrap())
    );
    let wrong = Bytes::from(bincode::serialize(&PersistedData::Data(wrong_kind)).unwrap());
    for reverse in [false, true] {
        let destination = Stores::new().await;
        let core = destination.core();
        let before = destination.snapshot();
        let mut candidate = page.clone();
        candidate.data_items.push((key.clone(), wrong.clone()));
        if reverse {
            candidate.data_items.reverse();
        }
        assert!(
            validate_page(&core, &candidate).is_err(),
            "a same-hash duplicate must satisfy the actual joins occurrence"
        );
        assert_eq!(destination.snapshot(), before);
    }
}

#[tokio::test]
async fn received_cold_rows_require_exact_response_keys_without_durable_fallback() {
    let (_, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let (key, bytes) = page.data_items[0].clone();
    destination
        .cold
        .put_one(key.bytes(), bytes.to_vec())
        .unwrap();
    destination
        .cold
        .put_one(bincode::serialize(&key).unwrap(), bytes.to_vec())
        .unwrap();
    let core = destination.core();
    let before = destination.snapshot();
    let mut missing = page.clone();
    missing.data_items.clear();
    assert!(
        validate_page(&core, &missing).is_err(),
        "stored bytes cannot replace a missing response row"
    );
    let extra_leaf = JoinsLeaf {
        bytes: encode_joins(&vec![vec!["other".to_owned()]]),
    };
    let extra_key = Blake2b256Hash::new(&bincode::serialize(&extra_leaf).unwrap());
    assert_ne!(extra_key, key);
    let mut extra = page;
    extra.data_items.push((
        extra_key,
        Bytes::from(bincode::serialize(&PersistedData::Joins(extra_leaf)).unwrap()),
    ));
    assert!(
        validate_page(&core, &extra).is_err(),
        "unrequested rows cannot become imported state"
    );
    assert_eq!(destination.snapshot(), before);
}

#[tokio::test]
async fn received_cold_rows_reject_non_hash_width_keys_without_panicking() {
    let (_, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let core = destination.core();
    let before = destination.snapshot();
    for width in (0..=65).filter(|width| *width != 32) {
        let mut candidate = page.clone();
        candidate.data_items[0].0 = Blake2b256Hash::from_bytes(vec![0; width]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            validate_page(&core, &candidate)
        }));
        assert!(
            matches!(result, Ok(Err(_))),
            "invalid key width {width} must return an error"
        );
        assert_eq!(destination.snapshot(), before);
    }
}

#[tokio::test]
async fn physical_cold_observation_rejects_required_read_failure_in_either_alias() {
    let (_, page) = exported_nonempty_trie().await;
    let (hash, bytes) = page.data_items[0].clone();
    let keys = [hash.bytes(), bincode::serialize(&hash).unwrap()];
    let mut outcomes = Vec::new();
    for key in &keys {
        let destination = Stores::new().await;
        for stored_key in &keys {
            destination
                .cold
                .put_one(stored_key.clone(), bytes.to_vec())
                .unwrap();
        }
        let before = destination.snapshot();
        let failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let reads = Arc::new(std::sync::Mutex::new(Vec::new()));
        let cold = FaultingKeyStore {
            fault: KeyStoreFault::Read,
            inner: destination.cold.clone(),
            key: key.clone(),
            failures: failures.clone(),
            reads: reads.clone(),
        };
        assert!(matches!(cold.get_one(key), Err(KvStoreError::IoError(_))));
        failures.store(0, std::sync::atomic::Ordering::SeqCst);
        reads.lock().unwrap().clear();
        let mut core = destination.core();
        core.importer = Arc::new(RSpaceImporterStore::create(
            destination.history.clone(),
            Arc::new(cold),
            destination.roots.clone(),
        ));
        let validation =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| validate_page(&core, &page)));
        assert_eq!(destination.snapshot(), before);
        outcomes.push((
            matches!(validation, Ok(Err(_))),
            failures.load(std::sync::atomic::Ordering::SeqCst) > 0,
        ));
    }
    assert_eq!(
        outcomes,
        vec![(true, true); 2],
        "both physical aliases require fallible compatibility reads"
    );
}

#[tokio::test]
async fn physical_cold_observation_consumer_does_not_fallback_after_raw_read_failure() {
    let (root, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let (leaf_hash, bytes) = page.data_items[0].clone();
    let raw = leaf_hash.bytes();
    let legacy = bincode::serialize(&leaf_hash).unwrap();
    destination.cold.put_one(legacy, bytes.to_vec()).unwrap();
    destination
        .history
        .put_one(root.bytes(), page.history_items[0].1.to_vec())
        .unwrap();
    let before = destination.snapshot();
    let failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let reads = Arc::new(std::sync::Mutex::new(Vec::new()));
    let cold = Arc::new(FaultingKeyStore {
        fault: KeyStoreFault::Read,
        inner: destination.cold.clone(),
        key: raw.clone(),
        failures: failures.clone(),
        reads: reads.clone(),
    });
    let history = RadixHistory::create(root.clone(), destination.history.clone()).unwrap();
    let reader =
        RSpaceHistoryReaderImpl::<String, String, String, String>::new(Box::new(history), cold);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reader.get_joins(&hash(&"channel".to_owned()))
    }));
    assert!(matches!(result, Ok(Err(_))));
    assert_eq!(*reads.lock().unwrap(), vec![raw]);
    assert_eq!(failures.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(destination.snapshot(), before);
}

#[tokio::test]
async fn missing_referenced_cold_data_cannot_publish_root() {
    let (root, mut page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let mut core = destination.core();
    let now = Instant::now();
    let path = acquire(&mut core, &root, now);
    let before = destination.snapshot();
    page.data_items.clear();
    assert!(!page.history_items.is_empty());
    drop(core.on_items(page, now + Duration::from_millis(1)));
    assert_rejected_without_effects(&mut core, &destination, &before, &root, &path, now);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        max_shrink_iters: 256,
        ..ProptestConfig::default()
    })]

    #[test]
    fn root_publication_native_errors_preserve_exact_write_prefixes(
        seed in any::<[u8; 32]>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            for pointer_failure in [false, true] {
                for after_write in [false, true] {
                    let stores = Stores::new().await;
                    let roots = RootsStoreInstances::roots_store(stores.roots.clone());
                    let previous = roots.current_root().unwrap().unwrap();
                    let mut target_bytes = seed.to_vec();
                    if target_bytes == previous.bytes() {
                        target_bytes[0] ^= 1;
                    }
                    let target = Blake2b256Hash::from_bytes(target_bytes.clone());
                    let before = stores.snapshot();
                    let failure_key = if pointer_failure {
                        b"current-root".to_vec()
                    } else {
                        target_bytes.clone()
                    };
                    let failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                    let wrapped = FaultingKeyStore {
                        inner: stores.roots.clone(),
                        key: failure_key,
                        fault: if after_write { KeyStoreFault::AfterWrite } else { KeyStoreFault::BeforeWrite },
                        failures: failures.clone(),
                        reads: Arc::new(std::sync::Mutex::new(Vec::new())),
                    };
                    let failing_roots = RootsStoreInstances::roots_store(Arc::new(wrapped));
                    assert!(failing_roots.record_root(&target).is_err());
                    assert_eq!(failures.load(std::sync::atomic::Ordering::SeqCst), 1);
                    let mut expected = before.roots.clone();
                    if pointer_failure || after_write {
                        expected.insert(target_bytes.clone(), b"tag".to_vec());
                    }
                    if pointer_failure && after_write {
                        expected.insert(b"current-root".to_vec(), target_bytes);
                    }
                    let after = stores.snapshot();
                    assert_eq!(after.roots, expected);
                    assert_eq!(after.history, before.history);
                    assert_eq!(after.cold, before.cold);
                    assert!(roots.contains_root(&previous).unwrap());
                    assert_eq!(roots.contains_root(&target).unwrap(), pointer_failure || after_write);
                    assert_eq!(roots.current_root().unwrap(), Some(if pointer_failure && after_write {
                        target.clone()
                    } else {
                        previous
                    }));
                    roots.record_root(&target).unwrap();
                    assert!(roots.contains_root(&target).unwrap());
                    assert_eq!(roots.current_root().unwrap(), Some(target));
                }
            }
        });
    }
}

#[tokio::test]
async fn forged_continuation_preserves_real_stores_and_retry_owner() {
    let (root, mut page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let mut core = destination.core();
    let now = Instant::now();
    let path = acquire(&mut core, &root, now);
    assert_eq!(page.last_path, path, "exporter supplied a terminal cursor");
    let before = destination.snapshot();
    page.last_path = vec![(Blake2b256Hash::from_bytes(vec![92; 32]), None)];
    drop(core.on_items(page, now + Duration::from_millis(1)));
    assert_rejected_without_effects(&mut core, &destination, &before, &root, &path, now);
}

#[tokio::test]
async fn imported_nonempty_state_can_be_reexported() {
    let (root, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let mut core = destination.core();
    let now = Instant::now();
    acquire(&mut core, &root, now);
    assert!(core.on_items(page.clone(), now).is_none());
    assert_eq!(destination.read_joins(&root), vec![vec![
        "channel".to_owned()
    ]]);
    let exported = destination.export(&root);
    assert_eq!(exported.history_items, page.history_items);
    assert_eq!(
        exported.data_items, page.data_items,
        "a later peer must receive the imported cold data"
    );
    assert_eq!(exported.last_path, page.last_path);
}

#[tokio::test]
async fn importing_an_equivalent_envelope_preserves_existing_encoded_bytes() {
    let (root, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let key = page.data_items[0].0.bytes();
    let mut existing = page.data_items[0].1.to_vec();
    existing.extend([91, 92, 93]);
    destination
        .cold
        .put_one(key.clone(), existing.clone())
        .unwrap();
    destination
        .history
        .put_one(root.bytes(), page.history_items[0].1.to_vec())
        .unwrap();
    let before = destination.read_joins(&root);
    let mut core = destination.core();
    let now = Instant::now();
    acquire(&mut core, &root, now);
    assert!(core.on_items(page, now).is_none());
    assert_eq!(destination.read_joins(&root), before);
    assert_eq!(
        destination.cold.get_one(&key).unwrap(),
        Some(existing),
        "a compatible envelope must not replace existing encoded bytes"
    );
}

#[tokio::test]
async fn malformed_present_raw_alias_cannot_be_treated_as_absent_during_import() {
    let (root, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let key = page.data_items[0].0.bytes();
    destination
        .cold
        .put_one(key.clone(), b"bad".to_vec())
        .unwrap();
    destination
        .cold
        .put_one(
            bincode::serialize(&key).unwrap(),
            page.data_items[0].1.to_vec(),
        )
        .unwrap();
    destination
        .history
        .put_one(root.bytes(), page.history_items[0].1.to_vec())
        .unwrap();
    let history = RadixHistory::create(root.clone(), destination.history.clone()).unwrap();
    let reader = RSpaceHistoryReaderImpl::<String, String, String, String>::new(
        Box::new(history),
        destination.cold.clone(),
    );
    assert!(
        reader.get_joins(&hash(&"channel".to_owned())).is_err(),
        "malformed raw bytes do not permit fallback to the valid legacy alias"
    );
    let mut core = destination.core();
    let now = Instant::now();
    let path = acquire(&mut core, &root, now);
    let before = destination.snapshot();
    drop(core.on_items(page, now));
    assert_rejected_without_effects(&mut core, &destination, &before, &root, &path, now);
}

#[tokio::test]
async fn absent_raw_alias_can_be_added_without_changing_compatible_legacy_bytes() {
    let (root, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let key = page.data_items[0].0.bytes();
    let legacy_key = bincode::serialize(&key).unwrap();
    let canonical = page.data_items[0].1.to_vec();
    let mut legacy = canonical.clone();
    legacy.extend([94, 95, 96]);
    destination
        .cold
        .put_one(legacy_key.clone(), legacy.clone())
        .unwrap();
    destination
        .history
        .put_one(root.bytes(), page.history_items[0].1.to_vec())
        .unwrap();
    let before = destination.read_joins(&root);
    let mut core = destination.core();
    let now = Instant::now();
    acquire(&mut core, &root, now);
    assert!(core.on_items(page, now).is_none());
    assert_eq!(destination.read_joins(&root), before);
    assert_eq!(destination.cold.get_one(&key).unwrap(), Some(canonical));
    assert_eq!(destination.cold.get_one(&legacy_key).unwrap(), Some(legacy));
}

#[tokio::test]
async fn existing_raw_first_reader_does_not_inspect_a_malformed_legacy_alias() {
    let (root, page) = exported_nonempty_trie().await;
    let destination = Stores::new().await;
    let key = page.data_items[0].0.bytes();
    destination
        .cold
        .put_one(key.clone(), page.data_items[0].1.to_vec())
        .unwrap();
    destination
        .cold
        .put_one(bincode::serialize(&key).unwrap(), b"bad".to_vec())
        .unwrap();
    destination
        .history
        .put_one(root.bytes(), page.history_items[0].1.to_vec())
        .unwrap();
    assert_eq!(destination.read_joins(&root), vec![vec![
        "channel".to_owned()
    ]]);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        max_shrink_iters: 256,
        ..ProptestConfig::default()
    })]

    #[test]
    fn received_cold_rows_generated_permutations_preserve_typed_values(
        trailers in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..17), 1..9),
        rotation in any::<usize>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let (root, page) = exported_nonempty_trie().await;
            let (key, bytes) = page.data_items[0].clone();
            let mut received = trailers.into_iter().map(|trailer| {
                let mut encoded = bytes.to_vec();
                encoded.extend(trailer);
                (key.clone(), Bytes::from(encoded))
            }).collect::<Vec<_>>();
            let offset = rotation % received.len();
            received.rotate_left(offset);
            for reverse in [false, true] {
                let destination = Stores::new().await;
                let mut core = destination.core();
                let before = destination.snapshot();
                let mut candidate = page.clone();
                candidate.data_items = received.clone();
                if reverse {
                    candidate.data_items.reverse();
                }
                validate_page(&core, &candidate).unwrap();
                assert_eq!(destination.snapshot(), before);
                let now = Instant::now();
                acquire(&mut core, &root, now);
                assert!(core.on_items(candidate, now).is_none());
                assert_eq!(destination.read_joins(&root), vec![vec!["channel".to_owned()]]);
                assert_eq!(destination.cold.to_map().unwrap().len(), 1);
            }
        });
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1_024,
        max_shrink_iters: 4_096,
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_alias_cas_preserves_the_commit_state_and_unrelated_updates(
        raw in proptest::option::of(prop::collection::vec(any::<u8>(), 0..33)),
        legacy in proptest::option::of(prop::collection::vec(any::<u8>(), 0..33)),
        intervening in prop::collection::vec(
            (any::<bool>(), proptest::option::of(prop::collection::vec(any::<u8>(), 0..33))),
            0..9,
        ),
        target_raw in any::<bool>(),
        incoming in prop::collection::vec(any::<u8>(), 0..33),
        unrelated in prop::collection::vec(any::<u8>(), 0..33),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let (actual, observed, expected, matches) = runtime.block_on(async {
            let stores = Stores::new().await;
            let hash = Blake2b256Hash::from_bytes(vec![7; 32]);
            let keys = [hash.bytes(), bincode::serialize(&hash).unwrap()];
            assert_ne!(keys[0], keys[1]);
            let before = [raw, legacy];
            for (key, value) in keys.iter().zip(&before) {
                if let Some(value) = value {
                    stores.cold.put_one(key.clone(), value.clone()).unwrap();
                }
            }
            for (raw_alias, value) in intervening {
                let key = &keys[usize::from(!raw_alias)];
                match value {
                    Some(bytes) => stores.cold.put_one(key.clone(), bytes).unwrap(),
                    None => { stores.cold.delete(vec![key.clone()]).unwrap(); },
                }
            }
            stores.cold.put_one(vec![8; 32], unrelated).unwrap();
            let current = stores.cold.to_map().unwrap();
            let matches = keys.iter().zip(&before)
                .all(|(key, value)| current.get(key) == value.as_ref());
            let target = usize::from(!target_raw);
            let other = 1 - target;
            let replacement = before[target].clone().or(Some(incoming));
            let mutations = [
                AtomicStoreMutation {
                    store: stores.cold.as_ref(),
                    key: keys[target].clone(),
                    operation: AtomicStoreOperation::CompareAndSwap {
                        expected: before[target].clone(),
                        replacement: replacement.clone(),
                    },
                },
                AtomicStoreMutation {
                    store: stores.cold.as_ref(),
                    key: keys[other].clone(),
                    operation: AtomicStoreOperation::CompareAndSwap {
                        expected: before[other].clone(),
                        replacement: before[other].clone(),
                    },
                },
            ];
            let actual = strict_atomic_mutate(&mutations);
            let observed = stores.cold.to_map().unwrap();
            let mut expected = current;
            if matches {
                expected.insert(keys[target].clone(), replacement.unwrap());
            }
            (actual, observed, expected, matches)
        });
        prop_assert_eq!(actual.is_ok(), matches);
        if !matches {
            prop_assert!(matches!(actual, Err(KvStoreError::TransactionConflict(_))));
        }
        prop_assert_eq!(observed, expected);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        max_shrink_iters: 32,
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_trailing_bytes_preserve_the_actual_typed_reader(
        channels in prop::collection::vec("[a-z]{1,12}", 1..5),
        item_tail in prop::collection::vec(any::<u8>(), 0..17),
        collection_tail in prop::collection::vec(any::<u8>(), 0..17),
        envelope_tail in prop::collection::vec(any::<u8>(), 0..17),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let (accepted, source_values, imported_values) = runtime.block_on(async {
            let mut item = bincode::serialize(&channels).unwrap();
            item.extend(item_tail);
            let mut payload = bincode::serialize(&vec![item]).unwrap();
            payload.extend(collection_tail);
            let (source, root, mut page) = exported_joins_trie(JoinsLeaf { bytes: payload }).await;
            let mut envelope = page.data_items[0].1.to_vec();
            envelope.extend(envelope_tail);
            page.data_items[0].1 = Bytes::from(envelope);
            let destination = Stores::new().await;
            let mut core = destination.core();
            let accepted = validate_page(&core, &page);
            let now = Instant::now();
            acquire(&mut core, &root, now);
            assert!(core.on_items(page, now).is_none());
            (accepted, source.read_joins(&root), destination.read_joins(&root))
        });
        prop_assert!(accepted.is_ok());
        prop_assert_eq!(&source_values, &vec![channels]);
        prop_assert_eq!(imported_values, source_values);
    }

    #[test]
    fn generated_hash_valid_truncated_nested_items_must_be_rejected(
        present in prop::collection::vec(b'a'..=b'z', 0..33),
        missing in 1u64..33,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let accepted = runtime.block_on(async {
            let mut item = 1u64.to_le_bytes().to_vec();
            item.extend((present.len() as u64 + missing).to_le_bytes());
            item.extend(present);
            assert!(bincode::deserialize::<Vec<String>>(&item).is_err());
            let payload = bincode::serialize(&vec![item]).unwrap();
            let (_, _, page) = exported_joins_trie(JoinsLeaf { bytes: payload }).await;
            let destination = Stores::new().await;
            validate_page(&destination.core(), &page)
        });
        prop_assert!(accepted.is_err(), "a matching hash cannot authorize an unreadable nested item");
    }

    #[test]
    fn generated_envelope_kind_substitution_must_be_rejected(
        channels in prop::collection::vec("[a-z]{1,12}", 1..5),
        use_continuations in any::<bool>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let accepted = runtime.block_on(async {
            let bytes = encode_joins(&vec![channels]);
            let (_, _, mut page) = exported_joins_trie(JoinsLeaf { bytes: bytes.clone() }).await;
            let substituted = if use_continuations {
                assert_eq!(
                    Blake2b256Hash::new(&bincode::serialize(&ContinuationsLeaf { bytes: bytes.clone() }).unwrap()),
                    page.data_items[0].0,
                );
                PersistedData::Continuations(ContinuationsLeaf { bytes })
            } else {
                assert_eq!(
                    Blake2b256Hash::new(&bincode::serialize(&DataLeaf { bytes: bytes.clone() }).unwrap()),
                    page.data_items[0].0,
                );
                PersistedData::Data(DataLeaf { bytes })
            };
            page.data_items[0].1 = Bytes::from(bincode::serialize(&substituted).unwrap());
            let destination = Stores::new().await;
            validate_page(&destination.core(), &page)
        });
        prop_assert!(accepted.is_err(), "the trie occurrence, not the envelope, determines the expected kind");
    }
}
