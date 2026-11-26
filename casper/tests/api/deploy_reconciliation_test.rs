// Proof tests for the on-demand reconciliation of never-observed deploys
// (#1103): `deploy_reconciliation::reconcile` over synthetic DAGs with
// exact-height control (expiry boundaries, restore-anchor gaps, unlimited
// contestability, lookup precedence), plus the `BlockAPI` wiring on a real
// node (sig-only parity, basis reporting, shard and sig refusal).

use std::collections::HashMap;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::dag::deploy_lifecycle_types::{TerminalRecord, TerminalState};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::api::block_api::{BlockAPI, DeployValidationError};
use casper::rust::api::deploy_finalization_status::DeployFinalizationState;
use casper::rust::api::deploy_reconciliation;
use casper::rust::casper::{CasperShardConf, MultiParentCasper};
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::engine_with_casper::EngineWithCasper;
use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy::source_deploy_now;
use crypto::rust::public_key::PublicKey;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new() -> Self {
        fn bonds_function(
            validators: Vec<PublicKey>,
        ) -> HashMap<PublicKey, shared::rust::token_amount::TokenAmount> {
            validators
                .into_iter()
                .zip(vec![10u64, 10u64, 10u64])
                .map(|(key, stake)| (key, stake.into()))
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
    let engine = EngineWithCasper::new(node.casper.clone());
    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;
    engine_cell
}

/// A shard conf judged by the reconciliation rule: only the lifespan, the
/// citability horizon source, and the ftt provenance matter here, since the
/// floors of the synthetic chain are seeded directly.
fn conf(deploy_lifespan: i64, max_parent_depth: i32) -> CasperShardConf {
    let mut conf = CasperShardConf::new();
    conf.fault_tolerance_threshold_ppm = 100_000;
    conf.deploy_lifespan = deploy_lifespan;
    conf.max_parent_depth = max_parent_depth;
    conf
}

fn never_observed_deploy(
    valid_after: i64,
) -> crypto::rust::signatures::signed::Signed<
    models::rust::casper::protocol::casper_message::DeployData,
> {
    source_deploy_now(
        format!("@reconcile_{valid_after}!({valid_after})"),
        None,
        Some(valid_after),
        Some("test-shard".to_string()),
    )
    .expect("signed deploy")
}

fn chain_block(number: i64, parent: Option<&BlockMessage>) -> BlockMessage {
    let parents = parent.into_iter().map(|b| b.block_hash.clone()).collect();
    get_random_block(
        Some(number),
        Some(number as i32),
        None,
        None,
        None,
        None,
        Some(number),
        Some(parents),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        None,
        Some("test-shard".into()),
        None,
    )
}

/// A single-parent chain `0..=top`, every floor seeded to the block's own
/// parent, and `top` adopted as the LFB: the verdict basis is the block
/// at height `top - 1`.
async fn adopted_chain(
    top: i64,
) -> (
    BlockDagKeyValueStorage,
    KeyValueBlockStore,
    Vec<BlockMessage>,
) {
    let (storage, store, blocks) = chain_without_coverage(top).await;
    storage.record_carrier_coverage_from(0).unwrap();
    (storage, store, blocks)
}

async fn chain_without_coverage(
    top: i64,
) -> (
    BlockDagKeyValueStorage,
    KeyValueBlockStore,
    Vec<BlockMessage>,
) {
    let mut kvm = InMemoryStoreManager::new();
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
        .await
        .expect("dag storage");

    let mut blocks: Vec<BlockMessage> = Vec::new();
    for number in 0..=top {
        blocks.push(chain_block(number, blocks.last()));
    }
    for block in &blocks {
        block_store.put_block_message(block).expect("store block");
    }
    dag_storage
        .insert(&blocks[0], InsertMode::Approved)
        .expect("insert genesis");
    for block in &blocks[1..] {
        dag_storage
            .insert(block, InsertMode::Normal)
            .expect("insert block");
    }

    let dag = dag_storage.get_representation().expect("dag");
    dag.put_cached_floor(blocks[0].block_hash.clone(), blocks[0].block_hash.clone())
        .expect("genesis is its own floor");
    for (number, block) in blocks.iter().enumerate().skip(1) {
        dag.put_cached_floor(
            block.block_hash.clone(),
            blocks[number - 1].block_hash.clone(),
        )
        .expect("seed floor to the parent");
    }
    let top_hash = blocks.last().expect("non-empty chain").block_hash.clone();
    dag_storage
        .record_directly_finalized(top_hash, 0.5, |_| async { Ok(()) })
        .await
        .expect("adopt the top block");
    (dag_storage, block_store, blocks)
}

/// The bound is closed at the basis and open one past it
/// (`bound = valid_after + deploy_lifespan + citability_horizon`), and an
/// Expired verdict names the basis it was judged against.
#[tokio::test]
async fn a_never_observed_deploy_expires_exactly_past_the_bound_and_names_the_basis() {
    // bound = 5 + 10 + 10 = 25; basis height 25 stays Pending.
    let (dag_storage, block_store, blocks) = adopted_chain(26).await;
    let dag = dag_storage.get_representation().expect("dag");
    let deploy = never_observed_deploy(5);
    let reconciled =
        deploy_reconciliation::reconcile(&dag, &block_store, &conf(10, 10), 0, &deploy)
            .await
            .expect("reconcile at the bound");
    assert_eq!(reconciled.status.state, DeployFinalizationState::Pending);
    let basis = reconciled.basis.expect("the basis is resolvable");
    assert_eq!(basis.block_number, 25, "the basis is the LFB's floor");
    assert_eq!(basis.hash, blocks[25].block_hash);

    // One block further, the basis passes the bound: proven Expired.
    let (dag_storage, block_store, blocks) = adopted_chain(27).await;
    let dag = dag_storage.get_representation().expect("dag");
    let reconciled =
        deploy_reconciliation::reconcile(&dag, &block_store, &conf(10, 10), 0, &deploy)
            .await
            .expect("reconcile past the bound");
    assert_eq!(reconciled.status.state, DeployFinalizationState::Expired);
    assert_eq!(reconciled.status.rejection_count, 0);
    assert!(reconciled.status.latest_block_hash.is_none());
    let basis = reconciled
        .basis
        .expect("an Expired verdict names its basis");
    assert_eq!(basis.block_number, 26);
    assert_eq!(basis.hash, blocks[26].block_hash);
}

/// Delayed inclusion is still possible while the basis is inside the
/// contestability window, and a lagging observer simply sits there: the
/// verdict stays Pending and still reports how far the basis advanced.
#[tokio::test]
async fn a_never_observed_deploy_stays_pending_while_the_basis_is_inside_the_window() {
    // bound = 5 + 20 + 10 = 35; the basis (29) has not passed it.
    let (dag_storage, block_store, _blocks) = adopted_chain(30).await;
    let dag = dag_storage.get_representation().expect("dag");
    let deploy = never_observed_deploy(5);
    let reconciled =
        deploy_reconciliation::reconcile(&dag, &block_store, &conf(20, 10), 0, &deploy)
            .await
            .expect("reconcile");
    assert_eq!(reconciled.status.state, DeployFinalizationState::Pending);
    let basis = reconciled
        .basis
        .expect("a pending verdict still names its basis");
    assert_eq!(basis.block_number, 29);
}

#[tokio::test]
async fn a_delayed_carrier_replaces_the_unobserved_answer() {
    let (storage, block_store, blocks) = adopted_chain(14).await;
    let deploy = never_observed_deploy(5);
    let config = conf(15, 4);
    let dag = storage.get_representation().unwrap();
    let before = deploy_reconciliation::reconcile(&dag, &block_store, &config, 0, &deploy)
        .await
        .unwrap();
    assert_eq!(before.status.state, DeployFinalizationState::Pending);
    assert_eq!(before.basis.unwrap().hash, blocks[13].block_hash);

    let mut carrier = chain_block(15, blocks.last());
    carrier.body.deploys.push(
        models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(deploy.clone()),
    );
    block_store.put_block_message(&carrier).unwrap();
    let dag = storage.insert(&carrier, InsertMode::Normal).unwrap();
    let after = deploy_reconciliation::reconcile(&dag, &block_store, &config, 0, &deploy)
        .await
        .unwrap();
    assert_eq!(after.status.state, DeployFinalizationState::Pending);
    assert_eq!(after.status.latest_block_hash, Some(carrier.block_hash));
    assert!(after.basis.is_none());
}

/// Unlimited contestability (`max_parent_depth = i32::MAX`) means nothing
/// is ever provably uncitable: the verdict stays Pending forever.
#[tokio::test]
async fn unlimited_contestability_keeps_a_never_observed_deploy_pending() {
    let (dag_storage, block_store, _blocks) = adopted_chain(30).await;
    let dag = dag_storage.get_representation().expect("dag");
    let deploy = never_observed_deploy(5);
    let reconciled =
        deploy_reconciliation::reconcile(&dag, &block_store, &conf(10, i32::MAX), 0, &deploy)
            .await
            .expect("reconcile");
    assert_eq!(reconciled.status.state, DeployFinalizationState::Pending);
}

/// A validity window that may lie at or below the restore anchor cannot be
/// proven absent — bodies there were never ingested — so the verdict fails
/// closed to Pending. At the boundary (`valid_after == anchor`) the lowest
/// possible carrier sits strictly above the anchor and absence is again a
/// proof.
#[tokio::test]
async fn a_window_below_the_restore_anchor_fails_closed_to_pending() {
    let (dag_storage, block_store, _blocks) = adopted_chain(30).await;
    let dag = dag_storage.get_representation().expect("dag");
    let deploy = never_observed_deploy(5);

    let below = deploy_reconciliation::reconcile(&dag, &block_store, &conf(10, 10), 6, &deploy)
        .await
        .expect("reconcile below the anchor");
    assert_eq!(below.status.state, DeployFinalizationState::Pending);
    assert!(
        below.basis.is_some(),
        "the gap verdict still names its basis"
    );

    let at = deploy_reconciliation::reconcile(&dag, &block_store, &conf(10, 10), 5, &deploy)
        .await
        .expect("reconcile at the anchor");
    assert_eq!(at.status.state, DeployFinalizationState::Expired);
}

#[tokio::test]
async fn an_upgrade_cannot_prove_absence_before_index_coverage() {
    let (storage, block_store, _) = chain_without_coverage(30).await;
    let dag = storage.get_representation().unwrap();
    let deploy = never_observed_deploy(5);
    let config = conf(1, 1);
    let reconcile =
        |deploy| deploy_reconciliation::reconcile(&dag, &block_store, &config, 0, deploy);
    assert_eq!(
        reconcile(&deploy).await.unwrap().status.state,
        DeployFinalizationState::Pending
    );
    assert_eq!(storage.ensure_carrier_watermark().unwrap(), Some(31));
    assert_eq!(
        reconcile(&deploy).await.unwrap().status.state,
        DeployFinalizationState::Pending
    );
}

#[tokio::test]
async fn missing_floor_metadata_keeps_the_verdict_pending_without_a_basis() {
    let (storage, block_store, blocks) = adopted_chain(30).await;
    let dag = storage.get_representation().unwrap();
    dag.put_cached_floor(blocks[30].block_hash.clone(), vec![0xDD; 32].into())
        .unwrap();
    let deploy = never_observed_deploy(5);
    let result = deploy_reconciliation::reconcile(&dag, &block_store, &conf(1, 1), 0, &deploy)
        .await
        .unwrap();
    assert_eq!(result.status.state, DeployFinalizationState::Pending);
    assert!(result.basis.is_none());
}

/// Lookup precedence: a terminal register record answers with its frozen
/// fields and carries no basis — a reconciliation never overrides the
/// register, covering delayed inclusion and re-application it already
/// observed.
#[tokio::test]
async fn a_terminal_record_answers_before_any_reconciliation() {
    let (dag_storage, block_store, _blocks) = adopted_chain(30).await;
    let dag = dag_storage.get_representation().expect("dag");
    let deploy = never_observed_deploy(5);
    dag.put_deploy_terminal_if_absent(deploy.sig.as_ref(), TerminalRecord {
        state: TerminalState::Finalized,
        rejection_count: 2,
        latest_height: 9,
        latest_block_hash: vec![0xA1; 32],
    })
    .expect("terminal record");

    let reconciled =
        deploy_reconciliation::reconcile(&dag, &block_store, &conf(10, 10), 0, &deploy)
            .await
            .expect("reconcile");
    assert_eq!(reconciled.status.state, DeployFinalizationState::Finalized);
    assert_eq!(reconciled.status.rejection_count, 2);
    assert!(
        reconciled.basis.is_none(),
        "a register answer carries no basis"
    );
}

/// An open admission row (the admitting validator's pool record) keeps the
/// register's own Pending answer — the finalizer's settle owns that row's
/// verdict, not the on-demand path.
#[tokio::test]
async fn an_open_admission_row_answers_pending_with_no_basis() {
    let (dag_storage, block_store, _blocks) = adopted_chain(30).await;
    let deploy = never_observed_deploy(5);
    dag_storage
        .record_deploy_admitted(deploy.sig.as_ref(), 5)
        .expect("admission row");
    let dag = dag_storage.get_representation().expect("dag");

    let reconciled =
        deploy_reconciliation::reconcile(&dag, &block_store, &conf(10, 10), 0, &deploy)
            .await
            .expect("reconcile");
    assert_eq!(reconciled.status.state, DeployFinalizationState::Pending);
    assert_eq!(reconciled.status.rejection_count, 0);
    assert!(reconciled.status.latest_block_hash.is_none());
    assert!(reconciled.basis.is_none());
}

/// Wiring on a real node: a never-observed deploy against the fresh node's
/// adopted LFB answers Pending and reports the floor of that LFB as the
/// basis — the same floor `floor_of_block` computes.
#[tokio::test]
async fn a_real_node_reconciles_a_never_observed_deploy_with_the_lfbs_floor() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();
    let deploy = source_deploy_now(
        "@wiring!({0})".to_string(),
        None,
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("signed deploy");
    let sig = deploy.sig.clone();

    let reconciled = BlockAPI::deploy_finalization_status_reconciled(
        &engine_cell,
        &sig,
        Some(&deploy),
        &shard_id,
    )
    .await
    .expect("reconcile");
    assert_eq!(reconciled.status.state, DeployFinalizationState::Pending);

    let dag = nodes[0].casper.block_dag().await.expect("dag");
    let shard_conf = nodes[0].casper.casper_shard_conf();
    let ftt = FtThreshold::from_ppm(shard_conf.fault_tolerance_threshold_ppm);
    let lfb = dag.last_finalized_block();
    let floor = floor_of_block(&dag, nodes[0].casper.block_store(), &lfb, ftt)
        .await
        .expect("floor");
    let basis = reconciled.basis.expect("a fresh node resolves its floor");
    assert_eq!(basis.hash, floor.hash);
    assert_eq!(basis.block_number, floor.block_number);
}

/// A sig-only query through the reconciled entry point keeps the exact
/// pure-lookup behavior of `deploy_finalization_status`.
#[tokio::test]
async fn a_sig_only_query_keeps_the_pure_lookup_answer() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    let sig = vec![0xBB; 32];
    let status =
        BlockAPI::deploy_finalization_status_reconciled(&engine_cell, &sig, None, &shard_id)
            .await
            .expect("lookup");
    assert_eq!(status.status.state, DeployFinalizationState::Pending);
    assert_eq!(status.status.rejection_count, 0);
    assert!(status.status.latest_block_hash.is_none());
    assert!(status.basis.is_none());
}

/// Caller errors are refused before any verdict: a deploy whose signature
/// does not match the queried sig, and a deploy of a foreign shard.
#[tokio::test]
async fn the_wrapper_refuses_a_sig_mismatch_and_a_foreign_shard() {
    let ctx = TestContext::new().await;
    let nodes = TestNode::create_network(ctx.genesis.clone(), 1, None, None, None, None)
        .await
        .unwrap();
    let engine_cell = create_engine_cell(&nodes[0]).await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    let deploy = source_deploy_now(
        "@mismatch!({0})".to_string(),
        None,
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("signed deploy");
    let err = BlockAPI::deploy_finalization_status_reconciled(
        &engine_cell,
        &[0xEE; 32],
        Some(&deploy),
        &shard_id,
    )
    .await
    .expect_err("the queried sig does not match the deploy");
    assert!(
        err.chain()
            .any(|cause| cause.downcast_ref::<DeployValidationError>().is_some()),
        "a sig mismatch is a deploy validation refusal, got: {err:#}"
    );

    let foreign = source_deploy_now(
        "@foreign!({0})".to_string(),
        None,
        Some(0),
        Some("elsewhere".to_string()),
    )
    .expect("signed deploy");
    let foreign_sig = foreign.sig.clone();
    let err = BlockAPI::deploy_finalization_status_reconciled(
        &engine_cell,
        &foreign_sig,
        Some(&foreign),
        &shard_id,
    )
    .await
    .expect_err("the deploy belongs to another shard");
    assert!(
        err.chain()
            .any(|cause| cause.downcast_ref::<DeployValidationError>().is_some()),
        "a shard mismatch is a deploy validation refusal, got: {err:#}"
    );
}
