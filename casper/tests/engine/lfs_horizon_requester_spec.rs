use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use casper::rust::engine::lfs_horizon_requester::{self, HorizonRequesterOps, ST};
use casper::rust::engine::lfs_tuple_space_requester::StatePartPath;
use casper::rust::errors::CasperError;
use futures::StreamExt;
use models::rust::casper::protocol::casper_message::StoreItemsMessage;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use tokio::sync::mpsc;

use crate::engine::runtime_state_requester_spec::{
    export_chunks, genesis_root, history_keys, importer_and_has_root, private_store, seed, wait_for,
};
use crate::engine::setup::TestFixture;

/// Records the chunk paths the requester asks for. It keeps the default
/// `validate_horizon_chunk`, so chunks go through the real validation.
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

type Outcome = (Option<ST<StatePartPath>>, Option<CasperError>);

async fn drive(
    roots: Vec<Blake2b256Hash>,
    store: &rspace_plus_plus::rspace::rspace::RSpaceStore,
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

fn forged_chunk(absent_root: &Blake2b256Hash, held_root: &Blake2b256Hash) -> StoreItemsMessage {
    let path: StatePartPath = vec![(absent_root.clone(), None)];
    StoreItemsMessage {
        start_path: path.clone(),
        last_path: path,
        history_items: vec![(held_root.clone(), Bytes::from_static(b"garbage"))],
        data_items: vec![],
    }
}

/// The honest path the fix must keep: a joiner imports the chunks the
/// bootstrap serves for a horizon root, and records the root.
#[tokio::test]
async fn honest_horizon_chunks_import_a_missing_root() {
    let fixture = TestFixture::new().await;
    let root = genesis_root(&fixture);
    let honest = export_chunks(
        &fixture.rspace_store,
        &root,
        lfs_horizon_requester::PAGE_SIZE,
    )
    .expect("genesis state exports");

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
    for chunk in honest.iter().cloned() {
        items_tx.send(chunk).await.unwrap();
    }

    let (final_state, error) = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the horizon sync ends")
        .unwrap();
    assert!(error.is_none(), "honest sync failed: {:?}", error);
    assert!(final_state.expect("a final state").is_finished());
    assert!(has_root(&root).unwrap());
    let imported = export_chunks(&store, &root, lfs_horizon_requester::PAGE_SIZE)
        .expect("the imported state reads back");
    assert_eq!(history_keys(&imported), history_keys(&honest));
}

/// A forged chunk for a pending horizon root must not write anything. It names
/// the root, carries one history item under the hash of a node the store
/// already holds, and claims to be the last chunk. The joiner rejects it and
/// asks for the path again.
#[tokio::test]
async fn a_forged_horizon_chunk_writes_nothing_and_is_asked_again() {
    let fixture = TestFixture::new().await;
    let held_root = genesis_root(&fixture);
    let honest = export_chunks(
        &fixture.rspace_store,
        &held_root,
        lfs_horizon_requester::PAGE_SIZE,
    )
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

/// A peer that keeps sending chunks the joiner does not import, each one sooner
/// than the request timeout, must not hold off the no-progress deadline. The
/// chunks here answer a path that was never asked for.
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
