use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

use thiserror::Error;

use super::{
    resource_key, CheckedPhloObligations, PhloExecutionError, PhloObligationKey, WorkBudget,
};
use crate::rust::interpreter::accounting::monetary_allocation::{
    FundingAssignmentError, FundingAssignmentTotals, FundingBranchReservation,
    FundingReservationError,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloFundingSource<'a> {
    pub custody: &'a [u8],
    pub capacity: u64,
    pub exposure_limit: u64,
    pub debit_limit: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloFundingCase<'a> {
    pub obligations: &'a CheckedPhloObligations<'a>,
    pub eligible: &'a [Vec<bool>],
    pub assignment: &'a [Vec<u64>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloFundingLimits {
    pub sources: NonZeroUsize,
    pub cases: NonZeroUsize,
    pub obligations: NonZeroUsize,
    pub assignment_cells: usize,
    pub custody_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedPhloFundingFamily<'a> {
    sources: &'a [PhloFundingSource<'a>],
    cases: &'a [PhloFundingCase<'a>],
    reservation: FundingBranchReservation,
    total_exposure_limit: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloRefund<'a> {
    pub custody: &'a [u8],
    pub amount: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePhloSourceAmounts<'a> {
    custody: &'a [u8],
    hold: i64,
    acquisition: i64,
    fee: i64,
    refund: i64,
}

impl<'a> NativePhloSourceAmounts<'a> {
    pub fn custody(self) -> &'a [u8] { self.custody }

    pub fn hold(self) -> i64 { self.hold }

    pub fn acquisition(self) -> i64 { self.acquisition }

    pub fn fee(self) -> i64 { self.fee }

    pub fn refund(self) -> i64 { self.refund }

    pub(super) fn checked(
        custody: &'a [u8],
        hold: u64,
        debit: u64,
        fee: u64,
    ) -> Result<Self, NativePhloAmountError> {
        let hold = i64::try_from(hold).map_err(|_| NativePhloAmountError::OutOfRange)?;
        let debit = i64::try_from(debit).map_err(|_| NativePhloAmountError::OutOfRange)?;
        let fee = i64::try_from(fee).map_err(|_| NativePhloAmountError::OutOfRange)?;
        if debit > hold || fee > debit {
            return Err(NativePhloAmountError::InconsistentAmounts);
        }
        Ok(Self {
            custody,
            hold,
            acquisition: debit - fee,
            fee,
            refund: hold - debit,
        })
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum NativePhloAmountError {
    #[error("phlo source amount exceeds the native signed integer range")]
    OutOfRange,
    #[error("phlo fee, debit, and hold do not form a valid source settlement")]
    InconsistentAmounts,
    #[error(transparent)]
    Selection(#[from] FundingReservationError),
}

impl<'a> CheckedPhloFundingFamily<'a> {
    pub fn sources(&self) -> &'a [PhloFundingSource<'a>] { self.sources }

    pub fn cases(&self) -> &'a [PhloFundingCase<'a>] { self.cases }

    pub fn source_holds(&self) -> &[u64] { self.reservation.source_holds() }

    pub fn total_held(&self) -> u128 { self.reservation.total_held() }

    pub fn total_exposure_limit(&self) -> u128 { self.total_exposure_limit }

    pub fn maximum_charge(&self) -> u64 { self.reservation.maximum_charge() }

    pub fn native_amounts(
        &self,
        selected: usize,
    ) -> Result<Vec<NativePhloSourceAmounts<'a>>, NativePhloAmountError> {
        let assignment = self.selected_assignment(selected)?;
        self.sources
            .iter()
            .zip(self.source_holds())
            .zip(assignment.source_debits())
            .zip(self.cases[selected].assignment)
            .map(|(((source, hold), debit), row)| {
                NativePhloSourceAmounts::checked(source.custody, *hold, *debit, row[0])
            })
            .collect()
    }

    pub fn selected_assignment(
        &self,
        selected: usize,
    ) -> Result<&FundingAssignmentTotals, FundingReservationError> {
        self.reservation.branch(selected)
    }

    pub fn refunds(&self, selected: usize) -> Result<Vec<PhloRefund<'a>>, FundingReservationError> {
        Ok(self
            .reservation
            .refunds(selected)?
            .into_iter()
            .zip(self.sources)
            .map(|(amount, source)| PhloRefund {
                custody: source.custody,
                amount,
            })
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloFundingError {
    #[error("phlo funding requires physical sources")]
    EmptySources,
    #[error("phlo funding requires execution cases")]
    EmptyCases,
    #[error("phlo funding exceeds the source limit")]
    TooManySources,
    #[error("phlo funding exceeds the case limit")]
    TooManyCases,
    #[error("phlo funding exceeds the obligation limit")]
    TooManyObligations,
    #[error("phlo funding exceeds the assignment-cell limit")]
    TooManyAssignmentCells,
    #[error("phlo funding exceeds the custody-byte limit")]
    TooManyCustodyBytes,
    #[error("phlo funding repeats a physical custody identity")]
    DuplicateCustody,
    #[error("phlo funding cases have different checked controls")]
    DifferentControls,
    #[error("phlo funding gives different permissions to the same resource key")]
    InconsistentPermission,
    #[error("phlo funding exceeds signed total exposure")]
    TotalExposureExceeded,
    #[error(transparent)]
    Execution(#[from] PhloExecutionError),
    #[error(transparent)]
    Assignment(#[from] FundingAssignmentError),
    #[error(transparent)]
    Reservation(#[from] FundingReservationError),
}

pub fn check_phlo_funding_family<'a>(
    sources: &'a [PhloFundingSource<'a>],
    cases: &'a [PhloFundingCase<'a>],
    total_exposure_limit: u128,
    limits: PhloFundingLimits,
) -> Result<CheckedPhloFundingFamily<'a>, PhloFundingError> {
    if sources.is_empty() {
        return Err(PhloFundingError::EmptySources);
    }
    if sources.len() > limits.sources.get() {
        return Err(PhloFundingError::TooManySources);
    }
    let Some(first) = cases.first() else {
        return Err(PhloFundingError::EmptyCases);
    };
    if cases.len() > limits.cases.get() {
        return Err(PhloFundingError::TooManyCases);
    }
    let mut bytes = limits.custody_bytes;
    for source in sources {
        bytes = bytes
            .checked_sub(source.custody.len())
            .ok_or(PhloFundingError::TooManyCustodyBytes)?;
    }
    let mut cells = limits.assignment_cells;
    for case in cases {
        let count = case.obligations.amounts().len();
        if count > limits.obligations.get() {
            return Err(PhloFundingError::TooManyObligations);
        }
        cells = sources
            .len()
            .checked_mul(count)
            .and_then(|count| cells.checked_sub(count))
            .ok_or(PhloFundingError::TooManyAssignmentCells)?;
        if case.eligible.len() != sources.len()
            || case.assignment.len() != sources.len()
            || case.eligible.iter().any(|row| row.len() != count)
            || case.assignment.iter().any(|row| row.len() != count)
        {
            return Err(FundingAssignmentError::InvalidDimensions.into());
        }
    }
    let mut identities = BTreeSet::new();
    for source in sources {
        if !identities.insert(source.custody) {
            return Err(PhloFundingError::DuplicateCustody);
        }
    }
    let capacities: Vec<_> = sources.iter().map(|source| source.capacity).collect();
    let debit_caps: Vec<_> = sources
        .iter()
        .map(|source| source.capacity.min(source.debit_limit))
        .collect();
    let exposures: Vec<_> = sources.iter().map(|source| source.exposure_limit).collect();
    let controls = first.obligations.execution().controls();
    let mut plans = Vec::with_capacity(cases.len());
    for case in cases {
        if case.obligations.execution().controls() != controls {
            return Err(PhloFundingError::DifferentControls);
        }
        let execution = case.obligations.execution();
        let mut budget = WorkBudget {
            remaining_nodes: execution.limits.authority_nodes,
            remaining_bytes: execution.limits.key_bytes,
        };
        let mut occurrences = BTreeMap::new();
        for (slot, key) in case.obligations.keys().iter().enumerate() {
            let identity = match key {
                PhloObligationKey::Fee => None,
                PhloObligationKey::Resource(resource) => {
                    Some((false, resource_key(*resource, &mut budget)?))
                }
                PhloObligationKey::RetainedResource(resource) => {
                    Some((true, resource_key(*resource, &mut budget)?))
                }
            };
            if let Some(previous) = occurrences.insert(identity, slot) {
                if case.eligible.iter().any(|row| row[previous] != row[slot]) {
                    return Err(PhloFundingError::InconsistentPermission);
                }
            }
        }
        plans.push(case.obligations.check_assignment(
            &debit_caps,
            case.eligible,
            case.assignment,
            limits.sources,
            limits.obligations,
        )?);
    }
    let reservation = FundingBranchReservation::from_plans(
        &capacities,
        &exposures,
        plans,
        limits.sources,
        limits.cases,
    )?;
    if reservation.total_held() > total_exposure_limit {
        return Err(PhloFundingError::TotalExposureExceeded);
    }
    Ok(CheckedPhloFundingFamily {
        sources,
        cases,
        reservation,
        total_exposure_limit,
    })
}

#[cfg(test)]
mod tests;
