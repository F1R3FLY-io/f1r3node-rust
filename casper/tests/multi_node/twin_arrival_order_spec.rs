use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;
use casper::rust::validate::Validate;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::block_util::resign_block;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const EQUIVOCATOR: usize = 1;

async fn network() -> (Vec<TestNode>, String) {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, 4, Some(0.0), None, None, None)
        .await
        .expect("network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    (nodes, shard_id)
}

fn identity(node: &TestNode) -> Validator {
    prost::bytes::Bytes::copy_from_slice(
        &node
            .casper
            .get_validator()
            .expect("validator")
            .public_key
            .bytes,
    )
}

async fn propose(node: &mut TestNode) -> BlockMessage {
    node.create_block_unsafe(&[]).await.expect("block creation")
}

async fn admit(node: &mut TestNode, block: &BlockMessage) {
    let verdict = node.add_block(block.clone()).await.expect("process");
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "block {} refused: {verdict:?}",
        block.body.state.block_number
    );
}

async fn admit_everywhere(nodes: &mut [TestNode], block: &BlockMessage) {
    for node in nodes.iter_mut() {
        admit(node, block).await;
    }
}

/// Two blocks the equivocator signs at one sequence number: the first
/// carries a user deploy, the second is empty. Returns them with the
/// deploy's signature.
async fn twins(
    nodes: &mut [TestNode],
    shard_id: &str,
) -> (BlockMessage, BlockMessage, prost::bytes::Bytes) {
    let empty = propose(&mut nodes[EQUIVOCATOR]).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let deploy = construct_deploy::source_deploy_now_full(
        r#"@"twin_arrival_order"!(1)"#.to_string(),
        None,
        None,
        None,
        Some(0),
        Some(shard_id.to_string()),
    )
    .expect("deploy");
    let sig = deploy.sig.clone();
    let carrying = nodes[EQUIVOCATOR]
        .create_block_unsafe(std::slice::from_ref(&deploy))
        .await
        .expect("block creation");
    assert_eq!(carrying.seq_num, empty.seq_num);
    assert!(carrying
        .body
        .deploys
        .iter()
        .any(|processed| processed.deploy.sig == sig));
    assert!(empty.body.deploys.is_empty());
    (carrying, empty, sig)
}

async fn deliver_twins(node: &mut TestNode, first: &BlockMessage, second: &BlockMessage) {
    admit(node, first).await;
    let verdict = node.add_block(second.clone()).await.expect("process");
    assert!(
        matches!(
            verdict,
            Either::Left(BlockError::Invalid(
                InvalidBlock::IgnorableEquivocation | InvalidBlock::AdmissibleEquivocation
            ))
        ),
        "the second twin is an equivocation: {verdict:?}"
    );
}

/// A block whose justification for a validator moves back to a lower
/// sequence number is judged alike whichever of that validator's twins a
/// node received first.
#[tokio::test]
async fn a_regressing_justification_is_judged_alike_whichever_twin_arrived_first() {
    let (mut nodes, shard_id) = network().await;
    let (carrying, empty, _) = twins(&mut nodes, &shard_id).await;
    admit(&mut nodes[EQUIVOCATOR], &carrying).await;
    let above = propose(&mut nodes[EQUIVOCATOR]).await;
    assert!(above.seq_num > empty.seq_num);

    for block in [&carrying, &above] {
        admit(&mut nodes[0], block).await;
    }
    let previous = propose(&mut nodes[0]).await;
    admit(&mut nodes[0], &previous).await;
    let mut regressing = propose(&mut nodes[0]).await;
    let equivocator = identity(&nodes[EQUIVOCATOR]);
    let cited = regressing
        .justifications
        .iter_mut()
        .find(|justification| justification.validator == equivocator)
        .expect("the block cites the equivocator");
    assert_eq!(cited.latest_block_hash, above.block_hash);
    cited.latest_block_hash = empty.block_hash.clone();
    let key = &nodes[0]
        .validator_id_opt
        .as_ref()
        .expect("validator")
        .private_key;
    let regressing = resign_block(&regressing, key);

    let mut verdicts = Vec::new();
    for (judge, first, second) in [(2, &carrying, &empty), (3, &empty, &carrying)] {
        deliver_twins(&mut nodes[judge], first, second).await;
        for block in [&above, &previous] {
            admit(&mut nodes[judge], block).await;
        }
        let mut snapshot = nodes[judge].casper.get_snapshot().await.expect("snapshot");
        verdicts.push(Validate::justification_regressions(
            &regressing,
            &mut snapshot,
        ));
    }
    assert!(
        matches!(verdicts.as_slice(), [
            Either::Left(BlockError::Invalid(InvalidBlock::JustificationRegression)),
            Either::Left(BlockError::Invalid(InvalidBlock::JustificationRegression))
        ]),
        "verdicts by arrival order (carrying twin first, empty twin first): {verdicts:?}"
    );
}

/// A block is judged against its own previous block, not against the latest
/// block of its sender the judge holds: a judge that already holds a later
/// block of the sender, as a node catching up does, reaches the same verdict.
#[tokio::test]
async fn a_block_is_judged_against_its_own_previous_block() {
    let (mut nodes, _) = network().await;
    let mut chain = Vec::new();
    for index in [0, 1, 0, 1, 0] {
        let block = propose(&mut nodes[index]).await;
        admit_everywhere(&mut nodes, &block).await;
        chain.push(block);
    }
    let (judged, later) = (&chain[2], &chain[4]);
    let mut snapshot = nodes[2].casper.get_snapshot().await.expect("snapshot");
    assert_eq!(
        snapshot.dag.latest_message_hash(&judged.sender),
        Some(later.block_hash.clone())
    );
    let verdict = Validate::justification_regressions(judged, &mut snapshot);
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "an honest block judged after its sender's later block: {verdict:?}"
    );
}
