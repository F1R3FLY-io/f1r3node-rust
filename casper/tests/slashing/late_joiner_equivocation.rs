// A validator that bonds after genesis and equivocates at its first chance
// must be slashable by every block that is required to slash it.

use std::collections::HashMap;

use casper::rust::block_status::ValidBlock;
use casper::rust::util::construct_deploy::{self, DEFAULT_PUB, DEFAULT_SEC};
use casper::rust::util::proto_util;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, DeployData, ProcessedSystemDeploy, SystemDeployData,
};
use rspace_plus_plus::rspace::history::Either;
use shared::rust::token_amount::TokenAmount;

use super::integration_helpers::equivocate_block;
use crate::helper::bonding_util;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, DEFAULT_VALIDATOR_KEY_PAIRS};

const JOINER: usize = 3;

fn deploy(nonce: i32, shard_id: &str) -> Signed<DeployData> {
    construct_deploy::basic_deploy_data(nonce, None, Some(shard_id.to_string())).expect("deploy")
}

fn seats_joiner(block: &BlockMessage) -> bool {
    block
        .body
        .state
        .bonds
        .iter()
        .any(|bond| bond.validator == DEFAULT_PUB.bytes && !bond.stake.is_zero())
}

fn slashes(block: &BlockMessage, sibling: &BlockMessage) -> bool {
    block.body.system_deploys.iter().any(|deploy| {
        matches!(
            deploy,
            ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::Slash { evidence, .. },
                ..
            } if [&evidence.first, &evidence.second]
                .into_iter()
                .any(|commitment| {
                    proto_util::commitment_hash(commitment).as_ref() == sibling.block_hash.as_ref()
                })
        )
    })
}

#[serial_test::serial]
#[tokio::test]
async fn a_validator_that_bonds_late_and_equivocates_at_once_is_slashed_by_a_valid_block() {
    let key_pairs = vec![
        DEFAULT_VALIDATOR_KEY_PAIRS[0].clone(),
        DEFAULT_VALIDATOR_KEY_PAIRS[1].clone(),
        DEFAULT_VALIDATOR_KEY_PAIRS[2].clone(),
        (DEFAULT_SEC.clone(), DEFAULT_PUB.clone()),
    ];
    let bonds: HashMap<PublicKey, TokenAmount> = key_pairs
        .iter()
        .take(JOINER)
        .map(|(_, pk)| (pk.clone(), TokenAmount::from(100)))
        .collect();
    let mut parameters = GenesisBuilder::build_genesis_parameters(key_pairs, &bonds);
    parameters.2.proof_of_stake.epoch_length = 4;
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, 4, None, None, None)
        .await
        .expect("network");

    let bond = bonding_util::bonding_deploy(100u64, &DEFAULT_SEC, Some(shard_id.clone()))
        .expect("bond deploy");
    TestNode::propagate_block_at_index(&mut nodes, 0, &[bond])
        .await
        .expect("bond block");
    let mut seated = false;
    for round in 0..40 {
        let block = TestNode::propagate_block_at_index(&mut nodes, round % JOINER, &[deploy(
            round as i32,
            &shard_id,
        )])
        .await
        .expect("filler block");
        if seats_joiner(&block) {
            seated = true;
            break;
        }
    }
    assert!(seated, "the joiner never entered the committee");

    let first = nodes[JOINER]
        .create_block_unsafe(&[deploy(100, &shard_id)])
        .await
        .expect("the joiner's block");
    let sibling = equivocate_block(&mut nodes[JOINER], &first, vec![deploy(101, &shard_id)])
        .await
        .expect("the sibling");

    // v1 builds on the first block and v2 on the sibling, so a block citing
    // both of theirs sees the pair and owes the slash.
    let on_first = builds_on(&mut nodes, 1, &first, 102, &shard_id).await;
    let on_sibling = builds_on(&mut nodes, 2, &sibling, 103, &shard_id).await;

    for block in [&first, &sibling, &on_first, &on_sibling] {
        nodes[0]
            .process_block(block.clone())
            .await
            .expect("v0 processes what v1 and v2 saw");
    }
    let proposal = nodes[0]
        .create_block_unsafe(&[deploy(104, &shard_id)])
        .await
        .expect("v0 proposes");
    let own = nodes[0]
        .process_block(proposal.clone())
        .await
        .expect("v0 validates its proposal");
    assert!(
        matches!(own, Either::Right(ValidBlock::Valid)),
        "v0 rejects its own block: {own:?}"
    );
    assert!(
        slashes(&proposal, &sibling),
        "v0's block slashes the joiner"
    );

    for block in [&sibling, &on_sibling] {
        nodes[1]
            .process_block(block.clone())
            .await
            .expect("v1 processes what v2 saw");
    }
    let verdict = nodes[1]
        .process_block(proposal)
        .await
        .expect("v1 processes v0's block");
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "v1 rejects v0's block: {verdict:?}"
    );
}

/// `nodes[node]` accepts the joiner's `block` as the only one at its height
/// and proposes on it.
async fn builds_on(
    nodes: &mut [TestNode],
    node: usize,
    block: &BlockMessage,
    nonce: i32,
    shard_id: &str,
) -> BlockMessage {
    let verdict = nodes[node]
        .process_block(block.clone())
        .await
        .expect("process the joiner's block");
    assert!(
        matches!(verdict, Either::Right(_)),
        "the joiner is a weighted sender: {verdict:?}"
    );
    nodes[node]
        .add_block_from_deploys(&[deploy(nonce, shard_id)])
        .await
        .expect("propose on the joiner's block")
}
