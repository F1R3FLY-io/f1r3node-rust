// Pre-fix regression backstop for bug #7 (off-by-one seq-number
// density assumption).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.8.
// Out-of-band approach: this asserts the post-fix invariant — an
// equivocation by a validator who has *skipped* a sequence number
// is still detected. Pre-fix, `add_equivocation_child` used
// `target_seq_num = baseSeqNum + 1` and an exact-seq match,
// failing if the validator legitimately skipped a number under
// partition recovery. Post-fix, the production detector uses the
// canonical visible self-chain child above `baseSeq`.
//
// The harness's `detect` operates on (sender, seq) pairs and is
// independent of the production code's creator/self-justification
// search; the harness can construct synthetic seq-skip scenarios
// to document the post-fix invariant. The full DAG-level proof
// of T-9.7 is exercised by the Rocq theorem at
// formal/rocq/slashing/theories/BugFixSeqNumDensity.v.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn pre_fix_bug_7_seq_skip_equivocation_detected() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // v0 publishes at seq=0, then SKIPS seq=1, jumps to seq=2.
    let _b_0 = harness.sign_block("v0", 0);
    let _b_2 = harness.sign_block("v0", 2);

    // v0 equivocates at seq=2 — the gap at seq=1 must NOT prevent
    // detection.
    let bad = harness.sign_block_distinct("v0", 2);
    let s = harness.dispatch(bad);

    // Post-fix #7 invariant: detection works regardless of seq density.
    // Pre-fix the production `add_equivocation_child`'s exact-seq
    // match could fail when the validator skipped a number.
    assert_eq!(
        s,
        Status::IgnorableEquivocation,
        "post-fix #7: equivocation at seq=2 (with seq=1 skipped) is still detected"
    );
    assert!(
        harness.has_record("v0", 1),
        "post-fix #7: dispatcher mints record at base=1 even when seq=1 was skipped"
    );
}

#[test]
fn pre_fix_bug_7_far_seq_jump_equivocation_detected() {
    // Even a large jump (skip 100 sequences) does not prevent detection.
    let mut harness = SlashingTestHarness::new(2, 100);

    let _b_0 = harness.sign_block("v0", 0);
    let _b_100 = harness.sign_block("v0", 100);

    let bad = harness.sign_block_distinct("v0", 100);
    let s = harness.dispatch(bad);
    assert_eq!(s, Status::IgnorableEquivocation);
    assert!(
        harness.has_record("v0", 99),
        "post-fix #7: canonical self-chain search handles arbitrary gaps"
    );
}
