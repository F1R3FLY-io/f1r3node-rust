use models::rust::host_work::HostWorkDimension;

use super::funding_feasibility::{filled_vec, reserve_work};
use super::{
    check_funding_deficit, FundingAssignmentError, FundingSearchError, FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingDomainCounterexample {
    contributions: Vec<u64>,
    selected_obligations: Vec<bool>,
}

#[derive(Clone, Copy, Debug)]
pub struct FundingDomainCounterexampleView<'a> {
    pub contributions: &'a [u64],
    pub selected_obligations: &'a [bool],
}

impl FundingDomainCounterexample {
    pub fn view(&self) -> FundingDomainCounterexampleView<'_> {
        FundingDomainCounterexampleView {
            contributions: &self.contributions,
            selected_obligations: &self.selected_obligations,
        }
    }
}

fn validate_problem(
    capacities: &[u64],
    obligations: &[u64],
    eligible: &[Vec<bool>],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<(u64, usize), FundingSearchError> {
    if capacities.is_empty() {
        return Err(FundingAssignmentError::EmptySources.into());
    }
    if capacities.len() > limits.source_cap.get() {
        return Err(FundingAssignmentError::TooManySources.into());
    }
    if obligations.len() > limits.obligation_cap.get() {
        return Err(FundingAssignmentError::TooManyObligations.into());
    }
    let scan = capacities
        .len()
        .checked_mul(obligations.len())
        .and_then(|cells| cells.checked_add(capacities.len()))
        .and_then(|cells| cells.checked_add(obligations.len()))
        .and_then(|cells| cells.checked_add(1))
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, scan)?;
    if eligible.len() != capacities.len()
        || eligible.iter().any(|row| row.len() != obligations.len())
    {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
    let total = obligations
        .iter()
        .try_fold(0_u64, |sum, amount| sum.checked_add(*amount))
        .ok_or(FundingAssignmentError::Overflow)?;
    Ok((total, scan))
}

pub fn check_funding_domain_counterexample(
    capacities: &[u64],
    obligations: &[u64],
    eligible: &[Vec<bool>],
    witness: FundingDomainCounterexampleView<'_>,
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<bool, FundingSearchError> {
    let (total, scan) = validate_problem(capacities, obligations, eligible, limits, budget)?;
    if witness.contributions.len() != capacities.len()
        || witness.selected_obligations.len() != obligations.len()
    {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        scan.checked_mul(2).ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut funded = 0_u64;
    for (&amount, &capacity) in witness.contributions.iter().zip(capacities) {
        if amount > capacity {
            return Ok(false);
        }
        let Some(next) = funded.checked_add(amount) else {
            return Ok(false);
        };
        funded = next;
    }
    if funded != total {
        return Ok(false);
    }
    check_funding_deficit(
        witness.contributions,
        obligations,
        eligible,
        witness.selected_obligations,
        limits.source_cap,
        limits.obligation_cap,
    )
    .map_err(Into::into)
}

pub fn build_funding_domain_counterexample(
    capacities: &[u64],
    obligations: &[u64],
    eligible: &[Vec<bool>],
    selected_sources: &[bool],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<Option<FundingDomainCounterexample>, FundingSearchError> {
    let (total, scan) = validate_problem(capacities, obligations, eligible, limits, budget)?;
    if selected_sources.len() != capacities.len() {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
    let bytes = capacities
        .len()
        .checked_mul(8)
        .and_then(|value| value.checked_add(obligations.len()))
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        scan.checked_mul(2).ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut contributions = filled_vec(capacities.len(), 0_u64)?;
    let mut remaining = total;
    for priority in [true, false] {
        for i in (0..capacities.len()).rev() {
            if selected_sources[i] == priority {
                let draw = capacities[i].min(remaining);
                contributions[i] = draw;
                remaining -= draw;
            }
        }
    }
    if remaining != 0 {
        return Err(FundingAssignmentError::InsufficientSourceCapacity.into());
    }
    let mut selected_obligations = filled_vec(obligations.len(), true)?;
    for (selected, row) in selected_sources.iter().zip(eligible) {
        if *selected {
            for (outside, allowed) in selected_obligations.iter_mut().zip(row) {
                *outside &= !allowed;
            }
        }
    }
    let witness = FundingDomainCounterexample {
        contributions,
        selected_obligations,
    };
    if check_funding_domain_counterexample(
        capacities,
        obligations,
        eligible,
        witness.view(),
        limits,
        budget,
    )? {
        Ok(Some(witness))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
#[path = "funding_domain_witness_tests.rs"]
mod tests;
