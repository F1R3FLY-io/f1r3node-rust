use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

use block_storage::rust::dag::block_dag_key_value_storage::FinalizationWitnessInputs;
use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::causal_equivocation::CertifiedConsensusContext;
use casper::rust::finality::certificate::{self, CertificateVerificationSchedule};
use casper::rust::finality::floor::{floor_of_frozen_vote_projection, Floor, FloorOfView};
use casper::rust::genesis::genesis::Genesis;
use casper::rust::safety::clique_oracle::{CliqueOracle, FtThreshold};
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::vault_payer::balance_query_source;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::validate::Validate;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::block_hash::BlockHashSerde;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::ValidatorSerde;
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::rho_type::RhoNumber;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use tokio::time::{timeout, Duration};

use super::staging::{mint_on_expected_snapshot, ExpectedParents};
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

async fn network(validator_count: usize) -> Vec<TestNode> {
    network_with_finalization_rate(validator_count, 1).await
}

async fn network_with_finalization_rate(
    validator_count: usize,
    finalization_rate: i32,
) -> Vec<TestNode> {
    let parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(validator_count));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("build genesis");
    let mut nodes = TestNode::create_network_with_finalization_rate(
        genesis,
        validator_count,
        None,
        None,
        None,
        None,
        finalization_rate,
    )
    .await
    .expect("create network");
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }
    nodes
}

async fn three_node_network() -> Vec<TestNode> { network(3).await }

async fn concurrent_siblings(nodes: &mut [TestNode]) -> (BlockMessage, BlockMessage) {
    let first = nodes[0]
        .add_block_from_deploys(&[])
        .await
        .expect("first sibling");
    let second = nodes[1]
        .add_block_from_deploys(&[])
        .await
        .expect("second sibling");
    assert_eq!(
        first.header.parents_hash_list, second.header.parents_hash_list,
        "siblings must use one shared frontier"
    );
    (first, second)
}

async fn deliver_valid(node: &mut TestNode, block: &BlockMessage, label: &str) {
    let status = node
        .process_block(block.clone())
        .await
        .unwrap_or_else(|error| panic!("deliver[{label}] failed: {error}"));
    assert!(
        matches!(status, Either::Right(_)),
        "deliver[{label}] rejected a valid block: {status:?}"
    );
}

async fn wait_for_finalizer_quiescence(node: &TestNode) {
    timeout(Duration::from_secs(30), async {
        let mut stable_samples = 0;
        loop {
            let quiescent = node.casper.finalization_schedule.is_quiescent()
                && node.casper.finalization_in_progress.load(Ordering::SeqCst) == 0;
            if quiescent {
                stable_samples += 1;
                if stable_samples == 3 {
                    return;
                }
            } else {
                stable_samples = 0;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("background finalizer quiescence");
}

async fn vault_balance(
    runtime_manager: &RuntimeManager,
    state_hash: &Bytes,
    address: &VaultAddress,
) -> i64 {
    let (values, _) = runtime_manager
        .play_exploratory_deploy(balance_query_source(address), state_hash, None)
        .await
        .expect("query replayed vault balance");
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).expect("numeric replayed vault balance")
}

async fn build_with_forced_declared_parent(
    node: &mut TestNode,
    retained_parent: &BlockMessage,
) -> Result<BlockMessage, casper::rust::errors::CasperError> {
    let mut snapshot = node.casper.get_snapshot().await.expect("honest snapshot");
    snapshot.parents = vec![retained_parent.clone()];
    snapshot.max_block_num = retained_parent.body.state.block_number;
    let validator_identity = node.validator_id_opt.clone().expect("validator identity");
    let created = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        node.deploy_storage.clone(),
        node.rejected_deploy_buffer.clone(),
        &node.runtime_manager,
        &mut node.block_store,
        node.allow_empty_blocks,
    )
    .await?;
    let BlockCreatorResult::Created(block, _, _) = created else {
        return Err(casper::rust::errors::CasperError::RuntimeError(
            "declared-parent-subset scenario did not create a candidate".to_string(),
        ));
    };
    Ok(block)
}

#[tokio::test]
async fn declared_parent_subset_may_retain_non_parent_justifications() {
    let mut nodes = three_node_network().await;
    let (first, second) = concurrent_siblings(&mut nodes).await;

    deliver_valid(&mut nodes[2], &first, "first to builder").await;
    deliver_valid(&mut nodes[2], &second, "second to builder").await;
    deliver_valid(&mut nodes[1], &first, "first to peer").await;

    let honest_snapshot = nodes[2]
        .casper
        .get_snapshot()
        .await
        .expect("builder snapshot");
    let honest_parents = honest_snapshot
        .parents
        .iter()
        .map(|parent| parent.block_hash.clone())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        honest_parents,
        std::collections::HashSet::from([first.block_hash.clone(), second.block_hash.clone()])
    );

    let builder = nodes[2]
        .validator_id_opt
        .as_ref()
        .expect("builder identity")
        .public_key
        .bytes
        .clone();
    let candidate = build_with_forced_declared_parent(&mut nodes[2], &first)
        .await
        .expect("build declared-parent-subset block");
    assert_eq!(candidate.header.parents_hash_list, vec![first
        .block_hash
        .clone()]);
    assert!(
        candidate
            .justifications
            .iter()
            .any(|justification| justification.latest_block_hash == second.block_hash),
        "the frozen justification context must retain the omitted sibling"
    );
    deliver_valid(&mut nodes[1], &candidate, "declared parent subset").await;
    assert!(nodes[1].contains(&candidate.block_hash));
    let after = nodes[1]
        .casper
        .block_dag()
        .await
        .expect("peer DAG after acceptance")
        .latest_message_hash(&builder);
    assert_eq!(after, Some(candidate.block_hash));
}

#[tokio::test]
async fn candidate_parent_frontier_must_include_committed_floor_ancestry() {
    let mut nodes = network_with_finalization_rate(4, 0).await;
    let genesis = nodes[0]
        .casper
        .last_finalized_block()
        .await
        .expect("genesis floor");
    let (floor_candidate, incompatible_sibling) = concurrent_siblings(&mut nodes).await;

    deliver_valid(
        &mut nodes[2],
        &floor_candidate,
        "floor candidate to witness",
    )
    .await;
    deliver_valid(&mut nodes[1], &floor_candidate, "floor candidate to voter").await;
    deliver_valid(
        &mut nodes[3],
        &floor_candidate,
        "floor candidate to builder",
    )
    .await;
    let witness = nodes[2]
        .add_block_from_deploys(&[])
        .await
        .expect("independent witness block");
    deliver_valid(&mut nodes[0], &witness, "witness to builder").await;
    deliver_valid(&mut nodes[1], &witness, "witness to voter").await;
    deliver_valid(&mut nodes[3], &witness, "witness to builder").await;

    deliver_valid(
        &mut nodes[0],
        &incompatible_sibling,
        "incompatible sibling to floor builder",
    )
    .await;
    deliver_valid(
        &mut nodes[2],
        &incompatible_sibling,
        "incompatible sibling to witness",
    )
    .await;
    deliver_valid(&mut nodes[3], &incompatible_sibling, "sibling to builder").await;

    let co_witness = nodes[1]
        .add_block_from_deploys(&[])
        .await
        .expect("co-witness block");
    assert!(nodes[1]
        .casper
        .block_dag()
        .await
        .expect("co-witness DAG")
        .is_in_main_chain(&floor_candidate.block_hash, &co_witness.block_hash)
        .expect("co-witness main-chain query"));
    deliver_valid(&mut nodes[0], &co_witness, "co-witness to floor builder").await;
    deliver_valid(&mut nodes[2], &co_witness, "co-witness to witness").await;
    deliver_valid(&mut nodes[3], &co_witness, "co-witness to builder").await;

    let reciprocal_witness = nodes[2]
        .add_block_from_deploys(&[])
        .await
        .expect("reciprocal witness block");
    assert!(nodes[2]
        .casper
        .block_dag()
        .await
        .expect("reciprocal witness DAG")
        .is_in_main_chain(&floor_candidate.block_hash, &reciprocal_witness.block_hash,)
        .expect("reciprocal witness main-chain query"));
    deliver_valid(
        &mut nodes[0],
        &reciprocal_witness,
        "reciprocal witness to floor builder",
    )
    .await;
    deliver_valid(
        &mut nodes[1],
        &reciprocal_witness,
        "reciprocal witness to voter",
    )
    .await;
    deliver_valid(
        &mut nodes[3],
        &reciprocal_witness,
        "reciprocal witness to builder",
    )
    .await;

    let closing_witness = nodes[0]
        .add_block_from_deploys(&[])
        .await
        .expect("closing witness block");
    assert!(nodes[0]
        .casper
        .block_dag()
        .await
        .expect("closing witness DAG")
        .is_in_main_chain(&floor_candidate.block_hash, &closing_witness.block_hash)
        .expect("closing witness main-chain query"));
    deliver_valid(&mut nodes[1], &closing_witness, "closing witness to voter").await;
    deliver_valid(&mut nodes[2], &closing_witness, "closing witness to peer").await;
    deliver_valid(
        &mut nodes[3],
        &closing_witness,
        "closing witness to builder",
    )
    .await;

    let vote_carrier = nodes[1]
        .add_block_from_deploys(&[])
        .await
        .expect("vote carrier");
    assert!(nodes[1]
        .casper
        .block_dag()
        .await
        .expect("vote carrier DAG")
        .is_in_main_chain(&floor_candidate.block_hash, &vote_carrier.block_hash)
        .expect("vote carrier main-chain query"));
    deliver_valid(
        &mut nodes[0],
        &vote_carrier,
        "vote carrier to floor builder",
    )
    .await;
    deliver_valid(&mut nodes[2], &vote_carrier, "vote carrier to peer").await;
    deliver_valid(&mut nodes[3], &vote_carrier, "vote carrier to builder").await;

    let snapshot = nodes[0]
        .casper
        .get_snapshot()
        .await
        .expect("supported view");
    let threshold = FtThreshold::from_ppm(
        snapshot
            .on_chain_state
            .shard_conf
            .fault_tolerance_threshold_ppm,
    );
    let current = Floor {
        hash: genesis.block_hash.clone(),
        block_number: genesis.body.state.block_number,
    };
    let decision_context =
        CertifiedConsensusContext::for_finalized_floor(&snapshot.dag, current.hash.clone())
            .expect("build frozen finalization context");
    let FloorOfView::Advance(derived) = floor_of_frozen_vote_projection(
        &snapshot.dag,
        &nodes[0].block_store,
        &current,
        decision_context
            .vote_projection()
            .eligible_latest_messages(),
        threshold,
    )
    .await
    .expect("derive supported floor") else {
        panic!("supported frozen projection must advance the floor");
    };
    assert!(derived.block_number > genesis.body.state.block_number);
    assert!(snapshot
        .dag
        .is_dag_ancestor(&floor_candidate.block_hash, &derived.hash)
        .expect("derived floor ancestry query"));
    assert!(!snapshot
        .dag
        .is_dag_ancestor(&derived.hash, &incompatible_sibling.block_hash)
        .expect("derived floor incompatibility query"));

    let validator_identity = nodes[0]
        .validator_id_opt
        .clone()
        .expect("proposal validator identity");
    let deploy_storage = nodes[0].deploy_storage.clone();
    let rejected_deploy_buffer = nodes[0].rejected_deploy_buffer.clone();
    let runtime_manager = nodes[0].runtime_manager.clone();
    let allow_empty_blocks = nodes[0].allow_empty_blocks;
    let proposal_before_materialization = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        deploy_storage,
        rejected_deploy_buffer,
        &runtime_manager,
        &mut nodes[0].block_store,
        allow_empty_blocks,
    )
    .await
    .expect("create from certified floor while newer candidate is held");
    let BlockCreatorResult::Created(proposal_before_materialization, _, _) =
        proposal_before_materialization
    else {
        panic!("independent candidate evidence must not gate proposal");
    };
    assert_eq!(
        proposal_before_materialization
            .header
            .finalized_floor
            .as_ref()
            .expect("proposal floor commitment")
            .floor_hash,
        genesis.block_hash
    );

    wait_for_finalizer_quiescence(&nodes[0]).await;
    let exact_latest = decision_context
        .vote_projection()
        .exact_latest_messages()
        .iter()
        .map(|(validator, hash)| {
            (
                ValidatorSerde(validator.clone()),
                BlockHashSerde(hash.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert!(CliqueOracle::ft_witnessed_exact(
        &derived.hash,
        &snapshot.dag,
        decision_context
            .vote_projection()
            .eligible_latest_messages(),
        threshold,
    )
    .await
    .expect("exact floor certificate decision"));
    let zero = Bytes::from(vec![0; models::rust::block_hash::LENGTH]);
    let finalization_base = nodes[0]
        .block_dag_storage
        .capture_finalization_base()
        .expect("capture finalization base");
    let ft_value = CliqueOracle::normalized_fault_tolerance(&derived.hash, &snapshot.dag)
        .await
        .expect("normalized floor fault tolerance");
    let witness_inputs = FinalizationWitnessInputs {
        protocol_version: snapshot.on_chain_state.shard_conf.casper_version,
        shard_id: snapshot.on_chain_state.shard_conf.shard_name.clone(),
        predecessor_certificate_digest: BlockHashSerde(zero.clone()),
        predecessor_certificate_block_hash: BlockHashSerde(zero),
        fault_tolerance_numerator: threshold.num,
        fault_tolerance_denominator: threshold.den,
        latest_messages: exact_latest,
        authority_context_digest: BlockHashSerde(decision_context.digest().clone()),
    };
    nodes[0]
        .block_dag_storage
        .record_directly_finalized_certified_atomic(
            &finalization_base.head,
            derived.hash.clone(),
            ft_value,
            witness_inputs.clone(),
            |_revision, _finalized| async { Ok(()) },
        )
        .await
        .expect("materialize the derived floor");
    let peer_finalization_base = nodes[1]
        .block_dag_storage
        .capture_finalization_base()
        .expect("capture peer finalization base");
    nodes[1]
        .block_dag_storage
        .record_directly_finalized_certified_atomic(
            &peer_finalization_base.head,
            derived.hash.clone(),
            ft_value,
            witness_inputs.clone(),
            |_revision, _finalized| async { Ok(()) },
        )
        .await
        .expect("materialize the derived floor for the peer");
    deliver_valid(
        &mut nodes[1],
        &proposal_before_materialization,
        "captured proposal after floor promotion",
    )
    .await;
    assert!(nodes[1].contains(&proposal_before_materialization.block_hash));
    let builder_finalization_base = nodes[3]
        .block_dag_storage
        .capture_finalization_base()
        .expect("capture builder finalization base");
    nodes[3]
        .block_dag_storage
        .record_directly_finalized_certified_atomic(
            &builder_finalization_base.head,
            derived.hash.clone(),
            ft_value,
            witness_inputs,
            |_revision, _finalized| async { Ok(()) },
        )
        .await
        .expect("materialize the derived floor for the builder");
    let carrier_snapshot = nodes[0]
        .casper
        .get_snapshot()
        .await
        .expect("certificate carrier snapshot");
    let validator_identity = nodes[0]
        .validator_id_opt
        .clone()
        .expect("certificate carrier identity");
    let deploy_storage = nodes[0].deploy_storage.clone();
    let rejected_deploy_buffer = nodes[0].rejected_deploy_buffer.clone();
    let runtime_manager = nodes[0].runtime_manager.clone();
    let allow_empty_blocks = nodes[0].allow_empty_blocks;
    let created = block_creator::create(
        &carrier_snapshot,
        &validator_identity,
        None,
        deploy_storage,
        rejected_deploy_buffer,
        &runtime_manager,
        &mut nodes[0].block_store,
        allow_empty_blocks,
    )
    .await
    .expect("build certificate carrier");
    let BlockCreatorResult::Created(certificate_carrier, _, _) = created else {
        panic!("certificate carrier scenario must create a block");
    };
    let commitment = certificate_carrier
        .header
        .finalized_floor
        .as_ref()
        .expect("certificate carrier commitment");
    assert_eq!(commitment.floor_hash, derived.hash);
    let certificate = certificate_carrier
        .finalized_floor_certificate
        .as_ref()
        .expect("certificate carrier proof");
    let carrier_latest = certificate_carrier
        .justifications
        .iter()
        .map(|justification| {
            (
                justification.validator.clone(),
                justification.latest_block_hash.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let carrier_context = CertifiedConsensusContext::for_frozen_floor(
        &carrier_snapshot.dag,
        commitment.floor_hash.clone(),
        &carrier_latest,
    )
    .expect("certificate carrier authority context");
    assert!(carrier_context.has_complete_latest_message_slots());
    assert_eq!(
        commitment.authority_context_digest,
        *carrier_context.digest()
    );
    assert!(matches!(
        Validate::justifications_well_formed(&certificate_carrier),
        Either::Right(_)
    ));
    assert!(matches!(
        Validate::justification_provenance(&certificate_carrier, &genesis, &nodes[0].block_store,),
        Either::Right(_)
    ));
    certificate::verify(
        &certificate_carrier,
        commitment,
        certificate,
        &carrier_snapshot.dag,
        &nodes[0].block_store,
        &genesis,
        carrier_snapshot.on_chain_state.shard_conf.casper_version,
        &carrier_snapshot.on_chain_state.shard_conf.shard_name,
        threshold,
        &carrier_snapshot.on_chain_state.shard_conf.finalizer_conf,
        &CertificateVerificationSchedule::new(1),
    )
    .await
    .expect("certificate carrier proof verification");
    let mut mismatched_certificate = certificate.clone();
    mismatched_certificate.fault_tolerance_numerator += 1;
    let mismatched_commitment =
        mismatched_certificate.commitment(commitment.authority_context_digest.clone());
    assert!(matches!(
        certificate::verify(
            &certificate_carrier,
            &mismatched_commitment,
            &mismatched_certificate,
            &carrier_snapshot.dag,
            &nodes[0].block_store,
            &genesis,
            carrier_snapshot.on_chain_state.shard_conf.casper_version,
            &carrier_snapshot.on_chain_state.shard_conf.shard_name,
            threshold,
            &carrier_snapshot.on_chain_state.shard_conf.finalizer_conf,
            &CertificateVerificationSchedule::new(1),
        )
        .await,
        Err(certificate::FinalizationCertificateError::Invalid(detail))
            if detail == "protocol, shard, genesis, or threshold binding differs"
    ));
    deliver_valid(
        &mut nodes[0],
        &certificate_carrier,
        "certificate carrier to producer",
    )
    .await;
    deliver_valid(
        &mut nodes[2],
        &certificate_carrier,
        "certificate carrier to peer",
    )
    .await;
    deliver_valid(
        &mut nodes[3],
        &certificate_carrier,
        "certificate carrier to builder",
    )
    .await;

    let proposer_error = build_with_forced_declared_parent(&mut nodes[3], &incompatible_sibling)
        .await
        .expect_err("proposer must reject a parent frontier disconnected from its signed floor");
    assert!(proposer_error
        .to_string()
        .contains("certified floor is absent from the causal parent frontier"));

    let builder = validator_identity.public_key.bytes.clone();
    let before = nodes[2]
        .casper
        .block_dag()
        .await
        .expect("peer DAG")
        .latest_message_hash(&builder);
    let mut disconnected_candidate = certificate_carrier.clone();
    disconnected_candidate.header.parents_hash_list = vec![incompatible_sibling.block_hash.clone()];
    let candidate = crate::helper::block_util::resign_block(
        &disconnected_candidate,
        &validator_identity.private_key,
    );
    let candidate_commitment = candidate
        .header
        .finalized_floor
        .as_ref()
        .expect("candidate floor commitment");
    assert_eq!(candidate_commitment.floor_hash, derived.hash);
    assert!(!nodes[0]
        .casper
        .block_dag()
        .await
        .expect("proposer DAG")
        .is_dag_ancestor(&derived.hash, &incompatible_sibling.block_hash)
        .expect("ancestry query"));

    let status = nodes[2]
        .process_block(candidate.clone())
        .await
        .expect("peer validates disconnected candidate");
    assert_eq!(
        status,
        Either::Left(BlockError::Invalid(InvalidBlock::InvalidFollows))
    );
    assert!(!nodes[2].contains(&candidate.block_hash));
    let after = nodes[2]
        .casper
        .block_dag()
        .await
        .expect("peer DAG after rejection")
        .latest_message_hash(&builder);
    assert_eq!(after, before);
}

#[tokio::test]
async fn honest_v6_multi_parent_snapshot_replays_on_peer() {
    let mut nodes = three_node_network().await;
    let (first, second) = concurrent_siblings(&mut nodes).await;

    deliver_valid(&mut nodes[2], &first, "first to proposer").await;
    deliver_valid(&mut nodes[2], &second, "second to proposer").await;
    deliver_valid(&mut nodes[1], &first, "first to reverse-order peer").await;

    let shard_id = nodes[2].genesis.shard_id.clone();
    let funding_secrets = [
        construct_deploy::DEFAULT_SEC.clone(),
        construct_deploy::DEFAULT_SEC2.clone(),
    ];
    let deploys = funding_secrets
        .iter()
        .enumerate()
        .map(|(index, secret)| {
            construct_deploy::basic_deploy_data(
                70_000 + index as i32,
                Some(secret.clone()),
                Some(shard_id.clone()),
            )
            .expect("funded protocol-v6 replay deploy")
        })
        .collect::<Vec<_>>();
    for deploy in &deploys {
        assert!(matches!(
            nodes[2]
                .submit_deploy(deploy.clone())
                .expect("submit funded protocol-v6 replay deploy"),
            Either::Right(_)
        ));
    }

    let block = mint_on_expected_snapshot(
        &mut nodes[2],
        ExpectedParents::members(&[&first, &second]),
        "honest multi-parent replay",
    )
    .await;
    let canonical_bytes = block.to_proto().encode_to_vec();
    let canonical_hash = block.block_hash.clone();
    let canonical_pre_state = block.body.state.pre_state_hash.clone();
    let canonical_post_state = block.body.state.post_state_hash.clone();
    assert_eq!(block.body.deploys.len(), deploys.len());
    let canonical_deploy_ids = block
        .body
        .deploys
        .iter()
        .map(|deploy| {
            deploy
                .deploy_id_for_protocol(block.header.version)
                .expect("canonical processed deploy identity")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        canonical_deploy_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .len(),
        canonical_deploy_ids.len()
    );
    let canonical_witnesses = block
        .body
        .deploys
        .iter()
        .map(|deploy| {
            deploy
                .authority_cost_witness
                .clone()
                .expect("canonical economic witness")
        })
        .collect::<Vec<_>>();
    let canonical_event_ids = canonical_witnesses
        .iter()
        .flat_map(|witness| {
            witness
                .events
                .iter()
                .map(|event| event.event_id.clone())
                .chain(
                    witness
                        .byte_events
                        .iter()
                        .map(|event| event.event_id.clone()),
                )
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(!canonical_event_ids.is_empty());
    assert_eq!(
        canonical_event_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .len(),
        canonical_event_ids.len()
    );
    let funding_addresses = funding_secrets
        .iter()
        .map(|secret| {
            VaultAddress::from_public_key(&Secp256k1.to_public(secret))
                .expect("canonical funding vault address")
        })
        .collect::<Vec<_>>();
    let mut proposer_pre_balances = Vec::new();
    let mut proposer_post_balances = Vec::new();
    for address in &funding_addresses {
        proposer_pre_balances
            .push(vault_balance(&nodes[2].runtime_manager, &canonical_pre_state, address).await);
        proposer_post_balances
            .push(vault_balance(&nodes[2].runtime_manager, &canonical_post_state, address).await);
    }
    assert!(proposer_pre_balances
        .iter()
        .zip(&proposer_post_balances)
        .all(|(before, after)| after < before));

    let replayed_post_state = nodes[1]
        .runtime_manager
        .replay_block_from_consensus_data(&canonical_pre_state, &block, None)
        .await
        .expect("independent peer replay");
    assert_eq!(replayed_post_state, canonical_post_state);
    let mut peer_post_balances = Vec::new();
    for address in &funding_addresses {
        peer_post_balances
            .push(vault_balance(&nodes[1].runtime_manager, &replayed_post_state, address).await);
    }
    assert_eq!(peer_post_balances, proposer_post_balances);

    deliver_valid(&mut nodes[1], &block, "honest block to peer").await;
    assert!(nodes[1].contains(&canonical_hash));
    let stored = nodes[1]
        .block_store
        .get(&canonical_hash)
        .expect("peer block read")
        .expect("peer stored block");
    assert_eq!(stored.block_hash, canonical_hash);
    assert_eq!(stored.body.state.post_state_hash, canonical_post_state);
    assert_eq!(
        stored
            .body
            .deploys
            .iter()
            .map(|deploy| {
                deploy
                    .deploy_id_for_protocol(stored.header.version)
                    .expect("stored processed deploy identity")
            })
            .collect::<Vec<_>>(),
        canonical_deploy_ids
    );
    assert_eq!(
        stored
            .body
            .deploys
            .iter()
            .map(|deploy| {
                deploy
                    .authority_cost_witness
                    .clone()
                    .expect("stored economic witness")
            })
            .collect::<Vec<_>>(),
        canonical_witnesses
    );
    assert_eq!(stored.to_proto().encode_to_vec(), canonical_bytes);
}

#[tokio::test]
async fn proposer_without_the_certified_floor_state_cannot_sign() {
    let mut nodes = network(1).await;
    let snapshot = nodes[0]
        .casper
        .get_snapshot()
        .await
        .expect("proposal snapshot");
    let validator_identity = nodes[0]
        .validator_id_opt
        .clone()
        .expect("validator identity");

    let mut empty_store = InMemoryStoreManager::new();
    let rspace_store = empty_store
        .r_space_stores()
        .await
        .expect("empty RSpace stores");
    let mergeable_store = RuntimeManager::mergeable_store(&mut empty_store)
        .await
        .expect("empty mergeable store");
    let runtime_manager = RuntimeManager::create_with_store(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        ExternalServices::noop(),
    );

    let created = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        nodes[0].deploy_storage.clone(),
        nodes[0].rejected_deploy_buffer.clone(),
        &runtime_manager,
        &mut nodes[0].block_store,
        true,
    )
    .await;

    assert!(
        created.is_err(),
        "a proposer without the certified floor state must fail before signing"
    );
}
