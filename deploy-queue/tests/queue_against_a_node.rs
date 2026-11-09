//! The queue's engine, event link and node client against a stand-in node
//! that serves the node's HTTP API and event stream.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Signed;
use deploy_queue::engine::{Engine, EngineHandle, COMMAND_BUFFER};
use deploy_queue::error::IntakeError;
use deploy_queue::events::run_event_link;
use deploy_queue::http::accept;
use deploy_queue::node::NodeHttp;
use deploy_queue::record::QueueState;
use deploy_queue::store::QueueStore;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_request::DeployRequest;
use models::rust::deploy_window::last_includable_block;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

#[derive(Clone)]
struct FakeNode {
    status: Arc<Mutex<Value>>,
    deploy_answer: Arc<Mutex<(StatusCode, Value)>>,
    deploy_status: Arc<Mutex<Value>>,
    received: Arc<Mutex<Vec<DeployRequest>>>,
    events: broadcast::Sender<String>,
}

impl FakeNode {
    fn new(tip: i64, lfb: i64) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            status: Arc::new(Mutex::new(status_body(tip, lfb))),
            deploy_answer: Arc::new(Mutex::new((StatusCode::OK, json!("Success!")))),
            deploy_status: Arc::new(Mutex::new(
                json!({"state":"Pending","rejection_count":0,"latest_block_hash":null}),
            )),
            received: Arc::new(Mutex::new(Vec::new())),
            events,
        }
    }

    fn received(&self) -> Vec<DeployRequest> { self.received.lock().unwrap().clone() }

    fn emit(&self, event: Value) { self.events.send(event.to_string()).unwrap(); }

    async fn serve(self) -> String {
        let router = Router::new()
            .route("/api/status", get(status))
            .route("/api/deploy", post(take_deploy))
            .route("/api/deploy-finalization-status/{sig}", get(deploy_status))
            .route("/ws/events", get(events))
            .with_state(self);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{}", address)
    }
}

fn status_body(tip: i64, lfb: i64) -> Value {
    json!({
        "shardId": "root",
        "minPhloPrice": 1,
        "lastFinalizedBlockNumber": lfb,
        "latestBlockNumber": tip,
        "deployLifespan": 50,
        "maxUserDeploysPerBlock": 4,
        "maxParentDepth": 25,
    })
}

async fn status(State(node): State<FakeNode>) -> Json<Value> {
    Json(node.status.lock().unwrap().clone())
}

async fn deploy_status(State(node): State<FakeNode>) -> Json<Value> {
    Json(node.deploy_status.lock().unwrap().clone())
}

async fn take_deploy(State(node): State<FakeNode>, Json(request): Json<DeployRequest>) -> Response {
    node.received.lock().unwrap().push(request);
    let (code, body) = node.deploy_answer.lock().unwrap().clone();
    (code, Json(body)).into_response()
}

async fn events(State(node): State<FakeNode>, upgrade: WebSocketUpgrade) -> Response {
    let events = node.events.subscribe();
    upgrade.on_upgrade(move |socket| forward(socket, events))
}

async fn forward(mut socket: WebSocket, mut events: broadcast::Receiver<String>) {
    while let Ok(event) = events.recv().await {
        if socket.send(Message::Text(event.into())).await.is_err() {
            return;
        }
    }
}

struct Queue {
    handle: EngineHandle,
    tasks: Vec<JoinHandle<()>>,
}

impl Queue {
    async fn start(node_url: &str, data_dir: &std::path::Path) -> Self {
        let node = NodeHttp::new(node_url).unwrap();
        let store = QueueStore::open(data_dir).unwrap();
        let (commands, receiver) = mpsc::channel(COMMAND_BUFFER);
        let engine = Engine::new(store, node.clone(), commands.clone()).unwrap();
        let tasks = vec![
            tokio::spawn(async move { engine.run(receiver).await.unwrap() }),
            tokio::spawn(run_event_link(node, commands.clone())),
        ];
        let queue = Self {
            handle: EngineHandle::new(commands),
            tasks,
        };
        eventually("the queue connects", || async {
            queue.handle.snapshot().await.unwrap().connected
        })
        .await;
        queue
    }

    async fn state(&self, sig: &[u8]) -> Option<QueueState> {
        self.handle
            .status(sig.to_vec())
            .await
            .unwrap()
            .map(|view| view.state)
    }

    fn stop(self) {
        for task in self.tasks {
            task.abort();
        }
    }
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..250 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting until {}", what);
}

fn deploy(valid_after: i64, shard_id: &str, seed: u8) -> Signed<DeployData> {
    Signed::create(
        DeployData {
            term: "new x in { x!(1) }".to_string(),
            time_stamp: i64::from(seed),
            phlo_price: 1,
            phlo_limit: 100_000,
            valid_after_block_number: valid_after,
            shard_id: shard_id.to_string(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        },
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[seed; 32]),
    )
    .unwrap()
}

/// One deployer's `number`th deploy, for lines longer than the seeds allow.
fn numbered_deploy(valid_after: i64, number: i64) -> Signed<DeployData> {
    Signed::create(
        DeployData {
            term: "new x in { x!(1) }".to_string(),
            time_stamp: number,
            phlo_price: 1,
            phlo_limit: 100_000,
            valid_after_block_number: valid_after,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        },
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[200; 32]),
    )
    .unwrap()
}

fn block_added(number: i64, deploy_ids: &[&[u8]]) -> Value {
    json!({
        "event": "block-added",
        "schema-version": 1,
        "payload": {
            "block-hash": format!("{:064x}", number),
            "block-number": number,
            "timestamp": 0,
            "parent-hashes": [],
            "justification-hashes": [],
            "deploys": deploy_ids
                .iter()
                .map(|id| json!({"id": hex::encode(id), "cost": 1, "deployer": "d", "errored": false}))
                .collect::<Vec<_>>(),
            "creator": "c",
            "seq-num": 1,
        }
    })
}

fn block_finalised(number: i64) -> Value {
    let mut event = block_added(number, &[]);
    event["event"] = json!("block-finalised");
    event
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deploy_is_released_and_follows_the_node_to_its_verdict() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    let deploy = deploy(5, "root", 1);
    let sig = accept(&queue.handle, deploy.clone()).await.unwrap();
    assert_eq!(sig, deploy.sig.to_vec());
    eventually("the node receives the deploy", || async { node.received().len() == 1 }).await;
    assert_eq!(node.received()[0].to_signed().unwrap(), deploy);
    assert_eq!(queue.state(&sig).await, Some(QueueState::Submitted));

    node.emit(block_added(11, &[&sig]));
    eventually("the deploy is in a block", || async {
        queue.state(&sig).await == Some(QueueState::InBlock)
    })
    .await;

    node.emit(json!({
        "event": "deploy-verdicts",
        "schema-version": 1,
        "payload": {"verdicts": [{"deploy-id": hex::encode(&sig), "state": "finalized",
                    "rejection-count": 0, "block-hash": format!("{:064x}", 11)}]}
    }));
    eventually("the verdict lands", || async {
        queue.state(&sig).await == Some(QueueState::Finalized)
    })
    .await;
    let view = queue.handle.status(sig.clone()).await.unwrap().unwrap();
    assert_eq!(view.latest_block_hash, Some(hex::decode(format!("{:064x}", 11)).unwrap()));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn release_waits_while_finality_trails_the_tip() {
    let node = FakeNode::new(40, 10);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    let sig = accept(&queue.handle, deploy(39, "root", 2)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(node.received().is_empty(), "40 - 10 exceeds the parent depth of 25");
    assert_eq!(queue.state(&sig).await, Some(QueueState::Queued));

    node.emit(block_finalised(20));
    eventually("release resumes", || async { node.received().len() == 1 }).await;
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn refusals_follow_the_node_rules() {
    let node = FakeNode::new(100, 99);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    assert!(matches!(
        accept(&queue.handle, deploy(90, "other", 3)).await,
        Err(IntakeError::IllegalArgument(_))
    ));
    assert!(matches!(
        accept(&queue.handle, deploy(50, "root", 4)).await,
        Err(IntakeError::DeployExpired(_))
    ));
    let held = deploy(95, "root", 5);
    accept(&queue.handle, held.clone()).await.unwrap();
    assert_eq!(
        accept(&queue.handle, held.clone()).await,
        Err(IntakeError::Duplicate(held.sig.to_vec()))
    );
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deploy_too_late_to_land_is_refused_as_full() {
    let node = FakeNode::new(100, 99);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    // Open through block 109 at lifespan 50, but landing from tip 100 needs a
    // height of inclusion plus the 25-height margin: 126.
    assert!(matches!(
        accept(&queue.handle, deploy(60, "root", 6)).await,
        Err(IntakeError::QueueFull { .. })
    ));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_from_the_node_is_terminal() {
    let node = FakeNode::new(10, 9);
    *node.deploy_answer.lock().unwrap() = (
        StatusCode::BAD_REQUEST,
        json!({"error": "illegal_argument", "message": "Deploy was rejected because node's validator is not bonded."}),
    );
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    let sig = accept(&queue.handle, deploy(5, "root", 7)).await.unwrap();
    eventually("the refusal lands", || async {
        queue.state(&sig).await == Some(QueueState::Rejected)
    })
    .await;
    let view = queue.handle.status(sig).await.unwrap().unwrap();
    assert_eq!(view.state.status_label(), "Failed");
    assert!(view.detail.unwrap().starts_with("illegal_argument"));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn release_waits_for_released_deploys_to_settle() {
    let node = FakeNode::new(10, 9);
    // Full blocks large enough that the depth, not the ceiling, binds.
    node.status.lock().unwrap()["maxUserDeploysPerBlock"] = json!(128);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    for seed in 20..36 {
        accept(&queue.handle, deploy(5, "root", seed)).await.unwrap();
    }
    eventually("release starts", || async { !node.received().is_empty() }).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        node.received().len(),
        13,
        "release stops at half the depth; queue {:?}",
        queue.handle.snapshot().await.unwrap()
    );
    let snapshot = queue.handle.snapshot().await.unwrap();
    assert_eq!(
        snapshot.released_limit, 13,
        "half of a 25-height depth at one deploy per height"
    );
    assert_eq!((snapshot.submitted, snapshot.ready), (13, 3));

    let released: Vec<Vec<u8>> = node
        .received()
        .iter()
        .map(|request| request.to_signed().unwrap().sig.to_vec())
        .collect();
    let ids: Vec<&[u8]> = released.iter().map(|sig| sig.as_slice()).collect();
    node.emit(block_added(11, &ids));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        node.received().len(),
        13,
        "a block does not free room: its deploys still wait for verdicts"
    );

    let verdicts: Vec<Value> = released
        .iter()
        .map(|sig| {
            json!({"deploy-id": hex::encode(sig), "state": "finalized",
                   "rejection-count": 0, "block-hash": format!("{:064x}", 11)})
        })
        .collect();
    node.emit(json!({
        "event": "deploy-verdicts",
        "schema-version": 1,
        "payload": {"verdicts": verdicts}
    }));
    eventually("verdicts free room for the rest", || async {
        node.received().len() == 16
    })
    .await;
    queue.stop();
}

/// A queue at tip 10 with full blocks large enough that the window, not
/// the ceiling, binds, holding `count` deploys of one deployer.
async fn queue_with_backlog(count: i64) -> (FakeNode, Queue, tempfile::TempDir) {
    let node = FakeNode::new(10, 9);
    node.status.lock().unwrap()["maxUserDeploysPerBlock"] = json!(128);
    node.status.lock().unwrap()["deployLifespan"] = json!(300);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    for number in 0..count {
        accept(&queue.handle, numbered_deploy(10, number)).await.unwrap();
    }
    eventually("the first window is released", || async {
        node.received().len() == 13
    })
    .await;
    (node, queue, dir)
}

fn finalized_verdicts(node: &FakeNode, block: i64) -> Value {
    let verdicts: Vec<Value> = node
        .received()
        .iter()
        .map(|request| {
            json!({"deploy-id": hex::encode(request.to_signed().unwrap().sig),
                   "state": "finalized", "rejection-count": 0,
                   "block-hash": format!("{:064x}", block)})
        })
        .collect();
    json!({"event": "deploy-verdicts", "schema-version": 1, "payload": {"verdicts": verdicts}})
}

async fn released_limit(queue: &Queue) -> usize {
    queue.handle.snapshot().await.unwrap().released_limit
}

/// New tips `from..=to`, finality one behind each.
fn advance(node: &FakeNode, from: i64, to: i64) {
    for tip in from..=to {
        node.emit(block_added(tip, &[]));
        node.emit(block_finalised(tip - 1));
    }
}

fn distinct_received(node: &FakeNode) -> HashSet<Vec<u8>> {
    node.received()
        .iter()
        .map(|request| request.to_signed().unwrap().sig.to_vec())
        .collect()
}

// Rounds are 12.5 heights; the first starts when the queue connects at tip
// 10, so rounds end at tips 23, 36, 49 and so on.

async fn reaches_tip(queue: &Queue, tip: i64) {
    eventually("the queue reaches the tip", || async {
        queue.handle.snapshot().await.unwrap().tip == Some(tip)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn timely_verdicts_grow_the_release_window() {
    let (node, queue, _dir) = queue_with_backlog(60).await;
    node.emit(block_added(11, &[]));
    node.emit(finalized_verdicts(&node, 11));
    eventually("the freed window is refilled", || async {
        node.received().len() == 26
    })
    .await;
    // Thirteen timely verdicts with deploys held back: at the round's end
    // slow start doubles the window.
    advance(&node, 12, 24);
    eventually("the doubled window is released", || async {
        node.received().len() == 39
    })
    .await;
    assert_eq!(released_limit(&queue).await, 26);
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deploy_that_keeps_losing_does_not_shrink_its_queue() {
    let (node, queue, _dir) = queue_with_backlog(200).await;
    let stuck = node.received()[0].to_signed().unwrap().sig.to_vec();
    // Every other released deploy settles the height after release; one
    // never does, as a deploy that loses every merge would not.
    for tip in 11..=60 {
        node.emit(block_added(tip, &[]));
        node.emit(block_finalised(tip - 1));
        let verdicts: Vec<Value> = distinct_received(&node)
            .into_iter()
            .filter(|sig| *sig != stuck)
            .map(|sig| {
                json!({"deploy-id": hex::encode(sig), "state": "finalized",
                       "rejection-count": 0, "block-hash": format!("{:064x}", tip)})
            })
            .collect();
        node.emit(json!({"event": "deploy-verdicts", "schema-version": 1,
                         "payload": {"verdicts": verdicts}}));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let snapshot = queue.handle.snapshot().await.unwrap();
    assert_eq!(queue.state(&stuck).await, Some(QueueState::Submitted));
    assert_eq!(
        snapshot.slow_start_threshold,
        Some(256.0),
        "a deploy unsettled for 50 heights among timely ones never halves the window: {:?}",
        snapshot
    );
    assert!(snapshot.released_limit > 13, "{:?}", snapshot);
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn admission_tightens_as_soon_as_release_is_cut() {
    let (node, queue, _dir) = queue_with_backlog(120).await;
    advance(&node, 11, 30);
    reaches_tip(&queue, 30).await;
    node.emit(finalized_verdicts(&node, 30));
    advance(&node, 31, 37);
    reaches_tip(&queue, 37).await;
    // The round ending at 36 saw only verdicts 20 heights after release:
    // the window halved to 6.5 deploys at 20 heights each, well below the
    // settle rate the queue measured.
    let snapshot = queue.handle.snapshot().await.unwrap();
    let release_rate = snapshot.release_rate.expect("a round with verdicts sets the rate");
    assert!(
        release_rate < snapshot.settled_per_height.unwrap(),
        "{:?}",
        snapshot
    );
    // With about 107 deploys ahead, a fresh deploy would land in time at the
    // measured rate but not at what the window now lets through.
    assert!(matches!(
        accept(&queue.handle, numbered_deploy(37, 1000)).await,
        Err(IntakeError::QueueFull { .. })
    ));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deploy_without_time_to_be_included_is_not_released() {
    let node = FakeNode::new(10, 9);
    node.status.lock().unwrap()["maxUserDeploysPerBlock"] = json!(128);
    node.status.lock().unwrap()["deployLifespan"] = json!(60);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    // Twenty deploys whose window runs out at block 69.
    for number in 0..20 {
        accept(&queue.handle, numbered_deploy(10, number)).await.unwrap();
    }
    eventually("the first window is released", || async {
        node.received().len() == 13
    })
    .await;
    // The released deploys land in a block one height later and then never
    // settle, so the other seven wait in the queue until block 68.
    let released: Vec<Vec<u8>> = distinct_received(&node).into_iter().collect();
    let ids: Vec<&[u8]> = released.iter().map(Vec::as_slice).collect();
    node.emit(block_added(11, &ids));
    node.emit(block_finalised(10));
    advance(&node, 12, 68);
    node.emit(finalized_verdicts(&node, 68));
    tokio::time::sleep(Duration::from_millis(500)).await;
    // One height of window left is less than the two the shard takes to
    // include a deploy: the seven are closed in the queue, not released.
    assert_eq!(distinct_received(&node).len(), 13);
    assert_eq!(queue.handle.snapshot().await.unwrap().closed, 7);
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_that_settles_nothing_shrinks_release_without_verdicts() {
    let (node, queue, _dir) = queue_with_backlog(60).await;
    advance(&node, 11, 49);
    // The first round holds the release itself; the rounds ending at 36 and
    // 49 pass with deploys held back, no verdict, and the deploys released
    // at 10 still waiting: 13 -> 6.5 -> 3.25.
    eventually("the window halves twice", || async {
        released_limit(&queue).await == 3
    })
    .await;
    assert_eq!(
        distinct_received(&node).len(),
        13,
        "no new deploy goes to a node that settles nothing"
    );
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_verdict_after_a_resend_counts_from_the_first_release() {
    let (node, queue, _dir) = queue_with_backlog(60).await;
    // Past the citable depth the stuck deploys are re-sent; their verdicts
    // then arrive one height after the re-send but 27 after release.
    advance(&node, 11, 36);
    eventually("the stuck deploys are re-sent", || async {
        node.received().len() == 26
    })
    .await;
    let window_before = queue.handle.snapshot().await.unwrap().release_window.unwrap();
    let first = node.received()[0].to_signed().unwrap().sig.to_vec();
    node.emit(block_added(37, &[]));
    node.emit(finalized_verdicts(&node, 37));
    eventually("the verdicts land", || async {
        queue.state(&first).await == Some(QueueState::Finalized)
    })
    .await;
    // The round holding those verdicts ends at 49: late verdicts halve the
    // window, where timely ones with deploys held back would grow it.
    advance(&node, 38, 50);
    reaches_tip(&queue, 50).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let window_after = queue.handle.snapshot().await.unwrap().release_window.unwrap();
    assert!(
        window_after < window_before,
        "verdicts 27 heights after release must count as late: {} -> {}",
        window_before,
        window_after
    );
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backlog_past_the_citable_depth_waits_within_its_window() {
    let node = FakeNode::new(10, 9);
    node.status.lock().unwrap()["deployLifespan"] = json!(300);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    // At one deploy per height, a deploy fits while the tip, its place in
    // line and the 25-height depth stay inside its window.
    let last = last_includable_block(10, 300).unwrap();
    let fitting = last - 10 - 25;
    assert!(fitting > 25, "the window, not the depth, bounds the line");
    for number in 0..fitting {
        accept(&queue.handle, numbered_deploy(10, number))
            .await
            .unwrap_or_else(|error| panic!("deploy {} of {} refused: {}", number, fitting, error));
    }
    assert!(matches!(
        accept(&queue.handle, numbered_deploy(10, fitting)).await,
        Err(IntakeError::QueueFull { .. })
    ));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deploy_the_node_already_decided_takes_the_nodes_verdict() {
    let node = FakeNode::new(10, 9);
    *node.deploy_answer.lock().unwrap() = (
        StatusCode::CONFLICT,
        json!({"error": "duplicate_deploy", "message": "Deploy already known"}),
    );
    *node.deploy_status.lock().unwrap() =
        json!({"state":"Finalized","rejection_count":1,"latest_block_hash":"ab"});
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    let sig = accept(&queue.handle, deploy(5, "root", 9)).await.unwrap();
    eventually("the node's verdict is adopted", || async {
        queue.state(&sig).await == Some(QueueState::Finalized)
    })
    .await;
    let view = queue.handle.status(sig).await.unwrap().unwrap();
    assert_eq!(view.rejection_count, 1);
    assert_eq!(view.latest_block_hash, Some(vec![0xab]));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn queued_deploys_survive_a_restart_and_are_released_after_it() {
    let node = FakeNode::new(40, 10);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();

    let queue = Queue::start(&url, dir.path()).await;
    let sig = accept(&queue.handle, deploy(39, "root", 8)).await.unwrap();
    queue.stop();
    tokio::time::sleep(Duration::from_millis(100)).await;

    *node.status.lock().unwrap() = status_body(41, 30);
    let queue = Queue::start(&url, dir.path()).await;
    eventually("the restarted queue releases", || async { node.received().len() == 1 }).await;
    assert_eq!(queue.state(&sig).await, Some(QueueState::Submitted));
    queue.stop();
}
