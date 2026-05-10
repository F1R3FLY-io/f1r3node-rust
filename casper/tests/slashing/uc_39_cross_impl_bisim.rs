// UC-39 — Cross-implementation bisimilarity check.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-39.
// Theorem: T-15 (`bisimulation`,
// formal/rocq/slashing/theories/Bisimulation.v).
// Reference: docs/theory/slashing/design/14-test-plan.md §14.5,
// design/10-bisimilarity.md.
//
// For every harness operation, run the same operation through the
// oracle (Rust mirror of the Rocq definitions) and assert the
// projected post-state matches. A discrepancy indicates either
// (a) a bug in the harness, (b) a bug in the oracle, or (c) a
// bisimilarity violation in the canonical model — any of which
// invalidate T-15.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::oracle::{oracle_detect, oracle_dispatch, oracle_slash};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    /// Harness::detect(hash) ≡ oracle_detect(dag, hash) for every
    /// hash in the DAG.
    #[test]
    fn detect_bisim(
        validator_count in 1usize..6,
        depth in 1u64..6,
    ) {
        let mut harness = SlashingTestHarness::new(validator_count, 100);
        for seq in 0..depth {
            for i in 0..validator_count {
                harness.sign_block(&format!("v{}", i), seq);
            }
        }
        // Inject one equivocation.
        if validator_count > 0 {
            harness.sign_block_distinct("v0", 0);
        }
        let hashes: Vec<u64> = harness.dag.blocks.keys().copied().collect();
        for hash in &hashes {
            let h_status = harness.detect(*hash);
            let o_status = oracle_detect(&harness.dag, *hash);
            prop_assert_eq!(h_status, o_status,
                "detect bisim failed at hash {}: harness vs oracle disagreed", hash);
        }
    }

    /// Harness::dispatch(hash) ≡ oracle_dispatch(dag, tracker, hash, ...)
    /// up to the projected (DagState, EqRecordSet) tuple.
    #[test]
    fn dispatch_bisim(
        validator_count in 1usize..6,
        equivocator_idx in 0usize..6,
        seq in 1u64..10,
    ) {
        let n = validator_count;
        let equivocator = format!("v{}", equivocator_idx % n);
        let mut harness = SlashingTestHarness::new(n, 100);
        let _b1 = harness.sign_block(&equivocator, seq);
        let bad = harness.sign_block_distinct(&equivocator, seq);

        // Run dispatch through the oracle starting from the harness's
        // pre-dispatch state.
        let pre_dag = harness.dag.clone();
        let pre_tracker = harness.tracker.clone();

        let h_status = harness.dispatch(bad);
        let (oracle_dag, oracle_tracker) = oracle_dispatch(
            &pre_dag, &pre_tracker, bad, &h_status,
        );

        // Bisim: invalid index, tracker contents, and witness sets all
        // match.
        prop_assert_eq!(harness.dag.invalid.contains(&bad),
            oracle_dag.invalid.contains(&bad));
        prop_assert_eq!(
            harness.tracker.contains(&equivocator, seq.saturating_sub(1)),
            oracle_tracker.contains(&equivocator, seq.saturating_sub(1))
        );
        prop_assert_eq!(
            harness.tracker.witnesses(&equivocator, seq.saturating_sub(1)),
            oracle_tracker.witnesses(&equivocator, seq.saturating_sub(1))
        );
    }

    /// Harness::execute_slash(target) ≡ oracle_slash(pos_state, target).
    #[test]
    fn slash_bisim(
        validator_count in 1usize..16,
        stake in 1i64..1_000_000,
        target_idx in 0usize..16,
    ) {
        let n = validator_count;
        let target = format!("v{}", target_idx % n);

        let mut harness = SlashingTestHarness::new(n, stake);
        let pre_pos = harness.pos_state.clone();
        let _ = harness.execute_slash(&target);

        let (oracle_post, _) = oracle_slash(&pre_pos, &target);

        prop_assert_eq!(harness.bond(&target), oracle_post.bond(&target));
        prop_assert_eq!(harness.is_active(&target), oracle_post.is_active(&target));
        prop_assert_eq!(harness.coop_vault(), oracle_post.coop_vault);
        prop_assert_eq!(
            harness.pos_state.slashed.contains(&target),
            oracle_post.slashed.contains(&target)
        );
    }
}
