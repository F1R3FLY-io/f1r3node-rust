use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkDimension, HostWorkReservationError, HostWorkUnits};
use thiserror::Error;

use super::funding_adjacency::advance_adjacency;
#[cfg(test)]
use super::funding_arithmetic::checked_residual_transfer;
use super::funding_augmentation::augment_edge;
use super::funding_discovery::discover_parent;
use super::funding_graph::add_edge;
#[cfg(test)]
use super::funding_graph::ResidualEdge;
use super::{
    check_funding_assignment, check_funding_deficit, FundingAssignmentError,
    FundingAssignmentTotals,
};

#[cfg(test)]
#[path = "funding_search_properties.rs"]
mod search_properties;
#[cfg(test)]
#[path = "funding_domain_properties.rs"]
mod domain_properties;
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct FundingSearchLimits {
    pub source_cap: NonZeroUsize,
    pub obligation_cap: NonZeroUsize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FundingFeasibility {
    Feasible {
        assignment: Vec<Vec<u64>>,
        totals: FundingAssignmentTotals,
    },
    Infeasible {
        selected_obligations: Vec<bool>,
    },
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum FundingSearchError {
    #[error(transparent)]
    InvalidProblem(#[from] FundingAssignmentError),
    #[error(transparent)]
    HostWork(#[from] HostWorkReservationError),
    #[error("funding search allocation failed")]
    AllocationFailed,
    #[error("funding search arithmetic overflow")]
    Overflow,
    #[error("funding search result failed independent verification")]
    InvalidResult,
    #[error("funding source lower bound exceeds its upper bound")]
    InvalidSourceBounds,
    #[error("funding priority cursor is outside the source cohort")]
    InvalidPriorityCursor,
    #[error("funding assignment entry is outside the captured matrix")]
    InvalidAssignmentCell,
    #[error("exact funding contributions do not equal the total obligation")]
    UnequalFundingTotals,
}

const ABSENT: usize = usize::MAX;

pub(in crate::rust::interpreter::accounting) fn reserve_work(
    budget: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), FundingSearchError> {
    let units = u64::try_from(amount).map_err(|_| FundingSearchError::Overflow)?;
    budget.reserve(dimension, HostWorkUnits::new(units))?;
    Ok(())
}

fn reserved_vec<T>(capacity: usize) -> Result<Vec<T>, FundingSearchError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(capacity)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    Ok(result)
}

pub(in crate::rust::interpreter::accounting) fn filled_vec<T: Clone>(
    count: usize,
    value: T,
) -> Result<Vec<T>, FundingSearchError> {
    let mut result = reserved_vec(count)?;
    result.resize(count, value);
    Ok(result)
}

pub fn solve_funding_feasibility(
    capacities: &[u64],
    obligations: &[u64],
    eligible: &[Vec<bool>],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<FundingFeasibility, FundingSearchError> {
    let sources = capacities.len();
    let count = obligations.len();
    if sources == 0 {
        return Err(FundingAssignmentError::EmptySources.into());
    }
    if sources > limits.source_cap.get() {
        return Err(FundingAssignmentError::TooManySources.into());
    }
    if count > limits.obligation_cap.get() {
        return Err(FundingAssignmentError::TooManyObligations.into());
    }
    let cells = sources
        .checked_mul(count)
        .ok_or(FundingSearchError::Overflow)?;
    let vertices = sources
        .checked_add(count)
        .and_then(|n| n.checked_add(2))
        .ok_or(FundingSearchError::Overflow)?;
    let scan = cells
        .checked_add(vertices)
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, scan)?;
    if eligible.len() != sources || eligible.iter().any(|row| row.len() != count) {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
    let total = obligations
        .iter()
        .try_fold(0_u64, |sum, amount| sum.checked_add(*amount))
        .ok_or(FundingAssignmentError::Overflow)?;
    let allowed = eligible
        .iter()
        .flatten()
        .filter(|permitted| **permitted)
        .count();
    let directed_edges = allowed
        .checked_add(sources)
        .and_then(|n| n.checked_add(count))
        .and_then(|n| n.checked_mul(2))
        .ok_or(FundingSearchError::Overflow)?;
    let state_bytes = [
        (directed_edges, 24_usize),
        (vertices, 24),
        (cells, 8),
        (sources, 32),
        (count, 9),
    ]
    .into_iter()
    .try_fold(0_usize, |sum, (n, width)| {
        n.checked_mul(width)
            .and_then(|bytes| sum.checked_add(bytes))
    })
    .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchStateBytes, state_bytes)?;
    let mut edges = reserved_vec(directed_edges)?;
    let mut heads = filled_vec(vertices, ABSENT)?;
    let first_obligation = sources + 1;
    let sink = vertices - 1;
    for (i, capacity) in capacities.iter().enumerate() {
        add_edge(&mut edges, &mut heads, 0, i + 1, (*capacity).min(total));
    }
    for (i, row) in eligible.iter().enumerate() {
        for (j, permitted) in row.iter().enumerate() {
            if *permitted {
                add_edge(&mut edges, &mut heads, i + 1, first_obligation + j, total);
            }
        }
    }
    for (j, amount) in obligations.iter().enumerate() {
        add_edge(&mut edges, &mut heads, first_obligation + j, sink, *amount);
    }
    let mut parents = filled_vec(vertices, ABSENT)?;
    let mut queue = reserved_vec(vertices)?;
    let mut funded = 0_u64;
    #[cfg(test)]
    let mut progress = search_properties::FundingProgress::new(funded, total);
    #[cfg(test)]
    tests::assert_residual_state(capacities, obligations, eligible, &edges, &heads, funded);
    while funded < total {
        reserve_work(budget, HostWorkDimension::SearchCandidates, vertices)?;
        parents.fill(ABSENT);
        parents[0] = 0;
        queue.clear();
        queue.push(0);
        let mut cursor = 0;
        #[cfg(test)]
        search_properties::assert_search_state(&edges, &heads, &parents, &queue, cursor);
        while cursor < queue.len() && parents[sink] == ABSENT {
            #[cfg(test)]
            let scan = search_properties::SearchStepSnapshot::new(&queue, cursor);
            let node = queue[cursor];
            cursor += 1;
            let mut adjacency_cursor = heads[node];
            while adjacency_cursor != ABSENT {
                reserve_work(budget, HostWorkDimension::SearchCandidates, 1)?;
                let index = advance_adjacency(&edges, &mut adjacency_cursor)
                    .ok_or(FundingSearchError::InvalidResult)?;
                let edge = &edges[index];
                discover_parent(edge, index, &mut parents, &mut queue);
            }
            #[cfg(test)]
            scan.assert_completed(&queue, cursor);
            #[cfg(test)]
            search_properties::assert_search_state(&edges, &heads, &parents, &queue, cursor);
        }
        if parents[sink] == ABSENT {
            #[cfg(test)]
            assert_eq!(
                cursor,
                queue.len(),
                "unsuccessful search must exhaust its queue"
            );
            #[cfg(test)]
            tests::assert_closed_cut(capacities, obligations, eligible, &edges, &parents, funded);
            let mut selected = reserved_vec(count)?;
            selected.extend((0..count).map(|j| parents[first_obligation + j] == ABSENT));
            reserve_work(budget, HostWorkDimension::VerificationOperations, scan)?;
            match check_funding_deficit(
                capacities,
                obligations,
                eligible,
                &selected,
                limits.source_cap,
                limits.obligation_cap,
            ) {
                Ok(true) => {
                    return Ok(FundingFeasibility::Infeasible {
                        selected_obligations: selected,
                    })
                }
                _ => return Err(FundingSearchError::InvalidResult),
            }
        }
        let mut amount = total - funded;
        let mut node = sink;
        while node != 0 {
            reserve_work(budget, HostWorkDimension::SearchCandidates, 1)?;
            let index = parents[node];
            amount = amount.min(edges[index].capacity);
            node = edges[index ^ 1].to;
        }
        if amount == 0 {
            return Err(FundingSearchError::InvalidResult);
        }
        #[cfg(test)]
        tests::assert_augmenting_path(&edges, &parents, sink, amount, total - funded);
        #[cfg(test)]
        let augmentation =
            search_properties::AugmentationSnapshot::new(&edges, &parents, sink, amount);
        node = sink;
        while node != 0 {
            reserve_work(budget, HostWorkDimension::SearchCandidates, 1)?;
            let index = parents[node];
            node =
                augment_edge(&mut edges, index, amount).ok_or(FundingSearchError::InvalidResult)?;
        }
        funded = funded
            .checked_add(amount)
            .ok_or(FundingSearchError::InvalidResult)?;
        #[cfg(test)]
        progress.record(funded, amount);
        #[cfg(test)]
        augmentation.assert_completed(&edges);
        #[cfg(test)]
        tests::assert_residual_state(capacities, obligations, eligible, &edges, &heads, funded);
    }
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        scan.checked_add(directed_edges)
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut assignment = reserved_vec(sources)?;
    for _ in 0..sources {
        assignment.push(filled_vec(count, 0_u64)?);
    }
    for index in (0..edges.len()).step_by(2) {
        let from = edges[index ^ 1].to;
        let to = edges[index].to;
        if (1..first_obligation).contains(&from) && (first_obligation..sink).contains(&to) {
            assignment[from - 1][to - first_obligation] = edges[index ^ 1].capacity;
        }
    }
    let totals = check_funding_assignment(
        capacities,
        obligations,
        eligible,
        &assignment,
        limits.source_cap,
        limits.obligation_cap,
    )
    .map_err(|_| FundingSearchError::InvalidResult)?;
    Ok(FundingFeasibility::Feasible { assignment, totals })
}

#[cfg(test)]
mod tests {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use proptest::prelude::*;

    use super::*;

    fn residual_assignment(
        edges: &[ResidualEdge],
        sources: usize,
        obligations: usize,
    ) -> Vec<Vec<u64>> {
        let mut assignment = vec![vec![0; obligations]; sources];
        for index in (0..edges.len()).step_by(2) {
            let from = edges[index ^ 1].to;
            let to = edges[index].to;
            if (1..=sources).contains(&from)
                && (sources + 1..sources + 1 + obligations).contains(&to)
            {
                assignment[from - 1][to - sources - 1] = edges[index ^ 1].capacity;
            }
        }
        assignment
    }

    pub(super) fn assert_residual_state(
        capacities: &[u64],
        obligations: &[u64],
        eligible: &[Vec<bool>],
        edges: &[ResidualEdge],
        heads: &[usize],
        funded: u64,
    ) {
        let sources = capacities.len();
        let count = obligations.len();
        let total: u64 = obligations.iter().sum();
        let sink = sources + count + 1;
        search_properties::assert_unique_funding_pairs(edges);
        let mut expected = Vec::new();
        for (i, capacity) in capacities.iter().enumerate() {
            expected.push((0, i + 1, (*capacity).min(total)));
        }
        for (i, row) in eligible.iter().enumerate() {
            for (j, permitted) in row.iter().enumerate() {
                if *permitted {
                    expected.push((i + 1, sources + 1 + j, total));
                }
            }
        }
        for (j, demand) in obligations.iter().enumerate() {
            expected.push((sources + 1 + j, sink, *demand));
        }
        assert_eq!(edges.len(), 2 * expected.len());
        for (index, (from, to, capacity)) in expected.into_iter().enumerate() {
            let forward = &edges[2 * index];
            let reverse = &edges[2 * index + 1];
            assert_eq!(forward.to, to);
            assert_eq!(reverse.to, from);
            assert_eq!(
                u128::from(forward.capacity) + u128::from(reverse.capacity),
                u128::from(capacity),
                "paired residual capacities must conserve original capacity"
            );
        }
        let mut seen = vec![false; edges.len()];
        assert_eq!(heads.len(), sink + 1);
        for (node, head) in heads.iter().enumerate() {
            let mut index = *head;
            while index != ABSENT {
                assert!(
                    !seen[index],
                    "each residual edge belongs to exactly one adjacency list"
                );
                seen[index] = true;
                assert_eq!(edges[index ^ 1].to, node);
                index = edges[index].next;
            }
        }
        assert!(seen.into_iter().all(|value| value));
        let assignment = residual_assignment(edges, sources, count);
        let mut columns = vec![0_u128; count];
        let mut sum = 0_u128;
        for (i, row) in assignment.iter().enumerate() {
            let draw: u128 = row.iter().copied().map(u128::from).sum();
            assert!(draw <= u128::from(capacities[i].min(total)));
            assert_eq!(
                draw,
                u128::from(edges[2 * i + 1].capacity),
                "source flow must equal assigned row"
            );
            sum += draw;
            for (j, amount) in row.iter().enumerate() {
                assert!(*amount == 0 || eligible[i][j]);
                columns[j] += u128::from(*amount);
            }
        }
        let sink_edges_start = edges.len() - 2 * count;
        for (j, (draw, demand)) in columns.iter().zip(obligations).enumerate() {
            assert!(*draw <= u128::from(*demand));
            assert_eq!(
                *draw,
                u128::from(edges[sink_edges_start + 2 * j + 1].capacity)
            );
        }
        assert_eq!(sum, columns.iter().sum::<u128>());
        assert_eq!(
            sum,
            u128::from(funded),
            "funded counter must equal projected assignment"
        );
        assert!(funded <= total);
        search_properties::assert_projection_identities(edges, sources, count);
        search_properties::assert_graph_relation(
            capacities,
            obligations,
            eligible,
            &assignment,
            edges,
        );
    }

    pub(super) fn assert_closed_cut(
        capacities: &[u64],
        obligations: &[u64],
        eligible: &[Vec<bool>],
        edges: &[ResidualEdge],
        parents: &[usize],
        funded: u64,
    ) {
        let sources = capacities.len();
        let total: u64 = obligations.iter().sum();
        assert!(funded < total);
        let assignment = residual_assignment(edges, sources, obligations.len());
        let mut columns = vec![0_u64; obligations.len()];
        for (i, row) in assignment.iter().enumerate() {
            let draw: u64 = row.iter().sum();
            assert!(draw < total);
            if parents[i + 1] == ABSENT {
                assert_eq!(draw, capacities[i].min(total));
                assert_eq!(draw, capacities[i]);
            }
            for (j, amount) in row.iter().enumerate() {
                assert!(*amount < total);
                columns[j] += amount;
                if parents[i + 1] != ABSENT && eligible[i][j] {
                    assert_ne!(
                        parents[sources + 1 + j],
                        ABSENT,
                        "forward residual edge must preserve reachability"
                    );
                }
                if parents[sources + 1 + j] != ABSENT && *amount != 0 {
                    assert_ne!(
                        parents[i + 1],
                        ABSENT,
                        "reverse residual edge must preserve reachability"
                    );
                }
                if parents[i + 1] == ABSENT && parents[sources + 1 + j] != ABSENT {
                    assert_eq!(*amount, 0);
                }
            }
        }
        for (j, draw) in columns.iter().enumerate() {
            if parents[sources + 1 + j] != ABSENT {
                assert_eq!(
                    *draw, obligations[j],
                    "reachable obligation must be saturated when the sink is unreachable"
                );
            }
        }
        let selected: Vec<bool> = (0..obligations.len())
            .map(|j| parents[sources + 1 + j] == ABSENT)
            .collect();
        let mut supply = 0_u128;
        for (i, capacity) in capacities.iter().enumerate() {
            if eligible[i]
                .iter()
                .zip(&selected)
                .any(|(allowed, chosen)| *allowed && *chosen)
            {
                assert_eq!(parents[i + 1], ABSENT);
                supply += u128::from(*capacity);
            }
        }
        let delivered: u128 = columns
            .iter()
            .zip(&selected)
            .filter(|(_, chosen)| **chosen)
            .map(|(amount, _)| u128::from(*amount))
            .sum();
        let required: u128 = obligations
            .iter()
            .zip(&selected)
            .filter(|(_, chosen)| **chosen)
            .map(|(amount, _)| u128::from(*amount))
            .sum();
        assert_eq!(supply, delivered);
        assert!(delivered < required);
    }

    pub(super) fn assert_augmenting_path(
        edges: &[ResidualEdge],
        parents: &[usize],
        sink: usize,
        amount: u64,
        remaining: u64,
    ) {
        assert!(amount > 0 && amount <= remaining);
        let mut node = sink;
        let mut seen = vec![false; parents.len()];
        let mut boundary = vec![0_i128; parents.len()];
        let mut bottleneck = remaining;
        let mut saturates_edge = false;
        while node != 0 {
            assert!(!seen[node], "an augmenting path must be simple");
            seen[node] = true;
            let index = parents[node];
            assert_eq!(edges[index].to, node);
            assert!(edges[index].capacity >= amount);
            bottleneck = bottleneck.min(edges[index].capacity);
            saturates_edge |= edges[index].capacity == amount;
            let previous = edges[index ^ 1].to;
            boundary[previous] += i128::from(amount);
            boundary[node] -= i128::from(amount);
            node = previous;
        }
        assert_eq!(amount, bottleneck);
        assert!(
            amount == remaining || saturates_edge,
            "augmentation must complete demand or saturate a residual edge"
        );
        for (vertex, change) in boundary.into_iter().enumerate() {
            let expected = if vertex == 0 {
                i128::from(amount)
            } else if vertex == sink {
                -i128::from(amount)
            } else {
                0
            };
            assert_eq!(
                change, expected,
                "a complete path changes only its endpoint balances"
            );
        }
    }

    fn budget() -> HostWorkBudget {
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
    }

    #[test]
    #[should_panic(expected = "an augmenting path must be simple")]
    fn parent_path_checks_detect_a_cycle_disconnected_from_the_root() {
        let mut edges = Vec::new();
        let mut heads = vec![ABSENT; 3];
        add_edge(&mut edges, &mut heads, 1, 2, 1);
        add_edge(&mut edges, &mut heads, 2, 1, 1);
        assert_augmenting_path(&edges, &[0, 2, 0], 2, 1, 1);
    }

    #[test]
    fn paired_transfer_matches_complete_small_arithmetic_reference() {
        for forward in 0_u64..=16 {
            for reverse in 0_u64..=16 {
                for amount in 0_u64..=32 {
                    let expected = if amount <= forward {
                        Some((forward - amount, reverse + amount))
                    } else {
                        None
                    };
                    assert_eq!(
                        checked_residual_transfer(forward, reverse, amount),
                        expected
                    );
                }
            }
        }
        assert_eq!(
            checked_residual_transfer(u64::MAX, 0, u64::MAX),
            Some((0, u64::MAX))
        );
        assert_eq!(
            checked_residual_transfer(0, u64::MAX, 0),
            Some((0, u64::MAX))
        );
        assert_eq!(checked_residual_transfer(1, u64::MAX, 1), None);
        assert_eq!(checked_residual_transfer(0, 0, 1), None);
    }

    fn one_source_graph(
        capacity: u64,
        demand: u64,
        funded: u64,
    ) -> (Vec<ResidualEdge>, Vec<usize>) {
        let mut edges = Vec::new();
        let mut heads = vec![ABSENT; 4];
        add_edge(&mut edges, &mut heads, 0, 1, capacity.min(demand));
        add_edge(&mut edges, &mut heads, 1, 2, demand);
        add_edge(&mut edges, &mut heads, 2, 3, demand);
        for index in (0..edges.len()).step_by(2) {
            edges[index].capacity -= funded;
            edges[index + 1].capacity = funded;
        }
        (edges, heads)
    }

    #[test]
    #[should_panic(expected = "paired residual capacities must conserve original capacity")]
    fn invariant_checks_detect_a_missing_reverse_update() {
        let (mut edges, heads) = one_source_graph(1, 1, 0);
        edges[2].capacity = 0;
        assert_residual_state(&[1], &[1], &[vec![true]], &edges, &heads, 0);
    }

    #[test]
    #[should_panic(expected = "source flow must equal assigned row")]
    fn invariant_checks_detect_disconnected_source_funding() {
        let (mut edges, heads) = one_source_graph(1, 1, 0);
        edges[2].capacity = 0;
        edges[3].capacity = 1;
        assert_residual_state(&[1], &[1], &[vec![true]], &edges, &heads, 0);
    }

    #[test]
    #[should_panic(expected = "funded counter must equal projected assignment")]
    fn invariant_checks_detect_a_counter_without_funding() {
        let (edges, heads) = one_source_graph(1, 1, 0);
        assert_residual_state(&[1], &[1], &[vec![true]], &edges, &heads, 1);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn full_width_reverse_reassignment_increases_only_actual_funding(amount in 1_u64..=u64::MAX / 2) {
            let result = solve(&[amount, amount], &[amount, amount], &[vec![false, true], vec![true, true]]);
            let FundingFeasibility::Feasible { assignment, totals } = result else {
                return Err(TestCaseError::fail("feasible reassignment rejected"));
            };
            prop_assert_eq!(assignment, vec![vec![0, amount], vec![amount, 0]]);
            prop_assert_eq!(totals.source_debits(), &[amount, amount]);
        }
    }

    #[test]
    #[should_panic(expected = "forward residual edge must preserve reachability")]
    fn invariant_checks_detect_incomplete_forward_search() {
        let (edges, _) = one_source_graph(1, 1, 0);
        assert_closed_cut(
            &[1],
            &[1],
            &[vec![true]],
            &edges,
            &[0, 0, ABSENT, ABSENT],
            0,
        );
    }

    #[test]
    #[should_panic(expected = "reverse residual edge must preserve reachability")]
    fn invariant_checks_detect_incomplete_reverse_search() {
        let (edges, _) = one_source_graph(1, 2, 1);
        assert_closed_cut(
            &[1],
            &[2],
            &[vec![true]],
            &edges,
            &[0, ABSENT, 2, ABSENT],
            1,
        );
    }

    #[test]
    #[should_panic(
        expected = "reachable obligation must be saturated when the sink is unreachable"
    )]
    fn invariant_checks_detect_an_omitted_sink_path() {
        let (edges, _) = one_source_graph(2, 2, 1);
        assert_closed_cut(&[2], &[2], &[vec![true]], &edges, &[0, 0, 2, ABSENT], 1);
    }

    fn limits() -> FundingSearchLimits {
        FundingSearchLimits {
            source_cap: NonZeroUsize::new(128).unwrap(),
            obligation_cap: NonZeroUsize::new(128).unwrap(),
        }
    }

    fn solve(
        capacities: &[u64],
        obligations: &[u64],
        eligible: &[Vec<bool>],
    ) -> FundingFeasibility {
        solve_funding_feasibility(capacities, obligations, eligible, limits(), &budget()).unwrap()
    }

    fn three_source_reference(
        capacities: [u64; 3],
        demands: &[u64],
        eligible: &[Vec<bool>],
        column: usize,
    ) -> bool {
        if column == demands.len() {
            return true;
        }
        let need = demands[column];
        (0..=need.min(capacities[0])).any(|a| {
            (0..=(need - a).min(capacities[1])).any(|b| {
                let c = need - a - b;
                c <= capacities[2]
                    && (a == 0 || eligible[0][column])
                    && (b == 0 || eligible[1][column])
                    && (c == 0 || eligible[2][column])
                    && three_source_reference(
                        [capacities[0] - a, capacities[1] - b, capacities[2] - c],
                        demands,
                        eligible,
                        column + 1,
                    )
            })
        })
    }

    #[test]
    fn reverse_residual_path_repairs_a_greedy_choice() {
        let result = solve(&[1, 1], &[1, 1], &[vec![false, true], vec![true, true]]);
        let FundingFeasibility::Feasible { assignment, totals } = result else {
            panic!("feasible problem rejected")
        };
        assert_eq!(assignment, vec![vec![0, 1], vec![1, 0]]);
        assert_eq!(totals.source_debits(), &[1, 1]);
    }

    #[test]
    fn rejection_carries_an_independently_valid_deficit() {
        let capacities = [100, 1];
        let demands = [1, 2];
        let eligible = [vec![true, false], vec![true, true]];
        let FundingFeasibility::Infeasible {
            selected_obligations,
        } = solve(&capacities, &demands, &eligible)
        else {
            panic!("infeasible problem accepted")
        };
        assert_eq!(selected_obligations, vec![false, true]);
        assert_eq!(
            check_funding_deficit(
                &capacities,
                &demands,
                &eligible,
                &selected_obligations,
                limits().source_cap,
                limits().obligation_cap
            ),
            Ok(true)
        );
    }

    #[test]
    fn zero_demands_and_maximum_amounts_do_not_need_unit_expansion() {
        let FundingFeasibility::Feasible { totals, assignment } =
            solve(&[0, u64::MAX], &[], &[vec![], vec![]])
        else {
            panic!("zero demand rejected")
        };
        assert_eq!(totals.total(), 0);
        assert_eq!(assignment, vec![Vec::<u64>::new(), Vec::new()]);
        let mut usages = Vec::new();
        for amount in [1, u64::MAX] {
            let work = budget();
            let result = solve_funding_feasibility(
                &[amount, amount],
                &[amount],
                &[vec![true], vec![true]],
                limits(),
                &work,
            )
            .unwrap();
            let FundingFeasibility::Feasible { totals, .. } = result else {
                panic!("maximum amount rejected")
            };
            assert_eq!(totals.total(), amount);
            usages.push(work.usages());
        }
        assert_eq!(usages[0], usages[1]);
        assert!(matches!(
            solve(&[u64::MAX], &[1], &[vec![false]]),
            FundingFeasibility::Infeasible { .. }
        ));
    }

    #[test]
    fn work_exhaustion_is_not_financial_infeasibility() {
        for dimension in [
            HostWorkDimension::SearchCandidates,
            HostWorkDimension::SearchStateBytes,
            HostWorkDimension::VerificationOperations,
        ] {
            let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(100_000));
            bounds.set(dimension, HostWorkLimit::new(0));
            let work = HostWorkBudget::new(bounds);
            let result = solve_funding_feasibility(&[1], &[1], &[vec![true]], limits(), &work);
            assert!(matches!(result, Err(FundingSearchError::HostWork(_))));
            assert!(work.is_rejected());
        }
    }

    #[test]
    fn every_budget_prefix_discards_partial_search_without_a_financial_result() {
        let demands = [1, 1];
        let eligible = [vec![false, true], vec![true, true]];
        for capacities in [[1, 1], [0, 1]] {
            let measured = budget();
            let expected =
                solve_funding_feasibility(&capacities, &demands, &eligible, limits(), &measured)
                    .unwrap();
            for dimension in [
                HostWorkDimension::SearchCandidates,
                HostWorkDimension::SearchStateBytes,
                HostWorkDimension::VerificationOperations,
            ] {
                let required = measured.usage(dimension).get();
                for available in 0..=required {
                    let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
                    bounds.set(dimension, HostWorkLimit::new(available));
                    let work = HostWorkBudget::new(bounds);
                    let result = solve_funding_feasibility(
                        &capacities,
                        &demands,
                        &eligible,
                        limits(),
                        &work,
                    );
                    if available < required {
                        assert!(matches!(result, Err(FundingSearchError::HostWork(_))));
                        assert_eq!(work.rejection().unwrap().dimension(), dimension);
                    } else {
                        assert_eq!(result, Ok(expected.clone()));
                        assert!(!work.is_rejected());
                    }
                    assert_eq!(solve(&capacities, &demands, &eligible), expected);
                }
            }
        }
    }

    #[test]
    fn invalid_dimensions_limits_and_total_are_rejected() {
        for (capacities, demands, eligible, expected) in [
            (vec![], vec![], vec![], FundingAssignmentError::EmptySources),
            (
                vec![1],
                vec![1],
                vec![],
                FundingAssignmentError::InvalidDimensions,
            ),
            (
                vec![1],
                vec![1],
                vec![vec![]],
                FundingAssignmentError::InvalidDimensions,
            ),
            (
                vec![u64::MAX],
                vec![u64::MAX, 1],
                vec![vec![true, true]],
                FundingAssignmentError::Overflow,
            ),
            (
                vec![0; 129],
                vec![],
                vec![vec![]; 129],
                FundingAssignmentError::TooManySources,
            ),
            (
                vec![0],
                vec![0; 129],
                vec![vec![false; 129]],
                FundingAssignmentError::TooManyObligations,
            ),
        ] {
            assert_eq!(
                solve_funding_feasibility(&capacities, &demands, &eligible, limits(), &budget()),
                Err(FundingSearchError::InvalidProblem(expected))
            );
        }
    }

    #[test]
    fn complete_two_source_problem_sets_match_allocation_enumeration() {
        for mask in 0..16 {
            let eligible = vec![vec![mask & 1 != 0, mask & 2 != 0], vec![
                mask & 4 != 0,
                mask & 8 != 0,
            ]];
            for c0 in 0..=2 {
                for c1 in 0..=2 {
                    for q0 in 0..=2 {
                        for q1 in 0..=2 {
                            let reference = (0..=q0).any(|a| {
                                (0..=q1).any(|b| {
                                    a + b <= c0
                                        && q0 - a + q1 - b <= c1
                                        && (a == 0 || eligible[0][0])
                                        && (b == 0 || eligible[0][1])
                                        && (a == q0 || eligible[1][0])
                                        && (b == q1 || eligible[1][1])
                                })
                            });
                            assert_eq!(
                                matches!(
                                    solve(&[c0, c1], &[q0, q1], &eligible),
                                    FundingFeasibility::Feasible { .. }
                                ),
                                reference
                            );
                        }
                    }
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn ranked_parent_forests_produce_simple_capacity_bounded_paths(
            vertices in 2_usize..130,
            choices in prop::collection::vec(any::<u16>(), 1..129),
            capacities in prop::collection::vec(any::<u64>(), 1..129),
            remaining in 1_u64..=u64::MAX,
            force_chain in any::<bool>(),
            reverse_edges in any::<bool>(),
        ) {
            let mut edges = Vec::new();
            let mut heads = vec![ABSENT; vertices];
            let mut parents = vec![ABSENT; vertices];
            parents[0] = 0;
            for (node, parent_edge) in parents.iter_mut().enumerate().skip(1) {
                let predecessor = if force_chain { node - 1 } else { usize::from(choices[node % choices.len()]) % node };
                let capacity = capacities[node % capacities.len()].max(1);
                if reverse_edges && choices[node % choices.len()] & 1 == 1 {
                    *parent_edge = edges.len() + 1;
                    add_edge(&mut edges, &mut heads, node, predecessor, capacity);
                    edges[*parent_edge - 1].capacity = 0;
                    edges[*parent_edge].capacity = capacity;
                } else {
                    *parent_edge = edges.len();
                    add_edge(&mut edges, &mut heads, predecessor, node, capacity);
                }
            }
            let sink = vertices - 1;
            let mut node = sink;
            let mut bound = remaining;
            let mut length = 0;
            while node != 0 {
                let index = parents[node];
                bound = bound.min(edges[index].capacity);
                let predecessor = edges[index ^ 1].to;
                prop_assert!(predecessor < node);
                node = predecessor;
                length += 1;
            }
            prop_assert!(length <= sink);
            assert_augmenting_path(&edges, &parents, sink, bound, remaining);
            let snapshot = search_properties::AugmentationSnapshot::new(&edges, &parents, sink, bound);
            node = sink;
            let mut applied = 0;
            while node != 0 {
                node = augment_edge(&mut edges, parents[node], bound).expect("ranked feasible path must update");
                applied += 1;
            }
            prop_assert_eq!(applied, length);
            snapshot.assert_completed(&edges);
        }

        #[test]
        fn arbitrary_residual_transfer_histories_preserve_conservation_and_bounds(
            total in any::<u64>(),
            split in any::<u64>(),
            operations in prop::collection::vec((any::<bool>(), any::<u64>()), 0..257),
        ) {
            let forward = (u128::from(split) % (u128::from(total) + 1)) as u64;
            let mut pair = (forward, total - forward);
            for (reversed, amount) in operations {
                let before = pair;
                let (from, to) = if reversed { (pair.1, pair.0) } else { pair };
                let transferred = checked_residual_transfer(from, to, amount);
                prop_assert_eq!(transferred.is_some(), amount <= from);
                if let Some((next_from, next_to)) = transferred {
                    prop_assert_eq!(u128::from(next_from) + u128::from(next_to), u128::from(total));
                    prop_assert!(next_from <= total && next_to <= total);
                    prop_assert_eq!(checked_residual_transfer(next_to, next_from, amount), Some((to, from)));
                    pair = if reversed { (next_to, next_from) } else { (next_from, next_to) };
                } else {
                    prop_assert_eq!(pair, before);
                }
            }
            prop_assert_eq!(u128::from(pair.0) + u128::from(pair.1), u128::from(total));
        }

        #[test]
        fn arbitrary_pair_inputs_match_wide_checked_reference(
            forward in any::<u64>(), reverse in any::<u64>(), amount in any::<u64>(),
        ) {
            let remaining = u128::from(forward).checked_sub(u128::from(amount));
            let returned = u128::from(reverse) + u128::from(amount);
            let expected = remaining.filter(|_| returned <= u128::from(u64::MAX))
                .map(|value| (value as u64, returned as u64));
            prop_assert_eq!(checked_residual_transfer(forward, reverse, amount), expected);
        }

        #[test]
        fn arbitrary_three_source_problems_match_independent_complete_search(
            capacities in prop::array::uniform3(0_u64..10),
            demands in prop::collection::vec(0_u64..5, 0..4),
            bits in prop::array::uniform9(any::<bool>()),
        ) {
            let eligible: Vec<Vec<bool>> = (0..3).map(|i| (0..demands.len()).map(|j| bits[i * 3 + j]).collect()).collect();
            let reference = three_source_reference(capacities, &demands, &eligible, 0);
            let first = solve(&capacities, &demands, &eligible);
            prop_assert_eq!(matches!(first, FundingFeasibility::Feasible { .. }), reference);
            let second = solve(&capacities, &demands, &eligible);
            prop_assert_eq!(&first, &second);
            let total: u64 = demands.iter().sum();
            let clipped = capacities.map(|capacity| capacity.min(total));
            prop_assert_eq!(&first, &solve(&clipped, &demands, &eligible));
            let mut reversed_capacities = capacities;
            reversed_capacities.reverse();
            let mut reversed = eligible;
            reversed.reverse();
            prop_assert_eq!(matches!(solve(&reversed_capacities, &demands, &reversed), FundingFeasibility::Feasible { .. }), reference);
        }

        #[test]
        fn feasible_assignments_through_128_sources_remain_feasible(
            sources in 1_usize..129,
            demands in prop::collection::vec(0_u64..1_000_000, 0..17),
            owners in prop::collection::vec(any::<u16>(), 1..33),
        ) {
            let mut capacities = vec![0_u64; sources];
            let mut eligible = vec![vec![false; demands.len()]; sources];
            for (j, amount) in demands.iter().enumerate() {
                let owner = usize::from(owners[j % owners.len()]) % sources;
                capacities[owner] += amount;
                eligible[owner][j] = true;
            }
            let result = solve(&capacities, &demands, &eligible);
            prop_assert!(matches!(result, FundingFeasibility::Feasible { .. }), "a constructed feasible assignment must remain feasible");
        }
    }
}
