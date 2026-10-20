use super::funding_feasibility::filled_vec;
use super::{
    check_funding_assignment, solve_funding_feasibility, FundingAssignmentError,
    FundingAssignmentTotals, FundingFeasibility, FundingSearchError, FundingSearchLimits,
};

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
    ) -> Result<FundingAssignmentTotals, FundingSearchError> {
        if self.capacities.len() > limits.source_cap.get() {
            return Err(FundingAssignmentError::TooManySources.into());
        }
        if self.obligations.len() > limits.obligation_cap.get() {
            return Err(FundingAssignmentError::TooManyObligations.into());
        }
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

fn thresholds(totals: &FundingAssignmentTotals) -> Result<Vec<u64>, FundingSearchError> {
    let count = totals
        .source_debits()
        .len()
        .checked_mul(2)
        .ok_or(FundingSearchError::Overflow)?;
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
) -> Result<bool, FundingSearchError> {
    if cut.selected_obligations.len() != problem.obligations.len() {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
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
) -> Result<bool, FundingSearchError> {
    let totals = problem.check_assignment(assignment, limits)?;
    let required = thresholds(&totals)?;
    if cuts.len() != required.len() {
        return Ok(false);
    }
    for (cut, threshold) in cuts.iter().zip(required) {
        if cut.threshold != threshold || !cut_is_exact(problem, &totals, cut)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn certify_funding_minimax(
    problem: FundingMinimaxProblem<'_>,
    assignment: &[Vec<u64>],
    limits: FundingSearchLimits,
) -> Result<Option<FundingMinimaxCertificate>, FundingSearchError> {
    let totals = problem.check_assignment(assignment, limits)?;
    let required = thresholds(&totals)?;
    let mut cuts = Vec::new();
    cuts.try_reserve_exact(required.len())
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut clipped = filled_vec(problem.capacities.len(), 0_u64)?;
    for threshold in required {
        for (value, capacity) in clipped.iter_mut().zip(problem.capacities) {
            *value = (*capacity).min(threshold);
        }
        let selected_obligations = match solve_funding_feasibility(
            &clipped,
            problem.obligations,
            problem.eligible,
            limits,
        )? {
            FundingFeasibility::Feasible { .. } => filled_vec(problem.obligations.len(), false)?,
            FundingFeasibility::Infeasible {
                selected_obligations,
            } => selected_obligations,
        };
        let cut = FundingExcessCut {
            threshold,
            selected_obligations,
        };
        if !cut_is_exact(problem, &totals, &cut)? {
            return Ok(None);
        }
        cuts.push(cut);
    }
    if !check_funding_minimax_certificate(problem, assignment, &cuts, limits)? {
        return Err(FundingSearchError::InvalidResult);
    }
    Ok(Some(FundingMinimaxCertificate { cuts }))
}
