//! The queue's engine, event link and node client against a stand-in node
//! that serves the node's HTTP API and event stream.

mod common;

use std::collections::HashSet;
use std::time::Duration;

use axum::http::StatusCode;
use common::{deploy, deploy_with_phlo_limit, eventually, finalized_verdict, FakeNode, Queue};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Signed;
use deploy_queue::engine::{FinalityHold, QueueSnapshot, COMMAND_BUFFER};
use deploy_queue::error::{Congestion, IntakeError};
use deploy_queue::events::run_event_link;
use deploy_queue::node::NodeHttp;
use deploy_queue::record::QueueState;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_window::last_includable_block;
use serde_json::{json, Value};

const VALIDATOR: &str = "validator";
const PEER: &str = "peer";

fn signed(valid_after: i64, number: i64, key: [u8; 32]) -> Signed<DeployData> {
    Signed::create(
        DeployData {
            term: "new x in { x!(1) }".to_string(),
            time_stamp: number,
            phlo_price: 1.into(),
            phlo_limit: 100_000,
            valid_after_block_number: valid_after,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        },
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&key),
    )
    .unwrap()
}

/// The `number`th deployer's deploy, for lines longer than the seeds allow.
/// Each deployer holds one, so no deployer's share binds.
fn numbered_deploy(valid_after: i64, number: i64) -> Signed<DeployData> {
    let mut key = [1u8; 32];
    key[24..].copy_from_slice(&number.to_be_bytes());
    signed(valid_after, number, key)
}

/// One deployer's `number`th deploy.
fn one_deployers_deploy(valid_after: i64, number: i64) -> Signed<DeployData> {
    signed(valid_after, number, [200; 32])
}

fn block_added(number: i64, creator: &str, deploy_ids: &[&[u8]]) -> Value {
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
            "creator": creator,
            "seq-num": 1,
        }
    })
}

/// The validator's block-created event for a block carrying `deploy_ids`: it
/// could take `cap` fresh deploys, and `cap_hit` says the cap left deploys in
/// the pool.
fn block_created(
    number: i64,
    cap: usize,
    cap_hit: bool,
    backpressure: bool,
    deploy_ids: &[&[u8]],
) -> Value {
    let mut event = block_added(number, VALIDATOR, deploy_ids);
    event["event"] = json!("block-created");
    event["payload"]["deploy-admission"] = json!({
        "cap": cap, "cap-hit": cap_hit, "byte-cap-hit": false,
        "cap-source": if backpressure { "hard" } else { "normal" },
        "backpressure": backpressure, "finality-lag": if backpressure { 9 } else { 2 },
    });
    event
}

/// The validator creates block `number`, which the node then adds carrying
/// `deploy_ids`.
fn own_block(node: &FakeNode, number: i64, cap: usize, cap_hit: bool, deploy_ids: &[&[u8]]) {
    node.emit(block_created(number, cap, cap_hit, false, deploy_ids));
    node.emit(block_added(number, VALIDATOR, deploy_ids));
}

fn block_finalised(number: i64) -> Value {
    let mut event = block_added(number, PEER, &[]);
    event["event"] = json!("block-finalised");
    event
}

/// Peer blocks for tips `from..=to`, finality one behind each.
fn advance(node: &FakeNode, from: i64, to: i64) {
    for tip in from..=to {
        node.emit(block_added(tip, PEER, &[]));
        node.emit(block_finalised(tip - 1));
    }
}

fn received_sigs(node: &FakeNode) -> Vec<Vec<u8>> {
    node.received()
        .iter()
        .map(|request| request.to_signed().unwrap().sig.to_vec())
        .collect()
}

fn distinct_received(node: &FakeNode) -> HashSet<Vec<u8>> {
    received_sigs(node).into_iter().collect()
}

async fn reaches_tip(queue: &Queue, tip: i64) {
    eventually("the queue reaches the tip", || async {
        queue.handle.snapshot().await.unwrap().tip == Some(tip)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deploy_is_released_and_follows_the_node_to_its_verdict() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    let deploy = deploy(5, "root", 1);
    let sig = queue.handle.intake(deploy.clone()).await.unwrap();
    assert_eq!(sig, deploy.sig.to_vec());
    eventually("the node receives the deploy", || async {
        node.received().len() == 1
    })
    .await;
    assert_eq!(node.received()[0].to_signed().unwrap(), deploy);
    assert_eq!(queue.state(&sig).await, Some(QueueState::Submitted));

    node.emit(block_added(11, VALIDATOR, &[&sig]));
    eventually("the deploy is in a block", || async {
        queue.state(&sig).await == Some(QueueState::InBlock)
    })
    .await;

    node.emit(finalized_verdict(&sig, 11));
    eventually("the verdict lands", || async {
        queue.state(&sig).await == Some(QueueState::Finalized)
    })
    .await;
    let view = queue.handle.status(sig.clone()).await.unwrap().unwrap();
    assert_eq!(
        view.latest_block_hash,
        Some(hex::decode(format!("{:064x}", 11)).unwrap())
    );
    let snapshot = queue.handle.snapshot().await.unwrap();
    assert_eq!((snapshot.finalized, snapshot.held), (1, 1));
    assert_eq!(held_by_state(&snapshot), snapshot.held);
    queue.stop();
}

fn held_by_state(snapshot: &QueueSnapshot) -> usize {
    snapshot.ready
        + snapshot.waiting
        + snapshot.closed
        + snapshot.submitted
        + snapshot.in_block
        + snapshot.finalized
        + snapshot.failed
        + snapshot.expired
        + snapshot.rejected
}

#[tokio::test(flavor = "multi_thread")]
async fn release_keeps_the_validators_pool_at_what_its_next_block_takes() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    for number in 0..12 {
        queue
            .handle
            .intake(numbered_deploy(10, number))
            .await
            .unwrap();
    }
    eventually("one deploy goes out until a cap is known", || async {
        node.received().len() == 1
    })
    .await;

    // The validator takes it under a cap of four. It waits in a block for a
    // verdict that has not come, and still four more go out: a deploy in a
    // block is the shard's to finalize, not the pool's to hold.
    let first = received_sigs(&node);
    own_block(&node, 11, 4, false, &[&first[0]]);
    eventually("the pool fills to the cap", || async {
        node.received().len() == 5
    })
    .await;

    // The next block's cap of two leaves two behind, though the node does not
    // report the cap as hit: the pool holds at the cap, so nothing more goes
    // out.
    let second: Vec<Vec<u8>> = received_sigs(&node)[1..].to_vec();
    own_block(&node, 12, 2, false, &[&second[0], &second[1]]);
    reaches_tip(&queue, 12).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(node.received().len(), 5);
    let snapshot = queue.handle.snapshot().await.unwrap();
    assert_eq!(
        (snapshot.submitted, snapshot.in_block, snapshot.ready),
        (2, 3, 7)
    );
    let validator = snapshot.validator.unwrap();
    assert_eq!(validator.pool_target, 2);
    assert!(validator.saturated);

    own_block(&node, 13, 2, false, &[&second[2], &second[3]]);
    eventually("the next two go out", || async {
        node.received().len() == 7
    })
    .await;
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn health_learns_an_own_block_once_and_only_after_the_node_adds_it() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    let own_blocks = || async {
        queue
            .handle
            .snapshot()
            .await
            .unwrap()
            .validator
            .map_or(0, |validator| validator.own_blocks)
    };

    let mut discarded = block_created(11, 8, false, false, &[]);
    discarded["payload"]["block-hash"] = json!(format!("{:064x}", 0xdead_u64));
    node.emit(discarded);
    advance(&node, 11, 11);
    reaches_tip(&queue, 11).await;
    assert_eq!(
        own_blocks().await,
        0,
        "a created block self-validation discarded"
    );

    node.emit(block_created(12, 8, false, false, &[]));
    node.emit(block_added(12, VALIDATOR, &[]));
    reaches_tip(&queue, 12).await;
    assert_eq!(own_blocks().await, 1);

    node.emit(block_created(12, 8, false, false, &[]));
    node.emit(block_added(12, VALIDATOR, &[]));
    node.emit(block_created(13, 8, false, false, &[]));
    node.emit(block_added(13, VALIDATOR, &[]));
    reaches_tip(&queue, 13).await;
    assert_eq!(own_blocks().await, 2, "a replayed block counts once");
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_names_the_validator_or_the_shard() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    let pooled = queue.handle.intake(numbered_deploy(10, 0)).await.unwrap();
    eventually("the deploy is released", || async {
        node.received().len() == 1
    })
    .await;

    // The node holds the validator to no fresh deploys because finality
    // trails the tip: its next block leaves the pooled deploy behind.
    advance(&node, 11, 11);
    node.emit(block_created(12, 0, true, true, &[]));
    node.emit(block_added(12, VALIDATOR, &[]));
    reaches_tip(&queue, 12).await;
    let refusal = queue.handle.intake(numbered_deploy(12, 1)).await;
    assert!(
        matches!(
            refusal,
            Err(IntakeError::QueueFull {
                cause: Congestion::Shard,
                ..
            })
        ),
        "{:?}",
        refusal
    );
    assert!(
        queue
            .handle
            .snapshot()
            .await
            .unwrap()
            .shard
            .unwrap()
            .backpressure
    );

    // Then it creates no block while its peers carry the shard through a
    // full citable depth: the idle validator, not the shard, holds deploys
    // back, and the queue takes none until it creates blocks again.
    advance(&node, 13, 41);
    reaches_tip(&queue, 41).await;
    assert!(matches!(
        queue.handle.intake(numbered_deploy(41, 2)).await,
        Err(IntakeError::QueueFull {
            cause: Congestion::ValidatorIdle,
            ..
        })
    ));
    assert_eq!(queue.state(&pooled).await, Some(QueueState::Submitted));
    queue.stop();
}

fn refused_for_the_shard(result: &Result<Vec<u8>, IntakeError>) -> bool {
    matches!(
        result,
        Err(IntakeError::QueueFull {
            cause: Congestion::Shard,
            ..
        })
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn finality_past_the_citable_depth_holds_release_and_intake_until_it_catches_up() {
    let node = FakeNode::new(10, 9);
    node.status.lock().unwrap()["deployLifespan"] = json!(300);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    for number in 0..3 {
        queue
            .handle
            .intake(numbered_deploy(10, number))
            .await
            .unwrap();
    }
    eventually("one deploy goes out until a cap is known", || async {
        node.received().len() == 1
    })
    .await;

    // Peers carry the tip 27 heights past the last finalized block, and the
    // validator's next block takes the pooled deploy under a cap of four.
    for tip in 11..=36 {
        node.emit(block_added(tip, PEER, &[]));
    }
    let first = received_sigs(&node);
    own_block(&node, 37, 4, false, &[&first[0]]);
    reaches_tip(&queue, 37).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(distinct_received(&node).len(), 1, "release is held");
    let snapshot = queue.handle.snapshot().await.unwrap();
    assert_eq!(snapshot.finality_hold, Some(FinalityHold { lag: 28 }));
    assert!(!snapshot.release_open);
    assert_eq!(snapshot.ready, 2);
    let refusal = queue.handle.intake(numbered_deploy(37, 3)).await;
    assert!(refused_for_the_shard(&refusal), "{:?}", refusal);

    // Finality comes back within the depth: release and intake resume.
    node.emit(block_finalised(20));
    eventually("the held deploys go out", || async {
        distinct_received(&node).len() == 3
    })
    .await;
    assert_eq!(queue.handle.snapshot().await.unwrap().finality_hold, None);
    queue.handle.intake(numbered_deploy(37, 4)).await.unwrap();
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_burst_of_heights_at_start_with_finality_within_the_depth_does_not_hold_intake() {
    let node = FakeNode::new(1, 0);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    // A new shard's first heights come in milliseconds while its first
    // finalized block has yet to move; time passing is not a stall.
    for tip in 2..=10 {
        node.emit(block_added(tip, PEER, &[]));
    }
    reaches_tip(&queue, 10).await;
    tokio::time::sleep(Duration::from_millis(2100)).await;
    queue.handle.intake(numbered_deploy(10, 0)).await.unwrap();
    assert_eq!(queue.handle.snapshot().await.unwrap().finality_hold, None);
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn refusals_follow_the_node_rules() {
    let node = FakeNode::new(100, 99);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    assert!(matches!(
        queue.handle.intake(deploy(90, "other", 3)).await,
        Err(IntakeError::IllegalArgument(_))
    ));
    assert!(matches!(
        queue.handle.intake(deploy_with_phlo_limit(90, "root", 5, 20_000_001)).await,
        Err(IntakeError::IllegalArgument(message)) if message.contains("maximum limit 20000000")
    ));
    queue
        .handle
        .intake(deploy_with_phlo_limit(90, "root", 6, 20_000_000))
        .await
        .unwrap();
    assert!(matches!(
        queue.handle.intake(deploy(50, "root", 4)).await,
        Err(IntakeError::DeployExpired(_))
    ));
    // Next block 101 plus the 25-height citable depth.
    assert!(matches!(
        queue.handle.intake(deploy(127, "root", 8)).await,
        Err(IntakeError::InvalidDeployWindow(message)) if message.contains("highest accepted block number 126")
    ));
    queue.handle.intake(deploy(126, "root", 9)).await.unwrap();
    let held = deploy(95, "root", 5);
    queue.handle.intake(held.clone()).await.unwrap();
    assert_eq!(
        queue.handle.intake(held.clone()).await,
        Err(IntakeError::Duplicate(held.sig.to_vec()))
    );
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn window_rules_follow_the_nodes_next_block_after_blocks_arrive() {
    let node = FakeNode::new(100, 99);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    advance(&node, 101, 102);
    reaches_tip(&queue, 102).await;
    // The node's next block is 103: lifespan 50 closes valid-after 53, and
    // the 25-height citable depth admits up to 128.
    assert!(matches!(
        queue.handle.intake(deploy(53, "root", 10)).await,
        Err(IntakeError::DeployExpired(_))
    ));
    queue.handle.intake(deploy(128, "root", 11)).await.unwrap();
    assert!(matches!(
        queue.handle.intake(deploy(129, "root", 12)).await,
        Err(IntakeError::InvalidDeployWindow(_))
    ));
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
        queue.handle.intake(deploy(60, "root", 6)).await,
        Err(IntakeError::QueueFull { .. })
    ));
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
    // line and the 25-height depth stay inside its window: the line the
    // queue holds scales with the shard's deploy lifespan.
    let last = last_includable_block(10, 300).unwrap();
    let fitting = last - 10 - 25;
    assert!(fitting > 25, "the window, not the depth, bounds the line");
    for number in 0..fitting {
        queue
            .handle
            .intake(numbered_deploy(10, number))
            .await
            .unwrap_or_else(|error| panic!("deploy {} of {} refused: {}", number, fitting, error));
    }
    assert!(matches!(
        queue.handle.intake(numbered_deploy(10, fitting)).await,
        Err(IntakeError::QueueFull { .. })
    ));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn one_deployer_cannot_take_the_room_another_needs() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    // At one deploy per height, a 49-height fresh window less the 25-height
    // depth holds 24 deploys; one deployer alone may hold half.
    for number in 0..12 {
        queue
            .handle
            .intake(one_deployers_deploy(10, number))
            .await
            .unwrap();
    }
    assert!(matches!(
        queue.handle.intake(one_deployers_deploy(10, 12)).await,
        Err(IntakeError::DeployerShareFull {
            held: 12,
            limit: 12,
            ..
        })
    ));
    queue.handle.intake(deploy(10, "root", 11)).await.unwrap();
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deploy_without_time_to_reach_the_validators_next_block_is_not_released() {
    let node = FakeNode::new(10, 9);
    node.status.lock().unwrap()["deployLifespan"] = json!(60);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    // Six deploys whose window runs out at block 69.
    for number in 0..6 {
        queue
            .handle
            .intake(numbered_deploy(10, number))
            .await
            .unwrap();
    }
    eventually("one deploy goes out until a cap is known", || async {
        node.received().len() == 1
    })
    .await;

    // The validator creates every fourth block under a cap of four, each
    // taking nothing of this queue's: the pool fills to four and the other
    // two wait in the queue.
    for tip in 11..=66 {
        if tip % 4 == 0 {
            own_block(&node, tip, 4, true, &[]);
        } else {
            node.emit(block_added(tip, PEER, &[]));
        }
        node.emit(block_finalised(tip - 1));
    }
    // No block carries the pooled four past the citable depth, so they are
    // also sent again; only distinct deploys count.
    eventually("the pool fills to the cap", || async {
        distinct_received(&node).len() == 4
    })
    .await;
    // At 67 the pool empties, but two heights of window are less than the
    // validator takes between its blocks: the two close in the queue.
    let pooled = received_sigs(&node);
    let ids: Vec<&[u8]> = pooled.iter().map(Vec::as_slice).collect();
    node.emit(block_added(67, VALIDATOR, &ids));
    reaches_tip(&queue, 67).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(distinct_received(&node).len(), 4);
    assert_eq!(queue.handle.snapshot().await.unwrap().closed, 2);
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deploy_no_block_carries_is_resent_after_the_citable_depth() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    let sig = queue.handle.intake(deploy(10, "root", 15)).await.unwrap();
    eventually("the deploy is released", || async {
        node.received().len() == 1
    })
    .await;

    advance(&node, 11, 35);
    reaches_tip(&queue, 35).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        node.received().len(),
        1,
        "within the depth it is not resent"
    );

    advance(&node, 36, 36);
    eventually("the deploy is resent", || async {
        node.received().len() == 2
    })
    .await;
    assert_eq!(received_sigs(&node), vec![sig.clone(), sig.clone()]);
    assert_eq!(queue.state(&sig).await, Some(QueueState::Submitted));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_from_the_node_is_terminal() {
    let node = FakeNode::new(10, 9);
    *node.deploy_answer.lock().unwrap() = (
        StatusCode::BAD_REQUEST,
        json!({"error": "illegal_argument", "message": "Phlo price 1 is less than minimum price 2."}),
    );
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    let sig = queue.handle.intake(deploy(5, "root", 7)).await.unwrap();
    eventually("the refusal lands", || async {
        queue.state(&sig).await == Some(QueueState::Rejected)
    })
    .await;
    let view = queue.handle.status(sig).await.unwrap().unwrap();
    assert_eq!(view.state.status_label(), "Rejected");
    assert!(view.detail.unwrap().starts_with("illegal_argument"));
    let snapshot = queue.handle.snapshot().await.unwrap();
    assert_eq!((snapshot.rejected, snapshot.held), (1, 1));
    assert_eq!(held_by_state(&snapshot), snapshot.held);
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_whose_validator_is_not_bonded_is_retried() {
    let node = FakeNode::new(10, 9);
    *node.deploy_answer.lock().unwrap() = (
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error": "validator_not_bonded", "message": "Deploy was rejected because node's validator is not bonded."}),
    );
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;

    let sig = queue.handle.intake(deploy(5, "root", 10)).await.unwrap();
    eventually("the node refuses it", || async {
        !node.received().is_empty()
    })
    .await;
    eventually("the deploy waits in the queue", || async {
        queue.state(&sig).await == Some(QueueState::Queued)
    })
    .await;

    *node.deploy_answer.lock().unwrap() = (StatusCode::OK, json!("Success!"));
    eventually("the node receives the retry", || async {
        node.received().len() >= 2
    })
    .await;
    eventually("the retry is taken", || async {
        queue.state(&sig).await == Some(QueueState::Submitted)
    })
    .await;
    queue.stop();
}

/// A queue holding one released deploy.
async fn queue_with_a_released_deploy(seed: u8) -> (FakeNode, Queue, tempfile::TempDir, Vec<u8>) {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    let sig = queue.handle.intake(deploy(5, "root", seed)).await.unwrap();
    eventually("the deploy is released", || async {
        queue.state(&sig).await == Some(QueueState::Submitted)
    })
    .await;
    (node, queue, dir, sig)
}

/// The node's status read answers the deploy finalized; no event says so.
fn finalize_without_an_event(node: &FakeNode) {
    *node.deploy_status.lock().unwrap() = json!({"state": "Finalized", "rejection_count": 0,
        "latest_block_hash": format!("{:064x}", 11)});
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lag_in_the_event_stream_reads_the_verdicts_it_lost() {
    let (node, queue, _dir, sig) = queue_with_a_released_deploy(12).await;
    finalize_without_an_event(&node);
    node.emit(json!({"event": "stream-lagged", "schema-version": 1, "payload": {"skipped": 3}}));
    eventually("the lost verdict is read", || async {
        queue.state(&sig).await == Some(QueueState::Finalized)
    })
    .await;
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dropped_event_stream_reconnects_and_reads_the_verdicts_it_lost() {
    let (node, queue, _dir, sig) = queue_with_a_released_deploy(13).await;
    finalize_without_an_event(&node);
    node.disconnect.send(()).unwrap();
    eventually("the verdict is read after the reconnect", || async {
        queue.state(&sig).await == Some(QueueState::Finalized)
    })
    .await;
    assert!(queue.handle.snapshot().await.unwrap().connected);
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_event_is_skipped_and_the_link_stays_up() {
    let (node, queue, _dir, sig) = queue_with_a_released_deploy(14).await;
    // The status read stays Pending, so only the event that follows the
    // unreadable one can finalize the deploy. A dropped link would lose it.
    node.events
        .send(r#"{"event":"block-added","payload":{}}"#.to_string())
        .unwrap();
    node.emit(finalized_verdict(&sig, 11));
    eventually("the event after the unreadable one arrives", || async {
        queue.state(&sig).await == Some(QueueState::Finalized)
    })
    .await;
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

    let sig = queue.handle.intake(deploy(5, "root", 9)).await.unwrap();
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
    let node = FakeNode::new(40, 39);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();

    // Not valid until block 41, so the first queue only holds it.
    let queue = Queue::start(&url, dir.path()).await;
    let sig = queue.handle.intake(deploy(41, "root", 8)).await.unwrap();
    assert_eq!(queue.state(&sig).await, Some(QueueState::Queued));
    queue.stop();
    tokio::time::sleep(Duration::from_millis(100)).await;

    node.status.lock().unwrap()["latestBlockNumber"] = json!(42);
    node.status.lock().unwrap()["lastFinalizedBlockNumber"] = json!(40);
    let queue = Queue::start(&url, dir.path()).await;
    eventually("the restarted queue releases", || async {
        node.received().len() == 1
    })
    .await;
    assert_eq!(queue.state(&sig).await, Some(QueueState::Submitted));
    queue.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_decided_deploy_keeps_its_record_on_disk_without_its_payload() {
    let node = FakeNode::new(10, 9);
    let url = node.clone().serve().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::start(&url, dir.path()).await;
    let first = deploy(5, "root", 1);
    let second = deploy(30, "root", 2);
    let sizes = [&first, &second].map(|d| deploy_queue::record::encode_deploy(d).len() as u64);
    let decided = queue.handle.intake(first).await.unwrap();
    let held = queue.handle.intake(second).await.unwrap();
    let payload_bytes = || async { queue.handle.snapshot().await.unwrap().payload_bytes };
    assert_eq!(payload_bytes().await, sizes[0] + sizes[1]);
    eventually("the node receives the due deploy", || async {
        node.received().len() == 1
    })
    .await;
    node.emit(block_added(11, VALIDATOR, &[&decided]));
    node.emit(finalized_verdict(&decided, 11));
    eventually("the verdict lands", || async {
        queue.state(&decided).await == Some(QueueState::Finalized)
    })
    .await;
    assert_eq!(payload_bytes().await, sizes[1]);
    queue.stop();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let restarted = Queue::start(&url, dir.path()).await;
    assert_eq!(
        restarted.handle.snapshot().await.unwrap().payload_bytes,
        sizes[1],
        "a restart counts the payloads still held"
    );
    restarted.stop();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let store = deploy_queue::store::QueueStore::open(dir.path()).unwrap();
    assert_eq!(store.load().unwrap().len(), 2);
    assert_eq!(store.payload(&decided).unwrap(), None);
    assert!(store.payload(&held).unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_link_stops_in_front_of_a_node_that_is_not_a_validator() {
    for (is_validator, is_read_only) in [(false, false), (true, true)] {
        let node = FakeNode::new(10, 8);
        {
            let mut status = node.status.lock().unwrap();
            status["isValidator"] = json!(is_validator);
            status["isReadOnly"] = json!(is_read_only);
        }
        let url = node.serve().await;
        let (commands, _engine_side) = tokio::sync::mpsc::channel(COMMAND_BUFFER);
        let linked = tokio::time::timeout(
            Duration::from_secs(10),
            run_event_link(NodeHttp::new(&url).unwrap(), commands),
        )
        .await
        .expect("the link stops instead of retrying");
        assert!(
            linked.is_err(),
            "isValidator {is_validator}, isReadOnly {is_read_only}"
        );
    }
}
