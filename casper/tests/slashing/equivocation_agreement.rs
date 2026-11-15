// Honest validators reach the same verdict on an equivocation and on the
// blocks that respond to it, whatever order they received the two signed
// blocks in and whichever of them they hold.

use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::util::{construct_deploy, proto_util};
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, DeployData, EquivocationEvidence, ProcessedSystemDeploy, SystemDeployData,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::equivocate_block;
use crate::helper::test_node::{ShardOverrides, TestNode};
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
    let nodes = TestNode::create_network_with_overrides(genesis, validators, ShardOverrides {
        own_rspace: true,
        ..Default::default()
    })
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

/// One node validates the second block of a pair; another receives it as
/// settled history below its restore anchor. A block whose justifications
/// show both blocks while the sender is bonded owes the slash on both.
#[tokio::test]
async fn nodes_holding_a_pair_by_different_paths_judge_its_neglect_alike() {
    use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;

    use super::detector_totality_helpers::{
        assert_neglected, block, justification, DetectorFixture,
    };

    let mut verdicts = Vec::new();
    for second_by_validation in [true, false] {
        let node = DetectorFixture::new().await;
        let bonded = node.validators.clone();
        let offender = bonded[1].clone();
        let first = block(20, offender.clone(), 1, vec![], bonded.clone());
        let second = block(30, offender.clone(), 1, vec![], bonded.clone());
        let observed = block(
            40,
            bonded[2].clone(),
            1,
            vec![justification(offender.clone(), second.block_hash.clone())],
            bonded.clone(),
        );
        node.add_block(&first);
        node.block_store
            .put_block_message(&second)
            .expect("store second");
        let mode = if second_by_validation {
            InsertMode::Equivocation {
                sibling: first.block_hash.clone(),
            }
        } else {
            InsertMode::SettledHistory
        };
        node.dag_storage
            .insert(&second, mode)
            .expect("insert second");
        node.add_block(&observed);

        let judged = block(
            50,
            bonded[3].clone(),
            1,
            vec![
                justification(offender.clone(), first.block_hash.clone()),
                justification(bonded[2].clone(), observed.block_hash.clone()),
            ],
            bonded,
        );
        verdicts.push(node.check(&judged).await);
    }
    for verdict in verdicts {
        assert_neglected(verdict);
    }
}

fn holds_post_state(node: &TestNode, block: &BlockMessage) -> bool {
    node.runtime_manager
        .has_root(&Blake2b256Hash::from_bytes_prost(
            &block.body.state.post_state_hash,
        ))
        .expect("has_root")
}

/// A sibling no block cites is recorded without a replay, and stays so when
/// it is its sender's latest message in the node's next snapshot, so an
/// equivocator cannot make a node replay every block it signs.
#[serial_test::serial]
#[tokio::test]
async fn an_uncited_sibling_is_not_replayed() {
    use casper::rust::casper::Casper;

    let (mut nodes, shard_id) = network(2, None).await;
    let b1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("b1");
    let sibling = equivocate_block(&mut nodes[0], &b1, vec![deploy(1, &shard_id)])
        .await
        .expect("sibling");
    accept(&mut nodes[1], &b1, "v1 accepts b1").await;

    let verdict = nodes[1]
        .process_block(sibling.clone())
        .await
        .expect("sibling");
    assert!(
        matches!(
            verdict,
            Either::Left(BlockError::Invalid(InvalidBlock::IgnorableEquivocation))
        ),
        "{verdict:?}"
    );
    assert!(!holds_post_state(&nodes[1], &sibling));

    nodes[1].casper.get_snapshot().await.expect("snapshot");
    assert!(
        !holds_post_state(&nodes[1], &sibling),
        "a snapshot does not replay the sibling"
    );
}

/// v1 holds the sibling as an equivocation without its post-state; v2
/// received the sibling first and builds on it. v1 replays the sibling when
/// v2's block needs its state, and v2's block is valid.
#[serial_test::serial]
#[tokio::test]
async fn a_block_built_on_an_equivocation_replays_it_on_demand() {
    let (mut nodes, shard_id) = network(3, None).await;
    let b1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("b1");
    let sibling = equivocate_block(&mut nodes[0], &b1, vec![deploy(1, &shard_id)])
        .await
        .expect("sibling");
    accept(&mut nodes[1], &b1, "v1 accepts b1").await;
    let recorded = nodes[1]
        .process_block(sibling.clone())
        .await
        .expect("v1 sibling");
    assert!(is_equivocation(&recorded), "{recorded:?}");
    assert!(!holds_post_state(&nodes[1], &sibling));

    accept(&mut nodes[2], &sibling, "v2 accepts the sibling").await;
    let built_on_sibling = nodes[2]
        .create_block_unsafe(&[deploy(2, &shard_id)])
        .await
        .expect("v2 block");
    assert!(built_on_sibling
        .header
        .parents_hash_list
        .contains(&sibling.block_hash));

    accept(&mut nodes[1], &built_on_sibling, "v1 judges v2's block").await;
    assert!(holds_post_state(&nodes[1], &sibling));
}

fn remembered_verdict(node: &TestNode, twin: &BlockHash) -> Option<Result<(), InvalidBlock>> {
    node.casper
        .judged_twins
        .get(twin)
        .map(|verdict| verdict.as_ref().clone())
}

/// v2 and v3 each build on the sibling v1 holds as an equivocation. v1 judges
/// the sibling for the first block and reuses that verdict for the second.
#[serial_test::serial]
#[tokio::test]
async fn blocks_built_on_one_twin_reuse_its_verdict() {
    let (mut nodes, shard_id) = network(4, None).await;
    let b1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("b1");
    let sibling = equivocate_block(&mut nodes[0], &b1, vec![deploy(1, &shard_id)])
        .await
        .expect("sibling");
    accept(&mut nodes[1], &b1, "v1 accepts b1").await;
    let recorded = nodes[1]
        .process_block(sibling.clone())
        .await
        .expect("v1 sibling");
    assert!(is_equivocation(&recorded), "{recorded:?}");

    accept(&mut nodes[2], &sibling, "v2 accepts the sibling").await;
    accept(&mut nodes[3], &sibling, "v3 accepts the sibling").await;
    let mut children = Vec::new();
    for (validator, nonce) in [(2, 2), (3, 3)] {
        let child = nodes[validator]
            .create_block_unsafe(&[deploy(nonce, &shard_id)])
            .await
            .expect("child");
        assert!(child.header.parents_hash_list.contains(&sibling.block_hash));
        children.push(child);
    }
    assert_eq!(remembered_verdict(&nodes[1], &sibling.block_hash), None);

    accept(&mut nodes[1], &children[0], "v1 judges v2's block").await;
    assert_eq!(
        remembered_verdict(&nodes[1], &sibling.block_hash),
        Some(Ok(()))
    );
    accept(&mut nodes[1], &children[1], "v1 judges v3's block").await;
}

/// v0 signs b1 and a twin t that carries other deploys but declares b1's
/// post-state; v2 builds c on t. v3 receives t first and refuses it on its
/// contents. v1 holds b1 first and t as the equivocation, so it judges t's
/// contents when c needs them, and refuses c.
#[serial_test::serial]
#[tokio::test]
async fn a_block_built_on_an_invalid_twin_is_held_by_no_node() {
    let (mut nodes, shard_id) = network(4, None).await;
    let b1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("b1");
    let t = invalid_twin(&mut nodes[0], &b1, &shard_id).await;

    accept(&mut nodes[2], &b1, "v2 accepts b1").await;
    let built = nodes[2]
        .create_block_unsafe(&[deploy(10, &shard_id)])
        .await
        .expect("v2 block");
    assert!(built.header.parents_hash_list.contains(&b1.block_hash));
    let mut on_t = built.clone();
    for parent in on_t.header.parents_hash_list.iter_mut() {
        if *parent == b1.block_hash {
            *parent = t.block_hash.clone();
        }
    }
    for justification in on_t.justifications.iter_mut() {
        if justification.validator == b1.sender {
            justification.latest_block_hash = t.block_hash.clone();
        }
    }
    let c = nodes[2]
        .validator_id_opt
        .as_ref()
        .expect("v2 identity")
        .sign_block(&on_t);

    refuse_invalid_twin(&mut nodes[3], &t, "v3 receives t first and refuses it").await;

    accept(&mut nodes[1], &b1, "v1 accepts b1").await;
    let v1_t = nodes[1].process_block(t.clone()).await.expect("v1 t");
    assert!(
        is_equivocation(&v1_t),
        "v1 holds t as the equivocation: {v1_t:?}"
    );
    let v1_c = nodes[1].process_block(c.clone()).await.expect("v1 c");
    assert!(
        matches!(
            v1_c,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents))
        ),
        "v1 refuses c, built on a twin whose contents are invalid: {v1_c:?}"
    );
    assert!(
        matches!(remembered_verdict(&nodes[1], &t.block_hash), Some(Err(_))),
        "v1 remembers t's contents are invalid"
    );
    let dag = nodes[1]
        .casper
        .block_dag_storage
        .get_representation()
        .expect("dag");
    assert!(!t.body.deploys.is_empty());
    for processed in &t.body.deploys {
        assert!(
            dag.deploy_lifecycle_events(&processed.deploy.sig)
                .expect("read row")
                .is_none(),
            "t's deploys stay out of v1's deploy history"
        );
    }
    assert_eq!(
        held_pair(&nodes[1], &c.block_hash),
        None,
        "v1 does not hold c"
    );
}

/// v0's twin of `b1` that carries other deploys but declares `b1`'s
/// post-state: an equivocation whose contents are invalid.
async fn invalid_twin(node: &mut TestNode, b1: &BlockMessage, shard_id: &str) -> BlockMessage {
    let sibling = equivocate_block(node, b1, vec![deploy(100, shard_id)])
        .await
        .expect("sibling");
    let mut forged = sibling;
    forged.body.state.post_state_hash = b1.body.state.post_state_hash.clone();
    node.validator_id_opt
        .as_ref()
        .expect("v0 identity")
        .sign_block(&forged)
}

async fn refuse_invalid_twin(node: &mut TestNode, t: &BlockMessage, what: &str) {
    let verdict = node.process_block(t.clone()).await.expect(what);
    assert!(
        matches!(
            verdict,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidTransaction))
        ),
        "{what}: {verdict:?}"
    );
}

/// v1 holds b1 and an invalid twin t of it as the equivocation. v1's next
/// block d cites b1 for v0 and still slashes v0. v3 received t first and
/// refused it; d needs only b1 from v3, so v3 adds d once it holds b1.
#[serial_test::serial]
#[tokio::test]
async fn a_block_cites_the_sibling_of_an_invalid_twin() {
    let (mut nodes, shard_id) = network(4, None).await;
    let b1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("b1");
    let t = invalid_twin(&mut nodes[0], &b1, &shard_id).await;
    refuse_invalid_twin(&mut nodes[3], &t, "v3 refuses t on its contents").await;

    accept(&mut nodes[1], &b1, "v1 accepts b1").await;
    let v1_t = nodes[1].process_block(t.clone()).await.expect("v1 t");
    assert!(
        is_equivocation(&v1_t),
        "v1 holds t as the equivocation: {v1_t:?}"
    );
    let d = nodes[1]
        .create_block_unsafe(&[deploy(11, &shard_id)])
        .await
        .expect("d");
    assert_eq!(
        cites(&d, &b1.sender),
        Some(b1.block_hash.clone()),
        "d cites b1 for v0"
    );
    assert!(
        !proto_util::dependencies_hashes_of(&d).contains(&t.block_hash),
        "d does not depend on t"
    );
    assert!(carries_slash(&d), "d slashes v0");
    accept(&mut nodes[1], &d, "v1 accepts d").await;

    let v3_d = nodes[3].process_block(d.clone()).await.expect("v3 d");
    assert!(
        matches!(v3_d, Either::Left(BlockError::MissingBlocks)),
        "v3 waits on b1 for d: {v3_d:?}"
    );
    let from_v1 = nodes[1].local.clone();
    nodes[3]
        .receive_block(b1.clone(), from_v1)
        .await
        .expect("v3 b1");
    assert!(
        nodes[3].contains(&d.block_hash),
        "v3 adds d once it holds b1"
    );
}

/// v2 signs e, which cites the invalid twin t for v0 and builds on neither
/// block of the pair. v1 holds t as the equivocation and refuses e on t's
/// contents. v3 received t first and refused it, so it never holds e.
#[serial_test::serial]
#[tokio::test]
async fn a_block_citing_an_invalid_twin_is_held_by_no_node() {
    let (mut nodes, shard_id) = network(4, None).await;
    let b1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("b1");
    let t = invalid_twin(&mut nodes[0], &b1, &shard_id).await;
    refuse_invalid_twin(&mut nodes[3], &t, "v3 refuses t on its contents").await;
    accept(&mut nodes[1], &b1, "v1 accepts b1").await;
    let v1_t = nodes[1].process_block(t.clone()).await.expect("v1 t");
    assert!(
        is_equivocation(&v1_t),
        "v1 holds t as the equivocation: {v1_t:?}"
    );

    let mut citing_t = nodes[2]
        .create_block_unsafe(&[deploy(12, &shard_id)])
        .await
        .expect("v2 block");
    for justification in citing_t.justifications.iter_mut() {
        if justification.validator == b1.sender {
            justification.latest_block_hash = t.block_hash.clone();
        }
    }
    let e = nodes[2]
        .validator_id_opt
        .as_ref()
        .expect("v2 identity")
        .sign_block(&citing_t);
    assert!(
        !e.header.parents_hash_list.contains(&t.block_hash)
            && !e.header.parents_hash_list.contains(&b1.block_hash),
        "e builds on neither block of the pair"
    );

    let v1_e = nodes[1].process_block(e.clone()).await.expect("v1 e");
    assert!(
        matches!(
            v1_e,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidJustifications))
        ),
        "v1 refuses e, which cites a twin whose contents are invalid: {v1_e:?}"
    );
    assert_eq!(
        held_pair(&nodes[1], &e.block_hash),
        None,
        "v1 does not hold e"
    );

    let v3_e = nodes[3].process_block(e.clone()).await.expect("v3 e");
    assert!(
        matches!(v3_e, Either::Left(BlockError::MissingBlocks)),
        "v3 waits on t for e: {v3_e:?}"
    );
    let from_v1 = nodes[1].local.clone();
    let v3_t = nodes[3]
        .receive_block(t.clone(), from_v1)
        .await
        .expect("v3 t from v1");
    assert!(
        matches!(
            v3_t,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidTransaction))
        ),
        "v3 refuses t from v1 again: {v3_t:?}"
    );
    assert_eq!(
        held_pair(&nodes[3], &e.block_hash),
        None,
        "v3 does not hold e"
    );
}

/// v1 holds b1 and a valid twin s as the equivocation, so s is v0's latest
/// message at v1, yet v1's next block d cites b1. v3 received s first and
/// holds b1 as the equivocation; it judges b1 and accepts d.
#[serial_test::serial]
#[tokio::test]
async fn a_block_cites_the_block_its_proposer_did_not_store_as_the_twin() {
    let (mut nodes, shard_id) = network(4, None).await;
    let b1 = nodes[0]
        .create_block_unsafe(&[deploy(0, &shard_id)])
        .await
        .expect("b1");
    let s = equivocate_block(&mut nodes[0], &b1, vec![deploy(1, &shard_id)])
        .await
        .expect("s");
    accept(&mut nodes[3], &s, "v3 accepts s").await;
    accept(&mut nodes[1], &b1, "v1 accepts b1").await;
    let v1_s = nodes[1].process_block(s.clone()).await.expect("v1 s");
    assert!(
        is_equivocation(&v1_s),
        "v1 holds s as the equivocation: {v1_s:?}"
    );

    let d = nodes[1]
        .create_block_unsafe(&[deploy(11, &shard_id)])
        .await
        .expect("d");
    assert_eq!(
        cites(&d, &b1.sender),
        Some(b1.block_hash.clone()),
        "d cites b1 for v0"
    );
    assert!(
        !proto_util::dependencies_hashes_of(&d).contains(&s.block_hash),
        "d does not depend on s"
    );

    let v3_b1 = nodes[3].process_block(b1.clone()).await.expect("v3 b1");
    assert!(
        is_equivocation(&v3_b1),
        "v3 holds b1 as the equivocation: {v3_b1:?}"
    );
    accept(&mut nodes[3], &d, "v3 accepts d").await;
}
