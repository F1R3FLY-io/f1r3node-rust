// Property-based test for T-9.4 (transfer-failure deterministic
// finite-time termination).
//
// Theorem: T-9.4 (`t_9_4_transfer_failure_safety`,
// formal/rocq/slashing/theories/BugFixTransferFailure.v).
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.5.
//
// Property: for every PoS state and every transfer outcome, the
// slash transition reaches a deterministic finite-time conclusion
// — either succeeds with bond=0 (T-7 + T-8 path) or returns
// `(false, "transfer failed")` with state unchanged. The pre-fix
// path could hang indefinitely on transfer failure; the post-fix
// guarantees termination.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_4_either_succeeds_with_zero_bond_or_unchanged(
        validator_count in 1usize..16,
        stake in 1i64..1_000_000,
        target_idx in 0usize..16,
        transfer_succeeded in any::<bool>(),
    ) {
        let n = validator_count;
        let target = format!("v{}", target_idx % n);
        let mut harness = SlashingTestHarness::new(n, stake);

        let pre_bond = harness.bond(&target);
        let pre_active = harness.is_active(&target);
        let pre_coop = harness.coop_vault();
        let pre_slashed = harness.pos_state.slashed.contains(&target);

        let result = harness.execute_slash_with_transfer_outcome(
            &target, transfer_succeeded,
        );

        if transfer_succeeded {
            // Success branch (T-7 + T-8): bond zeroed, vault grew,
            // active false.
            prop_assert!(result.success);
            prop_assert_eq!(harness.bond(&target), 0);
            prop_assert_eq!(harness.coop_vault(), pre_coop + pre_bond);
            prop_assert!(!harness.is_active(&target));
            prop_assert!(harness.pos_state.slashed.contains(&target));
        } else {
            // Failure branch (T-9.4): everything unchanged.
            prop_assert!(!result.success);
            prop_assert_eq!(result.error.as_deref(), Some("transfer failed"));
            prop_assert_eq!(harness.bond(&target), pre_bond);
            prop_assert_eq!(harness.is_active(&target), pre_active);
            prop_assert_eq!(harness.coop_vault(), pre_coop);
            prop_assert_eq!(
                harness.pos_state.slashed.contains(&target),
                pre_slashed
            );
        }
    }
}
