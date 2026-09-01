use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::MultiParentCasper;
use casper::rust::util::construct_deploy;
use models::rust::block_metadata::AdmissionRejectionReason;
use models::rust::casper::protocol::casper_message::Bond;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::propose_with_block_mutation;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_demoted_invalid_block_cannot_seed_a_neglect_cascade() {
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
    assert!(nodes[1].contains(&intrinsically_invalid.block_hash));
    let dag = nodes[1]
        .casper
        .block_dag()
        .await
        .expect("DAG representation");
    let metadata = dag
        .lookup(&intrinsically_invalid.block_hash)
        .expect("metadata lookup")
        .expect("certified rejection metadata");
    assert_eq!(
        metadata.rejection_reason(),
        Some(AdmissionRejectionReason::InvalidBondsCache)
    );
    assert!(!metadata.is_slash_evidence_eligible());
    let records_before = nodes[1]
        .casper
        .block_dag_storage
        .access_equivocations_tracker(|tracker| tracker.data())
        .expect("equivocation records before valid successor");
    assert!(records_before.is_empty());

    let neglecting_deploy =
        construct_deploy::basic_deploy_data(1, None, Some(shard_id)).expect("deploy");
    let valid_successor = nodes[2]
        .create_block_unsafe(&[neglecting_deploy])
        .await
        .expect("valid successor");

    let successor_status = nodes[1]
        .process_block(valid_successor)
        .await
        .expect("process valid successor");
    assert!(matches!(successor_status, Either::Right(_)));

    let records = nodes[1]
        .casper
        .block_dag_storage
        .access_equivocations_tracker(|tracker| tracker.data())
        .expect("equivocation records");
    assert!(records.is_empty());
}
