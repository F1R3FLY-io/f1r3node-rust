// Pre-fix regression backstop for bug #1 (IgnorableEquivocation drop).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.1.
// Out-of-band approach: this file asserts the *post-fix* invariant
// directly; running it against the parent of the fix commit (where
// `is_slashable()` returns `false` for IgnorableEquivocation and the
// dispatcher returns `Ok(dag.clone())` early without minting a
// record) reproduces the bug. See design/14-test-plan.md §14.7.
//
// The post-fix invariant: when a validator equivocates and no other
// block cites the equivocating block (the unrequested-equivocation
// case), the dispatcher *still* mints an EquivocationRecord so the
// proposing layer can eventually issue a SlashDeploy. Pre-fix, the
// validator could equivocate freely with no economic consequence —
// a documented DOS vector (see design/01-introduction.md §1.2 row 1).

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn pre_fix_bug_1_ignorable_dos_closed() {
    let mut harness = SlashingTestHarness::new(3, 100);
    // `_b1` is v0's *honest* block at seq=5 (underscored because no
    // assertion looks at it directly; it just establishes the prior
    // creator-justification). `b1_prime` is v0's *equivocating* block
    // at the same seq=5 — the offending block whose hash must appear
    // in the record's witness set below.
    let _b1 = harness.sign_block("v0", 5);
    let b1_prime = harness.sign_block_distinct("v0", 5);

    let status = harness.dispatch(b1_prime);
    assert_eq!(
        status,
        Status::IgnorableEquivocation,
        "the unrequested equivocation classifies as Ignorable"
    );

    // Post-fix #1 invariant — would FAIL on the parent of fa29d33's
    // followup-fix-#1 commit, where is_slashable() returns false for
    // IgnorableEquivocation and the dispatcher returns
    // Ok(dag.clone()) silently.
    assert!(
        harness.has_record("v0", 4),
        "post-fix #1: dispatcher mints EquivocationRecord even for Ignorable variant; \
         pre-fix this assertion fails (validator equivocates with no economic consequence)"
    );

    // The DOS-vector outcome we DON'T want: validator's bond unchanged
    // and no record means subsequent proposers cannot issue a
    // SlashDeploy. The post-fix mints a record so proposers can.
    let witnesses = harness.record_witnesses("v0", 4);
    assert!(
        witnesses.contains(&b1_prime),
        "the recorded witnesses include the offending block hash"
    );
}
