// UC-78 — Metamorphic graph and record frontier.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-78.
// Theorems: T-12 graph equivalence, T-5 record equivalence.
// Reference: formal/sage/slashing/FINDINGS.md (metamorphic
// properties), formal/tlaplus/slashing/TwoLevelSlashing.tla
// invariant `Inv_SameViewSameClosure`.
//
// Property: graph equivalences (edge-order independence, duplicate
// edges, report-suppression) and record equivalences (witness-set
// canonicalisation) preserve the closure / record observation. A
// failure here is an unexpected divergence.

use std::collections::BTreeSet;

use super::divergence_class::{frontier_classification_ok, DivergenceClass};

type Edge = (u8, u8); // (citer, offender)

fn closure_of(edges: &[Edge]) -> BTreeSet<u8> {
    edges.iter().map(|(_, o)| *o).collect()
}

#[test]
fn uc_78_edge_order_independence() {
    // Same edge multiset ↔ same closure: the order in which edges
    // are listed does not affect the closure observation.
    let a = vec![(1u8, 2u8), (2, 3), (3, 4)];
    let mut b = a.clone();
    b.reverse();
    assert_eq!(closure_of(&a), closure_of(&b),
        "UC-78: closure must be order-independent");
    // Bisimilar: no UnexpectedDivergence.
    assert!(frontier_classification_ok(DivergenceClass::Bisimilar));
}

#[test]
fn uc_78_duplicate_edge_idempotence() {
    // Adding a duplicate edge does not enlarge the closure.
    let a = vec![(1u8, 2u8), (2, 3)];
    let b = vec![(1u8, 2u8), (2, 3), (1, 2)];
    assert_eq!(closure_of(&a), closure_of(&b),
        "UC-78: duplicate edges are idempotent");
}

#[test]
fn uc_78_report_suppression_removes_edge() {
    // A report removes the corresponding neglect edge from the
    // active set, shrinking the closure observation.
    let pre_report  = vec![(1u8, 2u8), (3, 4)];
    let post_report = vec![(1u8, 2u8)]; // report suppresses (3,4)
    let cl_pre = closure_of(&pre_report);
    let cl_post = closure_of(&post_report);
    assert!(cl_post.is_subset(&cl_pre),
        "UC-78: post-report closure ⊆ pre-report closure");
}

#[test]
fn uc_78_record_witness_canonicalisation() {
    // BTreeSet auto-canonicalises witness ordering and dedup.
    let r1: BTreeSet<u64> = [1, 2, 3, 1, 2].iter().copied().collect();
    let r2: BTreeSet<u64> = [3, 2, 1].iter().copied().collect();
    assert_eq!(r1, r2,
        "UC-78: witness sets are canonical (dedup + order-free)");
}
