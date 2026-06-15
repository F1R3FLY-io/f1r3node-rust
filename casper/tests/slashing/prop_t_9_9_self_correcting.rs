// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-9.9 (self-correcting block admitted).
//
// Theorem: T-9.9 (`t_9_9_self_correcting_admitted`,
// formal/rocq/slashing/theories/MainTheorem.v).
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.10.
//
// Property: a block that cites an invalid block AND issues a
// SlashDeploy targeting that block's sender is admitted as Valid
// (post-fix #9). The honest slasher does not get a record.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::{base_seq_from_seq, Status};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_9_9_self_correcting_block_admitted(
        validator_count in 2usize..6,
        offender_idx in 0usize..6,
        slasher_idx in 0usize..6,
        offender_seq in 1u64..10,
        slasher_seq in 1u64..20,
    ) {
        let n = validator_count;
        let offender = format!("v{}", offender_idx % n);
        let slasher = format!("v{}", slasher_idx % n);
        prop_assume!(offender != slasher);

        let mut harness = SlashingTestHarness::new(n, 100);

        // Offender equivocates → record minted.
        let _v0a = harness.sign_block(&offender, offender_seq);
        let bad = harness.sign_block_distinct(&offender, offender_seq);
        let _ = harness.dispatch(bad);

        // Slasher publishes a self-correcting block.
        let correcting = harness.sign_block_citing_with_slash(
            &slasher, slasher_seq, bad, &offender,
        );
        let s = harness.dispatch(correcting);

        // T-9.9: self-correcting block is admitted as Valid.
        prop_assert_eq!(s, Status::Valid,
            "T-9.9: self-correcting block (cite + slash) is admitted");
        let slasher_base = base_seq_from_seq(slasher_seq).expect("generated slasher_seq is positive");
        prop_assert!(!harness.has_record(&slasher, slasher_base),
            "T-9.9: honest slasher does NOT get a record");
        prop_assert!(!harness.dag.invalid.contains(&correcting),
            "T-9.9: self-correcting block is NOT in invalid index");
    }
}
