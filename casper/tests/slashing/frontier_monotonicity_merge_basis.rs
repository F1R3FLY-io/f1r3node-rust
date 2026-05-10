use std::collections::BTreeSet;

use proptest::prelude::*;

type Edge = (u8, u8);

fn validators(input: &[u8]) -> BTreeSet<u8> { input.iter().copied().filter(|v| *v < 4).collect() }

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

fn slash_prefix(
    direct: &BTreeSet<u8>,
    graph: &BTreeSet<Edge>,
    max_level: usize,
) -> Vec<BTreeSet<u8>> {
    let mut rows = Vec::with_capacity(max_level + 1);
    let mut slashed = BTreeSet::new();
    rows.push(slashed.clone());
    for step in 0..max_level {
        let delta: BTreeSet<_> = if step == 0 {
            direct.difference(&slashed).copied().collect()
        } else {
            graph
                .iter()
                .filter_map(|(citer, offender)| {
                    (slashed.contains(offender) && !slashed.contains(citer)).then_some(*citer)
                })
                .collect()
        };
        slashed.extend(delta);
        rows.push(slashed.clone());
    }
    rows
}

fn union<T: Ord + Copy>(left: &BTreeSet<T>, right: &BTreeSet<T>) -> BTreeSet<T> {
    left.union(right).copied().collect()
}

fn reverse_graph(graph: &BTreeSet<Edge>) -> BTreeSet<Edge> {
    graph.iter().map(|(src, dst)| (*dst, *src)).collect()
}

fn active_edges(visible: &BTreeSet<Edge>, reports: &BTreeSet<Edge>) -> BTreeSet<Edge> {
    visible.difference(reports).copied().collect()
}

fn rename_set(values: &BTreeSet<u8>, permutation: &[u8; 4]) -> BTreeSet<u8> {
    values.iter().map(|v| permutation[*v as usize]).collect()
}

fn rename_graph(graph: &BTreeSet<Edge>, permutation: &[u8; 4]) -> BTreeSet<Edge> {
    graph
        .iter()
        .map(|(src, dst)| (permutation[*src as usize], permutation[*dst as usize]))
        .collect()
}

fn bounded_traversal(start: u8, graph: &BTreeSet<Edge>, fuel: usize) -> BTreeSet<u8> {
    let mut seen = BTreeSet::new();
    let mut frontier = BTreeSet::from([start]);
    for _ in 0..fuel {
        if frontier.is_empty() {
            break;
        }
        seen.extend(frontier.iter().copied());
        let next = graph
            .iter()
            .filter_map(|(src, dst)| frontier.contains(src).then_some(*dst))
            .filter(|dst| *dst < 4 && !seen.contains(dst))
            .collect();
        frontier = next;
    }
    seen.extend(frontier);
    seen
}

#[derive(Clone, Copy)]
enum Contribution {
    Missing,
    Detected,
    Child(u8),
}

fn detector_detectable(contributions: &[Contribution]) -> bool {
    let detected = contributions
        .iter()
        .any(|contribution| matches!(contribution, Contribution::Detected));
    let children = contributions
        .iter()
        .filter_map(|contribution| match contribution {
            Contribution::Child(hash) => Some(*hash),
            Contribution::Missing | Contribution::Detected => None,
        });
    detected || children.collect::<BTreeSet<_>>().len() >= 2
}

fn minimal_denial_size(direct: &BTreeSet<u8>, graph: &BTreeSet<Edge>, target: u8) -> Option<usize> {
    let all_edges: Vec<_> = graph.iter().copied().collect();
    let full = closure(direct, graph);
    if !full.contains(&target) {
        return None;
    }
    for mask in 1usize..(1usize << all_edges.len()) {
        let removed = all_edges
            .iter()
            .enumerate()
            .filter_map(|(index, edge)| ((mask >> index) & 1 == 1).then_some(*edge))
            .collect::<BTreeSet<_>>();
        let remaining = graph.difference(&removed).copied().collect();
        if !closure(direct, &remaining).contains(&target) {
            return Some(removed.len());
        }
    }
    None
}

#[test]
fn finding_98_evidence_addition_can_only_expand_closure() {
    let direct = BTreeSet::from([0]);
    let base_edges = BTreeSet::from([(1, 0)]);
    let expanded_edges = BTreeSet::from([(1, 0), (2, 1)]);

    let base = closure(&direct, &base_edges);
    let expanded = closure(&direct, &expanded_edges);

    assert!(base.is_subset(&expanded));
    assert_eq!(base, BTreeSet::from([0, 1]));
    assert_eq!(expanded, BTreeSet::from([0, 1, 2]));
}

#[test]
fn uc_109_frontier_monotonicity_merge_basis_guards_hold() {
    let direct = BTreeSet::from([0]);
    let base = BTreeSet::from([(1, 0)]);
    let expanded = BTreeSet::from([(1, 0), (2, 1)]);
    let reports = BTreeSet::from([(1, 0)]);

    assert!(closure(&direct, &base).is_subset(&closure(&direct, &expanded)));
    assert_eq!(closure(&direct, &active_edges(&expanded, &reports)), BTreeSet::from([0]));
    assert!(detector_detectable(&[Contribution::Child(7), Contribution::Child(8)]));
    assert_eq!(minimal_denial_size(&direct, &expanded, 2), Some(1));
}

#[test]
fn finding_99_merged_views_overapproximate_inputs() {
    let direct = BTreeSet::from([0]);
    let left = BTreeSet::from([(1, 0)]);
    let right = BTreeSet::from([(2, 1)]);
    let merged = union(&left, &right);
    let merged_reverse = union(&right, &left);

    let left_closure = closure(&direct, &left);
    let right_closure = closure(&direct, &right);
    let merged_closure = closure(&direct, &merged);

    assert!(left_closure.is_subset(&merged_closure));
    assert!(right_closure.is_subset(&merged_closure));
    assert_eq!(merged_closure, closure(&direct, &merged_reverse));
}

#[test]
fn finding_100_minimal_basis_for_chain_target() {
    let direct = BTreeSet::from([0]);
    let basis = BTreeSet::from([(1, 0), (2, 1), (3, 2)]);

    assert!(closure(&direct, &basis).contains(&3));

    for edge in &basis {
        let mut reduced = basis.clone();
        reduced.remove(edge);
        assert!(!closure(&direct, &reduced).contains(&3));
    }
}

#[test]
fn finding_102_detector_traversal_cycle_is_bounded() {
    let graph = BTreeSet::from([(0, 1), (1, 2), (2, 1)]);

    assert_eq!(bounded_traversal(0, &graph, 4), BTreeSet::from([0, 1, 2]));
}

#[test]
fn finding_103_detector_contributions_are_order_independent() {
    let a = [
        Contribution::Missing,
        Contribution::Child(1),
        Contribution::Child(1),
        Contribution::Child(2),
    ];
    let b = [
        Contribution::Child(2),
        Contribution::Missing,
        Contribution::Child(1),
        Contribution::Child(1),
    ];
    let c = [Contribution::Detected, Contribution::Missing];

    assert_eq!(detector_detectable(&a), detector_detectable(&b));
    assert!(detector_detectable(&c));
    assert!(!detector_detectable(&[
        Contribution::Missing,
        Contribution::Child(1),
        Contribution::Child(1),
    ]));
}

#[test]
fn finding_104_closure_fixed_point_replay_is_idempotent() {
    let direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0), (2, 1), (3, 2)]);

    let fixed = closure(&direct, &graph);
    assert_eq!(closure(&fixed, &graph), fixed);
}

#[test]
fn finding_105_report_retention_prevents_edge_reactivation() {
    let direct = BTreeSet::from([0]);
    let visible = BTreeSet::from([(1, 0)]);
    let retained_reports = BTreeSet::from([(1, 0)]);
    let active_retained: BTreeSet<_> = visible.difference(&retained_reports).copied().collect();
    let active_after_report_pruned = visible;

    assert_eq!(closure(&direct, &active_retained), BTreeSet::from([0]));
    assert_eq!(
        closure(&direct, &active_after_report_pruned),
        BTreeSet::from([0, 1])
    );
}

#[test]
fn finding_106_no_seed_cycle_does_not_slash() {
    let direct = BTreeSet::new();
    let cycle = BTreeSet::from([(0, 1), (1, 2), (2, 0)]);

    assert_eq!(closure(&direct, &cycle), BTreeSet::new());
}

#[test]
fn finding_107_slash_history_matches_closure_prefix() {
    let direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0), (2, 1), (3, 2)]);
    let rows = slash_prefix(&direct, &graph, 4);

    assert_eq!(rows[0], BTreeSet::new());
    assert_eq!(rows[1], BTreeSet::from([0]));
    assert_eq!(rows[2], BTreeSet::from([0, 1]));
    assert_eq!(rows[3], BTreeSet::from([0, 1, 2]));
    assert_eq!(rows[4], closure(&direct, &graph));
}

#[test]
fn finding_108_edge_orientation_is_semantic() {
    let direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0)]);
    let reversed = reverse_graph(&graph);

    assert_eq!(closure(&direct, &graph), BTreeSet::from([0, 1]));
    assert_eq!(closure(&direct, &reversed), BTreeSet::from([0]));
}

#[test]
fn finding_109_redundant_paths_raise_denial_cost() {
    let direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0), (3, 1), (2, 0), (3, 2)]);

    assert_eq!(closure(&direct, &graph), BTreeSet::from([0, 1, 2, 3]));
    assert_eq!(minimal_denial_size(&direct, &graph, 3), Some(2));
}

#[test]
fn finding_110_slash_targets_are_not_direct_evidence() {
    let slash_targets = BTreeSet::from([1, 2]);
    let empty_direct = BTreeSet::new();
    let graph = BTreeSet::new();

    assert_eq!(closure(&empty_direct, &graph), BTreeSet::new());
    assert_eq!(closure(&slash_targets, &graph), slash_targets);
}

#[test]
fn finding_111_reports_are_pair_scoped() {
    let direct = BTreeSet::from([2]);
    let visible = BTreeSet::from([(1, 0), (1, 2)]);
    let reports = BTreeSet::from([(1, 0)]);
    let blanket_projection = BTreeSet::new();

    assert_eq!(
        closure(&direct, &active_edges(&visible, &reports)),
        BTreeSet::from([1, 2])
    );
    assert_eq!(closure(&direct, &blanket_projection), BTreeSet::from([2]));
}

#[test]
fn finding_112_report_growth_cannot_expand_closure() {
    let direct = BTreeSet::from([0]);
    let visible = BTreeSet::from([(1, 0), (2, 1)]);
    let reports_before = BTreeSet::new();
    let reports_after = BTreeSet::from([(1, 0)]);

    let before = closure(&direct, &active_edges(&visible, &reports_before));
    let after = closure(&direct, &active_edges(&visible, &reports_after));

    assert!(after.is_subset(&before));
}

#[test]
fn finding_113_reports_do_not_suppress_direct_evidence() {
    let direct = BTreeSet::from([0]);
    let visible = BTreeSet::from([(1, 0)]);
    let reports = BTreeSet::from([(1, 0), (0, 1)]);

    assert!(closure(&direct, &active_edges(&visible, &reports)).contains(&0));
}

#[test]
fn finding_114_validator_renaming_equivariance() {
    let direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0), (2, 1)]);
    let permutation = [2, 0, 3, 1];
    let renamed_direct = direct.iter().map(|v| permutation[*v as usize]).collect();
    let renamed_graph = graph
        .iter()
        .map(|(src, dst)| (permutation[*src as usize], permutation[*dst as usize]))
        .collect();
    let renamed_base = closure(&direct, &graph)
        .iter()
        .map(|v| permutation[*v as usize])
        .collect::<BTreeSet<_>>();

    assert_eq!(closure(&renamed_direct, &renamed_graph), renamed_base);
}

#[test]
fn finding_115_bisimilarity_delta_guard_classifies_differences() {
    let direct = BTreeSet::from([0]);
    let single = BTreeSet::from([(1, 0)]);
    let duplicate = BTreeSet::from([(1, 0), (1, 0)]);
    let ordered = BTreeSet::from([(1, 0), (2, 1)]);
    let reversed_order = BTreeSet::from([(2, 1), (1, 0)]);
    let reversed_edge = BTreeSet::from([(0, 1)]);

    assert_eq!(closure(&direct, &single), closure(&direct, &duplicate));
    assert_eq!(
        closure(&direct, &ordered),
        closure(&direct, &reversed_order)
    );
    assert_ne!(closure(&direct, &single), closure(&direct, &reversed_edge));
    assert_ne!(
        closure(&BTreeSet::new(), &BTreeSet::new()),
        closure(&BTreeSet::from([1]), &BTreeSet::new())
    );
}

proptest! {
    #[test]
    fn prop_finding_98_evidence_addition_monotone(
        base_direct in prop::collection::vec(0u8..6, 0..6),
        extra_direct in prop::collection::vec(0u8..6, 0..6),
        base_edges in prop::collection::vec((0u8..6, 0u8..6), 0..12),
        extra_edges in prop::collection::vec((0u8..6, 0u8..6), 0..12),
    ) {
        let base_direct = validators(&base_direct);
        let extra_direct = validators(&extra_direct);
        let base_edges = edges(&base_edges);
        let extra_edges = edges(&extra_edges);
        let expanded_direct = union(&base_direct, &extra_direct);
        let expanded_edges = union(&base_edges, &extra_edges);

        let base = closure(&base_direct, &base_edges);
        let expanded = closure(&expanded_direct, &expanded_edges);

        prop_assert!(base.is_subset(&expanded));
    }

    #[test]
    fn prop_finding_99_view_merge_confluence(
        direct in prop::collection::vec(0u8..6, 0..6),
        left_edges in prop::collection::vec((0u8..6, 0u8..6), 0..12),
        right_edges in prop::collection::vec((0u8..6, 0u8..6), 0..12),
    ) {
        let direct = validators(&direct);
        let left_edges = edges(&left_edges);
        let right_edges = edges(&right_edges);
        let merged = union(&left_edges, &right_edges);
        let merged_reverse = union(&right_edges, &left_edges);

        let left = closure(&direct, &left_edges);
        let right = closure(&direct, &right_edges);
        let merged_closure = closure(&direct, &merged);

        prop_assert!(left.is_subset(&merged_closure));
        prop_assert!(right.is_subset(&merged_closure));
        prop_assert_eq!(merged_closure, closure(&direct, &merged_reverse));
    }

    #[test]
    fn prop_finding_112_report_growth_antitone(
        direct in prop::collection::vec(0u8..6, 0..6),
        visible in prop::collection::vec((0u8..6, 0u8..6), 0..12),
        reports_before in prop::collection::vec((0u8..6, 0u8..6), 0..12),
        reports_extra in prop::collection::vec((0u8..6, 0u8..6), 0..12),
    ) {
        let direct = validators(&direct);
        let visible = edges(&visible);
        let reports_before = edges(&reports_before);
        let reports_after = union(&reports_before, &edges(&reports_extra));
        let before = closure(&direct, &active_edges(&visible, &reports_before));
        let after = closure(&direct, &active_edges(&visible, &reports_after));

        prop_assert!(after.is_subset(&before));
    }

    #[test]
    fn prop_finding_113_reports_do_not_remove_direct(
        direct in prop::collection::vec(0u8..6, 0..6),
        visible in prop::collection::vec((0u8..6, 0u8..6), 0..12),
        reports in prop::collection::vec((0u8..6, 0u8..6), 0..12),
    ) {
        let direct = validators(&direct);
        let visible = edges(&visible);
        let reports = edges(&reports);
        let closed = closure(&direct, &active_edges(&visible, &reports));

        prop_assert!(direct.is_subset(&closed));
    }

    #[test]
    fn prop_finding_114_validator_renaming_equivariance(
        direct in prop::collection::vec(0u8..6, 0..6),
        graph in prop::collection::vec((0u8..6, 0u8..6), 0..12),
        permutation in prop::sample::select(vec![
            [0u8, 1, 2, 3],
            [1u8, 0, 2, 3],
            [2u8, 0, 3, 1],
            [3u8, 2, 1, 0],
            [1u8, 2, 3, 0],
            [2u8, 3, 0, 1],
        ]),
    ) {
        let direct = validators(&direct);
        let graph = edges(&graph);
        let renamed_direct = rename_set(&direct, &permutation);
        let renamed_graph = rename_graph(&graph, &permutation);
        let renamed_closure = rename_set(&closure(&direct, &graph), &permutation);

        prop_assert_eq!(closure(&renamed_direct, &renamed_graph), renamed_closure);
    }
}
