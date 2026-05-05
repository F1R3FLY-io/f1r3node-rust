// Property-based test for T-5 (record witness-set monotonicity).
//
// Theorem: T-5 (`record_monotone`,
// formal/rocq/slashing/theories/EquivocationRecord.v).
// Reference: docs/theory/slashing/slashing-specification.md §4
// (Theorem 4.5), design/05-storage-and-records.md.
//
// Property: the witness set of an EquivocationRecord is monotone
// under dispatch — successive equivocations at the same base_seq
// only ever grow the witness set. Witnesses never disappear.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_5_witnesses_grow_monotonically(
        seq in 1u64..10,
        steps in 1usize..6,
    ) {
        let mut harness = SlashingTestHarness::new(3, 100);
        let _ = harness.sign_block("v0", seq);

        let mut last_size = 0usize;
        for _ in 0..steps {
            let bad = harness.sign_block_distinct("v0", seq);
            let _ = harness.dispatch(bad);
            let now = harness.record_witnesses("v0", seq.saturating_sub(1));
            // T-5: witness set never shrinks.
            prop_assert!(now.len() >= last_size,
                "witnesses grew from {} to {}", last_size, now.len());
            last_size = now.len();
        }
    }
}
