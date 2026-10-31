use casper::rust::casper::Casper;
use casper::rust::metrics_constants::CASPER_BUFFER_RELEASE_SCAN_CANDIDATES_METRIC;
use casper::rust::util::construct_deploy;
use metrics_util::debugging::{DebugValue, DebuggingRecorder};
use models::rust::block_hash::BlockHashSerde;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn unrelated_hash(tag: u8, index: u8) -> BlockHashSerde {
    BlockHashSerde(prost::bytes::Bytes::from([tag, index].repeat(16)))
}

#[tokio::test]
async fn after_a_block_is_processed_only_the_blocks_it_released_are_examined() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let shard = genesis.genesis_block.shard_id.clone();

    let deploy_a = construct_deploy::basic_deploy_data(0, None, Some(shard.clone())).unwrap();
    let block_a = nodes[0].add_block_from_deploys(&[deploy_a]).await.unwrap();
    nodes[1].shutoff().unwrap();
    let deploy_b = construct_deploy::basic_deploy_data(1, None, Some(shard)).unwrap();
    let block_b = nodes[0].add_block_from_deploys(&[deploy_b]).await.unwrap();

    let parked = nodes[1].add_block(block_b.clone()).await.unwrap();
    assert!(
        matches!(parked, Either::Left(_)),
        "B waits on A, got {parked:?}"
    );
    for index in 0..20 {
        nodes[1]
            .casper
            .casper_buffer_storage
            .add_relation(unrelated_hash(0xA0, index), unrelated_hash(0xB0, index))
            .unwrap();
    }
    nodes[1].casper.get_dependency_free_from_buffer().unwrap();

    let validated = nodes[1].add_block(block_a.clone()).await.unwrap();
    assert!(
        matches!(validated, Either::Right(_)),
        "A must validate, got {validated:?}"
    );

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let released = metrics::with_local_recorder(&recorder, || {
        nodes[1].casper.get_dependency_free_from_buffer().unwrap()
    });

    assert!(
        released.iter().any(|b| b.block_hash == block_b.block_hash),
        "validating A releases B"
    );
    let candidates: Vec<f64> = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, _, _, _)| key.key().name() == CASPER_BUFFER_RELEASE_SCAN_CANDIDATES_METRIC)
        .flat_map(|(_, _, _, value)| match value {
            DebugValue::Histogram(samples) => samples.into_iter().map(|s| s.into_inner()).collect(),
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(
        candidates,
        vec![1.0],
        "the scan after A examines only B, not the 20 unrelated parked blocks"
    );
}

#[tokio::test]
async fn a_chain_of_missing_ancestors_releases_one_level_per_validation() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let shard = genesis.genesis_block.shard_id.clone();

    let mut chain = Vec::new();
    for index in 0..3 {
        let deploy = construct_deploy::basic_deploy_data(index, None, Some(shard.clone())).unwrap();
        chain.push(nodes[0].add_block_from_deploys(&[deploy]).await.unwrap());
    }
    let (block_a, block_b, block_c) = (chain[0].clone(), chain[1].clone(), chain[2].clone());
    nodes[1].shutoff().unwrap();

    for parked in [&block_c, &block_b] {
        let status = nodes[1].add_block(parked.clone()).await.unwrap();
        assert!(
            matches!(status, Either::Left(_)),
            "a block whose parent is missing parks, got {status:?}"
        );
    }
    let before = nodes[1].casper.get_dependency_free_from_buffer().unwrap();
    assert!(
        !before
            .iter()
            .any(|b| b.block_hash == block_b.block_hash || b.block_hash == block_c.block_hash),
        "no block of the chain is released while A is missing"
    );

    let validated_a = nodes[1].add_block(block_a.clone()).await.unwrap();
    assert!(
        matches!(validated_a, Either::Right(_)),
        "A must validate, got {validated_a:?}"
    );
    let after_a = nodes[1].casper.get_dependency_free_from_buffer().unwrap();
    assert!(
        after_a.iter().any(|b| b.block_hash == block_b.block_hash),
        "validating A releases B in the next scan"
    );
    assert!(
        !after_a.iter().any(|b| b.block_hash == block_c.block_hash),
        "C still waits on B, so the same scan does not release it"
    );

    let validated_b = nodes[1].add_block(block_b.clone()).await.unwrap();
    assert!(
        matches!(validated_b, Either::Right(_)),
        "B must validate, got {validated_b:?}"
    );
    let after_b = nodes[1].casper.get_dependency_free_from_buffer().unwrap();
    assert!(
        after_b.iter().any(|b| b.block_hash == block_c.block_hash),
        "validating B releases C in the next scan, with no timer in between"
    );
}
