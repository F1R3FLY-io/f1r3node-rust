//! DR-102: an offered block replays on its producer and on every validator.
//! Changed by DR-131: the replays no longer charge host work, so the helper
//! checks the replay records and the stored mergeable entries.

use casper::rust::api::block_api::BlockAPI;
use casper::rust::util::rholang::costacc::offered_acceptance::OfferedUsageKind;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;

fn records(
    node: &TestNode,
    kind: OfferedUsageKind,
    deploy_id: &[u8],
    pre_state_root: &[u8],
) -> usize {
    node.runtime_manager
        .offered_budget_usages()
        .into_iter()
        .filter(|record| {
            record.kind == kind
                && record.deploy_id == deploy_id
                && record.pre_state_root == pre_state_root
        })
        .count()
}

/// Proposes `offer` on node 0 and decodes the block from its wire form, as a
/// peer receives it. Every other node processes the block first, then node 0.
/// The function checks that:
/// - every node accepts the block and stores the producer's mergeable entry;
/// - the producer records one self-replay of the block;
/// - every node replays the block as a validator.
pub async fn propose_offer_replayed_by_every_role(
    nodes: &mut [TestNode],
    offer: models::casper::DeployDataProto,
    stage: &str,
) -> BlockMessage {
    let deploy_id = offer.deploy_id.to_vec();
    BlockAPI::deploy_offered(&nodes[0].engine_cell, offer, &None, false, "root")
        .await
        .unwrap_or_else(|error| panic!("{stage} admission: {error}"));
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .unwrap_or_else(|error| panic!("{stage} proposal: {error}"));
    let pre_state_root = block.body.state.pre_state_hash.to_vec();
    let (_, producer_entry) = nodes[0]
        .runtime_manager
        .get_mergeable_entry_bytes(&block)
        .expect("the producer's mergeable store is readable");
    let producer_entry = producer_entry.expect("the producer stores the mergeable entry");
    let wire = BlockMessage::from_proto(block.to_proto()).expect("an offered block decodes");
    for (index, node) in nodes.iter_mut().enumerate().skip(1) {
        assert!(
            matches!(
                node.process_block(wire.clone()).await.unwrap(),
                Either::Right(_)
            ),
            "{stage}: node {index} accepts the block"
        );
        let (_, entry) = node
            .runtime_manager
            .get_mergeable_entry_bytes(&wire)
            .expect("the validator's mergeable store is readable");
        assert_eq!(
            entry.as_ref(),
            Some(&producer_entry),
            "{stage}: node {index} stores the producer's mergeable entry"
        );
    }
    assert!(
        matches!(
            nodes[0].process_block(wire.clone()).await.unwrap(),
            Either::Right(_)
        ),
        "{stage}: the producer accepts its own block"
    );

    assert_eq!(
        records(
            &nodes[0],
            OfferedUsageKind::ProducerSelfReplay,
            &deploy_id,
            &pre_state_root,
        ),
        1,
        "{stage}: one producer self-replay"
    );
    for (index, node) in nodes.iter().enumerate() {
        assert!(
            records(
                node,
                OfferedUsageKind::ValidatorReplay,
                &deploy_id,
                &pre_state_root,
            ) > 0,
            "{stage}: node {index} replays the block"
        );
    }
    wire
}
