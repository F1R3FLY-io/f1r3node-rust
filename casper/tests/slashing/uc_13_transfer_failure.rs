// UC-13 — Slash with failed bond/vault transfer.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-13.
// Theorems: T-9.4.
// Reference: formal/rocq/slashing/theories/BugFixTransferFailure.v.
//
// Scenario: when the bond->coop-vault transfer fails inside slashing,
// the slash returns `success=false` and **no** state has mutated —
// bond, active flag, coop-vault balance, and the `slashed` set are all
// preserved so a future block can retry the slash. Pre-fix the transfer
// could partially apply, producing an inconsistent post-state.

use super::harness::SlashingTestHarness;

#[test]
fn uc_13_transfer_failure_preserves_state_for_retry() {
    let mut harness = SlashingTestHarness::new(3, 100);

    let pre_bond = harness.bond("v1");
    let pre_active = harness.is_active("v1");
    let pre_coop = harness.coop_vault();
    let pre_slashed = harness.pos_state.slashed.clone();

    let result = harness.execute_slash_with_transfer_outcome("v1", false);

    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("transfer failed"));
    assert_eq!(harness.bond("v1"), pre_bond);
    assert_eq!(harness.is_active("v1"), pre_active);
    assert_eq!(harness.coop_vault(), pre_coop);
    assert_eq!(harness.pos_state.slashed, pre_slashed);
}
