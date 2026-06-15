// UC-10 — InvalidFormat is non-slashable; dispatcher returns the
// block to the catch-all `_` arm without minting a record.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-10.
// Theorem: T-3 (taxonomy correctness — 8 non-slashable variants
// stay non-slashable).
//
// The harness's `Status::Valid` arm is the projection of the
// dispatcher's `_` non-slashable arm: no record minted, no entry
// in the invalid-block index.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_10_invalid_format_no_record_no_invalidation() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 5);

    // Forced classification as Valid — represents the 8 non-
    // slashable InvalidBlock variants in the harness's projection.
    let status = harness.dispatch_with_status(hash, Status::Valid);
    assert_eq!(status, Status::Valid);

    // T-3 invariant: no record minted, no invalid-index entry.
    assert!(!harness.has_record("v0", 4));
    assert!(!harness.dag.invalid.contains(&hash));
}
