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
// `#[ignore]`d for now: these are the green-gate TARGETS for the sealed-floor /
// record-driven-recovery design, not yet expected to pass on every base. Run with
// `-- --ignored`. Un-ignore each grade as it goes green.

use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::casper::protocol::casper_message::DeployData;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new(n_validators: usize) -> Self {
        let genesis_parameters =
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(genesis_parameters))
            .await
            .unwrap();
        Self { genesis }
    }
}

/// Distinct, genesis-funded deployer keys (one per validator) so the only conflict is
/// the single-value-cell keep-one, not a shared-vault precharge. Supports up to 3.
/// All three MUST be genesis-funded: 0/1 are DEFAULT_SEC/SEC2; 2 is the first EXTRA
/// genesis vault key (funded 9M Rev for the default 4-validator genesis). An UNFUNDED
/// key here fails precharge and never writes — which silently breaks the test.
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

fn map_set_deploy(key: &str, val: i64, sec: &PrivateKey, shard_id: &str) -> Signed<DeployData> {
    let rho = format!(r#"for (@m <- @"m") {{ @"m"!(m.set("{}", {})) }}"#, key, val);
    construct_deploy::source_deploy_now_full(
        rho,
        None,
        None,
        Some(sec.clone()),
        None,
        Some(shard_id.to_string()),
    )
    .expect("build map-set deploy")
}

fn par_to_i64(p: &Par) -> Option<i64> {
    p.exprs.first().and_then(|e| match &e.expr_instance {
        Some(ExprInstance::GInt(n)) => Some(*n),
        _ => None,
    })
}

/// Read which of `writes`' keys are present in `@"m"` at `state_hash` (on node 0).
async fn present_keys(
    node: &TestNode,
    state_hash: &prost::bytes::Bytes,
    writes: &[(String, i64)],
) -> Vec<String> {
    let mut keys = Vec::new();
    for (key, _) in writes {
        let term = format!(
            r#"new return in {{ for (@m <<- @"m") {{ return!(m.getOrElse("{}", -999)) }} }}"#,
            key
        );
        if let Ok((res, _)) = node
            .runtime_manager
            .play_exploratory_deploy(term, state_hash)
            .await
        {
            if res.first().and_then(par_to_i64) != Some(-999) {
                keys.push(key.clone());
            }
        }
    }
    keys
}

/// Number of datums currently on `@"m"` at `state_hash`. A single-value cell must hold
/// EXACTLY ONE; more than one is a multi-datum merge defect (the keep-one did not collapse
/// concurrent writes) — and is precisely what makes a peek read sample non-deterministically
/// across nodes. `get_data` returns every datum on the channel, so the count is exact.
async fn m_datum_count(node: &TestNode, state_hash: &prost::bytes::Bytes) -> usize {
    let m_channel = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString("m".to_string())),
        }],
        ..Default::default()
    };
    node.runtime_manager
        .get_data(state_hash.clone(), &m_channel)
        .await
        .expect("get_data @\"m\"")
        .len()
}

/// Finalized cell keys read on EVERY node at its own LFB. Asserts all nodes agree on
/// the LFB block AND the finalized key set — a divergence is the #71 node-identity
/// break (a node-local finalized-state corruption that need not itself stall
/// finalization, which a node-0-only read would miss). Returns the agreed
/// (lfb_block_number, sorted keys).
async fn finalized_keys_all_nodes(
    nodes: &[TestNode],
    writes: &[(String, i64)],
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
    let n0 = m_datum_count(&nodes[0], &lfb0.body.state.post_state_hash).await;
    assert_eq!(
        n0, 1,
        "SINGLE-VALUE-CELL: @\"m\" holds {} datums (expected 1) on node 0 at LFB #{} — keep-one did not collapse concurrent writes",
        n0, lfb0.body.state.block_number,
    );
    let mut fs0 = present_keys(&nodes[0], &lfb0.body.state.post_state_hash, writes).await;
    fs0.sort();
    for (j, node) in nodes.iter().enumerate().skip(1) {
        let lfbj = node.casper.last_finalized_block().await.expect("lfb");
        let nj = m_datum_count(node, &lfbj.body.state.post_state_hash).await;
        assert_eq!(
            nj, 1,
            "SINGLE-VALUE-CELL: @\"m\" holds {} datums (expected 1) on node {} at LFB #{} — keep-one did not collapse concurrent writes",
            nj, j, lfbj.body.state.block_number,
        );
        let mut fsj = present_keys(node, &lfbj.body.state.post_state_hash, writes).await;
        fsj.sort();
        assert_eq!(
            lfbj.block_hash, lfb0.block_hash,
            "NODE-IDENTITY: node {} finalized #{} but node 0 finalized #{} — LFB divergence",
            j, lfbj.body.state.block_number, lfb0.body.state.block_number,
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
/// then `drain_rounds` quiet rounds, and assert convergence + FS monotonicity.
async fn run_convergence(n_validators: usize, write_rounds: usize, drain_rounds: usize) {
    assert!((2..=3).contains(&n_validators));
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
    nodes[0].casper.deploy(init).expect("init deploy");
    let init_block = nodes[0].create_block_unsafe(&[]).await.expect("init block");
    for j in 0..n_validators {
        nodes[j]
            .process_block(init_block.clone())
            .await
            .expect("process init");
    }

    let mut writes: Vec<(String, i64)> = Vec::new();
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
        let mut sibling_blocks = Vec::new();
        for v in 0..n_validators {
            let key = format!("v{}_{}", v + 1, round);
            let val = (round * n_validators + v + 1) as i64;
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            let d = map_set_deploy(&key, val, &secs[v], &shard_id);
            println!(
                "WRITE-SIG key={} sig={}",
                key,
                hex::encode(&d.sig[..8.min(d.sig.len())])
            );
            nodes[v].casper.deploy(d).expect("deploy write");
            writes.push((key.clone(), val));
            let blk = nodes[v]
                .create_block_unsafe(&[])
                .await
                .expect("propose sibling");
            let own = present_keys(&nodes[v], &blk.body.state.post_state_hash, &[(
                key.clone(),
                val,
            )])
            .await;
            println!(
                "MSTACK-SIBLING v{} key={} wrote_own_key_in_own_sibling={}",
                v + 1,
                key,
                own.contains(&key)
            );
            sibling_blocks.push(blk);
        }
        for blk in &sibling_blocks {
            for j in 0..n_validators {
                nodes[j].process_block(blk.clone()).await.ok();
            }
        }
        let marker =
            construct_deploy::basic_deploy_data(round as i32, None, Some(shard_id.clone()))
                .expect("marker");
        nodes[0].casper.deploy(marker).expect("marker deploy");
        let merge = nodes[0]
            .create_block_unsafe(&[])
            .await
            .expect("merge block");
        for j in 0..n_validators {
            nodes[j].process_block(merge.clone()).await.ok();
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
        let marker = construct_deploy::basic_deploy_data(
            (1000 + extra) as i32,
            None,
            Some(shard_id.clone()),
        )
        .expect("drain marker");
        nodes[proposer].casper.deploy(marker).expect("drain deploy");
        if let Ok(blk) = nodes[proposer].create_block_unsafe(&[]).await {
            for j in 0..n_validators {
                nodes[j].process_block(blk.clone()).await.ok();
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
    }

    // Settle: node 0 proposes a final block; read the cell at its post-state.
    let final_marker = construct_deploy::basic_deploy_data(9999, None, Some(shard_id.clone()))
        .expect("final marker");
    nodes[0].casper.deploy(final_marker).expect("final deploy");
    let final_block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("final block");
    let final_keys =
        present_keys(&nodes[0], &final_block.body.state.post_state_hash, &writes).await;
    let missing: Vec<&(String, i64)> = writes
        .iter()
        .filter(|(k, _)| !final_keys.contains(k))
        .collect();

    assert!(
        fs_violation.is_none(),
        "FS monotonicity violated: {}",
        fs_violation.unwrap()
    );
    assert!(
        missing.is_empty(),
        "convergence failed for {} validator(s): MISSING {} of {} keys: {:?}",
        n_validators,
        missing.len(),
        writes.len(),
        missing
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
#[ignore = "green-gate target for the sealed-floor / record-recovery design (run with --ignored)"]
async fn two_writers_converge() { run_convergence(2, 1, 7).await; }

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
#[ignore = "green-gate target for the sealed-floor / record-recovery design (run with --ignored)"]
async fn three_writers_converge() { run_convergence(3, 1, 21).await; }

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
#[ignore = "green-gate target for the sealed-floor / record-recovery design (run with --ignored)"]
async fn three_writers_converge_under_load() { run_convergence(3, 3, 21).await; }
