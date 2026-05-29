// Regression test for the proposer-side InvalidRepeatDeploy cycle observed
// in `test_bonding_validators` integration attempt 5 (May 2026). After a
// legitimate recovery (a merge-rejected deploy is re-applied by a
// subsequent block), the next propose round attempted to re-include the
// same deploy *again*. The block self-validated with InvalidRepeatDeploy
// and the proposer logged "proposal conditions no longer met, skipping
// propose" — repeatedly, until parent shapes moved on.
//
// Empirical trace from validator1.log (attempt 5):
//
//     [FILTER-DECISION-KEEP] block_number=16 sig=3044022005b42351
//         in_scope=true in_rejected=true in_stale=false
//         reason=admit-back-rejected-not-stale
//     [APPLIED-SIGS-BLOCK-CREATE] block_number=16 merged_pre_size=16
//         body_deploys=1 post_size=16            ← body sig was already in pre
//     [REPEAT-DEPLOY-REJECT] sig=3044022005b42351 block=15ead8dbb591c35e
//         block_num=16 recorded_height=15 reason=applied_sigs_hit
//
// Same pattern, 5 times across validator1 and validator2.
//
// Two-filter mechanism in `prepare_user_deploys`:
//
//   1. Filter 1 (`deploys_in_scope` admit-back, block_creator.rs:155-280)
//      sees the sig in both `deploys_in_scope` and `rejected_in_scope`,
//      asks the legacy `resolve_at_parents_batch` resolver, gets
//      `RejectedCanonically` (the legacy resolver classifies any
//      unfinalized clean-inclusion-with-ancestor-rejection as
//      RejectedCanonically — LFB-dependent and wrong here), admits the
//      sig back via the `rejected_in_scope` exemption.
//
//   2. Filter 2 (applied_sigs guard, block_creator.rs:345-379) sees the
//      sig in the parents' applied_sigs union. The `rejected_in_scope`
//      exemption (line 357) fires silently — no `APPLIED-SIGS-PROPOSER-
//      FILTER` log — and the sig stays in `valid_unique`.
//
//   3. Self-validation runs `Validate::repeat_deploy` (validate.rs:418)
//      which recomputes the merged pre-state's `applied_sigs` over
//      *effective* parents (with `dag.ancestors`-based dedup and the
//      `merge_pre_state` rule) and finds the sig. InvalidRepeatDeploy.
//
// The cleanest fix is at Filter 2: drop the `rejected_in_scope`
// exemption, because Phase 1's merge-integrated `applied_sigs` already
// handles legitimate recovery — a sig that was merge-rejected by an
// ancestor is NOT in that ancestor's descendant's `applied_sigs` (the
// merge subtraction handled it), so the legitimate-recovery case passes
// Filter 2 without an exemption. The bug case (sig was merge-rejected
// in some ancestor but ALSO re-applied by a later canonical block) is
// exactly what Filter 2 must catch — the descendant's `applied_sigs`
// carries the recent canonical application.
//
// This test reproduces the bug shape and asserts the proposer does NOT
// re-include a sig that's already been recovered. End-to-end through
// the real propose pipeline: `block_creator::create` + the full block
// processor (which runs `Validate::repeat_deploy`).

use casper::rust::util::construct_deploy;
use prost::bytes::Bytes;
use serial_test::serial;

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

/// Trivial Rholang body. The conflict comes from the system-level
/// precharge against the source vault, not the body. Same as
/// `recovery_cycle_spec` — we reuse its conflict-generation pattern.
const CONFLICT_RHO: &str = r#"
Nil
"#;

/// Per-deploy phlogiston pricing. Combined precharge of two such deploys
/// against the same vault drives the balance below zero, which
/// `conflict_set_merger::fold_rejection` catches at the multi-parent
/// merge by dropping the lex-larger branch.
const PHLO_LIMIT: i64 = 8;
const PHLO_PRICE: i64 = 1_000_000;

/// Post-recovery propose must not re-include an already-recovered sig.
///
/// DAG shape (same setup as `recovery_cycle_spec`, then one more block):
///
///         genesis
///         /     \
///     block_a   block_b              same-key conflicting deploys;
///         \     /                    block_a's larger sig gets merge-rejected
///       merge_block                  proposed by validator 1
///            |
///     recovery_block                 proposed by validator 0;
///                                    re-applies the rejected sig
///            |
///   post_recovery_block              proposed by validator 0;
///                                    MUST NOT re-include the sig
///
/// After `recovery_block` is created, three conditions hold simultaneously
/// on validator 0:
///
///   1. The rejected sig is in `casper_snapshot.deploys_in_scope`
///      (its BFS over the scope window finds the sig in `block_a.body`
///      and `recovery_block.body`).
///   2. The rejected sig is in `casper_snapshot.rejected_in_scope`
///      (the same BFS finds it in `merge_block.body.rejected_deploys`).
///   3. The rejected sig is in `recovery_block.body.state.applied_sigs`
///      (Phase 1's merge-integrated post-state aggregation puts it there
///      with `height = recovery_block.block_number`).
///
/// Conditions 1+2 satisfy Filter 1's admit-back exemption. The legacy
/// `resolve_at_parents_batch` returns `RejectedCanonically` because LFB
/// hasn't advanced past the clean re-application (the recovery is still
/// unfinalized). Filter 1 admits the sig back. Condition 3 then triggers
/// Filter 2's applied_sigs check; with the `rejected_in_scope` exemption,
/// the sig stays in the body. The block self-validates as
/// `InvalidRepeatDeploy` and `add_block_from_deploys` returns an error.
///
/// After removing Filter 2's exemption, the sig is dropped by Filter 2
/// and the block proposes cleanly with only the marker deploy.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn post_recovery_propose_does_not_re_include_recovered_sig() {
    let ctx = TestContext::new().await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 2, None, None, None, None)
        .await
        .expect("create_network(2)");

    // --- Phase 1: build the conflict (same pattern as recovery_cycle_spec) ---

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

    // Route the lex-larger sig to validator 0's branch so the merge
    // rejection lands on validator 0's prior latest.
    let (deploy_a, deploy_b) = if deploy_x.sig >= deploy_y.sig {
        (deploy_x, deploy_y)
    } else {
        (deploy_y, deploy_x)
    };
    let sig_a: Bytes = deploy_a.sig.clone();
    let sig_b: Bytes = deploy_b.sig.clone();
    assert!(
        sig_a > sig_b,
        "deploy_a must hold the lex-larger sig so the negative-balance \
         merge rejection picks validator 0's deploy"
    );

    let block_a = nodes[0]
        .add_block_from_deploys(&[deploy_a.clone()])
        .await
        .expect("validator 0 proposes block_a");
    let block_b = nodes[1]
        .add_block_from_deploys(&[deploy_b.clone()])
        .await
        .expect("validator 1 proposes block_b");
    assert_ne!(block_a.block_hash, block_b.block_hash);

    // Cross-sync so each side sees the other's block as a parent.
    {
        let (a, b) = nodes.split_at_mut(1);
        a[0].sync_with_one(&mut b[0]).await.expect("sync 0 -> 1");
    }
    {
        let (a, b) = nodes.split_at_mut(1);
        b[0].sync_with_one(&mut a[0]).await.expect("sync 1 -> 0");
    }

    // --- Phase 2: merge_block (validator 1) rejects deploy_a ---

    let marker_merge = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone()))
            .expect("build marker_merge")
    };
    let merge_block = nodes[1]
        .add_block_from_deploys(&[marker_merge.clone()])
        .await
        .expect("validator 1 proposes merge_block over [block_a, block_b]");

    let rejected_sigs: Vec<Bytes> = merge_block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| rd.sig.clone())
        .collect();
    assert_eq!(
        rejected_sigs.iter().find(|s| **s == sig_a || **s == sig_b),
        Some(&sig_a),
        "merge_block must reject deploy_a (the lex-larger sig)"
    );
    let conflict_sig = sig_a.clone();

    // Sync merge_block back to validator 0; its rejected-deploy buffer
    // picks up `conflict_sig`.
    {
        let (a, b) = nodes.split_at_mut(1);
        a[0].sync_with_one(&mut b[0])
            .await
            .expect("sync merge_block 1 -> 0");
    }

    // --- Phase 3: recovery_block (validator 0) re-applies deploy_a ---

    let marker_recovery = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(1, None, Some(shard_id.clone()))
            .expect("build marker_recovery")
    };
    let recovery_block = nodes[0]
        .add_block_from_deploys(&[marker_recovery.clone()])
        .await
        .expect("validator 0 proposes recovery_block");
    assert!(
        recovery_block
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == conflict_sig),
        "recovery_block must re-include the recovered sig (sanity check on the \
         recovery_cycle setup; if this fails, the bug under test is upstream \
         of this regression)"
    );
    assert!(
        recovery_block
            .body
            .state
            .applied_sigs
            .contains_key(&conflict_sig),
        "recovery_block.body.state.applied_sigs must contain the recovered sig \
         (Phase 1 post-state aggregation); without this, condition 3 of the \
         bug shape isn't reproduced"
    );

    // --- Phase 4: post-recovery propose must NOT cycle on the recovered sig ---

    let marker_post_recovery = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(2, None, Some(shard_id.clone()))
            .expect("build marker_post_recovery")
    };
    let post_recovery_block = nodes[0]
        .add_block_from_deploys(&[marker_post_recovery.clone()])
        .await
        .expect(
            "validator 0 must propose post_recovery_block cleanly. \
             If this fails with InvalidRepeatDeploy on the conflict sig, \
             the proposer admitted the already-recovered sig back into \
             body.deploys and self-validation rejected — this is the \
             bonding-test cycling bug from attempt 5",
        );

    assert!(
        !post_recovery_block
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == conflict_sig),
        "post_recovery_block.body.deploys must NOT contain the recovered sig \
         a SECOND time — recovery_block already applied it at height {}, \
         re-inclusion is double-execution. Got body.deploys sigs = {:?}",
        recovery_block.body.state.block_number,
        post_recovery_block
            .body
            .deploys
            .iter()
            .map(|pd| hex::encode(&pd.deploy.sig))
            .collect::<Vec<_>>(),
    );
}
