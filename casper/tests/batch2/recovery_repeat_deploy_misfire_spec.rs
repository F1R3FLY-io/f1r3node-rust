// Tests covering the rejected-deploy-buffer recovery exemption:
//
//   - Validator side (`Validate::repeat_deploy`) MUST reject a recovery block
//     whose deploy is canonically Finalized via a different chain (the
//     rejection in `rejected_in_scope` came from a non-canonical sibling).
//     Re-executing such a deploy would be double-execution.
//
//   - Proposer side (`prepare_user_deploys`) MUST decline the exemption for
//     the same shape, otherwise it gossips a recovery block that downstream
//     validators correctly flag as `InvalidRepeatDeploy` — leading to
//     mutual-slashing on FTT=0 shards.

use std::sync::Arc;

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::util::construct_deploy;
use casper::rust::validate::Validate;
use dashmap::DashSet;
use models::rust::casper::protocol::casper_message::RejectedDeploy;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

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

#[ignore = "Phase 1 applied_sigs design: spurious-rejection defense moved out of \
repeat_deploy. The simplified repeat_deploy trusts body.rejected_deploys and \
subtracts it from applied_sigs unconditionally. A block claiming a spurious \
rejection (rejecting a sig that the merge didn't actually drop) is caught \
upstream by validate_block_checkpoint's InvalidRejectedDeploy check \
(computed-vs-claimed mismatch), not by repeat_deploy. This test was \
defense-in-depth against that scenario; the protection now lives at a \
different layer. See notes/applied-sigs-design.md §6."]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_correctly_rejects_stale_recovery_when_d_is_finalized() {
    crate::init_logger();

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let deploy_sig: Bytes = deploy.deploy.sig.clone();

        // Genesis (LFB) carries D — so D is canonically Finalized.
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![deploy.clone()]),
            None,
            None,
            None,
            None,
        );

        // Non-canonical sibling that declares D rejected. This is the
        // staleness shape: D's sig ends up in `rejected_in_scope` via the
        // ancestor scan, but the rejection itself is not canonical.
        let mut block_n = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        block_n.body.rejected_deploys = vec![RejectedDeploy {
            sig: deploy_sig.clone(),
        }];
        block_store
            .put(block_n.block_hash.clone(), &block_n)
            .unwrap();

        // Recovery block: parent=block_n, body.deploys=[D].
        let block_w = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![block_n.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            Some(vec![deploy]),
            None,
            None,
            None,
            None,
            None,
        );

        let dag = block_dag_storage.get_representation();
        let mut snapshot = mk_casper_snapshot(dag);

        let rejected: DashSet<Bytes> = DashSet::new();
        rejected.insert(deploy_sig.clone());
        snapshot.rejected_in_scope = Arc::new(rejected);

        let result = Validate::repeat_deploy(&block_w, &mut snapshot, &mut block_store, &std::collections::HashMap::new(), 50);

        assert!(
            matches!(
                result,
                Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy))
            ),
            "expected InvalidRepeatDeploy (D is canonically Finalized; rejection in \
             block_n is non-canonical so the exemption must decline), got {:?}",
            result
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proposer_must_skip_recovery_when_deploy_is_canonically_finalized() {
    use std::sync::Mutex as StdMutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
    use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
    use casper::rust::blocks::proposer::block_creator;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    crate::init_logger();

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let processed_deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let signed_deploy = processed_deploy.deploy.clone();
        let deploy_sig: Bytes = signed_deploy.sig.clone();

        // Genesis (LFB) carries D — so D is canonically Finalized.
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(vec![processed_deploy.clone()]),
            None,
            None,
            None,
            None,
        );

        let mut aux_kvm = InMemoryStoreManager::new();
        let deploy_storage = std::sync::Arc::new(StdMutex::new(
            KeyValueDeployStorage::new(&mut aux_kvm)
                .await
                .expect("Failed to create deploy storage"),
        ));
        let rejected_deploy_buffer = std::sync::Arc::new(StdMutex::new(
            KeyValueRejectedDeployBuffer::new(&mut aux_kvm)
                .await
                .expect("Failed to create rejected deploy buffer"),
        ));

        // D sits in the recovery buffer — the stale entry that the proposer
        // would otherwise re-include via the exemption path.
        {
            let mut buf = rejected_deploy_buffer.lock().unwrap();
            buf.add(vec![signed_deploy.clone()])
                .expect("Failed to add deploy to buffer");
        }

        let dag = block_dag_storage.get_representation();
        let mut snapshot = mk_casper_snapshot(dag);
        snapshot.last_finalized_block = block_dag_storage
            .get_representation()
            .last_finalized_block();
        // We're building on genesis — its body.deploys contains D, so D's
        // effects ARE in pre-state.
        snapshot.parents = vec![genesis.clone()];
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
            "prepare_user_deploys must skip a buffered deploy whose effects are \
             already in canonical state (re-including it would be double-execution \
             and the resulting block would be slashed by `repeat_deploy`).\n\
             Included: {:?}\nD's sig:  {}",
            included_sigs,
            hex::encode(&deploy_sig),
        );
    })
    .await
}

// Pending-canonical variant of the proposer-side test above.
//
// Shape: D is in a NON-finalized canonical ancestor (block_a, a descendant of
// genesis), AND in `rejected_in_scope` (via a sibling-merge rejection that
// the proposer's scope union picks up). D's effects ARE in the pre-state that
// the proposer will build on — re-including D is double-execution.
//
// The existing `Finalized` gate misses this case because resolve_batch returns
// Pending (block_a is not finalized), so stale_recoveries is empty and the
// exemption fires.
//
// Active guard for the Bug B fix: the recovery-exemption gate must anchor on
// canonical-from-our-parents inclusion (`resolve_at_parents`) rather than
// LFB-anchored `Finalized` state. Before the fix this test failed; the
// `Finalized`-only gate let block_a's pending inclusion slip through and the
// exemption fired, causing double-execution.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proposer_must_skip_recovery_when_deploy_is_in_pending_canonical_ancestor() {
    use std::sync::Mutex as StdMutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
    use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
    use casper::rust::blocks::proposer::block_creator;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    crate::init_logger();

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let processed_deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let signed_deploy = processed_deploy.deploy.clone();
        let deploy_sig: Bytes = signed_deploy.sig.clone();

        // Genesis carries NO deploys — D will be in a post-genesis block that
        // remains unfinalized (LFB = genesis throughout the test).
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        // block_a: D in body.deploys, descendant of genesis, NOT finalized.
        let block_a = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            Some(vec![processed_deploy.clone()]),
            None,
            None,
            None,
            None,
            None,
        );

        let mut aux_kvm = InMemoryStoreManager::new();
        let deploy_storage = std::sync::Arc::new(StdMutex::new(
            KeyValueDeployStorage::new(&mut aux_kvm)
                .await
                .expect("Failed to create deploy storage"),
        ));
        let rejected_deploy_buffer = std::sync::Arc::new(StdMutex::new(
            KeyValueRejectedDeployBuffer::new(&mut aux_kvm)
                .await
                .expect("Failed to create rejected deploy buffer"),
        ));

        // D sits in the recovery buffer — the stale entry the exemption would
        // re-propose.
        {
            let mut buf = rejected_deploy_buffer.lock().unwrap();
            buf.add(vec![signed_deploy.clone()])
                .expect("Failed to add deploy to buffer");
        }

        let dag = block_dag_storage.get_representation();
        let mut snapshot = mk_casper_snapshot(dag);
        snapshot.last_finalized_block = block_dag_storage
            .get_representation()
            .last_finalized_block();
        // We're building on block_a — its body.deploys contains D, so D's
        // effects ARE in pre-state. The parents-anchored resolver should
        // detect this.
        snapshot.parents = vec![block_a.clone()];
        // Trigger shape: D is in BOTH scope sets (accepted in pending
        // ancestor block_a; rejected somewhere upstream).
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
            "prepare_user_deploys must skip a buffered deploy whose sig is in \
             deploys_in_scope via a pending canonical ancestor — its effects are \
             in pre-state, so re-execution is double-execution.\n\
             Included: {:?}\nD's sig:  {}",
            included_sigs,
            hex::encode(&deploy_sig),
        );
    })
    .await
}
