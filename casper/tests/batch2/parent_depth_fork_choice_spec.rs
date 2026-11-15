//! Three equally bonded validators. A proposes a run of blocks on its own
//! chain without hearing from B or C; meanwhile C builds on B's block. When A
//! then receives B's and C's blocks, fork choice in A's view favours their
//! branch (two thirds of the stake), but its head sits further below A's own
//! tip than the parent-depth limit allows.

use std::collections::HashMap;

use casper::rust::block_status::ValidBlock;
use casper::rust::casper::Casper;
use casper::rust::estimator::Estimator;
use crypto::rust::public_key::PublicKey;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::token_amount::TokenAmount;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// Comfortably past the test node's parent-depth limit (30).
const SOLO_BLOCKS: usize = 40;

fn equal_bonds(pks: Vec<PublicKey>) -> HashMap<PublicKey, TokenAmount> {
    pks.into_iter().map(|pk| (pk, 100u64.into())).collect()
}

struct RunawayView {
    nodes: Vec<TestNode>,
    own_chain: Vec<BlockMessage>,
    own_tip: BlockMessage,
    heavier_tip: BlockMessage,
}

async fn validator_ran_ahead_of_a_heavier_branch() -> RunawayView {
    let parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(equal_bonds), Some(3));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 3, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let mut own_chain = Vec::with_capacity(SOLO_BLOCKS);
    for _ in 0..SOLO_BLOCKS {
        own_chain.push(
            nodes[0]
                .add_block_from_deploys(&[])
                .await
                .expect("solo block"),
        );
    }
    let own_tip = own_chain.last().expect("at least one solo block").clone();

    let b1 = nodes[1].add_block_from_deploys(&[]).await.expect("b1");
    let status = nodes[2].process_block(b1.clone()).await.expect("b1 to C");
    assert!(
        matches!(status, Either::Right(ValidBlock::Valid)),
        "staging: C must accept b1: {:?}",
        status
    );
    let c1 = nodes[2].add_block_from_deploys(&[]).await.expect("c1");
    assert_eq!(
        c1.header.parents_hash_list.first(),
        Some(&b1.block_hash),
        "staging: c1 must build on b1"
    );

    for block in [&b1, &c1] {
        let status = nodes[0]
            .process_block(block.clone())
            .await
            .expect("deliver to A");
        assert!(
            matches!(status, Either::Right(ValidBlock::Valid)),
            "staging: A must accept {}: {:?}",
            block.body.state.block_number,
            status
        );
    }
    RunawayView {
        nodes,
        own_chain,
        own_tip,
        heavier_tip: c1,
    }
}

/// The main parent must be fork choice's head even when the proposer's own tip
/// sits more than the parent-depth limit above it: a validator that keeps
/// extending its own tip instead never moves onto the branch the stake
/// supports, and its main chain parts from everyone else's.
#[tokio::test]
async fn the_main_parent_stays_the_fork_choice_head_when_the_own_tip_ran_past_the_depth_limit() {
    crate::init_logger();
    let RunawayView {
        mut nodes,
        own_chain,
        own_tip,
        heavier_tip,
    } = validator_ran_ahead_of_a_heavier_branch().await;

    let snapshot = nodes[0].casper.get_snapshot().await.expect("snapshot");
    let mut dag = snapshot.dag.clone();
    let floor = dag
        .lookup_unsafe(&snapshot.last_finalized_block)
        .expect("finalized block metadata");
    let latest: HashMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let unbounded = Estimator::apply()
        .tips_with_latest_messages(&mut dag, &floor, latest, Estimator::UNLIMITED_PARENTS, None)
        .await
        .expect("fork choice without the depth bound");
    assert_eq!(
        unbounded.tips.first(),
        Some(&heavier_tip.block_hash),
        "staging: fork choice in A's view must head the branch two thirds of the stake supports"
    );
    let depth = own_tip.body.state.block_number - heavier_tip.body.state.block_number;
    let limit = snapshot.on_chain_state.chain.max_parent_depth as i64;
    assert!(
        depth > limit,
        "staging: A's own tip must sit past the parent-depth limit above the head \
         (depth {}, limit {})",
        depth,
        limit
    );

    let main_parent = snapshot
        .parents
        .first()
        .map(|p| p.block_hash.clone())
        .expect("a snapshot always has a main parent");
    assert_eq!(
        main_parent, heavier_tip.block_hash,
        "A's next block must extend fork choice's head (#{}), not its own tip (#{})",
        heavier_tip.body.state.block_number, own_tip.body.state.block_number
    );

    let next = nodes[0]
        .add_block_from_deploys(&[])
        .await
        .expect("A's next block");
    assert_eq!(
        next.header.parents_hash_list.first(),
        Some(&heavier_tip.block_hash),
        "the block A creates must carry fork choice's head as its main parent"
    );
    for block in own_chain.iter().chain(std::iter::once(&heavier_tip)) {
        nodes[1]
            .process_block(block.clone())
            .await
            .expect("deliver A's chain and C's tip to B");
    }
    let status = nodes[1]
        .process_block(next.clone())
        .await
        .expect("B processes A's next block");
    assert!(
        matches!(status, Either::Right(ValidBlock::Valid)),
        "a peer must accept A's block on the head, though it sits below A's own last block: {:?}",
        status
    );
}
