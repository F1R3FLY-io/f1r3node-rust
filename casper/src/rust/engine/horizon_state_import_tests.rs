use std::collections::BTreeMap;

use futures::StreamExt;
use proptest::prelude::*;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use rspace_plus_plus::rspace::history::cold_store::{DataLeaf, JoinsLeaf, PersistedData};
use rspace_plus_plus::rspace::history::history::History;
use rspace_plus_plus::rspace::history::history_action::{HistoryAction, InsertAction};
use rspace_plus_plus::rspace::history::history_reader::HistoryReader;
use rspace_plus_plus::rspace::history::history_repository::{
    HistoryRepositoryInstances, PREFIX_DATUM, PREFIX_JOINS, PREFIX_KONT,
};
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
use rspace_plus_plus::rspace::history::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;
use rspace_plus_plus::rspace::history::radix_tree::{
    empty_node, hash_node, sequential_export, ExportData, ExportDataSettings, Item, Node,
    RadixTreeImpl,
};
use rspace_plus_plus::rspace::history::roots_store::{RootsStore, RootsStoreInstances};
use rspace_plus_plus::rspace::serializers::serializers::{
    decode_datums, decode_joins, encode_joins,
};
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::instances::rspace_exporter_store::RSpaceExporterStore;
use rspace_plus_plus::rspace::state::instances::rspace_importer_store::RSpaceImporterStore;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporterInstance;
use shared::rust::store::key_value_store::KeyValueStore;

use super::*;

struct Stores {
    history: Arc<dyn KeyValueStore>,
    cold: Arc<dyn KeyValueStore>,
    roots: Arc<dyn KeyValueStore>,
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    history: BTreeMap<Vec<u8>, Vec<u8>>,
    cold: BTreeMap<Vec<u8>, Vec<u8>>,
    roots: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl Stores {
    async fn pristine() -> Self {
        let mut manager = InMemoryStoreManager::new();
        Self {
            history: manager.store("history".to_owned()).await.unwrap(),
            cold: manager.store("cold".to_owned()).await.unwrap(),
            roots: manager.store("roots".to_owned()).await.unwrap(),
        }
    }

    async fn new() -> Self {
        let stores = Self::pristine().await;
        let (root, encoded) = hash_node(&empty_node());
        stores.history.put_one(root.clone(), encoded).unwrap();
        stores.publish(&Blake2b256Hash::from_bytes(root));
        stores
    }

    fn publish(&self, root: &Blake2b256Hash) {
        RootsStoreInstances::roots_store(self.roots.clone())
            .record_root(root)
            .unwrap();
    }

    fn has_root(&self) -> HasRootFn {
        let roots = RootsStoreInstances::roots_store(self.roots.clone());
        Arc::new(move |root| {
            roots
                .contains_root(root)
                .map_err(|error| CasperError::Other(error.to_string()))
        })
    }

    fn importer(&self) -> Arc<dyn RSpaceImporter> {
        Arc::new(RSpaceImporterStore::create(
            self.history.clone(),
            self.cold.clone(),
            self.roots.clone(),
        ))
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            history: self.history.to_map().unwrap(),
            cold: self.cold.to_map().unwrap(),
            roots: self.roots.to_map().unwrap(),
        }
    }

    fn add_joins(&self, root: &Blake2b256Hash, channels: &[String]) -> Blake2b256Hash {
        let actions = channels
            .iter()
            .map(|channel| {
                let leaf = JoinsLeaf {
                    bytes: encode_joins(&vec![vec![channel.clone()]]),
                };
                let leaf_hash = Blake2b256Hash::new(&bincode::serialize(&leaf).unwrap());
                self.cold
                    .put_one(
                        bincode::serialize(&leaf_hash).unwrap(),
                        bincode::serialize(&PersistedData::Joins(leaf)).unwrap(),
                    )
                    .unwrap();
                let mut key = vec![PREFIX_JOINS];
                key.extend(hash(channel).bytes());
                HistoryAction::Insert(InsertAction {
                    key,
                    hash: leaf_hash,
                })
            })
            .collect();
        let history = RadixHistory::create(root.clone(), self.history.clone()).unwrap();
        let root = history.process(actions).unwrap().root();
        self.publish(&root);
        root
    }

    fn read_joins(&self, root: &Blake2b256Hash, channel: &String) -> Vec<Vec<String>> {
        let history = RadixHistory::create(root.clone(), self.history.clone()).unwrap();
        let reader = RSpaceHistoryReaderImpl::<String, String, String, String>::new(
            Box::new(history),
            self.cold.clone(),
        );
        reader.base().get_joins(channel)
    }

    fn export(&self, path: StatePartPath, page_size: i32) -> StoreItemsMessage {
        let exporter = Arc::new(RSpaceExporterStore::create(
            self.history.clone(),
            self.cold.clone(),
            self.roots.clone(),
        ));
        let (history, data) =
            RSpaceExporterItems::get_history_and_data(exporter, path.clone(), 0, page_size);
        assert_eq!(history.last_path, data.last_path);
        StoreItemsMessage {
            start_path: path,
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

    fn export_with_prefixes(&self, root: &Blake2b256Hash) -> ExportData {
        let (exported, continuation) = self.export_prefix_page(root, None, 0, PAGE_SIZE);
        assert!(continuation.is_none());
        exported
    }

    fn export_prefix_page(
        &self,
        root: &Blake2b256Hash,
        prefix: Option<Vec<u8>>,
        skip: i32,
        take: i32,
    ) -> (ExportData, Option<Vec<u8>>) {
        let store = RadixHistory::create_store(self.history.clone());
        let (exported, continuation) = sequential_export(
            root.bytes(),
            prefix,
            skip,
            take,
            Arc::new(move |key| store.get_one(key).unwrap()),
            ExportDataSettings {
                flag_node_prefixes: true,
                flag_node_keys: true,
                flag_node_values: false,
                flag_leaf_prefixes: true,
                flag_leaf_values: true,
            },
        )
        .unwrap();
        assert_eq!(exported.node_keys.len(), exported.node_prefixes.len());
        assert_eq!(exported.leaf_values.len(), exported.leaf_prefixes.len());
        (exported, continuation)
    }
}

#[derive(Clone, Default)]
struct RecordingOps {
    paths: Arc<Mutex<Vec<StatePartPath>>>,
}

#[async_trait]
impl HorizonRequesterOps for RecordingOps {
    async fn request_for_horizon_chunk(
        &self,
        path: &StatePartPath,
        page_size: i32,
    ) -> Result<(), CasperError> {
        assert_eq!(page_size, PAGE_SIZE);
        self.paths.lock().unwrap().push(path.clone());
        Ok(())
    }
}

async fn one_page() -> (Blake2b256Hash, StoreItemsMessage) {
    let source = Stores::new().await;
    let channel = "channel".to_owned();
    let root = source.add_joins(
        &RadixHistory::empty_root_node_hash(),
        std::slice::from_ref(&channel),
    );
    assert_eq!(source.read_joins(&root, &channel), vec![vec![channel]]);
    let path = vec![(root.clone(), None)];
    let page = source.export(path.clone(), PAGE_SIZE);
    assert_eq!(page.start_path, path);
    assert_eq!(page.last_path, path);
    assert_eq!(page.data_items.len(), 1);
    (root, page)
}

async fn receive_one(
    stores: &Stores,
    root: &Blake2b256Hash,
    page: StoreItemsMessage,
) -> (ST<StatePartPath>, bool) {
    let ops = RecordingOps::default();
    let paths = ops.paths.clone();
    let (tx, rx) = mpsc::channel(2);
    let (states, error) = stream(
        vec![root.clone()],
        stores.has_root(),
        stores.importer(),
        ops,
        rx,
        Duration::from_secs(30),
        Duration::from_secs(120),
    )
    .await
    .unwrap();
    let mut states = Box::pin(states);
    let requested = states.next().await.unwrap();
    assert!(!requested.is_finished());
    assert_eq!(*paths.lock().unwrap(), vec![vec![(root.clone(), None)]]);
    tx.send(page).await.unwrap();
    let received = states.next().await.unwrap();
    let failed = error.lock().unwrap().is_some();
    (received, failed)
}

fn assert_rejected(
    stores: &Stores,
    before: &Snapshot,
    state: &ST<StatePartPath>,
    failed: bool,
    root: &Blake2b256Hash,
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
    if state.is_finished() {
        violations.push("invalid import completed startup sync");
    }
    if state.done_count() != 0 {
        violations.push("invalid page counted as completed progress");
    }
    let expected_path = vec![(root.clone(), None)];
    if state.d.keys().cloned().collect::<HashSet<_>>() != HashSet::from([expected_path.clone()]) {
        violations.push("invalid page changed the original request path");
    }
    if !failed && state.get_next(true).1 != vec![expected_path] {
        violations.push("rejected page neither failed startup nor retained requestability");
    }
    assert!(
        violations.is_empty(),
        "invalid horizon page effects: {violations:?}"
    );
}

#[tokio::test]
async fn horizon_valid_exporter_page_imports_readable_state() {
    let (root, page) = one_page().await;
    let stores = Stores::new().await;
    RSpaceImporterInstance::validate_state_items(
        page.history_items
            .iter()
            .map(|(k, v)| (k.clone(), v.to_vec()))
            .collect(),
        page.data_items
            .iter()
            .map(|(k, v)| (k.clone(), v.to_vec()))
            .collect(),
        page.start_path.clone(),
        PAGE_SIZE,
        0,
        stores.importer(),
    )
    .unwrap();
    let (state, failed) = receive_one(&stores, &root, page).await;
    assert!(!failed);
    assert!(state.is_finished());
    assert!(stores.has_root()(&root).unwrap());
    assert_eq!(stores.read_joins(&root, &"channel".to_owned()), vec![vec![
        "channel".to_owned()
    ]]);
}

#[tokio::test]
async fn horizon_unknown_response_preserves_stores_and_pending_state() {
    let (root, mut page) = one_page().await;
    let stores = Stores::new().await;
    let before = stores.snapshot();
    page.start_path = vec![(Blake2b256Hash::new(b"unknown-root"), None)];
    let (state, failed) = receive_one(&stores, &root, page).await;
    assert!(!failed);
    assert_rejected(&stores, &before, &state, failed, &root);
    assert_eq!(
        state.d,
        HashMap::from([(vec![(root, None)], ReqStatus::Requested)])
    );
}

#[tokio::test]
async fn horizon_mismatched_history_hash_preserves_stores() {
    let (root, mut page) = one_page().await;
    let stores = Stores::new().await;
    let (key, original) = &page.history_items[0];
    stores
        .history
        .put_one(key.bytes(), original.to_vec())
        .unwrap();
    let before = stores.snapshot();
    page.history_items[0].1 = Bytes::from_static(b"invalid-history");
    let (state, failed) = receive_one(&stores, &root, page).await;
    assert_rejected(&stores, &before, &state, failed, &root);
}

#[tokio::test]
async fn horizon_corrupt_cold_value_preserves_stores() {
    let (root, mut page) = one_page().await;
    let stores = Stores::new().await;
    let (key, original) = &page.data_items[0];
    stores.cold.put_one(key.bytes(), original.to_vec()).unwrap();
    let before = stores.snapshot();
    page.data_items[0].1 = Bytes::from_static(b"invalid-cold");
    let (state, failed) = receive_one(&stores, &root, page).await;
    assert_rejected(&stores, &before, &state, failed, &root);
}

#[tokio::test]
async fn horizon_missing_cold_value_cannot_complete_startup() {
    let (root, mut page) = one_page().await;
    let stores = Stores::new().await;
    let before = stores.snapshot();
    page.data_items.clear();
    let (state, failed) = receive_one(&stores, &root, page).await;
    assert_rejected(&stores, &before, &state, failed, &root);
}

#[tokio::test]
async fn horizon_forged_cursor_preserves_stores_and_progress() {
    let (root, mut page) = one_page().await;
    let stores = Stores::new().await;
    let before = stores.snapshot();
    page.last_path = vec![(Blake2b256Hash::new(b"forged-cursor"), None)];
    let (state, failed) = receive_one(&stores, &root, page).await;
    assert_rejected(&stores, &before, &state, failed, &root);
}

#[tokio::test]
async fn horizon_canonical_multipage_stream_completes_shared_history_roots() {
    let source = Stores::new().await;
    let channels: Vec<String> = (0..16384).map(|i| format!("channel-{i}")).collect();
    let first = source.add_joins(&RadixHistory::empty_root_node_hash(), &channels);
    let second = source.add_joins(&first, &["additional-channel".to_owned()]);
    let stores = Stores::new().await;
    let ops = RecordingOps::default();
    let paths = ops.paths.clone();
    let (tx, rx) = mpsc::channel(16);
    let (states, error) = stream(
        vec![first.clone(), second.clone()],
        stores.has_root(),
        stores.importer(),
        ops,
        rx,
        Duration::from_secs(30),
        Duration::from_secs(120),
    )
    .await
    .unwrap();
    let mut states = Box::pin(states);
    let mut nonterminal_pages = 0;
    let mut bounded_roots = HashSet::new();
    let mut seen_requests = HashSet::new();
    let mut complete = false;
    for _ in 0..32 {
        let state = states.next().await.expect("sync emits a state");
        assert!(
            error.lock().unwrap().is_none(),
            "canonical exporter must not cause a stream error"
        );
        if state.is_finished() {
            complete = true;
            break;
        }
        let requested = std::mem::take(&mut *paths.lock().unwrap());
        for path in requested {
            if path.len() == 7 {
                assert!(
                    seen_requests.insert(path.clone()),
                    "a root-prefixed continuation must not cycle"
                );
            }
            let page = source.export(path, PAGE_SIZE);
            if page.last_path != page.start_path {
                nonterminal_pages += 1;
            }
            if page.last_path.len() == 7 {
                bounded_roots.insert(page.last_path[0].0.clone());
            }
            tx.send(page).await.unwrap();
        }
    }
    assert!(
        complete,
        "two finite exported tries must complete within their page count"
    );
    assert!(
        nonterminal_pages >= 2,
        "both roots require actual pagination"
    );
    assert_eq!(
        bounded_roots,
        HashSet::from([first.clone(), second.clone()]),
        "both roots cross the actual history-node page budget"
    );
    for root in [&first, &second] {
        assert!(stores.has_root()(root).unwrap());
        for channel in &channels {
            assert_eq!(stores.read_joins(root, channel), vec![
                vec![channel.clone()]
            ]);
        }
    }
    assert_eq!(
        stores.read_joins(&second, &"additional-channel".to_owned()),
        vec![vec!["additional-channel".to_owned()]]
    );
    assert!(stores
        .read_joins(&first, &"additional-channel".to_owned())
        .is_empty());
}

async fn shared_cursor_fixture() -> (
    Stores,
    Vec<String>,
    String,
    Blake2b256Hash,
    Blake2b256Hash,
    StoreItemsMessage,
    StoreItemsMessage,
) {
    let source = Stores::new().await;
    let channels: Vec<String> = (0..2048).map(|i| format!("channel-{i}")).collect();
    let first = source.add_joins(&RadixHistory::empty_root_node_hash(), &channels);
    let extra = (0..4096)
        .map(|i| format!("extra-low-channel-{i}"))
        .find(|channel| hash(channel).bytes()[0] == 0)
        .expect("fixture has a distinct low-prefix channel");
    let second = source.add_joins(&first, std::slice::from_ref(&extra));
    assert_ne!(first, second);
    let first_page = source.export(vec![(first.clone(), None)], PAGE_SIZE);
    let second_page = source.export(vec![(second.clone(), None)], PAGE_SIZE);
    let shared = first_page.last_path.clone();
    assert_eq!(shared.len(), 1, "fixture ends at a shared subtree cursor");
    assert_eq!(shared, second_page.last_path);
    assert_ne!(shared, first_page.start_path);
    assert_ne!(shared, second_page.start_path);
    (
        source,
        channels,
        extra,
        first,
        second,
        first_page,
        second_page,
    )
}

async fn canonical_shared_cursor(late_second_root: bool) {
    let (source, channels, extra, first, second, first_page, second_page) =
        shared_cursor_fixture().await;
    let shared = first_page.last_path.clone();
    let terminal = source.export(shared.clone(), PAGE_SIZE);
    assert_eq!(terminal.last_path, shared);
    let stores = Stores::new().await;
    let ops = RecordingOps::default();
    let paths = ops.paths.clone();
    let (tx, rx) = mpsc::channel(4);
    let (states, error) = stream(
        vec![first.clone(), second.clone()],
        stores.has_root(),
        stores.importer(),
        ops,
        rx,
        Duration::from_secs(30),
        Duration::from_secs(120),
    )
    .await
    .unwrap();
    let mut states = Box::pin(states);
    assert!(!states.next().await.unwrap().is_finished());
    let initial: HashSet<_> = std::mem::take(&mut *paths.lock().unwrap())
        .into_iter()
        .collect();
    assert_eq!(
        initial,
        HashSet::from([
            first_page.start_path.clone(),
            second_page.start_path.clone()
        ])
    );
    tx.send(first_page).await.unwrap();
    assert!(!states.next().await.unwrap().is_finished());
    if late_second_root {
        assert!(!states.next().await.unwrap().is_finished());
        assert_eq!(std::mem::take(&mut *paths.lock().unwrap()), vec![
            shared.clone()
        ]);
        tx.send(terminal.clone()).await.unwrap();
        assert!(!states.next().await.unwrap().is_finished());
        assert!(stores.has_root()(&first).unwrap());
        assert!(!stores.has_root()(&second).unwrap());
    }
    tx.send(second_page).await.unwrap();
    assert!(!states.next().await.unwrap().is_finished());
    assert!(!states.next().await.unwrap().is_finished());
    assert_eq!(std::mem::take(&mut *paths.lock().unwrap()), vec![shared]);
    tx.send(terminal).await.unwrap();
    assert!(states.next().await.unwrap().is_finished());
    assert!(error.lock().unwrap().is_none());
    for root in [&first, &second] {
        assert!(stores.has_root()(root).unwrap());
        for channel in &channels {
            assert_eq!(stores.read_joins(root, channel), vec![
                vec![channel.clone()]
            ]);
        }
    }
    assert!(stores.read_joins(&first, &extra).is_empty());
    assert_eq!(stores.read_joins(&second, &extra), vec![vec![extra]]);
}

#[tokio::test]
async fn horizon_canonical_shared_cursor_completes_both_waiting_roots() {
    canonical_shared_cursor(false).await;
}

#[tokio::test]
async fn horizon_canonical_shared_cursor_reopens_for_late_root() {
    canonical_shared_cursor(true).await;
}

async fn shared_singleton_context_fixture() -> (Stores, StoreItemsMessage) {
    let (source, _, _, first, second, first_page, _) = shared_cursor_fixture().await;
    let shared = first_page.last_path;
    let local = source.export_with_prefixes(&shared[0].0);
    assert!(!local.leaf_values.is_empty());
    assert!(local
        .leaf_prefixes
        .iter()
        .any(|prefix| prefix.first() != Some(&PREFIX_JOINS)));
    let mut origins = Vec::new();
    for root in [first, second] {
        let complete = source.export_with_prefixes(&root);
        let occurrence_prefixes: Vec<_> = complete
            .node_keys
            .iter()
            .zip(&complete.node_prefixes)
            .filter_map(|(key, prefix)| (key == &shared[0].0.bytes()).then_some(prefix))
            .collect();
        assert!(!occurrence_prefixes.is_empty());
        for origin in occurrence_prefixes {
            assert_eq!(origin.first(), Some(&PREFIX_JOINS));
            for (key, prefix) in local.leaf_values.iter().zip(&local.leaf_prefixes) {
                let absolute: Vec<_> = origin.iter().chain(prefix).copied().collect();
                assert_eq!(absolute.first(), Some(&PREFIX_JOINS));
                assert!(complete
                    .leaf_values
                    .iter()
                    .zip(&complete.leaf_prefixes)
                    .any(|(candidate, path)| candidate == key && path == &absolute));
            }
            origins.push((root.clone(), shared.clone(), origin.clone()));
        }
    }
    assert!(origins.iter().any(|left| origins
        .iter()
        .any(|right| left.0 != right.0 && left.1 == right.1)));
    let page = source.export(shared.clone(), PAGE_SIZE);
    assert_eq!(page.last_path, shared);
    assert!(!page.history_items.is_empty());
    assert!(!page.data_items.is_empty());
    (source, page)
}

fn validate_singleton_page(stores: &Stores, page: &StoreItemsMessage) -> Result<(), String> {
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
        stores.importer(),
    )
}

#[tokio::test]
async fn canonical_shared_singleton_preserves_original_kind_context() {
    let (_, page) = shared_singleton_context_fixture().await;
    let destination = Stores::new().await;
    validate_singleton_page(&destination, &page).unwrap();
}

#[tokio::test]
async fn canonical_shared_singleton_rejects_hash_preserving_kind_substitution() {
    let (_, mut page) = shared_singleton_context_fixture().await;
    let destination = Stores::new().await;
    let before = destination.snapshot();
    validate_singleton_page(&destination, &page).unwrap();
    for (key, value) in &mut page.data_items {
        let PersistedData::Joins(leaf) = bincode::deserialize(value).unwrap() else {
            panic!("the original-root occurrence requires joins");
        };
        let substituted = DataLeaf { bytes: leaf.bytes };
        assert_eq!(
            Blake2b256Hash::new(&bincode::serialize(&substituted).unwrap()),
            *key
        );
        *value = Bytes::from(bincode::serialize(&PersistedData::Data(substituted)).unwrap());
    }
    let result = validate_singleton_page(&destination, &page);
    assert_eq!(destination.snapshot(), before);
    assert!(
        result.is_err(),
        "a shared singleton must retain the original joins requirement despite an unchanged payload hash"
    );
}

#[tokio::test]
async fn canonical_shared_singleton_rejects_dually_decodable_kind_substitution() {
    let source = Stores::new().await;
    let bytes = encode_joins(&Vec::<Vec<String>>::new());
    assert!(decode_joins::<String>(&bytes).is_empty());
    assert!(decode_datums::<String>(&bytes).is_empty());
    let leaf = JoinsLeaf { bytes };
    let leaf_hash = Blake2b256Hash::new(&bincode::serialize(&leaf).unwrap());
    source
        .cold
        .put_one(
            bincode::serialize(&leaf_hash).unwrap(),
            bincode::serialize(&PersistedData::Joins(leaf.clone())).unwrap(),
        )
        .unwrap();
    let channels = [
        "empty-joins-first".to_owned(),
        "empty-joins-second".to_owned(),
    ];
    let actions = channels
        .iter()
        .map(|channel| {
            let mut key = vec![PREFIX_JOINS];
            key.extend(hash(channel).bytes());
            HistoryAction::Insert(InsertAction {
                key,
                hash: leaf_hash.clone(),
            })
        })
        .collect();
    let root = RadixHistory::create(RadixHistory::empty_root_node_hash(), source.history.clone())
        .unwrap()
        .process(actions)
        .unwrap()
        .root();
    source.publish(&root);
    let original = vec![(root.clone(), None)];
    let shared = source.export(original.clone(), PAGE_SIZE).last_path;
    assert_eq!(shared.len(), 1);
    assert_ne!(shared, original);
    let complete = source.export_with_prefixes(&root);
    assert!(complete
        .node_keys
        .iter()
        .zip(&complete.node_prefixes)
        .any(|(key, origin)| key == &shared[0].0.bytes() && origin.first() == Some(&PREFIX_JOINS)));
    let mut page = source.export(shared.clone(), PAGE_SIZE);
    assert_eq!(page.last_path, shared);
    assert!(!page.history_items.is_empty());
    assert_eq!(page.data_items.len(), 1);
    let destination = Stores::new().await;
    validate_singleton_page(&destination, &page).unwrap();
    for channel in &channels {
        assert!(source.read_joins(&root, channel).is_empty());
    }
    let substituted = DataLeaf { bytes: leaf.bytes };
    assert_eq!(
        Blake2b256Hash::new(&bincode::serialize(&substituted).unwrap()),
        leaf_hash
    );
    page.data_items[0].1 =
        Bytes::from(bincode::serialize(&PersistedData::Data(substituted)).unwrap());
    assert!(
        validate_singleton_page(&destination, &page).is_err(),
        "an envelope-selected decoder cannot replace the authenticated occurrence kind"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        max_shrink_iters: 1_024,
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_subtree_occurrences_preserve_full_joins_paths(seed in any::<u64>(), count in 2usize..33) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let source = Stores::new().await;
            let channels: Vec<_> = (0..count).map(|i| format!("context-{seed}-{i}")).collect();
            let root = source.add_joins(&RadixHistory::empty_root_node_hash(), &channels);
            let complete = source.export_with_prefixes(&root);
            prop_assert_eq!(complete.leaf_values.len(), count);
            let full_occurrences: HashSet<_> = complete.leaf_values.iter()
                .cloned().zip(complete.leaf_prefixes.iter().cloned()).collect();
            prop_assert_eq!(full_occurrences.len(), count);
            let mut checked_subtrees = 0;
            for (key, origin) in complete.node_keys.iter().zip(&complete.node_prefixes) {
                if origin.is_empty() {
                    continue;
                }
                checked_subtrees += 1;
                prop_assert_eq!(origin.first(), Some(&PREFIX_JOINS));
                let local = source.export_with_prefixes(&Blake2b256Hash::from_bytes(key.clone()));
                prop_assert!(!local.leaf_values.is_empty());
                let expected: HashSet<_> = full_occurrences.iter()
                    .filter(|(_, path)| path.starts_with(origin))
                    .cloned().collect();
                let mut actual = HashSet::new();
                for (leaf, suffix) in local.leaf_values.iter().zip(&local.leaf_prefixes) {
                    let absolute: Vec<_> = origin.iter().chain(suffix).copied().collect();
                    prop_assert_eq!(absolute.first(), Some(&PREFIX_JOINS));
                    prop_assert!(actual.insert((leaf.clone(), absolute)));
                }
                prop_assert_eq!(actual, expected);
            }
            prop_assert!(checked_subtrees > 0);
            for channel in &channels {
                prop_assert_eq!(source.read_joins(&root, channel), vec![vec![channel.clone()]]);
            }
            Ok::<(), proptest::test_runner::TestCaseError>(())
        })?;
    }

    #[test]
    fn generated_cursor_pages_follow_occurrence_skip_and_take(
        seed in any::<u64>(),
        count in 2usize..65,
        resume in any::<bool>(),
        selector in any::<usize>(),
        skip in 0i32..80,
        take in prop_oneof![1i32..8, Just(750), Just(1024)],
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let source = Stores::new().await;
            let channels: Vec<_> = (0..count).map(|i| format!("slice-{seed}-{i}")).collect();
            let root = source.add_joins(&RadixHistory::empty_root_node_hash(), &channels);
            let complete = source.export_with_prefixes(&root);
            let mut trace: Vec<_> = complete.node_keys.iter().zip(&complete.node_prefixes)
                .map(|(key, path)| (false, path.clone(), key.clone()))
                .chain(complete.leaf_values.iter().zip(&complete.leaf_prefixes)
                    .map(|(key, path)| (true, path.clone(), key.clone())))
                .collect();
            trace.sort_by(|left, right| left.1.cmp(&right.1));
            prop_assert!(trace.windows(2).all(|pair| pair[0].1 < pair[1].1));
            prop_assert_eq!(&trace[0], &(false, Vec::new(), root.bytes()));
            let prefix = resume.then(|| complete.node_prefixes[selector % complete.node_prefixes.len()].clone());
            let start = match &prefix {
                Some(prefix) => trace.iter().position(|(leaf, path, _)| !leaf && path == prefix).unwrap() + 1,
                None => 0,
            };
            let mut skip_remaining = skip;
            let mut take_remaining = take;
            let mut expected_history = Vec::new();
            let mut expected_leaves = Vec::new();
            for (leaf, path, key) in &trace[start..] {
                if skip_remaining > 0 {
                    skip_remaining -= i32::from(!leaf);
                    continue;
                }
                if take_remaining == 0 {
                    break;
                }
                if *leaf {
                    expected_leaves.push((key.clone(), path.clone()));
                } else {
                    expected_history.push((key.clone(), path.clone()));
                    take_remaining -= 1;
                }
            }
            let (page, continuation) = source.export_prefix_page(&root, prefix, skip, take);
            let actual_history: Vec<_> = page.node_keys.into_iter().zip(page.node_prefixes).collect();
            let actual_leaves: Vec<_> = page.leaf_values.into_iter().zip(page.leaf_prefixes).collect();
            prop_assert_eq!(&actual_history, &expected_history);
            prop_assert_eq!(&actual_leaves, &expected_leaves);
            prop_assert!(actual_history.len() <= take as usize);
            if let Some(next) = continuation {
                prop_assert_eq!(actual_history.len(), take as usize);
                prop_assert_eq!(&next, &actual_history.last().unwrap().1);
                let (remainder, done) = source.export_prefix_page(&root, Some(next.clone()), 0, PAGE_SIZE);
                prop_assert!(done.is_none());
                let position = trace.iter().position(|(leaf, path, _)| !leaf && path == &next).unwrap();
                let expected_history: Vec<_> = trace[position + 1..].iter().filter(|(leaf, _, _)| !leaf)
                    .map(|(_, path, key)| (key.clone(), path.clone())).collect();
                let expected_leaves: Vec<_> = trace[position + 1..].iter().filter(|(leaf, _, _)| *leaf)
                    .map(|(_, path, key)| (key.clone(), path.clone())).collect();
                prop_assert_eq!(remainder.node_keys.into_iter().zip(remainder.node_prefixes).collect::<Vec<_>>(), expected_history);
                prop_assert_eq!(remainder.leaf_values.into_iter().zip(remainder.leaf_prefixes).collect::<Vec<_>>(), expected_leaves);
            }
            Ok::<(), proptest::test_runner::TestCaseError>(())
        })?;
    }
}

#[tokio::test]
async fn exporter_exact_history_page_boundary_preserves_cold_leaf() {
    let source = Stores::new().await;
    let channel = "boundary-channel".to_owned();
    let root = source.add_joins(
        &RadixHistory::empty_root_node_hash(),
        std::slice::from_ref(&channel),
    );
    let complete = source.export(vec![(root.clone(), None)], PAGE_SIZE);
    assert_eq!(complete.history_items.len(), 1);
    assert_eq!(complete.data_items.len(), 1);
    let first = source.export(vec![(root.clone(), None)], 1);
    assert_eq!(first.last_path.len(), 7);
    assert!(first.data_items.is_empty());
    let remaining = source.export(first.last_path, 1);
    assert_eq!(
        remaining.data_items, complete.data_items,
        "a leaf-only continuation must retain its cold item"
    );
    assert_eq!(source.read_joins(&root, &channel), vec![vec![channel]]);
}

async fn check_production_history_page_boundary(page_size: i32, channel_count: usize) {
    let source = Stores::new().await;
    let channels: Vec<_> = (0..channel_count)
        .map(|index| format!("exact-history-{page_size}-{index}"))
        .collect();
    let root = source.add_joins(&RadixHistory::empty_root_node_hash(), &channels);
    let (traversal, continuation) = source.export_prefix_page(&root, None, 0, page_size + 1);
    assert_eq!(
        traversal.node_keys.len(),
        usize::try_from(page_size).unwrap()
    );
    assert!(continuation.is_none());
    for channel in [channels.first().unwrap(), channels.last().unwrap()] {
        assert_eq!(source.read_joins(&root, channel), vec![vec![
            channel.clone()
        ]]);
    }
    let prefix = traversal.node_prefixes.last().unwrap().clone();
    let (tail, continuation) = source.export_prefix_page(&root, Some(prefix), 0, page_size);
    assert!(tail.node_keys.is_empty());
    assert!(!tail.leaf_values.is_empty());
    assert!(continuation.is_none());
    eprintln!(
        "prepared boundary: page_size={page_size}, history_nodes={}, cold_values={}, tail_cold_values={}",
        traversal.node_keys.len(),
        traversal.leaf_values.len(),
        tail.leaf_values.len(),
    );
    let complete = source.export(vec![(root.clone(), None)], page_size + 1);
    let first = source.export(vec![(root, None)], page_size);
    assert_eq!(
        first.history_items.len(),
        usize::try_from(page_size).unwrap()
    );
    assert_eq!(first.last_path.len(), 7);
    assert!(first.data_items.len() < complete.data_items.len());
    let remaining = source.export(first.last_path, page_size);
    assert!(remaining.history_items.is_empty());
    let actual: BTreeMap<_, _> = first
        .data_items
        .into_iter()
        .chain(remaining.data_items)
        .map(|(key, value)| (key.bytes(), value.to_vec()))
        .collect();
    let expected: BTreeMap<_, _> = complete
        .data_items
        .into_iter()
        .map(|(key, value)| (key.bytes(), value.to_vec()))
        .collect();
    assert!(
        actual == expected,
        "the cold-only final page must preserve every value: expected={}, actual={}, first mismatched keys={:?}",
        expected.len(),
        actual.len(),
        expected.iter()
            .filter(|(key, value)| actual.get(*key) != Some(*value))
            .take(4)
            .map(|(key, _)| hex::encode(key))
            .collect::<Vec<_>>(),
    );
}

#[tokio::test]
async fn exporter_exact_750_history_page_boundary_preserves_cold_values() {
    check_production_history_page_boundary(750, 8326).await;
}

#[tokio::test]
async fn exporter_exact_1024_history_page_boundary_preserves_cold_values() {
    check_production_history_page_boundary(1024, 10732).await;
}

#[tokio::test]
async fn state_import_checkpoint_pristine_root_has_exportable_history() {
    let stores = Stores::pristine().await;
    let repository = HistoryRepositoryInstances::<String, String, String, String>::lmdb_repository(
        stores.history.clone(),
        stores.roots.clone(),
        stores.cold.clone(),
    )
    .unwrap();
    let root = repository.root();
    assert_eq!(root, RadixHistory::empty_root_node_hash());
    assert!(repository.contains_root(&root).unwrap());
    assert_eq!(
        stores.history.get_one(&root.bytes()).unwrap(),
        Some(hash_node(&empty_node()).1),
        "a published empty root must have its exact exportable history row"
    );
    let page = stores.export(vec![(root.clone(), None)], PAGE_SIZE);
    assert_eq!(page.history_items.len(), 1);
    assert_eq!(page.history_items[0].0, root);
    assert!(page.data_items.is_empty());
}

#[tokio::test]
async fn state_import_checkpoint_missing_nonempty_base_is_rejected() {
    let stores = Stores::pristine().await;
    let root = Blake2b256Hash::new(b"missing nonempty checkpoint base");
    assert_ne!(root, RadixHistory::empty_root_node_hash());
    stores.publish(&root);
    let repository = HistoryRepositoryInstances::<String, String, String, String>::lmdb_repository(
        stores.history.clone(),
        stores.roots.clone(),
        stores.cold.clone(),
    );
    assert!(
        repository.is_err(),
        "a missing nonempty root cannot become an empty checkpoint base"
    );
}

#[tokio::test]
async fn state_import_checkpoint_reset_missing_root_preserves_selection() {
    let stores = Stores::new().await;
    let channel = "retained-root-after-missing-reset".to_owned();
    let base = stores.add_joins(
        &RadixHistory::empty_root_node_hash(),
        std::slice::from_ref(&channel),
    );
    let repository = HistoryRepositoryInstances::<String, String, String, String>::lmdb_repository(
        stores.history.clone(),
        stores.roots.clone(),
        stores.cold.clone(),
    )
    .unwrap();
    let missing = Blake2b256Hash::new(b"tagged target without checkpoint history");
    assert_ne!(missing, base);
    let roots = RootsStoreInstances::roots_store(stores.roots.clone());
    roots.record_root(&missing).unwrap();
    roots.validate_and_set_current_root(base.clone()).unwrap();
    assert_eq!(roots.current_root().unwrap(), Some(base.clone()));
    assert!(roots.contains_root(&missing).unwrap());
    assert!(stores.history.get_one(&missing.bytes()).unwrap().is_none());
    let before = stores.snapshot();
    let result = repository.reset(&missing);
    let after = stores.snapshot();
    assert_eq!(
        (result.is_err(), after == before),
        (true, true),
        "a missing target must fail before this reset changes any stored state"
    );
    assert_eq!(repository.root(), base);
    assert_eq!(stores.read_joins(&base, &channel), vec![vec![channel]]);
}

async fn checkpoint_invalid_root_loads(reset: bool) {
    let mut violations = Vec::new();
    for kind in ["wrong-hash", "malformed-node", "invalid-key-width"] {
        let stores = Stores::new().await;
        let base = RadixHistory::empty_root_node_hash();
        let repository =
            HistoryRepositoryInstances::<String, String, String, String>::lmdb_repository(
                stores.history.clone(),
                stores.roots.clone(),
                stores.cold.clone(),
            )
            .unwrap();
        let (target, encoded) = match kind {
            "wrong-hash" => (
                Blake2b256Hash::new(b"wrong history row binding"),
                hash_node(&empty_node()).1,
            ),
            "malformed-node" => {
                let bytes = vec![255];
                (Blake2b256Hash::new(&bytes), bytes)
            }
            "invalid-key-width" => (
                Blake2b256Hash::from_bytes(vec![17; 31]),
                hash_node(&empty_node()).1,
            ),
            _ => unreachable!(),
        };
        stores.history.put_one(target.bytes(), encoded).unwrap();
        let roots = RootsStoreInstances::roots_store(stores.roots.clone());
        roots.record_root(&target).unwrap();
        if reset {
            roots.validate_and_set_current_root(base.clone()).unwrap();
        }
        let before = stores.snapshot();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if reset {
                repository
                    .reset(&target)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            } else {
                HistoryRepositoryInstances::<String, String, String, String>::lmdb_repository(
                    stores.history.clone(),
                    stores.roots.clone(),
                    stores.cold.clone(),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            }
        }));
        match result {
            Ok(Err(_)) => {}
            Ok(Ok(())) => violations.push(format!("{kind}: accepted invalid history")),
            Err(_) => violations.push(format!(
                "{kind}: panicked instead of returning a storage error"
            )),
        }
        if stores.snapshot() != before {
            violations.push(format!("{kind}: invalid load changed stored state"));
        }
    }
    assert!(
        violations.is_empty(),
        "invalid root-load violations: {violations:?}"
    );
}

#[tokio::test]
async fn state_import_checkpoint_constructor_rejects_invalid_root_rows_without_panicking() {
    checkpoint_invalid_root_loads(false).await;
}

#[tokio::test]
async fn state_import_checkpoint_reset_rejects_invalid_root_rows_without_selection() {
    checkpoint_invalid_root_loads(true).await;
}

fn codec_wire_record(slot: u8, history: bool, prefix: &[u8], target: &[u8; 32]) -> Vec<u8> {
    assert!(prefix.len() <= 127);
    let mut bytes = vec![slot, prefix.len() as u8 | if history { 128 } else { 0 }];
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(target);
    bytes
}

fn codec_item(history: bool, prefix: Vec<u8>, target: [u8; 32]) -> Item {
    if history {
        Item::NodePtr {
            prefix,
            ptr: target.to_vec(),
        }
    } else {
        Item::Leaf {
            prefix,
            value: target.to_vec(),
        }
    }
}

fn codec_load_preserves_projection(
    bytes: Vec<u8>,
    expected: &Node,
) -> proptest::test_runner::TestCaseResult {
    let store = Arc::new(InMemoryKeyValueStore::new());
    let key = Blake2b256Hash::new(&bytes).bytes();
    store.put_one(key.clone(), bytes.clone()).unwrap();
    let before = store.to_map().unwrap();
    let tree = RadixTreeImpl::new(store.clone());
    for mode in [Some(true), None, Some(false)] {
        let result = tree.load_node(key.clone(), mode);
        prop_assert!(result.is_ok(), "valid wire records must load: {:?}", result);
        prop_assert_eq!(&result.unwrap(), expected);
    }
    prop_assert_eq!(tree.cache_r.len(), 1);
    prop_assert!(tree.cache_r.contains_key(&key));
    prop_assert!(tree.cache_w.is_empty());
    prop_assert_eq!(store.get_one(&key).unwrap(), Some(bytes));
    prop_assert_eq!(store.to_map().unwrap(), before);
    Ok(())
}

fn codec_load_rejects_without_effects(
    key: Vec<u8>,
    bytes: Vec<u8>,
) -> proptest::test_runner::TestCaseResult {
    let store = Arc::new(InMemoryKeyValueStore::new());
    store.put_one(key.clone(), bytes).unwrap();
    let before = store.to_map().unwrap();
    for mode in [Some(true), None, Some(false)] {
        for warm in [false, true] {
            let tree = RadixTreeImpl::new(store.clone());
            if warm {
                let mut pending = empty_node();
                pending[255] = codec_item(false, vec![29, 57], [43; 32]);
                let pending_key = tree.save_node(pending);
                prop_assert_ne!(&pending_key, &key);
            }
            let read_snapshot = || {
                tree.cache_r
                    .iter()
                    .map(|entry| (entry.key().clone(), entry.value().as_ref().clone()))
                    .collect::<BTreeMap<_, _>>()
            };
            let write_snapshot = || {
                tree.cache_w
                    .iter()
                    .map(|entry| (entry.key().clone(), entry.value().clone()))
                    .collect::<BTreeMap<_, _>>()
            };
            let read_before = read_snapshot();
            let write_before = write_snapshot();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tree.load_node(key.clone(), mode)
            }));
            prop_assert!(
                result.is_ok(),
                "invalid radix input must return an error, not panic"
            );
            prop_assert!(
                result.unwrap().is_err(),
                "invalid radix input must not authenticate"
            );
            prop_assert_eq!(read_snapshot(), read_before);
            prop_assert_eq!(write_snapshot(), write_before);
            prop_assert_eq!(&store.to_map().unwrap(), &before);
        }
    }
    Ok(())
}

#[test]
fn state_import_codec_all_headers_and_maximum_capacity_preserve_raw_identity() {
    for maximal_prefix in [false, true] {
        let mut records = Vec::new();
        let mut expected = empty_node();
        for slot in 0..=255u8 {
            let history = slot >= 128;
            let prefix = vec![
                slot;
                if maximal_prefix {
                    127
                } else {
                    usize::from(slot % 128)
                }
            ];
            let target = [slot; 32];
            records.push(codec_wire_record(slot, history, &prefix, &target));
            expected[usize::from(slot)] = codec_item(history, prefix, target);
        }
        if maximal_prefix {
            assert_eq!(records.iter().map(Vec::len).sum::<usize>(), 256 * 161);
        }
        for order in [records.clone(), records.into_iter().rev().collect()] {
            codec_load_preserves_projection(order.concat(), &expected).unwrap();
            let mut prefix = Vec::new();
            let mut projected = empty_node();
            for record in order.iter().take(3) {
                prefix.extend_from_slice(record);
                projected[usize::from(record[0])] = expected[usize::from(record[0])].clone();
                codec_load_preserves_projection(prefix.clone(), &projected).unwrap();
            }
        }
    }
    codec_load_preserves_projection(Vec::new(), &empty_node()).unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, max_shrink_iters: 128, ..ProptestConfig::default() })]

    #[test]
    fn state_import_codec_generated_record_order_preserves_slot_projection(
        records in prop::collection::btree_map(
            any::<u8>(),
            (any::<bool>(), prop::collection::vec(any::<u8>(), 0..128), any::<[u8; 32]>()),
            0..33,
        ),
        rotation in any::<usize>(),
        reverse in any::<bool>(),
    ) {
        let mut expected = empty_node();
        let mut encoded = Vec::new();
        for (slot, (history, prefix, target)) in records {
            encoded.push(codec_wire_record(slot, history, &prefix, &target));
            expected[usize::from(slot)] = codec_item(history, prefix, target);
        }
        if reverse {
            encoded.reverse();
        }
        if !encoded.is_empty() {
            let offset = rotation % encoded.len();
            encoded.rotate_left(offset);
        }
        codec_load_preserves_projection(encoded.concat(), &expected)?;
    }

    #[test]
    fn state_import_codec_truncated_records_fail_without_panics_or_effects(
        slot in any::<u8>(),
        history in any::<bool>(),
        prefix in prop::collection::vec(any::<u8>(), 0..128),
        target in any::<[u8; 32]>(),
        cut in any::<usize>(),
    ) {
        let record = codec_wire_record(slot, history, &prefix, &target);
        let cut = 1 + cut % (record.len() - 1);
        let truncated = record[..cut].to_vec();
        let key = Blake2b256Hash::new(&truncated).bytes();
        codec_load_rejects_without_effects(key, truncated.clone())?;
        let mut late_truncation = codec_wire_record(slot.wrapping_add(1), false, &[], &[0; 32]);
        late_truncation.extend(codec_wire_record(slot.wrapping_add(2), true, &[255; 127], &[0; 32]));
        late_truncation.extend(truncated);
        let key = Blake2b256Hash::new(&late_truncation).bytes();
        codec_load_rejects_without_effects(key, late_truncation)?;
        let mut trailing = record;
        trailing.push(slot.wrapping_add(1));
        let key = Blake2b256Hash::new(&trailing).bytes();
        codec_load_rejects_without_effects(key, trailing)?;
    }

    #[test]
    fn state_import_codec_duplicate_slots_fail_without_panics_or_effects(
        slot in any::<u8>(),
        history in any::<bool>(),
        prefix in prop::collection::vec(any::<u8>(), 0..128),
        target in any::<[u8; 32]>(),
        different in any::<bool>(),
    ) {
        let record = codec_wire_record(slot, history, &prefix, &target);
        let mut duplicate = record.clone();
        let mut second = record;
        if different {
            *second.last_mut().unwrap() ^= 1;
        }
        duplicate.extend(second);
        let key = Blake2b256Hash::new(&duplicate).bytes();
        codec_load_rejects_without_effects(key, duplicate)?;
    }

    #[test]
    fn state_import_codec_invalid_key_bindings_fail_without_cache_or_store_effects(
        slot in any::<u8>(),
        history in any::<bool>(),
        prefix in prop::collection::vec(any::<u8>(), 0..128),
        target in any::<[u8; 32]>(),
        changed_byte in 0usize..32,
    ) {
        let record = codec_wire_record(slot, history, &prefix, &target);
        let mut wrong_key = Blake2b256Hash::new(&record).bytes();
        wrong_key[changed_byte] ^= 1;
        codec_load_rejects_without_effects(wrong_key, record.clone())?;
        for width in [0, 1, 31, 33, 40, 64] {
            codec_load_rejects_without_effects(vec![slot; width], record.clone())?;
        }
    }
}

#[tokio::test]
async fn state_import_checkpoint_first_nonempty_root_consumes_exact_joins() {
    use rspace_plus_plus::rspace::hot_store_action::{HotStoreAction, InsertAction, InsertJoins};

    let stores = Stores::pristine().await;
    let repository = HistoryRepositoryInstances::<String, String, String, String>::lmdb_repository(
        stores.history.clone(),
        stores.roots.clone(),
        stores.cold.clone(),
    )
    .unwrap();
    let channel = "first-checkpoint-channel".to_owned();
    let joins = vec![vec!["first-checkpoint-value".to_owned()]];
    let checkpoint = repository.checkpoint(vec![HotStoreAction::Insert(
        InsertAction::InsertJoins(InsertJoins {
            channel: channel.clone(),
            joins: joins.clone(),
        }),
    )]);
    let root = checkpoint.root();
    assert_ne!(root, RadixHistory::empty_root_node_hash());
    assert!(checkpoint.contains_root(&root).unwrap());
    assert!(stores.history.get_one(&root.bytes()).unwrap().is_some());
    assert_eq!(stores.read_joins(&root, &channel), joins);
    let exported = stores.export_with_prefixes(&root);
    assert_eq!(exported.leaf_values.len(), 1);
    let mut expected_path = vec![PREFIX_JOINS];
    expected_path.extend(hash(&channel).bytes());
    assert_eq!(exported.leaf_prefixes, vec![expected_path]);
}

#[tokio::test]
async fn state_import_exporter_standalone_cold_matches_combined() {
    let stores = Stores::new().await;
    let channel = "standalone-cold-export".to_owned();
    let root = stores.add_joins(&RadixHistory::empty_root_node_hash(), &[channel]);
    let path = vec![(root, None)];
    let combined = stores.export(path.clone(), PAGE_SIZE);
    assert_eq!(combined.data_items.len(), 1);
    let exporter = RSpaceExporterStore::create(stores.history, stores.cold, stores.roots);
    let standalone =
        RSpaceExporterItems.get_data(Arc::new(Mutex::new(Box::new(exporter))), path, 0, PAGE_SIZE);
    let actual: BTreeMap<_, _> = standalone
        .items
        .into_iter()
        .map(|(key, value)| (key, Bytes::from(value)))
        .collect();
    let expected: BTreeMap<_, _> = combined.data_items.into_iter().collect();
    assert_eq!(
        actual, expected,
        "standalone cold export must select leaf records"
    );
    assert_eq!(standalone.last_path, combined.last_path);
}

type CheckpointAction = rspace_plus_plus::rspace::hot_store_trie_action::HotStoreTrieAction<
    String,
    String,
    String,
    String,
>;

fn checkpoint_channel_key(kind: u8, channel: &String) -> Blake2b256Hash {
    if kind == PREFIX_KONT {
        hash_from_vec(&vec![channel.clone()])
    } else {
        hash(channel)
    }
}

fn checkpoint_typed_insert(
    kind: u8,
    channel: &String,
    values: &[(u16, bool)],
) -> (CheckpointAction, Vec<Vec<u8>>) {
    use rspace_plus_plus::rspace::hot_store_trie_action::{
        HotStoreTrieAction, TrieInsertAction, TrieInsertConsume, TrieInsertJoins, TrieInsertProduce,
    };
    use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};

    let key = checkpoint_channel_key(kind, channel);
    let (insert, mut encoded): (_, Vec<Vec<u8>>) = match kind {
        PREFIX_DATUM => {
            let data: Vec<_> = values
                .iter()
                .map(|(value, persist)| Datum::create(channel, format!("datum-{value}"), *persist))
                .collect();
            let encoded = data
                .iter()
                .map(|value| bincode::serialize(value).unwrap())
                .collect();
            (
                TrieInsertAction::TrieInsertProduce(TrieInsertProduce { hash: key, data }),
                encoded,
            )
        }
        PREFIX_KONT => {
            let continuations: Vec<_> = values
                .iter()
                .map(|(value, persist)| {
                    WaitingContinuation::create(
                        &vec![channel.clone()],
                        &vec![format!("pattern-{value}")],
                        &format!("continuation-{value}"),
                        *persist,
                        if *persist {
                            std::collections::BTreeSet::from([0])
                        } else {
                            Default::default()
                        },
                    )
                })
                .collect();
            let encoded = continuations
                .iter()
                .map(|value| bincode::serialize(value).unwrap())
                .collect();
            (
                TrieInsertAction::TrieInsertConsume(TrieInsertConsume {
                    hash: key,
                    continuations,
                }),
                encoded,
            )
        }
        PREFIX_JOINS => {
            let joins: Vec<_> = values
                .iter()
                .map(|(value, persist)| {
                    vec![format!("join-{value}"), format!("persistent-{persist}")]
                })
                .collect();
            let encoded = joins
                .iter()
                .map(|value| bincode::serialize(value).unwrap())
                .collect();
            (
                TrieInsertAction::TrieInsertJoins(TrieInsertJoins { hash: key, joins }),
                encoded,
            )
        }
        _ => panic!("invalid checkpoint fixture kind"),
    };
    encoded.sort();
    (HotStoreTrieAction::TrieInsertAction(insert), encoded)
}

fn checkpoint_binary_insert(kind: u8, channel: &String, values: Vec<Vec<u8>>) -> CheckpointAction {
    use rspace_plus_plus::rspace::hot_store_trie_action::{
        HotStoreTrieAction, TrieInsertAction, TrieInsertBinaryConsume, TrieInsertBinaryJoins,
        TrieInsertBinaryProduce,
    };

    let key = checkpoint_channel_key(kind, channel);
    HotStoreTrieAction::TrieInsertAction(match kind {
        PREFIX_DATUM => TrieInsertAction::TrieInsertBinaryProduce(TrieInsertBinaryProduce {
            hash: key,
            data: values,
        }),
        PREFIX_KONT => TrieInsertAction::TrieInsertBinaryConsume(TrieInsertBinaryConsume {
            hash: key,
            continuations: values,
        }),
        PREFIX_JOINS => TrieInsertAction::TrieInsertBinaryJoins(TrieInsertBinaryJoins {
            hash: key,
            joins: values,
        }),
        _ => panic!("invalid checkpoint fixture kind"),
    })
}

fn checkpoint_delete(kind: u8, channel: &String) -> CheckpointAction {
    use rspace_plus_plus::rspace::hot_store_trie_action::{
        HotStoreTrieAction, TrieDeleteAction, TrieDeleteConsume, TrieDeleteJoins, TrieDeleteProduce,
    };

    let key = checkpoint_channel_key(kind, channel);
    HotStoreTrieAction::TrieDeleteAction(match kind {
        PREFIX_DATUM => TrieDeleteAction::TrieDeleteProduce(TrieDeleteProduce { hash: key }),
        PREFIX_KONT => TrieDeleteAction::TrieDeleteConsume(TrieDeleteConsume { hash: key }),
        PREFIX_JOINS => TrieDeleteAction::TrieDeleteJoins(TrieDeleteJoins { hash: key }),
        _ => panic!("invalid checkpoint fixture kind"),
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, max_shrink_iters: 1_024, ..ProptestConfig::default() })]

    #[test]
    fn checkpoint_generated_grafts_preserve_typed_values_and_base_reads(
        seed in any::<u64>(),
        actions in prop::collection::vec((0usize..16, any::<bool>(), any::<u16>(), any::<bool>()), 1..25),
    ) {
        use rspace_plus_plus::rspace::hot_store_trie_action::{
            HotStoreTrieAction, TrieDeleteAction, TrieDeleteJoins, TrieInsertAction,
            TrieInsertBinaryJoins, TrieInsertJoins,
        };

        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let stores = Stores::pristine().await;
            let mut repository = HistoryRepositoryInstances::<String, String, String, String>::lmdb_repository(
                stores.history.clone(), stores.roots.clone(), stores.cold.clone(),
            ).unwrap();
            let channels: Vec<_> = (0..16).map(|index| format!("graft-{seed}-{index}")).collect();
            let mut expected = BTreeMap::new();
            let initial = channels.iter().enumerate().map(|(index, channel)| {
                let joins = vec![vec![format!("initial-{index}")]];
                expected.insert(channel.clone(), joins.clone());
                HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertJoins(
                    TrieInsertJoins { hash: hash(channel), joins },
                ))
            }).collect();
            repository = repository.do_checkpoint(initial);
            let mut retained = vec![(repository.root(), expected.clone())];
            for (index, delete, value, binary) in actions {
                let channel = &channels[index];
                let action = if delete {
                    expected.remove(channel);
                    HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteJoins(
                        TrieDeleteJoins { hash: hash(channel) },
                    ))
                } else {
                    let joins = vec![vec![format!("replacement-{value}")]];
                    expected.insert(channel.clone(), joins.clone());
                    let insert = if binary {
                        TrieInsertAction::TrieInsertBinaryJoins(TrieInsertBinaryJoins {
                            hash: hash(channel),
                            joins: joins.iter().map(|entry| bincode::serialize(entry).unwrap()).collect(),
                        })
                    } else {
                        TrieInsertAction::TrieInsertJoins(TrieInsertJoins { hash: hash(channel), joins })
                    };
                    HotStoreTrieAction::TrieInsertAction(insert)
                };
                let before = stores.snapshot();
                repository = repository.do_checkpoint(vec![action]);
                let root = repository.root();
                prop_assert!(repository.contains_root(&root).unwrap());
                let exported = stores.export_with_prefixes(&root);
                let actual: BTreeMap<_, _> = exported.leaf_prefixes.into_iter().zip(exported.leaf_values).collect();
                let expected_occurrences: BTreeMap<_, _> = expected.iter().map(|(channel, joins)| {
                    let mut path = vec![PREFIX_JOINS];
                    path.extend(hash(channel).bytes());
                    let leaf = JoinsLeaf { bytes: encode_joins(joins) };
                    (path, Blake2b256Hash::new(&bincode::serialize(&leaf).unwrap()).bytes())
                }).collect();
                prop_assert_eq!(actual, expected_occurrences);
                let after = stores.snapshot();
                for (key, value) in &before.history {
                    prop_assert_eq!(after.history.get(key), Some(value));
                }
                for (key, value) in &before.cold {
                    prop_assert_eq!(after.cold.get(key), Some(value));
                }
                retained.push((root, expected.clone()));
            }
            for (root, expected) in retained {
                for channel in &channels {
                    prop_assert_eq!(stores.read_joins(&root, channel), expected.get(channel).cloned().unwrap_or_default());
                }
            }
            Ok::<(), proptest::test_runner::TestCaseError>(())
        })?;
    }

    #[test]
    fn checkpoint_all_leaf_kinds_preserve_typed_binary_and_retained_root_equivalence(
        seed in any::<u64>(),
        actions in prop::collection::vec(
            (0usize..8, 0u8..3, any::<bool>(), prop::collection::vec((any::<u16>(), any::<bool>()), 1..5)),
            1..25,
        ),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let typed_store = Stores::new().await;
            let binary_store = Stores::new().await;
            let open = |stores: &Stores| HistoryRepositoryInstances::<String, String, String, String>::lmdb_repository(
                stores.history.clone(), stores.roots.clone(), stores.cold.clone(),
            ).unwrap();
            let mut typed = open(&typed_store);
            let mut binary = open(&binary_store);
            let channels: Vec<_> = (0..8).map(|index| format!("all-kinds-{seed}-{index}")).collect();
            let mut expected = BTreeMap::new();
            let mut initial_typed = Vec::new();
            let mut initial_binary = Vec::new();
            for (index, channel) in channels.iter().enumerate() {
                for kind in [PREFIX_DATUM, PREFIX_KONT, PREFIX_JOINS] {
                    let (action, values) = checkpoint_typed_insert(kind, channel, &[(index as u16, false), (99, true)]);
                    initial_typed.push(action);
                    initial_binary.push(checkpoint_binary_insert(kind, channel, values.clone()));
                    expected.insert((kind, index), values);
                }
            }
            typed = typed.do_checkpoint(initial_typed);
            binary = binary.do_checkpoint(initial_binary);
            prop_assert_eq!(typed.root(), binary.root());
            let mut retained = vec![(typed.root(), expected.clone())];
            for (index, kind, delete, values) in actions {
                let channel = &channels[index];
                let (typed_action, binary_action) = if delete {
                    expected.remove(&(kind, index));
                    (checkpoint_delete(kind, channel), checkpoint_delete(kind, channel))
                } else {
                    let (action, encoded) = checkpoint_typed_insert(kind, channel, &values);
                    expected.insert((kind, index), encoded.clone());
                    let mut reversed = encoded;
                    reversed.reverse();
                    (action, checkpoint_binary_insert(kind, channel, reversed))
                };
                typed = typed.do_checkpoint(vec![typed_action]);
                binary = binary.do_checkpoint(vec![binary_action]);
                prop_assert_eq!(typed.root(), binary.root());
                prop_assert_eq!(typed_store.snapshot(), binary_store.snapshot());
                let exported = typed_store.export_with_prefixes(&typed.root());
                let actual: BTreeMap<_, _> = exported.leaf_prefixes.into_iter().zip(exported.leaf_values).collect();
                let expected_occurrences: BTreeMap<_, _> = expected.iter().map(|((kind, index), values)| {
                    let mut path = vec![*kind];
                    path.extend(checkpoint_channel_key(*kind, &channels[*index]).bytes());
                    let leaf = DataLeaf { bytes: bincode::serialize(values).unwrap() };
                    (path, Blake2b256Hash::new(&bincode::serialize(&leaf).unwrap()).bytes())
                }).collect();
                prop_assert_eq!(actual, expected_occurrences);
                retained.push((typed.root(), expected.clone()));
            }
            for (root, expected) in retained {
                let typed_reader = typed.get_history_reader(&root).unwrap();
                let binary_reader = binary.get_history_reader(&root).unwrap();
                for (index, channel) in channels.iter().enumerate() {
                    for kind in [PREFIX_DATUM, PREFIX_KONT, PREFIX_JOINS] {
                        let key = checkpoint_channel_key(kind, channel);
                        let expected = expected.get(&(kind, index)).cloned().unwrap_or_default();
                        for reader in [&*typed_reader, &*binary_reader] {
                            let (actual, raw): (Vec<Vec<u8>>, _) = match kind {
                                PREFIX_DATUM => (
                                    reader.get_data(&key).unwrap().iter().map(|value| bincode::serialize(value).unwrap()).collect(),
                                    reader.get_data_proj_binary(&key).unwrap(),
                                ),
                                PREFIX_KONT => (
                                    reader.get_continuations(&key).unwrap().iter().map(|value| bincode::serialize(value).unwrap()).collect(),
                                    reader.get_continuations_proj_binary(&key).unwrap(),
                                ),
                                PREFIX_JOINS => (
                                    reader.get_joins(&key).unwrap().iter().map(|value| bincode::serialize(value).unwrap()).collect(),
                                    reader.get_joins_proj_binary(&key).unwrap(),
                                ),
                                _ => unreachable!(),
                            };
                            prop_assert_eq!(&actual, &expected);
                            prop_assert_eq!(&raw, &expected);
                        }
                    }
                }
            }
            Ok::<(), proptest::test_runner::TestCaseError>(())
        })?;
    }
}
