//! Direct reproduction of the ucc finality fork (session 00e6a2e3),
//! end-to-end through the real pipeline. The live geometry, faithfully:
//!
//! - two sibling blocks over the SAME frontier, each carrying one of two
//!   conflicting contender deploys (the live `7cfec55673`/`2c479c6271` at
//!   #52, each carrying one round-1 RMW add);
//! - each owner extends its private sibling before sibling delivery;
//! - a third validator receives both siblings and merges them in block Y;
//! - each owner receives Y, merges Y into its private branch, and adds one
//!   final witness block while the opposite branch remains withheld;
//! - all blocks then cross-deliver, and a join uses both maximal branch tips.
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
use rspace_plus_plus::rspace::history::Either;

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
        .map(|rd| Bytes::copy_from_slice(rd.deploy_id()))
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

async fn deliver_valid(node: &mut TestNode, block: &BlockMessage, label: &str) {
    let status = node
        .process_block(block.clone())
        .await
        .unwrap_or_else(|error| panic!("deliver[{label}] failed: {error}"));
    assert!(
        matches!(status, Either::Right(_)),
        "deliver[{label}] rejected a protocol-valid block: {status:?}"
    );
}

use super::staging::{mint_on_expected_snapshot, ExpectedParents};

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
        deliver_valid(&mut nodes[i], &seed_block, "seed").await;
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

    let a1 = mint_on_expected_snapshot(&mut nodes[0], ExpectedParents::ordered(&[&s1]), "A1").await;
    let a2 = mint_on_expected_snapshot(&mut nodes[1], ExpectedParents::ordered(&[&s2]), "A2").await;

    for b in [&s1, &s2] {
        deliver_valid(&mut nodes[2], b, "siblings to v3").await;
    }

    let y =
        mint_on_expected_snapshot(&mut nodes[2], ExpectedParents::members(&[&s1, &s2]), "Y").await;
    let y_rejected = rejected_sigs(&y);
    let contender_1_id = Bytes::copy_from_slice(s1.body.deploys[0].deploy_id());
    let contender_2_id = Bytes::copy_from_slice(s2.body.deploys[0].deploy_id());
    let one_lost = y_rejected.contains(&contender_1_id) ^ y_rejected.contains(&contender_2_id);
    assert!(
        one_lost,
        "staging: Y's merge must reject exactly one contender chain \
         (rejected {})",
        y_rejected.len()
    );
    deliver_valid(&mut nodes[0], &s2, "S2 to v1").await;
    deliver_valid(&mut nodes[0], &y, "Y to v1").await;
    deliver_valid(&mut nodes[1], &s1, "S1 to v2").await;
    deliver_valid(&mut nodes[1], &y, "Y to v2").await;

    let b1 =
        mint_on_expected_snapshot(&mut nodes[0], ExpectedParents::members(&[&a1, &y]), "B1").await;
    let b2 =
        mint_on_expected_snapshot(&mut nodes[1], ExpectedParents::members(&[&a2, &y]), "B2").await;
    let c1 = mint_on_expected_snapshot(&mut nodes[0], ExpectedParents::ordered(&[&b1]), "C1").await;
    let c2 = mint_on_expected_snapshot(&mut nodes[1], ExpectedParents::ordered(&[&b2]), "C2").await;

    for block in [&a1, &b1, &c1] {
        deliver_valid(&mut nodes[1], block, "branch 1 to v2").await;
        deliver_valid(&mut nodes[2], block, "branch 1 to v3").await;
    }
    for block in [&a2, &b2, &c2] {
        deliver_valid(&mut nodes[0], block, "branch 2 to v1").await;
        deliver_valid(&mut nodes[2], block, "branch 2 to v3").await;
    }

    // Diagnostic: record what each branch froze (the live run froze the two
    // siblings divergently: 65 derivations on one, 21 on the other).
    {
        let dag = nodes[2].casper.block_dag().await.expect("dag");
        let thr = FtThreshold::from_f32_lossy(0.1);
        for (label, hash) in [("C1", &c1.block_hash), ("C2", &c2.block_hash)] {
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

    let join =
        mint_on_expected_snapshot(&mut nodes[2], ExpectedParents::members(&[&c1, &c2]), "JOIN")
            .await;
    for i in [0usize, 1usize] {
        deliver_valid(&mut nodes[i], &join, "join").await;
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
        let next = mint_on_expected_snapshot(
            &mut nodes[minter],
            ExpectedParents::ordered(&[&tip]),
            "liveness",
        )
        .await;
        for (i, node) in nodes.iter_mut().enumerate() {
            if i != minter {
                deliver_valid(node, &next, "liveness round delivery").await;
            }
        }
        tip = next;
        for (i, node) in nodes.iter().enumerate() {
            let dag = node.casper.block_dag().await.expect("dag");
            if let Some(advanced) = floor_of_view(&dag, &node.block_store, &floors[i], thr)
                .await
                .expect("floor_of_view must never error on an adjudicated view")
                .advanced()
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
