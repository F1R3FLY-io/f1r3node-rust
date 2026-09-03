// A proposer's own unmerged work is MERGED BACK, never orphaned.
//
// Recovery-context parent selection used to collapse the frontier to the
// single deploy-heavier tip. The candidate set IS the latest-message
// frontier, and merging it is what makes a branch unorphanable: a block
// that merges every tip keeps every branch in its cone, and the finality
// oracle infers "cannot be orphaned" from an agreement pattern that only
// holds while validators keep following the estimator. Collapsing to one
// tip removed that property — under load it fired on essentially every
// proposal, the DAG stopped re-merging, validators flipped between
// parallel single-parent chains, and a block all five nodes had finalized
// was orphaned three heights later, taking a deploy with it (ucc gate
// 38237bb7).
//
// SELF-orphaning in particular becomes structurally impossible once the
// frontier is preserved: a validator's own latest message is always in its
// frontier, and the frontier is always merged. The deploy on the unmerged
// carrier is then already in the parents' ancestry, so the ordinary
// in-scope filter suppresses a second copy — the work rides back on its
// own carrier, with no second execution and no second copy to adjudicate.
//
// The pool-re-proposal path remains reachable only through FOREIGN
// orphaning: a carrier the shard's fork choice leaves behind AND which has
// fallen past the parent-depth horizon, so no block may cite it back into
// the cone. That case is pinned by the second test below.
//
// DAG choreography (three validators; validator 1 risks orphaning itself):
//
//   genesis <- X(d)                v0's carrier; never delivered
//   genesis <- I(@"m"!({}))        v1 seeds a single-value cell
//   I <- A(set a), I <- B(set b)   v1/v2 contended cell writers
//   [A, B] <- M                    v1's merge; the cell race rejects one
//                                  writer WITH a record, arming the
//                                  recovery context on later snapshots
//   v0 syncs I/A/B/M and proposes: X must stay among its parents.

use casper::rust::casper::MultiParentCasper;
use casper::rust::util::{construct_deploy, proto_util};
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn own_unmerged_carrier_is_merged_back_never_orphaned() {
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

    // Phase 1 — validator 1 proposes X carrying d, then goes dark. X is
    // never delivered, so d's only copies are v0's own pool entry and its
    // own unmerged block.
    let orphan_deploy = construct_deploy::source_deploy_now_full(
        r#"@"orphan"!(1)"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build orphan deploy");
    let orphan_id = nodes[0]
        .canonical_deploy_id(&orphan_deploy)
        .expect("orphan deploy identity");
    let block_x = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&orphan_deploy))
        .await
        .expect("validator 1 proposes the carrier X");
    assert!(
        block_x.body.deploys.iter().any(|pd| {
            pd.deploy_id_for_protocol(block_x.header.version)
                .is_ok_and(|deploy_id| deploy_id == orphan_id)
                && !pd.is_failed
        }),
        "X must carry d cleanly"
    );

    // Phase 2 — a rejection record on the main branch: validators 2 and
    // 3 race a seeded single-value cell (only one datum to consume, so
    // the writers conflict deterministically) and validator 2 merges.
    // The unresolved rejection arms the recovery context for every
    // later snapshot that sees it.
    let init_cell = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"@"m"!({})"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("build cell init")
    };
    let cell_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&init_cell))
        .await
        .expect("validator 2 seeds the cell");
    nodes[2]
        .process_block(cell_block.clone())
        .await
        .expect("validator 3 processes the cell init");
    let writer_1 = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@m <- @"m") { @"m"!(m.set("a", 1)) }"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("build writer_1")
    };
    let writer_2 = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@m <- @"m") { @"m"!(m.set("b", 2)) }"#.to_string(),
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
        .expect("build writer_2")
    };
    let block_a = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&writer_1))
        .await
        .expect("validator 2 proposes A");
    let block_b = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(&writer_2))
        .await
        .expect("validator 3 proposes B");
    nodes[2]
        .process_block(block_a.clone())
        .await
        .expect("validator 3 processes A");
    nodes[1]
        .process_block(block_b.clone())
        .await
        .expect("validator 2 processes B");
    let merge_marker = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            0,
            Some(
                crate::util::genesis_builder::EXTRA_GENESIS_VAULT_KEY_PAIRS[0]
                    .0
                    .clone(),
            ),
            Some(shard_id.clone()),
        )
        .expect("build merge marker")
    };
    let merge_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&merge_marker))
        .await
        .expect("validator 2 proposes the merge M");
    assert!(
        !rejected_sigs(&merge_block).is_empty(),
        "the same-payer conflict must produce a rejection record in M"
    );

    // Phase 3 — validator 1 learns the main branch (one-way: nobody ever
    // receives X). Blocks are delivered directly through the validation
    // pipeline in causal order, so M is fully VALIDATED on validator 1 —
    // recorded as validator 2's latest message — before the re-propose.
    for block in [&cell_block, &block_a, &block_b, &merge_block] {
        nodes[0]
            .process_block((*block).clone())
            .await
            .expect("validator 1 processes the main branch");
    }
    let dag = nodes[0].casper.block_dag().await.expect("dag");
    assert!(
        dag.latest_message_hashes()
            .into_iter()
            .any(|(_, hash)| hash == merge_block.block_hash),
        "validator 1 must fully validate M (validator 2's latest message) \
         before re-proposing"
    );

    // Phase 4 — validator 1 proposes. Its own unmerged carrier X is its
    // latest message, so it is in the frontier, and parent selection
    // ORDERS the frontier rather than narrowing it away: X is merged in.
    let reinclude_block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("validator 1 re-proposes");
    assert!(
        reinclude_block
            .header
            .parents_hash_list
            .contains(&block_x.block_hash),
        "a validator's own latest message must remain among its parents: \
         dropping it is self-orphaning, which is what killed a finalized \
         block in ucc gate 38237bb7. parents={:?}",
        reinclude_block
            .header
            .parents_hash_list
            .iter()
            .map(|h| hex::encode(&h[..4.min(h.len())]))
            .collect::<Vec<_>>()
    );
    assert!(
        !reinclude_block.body.deploys.iter().any(|pd| {
            pd.deploy_id_for_protocol(reinclude_block.header.version)
                .is_ok_and(|deploy_id| deploy_id == orphan_id)
        }),
        "with X merged, d is already in the parents' ancestry, so the \
         ordinary in-scope filter must suppress a second copy — \
         re-proposing it here would duplicate work the merge already \
         recovered (body sigs: {:?})",
        reinclude_block
            .body
            .deploys
            .iter()
            .map(|pd| hex::encode(pd.deploy_id()))
            .collect::<Vec<_>>()
    );
}

// FOREIGN orphaning: the carrier is known to every validator, but the
// parent-depth spread rule forbids any block from citing it — the branch is
// left behind by fork choice AND past the citability horizon, so merging it
// back is impossible. The deploy's only route back is its owner's pool copy,
// and re-proposing it is legal shard-wide: the carrier is outside every
// selected cone, so the deploy is in no scope walk (no repeat), and it has
// no rejection record, so the retry gate does not apply.
//
// DAG choreography (three validators, max_parent_depth = 3):
//
//   genesis <- X(d)          v0's carrier, height 1; withheld
//   genesis <- b1 <- ... <- b6   v1's chain climbs past the spread horizon
//   X delivered LATE to v1/v2: everyone holds it, no one may cite it
//   v0 syncs b1..b6 and proposes: X is depth-capped out of its parents,
//   and d rides again from v0's pool.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn foreign_orphaned_work_returns_by_owner_pool_reproposal() {
    let n_validators = 3usize;
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();

    let max_parent_depth = 3i32;
    let mut nodes = TestNode::create_network(
        genesis,
        n_validators,
        None,
        None,
        Some(max_parent_depth),
        None,
    )
    .await
    .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    // Phase 1 — validator 1 proposes X carrying d, then goes dark. X is
    // withheld from the shard while the chain advances.
    let orphan_deploy = construct_deploy::source_deploy_now_full(
        r#"@"foreignorphan"!(1)"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build orphan deploy");
    let orphan_id = nodes[0]
        .canonical_deploy_id(&orphan_deploy)
        .expect("orphan deploy identity");
    let block_x = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&orphan_deploy))
        .await
        .expect("validator 1 proposes the carrier X");

    // Phase 2 — validator 2 chains rounds until the frontier's height
    // spread from X exceeds max_parent_depth: X falls past the citability
    // horizon while still unknown to the rest of the shard.
    let mut main_chain: Vec<BlockMessage> = Vec::new();
    for round in 0..6i32 {
        let marker = construct_deploy::basic_deploy_data(
            100 + round,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("build round marker");
        let b = nodes[1]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("validator 2 advances the chain");
        nodes[2]
            .process_block(b.clone())
            .await
            .expect("validator 3 follows the chain");
        main_chain.push(b);
    }
    let tip_height = proto_util::block_number(main_chain.last().expect("rounds ran"));
    let x_height = proto_util::block_number(&block_x);
    assert!(
        tip_height - x_height > max_parent_depth as i64,
        "staging precondition: the frontier must be more than \
         max_parent_depth above X (tip {}, X {})",
        tip_height,
        x_height
    );

    // Phase 3 — X arrives late. Every validator now HOLDS the carrier
    // (so blocks citing it in justifications have no missing
    // dependencies), but the spread rule forbids CITING it as a parent:
    // the branch is orphaned shard-wide.
    for idx in [1usize, 2] {
        let outcome = nodes[idx]
            .process_block(block_x.clone())
            .await
            .expect("late delivery of X");
        assert!(
            matches!(outcome, Either::Right(_)),
            "X is a valid block and must be accepted on late delivery, got {:?}",
            outcome
        );
    }

    // Phase 4 — validator 1 syncs the main chain and proposes. Its own
    // latest message X is past the horizon: the depth cap drops it
    // (protocol-forced orphaning), its cone leaves the scope walk, and
    // the pool copy of d is the only route back — the proposer takes it.
    for b in &main_chain {
        nodes[0]
            .process_block(b.clone())
            .await
            .expect("validator 1 syncs the main chain");
    }
    let reproposal = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("validator 1 proposes past the horizon");
    assert!(
        !reproposal
            .header
            .parents_hash_list
            .contains(&block_x.block_hash),
        "staging precondition: X is past the parent-depth horizon and must \
         be depth-capped out of the parents; citing it would be \
         InvalidParents. parents={:?}",
        reproposal
            .header
            .parents_hash_list
            .iter()
            .map(|h| hex::encode(&h[..4.min(h.len())]))
            .collect::<Vec<_>>()
    );
    assert!(
        reproposal.body.deploys.iter().any(|pd| {
            pd.deploy_id_for_protocol(reproposal.header.version)
                .is_ok_and(|deploy_id| deploy_id == orphan_id)
                && !pd.is_failed
        }),
        "the owner must re-propose d from its pool: the carrier left every \
         cone, so the pool copy is the last route back for the work \
         (body sigs: {:?})",
        reproposal
            .body
            .deploys
            .iter()
            .map(|pd| hex::encode(pd.deploy_id()))
            .collect::<Vec<_>>()
    );

    // The re-proposal is legal on every validator: d is in no scope walk
    // (X is outside every selected cone) and has no rejection record, so
    // neither the repeat rule nor the retry gate rejects it.
    for idx in [1usize, 2] {
        let outcome = nodes[idx]
            .process_block(reproposal.clone())
            .await
            .expect("re-proposal delivery");
        assert!(
            matches!(outcome, Either::Right(_)),
            "validator {} must validate the owner's re-proposal, got {:?}",
            idx + 1,
            outcome
        );
    }
}
