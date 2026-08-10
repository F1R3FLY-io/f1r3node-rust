// Regression for the finalized-win blindness in rejected-deploy recovery
// (root-caused from integration run 31150220859, test_bridge_api_real_deploy):
// a deploy whose winning inclusion block has FINALIZED can still be
// conflict-rejected by a later multi-parent merge sitting ABOVE the LFB.
// Block finalization does not finalize a deploy's effects — once the
// rejecting block finalizes, canonical state has dropped the deploy, so
// the recovery pipeline must keep it re-proposable. Two sites instead
// treat "finalized canonical win" as terminal while being unable to see
// the pending rejection above the LFB:
//
//   1. `retain_pending_rejected_deploys_for_buffer` (populate filter):
//      `deploy_finalization_status::resolve_batch` scans only the
//      finalized chain, reports `Finalized` from the win, and populate
//      skips the deploy — even though the populate call exists precisely
//      because a merge just rejected it.
//   2. The terminal purge in `block_creator::prepare_user_deploys`:
//      `canonical_won_sigs` is walked from `last_finalized_block` only,
//      so the in-scope rejection is invisible and the buffer entry is
//      dropped as "finalized canonical win".
//
use casper::rust::util::construct_deploy;
use prost::bytes::Bytes;
use serial_test::serial;

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
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
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

/// Site 1 — populate filter. The winning block finalizes BEFORE the
/// rejecting merge arrives. When validator 0 validates the merge, the
/// populate path must still buffer the rejected deploy: the rejection sits
/// above the LFB, so the "Finalized" the resolver reports from the win is
/// not a terminal disposition — it is exactly the state this rejection is
/// about to overturn.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn populate_buffers_rejected_deploy_whose_win_block_already_finalized() {
    let ctx = TestContext::new().await;
    let mut fixture = build_d3_vault_conflict_siblings(&ctx.genesis).await;

    fixture.nodes[0]
        .block_dag_storage
        .record_directly_finalized(fixture.winning_block.block_hash.clone(), 1.0, |_| async {
            Ok(())
        })
        .await
        .expect("finalize the winning block before the rejecting merge is seen");

    let merge_block = propose_d3_vault_rejecting_merge(&mut fixture, 0).await;

    {
        let (a, b) = fixture.nodes.split_at_mut(1);
        a[0].sync_with_one(&mut b[0])
            .await
            .expect("sync merge_block 1 -> 0");
    }
    assert!(
        fixture.nodes[0].contains(&merge_block.block_hash),
        "validator 0 must validate the rejecting merge"
    );

    assert!(
        buffer_contains(&fixture.nodes[0], &fixture.rejected_sig),
        "populate must buffer sig {} despite its win block being finalized: \
         the rejection above the LFB is pending, and once the rejecting block \
         finalizes the deploy's effects are dropped from canonical state with \
         no re-proposable copy",
        hex::encode(&fixture.rejected_sig)
    );
}

/// Site 2 — terminal purge. The deploy is buffered normally (win block
/// not yet finalized when the rejection arrives, as in
/// `recovery_cycle_spec`), and only THEN does the winning block finalize
/// while the rejecting merge stays above the LFB. The next proposal's
/// buffer scan must keep the entry: the in-scope rejection can still
/// finalize and orphan the win's effects from canonical state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn purge_keeps_buffered_deploy_with_pending_rejection_above_finalized_win() {
    let ctx = TestContext::new().await;
    let mut fixture = build_d3_vault_conflict_siblings(&ctx.genesis).await;

    let merge_block = propose_d3_vault_rejecting_merge(&mut fixture, 0).await;

    {
        let (a, b) = fixture.nodes.split_at_mut(1);
        a[0].sync_with_one(&mut b[0])
            .await
            .expect("sync merge_block 1 -> 0");
    }
    assert!(
        buffer_contains(&fixture.nodes[0], &fixture.rejected_sig),
        "precondition (proven by recovery_cycle_spec): with the win block \
         unfinalized, populate buffers the rejected sig"
    );

    fixture.nodes[0]
        .block_dag_storage
        .record_directly_finalized(fixture.winning_block.block_hash.clone(), 1.0, |_| async {
            Ok(())
        })
        .await
        .expect("finalize the winning block while the rejection stays above the LFB");

    let marker_deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            1,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(fixture.shard_id.clone()),
        )
        .expect("build post-finality marker deploy")
    };
    let next_block = fixture.nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&marker_deploy))
        .await
        .expect("validator 0 proposes with the win finalized and the rejection pending");

    assert!(
        buffer_contains(&fixture.nodes[0], &fixture.rejected_sig),
        "the buffer entry for sig {} must survive the terminal purge while \
         its rejection in {} is above the LFB: \"finalized canonical win\" is \
         not terminal when a pending rejection can still overturn it \
         (proposed block: {})",
        hex::encode(&fixture.rejected_sig),
        hex::encode(&merge_block.block_hash),
        hex::encode(&next_block.block_hash)
    );
}
