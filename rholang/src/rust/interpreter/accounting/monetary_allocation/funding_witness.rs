use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::funding_feasibility::{filled_vec, reserve_work};
use super::{
    check_funding_cell_deficit, solve_funding_cell_bound, solve_funding_feasibility,
    FundingAssignmentError, FundingAssignmentTotals, FundingCellQuery, FundingFeasibility,
    FundingMinimaxProblem, FundingSearchError, FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FundingEntryMinimum {
    Zero,
    Deficit { selected_obligations: Vec<bool> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingWitnessCertificate {
    entries: Vec<FundingEntryMinimum>,
}

impl FundingWitnessCertificate {
    pub fn entries(&self) -> &[FundingEntryMinimum] { &self.entries }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FundingWitnessResult {
    Canonical {
        assignment: Vec<Vec<u64>>,
        totals: FundingAssignmentTotals,
        certificate: FundingWitnessCertificate,
    },
    Infeasible {
        selected_obligations: Vec<bool>,
    },
}

struct WitnessResidual {
    contributions: Vec<u64>,
    obligations: Vec<u64>,
    eligible: Vec<Vec<bool>>,
}

impl WitnessResidual {
    fn new(
        problem: FundingMinimaxProblem<'_>,
        budget: &HostWorkBudget,
    ) -> Result<Self, FundingSearchError> {
        let n = problem.capacities.len();
        let m = problem.obligations.len();
        let cells = n.checked_mul(m).ok_or(FundingSearchError::Overflow)?;
        let bytes = n
            .checked_mul(size_of::<u64>() + size_of::<Vec<bool>>())
            .and_then(|x| {
                m.checked_mul(size_of::<u64>())
                    .and_then(|y| x.checked_add(y))
            })
            .and_then(|x| x.checked_add(cells))
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        reserve_work(
            budget,
            HostWorkDimension::SearchCandidates,
            cells
                .checked_add(n)
                .and_then(|x| x.checked_add(m))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let mut contributions = filled_vec(n, 0_u64)?;
        let mut obligations = filled_vec(m, 0_u64)?;
        let mut eligible = filled_vec(n, Vec::new())?;
        contributions.copy_from_slice(problem.capacities);
        obligations.copy_from_slice(problem.obligations);
        for (row, original) in eligible.iter_mut().zip(problem.eligible) {
            *row = filled_vec(m, false)?;
            row.copy_from_slice(original);
        }
        Ok(Self {
            contributions,
            obligations,
            eligible,
        })
    }

    fn problem(&self) -> FundingMinimaxProblem<'_> {
        FundingMinimaxProblem {
            capacities: &self.contributions,
            obligations: &self.obligations,
            eligible: &self.eligible,
        }
    }

    fn freeze(
        &mut self,
        source: usize,
        obligation: usize,
        amount: u64,
    ) -> Result<(), FundingSearchError> {
        let contribution = self.contributions[source]
            .checked_sub(amount)
            .ok_or(FundingSearchError::InvalidResult)?;
        let demand = self.obligations[obligation]
            .checked_sub(amount)
            .ok_or(FundingSearchError::InvalidResult)?;
        self.contributions[source] = contribution;
        self.obligations[obligation] = demand;
        self.eligible[source][obligation] = false;
        Ok(())
    }
}

fn validate_exact_totals(
    problem: FundingMinimaxProblem<'_>,
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<usize, FundingSearchError> {
    let n = problem.capacities.len();
    let m = problem.obligations.len();
    if n == 0 {
        return Err(FundingAssignmentError::EmptySources.into());
    }
    if n > limits.source_cap.get() {
        return Err(FundingAssignmentError::TooManySources.into());
    }
    if m > limits.obligation_cap.get() {
        return Err(FundingAssignmentError::TooManyObligations.into());
    }
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        n.checked_add(m).ok_or(FundingSearchError::Overflow)?,
    )?;
    if problem.eligible.len() != n || problem.eligible.iter().any(|row| row.len() != m) {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
    let rows = problem
        .capacities
        .iter()
        .try_fold(0_u64, |sum, value| sum.checked_add(*value))
        .ok_or(FundingAssignmentError::Overflow)?;
    let columns = problem
        .obligations
        .iter()
        .try_fold(0_u64, |sum, value| sum.checked_add(*value))
        .ok_or(FundingAssignmentError::Overflow)?;
    if rows != columns {
        return Err(FundingSearchError::UnequalFundingTotals);
    }
    n.checked_mul(m).ok_or(FundingSearchError::Overflow)
}

pub fn check_fixed_funding_witness(
    problem: FundingMinimaxProblem<'_>,
    assignment: &[Vec<u64>],
    entries: &[FundingEntryMinimum],
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<bool, FundingSearchError> {
    let cells = validate_exact_totals(problem, limits, budget)?;
    let totals = problem.check_assignment(assignment, limits, budget)?;
    if entries.len() != cells || totals.source_debits() != problem.capacities {
        return Ok(false);
    }
    let mut residual = WitnessResidual::new(problem, budget)?;
    let mut proofs = entries.iter();
    for (i, row) in assignment.iter().enumerate() {
        for (j, amount) in row.iter().copied().enumerate() {
            reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
            match proofs.next().ok_or(FundingSearchError::InvalidResult)? {
                FundingEntryMinimum::Zero => {
                    if amount != 0 {
                        return Ok(false);
                    }
                }
                FundingEntryMinimum::Deficit {
                    selected_obligations,
                } => {
                    if amount == 0 {
                        return Ok(false);
                    }
                    let query = FundingCellQuery {
                        exact: residual.problem(),
                        source: i,
                        obligation: j,
                        maximum: amount - 1,
                    };
                    if !check_funding_cell_deficit(query, selected_obligations, limits, budget)? {
                        return Ok(false);
                    }
                }
            }
            residual.freeze(i, j, amount)?;
        }
    }
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        problem
            .capacities
            .len()
            .checked_add(problem.obligations.len())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    Ok(residual
        .contributions
        .iter()
        .chain(&residual.obligations)
        .all(|value| *value == 0))
}

pub fn select_fixed_funding_witness(
    problem: FundingMinimaxProblem<'_>,
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<FundingWitnessResult, FundingSearchError> {
    let cells = validate_exact_totals(problem, limits, budget)?;
    let mut working = match solve_funding_feasibility(
        problem.capacities,
        problem.obligations,
        problem.eligible,
        limits,
        budget,
    )? {
        FundingFeasibility::Feasible { assignment, .. } => assignment,
        FundingFeasibility::Infeasible {
            selected_obligations,
        } => {
            return Ok(FundingWitnessResult::Infeasible {
                selected_obligations,
            })
        }
    };
    let n = problem.capacities.len();
    let m = problem.obligations.len();
    let mut residual = WitnessResidual::new(problem, budget)?;
    let bytes = cells
        .checked_mul(size_of::<u64>() + size_of::<FundingEntryMinimum>())
        .and_then(|x| {
            n.checked_mul(size_of::<Vec<u64>>())
                .and_then(|y| x.checked_add(y))
        })
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, cells)?;
    let mut assignment = filled_vec(n, Vec::new())?;
    for row in &mut assignment {
        *row = filled_vec(m, 0_u64)?;
    }
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(cells)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    for (i, row) in assignment.iter_mut().enumerate() {
        for (j, selected) in row.iter_mut().enumerate() {
            reserve_work(budget, HostWorkDimension::SearchCandidates, 1)?;
            let mut lower = 0;
            let mut upper = working[i][j];
            while lower < upper {
                let middle = lower + (upper - lower) / 2;
                let query = FundingCellQuery {
                    exact: residual.problem(),
                    source: i,
                    obligation: j,
                    maximum: middle,
                };
                match solve_funding_cell_bound(query, limits, budget)? {
                    FundingFeasibility::Feasible {
                        assignment: next, ..
                    } => {
                        if next[i][j] < lower || next[i][j] > middle {
                            return Err(FundingSearchError::InvalidResult);
                        }
                        upper = next[i][j];
                        working = next;
                    }
                    FundingFeasibility::Infeasible { .. } => {
                        lower = middle.checked_add(1).ok_or(FundingSearchError::Overflow)?
                    }
                }
            }
            if working[i][j] != lower {
                return Err(FundingSearchError::InvalidResult);
            }
            let proof = if lower == 0 {
                FundingEntryMinimum::Zero
            } else {
                let query = FundingCellQuery {
                    exact: residual.problem(),
                    source: i,
                    obligation: j,
                    maximum: lower - 1,
                };
                match solve_funding_cell_bound(query, limits, budget)? {
                    FundingFeasibility::Infeasible {
                        selected_obligations,
                    } => FundingEntryMinimum::Deficit {
                        selected_obligations,
                    },
                    FundingFeasibility::Feasible { .. } => {
                        return Err(FundingSearchError::InvalidResult)
                    }
                }
            };
            entries.push(proof);
            *selected = lower;
            working[i][j] = 0;
            residual.freeze(i, j, lower)?;
        }
    }
    let totals = problem.check_assignment(&assignment, limits, budget)?;
    if !check_fixed_funding_witness(problem, &assignment, &entries, limits, budget)? {
        return Err(FundingSearchError::InvalidResult);
    }
    Ok(FundingWitnessResult::Canonical {
        assignment,
        totals,
        certificate: FundingWitnessCertificate { entries },
    })
}

#[cfg(test)]
#[path = "funding_witness_tests.rs"]
mod tests;
