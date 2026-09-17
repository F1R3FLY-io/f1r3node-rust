// Slash-recovery coverage for the merge-rejected-slash re-issuance
// path in `block_creator::create`. Two tests:
//   * `slash_for_equivocator_survives_multi_parent_merge` — end-to-end:
//     equivocation, both honest validators slash, multi-parent merge,
//     slash effect lands in canonical post-state.
//   * `e1c_re_issues_merge_rejected_slash` — focused: a synthetic
//     `RejectedSlash` is injected into the parents-post-state cache so
//     `block_creator::create` exercises the re-issuance loop and emits
//     a SlashDeploy in the proposed block's body.

use std::collections::BTreeSet;

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use casper::rust::casper::Casper;
use casper::rust::merging::rejected_slash::RejectedSlash;
use casper::rust::slashing_authorization::checked_base_seq;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::ParentsPostStateCacheKey;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::equivocation_record::EquivocationRecord;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

/// Seed the evidence state an equivocation verdict leaves behind: the
/// invalid record in the DAG plus the tracker's EquivocationRecord. The
/// recovery machinery under test starts downstream of that state;
/// verdict classification itself is covered by the `slashing::`
/// integration suite, and only the equivocation class mints evidence
/// since the `is_slashable` narrowing.
fn observe_slashable_evidence(node: &TestNode, block: &BlockMessage) {
    node.casper
        .block_dag_storage
        .insert(block, InsertMode::Invalid)
        .expect("record invalid evidence");
    let base_seq = checked_base_seq(block.seq_num).expect("evidence block has positive seq");
    node.casper
        .block_dag_storage
        .access_equivocations_tracker(|tracker| {
            tracker.add(EquivocationRecord::new(
                block.sender.clone(),
                base_seq,
                BTreeSet::new(),
            ))
        })
        .expect("record equivocation evidence");
}

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

    // Equivocation: a forged copy of node[0]'s block with a mutated
    // seq_num. Honest validators see this as InvalidBlockHash and queue
    // a slashing deploy for the equivocator.
    let deploy_data = construct_deploy::basic_deploy_data(0, None, Some(ctx.shard_id.clone()))
        .expect("build deploy");
    nodes[0]
        .casper
        .deploy(deploy_data)
        .expect("validator 0 deploy");
    let signed_block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("validator 0 creates signed_block");
    let invalid_block = {
        let mut b = signed_block.clone();
        b.seq_num = 47;
        b
    };
    observe_slashable_evidence(&nodes[1], &invalid_block);
    observe_slashable_evidence(&nodes[2], &invalid_block);

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

    let slashes_in =
        |block: &models::rust::casper::protocol::casper_message::BlockMessage| -> Vec<prost::bytes::Bytes> {
            block
                .body
                .system_deploys
                .iter()
                .filter_map(|psd| match psd {
                    ProcessedSystemDeploy::Succeeded {
                        system_deploy:
                            SystemDeployData::Slash {
                                invalid_block_hash, ..
                            },
                        ..
                    } => Some(invalid_block_hash.clone()),
                    _ => None,
                })
                .collect()
        };
    assert!(
        slashes_in(&block_1).contains(&invalid_block.block_hash),
        "block_1 must contain a SlashDeploy for the equivocator's invalid_block"
    );
    assert!(
        slashes_in(&block_2).contains(&invalid_block.block_hash),
        "block_2 must contain a SlashDeploy for the equivocator's invalid_block"
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
        .expect("equivocator must still appear in bonds map");
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
// `Validate::bonds_cache_from_floor` does not yet force it; safety in that
// window comes from the merge combining the parents' bond-channel state without
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

    // V0 equivocates: a forged copy of node[0]'s block with a mutated seq_num.
    let deploy_data = construct_deploy::basic_deploy_data(0, None, Some(ctx.shard_id.clone()))
        .expect("build deploy");
    nodes[0]
        .casper
        .deploy(deploy_data)
        .expect("validator 0 deploy");
    let signed_block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("validator 0 creates signed_block");
    let invalid_block = {
        let mut b = signed_block.clone();
        b.seq_num = 47;
        b
    };

    // ONLY node 1 observes the equivocation → only node 1 will slash.
    observe_slashable_evidence(&nodes[1], &invalid_block);

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

    let slashes_in =
        |block: &models::rust::casper::protocol::casper_message::BlockMessage| -> Vec<prost::bytes::Bytes> {
            block
                .body
                .system_deploys
                .iter()
                .filter_map(|psd| match psd {
                    ProcessedSystemDeploy::Succeeded {
                        system_deploy:
                            SystemDeployData::Slash {
                                invalid_block_hash, ..
                            },
                        ..
                    } => Some(invalid_block_hash.clone()),
                    _ => None,
                })
                .collect()
        };
    assert!(
        slashes_in(&slash_block).contains(&invalid_block.block_hash),
        "slash_block must contain a SlashDeploy for the equivocator's invalid_block"
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
        .find(|b| b.validator == equivocator_pk.bytes)
        .map(|b| b.stake)
        .expect("equivocator must still appear in bonds map");
    assert!(
        equivocator_stake <= 1,
        "post-merge equivocator stake must be at the bond floor (<=1) even when a \
         pre-slash sibling is merged; got {}",
        equivocator_stake
    );
}

// Exercises the merge-rejected-slash recovery path
// (block_creator.rs:594-609). A synthetic `RejectedSlash` is written
// into the parents-post-state cache so the proposer's
// `compute_parents_post_state` call returns it as if the merge engine
// had rejected a slash chain. The synthetic entry uses a different
// `issuer_public_key` from any own-detected slash so `filter_recoverable`
// keeps it; the E1c loop then emits a SlashDeploy under the proposer's
// identity, which executes against an already-slashed PoS state and
// produces a Failed system-deploy entry in the proposed block's body.
//
// The proposer needs sibling parents (neither a descendant of the
// other) so `compute_parents_post_state` runs the cache-consulting
// merge path. The single-parent path and the descendant-fast-path both
// bypass the cache and skip E1c.
#[tokio::test]
async fn e1c_re_issues_merge_rejected_slash() {
    let ctx = TestContext::new().await;

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");
    let alt_issuer_pk = nodes[2]
        .validator_id_opt
        .as_ref()
        .expect("node 2 has validator identity")
        .public_key
        .clone();

    // Forge an equivocation. node 1 processes the invalid block so
    // own-detection will emit a SlashDeploy under node 1's pk.
    let deploy_data = construct_deploy::basic_deploy_data(0, None, Some(ctx.shard_id.clone()))
        .expect("build deploy");
    nodes[0]
        .casper
        .deploy(deploy_data)
        .expect("validator 0 deploy");
    let signed_block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("validator 0 creates signed_block");
    let invalid_block = {
        let mut b = signed_block.clone();
        b.seq_num = 47;
        b
    };
    observe_slashable_evidence(&nodes[1], &invalid_block);
    observe_slashable_evidence(&nodes[2], &invalid_block);

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

    // Snapshot the proposer's view to derive the cache key the next
    // propose will compute. The merge runs once here so we obtain the
    // real merged pre-state and rejected-deploy list — overwriting the
    // cache entry then lets us augment rejected_slashes without
    // disturbing the rest of the merged-state computation.
    let snapshot = nodes[1].casper.get_snapshot().await.expect("get_snapshot");
    assert!(
        snapshot.parents.len() >= 2,
        "test setup requires multi-parent proposer view; got {} parent(s)",
        snapshot.parents.len()
    );
    let mut sorted_parent_hashes: Vec<prost::bytes::Bytes> = snapshot
        .parents
        .iter()
        .map(|p| p.block_hash.clone())
        .collect();
    sorted_parent_hashes.sort();
    let key_latest_messages: std::collections::BTreeMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let cache_key = ParentsPostStateCacheKey {
        sorted_parent_hashes,
        snapshot_lfb_hash: snapshot.last_finalized_block.clone(),
        sorted_latest_messages: key_latest_messages.into_iter().collect(),
        buffer_populated: true,
    };

    let latest_messages: std::collections::BTreeMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let mut merged = casper::rust::util::rholang::interpreter_util::compute_parents_post_state(
        &nodes[1].block_store,
        snapshot.parents.clone(),
        &snapshot,
        &nodes[1].runtime_manager,
        &latest_messages,
        Some(&nodes[1].rejected_deploy_buffer),
        None,
        None,
    )
    .await
    .expect("real merge to seed cache value");

    let synthetic = RejectedSlash {
        invalid_block_hash: invalid_block.block_hash.clone(),
        issuer_public_key: alt_issuer_pk,
        source_block_hash: invalid_block.block_hash.clone(),
    };
    nodes[1]
        .runtime_manager
        .put_cached_parents_post_state(cache_key, {
            merged.rejected_slashes = vec![synthetic];
            merged
        });
    drop(snapshot);

    // Propose. A user deploy keeps `create_block` from short-circuiting
    // on `NoNewDeploys`; the slash entries are then driven by
    // own-detection plus the cache-injected RejectedSlash.
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

    // Own-detection at the merge proposer is filtered out: parents.first()
    // post-state already shows the equivocator at bond floor, so
    // `prepare_slashing_deploys` returns an empty list. The single
    // SlashDeploy entry in the body therefore comes from the E1c
    // re-issuance loop driven by the cache-injected RejectedSlash. PoS's
    // slash entry-point is idempotent for already-slashed validators, so
    // the re-issued slash succeeds (returns true with no further state
    // change), producing a Succeeded entry in the body.
    let succeeded_slash_for_invalid_block = block
        .body
        .system_deploys
        .iter()
        .filter(|psd| {
            matches!(
                psd,
                ProcessedSystemDeploy::Succeeded {
                    system_deploy: SystemDeployData::Slash { invalid_block_hash, .. },
                    ..
                } if *invalid_block_hash == invalid_block.block_hash
            )
        })
        .count();
    assert_eq!(
        succeeded_slash_for_invalid_block, 1,
        "merge_block.body must contain exactly one Succeeded SlashDeploy \
         for invalid_block. The cache-injected RejectedSlash should \
         survive `filter_recoverable` (different issuer pk than any \
         own-detected slash) and reach the E1c re-issuance loop. Got {} \
         entries — if 0, the E1c loop in `block_creator::create` is not \
         emitting a SlashDeploy for cache-supplied RejectedSlashes.",
        succeeded_slash_for_invalid_block
    );
}

// Regression for the empty-block skip path. A heartbeat-disabled proposer
// (allow_empty_blocks=false, the production default) used to fast-fail on
// `NoNewDeploys` whenever it had no user deploys and no own-detected
// slashes — even when the parent merge had produced rejected slashes that
// only this proposer could re-issue. The fix moves the merge above the
// skip check so `recovered_rejected_slashes` can keep the proposer alive.
//
// Setup mirrors `e1c_re_issues_merge_rejected_slash` (cache-injected
// RejectedSlash, own-detection filtered by bond floor) but omits the
// keep-alive user deploy. Pre-fix this test would error on `NoNewDeploys`;
// post-fix the proposer must still emit the cache-supplied SlashDeploy.
#[tokio::test]
async fn rejected_slash_recovery_keeps_empty_proposer_alive() {
    let ctx = TestContext::new().await;

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");
    let alt_issuer_pk = nodes[2]
        .validator_id_opt
        .as_ref()
        .expect("node 2 has validator identity")
        .public_key
        .clone();

    let deploy_data = construct_deploy::basic_deploy_data(0, None, Some(ctx.shard_id.clone()))
        .expect("build deploy");
    nodes[0]
        .casper
        .deploy(deploy_data)
        .expect("validator 0 deploy");
    let signed_block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("validator 0 creates signed_block");
    let invalid_block = {
        let mut b = signed_block.clone();
        b.seq_num = 47;
        b
    };
    observe_slashable_evidence(&nodes[1], &invalid_block);
    observe_slashable_evidence(&nodes[2], &invalid_block);

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
    let mut sorted_parent_hashes: Vec<prost::bytes::Bytes> = snapshot
        .parents
        .iter()
        .map(|p| p.block_hash.clone())
        .collect();
    sorted_parent_hashes.sort();
    let key_latest_messages: std::collections::BTreeMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let cache_key = ParentsPostStateCacheKey {
        sorted_parent_hashes,
        snapshot_lfb_hash: snapshot.last_finalized_block.clone(),
        sorted_latest_messages: key_latest_messages.into_iter().collect(),
        buffer_populated: true,
    };

    let latest_messages: std::collections::BTreeMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let mut merged = casper::rust::util::rholang::interpreter_util::compute_parents_post_state(
        &nodes[1].block_store,
        snapshot.parents.clone(),
        &snapshot,
        &nodes[1].runtime_manager,
        &latest_messages,
        Some(&nodes[1].rejected_deploy_buffer),
        None,
        None,
    )
    .await
    .expect("real merge to seed cache value");

    let synthetic = RejectedSlash {
        invalid_block_hash: invalid_block.block_hash.clone(),
        issuer_public_key: alt_issuer_pk,
        source_block_hash: invalid_block.block_hash.clone(),
    };
    nodes[1]
        .runtime_manager
        .put_cached_parents_post_state(cache_key, {
            merged.rejected_slashes = vec![synthetic];
            merged
        });
    drop(snapshot);

    // No user deploy. With allow_empty_blocks=false (TestNode default) and
    // own-detection filtered out by bond floor, the only thing keeping
    // the proposer alive is the cache-injected RejectedSlash flowing
    // through `recovered_rejected_slashes`. If the skip check is still
    // pre-merge, `create_block` returns NoNewDeploys and `create_block_unsafe`
    // errors here.
    let block = nodes[1].create_block_unsafe(&[]).await.expect(
        "validator 1 must propose a block even with no user deploys and no own-detected \
         slashes — a pending merge-rejected slash should keep the proposer alive. If this \
         fails with NoNewDeploys, the empty-block skip check is running before the merge \
         and dropping rejected-slash recovery.",
    );

    let succeeded_slash_for_invalid_block = block
        .body
        .system_deploys
        .iter()
        .filter(|psd| {
            matches!(
                psd,
                ProcessedSystemDeploy::Succeeded {
                    system_deploy: SystemDeployData::Slash { invalid_block_hash, .. },
                    ..
                } if *invalid_block_hash == invalid_block.block_hash
            )
        })
        .count();
    assert_eq!(
        succeeded_slash_for_invalid_block, 1,
        "block.body must contain exactly one Succeeded SlashDeploy for invalid_block. \
         Got {} entries — the skip check should have allowed the proposer through on \
         the strength of recovered_rejected_slashes alone.",
        succeeded_slash_for_invalid_block
    );
}
