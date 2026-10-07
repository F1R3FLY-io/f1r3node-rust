use std::cmp::Ordering;
use std::mem::size_of;

use models::rust::phlo_obligation::PhloObligationKeyLimits;

use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::{
    canonicalize_funding_problem, filled_vec, reserve_work, select_funding_family_policy,
    CanonicalFundingProblem, FundingAssignmentError, FundingAssignmentTotals,
    FundingBranchReservation, FundingFamilyError, FundingFamilyLimits,
    FundingFamilyOptimizationProblem, FundingIdentityError, FundingMinimaxProblem,
    FundingOutcomeCursorTransition, FundingOutcomeProblem, FundingSearchError, FundingSearchLimits,
};

#[derive(Clone, Copy, Debug)]
pub struct PhloFundingRequirement<'a> {
    pub obligations: &'a CheckedPhloObligations<'a>,
    pub eligible: &'a [Vec<bool>],
}

#[derive(Clone, Copy, Debug)]
pub struct PhloFamilyFundingInput<'a> {
    pub sources: &'a [PhloFundingSource<'a>],
    pub outcomes: &'a [PhloFundingRequirement<'a>],
    pub total_exposure_limit: u128,
    pub canonical_resource_cursor: usize,
    pub canonical_fee_cursor: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct PhloFamilyFundingLimits {
    pub funding: PhloFundingLimits,
    pub keys: PhloObligationKeyLimits,
    pub aggregate_key_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloFamilyFundingSelection {
    assignments: Vec<Vec<Vec<u64>>>,
    source_holds: Vec<u64>,
    total_held: u128,
    canonical_outcome_positions: Vec<usize>,
    cursor_transitions: Vec<FundingOutcomeCursorTransition>,
}

#[derive(Clone, Copy, Debug)]
pub struct PhloFamilyFundingProposal<'a> {
    pub assignments: &'a [Vec<Vec<u64>>],
    pub source_holds: &'a [u64],
}

pub fn verify_phlo_funding_family(
    input: PhloFamilyFundingInput<'_>,
    proposed: PhloFamilyFundingProposal<'_>,
    limits: PhloFamilyFundingLimits,
    budget: &HostWorkBudget,
) -> Result<bool, PhloFamilyFundingError> {
    let Some(selected) = select_phlo_funding_family(input, limits, budget)? else {
        return Ok(false);
    };
    let work = input
        .outcomes
        .iter()
        .try_fold(input.sources.len(), |total, case| {
            input
                .sources
                .len()
                .checked_mul(case.obligations.amounts().len().checked_add(1)?)
                .and_then(|cells| total.checked_add(cells))
                .and_then(|value| value.checked_add(1))
        })
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::VerificationOperations, work)?;
    Ok(selected.assignments() == proposed.assignments
        && selected.source_holds() == proposed.source_holds)
}

impl CheckedPhloFundingIntent<'_> {
    pub fn verify_funding_policy(
        self,
        canonical_resource_cursor: usize,
        canonical_fee_cursor: usize,
        limits: PhloFamilyFundingLimits,
        budget: &HostWorkBudget,
    ) -> Result<bool, PhloFamilyFundingError> {
        Ok(self
            .verified_funding_selection(
                canonical_resource_cursor,
                canonical_fee_cursor,
                limits,
                budget,
            )?
            .is_some())
    }

    pub(super) fn verified_funding_selection(
        self,
        canonical_resource_cursor: usize,
        canonical_fee_cursor: usize,
        limits: PhloFamilyFundingLimits,
        budget: &HostWorkBudget,
    ) -> Result<Option<PhloFamilyFundingSelection>, PhloFamilyFundingError> {
        let family = self.bound().consent().family();
        if family.cases().len() > limits.funding.cases.get() {
            return Err(PhloFundingError::TooManyCases.into());
        }
        let Some(first) = family.cases().first() else {
            return Err(PhloFundingError::EmptyCases.into());
        };
        let mut outcomes = vector(
            family.cases().len(),
            PhloFundingRequirement {
                obligations: first.obligations,
                eligible: first.eligible,
            },
            budget,
        )?;
        for (output, case) in outcomes.iter_mut().zip(family.cases()) {
            *output = PhloFundingRequirement {
                obligations: case.obligations,
                eligible: case.eligible,
            };
        }
        let Some(selected) = select_phlo_funding_family(
            PhloFamilyFundingInput {
                sources: family.sources(),
                outcomes: &outcomes,
                total_exposure_limit: family.total_exposure_limit(),
                canonical_resource_cursor,
                canonical_fee_cursor,
            },
            limits,
            budget,
        )?
        else {
            return Ok(None);
        };
        let work = family
            .cases()
            .iter()
            .try_fold(family.sources().len(), |total, case| {
                family
                    .sources()
                    .len()
                    .checked_mul(case.obligations.amounts().len().checked_add(1)?)
                    .and_then(|cells| total.checked_add(cells))
                    .and_then(|value| value.checked_add(1))
            })
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::VerificationOperations, work)?;
        let matches = selected.source_holds() == family.source_holds()
            && selected
                .assignments()
                .iter()
                .zip(family.cases())
                .all(|(assignment, case)| assignment == case.assignment);
        Ok(matches.then_some(selected))
    }
}

impl PhloFamilyFundingSelection {
    pub fn cursor_transitions(&self) -> &[FundingOutcomeCursorTransition] {
        &self.cursor_transitions
    }
    pub fn assignments(&self) -> &[Vec<Vec<u64>>] { &self.assignments }
    pub fn source_holds(&self) -> &[u64] { &self.source_holds }
    pub fn total_held(&self) -> u128 { self.total_held }
    pub fn canonical_outcome_positions(&self) -> &[usize] { &self.canonical_outcome_positions }
}

#[derive(Debug, Error)]
pub enum PhloFamilyFundingError {
    #[error(transparent)]
    Funding(#[from] PhloFundingError),
    #[error(transparent)]
    Obligations(#[from] PhloObligationError),
    #[error(transparent)]
    Identity(#[from] FundingIdentityError),
    #[error(transparent)]
    Family(#[from] FundingFamilyError),
    #[error(transparent)]
    Search(#[from] FundingSearchError),
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

struct PreparedOutcome<'a> {
    problem: CanonicalFundingProblem<'a>,
    quantities: Vec<u64>,
    unit_amounts: Vec<u64>,
    fee: u64,
    fee_eligible: Vec<bool>,
    comparison_work: usize,
}

fn compare_runs<T: Ord>(
    left: &[T],
    left_counts: &[u64],
    right: &[T],
    right_counts: &[u64],
) -> Ordering {
    let (mut l, mut r) = (0, 0);
    let (mut lc, mut rc) = (0, 0);
    while l < left.len() && r < right.len() {
        let order = left[l].cmp(&right[r]);
        if order != Ordering::Equal {
            return order;
        }
        if lc == 0 {
            lc = left_counts[l];
        }
        if rc == 0 {
            rc = right_counts[r];
        }
        let consumed = lc.min(rc);
        lc -= consumed;
        rc -= consumed;
        if lc == 0 {
            l += 1;
        }
        if rc == 0 {
            r += 1;
        }
    }
    (left.len() - l).cmp(&(right.len() - r))
}

fn compare(
    left: &PreparedOutcome<'_>,
    right: &PreparedOutcome<'_>,
    budget: &HostWorkBudget,
) -> Result<Ordering, FundingSearchError> {
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        left.comparison_work
            .checked_add(right.comparison_work)
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    Ok(left
        .fee
        .cmp(&right.fee)
        .then_with(|| {
            compare_runs(
                left.problem.obligation_keys(),
                &left.quantities,
                right.problem.obligation_keys(),
                &right.quantities,
            )
        })
        .then_with(|| {
            compare_runs(
                &left.unit_amounts,
                &left.quantities,
                &right.unit_amounts,
                &right.quantities,
            )
        })
        .then_with(|| left.fee_eligible.cmp(&right.fee_eligible))
        .then_with(|| {
            left.problem
                .problem()
                .eligible
                .iter()
                .zip(right.problem.problem().eligible)
                .map(|(l, r)| compare_runs(l, &left.quantities, r, &right.quantities))
                .find(|order| *order != Ordering::Equal)
                .unwrap_or(Ordering::Equal)
        }))
}

fn outcome_order(
    outcomes: &[PreparedOutcome<'_>],
    budget: &HostWorkBudget,
) -> Result<Vec<usize>, FundingSearchError> {
    let n = outcomes.len();
    let mut order = vector(n, 0, budget)?;
    for (index, value) in order.iter_mut().enumerate() {
        *value = index;
    }
    let mut buffer = vector(n, 0, budget)?;
    let mut width = 1;
    while width < n {
        let mut start = 0;
        while start < n {
            let middle = start + width.min(n - start);
            let end = middle + width.min(n - middle);
            let (mut left, mut right) = (start, middle);
            for output in &mut buffer[start..end] {
                reserve_work(budget, HostWorkDimension::SearchCandidates, 1)?;
                if left < middle
                    && (right == end
                        || compare(&outcomes[order[left]], &outcomes[order[right]], budget)?
                            != Ordering::Greater)
                {
                    *output = order[left];
                    left += 1;
                } else {
                    *output = order[right];
                    right += 1;
                }
            }
            start = end;
        }
        std::mem::swap(&mut order, &mut buffer);
        width = width.checked_mul(2).unwrap_or(n);
    }
    Ok(order)
}

fn validate(
    input: PhloFamilyFundingInput<'_>,
    limits: PhloFundingLimits,
    budget: &HostWorkBudget,
) -> Result<(), PhloFamilyFundingError> {
    let n = input.sources.len();
    if n == 0 {
        return Err(PhloFundingError::EmptySources.into());
    }
    if n > limits.sources.get() {
        return Err(PhloFundingError::TooManySources.into());
    }
    let Some(first) = input.outcomes.first() else {
        return Err(PhloFundingError::EmptyCases.into());
    };
    if input.outcomes.len() > limits.cases.get() {
        return Err(PhloFundingError::TooManyCases.into());
    }
    if input.canonical_resource_cursor >= n || input.canonical_fee_cursor >= n {
        return Err(FundingSearchError::InvalidPriorityCursor.into());
    }
    reserve_work(budget, HostWorkDimension::VerificationOperations, n)?;
    let mut remaining = limits.custody_bytes;
    for source in input.sources {
        remaining = remaining
            .checked_sub(source.custody.len())
            .ok_or(PhloFundingError::TooManyCustodyBytes)?;
    }
    let mut remaining = limits.assignment_cells;
    for case in input.outcomes {
        let m = case.obligations.amounts().len();
        if m > limits.obligations.get() {
            return Err(PhloFundingError::TooManyObligations.into());
        }
        let cells = n.checked_mul(m).ok_or(FundingSearchError::Overflow)?;
        remaining = remaining
            .checked_sub(cells)
            .ok_or(PhloFundingError::TooManyAssignmentCells)?;
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            n.checked_add(1).ok_or(FundingSearchError::Overflow)?,
        )?;
        if case.eligible.len() != n || case.eligible.iter().any(|row| row.len() != m) {
            return Err(FundingSearchError::from(FundingAssignmentError::InvalidDimensions).into());
        }
        if case.obligations.keys().first() != Some(&PhloObligationKey::Fee)
            || case
                .obligations
                .amounts()
                .first()
                .is_none_or(|fee| *fee > 1)
        {
            return Err(FundingSearchError::InvalidResult.into());
        }
        let controls = case.obligations.execution().controls();
        let first_controls = first.obligations.execution().controls();
        reserve_controls(controls, budget)?;
        reserve_controls(first_controls, budget)?;
        if controls != first_controls {
            return Err(PhloFundingError::DifferentControls.into());
        }
    }
    Ok(())
}

fn reserve_controls(
    controls: CheckedPhloControls<'_>,
    budget: &HostWorkBudget,
) -> Result<(), FundingSearchError> {
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        controls
            .terms()
            .required_owner_ceilings
            .len()
            .checked_add(16)
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    for schedule in std::iter::once(controls.schedule())
        .chain(controls.terms().permitted_schedules.iter().copied())
    {
        let environment = schedule.environment;
        let work = [
            32,
            environment.network.len(),
            environment.shard.len(),
            environment.asset.len(),
            environment.unit.len(),
            schedule.weights.len(),
        ]
        .into_iter()
        .try_fold(0_usize, |sum, value| {
            sum.checked_add(value).ok_or(FundingSearchError::Overflow)
        })?;
        reserve_work(budget, HostWorkDimension::VerificationOperations, work)?;
    }
    Ok(())
}

pub fn select_phlo_funding_family(
    input: PhloFamilyFundingInput<'_>,
    limits: PhloFamilyFundingLimits,
    budget: &HostWorkBudget,
) -> Result<Option<PhloFamilyFundingSelection>, PhloFamilyFundingError> {
    validate(input, limits.funding, budget)?;
    let n = input.sources.len();
    let k = input.outcomes.len();
    let search = FundingSearchLimits {
        source_cap: limits.funding.sources,
        obligation_cap: limits.funding.obligations,
    };
    let mut source_keys = vector(n, &[][..], budget)?;
    let mut debit_caps = vector(n, 0_u64, budget)?;
    let mut capacities = vector(n, 0_u64, budget)?;
    let mut exposures = vector(n, 0_u64, budget)?;
    for (i, source) in input.sources.iter().enumerate() {
        source_keys[i] = source.custody;
        debit_caps[i] = source.capacity.min(source.debit_limit);
        capacities[i] = source.capacity;
        exposures[i] = source.exposure_limit;
    }
    let mut encoded = vector(k, Vec::new(), budget)?;
    let mut remaining = limits.aggregate_key_bytes;
    for (keys, case) in encoded.iter_mut().zip(input.outcomes) {
        *keys = case
            .obligations
            .encoded_keys_with_budget(limits.keys, remaining, budget)?;
        for key in keys.iter() {
            remaining = remaining
                .checked_sub(key.len())
                .ok_or(FundingSearchError::InvalidResult)?;
        }
    }
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        k.checked_mul(size_of::<PreparedOutcome<'_>>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut prepared = Vec::new();
    prepared
        .try_reserve_exact(k)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    for (case, encoded) in input.outcomes.iter().zip(&encoded) {
        let m = case.obligations.amounts().len() - 1;
        let mut keys = vector(m, &[][..], budget)?;
        let mut edges = vector(n, Vec::new(), budget)?;
        for (key, encoded) in keys.iter_mut().zip(&encoded[1..]) {
            *key = encoded;
        }
        for (edge, row) in edges.iter_mut().zip(case.eligible) {
            *edge = vector(m, false, budget)?;
            edge.copy_from_slice(&row[1..]);
        }
        let problem = canonicalize_funding_problem(
            FundingMinimaxProblem {
                capacities: &debit_caps,
                obligations: &case.obligations.amounts()[1..],
                eligible: &edges,
            },
            &source_keys,
            &keys,
            search,
            budget,
        )?;
        let mut fee_eligible = vector(n, false, budget)?;
        let mut quantities = vector(m, 0_u64, budget)?;
        let mut unit_amounts = vector(m, 0_u64, budget)?;
        for (index, original) in problem
            .original_obligation_positions()
            .iter()
            .copied()
            .enumerate()
        {
            let quantity = case.obligations.quantities()[original + 1];
            let amount = case.obligations.amounts()[original + 1];
            if quantity == 0 || amount % quantity != 0 {
                return Err(FundingSearchError::InvalidResult.into());
            }
            quantities[index] = quantity;
            unit_amounts[index] = amount / quantity;
        }
        for (i, original) in problem
            .original_source_positions()
            .iter()
            .copied()
            .enumerate()
        {
            fee_eligible[i] = case.eligible[original][0];
        }
        let comparison_work = keys
            .iter()
            .try_fold(1_usize, |total, key| {
                total
                    .checked_add(key.len())
                    .and_then(|v| v.checked_add(1))
                    .ok_or(FundingSearchError::Overflow)
            })?
            .checked_add(
                n.checked_mul(m.checked_add(2).ok_or(FundingSearchError::Overflow)?)
                    .ok_or(FundingSearchError::Overflow)?,
            )
            .and_then(|v| v.checked_add(m.checked_mul(8)?))
            .ok_or(FundingSearchError::Overflow)?;
        prepared.push(PreparedOutcome {
            problem,
            quantities,
            unit_amounts,
            fee: case.obligations.amounts()[0],
            fee_eligible,
            comparison_work,
        });
    }
    let order = outcome_order(&prepared, budget)?;
    let mut common = vector(n, 0_u64, budget)?;
    for (i, original) in prepared[0]
        .problem
        .original_source_positions()
        .iter()
        .copied()
        .enumerate()
    {
        common[i] = capacities[original].min(exposures[original]);
    }
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        k.checked_mul(size_of::<FundingOutcomeProblem<'_>>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut outcomes = Vec::new();
    outcomes
        .try_reserve_exact(k)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    for index in &order {
        let case = &prepared[*index];
        outcomes.push(FundingOutcomeProblem {
            resources: case.problem.problem(),
            unit_fee_eligible: (case.fee == 1).then_some(case.fee_eligible.as_slice()),
        });
    }
    let mut groups = vector(k, 0_usize, budget)?;
    for index in 1..k {
        groups[index] = groups[index - 1]
            + usize::from(
                compare(&prepared[order[index - 1]], &prepared[order[index]], budget)?
                    != Ordering::Equal,
            );
    }
    let Some(policy) = select_funding_family_policy(
        FundingFamilyOptimizationProblem {
            capacities: &common,
            outcomes: &outcomes,
            exposure_limit: input.total_exposure_limit,
            resource_cursor: input.canonical_resource_cursor,
            fee_cursor: input.canonical_fee_cursor,
        },
        &groups,
        FundingFamilyLimits {
            search,
            case_cap: limits.funding.cases,
        },
        budget,
    )?
    else {
        return Ok(None);
    };
    let (selected, transitions) = policy.into_parts();
    let mut restored = vector(k, None, budget)?;
    for (transition, original) in transitions.into_iter().zip(order.iter().copied()) {
        restored[original] = Some(transition);
    }
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        k.checked_mul(size_of::<FundingOutcomeCursorTransition>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut cursor_transitions = Vec::new();
    cursor_transitions
        .try_reserve_exact(k)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    for transition in restored {
        cursor_transitions.push(transition.ok_or(FundingSearchError::InvalidResult)?);
    }
    let mut assignments = vector(k, Vec::new(), budget)?;
    for (allocation, original) in selected.outcomes().iter().zip(order.iter().copied()) {
        let case = &prepared[original];
        let resources = case
            .problem
            .restore_assignment(allocation.resources(), budget)?;
        let m = input.outcomes[original].obligations.amounts().len();
        assignments[original] = vector(n, Vec::new(), budget)?;
        for (canonical, source) in case
            .problem
            .original_source_positions()
            .iter()
            .copied()
            .enumerate()
        {
            assignments[original][source] = vector(m, 0_u64, budget)?;
            assignments[original][source][0] = allocation.fee_debits()[canonical];
            assignments[original][source][1..].copy_from_slice(&resources[source]);
        }
    }
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        k.checked_mul(size_of::<FundingAssignmentTotals>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut plans = Vec::new();
    plans
        .try_reserve_exact(k)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    for (case, assignment) in input.outcomes.iter().zip(&assignments) {
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            n.checked_mul(
                case.obligations
                    .amounts()
                    .len()
                    .checked_add(2)
                    .ok_or(FundingSearchError::Overflow)?,
            )
            .ok_or(FundingSearchError::Overflow)?,
        )?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            n.checked_add(case.obligations.amounts().len())
                .and_then(|v| v.checked_mul(size_of::<u64>()))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        plans.push(
            case.obligations
                .check_assignment(
                    &debit_caps,
                    case.eligible,
                    assignment,
                    limits.funding.sources,
                    limits.funding.obligations,
                )
                .map_err(PhloFundingError::from)?,
        );
    }
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        n.checked_mul(k.checked_add(2).ok_or(FundingSearchError::Overflow)?)
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        n.checked_mul(2 * size_of::<u64>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let reservation = FundingBranchReservation::from_plans(
        &capacities,
        &exposures,
        plans,
        limits.funding.sources,
        limits.funding.cases,
    )
    .map_err(PhloFundingError::from)?;
    if reservation.total_held() != selected.total_held()
        || reservation.total_held() > input.total_exposure_limit
        || prepared[0]
            .problem
            .original_source_positions()
            .iter()
            .copied()
            .enumerate()
            .any(|(canonical, original)| {
                reservation.source_holds()[original] != selected.holds()[canonical]
            })
    {
        return Err(FundingSearchError::InvalidResult.into());
    }
    let mut source_holds = filled_vec(n, 0_u64)?;
    source_holds.copy_from_slice(reservation.source_holds());
    Ok(Some(PhloFamilyFundingSelection {
        assignments,
        source_holds,
        total_held: reservation.total_held(),
        canonical_outcome_positions: order,
        cursor_transitions,
    }))
}

#[cfg(test)]
#[path = "family_policy_tests.rs"]
mod tests;
