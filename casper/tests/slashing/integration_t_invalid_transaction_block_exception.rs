// Integration test — Tier 1 production-path verification of the
// `InvalidTransaction` arm reached via the BlockException dispatch
// path (sibling to UC-34, which exercises the `Right(None)` path).
//
// Theorem citation: T-9.3 (catch-all dispatcher), Rocq
// formal/rocq/slashing/theories/BugFixDispatcher.v `t_9_3_dispatch_complete`.
// Spec reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.4.
//
// What this pins (and UC-34 does not):
//   - block_processor.rs:358-371 — the BlockException catch-all. UC-34
//     reaches InvalidTransaction via the `Right(None)` path in
//     validation_dispatcher.rs:114-118 (returned from
//     validate_block_checkpoint when computed_post_state_hash disagrees
//     with block.body.state.post_state_hash). The two paths feed
//     bit-identical arguments into effects_for_invalid_block at the
//     DAG-mutation layer, BUT they are NOT identical at the
//     returned-status layer:
//       * UC-34 (Right(None) path): returned status is converted to
//         `Left(BlockError::Invalid(InvalidTransaction))` by the
//         dispatcher at validation_dispatcher.rs:114-118 before
//         returning to block_processor; both DAG and status are
//         normalized.
//       * This test (BlockException path): returned status is the
//         ORIGINAL `Left(BlockError::BlockException(_))` raised by
//         validate_block_checkpoint; the block_processor.rs:358-371
//         catch-all converts ONLY at the DAG-mutation layer (via
//         `effects_for_invalid_block(..., &InvalidBlock::InvalidTransaction, ...)`),
//         NOT at the status returned to the caller. The pre-existing
//         comment at line 339-340 ("this is to maintain backward
//         compatibility with casper validate method. as it returns
//         not only InvalidBlock or ValidBlock") confirms this is
//         intentional.
//   - The propose_with_block_mutation helper's "mutation runs AFTER
//     compute_deploys_checkpoint records the deployLog" property for
//     timestamp-class mutations.
//   - T-9.3 invariant across BOTH dispatcher entry paths: regardless
//     of which path was taken, the EquivocationRecord IS minted on
//     v0 because both paths flow through the dispatcher's
//     is_slashable() catch-all (validation_dispatcher.rs:548).
//
// Forge mechanics:
//   compute_deploys_checkpoint records the closeBlock system deploy's
//   deployLog against the original BlockData.time_stamp (= parent_max_ts
//   + 1). The mutator increments header.timestamp AFTER the deployLog is
//   sealed but BEFORE signing. The receiver derives a fresh BlockData
//   from the (mutated) header during replay; closeBlock produces events
//   on blockDataCh with the NEW timestamp; the matcher compares against
//   the recorded deployLog (events for the OLD timestamp); ConsumeFailed
//   fires; after the 3-retry budget gives up, BlockException propagates
//   out of validate_block_checkpoint; block_processor.rs:358-371's
//   catch-all maps it to InvalidTransaction; the dispatcher's
//   is_slashable() arm (validation_dispatcher.rs:548) mints an
//   EquivocationRecord at (v0, seq-1) identically to the
//   AdmissibleEquivocation arm.

use casper::rust::block_status::BlockError;
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::{
    canonical_validator_order, production_snapshot_at, propose_with_block_mutation,
};
use super::observer::SlashingObserver;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_invalid_transaction_block_exception() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    let d1 = construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).expect("d1");
    let mutated = propose_with_block_mutation(&mut nodes[0], vec![d1], |b| {
        // Mutate header.timestamp by +1ms AFTER compute_deploys_checkpoint
        // has recorded the closeBlock deployLog against the original
        // timestamp. The +1ms is the smallest viable perturbation —
        // closeBlock writes (block_num, sender, timestamp) to rspace via
        // rho:block:data, and the receiver's replay re-derives all three
        // from block.header at validation time. With the timestamp changed,
        // the replay's events diverge from the recorded deployLog
        // verbatim, surfacing as SystemRuntimeError(ConsumeFailed) after
        // the 3-retry budget gives up.
        b.header.timestamp += 1;
    })
    .await
    .expect("propose_with_block_mutation");

    let status = nodes[1]
        .process_block(mutated.clone())
        .await
        .expect("process_block");

    // Status assertion: the BlockException is returned to the caller
    // verbatim. The block_processor.rs:358-371 catch-all does NOT
    // convert the status — only the DAG-level state. This is the
    // semantic distinction from UC-34, which receives the converted
    // `Left(BlockError::Invalid(InvalidTransaction))` because the
    // dispatcher normalizes status BEFORE the value reaches the
    // block_processor's match arms (validation_dispatcher.rs:114-118).
    assert!(
        matches!(status, Either::Left(BlockError::BlockException(_))),
        "expected BlockException raised by closeBlock replay \
         (SystemRuntimeError(ConsumeFailed) after 3-retry budget), \
         got: {:?}",
        status
    );

    // DAG-layer assertion: the block IS recorded as InvalidTransaction
    // (via the catch-all's `effects_for_invalid_block(..., &InvalidBlock::InvalidTransaction, ...)`
    // call at block_processor.rs:362-369), and the dispatcher's
    // `is_slashable()` catch-all (validation_dispatcher.rs:548) minted
    // an EquivocationRecord for v0. This is the T-9.3 invariant: a
    // slashable invalid block always produces a record, regardless of
    // which upstream path classified it.
    let snapshot = production_snapshot_at(
        &nodes[1],
        &genesis.genesis_block,
        &genesis.genesis_block,
        validators,
    )
    .await
    .expect("snapshot");

    let has_v0 = (0..=10).any(|b| <_ as SlashingObserver>::has_record(&snapshot, "v0", b));
    assert!(
        has_v0,
        "post-fix #3 catch-all: dispatcher mints EquivocationRecord for v0 \
         on BlockException → InvalidTransaction DAG-layer dispatch \
         (validation_dispatcher.rs:548 is_slashable() arm). The status \
         layer carries the original BlockException — see status assertion."
    );
}
