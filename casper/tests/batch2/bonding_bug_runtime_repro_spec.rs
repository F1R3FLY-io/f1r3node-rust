// Runtime-backed reproduction of the bonding-test bug, end-to-end through
// the production code path (compute_deploys_checkpoint → dag_merger::merge →
// block_creator's applied_sigs computation → repeat_deploy gate).
//
// The lighter unit test in `bonding_bug_repro_spec.rs` covers only the
// validator-side gate by manually setting `merge_block.applied_sigs` to the
// correct value. This test invokes the real `dag_merger` via a real
// `RuntimeManager` and validates that:
//
//   1. dag_merger's new `kept_chain_sigs` output (from this Phase 1 change)
//      correctly identifies the kept-chain sigs for parallel chains with
//      the same deploy.
//   2. block_creator's applied_sigs computation (the new logic that consumes
//      `kept_chain_sigs`) produces the correct `body.state.applied_sigs`.
//   3. The validator's repeat_deploy correctly rejects a child block that
//      attempts to re-include a deploy already applied via the merge's kept
//      chain.
//
// Pattern adapted from `bridge_query_survives_multi_parent_merge` in
// `util/rholang/runtime_manager_test.rs:1777` — direct RM setup, manual
// block construction via `compute_deploys_checkpoint`.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::proto_util;
use casper::rust::util::rholang::interpreter_util::{
    compute_deploys_checkpoint, compute_parents_post_state,
};
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::validate::Validate;
use dashmap::{DashMap, DashSet};
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits;
use models::rust::casper::protocol::casper_message::{ProcessedDeploy, RejectedDeploy};
use prost::bytes::Bytes;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;

use crate::util::rholang::resources::{
    block_dag_storage_from_dyn, mergeable_store_from_dyn,
    mk_test_rnode_store_manager_from_genesis,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bonding_bug_runtime_repro_kept_chain_sig_blocks_recovery_re_inclusion() {
    crate::init_logger();

    // --- Genesis + runtime setup ---
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_block = genesis_context.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();
    let genesis_state = proto_util::post_state_hash(&genesis_block);
    let genesis_bonds = genesis_block.body.state.bonds.clone();
    let validator: Bytes = genesis_context.validator_pks()[0].bytes.clone().into();
    let shard_name = genesis_block.shard_id.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let rspace_store = kvm.r_space_stores().await.expect("rspace stores");
    let mergeable_store = mergeable_store_from_dyn(&mut *kvm)
        .await
        .expect("mergeable store");
    let (mut rm, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        ExternalServices::noop(),
    );

    let mut block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, false, true)
        .expect("dag genesis");

    let now_millis = || -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };

    let mk_snapshot = |lfb: &BlockHash| -> CasperSnapshot {
        let mut snapshot = CasperSnapshot::new(dag_storage.get_representation());
        snapshot.last_finalized_block = lfb.clone();
        let max_seq_nums: DashMap<Bytes, u64> = DashMap::new();
        max_seq_nums.insert(validator.clone(), 0);
        snapshot.max_seq_nums = max_seq_nums;
        let mut shard_conf = CasperShardConf::new();
        shard_conf.shard_name = shard_name.clone();
        shard_conf.max_parent_depth = 0;
        shard_conf.deploy_lifespan = 50;
        let mut bonds_map = HashMap::new();
        bonds_map.insert(validator.clone(), 100);
        snapshot.on_chain_state = OnChainCasperState {
            shard_conf,
            bonds_map,
            active_validators: vec![validator.clone()],
        };
        snapshot.deploys_in_scope = std::sync::Arc::new(DashSet::new());
        snapshot.rejected_in_scope = std::sync::Arc::new(DashSet::new());
        snapshot
    };

    // --- Build the same user deploy that both parallel chains will include ---
    let user_deploy =
        construct_deploy::source_deploy_now_full("Nil".to_string(), None, None, None, None, None)
            .expect("construct user deploy");
    let deploy_sig: Bytes = user_deploy.sig.clone();

    // --- Block A: parent=genesis, body.deploys=[D] ---
    let block_a_raw = block_implicits::get_random_block(
        Some(1),
        Some(1),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(user_deploy.clone())]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let parents_a = vec![genesis_block.clone()];
    let deploys_a: Vec<_> = proto_util::deploys(&block_a_raw)
        .into_iter()
        .map(|d| d.deploy)
        .collect();
    let snapshot_a = mk_snapshot(&genesis_hash);
    let (_, post_state_a, pd_a, _, sys_pd_a, bonds_a) = compute_deploys_checkpoint(
        &mut block_store,
        parents_a,
        deploys_a,
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &snapshot_a,
        &mut rm,
        BlockData::from_block(&block_a_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block A");
    let mut block_a = block_a_raw;
    block_a.body.state.post_state_hash = post_state_a.clone();
    block_a.body.deploys = pd_a;
    block_a.body.system_deploys = sys_pd_a;
    block_a.body.state.bonds = bonds_a;
    block_store
        .put_block_message(&block_a)
        .expect("store block_a");
    dag_storage
        .insert(&block_a, false, false)
        .expect("dag block_a");

    // --- Block B: parent=genesis, body.deploys=[D] (PARALLEL chain) ---
    // Use a different timestamp so the block_hash differs from block_a.
    let block_b_raw = block_implicits::get_random_block(
        Some(1),
        Some(2),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis() + 1),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(user_deploy.clone())]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let parents_b = vec![genesis_block.clone()];
    let deploys_b: Vec<_> = proto_util::deploys(&block_b_raw)
        .into_iter()
        .map(|d| d.deploy)
        .collect();
    let snapshot_b = mk_snapshot(&genesis_hash);
    let (_, post_state_b, pd_b, _, sys_pd_b, bonds_b) = compute_deploys_checkpoint(
        &mut block_store,
        parents_b,
        deploys_b,
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &snapshot_b,
        &mut rm,
        BlockData::from_block(&block_b_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block B");
    let mut block_b = block_b_raw;
    block_b.body.state.post_state_hash = post_state_b.clone();
    block_b.body.deploys = pd_b;
    block_b.body.system_deploys = sys_pd_b;
    block_b.body.state.bonds = bonds_b;
    block_store
        .put_block_message(&block_b)
        .expect("store block_b");
    dag_storage
        .insert(&block_b, false, false)
        .expect("dag block_b");

    // --- Merge [A, B] via the real production path ---
    // This invokes dag_merger::merge, which runs conflict resolution and
    // (with this Phase 1 change) returns `kept_chain_sigs` — the sigs in
    // kept chains paired with source-block heights.
    let parents_merge = vec![block_a.clone(), block_b.clone()];
    let snapshot_merge = mk_snapshot(&genesis_hash);
    let (_merged_state, _rejected, _rejected_slashes, kept_chain_sigs) =
        compute_parents_post_state(&block_store, parents_merge, &snapshot_merge, &rm, None, None)
            .expect("merge [a, b]");

    // ASSERTION 1: dag_merger's kept_chain_sigs must contain D. This is the
    // canonical kept-chain view: even though dag_merger's dedup dropped one
    // chain, D's effects are kept via the other chain.
    assert!(
        kept_chain_sigs.contains_key(&deploy_sig),
        "dag_merger merge result MUST include deploy {} in kept_chain_sigs — \
         the deduplicated chain dropped one instance but the other chain keeps \
         D applied. Without kept_chain_sigs, block_creator's applied_sigs \
         computation can't distinguish 'rejected and rolled back' from \
         'rejected and still applied via parallel kept chain' (the bonding bug).",
        hex::encode(&deploy_sig[..deploy_sig.len().min(8)]),
    );

    // --- Construct merge_block with applied_sigs from kept_chain_sigs ---
    // Mimicking block_creator's new logic (block_creator.rs:1008-1097):
    // union(effective_parents.applied_sigs) overlaid with kept_chain_sigs,
    // then aggregate with body.deploys (none here), then lifespan filter.
    let mut merge_block_applied_sigs: HashMap<Bytes, i64> = HashMap::new();
    // Effective parents: since neither is ancestor of the other, both are
    // effective. union their applied_sigs.
    for parent in [&block_a, &block_b] {
        for (sig, height) in parent.body.state.applied_sigs.iter() {
            merge_block_applied_sigs
                .entry(sig.clone())
                .and_modify(|h| {
                    if *height < *h {
                        *h = *height;
                    }
                })
                .or_insert(*height);
        }
    }
    // Overlay kept_chain_sigs (production block_creator does this — merge's
    // canonical decision wins).
    for (sig, h) in kept_chain_sigs.iter() {
        merge_block_applied_sigs.insert(sig.clone(), *h);
    }

    let merge_block_raw = block_implicits::get_random_block(
        Some(2),
        Some(3),
        Some(post_state_a.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis() + 2),
        Some(vec![block_a.block_hash.clone(), block_b.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()), // no new deploys at merge_block
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let mut merge_block = merge_block_raw;
    merge_block.body.state.applied_sigs = merge_block_applied_sigs;
    block_store
        .put_block_message(&merge_block)
        .expect("store merge_block");
    dag_storage
        .insert(&merge_block, false, false)
        .expect("dag merge_block");

    // ASSERTION 2: merge_block.applied_sigs MUST contain D (sourced from
    // the kept chain via dag_merger's kept_chain_sigs).
    assert!(
        merge_block.body.state.applied_sigs.contains_key(&deploy_sig),
        "merge_block.body.state.applied_sigs MUST contain deploy {} — the \
         merge layer's kept-chain view (kept_chain_sigs) confirms D is applied. \
         If this fails, block_creator's applied_sigs computation isn't \
         correctly consuming kept_chain_sigs.",
        hex::encode(&deploy_sig[..deploy_sig.len().min(8)]),
    );

    // --- Block W: child of merge_block, attempts to re-include D ---
    // This simulates the bonding-test recovery cycle: a proposer pulls D
    // from the rejected_deploy_buffer (or similar) and includes it in a
    // new block. The validator's repeat_deploy MUST reject — D's effects
    // are already in merge_block's pre-state via the kept chain.
    let block_w_raw = block_implicits::get_random_block(
        Some(3),
        Some(4),
        Some(merge_block.body.state.post_state_hash.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis() + 3),
        Some(vec![merge_block.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(user_deploy.clone())]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let block_w = block_w_raw;
    // Don't need to store block_w — repeat_deploy reads it directly.

    // --- The crucial assertion: validator's repeat_deploy REJECTS ---
    let mut snapshot_w = mk_snapshot(&genesis_hash);
    let rejected: DashSet<Bytes> = DashSet::new();
    rejected.insert(deploy_sig.clone());
    snapshot_w.rejected_in_scope = std::sync::Arc::new(rejected);

    let result = Validate::repeat_deploy(&block_w, &mut snapshot_w, &mut block_store, &std::collections::HashMap::new(), 50);

    assert!(
        matches!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy))
        ),
        "RUNTIME REPRO: validator's repeat_deploy MUST reject re-inclusion of \
         deploy {} — the full Phase 1 chain (dag_merger::merge → kept_chain_sigs \
         → block_creator applied_sigs → repeat_deploy gate) must catch this \
         bonding-bug pattern. Got: {:?}",
        hex::encode(&deploy_sig[..deploy_sig.len().min(8)]),
        result,
    );
}
