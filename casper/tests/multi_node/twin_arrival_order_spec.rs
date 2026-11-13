use casper::rust::api::deploy_finalization_status::{self, DeployFinalizationState};
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

/// A deploy carried by one of an equivocator's twins has the same status on
/// every node once the twin is finalized, whichever twin a node received
/// first.
#[tokio::test]
async fn a_deploys_status_does_not_depend_on_which_twin_arrived_first() {
    let (mut nodes, shard_id) = network().await;
    let (carrying, empty, sig) = twins(&mut nodes, &shard_id).await;
    deliver_twins(&mut nodes[2], &carrying, &empty).await;
    deliver_twins(&mut nodes[3], &empty, &carrying).await;
    for index in [0, EQUIVOCATOR] {
        admit(&mut nodes[index], &carrying).await;
    }
    let built_on = propose(&mut nodes[0]).await;
    assert!(
        built_on
            .header
            .parents_hash_list
            .contains(&carrying.block_hash),
        "a node that has seen only the carrying twin builds on it"
    );
    admit_everywhere(&mut nodes, &built_on).await;
    for index in [0, EQUIVOCATOR] {
        let verdict = nodes[index]
            .add_block(empty.clone())
            .await
            .expect("process");
        assert!(matches!(verdict, Either::Left(BlockError::Invalid(_))));
    }

    for _ in 0..12 {
        for index in [0, 2, 3] {
            let block = propose(&mut nodes[index]).await;
            admit_everywhere(&mut nodes, &block).await;
        }
    }

    let mut states = Vec::new();
    for judge in [2, 3] {
        for _ in 0..12 {
            nodes[judge]
                .casper
                .finalize()
                .await
                .expect("finalizer cycle");
        }
        let dag = nodes[judge].casper.block_dag().await.expect("dag");
        let lfb = dag.last_finalized_block();
        assert!(
            lfb == carrying.block_hash
                || dag
                    .is_dag_ancestor(&carrying.block_hash, &lfb)
                    .expect("ancestry"),
            "node {judge}: the finalized block must descend from the carrying twin"
        );
        states.push(
            deploy_finalization_status::resolve(
                &dag,
                nodes[judge].casper.block_store(),
                &sig,
                None,
            )
            .expect("resolve")
            .state,
        );
    }
    assert_eq!(
        states,
        [
            DeployFinalizationState::Finalized,
            DeployFinalizationState::Finalized
        ],
        "(carrying twin first, empty twin first)"
    );
}

/// A twin's contents are judged only when a block builds on it, so a twin
/// nobody builds on adds nothing to the deploy history: one that copies a
/// finalized deploy is not reported as that deploy's block.
#[tokio::test]
async fn a_twin_nobody_builds_on_is_not_where_a_deploy_appears() {
    let (mut nodes, shard_id) = network().await;
    let deploy = construct_deploy::source_deploy_now_full(
        r#"@"twin_arrival_order"!(2)"#.to_string(),
        None,
        None,
        None,
        Some(0),
        Some(shard_id.to_string()),
    )
    .expect("deploy");
    let carrier = nodes[0]
        .create_block_unsafe(std::slice::from_ref(&deploy))
        .await
        .expect("block creation");
    admit_everywhere(&mut nodes, &carrier).await;
    let empty = propose(&mut nodes[EQUIVOCATOR]).await;
    admit_everywhere(&mut nodes, &empty).await;
    let mut copied = empty.clone();
    copied.body.deploys = carrier.body.deploys.clone();
    let twin = nodes[EQUIVOCATOR]
        .validator_id_opt
        .as_ref()
        .expect("validator")
        .sign_block(&copied);
    for node in nodes.iter_mut() {
        let verdict = node.add_block(twin.clone()).await.expect("process");
        assert!(
            matches!(
                verdict,
                Either::Left(BlockError::Invalid(
                    InvalidBlock::IgnorableEquivocation | InvalidBlock::AdmissibleEquivocation
                ))
            ),
            "the copying twin is an equivocation: {verdict:?}"
        );
    }

    for _ in 0..12 {
        for index in [0, 2, 3] {
            let block = propose(&mut nodes[index]).await;
            admit_everywhere(&mut nodes, &block).await;
        }
    }
    for _ in 0..12 {
        nodes[3].casper.finalize().await.expect("finalizer cycle");
    }
    let dag = nodes[3].casper.block_dag().await.expect("dag");
    let status =
        deploy_finalization_status::resolve(&dag, nodes[3].casper.block_store(), &deploy.sig, None)
            .expect("resolve");
    assert_eq!(status.state, DeployFinalizationState::Finalized);
    assert_eq!(status.latest_block_hash, Some(carrier.block_hash.clone()));
}
