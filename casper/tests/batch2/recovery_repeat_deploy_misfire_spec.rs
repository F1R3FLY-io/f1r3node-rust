// Tests covering the rejected-deploy-buffer recovery exemption:
//
//   - Validator side (`Validate::repeat_deploy`) computes the verdict purely
//     from the block's own ancestry: a prior inclusion makes re-inclusion a
//     REPEAT (invalid) unless a later-or-equal ancestor records the sig in
//     `body.rejected_deploys` — the legal recovery re-proposal. Rejection
//     records in a valid ancestry are themselves consensus-validated (the
//     InvalidRejectedDeploy equality check at the recording block), so they
//     are trustworthy inputs; node-local views (rejected_in_scope, local
//     finalization status) are NOT consulted — they split verdicts across
//     nodes with different attach times.
//
//   - Proposer side (`prepare_user_deploys`) declines recovery for deploys
//     already resolved in its canonical view, so it does not gossip blocks
//     that waste proposal slots.

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::util::construct_deploy;
use casper::rust::validate::Validate;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_rejects_reinclusion_without_ancestry_rejection() {
    crate::init_logger();

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();

        // Genesis carries D; no ancestor ever records D rejected.
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

        let block_n = create_block(
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

        // Re-inclusion of D with no rejection record anywhere in the ancestry:
        // a plain double-execution attempt.
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
        let runtime_manager =
            crate::util::rholang::resources::mk_runtime_manager("validate-repeat-", None).await;

        let result = Validate::repeat_deploy(
            &block_w,
            &mut snapshot,
            &mut block_store,
            &runtime_manager,
            50,
        )
        .await;

        assert!(
            matches!(
                result,
                Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy))
            ),
            "expected InvalidRepeatDeploy (prior inclusion with no rejection record in \
             the ancestry is double-execution), got {:?}",
            result
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_exempts_reinclusion_after_ancestry_rejection_record() {
    crate::init_logger();

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let deploy_sig: Bytes = deploy.deploy.sig.clone();

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

        // D's original inclusion.
        let block_i = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            None,
            None,
            None,
            Some(vec![deploy.clone()]),
            None,
            None,
            None,
            None,
            None,
        );

        // A later ancestor records D rejected (the consensus-validated record a
        // merging block writes when cost-optimal resolution drops D's chain).
        let mut block_n = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![block_i.block_hash.clone()],
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
            // The rejection blames D's inclusion in block_i (the rejected chain's
            // source block), so the per-inclusion guard sees block_i as not live →
            // re-proposal in block_w is the legal recovery path.
            host: block_i.block_hash.clone(),
        }];
        block_store
            .put(block_n.block_hash.clone(), &block_n)
            .unwrap();

        // Recovery block: parent=block_n, body.deploys=[D]. The latest
        // inclusion (block_i, #1) is covered by the rejection record
        // (block_n, #2), so re-proposal is the legal recovery path.
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
        let runtime_manager =
            crate::util::rholang::resources::mk_runtime_manager("validate-repeat-", None).await;

        let result = Validate::repeat_deploy(
            &block_w,
            &mut snapshot,
            &mut block_store,
            &runtime_manager,
            50,
        )
        .await;

        assert!(
            matches!(result, Either::Right(_)),
            "expected Valid (latest inclusion was rejected by a later ancestor; \
             re-proposal is the recovery path), got {:?}",
            result
        );
    })
    .await
}

/// Proposer-side recovery gate after the buffer-drain change.
///
/// The proposer no longer applies a canonical-state / liveness filter to
/// recovered deploys. The recovery buffer holds only merge losers —
/// `handle_valid_block` purges a deploy on block acceptance and the merge
/// re-adds it on rejection — so a buffered deploy is by construction NOT in
/// the execution base, and re-executing it can never be the content-twin.
/// The only remaining recovery gate is single-owner: `prepare_user_deploys`
/// ADMITS an owned recovered deploy and SKIPS one this validator does not own
/// (so every node holding the rejected sig does not re-propose it concurrently).
/// `Validate::repeat_deploy` is the consensus backstop if a stale entry ever
/// slips through.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prepare_user_deploys_admits_owned_recovered_and_skips_non_owned() {
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

        // Genesis carries D, so D's indexed inclusion is genesis and its owner
        // (for the single-owner gate) is genesis.sender.
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

        // D sits in the recovery buffer — a recovered (merge-rejected) candidate.
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

        let now_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        // Owned: self == the deploy's owner (genesis.sender) → admitted.
        let owned = block_creator::prepare_user_deploys(
            &snapshot,
            10,
            now_millis,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            Some(&genesis.sender),
        )
        .await
        .expect("prepare_user_deploys should not error");
        assert!(
            owned.deploys.iter().any(|d| d.sig == deploy_sig),
            "prepare_user_deploys must ADMIT an owned recovered deploy: the buffer holds only \
             merge losers (kept clean by the accept-time purge in handle_valid_block), so the \
             proposer applies no canonical-state filter. Included: {:?}",
            owned
                .deploys
                .iter()
                .map(|d| hex::encode(&d.sig))
                .collect::<Vec<_>>(),
        );

        // Non-owned: self != the deploy's owner → skipped (single-owner recovery).
        // (This fixture's genesis carries an empty sender, so the owner is empty;
        // any non-empty key is a non-owner.)
        let other_validator = Bytes::from(vec![0xEEu8; 32]);
        assert_ne!(
            other_validator, genesis.sender,
            "fixture sanity: the non-owner validator must differ from genesis.sender"
        );
        let non_owned = block_creator::prepare_user_deploys(
            &snapshot,
            10,
            now_millis,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            Some(&other_validator),
        )
        .await
        .expect("prepare_user_deploys should not error");
        assert!(
            !non_owned.deploys.iter().any(|d| d.sig == deploy_sig),
            "prepare_user_deploys must SKIP a recovered deploy this validator does not own \
             (single-owner recovery prevents duplicate-conflict storms). Included: {:?}",
            non_owned
                .deploys
                .iter()
                .map(|d| hex::encode(&d.sig))
                .collect::<Vec<_>>(),
        );
    })
    .await
}
