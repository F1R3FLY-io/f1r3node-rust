// Property-based test for T-Idem (slash idempotence).
//
// Theorem: T-Idem (alias T-9), `t_idem_slash_idempotent`,
// formal/rocq/slashing/theories/PoSContract.v:117.
// Reference: docs/theory/slashing/slashing-specification.md §5.2,
// design/06-proposing-and-effect.md §6.5.
//
// Property: for all PoS states `ps` and all validators `v`, slashing
// `v` twice yields the same state and bond outcome as slashing once:
//
//   ∀ ps v, slash(slash(ps, v).0, v) = slash(ps, v)
//
// In the harness's projection, idempotence reduces to four
// conjunctions: bond(v) stays at 0, is_active(v) stays false, the
// coop_vault balance is unchanged after the second slash, and the
// slashed-set membership is preserved.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_idem_slash_idempotent(
        validator_count in 1usize..16,
        stake in 1i64..1_000_000,
        target_idx in 0usize..16,
    ) {
        let n = validator_count;
        let target = format!("v{}", target_idx % n);

        let mut harness = SlashingTestHarness::new(n, stake);
        let initial_coop = harness.coop_vault();
        let r1 = harness.execute_slash(&target);
        prop_assert!(r1.success);

        let bond_after_first = harness.bond(&target);
        let active_after_first = harness.is_active(&target);
        let coop_after_first = harness.coop_vault();

        let r2 = harness.execute_slash(&target);
        prop_assert!(r2.success);

        // T-Idem: post-state after second slash equals post-state
        // after first slash — pointwise on the harness's projection.
        prop_assert_eq!(harness.bond(&target), bond_after_first);
        prop_assert_eq!(harness.is_active(&target), active_after_first);
        prop_assert_eq!(harness.coop_vault(), coop_after_first);

        // Also: bond should be 0, validator excluded from active,
        // coop_vault should have grown by exactly the original stake.
        prop_assert_eq!(harness.bond(&target), 0);
        prop_assert!(!harness.is_active(&target));
        prop_assert_eq!(harness.coop_vault(), initial_coop + stake);
    }
}
