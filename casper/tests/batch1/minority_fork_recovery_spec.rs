use std::time::Duration;

use casper::rust::casper::MultiParentCasper;
use casper::rust::engine::running::update_fork_choice_tips_if_stuck;
use casper::rust::util::construct_deploy;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn validator_on_minority_fork_recovers_through_normal_dag_admission() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3)),
        ))
        .await
        .expect("Failed to build genesis");

    let shard_id = genesis.genesis_block.shard_id.clone();
    let majority_bootstrap = 1;
    let majority_peer = 2;
    let minority_validator = 0;
    let mut nodes =
        TestNode::create_network_with_bootstrap_index(genesis.clone(), 3, majority_bootstrap)
            .await
            .expect("Failed to create 3-node network");

    let minority_deploy = construct_deploy::source_deploy_now(
        "@101!(\"minority\")".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .expect("Failed to create minority deploy");
    let minority_block = nodes[minority_validator]
        .add_block_from_deploys(&[minority_deploy])
        .await
        .expect("Validator on minority fork should create a local block");

    let majority_deploy = construct_deploy::source_deploy_now(
        "@201!(\"majority\")".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .expect("Failed to create majority deploy");
    let majority_block =
        TestNode::propagate_block_to_one(&mut nodes, majority_bootstrap, majority_peer, &[
            majority_deploy,
        ])
        .await
        .expect("Majority block should reach the second majority validator");

    let support_deploy = construct_deploy::source_deploy_now(
        "@202!(\"majority-support\")".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .expect("Failed to create majority support deploy");
    let majority_tip =
        TestNode::propagate_block_to_one(&mut nodes, majority_peer, majority_bootstrap, &[
            support_deploy,
        ])
        .await
        .expect("Second majority validator should extend and return the majority fork");

    assert!(!nodes[minority_validator].contains(&majority_block.block_hash));
    assert!(!nodes[minority_validator].contains(&majority_tip.block_hash));
    assert!(nodes[minority_validator].contains(&minority_block.block_hash));

    update_fork_choice_tips_if_stuck(
        &nodes[minority_validator].engine_cell,
        &nodes[minority_validator].tle,
        &nodes[minority_validator].connections_cell,
        &nodes[minority_validator].rp_conf,
        Duration::from_millis(0),
    )
    .await
    .expect("Stale minority validator should trigger live recovery");

    assert!(
        nodes[minority_validator]
            .engine_cell
            .get()
            .await
            .with_casper()
            .is_some(),
        "Live recovery must retain the existing Casper instance and durable state"
    );

    for _ in 0..12 {
        for node in &nodes {
            node.handle_receive()
                .await
                .expect("Recovery gossip should remain valid Casper traffic");
        }
        tokio::task::yield_now().await;
    }

    assert!(nodes[minority_validator].contains(&majority_block.block_hash));
    assert!(nodes[minority_validator].contains(&majority_tip.block_hash));

    let recovered_lfb = nodes[minority_validator]
        .casper
        .last_finalized_block()
        .await
        .expect("Minority validator should recompute finality locally");
    let recovered_dag = nodes[minority_validator]
        .casper
        .block_dag()
        .await
        .expect("Recovered DAG should be readable");
    assert!(
        recovered_lfb.block_hash == majority_block.block_hash
            || recovered_dag
                .is_dag_ancestor(&majority_block.block_hash, &recovered_lfb.block_hash)
                .expect("Recovered finalized lineage should be decidable")
    );

    let resumed_deploy = construct_deploy::source_deploy_now(
        "@301!(\"resumed\")".to_string(),
        None,
        None,
        Some(shard_id),
    )
    .expect("Failed to create resumed deploy");
    let resumed = nodes[minority_validator]
        .add_block_from_deploys(&[resumed_deploy])
        .await
        .expect("Recovered validator should resume ordinary proposal");
    let resumed_dag = nodes[minority_validator]
        .casper
        .block_dag()
        .await
        .expect("Resumed DAG should be readable");
    assert!(resumed_dag
        .is_dag_ancestor(&recovered_lfb.block_hash, &resumed.block_hash)
        .expect("Resumed block lineage should be decidable"));
}
