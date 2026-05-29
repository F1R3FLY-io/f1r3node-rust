// Reproduction of the second residual recovery misfire observed in
// `test_bonding_validators` attempt 12 (2026-05-29, session dca628c8).
//
// ## Production failure shape (validator1.log around 03:40:22)
//
// A bg_load liveness deploy `304502210087edaf` (sig D) landed in block #57
// across the shard. Multiple validators observed this and produced parallel
// height-58 blocks declaring D in `body.rejected_deploys`. The rejection
// cascade continued at height 59. V1's block#60 prep at 03:40:22 ran with
// LFB still at height 55 (LFB had not yet advanced past D's clean
// inclusion).
//
// The resolver trace at block#60 prep:
//   [TRACE-RESOLVE-AT-PARENTS] sig=304502210087edaf...
//      parents_count=5 cleans=1 rejects=4
//      lfb_block_number=55 in_lfb_applied_sigs=false
//      cleans_detail=[(57,2e8e698c9be96ab0)]
//      rejects_detail=[(58,83bbc1e8abda9993),(58,1d9c563205fcbb8e),
//                      (59,d49724c8c536ad7a),(59,092a08f575cc2648)]
//      clean_check=[(2e8e698c9be96ab0,parent_main=true,lfb_main=false)]
//      reject_check=[(83bbc1e8abda9993,parent_main=true,lfb_main=false),
//                    (1d9c563205fcbb8e,parent_main=true,lfb_main=false),
//                    (d49724c8c536ad7a,parent_main=true,lfb_main=false),
//                    (092a08f575cc2648,parent_main=true,lfb_main=false)]
//      has_canonical_rejection=true
//      state=RejectedCanonically reason=unfinalized-clean+rejection
//
// → Filter 1 admits D as `admit-back-rejected-not-stale`
// → D goes into block#60's body
// → compute_parents_post_state's merge base STILL contains D's writes
//   (block_57 was locally compute_stat'd, the descendant rejections are
//    metadata-only — Phase 1 subtracts D from applied_sigs but the trie
//    state inherits D via the merge base path)
// → D re-executes against state containing D's prior writes → multi-Datum
// → `BUG FOUND: purse deposit failed`
//
// ## How this differs from canonical_sibling_recovery_misfire_spec
//
// In the canonical-sibling case (attempt 11 V3 block#6):
//   - LFB was AT D's height (block#2 = LFB)
//   - LFB.applied_sigs DID contain D
//   - The previous fix's `in_lfb_applied_sigs` check catches it
//
// In THIS case (attempt 12 V1 block#60):
//   - LFB is BELOW D's height (#55 < #57)
//   - LFB.applied_sigs does NOT yet contain D
//   - The `in_lfb_applied_sigs` check correctly fires false → no help
//   - But D's effects ARE in the merge base (block_57 was locally
//     compute_stat'd; the trie inherits D's writes through the merge
//     base inheritance path)
//
// The resolver currently classifies this as `RejectedCanonically` because:
//   - clean_block is parent_main but not lfb_main (LFB hasn't caught up)
//   - rejections exist in parent_main
//   - has_finalized_clean=false (LFB doesn't have D yet)
//   - Falls into the "unfinalized-clean+rejection" branch
//
// ## What the fix must do (preserves recovery)
//
// When D's clean inclusion is in parent's main chain AND the rejections
// are descendants of that clean inclusion (downstream rollback), D's
// effects ARE in the merged pre-state via merge-base inheritance even
// though the metadata layer says "rejected". Recovery via re-execution
// is unsafe. Classify as `InCanonicalState`.
//
// Concretely: if clean_block has `parent_main=true` AND clean's height <
// every reject's height (i.e., clean predates every rejection in parent
// chain), then D's effects are in pre-state. Decline the exemption.
//
// This preserves recovery for legitimate cases:
//   - `recovery_cycle_spec`: rejection happens in a sibling chain, not
//     in parent_main; clean predates rejection in PARALLEL chain →
//     not this rule's trigger.
//   - `proposer_must_skip_recovery_when_deploy_is_in_pending_canonical_
//     ancestor`: no rejection at all → already handled by "no-rejection"
//     branch.
//
// ## Status
//
// RED today. After option-1 fix (clean-predates-rejection-in-parent-main →
// InCanonicalState), should go GREEN.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{SystemTime, UNIX_EPOCH};

use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use casper::rust::blocks::proposer::block_creator;
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::RejectedDeploy;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{create_block, create_genesis_block};

fn mk_casper_snapshot(
    dag: block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
) -> casper::rust::casper::CasperSnapshot {
    use std::collections::HashMap;

    use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};

    let shard_conf = CasperShardConf {
        fault_tolerance_threshold: 0.0,
        shard_name: "root".to_string(),
        parent_shard_id: "".to_string(),
        finalization_rate: 0,
        max_number_of_parents: 10,
        max_parent_depth: 0,
        synchrony_constraint_threshold: 0.0,
        height_constraint_threshold: 0,
        deploy_lifespan: 50,
        casper_version: 1,
        config_version: 1,
        bond_minimum: 0,
        bond_maximum: i64::MAX,
        epoch_length: 0,
        quarantine_length: 0,
        min_phlo_price: 0,
        enable_mergeable_channel_gc: false,
        mergeable_channels_gc_depth_buffer: 10,
        disable_late_block_filtering: false,
        disable_validator_progress_check: false,
        ..CasperShardConf::new()
    };

    let on_chain_state = OnChainCasperState {
        shard_conf,
        bonds_map: HashMap::new(),
        active_validators: vec![],
    };

    let mut snapshot = CasperSnapshot::new(dag);
    snapshot.on_chain_state = on_chain_state;
    snapshot
}

/// RED — fails today, passes after the clean-predates-rejection-in-parent-main
/// rule is added to `resolve_at_parents_batch`.
///
/// Mirrors the V1 attempt 12 block#60 trace EXACTLY: 5 parents,
/// 1 clean inclusion at a height above LFB, 4 descendant rejections (2 at
/// the height immediately above clean, 2 at clean+2), all in parent's
/// main chain. LFB stays at genesis (below clean's height) so the
/// previous `in_lfb_applied_sigs` fix correctly doesn't trigger.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proposer_must_skip_recovery_when_clean_in_parent_main_predates_rejection() {
    crate::init_logger();

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let processed_deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let signed_deploy = processed_deploy.deploy.clone();
        let deploy_sig: Bytes = signed_deploy.sig.clone();

        // Genesis = LFB. NO D in genesis so the previous fix's
        // in_lfb_applied_sigs check correctly fires false.
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None, None, None, None, None, None, None, None,
        );

        // block_clean (mirrors production block 2e8e698c at height 57) —
        // parent=genesis, body=[D].
        let block_clean = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None, None, None,
            Some(vec![processed_deploy.clone()]),
            None, None, None, None, None,
        );
        assert!(
            block_clean.body.state.applied_sigs.contains_key(&deploy_sig),
            "PRECONDITION: block_clean must have D in applied_sigs (it's in body)"
        );

        // Helper to build a rejecter block at the given parent, with
        // body.rejected_deploys=[D], then re-populate applied_sigs so the
        // Phase 1 subtraction is honored (otherwise Filter 2 would mask
        // the bug via parents.applied_sigs).
        let build_rejecter = |block_store: &mut block_storage::rust::key_value_block_store::KeyValueBlockStore,
                              block_dag_storage: &mut block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage,
                              parent_hash: prost::bytes::Bytes,
                              genesis: &models::rust::casper::protocol::casper_message::BlockMessage,
                              sig: &Bytes|
         -> models::rust::casper::protocol::casper_message::BlockMessage {
            let mut b = create_block(
                block_store,
                block_dag_storage,
                vec![parent_hash],
                genesis,
                None, None, None,
                None, // no body deploys
                None, None, None, None, None,
            );
            b.body.rejected_deploys = vec![RejectedDeploy { sig: sig.clone() }];
            // Re-populate applied_sigs: Phase 1's merge subtracts
            // this_merge_rejected_deploys, so the canonical post-state has
            // D removed at the METADATA level (the trie state inheritance
            // is the actual bug we're documenting here).
            crate::helper::block_generator::populate_applied_sigs(&mut b, block_store);
            assert!(
                !b.body.state.applied_sigs.contains_key(sig),
                "rejecter block's applied_sigs must NOT contain D after \
                 the rejected_deploys re-populate — Phase 1's metadata \
                 subtraction is honored; the trie inheritance is the gap \
                 that motivates this test"
            );
            block_store.put(b.block_hash.clone(), &b).unwrap();
            b
        };

        // Two parallel rejecter blocks at clean+1 (mirrors production
        // 83bbc1e8 and 1d9c5632 at height 58, both with parent=block 57).
        let block_reject_a = build_rejecter(
            &mut block_store,
            &mut block_dag_storage,
            block_clean.block_hash.clone(),
            &genesis,
            &deploy_sig,
        );
        let block_reject_b = build_rejecter(
            &mut block_store,
            &mut block_dag_storage,
            block_clean.block_hash.clone(),
            &genesis,
            &deploy_sig,
        );
        assert_ne!(
            block_reject_a.block_hash, block_reject_b.block_hash,
            "PRECONDITION: rejecter siblings at clean+1 must be distinct"
        );

        // Two more rejecter blocks at clean+2, each descending from one of
        // the clean+1 rejecters (mirrors production d49724c8 and 092a08f5
        // at height 59).
        let block_reject_c = build_rejecter(
            &mut block_store,
            &mut block_dag_storage,
            block_reject_a.block_hash.clone(),
            &genesis,
            &deploy_sig,
        );
        let block_reject_d = build_rejecter(
            &mut block_store,
            &mut block_dag_storage,
            block_reject_b.block_hash.clone(),
            &genesis,
            &deploy_sig,
        );

        // Set up rejected_deploy_buffer with D — Filter 1's
        // `admit-back-rejected-not-stale` path picks up from here.
        let mut aux_kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(StdMutex::new(
            KeyValueDeployStorage::new(&mut aux_kvm)
                .await
                .expect("create deploy_storage"),
        ));
        let rejected_deploy_buffer = Arc::new(StdMutex::new(
            KeyValueRejectedDeployBuffer::new(&mut aux_kvm)
                .await
                .expect("create rejected_deploy_buffer"),
        ));
        {
            let mut buf = rejected_deploy_buffer.lock().unwrap();
            buf.add(vec![signed_deploy.clone()])
                .expect("add D to buffer");
        }

        // Snapshot — 5 parents matching production (`parents_count=5`):
        // block_clean + 2 height+1 rejecters + 2 height+2 rejecters.
        // effective_parent_indices reduces this to [block_reject_c,
        // block_reject_d] (antichain), neither of which has D in
        // applied_sigs — so Filter 2 misses D and the resolver decides.
        let dag = block_dag_storage.get_representation();
        let mut snapshot = mk_casper_snapshot(dag);
        snapshot.last_finalized_block = block_dag_storage
            .get_representation()
            .last_finalized_block();
        snapshot.parents = vec![
            block_clean.clone(),
            block_reject_a.clone(),
            block_reject_b.clone(),
            block_reject_c.clone(),
            block_reject_d.clone(),
        ];
        assert_eq!(
            snapshot.parents.len(),
            5,
            "PRECONDITION: must have parents_count=5 matching production trace"
        );
        snapshot.deploys_in_scope.insert(deploy_sig.clone());
        snapshot.rejected_in_scope.insert(deploy_sig.clone());

        let now_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let prepared = block_creator::prepare_user_deploys(
            &snapshot,
            10,
            now_millis,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            &block_store,
        )
        .await
        .expect("prepare_user_deploys should not error");

        let included_sigs: Vec<String> = prepared
            .deploys
            .iter()
            .map(|d| hex::encode(&d.sig))
            .collect();

        assert!(
            !prepared.deploys.iter().any(|d| d.sig == deploy_sig),
            "PENDING-CANONICAL CLEAN PREDATES REJECTION MISFIRE: \
             prepare_user_deploys must skip D — D's clean inclusion at \
             height 1 is in parent's main chain (parent_main=true), and \
             every rejection at heights 2 and 3 happened AFTER (downstream \
             of) the clean inclusion. D's effects are in the merged \
             pre-state via the merge-base inheritance path, even though \
             Phase 1's metadata layer correctly subtracts D from applied_sigs. \
             Re-execution against the merge base hits state that already has \
             D's writes → multi-Datum → BUG FOUND. \n\
             The resolver currently mis-classifies as RejectedCanonically \
             because rejections exist in parent_main; the fix must \
             recognize that clean-predates-rejection-in-parent-main means \
             D's effects survive the metadata rollback at the trie layer.\n\
             Included sigs: {:?}\n\
             D's sig:       {}",
            included_sigs,
            hex::encode(&deploy_sig),
        );
    })
    .await
}
