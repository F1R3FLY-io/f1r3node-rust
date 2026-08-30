// Write-once terminal verdicts must agree across nodes and always land.
//
// Five defect classes from the hunt era, each pinned against the
// settled-verdict register (verdicts read floor facts and recorded
// construction pointers only — never a node's record row at evaluation
// time, whose arrival-order dependence froze contradictory write-once
// verdicts: specimen 87a5d970, one deploy Expired on one node and
// Finalized on four):
//
// 1. Record arrival order must not split terminal verdicts.
// 2. Evidence that can never canonicalize must not strand Pending forever.
// 3. A reinstatement transient must not freeze a premature verdict.
// 4. A terminal verdict must cohere with the canonical state it reports.
// 5. Depth below the tips is not evidence of loss.
use casper::rust::api::deploy_finalization_status::{self};
use casper::rust::casper::MultiParentCasper;
use casper::rust::util::construct_deploy;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn rejected_sigs(block: &BlockMessage) -> Vec<Bytes> {
    block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| Bytes::copy_from_slice(rd.deploy_id()))
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
    // A FINITE parent depth: the register's Expired/Failed bound derives
    // from `citability_horizon(max_parent_depth)`; under the i32::MAX
    // disabled sentinel those verdicts are never writable (bound = None →
    // Pending by design) and the landing asserts here would fail
    // vacuously.
    let mut nodes = TestNode::create_network_with_deploy_lifespan(
        genesis,
        n_validators,
        None,
        None,
        Some(10),
        None,
        None,
    )
    .await
    .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    (nodes, shard_id)
}

/// One node's current verdict for a sig, as the status API reports it.
fn verdict(node: &TestNode, sig: &Bytes) -> String {
    let dag = node
        .block_dag_storage
        .get_representation()
        .expect("dag representation");
    let status = deploy_finalization_status::resolve(
        &dag,
        &node.block_store,
        &crate::legacy_deploy_id(sig),
        None,
    )
    .expect("resolve");
    format!("{:?}", status.state)
}

/// Contest only: seed -> contenders -> record at M -> carrier lineage on
/// the majority validator -> full convergence. The owner has NOT recovered
/// anything yet. Returns (nodes, shard_id, loser_sig, loser_owner, the
/// loser's witness cell).
async fn stage_contest() -> (Vec<TestNode>, String, Bytes, usize, &'static str) {
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

    // Two symmetric contenders consuming the seed; exactly one loses with
    // a record at the adjudicating merge M.
    let contender_d = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") { @"race"!("d") | @"XD"!(v) }"#.to_string(),
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
            r#"for (@v <- @"race") { @"race"!("f") | @"XF"!(v) }"#.to_string(),
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
    let m_marker = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            0,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("m marker")
    };
    let m_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&m_marker))
        .await
        .expect("adjudicating merge M");
    let m_rejected = rejected_sigs(&m_block);
    let d_lost = m_rejected.contains(&d_sig);
    let f_lost = m_rejected.contains(&f_sig);
    assert!(
        d_lost ^ f_lost,
        "exactly one contender must be rejected with a record at M \
         (rejected: {:?}, d: {}, f: {})",
        m_rejected.iter().map(short).collect::<Vec<_>>(),
        short(&d_sig),
        short(&f_sig),
    );
    let (loser_sig, loser_owner, carrier, loser_cell) = if d_lost {
        (d_sig.clone(), 0usize, c_block.clone(), "XD")
    } else {
        (f_sig.clone(), 1usize, a_block.clone(), "XF")
    };

    // The majority-stake validator extends the CARRIER lineage only — the
    // floor walk finds the carrier witnessed, which later opens the gate.
    if carrier.block_hash != s_block.block_hash {
        nodes[2]
            .process_block(carrier.clone())
            .await
            .expect("nodes[2] processes the carrier");
    }
    let spacer = |n: i32| {
        construct_deploy::basic_deploy_data(
            n,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("spacer deploy")
    };
    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let r2a = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(&spacer(1)))
        .await
        .expect("carrier-lineage spacer R2a");
    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let r2b = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(&spacer(2)))
        .await
        .expect("carrier-lineage spacer R2b");

    // Everyone converges on both branches.
    for node in nodes.iter_mut() {
        for block in [&c_block, &a_block, &m_block, &r2a, &r2b] {
            if node
                .casper
                .block_dag()
                .await
                .expect("dag")
                .contains(&block.block_hash)
            {
                continue;
            }
            node.process_block((*block).clone())
                .await
                .expect("converge on both branches");
        }
    }
    (nodes, shard_id, loser_sig, loser_owner, loser_cell)
}

/// RED 1 class (hunt specimen 87a5d970): record arrival order must never
/// split write-once verdicts across nodes. The specimen froze
/// ["Expired", "Finalized", "Finalized"] for one deploy — each node's
/// verdict was a function of WHEN its record row filled, and write-once
/// made the disagreement permanent.
///
/// On this base a verdict reads floor facts only. The contest's record
/// reaches every node at a different point in the delivery order, and the
/// floor settles the LOSER's carrier lineage — the record is per-merge
/// testimony, not a lever, so every node converges on the same verdict
/// (Finalized, with the rejection preserved in the display row) no matter
/// what it had ingested first. No sampling point may show two nodes with
/// contradictory TERMINAL states.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn in_flight_rejection_must_not_split_terminal_verdicts() {
    let (mut nodes, shard_id, loser_sig, _loser_owner, _loser_cell) = stage_contest().await;

    let mut all_terminal_equal = false;
    for round in 0..60i32 {
        let marker = {
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            construct_deploy::basic_deploy_data(
                60 + round,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("settle marker")
        };
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("settle proposal");
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone())
                    .await
                    .expect("settle delivery");
            }
        }

        // The class assertion, at EVERY observation point: no two nodes may
        // ever hold contradictory terminal states for one sig.
        let sample: Vec<String> = nodes.iter().map(|n| verdict(n, &loser_sig)).collect();
        let terminals: Vec<&String> = sample.iter().filter(|v| *v != "Pending").collect();
        assert!(
            terminals.windows(2).all(|w| w[0] == w[1]),
            "round {}: contradictory write-once terminal verdicts for {}: \
             {:?} — a verdict depended on record arrival order",
            round,
            short(&loser_sig),
            sample,
        );
        all_terminal_equal =
            sample.iter().all(|v| v != "Pending") && sample.windows(2).all(|w| w[0] == w[1]);
        if all_terminal_equal {
            break;
        }
    }
    assert!(
        all_terminal_equal,
        "every node must reach the same terminal verdict for {}; got {:?}",
        short(&loser_sig),
        nodes
            .iter()
            .map(|n| verdict(n, &loser_sig))
            .collect::<Vec<_>>(),
    );
    finish_race_assert(&nodes, &loser_sig, 0);
}

/// A deploy's verdict comes from state membership, not from its carrier's
/// spine position: a carrier merged only ever as a SECONDARY parent — no
/// spine passes through it — still lands Finalized on every node once the
/// floor covers the merge that applied its chain. This is the fact that
/// makes main-parent choice a pure fork-choice concern: no deploy needs
/// its carrier promoted onto the spine to finalize.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_deploy_finalizes_from_a_carrier_the_spine_never_holds() {
    let (mut nodes, shard_id) = three_node_network().await;

    // A settled common base.
    let base_marker = construct_deploy::basic_deploy_data(
        150,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(shard_id.clone()),
    )
    .expect("base marker");
    let base = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&base_marker))
        .await
        .expect("base block");
    for i in [0usize, 2usize] {
        nodes[i]
            .process_block(base.clone())
            .await
            .expect("process base");
    }

    // The carrier B on nodes[0] — a child of the base. nodes[1] has NOT
    // seen it when it mints the sibling S, so B and S race at one height.
    let deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"@"offspine"!("x")"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(0),
            Some(shard_id.clone()),
        )
        .expect("build off-spine deploy")
    };
    let sig: Bytes = deploy.sig.clone();
    let b_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy))
        .await
        .expect("carrier B");
    let sibling_marker = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            151,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("sibling marker")
    };
    let s_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&sibling_marker))
        .await
        .expect("sibling S");

    // Everyone learns both branches; the merge M is FORCED to spine
    // through S (parents[0]) with B on the secondary edge.
    nodes[1]
        .process_block(b_block.clone())
        .await
        .expect("nodes[1] processes B");
    for i in [0usize, 2usize] {
        nodes[i]
            .process_block(s_block.clone())
            .await
            .expect("process S");
    }
    nodes[2]
        .process_block(b_block.clone())
        .await
        .expect("nodes[2] processes B");
    let m_block =
        super::staging::mint_on_parents(&mut nodes[1], vec![s_block.clone(), b_block.clone()], "M")
            .await;
    for i in [0usize, 2usize] {
        nodes[i]
            .process_block(m_block.clone())
            .await
            .expect("process M");
    }

    // Settle rounds on M's spine until every node's verdict lands.
    let mut all_finalized = false;
    let mut last_tip = m_block.block_hash.clone();
    for round in 0..60i32 {
        let marker = {
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            construct_deploy::basic_deploy_data(
                160 + round,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("settle marker")
        };
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("settle proposal");
        last_tip = b.block_hash.clone();
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone())
                    .await
                    .expect("settle delivery");
            }
        }

        let sample: Vec<String> = nodes.iter().map(|n| verdict(n, &sig)).collect();
        assert!(
            sample.iter().all(|v| v == "Pending" || v == "Finalized"),
            "round {}: the off-spine carrier's deploy must never earn a \
             non-Finalized terminal — its chain was merge-applied; got {:?}",
            round,
            sample,
        );
        all_finalized = sample.iter().all(|v| v == "Finalized");
        if all_finalized {
            break;
        }
    }

    // The staging held: the spine never passed through B.
    let dag = nodes[1]
        .block_dag_storage
        .get_representation()
        .expect("dag representation");
    assert!(
        dag.is_in_main_chain(&s_block.block_hash, &last_tip)
            .expect("spine check S"),
        "staging: the settled spine must pass through the sibling S"
    );
    assert!(
        !dag.is_in_main_chain(&b_block.block_hash, &last_tip)
            .expect("spine check B"),
        "staging: no spine may pass through the carrier B"
    );
    assert!(
        all_finalized,
        "the off-spine carrier's deploy must finalize on every node; got {:?}",
        nodes.iter().map(|n| verdict(n, &sig)).collect::<Vec<_>>(),
    );
}

/// RED 2 class (85f52810 shape): a sig whose only evidence can never
/// become canonical must still reach SOME terminal verdict once every
/// horizon has passed — it must not strand Pending forever.
///
/// The owner proposes a carrier that is NEVER delivered and then stays
/// silent: the deploy exists in exactly one node's DAG, on a branch the
/// shard's floors never touch. Once the floor passes the contestability
/// bound (window end + citability horizon on a small-mpd shard), nothing
/// can ever apply or re-include the sig, and the owner's register must
/// write Expired — the one verdict a dead private branch can earn.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn private_carrier_must_not_strand_the_verdict_pending_forever() {
    let n_validators = 3usize;
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network_with_deploy_lifespan(
        genesis,
        n_validators,
        None,
        None,
        Some(5),
        None,
        Some(10),
    )
    .await
    .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let private_deploy = construct_deploy::source_deploy_now_full(
        r#"@"stranded"!(1)"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build private deploy");
    let private_sig: Bytes = private_deploy.sig.clone();
    nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&private_deploy))
        .await
        .expect("owner proposes the private carrier");

    // The shard advances without the carrier; the owner ingests every
    // round, so its register observes the floor passing the bound
    // (window_end 10 + horizon 5, small margins for floor lag).
    for round in 0..40i32 {
        let marker = {
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            construct_deploy::basic_deploy_data(
                700 + round,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("horizon marker")
        };
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("horizon round");
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone())
                    .await
                    .expect("horizon delivery");
            }
        }
        if verdict(&nodes[0], &private_sig) != "Pending" {
            break;
        }
    }
    assert_eq!(
        verdict(&nodes[0], &private_sig),
        "Expired",
        "stranded: the owner's register never terminalized {} although the \
         validity window is long closed and the carrier is long past the \
         citability horizon — nothing can ever apply it again",
        short(&private_sig),
    );
}

/// Every datum currently on `@"<name>"` at `state_hash`, decoded to string
/// payloads — the loser's witness cell holds the consumed value iff the
/// loser's execution is live in that state.
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

/// RED 5 (reinstatement transient): a chain rejected WITH a record by one
/// merge is re-applied by ANOTHER merge whose view lacks the record
/// (reinstatement — the primary recovery path). The reinstating lineage
/// carries the majority stake and becomes canonical — for a while. When
/// the record canonicalizes late, the pair re-pins the settled cut below
/// the carrier and the re-merge honors the record: the reinstated effect
/// is UNWOUND from the state (verified at head by this fixture's own
/// staging — the 2fb3dffd "Expired over live effects" endpoint is
/// structurally closed by carrier-equality unification, because a
/// canonical record always wins the state back).
///
/// The defect that remains — and that this spec pins: a register that
/// evaluates DURING the transient freezes a write-once verdict the state
/// then abandons. Verified failure on current code: the reinstating
/// node's register writes Finalized while the effect is canonical, the
/// state unwinds, and the terminal verdict permanently asserts effects
/// that do not exist. Under the settled-verdict design no verdict is
/// written before the formability bound, past which no admissible block
/// can introduce a record or re-pin the cut — the state under the cut is
/// stable, so every verdict is coherent and identical across nodes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn verdict_must_not_freeze_during_reinstatement_transient() {
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

    // nodes[2] mints its neutral branch off S FIRST — its merge later must
    // fork below everything so the loser's chain arrives via SCOPE, not
    // via spine inheritance (that is what forces applied_from_scope).
    let _n1 = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(
            &construct_deploy::basic_deploy_data(
                1100,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("neutral spacer"),
        ))
        .await
        .expect("neutral branch N1 on nodes[2]");

    // Contenders and the adjudicating merge M — nodes[0]/nodes[1] only.
    let contender_d = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") { @"race"!("d") | @"XD"!(v) }"#.to_string(),
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
            r#"for (@v <- @"race") { @"race"!("f") | @"XF"!(v) }"#.to_string(),
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
    nodes[1]
        .process_block(c_block.clone())
        .await
        .expect("nodes[1] processes C");
    let m_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(
            &construct_deploy::basic_deploy_data(
                1101,
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
    assert!(d_lost ^ f_lost, "exactly one contender rejected at M");
    let (loser_sig, loser_cell, carrier, winner_block) = if d_lost {
        (d_sig.clone(), "XD", c_block.clone(), a_block.clone())
    } else {
        (f_sig.clone(), "XF", a_block.clone(), c_block.clone())
    };
    // nodes[0] converges on the record side.
    for b in [&a_block, &m_block] {
        if !nodes[0]
            .casper
            .block_dag()
            .await
            .expect("dag")
            .contains(&b.block_hash)
        {
            nodes[0]
                .process_block((*b).clone())
                .await
                .expect("nodes[0] converges on the record side");
        }
    }

    // THE REINSTATEMENT. nodes[2] holds only S, N1, and now the loser's
    // carrier — no record anywhere in its view. Its next merge spans
    // (N1, carrier): the loser's chain is in scope, nothing adjudicates
    // it, so the merge must APPLY it.
    nodes[2]
        .process_block(carrier.clone())
        .await
        .expect("nodes[2] processes the loser's carrier only");
    let x_block = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(
            &construct_deploy::basic_deploy_data(
                1102,
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

    // The majority lineage becomes canonical: nodes[2] mints rounds on X,
    // delivered to everyone (nodes[0]/nodes[1] adopt it while privately
    // holding the record).
    for round in 0..10i32 {
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(
                &construct_deploy::basic_deploy_data(
                    1110 + round,
                    Some(construct_deploy::DEFAULT_SEC2.clone()),
                    Some(shard_id.clone()),
                )
                .expect("settle marker"),
            ))
            .await
            .expect("settle round");
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone())
                    .await
                    .expect("settle delivery");
            }
        }
    }

    // The record canonicalizes LATE: nodes[2] ingests the winner branch
    // and M, merges them in, and keeps minting; every node's floor now
    // covers the record while the reinstated effect stays in the state.
    for b in [&winner_block, &m_block] {
        nodes[2]
            .process_block((*b).clone())
            .await
            .expect("nodes[2] ingests the record side");
    }
    let mut tip_state: Option<Bytes> = None;
    for round in 0..90i32 {
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(
                &construct_deploy::basic_deploy_data(
                    1200 + round,
                    Some(construct_deploy::DEFAULT_SEC2.clone()),
                    Some(shard_id.clone()),
                )
                .expect("horizon marker"),
            ))
            .await
            .expect("horizon round");
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone())
                    .await
                    .expect("horizon delivery");
            }
        }
        tip_state = Some(b.body.state.post_state_hash.clone());
        let all_terminal = (0..nodes.len()).all(|v| verdict(&nodes[v], &loser_sig) != "Pending");
        if all_terminal {
            break;
        }
    }

    // COHERENCE, both directions, whichever way the state settled. The
    // reinstated effect was canonical for the settle rounds; a register
    // that wrote during that transient froze a verdict the state may no
    // longer back (verified at head: a late canonical record re-pins the
    // cut and UNWINDS the reinstated effect). Finalized requires the
    // effect live; Expired requires it absent — a write-once verdict over
    // a state that changed underneath it fails one of the two.
    let tip_state = tip_state.expect("at least one horizon round ran");
    let live = !string_datums(&nodes[2], &tip_state, loser_cell)
        .await
        .is_empty();
    for (i, node) in nodes.iter().enumerate() {
        let v = verdict(node, &loser_sig);
        if v == "Finalized" {
            assert!(
                live,
                "nodes[{}] reports Finalized but the loser's effect is NOT \
                 in the canonical tip state (cell @\"{}\") — it wrote during \
                 the reinstatement transient and the state unwound beneath \
                 the write-once verdict",
                i, loser_cell,
            );
        }
        if v == "Expired" {
            assert!(
                !live,
                "nodes[{}] reports Expired but the loser's effect IS live \
                 in the canonical tip state (cell @\"{}\") — a terminal \
                 loss verdict over live, reinstated effects",
                i, loser_cell,
            );
        }
    }
}

/// RED 4 (2fb3dffd shape, coherence guard): the register's verdict must
/// agree with the canonical state. With the owner permanently silent (no
/// recovery path ever runs), drive the chain deep; then on every node,
/// Finalized requires the loser's effect to be live in the tip state and
/// Expired requires it to be absent. "Expired over live effects" — the
/// 2fb3dffd production instance — is the defect this pins.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn terminal_verdict_must_cohere_with_canonical_state() {
    let (mut nodes, shard_id, loser_sig, loser_owner, loser_cell) = stage_contest().await;
    let _ = loser_owner; // the owner never proposes again

    let mut tip_state: Option<Bytes> = None;
    for round in 0..90i32 {
        let marker = {
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            construct_deploy::basic_deploy_data(
                900 + round,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("coherence marker")
        };
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("coherence round");
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone())
                    .await
                    .expect("coherence delivery");
            }
        }
        tip_state = Some(b.body.state.post_state_hash.clone());
        if verdict(&nodes[loser_owner], &loser_sig) != "Pending"
            && verdict(&nodes[2], &loser_sig) != "Pending"
        {
            break;
        }
    }

    let tip_state = tip_state.expect("at least one coherence round ran");
    let live = !string_datums(&nodes[2], &tip_state, loser_cell)
        .await
        .is_empty();
    for (i, node) in nodes.iter().enumerate() {
        let v = verdict(node, &loser_sig);
        if v == "Finalized" {
            assert!(
                live,
                "nodes[{}] reports Finalized but the loser's effect is NOT \
                 in the canonical tip state (cell @\"{}\" empty)",
                i, loser_cell,
            );
        }
        if v == "Expired" {
            assert!(
                !live,
                "nodes[{}] reports Expired but the loser's effect IS live \
                 in the canonical tip state (cell @\"{}\") — the 2fb3dffd \
                 class: a terminal loss verdict over live effects",
                i, loser_cell,
            );
        }
    }
}

fn finish_race_assert(nodes: &[TestNode], loser_sig: &Bytes, _diverger: usize) {
    let final_verdicts: Vec<String> = nodes.iter().map(|n| verdict(n, loser_sig)).collect();
    assert!(
        final_verdicts.iter().all(|v| v == &final_verdicts[0]),
        "write-once terminal verdicts diverged across nodes for {}: {:?} — \
         the verdict depends on record arrival order, which write-once \
         semantics make permanent",
        short(loser_sig),
        final_verdicts,
    );
}

/// RED 3 (38237bb7 shape, regression guard if green): a deep, uncontested,
/// finalized win must report Finalized on every node and must NEVER be
/// Expired — depth below the tips is not evidence of loss.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn deep_uncontested_win_must_finalize_never_expire() {
    let (mut nodes, shard_id) = three_node_network().await;

    let lone = construct_deploy::source_deploy_now_full(
        r#"@"deepwin"!("landed")"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build lone deploy");
    let lone_sig: Bytes = lone.sig.clone();
    let w_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&lone))
        .await
        .expect("carrier block W");
    for i in [1usize, 2usize] {
        nodes[i]
            .process_block(w_block.clone())
            .await
            .expect("process W");
    }

    // Drive the chain deep past the carrier: the majority validator mints,
    // everyone ingests. At every observation point the verdict must never
    // read Expired, and it must reach Finalized on all nodes.
    for round in 0..100i32 {
        let marker = {
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            construct_deploy::basic_deploy_data(
                500 + round,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("depth marker")
        };
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("depth round");
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone()).await.expect("depth delivery");
            }
        }
        for (i, node) in nodes.iter().enumerate() {
            let v = verdict(node, &lone_sig);
            assert_ne!(
                v, "Expired",
                "round {}: nodes[{}] expired a finalized uncontested win — \
                 depth below the tips treated as loss (38237bb7 class)",
                round, i,
            );
        }
        let all_finalized = nodes.iter().all(|n| verdict(n, &lone_sig) == "Finalized");
        if all_finalized && round > 40 {
            break;
        }
    }
    let final_verdicts: Vec<String> = nodes.iter().map(|n| verdict(n, &lone_sig)).collect();
    assert!(
        final_verdicts.iter().all(|v| v == "Finalized"),
        "a deep uncontested win must reach Finalized on every node; got {:?}",
        final_verdicts,
    );
}
