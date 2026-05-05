// Property-based test for T-6 (detect_neglected soundness +
// completeness).
//
// Theorem: T-6 (`detect_neglected`,
// formal/rocq/slashing/theories/EquivocationDetector.v).
// Reference: docs/theory/slashing/slashing-specification.md §4
// (Theorem 4.6), design/04-detection-and-pipeline.md §4.4.
//
// Two complementary properties:
//   • Soundness (no false positive): a block whose justifications
//     do not cite any validator with an outstanding record never
//     classifies as NeglectedEquivocation.
//   • Completeness (no false negative): a block that cites an
//     equivocator without slashing always classifies as
//     NeglectedEquivocation.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::Status;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    /// Soundness — no false positive neglect classification.
    #[test]
    fn t_6_neglect_soundness_no_false_positive(
        validator_count in 2usize..6,
        seq in 1u64..10,
    ) {
        let mut harness = SlashingTestHarness::new(validator_count, 100);
        // No equivocations injected — no validator has a record.
        let block = harness.sign_block("v1", seq);
        let s = harness.dispatch(block);
        prop_assert_ne!(s, Status::NeglectedEquivocation,
            "T-6 soundness: block in honest history must not classify Neglected");
    }

    /// Completeness — every cite-without-slash triggers the rule.
    #[test]
    fn t_6_neglect_completeness_cite_without_slash(
        validator_count in 2usize..6,
        equiv_seq in 1u64..10,
        cite_seq in 1u64..10,
    ) {
        let n = validator_count;
        let mut harness = SlashingTestHarness::new(n, 100);

        // v0 equivocates → record minted.
        let _v0a = harness.sign_block("v0", equiv_seq);
        let bad = harness.sign_block_distinct("v0", equiv_seq);
        let _ = harness.dispatch(bad);
        prop_assert!(harness.has_record("v0", equiv_seq.saturating_sub(1)));

        // v1 cites v0's bad block without slashing.
        let citing = harness.sign_block_citing("v1", cite_seq, bad);
        let s = harness.dispatch(citing);
        prop_assert_eq!(s, Status::NeglectedEquivocation,
            "T-6 completeness: cite-without-slash always triggers Neglect");
    }
}
