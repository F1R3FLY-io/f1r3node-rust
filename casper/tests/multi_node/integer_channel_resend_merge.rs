use casper::rust::block_status::ValidBlock;
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const FIRST_SEND: &str = r#"@"integer-resend"!(1)"#;
const SECOND_SEND: &str = r#"@"integer-resend"!(2)"#;

fn assert_no_failed_deploys(block: &BlockMessage) {
    assert!(
        block.body.deploys.iter().all(|deploy| !deploy.is_failed),
        "a deploy failed in block {}: {:?}",
        hex::encode(&block.block_hash[..block.block_hash.len().min(8)]),
        block
            .body
            .deploys
            .iter()
            .map(|deploy| deploy.system_deploy_error.clone())
            .collect::<Vec<_>>()
    );
}

/// A second send of an integer onto an untagged channel that already holds one
/// finalized integer is an ordinary Rholang produce: the channel holds both
/// values afterwards. When the block carrying it is merged as a side parent,
/// the merge must keep it.
///
/// Validator 1 proposes the second send without seeing two sibling blocks that
/// validators 0 and 2 build in sequence, so the merge block's fork choice
/// places validator 2's block (two validators' votes) as main parent and
/// merges the carrier (one vote) beside it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_integer_send_merged_as_a_side_parent_is_not_rejected() {
    crate::init_logger();

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, 3, None, None, None, None)
        .await
        .unwrap();
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let deploy = |source: &str, sec, offset: i64| {
        construct_deploy::source_deploy(
            source.to_string(),
            now + offset,
            Some(5_000_000),
            None,
            Some(sec),
            None,
            Some(shard_id.clone()),
        )
        .unwrap()
    };

    let first = nodes[0]
        .add_block_from_deploys(&[deploy(FIRST_SEND, construct_deploy::DEFAULT_SEC.clone(), 0)])
        .await
        .expect("validator 0 proposes the first send");
    assert_no_failed_deploys(&first);
    TestNode::sync_all(&mut nodes).await.unwrap();

    let sibling_a = nodes[0]
        .add_block_from_deploys(&[deploy("Nil", construct_deploy::DEFAULT_SEC.clone(), 1)])
        .await
        .expect("validator 0 proposes a sibling");
    let status = nodes[2].process_block(sibling_a.clone()).await.unwrap();
    assert_eq!(status, Either::Right(ValidBlock::Valid));
    let main_line = nodes[2]
        .add_block_from_deploys(&[deploy("Nil", construct_deploy::DEFAULT_SEC.clone(), 2)])
        .await
        .expect("validator 2 builds on validator 0's sibling");

    let resend = deploy(SECOND_SEND, construct_deploy::DEFAULT_SEC2.clone(), 3);
    let carrier = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&resend))
        .await
        .expect("validator 1 proposes the second send");
    assert_no_failed_deploys(&carrier);
    assert_eq!(
        carrier.header.parents_hash_list,
        vec![first.block_hash.clone()],
        "the carrier must be built without seeing the sibling line"
    );
    TestNode::sync_all(&mut nodes).await.unwrap();

    let merge_block = nodes[0]
        .add_block_from_deploys(&[deploy("Nil", construct_deploy::DEFAULT_SEC.clone(), 4)])
        .await
        .expect("validator 0 proposes the merge block");
    assert_eq!(
        merge_block.header.parents_hash_list.first(),
        Some(&main_line.block_hash),
        "precondition: the two-vote line is the main parent"
    );
    assert!(
        merge_block
            .header
            .parents_hash_list
            .contains(&carrier.block_hash),
        "precondition: the carrier is merged as a side parent"
    );

    for node in nodes[1..].iter_mut() {
        let status = node.process_block(merge_block.clone()).await.unwrap();
        assert_eq!(status, Either::Right(ValidBlock::Valid));
    }

    assert!(
        !merge_block
            .body
            .rejected_deploys
            .iter()
            .any(|rejected| rejected.sig == resend.sig),
        "the merge rejected a second integer send onto an untagged channel holding one integer"
    );
}
