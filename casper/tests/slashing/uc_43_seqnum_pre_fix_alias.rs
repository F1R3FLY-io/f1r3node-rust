// UC-43 — Pre-fix off-by-one seq-density regression (audit-tier alias).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-43.
// Theorem: T-9.7 (negative).
//
// §14.3.2 audit-blocker alias for `pre_fix_bug_7.rs`.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_43_seqnum_pre_fix_miss() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // Validator skips a sequence number.
    let _b_0 = harness.sign_block("v0", 0);
    let _b_2 = harness.sign_block("v0", 2);
    let bad = harness.sign_block_distinct("v0", 2);

    let s = harness.dispatch(bad);
    assert_eq!(s, Status::IgnorableEquivocation);
    assert!(harness.has_record("v0", 1),
        "post-fix #7: BFS-style descendant search handles seq-skip; \
         pre-fix this misses the equivocation");
}
