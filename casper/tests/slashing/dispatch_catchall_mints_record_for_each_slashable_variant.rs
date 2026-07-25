// Parameterized coverage for the `is_slashable()` catch-all arm of
// `validation_dispatcher::dispatch_handle_invalid_block`.
//
// The dispatcher routes invalid blocks through three arms:
//
//   match status {
//       InvalidBlock::AdmissibleEquivocation => { record + insert+buffer }
//       InvalidBlock::IgnorableEquivocation  => { record + insert+buffer }
//       status if status.is_slashable()       => { record + insert+buffer }  // ◀ here
//       _                                     => { buffer remove only }
//   }
//
// The catch-all guard at `validation_dispatcher.rs:489` covers
// **17** slashable variants. Individually most variants have a
// production-path integration test (e.g.
// `integration_t_invalid_block_hash_records`), but a single
// parameterized assertion over the full slashable taxonomy is what
// catches regressions that *re-route a variant past the catch-all*
// (e.g. adding a stray arm above that fails to mint a record). The
// individual integration tests cannot catch such a regression by
// construction — they only fail if the dispatcher mis-handles the
// specific variant they exercise.
//
// Per slashable variant `V` routed through the catch-all arm, this
// test asserts the documented post-conditions:
//   (a) `V.is_slashable() == true`        — compile-time-enumerated.
//   (b) `dag.contains(hash)`              — block committed to DAG.
//   (c) `!buffer.contains(hash)`          — buffer entry purged.
//   (d) `EquivocationRecord` minted at `(sender, seq_num - 1)`.
//
// Plan reference: Commit 12 / Test-5 of the second-pass review plan.
//
// Maps to: docs/theory/slashing/design/09-bug-fixes-and-rationale.md
// §9.3 (Bug #3: dispatcher catch-all). The catch-all originally
// silently skipped record-minting; the bug-fix commits land
// sequentially, so reverting to the parent of Bug #3 reproduces the
// regression this test guards.

use casper::rust::block_status::InvalidBlock;
use casper::rust::casper::{Casper, MultiParentCasper};
use models::rust::block_hash::BlockHashSerde;

use super::detector_totality_helpers::{block as synth_block, validator as synth_validator};
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// 17 slashable `InvalidBlock` variants routed via the
/// `is_slashable()` catch-all guard. AdmissibleEquivocation and
/// IgnorableEquivocation are NOT in this list — they have their own
/// dispatcher arms above the catch-all and are covered by
/// `integration_t_admissible_equivocation` / `integration_t_ignorable_equivocation`.
///
/// If a new slashable variant is added without updating this list,
/// the test still passes against the affected variant — but
/// `block_status.rs::is_slashable()` is exhaustively `match`-ed
/// (no wildcard), so the new variant forces a compile error there
/// first. Maintaining this constant in lockstep with the enum is a
/// one-line follow-up when new variants land.
const CATCHALL_SLASHABLE_VARIANTS: &[InvalidBlock] = &[
    InvalidBlock::DeployNotSigned,
    InvalidBlock::InvalidBlockNumber,
    InvalidBlock::InvalidRepeatDeploy,
    InvalidBlock::InvalidParents,
    InvalidBlock::InvalidFollows,
    InvalidBlock::InvalidSequenceNumber,
    InvalidBlock::InvalidShardId,
    InvalidBlock::JustificationRegression,
    InvalidBlock::NeglectedInvalidBlock,
    InvalidBlock::NeglectedEquivocation,
    InvalidBlock::InvalidTransaction,
    InvalidBlock::InvalidBondsCache,
    InvalidBlock::InvalidBlockHash,
    InvalidBlock::UnauthorizedSlashDeploy,
    InvalidBlock::ContainsExpiredDeploy,
    InvalidBlock::ContainsTimeExpiredDeploy,
    InvalidBlock::ContainsFutureDeploy,
];

#[tokio::test]
async fn dispatch_catchall_mints_record_for_each_slashable_variant() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to build standalone node");

    // Each iteration uses a freshly-minted synthetic block with a
    // distinct sender and block_hash so the per-variant assertions
    // are independent (no aliasing on `(sender, equivocation_base_block_seq_num)`
    // record-existence collisions). All blocks share `seq_num = 2`
    // ⇒ `equivocation_base_block_seq_num = 1` from `checked_base_seq`.
    let seq_num: i32 = 2;
    let expected_base_seq: i32 = 1;

    for (i, variant) in CATCHALL_SLASHABLE_VARIANTS.iter().enumerate() {
        // (a) Sanity: the variant must be slashable; otherwise the
        // catch-all guard would skip it and route to the `_ =>`
        // arm. This precondition pins the test's relevance to the
        // catch-all arm specifically.
        assert!(
            variant.is_slashable(),
            "[{:?}] variant must be slashable for this catch-all test to be meaningful",
            variant
        );

        let hash_byte = (i as u8).saturating_add(1);
        // Sender index offset to avoid colliding with genesis-bonded
        // validator bytes (which start at 0); +21 deconflicts within
        // the 0..=255 byte space well past the bonded-set range.
        let sender = synth_validator(hash_byte.saturating_add(21));
        let synth = synth_block(hash_byte, sender.clone(), seq_num, vec![], vec![]);

        // Pre-populate the buffer with the hash as a dependency-free
        // pendant. `put_pendant` is the canonical "register a block in
        // the buffer with no known parents" entry, and is how the
        // block_processor places freshly-received blocks awaiting
        // parent fetch. After admit/dispatch, the hash MUST no longer
        // appear among the buffer's pendants — that's the buffer half
        // of the (DAG insert, buffer remove) atomic step.
        let hash_serde = BlockHashSerde(synth.block_hash.clone());
        node.casper
            .casper_buffer_storage
            .put_pendant(hash_serde.clone())
            .expect("pre-populate buffer pendant");
        assert!(
            node.casper
                .casper_buffer_storage
                .get_pendants()
                .contains(&hash_serde),
            "[{:?}] pre-test setup: buffer should contain the pendant before dispatch",
            variant
        );

        // Drive the dispatcher's `is_slashable()` catch-all arm.
        let dag_repr = node.casper.block_dag().await.expect("dag representation");
        node.casper
            .handle_invalid_block(&synth, variant, &dag_repr)
            .expect("dispatcher catch-all arm must succeed for slashable variant");

        // (b) Block in DAG.
        let dag_after = node
            .casper
            .block_dag()
            .await
            .expect("post-dispatch dag representation");
        assert!(
            dag_after.contains(&synth.block_hash),
            "[{:?}] block must be in DAG after catch-all dispatch",
            variant
        );

        // (c) Buffer entry purged. The atomic helper invokes
        // `BufferTransition::RemoveFromBuffer(hash)` after the DAG
        // insert; a regression that re-routes the variant past this
        // step would leave the pendant in place.
        assert!(
            !node
                .casper
                .casper_buffer_storage
                .get_pendants()
                .contains(&hash_serde),
            "[{:?}] buffer must be purged after catch-all dispatch \
             (drift state — Bug #17 / T-9.20 atomicity contract violated)",
            variant
        );

        // (d) EquivocationRecord minted at (sender, seq - 1).
        let records = node
            .casper
            .block_dag_storage
            .access_equivocations_tracker(|tracker| tracker.data())
            .expect("equivocations tracker access");
        let has_record = records.iter().any(|record| {
            record.equivocator == synth.sender
                && record.equivocation_base_block_seq_num == expected_base_seq
        });
        assert!(
            has_record,
            "[{:?}] catch-all arm must mint an EquivocationRecord at \
             (sender, base_seq={}); pre-bug-#3-fix this assertion fails",
            variant, expected_base_seq
        );
    }
}
