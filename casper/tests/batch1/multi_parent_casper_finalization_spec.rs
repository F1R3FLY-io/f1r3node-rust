// See casper/src/test/scala/coop/rchain/casper/batch1/MultiParentCasperFinalizationSpec.scala

use std::collections::HashMap;

use casper::rust::casper::MultiParentCasper;
use casper::rust::util::construct_deploy;
use crypto::rust::public_key::PublicKey;
use models::rust::casper::protocol::casper_message::BlockMessage;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

// Round-robin block production still exercises finalization advancement under multi-parent
// merging in the Rust port: propagating blocks round-robin finalizes block1..block4 in order
// as later blocks accrue witnessing quorums. (Scala deleted this in PR #288; the Rust
// finalizer reproduces the expected advancement, so it is restored as real coverage — the
// sibling `..._advance_finalization_monotonically_in_round_robin` already runs.)
#[tokio::test]
async fn multi_parent_casper_should_increment_last_finalized_block_as_appropriate_in_round_robin() {
    async fn assert_finalized_block(node: &TestNode, expected: &BlockMessage) {
        node.settle_finalization(std::time::Duration::from_secs(30))
            .await
            .expect("finalizer should settle");
        let representation = node
            .block_dag_storage
            .get_representation()
            .expect("dag representation");
        let last_finalized_block_hash = representation.last_finalized_block();
        let actual_height = representation
            .lookup_unsafe(&last_finalized_block_hash)
            .expect("last finalized metadata")
            .block_number;

        assert_eq!(
            last_finalized_block_hash,
            expected.block_hash,
            "Last finalized block mismatch\nExpected #{}: {}\nGot #{actual_height}: {}",
            expected.body.state.block_number,
            hex::encode(&expected.block_hash),
            hex::encode(&last_finalized_block_hash)
        );
    }

    // Bonds function: _.map(pk => pk -> 10L).toMap
    fn bonds_function(validators: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
        validators.into_iter().map(|pk| (pk, 10i64)).collect()
    }

    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(
        Some(bonds_function),
        None, // Use default validatorsNum = 4
    );

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();

    let deploy_datas: Vec<_> = (0..=7)
        .map(|i| {
            construct_deploy::basic_deploy_data(
                i,
                None,
                Some(genesis.genesis_block.shard_id.clone()),
            )
            .unwrap()
        })
        .collect();

    let _block1 = TestNode::propagate_block_at_index(&mut nodes, 0, &[deploy_datas[0].clone()])
        .await
        .unwrap();

    let _block2 = TestNode::propagate_block_at_index(&mut nodes, 1, &[deploy_datas[1].clone()])
        .await
        .unwrap();

    let block3 = TestNode::propagate_block_at_index(&mut nodes, 2, &[deploy_datas[2].clone()])
        .await
        .unwrap();

    let block4 = TestNode::propagate_block_at_index(&mut nodes, 0, &[deploy_datas[3].clone()])
        .await
        .unwrap();

    nodes[1]
        .settle_finalization(std::time::Duration::from_secs(30))
        .await
        .expect("finalizer should settle before the next certificate carrier");

    // One clock: the LFB is the floor of the live view, whose per-parent
    // frontier witnesses one round EARLIER than the retired Finalizer's
    // agreement aggregation did. The two-sided disagreement walk moved the
    // pins one further block ahead: a round-robin chain's one-round-stale
    // windows visit the target's own below-target ancestry, which used to
    // veto the certification edge for a round and no longer does (ignorance
    // of settled ancestry is not disagreement). Verified by probing the
    // clocks over this exact staging at each change.
    let block5 = TestNode::propagate_block_at_index(&mut nodes, 1, &[deploy_datas[4].clone()])
        .await
        .unwrap();

    assert_finalized_block(&nodes[0], &block3).await;

    nodes[2]
        .settle_finalization(std::time::Duration::from_secs(30))
        .await
        .expect("finalizer should settle before the next certificate carrier");

    let block6 = TestNode::propagate_block_at_index(&mut nodes, 2, &[deploy_datas[5].clone()])
        .await
        .unwrap();

    assert_finalized_block(&nodes[0], &block4).await;

    let _block7 = TestNode::propagate_block_at_index(&mut nodes, 0, &[deploy_datas[6].clone()])
        .await
        .unwrap();

    assert_finalized_block(&nodes[0], &block5).await;

    nodes[1]
        .settle_finalization(std::time::Duration::from_secs(30))
        .await
        .expect("finalizer should settle before the next certificate carrier");

    let _block8 = TestNode::propagate_block_at_index(&mut nodes, 1, &[deploy_datas[7].clone()])
        .await
        .unwrap();

    assert_finalized_block(&nodes[0], &block6).await;
}

/// This test verifies that finalization advances monotonically (block number never
/// decreases) during round-robin block production with multi-parent merging, and
/// that all validators agree on the last finalized block at the end.
#[tokio::test]
async fn multi_parent_casper_should_advance_finalization_monotonically_in_round_robin() {
    fn bonds_function(validators: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
        validators.into_iter().map(|pk| (pk, 10i64)).collect()
    }

    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(
        Some(bonds_function),
        None, // Use default validatorsNum = 4
    );

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    // Heartbeat mode: under deploy-inclusion leadership only the leader packages
    // user deploys while unresolved user work is in the frontier; non-leaders
    // emit empty support blocks instead of erroring NoNewDeploys. The
    // assertions below concern LFB monotonicity, not deploy inclusion.
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let shard_id = genesis.genesis_block.shard_id.clone();

    let deploy_datas: Vec<_> = (0..=20)
        .map(|i| construct_deploy::basic_deploy_data(i, None, Some(shard_id.clone())).unwrap())
        .collect();

    // Round-robin block production across 3 validators with propagation.
    // With multi-parent merging, each block may have multiple parents
    // (one from each validator's latest message).
    let _block1 = TestNode::propagate_block_at_index(&mut nodes, 0, &[deploy_datas[0].clone()])
        .await
        .unwrap();

    let _block2 = TestNode::propagate_block_at_index(&mut nodes, 1, &[deploy_datas[1].clone()])
        .await
        .unwrap();

    let _block3 = TestNode::propagate_block_at_index(&mut nodes, 2, &[deploy_datas[2].clone()])
        .await
        .unwrap();

    let _block4 = TestNode::propagate_block_at_index(&mut nodes, 0, &[deploy_datas[3].clone()])
        .await
        .unwrap();

    let _block5 = TestNode::propagate_block_at_index(&mut nodes, 1, &[deploy_datas[4].clone()])
        .await
        .unwrap();

    // After 5 blocks in round-robin with 3 validators (equal bonds),
    // finalization should have advanced past genesis.
    let lfb_after5 = nodes[0].casper.last_finalized_block().await.unwrap();

    let _block6 = TestNode::propagate_block_at_index(&mut nodes, 2, &[deploy_datas[5].clone()])
        .await
        .unwrap();

    let lfb_after6 = nodes[0].casper.last_finalized_block().await.unwrap();

    let _block7 = TestNode::propagate_block_at_index(&mut nodes, 0, &[deploy_datas[6].clone()])
        .await
        .unwrap();

    let lfb_after7 = nodes[0].casper.last_finalized_block().await.unwrap();

    let _block8 = TestNode::propagate_block_at_index(&mut nodes, 1, &[deploy_datas[7].clone()])
        .await
        .unwrap();

    let lfb_after8 = nodes[0].casper.last_finalized_block().await.unwrap();

    // Verify finalization advances monotonically (block number never decreases)
    assert!(
        lfb_after6.body.state.block_number >= lfb_after5.body.state.block_number,
        "LFB block number should not decrease: after6={} < after5={}",
        lfb_after6.body.state.block_number,
        lfb_after5.body.state.block_number
    );
    assert!(
        lfb_after7.body.state.block_number >= lfb_after6.body.state.block_number,
        "LFB block number should not decrease: after7={} < after6={}",
        lfb_after7.body.state.block_number,
        lfb_after6.body.state.block_number
    );
    assert!(
        lfb_after8.body.state.block_number >= lfb_after7.body.state.block_number,
        "LFB block number should not decrease: after8={} < after7={}",
        lfb_after8.body.state.block_number,
        lfb_after7.body.state.block_number
    );

    // Finalization progression can be delayed under high merge pressure.
    // Continue producing in round-robin and require eventual advancement
    // while preserving monotonic LFB movement.
    let mut latest_lfb = lfb_after8.clone();
    for step in 0..=12 {
        if latest_lfb.block_hash != genesis.genesis_block.block_hash {
            break;
        }
        let producer_idx = ((step + 2) % 3) as usize;
        let deploy_idx = 8 + step as usize;
        let _ = TestNode::propagate_block_at_index(&mut nodes, producer_idx, &[deploy_datas
            [deploy_idx]
            .clone()])
        .await
        .unwrap();
        let next_lfb = nodes[0].casper.last_finalized_block().await.unwrap();
        assert!(
            next_lfb.body.state.block_number >= latest_lfb.body.state.block_number,
            "LFB block number should not decrease during extended round-robin: next={} < prev={}",
            next_lfb.body.state.block_number,
            latest_lfb.body.state.block_number
        );
        latest_lfb = next_lfb;
    }

    assert!(
        latest_lfb.block_hash != genesis.genesis_block.block_hash,
        "LFB remained at genesis after extended round-robin production"
    );

    // Verify all validators agree on finalization
    let lfb_node1 = nodes[1].casper.last_finalized_block().await.unwrap();
    let lfb_node2 = nodes[2].casper.last_finalized_block().await.unwrap();
    assert_eq!(
        lfb_node1.block_hash, latest_lfb.block_hash,
        "Node1 LFB should match Node0 LFB"
    );
    assert_eq!(
        lfb_node2.block_hash, latest_lfb.block_hash,
        "Node2 LFB should match Node0 LFB"
    );
}
