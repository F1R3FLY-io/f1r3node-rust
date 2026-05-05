// Property-based test for T-4 (record uniqueness).
//
// Theorem: T-4 (`record_uniqueness`,
// formal/rocq/slashing/theories/EquivocationRecord.v).
// Reference: docs/theory/slashing/slashing-specification.md §4
// (Theorem 4.4), design/05-storage-and-records.md.
//
// Property: there is at most one EquivocationRecord per
// `(validator, base_seq)` pair. Repeated equivocations at the same
// base_seq merge into a single record's witness set; the tracker
// never grows two distinct records keyed by the same pair.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_4_at_most_one_record_per_validator_seq_pair(
        n in 1usize..6,
        seq in 1u64..10,
        repetitions in 1usize..6,
    ) {
        let mut harness = SlashingTestHarness::new(n, 100);
        let _ = harness.sign_block("v0", seq);

        // Inject `repetitions` equivocations at (v0, seq).
        let mut witnesses = Vec::new();
        for _ in 0..repetitions {
            let bad = harness.sign_block_distinct("v0", seq);
            witnesses.push(bad);
            let _ = harness.dispatch(bad);
        }

        // T-4: there is exactly one record at (v0, base = seq-1).
        let key = ("v0".to_string(), seq.saturating_sub(1));
        let count = harness.tracker.records.iter().filter(|(k, _)| **k == key).count();
        prop_assert_eq!(count, 1, "exactly one record at {:?}", key);

        // Every injected witness ends up in the single record's set
        // (record-uniqueness AND witness-monotonicity).
        let actual = harness.record_witnesses("v0", seq.saturating_sub(1));
        for w in &witnesses {
            prop_assert!(actual.contains(w), "witness {} merged into the record", w);
        }
    }
}
