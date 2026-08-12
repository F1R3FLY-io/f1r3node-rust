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
// the cone. That case is covered by the foreign-orphaning spec staged with
// the retry gate, not here.
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
use casper::rust::util::construct_deploy;
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
        .map(|rd| rd.sig.clone())
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
    let orphan_sig: Bytes = orphan_deploy.sig.clone();
    let block_x = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&orphan_deploy))
        .await
        .expect("validator 1 proposes the carrier X");
    assert!(
        block_x
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == orphan_sig && !pd.is_failed),
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
        !reinclude_block
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == orphan_sig),
        "with X merged, d is already in the parents' ancestry, so the \
         ordinary in-scope filter must suppress a second copy — \
         re-proposing it here would duplicate work the merge already \
         recovered (body sigs: {:?})",
        reinclude_block
            .body
            .deploys
            .iter()
            .map(|pd| hex::encode(&pd.deploy.sig[..8.min(pd.deploy.sig.len())]))
            .collect::<Vec<_>>()
    );
}
