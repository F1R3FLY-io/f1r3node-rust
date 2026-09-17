use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::funding_feasibility::{filled_vec, reserve_work};
use super::{
    allocate_capped_max_min, certify_funding_minimax, solve_box_funding_feasibility,
    solve_funding_feasibility, FundingAssignmentTotals, FundingBoxFeasibility, FundingBoxProblem,
    FundingFeasibility, FundingMinimaxCertificate, FundingMinimaxProblem, FundingSearchError,
    FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FundingMinimaxRankResult {
    Optimal {
        assignment: Vec<Vec<u64>>,
        totals: FundingAssignmentTotals,
        certificate: FundingMinimaxCertificate,
    },
    Infeasible {
        selected_obligations: Vec<bool>,
    },
}

fn exact_transfer(
    problem: FundingMinimaxProblem<'_>,
    totals: &FundingAssignmentTotals,
    receiver: usize,
    donor: usize,
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<FundingFeasibility, FundingSearchError> {
    let count = problem.capacities.len();
    reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<u64>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut proposed = filled_vec(count, 0_u64)?;
    proposed.copy_from_slice(totals.source_debits());
    proposed[donor] = proposed[donor]
        .checked_sub(1)
        .ok_or(FundingSearchError::InvalidResult)?;
    proposed[receiver] = proposed[receiver]
        .checked_add(1)
        .ok_or(FundingSearchError::InvalidResult)?;
    if proposed[receiver] > problem.capacities[receiver] {
        return Err(FundingSearchError::InvalidResult);
    }
    solve_funding_feasibility(
        &proposed,
        problem.obligations,
        problem.eligible,
        limits,
        budget,
    )
}

pub fn solve_fixed_funding_minimax_rank(
    problem: FundingMinimaxProblem<'_>,
    limits: FundingSearchLimits,
    budget: &HostWorkBudget,
) -> Result<FundingMinimaxRankResult, FundingSearchError> {
    let (mut assignment, mut totals) = match solve_funding_feasibility(
        problem.capacities,
        problem.obligations,
        problem.eligible,
        limits,
        budget,
    )? {
        FundingFeasibility::Feasible { assignment, totals } => (assignment, totals),
        FundingFeasibility::Infeasible {
            selected_obligations,
        } => {
            return Ok(FundingMinimaxRankResult::Infeasible {
                selected_obligations,
            });
        }
    };
    let count = problem.capacities.len();
    let cells = count
        .checked_mul(problem.obligations.len())
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, cells)?;
    if problem.eligible.iter().flatten().all(|allowed| *allowed) {
        reserve_work(
            budget,
            HostWorkDimension::SearchCandidates,
            count.checked_mul(68).ok_or(FundingSearchError::Overflow)?,
        )?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            count
                .checked_mul(size_of::<u64>())
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let allocation =
            allocate_capped_max_min(problem.capacities, totals.total(), 0, limits.source_cap)
                .map_err(|_| FundingSearchError::InvalidResult)?;
        match solve_funding_feasibility(
            &allocation.debits,
            problem.obligations,
            problem.eligible,
            limits,
            budget,
        )? {
            FundingFeasibility::Feasible {
                assignment: next,
                totals: next_totals,
            } => {
                assignment = next;
                totals = next_totals;
            }
            FundingFeasibility::Infeasible { .. } => return Err(FundingSearchError::InvalidResult),
        }
    } else {
        let bytes = count
            .checked_mul(2 * size_of::<u64>() + 2 * size_of::<bool>())
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
        let mut frozen = filled_vec(count, false)?;
        let mut block = filled_vec(count, false)?;
        let mut lower = filled_vec(count, 0_u64)?;
        let mut upper = filled_vec(count, 0_u64)?;
        let mut remaining = count;
        while remaining > 0 {
            reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
            let mut lo = 0_u64;
            let mut hi = totals
                .source_debits()
                .iter()
                .zip(&frozen)
                .filter(|(_, fixed)| !**fixed)
                .map(|(value, _)| *value)
                .max()
                .ok_or(FundingSearchError::InvalidResult)?;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
                for i in 0..count {
                    lower[i] = if frozen[i] {
                        totals.source_debits()[i]
                    } else {
                        0
                    };
                    upper[i] = if frozen[i] {
                        lower[i]
                    } else {
                        problem.capacities[i].min(mid)
                    };
                }
                match solve_box_funding_feasibility(
                    FundingBoxProblem {
                        lower: &lower,
                        upper: &upper,
                        obligations: problem.obligations,
                        eligible: problem.eligible,
                    },
                    limits,
                    budget,
                )? {
                    FundingBoxFeasibility::Feasible {
                        assignment: next,
                        totals: next_totals,
                    } => {
                        hi = mid;
                        assignment = next;
                        totals = next_totals;
                    }
                    FundingBoxFeasibility::UpperDeficit { .. }
                    | FundingBoxFeasibility::LowerDeficit { .. } => lo = mid + 1,
                }
            }
            let level = lo;
            for donor in 0..count {
                reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
                if frozen[donor] || totals.source_debits()[donor] != level {
                    continue;
                }
                for receiver in 0..count {
                    if frozen[receiver]
                        || level.saturating_sub(totals.source_debits()[receiver]) < 2
                        || totals.source_debits()[receiver] == problem.capacities[receiver]
                    {
                        continue;
                    }
                    if let FundingFeasibility::Feasible {
                        assignment: next,
                        totals: next_totals,
                    } = exact_transfer(problem, &totals, receiver, donor, limits, budget)?
                    {
                        assignment = next;
                        totals = next_totals;
                        break;
                    }
                }
            }
            reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
            block.fill(false);
            for donor in 0..count {
                reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
                if frozen[donor] || totals.source_debits()[donor] != level {
                    continue;
                }
                block[donor] = true;
                if level == 0 {
                    continue;
                }
                for receiver in 0..count {
                    if frozen[receiver]
                        || block[receiver]
                        || totals.source_debits()[receiver] == problem.capacities[receiver]
                    {
                        continue;
                    }
                    if let FundingFeasibility::Feasible { .. } =
                        exact_transfer(problem, &totals, receiver, donor, limits, budget)?
                    {
                        block[receiver] = true;
                    }
                }
            }
            let before = remaining;
            reserve_work(budget, HostWorkDimension::VerificationOperations, count)?;
            for i in 0..count {
                if block[i] {
                    if frozen[i] || level.saturating_sub(totals.source_debits()[i]) > 1 {
                        return Err(FundingSearchError::InvalidResult);
                    }
                    frozen[i] = true;
                    remaining -= 1;
                }
            }
            if before == remaining {
                return Err(FundingSearchError::InvalidResult);
            }
        }
    }
    let verified = problem.check_assignment(&assignment, limits, budget)?;
    if verified != totals {
        return Err(FundingSearchError::InvalidResult);
    }
    let certificate = certify_funding_minimax(problem, &assignment, limits, budget)?
        .ok_or(FundingSearchError::InvalidResult)?;
    Ok(FundingMinimaxRankResult::Optimal {
        assignment,
        totals,
        certificate,
    })
}

#[cfg(test)]
#[path = "funding_minimax_tests.rs"]
mod tests;
