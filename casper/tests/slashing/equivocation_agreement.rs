// Honest validators reach the same verdict on an equivocation and on the
// blocks that respond to it, whatever order they received the two signed
// blocks in and whichever of them they hold.

use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::util::construct_deploy;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, DeployData, EquivocationEvidence, ProcessedSystemDeploy, SystemDeployData,
};
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::equivocate_block;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn deploy(nonce: i32, shard_id: &str) -> Signed<DeployData> {
    construct_deploy::basic_deploy_data(nonce, None, Some(shard_id.to_string())).expect("deploy")
}

async fn network(validators: usize, epoch_length: Option<i32>) -> (Vec<TestNode>, String) {
    let mut parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(validators));
    if let Some(epoch_length) = epoch_length {
        parameters.2.proof_of_stake.epoch_length = epoch_length;
    }
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let nodes = TestNode::create_network(genesis, validators, None, None, None)
        .await
        .expect("network");
    (nodes, shard_id)
}

fn carries_slash(block: &BlockMessage) -> bool { !slash_evidence(block).is_empty() }

fn slash_evidence(block: &BlockMessage) -> Vec<EquivocationEvidence> {
    block
        .body
        .system_deploys
        .iter()
        .filter_map(|deploy| match deploy {
            ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::Slash { evidence, .. },
                ..
            } => Some((**evidence).clone()),
            _ => None,
        })
        .collect()
}

fn cites(block: &BlockMessage, validator: &BlockHash) -> Option<BlockHash> {
    block
        .justifications
        .iter()
        .find(|justification| justification.validator == *validator)
        .map(|justification| justification.latest_block_hash.clone())
}

fn is_equivocation(verdict: &Either<BlockError, ValidBlock>) -> bool {
    matches!(
        verdict,
        Either::Left(BlockError::Invalid(
            InvalidBlock::IgnorableEquivocation | InvalidBlock::AdmissibleEquivocation
        ))
    )
}

/// What `node` records for `hash`: `None` if it does not hold the block,
/// otherwise the sibling it holds the block as an equivocation with.
fn held_pair(node: &TestNode, hash: &BlockHash) -> Option<Option<BlockHash>> {
    node.casper
        .block_dag_storage
        .get_representation()
        .expect("dag")
        .lookup(hash)
        .expect("lookup")
        .map(|meta| meta.equivocates)
}

async fn accept(node: &mut TestNode, block: &BlockMessage, what: &str) {
    let verdict = node.process_block(block.clone()).await.expect(what);
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "{what}: {verdict:?}"
    );
}

#[serial_test::serial]
#[tokio::test]
async fn validators_that_saw_an_equivocation_in_opposite_orders_accept_each_others_slash() {
    let (mut nodes, shard_id) = network(3, None).await;

    let b1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("b1");
    let sibling = equivocate_block(&mut nodes[0], &b1, vec![deploy(1, &shard_id)])
        .await
        .expect("sibling");

    for (node, first, second) in [(1, &b1, &sibling), (2, &sibling, &b1)] {
        accept(
            &mut nodes[node],
            first,
            &format!("v{node} accepts the first copy"),
        )
        .await;
        let recorded = nodes[node]
            .process_block(second.clone())
            .await
            .expect("second copy");
        assert!(
            is_equivocation(&recorded),
            "v{node} second copy: {recorded:?}"
        );
        assert_eq!(
            held_pair(&nodes[node], &second.block_hash),
            Some(Some(first.block_hash.clone())),
            "v{node} records the pair"
        );
    }

    let from_v1 = nodes[1]
        .create_block_unsafe(&[deploy(10, &shard_id)])
        .await
        .expect("v1");
    let from_v2 = nodes[2]
        .create_block_unsafe(&[deploy(11, &shard_id)])
        .await
        .expect("v2");
    assert!(
        carries_slash(&from_v1) && carries_slash(&from_v2),
        "both slash v0"
    );
    assert_eq!(
        slash_evidence(&from_v1),
        slash_evidence(&from_v2),
        "both issue the same evidence, whichever block each flagged"
    );

    let v2_judges_v1 = nodes[2]
        .process_block(from_v1.clone())
        .await
        .expect("v2 judges");
    let v1_judges_v2 = nodes[1]
        .process_block(from_v2.clone())
        .await
        .expect("v1 judges");
    assert!(
        matches!(v2_judges_v1, Either::Right(ValidBlock::Valid))
            && matches!(v1_judges_v2, Either::Right(ValidBlock::Valid)),
        "v2 judges v1's slash: {v2_judges_v1:?}; v1 judges v2's slash: {v1_judges_v2:?}"
    );
}

enum Arrival {
    Ignorable,
    Admissible,
}

/// v0 signs `chain` blocks and a sibling of the first; v1 receives the chain,
/// then the sibling. The sibling is an equivocation held invalid, and v1's
/// next block slashes v0, however far v0's chain had moved on.
async fn sibling_after_chain(chain: usize, arrival: Arrival) {
    let (mut nodes, shard_id) = network(4, None).await;

    let b1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("b1");
    let sibling = equivocate_block(&mut nodes[0], &b1, vec![deploy(100, &shard_id)])
        .await
        .expect("sibling");
    nodes[0]
        .process_block(b1.clone())
        .await
        .expect("v0 adopts b1");
    let mut blocks = vec![b1];
    for nonce in 1..chain as i32 {
        blocks.push(
            nodes[0]
                .add_block_from_deploys(&[deploy(nonce, &shard_id)])
                .await
                .expect("v0 extends"),
        );
    }

    for block in &blocks {
        accept(&mut nodes[1], block, "v1 accepts v0's chain").await;
    }
    let child = match arrival {
        Arrival::Ignorable => None,
        Arrival::Admissible => {
            accept(&mut nodes[3], &sibling, "v3 accepts the sibling").await;
            let child = nodes[3]
                .add_block_from_deploys(&[deploy(101, &shard_id)])
                .await
                .expect("v3 builds on the sibling");
            nodes[1]
                .process_block(child.clone())
                .await
                .expect("v1 holds the sibling's child");
            assert!(
                nodes[1].ingest_view.is_cited(&sibling.block_hash),
                "a block v1 holds cites the sibling"
            );
            Some(child)
        }
    };
    let verdict = nodes[1]
        .process_block(sibling.clone())
        .await
        .expect("sibling");
    let expected = match arrival {
        Arrival::Ignorable => InvalidBlock::IgnorableEquivocation,
        Arrival::Admissible => InvalidBlock::AdmissibleEquivocation,
    };
    assert!(
        matches!(&verdict, Either::Left(BlockError::Invalid(status)) if *status == expected),
        "chain {chain}: verdict {verdict:?}"
    );
    assert_eq!(
        held_pair(&nodes[1], &sibling.block_hash),
        Some(Some(blocks[0].block_hash.clone())),
        "chain {chain}: the sibling is held as an equivocation with b1"
    );

    let next = nodes[1]
        .create_block_unsafe(&[deploy(10, &shard_id)])
        .await
        .expect("v1");
    assert!(
        carries_slash(&next),
        "chain {chain}: v1's next block slashes v0"
    );

    for block in &blocks {
        accept(&mut nodes[2], block, "v2 accepts v0's chain").await;
    }
    let v2_sibling = nodes[2]
        .process_block(sibling.clone())
        .await
        .expect("v2 sibling");
    assert!(
        is_equivocation(&v2_sibling),
        "chain {chain}: v2 sibling {v2_sibling:?}"
    );
    if let Some(child) = child {
        nodes[2]
            .process_block(child)
            .await
            .expect("v2 receives the sibling's child");
    }
    accept(
        &mut nodes[2],
        &next,
        &format!("chain {chain}: v2 judges v1's slash"),
    )
    .await;
}

#[serial_test::serial]
#[tokio::test]
async fn a_sibling_of_the_latest_block_is_a_slashed_equivocation() {
    sibling_after_chain(1, Arrival::Ignorable).await;
    sibling_after_chain(1, Arrival::Admissible).await;
}

#[serial_test::serial]
#[tokio::test]
async fn a_sibling_below_a_longer_chain_is_a_slashed_equivocation() {
    sibling_after_chain(4, Arrival::Ignorable).await;
    sibling_after_chain(4, Arrival::Admissible).await;
}

/// v0 signs a1 ← a2 ← a3, a sibling w of a2 that extends a1, and z at a3's
/// sequence number whose own chain skips a2. v2 holds a1..a3 and receives w;
/// v1 holds a1..a3 and stores z as an equivocation before w arrives. Both
/// hold w as an equivocation with a2.
#[serial_test::serial]
#[tokio::test]
async fn a_stored_equivocation_does_not_hide_a_later_sibling() {
    let (mut nodes, shard_id) = network(3, None).await;

    let a1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("a1");
    let x = equivocate_block(&mut nodes[0], &a1, vec![deploy(100, &shard_id)])
        .await
        .expect("x");
    nodes[0]
        .process_block(a1.clone())
        .await
        .expect("v0 adopts a1");
    let a2 = nodes[0]
        .create_block_unsafe(&[deploy(1, &shard_id)])
        .await
        .expect("a2");
    let w = equivocate_block(&mut nodes[0], &a2, vec![deploy(101, &shard_id)])
        .await
        .expect("w");
    nodes[0]
        .process_block(a2.clone())
        .await
        .expect("v0 adopts a2");
    let a3 = nodes[0]
        .add_block_from_deploys(&[deploy(2, &shard_id)])
        .await
        .expect("a3");
    let mut z = x.clone();
    z.seq_num = a3.seq_num;
    let z = nodes[0]
        .validator_id_opt
        .as_ref()
        .expect("v0 identity")
        .sign_block(&z);

    for node in [1, 2] {
        for block in [&a1, &a2, &a3] {
            accept(
                &mut nodes[node],
                block,
                &format!("v{node} accepts v0's chain"),
            )
            .await;
        }
    }
    let z_verdict = nodes[1].process_block(z.clone()).await.expect("v1 z");
    assert!(
        is_equivocation(&z_verdict),
        "v1 holds z as an equivocation: {z_verdict:?}"
    );

    for node in [2, 1] {
        let verdict = nodes[node].process_block(w.clone()).await.expect("w");
        assert!(is_equivocation(&verdict), "v{node} judges w: {verdict:?}");
        assert_eq!(
            held_pair(&nodes[node], &w.block_hash),
            Some(Some(a2.block_hash.clone())),
            "v{node} holds w as an equivocation with a2"
        );
    }
}

/// v0 signs a1, and the same block for another chain. v1 holds a1 and refuses
/// the other chain's block without holding it as an equivocation.
#[serial_test::serial]
#[tokio::test]
async fn a_block_of_another_chain_is_refused_not_taken_for_an_equivocation() {
    let (mut nodes, shard_id) = network(3, None).await;

    let a1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("a1");
    let mut foreign = a1.clone();
    foreign.header.chain_id = prost::bytes::Bytes::from(vec![0xC2; 32]);
    let foreign = nodes[0]
        .validator_id_opt
        .as_ref()
        .expect("v0 identity")
        .sign_block(&foreign);

    accept(&mut nodes[1], &a1, "v1 accepts a1").await;
    let verdict = nodes[1]
        .process_block(foreign.clone())
        .await
        .expect("foreign");
    assert!(
        matches!(
            verdict,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidChainId))
        ),
        "{verdict:?}"
    );
    assert_eq!(held_pair(&nodes[1], &foreign.block_hash), None);
}

/// v1 receives a1 then x and holds x as the equivocation. v2 receives only x,
/// so holds no pair, and proposes r citing x without a slash. v1 accepts r.
#[serial_test::serial]
#[tokio::test]
async fn a_validator_that_saw_one_sibling_may_cite_it_without_slashing() {
    let (mut nodes, shard_id) = network(3, None).await;

    let a1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("a1");
    let x = equivocate_block(&mut nodes[0], &a1, vec![deploy(100, &shard_id)])
        .await
        .expect("x");

    accept(&mut nodes[1], &a1, "v1 accepts a1").await;
    let flagged = nodes[1].process_block(x.clone()).await.expect("v1 x");
    assert!(
        is_equivocation(&flagged),
        "v1 holds x as the equivocation: {flagged:?}"
    );

    accept(&mut nodes[2], &x, "v2 accepts x").await;
    let r = nodes[2]
        .add_block_from_deploys(&[deploy(10, &shard_id)])
        .await
        .expect("r");
    assert_eq!(
        cites(&r, &x.sender),
        Some(x.block_hash.clone()),
        "r cites x"
    );
    assert!(!carries_slash(&r), "v2 holds no pair, so r slashes nothing");
    assert!(
        r.header.parents_hash_list.contains(&x.block_hash),
        "r has x as a parent: {:?}",
        r.header.parents_hash_list
    );

    accept(&mut nodes[1], &r, "v1 judges r").await;
}

/// v2 receives x then a1 and holds a1 as the equivocation. v0's next block a2
/// extends a1 and has no sibling: it is not an equivocation, and v2 accepts it.
#[serial_test::serial]
#[tokio::test]
async fn a_block_extending_a_recorded_equivocation_without_a_sibling_is_valid() {
    let (mut nodes, shard_id) = network(3, None).await;

    let a1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("a1");
    let x = equivocate_block(&mut nodes[0], &a1, vec![deploy(100, &shard_id)])
        .await
        .expect("x");
    nodes[0]
        .process_block(a1.clone())
        .await
        .expect("v0 adopts a1");
    let a2 = nodes[0]
        .add_block_from_deploys(&[deploy(1, &shard_id)])
        .await
        .expect("a2");

    accept(&mut nodes[2], &x, "v2 accepts x").await;
    let flagged = nodes[2].process_block(a1.clone()).await.expect("v2 a1");
    assert!(
        is_equivocation(&flagged),
        "v2 holds a1 as the equivocation: {flagged:?}"
    );
    accept(&mut nodes[2], &a2, "v2 judges a2").await;
}

/// Epoch length 2. v0 signs a1 at block 1 (epoch 0) and its sibling x at
/// block 2 (epoch 1). v2 receives a1 then x; v1 receives x then a1. Each
/// holds the other sibling as the equivocation, each slashes v0 in epoch 1,
/// and each accepts the other's slash.
#[serial_test::serial]
#[tokio::test]
async fn siblings_in_different_epochs_are_slashed_alike_whichever_was_flagged() {
    let (mut nodes, shard_id) = network(3, Some(2)).await;

    let a1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("a1");
    let c1 = nodes[1]
        .add_block_from_deploys(&[deploy(1, &shard_id)])
        .await
        .expect("c1");
    nodes[0]
        .process_block(c1.clone())
        .await
        .expect("v0 holds c1");
    let x = nodes[0]
        .create_block_unsafe(&[deploy(2, &shard_id)])
        .await
        .expect("x");
    assert_eq!(x.seq_num, a1.seq_num, "x is a1's sibling");
    assert_eq!(
        (a1.body.state.block_number, x.body.state.block_number),
        (1, 2),
        "the siblings straddle the epoch boundary"
    );

    accept(&mut nodes[2], &c1, "v2 accepts c1").await;
    accept(&mut nodes[2], &a1, "v2 accepts a1").await;
    let v2_flags = nodes[2].process_block(x.clone()).await.expect("v2 x");
    assert!(
        is_equivocation(&v2_flags),
        "v2 holds x as the equivocation: {v2_flags:?}"
    );

    accept(&mut nodes[1], &x, "v1 accepts x").await;
    let v1_flags = nodes[1].process_block(a1.clone()).await.expect("v1 a1");
    assert!(
        is_equivocation(&v1_flags),
        "v1 holds a1 as the equivocation: {v1_flags:?}"
    );

    let from_v1 = nodes[1]
        .create_block_unsafe(&[deploy(10, &shard_id)])
        .await
        .expect("v1");
    let from_v2 = nodes[2]
        .create_block_unsafe(&[deploy(11, &shard_id)])
        .await
        .expect("v2");
    assert!(
        carries_slash(&from_v1) && carries_slash(&from_v2),
        "both slash v0: v1 {}, v2 {}",
        carries_slash(&from_v1),
        carries_slash(&from_v2)
    );

    let v2_judges_v1 = nodes[2]
        .process_block(from_v1.clone())
        .await
        .expect("v2 judges");
    let v1_judges_v2 = nodes[1]
        .process_block(from_v2.clone())
        .await
        .expect("v1 judges");
    assert!(
        matches!(v2_judges_v1, Either::Right(ValidBlock::Valid))
            && matches!(v1_judges_v2, Either::Right(ValidBlock::Valid)),
        "v2 judges v1's slash: {v2_judges_v1:?}; v1 judges v2's slash: {v1_judges_v2:?}"
    );
}
