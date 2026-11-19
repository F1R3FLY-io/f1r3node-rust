use casper::rust::block_status::ValidBlock;
use casper::rust::blocks::proposer::propose_result::{
    CheckProposeConstraintsFailure, ProposeFailure, ProposeStatus,
};
use casper::rust::blocks::proposer::proposer::{new_proposer, ProductionProposer};
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

async fn admit(node: &mut TestNode, block: &BlockMessage) {
    let verdict = node.add_block(block.clone()).await.expect("process");
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "block #{} refused: {verdict:?}",
        block.body.state.block_number
    );
}

async fn propose_everywhere(nodes: &mut [TestNode], proposer: usize) -> BlockMessage {
    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let block = nodes[proposer]
        .create_block_unsafe(&[])
        .await
        .expect("block creation");
    for node in nodes.iter_mut() {
        admit(node, &block).await;
    }
    block
}

fn proposer_for<T: TransportLayer + Send + Sync + 'static>(
    node: &TestNode,
    transport: std::sync::Arc<T>,
) -> ProductionProposer<T> {
    new_proposer(
        node.validator_id_opt.clone().expect("validator"),
        None,
        node.runtime_manager.clone(),
        node.block_store.clone(),
        node.deploy_storage.clone(),
        node.rejected_deploy_buffer.clone(),
        transport,
        node.connections_cell.clone(),
        node.rp_conf.clone(),
        F1r3flyEvents::new(),
        true,
    )
}

fn is_held(status: &ProposeStatus) -> bool {
    matches!(
        status,
        ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
            CheckProposeConstraintsFailure::HistoryIncomplete
        ))
    )
}

#[tokio::test]
async fn a_restored_validator_does_not_reuse_the_sequence_number_of_its_lost_block() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis.clone(), 4, Some(0.0), None, None, None)
        .await
        .expect("network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let kept = propose_everywhere(&mut nodes, 0).await;
    let mut before_restore = Vec::new();
    for peer in 1..4 {
        before_restore.push(propose_everywhere(&mut nodes, peer).await);
    }
    let lost = propose_everywhere(&mut nodes, 0).await;

    let mut restored = TestNode::create_network(genesis, 1, Some(0.0), None, None, None)
        .await
        .expect("restored node")
        .remove(0);
    admit(&mut restored, &kept).await;
    for block in &before_restore {
        admit(&mut restored, block).await;
    }
    assert!(restored.casper.hold_proposals_until_own_history_known());

    let mut proposer = proposer_for(&restored, restored.tle.clone());

    let early = proposer
        .propose(restored.casper.clone(), true)
        .await
        .expect("propose");
    assert!(
        early.block_message_opt.is_none() && is_held(&early.propose_result.propose_status),
        "a restored validator proposed before it knew its own latest block: {:?}",
        early.propose_result.propose_status
    );

    let first_peer_block = propose_everywhere(&mut nodes, 1).await;
    admit(&mut restored, &lost).await;
    admit(&mut restored, &first_peer_block).await;
    let partial = proposer
        .propose(restored.casper.clone(), true)
        .await
        .expect("propose");
    assert!(
        partial.block_message_opt.is_none() && is_held(&partial.propose_result.propose_status),
        "the gate opened before every other active validator had a block after the restore: {:?}",
        partial.propose_result.propose_status
    );

    for peer in 2..4 {
        let block = propose_everywhere(&mut nodes, peer).await;
        admit(&mut restored, &block).await;
    }
    let resumed = proposer
        .propose(restored.casper.clone(), true)
        .await
        .expect("propose");
    let block = resumed.block_message_opt.unwrap_or_else(|| {
        panic!(
            "the restored validator did not resume proposing: {:?}",
            resumed.propose_result.propose_status
        )
    });
    assert_eq!(block.seq_num, lost.seq_num + 1);
    assert!(!restored.casper.own_history_gate.is_closed());
    admit(&mut nodes[1], &block).await;
}
