use models::rust::phlo_obligation::PhloObligationKeyLimits;
use thiserror::Error;

use super::{CheckedPhloFundingIntent, PhloFundingSource, PhloObligationError, PhloObligationKey};
use crate::rust::interpreter::accounting::monetary_allocation::{
    canonicalize_funding_problem, check_funding_assignment, filled_vec, FundingAssignmentError,
    FundingIdentityError, FundingMinimaxProblem, FundingReservationError, FundingSearchError,
    FundingSearchLimits,
};

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
    original_source_positions: Vec<usize>,
    sources: Vec<CapturedPhloSource<'a>>,
    obligation_keys: Vec<Vec<u8>>,
    original_obligation_positions: Vec<usize>,
    amounts: Vec<u64>,
    eligible: Vec<Vec<bool>>,
    assignment: Vec<Vec<u64>>,
}

impl<'a> CanonicalPhloFundingCapture<'a> {
    pub fn intent(&self) -> CheckedPhloFundingIntent<'a> { self.intent }
    pub fn branch(&self) -> usize { self.branch }
    pub fn original_source_positions(&self) -> &[usize] { &self.original_source_positions }
    pub fn sources(&self) -> &[CapturedPhloSource<'a>] { &self.sources }
    pub fn obligation_keys(&self) -> &[Vec<u8>] { &self.obligation_keys }
    pub fn amounts(&self) -> &[u64] { &self.amounts }
    pub fn eligible(&self) -> &[Vec<bool>] { &self.eligible }
    pub fn assignment(&self) -> &[Vec<u64>] { &self.assignment }

    pub fn obligations(&self) -> impl ExactSizeIterator<Item = CapturedPhloObligation<'_, 'a>> {
        (0..self.original_obligation_positions.len()).map(|position| CapturedPhloObligation {
            capture: self,
            position,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CapturedPhloObligation<'c, 'a> {
    capture: &'c CanonicalPhloFundingCapture<'a>,
    position: usize,
}

impl<'c, 'a> CapturedPhloObligation<'c, 'a> {
    pub fn key(self) -> PhloObligationKey<'a> {
        let family = self.capture.intent.bound().consent().family();
        family.cases()[self.capture.branch].obligations.keys()
            [self.capture.original_obligation_positions[self.position]]
    }

    pub fn quantity(self) -> u64 {
        let family = self.capture.intent.bound().consent().family();
        family.cases()[self.capture.branch].obligations.quantities()
            [self.capture.original_obligation_positions[self.position]]
    }

    pub fn encoded_key(self) -> &'c [u8] { &self.capture.obligation_keys[self.position] }

    pub fn amount(self) -> u64 { self.capture.amounts[self.position] }

    pub fn contributions(
        self,
    ) -> impl ExactSizeIterator<Item = (CapturedPhloSource<'a>, u64)> + 'c {
        self.capture
            .sources
            .iter()
            .copied()
            .zip(&self.capture.assignment)
            .map(move |(source, row)| (source, row[self.position]))
    }
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
        let keys = case
            .obligations
            .encoded_keys(limits.key, limits.aggregate_key_bytes)?;
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
        )?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(n)
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        let mut obligation_keys = filled_vec(m, Vec::new())?;
        let mut amounts = filled_vec(m, 0_u64)?;
        let mut original_obligation_positions = filled_vec(m, 0_usize)?;
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
            original_obligation_positions[j] = original;
        }
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
        let mut original_source_positions = Vec::new();
        original_source_positions
            .try_reserve_exact(n)
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        original_source_positions.extend_from_slice(canonical.original_source_positions());
        Ok(CanonicalPhloFundingCapture {
            intent: self,
            branch,
            original_source_positions,
            sources,
            obligation_keys,
            original_obligation_positions,
            amounts,
            eligible,
            assignment,
        })
    }
}
