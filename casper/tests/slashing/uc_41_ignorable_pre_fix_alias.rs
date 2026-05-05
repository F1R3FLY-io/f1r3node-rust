// UC-41 — Pre-fix Ignorable DOS regression (audit-tier alias).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-41.
// Theorem: T-9.1 (negative).
//
// §14.3.2 audit-blocker alias for `pre_fix_bug_1.rs`. The two tests
// share the same scenario (the canonical ignorable-equivocation
// counter-example); UC-41 is the §12-table entry, pre_fix_bug_1
// is the §14.7 backstop. Keeping both keeps the spec/test mapping
// bijective.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_41_ignorable_pre_fix_dos() {
    let mut harness = SlashingTestHarness::new(3, 100);
    let _b1 = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);

    let s = harness.dispatch(bad);
    assert_eq!(s, Status::IgnorableEquivocation);
    assert!(harness.has_record("v0", 4),
        "post-fix #1: dispatcher mints record for Ignorable; pre-fix this fails");
}
