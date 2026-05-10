// Property-based test for T-13a (bonds-map bisimulation).
//
// Theorem: T-13a (`bonds_bisim`,
// formal/rocq/slashing/theories/Bisimulation.v).
// Reference: docs/theory/slashing/slashing-specification.md §10
// (Theorem 10.2a), design/10-bisimilarity.md §10.4.
//
// Property: under any sequence of slash events, the bonds-map
// projections of the harness and the oracle agree pointwise.
// This is the bonds-component of the strong bisim relation T-15;
// T-13a names it explicitly.

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
    fn t_13a_bonds_agree_after_slash_sequence(
        validator_count in 1usize..8,
        stake in 1i64..1_000_000,
        targets in proptest::collection::vec(0usize..8, 0..10),
    ) {
        let n = validator_count;
        let mut harness = SlashingTestHarness::new(n, stake);
        let mut oracle = RocqOracleAdapter::new(n, stake);

        for t in &targets {
            let target = format!("v{}", t % n);
            let _ = harness.execute_slash(&target);
            let _ = oracle.execute_slash(&target);
        }

        // T-13a: bonds_map projections agree pointwise.
        for i in 0..n {
            let v = format!("v{}", i);
            prop_assert_eq!(
                <SlashingTestHarness as SlashingObserver>::bond(&harness, &v),
                <RocqOracleAdapter as SlashingObserver>::bond(&oracle, &v),
                "T-13a: bonds disagree on {}", v
            );
        }
    }
}
