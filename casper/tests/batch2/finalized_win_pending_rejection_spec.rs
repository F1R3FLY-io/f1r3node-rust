use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use block_storage::rust::dag::block_dag_key_value_storage::FinalizationWitnessInputs;
use casper::rust::blocks::proposer::block_creator::prepare_user_deploys;
use casper::rust::casper::Casper;
use casper::rust::causal_equivocation::CertifiedConsensusContext;
use casper::rust::finality::floor::{
    floor_of_block, floor_of_frozen_vote_projection, Floor, FloorOfView,
};
use casper::rust::safety::clique_oracle::{CliqueOracle, FtThreshold};
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::interpreter_util;
use crypto::rust::signatures::signed::Cosigned;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use models::rust::deploy_id::DeployLookupId;
use models::rust::validator::ValidatorSerde;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;
use tokio::time::{timeout, Duration};

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new() -> Self {
        let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(4));
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(parameters))
            .await
            .unwrap();

        Self { genesis }
    }
}

struct WinningReceiptFixture {
    nodes: Vec<TestNode>,
    envelope: Cosigned<DeployData>,
    deploy_id: Bytes,
    winning_block: BlockMessage,
}

fn buffer_contains(node: &TestNode, deploy_id: &Bytes) -> bool {
    node.rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .contains_id(&crate::current_deploy_id(deploy_id))
        .expect("buffer.contains_id")
}

async fn wait_for_finalizer_quiescence(node: &TestNode) {
    timeout(Duration::from_secs(30), async {
        let mut consecutive_quiescent_samples = 0;
        loop {
            let quiescent = node.casper.finalization_schedule.is_quiescent()
                && node.casper.finalization_in_progress.load(Ordering::SeqCst) == 0;
            if quiescent {
                consecutive_quiescent_samples += 1;
                if consecutive_quiescent_samples == 3 {
                    return;
                }
            } else {
                consecutive_quiescent_samples = 0;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("background finalizer quiescence");
}

async fn stage_state_preserving_winning_receipt(genesis: &GenesisContext) -> WinningReceiptFixture {
    let mut nodes = TestNode::create_network_with_finalization_rate(
        genesis.clone(),
        4,
        None,
        None,
        None,
        None,
        0,
    )
    .await
    .expect("create four-validator network");
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }

    let deploy =
        construct_deploy::basic_deploy_data(41, None, Some(genesis.genesis_block.shard_id.clone()))
            .expect("build winning deploy");
    let envelope = nodes[0]
        .envelope_for_deploy(&deploy)
        .expect("build winning envelope");
    let winning_block = nodes[0]
        .add_block_from_deploys(&[deploy])
        .await
        .expect("create winning block");
    let deploy_id = Bytes::copy_from_slice(winning_block.body.deploys[0].deploy_id());

    for node in nodes.iter_mut().skip(1) {
        let result = node
            .process_block(winning_block.clone())
            .await
            .expect("deliver winning block");
        assert!(matches!(result, Either::Right(_)));
    }

    for round in 0..2 {
        let mut support = Vec::with_capacity(nodes.len());
        for node in &mut nodes {
            support.push(
                node.add_block_from_deploys(&[])
                    .await
                    .expect("create state-preserving support"),
            );
        }
        for (source, block) in support.iter().enumerate() {
            for (target, node) in nodes.iter_mut().enumerate() {
                if source != target {
                    let result = node
                        .process_block(block.clone())
                        .await
                        .expect("deliver state-preserving support");
                    assert!(
                        matches!(result, Either::Right(_)),
                        "round {round} support from node {source} must validate on node {target}, got {result:?}"
                    );
                }
            }
        }
    }

    WinningReceiptFixture {
        nodes,
        envelope,
        deploy_id,
        winning_block,
    }
}

async fn finalize_and_materialize_winning_floor(fixture: &mut WinningReceiptFixture) -> BlockHash {
    wait_for_finalizer_quiescence(&fixture.nodes[0]).await;
    let snapshot = fixture.nodes[0]
        .casper
        .get_snapshot()
        .await
        .expect("supported finalization snapshot");
    let threshold = FtThreshold::from_ppm(
        snapshot
            .on_chain_state
            .shard_conf
            .fault_tolerance_threshold_ppm,
    );
    let current_metadata = snapshot
        .dag
        .lookup_unsafe(&snapshot.last_finalized_block)
        .expect("current floor metadata");
    let current = Floor {
        hash: snapshot.last_finalized_block.clone(),
        block_number: current_metadata.block_number,
    };
    let decision_context =
        CertifiedConsensusContext::for_finalized_floor(&snapshot.dag, current.hash.clone())
            .expect("build frozen finalization context");
    let FloorOfView::Advance(derived) = floor_of_frozen_vote_projection(
        &snapshot.dag,
        &fixture.nodes[0].block_store,
        &current,
        decision_context
            .vote_projection()
            .eligible_latest_messages(),
        threshold,
    )
    .await
    .expect("derive supported floor") else {
        panic!("state-preserving support must advance the floor");
    };
    assert!(snapshot
        .dag
        .is_dag_ancestor(&fixture.winning_block.block_hash, &derived.hash)
        .expect("winning receipt ancestry query"));
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
    let zero = Bytes::from(vec![0; models::rust::block_hash::LENGTH]);
    let finalization_base = fixture.nodes[0]
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
    fixture.nodes[0]
        .block_dag_storage
        .record_directly_finalized_certified_atomic(
            &finalization_base.head,
            derived.hash.clone(),
            ft_value,
            witness_inputs,
            |_revision, _finalized| async { Ok(()) },
        )
        .await
        .expect("materialize certified winning floor");
    let carrier = fixture.nodes[0]
        .add_block_from_deploys(&[])
        .await
        .expect("create finalized-floor certificate carrier");
    let commitment = carrier
        .header
        .finalized_floor
        .as_ref()
        .expect("certificate carrier commits a finalized floor");
    assert_eq!(
        commitment.floor_hash, derived.hash,
        "certificate carrier must commit the winning floor"
    );
    let snapshot = fixture.nodes[0]
        .casper
        .get_snapshot()
        .await
        .expect("snapshot after certificate carrier");
    let carrier_floor = floor_of_block(
        &snapshot.dag,
        &fixture.nodes[0].block_store,
        &carrier.block_hash,
        threshold,
    )
    .await
    .expect("derive certificate-carrier floor");
    assert_eq!(
        carrier_floor.hash, derived.hash,
        "certificate carrier must materialize the winning floor"
    );
    derived.hash
}

fn add_recovery_entry(fixture: &mut WinningReceiptFixture) {
    fixture.nodes[0]
        .rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .add(vec![crate::pending_envelope(fixture.envelope.clone())])
        .expect("add rejected deploy buffer entry");
    assert!(buffer_contains(&fixture.nodes[0], &fixture.deploy_id));
}

async fn assert_terminal_cleanup(fixture: &WinningReceiptFixture, finalized_floor: &BlockHash) {
    let snapshot = fixture.nodes[0]
        .casper
        .get_snapshot()
        .await
        .expect("snapshot for terminal cleanup");
    assert_eq!(
        snapshot.last_finalized_block, *finalized_floor,
        "cleanup must use the certified winning floor as the LFB"
    );
    assert!(snapshot
        .dag
        .is_dag_ancestor(
            &fixture.winning_block.block_hash,
            &snapshot.last_finalized_block,
        )
        .expect("finalized winning receipt ancestry"));
    let buffered = fixture.nodes[0]
        .rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .read_all()
        .expect("read rejected buffer")
        .into_iter()
        .find(|deploy| deploy.deploy_id() == &fixture.deploy_id)
        .expect("buffered rejected deploy");
    let next_block_number = snapshot
        .max_block_num
        .checked_add(1)
        .expect("next block number");
    let earliest_block_number =
        next_block_number - snapshot.on_chain_state.shard_conf.deploy_lifespan;
    let buffer_scan_floor = buffered
        .data()
        .valid_after_block_number
        .min(earliest_block_number);
    let parent_hashes = snapshot
        .parents
        .iter()
        .map(|block| block.block_hash.clone())
        .collect::<Vec<_>>();
    let terminal_sigs = interpreter_util::finalized_won_terminal_sigs(
        &fixture.nodes[0].block_store,
        &snapshot.last_finalized_block,
        &parent_hashes,
        buffer_scan_floor,
        snapshot.on_chain_state.shard_conf.casper_version,
    )
    .expect("compute finalized terminal signatures");
    assert!(terminal_sigs.contains(
        &DeployLookupId::from_protocol_bytes(
            snapshot.on_chain_state.shard_conf.casper_version,
            &fixture.deploy_id,
        )
        .expect("protocol deploy identity")
    ));

    let _prepared = prepare_user_deploys(
        &snapshot,
        next_block_number,
        buffered.data().time_stamp,
        fixture.nodes[0].deploy_storage.clone(),
        fixture.nodes[0].rejected_deploy_buffer.clone(),
        &fixture.nodes[0].block_store,
        true,
        true,
    )
    .await
    .expect("prepare deploys with a finalized win");
    assert!(
        !buffer_contains(&fixture.nodes[0], &fixture.deploy_id),
        "finalized winning deploy must be removed from the recovery buffer"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn finalized_base_win_purges_late_recovery_buffer_entry() {
    let ctx = TestContext::new().await;
    let mut fixture = stage_state_preserving_winning_receipt(&ctx.genesis).await;
    let finalized_floor = finalize_and_materialize_winning_floor(&mut fixture).await;
    add_recovery_entry(&mut fixture);
    assert_terminal_cleanup(&fixture, &finalized_floor).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn finalized_base_win_purges_preexisting_recovery_buffer_entry() {
    let ctx = TestContext::new().await;
    let mut fixture = stage_state_preserving_winning_receipt(&ctx.genesis).await;
    add_recovery_entry(&mut fixture);
    finalize_and_materialize_winning_floor(&mut fixture).await;
    assert!(
        !buffer_contains(&fixture.nodes[0], &fixture.deploy_id),
        "certificate-carrier preparation must remove the preexisting finalized win"
    );
}
