// How a slash survives a multi-parent merge, and who issues it: the slash
// effect lands once in the merged state, a proposer does not repeat a slash
// its merged parents already carry, and a proposer whose view trails its own
// finalized frontier still slashes.

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use casper::rust::block_status::ValidBlock;
use casper::rust::casper::Casper;
use casper::rust::util::{construct_deploy, proto_util};
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::slashing::integration_helpers::equivocate_block;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

/// Validator 0 signs two blocks at one sequence number. Each of `observers`
/// receives the first, then judges the second an equivocation and holds the
/// pair. Returns the equivocator.
async fn equivocation(nodes: &mut [TestNode], shard_id: &str, observers: &[usize]) -> Validator {
    let deploy = |nonce: i32| {
        construct_deploy::basic_deploy_data(nonce, None, Some(shard_id.to_string()))
            .expect("build deploy")
    };
    let first = nodes[0]
        .create_block_unsafe(&[deploy(900)])
        .await
        .expect("validator 0 creates a block");
    let second = equivocate_block(&mut nodes[0], &first, vec![deploy(901)])
        .await
        .expect("validator 0 signs a sibling");
    for &observer in observers {
        let accepted = nodes[observer]
            .process_block(first.clone())
            .await
            .expect("observer processes the first block");
        assert!(
            matches!(accepted, Either::Right(ValidBlock::Valid)),
            "node {observer} accepts the first block: {accepted:?}"
        );
        let flagged = nodes[observer]
            .process_block(second.clone())
            .await
            .expect("observer processes the sibling");
        assert!(
            matches!(flagged, Either::Left(_)),
            "node {observer} holds the sibling as an equivocation: {flagged:?}"
        );
    }
    first.sender
}

/// `node` receives `first`, then judges `second` an equivocation and holds
/// the pair.
async fn hold_pair(node: &mut TestNode, first: &BlockMessage, second: &BlockMessage) {
    node.process_block(first.clone())
        .await
        .expect("observer processes the first block");
    let flagged = node
        .process_block(second.clone())
        .await
        .expect("observer processes the sibling");
    assert!(
        matches!(flagged, Either::Left(_)),
        "the observer holds the sibling as an equivocation: {flagged:?}"
    );
}

struct TestContext {
    genesis: GenesisContext,
    shard_id: String,
}

type BondsFunction = fn(
    Vec<crypto::rust::public_key::PublicKey>,
) -> std::collections::HashMap<
    crypto::rust::public_key::PublicKey,
    shared::rust::token_amount::TokenAmount,
>;

impl TestContext {
    async fn new() -> Self {
        // The default `GenesisBuilder::create_bonds` formula
        // `(i as i64) * 2 + 1` puts validator 0 at stake 1, which is
        // exactly the genesis `minimum_bond`. With deductions from the
        // genesis PoS Rholang initialization, validator 0 ends up at
        // stake 0 (unbonded), which then trips
        // `slashing_authorization::validate_received_slash_deploys`'s
        // `NothingToSlash` guard when an honest validator tries to
        // slash validator 0 for equivocating. The dev-side tests in
        // this file assume the equivocator is bonded — so use a custom
        // bonds_function that gives every validator a comfortably-bonded
        // stake (well above `minimum_bond = 1`).
        //
        // 100 is arbitrary but chosen so that even after a "slash to 0"
        // and PoS reward redistribution, the active-validator
        // accounting stays clearly separated. 4 default validators are
        // built by `build_genesis_parameters_with_defaults(_, None)`.
        fn bonds_function(
            validators: Vec<crypto::rust::public_key::PublicKey>,
        ) -> std::collections::HashMap<
            crypto::rust::public_key::PublicKey,
            shared::rust::token_amount::TokenAmount,
        > {
            validators
                .into_iter()
                .zip(vec![100u64, 100, 100, 100])
                .map(|(key, stake)| (key, stake.into()))
                .collect()
        }
        Self::with_bonds(bonds_function).await
    }

    async fn with_bonds(bonds_function: BondsFunction) -> Self {
        let parameters =
            GenesisBuilder::build_genesis_parameters_with_defaults(Some(bonds_function), None);
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(parameters))
            .await
            .expect("Failed to build genesis");
        let shard_id = genesis.genesis_block.shard_id.clone();
        Self { genesis, shard_id }
    }
}

/// The validators the slashes in `block` punish.
fn slashes_in(block: &BlockMessage) -> Vec<Validator> {
    block
        .body
        .system_deploys
        .iter()
        .filter_map(|psd| match psd {
            ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::Slash { evidence, .. },
                ..
            } => Some(evidence.first.sender.clone()),
            _ => None,
        })
        .collect()
}

/// Validators 1 and 2 each slash validator 0's equivocation in parallel
/// blocks, and node 1 holds both.
struct ParallelSlashes {
    nodes: Vec<TestNode>,
    equivocator: Validator,
    block_1: BlockMessage,
    block_2: BlockMessage,
}

async fn parallel_slashes(ctx: &TestContext) -> ParallelSlashes {
    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");

    let equivocator = equivocation(&mut nodes, &ctx.shard_id, &[1, 2]).await;

    // Each honest validator proposes a block containing its own
    // auto-emitted SlashDeploy via prepare_slashing_deploys.
    let deploy_data_a = construct_deploy::basic_deploy_data(1, None, Some(ctx.shard_id.clone()))
        .expect("build deploy a");
    nodes[1]
        .casper
        .deploy(deploy_data_a)
        .expect("validator 1 deploy");
    let block_1 = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("validator 1 creates block_1");
    nodes[1]
        .process_block(block_1.clone())
        .await
        .expect("node 1 processes its own block_1");

    let deploy_data_b = construct_deploy::basic_deploy_data(2, None, Some(ctx.shard_id.clone()))
        .expect("build deploy b");
    nodes[2]
        .casper
        .deploy(deploy_data_b)
        .expect("validator 2 deploy");
    let block_2 = nodes[2]
        .create_block_unsafe(&[])
        .await
        .expect("validator 2 creates block_2");
    nodes[2]
        .process_block(block_2.clone())
        .await
        .expect("node 2 processes its own block_2");

    assert!(
        slashes_in(&block_1).contains(&equivocator),
        "block_1 must slash the equivocator"
    );
    assert!(
        slashes_in(&block_2).contains(&equivocator),
        "block_2 must slash the equivocator"
    );

    // Sync block_2 into node 1 so the next propose can take both as parents.
    nodes[1]
        .process_block(block_2.clone())
        .await
        .expect("node 1 processes block_2");
    assert!(
        nodes[1].contains(&block_2.block_hash),
        "node 1 must observe block_2 after process_block"
    );

    ParallelSlashes {
        nodes,
        equivocator,
        block_1,
        block_2,
    }
}

#[tokio::test]
async fn slash_for_equivocator_survives_multi_parent_merge() {
    let ctx = TestContext::new().await;
    let ParallelSlashes {
        mut nodes,
        block_1,
        block_2,
        ..
    } = parallel_slashes(&ctx).await;
    let equivocator_pk = nodes[0]
        .validator_id_opt
        .as_ref()
        .expect("node 0 has validator identity")
        .public_key
        .clone();
    let merge_proposer_pk = nodes[1]
        .validator_id_opt
        .as_ref()
        .expect("node 1 has validator identity")
        .public_key
        .clone();

    // A fresh user deploy keeps create_block from short-circuiting
    // on NoNewDeploys when the merge proposer's own slash detection is
    // already covered by the merged parent state.
    let marker_deploy = construct_deploy::basic_deploy_data(3, None, Some(ctx.shard_id.clone()))
        .expect("build marker deploy");
    nodes[1]
        .casper
        .deploy(marker_deploy)
        .expect("validator 1 deploys marker");
    let merge_block = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("validator 1 creates merge_block");

    let merge_parents: Vec<&prost::bytes::Bytes> =
        merge_block.header.parents_hash_list.iter().collect();
    assert!(
        merge_parents.iter().any(|h| **h == block_1.block_hash),
        "merge_block parents must include block_1"
    );
    assert!(
        merge_parents.iter().any(|h| **h == block_2.block_hash),
        "merge_block parents must include block_2"
    );

    // Post-merge bonds: equivocator must be at the bond floor
    // (<=1; tests currently use floor 0). Catches a regression where
    // the slash effect failed to land in canonical state through the
    // multi-parent merge.
    let post_merge_bonds = nodes[1]
        .runtime_manager
        .compute_bonds(&casper::rust::util::proto_util::post_state_hash(
            &merge_block,
        ))
        .await
        .expect("compute_bonds");
    let equivocator_stake = post_merge_bonds
        .iter()
        .find(|b| b.validator == equivocator_pk.bytes)
        .map(|b| b.stake.clone())
        .expect("equivocator must still appear in bonds map");
    assert!(
        equivocator_stake <= 1.into(),
        "post-merge equivocator stake must be at the bond floor (<=1); got {}",
        equivocator_stake
    );

    // Catches a regression where the slash hits the merge proposer
    // instead of (or in addition to) the equivocator.
    let proposer_stake = post_merge_bonds
        .iter()
        .find(|b| b.validator == merge_proposer_pk.bytes)
        .map(|b| b.stake.clone())
        .expect("merge proposer must still appear in bonds map");
    let proposer_genesis_stake = ctx
        .genesis
        .genesis_block
        .body
        .state
        .bonds
        .iter()
        .find(|b| b.validator == merge_proposer_pk.bytes)
        .map(|b| b.stake.clone())
        .expect("merge proposer must be bonded at genesis");
    assert_eq!(
        proposer_stake, proposer_genesis_stake,
        "merge proposer's stake must be unchanged after the merge"
    );
}

/// A merge that drops one parent's copy of a slash keeps the other's effect,
/// so the merged pre-state already shows the offender unbonded. Re-issuing
/// the dropped copy repeats a slash that has landed, and once the floor shows
/// the offender unbonded too, that block is unauthorized.
#[tokio::test]
async fn a_slash_the_merged_parents_carry_is_not_re_issued() {
    let ctx = TestContext::new().await;
    let ParallelSlashes {
        mut nodes,
        equivocator,
        block_1,
        block_2,
    } = parallel_slashes(&ctx).await;

    let marker_deploy = construct_deploy::basic_deploy_data(3, None, Some(ctx.shard_id.clone()))
        .expect("build marker deploy");
    nodes[1]
        .casper
        .deploy(marker_deploy)
        .expect("validator 1 deploys marker");
    let merge_block = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("validator 1 creates merge_block");
    assert!(
        merge_block
            .header
            .parents_hash_list
            .contains(&block_1.block_hash)
            && merge_block
                .header
                .parents_hash_list
                .contains(&block_2.block_hash),
        "merge_block must merge both slashing blocks"
    );

    assert!(
        !slashes_in(&merge_block).contains(&equivocator),
        "merge_block re-issues a slash its merged parents already applied"
    );
    let verdict = nodes[1]
        .process_block(merge_block.clone())
        .await
        .expect("node 1 validates merge_block");
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "merge_block: {verdict:?}"
    );
}

// Change 1c / Option A regression guard (sealed-floor merge). After removing
// the proposer-side bonded-set "intersection" parent-filter, a multi-parent
// merge that includes a PRE-slash-view sibling parent — one whose proposer had
// NOT yet observed the equivocation, so it carries no SlashDeploy and still
// shows the equivocator bonded — must NOT regress a slash carried on another
// parent.
//
// This is the "pre-finalization window" corner surfaced during the merge
// arbitration: the finalized floor may still predate the slash, so
// `Validate::bonds_cache_from_floor` does not yet force it; safety in that
// window comes from the merge combining the parents' bond-channel state without
// netting a re-bond. The equivocator must end at the bond floor. Contrast `slash_for_equivocator_survives_multi_parent_merge`,
// where BOTH parents carry the slash.
//
// Coverage note: the sibling here is a normal user block that does not itself
// write the equivocator's bond channel (the realistic case). A sibling that
// explicitly re-writes the offender's bond is not producible through ordinary
// deploys, so that deeper number-channel-netting path is out of scope here.
#[tokio::test]
async fn slash_survives_merge_with_pre_slash_sibling() {
    let ctx = TestContext::new().await;

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");
    // ONLY node 1 observes the equivocation → only node 1 will slash.
    let equivocator = equivocation(&mut nodes, &ctx.shard_id, &[1]).await;

    // Node 1 proposes a POST-slash block (auto-emitted SlashDeploy for V0).
    let deploy_data_a = construct_deploy::basic_deploy_data(1, None, Some(ctx.shard_id.clone()))
        .expect("build deploy a");
    nodes[1]
        .casper
        .deploy(deploy_data_a)
        .expect("validator 1 deploy");
    let slash_block = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("validator 1 creates slash_block");
    nodes[1]
        .process_block(slash_block.clone())
        .await
        .expect("node 1 processes its own slash_block");

    // Node 2 has NOT observed the equivocation → proposes a PRE-slash-view
    // block: a normal user block with the equivocator still bonded, no slash.
    let deploy_data_b = construct_deploy::basic_deploy_data(2, None, Some(ctx.shard_id.clone()))
        .expect("build deploy b");
    nodes[2]
        .casper
        .deploy(deploy_data_b)
        .expect("validator 2 deploy");
    let pre_slash_block = nodes[2]
        .create_block_unsafe(&[])
        .await
        .expect("validator 2 creates pre_slash_block");

    assert!(
        slashes_in(&slash_block).contains(&equivocator),
        "slash_block must slash the equivocator"
    );
    assert!(
        slashes_in(&pre_slash_block).is_empty(),
        "pre_slash_block must NOT carry a slash (node 2 never saw the equivocation)"
    );

    // Node 1 ingests the pre-slash sibling, then merges both parents.
    nodes[1]
        .process_block(pre_slash_block.clone())
        .await
        .expect("node 1 processes pre_slash_block");
    let marker_deploy = construct_deploy::basic_deploy_data(3, None, Some(ctx.shard_id.clone()))
        .expect("build marker deploy");
    nodes[1]
        .casper
        .deploy(marker_deploy)
        .expect("validator 1 deploys marker");
    let merge_block = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("validator 1 creates merge_block");

    // Both parents must be present — the removed intersection filter would have
    // been the only thing that could drop the pre-slash sibling at select time.
    let merge_parents: Vec<&prost::bytes::Bytes> =
        merge_block.header.parents_hash_list.iter().collect();
    assert!(
        merge_parents.iter().any(|h| **h == slash_block.block_hash),
        "merge_block parents must include slash_block"
    );
    assert!(
        merge_parents
            .iter()
            .any(|h| **h == pre_slash_block.block_hash),
        "merge_block parents must include the pre-slash sibling"
    );

    // The slash must not regress through the merge with a pre-slash sibling.
    let post_merge_bonds = nodes[1]
        .runtime_manager
        .compute_bonds(&casper::rust::util::proto_util::post_state_hash(
            &merge_block,
        ))
        .await
        .expect("compute_bonds");
    let equivocator_stake = post_merge_bonds
        .iter()
        .find(|b| b.validator == equivocator)
        .map(|b| b.stake.clone())
        .expect("equivocator must still appear in bonds map");
    assert!(
        equivocator_stake <= 1.into(),
        "post-merge equivocator stake must be at the bond floor (<=1) even when a \
         pre-slash sibling is merged; got {}",
        equivocator_stake
    );
}

/// A proposer whose finalized frontier is ahead of its proposal view still
/// slashes, and a peer with a current view accepts it.
#[tokio::test]
async fn a_view_lagging_its_finalized_frontier_slashes_and_peers_accept() {
    use casper::rust::blocks::proposer::block_creator;
    use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
    use casper::rust::blocks::proposer::proposer::DeploySelection;
    let ctx = TestContext::new().await;
    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");
    let deploy = |nonce: i32| {
        construct_deploy::basic_deploy_data(nonce, None, Some(ctx.shard_id.clone()))
            .expect("build deploy")
    };

    let block_a = nodes[1]
        .add_block_from_deploys(&[deploy(1)])
        .await
        .expect("validator 1 creates block_a");
    let block_b = nodes[2]
        .add_block_from_deploys(&[deploy(2)])
        .await
        .expect("validator 2 creates block_b");
    nodes[1]
        .process_block(block_b.clone())
        .await
        .expect("node 1 processes block_b");
    nodes[2]
        .process_block(block_a.clone())
        .await
        .expect("node 2 processes block_a");
    let mut lagging = nodes[1].casper.get_snapshot().await.expect("get_snapshot");

    let mut frontier = block_b.clone();
    for nonce in 10..14 {
        frontier = nodes[2]
            .add_block_from_deploys(&[deploy(nonce)])
            .await
            .expect("validator 2 extends the chain");
        nodes[1]
            .process_block(frontier.clone())
            .await
            .expect("node 1 processes validator 2's block");
    }
    assert!(
        frontier.body.state.block_number > lagging.max_block_num + 1,
        "the frontier must sit above the view"
    );

    let equivocator = equivocation(&mut nodes, &ctx.shard_id, &[1, 2]).await;
    lagging.dag = nodes[1]
        .casper
        .block_dag_storage
        .get_representation()
        .expect("dag representation");
    lagging.last_finalized_block = frontier.block_hash.clone();

    nodes[1]
        .casper
        .deploy(deploy(20))
        .expect("validator 1 deploys");
    let validator = nodes[1]
        .validator_id_opt
        .clone()
        .expect("node 1 has validator identity");
    let block = match block_creator::create(
        &lagging,
        &validator,
        None,
        nodes[1].deploy_storage.clone(),
        nodes[1].rejected_deploy_buffer.clone(),
        &nodes[1].runtime_manager,
        &mut nodes[1].block_store.clone(),
        DeploySelection::Standard,
    )
    .await
    .expect("block creation")
    {
        BlockCreatorResult::Created(block, ..) => block,
        other => panic!("validator 1 created no block: {other:?}"),
    };

    assert!(
        slashes_in(&block).contains(&equivocator),
        "block #{} carries no slash for the equivocation",
        block.body.state.block_number
    );
    let verdict = nodes[2]
        .process_block(block.clone())
        .await
        .expect("node 2 validates validator 1's block");
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "node 2 judges block #{}: {verdict:?}",
        block.body.state.block_number
    );
}

#[tokio::test]
async fn slash_block_replays_with_its_own_slash_targets() {
    let ctx = TestContext::new().await;

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");

    let equivocator = equivocation(&mut nodes, &ctx.shard_id, &[1, 2]).await;
    let slash_block = nodes[1]
        .create_block_unsafe(&[construct_deploy::basic_deploy_data(
            1,
            None,
            Some(ctx.shard_id.clone()),
        )
        .expect("build deploy")])
        .await
        .expect("validator 1 creates slash_block");

    let unrelated_invalid = {
        let mut b = slash_block.clone();
        b.block_hash = vec![7; 32].into();
        b.sender = ctx.genesis.validator_pks()[3].bytes.clone();
        b
    };
    nodes[2]
        .block_dag_storage
        .insert(&unrelated_invalid, InsertMode::Equivocation {
            sibling: vec![8; 32].into(),
        })
        .expect("record unrelated invalid block");

    let dag = nodes[2]
        .block_dag_storage
        .get_representation()
        .expect("dag");
    let own_targets = proto_util::slash_evidence_senders(proto_util::slash_evidence(&slash_block));
    let dag_invalid_set = dag.invalid_blocks_map().expect("dag invalid set");
    assert_eq!(own_targets.values().collect::<Vec<_>>(), vec![&equivocator]);
    assert!(dag_invalid_set.contains_key(&unrelated_invalid.block_hash));

    let pre_state = &slash_block.body.state.pre_state_hash;
    let post_state = &slash_block.body.state.post_state_hash;
    let block_data = BlockData::from_block(&slash_block);
    let with_dag_set = nodes[2]
        .runtime_manager
        .replay_compute_state(
            pre_state,
            proto_util::deploys(&slash_block),
            proto_util::system_deploys(&slash_block),
            &block_data,
            Some(dag_invalid_set),
            false,
        )
        .await;
    assert!(
        with_dag_set.map_or(true, |hash| hash != *post_state),
        "the extra invalid block must change the slash replay"
    );

    let with_own_targets = nodes[2]
        .runtime_manager
        .replay_compute_state(
            pre_state,
            proto_util::deploys(&slash_block),
            proto_util::system_deploys(&slash_block),
            &block_data,
            Some(own_targets),
            false,
        )
        .await
        .expect("replay with the block's own slash targets");
    assert_eq!(with_own_targets, *post_state);
}

/// Validator 2 outweighs the others, so a merge of its block with a sibling
/// takes its block as the main parent.
fn validator_2_heaviest(
    validators: Vec<crypto::rust::public_key::PublicKey>,
) -> std::collections::HashMap<
    crypto::rust::public_key::PublicKey,
    shared::rust::token_amount::TokenAmount,
> {
    validators
        .into_iter()
        .zip(vec![100u64, 100, 300, 100])
        .map(|(key, stake)| (key, stake.into()))
        .collect()
}

/// The only block carrying a slash loses its merge to a sibling that also
/// writes PoS state and is the merge's main parent: the merged pre-state still
/// shows the offender bonded, and the merge proposer, with no deploys of its
/// own, must slash again.
#[tokio::test]
async fn a_slash_lost_to_the_merge_is_issued_again_by_the_merge_proposer() {
    use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
    use casper::rust::util::construct_deploy::DEFAULT_SEC;
    use models::rust::casper::protocol::casper_message::Bond;

    use crate::helper::bonding_util;

    let ctx = TestContext::with_bonds(validator_2_heaviest).await;
    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");
    let deploy = |nonce: i32| {
        construct_deploy::basic_deploy_data(nonce, None, Some(ctx.shard_id.clone()))
            .expect("build deploy")
    };
    let offender = nodes[0]
        .validator_id_opt
        .as_ref()
        .expect("node 0 has validator identity")
        .public_key
        .bytes
        .clone();
    let stake = |bonds: &[Bond]| {
        bonds
            .iter()
            .find(|bond| bond.validator == offender)
            .map(|bond| bond.stake.clone())
    };

    let first = nodes[0]
        .create_block_unsafe(&[deploy(900)])
        .await
        .expect("validator 0 creates a block");
    let second = equivocate_block(&mut nodes[0], &first, vec![deploy(901)])
        .await
        .expect("validator 0 signs a sibling");
    hold_pair(&mut nodes[1], &first, &second).await;

    let block_1 = nodes[1]
        .create_block_unsafe(&[deploy(1)])
        .await
        .expect("validator 1 creates block_1");
    assert!(slashes_in(&block_1).contains(&offender));
    let verdict = nodes[1]
        .process_block(block_1.clone())
        .await
        .expect("node 1 processes block_1");
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "block_1: {verdict:?}"
    );

    let bond = bonding_util::bonding_deploy(100, &DEFAULT_SEC, Some(ctx.shard_id.clone()))
        .expect("bond deploy");
    let block_2 = nodes[2]
        .create_block_unsafe(&[bond])
        .await
        .expect("validator 2 creates block_2");
    assert!(slashes_in(&block_2).is_empty());
    for i in [2, 1] {
        let verdict = nodes[i]
            .process_block(block_2.clone())
            .await
            .expect("process block_2");
        assert!(
            matches!(verdict, Either::Right(ValidBlock::Valid)),
            "block_2 at node {i}: {verdict:?}"
        );
    }

    let merge_block = match nodes[1]
        .create_block(&[])
        .await
        .expect("create merge block")
    {
        BlockCreatorResult::Created(block, ..) => block,
        BlockCreatorResult::NoNewDeploys => {
            panic!("the merge proposer issues no block for the slash its merge lost")
        }
    };
    assert!(
        merge_block.header.parents_hash_list.first() == Some(&block_2.block_hash)
            && merge_block
                .header
                .parents_hash_list
                .contains(&block_1.block_hash),
        "precondition: merge_block merges block_1 under main parent block_2"
    );
    let pre_state_bonds = nodes[1]
        .runtime_manager
        .compute_bonds(&merge_block.body.state.pre_state_hash)
        .await
        .expect("pre-state bonds");
    assert!(
        stake(&pre_state_bonds).is_some_and(|s| !s.is_zero()),
        "precondition: the merge drops block_1's slash"
    );

    assert!(
        slashes_in(&merge_block).contains(&offender),
        "the merge proposer issues the lost slash again"
    );
    hold_pair(&mut nodes[2], &first, &second).await;
    let verdict = nodes[2]
        .process_block(block_1.clone())
        .await
        .expect("node 2 processes block_1");
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "block_1 at node 2: {verdict:?}"
    );
    for i in [1, 2] {
        let verdict = nodes[i]
            .process_block(merge_block.clone())
            .await
            .expect("validate merge_block");
        assert!(
            matches!(verdict, Either::Right(ValidBlock::Valid)),
            "merge_block at node {i}: {verdict:?}"
        );
    }
    let post_state_bonds = nodes[1]
        .runtime_manager
        .compute_bonds(&merge_block.body.state.post_state_hash)
        .await
        .expect("post-state bonds");
    assert_eq!(
        stake(&post_state_bonds),
        Some(shared::rust::token_amount::TokenAmount::zero()),
        "a block whose own slash succeeded leaves the offender at stake 0"
    );
}
