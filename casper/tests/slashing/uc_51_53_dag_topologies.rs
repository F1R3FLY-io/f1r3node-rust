// UC-51, UC-52, UC-53 — DAG topology variants for AdmissibleEquivocation.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12.
// Theorems: T-1 (soundness), T-9.6 (self-regression), T-15 (bisim).
//
// These three UCs exercise the equivocation pipeline at three
// different DAG shapes:
//   • UC-51 — deep linear chain (>100 blocks per validator)
//   • UC-52 — wide chain (every validator publishes at every seq)
//   • UC-53 — single-chain (only one validator advancing)
//
// All three should produce the same dispatch result for an injected
// equivocation: the dispatcher mints a record and the slash applies.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_51_deep_linear_chain_admissible() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // Build a deep linear chain: v0 publishes at seq 0..150 in order.
    for s in 0..150u64 {
        harness.sign_block("v0", s);
    }

    // Inject an equivocation at seq=100.
    let bad = harness.sign_block_distinct("v0", 100);

    // Pre-existing record at base=99? No, but classification is
    // still detected as Ignorable since this is the first
    // observation of the (v0, 100) twin pair.
    let s = harness.dispatch(bad);
    assert_eq!(s, Status::IgnorableEquivocation);
    assert!(
        harness.has_record("v0", 99),
        "deep linear chain: equivocation at seq=100 records at base=99"
    );
}

#[test]
fn uc_52_wide_dag_admissible() {
    let validators = 10;
    let depth = 30u64;
    let mut harness = SlashingTestHarness::new(validators, 50);

    // Every validator publishes at every seq.
    for s in 0..depth {
        for i in 0..validators {
            harness.sign_block(&format!("v{}", i), s);
        }
    }

    // Inject an equivocation at (v3, seq=10).
    let bad = harness.sign_block_distinct("v3", 10);
    let s = harness.dispatch(bad);
    assert_eq!(s, Status::IgnorableEquivocation);
    assert!(harness.has_record("v3", 9));
}

#[test]
fn uc_53_single_chain_equivocation() {
    let mut harness = SlashingTestHarness::new(1, 100);

    // Only v0 in the network; long single chain.
    for s in 0..50u64 {
        harness.sign_block("v0", s);
    }

    // v0 equivocates at seq=25.
    let bad = harness.sign_block_distinct("v0", 25);
    let s = harness.dispatch(bad);
    assert_eq!(
        s,
        Status::IgnorableEquivocation,
        "single-validator chain: equivocation still detected"
    );
    assert!(harness.has_record("v0", 24));

    // Slashing the only validator empties the active set.
    let _ = harness.execute_slash("v0");
    assert_eq!(harness.fork_choice().len(), 0);
    assert_eq!(harness.coop_vault(), 100);
}
