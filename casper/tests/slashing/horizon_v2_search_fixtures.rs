// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Search-horizon fixtures (v2): extends v1 with classifier-bound rows.
//
// Maps to: docs/theory/slashing/slashing-specification.md §14.6.
// Reference: formal/sage/horizon_v2_search_model.sage,
// formal/sage/slashing/FINDINGS.md,
// scripts/ci/slashing-search-horizon.sh.
//
// v2 differs from `horizon_search_fixtures.rs` by introducing the
// `DivergenceClass` axis: each contribution row now carries an expected
// classification, so this file checks both the structural detector
// outcome *and* the classifier label. New for v2: `Detected(u8)` carries
// the offender id (v1 only flagged Detected), allowing the test to
// distinguish detected-self from detected-other contributions.

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

type Edge = (u8, u8);

#[derive(Clone, Copy)]
enum Contribution {
    Missing,
    Detected(u8),
    Child(u8),
}

fn edges(input: &[Edge]) -> BTreeSet<Edge> {
    input
        .iter()
        .copied()
        .filter(|(citer, offender)| citer != offender && *citer < 4 && *offender < 4)
        .collect()
}

fn closure(direct: &BTreeSet<u8>, graph: &BTreeSet<Edge>) -> BTreeSet<u8> {
    let mut slashed = direct.clone();
    loop {
        let next = graph
            .iter()
            .filter_map(|(citer, offender)| slashed.contains(offender).then_some(*citer))
            .fold(slashed.clone(), |mut acc, citer| {
                acc.insert(citer);
                acc
            });
        if next == slashed {
            return slashed;
        }
        slashed = next;
    }
}

fn detector_detectable(contributions: &[Contribution]) -> bool {
    let detected = contributions
        .iter()
        .any(|contribution| matches!(contribution, Contribution::Detected(_)));
    let detected_hashes = contributions
        .iter()
        .filter_map(|contribution| match contribution {
            Contribution::Detected(hash) => Some(*hash),
            Contribution::Missing | Contribution::Child(_) => None,
        })
        .collect::<BTreeSet<_>>();
    let children = contributions
        .iter()
        .filter_map(|contribution| match contribution {
            Contribution::Child(hash) => Some(*hash),
            Contribution::Missing | Contribution::Detected(_) => None,
        })
        .collect::<BTreeSet<_>>();

    detected && !detected_hashes.is_empty() || children.len() >= 2
}

fn stake_sum(stakes: &[u64], validators: &BTreeSet<u8>) -> u64 {
    validators
        .iter()
        .map(|validator| stakes[*validator as usize])
        .sum()
}

fn retention_preserves_slashability(
    retention_window: u64,
    finality_depth: u64,
    gossip_delay: u64,
    inclusion_delay: u64,
) -> bool {
    retention_window >= finality_depth + gossip_delay + inclusion_delay
}

fn min_denial_size(direct: &BTreeSet<u8>, graph: &BTreeSet<Edge>, target: u8) -> Option<usize> {
    let full = closure(direct, graph);
    if !full.contains(&target) {
        return None;
    }
    let all_edges = graph.iter().copied().collect::<Vec<_>>();
    for mask in 1_usize..(1_usize << all_edges.len()) {
        let removed = all_edges
            .iter()
            .enumerate()
            .filter_map(|(index, edge)| ((mask >> index) & 1 == 1).then_some(*edge))
            .collect::<BTreeSet<_>>();
        let remaining = graph.difference(&removed).copied().collect::<BTreeSet<_>>();
        if !closure(direct, &remaining).contains(&target) {
            return Some(removed.len());
        }
    }
    None
}

#[test]
fn finding_117_detector_latest_message_gate_matches_rust_projection() {
    assert!(!detector_detectable(&[Contribution::Missing]));
    assert!(!detector_detectable(&[
        Contribution::Child(7),
        Contribution::Child(7)
    ]));
    assert!(!detector_detectable(&[
        Contribution::Child(7),
        Contribution::Child(7),
        Contribution::Missing
    ]));
    assert!(detector_detectable(&[
        Contribution::Child(7),
        Contribution::Child(8)
    ]));
    assert!(detector_detectable(&[
        Contribution::Missing,
        Contribution::Detected(9)
    ]));
}

#[test]
fn uc_111_horizon_v2_rust_aligned_guards_hold() {
    let direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0), (2, 1), (3, 2)]);
    let stakes = [1, 4, 4, 2];
    let slash_closure = closure(&direct, &graph);

    assert!(detector_detectable(&[
        Contribution::Missing,
        Contribution::Detected(9)
    ]));
    assert!(retention_preserves_slashability(4, 2, 1, 1));
    assert!(!retention_preserves_slashability(3, 2, 1, 1));
    assert_eq!(slash_closure, BTreeSet::from([0, 1, 2, 3]));
    assert_eq!(stake_sum(&stakes, &slash_closure), 11);
    assert_eq!(min_denial_size(&direct, &graph, 3), Some(1));
}

#[test]
fn finding_117_record_lifecycle_requires_detected_hash_retention() {
    let direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0), (2, 1)]);
    let retained = closure(&direct, &graph);
    let deleted_projection = closure(&BTreeSet::new(), &BTreeSet::new());
    let duplicate_records = BTreeSet::from([(0_u8, 1_u64, 40_u64), (0, 1, 40)]);

    assert_eq!(duplicate_records.len(), 1);
    assert_eq!(retained, BTreeSet::from([0, 1, 2]));
    assert_eq!(deleted_projection, BTreeSet::new());
    assert_ne!(retained, deleted_projection);
}

#[test]
fn finding_117_finality_retention_pruning_is_projection_risk() {
    let direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0), (2, 1), (3, 2)]);
    let retained = closure(&direct, &graph);
    let early_pruned = closure(&BTreeSet::new(), &BTreeSet::new());

    assert!(retention_preserves_slashability(4, 2, 1, 1));
    assert!(!retention_preserves_slashability(3, 2, 1, 1));
    assert_eq!(retained, BTreeSet::from([0, 1, 2, 3]));
    assert_ne!(retained, early_pruned);
}

#[test]
fn finding_117_weighted_objective_tracks_damage_and_denial_cost() {
    let stakes = [1, 4, 4, 2];
    let direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0), (2, 0), (3, 1), (3, 2)]);
    let slash_closure = closure(&direct, &graph);
    let direct_stake = stake_sum(&stakes, &direct);
    let closure_stake = stake_sum(&stakes, &slash_closure);

    assert_eq!(slash_closure, BTreeSet::from([0, 1, 2, 3]));
    assert_eq!(direct_stake, 1);
    assert_eq!(closure_stake, 11);
    assert_eq!(min_denial_size(&direct, &graph, 3), Some(2));
}

#[test]
fn finding_117_partition_era_boundary_filters_stale_rebond() {
    let current_epoch_direct = BTreeSet::new();
    let stale_projected_direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0), (2, 1)]);

    let strict = closure(&current_epoch_direct, &graph);
    let loose_projection = closure(&stale_projected_direct, &graph);

    assert_eq!(strict, BTreeSet::new());
    assert_eq!(loose_projection, BTreeSet::from([0, 1, 2]));
}

#[test]
fn finding_117_horizon_v2_boundary_is_documented() {
    let class = classify(DivergenceReason::HorizonV2Boundary);

    assert_eq!(class, DivergenceClass::CandidateBoundaryDivergence);
    assert!(frontier_classification_ok(class));
}

proptest! {
    #[test]
    fn finding_117_differential_classifier_rejects_unexpected_bounded_graphs(
        raw_edges in prop::collection::vec((0_u8..4, 0_u8..4), 0..8),
        direct_seed in prop::collection::btree_set(0_u8..4, 0..4),
    ) {
        let graph = edges(&raw_edges);
        let reversed = graph.iter().rev().copied().collect::<BTreeSet<_>>();
        let duplicated = graph
            .iter()
            .chain(graph.iter())
            .copied()
            .collect::<BTreeSet<_>>();
        let class = classify(DivergenceReason::HorizonV2Boundary);

        prop_assert_eq!(closure(&direct_seed, &graph), closure(&direct_seed, &reversed));
        prop_assert_eq!(closure(&direct_seed, &graph), closure(&direct_seed, &duplicated));
        prop_assert!(frontier_classification_ok(class));
    }
}
