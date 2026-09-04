use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Cosigned;
use models::rust::casper::protocol::casper_message::{
    DeployData, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::deploy_id::{DeployIdV6, DeployLookupId};
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

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
async fn dummy_only_block_is_funded_and_replays_on_a_peer() {
    crate::init_logger();
    let mut nodes = network().await;
    assert!(nodes[0]
        .deploy_storage
        .lock()
        .read_all_envelopes()
        .expect("read user deploy store")
        .is_empty());
    let snapshot = nodes[0].casper.get_snapshot().await.expect("snapshot");
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

    let replayed = nodes[1]
        .runtime_manager
        .replay_block_from_consensus_data(&pre_state, &block, None)
        .await
        .expect("dummy-only peer replay");
    assert_eq!(replayed, post_state);
    assert!(matches!(
        nodes[1]
            .process_block(block)
            .await
            .expect("dummy-only peer validation"),
        Either::Right(_)
    ));
}
