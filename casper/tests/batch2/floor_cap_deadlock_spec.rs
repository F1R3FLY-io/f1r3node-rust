//! Two equally bonded validators mint rival blocks at #1 and exchange them.
//! Both then extend their own chains in isolation for `SOLO_BLOCKS` blocks,
//! each opening on the same rival (the target), before exchanging the chains.
//! Every latest message has the target on its main chain, but one validator
//! last saw the other only at its rival block, which vetoes the edge. No
//! clique certifies, the floor stays at genesis, and the next block's fork
//! choice wants both tips, putting the floor distance past the merge cap.

use std::collections::{BTreeMap, HashMap};

use casper::rust::block_status::ValidBlock;
use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::DeploySelection;
use casper::rust::casper::{Casper, CasperSnapshot};
use casper::rust::finality::floor::{finalized_floor_with_candidates, Floor};
use casper::rust::safety::clique_oracle::FtThreshold;
use crypto::rust::public_key::PublicKey;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::token_amount::TokenAmount;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// Mirrors the crate-private `MAX_FLOOR_DISTANCE_BLOCKS`.
const FLOOR_DISTANCE_CAP: i64 = 256;
const SOLO_BLOCKS: usize = 257;

fn ftt() -> FtThreshold { FtThreshold::from_ppm(100_000) }

fn equal_bonds(pks: Vec<PublicKey>) -> HashMap<PublicKey, TokenAmount> {
    pks.into_iter().map(|pk| (pk, 100u64.into())).collect()
}

struct StalledShard {
    nodes: Vec<TestNode>,
    target: BlockMessage,
}

async fn stalled_two_validator_shard() -> StalledShard {
    let parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(equal_bonds), Some(2));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 2, None, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let rivals = [
        nodes[0]
            .add_block_from_deploys(&[])
            .await
            .expect("v0 rival"),
        nodes[1]
            .add_block_from_deploys(&[])
            .await
            .expect("v1 rival"),
    ];
    nodes[0]
        .process_block(rivals[1].clone())
        .await
        .expect("v1 rival to v0");
    nodes[1]
        .process_block(rivals[0].clone())
        .await
        .expect("v0 rival to v1");

    let mut chains: Vec<Vec<BlockMessage>> = vec![Vec::new(), Vec::new()];
    for (node, chain) in nodes.iter_mut().zip(chains.iter_mut()) {
        for _ in 0..SOLO_BLOCKS {
            chain.push(node.add_block_from_deploys(&[]).await.expect("solo block"));
        }
    }
    let main_parent = |chain: &[BlockMessage]| chain[0].header.parents_hash_list[0].clone();
    assert_eq!(
        main_parent(&chains[0]),
        main_parent(&chains[1]),
        "staging: both solo chains must open on the same rival"
    );
    let target = rivals
        .iter()
        .find(|r| r.block_hash == main_parent(&chains[0]))
        .expect("the solo chains open on one of the rivals")
        .clone();

    for (receiver, chain) in [(1usize, &chains[0]), (0usize, &chains[1])] {
        for block in chain {
            let status = nodes[receiver]
                .process_block(block.clone())
                .await
                .expect("deliver solo chain");
            assert!(
                matches!(status, Either::Right(ValidBlock::Valid)),
                "staging: solo block #{} must validate on the other node: {:?}",
                block.body.state.block_number,
                status
            );
        }
    }
    StalledShard { nodes, target }
}

fn latest_messages(snapshot: &CasperSnapshot) -> BTreeMap<Validator, BlockHash> {
    snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect()
}

async fn derived_floor(
    node: &TestNode,
    snapshot: &CasperSnapshot,
    parents: &[BlockHash],
    latest: &BTreeMap<Validator, BlockHash>,
) -> Floor {
    finalized_floor_with_candidates(&snapshot.dag, &node.block_store, parents, latest, ftt())
        .await
        .expect("floor derivation")
        .0
}

/// Asserts the stalled geometry from `node`'s view: fork choice selects two
/// parents, every latest message agrees on the target, the floor is below
/// the target, and the merge's floor distance exceeds the cap.
async fn assert_stalled_view(node: &TestNode, target: &BlockMessage) -> CasperSnapshot {
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    assert_eq!(
        snapshot.parents.len(),
        2,
        "staging: fork choice must select both solo tips"
    );
    let latest = latest_messages(&snapshot);
    assert_eq!(latest.len(), 2, "staging: both validators cited");
    for lm in latest.values() {
        assert!(
            snapshot
                .dag
                .is_in_main_chain(&target.block_hash, lm)
                .expect("is_in_main_chain"),
            "staging: every latest message must agree on the target"
        );
    }
    let parent_hashes: Vec<BlockHash> = snapshot
        .parents
        .iter()
        .map(|p| p.block_hash.clone())
        .collect();
    let floor = derived_floor(node, &snapshot, &parent_hashes, &latest).await;
    assert!(
        floor.block_number < target.body.state.block_number,
        "staging: the target must be agreed but not certified (floor #{})",
        floor.block_number
    );
    let max_parent = snapshot
        .parents
        .iter()
        .map(|p| p.body.state.block_number)
        .max()
        .expect("two parents");
    assert!(
        max_parent - floor.block_number > FLOOR_DISTANCE_CAP,
        "staging: floor distance {} must exceed the cap",
        max_parent - floor.block_number
    );
    snapshot
}

/// Mints on `node` with only fork choice's main parent, keeping the
/// snapshot's justifications (every validator's latest message).
async fn mint_on_main_parent_only(node: &mut TestNode) -> BlockMessage {
    let mut snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let main_parent = snapshot.parents[0].clone();
    snapshot.max_block_num = main_parent.body.state.block_number;
    snapshot.parents = vec![main_parent];
    let validator_identity = node.validator_id_opt.clone().expect("validator identity");
    let created = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        node.deploy_storage.clone(),
        node.rejected_deploy_buffer.clone(),
        &node.runtime_manager.clone(),
        &mut node.block_store,
        DeploySelection::StandardAllowEmpty,
    )
    .await
    .unwrap_or_else(|e| panic!("single-parent create must succeed: {:?}", e));
    match created {
        BlockCreatorResult::Created(block, ..) => block,
        other => panic!("single-parent create must mint: {:?}", other),
    }
}

#[tokio::test]
async fn single_parent_block_citing_every_latest_message_validates_past_the_floor_cap() {
    crate::init_logger();
    let started = std::time::Instant::now();
    let StalledShard { mut nodes, target } = stalled_two_validator_shard().await;
    tracing::info!(
        target: "f1r3fly.test.floor_cap",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "stalled shard staged"
    );
    let snapshot = assert_stalled_view(&nodes[1], &target).await;
    let tips = latest_messages(&snapshot);

    let hatch = mint_on_main_parent_only(&mut nodes[1]).await;
    assert_eq!(
        hatch.header.parents_hash_list,
        vec![snapshot.parents[0].block_hash.clone()],
        "the block declares only the main parent"
    );
    let cited: BTreeMap<Validator, BlockHash> = hatch
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    assert_eq!(cited, tips, "the block cites every latest message");
    assert!(
        hatch.body.deploys.is_empty(),
        "the block carries no user deploys"
    );
    let floor = derived_floor(
        &nodes[1],
        &snapshot,
        &hatch.header.parents_hash_list,
        &cited,
    )
    .await;
    let distance = snapshot.parents[0].body.state.block_number - floor.block_number;
    assert!(
        distance > FLOOR_DISTANCE_CAP,
        "the block's own floor distance {} must exceed the cap",
        distance
    );

    let peer_status = nodes[0]
        .process_block(hatch.clone())
        .await
        .expect("peer processes the block");
    assert!(
        matches!(peer_status, Either::Right(ValidBlock::Valid)),
        "a peer must accept the single-parent block (floor distance {}): {:?}",
        distance,
        peer_status
    );
    let own_status = nodes[1]
        .process_block(hatch.clone())
        .await
        .expect("creator processes the block");
    assert!(
        matches!(own_status, Either::Right(ValidBlock::Valid)),
        "the creator must accept its own single-parent block: {:?}",
        own_status
    );
    tracing::info!(
        target: "f1r3fly.test.floor_cap",
        elapsed_ms = started.elapsed().as_millis() as u64,
        floor_distance = distance,
        "single-parent block accepted"
    );
}

#[tokio::test]
async fn proposer_mints_past_the_floor_cap_while_the_target_is_agreed_but_uncertified() {
    crate::init_logger();
    let started = std::time::Instant::now();
    let StalledShard { mut nodes, target } = stalled_two_validator_shard().await;
    let snapshot = assert_stalled_view(&nodes[1], &target).await;
    let tips = latest_messages(&snapshot);

    let result = nodes[1].create_block(&[]).await;
    tracing::info!(
        target: "f1r3fly.test.floor_cap",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "proposal attempted"
    );
    let witness = match result {
        Ok(BlockCreatorResult::Created(block, ..)) => block,
        other => panic!(
            "the proposer must mint a block while the floor distance exceeds the cap: {:?}",
            other
        ),
    };
    assert_eq!(
        witness.header.parents_hash_list,
        vec![snapshot.parents[0].block_hash.clone()],
        "over the cap the block declares only fork choice's main parent"
    );
    let cited: BTreeMap<Validator, BlockHash> = witness
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    assert_eq!(cited, tips, "the block still cites every latest message");
    assert!(
        witness.body.deploys.is_empty(),
        "the block carries no user deploys"
    );
    let peer_status = nodes[0]
        .process_block(witness.clone())
        .await
        .expect("peer processes the block");
    assert!(
        matches!(peer_status, Either::Right(ValidBlock::Valid)),
        "a peer must accept the witness block: {:?}",
        peer_status
    );
}

#[tokio::test]
async fn a_proposal_that_may_not_be_empty_is_refused_with_the_typed_hold_past_the_cap() {
    crate::init_logger();
    let StalledShard { mut nodes, target } = stalled_two_validator_shard().await;
    assert_stalled_view(&nodes[1], &target).await;

    nodes[1].allow_empty_blocks = false;
    let result = nodes[1].create_block(&[]).await;
    assert!(
        matches!(
            result,
            Err(casper::rust::errors::CasperError::FloorDistanceExceeded { cap, .. })
                if cap == FLOOR_DISTANCE_CAP
        ),
        "a proposal that may not be empty must not mint a witness: {:?}",
        result
    );
}
