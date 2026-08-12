// Exactly-once at the merge: one signature, one effect, in every committed
// state.
//
// Two reds, one per delivery mechanism of the duplicate:
//
// 1. SIBLING COPIES ACROSS THE BASE/SCOPE BOUNDARY — the majority-stake
//    validator's copy becomes the merge base (instant self-witness floor),
//    the other copy arrives in scope, and freshest-copy dedup never sees a
//    pair because it only compares scope chains. Both effects land.
//
// 2. REINSTATED EFFECT PLUS FRESH RE-EXECUTION — a merge re-applies a
//    rejected chain from scope (reinstatement, invisible to the
//    deploys-in-scope walk because the effect travels as diffs, not as a
//    body deploy), and the retry path re-selects the same deploy from the
//    proposer's stores (the rejected-in-scope exemption bypasses the scope
//    filter). The block executes a deploy whose effect is already in its
//    own pre-state.
//
// Both were verified in production runs (two-datum single-value cells,
// `[0, 0]` twin initializations, the refund-failure quarantine destroying
// the deploy). The fix under test: the settled-in-base dedup sentinel in
// the merger and the never-execute-what-the-pre-state-has guard in the
// block creator.

use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use prost::bytes::Bytes;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn rejected_sigs(block: &BlockMessage) -> Vec<Bytes> {
    block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| rd.sig.clone())
        .collect()
}

fn short(sig: &Bytes) -> String { hex::encode(&sig[..8.min(sig.len())]) }

async fn three_node_network() -> (Vec<TestNode>, String) {
    let n_validators = 3usize;
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, n_validators, None, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    (nodes, shard_id)
}

/// Every datum currently on `@"<name>"` at `state_hash`, decoded to string
/// payloads. The exactly-once assert is a datum COUNT on these cells.
async fn string_datums(node: &TestNode, state_hash: &Bytes, name: &str) -> Vec<String> {
    let channel = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(name.to_string())),
        }],
        ..Default::default()
    };
    node.runtime_manager
        .get_data(state_hash.clone(), &channel)
        .await
        .unwrap_or_else(|e| panic!("get_data @\"{}\": {:?}", name, e))
        .iter()
        .map(
            |par| match par.exprs.first().and_then(|e| e.expr_instance.clone()) {
                Some(ExprInstance::GString(s)) => s,
                other => format!("<non-string datum: {:?}>", other),
            },
        )
        .collect()
}

/// True iff `hash` is the floor of `tip` or one of that floor's DAG
/// ancestors, per this node's DAG.
async fn floor_covers(node: &TestNode, tip: &Bytes, hash: &Bytes, height: i64) -> bool {
    let dag = node.casper.block_dag().await.expect("dag representation");
    let floor = floor_of_block(&dag, tip, FtThreshold::from_f32_lossy(0.0))
        .await
        .expect("floor_of_block");
    if floor.hash == *hash {
        return true;
    }
    floor.block_number >= height
        && dag
            .is_dag_ancestor(hash, &floor.hash)
            .expect("is_dag_ancestor")
}

/// RED 1: two SIBLING inclusions of one signature, no rejection record
/// anywhere. The majority-stake validator's copy becomes the floor of the
/// reconciling merge (instant self-witness), so its copy arrives via the
/// BASE while only the other copy is in scope — freshest-copy dedup never
/// sees a pair (it only compares scope chains against each other). The
/// settled-in-base sentinel must drop the scope copy. One signature, one
/// effect.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn duplicate_sibling_inclusions_must_reconcile_to_one_effect() {
    let (mut nodes, shard_id) = three_node_network().await;

    let dup_deploy = construct_deploy::source_deploy_now_full(
        r#"@"X3"!("once")"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build duplicate deploy");

    // Copy in C (nodes[2], height 1) and copy in A (nodes[0], height 2 —
    // above a spacer so the copies sit at distinct heights and dedup's
    // freshest-wins ordering is deterministic).
    let c_block = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(&dup_deploy))
        .await
        .expect("sibling C");
    let s0 = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        let spacer = construct_deploy::basic_deploy_data(
            0,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("spacer");
        nodes[0]
            .add_block_from_deploys(std::slice::from_ref(&spacer))
            .await
            .expect("spacer S0")
    };
    let a_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&dup_deploy))
        .await
        .expect("sibling A");

    // The reconciling merge (nodes[1]).
    for block in [&c_block, &s0, &a_block] {
        nodes[1]
            .process_block((*block).clone())
            .await
            .expect("nodes[1] processes the siblings");
    }
    let marker = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            1,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("merge marker")
    };
    let m_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&marker))
        .await
        .expect("reconciling merge M");
    assert!(
        m_block
            .header
            .parents_hash_list
            .contains(&a_block.block_hash)
            && m_block
                .header
                .parents_hash_list
                .contains(&c_block.block_hash),
        "M must merge both sibling carriers; parents={:?}",
        m_block
            .header
            .parents_hash_list
            .iter()
            .map(|h| hex::encode(&h[..4.min(h.len())]))
            .collect::<Vec<_>>()
    );

    // THE RED: one signature, one effect — however the merge reconciles
    // (settled-in-base drop or rejection-with-record), the merged state
    // must hold exactly one datum.
    let x3_at_m = string_datums(&nodes[1], &m_block.body.state.post_state_hash, "X3").await;
    assert_eq!(
        x3_at_m.len(),
        1,
        "DUPLICATE CO-MERGE: @\"X3\" holds {} datums after the merge that \
         should reconcile two sibling copies of one signature (datums: \
         {:?}; rejected: {:?})",
        x3_at_m.len(),
        x3_at_m,
        rejected_sigs(&m_block)
            .iter()
            .map(short)
            .collect::<Vec<_>>()
    );
}

/// Shared staging for the reinstated-effect specs — a floor-covered
/// reinstatement with a late-arriving record:
/// - contest on nodes[0]/nodes[1]; the adjudicating merge M rejects one
///   contender with a record;
/// - nodes[2] (majority stake, no record in view) merges the loser's
///   carrier: the loser's chain is APPLIED FROM SCOPE (reinstatement). The
///   effect now travels as merge diffs — invisible to the
///   deploys-in-scope body walk;
/// - nodes[2]'s floor advances over the carrier (self-witness spacers,
///   verified per round — staging precondition, not assumed);
/// - the record side (winner + M) is delivered LATE: nodes[2] now holds a
///   record adjudicating work its canonical lineage already carries.
///
/// Returns (nodes, shard_id, loser_sig, loser_cell, loser_deploy, m_block).
async fn stage_floor_covered_reinstatement() -> (
    Vec<TestNode>,
    String,
    Bytes,
    &'static str,
    Signed<DeployData>,
    BlockMessage,
) {
    let (mut nodes, shard_id) = three_node_network().await;

    // Seed the contended cell (nodes[1]); nodes[0] and nodes[2] process it.
    let seed = construct_deploy::source_deploy_now_full(
        r#"@"race"!("s")"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build seed");
    let s_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&seed))
        .await
        .expect("seed block S");
    for i in [0usize, 2usize] {
        nodes[i]
            .process_block(s_block.clone())
            .await
            .expect("process S");
    }

    // nodes[2]'s neutral branch off S FIRST, so its later merge takes the
    // loser's chain via SCOPE (not spine inheritance).
    let _n1 = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(
            &construct_deploy::basic_deploy_data(
                100,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("neutral spacer"),
        ))
        .await
        .expect("neutral branch N1 on nodes[2]");

    // Contenders: both consume the seed (genuine conflict), both write a
    // witness cell, and both CREATE a number cell (`new c in { c!(0) }`) so
    // the deploy's effect is visible to the created-cell state probe.
    let contender_d = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") { @"race"!("d") | @"XD"!(v) | new c in { c!(0) } }"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(0),
            Some(shard_id.clone()),
        )
        .expect("build contender d")
    };
    let contender_f = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") { @"race"!("f") | @"XF"!(v) | new c in { c!(0) } }"#.to_string(),
            None,
            None,
            Some(
                crate::util::genesis_builder::EXTRA_GENESIS_VAULT_KEY_PAIRS[0]
                    .0
                    .clone(),
            ),
            None,
            Some(shard_id.clone()),
        )
        .expect("build contender f")
    };
    let d_sig: Bytes = contender_d.sig.clone();
    let f_sig: Bytes = contender_f.sig.clone();
    let c_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&contender_d))
        .await
        .expect("block C with d");
    let a_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&contender_f))
        .await
        .expect("block A with f");

    // Adjudicating merge M on nodes[1].
    nodes[1]
        .process_block(c_block.clone())
        .await
        .expect("nodes[1] processes C");
    let m_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(
            &construct_deploy::basic_deploy_data(
                101,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("m marker"),
        ))
        .await
        .expect("adjudicating merge M");
    let m_rejected = rejected_sigs(&m_block);
    let d_lost = m_rejected.contains(&d_sig);
    let f_lost = m_rejected.contains(&f_sig);
    assert!(
        d_lost ^ f_lost,
        "exactly one contender must be rejected at M (rejected: {:?})",
        m_rejected.iter().map(short).collect::<Vec<_>>(),
    );
    let (loser_sig, loser_cell, carrier, loser_deploy, winner_block) = if d_lost {
        (
            d_sig.clone(),
            "XD",
            c_block.clone(),
            contender_d,
            a_block.clone(),
        )
    } else {
        (
            f_sig.clone(),
            "XF",
            a_block.clone(),
            contender_f,
            c_block.clone(),
        )
    };

    // THE REINSTATEMENT: nodes[2] sees only S, N1, and the loser's carrier
    // — no record. Its merge applies the loser's chain from scope.
    nodes[2]
        .process_block(carrier.clone())
        .await
        .expect("nodes[2] processes the loser's carrier only");
    let x_block = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(
            &construct_deploy::basic_deploy_data(
                102,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("x marker"),
        ))
        .await
        .expect("reinstating merge X on nodes[2]");
    assert!(
        !rejected_sigs(&x_block).contains(&loser_sig),
        "X must not reject the loser (no record in view)"
    );
    let x_live = !string_datums(&nodes[2], &x_block.body.state.post_state_hash, loser_cell)
        .await
        .is_empty();
    assert!(
        x_live,
        "the reinstating merge X must carry the loser's effect in its \
         post-state (applied from scope)"
    );

    // Advance nodes[2]'s floor over the carrier so the reinstated content
    // sits below the base of its next merge. Verified per round.
    let carrier_height = carrier.body.state.block_number;
    let mut covered = false;
    let mut last_spacer: Option<BlockMessage> = None;
    for round in 0..30i32 {
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(
                &construct_deploy::basic_deploy_data(
                    110 + round,
                    Some(construct_deploy::DEFAULT_SEC2.clone()),
                    Some(shard_id.clone()),
                )
                .expect("floor spacer"),
            ))
            .await
            .expect("floor spacer block");
        last_spacer = Some(b.clone());
        if floor_covers(
            &nodes[2],
            &b.block_hash,
            &carrier.block_hash,
            carrier_height,
        )
        .await
        {
            covered = true;
            break;
        }
    }
    assert!(
        covered,
        "staging precondition: nodes[2]'s floor must come to cover the \
         loser's carrier within the spacer rounds"
    );
    let _ = last_spacer;

    // The record side arrives: winner branch and M. nodes[2]'s next
    // proposal will merge its own lineage with the record branch.
    for b in [&winner_block, &m_block] {
        nodes[2]
            .process_block((*b).clone())
            .await
            .expect("nodes[2] ingests the record side");
    }

    (
        nodes,
        shard_id,
        loser_sig,
        loser_cell,
        loser_deploy,
        m_block,
    )
}

/// RED 2: a reinstated effect must not be executed again by the block that
/// inherits it.
///
/// On top of the shared staging, the loser is placed in nodes[2]'s stores
/// the way the recovery plane does (deploy storage + rejected buffer); the
/// rejected-in-scope exemption then re-admits it through selection.
///
/// The proposal is driven through `block_creator::create` on the FULL
/// frontier (own tip + the record branch). Going through the node's own
/// snapshot instead cannot stage the geometry: parent narrowing collapses
/// the frontier to a single parent whenever a record is in candidate
/// scope, so the record branch never enters scope and selection filters
/// the retry. Narrowing only serializes one node's own proposals — any
/// validator without the narrowing trigger active legally mints the
/// widened block from the same view, which is the block this call
/// constructs. The scope sets are hand-extended to their true
/// widened-frontier values (the record is visible from these parents),
/// which is what re-admits the retry through the rejected-in-scope
/// exemption.
///
/// Without the pre-state guard the created block executes the deploy
/// twice: the witness cell holds two datums in its committed post-state.
/// With the guard the fresh copy is dropped and the effect appears exactly
/// once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn reinstated_effect_must_not_be_executed_again() {
    let (mut nodes, _shard_id, loser_sig, loser_cell, loser_deploy, m_block) =
        stage_floor_covered_reinstatement().await;

    // The recovery plane returns the loser to nodes[2]'s stores (the
    // verified return routes are several — validate-side populate, owner
    // retry, client resubmission; the seam models their common endpoint).
    nodes[2]
        .deploy_storage
        .lock()
        .add(vec![loser_deploy.clone()])
        .expect("inject loser into deploy storage");
    nodes[2]
        .rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .add(vec![loser_deploy])
        .expect("inject loser into rejected buffer");

    // Drive the proposal through `create` on the FULL frontier: nodes[2]'s
    // own tip plus the record branch — the block any non-narrowed proposer
    // would mint from this view. The scope sets get their true
    // widened-frontier values: the record is visible from these parents,
    // which re-admits the buffered retry through the rejected-in-scope
    // exemption.
    let mut snapshot = nodes[2]
        .casper
        .get_snapshot()
        .await
        .expect("nodes[2] snapshot");
    if !snapshot
        .parents
        .iter()
        .any(|p| p.block_hash == m_block.block_hash)
    {
        snapshot.parents.push(m_block.clone());
    }
    snapshot.rejected_in_scope.insert(loser_sig.clone());
    snapshot.deploys_in_scope.insert(loser_sig.clone());

    let validator_identity = nodes[2]
        .validator_id_opt
        .clone()
        .expect("nodes[2] validator identity");
    let deploy_storage = nodes[2].deploy_storage.clone();
    let rejected_buffer = nodes[2].rejected_deploy_buffer.clone();
    let runtime_manager = nodes[2].runtime_manager.clone();
    let created = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        deploy_storage,
        rejected_buffer,
        &runtime_manager,
        &mut nodes[2].block_store,
        true,
    )
    .await
    .expect("create proposal B on the full frontier");
    let BlockCreatorResult::Created(b_block, _pre, b_post) = created else {
        panic!("create must mint a block on the full frontier; got a non-Created result");
    };

    // Positive purge pin: the loser's effect is settled in nodes[2]'s floor
    // state (the floor covers its carrier — staged above), and the loser
    // creates a number cell, so the effect probe attests it. The prepare
    // inside `create` must therefore evict the buffer entry: floor-settled
    // work has left recovery custody.
    assert!(
        !nodes[2]
            .rejected_deploy_buffer
            .lock()
            .expect("buffer lock")
            .contains_sig(&loser_sig)
            .expect("buffer.contains_sig"),
        "a buffer entry whose effect is settled in the floor state must be \
         purged by the proposer's prepare"
    );

    // THE RED: the loser's witness cell must hold EXACTLY ONE datum in B's
    // committed post-state. Two datums = the same signature executed twice
    // into one state (the verified [0,0] twin-initialization class).
    let witness = string_datums(&nodes[2], &b_post, loser_cell).await;
    assert_eq!(
        witness.len(),
        1,
        "DOUBLE EXECUTION: @\"{}\" holds {} datums in B's post-state — the \
         reinstated effect was in B's pre-state and the block executed the \
         deploy again (datums: {:?}; B carries loser fresh: {}; B rejected: \
         {:?})",
        loser_cell,
        witness.len(),
        witness,
        b_block
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == loser_sig),
        rejected_sigs(&b_block)
            .iter()
            .map(short)
            .collect::<Vec<_>>(),
    );
}

/// THE SCHEDULED FALSIFIER (state plane): a floor-covered effect is
/// immovable — a late record must not unwind it.
///
/// An instrumented experiment on the old branch once unwound a
/// floor-covered effect after a late record arrived, with the merge cut
/// clamped to the floor; the mechanism was never root-caused. On this
/// base — merge = pure function(floor state, above-floor diffs), records
/// never reaching state derivation — that unwinding should be structurally
/// impossible: the base always carries the effect forward, and the record
/// is testimony about an adjudication the canonical lineage never took.
///
/// On top of the shared staging (reinstated effect floor-covered on
/// nodes[2], record delivered late), the two live validators alternate
/// rounds until every floor covers the record's carrier M, plus settle
/// rounds. The loser's witness effect must still be in the tip state.
///
/// This is an EXPERIMENT with both outcomes recorded: a red here finds the
/// old anomaly's mechanism on the clean rebuild at the cheap end of the
/// stack; a green pins that the old unwinding was branch-residual
/// machinery and licenses the verdict plane to read floor-covered effects
/// as settled.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn floor_covered_effect_survives_a_late_record() {
    let (mut nodes, shard_id, loser_sig, loser_cell, _loser_deploy, m_block) =
        stage_floor_covered_reinstatement().await;
    let m_height = m_block.body.state.block_number;

    // Join the lineages the way any non-narrowed validator legally would:
    // one widened merge J over nodes[2]'s own tip and the record branch.
    // The join cannot be staged through the node's own propose path —
    // M's record sits in candidate scope, so parent narrowing collapses
    // every self-proposal to a single parent and the lineages never meet
    // (same diagnosis as the spec above).
    let j_block = {
        let mut snapshot = nodes[2]
            .casper
            .get_snapshot()
            .await
            .expect("nodes[2] snapshot");
        if !snapshot
            .parents
            .iter()
            .any(|p| p.block_hash == m_block.block_hash)
        {
            snapshot.parents.push(m_block.clone());
        }
        let validator_identity = nodes[2]
            .validator_id_opt
            .clone()
            .expect("nodes[2] validator identity");
        let deploy_storage = nodes[2].deploy_storage.clone();
        let rejected_buffer = nodes[2].rejected_deploy_buffer.clone();
        let runtime_manager = nodes[2].runtime_manager.clone();
        let created = block_creator::create(
            &snapshot,
            &validator_identity,
            None,
            deploy_storage,
            rejected_buffer,
            &runtime_manager,
            &mut nodes[2].block_store,
            true,
        )
        .await
        .expect("create the joining merge J");
        let BlockCreatorResult::Created(j_block, _pre, _post) = created else {
            panic!("create must mint the joining merge J on the full frontier");
        };
        j_block
    };
    for node in nodes.iter_mut() {
        if !node.contains(&j_block.block_hash) {
            node.process_block(j_block.clone())
                .await
                .expect("admit the joining merge J");
        }
    }

    // Settle rounds: nodes[2] (5/9 stake — a self-witnessing majority)
    // mints on the joined spine, delivered to all, until its floor covers
    // the record's carrier M, plus a few rounds beyond.
    let mut tip: Option<BlockMessage> = None;
    let mut rounds_after_coverage = 0i32;
    for round in 0..30i32 {
        let marker = {
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            construct_deploy::basic_deploy_data(
                300 + round,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("settle marker")
        };
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("settle round");
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone())
                    .await
                    .expect("settle delivery");
            }
        }
        tip = Some(b.clone());
        if floor_covers(&nodes[2], &b.block_hash, &m_block.block_hash, m_height).await {
            rounds_after_coverage += 1;
            if rounds_after_coverage >= 5 {
                break;
            }
        }
    }
    let tip = tip.expect("at least one settle round ran");
    assert!(
        rounds_after_coverage >= 5,
        "staging precondition: the floor must come to cover the record's \
         carrier M within the settle rounds, plus rounds beyond — \
         otherwise the falsifier never exercises the transient",
    );

    // THE FALSIFIER. The reinstated effect was floor-covered BEFORE the
    // record entered the canonical lineage; every merge since starts from
    // a base that carries it. If the effect is gone, a record reached back
    // into state derivation — the fifth unwinding mechanism, found.
    let live = !string_datums(&nodes[2], &tip.body.state.post_state_hash, loser_cell)
        .await
        .is_empty();
    assert!(
        live,
        "UNWINDING: the loser's floor-covered effect (@\"{}\", sig {}) is \
         no longer in the canonical tip state after its record canonicalized \
         late — a rejection record reached back into state derivation",
        loser_cell,
        short(&loser_sig),
    );
}
