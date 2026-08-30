use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ProcessedSystemDeploy, SystemDeployData,
};

use crate::helper::test_node::TestNode;
use crate::slashing::integration_helpers::equivocate_block;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

struct TestContext {
    genesis: GenesisContext,
    shard_id: String,
}

impl TestContext {
    async fn new() -> Self {
        // The default `GenesisBuilder::create_bonds` formula
        // `(i as i64) * 2 + 1` puts validator 0 at stake 1, which is
        // exactly the genesis `minimum_bond`. With deductions from the
        // genesis PoS Rholang initialization, validator 0 ends up at
        // stake 0 (unbonded), which then trips
        // `slashing_authorization::validate_received_slash_deploys`'s
        // `TargetNotBonded` guard when an honest validator tries to
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
        ) -> std::collections::HashMap<crypto::rust::public_key::PublicKey, i64> {
            validators
                .into_iter()
                .zip(vec![100i64, 100, 100, 100])
                .collect()
        }
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

async fn signed_equivocation(
    nodes: &mut [TestNode],
    shard_id: &str,
    first_nonce: i32,
    second_nonce: i32,
) -> (BlockMessage, BlockMessage) {
    let first_deploy =
        construct_deploy::basic_deploy_data(first_nonce, None, Some(shard_id.to_string()))
            .expect("build first equivocation deploy");
    nodes[0]
        .casper
        .deploy(first_deploy)
        .expect("validator 0 deploy");
    let first = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("validator 0 creates first sibling");
    let second_deploy =
        construct_deploy::basic_deploy_data(second_nonce, None, Some(shard_id.to_string()))
            .expect("build second equivocation deploy");
    let second = equivocate_block(&mut nodes[0], &first, vec![second_deploy])
        .await
        .expect("validator 0 creates second sibling");
    (first, second)
}

fn contains_objective_slash(
    block: &BlockMessage,
    left: &BlockMessage,
    right: &BlockMessage,
) -> bool {
    block.body.system_deploys.iter().any(|processed| {
        matches!(
            processed,
            ProcessedSystemDeploy::Succeeded {
                system_deploy:
                    SystemDeployData::Slash {
                        invalid_block_hash,
                        equivocation_block_hash: Some(equivocation_block_hash),
                        ..
                    },
                ..
            } if (invalid_block_hash == &left.block_hash
                && equivocation_block_hash == &right.block_hash)
                || (invalid_block_hash == &right.block_hash
                    && equivocation_block_hash == &left.block_hash)
        )
    })
}

#[tokio::test]
async fn slash_for_equivocator_survives_multi_parent_merge() {
    let ctx = TestContext::new().await;

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");
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

    let (signed_block, invalid_block) = signed_equivocation(&mut nodes, &ctx.shard_id, 0, 7).await;
    nodes[1]
        .process_block(signed_block.clone())
        .await
        .expect("node 1 processes first sibling");
    nodes[1]
        .process_block(invalid_block.clone())
        .await
        .expect("node 1 processes second sibling");
    nodes[2]
        .process_block(invalid_block.clone())
        .await
        .expect("node 2 processes second sibling");
    nodes[2]
        .process_block(signed_block.clone())
        .await
        .expect("node 2 processes first sibling");

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
        contains_objective_slash(&block_1, &signed_block, &invalid_block),
        "block_1 must contain the canonical two-sibling SlashDeploy"
    );
    assert!(
        contains_objective_slash(&block_2, &signed_block, &invalid_block),
        "block_2 must contain the canonical two-sibling SlashDeploy"
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
        .map(|b| b.stake)
        .unwrap_or(0);
    assert!(
        equivocator_stake <= 1,
        "post-merge equivocator stake must be at the bond floor (<=1); got {}",
        equivocator_stake
    );

    // Catches a regression where the slash hits the merge proposer
    // instead of (or in addition to) the equivocator.
    let proposer_stake = post_merge_bonds
        .iter()
        .find(|b| b.validator == merge_proposer_pk.bytes)
        .map(|b| b.stake)
        .expect("merge proposer must still appear in bonds map");
    let proposer_genesis_stake = ctx
        .genesis
        .genesis_block
        .body
        .state
        .bonds
        .iter()
        .find(|b| b.validator == merge_proposer_pk.bytes)
        .map(|b| b.stake)
        .expect("merge proposer must be bonded at genesis");
    assert_eq!(
        proposer_stake, proposer_genesis_stake,
        "merge proposer's stake must be unchanged after the merge"
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
// Finalized-floor authority does not itself apply the transition; safety in
// that window comes from the merge combining the parents' bond-channel state without
// netting a re-bond (and, on a node that has seen the equivocation,
// `neglected_invalid_block` re-enforcing it). Either way the equivocator must
// end at the bond floor. Contrast `slash_for_equivocator_survives_multi_parent_merge`,
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
    let equivocator_pk = nodes[0]
        .validator_id_opt
        .as_ref()
        .expect("node 0 has validator identity")
        .public_key
        .clone();

    let (signed_block, invalid_block) = signed_equivocation(&mut nodes, &ctx.shard_id, 0, 7).await;

    nodes[1]
        .process_block(signed_block.clone())
        .await
        .expect("node 1 processes first sibling");
    nodes[1]
        .process_block(invalid_block.clone())
        .await
        .expect("node 1 processes second sibling");

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
        contains_objective_slash(&slash_block, &signed_block, &invalid_block),
        "slash_block must contain the canonical two-sibling SlashDeploy"
    );
    assert!(
        !contains_objective_slash(&pre_slash_block, &signed_block, &invalid_block),
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
        .find(|b| b.validator == equivocator_pk.bytes)
        .map(|b| b.stake)
        .unwrap_or(0);
    assert!(
        equivocator_stake <= 1,
        "post-merge equivocator stake must be at the bond floor (<=1) even when a \
         pre-slash sibling is merged; got {}",
        equivocator_stake
    );
}

#[tokio::test]
async fn canonical_prestate_zero_bond_excludes_duplicate_slash() {
    let ctx = TestContext::new().await;

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");
    let (signed_block, invalid_block) = signed_equivocation(&mut nodes, &ctx.shard_id, 0, 7).await;
    nodes[1]
        .process_block(signed_block.clone())
        .await
        .expect("node 1 processes first sibling");
    nodes[1]
        .process_block(invalid_block.clone())
        .await
        .expect("node 1 processes second sibling");
    nodes[2]
        .process_block(invalid_block.clone())
        .await
        .expect("node 2 processes second sibling");
    nodes[2]
        .process_block(signed_block.clone())
        .await
        .expect("node 2 processes first sibling");

    // Each honest validator proposes a sibling block at block_number=1
    // so the merge proposer's tip set contains two non-ancestor parents.
    // Without that, `compute_parents_post_state` skips the cache via
    // either the single-parent path or the descendant-fast-path.
    let deploy_a = construct_deploy::basic_deploy_data(1, None, Some(ctx.shard_id.clone()))
        .expect("build deploy a");
    nodes[1]
        .casper
        .deploy(deploy_a)
        .expect("validator 1 deploy a");
    let block_a = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("validator 1 creates block_a");
    nodes[1]
        .process_block(block_a.clone())
        .await
        .expect("node 1 processes its own block_a");

    let deploy_b = construct_deploy::basic_deploy_data(2, None, Some(ctx.shard_id.clone()))
        .expect("build deploy b");
    nodes[2]
        .casper
        .deploy(deploy_b)
        .expect("validator 2 deploy b");
    let block_b = nodes[2]
        .create_block_unsafe(&[])
        .await
        .expect("validator 2 creates block_b");
    nodes[1]
        .process_block(block_b.clone())
        .await
        .expect("node 1 processes block_b");

    let snapshot = nodes[1].casper.get_snapshot().await.expect("get_snapshot");
    assert!(
        snapshot.parents.len() >= 2,
        "test setup requires multi-parent proposer view; got {} parent(s)",
        snapshot.parents.len()
    );
    let latest_messages: std::collections::BTreeMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let merged_state = casper::rust::util::rholang::interpreter_util::compute_parents_post_state(
        &nodes[1].block_store,
        snapshot.parents.clone(),
        &snapshot,
        &nodes[1].runtime_manager,
        &latest_messages,
        None,
        Some(&nodes[1].rejected_deploy_buffer),
        None,
        None,
    )
    .await
    .expect("real merge to seed cache value")
    .state;

    let merged_bonds = nodes[1]
        .runtime_manager
        .compute_bonds(&merged_state)
        .await
        .expect("compute merged bonds");
    let offender_stake = merged_bonds
        .iter()
        .find(|bond| bond.validator == invalid_block.sender)
        .map(|bond| bond.stake)
        .unwrap_or(0);
    assert_eq!(offender_stake, 0);
    drop(snapshot);

    let user_deploy = construct_deploy::basic_deploy_data(1, None, Some(ctx.shard_id.clone()))
        .expect("build user deploy");
    nodes[1]
        .casper
        .deploy(user_deploy)
        .expect("validator 1 deploys");
    let block = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("validator 1 creates block");

    assert!(
        !contains_objective_slash(&block, &signed_block, &invalid_block),
        "canonical candidate selection must exclude an offender whose merged-pre-state bond is zero"
    );
}

#[tokio::test]
async fn canonical_prestate_zero_bond_is_not_proposal_work() {
    let ctx = TestContext::new().await;

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");
    let (signed_block, invalid_block) = signed_equivocation(&mut nodes, &ctx.shard_id, 0, 7).await;
    nodes[1]
        .process_block(signed_block.clone())
        .await
        .expect("node 1 processes first sibling");
    nodes[1]
        .process_block(invalid_block.clone())
        .await
        .expect("node 1 processes second sibling");
    nodes[2]
        .process_block(invalid_block.clone())
        .await
        .expect("node 2 processes second sibling");
    nodes[2]
        .process_block(signed_block.clone())
        .await
        .expect("node 2 processes first sibling");

    let deploy_a = construct_deploy::basic_deploy_data(1, None, Some(ctx.shard_id.clone()))
        .expect("build deploy a");
    nodes[1]
        .casper
        .deploy(deploy_a)
        .expect("validator 1 deploy a");
    let block_a = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("validator 1 creates block_a");
    nodes[1]
        .process_block(block_a.clone())
        .await
        .expect("node 1 processes its own block_a");

    let deploy_b = construct_deploy::basic_deploy_data(2, None, Some(ctx.shard_id.clone()))
        .expect("build deploy b");
    nodes[2]
        .casper
        .deploy(deploy_b)
        .expect("validator 2 deploy b");
    let block_b = nodes[2]
        .create_block_unsafe(&[])
        .await
        .expect("validator 2 creates block_b");
    nodes[1]
        .process_block(block_b.clone())
        .await
        .expect("node 1 processes block_b");

    let snapshot = nodes[1].casper.get_snapshot().await.expect("get_snapshot");
    assert!(
        snapshot.parents.len() >= 2,
        "test setup requires multi-parent proposer view; got {} parent(s)",
        snapshot.parents.len()
    );
    let latest_messages: std::collections::BTreeMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let merged_state = casper::rust::util::rholang::interpreter_util::compute_parents_post_state(
        &nodes[1].block_store,
        snapshot.parents.clone(),
        &snapshot,
        &nodes[1].runtime_manager,
        &latest_messages,
        None,
        Some(&nodes[1].rejected_deploy_buffer),
        None,
        None,
    )
    .await
    .expect("real merge to seed cache value")
    .state;

    let merged_bonds = nodes[1]
        .runtime_manager
        .compute_bonds(&merged_state)
        .await
        .expect("compute merged bonds");
    let offender_stake = merged_bonds
        .iter()
        .find(|bond| bond.validator == invalid_block.sender)
        .map(|bond| bond.stake)
        .unwrap_or(0);
    assert_eq!(offender_stake, 0);
    drop(snapshot);

    let result = nodes[1]
        .create_block(&[])
        .await
        .expect("evaluate empty proposal");
    assert!(matches!(result, BlockCreatorResult::NoNewDeploys));
}
