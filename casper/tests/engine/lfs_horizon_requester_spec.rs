use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use casper::rust::engine::lfs_horizon_requester::{
    self, HorizonProgress, HorizonRequesterOps, MAX_IN_FLIGHT_REQUESTS, PAGE_SIZE,
};
use casper::rust::engine::lfs_tuple_space_requester::StatePartPath;
use casper::rust::errors::CasperError;
use futures::StreamExt;
use models::rust::casper::protocol::casper_message::StoreItemsMessage;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use tokio::sync::mpsc;

use crate::engine::runtime_state_requester_spec::{
    a_redirect_to_malformed_bytes, export_chunks, export_page, genesis_root, history_keys,
    importer_and_has_root, private_store, seed, wait_for, with_a_swapped_leaf_kind,
};
use crate::engine::setup::TestFixture;

#[derive(Clone, Default)]
struct RecordingOps {
    requested: Arc<Mutex<Vec<StatePartPath>>>,
}

impl RecordingOps {
    fn requests_for(&self, path: &StatePartPath) -> usize {
        self.requested
            .lock()
            .unwrap()
            .iter()
            .filter(|p| *p == path)
            .count()
    }
}

#[async_trait]
impl HorizonRequesterOps for RecordingOps {
    async fn request_for_horizon_chunk(
        &self,
        path: &StatePartPath,
        _page_size: i32,
    ) -> Result<(), CasperError> {
        self.requested.lock().unwrap().push(path.clone());
        Ok(())
    }
}

const EXPORT_PAGE: i32 = 1024;

type Outcome = (Option<HorizonProgress>, Option<CasperError>);

async fn drive(
    roots: Vec<Blake2b256Hash>,
    store: &RSpaceStore,
    ops: RecordingOps,
    items_rx: mpsc::Receiver<StoreItemsMessage>,
    request_timeout: Duration,
    overall_deadline: Duration,
) -> tokio::task::JoinHandle<Outcome> {
    let (importer, has_root) = importer_and_has_root(store);
    let (stream, err) = lfs_horizon_requester::stream(
        roots,
        has_root,
        importer,
        ops,
        items_rx,
        request_timeout,
        overall_deadline,
    )
    .await
    .expect("horizon stream starts");
    tokio::spawn(async move {
        let mut stream = Box::pin(stream);
        let mut last = None;
        while let Some(st) = stream.next().await {
            last = Some(st);
        }
        let error = err.lock().unwrap().take();
        (last, error)
    })
}

fn serve(
    store: &RSpaceStore,
    ops: &RecordingOps,
    items_tx: mpsc::Sender<StoreItemsMessage>,
    mut change: impl FnMut(StoreItemsMessage) -> Vec<StoreItemsMessage> + Send + 'static,
) -> tokio::task::JoinHandle<usize> {
    let store = store.clone();
    let ops = ops.clone();
    tokio::spawn(async move {
        let mut served = 0;
        let mut max_outstanding = 0;
        loop {
            let sent: Vec<StatePartPath> = ops.requested.lock().unwrap()[served..].to_vec();
            max_outstanding = max_outstanding.max(sent.len());
            for path in sent {
                let page =
                    export_page(&store, &path, PAGE_SIZE).expect("the server holds the path");
                for message in change(page) {
                    if items_tx.send(message).await.is_err() {
                        return max_outstanding;
                    }
                }
                served += 1;
            }
            if items_tx.is_closed() {
                return max_outstanding;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
}

fn data_keys(chunks: &[StoreItemsMessage]) -> Vec<Blake2b256Hash> {
    chunks
        .iter()
        .flat_map(|chunk| chunk.data_items.iter().map(|(key, _)| key.clone()))
        .collect()
}

fn items_in(chunks: &[StoreItemsMessage]) -> usize {
    chunks
        .iter()
        .map(|chunk| chunk.history_items.len() + chunk.data_items.len())
        .sum()
}

fn forged_chunk(absent_root: &Blake2b256Hash, held_root: &Blake2b256Hash) -> StoreItemsMessage {
    let path: StatePartPath = vec![(absent_root.clone(), None)];
    StoreItemsMessage {
        start_path: path.clone(),
        last_path: path,
        history_items: vec![(held_root.clone(), Bytes::from_static(b"garbage"))],
        data_items: vec![],
    }
}

#[tokio::test]
async fn honest_horizon_chunks_import_a_missing_root() {
    let fixture = TestFixture::new().await;
    let root = genesis_root(&fixture);
    let honest =
        export_chunks(&fixture.rspace_store, &root, EXPORT_PAGE).expect("genesis state exports");

    let store = private_store().await;
    let (_, has_root) = importer_and_has_root(&store);
    let ops = RecordingOps::default();
    let (items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![root.clone()],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_secs(10),
        Duration::from_secs(60),
    )
    .await;

    let first: StatePartPath = vec![(root.clone(), None)];
    wait_for("the root request", || ops.requests_for(&first) > 0).await;
    serve(&fixture.rspace_store, &ops, items_tx, |page| vec![page]);

    let (final_state, error) = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the horizon sync ends")
        .unwrap();
    assert!(error.is_none(), "honest sync failed: {:?}", error);
    assert!(final_state.expect("a final state").is_finished());
    assert!(has_root(&root).unwrap());
    let imported =
        export_chunks(&store, &root, EXPORT_PAGE).expect("the imported state reads back");
    assert_eq!(history_keys(&imported), history_keys(&honest));
}

#[tokio::test]
async fn a_forged_horizon_chunk_writes_nothing_and_is_asked_again() {
    let fixture = TestFixture::new().await;
    let held_root = genesis_root(&fixture);
    let honest = export_chunks(&fixture.rspace_store, &held_root, EXPORT_PAGE)
        .expect("genesis state exports");

    let store = private_store().await;
    let (importer, has_root) = importer_and_has_root(&store);
    seed(&importer, &held_root, &honest);
    let held_node = store.history.get_one(&held_root.bytes()).unwrap();
    assert!(held_node.is_some(), "the store holds the root node");

    let absent_root = Blake2b256Hash::new(b"a horizon root this node does not hold");
    let path: StatePartPath = vec![(absent_root.clone(), None)];
    let ops = RecordingOps::default();
    let (items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![absent_root.clone()],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_millis(100),
        Duration::from_secs(2),
    )
    .await;

    wait_for("the root request", || ops.requests_for(&path) > 0).await;
    let before = ops.requests_for(&path);
    items_tx
        .send(forged_chunk(&absent_root, &held_root))
        .await
        .unwrap();
    wait_for("the chunk to be handled", || {
        ops.requests_for(&path) > before || run.is_finished()
    })
    .await;

    assert_eq!(
        store.history.get_one(&held_root.bytes()).unwrap(),
        held_node,
        "a forged item overwrote a history node the store already held"
    );
    assert!(
        !has_root(&absent_root).unwrap(),
        "the absent root was recorded although none of its nodes arrived"
    );
    assert!(
        ops.requests_for(&path) > before,
        "the rejected path was not asked again"
    );

    let (final_state, error) = tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .expect("the horizon sync ends at its deadline")
        .unwrap();
    assert!(!final_state.expect("a final state").is_finished());
    assert!(
        error.is_some(),
        "a sync with no progress must end with an error"
    );
}

#[tokio::test]
async fn chunks_that_are_not_imported_do_not_hold_off_the_deadline() {
    let store = private_store().await;
    let absent_root = Blake2b256Hash::new(b"another horizon root this node does not hold");
    let path: StatePartPath = vec![(absent_root.clone(), None)];
    let ops = RecordingOps::default();
    let (items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![absent_root.clone()],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_millis(200),
        Duration::from_secs(1),
    )
    .await;
    wait_for("the root request", || ops.requests_for(&path) > 0).await;

    let unasked: StatePartPath = vec![(Blake2b256Hash::new(b"a path nobody asked for"), None)];
    let stale = StoreItemsMessage {
        start_path: unasked.clone(),
        last_path: unasked,
        history_items: vec![],
        data_items: vec![],
    };
    let sender = tokio::spawn(async move {
        loop {
            if items_tx.send(stale.clone()).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    let outcome = tokio::time::timeout(Duration::from_secs(10), run).await;
    sender.abort();
    let (final_state, error) = outcome
        .expect("chunks that were not imported held off the no-progress deadline")
        .unwrap();
    assert!(!final_state.expect("a final state").is_finished());
    assert!(error.is_some());
}

#[tokio::test]
async fn a_horizon_chunk_with_a_swapped_leaf_kind_does_not_overwrite_a_held_leaf() {
    let fixture = TestFixture::new().await;
    let root = genesis_root(&fixture);
    let honest =
        export_chunks(&fixture.rspace_store, &root, EXPORT_PAGE).expect("genesis state exports");
    let (_, key, original) = with_a_swapped_leaf_kind(&honest);

    let store = private_store().await;
    let (importer, has_root) = importer_and_has_root(&store);
    importer.set_data_items(vec![(key.clone(), original.to_vec())]);
    let absent = importer
        .has_data_items(&data_keys(&honest))
        .iter()
        .any(|held| !held);
    assert!(absent, "the state has a leaf besides the held one");
    let ops = RecordingOps::default();
    let (items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![root.clone()],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_millis(200),
        Duration::from_secs(2),
    )
    .await;

    let first: StatePartPath = vec![(root.clone(), None)];
    wait_for("the root request", || ops.requests_for(&first) > 0).await;
    serve(&fixture.rspace_store, &ops, items_tx, |page| {
        if page.data_items.is_empty() {
            vec![page]
        } else {
            with_a_swapped_leaf_kind(&[page]).0
        }
    });

    let (_, error) = tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .expect("the horizon sync ends")
        .unwrap();
    assert_eq!(
        store.cold.get_one(&key.bytes()).unwrap(),
        Some(original.to_vec()),
        "a data item with a changed leaf kind overwrote a leaf the store held"
    );
    assert!(!has_root(&root).unwrap());
    assert!(error.is_some());
}

#[tokio::test]
async fn a_horizon_last_path_redirected_to_malformed_bytes_is_rejected() {
    let fixture = TestFixture::new().await;
    let root = genesis_root(&fixture);
    let store = private_store().await;
    let (_, has_root) = importer_and_has_root(&store);
    let ops = RecordingOps::default();
    let (items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![root.clone()],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_millis(200),
        Duration::from_secs(10),
    )
    .await;

    let first: StatePartPath = vec![(root.clone(), None)];
    wait_for("the root request", || ops.requests_for(&first) > 0).await;
    let mut redirected = false;
    serve(&fixture.rspace_store, &ops, items_tx, move |page| {
        if redirected {
            return vec![page];
        }
        redirected = true;
        a_redirect_to_malformed_bytes(&page).into()
    });

    let (final_state, error) = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the horizon sync ends")
        .expect("the horizon sync does not panic");
    assert!(
        error.is_none(),
        "the honest chunks did not complete the sync: {:?}",
        error
    );
    assert!(final_state.expect("a final state").is_finished());
    assert!(has_root(&root).unwrap());
}

#[tokio::test]
async fn two_empty_horizon_chunks_do_not_record_an_absent_root() {
    let store = private_store().await;
    let (_, has_root) = importer_and_has_root(&store);
    let absent_root = Blake2b256Hash::new(b"a third horizon root this node does not hold");
    let first: StatePartPath = vec![(absent_root.clone(), None)];
    let continuation: StatePartPath =
        vec![(Blake2b256Hash::new(b"a continuation nobody walked"), None)];
    let ops = RecordingOps::default();
    let (items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![absent_root.clone()],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_millis(200),
        Duration::from_secs(2),
    )
    .await;
    wait_for("the root request", || ops.requests_for(&first) > 0).await;

    for (start_path, last_path) in [
        (first.clone(), continuation.clone()),
        (continuation.clone(), continuation),
    ] {
        items_tx
            .send(StoreItemsMessage {
                start_path,
                last_path,
                history_items: vec![],
                data_items: vec![],
            })
            .await
            .unwrap();
    }

    let (final_state, error) = tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .expect("the horizon sync ends")
        .unwrap();
    assert!(
        !has_root(&absent_root).unwrap(),
        "two empty chunks recorded a root with no trie"
    );
    assert!(!final_state.expect("a final state").is_finished());
    assert!(error.is_some());
}

#[tokio::test]
async fn only_the_nodes_and_data_a_node_lacks_are_requested() {
    let fixture = TestFixture::new().await;
    let root = genesis_root(&fixture);
    let honest = export_chunks(&fixture.rspace_store, &root, EXPORT_PAGE).unwrap();

    let store = private_store().await;
    let (importer, has_root) = importer_and_has_root(&store);
    let lacking_node = honest
        .iter()
        .flat_map(|chunk| chunk.history_items.iter())
        .map(|(key, _)| key.clone())
        .find(|key| *key != root)
        .expect("the state has more than one node");
    let lacking_data = data_keys(&honest)[0].clone();
    for chunk in &honest {
        importer.set_history_items(
            chunk
                .history_items
                .iter()
                .filter(|(key, _)| *key != lacking_node)
                .map(|(key, bytes)| (key.clone(), bytes.to_vec()))
                .collect(),
        );
        importer.set_data_items(
            chunk
                .data_items
                .iter()
                .filter(|(key, _)| *key != lacking_data)
                .map(|(key, bytes)| (key.clone(), bytes.to_vec()))
                .collect(),
        );
    }

    let ops = RecordingOps::default();
    let (items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![root.clone()],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_secs(10),
        Duration::from_secs(60),
    )
    .await;
    serve(&fixture.rspace_store, &ops, items_tx, |page| vec![page]);

    let (final_state, error) = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the horizon sync ends")
        .unwrap();
    assert!(error.is_none(), "sync failed: {:?}", error);
    assert!(final_state.expect("a final state").is_finished());
    assert!(has_root(&root).unwrap());
    assert!(ops.requested.lock().unwrap().len() <= 2);
    let imported = export_chunks(&store, &root, EXPORT_PAGE).unwrap();
    assert_eq!(items_in(&imported), items_in(&honest));
}

#[tokio::test]
async fn a_root_whose_state_is_already_held_is_recorded_without_a_request() {
    let fixture = TestFixture::new().await;
    let root = genesis_root(&fixture);
    let honest = export_chunks(&fixture.rspace_store, &root, EXPORT_PAGE).unwrap();
    let store = private_store().await;
    let (importer, has_root) = importer_and_has_root(&store);
    for chunk in &honest {
        importer.set_history_items(
            chunk
                .history_items
                .iter()
                .map(|(key, bytes)| (key.clone(), bytes.to_vec()))
                .collect(),
        );
        importer.set_data_items(
            chunk
                .data_items
                .iter()
                .map(|(key, bytes)| (key.clone(), bytes.to_vec()))
                .collect(),
        );
    }
    let ops = RecordingOps::default();
    let (_items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![root.clone()],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_secs(10),
        Duration::from_secs(60),
    )
    .await;
    let (final_state, error) = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the horizon sync ends")
        .unwrap();
    assert!(error.is_none());
    assert!(final_state.expect("a final state").is_finished());
    assert!(has_root(&root).unwrap());
    assert!(ops.requested.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_recorded_root_is_not_requested() {
    let fixture = TestFixture::new().await;
    let root = genesis_root(&fixture);
    let honest = export_chunks(&fixture.rspace_store, &root, EXPORT_PAGE).unwrap();
    let store = private_store().await;
    let (importer, _) = importer_and_has_root(&store);
    seed(&importer, &root, &honest);
    let ops = RecordingOps::default();
    let (_items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![root],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_secs(10),
        Duration::from_secs(60),
    )
    .await;
    let (final_state, error) = run.await.unwrap();
    let final_state = final_state.expect("a final state");
    assert!(error.is_none());
    assert!(final_state.is_finished());
    assert_eq!(final_state.len(), 0);
    assert!(ops.requested.lock().unwrap().is_empty());
}

#[tokio::test]
async fn at_most_max_in_flight_requests_are_outstanding() {
    let store = private_store().await;
    let roots: Vec<Blake2b256Hash> = (0..3 * MAX_IN_FLIGHT_REQUESTS)
        .map(|i| Blake2b256Hash::new(&i.to_be_bytes()))
        .collect();
    let ops = RecordingOps::default();
    let (_items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        roots,
        &store,
        ops.clone(),
        items_rx,
        Duration::from_secs(10),
        Duration::from_millis(500),
    )
    .await;
    let (final_state, error) = run.await.unwrap();
    assert!(!final_state.expect("a final state").is_finished());
    assert!(error.is_some());
    assert_eq!(ops.requested.lock().unwrap().len(), MAX_IN_FLIGHT_REQUESTS);
}

#[tokio::test]
async fn an_empty_answer_for_a_root_ends_the_sync() {
    let store = private_store().await;
    let root = Blake2b256Hash::new(b"a root the bootstrap does not hold");
    let path: StatePartPath = vec![(root.clone(), None)];
    let ops = RecordingOps::default();
    let (items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![root],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_secs(10),
        Duration::from_secs(60),
    )
    .await;
    wait_for("the root request", || ops.requests_for(&path) > 0).await;
    items_tx
        .send(StoreItemsMessage {
            start_path: path.clone(),
            last_path: path,
            history_items: vec![],
            data_items: vec![],
        })
        .await
        .unwrap();
    let (final_state, error) = tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .expect("the sync ends on the empty answer")
        .unwrap();
    assert!(!final_state.expect("a final state").is_finished());
    assert!(error.is_some());
}

#[tokio::test]
async fn a_lost_response_is_asked_again_with_backoff() {
    let fixture = TestFixture::new().await;
    let root = genesis_root(&fixture);
    let lost: StatePartPath = vec![(root.clone(), None)];
    let store = private_store().await;
    let (_, has_root) = importer_and_has_root(&store);
    let ops = RecordingOps::default();
    let (items_tx, items_rx) = mpsc::channel(64);
    let run = drive(
        vec![root.clone()],
        &store,
        ops.clone(),
        items_rx,
        Duration::from_millis(50),
        Duration::from_secs(20),
    )
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let sends = ops.requests_for(&lost);
    assert!(sends >= 2, "the lost path was sent {} time(s)", sends);
    assert!(
        sends <= 8,
        "the lost path was sent {} times in one second",
        sends
    );

    serve(&fixture.rspace_store, &ops, items_tx, |page| vec![page]);
    let (final_state, error) = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the horizon sync ends")
        .unwrap();
    assert!(error.is_none(), "sync failed: {:?}", error);
    assert!(final_state.expect("a final state").is_finished());
    assert!(has_root(&root).unwrap());
}
