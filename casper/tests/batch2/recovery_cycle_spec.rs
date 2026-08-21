// D3 (DR-9, OD-2): this file previously exercised the rejected-deploy
// recovery pipeline using a PRECHARGE-driven multi-parent-merge conflict: two
// same-key deploys whose combined `phlo_limit × phlo_price` precharge drove the
// shared REV vault below zero, which `conflict_set_merger::fold_rejection`
// rejected, after which the rejected deploy landed in
// `KeyValueRejectedDeployBuffer` and was re-proposed.
//
// D3 removes the maximum-cost precharge. Protocol 4 still commits each branch's
// exact physical, byte, and fee settlement to the payer's vault, but both benign
// same-key deploys fit the shared authenticated balance and therefore merge.
// Admission proves every branch against its pre-state authority; merge then
// checks the complete aggregate durable debit. The recovery-buffer/re-propose
// path is exercised below with a genuine vault-draining transfer whose
// application transfer plus protocol settlement crosses that aggregate bound.

use casper::rust::casper::Casper;
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
        let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(4));
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(parameters))
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
/// transfers no application REV. Its exact protocol debit remains far below the
/// funded aggregate boundary exercised by the benign merge tests.
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
            .load_mergeable_channels(block)
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

/// D3 same-payer benign-merge regression.
///
/// DAG shape:
///
///         genesis
///         /     \
///     block_a   block_b      distinct deploys from one funded key
///         \     /
///       merge_block          both benign effects survive the merge
///
/// This proves that removing maximum precharge removes its artificial same-payer
/// merge conflict: both complete exact branch debits fit, and signatures or
/// branch order do not change acceptance.
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

    // Keep deterministic labels for diagnostics without assigning protocol
    // meaning to signature order.
    let (deploy_a, deploy_b) = if deploy_x.sig >= deploy_y.sig {
        (deploy_x, deploy_y)
    } else {
        (deploy_y, deploy_x)
    };
    let sig_a: Bytes = deploy_a.sig.clone();
    let sig_b: Bytes = deploy_b.sig.clone();
    assert!(
        sig_a > sig_b,
        "deploy_a must hold the lex-larger signature used by diagnostics"
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
    // precharge. Each benign `CONFLICT_RHO` branch still records exact physical,
    // byte, and fee settlement, but the complete aggregate fits the shared
    // authenticated vault. Admission and merge both enforce the same complete
    // debit, so the merge admits both branches without a precharge artifact.
    let rejected_sigs: Vec<Bytes> = merge_block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| rd.sig.clone())
        .collect();
    assert!(
        !rejected_sigs.iter().any(|s| *s == sig_a || *s == sig_b),
        "D3: neither same-key benign deploy is rejected at MERGE — the \
         precharge-driven vault-balance conflict is removed; the complete \
         application-plus-protocol aggregate remains solvent. Got merge rejected sigs={:?}, \
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
/// cost-accounted model precharge is removed. Each branch still carries exact
/// physical, byte, and fee settlement, but the complete same-payer aggregate
/// is solvent (hence 0, not 2, user rejections). Every sibling is individually
/// certified, and merge independently checks the complete durable aggregate.
/// The merge still keeps the
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
        "D3 (DR-9): maximum precharge is removed, and the complete exact debits of \
         these same-payer siblings remain within the shared vault balance. Each \
         sibling is individually certified and the merge verifies the aggregate. The merge \
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

/// Per-transfer REV amount. The fixture verifies against every produced
/// protocol-4 witness that one complete branch debit is solvent, two complete
/// branch debits fit, and three complete branch debits overdraw the shared
/// genesis vault. A complete branch debit includes the transfer, physical
/// settlement, byte settlement, and fixed fee.
const TRANSFER_AMOUNT: i64 = 3_900_000;

pub(super) struct D3VaultConflictFixture {
    pub(super) nodes: Vec<TestNode>,
    pub(super) shard_id: String,
    pub(super) merge_proposer_index: usize,
    siblings: Vec<BlockMessage>,
    transfer_sigs: [Bytes; 3],
}

pub(super) struct D3VaultMergeOutcome {
    pub(super) merge_block: BlockMessage,
    pub(super) winning_block: BlockMessage,
    pub(super) rejected_sig: Bytes,
    pub(super) surviving_sigs: [Bytes; 2],
    pub(super) recovery_validator_index: usize,
}

pub(super) async fn build_d3_vault_conflict_siblings(
    genesis: &GenesisContext,
) -> D3VaultConflictFixture {
    let shard_id = genesis.genesis_block.shard_id.clone();
    let from_addr = VaultAddress::from_public_key(&construct_deploy::DEFAULT_PUB)
        .expect("DEFAULT_PUB vault address")
        .to_base58();
    let to_addr = VaultAddress::from_public_key(&construct_deploy::DEFAULT_PUB2)
        .expect("DEFAULT_PUB2 vault address")
        .to_base58();
    assert_ne!(from_addr, to_addr);

    let mut nodes = TestNode::create_network(genesis.clone(), 4, None, None, None, None)
        .await
        .expect("create_network(4)");
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }
    let merge_proposer_index = nodes.len() - 1;
    let mut assigned_deploys = Vec::with_capacity(3);
    for _ in 0..3 {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        assigned_deploys.push(
            construct_deploy::source_deploy_now_full(
                transfer_rho(&from_addr, &to_addr, TRANSFER_AMOUNT),
                None,
                None,
                Some(construct_deploy::DEFAULT_SEC.clone()),
                None,
                Some(shard_id.clone()),
            )
            .expect("build transfer deploy"),
        );
    }
    let transfer_sigs: [Bytes; 3] = assigned_deploys
        .iter()
        .map(|deploy| deploy.sig.clone())
        .collect::<Vec<_>>()
        .try_into()
        .expect("fixture has exactly three transfer deploys");
    let mut siblings = Vec::with_capacity(nodes.len());
    for (index, deploy) in assigned_deploys.iter().enumerate() {
        siblings.push(
            nodes[index]
                .add_block_from_deploys(std::slice::from_ref(deploy))
                .await
                .unwrap_or_else(|error| panic!("validator {index} proposes transfer: {error}")),
        );
    }

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
            assert!(node.contains(&block.block_hash));
        }
    }

    D3VaultConflictFixture {
        nodes,
        shard_id,
        merge_proposer_index,
        siblings,
        transfer_sigs,
    }
}

pub(super) async fn propose_d3_vault_rejecting_merge(
    fixture: &mut D3VaultConflictFixture,
    nonce: i32,
) -> D3VaultMergeOutcome {
    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let marker = construct_deploy::basic_deploy_data(
        nonce,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(fixture.shard_id.clone()),
    )
    .expect("build merge marker");
    let merge_block = fixture.nodes[fixture.merge_proposer_index]
        .add_block_from_deploys(&[marker])
        .await
        .expect("elected non-recovery validator proposes merge over vault-conflict siblings");

    for block in &fixture.siblings {
        assert!(merge_block
            .header
            .parents_hash_list
            .contains(&block.block_hash));
    }

    let rejected_transfers: Vec<Bytes> = fixture
        .transfer_sigs
        .iter()
        .filter(|sig| {
            merge_block
                .body
                .rejected_deploys
                .iter()
                .any(|rejected| rejected.sig == **sig)
        })
        .cloned()
        .collect();
    let transfer_accounting = fixture
        .siblings
        .iter()
        .map(|block| {
            let processed = &block.body.deploys[0];
            let witness = processed
                .authority_cost_witness
                .as_ref()
                .expect("transfer carries an authority cost witness");
            let certificate = processed
                .authority_funding_certificate
                .as_ref()
                .expect("transfer carries an authority funding certificate");
            (
                processed.cost.cost,
                witness
                    .realized
                    .iter()
                    .map(|resource| resource.amount)
                    .sum::<u64>(),
                witness
                    .settlement
                    .iter()
                    .map(|resource| resource.amount)
                    .sum::<u64>(),
                witness.byte_cost,
                witness
                    .byte_settlement
                    .iter()
                    .map(|resource| resource.amount)
                    .sum::<u64>(),
                certificate
                    .fee_allocation
                    .iter()
                    .map(|resource| resource.amount)
                    .sum::<u64>(),
                certificate
                    .allocation
                    .iter()
                    .chain(&certificate.byte_allocation)
                    .chain(&certificate.fee_allocation)
                    .map(|resource| resource.amount)
                    .sum::<u64>(),
            )
        })
        .collect::<Vec<_>>();
    let per_branch_protocol_debits = transfer_accounting
        .iter()
        .map(|accounting| accounting.2 + accounting.4 + accounting.5)
        .collect::<Vec<_>>();
    assert!(
        per_branch_protocol_debits
            .windows(2)
            .all(|pair| pair[0] == pair[1]),
        "economically identical siblings must have identical protocol debits: \
         {per_branch_protocol_debits:?}"
    );
    let complete_branch_debit =
        u128::from(TRANSFER_AMOUNT as u64) + u128::from(per_branch_protocol_debits[0]);
    assert!(
        2 * complete_branch_debit <= GENESIS_VAULT_BALANCE as u128
            && 3 * complete_branch_debit > GENESIS_VAULT_BALANCE as u128,
        "fixture must admit exactly two complete branch debits; branch debit \
         {complete_branch_debit}, genesis balance {GENESIS_VAULT_BALANCE}, \
         accounting {transfer_accounting:?}"
    );
    assert_eq!(
        rejected_transfers.len(),
        1,
        "three individually solvent transfers must produce exactly one vault rejection; \
         accounting=(cost, realized, physical settlement, byte cost, byte settlement, fee, \
         reservation) {transfer_accounting:?}; rejected={:?}",
        merge_block.body.rejected_deploys,
    );
    let rejected_sig = rejected_transfers[0].clone();
    let recovery_validator_index = fixture
        .transfer_sigs
        .iter()
        .position(|sig| *sig == rejected_sig)
        .expect("rejected transfer belongs to a sibling validator");
    let rejected_record = merge_block
        .body
        .rejected_deploys
        .iter()
        .find(|rejected| rejected.sig == rejected_sig)
        .expect("merge carries the rejected transfer record");
    assert!(
        rejected_record.has_provenance(),
        "merge rejection must identify its exact source occurrence"
    );
    assert_eq!(
        rejected_record.source_block_hash, fixture.siblings[recovery_validator_index].block_hash,
        "merge rejection provenance must name the rejected transfer's source block"
    );
    let surviving_sigs: [Bytes; 2] = fixture
        .transfer_sigs
        .iter()
        .filter(|sig| **sig != rejected_sig)
        .cloned()
        .collect::<Vec<_>>()
        .try_into()
        .expect("one of three transfers rejected leaves two survivors");

    D3VaultMergeOutcome {
        merge_block,
        winning_block: fixture.siblings[recovery_validator_index].clone(),
        rejected_sig,
        surviving_sigs,
        recovery_validator_index,
    }
}

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
/// IntegerAdd check rejects exactly one transfer to keep the merged vault
/// solvent. That rejection is D3-CORRECT: it is the genuine
/// vault-balance conflict, not a precharge artifact — the double-spend acceptance
/// gate is orthogonal and admits each individually-fundable sibling.
///
/// DAG shape (4 validators):
///
///           genesis
///          ╱   │    ╲
///    block_0 block_1 block_2   one transfer per sibling validator
///          ╲   │    ╱
///        merge_block           proposed by the fourth validator
///             │
///       recovery_block         proposed by the finalized-view recovery leader
///                              must resurface in body.deploys
///
/// Recovery pipeline (consensus-critical, D3-independent):
///   1. multi-parent merge → `dag_merger::merge` returns the rejected sig;
///      `compute_rejected_buffer_admits` admits it (its finalization state is
///      `Pending`) to the buffer.
///   2. the recovery leader validates merge_block → `validate_block_checkpoint`
///      populates its `KeyValueRejectedDeployBuffer`.
///   3. that validator proposes recovery_block → `prepare_user_deploys` pulls the
///      buffered sig, and `canonical_won_sigs` exempts it (its highest-block
///      disposition is the REJECTION at merge_block height 2, which overrides the
///      WIN in block_0 at height 1) so it survives the self-chain dedup filter
///      and reaches `body.deploys`.
///
/// Determinism: all three transfers are economically identical and signed by
/// `DEFAULT_SEC`. Consensus deterministically selects one exact rejected source
/// occurrence from the complete branch indices. The fixture observes that
/// protocol result, proves its provenance names one sibling block, and rotates
/// finalized views until that source validator is the recovery leader. No test
/// assertion depends on signature ordering, incidental branch construction, or
/// a preselected validator losing the merge.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn d3_vault_draining_transfers_reject_at_merge_and_recover() {
    let ctx = TestContext::new().await;
    let mut fixture = build_d3_vault_conflict_siblings(&ctx.genesis).await;
    let outcome = propose_d3_vault_rejecting_merge(&mut fixture, 1).await;
    let merge_block = outcome.merge_block;
    let rejected_sig = outcome.rejected_sig;
    let [surviving_sig_1, surviving_sig_2] = outcome.surviving_sigs;
    let recovery_validator_index = outcome.recovery_validator_index;
    let D3VaultConflictFixture {
        mut nodes,
        shard_id,
        merge_proposer_index,
        ..
    } = fixture;

    for (index, node) in nodes.iter_mut().enumerate() {
        if index != merge_proposer_index {
            node.process_block(merge_block.clone())
                .await
                .unwrap_or_else(|error| panic!("validator {index} processes merge_block: {error}"));
        }
    }
    assert!(
        nodes[recovery_validator_index].contains(&merge_block.block_hash),
        "the recovery leader must observe merge_block before the recovery propose"
    );
    {
        let buffer_guard = nodes[recovery_validator_index]
            .rejected_deploy_buffer
            .lock()
            .expect("buffer lock");
        assert!(
            buffer_guard
                .contains_sig(&rejected_sig)
                .expect("buffer.contains_sig"),
            "the recovery leader's rejected-deploy buffer must contain the merge-rejected \
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
    let snapshot = nodes[recovery_validator_index]
        .casper
        .get_snapshot()
        .await
        .expect("snapshot for recovery-leader rotation");
    let mut active_validators = snapshot.on_chain_state.active_validators;
    if active_validators.is_empty() {
        active_validators = snapshot
            .on_chain_state
            .bonds_map
            .into_iter()
            .filter(|(_, stake)| *stake > 0)
            .map(|(validator, _)| validator)
            .collect();
    }
    active_validators.sort();
    active_validators.dedup();
    let recovery_key = nodes[recovery_validator_index]
        .validator_id_opt
        .as_ref()
        .expect("recovery validator identity")
        .public_key
        .bytes
        .clone();
    let recovery_slot = active_validators
        .iter()
        .position(|validator| *validator == recovery_key)
        .expect("recovery validator is active");
    let mut finalization_block = merge_block.clone();
    for node in &nodes {
        node.block_dag_storage
            .record_directly_finalized(finalization_block.block_hash.clone(), 1.0, |_| async {
                Ok(())
            })
            .await
            .expect("finalize the rejecting merge");
    }
    while finalization_block.body.state.block_number.max(0) as usize % active_validators.len()
        != recovery_slot
    {
        let finalized_height = finalization_block.body.state.block_number.max(0) as usize;
        let current_leader = &active_validators[finalized_height % active_validators.len()];
        let support_proposer = nodes
            .iter()
            .enumerate()
            .find(|(index, node)| {
                *index != recovery_validator_index
                    && node
                        .validator_id_opt
                        .as_ref()
                        .expect("support validator identity")
                        .public_key
                        .bytes
                        != *current_leader
            })
            .map(|(index, _)| index)
            .expect("non-leader support proposer");
        let support = nodes[support_proposer]
            .add_block_from_deploys(&[])
            .await
            .expect("non-leader proposes finality support block");
        for (index, node) in nodes.iter_mut().enumerate() {
            if index != support_proposer {
                node.process_block(support.clone())
                    .await
                    .expect("process finality support block");
            }
        }
        for node in &nodes {
            node.block_dag_storage
                .record_directly_finalized(support.block_hash.clone(), 1.0, |_| async { Ok(()) })
                .await
                .expect("finalize the non-leader support block");
        }
        finalization_block = support;
    }

    let recovery_snapshot = nodes[recovery_validator_index]
        .casper
        .get_snapshot()
        .await
        .expect("post-finalization recovery snapshot");
    let finalized_height = recovery_snapshot
        .dag
        .lookup_unsafe(&recovery_snapshot.last_finalized_block)
        .expect("finalized block metadata")
        .block_number
        .max(0) as usize;
    let mut finalized_validators = recovery_snapshot.on_chain_state.active_validators.clone();
    if finalized_validators.is_empty() {
        finalized_validators = recovery_snapshot
            .on_chain_state
            .bonds_map
            .iter()
            .filter(|(_, stake)| **stake > 0)
            .map(|(validator, _)| validator.clone())
            .collect();
    }
    finalized_validators.sort();
    finalized_validators.dedup();
    assert_eq!(
        finalized_validators[finalized_height % finalized_validators.len()],
        recovery_key,
        "finalized view must elect the rejected transfer's validator"
    );
    assert!(
        nodes[recovery_validator_index]
            .rejected_deploy_buffer
            .lock()
            .expect("buffer lock after finalized-view rotation")
            .contains_sig(&rejected_sig)
            .expect("buffer.contains_sig after finalized-view rotation"),
        "rejected transfer must remain buffered through finalized-view rotation"
    );
    let parent_hashes: Vec<Bytes> = recovery_snapshot
        .parents
        .iter()
        .map(|parent| parent.block_hash.clone())
        .collect();
    let scan_floor = recovery_snapshot.max_block_num
        - recovery_snapshot.on_chain_state.shard_conf.deploy_lifespan;
    let (canonical_won, canonical_rejected) =
        casper::rust::util::rholang::interpreter_util::canonical_disposition_sets(
            &nodes[recovery_validator_index].block_store,
            &parent_hashes,
            scan_floor,
        )
        .expect("canonical recovery disposition");
    assert!(
        !canonical_won.contains(&rejected_sig) && canonical_rejected.contains(&rejected_sig),
        "exact merge tombstone must make the rejected source canonically rejected"
    );

    // Recovery: the finalized-view leader proposes recovery_block. `prepare_user_deploys` pulls
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
    let recovery_block = nodes[recovery_validator_index]
        .add_block_from_deploys(&[marker_2])
        .await
        .expect("finalized-view leader proposes recovery_block");
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
        let buffer_guard = nodes[recovery_validator_index]
            .rejected_deploy_buffer
            .lock()
            .expect("buffer lock");
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
    let representation = nodes[recovery_validator_index]
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
