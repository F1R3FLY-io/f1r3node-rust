use std::mem::size_of;
use std::num::NonZeroUsize;

use models::rust::host_work::HostWorkDimension;
use thiserror::Error;

use super::{
    filled_vec, reserve_work, solve_box_funding_feasibility, FundingAssignmentError,
    FundingAssignmentTotals, FundingBoxFeasibility, FundingBoxProblem, FundingSearchError,
    FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct FundingFamilyProblem<'a> {
    pub capacities: &'a [u64],
    pub cases: &'a [FundingBoxProblem<'a>],
    pub exposure_limit: u128,
}

#[derive(Clone, Copy, Debug)]
pub struct FundingFamilyLimits {
    pub search: FundingSearchLimits,
    pub case_cap: NonZeroUsize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingFamilyWitness {
    assignments: Vec<Vec<Vec<u64>>>,
    totals: Vec<FundingAssignmentTotals>,
    holds: Vec<u64>,
    total_held: u128,
}

impl FundingFamilyWitness {
    pub fn assignments(&self) -> &[Vec<Vec<u64>>] { &self.assignments }
    pub fn totals(&self) -> &[FundingAssignmentTotals] { &self.totals }
    pub fn holds(&self) -> &[u64] { &self.holds }
    pub fn total_held(&self) -> u128 { self.total_held }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum FundingFamilyError {
    #[error("funding family outcome groups are not contiguous equivalent canonical domains")]
    InvalidOutcomeGroups,
    #[error("funding family requires at least one outcome")]
    EmptyCases,
    #[error("funding family exceeds the configured outcome cap")]
    TooManyCases,
    #[error("funding family priority keys have different dimensions, outcome amounts, or cursors")]
    IncompatiblePriorityContext,
    #[error(transparent)]
    Search(#[from] FundingSearchError),
}

pub(super) struct HoldBox {
    pub(super) lower: Vec<u64>,
    pub(super) upper: Vec<u64>,
}

pub(super) fn vector(
    count: usize,
    budget: &HostWorkBudget,
) -> Result<Vec<u64>, FundingSearchError> {
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<u64>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
    filled_vec(count, 0)
}

pub(super) fn sum(values: &[u64]) -> Result<u128, FundingSearchError> {
    values.iter().try_fold(0_u128, |total, value| {
        total
            .checked_add(u128::from(*value))
            .ok_or(FundingSearchError::Overflow)
    })
}

pub(super) fn push(
    stack: &mut Vec<HoldBox>,
    bounds: HoldBox,
    budget: &HostWorkBudget,
) -> Result<(), FundingSearchError> {
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        size_of::<HoldBox>(),
    )?;
    stack
        .try_reserve_exact(1)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    stack.push(bounds);
    Ok(())
}

fn validate(
    problem: FundingFamilyProblem<'_>,
    limits: FundingFamilyLimits,
    budget: &HostWorkBudget,
) -> Result<HoldBox, FundingFamilyError> {
    let n = problem.capacities.len();
    if n == 0 {
        return Err(FundingSearchError::from(FundingAssignmentError::EmptySources).into());
    }
    if n > limits.search.source_cap.get() {
        return Err(FundingSearchError::from(FundingAssignmentError::TooManySources).into());
    }
    if problem.cases.is_empty() {
        return Err(FundingFamilyError::EmptyCases);
    }
    if problem.cases.len() > limits.case_cap.get() {
        return Err(FundingFamilyError::TooManyCases);
    }
    let mut lower = vector(n, budget)?;
    let mut upper = vector(n, budget)?;
    for case in problem.cases {
        if case.upper.len() != n {
            return Err(FundingSearchError::from(FundingAssignmentError::InvalidDimensions).into());
        }
        if case.obligations.len() > limits.search.obligation_cap.get() {
            return Err(
                FundingSearchError::from(FundingAssignmentError::TooManyObligations).into(),
            );
        }
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            case.obligations
                .len()
                .checked_add(1)
                .and_then(|width| n.checked_mul(width))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        case.validate(limits.search, budget)?;
        let total = case.obligations.iter().try_fold(0_u64, |total, amount| {
            total
                .checked_add(*amount)
                .ok_or(FundingSearchError::Overflow)
        })?;
        for i in 0..n {
            lower[i] = lower[i].max(case.lower[i]);
            upper[i] = upper[i].max(case.upper[i].min(total).min(problem.capacities[i]));
        }
    }
    Ok(HoldBox { lower, upper })
}

fn relaxed_witness(
    problem: FundingFamilyProblem<'_>,
    bounds: &HoldBox,
    limits: FundingFamilyLimits,
    budget: &HostWorkBudget,
) -> Result<Option<FundingFamilyWitness>, FundingSearchError> {
    let n = problem.capacities.len();
    let count = problem.cases.len();
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<Vec<Vec<u64>>>() + size_of::<FundingAssignmentTotals>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut assignments = Vec::new();
    assignments
        .try_reserve_exact(count)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut totals = Vec::new();
    totals
        .try_reserve_exact(count)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut holds = vector(n, budget)?;
    let mut upper = vector(n, budget)?;
    for case in problem.cases {
        reserve_work(budget, HostWorkDimension::SearchCandidates, n)?;
        for (i, value) in upper.iter_mut().enumerate() {
            *value = bounds.upper[i].min(case.upper[i]);
            if case.lower[i] > *value {
                return Ok(None);
            }
        }
        match solve_box_funding_feasibility(
            FundingBoxProblem {
                upper: &upper,
                ..*case
            },
            limits.search,
            budget,
        )? {
            FundingBoxFeasibility::Feasible {
                assignment,
                totals: draws,
            } => {
                for (hold, draw) in holds.iter_mut().zip(draws.source_debits()) {
                    *hold = (*hold).max(*draw);
                }
                assignments.push(assignment);
                totals.push(draws);
            }
            FundingBoxFeasibility::UpperDeficit { .. }
            | FundingBoxFeasibility::LowerDeficit { .. } => return Ok(None),
        }
    }
    let total_held = sum(&holds)?;
    Ok(Some(FundingFamilyWitness {
        assignments,
        totals,
        holds,
        total_held,
    }))
}

pub(super) fn split_hold_box(
    mut bounds: HoldBox,
    stack: &mut Vec<HoldBox>,
    budget: &HostWorkBudget,
) -> Result<(), FundingSearchError> {
    let n = bounds.lower.len();
    reserve_work(budget, HostWorkDimension::SearchCandidates, n)?;
    let mut split = None;
    let mut width = 0;
    for (i, (lo, hi)) in bounds.lower.iter().zip(&bounds.upper).enumerate() {
        if hi - lo > width {
            width = hi - lo;
            split = Some(i);
        }
    }
    let source = split.ok_or(FundingSearchError::InvalidResult)?;
    let middle = bounds.lower[source] + width / 2;
    let mut right_lower = vector(n, budget)?;
    right_lower.copy_from_slice(&bounds.lower);
    right_lower[source] = middle.checked_add(1).ok_or(FundingSearchError::Overflow)?;
    let mut left_upper = vector(n, budget)?;
    left_upper.copy_from_slice(&bounds.upper);
    left_upper[source] = middle;
    let right_upper = std::mem::replace(&mut bounds.upper, left_upper);
    push(
        stack,
        HoldBox {
            lower: right_lower,
            upper: right_upper,
        },
        budget,
    )?;
    push(stack, bounds, budget)
}

pub fn solve_funding_family_feasibility(
    problem: FundingFamilyProblem<'_>,
    limits: FundingFamilyLimits,
    budget: &HostWorkBudget,
) -> Result<Option<FundingFamilyWitness>, FundingFamilyError> {
    let root = validate(problem, limits, budget)?;
    if root.lower.iter().zip(&root.upper).any(|(lo, hi)| lo > hi) {
        return Ok(None);
    }
    let n = problem.capacities.len();
    let mut stack = Vec::new();
    push(&mut stack, root, budget)?;
    while let Some(bounds) = stack.pop() {
        reserve_work(budget, HostWorkDimension::SearchCandidates, n)?;
        if sum(&bounds.lower)? > problem.exposure_limit {
            continue;
        }
        let Some(witness) = relaxed_witness(problem, &bounds, limits, budget)? else {
            continue;
        };
        let padded_total =
            bounds
                .lower
                .iter()
                .zip(&witness.holds)
                .try_fold(0_u128, |total, (lo, actual)| {
                    total
                        .checked_add(u128::from((*lo).max(*actual)))
                        .ok_or(FundingSearchError::Overflow)
                })?;
        if padded_total <= problem.exposure_limit {
            return Ok(Some(witness));
        }
        drop(witness);
        split_hold_box(bounds, &mut stack, budget)?;
    }
    Ok(None)
}

#[cfg(test)]
#[path = "funding_family_tests.rs"]
mod tests;
