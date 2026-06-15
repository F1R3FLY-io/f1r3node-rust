// UC-33 — DeployNotSigned variant flows through the post-fix
// dispatcher to a recorded slashable status.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-33.
// This is one of the 14 non-equivocation slashable variants the
// post-fix dispatcher (bug fix #3) records uniformly. The harness
// uses `dispatch_with_status` to simulate the upstream validator's
// classification.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_33_deploy_not_signed_recorded() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 8);

    // Upstream validation classifies the block as DeployNotSigned —
    // one of the 14 non-equivocation slashable variants. The harness
    // models this with `Status::SlashableOther`.
    let status = harness.dispatch_with_status(hash, Status::SlashableOther);
    assert_eq!(status, Status::SlashableOther);

    // Post-fix #3: a record is minted at base_seq = 7.
    assert!(
        harness.has_record("v0", 7),
        "post-fix #3: dispatcher mints record for non-equivocation slashable variant"
    );
    assert!(
        harness.dag.invalid.contains(&hash),
        "the offending block is added to the invalid index"
    );
}
