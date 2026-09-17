use std::cmp::Ordering;
use std::num::NonZeroUsize;

use models::rust::host_work::HostWorkDimension;
use models::rust::signed_phlo_deploy::FundedDeploy;
use thiserror::Error;

use super::{
    CanonicalPhloFundingCapture, CheckedPhloFundingFamily, CheckedSignedPhloFundingIntent,
    PhloCaptureError, PhloCaptureLimits, PhloFamilyFundingError, PhloFamilyFundingLimits,
    PhloFamilyFundingSelection, PhloFundingError,
};
use crate::rust::interpreter::accounting::monetary_allocation::{
    reserve_work, FundingReservationError, FundingSearchError, MonetaryCursor, MonetaryCursorError,
    MonetaryCursorTransition,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloScopedCursorSnapshot {
    pub scope: [u8; 32],
    pub cursor: MonetaryCursor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloFamilyCursorSnapshot<'a> {
    pub canonical_custodies: &'a [&'a [u8]],
    pub resource: PhloScopedCursorSnapshot,
    pub fee: PhloScopedCursorSnapshot,
}

#[derive(Clone, Debug)]
pub struct CheckedSignedPhloFamilyPolicy<'a, A = FundedDeploy> {
    signed_intent: CheckedSignedPhloFundingIntent<'a, A>,
    snapshots: PhloFamilyCursorSnapshot<'a>,
    selection: PhloFamilyFundingSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopedPhloFundingCapture<'a, A = FundedDeploy> {
    signed_intent: CheckedSignedPhloFundingIntent<'a, A>,
    capture: CanonicalPhloFundingCapture<'a>,
    resource_transition: Option<MonetaryCursorTransition>,
    fee_transition: Option<MonetaryCursorTransition>,
}

impl<'a, A> ScopedPhloFundingCapture<'a, A> {
    pub fn signed_intent(&self) -> CheckedSignedPhloFundingIntent<'a, A> { self.signed_intent }
    pub fn capture(&self) -> &CanonicalPhloFundingCapture<'a> { &self.capture }
    pub fn resource_transition(&self) -> Option<&MonetaryCursorTransition> {
        self.resource_transition.as_ref()
    }
    pub fn fee_transition(&self) -> Option<&MonetaryCursorTransition> {
        self.fee_transition.as_ref()
    }
}

#[derive(Debug, Error)]
pub enum PhloPolicyCaptureError {
    #[error("cursor snapshot custody identities differ from the canonical funding cohort")]
    CursorCohortMismatch,
    #[error(transparent)]
    Funding(#[from] PhloFamilyFundingError),
    #[error(transparent)]
    Capture(#[from] PhloCaptureError),
    #[error(transparent)]
    Cursor(#[from] MonetaryCursorError),
    #[error(transparent)]
    Search(#[from] FundingSearchError),
}

fn compare_custodies(
    left: &[u8],
    right: &[u8],
    budget: &HostWorkBudget,
) -> Result<Ordering, FundingSearchError> {
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        left.len()
            .min(right.len())
            .checked_add(1)
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    Ok(left.cmp(right))
}

fn check_snapshot_cohort(
    family: &CheckedPhloFundingFamily<'_>,
    snapshots: PhloFamilyCursorSnapshot<'_>,
    limits: PhloFamilyFundingLimits,
    budget: &HostWorkBudget,
) -> Result<NonZeroUsize, PhloPolicyCaptureError> {
    let n = family.sources().len();
    let count =
        NonZeroUsize::new(n).ok_or(PhloFamilyFundingError::from(PhloFundingError::EmptySources))?;
    if n > limits.funding.sources.get() {
        return Err(PhloFamilyFundingError::from(PhloFundingError::TooManySources).into());
    }
    if family.cases().len() > limits.funding.cases.get() {
        return Err(PhloFamilyFundingError::from(PhloFundingError::TooManyCases).into());
    }
    if snapshots.canonical_custodies.len() != n {
        return Err(PhloPolicyCaptureError::CursorCohortMismatch);
    }
    snapshots.resource.cursor.validate(count)?;
    snapshots.fee.cursor.validate(count)?;
    reserve_work(budget, HostWorkDimension::VerificationOperations, n)?;
    let mut remaining = limits.funding.custody_bytes;
    for custody in snapshots.canonical_custodies {
        if custody.is_empty() {
            return Err(PhloPolicyCaptureError::CursorCohortMismatch);
        }
        remaining = remaining
            .checked_sub(custody.len())
            .ok_or(PhloFamilyFundingError::from(
                PhloFundingError::TooManyCustodyBytes,
            ))?;
    }
    for adjacent in snapshots.canonical_custodies.windows(2) {
        if compare_custodies(adjacent[0], adjacent[1], budget)? != Ordering::Less {
            return Err(PhloPolicyCaptureError::CursorCohortMismatch);
        }
    }
    for source in family.sources() {
        let (mut low, mut high) = (0, n);
        let mut found = false;
        while low < high {
            let middle = low + (high - low) / 2;
            match compare_custodies(
                snapshots.canonical_custodies[middle],
                source.custody,
                budget,
            )? {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => {
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return Err(PhloPolicyCaptureError::CursorCohortMismatch);
        }
    }
    Ok(count)
}

impl<'a, A> CheckedSignedPhloFundingIntent<'a, A> {
    pub fn plan_funding_policy(
        self,
        snapshots: PhloFamilyCursorSnapshot<'a>,
        limits: PhloFamilyFundingLimits,
        budget: &HostWorkBudget,
    ) -> Result<Option<CheckedSignedPhloFamilyPolicy<'a, A>>, PhloPolicyCaptureError> {
        let family = self.intent().bound().consent().family();
        let count = check_snapshot_cohort(family, snapshots, limits, budget)?;
        let resource = snapshots.resource.cursor.position_index(count)?;
        let fee = snapshots.fee.cursor.position_index(count)?;
        let Some(selection) = self
            .intent()
            .verified_funding_selection(resource, fee, limits, budget)?
        else {
            return Ok(None);
        };
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            selection
                .cursor_transitions()
                .len()
                .checked_mul(2)
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        for transition in selection.cursor_transitions() {
            let _ = bind_transition(snapshots.resource, transition.resource_next_cursor(), count)?;
            let _ = bind_transition(snapshots.fee, transition.fee_next_cursor(), count)?;
        }
        Ok(Some(CheckedSignedPhloFamilyPolicy {
            signed_intent: self,
            snapshots,
            selection,
        }))
    }
}

fn bind_transition(
    snapshot: PhloScopedCursorSnapshot,
    next: Option<usize>,
    count: NonZeroUsize,
) -> Result<Option<MonetaryCursorTransition>, MonetaryCursorError> {
    next.map(|position| {
        let position = i64::try_from(position).map_err(|_| MonetaryCursorError::InvalidPosition)?;
        MonetaryCursorTransition::new(snapshot.scope, snapshot.cursor, position, count)
    })
    .transpose()
}

impl<'a, A> CheckedSignedPhloFamilyPolicy<'a, A> {
    pub fn signed_intent(&self) -> CheckedSignedPhloFundingIntent<'a, A> { self.signed_intent }
    pub fn snapshots(&self) -> PhloFamilyCursorSnapshot<'a> { self.snapshots }
    pub fn selection(&self) -> &PhloFamilyFundingSelection { &self.selection }

    pub fn capture_case(
        &self,
        branch: usize,
        limits: PhloCaptureLimits,
        budget: &HostWorkBudget,
    ) -> Result<ScopedPhloFundingCapture<'a, A>, PhloPolicyCaptureError> {
        reserve_work(budget, HostWorkDimension::VerificationOperations, 4)?;
        let transition =
            self.selection
                .cursor_transitions()
                .get(branch)
                .ok_or(PhloCaptureError::Branch(
                    FundingReservationError::UnknownBranch,
                ))?;
        let count = NonZeroUsize::new(self.snapshots.canonical_custodies.len())
            .ok_or(FundingSearchError::InvalidResult)?;
        let resource_transition = bind_transition(
            self.snapshots.resource,
            transition.resource_next_cursor(),
            count,
        )?;
        let fee_transition =
            bind_transition(self.snapshots.fee, transition.fee_next_cursor(), count)?;
        let capture = self
            .signed_intent
            .intent()
            .capture_case(branch, limits, budget)?;
        Ok(ScopedPhloFundingCapture {
            signed_intent: self.signed_intent,
            capture,
            resource_transition,
            fee_transition,
        })
    }
}
