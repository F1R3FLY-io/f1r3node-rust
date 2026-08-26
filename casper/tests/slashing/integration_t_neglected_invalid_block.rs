use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::{
    Bond, Justification, ProcessedSystemDeploy, SystemDeployData,
};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::{
    propose_with_block_mutation, propose_with_explicit_justifications,
};
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_neglected_invalid_block() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let invalid_deploy =
        construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).expect("deploy");
    let intrinsically_invalid =
        propose_with_block_mutation(&mut nodes[0], vec![invalid_deploy], |block| {
            block.body.state.bonds = vec![Bond {
                validator: Bytes::from(vec![0xa5; models::rust::validator::LENGTH]),
                stake: 999_999_999,
            }];
        })
        .await
        .expect("intrinsically invalid block");

    let invalid_status = nodes[1]
        .process_block(intrinsically_invalid.clone())
        .await
        .expect("process intrinsically invalid block");
    assert!(matches!(
        invalid_status,
        Either::Left(BlockError::Invalid(InvalidBlock::InvalidBondsCache))
    ));
    assert!(nodes[1]
        .casper
        .block_dag_storage
        .get_representation()
        .expect("DAG")
        .lookup_unsafe(&intrinsically_invalid.block_hash)
        .expect("intrinsic-invalid metadata")
        .is_rejected());

    let neglecting_deploy =
        construct_deploy::basic_deploy_data(1, None, Some(shard_id)).expect("deploy");
    let neglecting =
        propose_with_explicit_justifications(&mut nodes[2], vec![neglecting_deploy], vec![
            Justification {
                validator: intrinsically_invalid.sender.clone(),
                latest_block_hash: intrinsically_invalid.block_hash.clone(),
            },
        ])
        .await
        .expect("neglecting block");

    assert!(!neglecting.body.system_deploys.iter().any(|deploy| {
        matches!(deploy, ProcessedSystemDeploy::Succeeded {
            system_deploy: SystemDeployData::Slash { .. },
            ..
        })
    }));

    let neglect_status = nodes[1]
        .process_block(neglecting.clone())
        .await
        .expect("process neglecting block");
    assert!(matches!(
        neglect_status,
        Either::Left(BlockError::Invalid(InvalidBlock::NeglectedInvalidBlock))
    ));

    let generation = neglecting
        .header
        .sender_bond_generation
        .expect("neglecter generation");
    let expected_base_sequence = neglecting.seq_num - 1;
    let records = nodes[1]
        .casper
        .block_dag_storage
        .access_equivocations_tracker(|tracker| tracker.data())
        .expect("equivocation records");
    assert!(records.iter().any(|record| {
        record.equivocator == neglecting.sender
            && record.equivocator_bond_generation == generation
            && record.equivocation_base_block_seq_num == expected_base_sequence
    }));
}
