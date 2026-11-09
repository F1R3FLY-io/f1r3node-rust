// Added by DR-134 (DR-71 verification obligations): the exploratory estimate is
// a native unfunded dry run. It equals the measured phlo of the same term
// executed as a funded offer of the same key at the same root, and it leaves
// the root unchanged.

use std::time::{SystemTime, UNIX_EPOCH};

use casper::rust::api::block_api::BlockAPI;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::offered_deploy::owner_direct_offer;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const TERM: &str = r#"new x, out in { x!(1) | for (@v <- x) { out!(v) | @"dry_run_out"!(v) } }"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_dry_run_estimate_equals_the_measured_phlo_of_the_funded_offer() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("the protocol-6 genesis builds");
    let mut nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, None)
        .await
        .expect("a one-validator network");
    let policy = nodes[0]
        .casper
        .adopted_resource_policy
        .clone()
        .expect("the offered genesis has an adopted policy");
    let root = genesis.genesis_block.body.state.post_state_hash.clone();
    let owner = genesis.genesis_vaults[0].clone();

    let (_, estimate) = nodes[0]
        .runtime_manager
        .play_exploratory_dry_run(TERM.to_string(), &root, Some(owner.1.clone()), &policy)
        .await
        .expect("the dry run measures the term");
    assert!(estimate > 0, "the term communicates and introduces data");
    let (_, again) = nodes[0]
        .runtime_manager
        .play_exploratory_dry_run(TERM.to_string(), &root, Some(owner.1.clone()), &policy)
        .await
        .expect("a second dry run at the same root");
    assert_eq!(again, estimate, "the dry run leaves the root unchanged");

    let time_stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_millis() as i64;
    let offer = owner_direct_offer(&owner, time_stamp, TERM.to_string());
    BlockAPI::deploy_offered(&nodes[0].engine_cell, offer.clone(), &None, false, "root")
        .await
        .expect("the offer is admitted");
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the offer's block is created");
    assert_eq!(
        block.body.state.pre_state_hash, root,
        "the funded offer runs at the dry run's root"
    );
    assert!(matches!(
        nodes[0]
            .process_block(block.clone())
            .await
            .expect("the block is processed"),
        Either::Right(_)
    ));
    let receipt =
        BlockAPI::find_offered_settlement_receipt(&nodes[0].engine_cell, &offer.deploy_id.to_vec())
            .await
            .expect("the receipt query runs")
            .expect("the block holds the offer's receipt");
    assert_eq!(
        receipt.phlo_used, estimate,
        "the estimate equals the funded offer's measured phlo"
    );
}
