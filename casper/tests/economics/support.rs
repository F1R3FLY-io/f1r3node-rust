use std::collections::HashSet;
use std::sync::Arc;

use casper::rust::block_status::{BlockError, ValidBlock};
use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::DeploySelection;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::economics::bucket_view::{LeakyBucketDecision, LeakyBucketProposal};
use casper::rust::economics::claims::{FloorView, VoteHistory};
use casper::rust::finality::floor_context::FloorContext;
use casper::rust::validate::Validate;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;

/// Funds every genesis validator's vault, which the default genesis leaves
/// empty, so validators can pay for their own deploys.
pub fn fund_validators(genesis: &mut casper::rust::genesis::genesis::Genesis) {
    let validator_vaults: Vec<rholang::rust::interpreter::util::vault_address::VaultAddress> =
        genesis
            .proof_of_stake
            .validators
            .iter()
            .map(|validator| {
                rholang::rust::interpreter::util::vault_address::VaultAddress::from_public_key(
                    &validator.pk,
                )
                .expect("vault address")
            })
            .collect();
    for vault in genesis.vaults.iter_mut() {
        if validator_vaults.contains(&vault.vault_address) {
            vault.initial_balance = 1_000_000;
        }
    }
}

/// A block from `node` that carries its requests to eject `offers`, votes yea
/// on exactly the ejections of `drained`, and votes on rejoins and overflow
/// claims by its own view.
pub async fn propose(
    node: &mut TestNode,
    offers: &[Validator],
    drained: &[Validator],
) -> BlockMessage {
    request_ejections(node, offers).await;
    let own_view = own_decision(node);
    let drained: HashSet<Validator> = drained.iter().cloned().collect();
    let decision: LeakyBucketDecision = Arc::new(move |knowledge| {
        let own = own_view(knowledge);
        LeakyBucketProposal {
            drained: drained.clone(),
            ..own
        }
    });
    create(node, decision).await
}

/// A block from `node` that requests and votes exactly as its own view
/// decides, as a running node's proposer does.
pub async fn propose_by_own_view(node: &mut TestNode) -> BlockMessage {
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    node.casper
        .request_ejections(&snapshot)
        .await
        .expect("ejection requests");
    let decision = own_decision(node);
    create(node, decision).await
}

/// Admits to `node`'s pool its requests to eject `targets`.
pub async fn request_ejections(node: &TestNode, targets: &[Validator]) {
    if targets.is_empty() {
        return;
    }
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    node.casper
        .request_ejections_of(&snapshot, targets)
        .await
        .expect("ejection requests");
}

fn own_decision(node: &TestNode) -> LeakyBucketDecision {
    node.casper.bucket_view().decision(identity(node))
}

pub async fn create(node: &mut TestNode, decision: LeakyBucketDecision) -> BlockMessage {
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let validator = node.casper.get_validator().expect("validator");
    match block_creator::create(
        &snapshot,
        &validator,
        None,
        node.deploy_storage.clone(),
        node.rejected_deploy_buffer.clone(),
        &node.runtime_manager,
        &mut node.block_store,
        &node.block_dag_storage,
        DeploySelection::StandardAllowEmpty,
        decision,
    )
    .await
    .expect("block creation")
    {
        BlockCreatorResult::Created(block, ..) => block,
        other => panic!("expected a created block, got {other:?}"),
    }
}

pub async fn admit_everywhere(nodes: &mut [TestNode], block: &BlockMessage) {
    for node in nodes.iter_mut() {
        let verdict = node.add_block(block.clone()).await.expect("process");
        assert!(
            matches!(verdict, Either::Right(ValidBlock::Valid)),
            "block {} refused: {verdict:?}",
            block.body.state.block_number
        );
    }
}

pub fn identity(node: &TestNode) -> Validator {
    prost::bytes::Bytes::copy_from_slice(
        &node
            .casper
            .get_validator()
            .expect("validator")
            .public_key
            .bytes,
    )
}

/// `node`'s casper constructed again from the storage it holds, through
/// `hash_set_casper`, the constructor every running casper goes through.
pub async fn restart(
    node: &TestNode,
) -> casper::rust::engine::multi_parent_casper::MultiParentCasperImpl<
    crate::util::comm::transport_layer_test_impl::TransportLayerTestImpl,
> {
    let deploy_storage = (*node.casper.deploy_storage).clone();
    casper::rust::casper::hash_set_casper(
        node.casper.block_retriever.clone(),
        node.casper.event_publisher.clone(),
        node.casper.runtime_manager.clone(),
        node.casper.estimator.clone(),
        node.casper.block_store.clone(),
        node.casper.block_dag_storage.clone(),
        deploy_storage,
        node.casper.rejected_deploy_buffer.clone(),
        node.casper.casper_buffer_storage.clone(),
        node.casper.validator_id.clone(),
        node.casper.casper_shard_conf.clone(),
        node.casper.approved_block.clone(),
        node.casper.heartbeat_signal_ref.clone(),
    )
    .await
    .expect("restart")
}

/// `block`'s floor as `node` derives it.
pub async fn floor_view(node: &TestNode, block: &BlockMessage) -> FloorView {
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let latest_messages = block
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let ctx = FloorContext::derive(
        &snapshot.dag,
        &node.block_store,
        &block.header.parents_hash_list,
        &latest_messages,
        casper::rust::safety::clique_oracle::FtThreshold::from_ppm(
            snapshot.on_chain_state.chain.fault_tolerance_threshold_ppm,
        ),
    )
    .await
    .expect("floor");
    FloorView::read(&snapshot.dag, &node.runtime_manager, &ctx)
        .await
        .expect("floor view")
}

/// The claims rule judged on its own, against `node`'s view of `block`'s
/// floor, so a forged block need not survive replay to reach it.
pub async fn judge(node: &TestNode, block: &BlockMessage) -> Either<BlockError, ValidBlock> {
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let floor = floor_view(node, block).await;
    Validate::leaky_bucket_claims(
        block,
        &node.runtime_manager,
        &VoteHistory::new(&node.block_dag_storage, &node.block_store, &snapshot.dag),
        &floor,
        node.casper.bucket_view().params(),
    )
    .await
}
