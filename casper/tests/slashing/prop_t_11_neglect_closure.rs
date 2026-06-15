// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-11 (level-2 termination).
//
// Theorem: T-11 (`level_2_termination`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: docs/theory/slashing/slashing-specification.md §8
// (Theorem 8.1), design/08-two-level-and-collusion.md.
//
// Property: the slash closure terminates — applying the slash
// transition to every validator in the level-1 ∪ level-2 closure
// produces a stable state in finite steps. In the harness's
// projection, closure size is bounded by the number of distinct
// validators with EquivocationRecords.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_11_closure_size_bounded_by_validator_count(
        validator_count in 2usize..16,
        equivocator_count in 0usize..16,
    ) {
        let n = validator_count;
        let k = equivocator_count.min(n);
        let mut harness = SlashingTestHarness::new(n, 100);

        // Inject k equivocations.
        for i in 0..k {
            let v = format!("v{}", i);
            let _b1 = harness.sign_block(&v, 5);
            let bad = harness.sign_block_distinct(&v, 5);
            let _ = harness.dispatch(bad);
        }

        // The level-1 closure equals the set of validators with records.
        // Apply the slash transition to each.
        for i in 0..k {
            let v = format!("v{}", i);
            if harness.has_record(&v, 4) {
                let r = harness.execute_slash(&v);
                prop_assert!(r.success);
            }
        }

        // T-11: the closure terminates — no further slashes are needed.
        // Concretely, the slashed-set size equals exactly k (the number
        // of equivocators), bounded above by n.
        prop_assert!(harness.pos_state.slashed.len() <= n);
        prop_assert_eq!(harness.pos_state.slashed.len(), k);
    }
}
