// Proof tests for BlockAPI::deploy_finalization_status. Covers the
// API-surface states that can be triggered with the existing
// single-node TestNode fixture.

use std::collections::HashMap;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use casper::rust::api::block_api::BlockAPI;
use casper::rust::api::deploy_finalization_status::{self, DeployFinalizationState};
use casper::rust::casper::MultiParentCasper;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::engine_with_casper::EngineWithCasper;
use casper::rust::engine::multi_parent_casper::MultiParentCasperImpl;
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
        casper_buffer_storage: node.casper.casper_buffer_storage.clone(),
        validator_id: node.casper.validator_id.clone(),
        casper_shard_conf: node.casper.casper_shard_conf.clone(),
        approved_block: node.casper.approved_block.clone(),
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

/// A sig never seen anywhere in the DAG returns the "unknown" pending state:
/// no rejection count, no latest block hash. Regression guard for the most
/// common polling case (client polls right after deploy submission).
#[tokio::test]
async fn unknown_sig_returns_pending_with_empty_fields() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;

    let unknown_sig = vec![0xAA; 32];
    let status = BlockAPI::deploy_finalization_status(&engine_cell, &unknown_sig)
        .await
        .expect("resolver should not fail");

    assert_eq!(status.state, DeployFinalizationState::Pending);
    assert_eq!(status.rejection_count, 0);
    assert!(
        status.latest_block_hash.is_none(),
        "unknown sig must have no latest_block_hash, got {:?}",
        status.latest_block_hash
    );
}

/// Calls the pure `resolve` function directly (bypassing the async
/// `BlockAPI` wrapper) to confirm it is callable from non-engine-cell
/// contexts. This path is what the catchup gate in
/// `compute_parents_post_state` uses — the gate is not invoked in this
/// single-node test, but the pure-function signature contract is.
#[tokio::test]
async fn resolve_pure_function_returns_pending_for_unknown_sig() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();

    let dag = nodes[0]
        .casper
        .block_dag()
        .await
        .expect("fetch dag representation");
    let block_store = nodes[0].casper.block_store();
    let deploy_lifespan = nodes[0].casper.casper_shard_conf().deploy_lifespan;

    let unknown_sig = vec![0xBB; 32];
    let status =
        deploy_finalization_status::resolve(&dag, block_store, deploy_lifespan, &unknown_sig)
            .expect("resolve should not fail for unknown sig");

    assert_eq!(status.state, DeployFinalizationState::Pending);
    assert_eq!(status.rejection_count, 0);
    assert!(status.latest_block_hash.is_none());
}

#[tokio::test]
async fn resolve_keeps_old_finalized_inclusion_after_rolling_window_advances() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{ProcessedDeploy, RejectedDeploy};

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let deploy = construct_deploy::source_deploy_now_full(
        "@1!(1)".to_string(),
        None,
        None,
        None,
        Some(0),
        None,
    )
    .expect("construct deploy");
    let deploy_sig = deploy.sig.clone();
    let carrier = block_implicits::get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_block.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy)]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    block_store
        .put_block_message(&carrier)
        .expect("store carrier");
    dag_storage
        .insert(&carrier, InsertMode::Normal)
        .expect("dag carrier");

    let mut parent_hash = carrier.block_hash.clone();
    let mut duplicate_rejection_hash = None;
    for height in 2_i64..=4 {
        let mut block = block_implicits::get_random_block(
            Some(height),
            Some(height as i32),
            None,
            None,
            None,
            None,
            Some(0),
            Some(vec![parent_hash]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(genesis_block.body.state.bonds.clone()),
            Some(genesis_block.shard_id.clone()),
            None,
        );
        if height == 2 {
            block.body.rejected_deploys = vec![RejectedDeploy {
                sig: deploy_sig.clone(),
                duplicate: true,
                carrier: carrier.block_hash.clone(),
            }];
            duplicate_rejection_hash = Some(block.block_hash.clone());
        }
        block_store
            .put_block_message(&block)
            .expect("store descendant");
        dag_storage
            .insert(&block, InsertMode::Normal)
            .expect("dag descendant");
        parent_hash = block.block_hash.clone();
    }

    let mut dag = dag_storage
        .get_representation()
        .expect("get representation");
    dag.last_finalized_block_hash = parent_hash;

    let status = deploy_finalization_status::resolve(&dag, &block_store, 2, &deploy_sig)
        .expect("resolve should not fail");

    assert_eq!(status.state, DeployFinalizationState::Finalized);
    assert_eq!(status.latest_block_hash, duplicate_rejection_hash);
    assert_eq!(status.rejection_count, 1);
}

/// Regression test for the resolver's multi-parent DAG coverage.
///
/// Builds a minimal multi-parent DAG:
///
/// ```text
///     genesis (h=0)
///       |   |
///       A   B       both at h=1, children of genesis
///       |   |
///        \ /
///         C         at h=2, parents=[A, B] with A as main-parent; LFB
/// ```
///
/// The deploy sig under test lives only in `B.body.deploys`. B reaches
/// canonical state via C's secondary-parent slot, not via the main-parent
/// chain from C.
///
/// A main-parent-only walk (`dag.main_parent_chain(C, _)`) visits
/// `C → A → genesis` and never touches B, so it misses the sig and the
/// resolver reports `Pending`. A BFS over all parents visits B through C's
/// secondary slot, finds the sig in `body.deploys`, and reports `Finalized`.
///
/// This test exists to keep the BFS semantics (over `parents_hash_list`, not
/// just `main_parent`) locked in.
#[tokio::test]
async fn resolve_finds_sig_in_secondary_parent_branch() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::ProcessedDeploy;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let deploy_b =
        construct_deploy::source_deploy_now_full("Nil".to_string(), None, None, None, None, None)
            .expect("construct deploy_b");
    let deploy_b_sig = deploy_b.sig.to_vec();

    // Block A: empty-body sibling of genesis at h=1.
    let block_a = block_implicits::get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    // Block B: sibling of A at h=1, carries deploy_b in body.deploys.
    let block_b = block_implicits::get_random_block(
        Some(1),
        Some(2),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy_b)]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    // Block C: merge of [A, B] with A as main parent.
    let block_c = block_implicits::get_random_block(
        Some(2),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![block_a.block_hash.clone(), block_b.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    block_store.put_block_message(&block_a).expect("store A");
    block_store.put_block_message(&block_b).expect("store B");
    block_store.put_block_message(&block_c).expect("store C");
    dag_storage
        .insert(&block_a, InsertMode::Normal)
        .expect("dag insert A");
    dag_storage
        .insert(&block_b, InsertMode::Normal)
        .expect("dag insert B");
    dag_storage
        .insert(&block_c, InsertMode::Normal)
        .expect("dag insert C");

    // Promote C to LFB so the resolver's scan starts there. The DAG state
    // normally bumps LFB only via the finalization pipeline; for this unit
    // test we overwrite the representation's field directly.
    let mut dag = dag_storage
        .get_representation()
        .expect("get_representation");
    dag.last_finalized_block_hash = block_c.block_hash.clone();

    let deploy_lifespan = 50i64;
    let status =
        deploy_finalization_status::resolve(&dag, &block_store, deploy_lifespan, &deploy_b_sig)
            .expect("resolve should not fail");

    assert_eq!(
        status.state,
        DeployFinalizationState::Finalized,
        "sig in secondary-parent ancestor of LFB should be Finalized; got {:?}",
        status.state
    );
    assert_eq!(
        status.latest_block_hash.as_ref(),
        Some(&block_b.block_hash),
        "latest_block_hash must point at B (the block actually containing the sig)"
    );
    assert_eq!(status.rejection_count, 0);
}

/// A sig in an unfinalized block past `valid_after + lifespan` is still
/// in flight — its host block can finalize and the deploy's effects can
/// land. The expiry threshold is anchored to LFB height so the resolver
/// reports `Pending` (not `Expired`) until LFB advances past the cutoff.
///
/// DAG shape:
///
/// ```text
///   genesis (h=0, LFB)
///     |
///     B (h=1)               unfinalized; carries sig X with valid_after=0
/// ```
///
/// With `deploy_lifespan = 0`:
///   - tip (= 1) > valid_after (0) + lifespan (0) = 0
///   - LFB (= 0) is NOT past the cutoff
///   - Sig X awaits finalization of B
#[tokio::test]
async fn resolve_returns_pending_for_unfinalized_inclusion_past_lifespan() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::ProcessedDeploy;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    // Deploy with explicit valid_after_block_number = 0.
    let deploy = construct_deploy::source_deploy_now_full(
        "@1!(1)".to_string(),
        None,
        None,
        None,
        Some(0),
        None,
    )
    .expect("construct deploy");
    let deploy_sig = deploy.sig.clone();

    // Block at height 1, parent = genesis. UNFINALIZED — DAG will leave
    // LFB at genesis (h=0) since we never explicitly finalize block_b.
    let block_b = block_implicits::get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy)]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    block_store.put_block_message(&block_b).expect("store B");
    dag_storage
        .insert(&block_b, InsertMode::Normal)
        .expect("dag insert B");

    // LFB stays at genesis (h=0). Block B sits unfinalized at h=1.
    let dag = dag_storage
        .get_representation()
        .expect("get_representation");

    // Lifespan = 0 makes the cutoff equal to valid_after_block_number (0),
    // so tip (1) > 0 → the buggy tip-based expiry triggers; LFB (0) is NOT
    // greater than 0 → the LFB-based expiry does NOT trigger. The fix is
    // visible in the difference.
    let deploy_lifespan = 0i64;
    let status =
        deploy_finalization_status::resolve(&dag, &block_store, deploy_lifespan, &deploy_sig)
            .expect("resolve should not fail");

    assert_eq!(
        status.state,
        DeployFinalizationState::Pending,
        "sig in unfinalized block past lifespan must be Pending until LFB \
         advances past the cutoff; got {:?}",
        status.state,
    );
}

/// `Failed` and `Finalized` decisions both apply the canonical-descendant
/// gate in a multi-parent DAG. A failed inclusion in a non-main-chain
/// finalized sibling (visited via a secondary parent in the BFS) does not
/// terminate the state machine — the latest canonical inclusion wins.
///
/// DAG shape:
///
/// ```text
///   genesis (h=0)
///       |
///       A (h=1)            canonical main-parent
///      / \
///     B   S (both h=2)     B is canonical (main_parent=A), S is sibling
///      \ /                 (main_parent=A but NOT on LFB main chain)
///       C (h=3, LFB)       multi-parent merge: parents=[B, S]; main=B
///       |
///       D (h=4)            canonical clean inclusion of sig X
/// ```
///
/// Sig X appears in:
///   - S.body.deploys with `is_failed=true` (non-canonical sibling)
///   - D.body.deploys with `is_failed=false` (canonical, higher height)
///
/// The failed event in S is gated out (S is not on LFB's main-parent
/// chain); the latest canonical inclusion is D's clean event → `Finalized`.
#[tokio::test]
async fn resolve_returns_finalized_for_clean_canonical_after_failed_secondary() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::ProcessedDeploy;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let deploy_failed_then_clean = construct_deploy::source_deploy_now_full(
        "@9!(9)".to_string(),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("construct deploy");
    let sig_under_test = deploy_failed_then_clean.sig.clone();

    let mut pd_failed = ProcessedDeploy::empty(deploy_failed_then_clean.clone());
    pd_failed.is_failed = true;
    let pd_clean = ProcessedDeploy::empty(deploy_failed_then_clean.clone());

    // Block A: h=1, canonical, parent=genesis. Empty body.
    let block_a = block_implicits::get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    // Block B: h=2, canonical, main_parent=A. Empty body.
    let block_b = block_implicits::get_random_block(
        Some(2),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![block_a.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    // Block S: h=2, sibling of B (also main_parent=A). Carries sig with
    // is_failed=true.
    let block_s = block_implicits::get_random_block(
        Some(2),
        Some(2),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![block_a.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![pd_failed]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    // Block C: h=3, multi-parent merge of [B, S]. Main parent = B.
    let block_c = block_implicits::get_random_block(
        Some(3),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![block_b.block_hash.clone(), block_s.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    // Block D: h=4, canonical clean inclusion of sig X. main_parent=C.
    let block_d = block_implicits::get_random_block(
        Some(4),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![block_c.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![pd_clean]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    block_store.put_block_message(&block_a).expect("store A");
    block_store.put_block_message(&block_b).expect("store B");
    block_store.put_block_message(&block_s).expect("store S");
    block_store.put_block_message(&block_c).expect("store C");
    block_store.put_block_message(&block_d).expect("store D");
    dag_storage
        .insert(&block_a, InsertMode::Normal)
        .expect("dag insert A");
    dag_storage
        .insert(&block_b, InsertMode::Normal)
        .expect("dag insert B");
    dag_storage
        .insert(&block_s, InsertMode::Normal)
        .expect("dag insert S");
    dag_storage
        .insert(&block_c, InsertMode::Normal)
        .expect("dag insert C");
    dag_storage
        .insert(&block_d, InsertMode::Normal)
        .expect("dag insert D");

    // LFB = D (the clean canonical inclusion).
    let mut dag = dag_storage
        .get_representation()
        .expect("get_representation");
    dag.last_finalized_block_hash = block_d.block_hash.clone();

    let deploy_lifespan = 50i64;
    let status =
        deploy_finalization_status::resolve(&dag, &block_store, deploy_lifespan, &sig_under_test)
            .expect("resolve should not fail");

    assert_eq!(
        status.state,
        DeployFinalizationState::Finalized,
        "clean canonical inclusion at D must win over failed event in \
         non-canonical sibling S; got {:?}",
        status.state,
    );
    assert_eq!(
        status.latest_block_hash.as_ref(),
        Some(&block_d.block_hash),
        "latest_block_hash must point at D (the canonical clean inclusion), \
         not S (the failed sibling)",
    );
}

/// Same-chain symmetric gate: a deploy that fails canonically at A, gets
/// canonical-descendant-rejected at B, and is re-tried clean canonically
/// at C must resolve to `Finalized`. The latest canonical inclusion (C
/// clean at h=3) wins over the earlier failed inclusion at A. Without
/// this, `repeat_deploy` would exempt the sig as a recovery candidate
/// — allowing double-execution of a canonically-clean deploy.
///
/// DAG shape (single chain, no multi-parent):
///
/// ```text
///   genesis (h=0)
///       |
///       A (h=1)            canonical; sig X with is_failed=true
///       |
///       B (h=2)            canonical; sig X in body.rejected_deploys
///       |                  (canonical-descendant rejection of A's failed
///       |                   inclusion — recovery flow's first step)
///       C (h=3, LFB)       canonical; sig X clean (recovery succeeded)
/// ```
#[tokio::test]
async fn resolve_returns_finalized_when_canonical_clean_supersedes_canonical_failed() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{ProcessedDeploy, RejectedDeploy};

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let deploy = construct_deploy::source_deploy_now_full(
        "@7!(7)".to_string(),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("construct deploy");
    let sig_under_test = deploy.sig.clone();

    let mut pd_failed = ProcessedDeploy::empty(deploy.clone());
    pd_failed.is_failed = true;
    let pd_clean = ProcessedDeploy::empty(deploy.clone());

    // Block A: h=1, canonical, sig with is_failed=true.
    let block_a = block_implicits::get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![pd_failed]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    // Block B: h=2, canonical descendant of A. sig in rejected_deploys.
    let mut block_b = block_implicits::get_random_block(
        Some(2),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![block_a.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    block_b.body.rejected_deploys = vec![RejectedDeploy {
        sig: sig_under_test.clone(),
        duplicate: false,
        carrier: prost::bytes::Bytes::new(),
    }];

    // Block C: h=3 LFB, canonical descendant of B. sig clean (recovery
    // succeeded after B's rejection).
    let block_c = block_implicits::get_random_block(
        Some(3),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![block_b.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![pd_clean]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    block_store.put_block_message(&block_a).expect("store A");
    block_store.put_block_message(&block_b).expect("store B");
    block_store.put_block_message(&block_c).expect("store C");
    dag_storage
        .insert(&block_a, InsertMode::Normal)
        .expect("dag insert A");
    dag_storage
        .insert(&block_b, InsertMode::Normal)
        .expect("dag insert B");
    dag_storage
        .insert(&block_c, InsertMode::Normal)
        .expect("dag insert C");

    let mut dag = dag_storage
        .get_representation()
        .expect("get_representation");
    dag.last_finalized_block_hash = block_c.block_hash.clone();

    let deploy_lifespan = 50i64;
    let status =
        deploy_finalization_status::resolve(&dag, &block_store, deploy_lifespan, &sig_under_test)
            .expect("resolve should not fail");

    assert_eq!(
        status.state,
        DeployFinalizationState::Finalized,
        "canonical clean at C must supersede canonical failed at A; got {:?}",
        status.state,
    );
    assert_eq!(
        status.latest_block_hash.as_ref(),
        Some(&block_c.block_hash),
        "latest_block_hash must point at C (the canonical clean inclusion)",
    );
    // Rejection count should reflect B's canonical-descendant rejection event.
    assert_eq!(
        status.rejection_count, 1,
        "exactly one canonical-chain rejection event (in block B)",
    );
}

/// "Indexed but missing from body" is the case where the deploy index
/// claims a sig lives in some block, but that block's `body.deploys` does
/// not list the sig. The resolver returns a typed `DeployFinalizationCorruption`
/// error so the consensus path (`repeat_deploy`) conservative-fails (keep
/// the sig in the check set rather than exempting it as a recovery
/// candidate). `BlockAPI::deploy_finalization_status` downcasts and
/// converts to `pending_unknown` at the HTTP/gRPC boundary so callers
/// see a tractable response. The `f1r3fly.deploy_finalization_status.corruption`
/// warn target gives operators visibility for the inconsistency.
#[tokio::test]
async fn resolve_returns_typed_err_for_indexed_but_missing_from_body() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::api::deploy_finalization_status::DeployFinalizationCorruption;
    use models::rust::block_hash::BlockHashSerde;
    use models::rust::block_implicits;
    use prost::bytes::Bytes;
    use shared::rust::store::key_value_typed_store::KeyValueTypedStore;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    // Build a block with NO deploys in its body.
    let block_a = block_implicits::get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()), // empty body.deploys
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    block_store.put_block_message(&block_a).expect("store A");
    dag_storage
        .insert(&block_a, InsertMode::Normal)
        .expect("dag insert A");

    // Inject the inconsistency: write a fake mapping into the deploy index
    // claiming `corrupt_sig` lives in block_a, even though A's body does
    // not list it.
    let corrupt_sig = vec![0xDEu8; 32];
    {
        let deploy_index_handle = dag_storage.deploy_index_for_tests();
        let deploy_index_guard = deploy_index_handle.write();
        deploy_index_guard
            .put(vec![(
                Bytes::from(corrupt_sig.clone()).into(),
                BlockHashSerde(block_a.block_hash.clone()),
            )])
            .expect("inject corrupt deploy_index entry");
    }

    let dag = dag_storage
        .get_representation()
        .expect("get_representation");
    let deploy_lifespan = 50i64;

    let result =
        deploy_finalization_status::resolve(&dag, &block_store, deploy_lifespan, &corrupt_sig);

    let err = result.expect_err(
        "indexed-but-missing-from-body must propagate Err so repeat_deploy fails-conservative",
    );
    let corruption = err.downcast_ref::<DeployFinalizationCorruption>().expect(
        "Err must carry a DeployFinalizationCorruption sentinel so block_api can detect \
             and convert it; got {err}",
    );
    assert_eq!(
        corruption.sig.as_ref(),
        corrupt_sig.as_slice(),
        "sentinel must carry the corrupt sig",
    );
    assert_eq!(
        corruption.block_hash, block_a.block_hash,
        "sentinel must carry the inconsistent block hash",
    );
}

#[tokio::test]
async fn resolve_with_known_block_uses_fallback_block_when_deploy_index_misses() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::ProcessedDeploy;
    use prost::bytes::Bytes;
    use shared::rust::store::key_value_typed_store::KeyValueTypedStore;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let deploy =
        construct_deploy::source_deploy_now_full("Nil".to_string(), None, None, None, None, None)
            .expect("construct deploy");
    let deploy_sig = deploy.sig.to_vec();
    let block_a = block_implicits::get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy)]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    block_store.put_block_message(&block_a).expect("store A");
    dag_storage
        .insert(&block_a, InsertMode::Normal)
        .expect("dag insert A");

    {
        let deploy_index_handle = dag_storage.deploy_index_for_tests();
        let deploy_index_guard = deploy_index_handle.write();
        deploy_index_guard
            .delete(vec![Bytes::from(deploy_sig.clone()).into()])
            .expect("remove deploy index entry");
    }

    let mut dag = dag_storage
        .get_representation()
        .expect("dag representation");
    dag.last_finalized_block_hash = block_a.block_hash.clone();
    let deploy_lifespan = 50i64;

    let without_known_block =
        deploy_finalization_status::resolve(&dag, &block_store, deploy_lifespan, &deploy_sig)
            .expect("index-miss resolve should not fail");
    assert_eq!(without_known_block.state, DeployFinalizationState::Pending);
    assert!(without_known_block.latest_block_hash.is_none());

    let with_known_block = deploy_finalization_status::resolve_with_known_block(
        &dag,
        &block_store,
        deploy_lifespan,
        &deploy_sig,
        Some(&block_a.block_hash),
    )
    .expect("known-block resolve should not fail");

    assert_eq!(with_known_block.state, DeployFinalizationState::Finalized);
    assert_eq!(
        with_known_block.latest_block_hash.as_ref(),
        Some(&block_a.block_hash),
    );
    assert_eq!(with_known_block.rejection_count, 0);
}

/// Symmetric clean-side canonical-descendant gate.
///
/// A clean inclusion in a non-main-chain finalized sibling whose effects
/// are rejected at the canonical merge step must not resolve to
/// `Finalized`. The non-canonical clean event has to be invalidated by
/// the canonical rejection the same way `is_in_main_chain` invalidates a
/// canonical clean event when a canonical-descendant rejection exists.
///
/// DAG shape:
///
/// ```text
///   genesis (h=0)
///       |
///       A (h=1)               canonical
///      / \
///     B   Y (both h=2)        B canonical, Y non-canonical sibling
///      \ /                    (main_parent=A but not on LFB main chain)
///       C (h=3, LFB)          merge of [B, Y]; main parent = B.
///                             Body.rejected_deploys = [sig_X].
/// ```
///
/// Sig X appears in Y.body.deploys (clean) and C.body.rejected_deploys
/// (canonical merge rejection). Without the symmetric gate the resolver
/// returns `Finalized` because `is_in_main_chain(Y, C) = false` keeps
/// the existing canonical-descendant rule from firing — but Y is itself
/// non-canonical, and C's rejection records that the merge dropped the
/// effects when integrating Y. Sig X is not in canonical state.
///
/// `repeat_deploy` would treat the (incorrect) `Finalized` as kept-in-check
/// but the ancestor scan over canonical main-parent chain would not find
/// the sig (it lives only in non-canonical Y), letting a re-proposal
/// validate and re-execute → double-execution.
#[tokio::test]
async fn resolve_returns_pending_for_non_canonical_clean_with_canonical_reject() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{ProcessedDeploy, RejectedDeploy};

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let deploy = construct_deploy::source_deploy_now_full(
        "@8!(8)".to_string(),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("construct deploy");
    let sig_under_test = deploy.sig.clone();

    // A: h=1, canonical, empty body.
    let block_a = block_implicits::get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    // B: h=2, canonical (main parent of LFB), empty body.
    let block_b = block_implicits::get_random_block(
        Some(2),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![block_a.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    // Y: h=2, non-canonical sibling of B. Carries sig_X clean.
    let block_y = block_implicits::get_random_block(
        Some(2),
        Some(2),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![block_a.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy.clone())]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    // C: h=3, LFB. Multi-parent merge of [B, Y]. body.rejected_deploys
    // contains sig_X (the merge engine rejected the deploy when
    // integrating Y's chain).
    let mut block_c = block_implicits::get_random_block(
        Some(3),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![block_b.block_hash.clone(), block_y.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    block_c.body.rejected_deploys = vec![RejectedDeploy {
        sig: sig_under_test.clone(),
        duplicate: false,
        carrier: prost::bytes::Bytes::new(),
    }];

    block_store.put_block_message(&block_a).expect("store A");
    block_store.put_block_message(&block_b).expect("store B");
    block_store.put_block_message(&block_y).expect("store Y");
    block_store.put_block_message(&block_c).expect("store C");
    dag_storage
        .insert(&block_a, InsertMode::Normal)
        .expect("dag insert A");
    dag_storage
        .insert(&block_b, InsertMode::Normal)
        .expect("dag insert B");
    dag_storage
        .insert(&block_y, InsertMode::Normal)
        .expect("dag insert Y");
    dag_storage
        .insert(&block_c, InsertMode::Normal)
        .expect("dag insert C");

    let mut dag = dag_storage
        .get_representation()
        .expect("get_representation");
    dag.last_finalized_block_hash = block_c.block_hash.clone();

    let deploy_lifespan = 50i64;
    let status =
        deploy_finalization_status::resolve(&dag, &block_store, deploy_lifespan, &sig_under_test)
            .expect("resolve should not fail");

    assert_eq!(
        status.state,
        DeployFinalizationState::Pending,
        "non-canonical clean inclusion + canonical rejection must NOT resolve \
         to Finalized; got {:?}",
        status.state,
    );
    assert_eq!(
        status.rejection_count, 1,
        "exactly one canonical rejection event in C",
    );
}

#[tokio::test]
async fn resolve_accepts_later_secondary_recovery_after_canonical_rejection() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{ProcessedDeploy, RejectedDeploy};

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");
    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let deploy = construct_deploy::source_deploy_now_full(
        "@9!(9)".to_string(),
        None,
        None,
        None,
        Some(0),
        None,
    )
    .expect("construct deploy");
    let sig = deploy.sig.clone();
    let make_block = |height, seq, parents, deploys| {
        block_implicits::get_random_block(
            Some(height),
            Some(seq),
            None,
            None,
            None,
            None,
            Some(0),
            Some(parents),
            Some(Vec::new()),
            Some(deploys),
            Some(Vec::new()),
            Some(genesis_block.body.state.bonds.clone()),
            Some(genesis_block.shard_id.clone()),
            None,
        )
    };

    let block_a = make_block(1, 1, vec![genesis_block.block_hash.clone()], Vec::new());
    let mut rejected = make_block(2, 1, vec![block_a.block_hash.clone()], Vec::new());
    rejected.body.rejected_deploys = vec![RejectedDeploy {
        sig: sig.clone(),
        duplicate: false,
        carrier: prost::bytes::Bytes::new(),
    }];
    let canonical = make_block(3, 1, vec![rejected.block_hash.clone()], Vec::new());
    let recovered = make_block(3, 2, vec![rejected.block_hash.clone()], vec![
        ProcessedDeploy::empty(deploy),
    ]);
    let lfb = make_block(
        4,
        1,
        vec![canonical.block_hash.clone(), recovered.block_hash.clone()],
        Vec::new(),
    );

    for block in [&block_a, &rejected, &canonical, &recovered, &lfb] {
        block_store.put_block_message(block).expect("store block");
        dag_storage
            .insert(block, InsertMode::Normal)
            .expect("dag insert block");
    }
    let mut dag = dag_storage
        .get_representation()
        .expect("get representation");
    dag.last_finalized_block_hash = lfb.block_hash.clone();

    let status = deploy_finalization_status::resolve(&dag, &block_store, 50, &sig)
        .expect("resolve should not fail");

    assert_eq!(status.state, DeployFinalizationState::Finalized);
    assert_eq!(status.rejection_count, 1);
    assert_eq!(status.latest_block_hash, Some(recovered.block_hash));
}

#[tokio::test]
async fn resolve_waits_for_visible_unfinalized_rejection_disposition() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{ProcessedDeploy, RejectedDeploy};

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");
    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let deploy = construct_deploy::source_deploy_now_full(
        "@10!(10)".to_string(),
        None,
        None,
        None,
        Some(0),
        None,
    )
    .expect("construct deploy");
    let sig = deploy.sig.clone();
    let clean = block_implicits::get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_block.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy)]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    let mut rejected = block_implicits::get_random_block(
        Some(2),
        Some(2),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![clean.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    rejected.body.rejected_deploys = vec![RejectedDeploy {
        sig: sig.clone(),
        duplicate: false,
        carrier: clean.block_hash.clone(),
    }];

    for block in [&clean, &rejected] {
        block_store.put_block_message(block).expect("store block");
        dag_storage
            .insert(block, InsertMode::Normal)
            .expect("dag insert block");
    }
    let mut dag = dag_storage
        .get_representation()
        .expect("get representation");
    dag.last_finalized_block_hash = clean.block_hash.clone();

    let status = deploy_finalization_status::resolve(&dag, &block_store, 50, &sig)
        .expect("resolve should not fail");

    assert_eq!(status.state, DeployFinalizationState::Pending);
    assert_eq!(status.rejection_count, 0);
    assert_eq!(status.latest_block_hash, Some(rejected.block_hash));
}

#[tokio::test]
async fn resolve_waits_when_later_rejection_targets_a_newer_clean_carrier() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{ProcessedDeploy, RejectedDeploy};

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");
    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let deploy = construct_deploy::source_deploy_now_full(
        "@11!(11)".to_string(),
        None,
        None,
        None,
        Some(0),
        None,
    )
    .expect("construct deploy");
    let sig = deploy.sig.clone();
    let make_clean = |height, seq, parent| {
        block_implicits::get_random_block(
            Some(height),
            Some(seq),
            None,
            None,
            None,
            None,
            Some(0),
            Some(vec![parent]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            Some(genesis_block.body.state.bonds.clone()),
            Some(genesis_block.shard_id.clone()),
            None,
        )
    };
    let older_clean = make_clean(1, 1, genesis_block.block_hash.clone());
    let newer_clean = make_clean(2, 2, older_clean.block_hash.clone());
    let mut rejected = block_implicits::get_random_block(
        Some(3),
        Some(3),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![newer_clean.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    rejected.body.rejected_deploys = vec![RejectedDeploy {
        sig: sig.clone(),
        duplicate: false,
        carrier: newer_clean.block_hash.clone(),
    }];

    for block in [&older_clean, &newer_clean, &rejected] {
        block_store.put_block_message(block).expect("store block");
        dag_storage
            .insert(block, InsertMode::Normal)
            .expect("dag insert block");
    }
    let mut dag = dag_storage
        .get_representation()
        .expect("get representation");
    dag.last_finalized_block_hash = rejected.block_hash.clone();

    let status = deploy_finalization_status::resolve(&dag, &block_store, 50, &sig)
        .expect("resolve should not fail");

    assert_eq!(status.state, DeployFinalizationState::Pending);
    assert_eq!(status.rejection_count, 1);
    assert_eq!(status.latest_block_hash, Some(rejected.block_hash));
}

#[tokio::test]
async fn resolve_ignores_equal_height_sibling_rejection() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{ProcessedDeploy, RejectedDeploy};

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mk_test_rnode_store_manager_from_genesis,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let mut kvm = mk_test_rnode_store_manager_from_genesis(&ctx.genesis);
    let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");
    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let deploy = construct_deploy::source_deploy_now_full(
        "@12!(12)".to_string(),
        None,
        None,
        None,
        Some(0),
        None,
    )
    .expect("construct deploy");
    let sig = deploy.sig.clone();
    let clean = block_implicits::get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_block.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy)]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    let mut rejected = block_implicits::get_random_block(
        Some(1),
        Some(2),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![genesis_block.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    rejected.body.rejected_deploys = vec![RejectedDeploy {
        sig: sig.clone(),
        duplicate: false,
        carrier: clean.block_hash.clone(),
    }];
    let lfb = block_implicits::get_random_block(
        Some(2),
        Some(3),
        None,
        None,
        None,
        None,
        Some(0),
        Some(vec![clean.block_hash.clone(), rejected.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );

    for block in [&clean, &rejected, &lfb] {
        block_store.put_block_message(block).expect("store block");
        dag_storage
            .insert(block, InsertMode::Normal)
            .expect("dag insert block");
    }
    let mut dag = dag_storage
        .get_representation()
        .expect("get representation");
    dag.last_finalized_block_hash = lfb.block_hash;

    let status = deploy_finalization_status::resolve(&dag, &block_store, 50, &sig)
        .expect("resolve should not fail");

    assert_eq!(status.state, DeployFinalizationState::Finalized);
    assert_eq!(status.rejection_count, 1);
    assert_eq!(status.latest_block_hash, Some(rejected.block_hash));
}
