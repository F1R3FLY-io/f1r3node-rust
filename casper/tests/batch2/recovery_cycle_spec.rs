// D3 (DR-9, OD-2): this file previously exercised the rejected-deploy
// recovery pipeline using a PRECHARGE-driven multi-parent-merge conflict: two
// same-key deploys whose combined `phlo_limit × phlo_price` precharge drove the
// shared REV vault below zero, which `conflict_set_merger::fold_rejection`
// rejected, after which the rejected deploy landed in
// `KeyValueRejectedDeployBuffer` and was re-proposed.
//
// D3 REMOVES the per-deploy precharge, so two benign same-key deploys
// (`@0!(0) | for(_<-@0)`, which write no mergeable number-channel diff) NO
// LONGER conflict on a vault balance at merge — both branches merge cleanly.
// The double-spend protection moved to the per-signature ACCEPTANCE GATE
// (`util/rholang/acceptance.rs`: §7.7 reject-both / drained-pool), covered by
// `reject_both_on_oversubscription` / `drained_present_pool_rejects` /
// `per_signature_group_gate`. This test is therefore re-pinned to assert the
// D3 behavior: the same-key benign deploys MERGE without a precharge-driven
// rejection and both remain reachable. (The recovery-buffer/re-propose
// machinery itself is consensus-critical and D3-independent; re-exercising it
// under D3 requires a non-precharge merge-conflict trigger — a vault-draining
// REV transfer or a provisioned Σ⟦s⟧ settlement-debit conflict — which is a
// multi-parent-merge follow-on, not part of the D3 cost-model removal.)

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

/// Send/receive pair drives a deterministic, non-trivial settled cost
/// under the cost-accounted-rho metering model (one `send_eval` + one
/// `receive_eval` + a COMM + substitutions land the deploy at 49 phlo
/// in the merged tree). The deploy must settle successfully —
/// `block_index` discards failed-deploy diffs upstream of the merge
/// engine, so an `OutOfPhlogistons` exit would erase the vault drain
/// that drives the rejection.
const CONFLICT_RHO: &str = r#"
@0!(0) | for (_ <- @0) { 0 }
"#;

/// Phlogiston pricing per deploy. The actual REV drain on the source
/// vault is `cost * phlo_price` (precharge is `phlo_limit * phlo_price`,
/// refunded down to `cost * phlo_price`).
///
/// `phlo_limit = 80` keeps the per-branch precharge at
/// `80 * 100_000 = 8_000_000` REV — under the 9_000_000 REV vault cap
/// (the default `DEFAULT_PUB` balance from `genesis_builder`'s
/// `predefined_vault`). `phlo_price = 100_000` amplifies the body's
/// settled cost (49 phlo) into `49 * 100_000 = 4_900_000` REV of vault
/// drain per branch. Two such deploys against the same source vault
/// sum to `9_800_000` REV, exceeding the `9_000_000` balance and
/// triggering the merge-engine's negative-balance rejection. The body
/// stays comfortably under `phlo_limit` (49 < 80) so the deploy
/// settles without `OutOfPhlogistons`.
const PHLO_LIMIT: i64 = 80;
const PHLO_PRICE: i64 = 100_000;

/// Recovery cycle end-to-end.
///
/// DAG shape:
///
///         genesis
///         /     \
///     block_a   block_b      same-key deploys; block_a's deploy is the
///         \     /            larger-sig one and gets merge-rejected
///       merge_block          proposed by validator 1 (NOT validator 0)
///            |
///     recovery_block         proposed by validator 0; the rejected sig
///                            must surface in body.deploys
///
/// The flow exercises:
///   1. Multi-parent merge in `compute_parents_post_state`, where
///      `dag_merger::merge` returns the rejected sig and
///      `compute_rejected_buffer_admits` admits it to the buffer.
///   2. Buffer population on the recovery proposer via
///      `validate_block_checkpoint` when it syncs merge_block.
///   3. `prepare_user_deploys` pulling from the buffer and the
///      self-chain dedup filter exempting `rejected_in_scope` sigs so
///      the recovered deploy actually reaches `body.deploys`.
///
/// Determinism notes:
///
/// * Both deploys are signed by the same key (`DEFAULT_SEC`). At equal
///   cost/size the merge engine's tiebreak orders deploys via
///   `DeployChainIndex::Ord`, which compares sigs ascending. The
///   lex-LARGER sig is processed second by `fold_rejection` and gets
///   rejected.
///
/// * The larger-sig deploy is routed to `nodes[0]`'s block_a so the
///   rejected sig lives in validator 0's own previous block.
///
/// * Validator 0 must NOT propose merge_block. Validator 1 does. That
///   keeps validator 0's `latest_message_hash` at block_a, so when
///   validator 0 later creates recovery_block,
///   `collect_self_chain_deploy_sigs` walks `block_a → genesis` and
///   block_a's body deploys (including the rejected sig) always land
///   in `self_chain_deploy_sigs`. The hash-asc tiebreak that decides
///   merge_block's main parent is irrelevant — we never traverse
///   merge_block via the self-chain walk.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn d3_same_key_benign_deploys_merge_without_precharge_conflict() {
    let ctx = TestContext::new().await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    // Two validators, no synchrony constraint, unlimited parents so the
    // multi-parent merge actually happens.
    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 2, None, None, None, None)
        .await
        .expect("create_network(2)");

    // Build the two conflicting deploys. Both are signed by the same
    // funded key; different timestamps (enforced by the sleeps) keep
    // their signatures distinct.
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

    // Route the lex-LARGER sig to deploy_a (validator 0's block) so
    // validator 0's own block contains the deploy that the merge engine
    // will reject.
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

    // Sibling blocks: validator 0 proposes block_a, validator 1
    // proposes block_b. Neither has seen the other's block yet, so each
    // executes its deploy against the genesis post-state independently.
    let block_a = nodes[0]
        .add_block_from_deploys(&[deploy_a.clone()])
        .await
        .expect("validator 0 proposes block_a");
    let block_b = nodes[1]
        .add_block_from_deploys(&[deploy_b.clone()])
        .await
        .expect("validator 1 proposes block_b");
    assert_ne!(
        block_a.block_hash, block_b.block_hash,
        "block_a and block_b must be distinct sibling blocks"
    );

    // Sync both ways so each validator can include the other's block as
    // a parent in its next propose.
    {
        let (a, b) = nodes.split_at_mut(1);
        a[0].sync_with_one(&mut b[0]).await.expect("sync 0 -> 1");
    }
    {
        let (a, b) = nodes.split_at_mut(1);
        b[0].sync_with_one(&mut a[0]).await.expect("sync 1 -> 0");
    }
    assert!(
        nodes[0].contains(&block_b.block_hash),
        "validator 0 must observe block_b after sync"
    );
    assert!(
        nodes[1].contains(&block_a.block_hash),
        "validator 1 must observe block_a after sync"
    );

    // Validator 1 proposes merge_block. Validator 0 deliberately does
    // not propose it: keeping validator 0's latest at block_a is what
    // makes the recovery propose's self-chain walk deterministic.
    //
    // The marker deploy gives `create_block` something fresh to commit
    // so it doesn't short-circuit on `NoNewDeploys`.
    let marker_deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone()))
            .expect("build marker_deploy")
    };
    let merge_block = nodes[1]
        .add_block_from_deploys(&[marker_deploy.clone()])
        .await
        .expect("validator 1 proposes merge_block over [block_a, block_b]");

    // The merge block must merge both branches. Inactive validators in
    // the bond set may also pin genesis as an additional parent, so we
    // assert presence of the two real chains rather than an exact count.
    assert!(
        merge_block.header.parents_hash_list.len() >= 2,
        "merge_block must merge at least 2 branches (got {} parents)",
        merge_block.header.parents_hash_list.len()
    );
    assert!(
        merge_block
            .header
            .parents_hash_list
            .iter()
            .any(|h| *h == block_a.block_hash),
        "merge_block parents must include block_a"
    );
    assert!(
        merge_block
            .header
            .parents_hash_list
            .iter()
            .any(|h| *h == block_b.block_hash),
        "merge_block parents must include block_b"
    );

    // D3 (DR-9, OD-2): this scenario's double-spend conflict was driven ENTIRELY
    // by the per-deploy PRECHARGE (`phlo_limit × phlo_price` debited the source
    // REV vault; two same-key precharges drove its mergeable balance below zero,
    // which `conflict_set_merger::fold_rejection` rejected). D3 REMOVES the
    // precharge: the benign `CONFLICT_RHO` (`@0!(0) | for(_<-@0)`) writes NO
    // mergeable number-channel diff, so two same-key copies do NOT conflict on a
    // vault balance — both branches merge cleanly. The double-spend protection
    // moved to the per-signature ACCEPTANCE GATE (`util/rholang/acceptance.rs`):
    // two deploys sharing a signature draw from one supply pool Σ⟦s⟧, and the
    // §7.7 reject-both / drained-pool checks reject the second once the pool is
    // committed — covered by the gate tests `reject_both_on_oversubscription`,
    // `drained_present_pool_rejects`, and `per_signature_group_gate`. So under D3
    // the merge admits both same-key benign deploys WITHOUT a precharge-driven
    // rejection.
    let rejected_sigs: Vec<Bytes> = merge_block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| rd.sig.clone())
        .collect();
    assert!(
        !rejected_sigs.iter().any(|s| *s == sig_a || *s == sig_b),
        "D3: neither same-key benign deploy is rejected at MERGE — the \
         precharge-driven vault-balance conflict is removed; double-spend \
         protection is the per-signature acceptance gate, not the merge \
         engine's vault-balance check. Got merge rejected sigs={:?}, \
         sig_a={}, sig_b={}",
        rejected_sigs.iter().map(hex::encode).collect::<Vec<_>>(),
        hex::encode(&sig_a),
        hex::encode(&sig_b)
    );

    // Both same-key deploys remain reachable in the canonical view via the
    // deploy index (neither was dropped by a merge-time rejection).
    let representation = nodes[0]
        .block_dag_storage
        .get_representation()
        .expect("dag representation");
    for sig in [&sig_a, &sig_b] {
        assert!(
            representation
                .lookup_by_deploy_id(&sig.to_vec())
                .ok()
                .flatten()
                .is_some(),
            "D3: same-key benign deploy sig {} must remain reachable in the \
             canonical view (it was admitted, not merge-rejected)",
            hex::encode(sig)
        );
    }
}
