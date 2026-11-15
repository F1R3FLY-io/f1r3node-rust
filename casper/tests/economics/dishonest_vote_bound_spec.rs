use casper::rust::block_status::ValidBlock;
use casper::rust::casper::{Casper, MultiParentCasper};
use rspace_plus_plus::rspace::history::Either;

use super::support::{admit_everywhere, floor_view, identity, propose};
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// A node restarts through `hash_set_casper`, the constructor every running
/// casper goes through, and rebuilds its view from the blocks it holds: it
/// decides exactly what it decided before.
#[tokio::test]
async fn a_restarted_node_rebuilds_the_view_it_had() {
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 4, Some(0.0), None, None)
        .await
        .expect("network");
    let mut last = None;
    for round in 0..8 {
        let block = propose(&mut nodes[round % 4], &[], &[]).await;
        admit_everywhere(&mut nodes, &block).await;
        last = Some(block);
    }
    let last = last.expect("blocks were proposed");
    let node = &nodes[0];
    let own = identity(node);
    let before = node.casper.bucket_view().proposal(&own);
    assert!(
        !before.overflow.is_empty(),
        "before the restart the view tracks the committee"
    );

    let restarted = super::support::restart(node).await;
    assert!(
        restarted.dag_contains(&last.block_hash).expect("dag read"),
        "the restarted node still holds the blocks"
    );
    assert_eq!(
        restarted.bucket_view().proposal(&own),
        before,
        "the rebuilt view decides as the view it replaces did"
    );
}

/// Delivers to `node` every block of `pending` whose parents and
/// justifications it holds, at most `budget` of them, in creation order.
async fn deliver(
    node: &mut TestNode,
    pending: &[models::rust::casper::protocol::casper_message::BlockMessage],
    budget: usize,
) {
    let mut delivered = 0;
    loop {
        let next = pending.iter().find(|block| {
            !node
                .casper
                .dag_contains(&block.block_hash)
                .expect("dag read")
                && block
                    .header
                    .parents_hash_list
                    .iter()
                    .chain(block.justifications.iter().map(|j| &j.latest_block_hash))
                    .all(|dependency| node.casper.dag_contains(dependency).expect("dag read"))
        });
        let Some(block) = next.cloned() else {
            return;
        };
        if delivered == budget {
            return;
        }
        let verdict = node.add_block(block.clone()).await.expect("process");
        assert!(
            matches!(verdict, Either::Right(ValidBlock::Valid)),
            "block {} refused: {verdict:?}",
            block.body.state.block_number
        );
        delivered += 1;
    }
}

/// Under random partial delivery, every block a node proposes has a floor at
/// or above that node's last finalized block, and descends from it.
#[tokio::test]
async fn a_proposed_blocks_floor_is_never_below_its_proposers_finalized_block() {
    let mut violations = Vec::new();
    let mut checks = 0;
    for ftt_ppm in [0i64, 100_000, 670_000] {
        let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
        parameters.2.proof_of_stake.fault_tolerance_threshold_ppm = ftt_ppm;
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(parameters))
            .await
            .expect("genesis");
        let mut nodes = TestNode::create_network(genesis, 4, Some(0.0), None, None)
            .await
            .expect("network");
        let mut created = Vec::new();
        let mut highest_lfb = 0i64;
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15 ^ ftt_ppm as u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for step in 0..60 {
            for node in nodes.iter_mut() {
                let budget = if step % 15 == 14 {
                    usize::MAX
                } else {
                    (next() % 4) as usize
                };
                deliver(node, &created, budget).await;
            }
            let proposer = (next() % nodes.len() as u64) as usize;
            let lfb = nodes[proposer]
                .casper
                .last_finalized_block()
                .await
                .expect("last finalized block");
            let block = propose(&mut nodes[proposer], &[], &[]).await;
            let floor = floor_view(&nodes[proposer], &block).await;
            let verdict = nodes[proposer]
                .add_block(block.clone())
                .await
                .expect("process");
            assert!(matches!(verdict, Either::Right(ValidBlock::Valid)));
            let dag = nodes[proposer].casper.block_dag().await.expect("dag");
            let descends = dag
                .is_dag_ancestor(&lfb.block_hash, &block.block_hash)
                .expect("ancestry");
            checks += 1;
            if floor.number < lfb.body.state.block_number || !descends {
                violations.push((
                    ftt_ppm,
                    step,
                    block.body.state.block_number,
                    floor.number,
                    lfb.body.state.block_number,
                    descends,
                ));
            }
            highest_lfb = highest_lfb.max(lfb.body.state.block_number);
            created.push(block);
        }
        assert!(
            highest_lfb > 0,
            "ftt {ftt_ppm} ppm: the finalized block never left genesis"
        );
    }
    assert!(
        violations.is_empty(),
        "{} of {checks} proposals: (ftt ppm, step, block, floor, lfb, descends) {violations:?}",
        violations.len()
    );
}

/// A drain clocked by block number would bound what a voter drained only if
/// no voter had seen a block numbered at or above the block carrying its
/// vote. Fork choice can leave such a block out of the parents: here a heavier
/// validator, holding its own branch, cites only it under a parent cap of one,
/// and its valid block is numbered below a block it had already accepted.
#[tokio::test]
async fn a_valid_block_can_be_numbered_below_a_block_its_proposer_accepted() {
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let stake_of = |validator: &prost::bytes::Bytes| {
        genesis
            .genesis_block
            .body
            .state
            .bonds
            .iter()
            .find(|bond| bond.validator == *validator)
            .expect("bonded")
            .stake
            .clone()
    };
    let mut nodes = TestNode::create_network(genesis.clone(), 2, Some(0.0), Some(1), None)
        .await
        .expect("network");
    let (heavy, light) = if stake_of(&identity(&nodes[0])) > stake_of(&identity(&nodes[1])) {
        (0, 1)
    } else {
        (1, 0)
    };

    let own_branch = propose(&mut nodes[heavy], &[], &[]).await;
    assert!(matches!(
        nodes[heavy]
            .add_block(own_branch.clone())
            .await
            .expect("process"),
        Either::Right(ValidBlock::Valid)
    ));
    let mut highest_seen = None;
    for _ in 0..3 {
        let block = propose(&mut nodes[light], &[], &[]).await;
        for index in [light, heavy] {
            assert!(matches!(
                nodes[index]
                    .add_block(block.clone())
                    .await
                    .expect("process"),
                Either::Right(ValidBlock::Valid)
            ));
        }
        highest_seen = Some(block.body.state.block_number);
    }
    let highest_seen = highest_seen.expect("the light validator proposed");
    assert!(matches!(
        nodes[light].add_block(own_branch).await.expect("process"),
        Either::Right(ValidBlock::Valid)
    ));

    let vote_carrier = propose(&mut nodes[heavy], &[], &[]).await;
    let verdict = nodes[light]
        .add_block(vote_carrier.clone())
        .await
        .expect("process");
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "the heavier validator's block is valid: {verdict:?}"
    );
    assert!(
        vote_carrier.body.state.block_number <= highest_seen,
        "a block numbered {} after its proposer accepted one numbered {highest_seen}",
        vote_carrier.body.state.block_number
    );
}
