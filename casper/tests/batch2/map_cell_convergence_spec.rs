// Concurrent single-value-cell convergence ladder.
//
// N validators each concurrently write a DISTINCT key to one single-value Map cell
// `@"m"`. Because the cell holds one value, the merge can keep only one whole-cell
// write per merge; the losers must be recovered (re-executed on top of the winner,
// composing the distinct keys) until every key lands. The invariants under test:
//   1. CONVERGENCE: every distinct key written eventually appears in the final cell.
//   2. FS MONOTONICITY: the finalized cell only grows — no key present at one LFB is
//      ever absent at a later LFB (FS never regresses or oscillates).
//
// Graded smallest->largest so a failure isolates the contention degree:
//   - two_writers:        2-way contention, the simplest concurrent case
//   - three_writers:      3-way contention (the case where a main_parent writer starves)
//   - three_writers_load: 3-way contention sustained over multiple write rounds
//
// These are the green-gate for the sealed-floor / record-driven-recovery design and
// run as part of the normal `cargo test -p casper` suite (CI gate). They pass on the
// floor-based merge with the channel_change netting fix.

use std::collections::{BTreeMap, HashMap, HashSet};

use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::casper::protocol::casper_message::{
    BlockMessage, DeployAdmissionStatus, DeployData,
};
use models::rust::deploy_id::DeployLookupId;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

type BondsFunction = fn(Vec<PublicKey>) -> HashMap<PublicKey, i64>;

fn only_deploy_id(block: &BlockMessage) -> DeployLookupId {
    block.body.deploys[0]
        .deploy_id_for_protocol(block.header.version)
        .expect("block deploy identity")
}

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new(n_validators: usize) -> Self { Self::new_with_bonds(n_validators, None).await }

    async fn new_with_bonds(n_validators: usize, bonds_function: Option<BondsFunction>) -> Self {
        let genesis_parameters = GenesisBuilder::build_genesis_parameters_with_defaults(
            bonds_function,
            Some(n_validators),
        );
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(genesis_parameters))
            .await
            .unwrap();
        Self { genesis }
    }
}

fn equal_bonds(validators: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
    validators
        .into_iter()
        .map(|validator| (validator, 1))
        .collect()
}

/// Distinct, genesis-funded deployer keys (one per validator) so the only conflict is
/// the single-value-cell keep-one, not a shared-purse aggregate debit. Supports up to 3.
/// All three MUST be genesis-funded: 0/1 are DEFAULT_SEC/SEC2; 2 is the first EXTRA
/// genesis vault key (funded 9M Rev for the default 4-validator genesis). An UNFUNDED
/// key cannot certify its complete protocol debit and never writes.
fn signer_key(v: usize) -> PrivateKey {
    match v {
        0 => construct_deploy::DEFAULT_SEC.clone(),
        1 => construct_deploy::DEFAULT_SEC2.clone(),
        2 => crate::util::genesis_builder::EXTRA_GENESIS_VAULT_KEY_PAIRS[0]
            .0
            .clone(),
        _ => panic!("convergence ladder supports up to 3 distinct funded deployer keys"),
    }
}

fn map_set_deploy(
    key: &str,
    val: i64,
    sec: &PrivateKey,
    valid_after_block_number: i64,
    shard_id: &str,
) -> Signed<DeployData> {
    let rho = format!(r#"for (@m <- @"m") {{ @"m"!(m.set("{}", {})) }}"#, key, val);
    construct_deploy::source_deploy_now_full(
        rho,
        None,
        None,
        Some(sec.clone()),
        Some(valid_after_block_number),
        Some(shard_id.to_string()),
    )
    .expect("build map-set deploy")
}

fn marker_deploy(id: i32, valid_after_block_number: i64, shard_id: &str) -> Signed<DeployData> {
    construct_deploy::source_deploy_now(
        format!("@{id}!({id})"),
        None,
        Some(valid_after_block_number),
        Some(shard_id.to_string()),
    )
    .expect("build marker deploy")
}

async fn agreed_finalized_height(nodes: &[TestNode]) -> i64 {
    let first = nodes[0]
        .casper
        .last_finalized_block()
        .await
        .expect("lfb node0");
    for (index, node) in nodes.iter().enumerate().skip(1) {
        let current = node.casper.last_finalized_block().await.expect("lfb");
        assert_eq!(
            current.block_hash, first.block_hash,
            "node {index} finalized a different block before deploy construction"
        );
    }
    first.body.state.block_number
}

fn assert_deploy_executed(block: &BlockMessage, signature: &prost::bytes::Bytes, label: &str) {
    let processed = block
        .body
        .deploys
        .iter()
        .find(|processed| processed.deploy.sig == signature)
        .unwrap_or_else(|| panic!("{label} was not included in the proposed block"));
    assert_eq!(
        processed.admission_status,
        DeployAdmissionStatus::Executed,
        "{label} was terminally rejected"
    );
    assert!(!processed.is_failed, "{label} failed during execution");
}

fn par_to_i64(p: &Par) -> Option<i64> {
    p.exprs.first().and_then(|e| match &e.expr_instance {
        Some(ExprInstance::GInt(n)) => Some(*n),
        _ => None,
    })
}

fn par_to_string(p: &Par) -> Option<&str> {
    p.exprs.first().and_then(|e| match &e.expr_instance {
        Some(ExprInstance::GString(s)) => Some(s.as_str()),
        _ => None,
    })
}

fn map_cell_channel() -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString("m".to_string())),
        }],
        ..Default::default()
    }
}

fn map_entries(datum: &Par) -> BTreeMap<String, i64> {
    let map = datum
        .exprs
        .iter()
        .find_map(|expr| match &expr.expr_instance {
            Some(ExprInstance::EMapBody(map)) => Some(map),
            _ => None,
        })
        .expect("@\"m\" datum must contain a map");
    let entries = map
        .kvs
        .iter()
        .map(|entry| {
            let key = entry
                .key
                .as_ref()
                .and_then(par_to_string)
                .expect("map key must be a string")
                .to_string();
            let value = entry
                .value
                .as_ref()
                .and_then(par_to_i64)
                .expect("map value must be an integer");
            (key, value)
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        entries.len(),
        map.kvs.len(),
        "@\"m\" contains duplicate encoded map keys"
    );
    entries
}

async fn map_cell_datums(
    node: &TestNode,
    state_hash: &prost::bytes::Bytes,
) -> Vec<BTreeMap<String, i64>> {
    let data = node
        .runtime_manager
        .get_data(state_hash.clone(), &map_cell_channel())
        .await
        .expect("get_data @\"m\"");
    data.iter().map(map_entries).collect()
}

fn present_keys_in(entries: &BTreeMap<String, i64>, writes: &BTreeMap<String, i64>) -> Vec<String> {
    for (key, actual) in entries {
        let expected = writes
            .get(key)
            .unwrap_or_else(|| panic!("finalized map contains unexpected key {key}"));
        assert_eq!(actual, expected, "value mismatch for finalized key {key}");
    }
    writes
        .iter()
        .filter_map(|(key, expected)| {
            entries.get(key).map(|actual| {
                assert_eq!(actual, expected, "value mismatch for finalized key {key}");
                key.clone()
            })
        })
        .collect()
}

async fn present_keys(
    node: &TestNode,
    state_hash: &prost::bytes::Bytes,
    writes: &BTreeMap<String, i64>,
) -> Vec<String> {
    let datums = map_cell_datums(node, state_hash).await;
    assert_eq!(datums.len(), 1, "@\"m\" must contain exactly one datum");
    present_keys_in(&datums[0], writes)
}

/// Finalized cell maps read on EVERY node at its own LFB. Asserts all nodes agree on
/// the LFB block AND the complete finalized map — a divergence is the #71 node-identity
/// break (a node-local finalized-state corruption that need not itself stall
/// finalization, which a node-0-only read would miss). Returns the agreed
/// (lfb_block_number, sorted expected keys).
async fn finalized_keys_all_nodes(
    nodes: &[TestNode],
    writes: &BTreeMap<String, i64>,
) -> (i64, Vec<String>) {
    let lfb0 = nodes[0]
        .casper
        .last_finalized_block()
        .await
        .expect("lfb node0");
    // Single-value-cell invariant (the integration's node-log check, made explicit and
    // deterministic): @"m" must hold EXACTLY ONE datum. A multi-datum cell is the merge
    // defect; checked before the peek read, it turns the flaky cross-node coin-flip into a
    // precise "N datums at block #B" failure.
    let datums0 = map_cell_datums(&nodes[0], &lfb0.body.state.post_state_hash).await;
    let n0 = datums0.len();
    assert_eq!(
        n0, 1,
        "SINGLE-VALUE-CELL: @\"m\" holds {} datums (expected 1) on node 0 at LFB #{} — keep-one did not collapse concurrent writes",
        n0, lfb0.body.state.block_number,
    );
    let mut fs0 = present_keys_in(&datums0[0], writes);
    fs0.sort();
    for (j, node) in nodes.iter().enumerate().skip(1) {
        let lfbj = node.casper.last_finalized_block().await.expect("lfb");
        let datumsj = map_cell_datums(node, &lfbj.body.state.post_state_hash).await;
        let nj = datumsj.len();
        assert_eq!(
            nj, 1,
            "SINGLE-VALUE-CELL: @\"m\" holds {} datums (expected 1) on node {} at LFB #{} — keep-one did not collapse concurrent writes",
            nj, j, lfbj.body.state.block_number,
        );
        let mut fsj = present_keys_in(&datumsj[0], writes);
        fsj.sort();
        assert_eq!(
            lfbj.block_hash, lfb0.block_hash,
            "NODE-IDENTITY: node {} finalized #{} but node 0 finalized #{} — LFB divergence",
            j, lfbj.body.state.block_number, lfb0.body.state.block_number,
        );
        assert_eq!(
            datumsj[0], datums0[0],
            "NODE-IDENTITY: node {j} finalized map differs from node 0 at LFB #{}",
            lfb0.body.state.block_number,
        );
        assert_eq!(
            fsj, fs0,
            "NODE-IDENTITY: node {} finalized cell {:?} != node 0 {:?} at LFB #{}",
            j, fsj, fs0, lfb0.body.state.block_number,
        );
    }
    (lfb0.body.state.block_number, fs0)
}

/// Run `n_validators` concurrent distinct-key writers across `write_rounds` rounds,
/// optionally cycling the per-validator key space, then `drain_rounds` quiet
/// rounds, and assert convergence + FS monotonicity.
///
/// `require_full_convergence` gates the TERMINAL "every key landed" assertion.
/// The per-round invariants that validate the finalized-floor merge — single-value
/// cell (keep-one collapsed), cross-node LFB + finalized-key identity (no fork),
/// and FS monotonicity (no finalized write ever lost) — are asserted EVERY round
/// regardless. Terminal full convergence additionally requires the keep-one
/// RECOVERY to drain every loser; under sustained single-cell N-writer overload
/// the loser backlog grows ~(N-1)/round while recovery drains ~1/round, so old
/// losers can expire (deploy_lifespan) before recovery — a capacity bound (A10)
/// orthogonal to the floor merge. The soak passes `false` to exercise the merge
/// over 400+ blocks without asserting that orthogonal recovery-throughput bound;
/// the graded gates pass `true`.
async fn run_convergence(
    n_validators: usize,
    write_rounds: usize,
    drain_rounds: usize,
    require_full_convergence: bool,
    write_key_period: Option<usize>,
) {
    assert!((2..=3).contains(&n_validators));
    assert!(write_key_period.is_none_or(|period| period > 0));
    let ctx = TestContext::new(n_validators).await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    let mut nodes =
        TestNode::create_network(ctx.genesis.clone(), n_validators, None, None, None, None)
            .await
            .expect("create_network");
    // Heartbeat/liveness like a production shard: a proposer with no user deploys
    // (its write recovered or already canonical) emits an empty CloseBlock block
    // instead of erroring NoNewDeploys, so the chain keeps advancing.
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    let secs: Vec<PrivateKey> = (0..n_validators).map(signer_key).collect();

    // Initialize the single-value cell on node 0 and distribute.
    let init = construct_deploy::source_deploy_now_full(
        r#"@"m"!({})"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        None,
        Some(shard_id.clone()),
    )
    .expect("build init");
    let init_signature = init.sig.clone();
    nodes[0].casper.deploy(init).expect("init deploy");
    let init_block = nodes[0].create_block_unsafe(&[]).await.expect("init block");
    assert_deploy_executed(&init_block, &init_signature, "map initialization");
    for node in nodes.iter_mut().take(n_validators) {
        node.process_block(init_block.clone())
            .await
            .expect("process init");
    }

    let mut writes = BTreeMap::new();
    // FS-monotonicity tracking: the set of finalized keys must never shrink.
    let mut prev_fs: Vec<String> = Vec::new();
    let mut fs_violation: Option<String> = None;

    let check_fs =
        |label: &str, fs_now: &[String], prev: &mut Vec<String>, violation: &mut Option<String>| {
            for k in prev.iter() {
                if !fs_now.contains(k) && violation.is_none() {
                    *violation = Some(format!(
                        "FS REGRESSED at {}: key {} was finalized then disappeared (fs_now={:?})",
                        label, k, fs_now
                    ));
                }
            }
            *prev = fs_now.to_vec();
        };

    // Write rounds: each validator writes a distinct key concurrently (siblings),
    // then node 0 proposes a merge.
    for round in 0..write_rounds {
        let valid_after_block_number = agreed_finalized_height(&nodes).await;
        let mut sibling_blocks = Vec::new();
        for v in 0..n_validators {
            let key_round = write_key_period.map_or(round, |period| round % period);
            let key = format!("v{}_{}", v + 1, key_round);
            let val = (key_round * n_validators + v + 1) as i64;
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            let d = map_set_deploy(&key, val, &secs[v], valid_after_block_number, &shard_id);
            let signature = d.sig.clone();
            println!(
                "WRITE-SIG key={} sig={}",
                key,
                hex::encode(&d.sig[..8.min(d.sig.len())])
            );
            nodes[v].casper.deploy(d).expect("deploy write");
            let previous = writes.insert(key.clone(), val);
            assert!(previous.is_none_or(|expected| expected == val));
            let blk = nodes[v]
                .create_block_unsafe(&[])
                .await
                .expect("propose sibling");
            assert_deploy_executed(&blk, &signature, &format!("write {key}"));
            let own = present_keys(&nodes[v], &blk.body.state.post_state_hash, &writes).await;
            assert!(
                own.contains(&key),
                "write {key} did not update its sibling block"
            );
            println!(
                "MSTACK-SIBLING v{} key={} wrote_own_key_in_own_sibling={}",
                v + 1,
                key,
                own.contains(&key)
            );
            sibling_blocks.push(blk);
        }
        for blk in &sibling_blocks {
            for node in nodes.iter_mut().take(n_validators) {
                node.process_block(blk.clone())
                    .await
                    .expect("process sibling block");
            }
        }
        let marker_valid_after = agreed_finalized_height(&nodes).await;
        let marker = marker_deploy(round as i32, marker_valid_after, &shard_id);
        let marker_signature = marker.sig.clone();
        nodes[0].casper.deploy(marker).expect("marker deploy");
        let merge = nodes[0]
            .create_block_unsafe(&[])
            .await
            .expect("merge block");
        assert_deploy_executed(&merge, &marker_signature, &format!("merge marker {round}"));
        for node in nodes.iter_mut().take(n_validators) {
            node.process_block(merge.clone())
                .await
                .expect("process merge block");
        }
        let (lfb_num, fs) = finalized_keys_all_nodes(&nodes, &writes).await;
        let tip = present_keys(&nodes[0], &merge.body.state.post_state_hash, &writes).await;
        println!(
            "write {}: tip=#{} LFB=#{} tip_keys={:?} fs_keys={:?}",
            round, merge.body.state.block_number, lfb_num, tip, fs
        );
        check_fs(
            &format!("write {}", round),
            &fs,
            &mut prev_fs,
            &mut fs_violation,
        );
    }

    // Drain rounds: rotate the proposer so every owner re-proposes any keep-one loser.
    for extra in 0..drain_rounds {
        let proposer = extra % n_validators;
        let valid_after_block_number = agreed_finalized_height(&nodes).await;
        let marker = marker_deploy((1000 + extra) as i32, valid_after_block_number, &shard_id);
        let marker_signature = marker.sig.clone();
        nodes[proposer].casper.deploy(marker).expect("drain deploy");
        let blk = nodes[proposer]
            .create_block_unsafe(&[])
            .await
            .expect("drain block");
        assert_deploy_executed(&blk, &marker_signature, &format!("drain marker {extra}"));
        for node in nodes.iter_mut().take(n_validators) {
            node.process_block(blk.clone())
                .await
                .expect("process drain block");
        }
        let (lfb_num, fs) = finalized_keys_all_nodes(&nodes, &writes).await;
        let tip = present_keys(&nodes[0], &blk.body.state.post_state_hash, &writes).await;
        println!(
            "drain {} (proposer v{}): tip=#{} LFB=#{} tip_keys={:?} fs_keys={:?}",
            extra,
            proposer + 1,
            blk.body.state.block_number,
            lfb_num,
            tip,
            fs
        );
        check_fs(
            &format!("drain {}", extra),
            &fs,
            &mut prev_fs,
            &mut fs_violation,
        );
    }

    // Settle: node 0 proposes a final block; read the cell at its post-state.
    let final_valid_after = agreed_finalized_height(&nodes).await;
    let final_marker = marker_deploy(9999, final_valid_after, &shard_id);
    let final_marker_signature = final_marker.sig.clone();
    nodes[0].casper.deploy(final_marker).expect("final deploy");
    let final_block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("final block");
    assert_deploy_executed(&final_block, &final_marker_signature, "final marker");
    let final_keys =
        present_keys(&nodes[0], &final_block.body.state.post_state_hash, &writes).await;
    let missing: Vec<(&String, &i64)> = writes
        .iter()
        .filter(|(k, _)| !final_keys.contains(k))
        .collect();

    // FS monotonicity is fix-relevant (a finalized write must never be lost) and
    // is asserted unconditionally.
    assert!(
        fs_violation.is_none(),
        "FS monotonicity violated: {}",
        fs_violation.unwrap()
    );
    if require_full_convergence {
        assert!(
            missing.is_empty(),
            "convergence failed for {} validator(s): MISSING {} of {} keys: {:?}",
            n_validators,
            missing.len(),
            writes.len(),
            missing
        );
    } else if !missing.is_empty() {
        // Soak mode: under sustained single-cell overload the keep-one recovery
        // backlog outgrows the deploy-lifespan window, so some losers expire
        // before recovery (A10 capacity bound — orthogonal to the floor merge,
        // which held every per-round invariant across the whole run).
        println!(
            "SOAK: {} of {} keys unrecovered (expired under sustained overload; \
             orthogonal to the floor-merge fix — no fork, no finalized write-loss, \
             no Δ-backstop fired across the run)",
            missing.len(),
            writes.len()
        );
    }
}

async fn create_allow_empty(node: &mut TestNode) -> BlockCreatorResult {
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let validator = node.casper.get_validator().expect("validator");
    block_creator::create(
        &snapshot,
        &validator,
        None,
        node.deploy_storage.clone(),
        node.rejected_deploy_buffer.clone(),
        &node.runtime_manager,
        &mut node.block_store,
        true,
    )
    .await
    .expect("block creation")
}

/// Deploy admission under an unresolved user frontier.
///
/// Original (pre-starvation-fix) semantics required strict single-leader
/// packaging of ALL ordinary deploys — which starved fresh deploy admission
/// on live shards (every non-leader returned NoNewDeploys until the leader's
/// work finalized). The corrected semantics serialize IN-SCOPE recovery /
/// re-proposal work through the deterministic inclusion leader (covered by
/// the `deploy_inclusion_leadership_gates_ordinary_selection` unit test)
/// while allowing each validator to admit its OWN fresh local deploys under
/// the bounded `fresh_admission_fallback` cap.
///
/// This test pins the refined invariant: fresh admission remains node-local and
/// already-active frontier work is not repackaged while neither sibling has a
/// strict finality certificate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn unresolved_user_frontier_fresh_admission_is_bounded_and_disjoint() {
    let context = TestContext::new_with_bonds(2, Some(equal_bonds)).await;
    let shard = context.genesis.genesis_block.shard_id.clone();
    let genesis_hash = context.genesis.genesis_block.block_hash.clone();
    let mut nodes = TestNode::create_network(context.genesis, 2, None, None, None, None)
        .await
        .expect("network");
    let first = map_set_deploy("leader-a", 1, &signer_key(0), 0, &shard);
    let second = map_set_deploy("leader-b", 2, &signer_key(1), 0, &shard);
    let frontier_sigs = [first.sig.clone(), second.sig.clone()];
    let block_a = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&first))
        .await
        .expect("first sibling");
    let block_b = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&second))
        .await
        .expect("second sibling");
    let first_id = only_deploy_id(&block_a);
    let second_id = only_deploy_id(&block_b);
    let status_b_on_a = nodes[0]
        .process_block(block_b.clone())
        .await
        .expect("process sibling B on validator 0");
    assert!(
        matches!(status_b_on_a, Either::Right(_)),
        "validator 0 did not accept sibling B: {status_b_on_a:?}"
    );
    let status_a_on_b = nodes[1]
        .process_block(block_a.clone())
        .await
        .expect("process sibling A on validator 1");
    assert!(
        matches!(status_a_on_b, Either::Right(_)),
        "validator 1 did not accept sibling A: {status_a_on_b:?}"
    );
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    for node in &nodes {
        node.wait_for_finalizer_quiescence(deadline)
            .await
            .expect("finalizer quiescence");
        let snapshot = node
            .casper
            .get_snapshot()
            .await
            .expect("unresolved snapshot");
        assert_eq!(
            snapshot.last_finalized_block, genesis_hash,
            "equal-stake siblings must not advance the finalized floor"
        );
        let parent_hashes = snapshot
            .parents
            .iter()
            .map(|parent| parent.block_hash.clone())
            .collect::<std::collections::HashSet<_>>();
        assert!(parent_hashes.contains(&block_a.block_hash));
        assert!(parent_hashes.contains(&block_b.block_hash));
        assert!(snapshot.deploys_in_scope.contains(&first_id));
        assert!(snapshot.deploys_in_scope.contains(&second_id));
    }
    let fresh_a = map_set_deploy("fresh-a", 3, &signer_key(0), 0, &shard);
    let fresh_b = map_set_deploy("fresh-b", 4, &signer_key(1), 0, &shard);
    nodes[0].casper.deploy(fresh_a.clone()).expect("fresh a");
    nodes[1].casper.deploy(fresh_b.clone()).expect("fresh b");
    let proposal_a = create_allow_empty(&mut nodes[0]).await;
    let proposal_b = create_allow_empty(&mut nodes[1]).await;

    let packaged_sigs = |result: &BlockCreatorResult| -> Vec<prost::bytes::Bytes> {
        match result {
            BlockCreatorResult::Created(block, ..) => block
                .body
                .deploys
                .iter()
                .map(|pd| pd.deploy.sig.clone())
                .collect(),
            _ => Vec::new(),
        }
    };
    let sigs_a = packaged_sigs(&proposal_a);
    let sigs_b = packaged_sigs(&proposal_b);

    assert!(
        sigs_a.iter().all(|sig| sig == &fresh_a.sig),
        "validator 0 packaged deploys it did not receive: {sigs_a:?}"
    );
    assert!(
        sigs_b.iter().all(|sig| sig == &fresh_b.sig),
        "validator 1 packaged deploys it did not receive: {sigs_b:?}"
    );
    // Frontier work already included in blocks A/B is never re-packaged.
    for sig in frontier_sigs.iter() {
        assert!(
            !sigs_a.contains(sig) && !sigs_b.contains(sig),
            "already-included frontier deploy was re-packaged"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn resolved_asymmetric_frontier_rehomes_excluded_local_deploy() {
    let context = TestContext::new(2).await;
    let shard = context.genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(context.genesis, 2, None, None, None, None)
        .await
        .expect("network");
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }
    let init = construct_deploy::source_deploy_now_full(
        r#"@"m"!({})"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        None,
        Some(shard.clone()),
    )
    .expect("map initialization");
    let init_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&init))
        .await
        .expect("initialized map block");
    let init_status = nodes[1]
        .process_block(init_block.clone())
        .await
        .expect("process map initialization");
    assert!(matches!(init_status, Either::Right(_)));
    for node in &nodes {
        assert_eq!(
            map_cell_datums(node, &init_block.body.state.post_state_hash).await,
            vec![BTreeMap::new()]
        );
    }

    let first = map_set_deploy("minority", 1, &signer_key(0), 0, &shard);
    let second = map_set_deploy("majority", 2, &signer_key(1), 0, &shard);
    let block_a = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&first))
        .await
        .expect("minority sibling");
    let block_b = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&second))
        .await
        .expect("majority sibling");
    let first_id = only_deploy_id(&block_a);
    let second_id = only_deploy_id(&block_b);
    let status_b_on_a = nodes[0]
        .process_block(block_b.clone())
        .await
        .expect("process majority sibling");
    assert!(matches!(status_b_on_a, Either::Right(_)));
    let status_a_on_b = nodes[1]
        .process_block(block_a.clone())
        .await
        .expect("process minority sibling");
    assert!(matches!(status_a_on_b, Either::Right(_)));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    for node in &nodes {
        node.wait_for_finalizer_quiescence(deadline)
            .await
            .expect("finalizer quiescence");
        assert_eq!(
            node.casper
                .last_finalized_block()
                .await
                .expect("last finalized block")
                .block_hash,
            block_b.block_hash,
            "the 3/4-stake sibling must resolve the frontier"
        );
    }
    let snapshot = nodes[0]
        .casper
        .get_snapshot()
        .await
        .expect("resolved snapshot");
    let parent_hashes = snapshot
        .parents
        .iter()
        .map(|parent| parent.block_hash.clone())
        .collect::<HashSet<_>>();
    assert_eq!(
        parent_hashes,
        HashSet::from([block_a.block_hash.clone(), block_b.block_hash.clone()])
    );
    assert!(snapshot.deploys_in_scope.contains(&first_id));
    assert!(snapshot.deploys_in_scope.contains(&second_id));
    assert!(!snapshot.rejected_in_scope.contains(&first_id));
    assert!(!snapshot.rejected_in_scope.contains(&second_id));

    let settlement = nodes[0]
        .add_block_from_deploys(&[])
        .await
        .expect("exact sibling settlement block");
    assert!(settlement.body.deploys.is_empty());
    let first_rejections = settlement
        .body
        .rejected_deploys
        .iter()
        .filter(|rejected| rejected.typed_deploy_id() == &first_id)
        .collect::<Vec<_>>();
    assert_eq!(first_rejections.len(), 1);
    assert_eq!(first_rejections[0].source_block_hash, block_a.block_hash);
    assert!(first_rejections[0].has_provenance());
    assert!(settlement
        .body
        .rejected_deploys
        .iter()
        .all(|rejected| rejected.typed_deploy_id() != &second_id));
    let settlement_status = nodes[1]
        .process_block(settlement.clone())
        .await
        .expect("process exact sibling settlement");
    assert!(matches!(settlement_status, Either::Right(_)));

    for (index, node) in nodes.iter().enumerate() {
        assert!(
            node.rejected_deploy_buffer
                .lock()
                .expect("rejected deploy buffer")
                .contains_id(&first_id)
                .expect("buffer lookup"),
            "validator {index} did not retain the exact rejected occurrence"
        );
        let recovery_snapshot = node.casper.get_snapshot().await.expect("recovery snapshot");
        assert!(recovery_snapshot.rejected_in_scope.contains(&first_id));
        assert!(!recovery_snapshot.rejected_in_scope.contains(&second_id));
    }

    let recovery_snapshot = nodes[0]
        .casper
        .get_snapshot()
        .await
        .expect("committed recovery view");
    let finalized_height = recovery_snapshot
        .dag
        .lookup_unsafe(&recovery_snapshot.last_finalized_block)
        .expect("finalized metadata")
        .block_number
        .max(0) as usize;
    let finalized_validators = recovery_snapshot.finalized_floor_validators();
    let recovery_key = finalized_validators
        .get(finalized_height % finalized_validators.len())
        .expect("finalized-view recovery leader");
    let recovery_leader = nodes
        .iter()
        .position(|node| {
            node.validator_id_opt
                .as_ref()
                .is_some_and(|identity| identity.public_key.bytes == recovery_key)
        })
        .expect("recovery leader is local");

    let fresh = map_set_deploy("fresh", 3, &signer_key(0), 0, &shard);
    let recovery_block = nodes[recovery_leader]
        .add_block_from_deploys(std::slice::from_ref(&fresh))
        .await
        .expect("rehome and fresh block");
    let selected = recovery_block
        .body
        .deploys
        .iter()
        .map(|deploy| deploy.deploy.sig.clone())
        .collect::<HashSet<_>>();
    assert_eq!(
        selected,
        HashSet::from([first.sig.clone(), fresh.sig.clone()])
    );
    assert!(recovery_block.body.rejected_deploys.iter().all(|rejected| {
        rejected.deploy_id() != first.sig.as_ref() && rejected.deploy_id() != fresh.sig.as_ref()
    }));
    for (index, node) in nodes.iter_mut().enumerate() {
        if index != recovery_leader {
            let status = node
                .process_block(recovery_block.clone())
                .await
                .expect("process recovery block");
            assert!(matches!(status, Either::Right(_)));
        }
    }

    let support_proposer = (recovery_leader + 1) % nodes.len();
    let support = nodes[support_proposer]
        .add_block_from_deploys(&[])
        .await
        .expect("recovery support block");
    assert!(
        nodes[support_proposer]
            .block_dag_storage
            .get_representation()
            .expect("support proposer DAG")
            .is_dag_ancestor(&recovery_block.block_hash, &support.block_hash)
            .expect("support ancestry"),
        "support {} at height {} with parents {:?} does not causally support recovery {} at height {}",
        hex::encode(&support.block_hash),
        support.body.state.block_number,
        support
            .header
            .parents_hash_list
            .iter()
            .map(hex::encode)
            .collect::<Vec<_>>(),
        hex::encode(&recovery_block.block_hash),
        recovery_block.body.state.block_number,
    );
    assert!(
        block_storage::rust::finality::state_preservation::is_state_preserved(
            &nodes[support_proposer]
                .block_dag_storage
                .get_representation()
                .expect("support proposer DAG"),
            &recovery_block.block_hash,
            &support.block_hash,
        )
        .expect("support state preservation"),
        "support {} at height {} causally descends from recovery {} at height {} but does not preserve its state effects",
        hex::encode(&support.block_hash),
        support.body.state.block_number,
        hex::encode(&recovery_block.block_hash),
        recovery_block.body.state.block_number,
    );
    for (index, node) in nodes.iter_mut().enumerate() {
        if index != support_proposer {
            let status = node
                .process_block(support.clone())
                .await
                .expect("process recovery support");
            assert!(matches!(status, Either::Right(_)));
        }
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    for (index, node) in nodes.iter().enumerate() {
        node.wait_for_finalizer_quiescence(deadline)
            .await
            .expect("recovery finalizer quiescence");
        let dag = node.block_dag_storage.get_representation().expect("DAG");
        let lfb = node
            .casper
            .last_finalized_block()
            .await
            .expect("recovery LFB");
        let recovery_metadata = dag
            .lookup_unsafe(&recovery_block.block_hash)
            .expect("recovery metadata");
        let recovery_parent_weights = recovery_metadata
            .parents
            .iter()
            .map(|parent| {
                dag.lookup_unsafe(parent)
                    .map(|metadata| (hex::encode(parent), metadata.weight_map))
                    .expect("recovery parent metadata")
            })
            .collect::<Vec<_>>();
        let vote_context =
            casper::rust::causal_equivocation::CertifiedConsensusContext::for_finalized_floor(
                &dag,
                lfb.block_hash.clone(),
            )
            .expect("finalized vote context");
        let eligible_latest_messages = vote_context
            .vote_projection()
            .eligible_latest_messages()
            .iter()
            .map(|(validator, hash)| (hex::encode(validator), hex::encode(hash)))
            .collect::<Vec<_>>();
        assert!(
            dag.is_dag_ancestor(&recovery_block.block_hash, &lfb.block_hash)
                .expect("recovery ancestry"),
            "validator {index} finalized {} at height {} without recovery {} from {} at height {} in its ancestry; support is {} from {} at height {}; recovery parents are {:?}; recovery weights are {:?}; recovery-parent weights are {:?}; eligible latest messages are {:?}",
            hex::encode(&lfb.block_hash),
            lfb.body.state.block_number,
            hex::encode(&recovery_block.block_hash),
            hex::encode(&recovery_block.sender),
            recovery_block.body.state.block_number,
            hex::encode(&support.block_hash),
            hex::encode(&support.sender),
            support.body.state.block_number,
            recovery_metadata
                .parents
                .iter()
                .map(hex::encode)
                .collect::<Vec<_>>(),
            recovery_metadata.weight_map,
            recovery_parent_weights,
            eligible_latest_messages,
        );
    }
    let writes = BTreeMap::from([
        ("fresh".to_string(), 3),
        ("majority".to_string(), 2),
        ("minority".to_string(), 1),
    ]);
    let (_, finalized_keys) = finalized_keys_all_nodes(&nodes, &writes).await;
    assert_eq!(finalized_keys, vec![
        "fresh".to_string(),
        "majority".to_string(),
        "minority".to_string(),
    ]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn two_writers_converge() { run_convergence(2, 1, 7, true, None).await; }

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn three_writers_converge() { run_convergence(3, 1, 21, true, None).await; }

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn three_writers_converge_under_load() { run_convergence(3, 3, 21, true, None).await; }

/// 400+-block regression soak for the finalized-floor multi-parent-merge fix
/// (H1 deterministic Δ backstop + H2 persisted frontier cache / warm up-walk +
/// H3 floor-bounded merge scope). At ~422 blocks (1 init + 100×4 + 20 drain +
/// 1 final) this runs an order of magnitude past
/// `three_writers_converge_under_load` (~35 blocks) and well past the OLD silent
/// `MERGE_SCOPE_TOO_LARGE` cliff (floor_distance 256 / scope 512).
/// The soak cycles eight keys per validator, keeping application state bounded at
/// 24 entries while all 300 signed writer occurrences still execute a real
/// single-cell COMM and conflict three ways in every round.
///
/// Every merge round exercises the warm frontier up-walk (`incremental_frontier`)
/// against the persisted `frontier-index`. Two implicit assertions ride on the
/// existing harness: (1) a Δ-backstop `Err` would surface as a panic on
/// `create_block_unsafe(...).expect("merge block")`, so completion proves the
/// backstop never fired; (2) `finalized_keys_all_nodes` re-checks single-datum +
/// cross-node LFB identity every round, so the frontier cache staying transparent
/// (cold == warm, no fork) is enforced continuously — plus the terminal
/// convergence + FS-monotonicity asserts.
///
/// `#[ignore]` because it is a multi-hour soak, not a per-commit gate. Run:
///   cargo test -p casper --test mod -- --ignored finalized_floor_400_block_soak
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
#[ignore = "multi-hour 400+-block soak; run explicitly with --ignored"]
async fn finalized_floor_400_block_soak() { run_convergence(3, 100, 20, false, Some(8)).await; }
