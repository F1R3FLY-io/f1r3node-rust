use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use casper::rust::casper::MultiParentCasper;
use casper::rust::engine::engine::Engine;
use casper::rust::engine::lfs_horizon_requester::HasRootFn;
use casper::rust::engine::lfs_tuple_space_requester::{StatePartPath, PAGE_SIZE};
use casper::rust::engine::running::Running;
use casper::rust::engine::runtime_state_requester;
use casper::rust::errors::CasperError;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::test_instances::TransportLayerStub;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, ApprovedBlockCandidate, CasperMessage, StoreItemsMessage,
};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::cold_store::{
    ContinuationsLeaf, DataLeaf, JoinsLeaf, PersistedData,
};
use rspace_plus_plus::rspace::history::roots_store::{RootsStore, RootsStoreInstances};
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::instances::rspace_exporter_store::RSpaceExporterStore;
use rspace_plus_plus::rspace::state::instances::rspace_importer_store::RSpaceImporterImpl;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use tokio::sync::mpsc;

use crate::engine::setup::TestFixture;
use crate::util::rholang::resources::{generate_scope_id, mk_test_rnode_store_manager_shared};

pub(crate) fn genesis_root(fixture: &TestFixture) -> Blake2b256Hash {
    Blake2b256Hash::from_bytes_prost(&fixture.genesis.body.state.post_state_hash)
}

fn bootstrap(fixture: &TestFixture) -> PeerNode {
    fixture
        .rp_conf_ask
        .bootstrap
        .clone()
        .expect("the fixture has a bootstrap")
}

fn stranger() -> PeerNode {
    PeerNode {
        id: NodeIdentifier {
            key: Bytes::from_static(b"state-requester-not-the-bootstrap"),
        },
        endpoint: Endpoint::new("stranger.invalid".to_string(), 40400, 40404),
    }
}

pub(crate) fn export_chunks(
    store: &RSpaceStore,
    root: &Blake2b256Hash,
    page_size: i32,
) -> Result<Vec<StoreItemsMessage>, String> {
    let exporter = Arc::new(RSpaceExporterStore::create(
        store.history.clone(),
        store.cold.clone(),
        store.roots.clone(),
    ));
    let mut start_path: StatePartPath = vec![(root.clone(), None)];
    let mut chunks = Vec::new();
    loop {
        let (history, data) = RSpaceExporterItems::get_history_and_data(
            exporter.clone(),
            start_path.clone(),
            0,
            page_size,
        )
        .map_err(|e| e.to_string())?;
        let last_path = history.last_path.clone();
        chunks.push(StoreItemsMessage {
            start_path: start_path.clone(),
            last_path: last_path.clone(),
            history_items: history
                .items
                .into_iter()
                .map(|(k, v)| (k, Bytes::from(v)))
                .collect(),
            data_items: data
                .items
                .into_iter()
                .map(|(k, v)| (k, Bytes::from(v)))
                .collect(),
        });
        if last_path.len() <= 1 || last_path == start_path {
            return Ok(chunks);
        }
        start_path = last_path;
        if chunks.len() >= 1024 {
            return Err("too many chunks".to_string());
        }
    }
}

pub(crate) fn history_keys(chunks: &[StoreItemsMessage]) -> std::collections::BTreeSet<Vec<u8>> {
    chunks
        .iter()
        .flat_map(|chunk| chunk.history_items.iter().map(|(key, _)| key.bytes()))
        .collect()
}

pub(crate) async fn private_store() -> RSpaceStore {
    let mut kvm = mk_test_rnode_store_manager_shared(generate_scope_id());
    (*kvm).r_space_stores().await.expect("private rspace store")
}

pub(crate) fn importer_and_has_root(store: &RSpaceStore) -> (Arc<dyn RSpaceImporter>, HasRootFn) {
    let importer: Arc<dyn RSpaceImporter> = Arc::new(RSpaceImporterImpl {
        history_store: store.history.clone(),
        value_store: store.cold.clone(),
        roots_store: store.roots.clone(),
    });
    let roots = store.roots.clone();
    let has_root: HasRootFn = Arc::new(move |root| {
        RootsStoreInstances::roots_store(roots.clone())
            .contains_root(root)
            .map_err(|e| CasperError::RuntimeError(format!("{:?}", e)))
    });
    (importer, has_root)
}

pub(crate) fn seed(
    importer: &Arc<dyn RSpaceImporter>,
    root: &Blake2b256Hash,
    chunks: &[StoreItemsMessage],
) {
    for chunk in chunks {
        importer.set_history_items(
            chunk
                .history_items
                .iter()
                .map(|(k, v)| (k.clone(), v.to_vec()))
                .collect(),
        );
        importer.set_data_items(
            chunk
                .data_items
                .iter()
                .map(|(k, v)| (k.clone(), v.to_vec()))
                .collect(),
        );
    }
    importer.set_root(root);
}

pub(crate) fn with_a_swapped_leaf_kind(
    chunks: &[StoreItemsMessage],
) -> (Vec<StoreItemsMessage>, Blake2b256Hash, Bytes) {
    let mut forged = chunks.to_vec();
    let chunk = forged
        .iter_mut()
        .find(|chunk| !chunk.data_items.is_empty())
        .expect("the state has data items");
    let (key, original) = chunk.data_items[0].clone();
    let swapped = match bincode::deserialize::<PersistedData>(&original).unwrap() {
        PersistedData::Data(DataLeaf { bytes }) => {
            PersistedData::Continuations(ContinuationsLeaf { bytes })
        }
        PersistedData::Continuations(ContinuationsLeaf { bytes }) => {
            PersistedData::Joins(JoinsLeaf { bytes })
        }
        PersistedData::Joins(JoinsLeaf { bytes }) => PersistedData::Data(DataLeaf { bytes }),
    };
    chunk.data_items[0].1 = Bytes::from(bincode::serialize(&swapped).unwrap());
    (forged, key, original)
}

pub(crate) fn a_redirect_to_malformed_bytes(first: &StoreItemsMessage) -> [StoreItemsMessage; 2] {
    let malformed = Bytes::from_static(&[0x00, 0x05, 0xAA]);
    let malformed_hash = Blake2b256Hash::new(&malformed);
    let zero = Blake2b256Hash::from_bytes(vec![0; 32]);
    let mut target: StatePartPath = vec![(malformed_hash.clone(), None)];
    target.extend(std::iter::repeat_n((zero, None), 5));
    target.push((malformed_hash.clone(), None));
    let mut redirected = first.clone();
    redirected.last_path = target.clone();
    let malformed_chunk = StoreItemsMessage {
        start_path: target.clone(),
        last_path: target.clone(),
        history_items: vec![(malformed_hash, malformed)],
        data_items: vec![],
    };
    [redirected, malformed_chunk]
}

fn running(
    fixture: &TestFixture,
    items_tx: mpsc::Sender<StoreItemsMessage>,
) -> Running<TransportLayerStub> {
    Running::new(
        fixture.block_processing_queue_tx.clone(),
        fixture.blocks_in_processing.clone(),
        Arc::new(fixture.casper.clone()) as Arc<dyn MultiParentCasper + Send + Sync>,
        ApprovedBlock {
            candidate: ApprovedBlockCandidate {
                block: fixture.genesis.clone(),
                required_sigs: 0,
            },
            floor_seed: None,
            sigs: Vec::new(),
        },
        Arc::new(|| {
            Box::pin(async { Ok(()) })
                as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
        }),
        false,
        fixture.transport_layer.clone(),
        fixture.rp_conf_ask.clone(),
        fixture.block_retriever.clone(),
        Some(items_tx),
    )
}

pub(crate) async fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {}", what);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn wait_until_handled(
    fixture: &TestFixture,
    handles: &runtime_state_requester::StateRequesterHandles,
    tag: &[u8],
) {
    let before = fixture.transport_layer.request_count();
    handles
        .fetch_tx
        .send(Blake2b256Hash::new(tag))
        .await
        .expect("requester is running");
    wait_for("the requester to handle the chunk", || {
        fixture.transport_layer.request_count() > before
    })
    .await;
}

#[tokio::test]
async fn honest_chunks_for_a_missing_root_are_imported_and_recorded() {
    let fixture = TestFixture::new().await;
    fixture.transport_layer.reset();
    let root = genesis_root(&fixture);
    let honest =
        export_chunks(&fixture.rspace_store, &root, PAGE_SIZE).expect("genesis state exports");

    let store = private_store().await;
    let (importer, has_root) = importer_and_has_root(&store);
    let handles = runtime_state_requester::spawn(
        fixture.transport_layer.clone(),
        fixture.rp_conf_ask.clone(),
        importer,
        has_root.clone(),
    );
    let engine = running(&fixture, handles.items_tx.clone());

    handles.fetch_tx.send(root.clone()).await.unwrap();
    wait_for("the root request", || {
        fixture.transport_layer.request_count() > 0
    })
    .await;
    for chunk in honest.iter().cloned() {
        engine
            .handle(bootstrap(&fixture), CasperMessage::StoreItemsMessage(chunk))
            .await
            .unwrap();
    }

    wait_for("the root to be recorded", || has_root(&root).unwrap()).await;
    let imported = export_chunks(&store, &root, PAGE_SIZE).expect("the imported state reads back");
    assert_eq!(
        history_keys(&imported),
        history_keys(&honest),
        "the imported trie is the trie the bootstrap served"
    );
    for (key, value) in honest.iter().flat_map(|chunk| chunk.data_items.iter()) {
        assert_eq!(
            store.cold.get_one(&key.bytes()).unwrap(),
            Some(value.to_vec()),
            "a data item the bootstrap served is missing"
        );
    }
}

#[tokio::test]
async fn a_forged_chunk_does_not_overwrite_held_state_or_record_its_root() {
    let fixture = TestFixture::new().await;
    fixture.transport_layer.reset();
    let held_root = genesis_root(&fixture);
    let honest =
        export_chunks(&fixture.rspace_store, &held_root, PAGE_SIZE).expect("genesis state exports");

    let store = private_store().await;
    let (importer, has_root) = importer_and_has_root(&store);
    seed(&importer, &held_root, &honest);
    let held_node = store
        .history
        .get_one(&held_root.bytes())
        .unwrap()
        .expect("the store holds the root node");

    let handles = runtime_state_requester::spawn(
        fixture.transport_layer.clone(),
        fixture.rp_conf_ask.clone(),
        importer,
        has_root.clone(),
    );
    let engine = running(&fixture, handles.items_tx.clone());

    let absent_root = Blake2b256Hash::new(b"a root this node does not hold");
    handles.fetch_tx.send(absent_root.clone()).await.unwrap();
    wait_for("the root request", || {
        fixture.transport_layer.request_count() > 0
    })
    .await;

    let path: StatePartPath = vec![(absent_root.clone(), None)];
    let forged = StoreItemsMessage {
        start_path: path.clone(),
        last_path: path,
        history_items: vec![(held_root.clone(), Bytes::from_static(b"garbage"))],
        data_items: vec![],
    };
    engine
        .handle(
            bootstrap(&fixture),
            CasperMessage::StoreItemsMessage(forged),
        )
        .await
        .unwrap();
    wait_until_handled(&fixture, &handles, b"sentinel").await;

    assert_eq!(
        store.history.get_one(&held_root.bytes()).unwrap(),
        Some(held_node),
        "a forged item overwrote a history node the store already held"
    );
    assert!(
        export_chunks(&store, &held_root, PAGE_SIZE).is_ok(),
        "the held state no longer reads back from the store"
    );
    assert!(
        !has_root(&absent_root).unwrap(),
        "the absent root was recorded although none of its nodes arrived"
    );
}

#[tokio::test]
async fn a_chunk_with_a_swapped_leaf_kind_does_not_overwrite_a_held_leaf() {
    let fixture = TestFixture::new().await;
    fixture.transport_layer.reset();
    let root = genesis_root(&fixture);
    let honest =
        export_chunks(&fixture.rspace_store, &root, PAGE_SIZE).expect("genesis state exports");
    let (forged, key, original) = with_a_swapped_leaf_kind(&honest);

    let store = private_store().await;
    let (importer, has_root) = importer_and_has_root(&store);
    importer.set_data_items(vec![(key.clone(), original.to_vec())]);
    let handles = runtime_state_requester::spawn(
        fixture.transport_layer.clone(),
        fixture.rp_conf_ask.clone(),
        importer,
        has_root.clone(),
    );
    let engine = running(&fixture, handles.items_tx.clone());

    handles.fetch_tx.send(root.clone()).await.unwrap();
    wait_for("the root request", || {
        fixture.transport_layer.request_count() > 0
    })
    .await;
    for chunk in forged {
        engine
            .handle(bootstrap(&fixture), CasperMessage::StoreItemsMessage(chunk))
            .await
            .unwrap();
    }
    wait_until_handled(&fixture, &handles, b"sentinel").await;

    assert_eq!(
        store.cold.get_one(&key.bytes()).unwrap(),
        Some(original.to_vec()),
        "a data item with a changed leaf kind overwrote a leaf the store held"
    );
    assert!(!has_root(&root).unwrap());
}

#[tokio::test]
async fn a_last_path_redirected_to_malformed_bytes_does_not_stop_the_requester() {
    let fixture = TestFixture::new().await;
    fixture.transport_layer.reset();
    let root = genesis_root(&fixture);
    let honest =
        export_chunks(&fixture.rspace_store, &root, PAGE_SIZE).expect("genesis state exports");

    let store = private_store().await;
    let (importer, has_root) = importer_and_has_root(&store);
    let handles = runtime_state_requester::spawn(
        fixture.transport_layer.clone(),
        fixture.rp_conf_ask.clone(),
        importer,
        has_root.clone(),
    );
    let engine = running(&fixture, handles.items_tx.clone());

    handles.fetch_tx.send(root.clone()).await.unwrap();
    wait_for("the root request", || {
        fixture.transport_layer.request_count() > 0
    })
    .await;
    let chunks = a_redirect_to_malformed_bytes(&honest[0])
        .into_iter()
        .chain(honest.iter().cloned());
    for chunk in chunks {
        engine
            .handle(bootstrap(&fixture), CasperMessage::StoreItemsMessage(chunk))
            .await
            .unwrap();
    }

    wait_for("the root to be recorded from the honest chunks", || {
        has_root(&root).unwrap()
    })
    .await;
}

#[tokio::test]
async fn store_items_from_a_peer_that_was_not_asked_are_dropped() {
    let fixture = TestFixture::new().await;
    let (items_tx, mut items_rx) = mpsc::channel(8);
    let engine = running(&fixture, items_tx);

    let path: StatePartPath = vec![(Blake2b256Hash::new(b"some root"), None)];
    let chunk = StoreItemsMessage {
        start_path: path.clone(),
        last_path: path,
        history_items: vec![],
        data_items: vec![],
    };
    engine
        .handle(stranger(), CasperMessage::StoreItemsMessage(chunk.clone()))
        .await
        .unwrap();
    assert!(
        items_rx.try_recv().is_err(),
        "a chunk from a peer that was not asked reached the state requester"
    );

    engine
        .handle(
            bootstrap(&fixture),
            CasperMessage::StoreItemsMessage(chunk.clone()),
        )
        .await
        .unwrap();
    assert_eq!(items_rx.try_recv().ok(), Some(chunk));
}
