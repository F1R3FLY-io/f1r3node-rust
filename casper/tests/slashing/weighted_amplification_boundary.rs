// UC-70 — Weighted amplification outside closure bound.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-70.
// Theorem: T-12 weighted boundary (Sage finding 21 — "Weighted
// MIP optimization finds high amplification under a violated
// closure bound").
// Reference: formal/sage/slashing/FINDINGS.md row 21.
//
// Property: when the bounded-closure precondition of T-12 is
// VIOLATED, weighted-stake amplification is unbounded. This is a
// theorem-precondition witness — NOT a failure of T-12, but
// confirmation that T-12's hypothesis is load-bearing.
//
// Sage witness: n=4, stakes [3,3,1,1], fault=1, direct offender [2],
// edges 0→1→2, closure [0,1,2], extra slashed stake 6 for direct
// adversarial stake 1.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_70_amplification_outside_closure_bound() {
    // Reproduce the Sage witness: 4 validators with stakes [3,3,1,1].
    let mut harness = SlashingTestHarness::new(0, 0);
    assert!(harness.try_bond("v0", 3).is_ok());
    assert!(harness.try_bond("v1", 3).is_ok());
    assert!(harness.try_bond("v2", 1).is_ok());
    assert!(harness.try_bond("v3", 1).is_ok());

    // v2 is the direct offender (stake=1).
    let _v2a = harness.sign_block("v2", 5);
    let bad = harness.sign_block_distinct("v2", 5);
    let _ = harness.dispatch(bad);

    // Chain: v0 → v1 → v2 (v0 cites v1 cites v2).
    let v1_neg = harness.sign_block_citing("v1", 6, bad);
    let s1 = harness.dispatch(v1_neg);
    assert_eq!(s1, Status::NeglectedEquivocation);
    let v0_neg = harness.sign_block_citing("v0", 7, v1_neg);
    let s0 = harness.dispatch(v0_neg);
    assert_eq!(s0, Status::NeglectedEquivocation);

    // Closure: {v0, v1, v2}. Total slashed stake = 3+3+1 = 7;
    // extra (beyond direct offender's stake=1) = 6.
    let _ = harness.execute_slash("v0");
    let _ = harness.execute_slash("v1");
    let _ = harness.execute_slash("v2");
    assert_eq!(
        harness.coop_vault(),
        7,
        "Sage witness: amplification factor 7 = direct 1 + extra 6"
    );

    // T-12 weighted-bound precondition is violated here (closure
    // stake 7 > stake fault bound 1), which is precisely why the
    // amplification can occur. T-12 holds when the precondition
    // holds; this UC documents the boundary.
}
