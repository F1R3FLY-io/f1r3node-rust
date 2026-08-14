use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::interpreter_util;
use models::casper::RejectedDeployProto;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use prost::Message;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[derive(Clone, PartialEq, Message)]
struct CarrierRejectedDeployWire {
    #[prost(bytes = "bytes", tag = "1")]
    sig: Bytes,
    #[prost(bool, tag = "2")]
    duplicate: bool,
    #[prost(bytes = "bytes", tag = "3")]
    carrier: Bytes,
}

async fn three_node_network() -> (Vec<TestNode>, String) {
    let n_validators = 3usize;
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, n_validators, None, None, None, None)
        .await
        .expect("create network");
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }
    (nodes, shard_id)
}

async fn stage_contest(nodes: &mut [TestNode], shard_id: &str) -> BlockMessage {
    let seed = construct_deploy::source_deploy_now_full(
        r#"@"race"!("s")"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(0),
        Some(shard_id.to_string()),
    )
    .expect("build seed");
    let seed_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&seed))
        .await
        .expect("seed block");
    for index in [0usize, 2usize] {
        nodes[index]
            .process_block(seed_block.clone())
            .await
            .expect("process seed block");
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let first = construct_deploy::source_deploy_now_full(
        r#"for (@v <- @"race") { @"race"!("d") | @"XD"!(v) }"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.to_string()),
    )
    .expect("build first contender");
    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let second = construct_deploy::source_deploy_now_full(
        r#"for (@v <- @"race") { @"race"!("f") | @"XF"!(v) }"#.to_string(),
        None,
        None,
        Some(
            crate::util::genesis_builder::EXTRA_GENESIS_VAULT_KEY_PAIRS[0]
                .0
                .clone(),
        ),
        None,
        Some(shard_id.to_string()),
    )
    .expect("build second contender");
    let first_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&first))
        .await
        .expect("first contender block");
    nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&second))
        .await
        .expect("second contender block");
    first_block
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn record_carrier_is_consensus_checked() {
    let (mut nodes, shard_id) = three_node_network().await;
    let contender_block = stage_contest(&mut nodes, &shard_id).await;

    nodes[1]
        .process_block(contender_block)
        .await
        .expect("process contender block");
    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let marker = construct_deploy::basic_deploy_data(
        7,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(shard_id),
    )
    .expect("merge marker");
    let merge_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&marker))
        .await
        .expect("adjudicating merge block");
    assert!(
        !merge_block.body.rejected_deploys.is_empty(),
        "the merge must produce a rejection record"
    );

    let mut proto = merge_block.to_proto();
    let rejected = &mut proto.body.as_mut().expect("block body").rejected_deploys[0];
    let forged = CarrierRejectedDeployWire {
        sig: rejected.sig.clone(),
        duplicate: false,
        carrier: Bytes::from(vec![0xab; 32]),
    };
    *rejected = RejectedDeployProto::decode(forged.encode_to_vec().as_slice())
        .expect("decode forged rejection record");
    let tampered = BlockMessage::from_proto(proto).expect("decode tampered block");

    let runtime_manager = nodes[1].runtime_manager.clone();
    let mut snapshot = nodes[1]
        .casper
        .get_snapshot()
        .await
        .expect("validation snapshot");
    let result = interpreter_util::validate_block_checkpoint(
        &tampered,
        &nodes[1].block_store,
        &mut snapshot,
        &runtime_manager,
        None,
        None,
    )
    .await
    .expect("validation completes");
    assert!(
        matches!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRejectedDeploy))
        ),
        "a forged rejection-record carrier must be invalid; result: {result:?}"
    );
}
