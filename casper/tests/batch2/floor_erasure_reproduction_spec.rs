//! Direct reproduction of the ucc round-0 erasure (session 1f9bbf8f),
//! end-to-end through the real pipeline. The live geometry, faithfully:
//!
//! - one proposer (v2) carries BOTH contender branches (sibling blocks on a
//!   justification chain — the live shape: the rejecting merge sits on the
//!   same proposer's spine as the carrier it rejects);
//! - v2's spine grows tall over a STALE view (its stake cannot witness
//!   anything alone, so its floor never leaves the genesis era), and the
//!   rejecting merge R forms at the TOP of that spine — above where the
//!   floor will settle — adjudicating the contest against the
//!   pre-settlement base and rejecting one contender with a record;
//! - the rejected carrier then SETTLES: two clean settlers (who never saw R)
//!   witness it through a mutual-visibility chain whose floor lands BELOW
//!   R's height;
//! - the record side canonicalizes late, and one v2 re-mint creates the
//!   exact live probe moment: R is WITNESSED (the v1↔v2 clique closes over
//!   it) while nothing above R is — R is the top spine candidate.
//!
//! At that moment the old floor derivation designated R — a state missing
//! the settled effect — permanently erasing it while its deploy read
//! Finalized. The floor must never designate a state missing the settled
//! effect, and must still advance past R on capturing chains.
//!
//! Red pedigree (the neuter protocol, demonstrated during F-layer
//! development): with BOTH capture sites neutered — `derive_floor`'s
//! candidate arm to raw DAG ancestry and `floor_of_view`'s LFB guard to an
//! unconditional advance, the pre-F world — the r-witnessed probe
//! designates R and the settled-effect assertion fails. Neutering the
//! candidate arm ALONE stays green: `floor_of_view`'s guard independently
//! refuses the uncapturing floor ("does not capture the current LFB;
//! holding") — the two capture sites are genuine defense in depth.
//! Restored, the spec holds end to end.

use std::collections::HashMap;

use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::finality::floor::{floor_of_view, Floor};
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// v0=3, v1=3, v2=4 of S=10 at ftt 0.1 (witness needs q > 5.5): no single
/// validator witnesses anything alone — every finalization step requires a
/// mutual-visibility pair, giving the staging the witnessing LAG the live
/// erasure depended on — and v0+v1 form a clique without v2.
fn lagged_bonds(pks: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
    pks.into_iter()
        .enumerate()
        .map(|(i, pk)| (pk, if i == 2 { 4 } else { 3 }))
        .collect()
}

async fn lagged_three_node_network() -> (Vec<TestNode>, String, BlockMessage) {
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(lagged_bonds), Some(3));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let genesis_block = genesis.genesis_block.clone();
    let mut nodes = TestNode::create_network(genesis, 3, None, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    (nodes, shard_id, genesis_block)
}

fn rejected_sigs(block: &BlockMessage) -> Vec<Bytes> {
    block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| rd.sig.clone())
        .collect()
}

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

/// Mint a block on `node` from explicitly ORDERED parents (parents[0] is
/// the main parent — the spine the frontier walk follows), process it on
/// the same node, and return it. The snapshot's numbering is re-anchored to
/// the overridden parent set (`create` numbers from `max_block_num`, not
/// the parent list).
async fn mint_on_parents(
    node: &mut TestNode,
    parents: Vec<BlockMessage>,
    label: &str,
) -> BlockMessage {
    for p in &parents {
        assert!(
            node.casper.dag_contains(&p.block_hash),
            "staging[{label}]: parent {} must be IN THE DAG of the minting \
             node (buffered: {})",
            hex::encode(&p.block_hash[..6]),
            node.casper.buffer_contains(&p.block_hash),
        );
    }
    let mut snapshot = node.casper.get_snapshot().await.expect("snapshot");
    snapshot.max_block_num = parents
        .iter()
        .map(|p| p.body.state.block_number)
        .max()
        .expect("non-empty parent set");
    snapshot.parents = parents;
    let validator_identity = node.validator_id_opt.clone().expect("validator identity");
    let deploy_storage = node.deploy_storage.clone();
    let rejected_buffer = node.rejected_deploy_buffer.clone();
    let runtime_manager = node.runtime_manager.clone();
    let created = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        deploy_storage,
        rejected_buffer,
        &runtime_manager,
        &mut node.block_store,
        true,
    )
    .await
    .expect("create on ordered parents");
    let BlockCreatorResult::Created(block, _pre, _post) = created else {
        panic!("create must mint on the ordered parent set");
    };
    node.process_block(block.clone())
        .await
        .expect("self-process minted block");
    block
}

/// Assert the settled-effect invariant at a probe point: derive the floor
/// of the view on `node` and require the loser's effect in the FLOOR
/// BLOCK's own post-state. Returns the (possibly advanced) floor.
async fn probe_floor_state(
    node: &TestNode,
    current: &Floor,
    loser_cell: &str,
    label: &str,
) -> Floor {
    let dag = node.casper.block_dag().await.expect("dag");
    let derived = floor_of_view(&dag, current, FtThreshold::from_f32_lossy(0.1))
        .await
        .expect("floor_of_view")
        .unwrap_or_else(|| current.clone());
    tracing::info!(
        target: "repro",
        label,
        derived = %hex::encode(&derived.hash[..6]),
        derived_number = derived.block_number,
        "probe derived floor"
    );
    let floor_block = node
        .block_store
        .get(&derived.hash)
        .expect("floor block read")
        .expect("floor block present");
    let effect_live = !string_datums(node, &floor_block.body.state.post_state_hash, loser_cell)
        .await
        .is_empty();
    assert!(
        effect_live,
        "[{label}] ERASURE: floor {}#{} does not carry the settled effect \
         @\"{loser_cell}\" in its post-state",
        hex::encode(&derived.hash[..8]),
        derived.block_number,
    );
    derived
}

#[tokio::test]
async fn a_stale_based_rejecting_merge_never_becomes_the_floor_over_the_settled_carrier() {
    shared::rust::tracing_init::init_for_tests();
    let (mut nodes, shard_id, genesis_block) = lagged_three_node_network().await;

    // Seed the contended cell on v1; everyone sees it.
    let seed = construct_deploy::source_deploy_now_full(
        r#"@"race"!("s")"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("seed");
    let s_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&seed))
        .await
        .expect("seed block");
    for i in [0usize, 2usize] {
        nodes[i]
            .process_block(s_block.clone())
            .await
            .expect("process seed");
    }

    // Both conflicting contenders on v2, as SIBLING branches of S on one
    // justification chain (the live shape). The first sibling has clean
    // dependencies; the second cites the first.
    let contender_a = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") { @"race"!("a") | @"XA"!(v) }"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(0),
            Some(shard_id.clone()),
        )
        .expect("contender a")
    };
    let contender_b = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") { @"race"!("b") | @"XB"!(v) }"#.to_string(),
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
        .expect("contender b")
    };
    let c_a = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(&contender_a))
        .await
        .expect("contender-a carrier");
    // Sibling branch on the same parent S. Contender a leaves v2's pool
    // first — deploys are retained until terminal, and c_a is outside the
    // sibling's scope, so selection would otherwise double-include it.
    let c_b = {
        nodes[2]
            .deploy_storage
            .lock()
            .remove_by_sig(&contender_a.sig)
            .expect("purge contender a from the pool");
        let mut snapshot = nodes[2].casper.get_snapshot().await.expect("snapshot");
        snapshot.max_block_num = s_block.body.state.block_number;
        snapshot.parents = vec![s_block.clone()];
        nodes[2]
            .deploy_storage
            .lock()
            .add(vec![contender_b.clone()])
            .expect("stage contender b");
        let validator_identity = nodes[2]
            .validator_id_opt
            .clone()
            .expect("validator identity");
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
        .expect("create sibling contender branch");
        let BlockCreatorResult::Created(block, _pre, _post) = created else {
            panic!("create must mint the sibling contender branch");
        };
        nodes[2]
            .process_block(block.clone())
            .await
            .expect("process sibling branch");
        block
    };

    // v2's spine grows over its STALE view (its 4-of-10 stake witnesses
    // nothing, so its floor stays in the genesis era) — the height ladder
    // that will put R ABOVE the eventual settled floor.
    let mut v2_spine: Vec<BlockMessage> = Vec::new();
    let mut v2_tip = c_b.clone();
    for _ in 0..3 {
        v2_tip = mint_on_parents(&mut nodes[2], vec![v2_tip.clone()], "v2-spine").await;
        v2_spine.push(v2_tip.clone());
    }

    // R: the stale-based rejecting merge at the top of v2's spine,
    // adjudicating the contest against the pre-settlement base.
    let r_block = mint_on_parents(&mut nodes[2], vec![v2_tip.clone(), c_a.clone()], "R").await;
    let r_rejected = rejected_sigs(&r_block);
    let a_lost = r_rejected.contains(&contender_a.sig);
    let b_lost = r_rejected.contains(&contender_b.sig);
    assert!(
        a_lost ^ b_lost,
        "R must reject exactly one contender (rejected: {})",
        r_rejected.len()
    );
    let (carrier, carrier_deps, loser_cell): (BlockMessage, Vec<BlockMessage>, &str) = if a_lost {
        (c_a.clone(), vec![], "XA")
    } else {
        // The rejected carrier is the second sibling; its justifications
        // cite the first, which must travel with it.
        (c_b.clone(), vec![c_a.clone()], "XB")
    };
    tracing::info!(
        target: "repro",
        s = %hex::encode(&s_block.block_hash[..6]),
        c_a = %hex::encode(&c_a.block_hash[..6]),
        c_b = %hex::encode(&c_b.block_hash[..6]),
        r = %hex::encode(&r_block.block_hash[..6]),
        r_number = r_block.body.state.block_number,
        carrier = %hex::encode(&carrier.block_hash[..6]),
        loser_cell,
        "staged geometry"
    );
    assert!(
        string_datums(&nodes[2], &r_block.body.state.post_state_hash, loser_cell)
            .await
            .is_empty(),
        "R's state must NOT carry the rejected effect — it is the eraser"
    );

    // SETTLE the rejected carrier BELOW R's height: the settlers v0+v1
    // (who never see R) witness it through a mutual-visibility chain.
    // Minting is parent-FORCED so the unsettled sibling never contaminates
    // their chains with a re-adjudication.
    for dep in carrier_deps.iter().chain(std::iter::once(&carrier)) {
        for i in [0usize, 1usize] {
            if !nodes[i].contains(&dep.block_hash) {
                nodes[i]
                    .process_block(dep.clone())
                    .await
                    .expect("deliver the rejected carrier (and deps)");
            }
        }
    }
    let p1 = mint_on_parents(&mut nodes[0], vec![carrier.clone()], "p1").await;
    nodes[1].process_block(p1.clone()).await.expect("p1 to v1");
    let p2 = mint_on_parents(&mut nodes[1], vec![p1.clone()], "p2").await;
    nodes[0].process_block(p2.clone()).await.expect("p2 to v0");
    // p3 goes to v2 ONLY — v1's view stops at p2, keeping a live fork for
    // the erasure-arrangement merge below.
    let p3 = mint_on_parents(&mut nodes[0], vec![p2.clone()], "p3").await;
    for b in [&p1, &p2, &p3] {
        nodes[2]
            .process_block((*b).clone())
            .await
            .expect("settling chain to v2");
    }

    let genesis_floor = Floor {
        hash: genesis_block.block_hash.clone(),
        block_number: genesis_block.body.state.block_number,
    };
    let settled = probe_floor_state(&nodes[1], &genesis_floor, loser_cell, "settlement").await;
    assert!(
        settled.block_number >= carrier.body.state.block_number
            && settled.block_number < r_block.body.state.block_number,
        "staging precondition: the settled floor must cover the carrier and \
         sit BELOW the eraser (floor #{}, carrier #{}, R #{})",
        settled.block_number,
        carrier.body.state.block_number,
        r_block.body.state.block_number,
    );

    // THE ERASURE ARRANGEMENT: the record side canonicalizes late. v1
    // mints on R (main parent), then v2 re-mints citing v1's block —
    // closing the v1↔v2 clique over R while NOTHING above R is witnessed:
    // R is now the top spine candidate, the exact live moment.
    let mut record_side: Vec<BlockMessage> = vec![c_a.clone(), c_b.clone()];
    record_side.extend(v2_spine.iter().cloned());
    record_side.push(r_block.clone());
    for b in &record_side {
        for i in [0usize, 1usize] {
            if !nodes[i].contains(&b.block_hash) {
                nodes[i]
                    .process_block(b.clone())
                    .await
                    .expect("record side delivered late");
            }
        }
    }
    let y1 = mint_on_parents(&mut nodes[1], vec![r_block.clone(), p2.clone()], "y1").await;
    nodes[0].process_block(y1.clone()).await.expect("y1 to v0");
    nodes[2].process_block(y1.clone()).await.expect("y1 to v2");
    // y_v2 is a genuine MERGE (y1's branch and the withheld p3): its
    // derivation must choose a base, with R witnessed in v2's view as the
    // top spine candidate — the live erasing selection, now forced to
    // decide a real state.
    let y_v2 = mint_on_parents(&mut nodes[2], vec![y1.clone(), p3.clone()], "y_v2").await;
    for i in [0usize, 1usize] {
        nodes[i]
            .process_block(y_v2.clone())
            .await
            .expect("y_v2 delivered");
    }

    // The strongest deterministic observable: the late-canonicalizing
    // proposer's merge must have based on the settled floor, not the
    // eraser — its state carries the settled effect. Under the old
    // predicate v2 derived R as this merge's base and the state missed it.
    assert!(
        !string_datums(&nodes[2], &y_v2.body.state.post_state_hash, loser_cell)
            .await
            .is_empty(),
        "ERASURE: y_v2 based on the rejecting merge — its state is missing \
         the settled effect @\"{loser_cell}\""
    );

    // And the read surface, on the eraser-owning view (live: v2's).
    let held = probe_floor_state(&nodes[2], &settled, loser_cell, "r-witnessed").await;
    assert_ne!(
        held.hash, r_block.block_hash,
        "the stale-based rejecting merge must never be designated the floor"
    );

    // Flush the full staged topology to every node (the p3 withholding has
    // served its purpose; buffered blocks promote once their dependencies
    // arrive), then run the liveness rounds.
    let mut topology: Vec<BlockMessage> = vec![c_a.clone(), c_b.clone()];
    topology.extend(v2_spine.iter().cloned());
    topology.extend([
        r_block.clone(),
        p1.clone(),
        p2.clone(),
        p3.clone(),
        y1.clone(),
        y_v2.clone(),
    ]);
    for node in nodes.iter_mut() {
        for b in &topology {
            if !node.casper.dag_contains(&b.block_hash) {
                node.process_block(b.clone()).await.expect("topology flush");
            }
        }
    }

    // Liveness: further mutual rounds witness the capturing post-settlement
    // merges (bases at the settled floor); the floor advances past the
    // eraser's height with the effect intact.
    let mut current = held;
    let mut tip = y_v2;
    for round in 0..4i32 {
        let minter = (round % 2) as usize;
        let next = mint_on_parents(&mut nodes[minter], vec![tip.clone()], "liveness").await;
        for (i, node) in nodes.iter_mut().enumerate() {
            if i != minter {
                node.process_block(next.clone())
                    .await
                    .expect("liveness round delivery");
            }
        }
        tip = next;
        current = probe_floor_state(&nodes[2], &current, loser_cell, "liveness").await;
    }
    assert!(
        current.block_number > r_block.body.state.block_number,
        "the floor must advance past the eraser's height on capturing \
         chains (floor #{}, R #{})",
        current.block_number,
        r_block.body.state.block_number,
    );
}
