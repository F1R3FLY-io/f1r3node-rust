use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::funding_feasibility::{filled_vec, reserve_work};
use super::{
    check_funding_minimax_certificate, FundingAssignmentTotals, FundingBoxProblem,
    FundingExcessCut, FundingMinimaxProblem, FundingSearchError, FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingOptimalDomain {
    lower: Vec<u64>,
    upper: Vec<u64>,
    obligations: Vec<u64>,
    eligible: Vec<Vec<bool>>,
}

impl FundingOptimalDomain {
    pub fn problem(&self) -> FundingBoxProblem<'_> {
        FundingBoxProblem {
            lower: &self.lower,
            upper: &self.upper,
            obligations: &self.obligations,
            eligible: &self.eligible,
        }
    }

    pub(super) fn check_assignment(
        &self,
        assignment: &[Vec<u64>],
        limits: FundingSearchLimits,
        budget: &HostWorkBudget,
    ) -> Result<FundingAssignmentTotals, FundingSearchError> {
        let problem = FundingMinimaxProblem {
            capacities: &self.upper,
            obligations: &self.obligations,
            eligible: &self.eligible,
        };
        let totals = problem.check_assignment(assignment, limits, budget)?;
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            self.lower.len(),
        )?;
        if totals
            .source_debits()
            .iter()
            .zip(&self.lower)
            .any(|(draw, lower)| draw < lower)
        {
            return Err(FundingSearchError::InvalidResult);
        }
        Ok(totals)
    }
}

pub fn derive_funding_optimal_domain(
    problem: FundingMinimaxProblem<'_>,
    assignment: &[Vec<u64>],
    cuts: &[FundingExcessCut],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<FundingOptimalDomain, FundingSearchError> {
    if !check_funding_minimax_certificate(problem, assignment, cuts, limits, budget)? {
        return Err(FundingSearchError::InvalidResult);
    }
    let sources = problem.capacities.len();
    let obligations = problem.obligations.len();
    let cells = sources
        .checked_mul(obligations)
        .ok_or(FundingSearchError::Overflow)?;
    let bytes = sources
        .checked_mul(2 * size_of::<u64>() + size_of::<Vec<bool>>())
        .and_then(|n| n.checked_add(cells))
        .and_then(|n| {
            obligations
                .checked_mul(size_of::<u64>())
                .and_then(|m| n.checked_add(m))
        })
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    let preparation = cells
        .checked_add(sources)
        .and_then(|n| n.checked_add(obligations))
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, preparation)?;
    let mut domain = FundingOptimalDomain {
        lower: filled_vec(sources, 0_u64)?,
        upper: filled_vec(sources, 0_u64)?,
        obligations: filled_vec(obligations, 0_u64)?,
        eligible: filled_vec(sources, Vec::new())?,
    };
    domain.upper.copy_from_slice(problem.capacities);
    domain.obligations.copy_from_slice(problem.obligations);
    for (row, original) in domain.eligible.iter_mut().zip(problem.eligible) {
        *row = filled_vec(obligations, false)?;
        row.copy_from_slice(original);
    }
    let cut_work = cells
        .checked_mul(2)
        .and_then(|n| n.checked_add(sources))
        .ok_or(FundingSearchError::Overflow)?;
    for cut in cuts {
        reserve_work(budget, HostWorkDimension::SearchCandidates, cut_work)?;
        for (i, row) in problem.eligible.iter().enumerate() {
            let neighbor = row
                .iter()
                .zip(&cut.selected_obligations)
                .any(|(allowed, selected)| *allowed && *selected);
            let bound = problem.capacities[i].min(cut.threshold);
            if neighbor {
                domain.lower[i] = domain.lower[i].max(bound);
                for (allowed, selected) in
                    domain.eligible[i].iter_mut().zip(&cut.selected_obligations)
                {
                    *allowed &= *selected;
                }
            } else {
                domain.upper[i] = domain.upper[i].min(bound);
            }
        }
    }
    domain.check_assignment(assignment, limits, budget)?;
    Ok(domain)
}

#[cfg(test)]
#[path = "funding_optimal_domain_tests.rs"]
mod tests;
