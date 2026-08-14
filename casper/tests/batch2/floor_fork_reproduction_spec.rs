//! Direct reproduction of the ucc finality fork (session 00e6a2e3),
//! end-to-end through the real pipeline. The live geometry, faithfully:
//!
//! - two sibling blocks over the SAME frontier, each carrying one of two
//!   conflicting contender deploys (the live `7cfec55673`/`2c479c6271` at
//!   #52, each carrying one round-1 RMW add);
//! - a third validator MERGES both siblings (the live `ec591540df` — its
//!   merge adjudicates the contest and rejects one chain with a record);
//! - each contender's owner extends its OWN sibling's branch, citing the
//!   merger — so each branch derivation sees its sibling backed by two of
//!   three validators and freezes it as the finalized floor (the live
//!   divergent freeze: 65 derivations landed on one sibling, 21 on the
//!   other, each a legitimate in-view advancement);
//! - a join block then merges the two branches. Its parents descend BOTH
//!   siblings, and each inherited floor owes the other a contender sig it
//!   does not contain — neither containment nor a pure-cut re-merge holds.
//!
//! Live outcome: every propose and the LFB derivation errored
//! ("incompatible finalized fork"), block production halted at the join on
//! all five nodes, and the shard froze permanently. A witnessed sibling
//! fork must instead be ADJUDICATED — certification is exclusive per
//! height (a validator's chain backs one sibling), fork choice converges
//! on one branch, the winner's floor advances, and the loser's content is
//! re-adjudicated by records and recovery. The assertions below state that
//! behavior; on the certification-plural oracle they fail at the join.

use std::collections::HashMap;

use casper::rust::casper::MultiParentCasper;
use casper::rust::finality::floor::{floor_of_block, floor_of_view, Floor};
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// Equal stakes, mirroring the live shard (100/100/100 at ftt 0.1): no
/// validator witnesses anything alone, any two form a witnessing clique —
/// the symmetry that let BOTH siblings certify.
fn equal_bonds(pks: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
    pks.into_iter().map(|pk| (pk, 100)).collect()
}

async fn equal_three_node_network() -> (Vec<TestNode>, String, BlockMessage) {
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(equal_bonds), Some(3));
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

use super::staging::mint_on_parents;

#[tokio::test]
async fn a_co_witnessed_sibling_fork_must_adjudicate_and_advance() {
    shared::rust::tracing_init::init_for_tests();
    let (mut nodes, shard_id, genesis_block) = equal_three_node_network().await;

    // Seed the contended cell on v3; everyone sees it. The contenders are
    // single-consume RMW ops on the seed — the ucc map-cell shape: only one
    // applies against the seed; the other conflicts and can re-land later
    // against the winner's value.
    let seed = construct_deploy::source_deploy_now_full(
        r#"@"race"!("s")"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("seed");
    let seed_block = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(&seed))
        .await
        .expect("seed block");
    for i in [0usize, 1usize] {
        nodes[i]
            .process_block(seed_block.clone())
            .await
            .expect("process seed");
    }

    let contender_1 = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") { @"race"!("a") | @"XA"!(v) }"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(0),
            Some(shard_id.clone()),
        )
        .expect("contender 1")
    };
    let contender_2 = {
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
        .expect("contender 2")
    };

    // The same-height siblings over the SAME frontier: each owner mints its
    // contender before seeing the other's block (the live concurrent mint).
    let s1 = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&contender_1))
        .await
        .expect("sibling S1");
    let s2 = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&contender_2))
        .await
        .expect("sibling S2");
    assert_eq!(
        s1.header.parents_hash_list, s2.header.parents_hash_list,
        "staging: the siblings must be minted over the SAME frontier"
    );
    nodes[0].process_block(s2.clone()).await.expect("S2 to v1");
    nodes[1].process_block(s1.clone()).await.expect("S1 to v2");
    for b in [&s1, &s2] {
        nodes[2]
            .process_block((*b).clone())
            .await
            .expect("siblings to v3");
    }

    // Y: the third validator merges both siblings (the live ec591540df) —
    // its merge adjudicates the contest, rejecting exactly one chain.
    let y = mint_on_parents(&mut nodes[2], vec![s1.clone(), s2.clone()], "Y").await;
    let y_rejected = rejected_sigs(&y);
    let one_lost = y_rejected.contains(&contender_1.sig) ^ y_rejected.contains(&contender_2.sig);
    assert!(
        one_lost,
        "staging: Y's merge must reject exactly one contender chain \
         (rejected {})",
        y_rejected.len()
    );
    for i in [0usize, 1usize] {
        nodes[i]
            .process_block(y.clone())
            .await
            .expect("Y delivered");
    }

    // Branch extensions, in DISJOINT views (the live ingredient): each
    // owner extends its OWN sibling citing Y, then immediately extends
    // again — the second block's derivation runs over a snapshot whose
    // latest messages already cite Y, closing the owner+v3 witnessing
    // clique over the owner's sibling ONLY (the other owner's chain is
    // still seed-era in this view, so the rival sibling has no clique).
    // B1/B2 also descend BOTH siblings (via Y) — the live #53/#54 shape,
    // where every parent of the join carries a fork-height floor and
    // descends both branches. Nothing crosses between v1 and v2 until
    // both branch floors are frozen.
    let a1 = mint_on_parents(&mut nodes[0], vec![s1.clone()], "A1").await;
    let b1 = mint_on_parents(&mut nodes[0], vec![a1.clone(), y.clone()], "B1").await;
    let a2 = mint_on_parents(&mut nodes[1], vec![s2.clone()], "A2").await;
    let b2 = mint_on_parents(&mut nodes[1], vec![a2.clone(), y.clone()], "B2").await;
    for (blocks, skip) in [([&a1, &b1], 0usize), ([&a2, &b2], 1usize)] {
        for b in blocks {
            for (i, node) in nodes.iter_mut().enumerate() {
                if i != skip {
                    node.process_block((*b).clone())
                        .await
                        .expect("branch extension delivered");
                }
            }
        }
    }

    // Diagnostic: record what each branch froze (the live run froze the two
    // siblings divergently: 65 derivations on one, 21 on the other).
    {
        let dag = nodes[2].casper.block_dag().await.expect("dag");
        let thr = FtThreshold::from_f32_lossy(0.1);
        for (label, hash) in [("B1", &b1.block_hash), ("B2", &b2.block_hash)] {
            let frozen = floor_of_block(&dag, &nodes[2].block_store, hash, thr)
                .await
                .expect("floor_of_block");
            tracing::info!(
                target: "repro",
                label,
                floor = %hex::encode(&frozen.hash[..6]),
                floor_number = frozen.block_number,
                "branch frozen floor"
            );
        }
    }

    // THE JOIN — the live halt moment. On the certification-plural oracle
    // both siblings are inherited fork-height floors here, each owing the
    // other a contender sig, and create errors "incompatible finalized
    // fork" (mint_on_parents panics with it). The shard must instead
    // adjudicate: one sibling's certification wins, the join mints, and
    // every node accepts it.
    let join = mint_on_parents(&mut nodes[2], vec![b1.clone(), b2.clone()], "JOIN").await;
    for i in [0usize, 1usize] {
        nodes[i]
            .process_block(join.clone())
            .await
            .expect("the adjudicated join must validate on every node");
    }

    // Liveness: mutual rounds must advance the floor PAST the sibling
    // height on every node — the live shard instead froze at the fork
    // height forever (floor 52, tip 58, every propose failing).
    let sibling_height = s1.body.state.block_number;
    let genesis_floor = Floor {
        hash: genesis_block.block_hash.clone(),
        block_number: genesis_block.body.state.block_number,
    };
    let thr = FtThreshold::from_f32_lossy(0.1);
    let mut floors: Vec<Floor> = vec![genesis_floor.clone(), genesis_floor.clone(), genesis_floor];
    let mut tip = join.clone();
    for round in 0..8i32 {
        let minter = (round % 3) as usize;
        let next = mint_on_parents(&mut nodes[minter], vec![tip.clone()], "liveness").await;
        for (i, node) in nodes.iter_mut().enumerate() {
            if i != minter {
                node.process_block(next.clone())
                    .await
                    .expect("liveness round delivery");
            }
        }
        tip = next;
        for (i, node) in nodes.iter().enumerate() {
            let dag = node.casper.block_dag().await.expect("dag");
            if let Some(advanced) = floor_of_view(&dag, &node.block_store, &floors[i], thr)
                .await
                .expect("floor_of_view must never error on an adjudicated view")
            {
                floors[i] = advanced;
            }
        }
        if floors.iter().all(|f| f.block_number > sibling_height) {
            break;
        }
    }
    for (i, floor) in floors.iter().enumerate() {
        assert!(
            floor.block_number > sibling_height,
            "node {i}: the floor must advance past the sibling fork height \
             (floor #{}, siblings #{sibling_height}) — a frozen floor is the \
             live 00e6a2e3 halt",
            floor.block_number,
        );
    }

    // The settled adjudication must be UNIFORM: every node's floor state
    // carries the same winner of the seed consume, and the winner's effect
    // is present (exactly one of XA/XB holds the seed value "s").
    let mut winners: Vec<String> = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        let floor_block = node
            .block_store
            .get(&floors[i].hash)
            .expect("floor block read")
            .expect("floor block present");
        let state = &floor_block.body.state.post_state_hash;
        let xa = string_datums(node, state, "XA").await;
        let xb = string_datums(node, state, "XB").await;
        let seed_winner = match (xa.contains(&"s".to_string()), xb.contains(&"s".to_string())) {
            (true, false) => "contender_1",
            (false, true) => "contender_2",
            (a, b) => panic!(
                "node {i}: exactly one contender must have consumed the seed \
                 at the settled floor (XA={a:?}/{xa:?}, XB={b:?}/{xb:?})"
            ),
        };
        winners.push(seed_winner.to_string());
    }
    assert!(
        winners.windows(2).all(|w| w[0] == w[1]),
        "the settled winner must be identical on every node: {winners:?}"
    );
}
