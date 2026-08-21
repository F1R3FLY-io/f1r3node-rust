use std::sync::atomic::Ordering;

use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::interpreter_util;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;
use tokio::time::{timeout, Duration};

use super::recovery_cycle_spec::{
    build_d3_vault_conflict_siblings, propose_d3_vault_rejecting_merge,
};
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
        .contains_sig(sig)
        .expect("buffer.contains_sig")
}

async fn wait_for_finalizer_quiescence(node: &TestNode) {
    timeout(Duration::from_secs(30), async {
        let mut consecutive_quiescent_samples = 0;
        loop {
            let quiescent = !node
                .casper
                .finalizer_task_in_progress
                .load(Ordering::SeqCst)
                && !node.casper.finalizer_task_queued.load(Ordering::SeqCst)
                && !node.casper.finalization_in_progress.load(Ordering::SeqCst);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn finalized_base_win_is_not_buffered_by_later_scope_tombstone() {
    let ctx = TestContext::new().await;
    let mut fixture = build_d3_vault_conflict_siblings(&ctx.genesis).await;
    let outcome = propose_d3_vault_rejecting_merge(&mut fixture, 0).await;
    let winning_validator_index = outcome.recovery_validator_index;
    wait_for_finalizer_quiescence(&fixture.nodes[winning_validator_index]).await;
    fixture.nodes[winning_validator_index]
        .block_dag_storage
        .record_directly_finalized(outcome.winning_block.block_hash.clone(), 1.0, |_| async {
            Ok(())
        })
        .await
        .expect("finalize the winning block before the rejecting merge is seen");
    let merge_block = outcome.merge_block;

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
    let mut fixture = build_d3_vault_conflict_siblings(&ctx.genesis).await;
    let outcome = propose_d3_vault_rejecting_merge(&mut fixture, 0).await;
    let winning_validator_index = outcome.recovery_validator_index;
    let merge_block = outcome.merge_block;

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
    wait_for_finalizer_quiescence(&fixture.nodes[winning_validator_index]).await;

    fixture.nodes[winning_validator_index]
        .block_dag_storage
        .record_directly_finalized(outcome.winning_block.block_hash.clone(), 1.0, |_| async {
            Ok(())
        })
        .await
        .expect("finalize the winning block while the rejection stays above the LFB");
    let finalized_snapshot = fixture.nodes[winning_validator_index]
        .casper
        .get_snapshot()
        .await
        .expect("snapshot after finalizing the winning block");
    assert_eq!(
        finalized_snapshot.last_finalized_block, outcome.winning_block.block_hash,
        "the winning source must be the LFB while the rejecting merge remains unfinalized"
    );
    let buffered = fixture.nodes[winning_validator_index]
        .rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .read_all()
        .expect("read rejected buffer")
        .into_iter()
        .find(|deploy| deploy.sig == outcome.rejected_sig)
        .expect("buffered rejected deploy");
    let next_block_number = finalized_snapshot
        .max_block_num
        .checked_add(1)
        .expect("next block number");
    let earliest_block_number =
        next_block_number - finalized_snapshot.on_chain_state.shard_conf.deploy_lifespan;
    let buffer_scan_floor = buffered
        .data
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
        terminal_sigs.contains(&outcome.rejected_sig),
        "the finalized winning occurrence must be terminal before proposal"
    );

    let marker_deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            1,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(fixture.shard_id.clone()),
        )
        .expect("build post-finality marker deploy")
    };
    fixture.nodes[winning_validator_index]
        .casper
        .deploy(marker_deploy)
        .expect("store post-finality marker deploy");
    let proposal_snapshot = fixture.nodes[winning_validator_index]
        .casper
        .get_snapshot()
        .await
        .expect("snapshot for post-finality proposal");
    assert_eq!(
        proposal_snapshot.last_finalized_block, outcome.winning_block.block_hash,
        "proposal must build while the winning source is the LFB"
    );
    let next_block = fixture.nodes[winning_validator_index]
        .create_block_unsafe(&[])
        .await
        .expect("validator 0 proposes with the win finalized and the rejection pending");
    assert!(
        !buffer_contains(
            &fixture.nodes[winning_validator_index],
            &outcome.rejected_sig
        ),
        "block creation must purge the finalized winning occurrence"
    );
    assert!(
        next_block
            .body
            .rejected_deploys
            .iter()
            .all(|rejected| rejected.sig != outcome.rejected_sig),
        "the child block must not carry a rejection for its finalized-base occurrence"
    );
    let processing = fixture.nodes[winning_validator_index]
        .process_block(next_block.clone())
        .await
        .expect("process the post-finality marker block");
    assert!(
        matches!(processing, Either::Right(_)),
        "the post-finality marker block must validate"
    );

    assert!(
        !buffer_contains(
            &fixture.nodes[winning_validator_index],
            &outcome.rejected_sig
        ),
        "finalized-base sig {} must be purged despite scope tombstone {} \
         (proposed block: {})",
        hex::encode(&outcome.rejected_sig),
        hex::encode(&merge_block.block_hash),
        hex::encode(&next_block.block_hash)
    );
}
