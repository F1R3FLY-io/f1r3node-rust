use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::{
    filled_vec, reserve_work, solve_box_funding_feasibility, FundingAssignmentError,
    FundingBoxFeasibility, FundingBoxProblem, FundingFamilyError, FundingFamilyLimits,
    FundingFamilyOptimizationProblem, FundingOutcomeProblem, FundingSearchError,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub(super) enum FundingResourceConstraint<'a> {
    Fixed(&'a [u64]),
    Rank(&'a [u64]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FundingFamilySliceWitness {
    pub(super) draws: Vec<Vec<u64>>,
    pub(super) fees: Vec<Vec<u64>>,
    pub(super) holds: Vec<u64>,
}

fn vector<T: Clone>(
    count: usize,
    value: T,
    budget: &HostWorkBudget,
) -> Result<Vec<T>, FundingSearchError> {
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<T>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, count)?;
    filled_vec(count, value)
}

fn cyclic(count: usize, cursor: usize, offset: usize) -> usize {
    if offset >= count - cursor {
        offset - (count - cursor)
    } else {
        cursor + offset
    }
}

struct OutcomeCursor<'a> {
    constraint: FundingResourceConstraint<'a>,
    values: Vec<u64>,
    multiplicities: Vec<usize>,
    remaining: Vec<usize>,
    chosen: Vec<usize>,
    next: Vec<usize>,
    draws: Vec<u64>,
    fees: Vec<u64>,
    available: Vec<u64>,
    lower: Vec<u64>,
    upper: Vec<u64>,
    exposure: Vec<u128>,
    depth: usize,
    fee_next: usize,
    fee_active: bool,
    yielded: bool,
    all_edges: bool,
}

impl<'a> OutcomeCursor<'a> {
    fn new(
        constraint: FundingResourceConstraint<'a>,
        outcome: FundingOutcomeProblem<'_>,
        n: usize,
        budget: &HostWorkBudget,
    ) -> Result<Self, FundingSearchError> {
        let mut values = vector(n, 0_u64, budget)?;
        let mut multiplicities = vector(n, 0_usize, budget)?;
        let mut distinct = 0;
        if let FundingResourceConstraint::Rank(rank) = constraint {
            for value in rank {
                if distinct == 0 || values[distinct - 1] != *value {
                    values[distinct] = *value;
                    distinct += 1;
                }
                multiplicities[distinct - 1] += 1;
            }
        }
        values.truncate(distinct);
        multiplicities.truncate(distinct);
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            n.checked_mul(outcome.resources.obligations.len())
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        Ok(Self {
            constraint,
            remaining: vector(distinct, 0_usize, budget)?,
            values,
            multiplicities,
            chosen: vector(n, 0_usize, budget)?,
            next: vector(n, 0_usize, budget)?,
            draws: vector(n, 0_u64, budget)?,
            fees: vector(n, 0_u64, budget)?,
            available: vector(n, 0_u64, budget)?,
            lower: vector(n, 0_u64, budget)?,
            upper: vector(n, 0_u64, budget)?,
            exposure: vector(
                n.checked_add(1).ok_or(FundingSearchError::Overflow)?,
                0_u128,
                budget,
            )?,
            depth: 0,
            fee_next: 0,
            fee_active: false,
            yielded: false,
            all_edges: outcome
                .resources
                .eligible
                .iter()
                .flatten()
                .all(|edge| *edge),
        })
    }

    fn reset(&mut self) {
        self.depth = 0;
        self.fee_next = 0;
        self.fee_active = false;
        self.yielded = false;
    }

    fn start_fee(
        &mut self,
        problem: FundingFamilyOptimizationProblem<'_>,
        outcome: FundingOutcomeProblem<'_>,
        forced: Option<usize>,
        prior: &[u64],
        budget: &HostWorkBudget,
    ) -> Result<bool, FundingSearchError> {
        let n = prior.len();
        loop {
            reserve_work(budget, HostWorkDimension::SearchCandidates, 1)?;
            let payer = if let Some(eligible) = outcome.unit_fee_eligible {
                let candidate = if let Some(forced) = forced {
                    if self.fee_next != 0 {
                        return Ok(false);
                    }
                    self.fee_next = 1;
                    forced
                } else {
                    if self.fee_next == n {
                        return Ok(false);
                    }
                    let candidate = cyclic(n, problem.fee_cursor, self.fee_next);
                    self.fee_next += 1;
                    candidate
                };
                if !eligible[candidate]
                    || problem.capacities[candidate].min(outcome.resources.capacities[candidate])
                        == 0
                {
                    continue;
                }
                Some(candidate)
            } else {
                if self.fee_next != 0 {
                    return Ok(false);
                }
                self.fee_next = 1;
                None
            };
            reserve_work(
                budget,
                HostWorkDimension::SearchCandidates,
                n.checked_mul(4).ok_or(FundingSearchError::Overflow)?,
            )?;
            self.fees.fill(0);
            if let Some(payer) = payer {
                self.fees[payer] = 1;
            }
            self.exposure[0] = 0;
            for (i, held) in prior.iter().enumerate() {
                self.available[i] = problem.capacities[i]
                    .min(outcome.resources.capacities[i])
                    .checked_sub(self.fees[i])
                    .ok_or(FundingSearchError::InvalidResult)?;
                self.exposure[0] = self.exposure[0]
                    .checked_add(u128::from((*held).max(self.fees[i])))
                    .ok_or(FundingSearchError::Overflow)?;
            }
            if self.exposure[0] > problem.exposure_limit {
                continue;
            }
            self.remaining.copy_from_slice(&self.multiplicities);
            self.draws.fill(0);
            self.next.fill(0);
            self.depth = 0;
            self.yielded = false;
            self.fee_active = true;
            return Ok(true);
        }
    }

    fn flow_feasible(
        &mut self,
        outcome: FundingOutcomeProblem<'_>,
        cursor: usize,
        limits: FundingFamilyLimits,
        budget: &HostWorkBudget,
    ) -> Result<bool, FundingSearchError> {
        let n = self.draws.len();
        if self.all_edges {
            return Ok(true);
        }
        reserve_work(
            budget,
            HostWorkDimension::SearchCandidates,
            n.checked_mul(2).ok_or(FundingSearchError::Overflow)?,
        )?;
        self.lower.fill(0);
        self.upper.copy_from_slice(&self.available);
        for depth in 0..self.depth {
            let i = cyclic(n, cursor, depth);
            self.lower[i] = self.draws[i];
            self.upper[i] = self.draws[i];
        }
        if matches!(self.constraint, FundingResourceConstraint::Rank(_)) {
            let maximum = self
                .values
                .iter()
                .zip(&self.remaining)
                .find(|(_, count)| **count != 0)
                .map_or(0, |(value, _)| *value);
            for depth in self.depth..n {
                let i = cyclic(n, cursor, depth);
                self.upper[i] = self.upper[i].min(maximum);
            }
        }
        Ok(matches!(
            solve_box_funding_feasibility(
                FundingBoxProblem {
                    lower: &self.lower,
                    upper: &self.upper,
                    obligations: outcome.resources.obligations,
                    eligible: outcome.resources.eligible,
                },
                limits.search,
                budget,
            )?,
            FundingBoxFeasibility::Feasible { .. }
        ))
    }

    fn next_candidate(
        &mut self,
        problem: FundingFamilyOptimizationProblem<'_>,
        outcome: FundingOutcomeProblem<'_>,
        forced: Option<usize>,
        prior: &[u64],
        limits: FundingFamilyLimits,
        budget: &HostWorkBudget,
    ) -> Result<bool, FundingSearchError> {
        let n = prior.len();
        loop {
            if !self.fee_active && !self.start_fee(problem, outcome, forced, prior, budget)? {
                return Ok(false);
            }
            if let FundingResourceConstraint::Fixed(fixed) = self.constraint {
                self.fee_active = false;
                reserve_work(
                    budget,
                    HostWorkDimension::SearchCandidates,
                    n.checked_mul(2).ok_or(FundingSearchError::Overflow)?,
                )?;
                if fixed
                    .iter()
                    .zip(&self.available)
                    .any(|(draw, cap)| draw > cap)
                {
                    continue;
                }
                let total = fixed.iter().zip(&self.fees).zip(prior).try_fold(
                    0_u128,
                    |total, ((draw, fee), held)| {
                        let debit = draw.checked_add(*fee).ok_or(FundingSearchError::Overflow)?;
                        total
                            .checked_add(u128::from(debit.max(*held)))
                            .ok_or(FundingSearchError::Overflow)
                    },
                )?;
                if total > problem.exposure_limit {
                    continue;
                }
                self.draws.copy_from_slice(fixed);
                self.depth = n;
                if self.flow_feasible(outcome, problem.resource_cursor, limits, budget)? {
                    return Ok(true);
                }
                continue;
            }
            if self.yielded {
                self.yielded = false;
                self.depth -= 1;
                self.remaining[self.chosen[self.depth]] += 1;
            }
            loop {
                reserve_work(budget, HostWorkDimension::SearchCandidates, 1)?;
                if self.depth == n {
                    self.yielded = true;
                    return Ok(true);
                }
                let source = cyclic(n, problem.resource_cursor, self.depth);
                let mut found = false;
                while self.next[self.depth] < self.values.len() {
                    reserve_work(budget, HostWorkDimension::SearchCandidates, 1)?;
                    let choice = self.next[self.depth];
                    self.next[self.depth] += 1;
                    let value = self.values[choice];
                    if self.remaining[choice] == 0 || value > self.available[source] {
                        continue;
                    }
                    let debit = value
                        .checked_add(self.fees[source])
                        .ok_or(FundingSearchError::Overflow)?;
                    let increment = u128::from(debit.max(prior[source]))
                        - u128::from(self.fees[source].max(prior[source]));
                    let exposure = self.exposure[self.depth]
                        .checked_add(increment)
                        .ok_or(FundingSearchError::Overflow)?;
                    if exposure > problem.exposure_limit {
                        continue;
                    }
                    self.draws[source] = value;
                    self.chosen[self.depth] = choice;
                    self.remaining[choice] -= 1;
                    self.exposure[self.depth + 1] = exposure;
                    self.depth += 1;
                    if self.flow_feasible(outcome, problem.resource_cursor, limits, budget)? {
                        if self.depth < n {
                            self.next[self.depth] = 0;
                        }
                        found = true;
                        break;
                    }
                    self.depth -= 1;
                    self.remaining[choice] += 1;
                }
                if found {
                    continue;
                }
                self.next[self.depth] = 0;
                if self.depth == 0 {
                    self.fee_active = false;
                    break;
                }
                self.depth -= 1;
                self.remaining[self.chosen[self.depth]] += 1;
            }
        }
    }
}

pub(super) fn solve_funding_family_slice(
    problem: FundingFamilyOptimizationProblem<'_>,
    constraints: &[FundingResourceConstraint<'_>],
    fixed_fee_payers: &[Option<usize>],
    limits: FundingFamilyLimits,
    budget: &HostWorkBudget,
) -> Result<Option<FundingFamilySliceWitness>, FundingFamilyError> {
    let n = problem.capacities.len();
    let k = problem.outcomes.len();
    if n == 0 {
        return Err(FundingSearchError::from(FundingAssignmentError::EmptySources).into());
    }
    if n > limits.search.source_cap.get() {
        return Err(FundingSearchError::from(FundingAssignmentError::TooManySources).into());
    }
    if k == 0 {
        return Err(FundingFamilyError::EmptyCases);
    }
    if k > limits.case_cap.get() {
        return Err(FundingFamilyError::TooManyCases);
    }
    if constraints.len() != k || fixed_fee_payers.len() != k {
        return Err(FundingSearchError::from(FundingAssignmentError::InvalidDimensions).into());
    }
    if problem.resource_cursor >= n || problem.fee_cursor >= n {
        return Err(FundingSearchError::InvalidPriorityCursor.into());
    }
    let zero = vector(n, 0_u64, budget)?;
    for ((outcome, constraint), forced) in problem
        .outcomes
        .iter()
        .zip(constraints)
        .zip(fixed_fee_payers)
    {
        if outcome.resources.capacities.len() != n
            || outcome
                .unit_fee_eligible
                .is_some_and(|fees| fees.len() != n)
        {
            return Err(FundingSearchError::from(FundingAssignmentError::InvalidDimensions).into());
        }
        FundingBoxProblem {
            lower: &zero,
            upper: outcome.resources.capacities,
            obligations: outcome.resources.obligations,
            eligible: outcome.resources.eligible,
        }
        .validate(limits.search, budget)?;
        if forced.is_some_and(|payer| payer >= n || outcome.unit_fee_eligible.is_none()) {
            return Err(FundingSearchError::InvalidAssignmentCell.into());
        }
        let values = match constraint {
            FundingResourceConstraint::Fixed(values) | FundingResourceConstraint::Rank(values) => {
                *values
            }
        };
        if values.len() != n {
            return Err(FundingSearchError::from(FundingAssignmentError::InvalidDimensions).into());
        }
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            n.checked_mul(2)
                .and_then(|v| v.checked_add(outcome.resources.obligations.len()))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        if matches!(constraint, FundingResourceConstraint::Rank(_))
            && values.windows(2).any(|pair| pair[0] < pair[1])
        {
            return Err(FundingSearchError::InvalidResult.into());
        }
        let sum = |items: &[u64]| {
            items.iter().try_fold(0_u64, |total, value| {
                total
                    .checked_add(*value)
                    .ok_or(FundingSearchError::Overflow)
            })
        };
        if sum(values)? != sum(outcome.resources.obligations)? {
            return Err(FundingSearchError::UnequalFundingTotals.into());
        }
    }
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        k.checked_mul(size_of::<OutcomeCursor<'_>>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut cursors = Vec::new();
    cursors
        .try_reserve_exact(k)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    for (outcome, constraint) in problem.outcomes.iter().zip(constraints) {
        cursors.push(OutcomeCursor::new(*constraint, *outcome, n, budget)?);
    }
    let mut draws = vector(k, Vec::new(), budget)?;
    let mut fees = vector(k, Vec::new(), budget)?;
    let mut holds = vector(
        k.checked_add(1).ok_or(FundingSearchError::Overflow)?,
        Vec::new(),
        budget,
    )?;
    for row in draws.iter_mut().chain(&mut fees).chain(&mut holds) {
        *row = vector(n, 0_u64, budget)?;
    }
    let mut depth = 0;
    while depth < k {
        if cursors[depth].next_candidate(
            problem,
            problem.outcomes[depth],
            fixed_fee_payers[depth],
            &holds[depth],
            limits,
            budget,
        )? {
            reserve_work(
                budget,
                HostWorkDimension::SearchCandidates,
                n.checked_mul(3).ok_or(FundingSearchError::Overflow)?,
            )?;
            draws[depth].copy_from_slice(&cursors[depth].draws);
            fees[depth].copy_from_slice(&cursors[depth].fees);
            for i in 0..n {
                holds[depth + 1][i] = holds[depth][i].max(
                    draws[depth][i]
                        .checked_add(fees[depth][i])
                        .ok_or(FundingSearchError::Overflow)?,
                );
            }
            depth += 1;
            if depth < k {
                cursors[depth].reset();
            }
        } else if depth == 0 {
            return Ok(None);
        } else {
            depth -= 1;
        }
    }
    Ok(Some(FundingFamilySliceWitness {
        draws,
        fees,
        holds: holds.pop().ok_or(FundingSearchError::InvalidResult)?,
    }))
}

#[cfg(test)]
#[path = "funding_family_slice_tests.rs"]
mod tests;
