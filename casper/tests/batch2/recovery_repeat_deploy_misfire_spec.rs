// Tests covering the rejected-deploy-buffer recovery exemption:
//
//   - Validator side (`Validate::repeat_deploy`): the exemption is a PURE
//     FUNCTION OF THE BLOCK — a sig whose latest canonical disposition in
//     the block's own parent scope is a merge rejection is legal recovery.
//     An earlier version gated the exemption on the validating node's LOCAL
//     finalization status, which forked the network when two honest nodes'
//     finalization progress differed (roaming InvalidRepeatDeploy Heavy
//     Pipeline failures). Double-execution defense is layered:
//       * a win never rejected in scope keeps the sig in the check set and
//         the ancestor scan flags the repeat (deterministic);
//       * a FABRICATED rejection record (naming a floor-protected deploy an
//         honest merge could never reject) invalidates the fabricating
//         block itself via the rejected-list equality check in
//         `validate_block_checkpoint` (`InvalidRejectedDeploy`), so no
//         descendant can build on it.
//
//   - Proposer side (`prepare_user_deploys`) MUST decline the exemption
//     when the deploy's effects are already canonical, otherwise it gossips
//     a recovery block that downstream validators flag — leading to
//     mutual-slashing on FTT=0 shards.

use std::sync::Arc;

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::finality::floor_context::FloorContext;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy;
use casper::rust::validate::Validate;
use dashmap::DashSet;
use models::rust::casper::protocol::casper_message::{RejectedDeploy, RejectedDeployReason};
use models::rust::deploy_id::DeployLookupId;
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
        casper_version: casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
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
        bond_generations: HashMap::new(),
        active_validators: vec![],
    };

    let mut snapshot = CasperSnapshot::new(dag);
    snapshot.on_chain_state = on_chain_state;
    snapshot
}

/// A forged rejection above the finalized floor cannot override a finalized
/// win. The ordinary repeat check rejects the duplicate occurrence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finalized_win_dominates_parent_rejection_for_repeat_check() {
    crate::init_logger();

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let deploy_sig = Bytes::copy_from_slice(deploy.deploy_id());

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
        block_n.body.rejected_deploys = vec![RejectedDeploy::occurrence_v6(
            deploy.deploy_id_v6().expect("protocol-v6 deploy identity"),
            genesis.block_hash.clone(),
            RejectedDeployReason::MergeConflict,
        )];
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

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut snapshot = mk_casper_snapshot(dag);

        let rejected: DashSet<DeployLookupId> = DashSet::new();
        rejected.insert(crate::current_deploy_id(&deploy_sig));
        snapshot.rejected_in_scope = Arc::new(rejected);

        let latest_messages = block_w
            .justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect();
        let floor_context = FloorContext::derive(
            &snapshot.dag,
            &block_store,
            &block_w.header.parents_hash_list,
            &latest_messages,
            FtThreshold::from_ppm(0),
            block_w.header.version,
        )
        .await
        .expect("derive finalized floor");
        assert_eq!(floor_context.floor.hash, genesis.block_hash);
        assert!(
            !floor_context
                .rejected_sigs(&block_store, 0)
                .expect("derive floor-relative rejected deploys")
                .contains(&crate::current_deploy_id(&deploy_sig)),
            "the finalized winning source must dominate the forged visible rejection"
        );

        let result = Validate::repeat_deploy_at_floor(
            &block_w,
            &mut snapshot,
            &block_store,
            50,
            Some(&floor_context),
        );

        assert_eq!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy)),
            "the finalized win must retain the ordinary repeat-deploy rejection; got {:?}",
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
        let deploy_sig = Bytes::copy_from_slice(processed_deploy.deploy_id());

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
        let deploy_storage = std::sync::Arc::new(parking_lot::Mutex::new(
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
            buf.add(vec![crate::pending_envelope(
                processed_deploy
                    .to_cosigned()
                    .expect("protocol-v6 deploy envelope"),
            )])
            .expect("Failed to add deploy to buffer");
        }

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut snapshot = mk_casper_snapshot(dag);
        snapshot.last_finalized_block = block_dag_storage
            .get_representation()
            .expect("dag representation")
            .last_finalized_block();
        // A proposer always builds on at least one parent; the canonical-won
        // record scan walks the main-parent chain from here and must see D's
        // win in genesis.
        snapshot.parents = vec![genesis.clone()];
        snapshot
            .deploys_in_scope
            .insert(crate::current_deploy_id(&deploy_sig));
        snapshot
            .rejected_in_scope
            .insert(crate::current_deploy_id(&deploy_sig));

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
            true,
            true,
        )
        .await
        .expect("prepare_user_deploys should not error");

        let included_sigs: Vec<String> = prepared
            .deploys
            .iter()
            .map(|d| hex::encode(d.deploy_id()))
            .collect();

        assert!(
            !prepared
                .deploys
                .iter()
                .any(|d| d.deploy_id() == &deploy_sig),
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

/// Determinism regression (InvalidRepeatDeploy fork, 2026-07-15): the SAME
/// block must receive the SAME repeat_deploy verdict from validators whose
/// node-local views differ. The pre-fix exemption read the validating node's
/// `rejected_in_scope` snapshot set and the sig's LOCAL finalization status:
/// a node whose scope set (or finality progress) differed from its peers
/// returned `InvalidRepeatDeploy` for a recovery block the rest accepted,
/// permanently forking that node (Recording invalid block →
/// UnknownRootError cascade — the roaming Heavy Pipeline failures that
/// survived the protocol-FTT fix). The deterministic exemption reads only
/// the block's own parent scope, so the two divergent snapshots below must
/// agree — and agree on Valid (the on-chain rejection record makes the
/// re-inclusion legal recovery).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_deploy_verdict_is_identical_across_divergent_local_views() {
    use dashmap::DashSet;
    use rspace_plus_plus::rspace::history::Either as RE;

    crate::init_logger();

    with_storage(|mut block_store, mut block_dag_storage| async move {
        let deploy = construct_deploy::basic_processed_deploy(0, None).unwrap();
        let deploy_sig = Bytes::copy_from_slice(deploy.deploy_id());

        // Recovery shape with the on-chain disposition record: genesis →
        // block_x (D) → block_m (rejected_deploys=[D]) → block_w (D again).
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
        let block_x = create_block(
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
        let mut block_m = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![block_x.block_hash.clone()],
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
        block_m.body.rejected_deploys = vec![RejectedDeploy::occurrence_v6(
            deploy.deploy_id_v6().expect("protocol-v6 deploy identity"),
            block_x.block_hash.clone(),
            RejectedDeployReason::MergeConflict,
        )];
        block_store
            .put(block_m.block_hash.clone(), &block_m)
            .unwrap();
        let block_w = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![block_m.block_hash.clone()],
            &genesis,
            None,
            None,
            Some(std::collections::HashMap::from([(
                block_m.sender.clone(),
                block_m.block_hash.clone(),
            )])),
            Some(vec![deploy]),
            None,
            None,
            None,
            None,
            None,
        );

        // Validator A: its recovery pipeline has the sig in rejected_in_scope
        // (the view the pre-fix exemption keyed on).
        let dag_a = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut snapshot_a = mk_casper_snapshot(dag_a);
        let rejected: DashSet<DeployLookupId> = DashSet::new();
        rejected.insert(crate::current_deploy_id(&deploy_sig));
        snapshot_a.rejected_in_scope = Arc::new(rejected);

        // Validator B: same chain data, but its live view never surfaced the
        // sig in rejected_in_scope (fresh restart / different snapshot
        // timing). Pre-fix this node alone flagged InvalidRepeatDeploy.
        let dag_b = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut snapshot_b = mk_casper_snapshot(dag_b);

        let latest_messages = block_w
            .justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect();
        let floor_context = FloorContext::derive(
            &snapshot_a.dag,
            &block_store,
            &block_w.header.parents_hash_list,
            &latest_messages,
            FtThreshold::from_ppm(0),
            block_w.header.version,
        )
        .await
        .expect("certified recovery floor");
        assert_eq!(floor_context.floor.hash, block_m.block_hash);

        let verdict_a = Validate::repeat_deploy_at_floor(
            &block_w,
            &mut snapshot_a,
            &block_store,
            50,
            Some(&floor_context),
        );
        let verdict_b = Validate::repeat_deploy_at_floor(
            &block_w,
            &mut snapshot_b,
            &block_store,
            50,
            Some(&floor_context),
        );

        assert_eq!(
            verdict_a, verdict_b,
            "repeat_deploy must be a pure function of the block: divergent \
             node-local views returned different verdicts (fork)"
        );
        assert_eq!(
            verdict_a,
            RE::Right(casper::rust::block_status::ValidBlock::Valid),
            "the on-chain rejection record in block_m makes the re-inclusion \
             legal recovery on every node"
        );
    })
    .await
}
