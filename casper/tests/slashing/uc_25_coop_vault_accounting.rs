// UC-25 — Successful slash transfers the entire prior bond to coop vault.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-25.
// Theorems: T-7 (bond accounting after slash).
//
// Scenario: validator v2 is slashed. Post-slash: v2's bond is exactly 0,
// the coop vault has increased by the prior bond (no rounding loss),
// v2 is no longer active, and other validators' bonds are untouched.
// The harness asserts conservation: bond(v2)_pre + coop_pre = bond(v2)_post + coop_post.

use super::harness::SlashingTestHarness;

#[test]
fn uc_25_slash_transfers_prior_bond_to_coop_vault() {
    let mut harness = SlashingTestHarness::new(3, 100);

    let pre_bond = harness.bond("v2");
    let pre_coop = harness.coop_vault();
    let result = harness.execute_slash("v2");

    assert!(result.success);
    assert_eq!(harness.bond("v2"), 0);
    assert_eq!(harness.coop_vault(), pre_coop + pre_bond);
    assert!(!harness.is_active("v2"));
    assert_eq!(harness.bond("v0"), 100);
    assert_eq!(harness.bond("v1"), 100);
}
