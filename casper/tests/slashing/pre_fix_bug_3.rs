// Pre-fix regression backstop for bug #3 (catch-all dispatcher
// doesn't mint records for non-equivocation slashable variants).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.4.
// Out-of-band approach: this asserts the post-fix invariant that
// would FAIL on the parent of the bug-#3 fix commit (where
// multi_parent_casper_impl.rs:1090-1099's catch-all only called
// `handle_invalid_block_effect` without minting an EquivocationRecord
// for the 14 non-equivocation slashable variants:
//   InvalidBlockNumber, InvalidParents, InvalidFollows,
//   InvalidSequenceNumber, InvalidShardId, JustificationRegression,
//   NeglectedInvalidBlock, NeglectedEquivocation, InvalidTransaction,
//   InvalidBondsCache, InvalidBlockHash, ContainsExpiredDeploy,
//   ContainsTimeExpiredDeploy, ContainsFutureDeploy
// ).
//
// Post-fix invariant: every slashable status — equivocation OR
// non-equivocation — produces an EquivocationRecord so the proposing
// layer can later issue a SlashDeploy. Tests UC-28 through UC-36
// each cover one specific variant; this backstop exercises the
// catch-all dispatcher arm directly.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn pre_fix_bug_3_catchall_records_other_slashable() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // A valid-looking block by v0 at seq=7. We use
    // `dispatch_with_status` to simulate an upstream-validation
    // verdict of `SlashableOther` (representing any of the 14
    // non-equivocation slashable variants).
    let hash = harness.sign_block("v0", 7);
    let status = harness.dispatch_with_status(hash, Status::SlashableOther);

    assert_eq!(status, Status::SlashableOther);

    // Post-fix #3: the dispatcher's catch-all mints an
    // EquivocationRecord for the offender. Pre-fix this assertion
    // FAILS (the catch-all only logged + persisted the block as
    // invalid; no record was minted, so no proposer could later
    // issue a SlashDeploy).
    assert!(
        harness.has_record("v0", 6),
        "post-fix #3: dispatcher mints record for any slashable variant; \
         pre-fix the catch-all silently skipped record creation"
    );
}
