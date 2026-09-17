// Proof tests for BlockAPI::list_pending_deploys and the
// `Casper::list_pending_deploys` trait method.
//
// The snapshot reads `deploy_storage` (fresh, not yet proposed) and
// `rejected_deploy_buffer` (recovering after a merge conflict) and pairs
// each entry with an `is_rejected` flag. Read-only nodes (Casper not
// initialised) get an empty snapshot. The BlockAPI wrapper filters by
// deployer public key, sorts deterministically, and caps the result at
// `PENDING_DEPLOYS_MAX_RESULTS` entries with `total_available` reporting
// the pre-cap count.

use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::api::pending_deploys::{PendingDeploysSnapshot, PENDING_DEPLOYS_MAX_RESULTS};
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::engine_with_casper::EngineWithCasper;
use casper::rust::engine::multi_parent_casper::MultiParentCasperImpl;
use casper::rust::util::construct_deploy;
use crypto::rust::public_key::PublicKey;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new() -> Self {
        fn bonds_function(validators: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
            validators
                .into_iter()
                .zip(vec![10i64, 10i64, 10i64])
                .collect()
        }

        let parameters =
            GenesisBuilder::build_genesis_parameters_with_defaults(Some(bonds_function), None);
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(parameters))
            .await
            .expect("Failed to build genesis");

        Self { genesis }
    }
}

async fn create_engine_cell(node: &TestNode) -> EngineCell {
    let casper_for_engine = Arc::new(MultiParentCasperImpl {
        block_retriever: node.casper.block_retriever.clone(),
        event_publisher: node.casper.event_publisher.clone(),
        runtime_manager: node.casper.runtime_manager.clone(),
        estimator: node.casper.estimator.clone(),
        block_store: node.casper.block_store.clone(),
        block_dag_storage: node.casper.block_dag_storage.clone(),
        deploy_storage: node.casper.deploy_storage.clone(),
        rejected_deploy_buffer: node.casper.rejected_deploy_buffer.clone(),
        deploy_lifecycle: node.casper.deploy_lifecycle.clone(),
        casper_buffer_storage: node.casper.casper_buffer_storage.clone(),
        validator_id: node.casper.validator_id.clone(),
        casper_shard_conf: node.casper.casper_shard_conf.clone(),
        approved_block: node.casper.approved_block.clone(),
        divergence_monitor: node.casper.divergence_monitor.clone(),
        finalization_in_progress: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        finalizer_task_in_progress: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        finalizer_task_queued: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        heartbeat_signal_ref: casper::rust::heartbeat_signal::new_heartbeat_signal_ref(),
        deploys_in_scope_cache: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        active_validators_cache: std::sync::Arc::new(tokio::sync::Mutex::new(
            std::collections::HashMap::new(),
        )),
    });
    let engine = EngineWithCasper::new(casper_for_engine);
    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;
    engine_cell
}

/// Read-only nodes (Casper not initialised) get an empty snapshot, not an
/// error. Matches the trait's default and the `with_casper` None branch.
/// A bootstrapping node (`with_casper()` returns `None`) errors like the
/// other sixteen BlockAPI methods — an empty snapshot would read as
/// "nothing pending".
#[tokio::test]
async fn no_casper_returns_error() {
    let engine_cell = EngineCell::init();

    let result = BlockAPI::list_pending_deploys(&engine_cell, None).await;

    assert!(result.is_err(), "bootstrapping node must error");
}

/// A fresh deploy in `deploy_storage` is returned with `is_rejected = false`.
#[tokio::test]
async fn fresh_deploy_storage_returns_is_rejected_false() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;

    let deploy = construct_deploy::basic_deploy_data(1, None, None).expect("deploy");
    nodes[0]
        .casper
        .deploy_storage
        .lock()
        .add(vec![deploy.clone()])
        .expect("add fresh deploy");

    let snapshot = BlockAPI::list_pending_deploys(&engine_cell, None)
        .await
        .expect("snapshot");

    assert_eq!(snapshot.deploys.len(), 1);
    assert_eq!(snapshot.total_available, 1);
    assert!(
        !snapshot.deploys[0].1,
        "fresh deploy must be is_rejected=false"
    );
    assert_eq!(snapshot.deploys[0].0.sig, deploy.sig);
}

/// A deploy in `rejected_deploy_buffer` is returned with `is_rejected = true`.
#[tokio::test]
async fn rejected_deploy_buffer_returns_is_rejected_true() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;

    let deploy = construct_deploy::basic_deploy_data(2, None, None).expect("deploy");
    nodes[0]
        .casper
        .rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .add(vec![deploy.clone()])
        .expect("add rejected deploy");

    let snapshot = BlockAPI::list_pending_deploys(&engine_cell, None)
        .await
        .expect("snapshot");

    assert_eq!(snapshot.deploys.len(), 1);
    assert_eq!(snapshot.total_available, 1);
    assert!(
        snapshot.deploys[0].1,
        "rejected deploy must be is_rejected=true"
    );
    assert_eq!(snapshot.deploys[0].0.sig, deploy.sig);
}

/// A signature that sits in BOTH pools is emitted exactly once, with
/// `is_rejected = true` — the fresh-pool predicate excludes buffered sigs,
/// so `total_available` does not double-count.
#[tokio::test]
async fn sig_in_both_pools_emitted_once_as_rejected() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;

    let duplicated = construct_deploy::basic_deploy_data(7, None, None).expect("duplicated");
    // Distinct key so only_fresh is not a duplicate of duplicated.
    let only_fresh =
        construct_deploy::basic_deploy_data(8, Some(construct_deploy::DEFAULT_SEC2.clone()), None)
            .expect("only fresh");
    nodes[0]
        .casper
        .deploy_storage
        .lock()
        .add(vec![duplicated.clone(), only_fresh.clone()])
        .expect("add storage deploys");
    nodes[0]
        .casper
        .rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .add(vec![duplicated.clone()])
        .expect("add buffered deploy");

    let snapshot = BlockAPI::list_pending_deploys(&engine_cell, None)
        .await
        .expect("snapshot");

    assert_eq!(
        snapshot.deploys.len(),
        2,
        "duplicated sig must be emitted once"
    );
    assert_eq!(snapshot.total_available, 2);
    let by_sig: HashMap<_, _> = snapshot
        .deploys
        .iter()
        .map(|(d, r)| (d.sig.clone(), *r))
        .collect();
    assert_eq!(
        by_sig.get(&duplicated.sig),
        Some(&true),
        "sig in both pools must surface as rejected"
    );
    assert_eq!(by_sig.get(&only_fresh.sig), Some(&false));
}

/// Both pools are read in one snapshot: fresh + rejected deploys coexist.
#[tokio::test]
async fn both_pools_snapshot_together() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;

    let fresh = construct_deploy::basic_deploy_data(3, None, None).expect("fresh");
    let rejected = construct_deploy::basic_deploy_data(4, None, None).expect("rejected");
    nodes[0]
        .casper
        .deploy_storage
        .lock()
        .add(vec![fresh.clone()])
        .expect("add fresh");
    nodes[0]
        .casper
        .rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .add(vec![rejected.clone()])
        .expect("add rejected");

    let snapshot = BlockAPI::list_pending_deploys(&engine_cell, None)
        .await
        .expect("snapshot");

    assert_eq!(snapshot.deploys.len(), 2);
    assert_eq!(snapshot.total_available, 2);
    let by_sig: HashMap<_, _> = snapshot
        .deploys
        .iter()
        .map(|(d, r)| (d.sig.clone(), *r))
        .collect();
    assert_eq!(by_sig.get(&fresh.sig), Some(&false));
    assert_eq!(by_sig.get(&rejected.sig), Some(&true));
}

/// The deployer filter returns only deploys signed by the given public key.
#[tokio::test]
async fn deployer_filter_returns_only_matching() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;

    let deploy_a = construct_deploy::basic_deploy_data(5, None, None).expect("deploy a");
    let deploy_b =
        construct_deploy::basic_deploy_data(6, Some(construct_deploy::DEFAULT_SEC2.clone()), None)
            .expect("deploy b");
    nodes[0]
        .casper
        .deploy_storage
        .lock()
        .add(vec![deploy_a.clone(), deploy_b.clone()])
        .expect("add deploys");

    let pk_a = deploy_a.pk.bytes.clone();
    let snapshot = BlockAPI::list_pending_deploys(&engine_cell, Some(&pk_a))
        .await
        .expect("snapshot");

    assert_eq!(snapshot.deploys.len(), 1);
    assert_eq!(snapshot.total_available, 1);
    assert_eq!(snapshot.deploys[0].0.pk.bytes, pk_a);
}

/// When more than `PENDING_DEPLOYS_MAX_RESULTS` deploys match, the result
/// is capped and `total_available` reports the pre-cap count so callers
/// can detect truncation. The truncation keeps the smallest
/// `(timestamp, sig)` entries: deploys are inserted in descending
/// timestamp order and the result must come back sorted ascending.
#[tokio::test]
async fn cap_truncates_and_reports_total_available() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;

    let total = PENDING_DEPLOYS_MAX_RESULTS + 50;
    // Insert in DESCENDING timestamp order to prove the API sorts before
    // truncating rather than relying on storage order.
    let mut deploys: Vec<_> = (0..total)
        .map(|i| {
            construct_deploy::source_deploy(
                format!("@{}!({})", i, i),
                1_700_000_000_000 + i as i64,
                None,
                None,
                None,
                Some(0),
                None,
            )
            .expect("deploy")
        })
        .collect();
    deploys.reverse();
    nodes[0]
        .casper
        .deploy_storage
        .lock()
        .add(deploys)
        .expect("add deploys");

    let snapshot = BlockAPI::list_pending_deploys(&engine_cell, None)
        .await
        .expect("snapshot");

    assert_eq!(snapshot.deploys.len(), PENDING_DEPLOYS_MAX_RESULTS);
    assert_eq!(snapshot.total_available, total as u32);

    // The result must be sorted by (timestamp, sig): the first entry is
    // the oldest deploy inserted (timestamp base), and every next entry's
    // timestamp is greater or equal.
    let mut prev = (
        snapshot.deploys[0].0.data.time_stamp,
        snapshot.deploys[0].0.sig.as_ref().to_vec(),
    );
    assert_eq!(prev.0, 1_700_000_000_000, "oldest deploy kept first");
    for (d, _) in &snapshot.deploys[1..] {
        let cur = (d.data.time_stamp, d.sig.as_ref().to_vec());
        assert!(
            prev < cur,
            "deploys must be sorted by (timestamp, sig): {:?} then {:?}",
            prev,
            cur
        );
        prev = cur;
    }
}

/// `PendingDeploysSnapshot::empty()` is a convenience for the no-casper
/// path and unit tests that need a zero-value default.
#[test]
fn empty_snapshot_is_zero() {
    let s = PendingDeploysSnapshot::empty();
    assert!(s.deploys.is_empty());
    assert_eq!(s.total_available, 0);
}

/// A deploy whose validity window has not opened yet is still queued. It is not
/// proposable into the next block, but it is submitted and will land once the
/// chain reaches its `valid_after_block_number` — reporting it as absent would
/// tell a wallet its deploy vanished.
#[tokio::test]
async fn a_future_dated_deploy_is_still_reported_as_pending() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;

    let base = construct_deploy::basic_deploy_data(21, None, None).expect("deploy");
    let mut data = base.data.clone();
    data.valid_after_block_number = 100_000;
    let future = crypto::rust::signatures::signed::Signed::create(
        data,
        Box::new(crypto::rust::signatures::secp256k1::Secp256k1),
        construct_deploy::DEFAULT_SEC.clone(),
    )
    .expect("resign");

    nodes[0]
        .casper
        .deploy_storage
        .lock()
        .add(vec![future.clone()])
        .expect("add future deploy");

    let snapshot = BlockAPI::list_pending_deploys(&engine_cell, None)
        .await
        .expect("snapshot");

    assert_eq!(
        snapshot.deploys.len(),
        1,
        "a deploy ahead of its window is queued, not gone"
    );
    assert_eq!(snapshot.deploys[0].0.sig, future.sig);
}

/// The recovery backlog gets the same window test as the fresh pool: a deploy
/// whose expiration passed while it sat in the buffer can never land, so it is
/// not pending. The buffer is only purged when a proposal runs.
#[tokio::test]
async fn an_expired_deploy_in_the_rejected_buffer_is_not_reported() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;

    let live = construct_deploy::basic_deploy_data(22, None, None).expect("live");
    let base = construct_deploy::basic_deploy_data(23, None, None).expect("expired");
    let mut data = base.data.clone();
    data.expiration_timestamp = Some(1);
    let expired = crypto::rust::signatures::signed::Signed::create(
        data,
        Box::new(crypto::rust::signatures::secp256k1::Secp256k1),
        construct_deploy::DEFAULT_SEC.clone(),
    )
    .expect("resign");

    nodes[0]
        .casper
        .rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .add(vec![live.clone(), expired.clone()])
        .expect("add rejected deploys");

    let snapshot = BlockAPI::list_pending_deploys(&engine_cell, None)
        .await
        .expect("snapshot");

    let sigs: Vec<_> = snapshot
        .deploys
        .iter()
        .map(|(d, _)| d.sig.clone())
        .collect();
    assert!(
        sigs.contains(&live.sig),
        "a live rejected deploy is still awaiting retry"
    );
    assert!(
        !sigs.contains(&expired.sig),
        "an expired rejected deploy can never land, so it is not pending"
    );
}
