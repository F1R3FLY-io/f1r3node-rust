use std::mem::size_of;
use std::num::NonZeroUsize;

use models::rust::host_work::HostWorkDimension;

use super::funding_feasibility::{filled_vec, reserve_work};
use super::{
    check_funding_deficit, solve_funding_feasibility, FundingAssignmentError, FundingFeasibility,
    FundingMinimaxProblem, FundingSearchError, FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct FundingCellQuery<'a> {
    pub exact: FundingMinimaxProblem<'a>,
    pub source: usize,
    pub obligation: usize,
    pub maximum: u64,
}

struct SplitCellProblem {
    capacities: Vec<u64>,
    eligible: Vec<Vec<bool>>,
    limits: FundingSearchLimits,
}

fn split_query(
    query: FundingCellQuery<'_>,
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<SplitCellProblem, FundingSearchError> {
    let problem = query.exact;
    let sources = problem.capacities.len();
    let obligations = problem.obligations.len();
    if sources == 0 {
        return Err(FundingAssignmentError::EmptySources.into());
    }
    if sources > limits.source_cap.get() {
        return Err(FundingAssignmentError::TooManySources.into());
    }
    if obligations > limits.obligation_cap.get() {
        return Err(FundingAssignmentError::TooManyObligations.into());
    }
    let internal_sources = sources.checked_add(1).ok_or(FundingSearchError::Overflow)?;
    let cells = internal_sources
        .checked_mul(obligations)
        .ok_or(FundingSearchError::Overflow)?;
    let work = cells
        .checked_add(internal_sources)
        .and_then(|n| n.checked_add(obligations))
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, work)?;
    if problem.eligible.len() != sources
        || problem.eligible.iter().any(|row| row.len() != obligations)
    {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
    if query.source >= sources || query.obligation >= obligations {
        return Err(FundingSearchError::InvalidAssignmentCell);
    }
    let contributions = problem
        .capacities
        .iter()
        .try_fold(0_u64, |sum, value| sum.checked_add(*value))
        .ok_or(FundingAssignmentError::Overflow)?;
    let demand = problem
        .obligations
        .iter()
        .try_fold(0_u64, |sum, value| sum.checked_add(*value))
        .ok_or(FundingAssignmentError::Overflow)?;
    if contributions != demand {
        return Err(FundingSearchError::UnequalFundingTotals);
    }
    let bytes = internal_sources
        .checked_mul(size_of::<u64>() + size_of::<Vec<bool>>())
        .and_then(|n| n.checked_add(cells))
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    let mut capacities = filled_vec(internal_sources, 0_u64)?;
    capacities[..sources].copy_from_slice(problem.capacities);
    let limit = query.maximum.min(capacities[query.source]);
    capacities[sources] = capacities[query.source] - limit;
    capacities[query.source] = limit;
    let mut eligible = filled_vec(internal_sources, Vec::new())?;
    for (index, row) in eligible.iter_mut().enumerate() {
        *row = filled_vec(obligations, false)?;
        if index == sources {
            row.copy_from_slice(&problem.eligible[query.source]);
            row[query.obligation] = false;
        } else {
            row.copy_from_slice(&problem.eligible[index]);
        }
    }
    Ok(SplitCellProblem {
        capacities,
        eligible,
        limits: FundingSearchLimits {
            source_cap: NonZeroUsize::new(internal_sources).ok_or(FundingSearchError::Overflow)?,
            obligation_cap: limits.obligation_cap,
        },
    })
}

pub fn check_funding_cell_deficit(
    query: FundingCellQuery<'_>,
    selected_obligations: &[bool],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<bool, FundingSearchError> {
    let split = split_query(query, limits, budget)?;
    let work = split
        .capacities
        .len()
        .checked_mul(query.exact.obligations.len())
        .and_then(|n| n.checked_add(split.capacities.len()))
        .and_then(|n| n.checked_add(query.exact.obligations.len()))
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::VerificationOperations, work)?;
    Ok(check_funding_deficit(
        &split.capacities,
        query.exact.obligations,
        &split.eligible,
        selected_obligations,
        split.limits.source_cap,
        split.limits.obligation_cap,
    )?)
}

pub fn solve_funding_cell_bound(
    query: FundingCellQuery<'_>,
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<FundingFeasibility, FundingSearchError> {
    let split = split_query(query, limits, budget)?;
    let result = solve_funding_feasibility(
        &split.capacities,
        query.exact.obligations,
        &split.eligible,
        split.limits,
        budget,
    )?;
    match result {
        FundingFeasibility::Feasible { mut assignment, .. } => {
            let extra = assignment.pop().ok_or(FundingSearchError::InvalidResult)?;
            reserve_work(budget, HostWorkDimension::SearchCandidates, extra.len())?;
            for (amount, extra) in assignment[query.source].iter_mut().zip(extra) {
                *amount = amount
                    .checked_add(extra)
                    .ok_or(FundingSearchError::Overflow)?;
            }
            let totals = query.exact.check_assignment(&assignment, limits, budget)?;
            reserve_work(
                budget,
                HostWorkDimension::VerificationOperations,
                query.exact.capacities.len(),
            )?;
            if totals.source_debits() != query.exact.capacities
                || assignment[query.source][query.obligation] > query.maximum
            {
                return Err(FundingSearchError::InvalidResult);
            }
            Ok(FundingFeasibility::Feasible { assignment, totals })
        }
        FundingFeasibility::Infeasible {
            selected_obligations,
        } => {
            if !check_funding_cell_deficit(query, &selected_obligations, limits, budget)? {
                return Err(FundingSearchError::InvalidResult);
            }
            Ok(FundingFeasibility::Infeasible {
                selected_obligations,
            })
        }
    }
}

#[cfg(test)]
#[path = "funding_cell_bound_tests.rs"]
mod tests;
