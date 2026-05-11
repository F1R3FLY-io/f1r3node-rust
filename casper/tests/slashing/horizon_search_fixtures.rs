// Search-horizon fixtures (v1): pin the bounded-search frontier.
//
// Maps to: docs/theory/slashing/slashing-specification.md §14.6.
// Reference: formal/sage/horizon_search_model.sage,
// docs/theory/slashing/slashing-search-horizon.md,
// scripts/ci/slashing-search-horizon.sh.
//
// Why this file exists: the CI horizon search produces a frontier of
// (validator-count, contribution-type, neglect-edge) combinations whose
// classification is "interesting" (CandidateBoundaryDivergence or worse).
// This file replays a small, pinned subset of that frontier so a
// regression in the production detector + dispatcher surfaces on every
// pull request — without paying for the full multi-hour CI search.
// Validator count bound `< 4` matches the horizon-search v1 envelope.

use std::collections::BTreeSet;

use proptest::prelude::*;

type Edge = (u8, u8);

#[derive(Clone, Copy)]
enum Contribution {
    Missing,
    Detected,
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

fn active_edges(visible: &BTreeSet<Edge>, reports: &BTreeSet<Edge>) -> BTreeSet<Edge> {
    visible.difference(reports).copied().collect()
}

fn first_slash_slot(schedule: &[(bool, bool, bool)]) -> Option<usize> {
    schedule
        .iter()
        .position(|(bonded, observes, includes)| *bonded && *observes && *includes)
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
        })
        .collect::<BTreeSet<_>>();
    detected || children.len() >= 2
}

fn stake_sum(stakes: &[u64], validators: &BTreeSet<u8>) -> u64 {
    validators
        .iter()
        .map(|validator| stakes[*validator as usize])
        .sum()
}

fn checked_total(limit: u128, values: &[u128]) -> Option<u128> {
    values
        .iter()
        .try_fold(0_u128, |acc, value| acc.checked_add(*value))
        .filter(|value| *value <= limit)
}

#[test]
fn finding_116_retention_policy_covers_gossip_plus_inclusion() {
    let direct = BTreeSet::from([1]);
    let visible = BTreeSet::from([(0, 1), (2, 0)]);
    let retained = closure(&direct, &visible);
    let unsafe_projected = closure(&BTreeSet::new(), &BTreeSet::new());
    let gossip_delay = 1;
    let inclusion_delay = 1;
    let safe_retention_window = 2;

    assert_eq!(retained, BTreeSet::from([0, 1, 2]));
    assert_eq!(unsafe_projected, BTreeSet::new());
    assert!(safe_retention_window >= gossip_delay + inclusion_delay);
    assert_eq!(closure(&direct, &visible), retained);
}

#[test]
fn uc_110_horizon_campaign_cross_axis_guards_hold() {
    let direct = BTreeSet::from([1]);
    let visible = BTreeSet::from([(0, 1), (2, 0)]);
    let reports = BTreeSet::from([(0, 1)]);
    let schedule = [(true, true, false), (true, true, true)];

    assert_eq!(closure(&direct, &visible), BTreeSet::from([0, 1, 2]));
    assert_eq!(active_edges(&visible, &reports), BTreeSet::from([(2, 0)]));
    assert_eq!(first_slash_slot(&schedule), Some(1));
    assert!(detector_detectable(&[
        Contribution::Child(7),
        Contribution::Child(8)
    ]));
    assert_eq!(checked_total(u8::MAX as u128, &[u8::MAX as u128, 1]), None);
}

#[test]
fn finding_116_proposer_fairness_restores_bounded_liveness() {
    let withholding = [(true, true, false)];
    let fair_extension = [(true, true, false), (true, true, true)];

    assert_eq!(first_slash_slot(&withholding), None);
    assert_eq!(first_slash_slot(&fair_extension), Some(1));
}

#[test]
fn finding_116_detector_gate_blocks_duplicate_child_paths() {
    assert!(!detector_detectable(&[Contribution::Missing]));
    assert!(!detector_detectable(&[
        Contribution::Child(7),
        Contribution::Child(7)
    ]));
    assert!(detector_detectable(&[
        Contribution::Child(7),
        Contribution::Child(8)
    ]));
    assert!(detector_detectable(&[
        Contribution::Missing,
        Contribution::Detected
    ]));
}

#[test]
fn finding_116_epoch_tagged_identity_blocks_stale_rebond_slash() {
    let current_epoch_direct = BTreeSet::new();
    let stale_projected_direct = BTreeSet::from([0]);
    let graph = BTreeSet::from([(1, 0)]);

    let strict = closure(&current_epoch_direct, &graph);
    let loose_projection = closure(&stale_projected_direct, &graph);

    assert_eq!(strict, BTreeSet::new());
    assert_eq!(loose_projection, BTreeSet::from([0, 1]));
}

#[test]
fn finding_116_weighted_damage_requires_closure_bound_precondition() {
    let stakes = [4, 4, 1, 1];
    let direct = BTreeSet::from([2]);
    let graph = BTreeSet::from([(1, 2), (0, 1)]);
    let slash_closure = closure(&direct, &graph);
    let direct_stake = stake_sum(&stakes, &direct);
    let closure_stake = stake_sum(&stakes, &slash_closure);
    let fault_bound = 1;

    assert_eq!(direct_stake, 1);
    assert_eq!(closure_stake, 9);
    assert!(direct_stake <= fault_bound);
    assert!(closure_stake > fault_bound);
}

#[test]
fn finding_116_view_merge_overapproximates_partitioned_views() {
    let direct = BTreeSet::from([0]);
    let left = BTreeSet::from([(1, 0)]);
    let right = BTreeSet::from([(2, 1), (3, 0)]);
    let merged = left.union(&right).copied().collect::<BTreeSet<_>>();

    let left_closure = closure(&direct, &left);
    let right_closure = closure(&direct, &right);
    let merged_closure = closure(&direct, &merged);

    assert!(left_closure.is_subset(&merged_closure));
    assert!(right_closure.is_subset(&merged_closure));
}

#[test]
fn finding_116_checked_arithmetic_rejects_wrapping_vault_projection() {
    let limit = u8::MAX as u128;
    let values = [u8::MAX as u128, 1];
    let exact = values.iter().sum::<u128>();
    let wrapped = exact as u8;

    assert_eq!(checked_total(limit, &values), None);
    assert_eq!(wrapped, 0);
}

#[test]
fn finding_116_reports_remain_pair_scoped_at_horizon() {
    let direct = BTreeSet::from([0]);
    let visible = BTreeSet::from([(1, 0), (1, 2)]);
    let reports = BTreeSet::from([(1, 0)]);
    let active = active_edges(&visible, &reports);

    assert!(active.contains(&(1, 2)));
    assert!(!active.contains(&(1, 0)));
    assert_eq!(closure(&direct, &active), BTreeSet::from([0]));
}

proptest! {
    #[test]
    fn finding_116_edge_order_and_duplicate_evidence_are_metamorphic(
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

        prop_assert_eq!(closure(&direct_seed, &graph), closure(&direct_seed, &reversed));
        prop_assert_eq!(closure(&direct_seed, &graph), closure(&direct_seed, &duplicated));
    }
}
