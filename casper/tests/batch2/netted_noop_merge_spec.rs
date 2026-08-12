// A continuation installed on one chain and COMM-fired by a dependent
// chain, both inside ONE merge scope, net to an empty channel change
// (`ChannelChange::cancel_common`). The netted no-op is a legitimate
// merge outcome — the consume pointer ends exactly at base — and the
// merge must form. The pre-fix merger raised `Merging logic error:
// empty consume change when computing trie action` instead, failing the
// whole propose: every validator whose parent view spanned the pair was
// silenced simultaneously (the error is a pure function of the view).
// The shape needs no conflict and no rejection — clean dependent
// execution — so no recovery narrowing exists to route around it.
//
// Choreography (three validators, stakes {1,3,5}; the 5/9-stake v2 stays
// SILENT so v0+v1's 4/9 can never witness anything and the floor stays
// pinned at genesis — the install chain must remain ABOVE the merge base
// for the pair to net; a floor that reaches W settles the install into
// the base and defuses the shape, which is exactly why the defect fires
// under floor lag):
//
//   genesis <- W (v0: `for (@x <- @"nn_cell") { @"nn_out"!(x) }` —
//                installs the waiting continuation, no datum yet)
//   W <- P       (v1: `@"nn_cell"!(42)` — the produce COMM-fires the
//                waiting continuation; @"nn_out" receives 42)
//   W <- F       (v0: unrelated marker, sibling of P)
//   [P, F] <- M  (v0 merges over the genesis cut: scope = {W, P, F}; W's
//                install chain and P's fire chain are dependency-grouped
//                into one branch; their cont changes net to empty.
//                THE PIN: M must form and validate.)

use casper::rust::casper::MultiParentCasper;
use casper::rust::util::construct_deploy;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn gstring_channel(name: &str) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(name.to_string())),
        }],
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn netted_install_fire_pair_merges_as_a_noop() {
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

    // W: v0 installs the waiting continuation (no datum on the cell yet).
    let install = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@x <- @"nn_cell") { @"nn_out"!(x) }"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(0),
            Some(shard_id.clone()),
        )
        .expect("build install deploy")
    };
    let block_w = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&install))
        .await
        .expect("v0 proposes W (install)");
    nodes[1]
        .process_block(block_w.clone())
        .await
        .expect("v1 processes W");

    // P: v1 produces into the cell — the COMM fires W's continuation.
    let fire = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"@"nn_cell"!(42)"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(0),
            Some(shard_id.clone()),
        )
        .expect("build fire deploy")
    };
    let block_p = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&fire))
        .await
        .expect("v1 proposes P (fire)");

    // F: v0's unrelated sibling of P, so the next proposal is a real merge.
    let marker = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            7,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(shard_id.clone()),
        )
        .expect("build marker")
    };
    let block_f = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&marker))
        .await
        .expect("v0 proposes F (marker sibling)");
    nodes[1]
        .process_block(block_f.clone())
        .await
        .expect("v1 processes F");

    nodes[0]
        .process_block(block_p.clone())
        .await
        .expect("v0 processes P");

    // THE PIN: the merge over [P, F] spans W's install chain and P's fire
    // chain; their netted cont change must merge as a no-op. Pre-fix this
    // propose died with `empty consume change when computing trie action`.
    let block_m = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the netted install+fire pair must merge, not error the propose");
    // Genesis may ride along as an extra parent: the silent v2's latest
    // message is genesis, and covered-ancestor parents are legal.
    assert!(
        block_m
            .header
            .parents_hash_list
            .contains(&block_p.block_hash)
            && block_m
                .header
                .parents_hash_list
                .contains(&block_f.block_hash),
        "fixture precondition: M must merge P and F; parents={:?}",
        block_m
            .header
            .parents_hash_list
            .iter()
            .map(|h| hex::encode(&h[..4.min(h.len())]))
            .collect::<Vec<_>>()
    );
    assert!(
        block_m.body.rejected_deploys.is_empty(),
        "clean dependent execution: nothing may be rejected (got {:?})",
        block_m
            .body
            .rejected_deploys
            .iter()
            .map(|rd| hex::encode(&rd.sig[..8.min(rd.sig.len())]))
            .collect::<Vec<_>>()
    );

    let own_outcome = nodes[0]
        .process_block(block_m.clone())
        .await
        .expect("v0 processes its own merge");
    assert!(
        matches!(own_outcome, Either::Right(_)),
        "v0 must admit its own netted-noop merge; got {:?}",
        own_outcome
    );
    let outcome = nodes[1]
        .process_block(block_m.clone())
        .await
        .expect("v1 processes M");
    assert!(
        matches!(outcome, Either::Right(_)),
        "v1 must validate the netted-noop merge identically; got {:?}",
        outcome
    );

    // Fixture precondition: the merge ran over the GENESIS cut — the
    // install chain was still above the base, so the netting genuinely
    // occurred (a floor that reaches W settles the install into the base
    // and this spec stops testing anything).
    {
        let dag = nodes[0].casper.block_dag().await.expect("dag");
        let ftt = casper::rust::safety::clique_oracle::FtThreshold::from_ppm(
            nodes[0]
                .casper
                .casper_shard_conf()
                .fault_tolerance_threshold_ppm,
        );
        let floor = casper::rust::finality::floor::floor_of_block(&dag, &block_m.block_hash, ftt)
            .await
            .expect("floor_of_block(M)");
        assert_eq!(
            floor.block_number, 0,
            "fixture precondition: M's floor must still be genesis so the \
             install chain is in merge scope; got floor #{}",
            floor.block_number
        );
    }

    // The COMM's outcome lands exactly once and the cell ends empty: the
    // fired continuation must be neither re-inserted nor double-applied.
    let out_data = nodes[0]
        .runtime_manager
        .get_data(
            block_m.body.state.post_state_hash.clone(),
            &gstring_channel("nn_out"),
        )
        .await
        .expect("read @\"nn_out\"");
    assert_eq!(
        out_data.len(),
        1,
        "the fired continuation's output must land exactly once"
    );
    let cell_data = nodes[0]
        .runtime_manager
        .get_data(
            block_m.body.state.post_state_hash.clone(),
            &gstring_channel("nn_cell"),
        )
        .await
        .expect("read @\"nn_cell\"");
    assert!(
        cell_data.is_empty(),
        "the consumed cell must end empty in the merged state; datums: {}",
        cell_data.len()
    );
}
