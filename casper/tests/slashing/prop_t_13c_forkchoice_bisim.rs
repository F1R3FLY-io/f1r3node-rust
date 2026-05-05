// Property-based test for T-13c (fork-choice bisimulation).
//
// Theorem: T-13c (`forkchoice_bisim`,
// formal/rocq/slashing/theories/Bisimulation.v).
// Reference: docs/theory/slashing/slashing-specification.md §10
// (Theorem 10.2c).
//
// Property: under any sequence of slash events, the fork-choice
// projections of the harness and the oracle agree as sorted
// sequences. Fork-choice in both tiers is "active set sorted"
// minus those whose latest message is invalid; for the harness
// this is just the active set (the projection skips the invalid-
// latest-message filtering for simplicity — production handles
// it through the GHOST estimator).

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::observer::SlashingObserver;
use super::oracle_adapter::RocqOracleAdapter;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_13c_forkchoice_agrees_after_slash_sequence(
        validator_count in 1usize..8,
        targets in proptest::collection::vec(0usize..8, 0..10),
    ) {
        let n = validator_count;
        let mut harness = SlashingTestHarness::new(n, 100);
        let mut oracle = RocqOracleAdapter::new(n, 100);

        for t in &targets {
            let target = format!("v{}", t % n);
            let _ = harness.execute_slash(&target);
            let _ = oracle.execute_slash(&target);
        }

        // T-13c: fork_choice agrees pointwise.
        prop_assert_eq!(
            <SlashingTestHarness as SlashingObserver>::fork_choice(&harness),
            <RocqOracleAdapter as SlashingObserver>::fork_choice(&oracle),
            "T-13c: fork-choice projections disagree"
        );
    }
}
