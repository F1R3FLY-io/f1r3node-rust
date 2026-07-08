// UC-21 — System auth-token guard rejects spoofed slashes.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-21.
// Theorem: T-AuthCheck (system auth-token guard at PoS.rhox:437-439).
//
// Example trace mirroring the property-test version in
// `prop_t_auth_check.rs`. Asserts the deterministic rejection
// path under spoofed token.

use super::harness::SlashingTestHarness;

#[test]
fn uc_21_spoofed_token_rejects_with_deterministic_error() {
    let mut harness = SlashingTestHarness::new(3, 100);
    let pre_bond = harness.bond("v0");
    let pre_coop = harness.coop_vault();

    let result = harness.execute_slash_with_auth("v0", false);
    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("Invalid system auth token"));

    // No state change.
    assert_eq!(harness.bond("v0"), pre_bond);
    assert_eq!(harness.coop_vault(), pre_coop);
    assert!(harness.is_active("v0"));
}

#[test]
fn uc_21_valid_token_proceeds_normally() {
    let mut harness = SlashingTestHarness::new(3, 100);
    let result = harness.execute_slash_with_auth("v0", true);
    assert!(result.success);
    assert_eq!(harness.bond("v0"), 0);
    assert!(!harness.is_active("v0"));
    assert_eq!(harness.coop_vault(), 100);
}
