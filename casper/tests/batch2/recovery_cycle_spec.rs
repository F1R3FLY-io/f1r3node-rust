// End-to-end regression test for the rejected-deploy recovery pipeline:
// a deploy that is conflict-rejected during a multi-parent merge lands
// in `KeyValueRejectedDeployBuffer` and is re-included in a subsequent
// proposer's body.
//
// Conflict generator: two deploys signed by the SAME funded key, each
// requesting a phlogiston precharge that would individually leave the
// shared vault solvent but together would drive the vault balance below
// zero. `conflict_set_merger::fold_rejection` rejects whichever branch
// it processes second to keep the merged state non-negative. The
// Rholang body is `Nil`, so play execution has no `|` parallel
// composition and is fully deterministic.

use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
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

/// Trivial deploy body. The conflict comes from the system-level
/// precharge against the source vault, not from anything in the Rholang.
const CONFLICT_RHO: &str = r#"
Nil
"#;

/// Phlogiston pricing per deploy. The actual REV drain on the source
/// vault is `cost * phlo_price` (precharge is `phlo_limit * phlo_price`,
/// refunded down to `cost * phlo_price`).
///
/// `phlo_limit = 8` keeps the precharge under the 9_000_000 REV vault
/// cap (`8 * 1_000_000 = 8_000_000`). The deploy's actual cost is ~5
/// phlo, so per-deploy net drain ≈ `5 * 1_000_000 = 5_000_000` REV. Two
/// such deploys against the same vault sum to `10_000_000`, exceeding
/// the 9_000_000 balance and triggering the merge-engine's
/// negative-balance rejection.
const PHLO_LIMIT: i64 = 8;
const PHLO_PRICE: i64 = 1_000_000;

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
///                            must stay parked while its source is unresolved
///
/// The flow exercises:
///   1. Multi-parent merge in `compute_parents_post_state`, where
///      `dag_merger::merge` returns the rejected sig and admits it to the buffer.
///   2. Buffer population on the recovery proposer via
///      `validate_block_checkpoint` when it syncs merge_block.
///   3. `prepare_user_deploys` refusing to replay the buffered deploy
///      while the same sig is still visible in unresolved scope.
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
async fn recovery_cycle_rejected_deploy_retries_while_source_is_visible() {
    let ctx = TestContext::new().await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    // Two validators, no synchrony constraint, unlimited parents so the
    // multi-parent merge actually happens.
    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 2, None, None, None, None)
        .await
        .expect("create_network(2)");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

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

    // The merge engine's negative-balance check must have rejected one
    // of the two deploys, and it must be deploy_a (the lex-larger sig).
    let rejected_sigs: Vec<Bytes> = merge_block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| rd.sig.clone())
        .collect();
    assert!(
        !rejected_sigs.is_empty(),
        "merge_block.body.rejected_deploys must be non-empty — combined \
         precharge from two same-key deploys must drive the source vault \
         balance below zero, which `conflict_set_merger::fold_rejection` \
         catches by rejecting the second branch"
    );
    let conflict_sig = rejected_sigs
        .iter()
        .find(|s| **s == sig_a || **s == sig_b)
        .cloned()
        .expect("the rejected sig must be one of the two conflicting deploys");
    assert_eq!(
        conflict_sig,
        sig_a,
        "the rejected sig must be deploy_a's (the lex-larger sig that \
         `fold_rejection` processes second). Got rejected sigs={:?}, \
         sig_a={}, sig_b={}",
        rejected_sigs.iter().map(hex::encode).collect::<Vec<_>>(),
        hex::encode(&sig_a),
        hex::encode(&sig_b)
    );
    let surviving_sig = sig_b.clone();

    // Sync merge_block from validator 1 back to validator 0. The
    // receive-side `validate_block_checkpoint` runs
    // `compute_parents_post_state` with the buffer arg, which populates
    // validator 0's own `KeyValueRejectedDeployBuffer`. The recovery
    // proposer's snapshot BFS then sees merge_block's `rejected_deploys`
    // and populates `rejected_in_scope`.
    {
        let (a, b) = nodes.split_at_mut(1);
        a[0].sync_with_one(&mut b[0])
            .await
            .expect("sync merge_block 1 -> 0");
    }
    assert!(
        nodes[0].contains(&merge_block.block_hash),
        "validator 0 must observe merge_block before recovery propose"
    );

    // Validator 0's buffer must contain the rejected sig after sync.
    {
        let buffer_guard = nodes[0].rejected_deploy_buffer.lock().expect("buffer lock");
        let contains_rejected = buffer_guard
            .contains_sig(&conflict_sig)
            .expect("buffer.contains_sig");
        assert!(
            contains_rejected,
            "validator 0's buffer must contain the rejected sig {} after \
             syncing merge_block",
            hex::encode(&conflict_sig)
        );
    }
    nodes[0]
        .block_dag_storage
        .record_directly_finalized(merge_block.block_hash.clone(), 1.0, |_| async { Ok(()) })
        .await
        .expect("mark merge frontier finalized for recovery gate");

    // Validator 0 proposes another block while its source block is still
    // visible in unresolved scope. Since the merge rejection is visible and
    // the deploy is in the rejected-deploy buffer, the rejected sig must be
    // retryable rather than blocked until the source leaves the DAG window.
    let marker_deploy_2 = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(1, None, Some(shard_id.clone()))
            .expect("build marker_deploy_2")
    };
    let recovery_block = nodes[0]
        .add_block_from_deploys(&[marker_deploy_2.clone()])
        .await
        .expect("validator 0 proposes recovery_block");

    let recovery_sigs: Vec<&Bytes> = recovery_block
        .body
        .deploys
        .iter()
        .map(|pd| &pd.deploy.sig)
        .collect();
    assert!(
        recovery_sigs.iter().any(|s| **s == conflict_sig),
        "recovery_block.body.deploys must replay recovered sig {}; got body.deploys sigs = {:?}",
        hex::encode(&conflict_sig),
        recovery_sigs
            .iter()
            .map(|s| hex::encode(s.as_ref()))
            .collect::<Vec<_>>()
    );
    {
        let buffer_guard = nodes[0].rejected_deploy_buffer.lock().expect("buffer lock");
        assert!(
            !buffer_guard
                .contains_sig(&conflict_sig)
                .expect("buffer.contains_sig"),
            "selected recovered sig must be drained from the rejected-deploy buffer"
        );
    }

    let body_deploy_sigs: std::collections::HashSet<Bytes> = recovery_block
        .body
        .deploys
        .iter()
        .map(|pd| pd.deploy.sig.clone())
        .collect();
    let overlapping_rejected_sigs: Vec<Bytes> = recovery_block
        .body
        .rejected_deploys
        .iter()
        .filter_map(|rd| body_deploy_sigs.contains(&rd.sig).then_some(rd.sig.clone()))
        .collect();
    assert!(
        overlapping_rejected_sigs.is_empty(),
        "recovery_block must not list accepted deploy signatures as rejected; overlaps={:?}",
        overlapping_rejected_sigs
            .iter()
            .map(hex::encode)
            .collect::<Vec<_>>()
    );

    {
        let (a, b) = nodes.split_at_mut(1);
        b[0].sync_with_one(&mut a[0])
            .await
            .expect("sync recovery_block 0 -> 1");
    }
    assert!(
        nodes[1].contains(&recovery_block.block_hash),
        "validator 1 must validate the recovery block with filtered rejected_deploys"
    );

    // The surviving sig must remain reachable in the canonical view via
    // the deploy index, pointing back to its pre-merge block.
    assert!(
        nodes[0]
            .block_dag_storage
            .get_representation()
            .expect("dag representation")
            .lookup_by_deploy_id(&surviving_sig.to_vec())
            .ok()
            .flatten()
            .is_some(),
        "the surviving sig {} must be reachable in the canonical view via \
         the deploy index",
        hex::encode(&surviving_sig)
    );
}

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
        for target in 0..3 {
            if source != target {
                nodes[target]
                    .process_block(block.clone())
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
                .iter()
                .any(|parent| *parent == block.block_hash),
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
        .filter(|deploy| rejected_sigs.iter().any(|sig| *sig == deploy.sig))
        .count();
    assert_eq!(
        conflicting_rejections,
        2,
        "three same-payer siblings must leave exactly one deploy solvent; rejected={:?}",
        rejected_sigs.iter().map(hex::encode).collect::<Vec<_>>()
    );

    let mut observed_blocks = sibling_blocks.clone();
    observed_blocks.push(merge_block.clone());
    assert_touched_integer_add_channels_single_valued(
        &nodes[0],
        &merge_block.body.state.post_state_hash,
        &observed_blocks,
    );

    for target in 1..3 {
        nodes[target]
            .process_block(merge_block.clone())
            .await
            .expect("process merge block");
        assert_touched_integer_add_channels_single_valued(
            &nodes[target],
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
        for target in 0..3 {
            if target != proposer {
                nodes[target]
                    .process_block(block.clone())
                    .await
                    .expect("process post-merge traffic");
            }
        }
    }
}
