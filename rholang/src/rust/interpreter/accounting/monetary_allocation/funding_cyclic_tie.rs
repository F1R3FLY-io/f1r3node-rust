use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::funding_feasibility::{filled_vec, reserve_work};
use super::{
    check_lower_funding_cut, solve_box_funding_feasibility, FundingAssignmentTotals,
    FundingBoxFeasibility, FundingBoxProblem, FundingOptimalDomain, FundingSearchError,
    FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FundingPrefixExclusion {
    Capacity,
    LowerCut { selected_sources: Vec<bool> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingCyclicTieCertificate {
    exclusions: Vec<FundingPrefixExclusion>,
}

impl FundingCyclicTieCertificate {
    pub fn exclusions(&self) -> &[FundingPrefixExclusion] { &self.exclusions }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingCyclicTieSelection {
    pub assignment: Vec<Vec<u64>>,
    pub totals: FundingAssignmentTotals,
    pub certificate: FundingCyclicTieCertificate,
}

fn copied_bounds(
    domain: &FundingOptimalDomain,
    budget: &HostWorkBudget,
) -> Result<(Vec<u64>, Vec<u64>), FundingSearchError> {
    let problem = domain.problem();
    let count = problem.upper.len();
    reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(2 * size_of::<u64>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut lower = filled_vec(count, 0_u64)?;
    let mut upper = filled_vec(count, 0_u64)?;
    lower.copy_from_slice(problem.lower);
    upper.copy_from_slice(problem.upper);
    Ok((lower, upper))
}

pub fn check_funding_cyclic_tie_certificate(
    domain: &FundingOptimalDomain,
    assignment: &[Vec<u64>],
    cursor: usize,
    exclusions: &[FundingPrefixExclusion],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<bool, FundingSearchError> {
    let count = domain.problem().upper.len();
    if cursor >= count {
        return Err(FundingSearchError::InvalidPriorityCursor);
    }
    let totals = domain.check_assignment(assignment, limits, budget)?;
    if exclusions.len() != count {
        return Ok(false);
    }
    let (mut lower, mut upper) = copied_bounds(domain, budget)?;
    let mut position = cursor;
    for exclusion in exclusions {
        reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
        let selected = totals.source_debits()[position];
        match exclusion {
            FundingPrefixExclusion::Capacity => {
                if selected != upper[position] {
                    return Ok(false);
                }
            }
            FundingPrefixExclusion::LowerCut { selected_sources } => {
                if selected >= upper[position] {
                    return Ok(false);
                }
                lower[position] = selected
                    .checked_add(1)
                    .ok_or(FundingSearchError::Overflow)?;
                let probe = FundingBoxProblem {
                    lower: &lower,
                    upper: &upper,
                    ..domain.problem()
                };
                if !check_lower_funding_cut(probe, selected_sources, limits, budget)? {
                    return Ok(false);
                }
            }
        }
        lower[position] = selected;
        upper[position] = selected;
        position = if position == count - 1 {
            0
        } else {
            position + 1
        };
    }
    Ok(true)
}

pub fn select_funding_cyclic_tie(
    domain: &FundingOptimalDomain,
    cursor: usize,
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<FundingCyclicTieSelection, FundingSearchError> {
    let count = domain.problem().upper.len();
    if cursor >= count {
        return Err(FundingSearchError::InvalidPriorityCursor);
    }
    let (mut assignment, mut totals) =
        match solve_box_funding_feasibility(domain.problem(), limits, budget)? {
            FundingBoxFeasibility::Feasible { assignment, totals } => (assignment, totals),
            FundingBoxFeasibility::UpperDeficit { .. }
            | FundingBoxFeasibility::LowerDeficit { .. } => {
                return Err(FundingSearchError::InvalidResult)
            }
        };
    let (mut lower, mut upper) = copied_bounds(domain, budget)?;
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<FundingPrefixExclusion>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut exclusions = Vec::new();
    exclusions
        .try_reserve_exact(count)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut position = cursor;
    for _ in 0..count {
        reserve_work(budget, HostWorkDimension::SearchCandidates, 1)?;
        let mut low = totals.source_debits()[position];
        let mut high = upper[position].min(totals.total());
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            lower[position] = middle;
            match solve_box_funding_feasibility(
                FundingBoxProblem {
                    lower: &lower,
                    upper: &upper,
                    ..domain.problem()
                },
                limits,
                budget,
            )? {
                FundingBoxFeasibility::Feasible {
                    assignment: next,
                    totals: next_totals,
                } => {
                    let value = next_totals.source_debits()[position];
                    if value < middle || value > high {
                        return Err(FundingSearchError::InvalidResult);
                    }
                    low = value;
                    assignment = next;
                    totals = next_totals;
                }
                FundingBoxFeasibility::LowerDeficit { .. } => high = middle - 1,
                FundingBoxFeasibility::UpperDeficit { .. } => {
                    return Err(FundingSearchError::InvalidResult)
                }
            }
        }
        if totals.source_debits()[position] != low {
            return Err(FundingSearchError::InvalidResult);
        }
        let exclusion = if low == upper[position] {
            FundingPrefixExclusion::Capacity
        } else {
            lower[position] = low.checked_add(1).ok_or(FundingSearchError::Overflow)?;
            match solve_box_funding_feasibility(
                FundingBoxProblem {
                    lower: &lower,
                    upper: &upper,
                    ..domain.problem()
                },
                limits,
                budget,
            )? {
                FundingBoxFeasibility::LowerDeficit { selected_sources } => {
                    FundingPrefixExclusion::LowerCut { selected_sources }
                }
                FundingBoxFeasibility::Feasible { .. }
                | FundingBoxFeasibility::UpperDeficit { .. } => {
                    return Err(FundingSearchError::InvalidResult)
                }
            }
        };
        exclusions.push(exclusion);
        lower[position] = low;
        upper[position] = low;
        position = if position == count - 1 {
            0
        } else {
            position + 1
        };
    }
    if domain.check_assignment(&assignment, limits, budget)? != totals
        || !check_funding_cyclic_tie_certificate(
            domain,
            &assignment,
            cursor,
            &exclusions,
            limits,
            budget,
        )?
    {
        return Err(FundingSearchError::InvalidResult);
    }
    Ok(FundingCyclicTieSelection {
        assignment,
        totals,
        certificate: FundingCyclicTieCertificate { exclusions },
    })
}
