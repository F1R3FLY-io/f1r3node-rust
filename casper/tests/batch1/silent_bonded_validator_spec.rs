// Bonded validators that never run a node, and finalization (issue #18).

use std::collections::HashMap;

use casper::rust::blocks::proposer::propose_result::CheckProposeConstraintsResult;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::synchrony_constraint_checker;
use casper::rust::util::construct_deploy;
use casper::rust::util::construct_deploy::{DEFAULT_PUB, DEFAULT_SEC};
use crypto::rust::public_key::PublicKey;

use crate::helper::bonding_util;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, DEFAULT_VALIDATOR_KEY_PAIRS};

fn equal_bonds(validators: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
    validators.into_iter().map(|pk| (pk, 10)).collect()
}

/// Four validators with equal stake are bonded at genesis, but only `running`
/// of them run a node. The running ones propose `rounds` blocks round-robin.
/// Returns whether the last finalized block moved off genesis.
async fn lfb_moves_with_running_validators(running: usize, rounds: usize) -> bool {
    let parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(equal_bonds), None);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let genesis_hash = genesis.genesis_block.block_hash.clone();

    let mut nodes = TestNode::create_network(genesis, running, Some(0.0), None, None, None)
        .await
        .unwrap();
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    for round in 0..rounds {
        let deploy =
            construct_deploy::basic_deploy_data(round as i32, None, Some(shard_id.clone()))
                .unwrap();
        TestNode::propagate_block_at_index(&mut nodes, round % running, &[deploy])
            .await
            .unwrap();
    }

    let lfb = nodes[0].casper.last_finalized_block().await.unwrap();
    lfb.block_hash != genesis_hash
}

/// Genesis validators count before their first block, so that a minority can
/// never finalize alone. With half of the genesis stake silent, the agreeing
/// stake is at most half of the total and no block finalizes. This is the
/// intended safety trade-off, not #18.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn half_of_the_genesis_stake_silent_stalls_finalization() {
    assert!(!lfb_moves_with_running_validators(2, 8).await);
}

/// Control: one of four genesis validators is silent, the agreeing stake can
/// exceed half, and finalization advances in the same scenario.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_quarter_of_the_genesis_stake_silent_still_finalizes() {
    assert!(lfb_moves_with_running_validators(3, 8).await);
}

const ROUNDS: i32 = 12;

/// Three running validators propose `ROUNDS` blocks round-robin.
async fn run_rounds(nodes: &mut [TestNode], shard_id: &str, first_deploy: i32) {
    for round in 0..ROUNDS {
        let deploy =
            construct_deploy::basic_deploy_data(first_deploy + round, None, Some(shard_id.into()))
                .unwrap();
        TestNode::propagate_block_at_index(nodes, round as usize % 3, &[deploy])
            .await
            .unwrap();
    }
}

/// Three validators with stake 1, 3 and 5 run a node. A fourth bonds 1000 while
/// the network runs and becomes active at the next epoch boundary. With
/// `node_count == 4` the newcomer has a node (index 3) that receives blocks but
/// does not propose. Returns the nodes after `ROUNDS` blocks, with the newcomer
/// active at the last finalized block.
async fn network_with_a_silent_newcomer(node_count: usize) -> (Vec<TestNode>, String) {
    network_with_newcomer_stake(node_count, 1000, 0).await
}

/// `network_with_a_silent_newcomer` with the newcomer's bond and the on-chain
/// fault-tolerance threshold as parameters. At the default threshold of 0 silence
/// stalls finalization only above half the stake; the shipped 0.67 stalls from
/// about 17%, where a geometry under a third exercises the filter.
async fn network_with_newcomer_stake(
    node_count: usize,
    bond_amount: i64,
    ftt_ppm: i64,
) -> (Vec<TestNode>, String) {
    let validator_key_pairs = vec![
        DEFAULT_VALIDATOR_KEY_PAIRS[0].clone(),
        DEFAULT_VALIDATOR_KEY_PAIRS[1].clone(),
        DEFAULT_VALIDATOR_KEY_PAIRS[2].clone(),
        (DEFAULT_SEC.clone(), DEFAULT_PUB.clone()),
    ];
    let bonds: HashMap<PublicKey, i64> = validator_key_pairs
        .iter()
        .take(3)
        .enumerate()
        .map(|(i, (_, pk))| (pk.clone(), 2 * i as i64 + 1))
        .collect();
    let mut parameters = GenesisBuilder::build_genesis_parameters(validator_key_pairs, &bonds);
    parameters.2.proof_of_stake.epoch_length = 4;
    parameters.2.proof_of_stake.fault_tolerance_threshold_ppm = ftt_ppm;
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis, node_count, Some(0.0), None, None, None)
        .await
        .unwrap();
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let bond =
        bonding_util::bonding_deploy(bond_amount, &DEFAULT_SEC, Some(shard_id.clone())).unwrap();
    TestNode::propagate_block_at_index(&mut nodes, 0, &[bond])
        .await
        .unwrap();

    run_rounds(&mut nodes, &shard_id, 0).await;
    let lfb = nodes[0].casper.last_finalized_block().await.unwrap();
    let active = nodes[0]
        .runtime_manager
        .get_active_validators(&lfb.body.state.post_state_hash)
        .await
        .unwrap();
    assert!(
        active.iter().any(|v| v == &DEFAULT_PUB.bytes.to_vec()),
        "precondition: the new validator must be active at the LFB #{}",
        lfb.body.state.block_number
    );
    (nodes, shard_id)
}

/// #18: the silent newcomer's stake (1000) outweighs the running validators
/// (1 + 3 + 5). Finalization must keep advancing, and a user deploy sent while
/// the newcomer is silent must finalize instead of expiring.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_validator_bonded_later_that_never_runs_does_not_stall_finalization() {
    let (mut nodes, shard_id) = network_with_a_silent_newcomer(3).await;
    let before = nodes[0].casper.last_finalized_block().await.unwrap();

    let deploy = construct_deploy::basic_deploy_data(1000, None, Some(shard_id.clone())).unwrap();
    let carrier = TestNode::propagate_block_at_index(&mut nodes, 1, std::slice::from_ref(&deploy))
        .await
        .unwrap();
    assert!(
        carrier
            .body
            .deploys
            .iter()
            .any(
                |processed| processed.identity_bytes() == deploy.sig.as_ref()
                    && !processed.is_failed()
            ),
        "the user deploy must be included and executed"
    );

    run_rounds(&mut nodes, &shard_id, ROUNDS).await;
    let after = nodes[0].casper.last_finalized_block().await.unwrap();
    assert!(
        after.body.state.block_number > before.body.state.block_number,
        "finalization stalled at LFB #{} after {ROUNDS} more blocks",
        before.body.state.block_number
    );
    let dag = nodes[0].block_dag_storage.get_representation().unwrap();
    assert!(
        dag.is_finalized(&carrier.block_hash),
        "the block that carries the user deploy (#{}) must finalize; LFB is #{}",
        carrier.body.state.block_number,
        after.body.state.block_number
    );
}

/// #18 for block creation: with the synchrony constraint that heartbeat shards
/// use (0.33), the silent newcomer must not stop a running validator from
/// proposing. The other running validators proposed after validator 0, so it
/// has seen all of the stake that can speak.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silent_newcomer_does_not_block_the_synchrony_constraint() {
    let (nodes, _) = network_with_a_silent_newcomer(3).await;
    let mut snapshot = nodes[0].casper.get_snapshot().await.unwrap();
    snapshot
        .on_chain_state
        .shard_conf
        .synchrony_constraint_threshold = 0.33;

    let main_parent = &snapshot.parents[0];
    assert!(
        main_parent
            .body
            .state
            .bonds
            .iter()
            .any(|bond| bond.validator == DEFAULT_PUB.bytes),
        "precondition: the newcomer must be in the main parent's committee"
    );

    let identity = nodes[0].validator_id_opt.clone().unwrap();
    let result = synchrony_constraint_checker::check(&snapshot, &identity)
        .await
        .unwrap();
    assert!(
        matches!(result, CheckProposeConstraintsResult::Success),
        "{result:?}"
    );
}

/// The filter is in the oracle, not in the committee: the silent newcomer stays
/// in the committee, so when its node starts it can produce its first block and
/// the running nodes accept it. From that block its stake counts: while only the
/// small validators propose, nothing above the newcomer's block finalizes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_newcomer_that_starts_its_node_is_accepted_and_then_counts() {
    let (mut nodes, shard_id) = network_with_a_silent_newcomer(4).await;

    let deploy = construct_deploy::basic_deploy_data(2000, None, Some(shard_id.clone())).unwrap();
    let first = TestNode::propagate_block_at_index(&mut nodes, 3, &[deploy])
        .await
        .expect("the newcomer must be able to produce its first block");
    for node in &nodes[..3] {
        let dag = node.block_dag_storage.get_representation().unwrap();
        assert_eq!(
            dag.latest_messages_map.get(&DEFAULT_PUB.bytes),
            Some(&first.block_hash),
            "{} must accept the newcomer's first block",
            node.name
        );
    }

    let before = nodes[0]
        .casper
        .last_finalized_block()
        .await
        .unwrap()
        .body
        .state
        .block_number;
    run_rounds(&mut nodes, &shard_id, 3000).await;
    let lfb = nodes[0].casper.last_finalized_block().await.unwrap();
    assert!(
        lfb.body.state.block_number <= first.body.state.block_number,
        "the newcomer's stake must count from its first block (#{}), but LFB is #{}",
        first.body.state.block_number,
        lfb.body.state.block_number
    );
    assert!(
        lfb.body.state.block_number >= before,
        "finalization regressed from #{before} to #{}",
        lfb.body.state.block_number
    );
}

/// The newcomer holds 4 of 13 stake, under a third, at the shipped 0.67 threshold
/// where its silence stalls finalization without the filter. It is kept off the
/// rounds after a block finalizes, so it proposes a sibling from a stale view with
/// its stake counted again. No two nodes may finalize mutually unreachable blocks.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_in_model_silent_newcomer_cannot_finalize_a_fork_after_it_returns() {
    let (mut nodes, shard_id) = network_with_newcomer_stake(4, 4, 670_000).await;

    for round in 0..6 {
        let deploy =
            construct_deploy::basic_deploy_data(5000 + round, None, Some(shard_id.clone()))
                .unwrap();
        TestNode::propagate_block_at_index(&mut nodes[..3], round as usize % 3, &[deploy])
            .await
            .unwrap();
    }

    let deploy = construct_deploy::basic_deploy_data(6000, None, Some(shard_id.clone())).unwrap();
    let sibling = TestNode::propagate_block_at_index(&mut nodes, 3, &[deploy]).await;

    for round in 0..8 {
        let deploy =
            construct_deploy::basic_deploy_data(7000 + round, None, Some(shard_id.clone()))
                .unwrap();
        let _ = TestNode::propagate_block_at_index(&mut nodes[..3], round as usize % 3, &[deploy])
            .await;
    }

    let mut lfb_hashes = Vec::new();
    let mut lfb_numbers = Vec::new();
    for node in nodes.iter() {
        let lfb = node.casper.last_finalized_block().await.unwrap();
        lfb_hashes.push(lfb.block_hash.clone());
        lfb_numbers.push(lfb.body.state.block_number);
    }

    // A merged sibling is reachable and benign; only unreachable tips diverge.
    let dag = nodes[0].block_dag_storage.get_representation().unwrap();
    let closure = |start: &models::rust::block_hash::BlockHash| {
        let mut seen = std::collections::HashSet::new();
        let mut queue = vec![start.clone()];
        while let Some(hash) = queue.pop() {
            if !seen.insert(hash.clone()) {
                continue;
            }
            if let Ok(meta) = dag.lookup_unsafe(&hash) {
                queue.extend(meta.parents);
            }
        }
        seen
    };
    let incompatible: Vec<(usize, usize)> = (0..lfb_hashes.len())
        .flat_map(|i| ((i + 1)..lfb_hashes.len()).map(move |j| (i, j)))
        .filter(|(i, j)| {
            !closure(&lfb_hashes[*i]).contains(&lfb_hashes[*j])
                && !closure(&lfb_hashes[*j]).contains(&lfb_hashes[*i])
        })
        .collect();
    assert!(
        incompatible.is_empty(),
        "nodes finalized mutually unreachable blocks: pairs {incompatible:?}, LFBs \
         {lfb_numbers:?}, sibling #{}",
        match &sibling {
            Ok(block) => block.body.state.block_number,
            Err(_) => -1,
        }
    );
}
