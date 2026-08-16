//! Direct reproduction of the ucc finalization stall (session 3cd723b6),
//! end-to-end through the real pipeline. The live geometry, faithfully:
//!
//! - a contender's effect settles in a block that certifies as the
//!   finalized floor (the live `435239f1` inside `dbba9639…#42`);
//! - a STALE-BASED merge minted concurrently — its own derived floor
//!   still below the settling block — legally rejects that contender's
//!   chain (the live `c31a44c2…#46`, derived floor `#37`, tripwire
//!   rightly silent: the chain was not yet settled in ITS view), with the
//!   settling block on its SPINE but not on its state lineage;
//! - single-parent blocks then extend the stale merge (the live v3 solo
//!   run #50–55). Each takes its parent's post-state VERBATIM — no floor
//!   derivation, no re-base — so the settled effect can never re-enter:
//!   a single-parent block collects no scope. Every such block, once
//!   witnessed, fails containment against the floor (missing the settled
//!   sig), the floor freezes, unfinalized width piles up, and the
//!   backpressure cap turns the stall into a permanent halt.
//!
//! The stall is now prevented at its SOURCE rather than repaired
//! downstream. Step two above is what starts it, and it is the same move
//! that froze `bc35a3ad`: a merge adjudicating away content carried by its
//! own MAIN PARENT. Conflict resolution pins the main parent's chains
//! (`conflict_resolution_never_rejects_main_parent_content`), so the
//! stale-based merge keeps the settling block's effect and no descendant
//! ever inherits a state its own spine ancestor contradicts.
//!
//! This spec therefore now asserts the closure end-to-end through the real
//! pipeline: the merge keeps the content, every extension of it carries
//! the floor-settled effect, and the floor advances. The single-parent
//! re-base onto the floor stays implemented as the general repair for a
//! floor that a parent's lineage genuinely never held — it is simply no
//! longer reachable by this route, so the assertions below pin the
//! invariant (the effect is present) rather than the repair (a recorded
//! `merge_base`).

use std::collections::HashMap;

use casper::rust::casper::{Casper, MultiParentCasper};
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
async fn a_stale_based_merge_keeps_its_main_parents_settled_content() {
    shared::rust::tracing_init::init_for_tests();
    let (mut nodes, shard_id, _genesis_block) = equal_three_node_network().await;

    // Seed the contended cell on v3; everyone sees it.
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

    // Two conflicting contenders over the same frontier. The CHEAP chain is
    // the one a cost-optimal rejection drops, so the loser is deterministic:
    // contender_x (v1, cheap) loses; contender_y (v2, deliberately heavy)
    // wins.
    let contender_x = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") { @"race"!("a") | @"XA"!(v) }"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(0),
            Some(shard_id.clone()),
        )
        .expect("contender x")
    };
    let contender_y = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") {
                 @"race"!("b") | @"XB"!(v) |
                 @"pad1"!(1) | @"pad2"!(2) | @"pad3"!(3) | @"pad4"!(4) |
                 @"pad5"!(5) | @"pad6"!(6) | @"pad7"!(7) | @"pad8"!(8)
               }"#
            .to_string(),
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
        .expect("contender y")
    };
    let c = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&contender_x))
        .await
        .expect("carrier C");
    let e = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&contender_y))
        .await
        .expect("carrier E");
    for b in [&c, &e] {
        nodes[2]
            .process_block((*b).clone())
            .await
            .expect("carriers to v3");
    }
    nodes[0].process_block(e.clone()).await.expect("E to v1");
    nodes[1].process_block(c.clone()).await.expect("C to v2");

    // M: the stale-based merge — v3's derived floor is still genesis-era,
    // so rejecting the cheap chain is LEGAL in its view (the live #46).
    // C is M's MAIN parent: the settling block sits on M's spine while
    // M's state (based below C) drops its content.
    let m = mint_on_parents(&mut nodes[2], vec![c.clone(), e.clone()], "M").await;
    let m_rejected = rejected_sigs(&m);
    assert!(
        !m_rejected.contains(&contender_x.sig),
        "M rejected the chain carried by its OWN MAIN PARENT C. Cost-optimal \
         selection prefers the cheap chain and X is cheaper, so only the \
         main-parent pin stops this — and this is where the stall starts: M's \
         state drops content that sits on M's spine, so every descendant \
         inherits a state its own spine ancestor contradicts (rejected {:?})",
        m_rejected
            .iter()
            .map(|s| hex::encode(&s[..6]))
            .collect::<Vec<_>>(),
    );
    assert!(
        !string_datums(&nodes[2], &m.body.state.post_state_hash, "XA")
            .await
            .is_empty(),
        "M's state must carry its main parent's settled effect @\"XA\" — the \
         rejection that used to erase it is what this spec reproduced"
    );

    // Close the witnessing clique over C: v1 cites M (so its next block's
    // justifications carry v3's chain), and its extension A rides C's
    // spine. Over {v1:A, v3:M} both spines pass C and the mutual
    // justification bands close — C certifies in v3's next derivation.
    nodes[0].process_block(m.clone()).await.expect("M to v1");
    let a = mint_on_parents(&mut nodes[0], vec![c.clone()], "A").await;
    nodes[2].process_block(a.clone()).await.expect("A to v3");

    // T: a single-parent extension of M, minted in a view that now
    // witnesses C. T's derived floor is therefore C — a floor its
    // parent's STATE lineage never held. T must re-base onto C: record
    // the base and carry C's settled content. The verbatim fast path
    // instead inherits M's state, and every witnessed descendant is then
    // refused by containment forever (the 3cd723b6 freeze).
    let t = mint_on_parents(&mut nodes[2], vec![m.clone()], "T").await;
    {
        let dag = nodes[2].casper.block_dag().await.expect("dag");
        let frozen = floor_of_block(
            &dag,
            &nodes[2].block_store,
            &t.block_hash,
            FtThreshold::from_f32_lossy(0.1),
        )
        .await
        .expect("floor_of_block(T)");
        assert_eq!(
            frozen.hash,
            c.block_hash,
            "staging: T's derived floor must be the settled carrier C \
             (got {}#{})",
            hex::encode(&frozen.hash[..8]),
            frozen.block_number,
        );
    }
    // What must hold is the PROPERTY — T's state carries the floor's settled
    // content — not the mechanism that delivers it. Re-basing onto the floor
    // was the only route while a merge could drop its main parent's chain,
    // because M's state then genuinely lacked C's effect. Now that M keeps it,
    // T inherits a state that already contains the floor's content and records
    // no base of its own. Asserting `merge_base == C` here would pin the
    // repair rather than the invariant, and would fail precisely because the
    // damage it repaired no longer happens.
    assert!(
        !string_datums(&nodes[2], &t.body.state.post_state_hash, "XA")
            .await
            .is_empty(),
        "T's post-state must carry the floor-settled effect @\"XA\" — a block \
         whose state omits its own derived floor's content is refused by \
         containment forever, which is the 3cd723b6 freeze"
    );

    // Liveness: deliver the branch everywhere and run mutual rounds — the
    // floor must advance past T's height with the settled effect intact on
    // every node.
    for b in [&m, &a, &t] {
        for i in [0usize, 1usize] {
            if !nodes[i].casper.dag_contains(&b.block_hash) {
                nodes[i]
                    .process_block((*b).clone())
                    .await
                    .expect("stale branch delivered");
            }
        }
    }
    let thr = FtThreshold::from_f32_lossy(0.1);
    let mut floors: Vec<Floor> = {
        let mut out = Vec::new();
        for node in &nodes {
            let dag = node.casper.block_dag().await.expect("dag");
            out.push(
                floor_of_block(&dag, &node.block_store, &seed_block.block_hash, thr)
                    .await
                    .expect("seed floor"),
            );
        }
        out
    };
    let mut tip = t.clone();
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
                .expect("floor_of_view must not error")
            {
                floors[i] = advanced;
            }
        }
        if floors
            .iter()
            .all(|f| f.block_number > t.body.state.block_number)
        {
            break;
        }
    }
    for (i, floor) in floors.iter().enumerate() {
        assert!(
            floor.block_number > t.body.state.block_number,
            "node {i}: the floor must advance past the re-based block \
             (floor #{}, T #{})",
            floor.block_number,
            t.body.state.block_number,
        );
        let node = &nodes[i];
        let floor_block = node
            .block_store
            .get(&floor.hash)
            .expect("floor block read")
            .expect("floor block present");
        assert!(
            !string_datums(node, &floor_block.body.state.post_state_hash, "XA")
                .await
                .is_empty(),
            "node {i}: the settled effect must be present at every advanced floor"
        );
    }
}
