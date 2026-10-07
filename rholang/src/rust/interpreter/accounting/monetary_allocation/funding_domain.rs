use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::funding_feasibility::{filled_vec, reserve_work};
use super::{
    build_funding_domain_counterexample, solve_funding_feasibility, FundingAssignmentTotals,
    FundingDomainCounterexample, FundingFeasibility, FundingSearchError, FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FundingDomainClassification {
    Infeasible {
        selected_obligations: Vec<bool>,
    },
    Unrestricted {
        assignment: Vec<Vec<u64>>,
        totals: FundingAssignmentTotals,
    },
    Restricted {
        assignment: Vec<Vec<u64>>,
        totals: FundingAssignmentTotals,
        counterexample: FundingDomainCounterexample,
    },
}

pub fn classify_fixed_funding_domain(
    capacities: &[u64],
    obligations: &[u64],
    eligible: &[Vec<bool>],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<FundingDomainClassification, FundingSearchError> {
    let (assignment, totals) =
        match solve_funding_feasibility(capacities, obligations, eligible, limits, budget)? {
            FundingFeasibility::Feasible { assignment, totals } => (assignment, totals),
            FundingFeasibility::Infeasible {
                selected_obligations,
            } => {
                return Ok(FundingDomainClassification::Infeasible {
                    selected_obligations,
                });
            }
        };
    let total = totals.total();
    if total == 0 {
        return Ok(FundingDomainClassification::Unrestricted { assignment, totals });
    }
    let cells = capacities
        .len()
        .checked_mul(obligations.len())
        .ok_or(FundingSearchError::Overflow)?;
    let preparation = cells
        .checked_add(capacities.len())
        .and_then(|count| count.checked_add(obligations.len()))
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, preparation)?;
    let bytes = obligations
        .len()
        .checked_mul(size_of::<Vec<bool>>())
        .and_then(|outer| outer.checked_add(cells))
        .and_then(|matrix| {
            capacities
                .len()
                .checked_mul(size_of::<u64>() + size_of::<bool>())
                .and_then(|sources| matrix.checked_add(sources))
        })
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    let mut transposed = filled_vec(obligations.len(), Vec::new())?;
    for (j, row) in transposed.iter_mut().enumerate() {
        *row = filled_vec(capacities.len(), false)?;
        for (i, allowed) in row.iter_mut().enumerate() {
            *allowed = eligible[i][j];
        }
    }
    let mut excluded = filled_vec(capacities.len(), 0_u64)?;
    let mut selected_sources = filled_vec(capacities.len(), false)?;
    let transposed_limits = FundingSearchLimits {
        source_cap: limits.obligation_cap,
        obligation_cap: limits.source_cap,
    };
    for (j, amount) in obligations
        .iter()
        .enumerate()
        .filter(|(_, amount)| **amount > 0)
    {
        reserve_work(
            budget,
            HostWorkDimension::SearchCandidates,
            capacities.len(),
        )?;
        let mut excluded_total = 0_u128;
        for i in 0..capacities.len() {
            selected_sources[i] = !eligible[i][j];
            excluded[i] = if selected_sources[i] {
                capacities[i]
            } else {
                0
            };
            excluded_total = excluded_total
                .checked_add(u128::from(excluded[i]))
                .ok_or(FundingSearchError::Overflow)?;
        }
        if excluded_total <= u128::from(total - amount) {
            match solve_funding_feasibility(
                obligations,
                &excluded,
                &transposed,
                transposed_limits,
                budget,
            )? {
                FundingFeasibility::Feasible { .. } => continue,
                FundingFeasibility::Infeasible {
                    selected_obligations,
                } => {
                    reserve_work(
                        budget,
                        HostWorkDimension::VerificationOperations,
                        capacities.len(),
                    )?;
                    for (selected, certified) in
                        selected_sources.iter_mut().zip(selected_obligations)
                    {
                        *selected &= certified;
                    }
                }
            }
        }
        let counterexample = build_funding_domain_counterexample(
            capacities,
            obligations,
            eligible,
            &selected_sources,
            limits,
            budget,
        )?
        .ok_or(FundingSearchError::InvalidResult)?;
        return Ok(FundingDomainClassification::Restricted {
            assignment,
            totals,
            counterexample,
        });
    }
    Ok(FundingDomainClassification::Unrestricted { assignment, totals })
}
