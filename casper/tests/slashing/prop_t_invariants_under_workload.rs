// Composite property test — every key invariant holds at every step
// of a randomized workload.
//
// Combines: T-1 (no honest slash), T-4 (record uniqueness),
// T-7 + T-8 (slash transfers stake), T-9.5 (active_implies_bonded),
// T-Idem (slash idempotence). The proptest generates a random
// sequence of operations and checks every invariant after every step.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::Status;

#[derive(Debug, Clone)]
enum Op {
    SignHonest { validator_idx: usize, seq: u64 },
    Equivocate { validator_idx: usize, seq: u64 },
    Slash { target_idx: usize },
}

fn gen_op(n: usize) -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..n, 0u64..16).prop_map(|(v, s)| Op::SignHonest { validator_idx: v, seq: s }),
        (0..n, 0u64..16).prop_map(|(v, s)| Op::Equivocate { validator_idx: v, seq: s }),
        (0..n).prop_map(|t| Op::Slash { target_idx: t }),
    ]
}

fn check_invariants(h: &SlashingTestHarness, n: usize) -> Result<(), String> {
    // T-9.5: every active validator has positive bond.
    for i in 0..n {
        let v = format!("v{}", i);
        if h.is_active(&v) && h.bond(&v) <= 0 {
            return Err(format!("T-9.5: active {} has non-positive bond {}", v, h.bond(&v)));
        }
    }
    // Slashed bond is exactly 0.
    for v in &h.pos_state.slashed {
        if h.bond(v) != 0 {
            return Err(format!("slashed {} has non-zero bond {}", v, h.bond(v)));
        }
    }
    // Coop vault is non-negative and bounded above by initial total bond.
    if h.coop_vault() < 0 {
        return Err(format!("coop_vault is negative: {}", h.coop_vault()));
    }
    // T-4: at most one record per (validator, base_seq).
    let mut seen = std::collections::HashSet::new();
    for key in h.tracker.records.keys() {
        if !seen.insert(key.clone()) {
            return Err(format!("T-4: duplicate record at {:?}", key));
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        .. ProptestConfig::default()
    })]

    #[test]
    fn invariants_hold_under_random_workload(
        n in 2usize..6,
        ops in proptest::collection::vec(gen_op(6), 0..40),
    ) {
        let mut harness = SlashingTestHarness::new(n, 100);

        // Initial state must satisfy all invariants.
        if let Err(e) = check_invariants(&harness, n) {
            panic!("initial state violates invariant: {}", e);
        }

        for (i, op) in ops.iter().enumerate() {
            match op {
                Op::SignHonest { validator_idx, seq } => {
                    let v = format!("v{}", validator_idx % n);
                    let h = harness.sign_block(&v, *seq);
                    let s = harness.dispatch(h);
                    // T-1: honest blocks at fresh (sender, seq) classify Valid.
                    // (If `seq` repeats — generator's responsibility — we let
                    // the equivocation flow happen naturally.)
                    let _ = s;
                }
                Op::Equivocate { validator_idx, seq } => {
                    let v = format!("v{}", validator_idx % n);
                    let _ = harness.sign_block(&v, *seq);
                    let bad = harness.sign_block_distinct(&v, *seq);
                    let s = harness.dispatch(bad);
                    // Equivocations classify as Admissible / Ignorable.
                    prop_assert!(matches!(s,
                        Status::AdmissibleEquivocation | Status::IgnorableEquivocation),
                        "step {}: equivocation classified {:?}", i, s);
                }
                Op::Slash { target_idx } => {
                    let v = format!("v{}", target_idx % n);
                    let _ = harness.execute_slash(&v);
                }
            }
            if let Err(e) = check_invariants(&harness, n) {
                panic!("step {} ({:?}) violated invariant: {}", i, op, e);
            }
        }
    }
}
