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
// Conflict generator and DAG choreography mirror `recovery_cycle_spec`:
// two same-key deploys whose combined precharge over-drains the shared
// vault; `conflict_set_merger::fold_rejection` deterministically rejects
// the lex-larger sig, which is routed to validator 0's block_a.

use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new() -> Self {
        let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(2));
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(parameters))
            .await
            .unwrap();

        Self { genesis }
    }
}

/// Same conflict shape as `recovery_cycle_spec`: trivial body, conflict
/// comes from the system-level precharge against the shared source vault.
const CONFLICT_RHO: &str = r#"
Nil
"#;
const PHLO_LIMIT: i64 = 8;
const PHLO_PRICE: i64 = 1_000_000;

struct ConflictFixture {
    nodes: Vec<TestNode>,
    shard_id: String,
    block_a: BlockMessage,
    rejected_sig: Bytes,
}

/// Build the sibling-conflict DAG up to (but not including) the merge:
/// block_a on validator 0 carries the lex-larger (to-be-rejected) deploy,
/// block_b on validator 1 the survivor; both validators see both blocks.
async fn build_conflict_siblings(ctx: &TestContext) -> ConflictFixture {
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 2, None, None, None, None)
        .await
        .expect("create_network(2)");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let deploy_x = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            CONFLICT_RHO.to_string(),
            Some(PHLO_LIMIT),
            Some(PHLO_PRICE),
            Some(construct_deploy::DEFAULT_SEC.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("build deploy_x")
    };
    let deploy_y = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            CONFLICT_RHO.to_string(),
            Some(PHLO_LIMIT),
            Some(PHLO_PRICE),
            Some(construct_deploy::DEFAULT_SEC.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("build deploy_y")
    };

    let (deploy_a, deploy_b) = if deploy_x.sig >= deploy_y.sig {
        (deploy_x, deploy_y)
    } else {
        (deploy_y, deploy_x)
    };
    let rejected_sig: Bytes = deploy_a.sig.clone();
    assert!(
        deploy_a.sig > deploy_b.sig,
        "deploy_a must hold the lex-larger sig so the merge rejection is deterministic"
    );

    let block_a = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy_a))
        .await
        .expect("validator 0 proposes block_a");
    let block_b = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&deploy_b))
        .await
        .expect("validator 1 proposes block_b");
    assert_ne!(block_a.block_hash, block_b.block_hash);

    {
        let (a, b) = nodes.split_at_mut(1);
        a[0].sync_with_one(&mut b[0]).await.expect("sync 0 -> 1");
    }
    {
        let (a, b) = nodes.split_at_mut(1);
        b[0].sync_with_one(&mut a[0]).await.expect("sync 1 -> 0");
    }

    ConflictFixture {
        nodes,
        shard_id,
        block_a,
        rejected_sig,
    }
}

/// Validator 1 proposes the merge over [block_a, block_b]; the fixture's
/// lex-larger deploy must be in its `rejected_deploys`.
async fn propose_rejecting_merge(fixture: &mut ConflictFixture, nonce: i32) -> BlockMessage {
    let marker_deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(nonce, None, Some(fixture.shard_id.clone()))
            .expect("build marker deploy")
    };
    let merge_block = fixture.nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&marker_deploy))
        .await
        .expect("validator 1 proposes merge over [block_a, block_b]");
    assert!(
        merge_block
            .body
            .rejected_deploys
            .iter()
            .any(|rd| rd.sig == fixture.rejected_sig),
        "merge block must conflict-reject the lex-larger sig {}",
        hex::encode(&fixture.rejected_sig)
    );
    merge_block
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
    let mut fixture = build_conflict_siblings(&ctx).await;

    fixture.nodes[0]
        .block_dag_storage
        .record_directly_finalized(fixture.block_a.block_hash.clone(), 1.0, |_| async {
            Ok(())
        })
        .await
        .expect("finalize block_a (the win) before the rejecting merge is seen");

    let merge_block = propose_rejecting_merge(&mut fixture, 0).await;

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
    let mut fixture = build_conflict_siblings(&ctx).await;

    let merge_block = propose_rejecting_merge(&mut fixture, 0).await;

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
        .record_directly_finalized(fixture.block_a.block_hash.clone(), 1.0, |_| async {
            Ok(())
        })
        .await
        .expect("finalize block_a (the win) while the rejection stays above the LFB");

    let marker_deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(1, None, Some(fixture.shard_id.clone()))
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn finalized_noncanonical_deploy_is_reproposed_after_canonical_rejection() {
    let ctx = TestContext::new().await;
    let mut fixture = build_conflict_siblings(&ctx).await;

    fixture.nodes[0]
        .block_dag_storage
        .record_directly_finalized(fixture.block_a.block_hash.clone(), 1.0, |_| async {
            Ok(())
        })
        .await
        .expect("finalize the rejected deploy carrier");

    let merge_block = propose_rejecting_merge(&mut fixture, 0).await;
    {
        let (a, b) = fixture.nodes.split_at_mut(1);
        a[0].sync_with_one(&mut b[0])
            .await
            .expect("sync the rejecting merge");
    }
    fixture.nodes.sort_by(|left, right| {
        left.validator_id_opt
            .as_ref()
            .expect("left validator identity")
            .public_key
            .bytes
            .cmp(
                &right
                    .validator_id_opt
                    .as_ref()
                    .expect("right validator identity")
                    .public_key
                    .bytes,
            )
    });

    fixture.nodes[0]
        .block_dag_storage
        .record_directly_finalized(merge_block.block_hash.clone(), 1.0, |_| async { Ok(()) })
        .await
        .expect("finalize the canonical rejecting merge");

    let marker_deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(1, None, Some(fixture.shard_id.clone()))
            .expect("build recovery marker deploy")
    };
    let recovery_block = fixture.nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&marker_deploy))
        .await
        .expect("propose the recovery block");

    assert!(
        recovery_block
            .body
            .deploys
            .iter()
            .any(|processed| processed.deploy.sig == fixture.rejected_sig),
        "deploy {} must be re-proposed after the canonical merge rejects its finalized noncanonical carrier",
        hex::encode(&fixture.rejected_sig)
    );
}
