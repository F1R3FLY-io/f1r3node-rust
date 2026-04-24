use std::time::Duration;

use casper::rust::engine::running::update_fork_choice_tips_if_stuck;
use casper::rust::util::construct_deploy;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn validator_on_minority_fork_should_rejoin_majority_chain_after_stale_detection() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3)),
        ))
        .await
        .expect("Failed to build genesis");

    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create 3-node network");

    let minority_deploy = construct_deploy::source_deploy_now(
        "@101!(\"minority\")".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .expect("Failed to create minority deploy");

    let minority_block = nodes[0]
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

    let majority_block = TestNode::propagate_block_to_one(&mut nodes, 1, 2, &[majority_deploy])
        .await
        .expect("Node 1 should propagate a majority block to node 2");

    assert!(
        nodes[1].contains(&majority_block.block_hash),
        "Majority validator should contain latest majority block"
    );
    assert!(
        nodes[2].contains(&majority_block.block_hash),
        "Peer majority validator should contain latest majority block"
    );
    assert!(
        !nodes[0].contains(&majority_block.block_hash),
        "Minority validator should not yet know the majority block"
    );
    assert!(
        nodes[0].contains(&minority_block.block_hash),
        "Minority validator should still be on its local fork before recovery"
    );

    update_fork_choice_tips_if_stuck(
        &nodes[0].engine_cell,
        &nodes[0].tle,
        &nodes[0].connections_cell,
        &nodes[0].rp_conf,
        Duration::from_millis(0),
    )
    .await
    .expect("Stale minority validator should trigger recovery");

    let engine_after_detection = nodes[0].engine_cell.get().await;
    assert!(
        engine_after_detection.with_casper().is_none(),
        "Minority validator should move into Initializing after stale detection"
    );

    let queue_to_majority_peer_1 = nodes[1]
        .tle
        .test_network()
        .peer_queue(&nodes[1].local)
        .expect("Majority peer queue should be readable");
    let queue_to_majority_peer_2 = nodes[2]
        .tle
        .test_network()
        .peer_queue(&nodes[2].local)
        .expect("Majority peer queue should be readable");
    let request_type_ids: Vec<String> = queue_to_majority_peer_1
        .iter()
        .chain(queue_to_majority_peer_2.iter())
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
        "Recovery should request ApprovedBlock from majority peers, got queue types: {:?}",
        request_type_ids
    );

}
