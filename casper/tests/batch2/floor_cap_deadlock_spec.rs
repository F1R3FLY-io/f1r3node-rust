//! Two equally bonded validators mint rival blocks at #1 and exchange them.
//! Both then extend their own chains in isolation for `SOLO_BLOCKS` blocks,
//! each opening on the same rival (the target), before exchanging the chains.
//! Every latest message has the target on its main chain, but one validator
//! last saw the other only at its rival block, which vetoes the edge. No
//! clique certifies, the floor stays at genesis, and the next block's fork
//! choice wants both tips, putting the floor distance past the merge cap.

use std::collections::{BTreeMap, HashMap};

use casper::rust::block_status::ValidBlock;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::{Casper, CasperSnapshot};
use casper::rust::errors::CasperError;
use casper::rust::finality::floor::{finalized_floor_with_candidates, Floor};
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::rholang::interpreter_util::MAX_FLOOR_DISTANCE_BLOCKS;
use crypto::rust::public_key::PublicKey;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::token_amount::TokenAmount;

use crate::helper::test_node::{ShardOverrides, TestNode};
use crate::util::genesis_builder::GenesisBuilder;

const SOLO_BLOCKS: usize = MAX_FLOOR_DISTANCE_BLOCKS as usize + 1;

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
    let mut nodes = TestNode::create_network_with_overrides(genesis, 2, ShardOverrides {
        finalization_rate: Some(0),
        ..Default::default()
    })
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
        max_parent - floor.block_number > MAX_FLOOR_DISTANCE_BLOCKS,
        "staging: floor distance {} must exceed the cap",
        max_parent - floor.block_number
    );
    snapshot
}

/// Past the cap no merge can be built. A proposal that must not be empty is
/// refused with the typed hold; one that may be empty mints a single-parent
/// witness on fork choice's main parent that still cites every latest message
/// and that both validators accept, although its own floor distance is past
/// the cap too.
#[tokio::test]
async fn past_the_floor_cap_a_proposal_is_held_or_mints_a_witness_every_node_accepts() {
    crate::init_logger();
    let StalledShard { mut nodes, target } = stalled_two_validator_shard().await;
    let snapshot = assert_stalled_view(&nodes[1], &target).await;
    let tips = latest_messages(&snapshot);

    nodes[1].allow_empty_blocks = false;
    let held = nodes[1].create_block(&[]).await;
    assert!(
        matches!(
            held,
            Err(CasperError::FloorDistanceExceeded { cap, .. }) if cap == MAX_FLOOR_DISTANCE_BLOCKS
        ),
        "a proposal that may not be empty must be held, not minted: {:?}",
        held
    );

    nodes[1].allow_empty_blocks = true;
    let witness = match nodes[1].create_block(&[]).await {
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
    let floor = derived_floor(
        &nodes[1],
        &snapshot,
        &witness.header.parents_hash_list,
        &cited,
    )
    .await;
    let distance = snapshot.parents[0].body.state.block_number - floor.block_number;
    assert!(
        distance > MAX_FLOOR_DISTANCE_BLOCKS,
        "the witness's own floor distance {} must exceed the cap",
        distance
    );

    for (node, role) in [(0usize, "a peer"), (1usize, "its creator")] {
        let status = nodes[node]
            .process_block(witness.clone())
            .await
            .expect("process the witness");
        assert!(
            matches!(status, Either::Right(ValidBlock::Valid)),
            "{role} must accept the witness (floor distance {}): {:?}",
            distance,
            status
        );
    }
}
