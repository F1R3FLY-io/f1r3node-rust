use std::mem::size_of;

use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::{
    canonicalize_funding_problem, filled_vec, reserve_work, FundingIdentityError,
    FundingMinimaxProblem, FundingPolicyResult, FundingSearchError, FundingSearchLimits,
};

#[derive(Clone, Copy, Debug)]
pub struct PhloObligationFundingInput<'a> {
    pub source_keys: &'a [&'a [u8]],
    pub capacities: &'a [u64],
    pub eligible: &'a [Vec<bool>],
    pub canonical_resource_cursor: usize,
    pub canonical_fee_cursor: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct PhloObligationFundingLimits {
    pub search: FundingSearchLimits,
    pub keys: PhloObligationKeyLimits,
    pub aggregate_key_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloObligationFunding {
    assignment: Vec<Vec<u64>>,
    totals: FundingAssignmentTotals,
    resource_next_cursor: Option<usize>,
    fee_next_cursor: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
pub struct PhloObligationFundingProposal<'a> {
    pub assignment: &'a [Vec<u64>],
    pub resource_next_cursor: Option<usize>,
    pub fee_next_cursor: Option<usize>,
}

impl PhloObligationFunding {
    pub fn assignment(&self) -> &[Vec<u64>] { &self.assignment }
    pub fn totals(&self) -> &FundingAssignmentTotals { &self.totals }
    pub fn resource_next_cursor(&self) -> Option<usize> { self.resource_next_cursor }
    pub fn fee_next_cursor(&self) -> Option<usize> { self.fee_next_cursor }
}

#[derive(Debug, Error)]
pub enum PhloObligationFundingError {
    #[error(transparent)]
    Obligations(#[from] PhloObligationError),
    #[error(transparent)]
    Identity(#[from] FundingIdentityError),
    #[error(transparent)]
    Search(#[from] FundingSearchError),
}

impl CheckedPhloObligations<'_> {
    pub fn verify_funding(
        &self,
        input: PhloObligationFundingInput<'_>,
        proposed: PhloObligationFundingProposal<'_>,
        limits: PhloObligationFundingLimits,
        budget: &HostWorkBudget,
    ) -> Result<bool, PhloObligationFundingError> {
        let Some(selected) = self.select_funding(input, limits, budget)? else {
            return Ok(false);
        };
        let cells = input
            .capacities
            .len()
            .checked_mul(self.amounts.len())
            .and_then(|cells| cells.checked_add(input.capacities.len()))
            .and_then(|cells| cells.checked_add(2))
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::VerificationOperations, cells)?;
        Ok(selected.assignment() == proposed.assignment
            && selected.resource_next_cursor() == proposed.resource_next_cursor
            && selected.fee_next_cursor() == proposed.fee_next_cursor)
    }

    pub fn select_funding(
        &self,
        input: PhloObligationFundingInput<'_>,
        limits: PhloObligationFundingLimits,
        budget: &HostWorkBudget,
    ) -> Result<Option<PhloObligationFunding>, PhloObligationFundingError> {
        let count = input.capacities.len();
        let columns = self.amounts.len();
        if count == 0 {
            return Err(FundingSearchError::from(FundingAssignmentError::EmptySources).into());
        }
        if count > limits.search.source_cap.get() {
            return Err(FundingSearchError::from(FundingAssignmentError::TooManySources).into());
        }
        if columns > limits.search.obligation_cap.get() {
            return Err(
                FundingSearchError::from(FundingAssignmentError::TooManyObligations).into(),
            );
        }
        reserve_work(budget, HostWorkDimension::VerificationOperations, count)?;
        if input.eligible.len() != count || input.eligible.iter().any(|row| row.len() != columns) {
            return Err(FundingSearchError::from(FundingAssignmentError::InvalidDimensions).into());
        }
        if input.canonical_resource_cursor >= count || input.canonical_fee_cursor >= count {
            return Err(FundingSearchError::InvalidPriorityCursor.into());
        }
        if self.keys.first() != Some(&PhloObligationKey::Fee)
            || self.amounts.first().is_none_or(|amount| *amount > 1)
        {
            return Err(FundingSearchError::InvalidResult.into());
        }
        let cells = count
            .checked_mul(columns)
            .ok_or(FundingSearchError::Overflow)?;
        let bytes = count
            .checked_mul(size_of::<Vec<bool>>() + size_of::<Vec<u64>>() + size_of::<u64>() + 1)
            .and_then(|bytes| {
                cells
                    .checked_mul(1 + size_of::<u64>())
                    .and_then(|cells| bytes.checked_add(cells))
            })
            .and_then(|bytes| {
                columns
                    .checked_mul(size_of::<&[u8]>())
                    .and_then(|keys| bytes.checked_add(keys))
            })
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        reserve_work(budget, HostWorkDimension::SearchCandidates, cells)?;
        let mut resource_edges = filled_vec(count, Vec::new())?;
        let mut fee_eligible = filled_vec(count, false)?;
        for i in 0..count {
            resource_edges[i] = filled_vec(columns - 1, false)?;
            resource_edges[i].copy_from_slice(&input.eligible[i][1..]);
            fee_eligible[i] = input.eligible[i][0];
        }
        let encoded =
            self.encoded_keys_with_budget(limits.keys, limits.aggregate_key_bytes, budget)?;
        let mut keys = filled_vec(columns - 1, &[][..])?;
        for (key, encoded) in keys.iter_mut().zip(&encoded[1..]) {
            *key = encoded;
        }
        let problem = canonicalize_funding_problem(
            FundingMinimaxProblem {
                capacities: input.capacities,
                obligations: &self.amounts[1..],
                eligible: &resource_edges,
            },
            input.source_keys,
            &keys,
            limits.search,
            budget,
        )?;
        let (resource_assignment, fee, resource_next_cursor, fee_next_cursor) =
            if self.amounts[0] == 1 {
                let Some(selected) = problem.select_with_unit_fee(
                    &fee_eligible,
                    input.canonical_resource_cursor,
                    input.canonical_fee_cursor,
                    limits.search,
                    budget,
                )?
                else {
                    return Ok(None);
                };
                (
                    problem.restore_assignment(selected.resource_assignment(), budget)?,
                    Some(selected.fee().debits.clone()),
                    selected.resource_next_cursor(),
                    Some(selected.fee().next_cursor),
                )
            } else {
                let FundingPolicyResult::Selected(selected) =
                    problem.select(input.canonical_resource_cursor, limits.search, budget)?
                else {
                    return Ok(None);
                };
                (
                    problem.restore_assignment(selected.assignment(), budget)?,
                    None,
                    selected.next_cursor(),
                    None,
                )
            };
        let mut assignment = filled_vec(count, Vec::new())?;
        for (canonical, original) in problem
            .original_source_positions()
            .iter()
            .copied()
            .enumerate()
        {
            assignment[original] = filled_vec(columns, 0_u64)?;
            assignment[original][0] = fee.as_ref().map_or(0, |debits| debits[canonical]);
            assignment[original][1..].copy_from_slice(&resource_assignment[original]);
        }
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            cells
                .checked_add(count)
                .and_then(|value| value.checked_add(columns))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            count
                .checked_add(columns)
                .and_then(|value| value.checked_mul(size_of::<u64>()))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let totals = self
            .check_assignment(
                input.capacities,
                input.eligible,
                &assignment,
                limits.search.source_cap,
                limits.search.obligation_cap,
            )
            .map_err(FundingSearchError::from)?;
        Ok(Some(PhloObligationFunding {
            assignment,
            totals,
            resource_next_cursor,
            fee_next_cursor,
        }))
    }
}
