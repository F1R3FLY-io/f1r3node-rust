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
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
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
/// engine, so an `OutOfPhlogistons` exit would erase the settlement
/// diffs these merge tests exercise. This is a BENIGN contract: it
/// transfers no REV, so under D3 it drives no merge-time vault rejection.
const CONFLICT_RHO: &str = r#"
@0!(0) | for (_ <- @0) { 0 }
"#;

/// Phlogiston pricing per deploy. RETAINED only as (ignored) parameters for
/// `source_deploy_now_full`'s signature stability — under D3 (DR-9) a deploy
/// carries no phlo price/limit and there is NO precharge, so these values do
/// NOT drive a REV vault drain. `DeployData` has no phlo fields, and the
/// per-signature settlement demand is a static O(AST) analysis against Σ⟦s⟧ at
/// the acceptance gate, independent of `phlo_price`.
///
/// (Pre-D3 these drove a `cost * phlo_price` precharge — `phlo_price = 100_000`
/// amplifying the 49-phlo body into `4_900_000` REV of vault drain per branch,
/// so two deploys' `9_800_000` REV exceeded the `9_000_000` vault and triggered
/// the merge-engine's negative-balance rejection. That precharge model is
/// removed; the merge tests below assert the D3 outcome instead.)
const PHLO_LIMIT: i64 = 80;
const PHLO_PRICE: i64 = 100_000;

fn assert_touched_integer_add_channels_single_valued(
    node: &TestNode,
    state_hash: &Bytes,
    blocks: &[BlockMessage],
) {
    let mut channels = std::collections::BTreeMap::new();
    for block in blocks {
        let diffs = node
            .runtime_manager
            .load_mergeable_channels(
                &block.body.state.post_state_hash,
                block.sender.clone(),
                block.seq_num,
            )
            .expect("load mergeable channels");
        for diff in diffs {
            for (hash, (_, merge_type)) in diff {
                if merge_type == MergeType::IntegerAdd {
                    channels.insert(hash, merge_type);
                }
            }
        }
    }

    let root = Blake2b256Hash::from_bytes_prost(state_hash);
    let reader = node
        .runtime_manager
        .get_history_repo()
        .get_history_reader(&root)
        .expect("history reader");

    for (hash, _) in channels {
        let data = reader.get_data(&hash).expect("get mergeable channel data");
        let values: Vec<i64> = data
            .iter()
            .filter_map(|datum| {
                RholangMergingLogic::try_get_number_with_rnd(&datum.a).map(|(n, _)| n)
            })
            .collect();
        assert!(
            data.len() <= 1,
            "number channel {} holds {} values {:?}; IntegerAdd single-value invariant violated",
            hex::encode(hash.bytes()),
            data.len(),
            values
        );
        if data.len() == 1 {
            assert_eq!(
                values.len(),
                1,
                "number channel {} is not numeric at merged state",
                hex::encode(hash.bytes())
            );
        }
    }
}

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
        .add_block_from_deploys(std::slice::from_ref(&deploy_a))
        .await
        .expect("validator 0 proposes block_a");
    let block_b = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&deploy_b))
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
        .add_block_from_deploys(std::slice::from_ref(&marker_deploy))
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
            .contains(&block_a.block_hash),
        "merge_block parents must include block_a"
    );
    assert!(
        merge_block
            .header
            .parents_hash_list
            .contains(&block_b.block_hash),
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

/// D3 (DR-9): three sibling blocks, one same-payer deploy each. Under the
/// cost-accounted model precharge is removed, so a same-payer set does NOT
/// over-spend a shared REV vault at merge (the precharge-era premise of this
/// test — hence 0, not 2, user rejections). Funding settles per-signature at
/// the acceptance gate: each sibling's lone deploy is individually fundable
/// against the full genesis Σ⟦signer⟧, so none is gate-rejected, and the
/// inherited deploys are merged (not re-gated). The merge still keeps the
/// touched purse single-valued and stays LIVE by dropping the redundant SYSTEM
/// `CloseBlockDeploy` settlement branches (system chains — never mapped into
/// user `rejected_deploys`). This mirrors the rejection count of
/// `d3_same_key_benign_deploys_merge_without_precharge_conflict` while adding
/// 3-validator post-merge liveness coverage. A genuine value-draining
/// cross-sibling settlement conflict (an `IntegerAdd` / Σ⟦s⟧-draining transfer)
/// is the separate D3 follow-on noted at the top of this file, which this
/// benign contract deliberately cannot exercise.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn three_validator_same_payer_merge_keeps_purses_single_valued_and_live() {
    let ctx = TestContext::new().await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let mut deploys = Vec::new();
    for _ in 0..3 {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        deploys.push(
            construct_deploy::source_deploy_now_full(
                CONFLICT_RHO.to_string(),
                Some(PHLO_LIMIT),
                Some(PHLO_PRICE),
                Some(construct_deploy::DEFAULT_SEC.clone()),
                None,
                Some(shard_id.clone()),
            )
            .expect("build conflicting deploy"),
        );
    }

    let block_0 = nodes[0]
        .add_block_from_deploys(&[deploys[0].clone()])
        .await
        .expect("validator 0 proposes sibling");
    let block_1 = nodes[1]
        .add_block_from_deploys(&[deploys[1].clone()])
        .await
        .expect("validator 1 proposes sibling");
    let block_2 = nodes[2]
        .add_block_from_deploys(&[deploys[2].clone()])
        .await
        .expect("validator 2 proposes sibling");
    let sibling_blocks = vec![block_0, block_1, block_2];

    for (source, block) in sibling_blocks.iter().enumerate() {
        for (target, node) in nodes.iter_mut().enumerate() {
            if source != target {
                node.process_block(block.clone())
                    .await
                    .expect("process sibling");
            }
        }
    }

    for node in &nodes {
        for block in &sibling_blocks {
            assert!(node.contains(&block.block_hash));
        }
    }

    let marker = construct_deploy::basic_deploy_data(
        10_000,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(shard_id.clone()),
    )
    .expect("build merge marker");
    let merge_block = nodes[0]
        .add_block_from_deploys(&[marker])
        .await
        .expect("validator 0 proposes merge");

    for block in &sibling_blocks {
        assert!(
            merge_block
                .header
                .parents_hash_list
                .contains(&block.block_hash),
            "merge block must include sibling {}",
            hex::encode(&block.block_hash)
        );
    }

    let rejected_sigs: Vec<Bytes> = merge_block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| rd.sig.clone())
        .collect();
    let conflicting_rejections = deploys
        .iter()
        .filter(|deploy| rejected_sigs.contains(&deploy.sig))
        .count();
    assert_eq!(
        conflicting_rejections,
        0,
        "D3 (DR-9): precharge is removed, so same-payer siblings do NOT over-spend a \
         shared vault at merge (that was the precharge-era model). Funding settles \
         per-signature at the acceptance gate — each sibling's lone deploy is \
         individually fundable against the full genesis Σ⟦signer⟧, so none is \
         gate-rejected, and the inherited deploys are merged, not re-gated. The merge \
         keeps the purse single-valued by dropping redundant SYSTEM (CloseBlockDeploy) \
         settlement branches, which are never user rejections; rejected={:?}",
        rejected_sigs.iter().map(hex::encode).collect::<Vec<_>>()
    );

    let mut observed_blocks = sibling_blocks.clone();
    observed_blocks.push(merge_block.clone());
    assert_touched_integer_add_channels_single_valued(
        &nodes[0],
        &merge_block.body.state.post_state_hash,
        &observed_blocks,
    );

    for node in nodes.iter_mut().skip(1) {
        node.process_block(merge_block.clone())
            .await
            .expect("process merge block");
        assert_touched_integer_add_channels_single_valued(
            node,
            &merge_block.body.state.post_state_hash,
            &observed_blocks,
        );
    }

    for proposer in 0..3 {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        let traffic = construct_deploy::basic_deploy_data(
            20_000 + proposer as i32,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("build traffic deploy");
        let block = nodes[proposer]
            .add_block_from_deploys(&[traffic])
            .await
            .expect("post-merge validator traffic must propose");
        observed_blocks.push(block.clone());
        assert_touched_integer_add_channels_single_valued(
            &nodes[proposer],
            &block.body.state.post_state_hash,
            &observed_blocks,
        );
        for (target, node) in nodes.iter_mut().enumerate() {
            if target != proposer {
                node.process_block(block.clone())
                    .await
                    .expect("process post-merge traffic");
            }
        }
    }
}

/// Per-branch REV drain: transfer `amount` REV from the caller's own genesis
/// RevVault (`from_addr`) to a distinct existing genesis vault (`to_addr`). This
/// is the D3-compatible, non-precharge merge-conflict trigger the top-of-file
/// note describes. A successful `SystemVault!("transfer", ...)` sprouts a
/// destination purse and `deposit`s into it, which calls `balance!("sub", amount)`
/// on the SOURCE purse — writing a `-amount` diff onto the source vault's
/// `NonNegativeNumber` value store at `@(*MergeableTag, *valueStore)`, an
/// `IntegerAdd` mergeable number channel (its tag is
/// `non_negative_mergeable_tag_name()`, mapped to `MergeType::IntegerAdd` by
/// `default_mergeable_tags()`). Because the source vault is created ONCE at
/// genesis, every sibling branch decrements the SAME channel, so their `-amount`
/// diffs combine at multi-parent merge and are below-zero-checked together by
/// `conflict_set_merger::cal_merged_result`. Mirrors `vault_demo/3.transfer_funds.rho`.
fn transfer_rho(from_addr: &str, to_addr: &str, amount: i64) -> String {
    const TEMPLATE: &str = r#"
new
  rl(`rho:registry:lookup`), SystemVaultCh,
  deployerId(`rho:system:deployerId`),
  vaultCh, targetVaultCh, keyCh, resultCh
in {
  rl!(`rho:vault:system`, *SystemVaultCh) |
  for (@(_, SystemVault) <- SystemVaultCh) {
    @SystemVault!("findOrCreate", "%FROM%", *vaultCh) |
    @SystemVault!("findOrCreate", "%TO%", *targetVaultCh) |
    @SystemVault!("deployerAuthKey", *deployerId, *keyCh) |
    for (@(true, vault) <- vaultCh & key <- keyCh & @(true, _) <- targetVaultCh) {
      @vault!("transfer", "%TO%", %AMOUNT%, *key, *resultCh) |
      for (@_result <- resultCh) { Nil }
    }
  }
}
"#;
    TEMPLATE
        .replace("%FROM%", from_addr)
        .replace("%TO%", to_addr)
        .replace("%AMOUNT%", &amount.to_string())
}

/// Genesis `predefined_vault` balance (`casper/tests/util/genesis_builder.rs`):
/// each default genesis vault (`DEFAULT_PUB`, `DEFAULT_PUB2`) is funded with this
/// many REV. The `DEFAULT_PUB` vault is the shared merge-conflict channel below.
const GENESIS_VAULT_BALANCE: i64 = 9_000_000;

/// Per-transfer REV amount. Chosen so that (a) a SINGLE transfer is solvent
/// against the full `GENESIS_VAULT_BALANCE` (each sibling's in-VM `NonNegativeNumber`
/// `"sub"` sees the untouched genesis base and settles), (b) TWO transfers still
/// fit (`2 × 4_000_000 = 8_000_000 ≤ 9_000_000`), but (c) all THREE over-drain
/// (`3 × 4_000_000 = 12_000_000 > 9_000_000`). So the merge folds two `-4_000_000`
/// diffs onto the `9_000_000` base (→ `1_000_000`) and the THIRD (lex-largest sig,
/// folded last) drives it to `-3_000_000`, which `fold_rejection` rejects — exactly
/// one deterministic merge rejection.
const TRANSFER_AMOUNT: i64 = 4_000_000;

/// D3 (DR-9) end-to-end: vault-draining REV transfers reject at merge and recover.
///
/// This is the D3-compatible successor to the precharge-era recovery test that
/// DR-9 removed. It re-exercises the rejected-deploy RECOVERY pipeline end-to-end
/// through a NON-precharge merge-conflict trigger — a genuine vault-draining REV
/// transfer, exactly the follow-on named in the top-of-file note. Where the old
/// test relied on a per-deploy `phlo_limit × phlo_price` precharge to drain the
/// source vault, here three sibling `SystemVault` transfers each debit the SAME
/// genesis RevVault balance (an `IntegerAdd` mergeable number channel; see
/// [`transfer_rho`]). Each transfer of `TRANSFER_AMOUNT` is individually solvent
/// against the `GENESIS_VAULT_BALANCE`, so each sibling block plays and settles,
/// but their cumulative debit over-drains the vault, so
/// `conflict_set_merger::fold_rejection`'s `current.checked_add(diff) >= 0`
/// IntegerAdd check rejects exactly the last-folded (lex-largest-sig) transfer to
/// keep the merged vault solvent. That rejection is D3-CORRECT: it is the genuine
/// vault-balance conflict, not a precharge artifact — the double-spend acceptance
/// gate is orthogonal and admits each individually-fundable sibling.
///
/// DAG shape (3 validators):
///
///           genesis
///          ╱   │    ╲
///    block_0 block_1 block_2   one transfer each; block_0 (validator 0) holds
///          ╲   │    ╱          the lex-LARGEST sig → the merge-rejected one
///        merge_block           proposed by validator 1 (NOT validator 0)
///             │
///       recovery_block         proposed by validator 0; the rejected sig
///                              must resurface in body.deploys
///
/// Recovery pipeline (consensus-critical, D3-independent):
///   1. multi-parent merge → `dag_merger::merge` returns the rejected sig;
///      `compute_rejected_buffer_admits` admits it (its finalization state is
///      `Pending`) to the buffer.
///   2. validator 0 validates merge_block → `validate_block_checkpoint`
///      populates validator 0's `KeyValueRejectedDeployBuffer`.
///   3. validator 0 proposes recovery_block → `prepare_user_deploys` pulls the
///      buffered sig, and `canonical_won_sigs` exempts it (its highest-block
///      disposition is the REJECTION at merge_block height 2, which overrides the
///      WIN in block_0 at height 1) so it survives the self-chain dedup filter
///      and reaches `body.deploys`.
///
/// Determinism: all three transfers are signed by `DEFAULT_SEC` (same funded
/// payer). `fold_rejection` folds branches in ascending `DeployChainIndex` (sig)
/// order; with `3 × TRANSFER_AMOUNT` the running balance is exhausted exactly as
/// the lex-largest sig is folded, so THAT sig is the deterministic rejection. It
/// is routed to validator 0's `block_0` so validator 0's self-chain walk
/// (`block_0 → genesis`) finds it during recovery, and validator 0 must NOT
/// propose merge_block (that keeps its latest message at `block_0`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn d3_vault_draining_transfers_reject_at_merge_and_recover() {
    let ctx = TestContext::new().await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    // Caller's own funded genesis vault (source, 9_000_000 REV) and a distinct
    // existing genesis vault (target). The transfers sign with DEFAULT_SEC, whose
    // deployer id derives the DEFAULT_PUB vault address that the transfer authKey
    // (`deployerAuthKey`) authorizes — so `from` MUST be DEFAULT_PUB's address.
    let from_addr = VaultAddress::from_public_key(&construct_deploy::DEFAULT_PUB)
        .expect("DEFAULT_PUB vault address")
        .to_base58();
    let to_addr = VaultAddress::from_public_key(&construct_deploy::DEFAULT_PUB2)
        .expect("DEFAULT_PUB2 vault address")
        .to_base58();
    assert_ne!(
        from_addr, to_addr,
        "source and target vaults must differ so the transfer actually drains the source"
    );

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .expect("create_network(3)");

    // Three same-payer REV transfers (DEFAULT_SEC). Distinct timestamps
    // (enforced by the sleeps) keep the signatures distinct.
    let mut deploys = Vec::with_capacity(3);
    for _ in 0..3 {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        deploys.push(
            construct_deploy::source_deploy_now_full(
                transfer_rho(&from_addr, &to_addr, TRANSFER_AMOUNT),
                Some(PHLO_LIMIT),
                Some(PHLO_PRICE),
                Some(construct_deploy::DEFAULT_SEC.clone()),
                None,
                Some(shard_id.clone()),
            )
            .expect("build transfer deploy"),
        );
    }

    // Sort by signature ascending, then route the lex-LARGEST sig to validator 0's
    // block so validator 0's own prior block contains the deploy the merge engine
    // will reject (and later recover).
    deploys.sort_by(|a, b| a.sig.cmp(&b.sig));
    let deploy_v0 = deploys[2].clone(); // lex-largest → merge-rejected → recovered
    let deploy_v1 = deploys[1].clone();
    let deploy_v2 = deploys[0].clone();
    let rejected_sig: Bytes = deploy_v0.sig.clone();
    let surviving_sig_1: Bytes = deploy_v1.sig.clone();
    let surviving_sig_2: Bytes = deploy_v2.sig.clone();
    assert!(
        rejected_sig > surviving_sig_1 && surviving_sig_1 > surviving_sig_2,
        "the three transfer sigs must be strictly ordered so the negative-balance \
         merge rejection deterministically picks validator 0's (lex-largest) deploy"
    );

    // Sibling blocks: one transfer each, played independently against the genesis
    // post-state (each sees the untouched 9_000_000 REV source vault).
    let block_0 = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy_v0))
        .await
        .expect("validator 0 proposes block_0 (lex-largest transfer)");
    let block_1 = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&deploy_v1))
        .await
        .expect("validator 1 proposes block_1");
    let block_2 = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(&deploy_v2))
        .await
        .expect("validator 2 proposes block_2");
    let siblings = vec![block_0.clone(), block_1.clone(), block_2.clone()];

    // Distribute every sibling to every other validator so the merge proposer
    // (validator 1) sees all three branches.
    for (source, block) in siblings.iter().enumerate() {
        for (target, node) in nodes.iter_mut().enumerate() {
            if source != target {
                node.process_block(block.clone())
                    .await
                    .expect("process sibling");
            }
        }
    }
    for node in &nodes {
        for block in &siblings {
            assert!(
                node.contains(&block.block_hash),
                "every validator must observe every sibling before the merge"
            );
        }
    }

    // Validator 1 (NOT validator 0) proposes the merge over the three siblings.
    // The marker uses a DIFFERENT payer (DEFAULT_SEC2) so create_block has fresh
    // content (no NoNewDeploys short-circuit) while the DEFAULT_SEC source-vault
    // arithmetic stays driven solely by the three transfers.
    let marker = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            1,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("build merge marker")
    };
    let merge_block = nodes[1]
        .add_block_from_deploys(&[marker])
        .await
        .expect("validator 1 proposes merge_block over the three siblings");

    for block in &siblings {
        assert!(
            merge_block
                .header
                .parents_hash_list
                .contains(&block.block_hash),
            "merge_block must merge sibling {}",
            hex::encode(&block.block_hash)
        );
    }

    // D3-correct NON-ZERO merge rejection: the cumulative transfer over-drain
    // (`3 × TRANSFER_AMOUNT > GENESIS_VAULT_BALANCE`) drives the shared source-vault
    // IntegerAdd channel below zero, and `conflict_set_merger::fold_rejection`'s
    // `current.checked_add(diff) >= 0` check rejects exactly the lex-largest-sig
    // transfer (folded last). This is a genuine vault-balance conflict — precisely
    // the D3 follow-on the top-of-file note describes — NOT a precharge artifact.
    let rejected_sigs: Vec<Bytes> = merge_block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| rd.sig.clone())
        .collect();
    let rejected_transfers: Vec<Bytes> = [&rejected_sig, &surviving_sig_1, &surviving_sig_2]
        .into_iter()
        .filter(|s| rejected_sigs.iter().any(|r| r == *s))
        .cloned()
        .collect();
    assert_eq!(
        rejected_transfers.len(),
        1,
        "exactly one of the three same-payer transfers must be merge-rejected \
         (2 × {amt} = {two} ≤ {bal} fit; 3 × {amt} = {three} > {bal} over-drains the \
         shared source vault); got rejected transfer sigs = {got:?}",
        amt = TRANSFER_AMOUNT,
        two = 2 * TRANSFER_AMOUNT,
        three = 3 * TRANSFER_AMOUNT,
        bal = GENESIS_VAULT_BALANCE,
        got = rejected_transfers
            .iter()
            .map(hex::encode)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        rejected_transfers[0],
        rejected_sig,
        "the rejected transfer must be the lex-largest sig {} (the branch \
         `fold_rejection` folds last once the vault is exhausted); got {}",
        hex::encode(&rejected_sig),
        hex::encode(&rejected_transfers[0])
    );

    // Validator 0 validates merge_block → its own KeyValueRejectedDeployBuffer is
    // populated with the rejected sig (Pending finalization state ⇒ admitted).
    nodes[0]
        .process_block(merge_block.clone())
        .await
        .expect("validator 0 processes merge_block");
    nodes[2]
        .process_block(merge_block.clone())
        .await
        .expect("validator 2 processes merge_block");
    assert!(
        nodes[0].contains(&merge_block.block_hash),
        "validator 0 must observe merge_block before the recovery propose"
    );
    {
        let buffer_guard = nodes[0].rejected_deploy_buffer.lock().expect("buffer lock");
        assert!(
            buffer_guard
                .contains_sig(&rejected_sig)
                .expect("buffer.contains_sig"),
            "validator 0's rejected-deploy buffer must contain the merge-rejected \
             transfer {} after validating merge_block",
            hex::encode(&rejected_sig)
        );
    }

    // Mark the merge frontier finalized before the recovery propose (ported from
    // dev's `recovery_cycle_rejected_deploy_retries_while_source_is_visible`,
    // re-expressed on the D3 vault-draining trigger): the rejected sig must be
    // retryable WHILE its source block is still visible in unresolved scope —
    // the record-driven exemption is a pure function of the on-chain record, not
    // of the source leaving the DAG window.
    nodes[0]
        .block_dag_storage
        .record_directly_finalized(merge_block.block_hash.clone(), 1.0, |_| async { Ok(()) })
        .await
        .expect("mark merge frontier finalized for the recovery gate");

    // Recovery: validator 0 proposes recovery_block. `prepare_user_deploys` pulls
    // the buffered sig and the `canonical_won_sigs` exemption lets it past the
    // self-chain dedup filter (its highest-block disposition is the merge
    // rejection, not the block_0 win) into body.deploys.
    let marker_2 = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            2,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("build recovery marker")
    };
    let recovery_block = nodes[0]
        .add_block_from_deploys(&[marker_2])
        .await
        .expect("validator 0 proposes recovery_block");
    let recovery_sigs: Vec<&Bytes> = recovery_block
        .body
        .deploys
        .iter()
        .map(|pd| &pd.deploy.sig)
        .collect();
    assert!(
        recovery_sigs.iter().any(|s| **s == rejected_sig),
        "recovery_block.body.deploys must re-include the merge-rejected transfer \
         {} pulled from the rejected-deploy buffer; got body.deploys sigs = {:?}. \
         If this fires, check that `prepare_user_deploys` and the self-chain dedup \
         filter both exempt `rejected_in_scope` sigs",
        hex::encode(&rejected_sig),
        recovery_sigs
            .iter()
            .map(|s| hex::encode(s.as_ref()))
            .collect::<Vec<_>>()
    );

    // Packaging the replay must NOT drain the buffer entry (ported from dev's
    // retry test; this is the invariant behind the disabled finalization-time
    // purge): the recovery block is not yet canonical — it could be orphaned —
    // and the buffer holds the only re-proposable copy. The entry is purged only
    // once the replay is finalized-WON (proposer-side terminal purge in
    // `prepare_user_deploys_with_policy`).
    {
        let buffer_guard = nodes[0].rejected_deploy_buffer.lock().expect("buffer lock");
        assert!(
            buffer_guard
                .contains_sig(&rejected_sig)
                .expect("buffer.contains_sig"),
            "the recovered sig {} must remain buffered until its replay is \
             finalized-won (packaging alone must not evict it)",
            hex::encode(&rejected_sig)
        );
    }

    // A recovery block must never list one of its own accepted deploys as
    // rejected (ported from dev's retry test: accept/reject overlap would be an
    // InvalidRejectedDeploy-class self-contradiction).
    let recovery_body_sigs: std::collections::HashSet<Bytes> = recovery_block
        .body
        .deploys
        .iter()
        .map(|pd| pd.deploy.sig.clone())
        .collect();
    let overlapping: Vec<Bytes> = recovery_block
        .body
        .rejected_deploys
        .iter()
        .filter(|rd| recovery_body_sigs.contains(&rd.sig))
        .map(|rd| rd.sig.clone())
        .collect();
    assert!(
        overlapping.is_empty(),
        "recovery_block must not list accepted deploy signatures as rejected; overlaps = {:?}",
        overlapping.iter().map(hex::encode).collect::<Vec<_>>()
    );

    // The two surviving transfers stay reachable in the canonical view (they were
    // admitted at merge, not rejected).
    let representation = nodes[0]
        .block_dag_storage
        .get_representation()
        .expect("dag representation");
    for sig in [&surviving_sig_1, &surviving_sig_2] {
        assert!(
            representation
                .lookup_by_deploy_id(&sig.to_vec())
                .ok()
                .flatten()
                .is_some(),
            "surviving transfer {} must remain reachable in the canonical view \
             (it was admitted, not merge-rejected)",
            hex::encode(sig)
        );
    }
}
