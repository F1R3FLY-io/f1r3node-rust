//! Shared DAG-geometry staging for protocol-valid batch2 scenarios.

use std::collections::HashSet;

use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::Casper;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;

#[derive(Clone, Debug)]
pub enum ExpectedParents {
    ExactOrder(Vec<BlockHash>),
    ExactMembers(Vec<BlockHash>),
}

impl ExpectedParents {
    pub fn ordered(parents: &[&BlockMessage]) -> Self {
        Self::ExactOrder(
            parents
                .iter()
                .map(|parent| parent.block_hash.clone())
                .collect(),
        )
    }

    pub fn members(parents: &[&BlockMessage]) -> Self {
        Self::ExactMembers(
            parents
                .iter()
                .map(|parent| parent.block_hash.clone())
                .collect(),
        )
    }
}

pub async fn mint_on_expected_snapshot(
    node: &mut TestNode,
    expected: ExpectedParents,
    label: &str,
) -> BlockMessage {
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let actual = snapshot
        .parents
        .iter()
        .map(|parent| parent.block_hash.clone())
        .collect::<Vec<_>>();
    match expected {
        ExpectedParents::ExactOrder(expected) => {
            assert_eq!(
                actual, expected,
                "staging[{label}]: unexpected parent order"
            );
        }
        ExpectedParents::ExactMembers(expected) => {
            let actual_set = actual.iter().cloned().collect::<HashSet<_>>();
            let expected_set = expected.iter().cloned().collect::<HashSet<_>>();
            assert_eq!(
                actual_set.len(),
                actual.len(),
                "staging[{label}]: snapshot parents contain duplicates"
            );
            assert_eq!(
                expected_set.len(),
                expected.len(),
                "staging[{label}]: expected parents contain duplicates"
            );
            assert_eq!(
                actual_set, expected_set,
                "staging[{label}]: unexpected parent members"
            );
        }
    }
    let validator_identity = node.validator_id_opt.clone().expect("validator identity");
    let deploy_storage = node.deploy_storage.clone();
    let rejected_buffer = node.rejected_deploy_buffer.clone();
    let runtime_manager = node.runtime_manager.clone();
    let created = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        deploy_storage,
        rejected_buffer,
        &runtime_manager,
        &mut node.block_store,
        node.allow_empty_blocks,
    )
    .await
    .unwrap_or_else(|e| panic!("create[{label}] must succeed: {:?}", e));
    let BlockCreatorResult::Created(block, _pre, _post) = created else {
        panic!("create[{label}] must mint on the frozen snapshot");
    };
    assert_eq!(
        block.header.parents_hash_list, actual,
        "staging[{label}]: block changed the frozen snapshot parent order"
    );
    let result = node
        .process_block(block.clone())
        .await
        .expect("self-process minted block");
    assert!(
        matches!(result, Either::Right(_)),
        "staging[{label}]: minted block must validate, got {result:?}"
    );
    block
}

pub async fn mint_on_parents(
    node: &mut TestNode,
    parents: Vec<BlockMessage>,
    label: &str,
) -> BlockMessage {
    let expected = ExpectedParents::ExactOrder(
        parents
            .into_iter()
            .map(|parent| parent.block_hash)
            .collect(),
    );
    mint_on_expected_snapshot(node, expected, label).await
}
