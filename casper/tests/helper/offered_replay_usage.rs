//! DR-102: the replay budget of an offered deploy charges the same usage on its
//! producer and on every validator. Each role charges its own acceptance work
//! to a separate budget.

use casper::rust::api::block_api::BlockAPI;
use casper::rust::util::rholang::costacc::offered_acceptance::OfferedUsageKind;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::host_work::{HostWorkDimension, HostWorkUsages};
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;

fn usages(
    node: &TestNode,
    kind: OfferedUsageKind,
    deploy_id: &[u8],
    pre_state_root: &[u8],
) -> Vec<HostWorkUsages> {
    node.runtime_manager
        .offered_budget_usages()
        .into_iter()
        .filter(|record| {
            record.kind == kind
                && record.deploy_id == deploy_id
                && record.pre_state_root == pre_state_root
        })
        .map(|record| record.usages)
        .collect()
}

fn publication_comparison_bound() -> u64 {
    let protocol = offered_funded_v6_limits();
    [
        protocol.evidence.total_bytes,
        protocol.deploy_log_bytes,
        protocol.envelope.payload.deploy_bytes,
        protocol.envelope.payload.signing.total_bytes,
        protocol.envelope.payload.funding.wire.total_bytes,
    ]
    .into_iter()
    .map(|bytes| u64::try_from(bytes).expect("a protocol bound fits u64"))
    .sum()
}

/// Proposes `offer` on node 0 and decodes the block from its wire form, as a
/// peer receives it. Every other node processes the block first, then node 0.
/// The function checks that:
/// - every node stores the producer's mergeable entry for the block;
/// - the producer's self-replay and every validator replay charge the same
///   usage in every host-work dimension;
/// - the producer's acceptance work is the publication comparison alone;
/// - every validator's acceptance work equals the work that the producer
///   charged to its execution budget for the same mergeable entry.
pub async fn propose_offer_with_identical_replay_usage(
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

    let producer_replay = usages(
        &nodes[0],
        OfferedUsageKind::ProducerSelfReplay,
        &deploy_id,
        &pre_state_root,
    );
    assert_eq!(
        producer_replay.len(),
        1,
        "{stage}: one producer self-replay"
    );
    let producer_replay = producer_replay[0];
    for dimension in [
        HostWorkDimension::VerificationBytes,
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::SearchStateBytes,
    ] {
        assert!(
            producer_replay.get(dimension).get() > 0,
            "{stage}: the self-replay charges {dimension:?}"
        );
    }
    for (index, node) in nodes.iter().enumerate() {
        let replays = usages(
            node,
            OfferedUsageKind::ValidatorReplay,
            &deploy_id,
            &pre_state_root,
        );
        assert!(
            !replays.is_empty(),
            "{stage}: node {index} replays the block"
        );
        for replay in replays {
            assert_eq!(
                replay, producer_replay,
                "{stage}: node {index} replay usage equals the producer self-replay"
            );
        }
    }

    let bound = publication_comparison_bound();
    let producer_acceptance = usages(
        &nodes[0],
        OfferedUsageKind::ProducerAcceptance,
        &deploy_id,
        &pre_state_root,
    );
    assert_eq!(
        producer_acceptance.len(),
        1,
        "{stage}: one producer acceptance"
    );
    for dimension in HostWorkDimension::ALL {
        let expected = match dimension {
            HostWorkDimension::VerificationBytes | HostWorkDimension::VerificationOperations => {
                bound
            }
            _ => 0,
        };
        assert_eq!(
            producer_acceptance[0].get(dimension).get(),
            expected,
            "{stage}: producer acceptance {dimension:?}"
        );
    }

    let producer_mergeable = usages(
        &nodes[0],
        OfferedUsageKind::ProducerMergeableExecution,
        &deploy_id,
        &pre_state_root,
    );
    assert_eq!(
        producer_mergeable.len(),
        1,
        "{stage}: one producer mergeable step"
    );
    assert!(
        producer_mergeable[0]
            .get(HostWorkDimension::SearchStateBytes)
            .get()
            > 0,
        "{stage}: the producer charges its mergeable step"
    );
    for (index, node) in nodes.iter().enumerate() {
        for acceptance in usages(
            node,
            OfferedUsageKind::ValidatorAcceptance,
            &deploy_id,
            &pre_state_root,
        ) {
            assert_eq!(
                acceptance, producer_mergeable[0],
                "{stage}: node {index} acceptance equals the producer's mergeable step"
            );
        }
    }
    wire
}
