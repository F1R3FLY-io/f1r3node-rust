// UC-07 — InvalidRepeatDeploy variant flows through the post-fix
// dispatcher's catch-all.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-07.
// Theorem: T-9.3 (catch-all dispatcher records every slashable
// variant).
//
// Mirrors the per-variant Tier B treatment — the existing
// classifier in casper/src/rust/validate.rs covers the per-variant
// distinction; the harness uses Status::SlashableOther as the
// abstract umbrella for these 14 variants.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_07_invalid_repeat_deploy_recorded() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 6);
    let status = harness.dispatch_with_status(hash, Status::SlashableOther);
    assert_eq!(status, Status::SlashableOther);
    assert!(harness.has_record("v0", 5));
    assert!(harness.dag.invalid.contains(&hash));
}
