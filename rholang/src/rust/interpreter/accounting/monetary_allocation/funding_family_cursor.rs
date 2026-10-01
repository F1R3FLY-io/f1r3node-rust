use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;

use super::funding_family::{push, split_hold_box, sum, vector, HoldBox};
use super::funding_family_slice::{
    solve_funding_family_slice, FundingFamilySliceWitness, FundingResourceConstraint,
};
use super::*;
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingOutcomeCursorTransition {
    resource_next_cursor: Option<usize>,
    fee_next_cursor: Option<usize>,
    resource_unrestricted: bool,
    resource_restriction_witness: Option<Vec<u64>>,
    possible_fee_payers: Vec<bool>,
}

impl FundingOutcomeCursorTransition {
    pub fn resource_next_cursor(&self) -> Option<usize> { self.resource_next_cursor }
    pub fn fee_next_cursor(&self) -> Option<usize> { self.fee_next_cursor }
    pub fn resource_unrestricted(&self) -> bool { self.resource_unrestricted }
    pub fn possible_fee_payers(&self) -> &[bool] { &self.possible_fee_payers }
    pub fn resource_restriction_witness(&self) -> Option<&[u64]> {
        self.resource_restriction_witness.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingFamilyPolicySelection {
    allocation: FundingFamilyAllocation,
    transitions: Vec<FundingOutcomeCursorTransition>,
}

impl FundingFamilyPolicySelection {
    pub fn allocation(&self) -> &FundingFamilyAllocation { &self.allocation }
    pub fn transitions(&self) -> &[FundingOutcomeCursorTransition] { &self.transitions }
    pub fn into_parts(self) -> (FundingFamilyAllocation, Vec<FundingOutcomeCursorTransition>) {
        (self.allocation, self.transitions)
    }
}

fn array<T: Clone>(
    n: usize,
    value: T,
    budget: &HostWorkBudget,
) -> Result<Vec<T>, FundingSearchError> {
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        n.checked_mul(size_of::<T>())
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    reserve_work(budget, HostWorkDimension::SearchCandidates, n)?;
    filled_vec(n, value)
}

fn copy(values: &[u64], budget: &HostWorkBudget) -> Result<Vec<u64>, FundingSearchError> {
    let mut output = vector(values.len(), budget)?;
    output.copy_from_slice(values);
    Ok(output)
}

fn group_representatives(
    problem: FundingFamilyOptimizationProblem<'_>,
    groups: &[usize],
    limits: FundingFamilyLimits,
    budget: &HostWorkBudget,
) -> Result<Vec<usize>, FundingFamilyError> {
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
    if groups.len() != k || groups[0] != 0 {
        return Err(FundingFamilyError::InvalidOutcomeGroups);
    }
    let mut representatives = array(k, 0_usize, budget)?;
    let mut count = 0;
    for (i, case) in problem.outcomes.iter().enumerate() {
        let m = case.resources.obligations.len();
        if m > limits.search.obligation_cap.get() {
            return Err(
                FundingSearchError::from(FundingAssignmentError::TooManyObligations).into(),
            );
        }
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            n.checked_mul(m.checked_add(3).ok_or(FundingSearchError::Overflow)?)
                .and_then(|v| v.checked_add(m))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        if case.resources.capacities.len() != n
            || case.resources.eligible.len() != n
            || case.resources.eligible.iter().any(|row| row.len() != m)
            || case.unit_fee_eligible.is_some_and(|mask| mask.len() != n)
        {
            return Err(FundingSearchError::from(FundingAssignmentError::InvalidDimensions).into());
        }
        if i == 0 || groups[i] != groups[i - 1] {
            if groups[i] != count {
                return Err(FundingFamilyError::InvalidOutcomeGroups);
            }
            representatives[count] = i;
            count += 1;
        } else {
            let previous = problem.outcomes[representatives[count - 1]];
            if case.resources.capacities != previous.resources.capacities
                || case.resources.obligations != previous.resources.obligations
                || case.resources.eligible != previous.resources.eligible
                || case.unit_fee_eligible != previous.unit_fee_eligible
            {
                return Err(FundingFamilyError::InvalidOutcomeGroups);
            }
        }
    }
    representatives.truncate(count);
    Ok(representatives)
}

fn fill_cell(
    bounds: &HoldBox,
    total: u64,
    budget: &HostWorkBudget,
) -> Result<Option<Vec<u64>>, FundingSearchError> {
    reserve_work(
        budget,
        HostWorkDimension::SearchCandidates,
        bounds
            .lower
            .len()
            .checked_mul(3)
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let low = sum(&bounds.lower)?;
    if low > u128::from(total) || sum(&bounds.upper)? < u128::from(total) {
        return Ok(None);
    }
    let mut draw = copy(&bounds.lower, budget)?;
    let mut remaining = u128::from(total) - low;
    for (value, upper) in draw.iter_mut().zip(&bounds.upper) {
        let add = u128::from(*upper - *value).min(remaining);
        *value += u64::try_from(add).map_err(|_| FundingSearchError::Overflow)?;
        remaining -= add;
    }
    if remaining != 0 {
        return Err(FundingSearchError::InvalidResult);
    }
    Ok(Some(draw))
}

struct CursorContext<'a> {
    problem: FundingFamilyOptimizationProblem<'a>,
    selected: &'a [&'a FundingOutcomeAllocation],
    ranks: &'a [Vec<u64>],
    limits: FundingFamilyLimits,
    budget: &'a HostWorkBudget,
}

impl CursorContext<'_> {
    fn effective_capacities(&self, target: usize) -> Result<Vec<u64>, FundingSearchError> {
        let mut caps = vector(self.problem.capacities.len(), self.budget)?;
        for ((cap, common), local) in caps
            .iter_mut()
            .zip(self.problem.capacities)
            .zip(self.problem.outcomes[target].resources.capacities)
        {
            *cap = (*common).min(*local);
        }
        Ok(caps)
    }

    fn extension(
        &self,
        target: usize,
        draws: &[u64],
    ) -> Result<Option<FundingFamilySliceWitness>, FundingFamilyError> {
        let k = self.problem.outcomes.len();
        let mut constraints = array(k, FundingResourceConstraint::Fixed(draws), self.budget)?;
        let fees = array(k, None, self.budget)?;
        for (i, constraint) in constraints.iter_mut().enumerate() {
            *constraint = FundingResourceConstraint::Fixed(if i == target {
                draws
            } else {
                self.selected[i].resource_totals().source_debits()
            });
        }
        if let Some(witness) =
            solve_funding_family_slice(self.problem, &constraints, &fees, self.limits, self.budget)?
        {
            return Ok(Some(witness));
        }
        for (i, constraint) in constraints.iter_mut().enumerate().skip(target + 1) {
            *constraint = FundingResourceConstraint::Rank(&self.ranks[i]);
        }
        solve_funding_family_slice(self.problem, &constraints, &fees, self.limits, self.budget)
    }

    fn covers(
        &self,
        target: usize,
        bounds: &HoldBox,
        witness: &FundingFamilySliceWitness,
    ) -> Result<bool, FundingFamilyError> {
        let n = self.problem.capacities.len();
        reserve_work(
            self.budget,
            HostWorkDimension::VerificationOperations,
            n.checked_mul(
                self.problem
                    .outcomes
                    .len()
                    .checked_add(4)
                    .ok_or(FundingSearchError::Overflow)?,
            )
            .ok_or(FundingSearchError::Overflow)?,
        )?;
        let mut others = vector(n, self.budget)?;
        for (i, (draws, fees)) in witness.draws.iter().zip(&witness.fees).enumerate() {
            if i == target {
                continue;
            }
            for ((hold, draw), fee) in others.iter_mut().zip(draws).zip(fees) {
                *hold = (*hold).max(draw.checked_add(*fee).ok_or(FundingSearchError::Overflow)?);
            }
        }
        let mut coordinate_bound = 0_u128;
        for (i, upper) in bounds.upper.iter().enumerate() {
            let debit = u128::from(*upper) + u128::from(witness.fees[target][i]);
            if debit
                > u128::from(
                    self.problem.capacities[i]
                        .min(self.problem.outcomes[target].resources.capacities[i]),
                )
            {
                return Ok(false);
            }
            coordinate_bound = coordinate_bound
                .checked_add(debit.max(u128::from(others[i])))
                .ok_or(FundingSearchError::Overflow)?;
        }
        let total = self.selected[target].resource_totals().total();
        let additive_bound = sum(&others)?
            .checked_add(u128::from(total))
            .and_then(|v| {
                v.checked_add(u128::from(
                    self.problem.outcomes[target].unit_fee_eligible.is_some(),
                ))
            })
            .ok_or(FundingSearchError::Overflow)?;
        if coordinate_bound.min(additive_bound) > self.problem.exposure_limit {
            return Ok(false);
        }
        let case = self.problem.outcomes[target].resources;
        Ok(matches!(
            classify_fixed_funding_domain(
                &bounds.upper,
                case.obligations,
                case.eligible,
                self.limits.search,
                self.budget
            )?,
            FundingDomainClassification::Unrestricted { .. }
        ))
    }

    fn local_restriction(
        &self,
        target: usize,
        caps: &[u64],
    ) -> Result<Option<Vec<u64>>, FundingFamilyError> {
        let case = self.problem.outcomes[target];
        match classify_fixed_funding_domain(
            caps,
            case.resources.obligations,
            case.resources.eligible,
            self.limits.search,
            self.budget,
        )? {
            FundingDomainClassification::Restricted { counterexample, .. } => {
                return Ok(Some(copy(
                    counterexample.view().contributions,
                    self.budget,
                )?))
            }
            FundingDomainClassification::Infeasible { .. } => {
                return Err(FundingSearchError::InvalidResult.into())
            }
            FundingDomainClassification::Unrestricted { .. } => {}
        }
        let Some(allowed) = case.unit_fee_eligible else {
            return Ok(None);
        };
        reserve_work(
            self.budget,
            HostWorkDimension::VerificationOperations,
            caps.len(),
        )?;
        let capacity = caps
            .iter()
            .zip(allowed)
            .try_fold(0_u128, |sum, (cap, eligible)| {
                sum.checked_add(if *eligible { u128::from(*cap) } else { 0 })
                    .ok_or(FundingSearchError::Overflow)
            })?;
        let total = self.selected[target].resource_totals().total();
        if capacity > u128::from(total) {
            return Ok(None);
        }
        let mut draws = vector(caps.len(), self.budget)?;
        let mut remaining = total;
        for (i, eligible) in allowed.iter().enumerate() {
            if *eligible {
                draws[i] = caps[i];
                remaining = remaining
                    .checked_sub(caps[i])
                    .ok_or(FundingSearchError::InvalidResult)?;
            }
        }
        for (i, eligible) in allowed.iter().enumerate() {
            if !eligible {
                draws[i] = caps[i].min(remaining);
                remaining -= draws[i];
            }
        }
        if remaining != 0 {
            return Err(FundingSearchError::InvalidResult.into());
        }
        Ok(Some(draws))
    }

    fn restriction(
        &self,
        target: usize,
        caps: &[u64],
    ) -> Result<Option<Vec<u64>>, FundingFamilyError> {
        let total = self.selected[target].resource_totals().total();
        if total == 0 {
            return Ok(None);
        }
        if let Some(witness) = self.local_restriction(target, caps)? {
            return Ok(Some(witness));
        }
        let mut upper = copy(caps, self.budget)?;
        for value in &mut upper {
            *value = (*value).min(total);
        }
        let mut pending = Vec::new();
        push(
            &mut pending,
            HoldBox {
                lower: vector(caps.len(), self.budget)?,
                upper,
            },
            self.budget,
        )?;
        while let Some(bounds) = pending.pop() {
            let Some(draws) = fill_cell(&bounds, total, self.budget)? else {
                continue;
            };
            let Some(witness) = self.extension(target, &draws)? else {
                return Ok(Some(draws));
            };
            if !self.covers(target, &bounds, &witness)? {
                split_hold_box(bounds, &mut pending, self.budget)?;
            }
        }
        Ok(None)
    }

    fn fee_mask(&self, target: usize) -> Result<Vec<bool>, FundingFamilyError> {
        let n = self.problem.capacities.len();
        let k = self.problem.outcomes.len();
        let mut mask = array(n, false, self.budget)?;
        let Some(allowed) = self.problem.outcomes[target].unit_fee_eligible else {
            return Ok(mask);
        };
        let mut constraints = array(k, FundingResourceConstraint::Fixed(&[]), self.budget)?;
        let mut fees = array(k, None, self.budget)?;
        for (i, constraint) in constraints.iter_mut().enumerate() {
            *constraint = FundingResourceConstraint::Fixed(
                self.selected[i].resource_totals().source_debits(),
            );
            if i < target {
                fees[i] = self.selected[i].fee_debits().iter().position(|x| *x == 1);
            }
        }
        for (payer, possible) in mask.iter_mut().enumerate() {
            if !allowed[payer] {
                continue;
            }
            fees[target] = Some(payer);
            *possible = solve_funding_family_slice(
                self.problem,
                &constraints,
                &fees,
                self.limits,
                self.budget,
            )?
            .is_some();
        }
        Ok(mask)
    }
}

fn one_group(
    context: &CursorContext<'_>,
    caps: &[u64],
) -> Result<(bool, Option<usize>, Vec<bool>), FundingFamilyError> {
    let outcome = context.problem.outcomes[0];
    let resource = FundingMinimaxProblem {
        capacities: caps,
        ..outcome.resources
    };
    let cursor = context.problem.resource_cursor;
    let mut fee_mask = array(caps.len(), false, context.budget)?;
    if let Some(allowed) = outcome.unit_fee_eligible {
        let selected = select_funding_with_unit_fee(
            resource,
            allowed,
            cursor,
            context.problem.fee_cursor,
            context.limits.search,
            context.budget,
        )?
        .ok_or(FundingSearchError::InvalidResult)?;
        if selected.resource_totals() != context.selected[0].resource_totals() {
            return Err(FundingSearchError::InvalidResult.into());
        }
        for (i, possible) in fee_mask.iter_mut().enumerate() {
            *possible =
                allowed[i] && context.selected[0].resource_totals().source_debits()[i] < caps[i];
        }
        Ok((
            selected.resource_unrestricted(),
            selected.resource_next_cursor(),
            fee_mask,
        ))
    } else {
        let FundingPolicyResult::Selected(selected) =
            select_fixed_funding_policy(resource, cursor, context.limits.search, context.budget)?
        else {
            return Err(FundingSearchError::InvalidResult.into());
        };
        if selected.totals() != context.selected[0].resource_totals() {
            return Err(FundingSearchError::InvalidResult.into());
        }
        Ok((
            matches!(selected.fragment(), FundingPolicyFragment::Unrestricted),
            selected.next_cursor(),
            fee_mask,
        ))
    }
}

pub fn select_funding_family_policy(
    problem: FundingFamilyOptimizationProblem<'_>,
    groups: &[usize],
    limits: FundingFamilyLimits,
    budget: &HostWorkBudget,
) -> Result<Option<FundingFamilyPolicySelection>, FundingFamilyError> {
    let representatives = group_representatives(problem, groups, limits, budget)?;
    let Some(allocation) = optimize_funding_family(problem, limits, budget)? else {
        return Ok(None);
    };
    let k = representatives.len();
    let n = problem.capacities.len();
    let mut outcomes = array(k, problem.outcomes[0], budget)?;
    let mut selected = array(k, &allocation.outcomes()[0], budget)?;
    let mut ranks = array(k, Vec::new(), budget)?;
    for (group, index) in representatives.iter().copied().enumerate() {
        outcomes[group] = problem.outcomes[index];
        selected[group] = &allocation.outcomes()[index];
        ranks[group] = copy(selected[group].resource_totals().source_debits(), budget)?;
        reserve_work(
            budget,
            HostWorkDimension::SearchCandidates,
            n.checked_mul(
                n.checked_add(1)
                    .ok_or(FundingSearchError::Overflow)?
                    .ilog2() as usize
                    + 1,
            )
            .ok_or(FundingSearchError::Overflow)?,
        )?;
        ranks[group].sort_unstable_by(|a, b| b.cmp(a));
    }
    for (index, group) in groups.iter().copied().enumerate() {
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            n.checked_mul(2).ok_or(FundingSearchError::Overflow)?,
        )?;
        if allocation.outcomes()[index].resource_totals() != selected[group].resource_totals()
            || allocation.outcomes()[index].fee_debits() != selected[group].fee_debits()
        {
            return Err(FundingSearchError::InvalidResult.into());
        }
    }
    let context = CursorContext {
        problem: FundingFamilyOptimizationProblem {
            outcomes: &outcomes,
            ..problem
        },
        selected: &selected,
        ranks: &ranks,
        limits,
        budget,
    };
    let empty = FundingOutcomeCursorTransition {
        resource_next_cursor: None,
        fee_next_cursor: None,
        resource_unrestricted: true,
        resource_restriction_witness: None,
        possible_fee_payers: Vec::new(),
    };
    let mut transitions = array(k, empty, budget)?;
    for (target, transition) in transitions.iter_mut().enumerate() {
        let caps = context.effective_capacities(target)?;
        let total = selected[target].resource_totals().total();
        let (unrestricted, next, mask, excluded) = if k == 1 {
            let (unrestricted, next, mask) = one_group(&context, &caps)?;
            let excluded = if unrestricted {
                None
            } else {
                Some(
                    context
                        .local_restriction(target, &caps)?
                        .ok_or(FundingSearchError::InvalidResult)?,
                )
            };
            (unrestricted, next, mask, excluded)
        } else {
            let excluded = context.restriction(target, &caps)?;
            let unrestricted = excluded.is_none();
            let next = if total == 0 {
                None
            } else if unrestricted {
                reserve_work(
                    budget,
                    HostWorkDimension::SearchCandidates,
                    n.checked_mul(69).ok_or(FundingSearchError::Overflow)?,
                )?;
                reserve_work(
                    budget,
                    HostWorkDimension::SearchStateBytes,
                    n.checked_mul(size_of::<u64>())
                        .ok_or(FundingSearchError::Overflow)?,
                )?;
                let capped = allocate_capped_max_min(
                    &caps,
                    total,
                    problem.resource_cursor,
                    limits.search.source_cap,
                )
                .map_err(|_| FundingSearchError::InvalidResult)?;
                if capped.debits != selected[target].resource_totals().source_debits() {
                    return Err(FundingSearchError::InvalidResult.into());
                }
                Some(capped.next_cursor)
            } else {
                Some(if problem.resource_cursor == n - 1 {
                    0
                } else {
                    problem.resource_cursor + 1
                })
            };
            (unrestricted, next, context.fee_mask(target)?, excluded)
        };
        let fee_next = if outcomes[target].unit_fee_eligible.is_some() {
            let mut fee_caps = vector(n, budget)?;
            for (cap, allowed) in fee_caps.iter_mut().zip(&mask) {
                *cap = u64::from(*allowed);
            }
            reserve_work(
                budget,
                HostWorkDimension::SearchStateBytes,
                n.checked_mul(size_of::<u64>())
                    .ok_or(FundingSearchError::Overflow)?,
            )?;
            reserve_work(
                budget,
                HostWorkDimension::SearchCandidates,
                n.checked_mul(69).ok_or(FundingSearchError::Overflow)?,
            )?;
            let fee =
                allocate_capped_max_min(&fee_caps, 1, problem.fee_cursor, limits.search.source_cap)
                    .map_err(|_| FundingSearchError::InvalidResult)?;
            if fee.debits != selected[target].fee_debits() {
                return Err(FundingSearchError::InvalidResult.into());
            }
            Some(fee.next_cursor)
        } else {
            None
        };
        *transition = FundingOutcomeCursorTransition {
            resource_next_cursor: next,
            fee_next_cursor: fee_next,
            resource_unrestricted: unrestricted,
            resource_restriction_witness: excluded,
            possible_fee_payers: mask,
        };
    }
    let empty = FundingOutcomeCursorTransition {
        resource_next_cursor: None,
        fee_next_cursor: None,
        resource_unrestricted: true,
        resource_restriction_witness: None,
        possible_fee_payers: Vec::new(),
    };
    let mut expanded = array(groups.len(), empty, budget)?;
    for (entry, group) in expanded.iter_mut().zip(groups) {
        let value = &transitions[*group];
        let mut mask = array(n, false, budget)?;
        mask.copy_from_slice(&value.possible_fee_payers);
        *entry = FundingOutcomeCursorTransition {
            resource_next_cursor: value.resource_next_cursor,
            fee_next_cursor: value.fee_next_cursor,
            resource_unrestricted: value.resource_unrestricted,
            resource_restriction_witness: value
                .resource_restriction_witness
                .as_deref()
                .map(|draws| copy(draws, budget))
                .transpose()?,
            possible_fee_payers: mask,
        };
    }
    Ok(Some(FundingFamilyPolicySelection {
        allocation,
        transitions: expanded,
    }))
}

#[cfg(test)]
#[path = "funding_family_cursor_tests.rs"]
mod tests;
