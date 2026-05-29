// Reproduction of the canonical-sibling recovery misfire observed in
// `test_bonding_validators` attempt 11 (2026-05-29, session 9319132c).
//
// ## Production failure shape (validator3.log around 02:48:36)
//
// The V4 bond deploy `3045022100e3776605f3e723` (sig D) landed cleanly in
// block#2 across the shard. Multiple validators produced parallel block#2
// candidates that all included D — two of these blocks are relevant:
//
//   - X = hash `77787a2019cbe07f...` — the consensus-canonical block#2
//     (V3's local LFB at the time of block#6 prep is height 2 with this hash)
//   - Y = hash `6960907a0090cdd0...` — V3's local block#2 SIBLING of X.
//     Both X and Y have D in body; both have identical post_state (deploys
//     execute deterministically).
//
// As V3 extends its own chain (Y → block#3 → block#4 → block#5), each of
// those descendants of Y reports D in `body.rejected_deploys` (a downstream
// merge that observed multiple parallel inclusions resolved by rejecting D
// from THAT chain's contribution).
//
// At block#6 prep, the resolver scans D from V3's parents:
//   `[TRACE-RESOLVE-AT-PARENTS] sig=3045022100e3776605f3e723 ...
//      lfb_block_number=2 cleans_detail=[(2,6960907a0090cdd0)]
//      rejects_detail=[(3,...),(4,...),(5,...)]
//      clean_check=[(6960907a0090cdd0,parent_main=true,lfb_main=false)]
//      reject_check=[(2f5b5e02...,parent_main=true,lfb_main=false), ...]
//      state=RejectedCanonically reason=unfinalized-clean+rejection`
//
// The classification fires because:
//   - clean_block Y is in V3's parent main chain (`parent_main=true`)
//   - clean_block Y is NOT in LFB's main chain (`lfb_main=false`) — Y is a
//     SIBLING of canonical X at the same height
//   - rejects nonempty + has_finalized_clean=false → `RejectedCanonically`
//
// → Filter 1 admits D as `admit-back-rejected-not-stale`
// → D goes into block#6's body
// → compute_parents_post_state's merge base inherits LFB (= X) state
// → X's post_state already has D's writes (because X bonded V4 same as Y)
// → D re-executes against state that has D's prior writes → multi-Datum
// → `BUG FOUND: purse deposit failed` (CONTRACT-POS-CHARGEDEPLOY)
//
// The resolver's bug: it checks whether the SPECIFIC clean_block (Y) is in
// LFB's main chain, but the CANONICAL sibling (X) at the same height ALSO
// has D applied. D's effects ARE in canonical state via X's body. Recovery
// should be DENIED, not admitted.
//
// ## What the fix must do (preserves recovery)
//
// Resolver must classify D as `InCanonicalState` whenever the canonical
// chain (any LFB ancestor) has D in its applied_sigs — not only when the
// specific found clean_block is in LFB's main chain. Two equivalent shapes:
//
//   (a) Walk LFB's main chain to clean_block's height; if THAT block's
//       applied_sigs has D → InCanonicalState.
//   (b) Check the union of all LFB-ancestors' applied_sigs for D directly
//       (equivalently: check LFB.applied_sigs since that aggregates).
//
// This is metadata-only. Recovery proceeds normally when D is in a
// genuinely-orphaned chain whose canonical sibling at the same height did
// NOT include D (the recovery_cycle_spec / recovery_repeat_deploy_misfire
// shapes).
//
// ## Status
//
// Test is RED today. `prepare_user_deploys` admits D into prepared.deploys
// because the resolver classifies as `RejectedCanonically`. After the fix
// the resolver classifies as `InCanonicalState` and Filter 1 declines the
// exemption, leaving D in the buffer (correctly — D is already applied via
// canonical chain).

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

/// RED — fails today, passes after canonical-content resolver fix.
///
/// The resolver currently misclassifies a sig as `RejectedCanonically`
/// when the sig's clean_block is a sibling of LFB at the same height,
/// even though the canonical block (LFB) at that height ALSO contains
/// the sig. The fix must recognize D-via-canonical-sibling as
/// `InCanonicalState` and decline the recovery exemption.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proposer_must_skip_recovery_when_canonical_sibling_has_d() {
    crate::init_logger();

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let processed_deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let signed_deploy = processed_deploy.deploy.clone();
        let deploy_sig: Bytes = signed_deploy.sig.clone();

        // Genesis — no D (so the LFB advance later makes a difference)
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None, None, None, None, None, None, None, None,
        );

        // Block X — canonical block#1, parent=G, body=[D].
        // create_block runs populate_applied_sigs which puts D in
        // X.body.state.applied_sigs.
        let block_x = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None, None, None,
            Some(vec![processed_deploy.clone()]),
            None, None, None, None, None,
        );
        assert!(
            block_x.body.state.applied_sigs.contains_key(&deploy_sig),
            "PRECONDITION: block_x must have D in applied_sigs (it's in body)"
        );

        // Block Y — SIBLING of X at the same height, parent=G, body=[D].
        // Same content as X (both bond V4); their post_state hashes might
        // coincide via deterministic execution in production, but for this
        // test all that matters is the DAG shape.
        let block_y = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None, None, None,
            Some(vec![processed_deploy.clone()]),
            None, None, None, None, None,
        );
        assert!(
            block_y.body.state.applied_sigs.contains_key(&deploy_sig),
            "PRECONDITION: block_y (sibling of X) must have D in applied_sigs"
        );
        assert_ne!(
            block_x.block_hash, block_y.block_hash,
            "PRECONDITION: X and Y must have distinct block hashes"
        );

        // Block_n — descendant of Y that REJECTS D in its body.rejected_deploys.
        // This is the source of D in rejected_in_scope from the resolver's
        // perspective (it walks the parents' ancestry and finds this rejection).
        let mut block_n = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![block_y.block_hash.clone()],
            &genesis,
            None, None, None,
            None, // no body deploys
            None, None, None, None, None,
        );
        block_n.body.rejected_deploys = vec![RejectedDeploy {
            sig: deploy_sig.clone(),
        }];
        // Re-populate block_n.applied_sigs after the rejected_deploys
        // mutation — `create_block` ran populate_applied_sigs with empty
        // rejected_deploys, so block_n.applied_sigs would otherwise still
        // include D (inherited from Y). The Phase 1 merge subtracts
        // rejected_deploys, so the canonical block_n state has D removed.
        // Without this re-populate, Filter 2 (proposer's parents.applied_sigs
        // check) would drop D for the wrong reason and mask the bug.
        crate::helper::block_generator::populate_applied_sigs(&mut block_n, &block_store);
        assert!(
            !block_n.body.state.applied_sigs.contains_key(&deploy_sig),
            "PRECONDITION: block_n.applied_sigs must NOT contain D after the \
             rejected_deploys re-populate; this is what makes the resolver's \
             canonicality check the load-bearing gate"
        );
        block_store
            .put(block_n.block_hash.clone(), &block_n)
            .unwrap();

        // FINALIZE X as the canonical LFB (NOT Y). This makes Y a sibling
        // of LFB and reproduces the production check `lfb_main=false` for
        // Y in the resolver's clean_check trace.
        block_dag_storage
            .record_directly_finalized(block_x.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .expect("record_directly_finalized(block_x)");

        // Verify the LFB advance took effect.
        assert_eq!(
            block_dag_storage.get_representation().last_finalized_block(),
            block_x.block_hash,
            "PRECONDITION: LFB must be X (not Y, not genesis)"
        );

        // Set up rejected_deploy_buffer with D — this is the recovery
        // candidate that Filter 1 would re-admit via the
        // `admit-back-rejected-not-stale` path.
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

        // Snapshot — build on block_n (V3's local chain). The resolver
        // scans clean blocks from this parent's main chain and finds D in
        // block_y at height 1.
        let dag = block_dag_storage.get_representation();
        let mut snapshot = mk_casper_snapshot(dag);
        snapshot.last_finalized_block = block_dag_storage
            .get_representation()
            .last_finalized_block();
        snapshot.parents = vec![block_n.clone()];
        // Trigger shape from production: D appears in BOTH scope sets.
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
            "CANONICAL-SIBLING RESOLVER MISFIRE: prepare_user_deploys must \
             skip D — the canonical chain (LFB=X) at height 1 has D in \
             applied_sigs, so D's effects ARE in canonical state. The \
             current resolver mis-classifies D as RejectedCanonically \
             because it checks whether the specific clean_block (Y) is in \
             LFB's main chain (Y is a sibling, not in chain), missing that \
             the canonical block (X) at Y's height also has D. Re-including \
             D leads to double-execution against state that has D's writes \
             via the LFB base → multi-Datum → BUG FOUND. \n\
             Included sigs: {:?}\n\
             D's sig:       {}",
            included_sigs,
            hex::encode(&deploy_sig),
        );
    })
    .await
}
