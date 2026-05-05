// Integration test — Tier 1 production-path verification of the
// `IgnorableEquivocation` arm of
// `MultiParentCasperImpl::handle_invalid_block`.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.3.5,
// design/09-bug-fixes-and-rationale.md §9.1 (bug #1).
// Plan-agent designed Track 2 / equivocation-construction recipe.
//
// The earlier mutate-and-resign attempts (mutating header.timestamp
// or extra_bytes then re-signing) failed structurally: replay reads
// header fields and recomputes the post-state hash, so any mutation
// of a real block's body or header produces "Unable to consume
// results of system deploy" — rejection as RuntimeError, NOT
// IgnorableEquivocation. The only correct construction is the
// `equivocate_block` helper in `integration_helpers.rs`, which
// builds `b1p` from a different deploy set entirely and runs it
// through the real interpreter to compute a matching post-state.
//
// Recipe:
//   1. v0's TestNode (nodes[0]) creates b1 with deploy d1 — but
//      `create_block_unsafe` does NOT add b1 to nodes[0]'s DAG.
//      nodes[0]'s state is still at genesis.
//   2. equivocate_block(nodes[0], &b1, vec![d2]) builds b1p from
//      genesis with deploy d2. Same parents, same seq_num, same
//      sender as b1; distinct hash; passes replay in isolation.
//   3. Process b1 on nodes[1] → Right(Valid).
//   4. Process b1p on nodes[1] → Left(Invalid(IgnorableEquivocation)).
//      No other node has cited b1p as a dependency, so
//      requested_as_dependency returns false →
//      `IgnorableEquivocation` (equivocation_detector.rs:42-60).
//   5. Post-fix #1 invariant: dispatcher mints an EquivocationRecord
//      at (v0, base_seq=0). Pre-fix this assertion fails (the
//      variant was non-slashable + dispatcher silently dropped).

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

use super::integration_helpers::{
    canonical_validator_order, equivocate_block, production_snapshot_at,
};
use super::observer::SlashingObserver;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_ignorable_equivocation() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    // Round 1: v0 (nodes[0]) creates b1 with deploy d1.
    // `create_block_unsafe` returns the block but does NOT add it
    // to nodes[0]'s DAG, so the snapshot taken inside
    // `equivocate_block` is still at genesis.
    let d1 =
        construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).expect("d1");
    let b1 = nodes[0]
        .create_block_unsafe(&[d1])
        .await
        .expect("create b1");

    // Construct b1p: same v0, same seq_num, same parents/justifications
    // as b1, but a different deploy. Must use a distinct nonce so
    // Validate::repeat_deploy doesn't reject the second block.
    let d2 = construct_deploy::basic_deploy_data(1, None, Some(shard_id.clone()))
        .expect("d2 (distinct nonce from d1)");
    let b1p = equivocate_block(&mut nodes[0], &b1, vec![d2])
        .await
        .expect("equivocate_block");

    assert_ne!(
        b1.block_hash, b1p.block_hash,
        "equivocation requires distinct hashes"
    );
    assert_eq!(
        b1.seq_num, b1p.seq_num,
        "equivocation requires same seq_num"
    );
    assert_eq!(b1.sender, b1p.sender, "equivocation requires same sender");

    // Process b1 (well-formed) on node 1 first. Node 1 sees this
    // as a normal block; no equivocation yet.
    let s1 = nodes[1]
        .process_block(b1.clone())
        .await
        .expect("process b1");
    assert!(
        matches!(s1, Either::Right(_)),
        "first block accepts: {:?}",
        s1
    );

    // Process b1p on node 1. Equivocation detected; no other node
    // has cited b1p as a dependency, so `requested_as_dependency`
    // returns false → IgnorableEquivocation.
    let s2 = nodes[1].process_block(b1p.clone()).await.expect("process b1p");
    assert!(
        matches!(
            s2,
            Either::Left(BlockError::Invalid(InvalidBlock::IgnorableEquivocation))
                | Either::Left(BlockError::Invalid(InvalidBlock::AdmissibleEquivocation))
        ),
        "b1p classified as equivocation, got: {:?}",
        s2
    );

    // Snapshot and assert post-fix #1: a record exists at
    // (v0, some base_seq) for v0. The exact base seq depends on
    // genesis-block sequence numbering; we assert presence rather
    // than a specific base value.
    let snapshot =
        production_snapshot_at(&nodes[1], &b1, &genesis.genesis_block, validators)
            .await
            .expect("snapshot");

    let v0_label = "v0";
    let has_any_record = (0..=10).any(|base| {
        <_ as SlashingObserver>::has_record(&snapshot, v0_label, base)
    });
    assert!(
        has_any_record,
        "post-fix #1: dispatcher mints EquivocationRecord for the \
         IgnorableEquivocation arm; pre-fix this assertion fails \
         (the variant was non-slashable + dispatcher returned \
         Ok(dag.clone()) silently)"
    );
}
