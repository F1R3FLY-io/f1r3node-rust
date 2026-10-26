use std::cmp::Ordering;

use super::funding_feasibility::filled_vec;
use super::funding_policy::{
    canonical_assignment, select_restricted_candidate, RestrictedFundingCandidate,
};
use super::{
    allocate_capped_max_min, classify_fixed_funding_domain, select_fixed_funding_policy,
    FundingAssignmentError, FundingAssignmentTotals, FundingBurdenRank,
    FundingDomainClassification, FundingMinimaxProblem, FundingPolicyFragment, FundingPolicyResult,
    FundingSearchError, FundingSearchLimits, MonetaryAllocation,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingFeeSelection {
    resource_assignment: Vec<Vec<u64>>,
    resource_totals: FundingAssignmentTotals,
    resource_unrestricted: bool,
    resource_next_cursor: Option<usize>,
    fee: MonetaryAllocation,
}

impl FundingFeeSelection {
    pub(super) fn into_charges(self) -> (Vec<Vec<u64>>, FundingAssignmentTotals, Vec<u64>) {
        (
            self.resource_assignment,
            self.resource_totals,
            self.fee.debits,
        )
    }
    pub fn resource_assignment(&self) -> &[Vec<u64>] { &self.resource_assignment }
    pub fn resource_totals(&self) -> &FundingAssignmentTotals { &self.resource_totals }
    pub fn resource_unrestricted(&self) -> bool { self.resource_unrestricted }
    pub fn resource_next_cursor(&self) -> Option<usize> { self.resource_next_cursor }
    pub fn fee(&self) -> &MonetaryAllocation { &self.fee }
}

fn rank(totals: &FundingAssignmentTotals) -> Result<FundingBurdenRank, FundingSearchError> {
    Ok(FundingBurdenRank::from_assignment(totals))
}

fn cyclic_cmp(left: &[u64], right: &[u64], cursor: usize) -> Ordering {
    left[cursor..]
        .iter()
        .chain(&left[..cursor])
        .cmp(right[cursor..].iter().chain(&right[..cursor]))
}

pub fn select_funding_with_unit_fee(
    problem: FundingMinimaxProblem<'_>,
    fee_eligible: &[bool],
    resource_cursor: usize,
    fee_cursor: usize,
    limits: FundingSearchLimits,
) -> Result<Option<FundingFeeSelection>, FundingSearchError> {
    let count = problem.capacities.len();
    if count == 0 {
        return Err(FundingAssignmentError::EmptySources.into());
    }
    if count > limits.source_cap.get() {
        return Err(FundingAssignmentError::TooManySources.into());
    }
    if fee_eligible.len() != count {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
    if resource_cursor >= count || fee_cursor >= count {
        return Err(FundingSearchError::InvalidPriorityCursor);
    }
    let classification = classify_fixed_funding_domain(
        problem.capacities,
        problem.obligations,
        problem.eligible,
        limits,
    )?;
    let total = match &classification {
        FundingDomainClassification::Infeasible { .. } => return Ok(None),
        FundingDomainClassification::Unrestricted { totals, .. }
        | FundingDomainClassification::Restricted { totals, .. } => totals.total(),
    };
    let fee_capacity = problem.capacities.iter().zip(fee_eligible).try_fold(
        0_u128,
        |sum, (&capacity, &eligible)| {
            sum.checked_add(if eligible { u128::from(capacity) } else { 0 })
                .ok_or(FundingSearchError::Overflow)
        },
    )?;
    if fee_capacity == 0 {
        return Ok(None);
    }
    let (resource_assignment, resource_totals, resource_unrestricted, resource_next_cursor) =
        if u128::from(total) < fee_capacity {
            let FundingPolicyResult::Selected(selected) =
                select_fixed_funding_policy(problem, resource_cursor, limits)?
            else {
                return Err(FundingSearchError::InvalidResult);
            };
            let (assignment, totals, fragment, next) = selected.into_resource_parts();
            let unrestricted = matches!(fragment, FundingPolicyFragment::Unrestricted);
            (assignment, totals, unrestricted, next)
        } else {
            let mut capacities = filled_vec(count, 0_u64)?;
            capacities.copy_from_slice(problem.capacities);
            let mut best: Option<(RestrictedFundingCandidate, FundingBurdenRank)> = None;
            for payer in 0..count {
                if !fee_eligible[payer] || capacities[payer] == 0 {
                    continue;
                }
                capacities[payer] -= 1;
                let candidate = select_restricted_candidate(
                    FundingMinimaxProblem {
                        capacities: &capacities,
                        ..problem
                    },
                    resource_cursor,
                    limits,
                )?;
                capacities[payer] += 1;
                let Some(candidate) = candidate else {
                    continue;
                };
                let candidate_rank = rank(&candidate.totals)?;
                let replace = match &best {
                    None => true,
                    Some((current, current_rank)) => {
                        match candidate_rank
                            .compare(current_rank)
                            .map_err(|_| FundingSearchError::InvalidResult)?
                        {
                            Ordering::Less => true,
                            Ordering::Equal => {
                                cyclic_cmp(
                                    candidate.totals.source_debits(),
                                    current.totals.source_debits(),
                                    resource_cursor,
                                ) == Ordering::Greater
                            }
                            Ordering::Greater => false,
                        }
                    }
                };
                if replace {
                    best = Some((candidate, candidate_rank));
                }
            }
            let Some((best, _)) = best else {
                return Ok(None);
            };
            let (assignment, totals, _) =
                canonical_assignment(problem, best.totals.source_debits(), limits)?;
            let next = (total != 0).then_some(if resource_cursor == count - 1 {
                0
            } else {
                resource_cursor + 1
            });
            (assignment, totals, false, next)
        };
    let mut fee_capacities = filled_vec(count, 0_u64)?;
    for (i, capacity) in fee_capacities.iter_mut().enumerate() {
        let remaining = problem.capacities[i]
            .checked_sub(resource_totals.source_debits()[i])
            .ok_or(FundingSearchError::InvalidResult)?;
        if fee_eligible[i] {
            *capacity = remaining;
        }
    }
    let fee = allocate_capped_max_min(&fee_capacities, 1, fee_cursor, limits.source_cap)
        .map_err(|_| FundingSearchError::InvalidResult)?;
    Ok(Some(FundingFeeSelection {
        resource_assignment,
        resource_totals,
        resource_unrestricted,
        resource_next_cursor,
        fee,
    }))
}

#[cfg(test)]
#[path = "funding_fee_policy_tests.rs"]
mod tests;
