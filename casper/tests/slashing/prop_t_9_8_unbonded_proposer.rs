// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-9.8 (unbonded proposer emits empty
// slash list).
//
// Theorem: T-9.8 (`t_9_8_unbonded_proposer_no_slash`,
// formal/rocq/slashing/theories/BugFixUnbondedProposer.v).
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.9.
//
// Property: for every proposer with bond ≤ 0,
// `simulate_slash_proposal(proposer)` returns an empty list,
// regardless of how many EquivocationRecords are outstanding.
// Conversely, a positively-bonded proposer with k outstanding
// records emits k slash targets.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_8_unbonded_emits_empty(
        validator_count in 2usize..8,
        equivocator_count in 0usize..8,
    ) {
        let n = validator_count;
        let k = equivocator_count.min(n);
        let mut harness = SlashingTestHarness::new(n, 100);

        // Inject k equivocations.
        for i in 0..k {
            let v = format!("v{}", i);
            let _b = harness.sign_block(&v, 5);
            let bad = harness.sign_block_distinct(&v, 5);
            let _ = harness.dispatch(bad);
        }

        // For every validator, check the proposer-bond gate.
        for j in 0..n {
            let proposer = format!("v{}", j);
            let proposal = harness.simulate_slash_proposal(&proposer);
            let bond = harness.bond(&proposer);
            if bond <= 0 {
                prop_assert!(proposal.is_empty(),
                    "T-9.8: unbonded proposer {} emits empty list", proposer);
            } else {
                prop_assert_eq!(proposal.len(), k,
                    "bonded proposer {} emits {} slashes (one per record)",
                    proposer, k);
            }
        }
    }

    #[test]
    fn t_9_8_unknown_validator_emits_empty(
        unknown_idx in 1000usize..2000,
    ) {
        let harness = SlashingTestHarness::new(3, 100);
        let unknown = format!("v{}", unknown_idx);
        let proposal = harness.simulate_slash_proposal(&unknown);
        prop_assert!(proposal.is_empty(),
            "unknown validator (bond defaults to 0) emits empty list");
    }
}
