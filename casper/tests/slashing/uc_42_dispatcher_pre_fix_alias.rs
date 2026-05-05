// UC-42 — Pre-fix dispatcher stub regression (audit-tier alias).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-42.
// Theorem: T-9.3 (negative).
//
// §14.3.2 audit-blocker alias for `pre_fix_bug_3.rs`.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_42_dispatcher_pre_fix_drop() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 7);

    let status = harness.dispatch_with_status(hash, Status::SlashableOther);
    assert_eq!(status, Status::SlashableOther);
    assert!(harness.has_record("v0", 6),
        "post-fix #3: catch-all mints record; pre-fix this fails");
}
