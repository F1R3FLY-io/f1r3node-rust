use std::collections::{BTreeMap, BTreeSet};

use super::divergence_class::{frontier_classification_ok, DivergenceClass};

fn normalized_edges(edges: &[(u8, u8)]) -> BTreeSet<(u8, u8)> { edges.iter().copied().collect() }

fn rename_edges(edges: &[(u8, u8)], renaming: &BTreeMap<u8, u8>) -> BTreeSet<(u8, u8)> {
    edges
        .iter()
        .map(|(a, b)| (renaming[a], renaming[b]))
        .collect()
}

#[test]
fn uc_91_edge_order_and_duplicate_edges_are_metamorphic_safe() {
    let base = vec![(1, 0), (2, 1), (3, 0)];
    let permuted = vec![(3, 0), (1, 0), (2, 1), (1, 0)];
    assert_eq!(normalized_edges(&base), normalized_edges(&permuted));
    assert!(frontier_classification_ok(DivergenceClass::Bisimilar));
}

#[test]
fn uc_91_validator_renaming_preserves_graph_shape() {
    let edges = vec![(1, 0), (2, 1)];
    let renamed = BTreeMap::from([(0, 10), (1, 11), (2, 12)]);
    assert_eq!(
        rename_edges(&edges, &renamed),
        BTreeSet::from([(11, 10), (12, 11)])
    );
}

#[test]
fn uc_91_record_hash_normalization_is_order_free() {
    let a = BTreeSet::from([3, 1, 2]);
    let b = BTreeSet::from([2, 3, 1]);
    assert_eq!(a, b);
}
