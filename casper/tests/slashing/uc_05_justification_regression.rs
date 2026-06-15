// UC-05 — JustificationRegression dispatch.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-05.
// Theorems: T-3, T-6, T-9.3.
//
// Scenario: a block whose justifications regress on the validator's own
// prior latest-message classifies as `JustificationRegression`. The
// post-fix catch-all dispatcher mints an EquivocationRecord so the
// proposing layer can later issue a SlashDeploy.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_05_justification_regression_mints_record() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 6);

    let status = harness.dispatch_with_status(hash, Status::JustificationRegression);

    assert_eq!(status, Status::JustificationRegression);
    assert!(harness.has_record("v0", 5));
    assert!(harness.dag.invalid.contains(&hash));
}
