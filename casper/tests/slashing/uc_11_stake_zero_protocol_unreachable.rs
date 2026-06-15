// UC-11 — Stake-0 bonded validator is unreachable post-fix #5.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-11.
// Theorem: T-9.5 (active_implies_bonded preserved).
// Reference: design/09-bug-fixes-and-rationale.md §9.6.
//
// Post-fix #5 places an `amount <= 0` rejection in the PoS bond
// contract, making stake-0 bonded validators an unreachable state.
// The harness's `try_bond` mirrors this: zero or negative amounts
// are rejected, and the active set never admits an unbonded
// validator.

use super::harness::SlashingTestHarness;

#[test]
fn uc_11_stake_zero_validator_cannot_bond() {
    let mut harness = SlashingTestHarness::new(0, 0);

    // Cannot bond at zero.
    assert!(harness.try_bond("v_attacker", 0).is_err());
    // Cannot bond at negative.
    assert!(harness.try_bond("v_attacker", -100).is_err());
    // Can bond at positive.
    assert!(harness.try_bond("v_honest", 100).is_ok());

    // T-9.5: active set contains only positive-bond validators.
    for v in &harness.pos_state.active {
        assert!(harness.bond(v) > 0);
    }
}

#[test]
fn uc_11_active_implies_bonded_after_slash() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Slash one validator → bond=0, removed from active.
    let _ = harness.execute_slash("v0");

    // T-9.5 invariant: every active validator still has bond > 0.
    for v in &harness.pos_state.active {
        assert!(harness.bond(v) > 0);
    }
    // Slashed validator is NOT active and has bond=0.
    assert!(!harness.is_active("v0"));
    assert_eq!(harness.bond("v0"), 0);
}
