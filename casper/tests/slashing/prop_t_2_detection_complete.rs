// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-2 (detection completeness).
//
// Theorem: T-2 (`detection_complete`,
// formal/rocq/slashing/theories/EquivocationDetector.v:111).
// Reference: docs/theory/slashing/slashing-specification.md §4
// (Theorem 4.2).
//
// Property: for every DAG state where two distinct blocks share the
// same `(sender, seq)`, dispatching either block classifies as one
// of the two equivocation variants — never `Valid`. This is the
// completeness counterpart of T-1's soundness.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::{base_seq_from_seq, Status};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_2_every_equivocation_classifies_as_equivocation(
        validator_count in 1usize..8,
        equivocator_idx in 0usize..8,
        seq in 1u64..20,
    ) {
        let n = validator_count;
        let equivocator = format!("v{}", equivocator_idx % n);
        let mut harness = SlashingTestHarness::new(n, 100);

        // Inject the equivocation: two blocks at (equivocator, seq).
        let _b1 = harness.sign_block(&equivocator, seq);
        let b2 = harness.sign_block_distinct(&equivocator, seq);

        // T-2: dispatching the second block must classify as Admissible
        // or Ignorable — never Valid (and never JustificationRegression
        // for this trivial scenario without prior self-cite chain).
        let status = harness.dispatch(b2);
        prop_assert!(
            matches!(status, Status::AdmissibleEquivocation | Status::IgnorableEquivocation),
            "T-2: equivocation must classify as an equivocation variant, got {:?}", status
        );
        let base = base_seq_from_seq(seq).expect("generated seq is positive");
        prop_assert!(harness.has_record(&equivocator, base),
            "T-2 + post-fix #1/#3: detection plus dispatch always mints a record");
    }
}
