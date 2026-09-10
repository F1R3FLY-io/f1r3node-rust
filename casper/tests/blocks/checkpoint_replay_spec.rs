use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::Casper;
use casper::rust::errors::CasperError;
use casper::rust::metrics_constants::USER_DEPLOY_EVALUATION_ATTEMPTS_METRIC;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::replay_cache::ReplayCache;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Cosigned;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, DeployAdmissionStatus, DeployData, Event, ProcessedDeploy, ProcessedSystemDeploy,
    ProduceEvent, SystemDeployData,
};
use models::rust::deploy_id::{DeployIdV6, DeployLookupId};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

static EXECUTION_METRICS: OnceLock<metrics_util::debugging::Snapshotter> = OnceLock::new();
static EXECUTION_METRICS_LOCK: Mutex<()> = Mutex::new(());

fn execution_metrics() -> metrics_util::debugging::Snapshotter {
    let _guard = EXECUTION_METRICS_LOCK.lock().unwrap();
    EXECUTION_METRICS
        .get_or_init(|| {
            let recorder = metrics_util::debugging::DebuggingRecorder::new();
            let snapshotter = recorder.snapshotter();
            metrics::set_global_recorder(recorder).expect("install execution metrics recorder");
            snapshotter
        })
        .clone()
}

#[allow(clippy::mutable_key_type)]
fn execution_count(snapshotter: &metrics_util::debugging::Snapshotter, origin: &str) -> u64 {
    snapshotter
        .snapshot()
        .into_hashmap()
        .iter()
        .find_map(|(key, (_, _, value))| {
            let key = format!("{key:?}");
            if key.contains(USER_DEPLOY_EVALUATION_ATTEMPTS_METRIC) && key.contains(origin) {
                match value {
                    metrics_util::debugging::DebugValue::Counter(count) => Some(*count),
                    _ => None,
                }
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn execution_evidence_mutations(block: &BlockMessage) -> Vec<(&'static str, BlockMessage)> {
    fn mutate(
        block: &BlockMessage,
        name: &'static str,
        change: impl FnOnce(&mut ProcessedDeploy),
    ) -> (&'static str, BlockMessage) {
        let mut forged = block.clone();
        change(&mut forged.body.deploys[0]);
        (name, forged)
    }

    vec![
        mutate(block, "envelope identity", |deploy| {
            deploy.envelope_commitment = vec![0x91; 32].into();
        }),
        mutate(block, "cost", |deploy| deploy.cost.cost += 1),
        mutate(block, "event log", |deploy| {
            deploy.deploy_log.push(Event::Produce(ProduceEvent {
                channels_hash: vec![1; 32].into(),
                hash: vec![2; 32].into(),
                persistent: false,
                times_repeated: 0,
                is_deterministic: true,
                output_value: vec![vec![3].into()],
                failed: false,
            }));
        }),
        mutate(block, "failure status", |deploy| {
            deploy.is_failed = !deploy.is_failed;
        }),
        mutate(block, "system error", |deploy| {
            deploy.system_deploy_error = Some("forged".to_string());
        }),
        mutate(block, "pre-state root", |deploy| {
            deploy.pre_state_hash = vec![0x92; 32].into();
        }),
        mutate(block, "post-state root", |deploy| {
            deploy.post_state_hash = vec![0x93; 32].into();
        }),
        mutate(block, "certificate pre-state", |deploy| {
            deploy
                .authority_funding_certificate
                .as_mut()
                .unwrap()
                .pre_state_root = vec![0x94; 32].into();
        }),
        mutate(block, "certificate schedule", |deploy| {
            deploy
                .authority_funding_certificate
                .as_mut()
                .unwrap()
                .byte_cost_schedule_version += 1;
        }),
        mutate(block, "certificate cost surface", |deploy| {
            deploy
                .authority_funding_certificate
                .as_mut()
                .unwrap()
                .byte_cost_bound += 1;
        }),
        mutate(block, "witness pre-state", |deploy| {
            deploy
                .authority_cost_witness
                .as_mut()
                .unwrap()
                .pre_state_root = vec![0x95; 32].into();
        }),
        mutate(block, "witness post-state", |deploy| {
            deploy
                .authority_cost_witness
                .as_mut()
                .unwrap()
                .post_state_root = vec![0x96; 32].into();
        }),
        mutate(block, "witness schedule", |deploy| {
            deploy
                .authority_cost_witness
                .as_mut()
                .unwrap()
                .byte_cost_schedule_digest = vec![0x97; 32].into();
        }),
        mutate(block, "witness quantitative cost", |deploy| {
            deploy.authority_cost_witness.as_mut().unwrap().byte_cost += 1;
        }),
        mutate(block, "witness cost surface", |deploy| {
            deploy
                .authority_cost_witness
                .as_mut()
                .unwrap()
                .events
                .push(Default::default());
        }),
        mutate(block, "witness grade", |deploy| {
            deploy
                .authority_cost_witness
                .as_mut()
                .unwrap()
                .realized
                .push(Default::default());
        }),
        mutate(block, "admission status", |deploy| {
            deploy.admission_status = DeployAdmissionStatus::Rejected;
        }),
    ]
}

fn envelope(
    term: String,
    secret: PrivateKey,
    valid_after_block_number: i64,
    shard_id: &str,
) -> Cosigned<DeployData> {
    let signed = construct_deploy::source_deploy_now_full(
        term,
        None,
        None,
        Some(secret.clone()),
        Some(valid_after_block_number),
        Some(shard_id.to_string()),
    )
    .expect("construct deploy data");
    Cosigned::create_single_envelope(signed.data, Box::new(Secp256k1), secret)
        .expect("create protocol-v6 envelope")
}

fn deploy_id(envelope: &Cosigned<DeployData>) -> DeployLookupId {
    let commitment = envelope.envelope_commitment().expect("envelope commitment");
    DeployLookupId::V6(
        DeployIdV6::try_from(commitment.as_ref()).expect("protocol-v6 deploy identity"),
    )
}

fn block_deploy_ids(deploys: &[ProcessedDeploy], protocol_version: i64) -> Vec<DeployLookupId> {
    deploys
        .iter()
        .map(|deploy| {
            deploy
                .deploy_id_for_protocol(protocol_version)
                .expect("processed deploy identity")
        })
        .collect()
}

fn assert_one_successful_terminal_close(system_deploys: &[ProcessedSystemDeploy]) {
    let closes = system_deploys
        .iter()
        .filter(|deploy| {
            matches!(deploy, ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::CloseBlockSystemDeployData,
                ..
            })
        })
        .count();
    assert_eq!(closes, 1);
    assert!(matches!(
        system_deploys.last(),
        Some(ProcessedSystemDeploy::Succeeded {
            system_deploy: SystemDeployData::CloseBlockSystemDeployData,
            ..
        })
    ));
}

async fn network() -> Vec<TestNode> {
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(2));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("build genesis");
    TestNode::create_network(genesis, 2, None, None, None, None)
        .await
        .expect("create two-node network")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn nonretryable_checkpoint_failure_returns_no_created_block() {
    crate::init_logger();
    let mut nodes = network().await;
    let user = envelope(
        "@\"failed-checkpoint-retained\"!(0)".to_string(),
        construct_deploy::DEFAULT_SEC.clone(),
        0,
        &nodes[0].genesis.shard_id,
    );
    let user_id = deploy_id(&user);
    assert!(matches!(
        nodes[0]
            .casper
            .deploy_cosigned(user)
            .expect("submit protocol-v6 deploy"),
        Either::Right(_)
    ));
    let snapshot = nodes[0].casper.get_snapshot().await.expect("snapshot");
    let validator = nodes[0]
        .validator_id_opt
        .clone()
        .expect("validator identity");
    let deploy_storage = nodes[0].deploy_storage.clone();
    let rejected_buffer = nodes[0].rejected_deploy_buffer.clone();
    let runtime_manager = nodes[0].runtime_manager.clone();
    let (result, attempts) = block_creator::create_with_forced_checkpoint_failure(
        &snapshot,
        &validator,
        None,
        deploy_storage.clone(),
        rejected_buffer,
        &runtime_manager,
        &mut nodes[0].block_store,
        true,
    )
    .await;

    assert_eq!(attempts.len(), 1);
    assert!(matches!(result, Err(CasperError::SystemRuntimeError(_))));
    assert!(deploy_storage
        .lock()
        .contains_envelope(user_id.as_bytes())
        .expect("read retained deploy"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn forced_prefix_shrink_replays_the_canonical_retained_batch() {
    crate::init_logger();
    let mut nodes = network().await;
    let shard_id = nodes[0].genesis.shard_id.clone();
    let mut users = Vec::new();
    for index in 0..4 {
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        users.push(envelope(
            format!("@\"prefix-shrink-{index}\"!({index})"),
            construct_deploy::DEFAULT_SEC.clone(),
            0,
            &shard_id,
        ));
    }
    let user_ids: HashSet<DeployLookupId> = users.iter().map(deploy_id).collect();
    for deploy in &users {
        assert!(matches!(
            nodes[0]
                .casper
                .deploy_cosigned(deploy.clone())
                .expect("submit protocol-v6 deploy"),
            Either::Right(_)
        ));
    }

    let snapshot = nodes[0].casper.get_snapshot().await.expect("snapshot");
    let validator = nodes[0]
        .validator_id_opt
        .clone()
        .expect("validator identity");
    let deploy_storage = nodes[0].deploy_storage.clone();
    let rejected_buffer = nodes[0].rejected_deploy_buffer.clone();
    let runtime_manager = nodes[0].runtime_manager.clone();
    let (created, attempts) = block_creator::create_with_forced_checkpoint_retry(
        &snapshot,
        &validator,
        Some((
            construct_deploy::DEFAULT_SEC2.clone(),
            "@\"prefix-shrink-dummy\"!(0)".to_string(),
        )),
        deploy_storage.clone(),
        rejected_buffer,
        &runtime_manager,
        &mut nodes[0].block_store,
        false,
    )
    .await
    .expect("create block after forced prefix retry");
    let BlockCreatorResult::Created(block, pre_state, post_state) = created else {
        panic!("forced prefix retry must create a block");
    };

    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].user_deploy_limit, 4);
    assert_eq!(attempts[1].user_deploy_limit, 2);
    assert_eq!(attempts[0].deploy_ids.len(), 5);
    assert_eq!(attempts[1].deploy_ids.len(), 3);

    let first_users = attempts[0]
        .deploy_ids
        .iter()
        .filter(|id| user_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let retained_users = attempts[1]
        .deploy_ids
        .iter()
        .filter(|id| user_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(retained_users, first_users[..2]);

    let first_non_users = attempts[0]
        .deploy_ids
        .iter()
        .filter(|id| !user_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let retained_non_users = attempts[1]
        .deploy_ids
        .iter()
        .filter(|id| !user_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(first_non_users.len(), 1);
    assert_eq!(retained_non_users, first_non_users);

    let packaged_ids = block_deploy_ids(&block.body.deploys, block.header.version);
    assert_eq!(packaged_ids, attempts[1].deploy_ids);
    assert_eq!(pre_state, block.body.state.pre_state_hash);
    assert_eq!(post_state, block.body.state.post_state_hash);
    assert_one_successful_terminal_close(&block.body.system_deploys);

    let removed_users = user_ids
        .difference(&retained_users.iter().cloned().collect())
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(removed_users.len(), 2);
    for removed in &removed_users {
        assert!(deploy_storage
            .lock()
            .contains_envelope(removed.as_bytes())
            .expect("read deferred deploy"));
    }

    let replayed = nodes[1]
        .runtime_manager
        .replay_block_from_consensus_data(&pre_state, &block, None)
        .await
        .expect("independent replay");
    assert_eq!(replayed, post_state);
    assert!(matches!(
        nodes[1]
            .process_block(block.clone())
            .await
            .expect("peer validation"),
        Either::Right(_)
    ));

    let removed_envelope = users
        .iter()
        .find(|deploy| removed_users.contains(&deploy_id(deploy)))
        .expect("removed envelope");
    let mut forged = block.clone();
    forged
        .body
        .deploys
        .push(ProcessedDeploy::empty_from_cosigned(removed_envelope));
    let forged_replay = nodes[1]
        .runtime_manager
        .replay_block_from_consensus_data(&pre_state, &forged, None)
        .await;
    match forged_replay {
        Err(casper::rust::errors::CasperError::ReplayFailure(
            casper::rust::util::rholang::replay_failure::ReplayFailure::ReplayAdmissionMismatch {
                ..
            },
        )) => {}
        Err(casper::rust::errors::CasperError::InvalidCostSettlement(detail)) => {
            assert!(detail.contains("missing its authority certificate"));
        }
        other => panic!("forged suffix must fail admission or certificate validation: {other:?}"),
    }

    assert!(matches!(
        nodes[0]
            .process_block(block)
            .await
            .expect("proposer validation"),
        Either::Right(_)
    ));
    let next_snapshot = nodes[0]
        .casper
        .get_snapshot()
        .await
        .expect("post-block snapshot");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64;
    let prepared = block_creator::prepare_user_deploys(
        &next_snapshot,
        next_snapshot.max_block_num + 1,
        now,
        deploy_storage,
        nodes[0].rejected_deploy_buffer.clone(),
        &nodes[0].block_store,
        true,
        true,
    )
    .await
    .expect("prepare deferred suffix");
    let prepared_ids = prepared
        .deploys
        .iter()
        .map(|deploy| deploy.typed_deploy_id().clone())
        .collect::<HashSet<_>>();
    assert!(removed_users.iter().all(|id| prepared_ids.contains(id)));
    assert!(retained_users.iter().all(|id| !prepared_ids.contains(id)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn forced_shrink_publishes_only_final_window_rejections() {
    crate::init_logger();
    let mut nodes = network().await;
    let shard_id = nodes[0].genesis.shard_id.clone();
    let underfunded_key = PrivateKey::from_bytes(&[0x61; 32]);
    let dropped_underfunded_key = PrivateKey::from_bytes(&[0x62; 32]);
    let mut users = Vec::new();
    for (index, secret) in [
        construct_deploy::DEFAULT_SEC.clone(),
        underfunded_key,
        construct_deploy::DEFAULT_SEC.clone(),
        dropped_underfunded_key,
    ]
    .into_iter()
    .enumerate()
    {
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        users.push(envelope(
            format!("@\"recertified-window-{index}\"!({index})"),
            secret,
            0,
            &shard_id,
        ));
    }
    let underfunded_id = deploy_id(&users[1]);
    let dropped_underfunded_id = deploy_id(&users[3]);
    let mut canonical_users = users.clone();
    casper::rust::util::rholang::acceptance::canonical_sort(&mut canonical_users);
    let retained_user_ids = canonical_users
        .iter()
        .take(2)
        .map(deploy_id)
        .collect::<HashSet<_>>();
    assert!(retained_user_ids.contains(&underfunded_id));
    assert!(!retained_user_ids.contains(&dropped_underfunded_id));
    for deploy in &users {
        assert!(matches!(
            nodes[0]
                .casper
                .deploy_cosigned(deploy.clone())
                .expect("submit protocol-v6 deploy"),
            Either::Right(_)
        ));
    }

    let snapshot = nodes[0].casper.get_snapshot().await.expect("snapshot");
    let validator = nodes[0]
        .validator_id_opt
        .clone()
        .expect("validator identity");
    let deploy_storage = nodes[0].deploy_storage.clone();
    let rejected_buffer = nodes[0].rejected_deploy_buffer.clone();
    let runtime_manager = nodes[0].runtime_manager.clone();
    let (created, attempts) = block_creator::create_with_forced_checkpoint_retry(
        &snapshot,
        &validator,
        Some((
            construct_deploy::DEFAULT_SEC2.clone(),
            "@\"recertified-window-dummy\"!(0)".to_string(),
        )),
        deploy_storage.clone(),
        rejected_buffer,
        &runtime_manager,
        &mut nodes[0].block_store,
        false,
    )
    .await
    .expect("create block after final-window recertification");
    let BlockCreatorResult::Created(block, pre_state, post_state) = created else {
        panic!("forced checkpoint retry must create a block");
    };

    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].generation, 0);
    assert_eq!(attempts[1].generation, 1);
    assert_eq!(attempts[0].user_deploy_limit, 4);
    assert_eq!(attempts[1].user_deploy_limit, 2);
    assert_eq!(attempts[1].candidate_ids.len(), 3);
    assert!(attempts[0].rejected_ids.contains(&dropped_underfunded_id));
    assert_eq!(attempts[1].rejected_ids, vec![underfunded_id.clone()]);
    assert!(attempts[1].deferred_ids.is_empty());

    let expected_packaged_ids = attempts[1]
        .deploy_ids
        .iter()
        .chain(attempts[1].rejected_ids.iter())
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        block_deploy_ids(&block.body.deploys, block.header.version),
        expected_packaged_ids
    );
    assert!(!expected_packaged_ids.contains(&dropped_underfunded_id));
    let rejection = block
        .body
        .deploys
        .iter()
        .find(|deploy| {
            deploy
                .deploy_id_for_protocol(block.header.version)
                .is_ok_and(|id| id == underfunded_id)
        })
        .expect("final-window rejection");
    assert!(rejection.is_admission_rejected());
    assert_eq!(rejection.pre_state_hash, pre_state);
    assert_eq!(rejection.post_state_hash, pre_state);
    assert_eq!(rejection.cost.cost, 0);
    assert!(rejection.deploy_log.is_empty());
    assert_one_successful_terminal_close(&block.body.system_deploys);

    let removed_suffix = canonical_users
        .iter()
        .skip(2)
        .map(deploy_id)
        .collect::<Vec<_>>();
    for removed in &removed_suffix {
        assert!(deploy_storage
            .lock()
            .contains_envelope(removed.as_bytes())
            .expect("read deferred suffix"));
    }

    let replayed = nodes[1]
        .runtime_manager
        .replay_block_from_consensus_data(&pre_state, &block, None)
        .await
        .expect("independent replay");
    assert_eq!(replayed, post_state);
    assert!(matches!(
        nodes[1]
            .process_block(block.clone())
            .await
            .expect("peer validation"),
        Either::Right(_)
    ));
    assert!(matches!(
        nodes[0]
            .process_block(block)
            .await
            .expect("proposer validation"),
        Either::Right(_)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn dummy_only_block_is_funded_and_replays_on_a_peer() {
    let metrics = execution_metrics();
    crate::init_logger();
    let mut nodes = network().await;
    assert!(nodes[0]
        .deploy_storage
        .lock()
        .read_all_envelopes()
        .expect("read user deploy store")
        .is_empty());
    let snapshot = nodes[0].casper.get_snapshot().await.expect("snapshot");
    let proposal_before = execution_count(&metrics, "proposal");
    let validator = nodes[0]
        .validator_id_opt
        .clone()
        .expect("validator identity");
    let deploy_storage = nodes[0].deploy_storage.clone();
    let rejected_buffer = nodes[0].rejected_deploy_buffer.clone();
    let runtime_manager = nodes[0].runtime_manager.clone();
    let created = block_creator::create(
        &snapshot,
        &validator,
        Some((
            construct_deploy::DEFAULT_SEC.clone(),
            "@\"dummy-only\"!(0)".to_string(),
        )),
        deploy_storage,
        rejected_buffer,
        &runtime_manager,
        &mut nodes[0].block_store,
        false,
    )
    .await
    .expect("create dummy-only block");
    let BlockCreatorResult::Created(block, pre_state, post_state) = created else {
        panic!("a funded dummy deploy must create a block");
    };

    assert_eq!(block.body.deploys.len(), 1);
    let dummy = &block.body.deploys[0];
    assert!(!dummy.is_admission_rejected());
    assert!(dummy.authority_funding_certificate.is_some());
    assert!(dummy.authority_cost_witness.is_some());
    assert_one_successful_terminal_close(&block.body.system_deploys);
    assert_eq!(pre_state, block.body.state.pre_state_hash);
    assert_eq!(post_state, block.body.state.post_state_hash);
    assert_eq!(
        execution_count(&metrics, "proposal") - proposal_before,
        block.body.deploys.len() as u64
    );

    let validation_before = execution_count(&metrics, "validation");
    let replay_before = execution_count(&metrics, "replay");
    let replayed = nodes[1]
        .runtime_manager
        .replay_block_from_consensus_data(&pre_state, &block, None)
        .await
        .expect("dummy-only peer replay");
    assert_eq!(replayed, post_state);
    assert_eq!(
        execution_count(&metrics, "validation") - validation_before,
        block.body.deploys.len() as u64
    );
    assert_eq!(execution_count(&metrics, "replay"), replay_before);
    let warm_replay = nodes[1]
        .runtime_manager
        .replay_compute_state(
            &pre_state,
            block.body.deploys.clone(),
            block.body.system_deploys.clone(),
            &BlockData::from_block(&block),
            None,
            false,
        )
        .await
        .expect("same-context cached replay");
    assert_eq!(warm_replay, post_state);
    assert_eq!(execution_count(&metrics, "replay"), replay_before);
    assert_eq!(
        nodes[1]
            .runtime_manager
            .load_mergeable_channels(&block)
            .expect("validator mergeable evidence"),
        nodes[0]
            .runtime_manager
            .load_mergeable_channels(&block)
            .expect("proposer mergeable evidence")
    );

    for (name, forged) in execution_evidence_mutations(&block) {
        let result = nodes[1]
            .runtime_manager
            .replay_block_from_consensus_data(&pre_state, &forged, None)
            .await;
        assert!(
            matches!(
                result,
                Err(CasperError::ReplayFailure(
                    casper::rust::util::rholang::replay_failure::ReplayFailure::ReplayAdmissionMismatch { .. }
                ))
            ),
            "{name} mutation was not rejected as an admission mismatch: {result:?}"
        );
    }

    nodes[1]
        .runtime_manager
        .replay_cache
        .as_ref()
        .expect("replay cache")
        .clear();
    assert!(nodes[1]
        .runtime_manager
        .delete_mergeable_channels(&block)
        .expect("delete validator mergeable evidence"));
    let full_replay_before = execution_count(&metrics, "replay");
    let full_replay = nodes[1]
        .runtime_manager
        .replay_compute_state(
            &pre_state,
            block.body.deploys.clone(),
            block.body.system_deploys.clone(),
            &BlockData::from_block(&block),
            None,
            false,
        )
        .await
        .expect("full differential replay");
    assert_eq!(full_replay, post_state);
    assert_eq!(
        execution_count(&metrics, "replay") - full_replay_before,
        block.body.deploys.len() as u64
    );

    let recovery_before = execution_count(&metrics, "recovery");
    nodes[1]
        .runtime_manager
        .ensure_mergeable_entry(&block, Default::default())
        .await
        .expect("recover validator mergeable evidence");
    assert_eq!(
        execution_count(&metrics, "recovery") - recovery_before,
        block.body.deploys.len() as u64
    );
    assert_eq!(
        nodes[1]
            .runtime_manager
            .load_mergeable_channels(&block)
            .expect("recovered validator mergeable evidence"),
        nodes[0]
            .runtime_manager
            .load_mergeable_channels(&block)
            .expect("proposer mergeable evidence")
    );

    assert!(matches!(
        nodes[1]
            .process_block(block.clone())
            .await
            .expect("dummy-only peer validation"),
        Either::Right(_)
    ));
}
