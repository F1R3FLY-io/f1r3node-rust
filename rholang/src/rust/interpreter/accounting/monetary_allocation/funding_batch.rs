use std::mem::size_of;
use std::num::NonZeroUsize;

use models::rust::host_work::HostWorkDimension;
use thiserror::Error;

use super::{
    check_funding_assignment, filled_vec, reserve_work, FundingAssignmentError, FundingSearchError,
    FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct FixedFundingBatchEntry<'a> {
    pub obligations: &'a [u64],
    pub eligible: &'a [Vec<bool>],
    pub assignment: &'a [Vec<u64>],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedFixedFundingBatch {
    remaining: Vec<u64>,
    debits: Vec<u64>,
    total: u128,
}

impl CheckedFixedFundingBatch {
    pub fn remaining(&self) -> &[u64] { &self.remaining }
    pub fn source_debits(&self) -> &[u64] { &self.debits }
    pub fn total(&self) -> u128 { self.total }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum FundingBatchError {
    #[error("funding batch exceeds its entry limit")]
    TooManyEntries,
    #[error(transparent)]
    Check(#[from] FundingSearchError),
}

pub fn check_fixed_funding_batch(
    capacities: &[u64],
    entries: &[FixedFundingBatchEntry<'_>],
    limits: FundingSearchLimits,
    entry_cap: NonZeroUsize,
    budget: &HostWorkBudget,
) -> Result<CheckedFixedFundingBatch, FundingBatchError> {
    let sources = capacities.len();
    if sources == 0 {
        return Err(FundingSearchError::from(FundingAssignmentError::EmptySources).into());
    }
    if sources > limits.source_cap.get() {
        return Err(FundingSearchError::from(FundingAssignmentError::TooManySources).into());
    }
    if entries.len() > entry_cap.get() {
        return Err(FundingBatchError::TooManyEntries);
    }
    let state_bytes = sources
        .checked_mul(2 * size_of::<u64>())
        .ok_or(FundingSearchError::Overflow)?;
    reserve_work(budget, HostWorkDimension::SearchStateBytes, state_bytes)?;
    reserve_work(budget, HostWorkDimension::VerificationOperations, sources)?;
    let mut remaining = filled_vec(sources, 0_u64)?;
    remaining.copy_from_slice(capacities);
    let mut total = 0_u128;
    for entry in entries {
        let obligations = entry.obligations.len();
        if obligations > limits.obligation_cap.get() {
            return Err(
                FundingSearchError::from(FundingAssignmentError::TooManyObligations).into(),
            );
        }
        let work = sources
            .checked_mul(obligations)
            .and_then(|cells| cells.checked_add(sources.checked_mul(3)?))
            .and_then(|count| count.checked_add(obligations))
            .and_then(|count| count.checked_add(1))
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::VerificationOperations, work)?;
        let bytes = sources
            .checked_add(obligations)
            .and_then(|count| count.checked_mul(size_of::<u64>()))
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        let checked = check_funding_assignment(
            &remaining,
            entry.obligations,
            entry.eligible,
            entry.assignment,
            limits.source_cap,
            limits.obligation_cap,
        )
        .map_err(FundingSearchError::from)?;
        for (balance, debit) in remaining.iter_mut().zip(checked.source_debits()) {
            *balance = balance
                .checked_sub(*debit)
                .ok_or(FundingSearchError::InvalidResult)?;
        }
        total = total
            .checked_add(u128::from(checked.total()))
            .ok_or(FundingSearchError::Overflow)?;
    }
    reserve_work(budget, HostWorkDimension::VerificationOperations, sources)?;
    let mut debits = filled_vec(sources, 0_u64)?;
    for ((debit, capacity), residual) in debits.iter_mut().zip(capacities).zip(&remaining) {
        *debit = capacity
            .checked_sub(*residual)
            .ok_or(FundingSearchError::InvalidResult)?;
    }
    Ok(CheckedFixedFundingBatch {
        remaining,
        debits,
        total,
    })
}
