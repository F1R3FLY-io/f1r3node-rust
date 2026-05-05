// Property-based test for T-9.5 corollary: active_implies_bonded.
//
// Theorem: T-9.5 corollary (`t_9_5_active_has_positive_bond`,
// formal/rocq/slashing/theories/BugFixStakeZero.v:58).
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.6.
//
// Property: every validator in the active set has a strictly
// positive bond, AND this invariant is preserved by the slash
// transition (slashed validators leave the active set, so the
// remaining active validators still satisfy bond > 0).

use proptest::prelude::*;

use super::harness::SlashingTestHarness;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_5_active_implies_bonded_initial(
        validator_count in 1usize..16,
        stake in 1i64..1_000_000,
    ) {
        let harness = SlashingTestHarness::new(validator_count, stake);
        for i in 0..validator_count {
            let v = format!("v{}", i);
            if harness.is_active(&v) {
                prop_assert!(harness.bond(&v) > 0,
                    "active validator {} has positive bond", v);
            }
        }
    }

    #[test]
    fn t_9_5_active_implies_bonded_preserved_by_slash(
        validator_count in 2usize..16,
        stake in 1i64..1_000_000,
        target_idx in 0usize..16,
    ) {
        let n = validator_count;
        let target = format!("v{}", target_idx % n);
        let mut harness = SlashingTestHarness::new(n, stake);

        let _ = harness.execute_slash(&target);

        // Post-slash invariant: the target is no longer active, and
        // every remaining active validator still has a positive bond.
        prop_assert!(!harness.is_active(&target),
            "slashed target {} must leave active set", target);
        for i in 0..n {
            let v = format!("v{}", i);
            if harness.is_active(&v) {
                prop_assert!(harness.bond(&v) > 0,
                    "active validator {} still has positive bond after slash", v);
            }
        }
    }
}
