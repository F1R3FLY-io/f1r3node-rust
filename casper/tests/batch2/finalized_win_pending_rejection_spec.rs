use std::sync::atomic::Ordering;

use casper::rust::blocks::proposer::block_creator::prepare_user_deploys;
use casper::rust::casper::Casper;
use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::rholang::interpreter_util;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::deploy_id::DeployLookupId;
use prost::bytes::Bytes;
use serial_test::serial;
use tokio::time::{timeout, Duration};

use super::recovery_cycle_spec::{
    build_d3_vault_conflict_siblings_with_finalization_rate, propose_d3_vault_rejecting_merge,
    D3VaultConflictFixture, D3VaultMergeOutcome,
};
use super::staging::mint_on_parents;
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

fn buffer_contains(node: &TestNode, sig: &Bytes) -> bool {
    node.rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .contains_id(&crate::current_deploy_id(sig))
        .expect("buffer.contains_sig")
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

async fn stage_winning_support(
    fixture: &mut D3VaultConflictFixture,
    outcome: &D3VaultMergeOutcome,
) -> Vec<Option<BlockMessage>> {
    let mut support = vec![None; fixture.nodes.len()];
    for (index, slot) in support.iter_mut().enumerate() {
        if index != fixture.merge_proposer_index {
            *slot = Some(
                mint_on_parents(
                    &mut fixture.nodes[index],
                    vec![outcome.winning_block.clone()],
                    "finalized-win support",
                )
                .await,
            );
        }
    }
    for (source, block) in support.iter().enumerate() {
        let Some(block) = block else { continue };
        for (target, node) in fixture.nodes.iter_mut().enumerate() {
            if source != target {
                let result = node
                    .process_block(block.clone())
                    .await
                    .expect("deliver finalized-win support");
                assert!(
                    matches!(result, rspace_plus_plus::rspace::history::Either::Right(_)),
                    "finalized-win support must validate on node {target}, got {result:?}"
                );
            }
        }
    }
    support
}

async fn finalize_and_materialize_winning_floor(
    fixture: &mut D3VaultConflictFixture,
    outcome: &D3VaultMergeOutcome,
) -> BlockMessage {
    let support = stage_winning_support(fixture, outcome).await;
    let winning_validator_index = outcome.recovery_validator_index;
    wait_for_finalizer_quiescence(&fixture.nodes[winning_validator_index]).await;
    fixture.nodes[winning_validator_index]
        .block_dag_storage
        .record_directly_finalized(outcome.winning_block.block_hash.clone(), 1.0, |_| async {
            Ok(())
        })
        .await
        .expect("finalize the winning block after its supporting view exists");
    let carrier = mint_on_parents(
        &mut fixture.nodes[winning_validator_index],
        vec![support[winning_validator_index]
            .clone()
            .expect("winning validator has a supporting block")],
        "finalized-win certificate carrier",
    )
    .await;
    let commitment = carrier
        .header
        .finalized_floor
        .as_ref()
        .expect("certificate carrier commits a finalized floor");
    assert_eq!(
        commitment.floor_hash, outcome.winning_block.block_hash,
        "certificate carrier must commit the winning floor"
    );
    let snapshot = fixture.nodes[winning_validator_index]
        .casper
        .get_snapshot()
        .await
        .expect("snapshot after certificate carrier");
    let carrier_floor = floor_of_block(
        &snapshot.dag,
        &fixture.nodes[winning_validator_index].block_store,
        &carrier.block_hash,
        FtThreshold::from_ppm(
            snapshot
                .on_chain_state
                .shard_conf
                .fault_tolerance_threshold_ppm,
        ),
    )
    .await
    .expect("derive certificate-carrier floor");
    assert_eq!(
        carrier_floor.hash, outcome.winning_block.block_hash,
        "certificate carrier must materialize the winning floor"
    );
    carrier
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn finalized_base_win_is_not_buffered_by_later_scope_tombstone() {
    let ctx = TestContext::new().await;
    let mut fixture =
        build_d3_vault_conflict_siblings_with_finalization_rate(&ctx.genesis, 0).await;
    let outcome = propose_d3_vault_rejecting_merge(&mut fixture, 0).await;
    let winning_validator_index = outcome.recovery_validator_index;
    finalize_and_materialize_winning_floor(&mut fixture, &outcome).await;
    let merge_block = outcome.merge_block.clone();

    fixture.nodes[winning_validator_index]
        .process_block(merge_block.clone())
        .await
        .expect("winning validator processes rejecting merge");
    assert!(
        fixture.nodes[winning_validator_index].contains(&merge_block.block_hash),
        "winning validator must validate the rejecting merge"
    );

    assert!(
        !buffer_contains(
            &fixture.nodes[winning_validator_index],
            &outcome.rejected_sig
        ),
        "scope-local tombstone must not buffer finalized-base sig {}",
        hex::encode(&outcome.rejected_sig)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn finalized_base_win_purges_preexisting_recovery_buffer_entry() {
    let ctx = TestContext::new().await;
    let mut fixture =
        build_d3_vault_conflict_siblings_with_finalization_rate(&ctx.genesis, 0).await;
    let outcome = propose_d3_vault_rejecting_merge(&mut fixture, 0).await;
    let winning_validator_index = outcome.recovery_validator_index;
    let merge_block = outcome.merge_block.clone();

    fixture.nodes[winning_validator_index]
        .process_block(merge_block.clone())
        .await
        .expect("winning validator processes rejecting merge");
    assert!(
        buffer_contains(
            &fixture.nodes[winning_validator_index],
            &outcome.rejected_sig
        ),
        "precondition (proven by recovery_cycle_spec): with the win block \
         unfinalized, populate buffers the rejected sig"
    );
    let pre_finalization_snapshot = fixture.nodes[winning_validator_index]
        .casper
        .get_snapshot()
        .await
        .expect("snapshot before direct finalization");
    assert_eq!(
        pre_finalization_snapshot.last_finalized_block,
        ctx.genesis.genesis_block.block_hash
    );

    finalize_and_materialize_winning_floor(&mut fixture, &outcome).await;
    let finalized_snapshot = fixture.nodes[winning_validator_index]
        .casper
        .get_snapshot()
        .await
        .expect("snapshot after finalizing the winning block");
    assert_eq!(
        finalized_snapshot.last_finalized_block, outcome.winning_block.block_hash,
        "the winning source must be the LFB while the rejecting merge remains unfinalized"
    );
    assert!(!fixture.nodes[winning_validator_index]
        .block_dag_storage
        .get_representation()
        .expect("dag representation")
        .is_finalized(&merge_block.block_hash));
    let buffered = fixture.nodes[winning_validator_index]
        .rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .read_all()
        .expect("read rejected buffer")
        .into_iter()
        .find(|deploy| deploy.deploy_id() == &outcome.rejected_sig)
        .expect("buffered rejected deploy");
    let next_block_number = finalized_snapshot
        .max_block_num
        .checked_add(1)
        .expect("next block number");
    let earliest_block_number =
        next_block_number - finalized_snapshot.on_chain_state.shard_conf.deploy_lifespan;
    let buffer_scan_floor = buffered
        .data()
        .valid_after_block_number
        .min(earliest_block_number);
    let parent_hashes = finalized_snapshot
        .parents
        .iter()
        .map(|block| block.block_hash.clone())
        .collect::<Vec<_>>();
    let terminal_sigs = interpreter_util::finalized_won_terminal_sigs(
        &fixture.nodes[winning_validator_index].block_store,
        &finalized_snapshot.last_finalized_block,
        &parent_hashes,
        buffer_scan_floor,
        finalized_snapshot.on_chain_state.shard_conf.casper_version,
    )
    .expect("compute finalized terminal signatures");
    assert!(
        terminal_sigs.contains(
            &DeployLookupId::from_protocol_bytes(
                finalized_snapshot.on_chain_state.shard_conf.casper_version,
                &outcome.rejected_sig,
            )
            .expect("protocol deploy identity")
        ),
        "the finalized winning occurrence must be terminal before proposal"
    );

    let proposal_snapshot = fixture.nodes[winning_validator_index]
        .casper
        .get_snapshot()
        .await
        .expect("snapshot for post-finality proposal");
    assert_eq!(
        proposal_snapshot.last_finalized_block, outcome.winning_block.block_hash,
        "cleanup must use the winning source as the LFB"
    );
    let _prepared = prepare_user_deploys(
        &proposal_snapshot,
        next_block_number,
        buffered.data().time_stamp,
        fixture.nodes[winning_validator_index]
            .deploy_storage
            .clone(),
        fixture.nodes[winning_validator_index]
            .rejected_deploy_buffer
            .clone(),
        &fixture.nodes[winning_validator_index].block_store,
        true,
        true,
    )
    .await
    .expect("prepare deploys with the win finalized and the rejection pending");
    assert!(
        !buffer_contains(
            &fixture.nodes[winning_validator_index],
            &outcome.rejected_sig
        ),
        "deploy preparation must purge the finalized winning occurrence"
    );

    assert!(
        !buffer_contains(
            &fixture.nodes[winning_validator_index],
            &outcome.rejected_sig
        ),
        "finalized-base sig {} must be purged despite scope tombstone {} \
         at the terminal cleanup boundary",
        hex::encode(&outcome.rejected_sig),
        hex::encode(&merge_block.block_hash)
    );
}
