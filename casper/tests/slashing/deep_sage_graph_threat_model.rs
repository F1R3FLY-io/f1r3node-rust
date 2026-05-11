// UC-93 — Deep neglect-chain threat: reverse-reachability path certificate.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-93.
// Threat class: Graph-shape (deep neglect chain) (Sage row
// `dag_behavior_model.sage` + `deep_threat_model.sage`).
// Reference: formal/sage/deep_threat_model.sage,
// formal/sage/slashing/FINDINGS.md.
//
// Sage finding: an adversary may build a long chain of validators where
// each member neglects the previous offender, attempting to obscure the
// original offender's accountability through chain length. The post-fix
// invariant is reverse-reachability closure — every neglecter transitively
// upstream of a directly-detected offender belongs to the slash closure.
// This file pins a 4-deep chain (`1 → 0`, `2 → 1`, `3 → 2`) and asserts
// that direct detection of `0` produces closure `{0,1,2,3}` — every
// neglecter is reachable.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

fn reverse_reachability(direct: BTreeSet<u8>, neglect_edges: &[(u8, u8)]) -> BTreeSet<u8> {
    let mut reverse = BTreeMap::<u8, Vec<u8>>::new();
    for (neglecter, target) in neglect_edges {
        reverse.entry(*target).or_default().push(*neglecter);
    }

    let mut closure = direct.clone();
    let mut queue = VecDeque::from_iter(direct);
    while let Some(target) = queue.pop_front() {
        for next in reverse.get(&target).into_iter().flatten() {
            if closure.insert(*next) {
                queue.push_back(*next);
            }
        }
    }
    closure
}

#[test]
fn uc_93_deep_reachability_chain_has_path_certificate() {
    let edges = [(1, 0), (2, 1), (3, 2)];
    let closure = reverse_reachability(BTreeSet::from([0]), &edges);
    assert_eq!(closure, BTreeSet::from([0, 1, 2, 3]));
}

#[test]
fn uc_93_disconnected_cycle_cannot_exploit_closure() {
    let edges = [(1, 2), (2, 1)];
    let closure = reverse_reachability(BTreeSet::from([0]), &edges);
    assert_eq!(closure, BTreeSet::from([0]));
}
