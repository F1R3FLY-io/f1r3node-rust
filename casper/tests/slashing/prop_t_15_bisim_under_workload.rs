// Property-based test for T-15 (bisimilarity) under randomized
// operation sequences.
//
// Theorem: T-15 (`bisimulation`,
// formal/rocq/slashing/theories/Bisimulation.v).
// Reference: docs/theory/slashing/slashing-specification.md §10
// (Theorem 10.1), design/10-bisimilarity.md.
//
// Property: for *any* sequence of harness operations, the harness's
// post-state equals the oracle's post-state up to the projection
// (DagState, EqRecordSet, PoSState). This is the strong-bisim
// version of UC-39 (which tests single operations in isolation).

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::oracle::{oracle_dispatch, oracle_slash};
use super::types::{DagState, EqRecordSet, PoSState};

#[derive(Debug, Clone)]
enum Op {
    Equivocate { validator_idx: usize, seq: u64 },
    Slash { target_idx: usize },
}

fn gen_op(n: usize) -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..n, 0u64..16).prop_map(|(v, s)| Op::Equivocate {
            validator_idx: v,
            seq: s
        }),
        (0..n).prop_map(|t| Op::Slash { target_idx: t }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_15_harness_equals_oracle_under_random_workload(
        n in 2usize..6,
        ops in proptest::collection::vec(gen_op(6), 0..20),
    ) {
        let mut harness = SlashingTestHarness::new(n, 100);
        let mut oracle_dag = DagState::default();
        let mut oracle_tracker = EqRecordSet::default();
        let mut oracle_pos = harness.pos_state.clone();

        for (i, op) in ops.iter().enumerate() {
            match op {
                Op::Equivocate { validator_idx, seq } => {
                    let v = format!("v{}", validator_idx % n);
                    let _ = harness.sign_block(&v, *seq);
                    let bad = harness.sign_block_distinct(&v, *seq);

                    // Mirror the sign_block sequence in the oracle's DAG by
                    // copying the harness's pre-dispatch DAG state.
                    oracle_dag = harness.dag.clone();

                    // Both run dispatch on the same hash with the same
                    // forced classification (taken from the harness's
                    // detect for fairness).
                    let h_status = harness.detect(bad);
                    let (new_dag, new_tracker) = oracle_dispatch(
                        &oracle_dag, &oracle_tracker, bad, &h_status,
                    );
                    oracle_dag = new_dag;
                    oracle_tracker = new_tracker;
                    let _ = harness.dispatch_with_status(bad, h_status);
                }
                Op::Slash { target_idx } => {
                    let v = format!("v{}", target_idx % n);
                    let _ = harness.execute_slash(&v);
                    let (new_pos, _) = oracle_slash(&oracle_pos, &v);
                    oracle_pos = new_pos;
                }
            }

            // Pointwise bisim invariants after each operation:
            //   bond, active, slashed, coop_vault.
            for j in 0..n {
                let v = format!("v{}", j);
                prop_assert_eq!(harness.bond(&v), oracle_pos.bond(&v),
                    "step {}: bond mismatch on {}", i, v);
                prop_assert_eq!(harness.is_active(&v), oracle_pos.is_active(&v),
                    "step {}: active mismatch on {}", i, v);
                prop_assert_eq!(
                    harness.pos_state.slashed.contains(&v),
                    oracle_pos.slashed.contains(&v),
                    "step {}: slashed mismatch on {}", i, v
                );
            }
            prop_assert_eq!(harness.coop_vault(), oracle_pos.coop_vault,
                "step {}: coop_vault mismatch", i);

            // Tracker keys match.
            let h_keys: std::collections::BTreeSet<_> = harness.tracker.records.keys().collect();
            let o_keys: std::collections::BTreeSet<_> = oracle_tracker.records.keys().collect();
            prop_assert_eq!(h_keys, o_keys, "step {}: tracker key sets diverge", i);
        }
    }
}
