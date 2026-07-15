use std::time::Duration;

use casper::rust::engine::running::update_fork_choice_tips_if_stuck;
use casper::rust::util::construct_deploy;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn validator_on_minority_fork_should_request_approved_block_for_rejoin() {
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
        .expect(
            "Majority bootstrap should propagate a majority block to another majority validator",
        );

    assert!(
        nodes[majority_bootstrap].contains(&majority_block.block_hash),
        "Majority bootstrap should contain latest majority block"
    );
    assert!(
        nodes[majority_peer].contains(&majority_block.block_hash),
        "Peer majority validator should contain latest majority block"
    );
    assert!(
        !nodes[minority_validator].contains(&majority_block.block_hash),
        "Minority validator should not yet know the majority block"
    );
    assert!(
        nodes[minority_validator].contains(&minority_block.block_hash),
        "Minority validator should still be on its local fork before recovery"
    );

    update_fork_choice_tips_if_stuck(
        &nodes[minority_validator].engine_cell,
        &nodes[minority_validator].tle,
        &nodes[minority_validator].connections_cell,
        &nodes[minority_validator].rp_conf,
        Duration::from_millis(0),
    )
    .await
    .expect("Stale minority validator should trigger recovery");

    let engine_after_detection = nodes[minority_validator].engine_cell.get().await;
    assert!(
        engine_after_detection.with_casper().is_none(),
        "Minority validator should move into Initializing after stale detection"
    );

    let queue_to_bootstrap = nodes[majority_bootstrap]
        .tle
        .test_network()
        .peer_queue(&nodes[majority_bootstrap].local)
        .expect("Bootstrap queue should be readable");
    let request_type_ids: Vec<String> = queue_to_bootstrap
        .iter()
        .filter_map(|protocol| {
            protocol.message.as_ref().and_then(|message| match message {
                models::routing::protocol::Message::Packet(packet) => Some(packet.type_id.clone()),
                _ => None,
            })
        })
        .collect();
    assert!(
        request_type_ids
            .iter()
            .any(|type_id| type_id == "ApprovedBlockRequest"),
        "Recovery should request ApprovedBlock from bootstrap, got queue types: {:?}",
        request_type_ids
    );

    nodes[majority_bootstrap]
        .handle_receive()
        .await
        .expect("Bootstrap should answer the approved-block request");

    // Full Initializing restore needs reentrant LFS queue pumping; this harness
    // asserts through the approved-block handoff.
    let queue_to_minority = nodes[minority_validator]
        .tle
        .test_network()
        .peer_queue(&nodes[minority_validator].local)
        .expect("Minority validator queue should be readable");
    let response_type_ids: Vec<String> = queue_to_minority
        .iter()
        .filter_map(|protocol| {
            protocol.message.as_ref().and_then(|message| match message {
                models::routing::protocol::Message::Packet(packet) => Some(packet.type_id.clone()),
                _ => None,
            })
        })
        .collect();

    assert!(
        response_type_ids
            .iter()
            .any(|type_id| type_id == "ApprovedBlock"),
        "Bootstrap should send ApprovedBlock back to the minority validator, got queue types: {:?}",
        response_type_ids
    );

    assert!(
        nodes[majority_peer].contains(&majority_block.block_hash),
        "Majority peer should still contain the majority block ready for later block sync"
    );
}
