// Property-based test for T-13b (records bisimulation, mod
// iteration order).
//
// Theorem: T-13b (`records_bisim`,
// formal/rocq/slashing/theories/Bisimulation.v).
// Reference: docs/theory/slashing/slashing-specification.md §10
// (Theorem 10.2b).
//
// Property: under any sequence of dispatch events, the
// equivocation-record set projections of the harness and the
// oracle agree as sets (mutual containment of keys, agreement on
// witness sets).

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::observer::SlashingObserver;
use super::oracle_adapter::RocqOracleAdapter;
use super::types::{base_seq_from_seq, BlockMeta};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_13b_records_agree_after_dispatch_sequence(
        validator_count in 2usize..6,
        events in proptest::collection::vec((0usize..6, 1u64..10), 1..8),
    ) {
        let n = validator_count;
        let mut harness = SlashingTestHarness::new(n, 100);
        let mut oracle = RocqOracleAdapter::new(n, 100);

        for (v_idx, seq) in &events {
            let v = format!("v{}", v_idx % n);
            // Inject one equivocation per event into both tiers.
            let _b = harness.sign_block(&v, *seq);
            let bad = harness.sign_block_distinct(&v, *seq);
            // Mirror in the oracle: insert the bad block, then
            // dispatch with the same status the harness derives.
            let detected = harness.detect(bad);
            oracle.insert_block(BlockMeta {
                hash: bad,
                sender: v.clone(),
                seq: *seq,
                justifications: vec![],
                slash_targets: vec![],
            });
            let _ = harness.dispatch(bad);
            oracle.dispatch_with_status(bad, detected.clone());
        }

        // T-13b: records agree as sets keyed by (validator, base_seq).
        for (v_idx, seq) in &events {
            let v = format!("v{}", v_idx % n);
            let Some(base) = base_seq_from_seq(*seq) else {
                continue;
            };
            prop_assert_eq!(
                <SlashingTestHarness as SlashingObserver>::has_record(&harness, &v, base),
                <RocqOracleAdapter as SlashingObserver>::has_record(&oracle, &v, base),
                "T-13b: record presence disagrees on ({}, {})", v, base
            );
            prop_assert_eq!(
                <SlashingTestHarness as SlashingObserver>::record_witnesses(&harness, &v, base),
                <RocqOracleAdapter as SlashingObserver>::record_witnesses(&oracle, &v, base),
                "T-13b: witness sets disagree on ({}, {})", v, base
            );
        }
    }
}
