use std::cmp::Ordering;
use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::funding_family::{push, split_hold_box, sum, vector, HoldBox};
use super::{
    reserve_work, select_fixed_funding_policy, select_funding_with_unit_fee,
    FundingAssignmentError, FundingAssignmentTotals, FundingBoxProblem, FundingFamilyError,
    FundingFamilyLimits, FundingFamilyResourcePriority, FundingMinimaxProblem, FundingPolicyResult,
    FundingSearchError,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct FundingOutcomeProblem<'a> {
    pub resources: FundingMinimaxProblem<'a>,
    pub unit_fee_eligible: Option<&'a [bool]>,
}

#[derive(Clone, Copy, Debug)]
pub struct FundingFamilyOptimizationProblem<'a> {
    pub capacities: &'a [u64],
    pub outcomes: &'a [FundingOutcomeProblem<'a>],
    pub exposure_limit: u128,
    pub resource_cursor: usize,
    pub fee_cursor: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingOutcomeAllocation {
    resources: Vec<Vec<u64>>,
    resource_totals: FundingAssignmentTotals,
    fee_debits: Vec<u64>,
}

impl FundingOutcomeAllocation {
    pub fn resources(&self) -> &[Vec<u64>] { &self.resources }
    pub fn resource_totals(&self) -> &FundingAssignmentTotals { &self.resource_totals }
    pub fn fee_debits(&self) -> &[u64] { &self.fee_debits }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingFamilyAllocation {
    outcomes: Vec<FundingOutcomeAllocation>,
    holds: Vec<u64>,
    total_held: u128,
    resource_priority: FundingFamilyResourcePriority,
}

impl FundingFamilyAllocation {
    pub fn outcomes(&self) -> &[FundingOutcomeAllocation] { &self.outcomes }
    pub fn holds(&self) -> &[u64] { &self.holds }
    pub fn total_held(&self) -> u128 { self.total_held }
}

#[derive(Clone, Copy, Debug)]
pub struct FundingOutcomeProposal<'a> {
    pub resources: &'a [Vec<u64>],
    pub fee_debits: &'a [u64],
}

#[derive(Clone, Copy, Debug)]
pub struct FundingFamilyProposal<'a> {
    pub outcomes: &'a [FundingOutcomeProposal<'a>],
    pub holds: &'a [u64],
}

fn validate(
    problem: FundingFamilyOptimizationProblem<'_>,
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
    if problem.outcomes.is_empty() {
        return Err(FundingFamilyError::EmptyCases);
    }
    if problem.outcomes.len() > limits.case_cap.get() {
        return Err(FundingFamilyError::TooManyCases);
    }
    if problem.resource_cursor >= n || problem.fee_cursor >= n {
        return Err(FundingSearchError::InvalidPriorityCursor.into());
    }
    let lower = vector(n, budget)?;
    let mut upper = vector(n, budget)?;
    for outcome in problem.outcomes {
        let resources = outcome.resources;
        if resources.capacities.len() != n
            || outcome
                .unit_fee_eligible
                .is_some_and(|allowed| allowed.len() != n)
        {
            return Err(FundingSearchError::from(FundingAssignmentError::InvalidDimensions).into());
        }
        if resources.obligations.len() > limits.search.obligation_cap.get() {
            return Err(
                FundingSearchError::from(FundingAssignmentError::TooManyObligations).into(),
            );
        }
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            resources
                .obligations
                .len()
                .checked_add(2)
                .and_then(|m| n.checked_mul(m))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        FundingBoxProblem {
            lower: &lower,
            upper: resources.capacities,
            obligations: resources.obligations,
            eligible: resources.eligible,
        }
        .validate(limits.search, budget)?;
        let total = resources.obligations.iter().try_fold(
            u128::from(outcome.unit_fee_eligible.is_some()),
            |total, amount| {
                total
                    .checked_add(u128::from(*amount))
                    .ok_or(FundingSearchError::Overflow)
            },
        )?;
        for (i, cap) in upper.iter_mut().enumerate() {
            let bounded = u128::from(resources.capacities[i].min(problem.capacities[i])).min(total);
            *cap = (*cap).max(u64::try_from(bounded).map_err(|_| FundingSearchError::Overflow)?);
        }
    }
    Ok(HoldBox { lower, upper })
}

fn relaxed_optimum(
    problem: FundingFamilyOptimizationProblem<'_>,
    bounds: &HoldBox,
    limits: FundingFamilyLimits,
    budget: &HostWorkBudget,
) -> Result<Option<FundingFamilyAllocation>, FundingFamilyError> {
    let n = problem.capacities.len();
    let count = problem.outcomes.len();
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<FundingOutcomeAllocation>() + size_of::<&[u64]>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut outcomes = Vec::new();
    outcomes
        .try_reserve_exact(count)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut holds = vector(n, budget)?;
    let mut capacities = vector(n, budget)?;
    for outcome in problem.outcomes {
        reserve_work(budget, HostWorkDimension::SearchCandidates, n)?;
        for (i, cap) in capacities.iter_mut().enumerate() {
            *cap = bounds.upper[i].min(outcome.resources.capacities[i]);
        }
        let resources = FundingMinimaxProblem {
            capacities: &capacities,
            ..outcome.resources
        };
        let (assignment, totals, fees) = if let Some(eligible) = outcome.unit_fee_eligible {
            let Some(selected) = select_funding_with_unit_fee(
                resources,
                eligible,
                problem.resource_cursor,
                problem.fee_cursor,
                limits.search,
                budget,
            )?
            else {
                return Ok(None);
            };
            selected.into_charges()
        } else {
            let FundingPolicyResult::Selected(selected) = select_fixed_funding_policy(
                resources,
                problem.resource_cursor,
                limits.search,
                budget,
            )?
            else {
                return Ok(None);
            };
            let (assignment, totals, _, _) = selected.into_resource_parts();
            (assignment, totals, vector(n, budget)?)
        };
        for ((hold, resource), fee) in holds.iter_mut().zip(totals.source_debits()).zip(&fees) {
            *hold = (*hold).max(
                resource
                    .checked_add(*fee)
                    .ok_or(FundingSearchError::Overflow)?,
            );
        }
        outcomes.push(FundingOutcomeAllocation {
            resources: assignment,
            resource_totals: totals,
            fee_debits: fees,
        });
    }
    let mut draws = Vec::new();
    draws
        .try_reserve_exact(count)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    for outcome in &outcomes {
        draws.push(outcome.resource_totals.source_debits());
    }
    let resource_priority = FundingFamilyResourcePriority::from_canonical_resource_draws(
        &draws,
        problem.resource_cursor,
        limits,
        budget,
    )?;
    let total_held = sum(&holds)?;
    Ok(Some(FundingFamilyAllocation {
        outcomes,
        holds,
        total_held,
        resource_priority,
    }))
}

fn compare(
    left: &FundingFamilyAllocation,
    right: &FundingFamilyAllocation,
    fee_cursor: usize,
    budget: &HostWorkBudget,
) -> Result<Ordering, FundingFamilyError> {
    let resources = left
        .resource_priority
        .compare(&right.resource_priority, budget)?;
    if resources != Ordering::Equal {
        return Ok(resources);
    }
    for (lhs, rhs) in left.outcomes.iter().zip(&right.outcomes) {
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            lhs.fee_debits.len(),
        )?;
        let order = rhs.fee_debits[fee_cursor..]
            .iter()
            .chain(&rhs.fee_debits[..fee_cursor])
            .cmp(
                lhs.fee_debits[fee_cursor..]
                    .iter()
                    .chain(&lhs.fee_debits[..fee_cursor]),
            );
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    for (lhs, rhs) in left.outcomes.iter().zip(&right.outcomes) {
        for (left_row, right_row) in lhs.resources.iter().zip(&rhs.resources) {
            reserve_work(
                budget,
                HostWorkDimension::VerificationOperations,
                left_row.len(),
            )?;
            let order = left_row.cmp(right_row);
            if order != Ordering::Equal {
                return Ok(order);
            }
        }
    }
    Ok(Ordering::Equal)
}

pub fn optimize_funding_family(
    problem: FundingFamilyOptimizationProblem<'_>,
    limits: FundingFamilyLimits,
    budget: &HostWorkBudget,
) -> Result<Option<FundingFamilyAllocation>, FundingFamilyError> {
    let root = validate(problem, limits, budget)?;
    let mut stack = Vec::new();
    push(&mut stack, root, budget)?;
    let mut best = None;
    while let Some(bounds) = stack.pop() {
        reserve_work(
            budget,
            HostWorkDimension::SearchCandidates,
            problem.capacities.len(),
        )?;
        if sum(&bounds.lower)? > problem.exposure_limit {
            continue;
        }
        let Some(relaxed) = relaxed_optimum(problem, &bounds, limits, budget)? else {
            continue;
        };
        if let Some(incumbent) = &best {
            if compare(&relaxed, incumbent, problem.fee_cursor, budget)? != Ordering::Less {
                continue;
            }
        }
        let padded = relaxed.holds.iter().zip(&bounds.lower).try_fold(
            0_u128,
            |total, (actual, lower)| {
                total
                    .checked_add(u128::from((*actual).max(*lower)))
                    .ok_or(FundingSearchError::Overflow)
            },
        )?;
        if padded <= problem.exposure_limit {
            best = Some(relaxed);
        } else {
            drop(relaxed);
            split_hold_box(bounds, &mut stack, budget)?;
        }
    }
    Ok(best)
}

pub fn verify_funding_family_allocation(
    problem: FundingFamilyOptimizationProblem<'_>,
    proposal: FundingFamilyProposal<'_>,
    limits: FundingFamilyLimits,
    budget: &HostWorkBudget,
) -> Result<bool, FundingFamilyError> {
    let Some(expected) = optimize_funding_family(problem, limits, budget)? else {
        return Ok(false);
    };
    let n = problem.capacities.len();
    reserve_work(budget, HostWorkDimension::VerificationOperations, n)?;
    if proposal.holds != expected.holds || proposal.outcomes.len() != expected.outcomes.len() {
        return Ok(false);
    }
    for (supplied, correct) in proposal.outcomes.iter().zip(&expected.outcomes) {
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            n.checked_mul(2).ok_or(FundingSearchError::Overflow)?,
        )?;
        if supplied.fee_debits != correct.fee_debits || supplied.resources.len() != n {
            return Ok(false);
        }
        for (row, expected_row) in supplied.resources.iter().zip(&correct.resources) {
            reserve_work(
                budget,
                HostWorkDimension::VerificationOperations,
                expected_row.len(),
            )?;
            if row != expected_row {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
#[path = "funding_family_optimizer_tests.rs"]
mod tests;
