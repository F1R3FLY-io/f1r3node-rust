use models::rust::signed_phlo_deploy::FundedDeploy;
use thiserror::Error;

use super::{
    CheckedSignedPhloFamilyPolicy, NativePhloAmountError, NativePhloSourceAmounts,
    PhloCaptureLimits, PhloPolicyCaptureError, ScopedPhloFundingCapture,
};
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchError;

#[derive(Clone, Debug)]
pub struct CheckedNativeSignedPhloFamilyPolicy<'a, A = FundedDeploy> {
    policy: CheckedSignedPhloFamilyPolicy<'a, A>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeScopedPhloFundingCapture<'a, A = FundedDeploy> {
    scoped: ScopedPhloFundingCapture<'a, A>,
    amounts: Vec<NativePhloSourceAmounts<'a>>,
}

#[derive(Debug, Error)]
pub enum NativePhloPolicyError {
    #[error(transparent)]
    Amount(#[from] NativePhloAmountError),
    #[error(transparent)]
    Policy(#[from] PhloPolicyCaptureError),
    #[error(transparent)]
    Search(#[from] FundingSearchError),
}

fn check_native_holds(holds: &[u64]) -> Result<(), NativePhloPolicyError> {
    if holds.iter().any(|hold| *hold > i64::MAX as u64) {
        return Err(NativePhloAmountError::OutOfRange.into());
    }
    Ok(())
}

impl<'a, A> CheckedSignedPhloFamilyPolicy<'a, A> {
    pub fn into_native(
        self,
    ) -> Result<CheckedNativeSignedPhloFamilyPolicy<'a, A>, NativePhloPolicyError> {
        check_native_holds(
            self.signed_intent()
                .intent()
                .bound()
                .consent()
                .family()
                .source_holds(),
        )?;
        Ok(CheckedNativeSignedPhloFamilyPolicy { policy: self })
    }
}

impl<'a, A> CheckedNativeSignedPhloFamilyPolicy<'a, A> {
    pub fn policy(&self) -> &CheckedSignedPhloFamilyPolicy<'a, A> { &self.policy }

    pub fn capture_case(
        &self,
        branch: usize,
        limits: PhloCaptureLimits,
    ) -> Result<NativeScopedPhloFundingCapture<'a, A>, NativePhloPolicyError> {
        let scoped = self.policy.capture_case(branch, limits)?;
        let sources = scoped.capture().sources();
        let mut amounts = Vec::new();
        amounts
            .try_reserve_exact(sources.len())
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        for source in sources {
            amounts.push(NativePhloSourceAmounts::checked(
                source.source().custody,
                source.hold(),
                source.debit(),
                source.fee(),
            )?);
        }
        Ok(NativeScopedPhloFundingCapture { scoped, amounts })
    }
}

impl<'a, A> NativeScopedPhloFundingCapture<'a, A> {
    pub fn scoped(&self) -> &ScopedPhloFundingCapture<'a, A> { &self.scoped }
    pub fn amounts(&self) -> &[NativePhloSourceAmounts<'a>] { &self.amounts }
}

#[cfg(test)]
mod tests;
