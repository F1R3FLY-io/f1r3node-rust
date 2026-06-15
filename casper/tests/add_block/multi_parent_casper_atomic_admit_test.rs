// Atomic admit invariant (Bug #17 / T-9.20) — production-path check.
//
// `block_admission::admit_handle_valid_block` is the production caller
// of `block_storage::rust::dag::buffer_dag_transition::atomic_insert_then_buffer`
// for the *valid* admission path (the invalid-path counterpart is
// `validation_dispatcher::dispatch_handle_invalid_block`, exercised
// by `dispatch_catchall_mints_record_for_each_slashable_variant.rs`).
//
// The atomicity contract is:
//   At every observable boundary,  block ∈ DAG  ⇔  block ∉ casper_buffer.
//   The crash-window drift state `block ∈ DAG ∧ block ∈ casper_buffer`
//   is closed by `reconcile_buffer_against_dag` on resume (UC-55 covers
//   the harness-model side, T-9.20.recon covers the Rocq side).
//
// This test pins the invariant on the production code path by:
//   1. Submitting a real deploy through `add_block_from_deploys` (which
//      routes through `admit_handle_valid_block`).
//   2. Asserting the post-condition on the producer node.
//   3. Synchronizing with a peer and asserting the same post-condition
//      there (i.e. the invariant survives gossip → admit → store).
//   4. Asserting the inverse drift state is never observable from
//      outside the helper (paranoid wedge).
//
// Plan reference: Commit 12 / Test-4 of the second-pass review plan.

use casper::rust::casper::MultiParentCasper;
use casper::rust::util::construct_deploy;
use models::rust::block_hash::BlockHashSerde;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn handle_valid_block_satisfies_atomic_dag_buffer_invariant() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("Failed to create network");

    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(shard_id)).expect("deploy build");

    // Round 1: node 0 proposes a valid block. Internally this routes
    // through `admit_handle_valid_block`, which calls
    // `atomic_insert_then_buffer` to commit the (DAG insert,
    // buffer remove) pair under the documented lock-order contract
    // (DAG global_lock A ▸ buffer state_lock B).
    let signed_block = TestNode::publish_block_at_index(&mut nodes, 0, &[deploy])
        .await
        .expect("publish block");

    // --- Atomicity post-condition on the producer (node 0) ---
    let block_hash = signed_block.block_hash.clone();
    let block_hash_serde = BlockHashSerde(block_hash.clone());

    let dag_repr_n0 = nodes[0]
        .casper
        .block_dag()
        .await
        .expect("block_dag on producer");
    let dag_contains_n0 = dag_repr_n0.contains(&block_hash);
    let buffer_contains_n0 = nodes[0]
        .casper
        .casper_buffer_storage
        .contains(&block_hash_serde);
    assert!(
        dag_contains_n0,
        "[producer] valid block must be in DAG after admit; got dag_contains=false"
    );
    assert!(
        !buffer_contains_n0,
        "[producer] valid block must NOT be in casper_buffer after admit; got buffer_contains=true \
         (drift state — Bug #17 / T-9.20 atomicity contract violated)"
    );
    // Paranoid wedge: the inverse drift state must not be externally
    // observable. If both halves are true the invariant has been broken.
    assert!(
        !(dag_contains_n0 && buffer_contains_n0),
        "[producer] forbidden drift state (in DAG ∧ in buffer) observed"
    );

    // --- Synchronize with peer and re-assert on receiver (node 1) ---
    {
        let (left, right) = nodes.split_at_mut(1);
        right[0]
            .sync_with_one(&mut left[0])
            .await
            .expect("sync_with_one");
    }

    let dag_repr_n1 = nodes[1]
        .casper
        .block_dag()
        .await
        .expect("block_dag on receiver");
    let dag_contains_n1 = dag_repr_n1.contains(&block_hash);
    let buffer_contains_n1 = nodes[1]
        .casper
        .casper_buffer_storage
        .contains(&block_hash_serde);
    assert!(
        dag_contains_n1,
        "[receiver] valid block must be in DAG after gossip → admit"
    );
    assert!(
        !buffer_contains_n1,
        "[receiver] valid block must NOT be in casper_buffer after gossip → admit \
         (drift state — atomicity contract violated on the receive path)"
    );
    assert!(
        !(dag_contains_n1 && buffer_contains_n1),
        "[receiver] forbidden drift state (in DAG ∧ in buffer) observed"
    );
}
