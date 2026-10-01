// Proof tests for BlockAPI::deploy_finalization_status and the pure
// `resolve` lookup.
//
// The resolver is a LOOKUP over the deploy-lifecycle register: terminal
// verdicts are determined once by `finality::deploy_lifecycle` (specced in
// `finalized_floor::deploy_lifecycle_spec`) and persisted write-once; this
// file pins the lookup surface — terminal-record mapping, open-row Pending
// display, the unknown/fallback preludes, and the corruption sentinel —
// plus one end-to-end pin that the block-admission hook drives verdicts on
// a real node.

use std::collections::HashMap;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use block_storage::rust::dag::deploy_lifecycle_types::{TerminalRecord, TerminalState};
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
        divergence_monitor: node.casper.divergence_monitor.clone(),
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

/// A sig never seen anywhere returns the "unknown" pending state: no
/// rejection count, no latest block hash. Regression guard for the most
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
/// contexts.
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

    let unknown_sig = vec![0xBB; 32];
    let status = deploy_finalization_status::resolve(&dag, block_store, &unknown_sig, None)
        .expect("resolve should not fail for unknown sig");

    assert_eq!(status.state, DeployFinalizationState::Pending);
    assert_eq!(status.rejection_count, 0);
    assert!(status.latest_block_hash.is_none());
}

/// The lookup maps every terminal register state onto the API enum with
/// the record's frozen display fields — no recomputation anywhere.
#[tokio::test]
async fn terminal_records_map_states_and_frozen_fields() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let dag = nodes[0].casper.block_dag().await.expect("dag");
    let block_store = nodes[0].casper.block_store();

    let cases = [
        (TerminalState::Finalized, DeployFinalizationState::Finalized),
        (TerminalState::Expired, DeployFinalizationState::Expired),
        (TerminalState::Failed, DeployFinalizationState::Failed),
    ];
    for (i, (register_state, api_state)) in cases.into_iter().enumerate() {
        let sig = vec![0xC0 + i as u8; 32];
        let latest = vec![0xD0 + i as u8; 32];
        dag.put_deploy_terminal_if_absent(&sig, TerminalRecord {
            state: register_state,
            rejection_count: 3,
            latest_height: 7,
            latest_block_hash: latest.clone(),
        })
        .expect("write terminal record");

        let status =
            deploy_finalization_status::resolve(&dag, block_store, &sig, None).expect("resolve");
        assert_eq!(
            status.state, api_state,
            "state mapping for {:?}",
            register_state
        );
        assert_eq!(status.rejection_count, 3, "frozen rejection count");
        assert_eq!(
            status.latest_block_hash,
            Some(prost::bytes::Bytes::from(latest)),
            "frozen latest block hash"
        );
    }
}

/// The write-once contract at the lookup level: a second terminal write
/// for the same sig is refused, and the lookup keeps answering with the
/// first record.
#[tokio::test]
async fn terminal_lookup_is_write_once() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let dag = nodes[0].casper.block_dag().await.expect("dag");
    let block_store = nodes[0].casper.block_store();

    let sig = vec![0xE1; 32];
    dag.put_deploy_terminal_if_absent(&sig, TerminalRecord {
        state: TerminalState::Expired,
        rejection_count: 1,
        latest_height: 4,
        latest_block_hash: vec![0xE2; 32],
    })
    .expect("first terminal write");
    let survivor = dag
        .put_deploy_terminal_if_absent(&sig, TerminalRecord {
            state: TerminalState::Finalized,
            rejection_count: 9,
            latest_height: 9,
            latest_block_hash: vec![0xE3; 32],
        })
        .expect("second terminal write attempt");
    assert_eq!(
        survivor.state,
        TerminalState::Expired,
        "the store must refuse the overwrite and return the survivor"
    );

    let status =
        deploy_finalization_status::resolve(&dag, block_store, &sig, None).expect("resolve");
    assert_eq!(status.state, DeployFinalizationState::Expired);
    assert_eq!(status.rejection_count, 1);
}

/// A caller-claimed block whose body does not list the sig still
/// propagates the typed sentinel for the API layer to downcast — the
/// known-block fallback's consistency check.
#[tokio::test]
async fn resolve_returns_typed_err_for_claimed_but_missing_from_body() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::api::deploy_finalization_status::DeployFinalizationCorruption;
    use models::rust::block_implicits;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, generate_scope_id, mk_test_rnode_store_manager_shared,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();

    let mut kvm = mk_test_rnode_store_manager_shared(generate_scope_id());
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

    // The inconsistency arrives from the CALLER: a claimed block hash for a
    // sig that block's body does not list.
    let corrupt_sig = vec![0xDEu8; 32];
    let dag = dag_storage
        .get_representation()
        .expect("get_representation");

    let result = deploy_finalization_status::resolve(
        &dag,
        &block_store,
        &corrupt_sig,
        Some(&block_a.block_hash),
    );

    let err = result.expect_err(
        "indexed-but-missing-from-body must propagate a typed Err for the API layer to downcast",
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

/// A sig with no register row resolves against a caller-provided block
/// hash — the client-side fallback for a block the node holds but has
/// not yet inserted (so the register has not seen it).
#[tokio::test]
async fn resolve_uses_known_block_fallback_when_the_register_misses() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::ProcessedDeploy;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, generate_scope_id, mk_test_rnode_store_manager_shared,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();

    let mut kvm = mk_test_rnode_store_manager_shared(generate_scope_id());
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
    // Stored but NOT dag-inserted: the register never ingested it.
    block_store.put_block_message(&block_a).expect("store A");

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");

    let without_known_block =
        deploy_finalization_status::resolve(&dag, &block_store, &deploy_sig, None)
            .expect("index-miss resolve should not fail");
    assert_eq!(without_known_block.state, DeployFinalizationState::Pending);
    assert!(without_known_block.latest_block_hash.is_none());

    let with_known_block = deploy_finalization_status::resolve(
        &dag,
        &block_store,
        &deploy_sig,
        Some(&block_a.block_hash),
    )
    .expect("known-block resolve should not fail");

    // The fallback's observable: the sig resolves against the provided
    // block (latest_block_hash populated) instead of pending_unknown.
    assert_eq!(with_known_block.state, DeployFinalizationState::Pending);
    assert_eq!(
        with_known_block.latest_block_hash.as_ref(),
        Some(&block_a.block_hash),
    );
    assert_eq!(with_known_block.rejection_count, 0);
}

/// End to end through the ADMISSION HOOK: a real node proposes a deploy,
/// the chain settles past the register's horizon, and the API reports
/// Finalized — pinning that block admission actually drives the register
/// (ingest at insert, observe at admission) with no test-side plumbing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn end_to_end_win_finalizes_via_admission_hook() {
    use casper::rust::util::construct_deploy;

    // A SINGLE-validator genesis: with silent bonded validators the frozen
    // floor cannot advance (their witnessing weight never arrives — the
    // known liveness-committee prerequisite), so the one running validator
    // must hold all the stake for the register's floor clock to move.
    let genesis_parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .expect("build single-validator genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    // Finite parent depth — the register's contestability bound; the
    // TestNode default is the i32::MAX disabled sentinel.
    let mut nodes = TestNode::create_network(genesis, 1, None, None, Some(10), None)
        .await
        .expect("create_network");
    nodes[0].allow_empty_blocks = true;

    let deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"@"dfs_e2e"!(1)"#.to_string(),
            None,
            None,
            None,
            Some(0),
            Some(shard_id.clone()),
        )
        .expect("build deploy")
    };
    let sig = deploy.sig.clone();
    nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy))
        .await
        .expect("propose the win");

    // Settle: single-validator chain; each proposal flows through block
    // admission on the proposer itself, advancing the register's clocks.
    // The ladder spans the register's contestability bound: window_end
    // (lifespan 50) + depth bound (~20).
    let mut finalized = false;
    for round in 0..90i32 {
        let marker = {
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            construct_deploy::basic_deploy_data(round, None, Some(shard_id.clone()))
                .expect("marker")
        };
        nodes[0]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("settle proposal");
        let dag = nodes[0].casper.block_dag().await.expect("dag");
        let status =
            deploy_finalization_status::resolve(&dag, nodes[0].casper.block_store(), &sig, None)
                .expect("resolve");
        if status.state == DeployFinalizationState::Finalized {
            finalized = true;
            break;
        }
    }
    assert!(
        finalized,
        "the admission-hook-driven register must finalize the win within \
         the settle ladder"
    );
}

/// The register's ingest covers every inserted block regardless of how it
/// hangs in the DAG: a sig whose only inclusion is in a block reachable
/// solely through a SECONDARY parent slot still has a row, so the
/// resolver answers with that inclusion instead of `pending_unknown`.
/// (The strong form — such an inclusion FINALIZES via merge + floor
/// state — is pinned end-to-end by
/// `verdict_convergence_spec::a_deploy_finalizes_from_a_carrier_the_spine_never_holds`.)
///
/// ```text
///     genesis (h=0)
///       |   |
///       A   B       both at h=1; the sig lives only in B.body.deploys
///        \ /
///         C         h=2, parents=[A, B] — B is the secondary slot
/// ```
#[tokio::test]
async fn resolve_finds_sig_in_secondary_parent_branch() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::util::construct_deploy;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::ProcessedDeploy;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, generate_scope_id, mk_test_rnode_store_manager_shared,
    };

    let ctx = TestContext::new().await;
    let genesis_block = ctx.genesis.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();

    let mut kvm = mk_test_rnode_store_manager_shared(generate_scope_id());
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
            .expect("construct deploy");
    let deploy_b_sig = deploy_b.sig.to_vec();

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
        Some(vec![ProcessedDeploy::empty(deploy_b)]),
        Some(Vec::new()),
        Some(genesis_block.body.state.bonds.clone()),
        Some(genesis_block.shard_id.clone()),
        None,
    );
    let block_c = block_implicits::get_random_block(
        Some(2),
        Some(2),
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
    for block in [&block_a, &block_b, &block_c] {
        block_store.put_block_message(block).expect("store block");
        dag_storage
            .insert(block, InsertMode::Normal)
            .expect("dag insert");
    }

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    let status = deploy_finalization_status::resolve(&dag, &block_store, &deploy_b_sig, None)
        .expect("resolve");

    assert_eq!(status.state, DeployFinalizationState::Pending);
    assert_eq!(
        status.latest_block_hash.as_ref(),
        Some(&block_b.block_hash),
        "the row fed at B's insert must surface B as the sig's appearance \
         even though B is reachable only through C's secondary parent slot"
    );
    assert_eq!(status.rejection_count, 0);
}
