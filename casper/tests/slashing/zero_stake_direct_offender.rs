// UC-56 — Zero-stake / stale direct offender cannot seed closure
// after current-bonded filtering.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-56.
// Theorems: T-9.5 (active_implies_bonded),
// T-12 weighted (`zero_stake_not_direct_offender_under_bonded_precondition`).
// Reference: formal/sage/slashing/FINDINGS.md row 3.
//
// Property: a validator with zero stake cannot serve as a direct
// equivocator in the slashing-evidence domain — the bond contract
// rejects the bond at amount=0 (post-fix #5), so a zero-stake
// validator is unreachable as an active validator.

use super::harness::SlashingTestHarness;

#[test]
fn uc_56_zero_stake_cannot_become_direct_offender() {
    let mut harness = SlashingTestHarness::new(0, 0);

    // Bond two validators with positive stake.
    assert!(harness.try_bond("v0", 100).is_ok());
    assert!(harness.try_bond("v1", 100).is_ok());

    // Attempt to bond a third with zero stake — rejected.
    assert!(harness.try_bond("v_zero", 0).is_err());

    // The active set excludes zero-stake validators (T-9.5
    // active_implies_bonded invariant).
    assert!(!harness.is_active("v_zero"));
    for v in &harness.pos_state.active {
        assert!(harness.bond(v) > 0,
            "active_implies_bonded: {} has positive bond", v);
    }
}

#[test]
fn uc_56_negative_stake_also_rejected() {
    let mut harness = SlashingTestHarness::new(0, 0);
    assert!(harness.try_bond("v_neg", -1).is_err());
    assert!(!harness.is_active("v_neg"));
}
