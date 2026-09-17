use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use models::rust::phlo_obligation::PhloObligationKeyLimits;
use thiserror::Error;

use super::{CheckedPhloFundingIntent, PhloFundingSource, PhloObligationError};
use crate::rust::interpreter::accounting::monetary_allocation::{
    canonicalize_funding_problem, check_funding_assignment, filled_vec, reserve_work,
    FundingAssignmentError, FundingIdentityError, FundingMinimaxProblem, FundingReservationError,
    FundingSearchError, FundingSearchLimits,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct PhloCaptureLimits {
    pub funding: FundingSearchLimits,
    pub key: PhloObligationKeyLimits,
    pub aggregate_key_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturedPhloSource<'a> {
    source: PhloFundingSource<'a>,
    hold: u64,
    debit: u64,
    fee: u64,
    refund: u64,
}

impl<'a> CapturedPhloSource<'a> {
    pub fn source(self) -> PhloFundingSource<'a> { self.source }
    pub fn hold(self) -> u64 { self.hold }
    pub fn debit(self) -> u64 { self.debit }
    pub fn fee(self) -> u64 { self.fee }
    pub fn acquisition(self) -> u64 { self.debit - self.fee }
    pub fn refund(self) -> u64 { self.refund }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalPhloFundingCapture<'a> {
    intent: CheckedPhloFundingIntent<'a>,
    branch: usize,
    sources: Vec<CapturedPhloSource<'a>>,
    obligation_keys: Vec<Vec<u8>>,
    amounts: Vec<u64>,
    eligible: Vec<Vec<bool>>,
    assignment: Vec<Vec<u64>>,
}

impl<'a> CanonicalPhloFundingCapture<'a> {
    pub fn intent(&self) -> CheckedPhloFundingIntent<'a> { self.intent }
    pub fn branch(&self) -> usize { self.branch }
    pub fn sources(&self) -> &[CapturedPhloSource<'a>] { &self.sources }
    pub fn obligation_keys(&self) -> &[Vec<u8>] { &self.obligation_keys }
    pub fn amounts(&self) -> &[u64] { &self.amounts }
    pub fn eligible(&self) -> &[Vec<bool>] { &self.eligible }
    pub fn assignment(&self) -> &[Vec<u64>] { &self.assignment }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloCaptureError {
    #[error(transparent)]
    Search(#[from] FundingSearchError),
    #[error(transparent)]
    Identity(#[from] FundingIdentityError),
    #[error(transparent)]
    Obligation(#[from] PhloObligationError),
    #[error(transparent)]
    Branch(#[from] FundingReservationError),
}

impl<'a> CheckedPhloFundingIntent<'a> {
    pub fn capture_case(
        self,
        branch: usize,
        limits: PhloCaptureLimits,
        budget: &HostWorkBudget,
    ) -> Result<CanonicalPhloFundingCapture<'a>, PhloCaptureError> {
        let family = self.bound().consent().family();
        let case = family
            .cases()
            .get(branch)
            .ok_or(FundingReservationError::UnknownBranch)?;
        let n = family.sources().len();
        let m = case.obligations.keys().len();
        if n > limits.funding.source_cap.get() {
            return Err(FundingSearchError::from(FundingAssignmentError::TooManySources).into());
        }
        if m > limits.funding.obligation_cap.get() {
            return Err(
                FundingSearchError::from(FundingAssignmentError::TooManyObligations).into(),
            );
        }
        let cells = n.checked_mul(m).ok_or(FundingSearchError::Overflow)?;
        let preparation = n
            .checked_add(m)
            .and_then(|x| x.checked_add(cells))
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::SearchCandidates, preparation)?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            preparation
                .checked_mul(size_of::<u64>() + size_of::<&[u8]>())
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let keys = case.obligations.encoded_keys_with_budget(
            limits.key,
            limits.aggregate_key_bytes,
            budget,
        )?;
        let mut source_keys = filled_vec(n, &[][..])?;
        let mut capacity = filled_vec(n, 0_u64)?;
        let mut key_refs = filled_vec(m, &[][..])?;
        for (i, source) in family.sources().iter().enumerate() {
            source_keys[i] = source.custody;
            capacity[i] = source.capacity.min(source.debit_limit);
        }
        for (reference, key) in key_refs.iter_mut().zip(&keys) {
            *reference = key;
        }
        let canonical = canonicalize_funding_problem(
            FundingMinimaxProblem {
                capacities: &capacity,
                obligations: case.obligations.amounts(),
                eligible: case.eligible,
            },
            &source_keys,
            &key_refs,
            limits.funding,
            budget,
        )?;
        let total_keys = keys
            .iter()
            .try_fold(0_usize, |sum, key| sum.checked_add(key.len()))
            .ok_or(FundingSearchError::Overflow)?;
        let bytes = n
            .checked_mul(
                size_of::<CapturedPhloSource>() + size_of::<Vec<bool>>() + size_of::<Vec<u64>>(),
            )
            .and_then(|x| {
                m.checked_mul(size_of::<Vec<u8>>() + size_of::<u64>())
                    .and_then(|y| x.checked_add(y))
            })
            .and_then(|x| {
                cells
                    .checked_mul(size_of::<u64>() + size_of::<bool>())
                    .and_then(|y| x.checked_add(y))
            })
            .and_then(|x| x.checked_add(total_keys))
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        reserve_work(
            budget,
            HostWorkDimension::SearchCandidates,
            preparation
                .checked_add(total_keys)
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(n)
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        let mut obligation_keys = filled_vec(m, Vec::new())?;
        let mut amounts = filled_vec(m, 0_u64)?;
        let mut eligible = filled_vec(n, Vec::new())?;
        let mut assignment = filled_vec(n, Vec::new())?;
        let selected = family.selected_assignment(branch)?;
        for (i, original) in canonical
            .original_source_positions()
            .iter()
            .copied()
            .enumerate()
        {
            let hold = family.source_holds()[original];
            let debit = selected.source_debits()[original];
            let fee = case.assignment[original][0];
            if fee > debit {
                return Err(FundingSearchError::InvalidResult.into());
            }
            let refund = hold
                .checked_sub(debit)
                .ok_or(FundingSearchError::InvalidResult)?;
            sources.push(CapturedPhloSource {
                source: family.sources()[original],
                hold,
                debit,
                fee,
                refund,
            });
            eligible[i] = filled_vec(m, false)?;
            assignment[i] = filled_vec(m, 0_u64)?;
            for (j, column) in canonical
                .original_obligation_positions()
                .iter()
                .copied()
                .enumerate()
            {
                eligible[i][j] = case.eligible[original][column];
                assignment[i][j] = case.assignment[original][column];
            }
        }
        for (j, original) in canonical
            .original_obligation_positions()
            .iter()
            .copied()
            .enumerate()
        {
            obligation_keys[j] = filled_vec(keys[original].len(), 0_u8)?;
            obligation_keys[j].copy_from_slice(&keys[original]);
            amounts[j] = case.obligations.amounts()[original];
        }
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            preparation,
        )?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            n.checked_add(m)
                .and_then(|x| x.checked_mul(size_of::<u64>()))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let checked = check_funding_assignment(
            canonical.problem().capacities,
            &amounts,
            &eligible,
            &assignment,
            limits.funding.source_cap,
            limits.funding.obligation_cap,
        )
        .map_err(FundingSearchError::from)?;
        if sources
            .iter()
            .zip(checked.source_debits())
            .any(|(source, debit)| source.debit != *debit)
        {
            return Err(FundingSearchError::InvalidResult.into());
        }
        Ok(CanonicalPhloFundingCapture {
            intent: self,
            branch,
            sources,
            obligation_keys,
            amounts,
            eligible,
            assignment,
        })
    }
}
