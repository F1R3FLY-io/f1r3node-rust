use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::funding_feasibility::{filled_vec, reserve_work};
use super::{
    check_funding_assignment, solve_funding_feasibility, FundingAssignmentError,
    FundingAssignmentTotals, FundingFeasibility, FundingSearchError, FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct FundingMinimaxProblem<'a> {
    pub capacities: &'a [u64],
    pub obligations: &'a [u64],
    pub eligible: &'a [Vec<bool>],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingExcessCut {
    pub threshold: u64,
    pub selected_obligations: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingMinimaxCertificate {
    cuts: Vec<FundingExcessCut>,
}

impl FundingMinimaxCertificate {
    pub fn cuts(&self) -> &[FundingExcessCut] { &self.cuts }
}

impl FundingMinimaxProblem<'_> {
    pub(super) fn check_assignment(
        self,
        assignment: &[Vec<u64>],
        limits: FundingSearchLimits,
        budget: &HostWorkBudget,
    ) -> Result<FundingAssignmentTotals, FundingSearchError> {
        if self.capacities.len() > limits.source_cap.get() {
            return Err(FundingAssignmentError::TooManySources.into());
        }
        if self.obligations.len() > limits.obligation_cap.get() {
            return Err(FundingAssignmentError::TooManyObligations.into());
        }
        let cells = self
            .capacities
            .len()
            .checked_mul(self.obligations.len())
            .and_then(|n| n.checked_add(self.capacities.len()))
            .and_then(|n| n.checked_add(self.obligations.len()))
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::VerificationOperations, cells)?;
        let bytes = self
            .capacities
            .len()
            .checked_add(self.obligations.len())
            .and_then(|n| n.checked_mul(size_of::<u64>()))
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        Ok(check_funding_assignment(
            self.capacities,
            self.obligations,
            self.eligible,
            assignment,
            limits.source_cap,
            limits.obligation_cap,
        )?)
    }
}

fn thresholds(
    totals: &FundingAssignmentTotals,
    budget: &HostWorkBudget,
) -> Result<Vec<u64>, FundingSearchError> {
    let count = totals
        .source_debits()
        .len()
        .checked_mul(2)
        .ok_or(FundingSearchError::Overflow)?;
    let preparation = count.checked_mul(4).ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, preparation)?;
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<u64>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut values = filled_vec(count, 0_u64)?;
    values.clear();
    for value in totals
        .source_debits()
        .iter()
        .copied()
        .filter(|value| *value > 0)
    {
        values.push(value - 1);
        values.push(value);
    }
    for i in 1..values.len() {
        let value = values[i];
        let mut position = i;
        while position > 0 {
            reserve_work(budget, HostWorkDimension::SearchCandidates, 1)?;
            if values[position - 1] <= value {
                break;
            }
            values[position] = values[position - 1];
            position -= 1;
        }
        values[position] = value;
    }
    values.dedup();
    Ok(values)
}

fn cut_is_exact(
    problem: FundingMinimaxProblem<'_>,
    totals: &FundingAssignmentTotals,
    cut: &FundingExcessCut,
    budget: &HostWorkBudget,
) -> Result<bool, FundingSearchError> {
    if cut.selected_obligations.len() != problem.obligations.len() {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
    let work = problem
        .capacities
        .len()
        .checked_mul(problem.obligations.len())
        .and_then(|n| n.checked_add(problem.capacities.len()))
        .and_then(|n| n.checked_add(problem.obligations.len()))
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::VerificationOperations, work)?;
    let demand = problem
        .obligations
        .iter()
        .zip(&cut.selected_obligations)
        .filter(|(_, selected)| **selected)
        .try_fold(0_u128, |sum, (amount, _)| {
            sum.checked_add(u128::from(*amount))
        })
        .ok_or(FundingSearchError::Overflow)?;
    let mut supply = 0_u128;
    let mut excess = 0_u128;
    for (i, row) in problem.eligible.iter().enumerate() {
        excess = excess
            .checked_add(u128::from(
                totals.source_debits()[i].saturating_sub(cut.threshold),
            ))
            .ok_or(FundingSearchError::Overflow)?;
        if row
            .iter()
            .zip(&cut.selected_obligations)
            .any(|(allowed, selected)| *allowed && *selected)
        {
            supply = supply
                .checked_add(u128::from(problem.capacities[i].min(cut.threshold)))
                .ok_or(FundingSearchError::Overflow)?;
        }
    }
    Ok(demand.checked_sub(supply) == Some(excess))
}

pub fn check_funding_minimax_certificate(
    problem: FundingMinimaxProblem<'_>,
    assignment: &[Vec<u64>],
    cuts: &[FundingExcessCut],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<bool, FundingSearchError> {
    let totals = problem.check_assignment(assignment, limits, budget)?;
    let required = thresholds(&totals, budget)?;
    if cuts.len() != required.len() {
        return Ok(false);
    }
    for (cut, threshold) in cuts.iter().zip(required) {
        if cut.threshold != threshold || !cut_is_exact(problem, &totals, cut, budget)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn certify_funding_minimax(
    problem: FundingMinimaxProblem<'_>,
    assignment: &[Vec<u64>],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<Option<FundingMinimaxCertificate>, FundingSearchError> {
    let totals = problem.check_assignment(assignment, limits, budget)?;
    let required = thresholds(&totals, budget)?;
    let bytes = required
        .len()
        .checked_mul(size_of::<FundingExcessCut>())
        .and_then(|n| {
            problem
                .capacities
                .len()
                .checked_mul(size_of::<u64>())
                .and_then(|m| n.checked_add(m))
        })
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    let mut cuts = Vec::new();
    cuts.try_reserve_exact(required.len())
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut clipped = filled_vec(problem.capacities.len(), 0_u64)?;
    for threshold in required {
        reserve_work(budget, HostWorkDimension::SearchCandidates, clipped.len())?;
        for (value, capacity) in clipped.iter_mut().zip(problem.capacities) {
            *value = (*capacity).min(threshold);
        }
        let selected_obligations = match solve_funding_feasibility(
            &clipped,
            problem.obligations,
            problem.eligible,
            limits,
            budget,
        )? {
            FundingFeasibility::Feasible { .. } => {
                reserve_work(
                    budget,
                    HostWorkDimension::SearchCandidates,
                    problem.obligations.len(),
                )?;
                reserve_work(
                    budget,
                    HostWorkDimension::SearchStateBytes,
                    problem.obligations.len(),
                )?;
                filled_vec(problem.obligations.len(), false)?
            }
            FundingFeasibility::Infeasible {
                selected_obligations,
            } => selected_obligations,
        };
        let cut = FundingExcessCut {
            threshold,
            selected_obligations,
        };
        if !cut_is_exact(problem, &totals, &cut, budget)? {
            return Ok(None);
        }
        cuts.push(cut);
    }
    if !check_funding_minimax_certificate(problem, assignment, &cuts, limits, budget)? {
        return Err(FundingSearchError::InvalidResult);
    }
    Ok(Some(FundingMinimaxCertificate { cuts }))
}
