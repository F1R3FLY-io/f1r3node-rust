// The parameters fixed at genesis are consensus values, so they live in the
// PoS contract and every node reads the same integers back. This pins the
// whole path: the genesis substitutions, the reader's mapping of every name,
// and the node's adoption of it.

use std::num::NonZeroU32;

use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::chain_parameters::ChainParameters;
use casper::rust::economics::bucket_view::LeakyBucketParams;
use casper::rust::economics::reward::RewardParams;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn the_pos_contract_exposes_every_chain_parameter() {
    // Every value differs from its default and from every other, so a field
    // read under the wrong name cannot pass.
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let pos = &mut parameters.2.proof_of_stake;
    pos.fault_tolerance_threshold_ppm = 123_456;
    pos.max_parent_depth = 17;
    pos.deploy_lifespan = 61;
    pos.min_phlo_price = 3;
    pos.epoch_length = 997;
    pos.quarantine_length = 50_021;
    pos.lambda_base_ppm = 11_111;
    pos.theta_eject_ppm = 888_888;
    pos.eject_observation_blocks = 47;
    pos.breadth_exponent = 3;
    pos.recency_reference_ppm = 900_001;
    pos.vote_list_bound = 7;
    pos.epoch_phlogiston = 1_234_567;
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let post_state = genesis.genesis_block.body.state.post_state_hash.clone();
    let nodes = TestNode::create_network(genesis, 1, Some(0.0), None, None)
        .await
        .expect("network");

    let read = nodes[0]
        .runtime_manager
        .chain_parameters(&post_state)
        .await
        .expect("the PoS contract exposes the chain parameters");
    let expected = ChainParameters {
        fault_tolerance_threshold_ppm: 123_456,
        max_parent_depth: 17,
        deploy_lifespan: 61,
        min_phlo_price: 3,
        epoch_length: 997,
        quarantine_length: 50_021,
        leaky_bucket: LeakyBucketParams {
            lambda_ppm: 11_111,
            theta_eject_ppm: 888_888,
            eject_observation_blocks: 47,
            // One epoch, derived rather than fixed.
            claim_deadline_blocks: 997,
            // The parent depth plus the longest look-back, the epoch here.
            retention_window_blocks: Some(17 + 997),
            vote_list_bound: NonZeroU32::new(7).expect("positive"),
            reward: RewardParams {
                epoch_phlogiston: 1_234_567,
                epoch_length: std::num::NonZeroU64::new(997).expect("positive"),
                p_breadth: 3,
                y_ref_ppm: 900_001,
            },
        },
    };
    assert_eq!(read, expected, "each value arrives under its own name");
    assert_eq!(
        *nodes[0].casper.chain_parameters(),
        expected,
        "the node judges blocks by the chain's values"
    );
    let snapshot = nodes[0].casper.get_snapshot().await.expect("snapshot");
    assert_eq!(
        snapshot.on_chain_state.chain, expected,
        "every snapshot carries the chain parameters the node adopted"
    );
}
