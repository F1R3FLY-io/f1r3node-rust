// Property-based test for T-AuthCheck (system auth-token guard).
//
// Theorem: T-AuthCheck — Rholang-level observation at
// PoS.rhox:437-439 (`sysAuthTokenOps!("check", sysAuthToken,
// *isValidTokenCh)`).
// Reference: docs/theory/slashing/slashing-specification.md §6.7,
// design/06-proposing-and-effect.md §6.7.
//
// Property: a slash deploy with a spoofed/invalid system auth token
// is rejected at the first guard with `(false, "Invalid system auth
// token")` and produces no state change. A valid token produces the
// normal slash outcome.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_auth_check_invalid_token_rejected(
        validator_count in 1usize..16,
        stake in 1i64..1_000_000,
        target_idx in 0usize..16,
    ) {
        let n = validator_count;
        let target = format!("v{}", target_idx % n);
        let mut harness = SlashingTestHarness::new(n, stake);
        let pre_bond = harness.bond(&target);
        let pre_active = harness.is_active(&target);
        let pre_coop = harness.coop_vault();

        let result = harness.execute_slash_with_auth(&target, false);

        // T-AuthCheck: invalid token → deterministic rejection.
        prop_assert!(!result.success);
        prop_assert_eq!(result.error.as_deref(), Some("Invalid system auth token"));

        // No state change.
        prop_assert_eq!(harness.bond(&target), pre_bond);
        prop_assert_eq!(harness.is_active(&target), pre_active);
        prop_assert_eq!(harness.coop_vault(), pre_coop);
        prop_assert!(!harness.pos_state.slashed.contains(&target));
    }

    #[test]
    fn t_auth_check_valid_token_proceeds(
        validator_count in 1usize..16,
        stake in 1i64..1_000_000,
        target_idx in 0usize..16,
    ) {
        let n = validator_count;
        let target = format!("v{}", target_idx % n);
        let mut harness = SlashingTestHarness::new(n, stake);
        let pre_bond = harness.bond(&target);
        let pre_coop = harness.coop_vault();

        let result = harness.execute_slash_with_auth(&target, true);

        prop_assert!(result.success);
        prop_assert_eq!(harness.bond(&target), 0);
        prop_assert_eq!(harness.coop_vault(), pre_coop + pre_bond);
    }
}
