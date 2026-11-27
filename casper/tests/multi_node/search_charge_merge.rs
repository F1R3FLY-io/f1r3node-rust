use casper::rust::block_status::ValidBlock;
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn assert_no_failed_deploys(block: &BlockMessage) {
    assert!(
        block.body.deploys.iter().all(|deploy| !deploy.is_failed),
        "a deploy failed: {:?}",
        block
            .body
            .deploys
            .iter()
            .map(|deploy| deploy.system_deploy_error.clone())
            .collect::<Vec<_>>()
    );
}

/// Sends made in two concurrent blocks wait on one channel once the blocks
/// are merged. A block built on the merge pays for its receives' searches
/// over both sets, and every validator, replaying it from the merged state,
/// charges the same.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn searches_over_data_merged_from_concurrent_blocks_replay_at_the_played_cost() {
    crate::init_logger();

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, 3, None, None, None, None)
        .await
        .unwrap();

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

    let left = nodes[0]
        .add_block_from_deploys(&[deploy(
            r#"@"merged"!(1) | @"merged"!(2)"#,
            construct_deploy::DEFAULT_SEC.clone(),
            0,
        )])
        .await
        .expect("validator 0 proposes sends");
    let right = nodes[1]
        .add_block_from_deploys(&[deploy(
            r#"@"merged"!(3) | @"merged"!(4)"#,
            construct_deploy::DEFAULT_SEC2.clone(),
            1,
        )])
        .await
        .expect("validator 1 proposes sends without seeing validator 0's");
    assert_no_failed_deploys(&left);
    assert_no_failed_deploys(&right);
    TestNode::sync_all(&mut nodes).await.unwrap();

    let searching = nodes[2]
        .add_block_from_deploys(&[deploy(
            r#"for (@x <- @"merged") { Nil } | for (@3 <- @"merged") { Nil } | for (@7 <- @"merged") { Nil } | @"merged"!(5)"#,
            construct_deploy::DEFAULT_SEC.clone(),
            2,
        )])
        .await
        .expect("validator 2 proposes receives on the merged state");
    assert!(
        searching
            .header
            .parents_hash_list
            .contains(&left.block_hash)
            && searching
                .header
                .parents_hash_list
                .contains(&right.block_hash),
        "precondition: the block merges both senders' blocks, parents {:?}",
        searching.header.parents_hash_list
    );
    assert!(
        searching.body.rejected_deploys.is_empty(),
        "precondition: the merge keeps both blocks' sends"
    );
    assert_no_failed_deploys(&searching);

    for node in nodes[..2].iter_mut() {
        let status = node.process_block(searching.clone()).await.unwrap();
        assert_eq!(status, Either::Right(ValidBlock::Valid));
    }
}
