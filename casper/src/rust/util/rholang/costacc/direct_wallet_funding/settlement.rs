use std::mem::size_of;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::host_work::{HostWorkDimension, HostWorkReservationError, HostWorkUnits};
use rholang::rust::interpreter::accounting::monetary_allocation::{
    MonetaryCursor, MonetaryCursorError, MonetaryCursorTransition,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    CheckedPhloExecution, NativePhloSourceAmounts, NativeScopedPhloFundingCapture,
    PhloCaptureLimits, PhloOutcome, PhloOutcomeMatchError, PhloOutcomeMatchLimits,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::rho_type::Extractor;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;

use super::*;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::vault_cost_deploy::{
    ApplyCostDeploy, ApplyPhloCostDeploy, VaultAllocation, VaultSettlement,
};
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;

#[derive(Debug)]
pub struct CheckedDirectWalletSettlement<'a, A = FundedDeploy> {
    snapshot: &'a DirectWalletPolicySnapshot<'a, A>,
    capture: NativeScopedPhloFundingCapture<'a, A>,
}

pub struct PreparedDirectWalletSettlement<'s, 'a, A = FundedDeploy> {
    checked: &'s CheckedDirectWalletSettlement<'a, A>,
    request: Option<ApplyPhloCostDeploy>,
}

pub struct CheckedDirectWalletRequest<'s, 'a, A = FundedDeploy> {
    checked: &'s CheckedDirectWalletSettlement<'a, A>,
    request: &'s mut ApplyPhloCostDeploy,
}

#[derive(Debug, Error)]
pub enum DirectWalletSettlementError {
    #[error("settlement does not preserve the authenticated envelope")]
    EnvelopeMismatch,
    #[error("settlement does not preserve the complete canonical wallet cohort")]
    CustodyMismatch,
    #[error("settlement amounts do not conserve the source hold")]
    InconsistentAmounts,
    #[error("settlement cursor presence does not match positive contributions")]
    CursorPresence,
    #[error("settlement work calculation overflowed")]
    Overflow,
    #[error("settlement allocation failed")]
    AllocationFailed,
    #[error(transparent)]
    Outcome(#[from] PhloOutcomeMatchError),
    #[error(transparent)]
    Cursor(#[from] MonetaryCursorError),
    #[error(transparent)]
    HostWork(#[from] HostWorkReservationError),
    #[error(transparent)]
    Native(#[from] CasperError),
}

#[derive(Clone, Copy)]
struct SettlementRow<'a> {
    custody: &'a [u8],
    hold: i64,
    acquisition: i64,
    fee: i64,
    refund: i64,
}

impl<'a> From<NativePhloSourceAmounts<'a>> for SettlementRow<'a> {
    fn from(amounts: NativePhloSourceAmounts<'a>) -> Self {
        Self {
            custody: amounts.custody(),
            hold: amounts.hold(),
            acquisition: amounts.acquisition(),
            fee: amounts.fee(),
            refund: amounts.refund(),
        }
    }
}

struct ProjectedSettlement {
    allocations: Vec<VaultAllocation>,
    settlements: Vec<VaultSettlement>,
    payer_count: NonZeroUsize,
    has_resource: bool,
    has_fee: bool,
}

fn reserve_projection(
    count: usize,
    budget: &HostWorkBudget,
) -> Result<(), DirectWalletSettlementError> {
    for (dimension, per_source, fixed) in [
        (HostWorkDimension::SearchCandidates, 1, 1),
        (
            HostWorkDimension::SearchStateBytes,
            size_of::<VaultAllocation>() + size_of::<VaultSettlement>() + 1024,
            1024,
        ),
        (
            HostWorkDimension::VerificationOperations,
            512usize
                .checked_add(64 * usize::BITS as usize)
                .ok_or(DirectWalletSettlementError::Overflow)?,
            512,
        ),
    ] {
        let work = count
            .checked_mul(per_source)
            .and_then(|n| n.checked_add(fixed))
            .and_then(|n| u64::try_from(n).ok())
            .ok_or(DirectWalletSettlementError::Overflow)?;
        budget.reserve(dimension, HostWorkUnits::new(work))?;
    }
    Ok(())
}

fn project_amounts<'a>(
    payers: &BTreeMap<[u8; 32], VaultPayer>,
    canonical_custodies: &[&[u8]],
    rows: impl ExactSizeIterator<Item = SettlementRow<'a>>,
    budget: &HostWorkBudget,
) -> Result<ProjectedSettlement, DirectWalletSettlementError> {
    let count = payers.len();
    let payer_count =
        NonZeroUsize::new(count).ok_or(DirectWalletSettlementError::CustodyMismatch)?;
    if canonical_custodies.len() != count || rows.len() != count {
        return Err(DirectWalletSettlementError::CustodyMismatch);
    }
    reserve_projection(count, budget)?;
    let mut result = ProjectedSettlement {
        allocations: Vec::new(),
        settlements: Vec::new(),
        payer_count,
        has_resource: false,
        has_fee: false,
    };
    result
        .allocations
        .try_reserve_exact(count)
        .map_err(|_| DirectWalletSettlementError::AllocationFailed)?;
    result
        .settlements
        .try_reserve_exact(count)
        .map_err(|_| DirectWalletSettlementError::AllocationFailed)?;
    for (((key, payer), custody), row) in payers.iter().zip(canonical_custodies).zip(rows) {
        if key.as_slice() != *custody || row.custody != *custody || payer.custody_key != *key {
            return Err(DirectWalletSettlementError::CustodyMismatch);
        }
        if row.hold < 0
            || row.acquisition < 0
            || row.fee < 0
            || row.refund < 0
            || row
                .acquisition
                .checked_add(row.fee)
                .and_then(|n| n.checked_add(row.refund))
                != Some(row.hold)
        {
            return Err(DirectWalletSettlementError::InconsistentAmounts);
        }
        if row.hold == 0 {
            continue;
        }
        let address = payer.address.to_base58();
        result
            .allocations
            .push(VaultAllocation::new(address.clone(), row.hold)?);
        result
            .settlements
            .push(VaultSettlement::new(address, row.acquisition, row.fee)?);
        result.has_resource |= row.acquisition > 0;
        result.has_fee |= row.fee > 0;
    }
    Ok(result)
}

fn check_transition(
    transition: Option<&MonetaryCursorTransition>,
    scope: &[u8; 32],
    observed: Option<MonetaryCursor>,
    positive: bool,
    count: NonZeroUsize,
) -> Result<(), DirectWalletSettlementError> {
    if transition.is_some() != positive {
        return Err(DirectWalletSettlementError::CursorPresence);
    }
    if let Some(transition) = transition {
        transition.checked_successor(scope, observed.unwrap_or(MonetaryCursor::INITIAL), count)?;
    }
    Ok(())
}

impl<'a, A> CheckedDirectWalletPolicy<'a, A> {
    pub fn capture_settlement(
        &self,
        observed: CheckedPhloExecution<'_>,
        outcome: PhloOutcome<'_>,
        limits: PhloOutcomeMatchLimits,
        capture_limits: PhloCaptureLimits,
        budget: &HostWorkBudget,
    ) -> Result<CheckedDirectWalletSettlement<'a, A>, DirectWalletSettlementError> {
        let capture = self.policy().capture_matching_execution(
            observed,
            outcome,
            limits,
            capture_limits,
            budget,
        )?;
        if !std::ptr::eq(
            capture.scoped().signed_intent().envelope(),
            self.snapshot().wallets().authorization().envelope(),
        ) {
            return Err(DirectWalletSettlementError::EnvelopeMismatch);
        }
        Ok(CheckedDirectWalletSettlement {
            snapshot: self.snapshot(),
            capture,
        })
    }
}

impl<'a, A> CheckedDirectWalletSettlement<'a, A> {
    pub fn snapshot(&self) -> &'a DirectWalletPolicySnapshot<'a, A> { self.snapshot }
    pub fn capture(&self) -> &NativeScopedPhloFundingCapture<'a, A> { &self.capture }

    pub fn prepare_request<'s>(
        &'s self,
        reservation_id: [u8; 32],
        fee_address: &VaultAddress,
        initial_rand: Blake2b512Random,
        budget: &HostWorkBudget,
    ) -> Result<PreparedDirectWalletSettlement<'s, 'a, A>, DirectWalletSettlementError> {
        let amounts = project_amounts(
            self.snapshot.wallets().authorization().payers(),
            self.snapshot.canonical_custodies(),
            self.capture
                .amounts()
                .iter()
                .copied()
                .map(SettlementRow::from),
            budget,
        )?;
        let scoped = self.capture.scoped();
        check_transition(
            scoped.resource_transition(),
            &self.snapshot.resource_scope(),
            self.snapshot.resource_cursor(),
            amounts.has_resource,
            amounts.payer_count,
        )?;
        check_transition(
            scoped.fee_transition(),
            &self.snapshot.fee_scope(),
            self.snapshot.fee_cursor(),
            amounts.has_fee,
            amounts.payer_count,
        )?;
        let request = if amounts.allocations.is_empty() {
            None
        } else {
            Some(ApplyPhloCostDeploy::new(
                ApplyCostDeploy::new(
                    reservation_id,
                    amounts.allocations,
                    amounts.settlements,
                    fee_address.to_base58(),
                    initial_rand,
                )?,
                scoped.resource_transition().cloned(),
                scoped.fee_transition().cloned(),
                amounts.payer_count,
            )?)
        };
        Ok(PreparedDirectWalletSettlement {
            checked: self,
            request,
        })
    }
}

impl<'s, 'a, A> PreparedDirectWalletSettlement<'s, 'a, A> {
    pub fn checked(&self) -> &'s CheckedDirectWalletSettlement<'a, A> { self.checked }
    pub fn request(&mut self) -> Option<CheckedDirectWalletRequest<'_, 'a, A>> {
        self.request
            .as_mut()
            .map(|request| CheckedDirectWalletRequest {
                checked: self.checked,
                request,
            })
    }
}

impl<'s, 'a, A> CheckedDirectWalletRequest<'s, 'a, A> {
    pub fn checked(&self) -> &'s CheckedDirectWalletSettlement<'a, A> { self.checked }
}

impl<A: Send + Sync> SystemDeployTrait for CheckedDirectWalletRequest<'_, '_, A> {
    type Output = <ApplyPhloCostDeploy as SystemDeployTrait>::Output;
    type Result = ();

    fn source() -> &'static str { ApplyPhloCostDeploy::source() }
    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, ()> {
        ApplyPhloCostDeploy::process_result(value)
    }
    fn as_any(&self) -> &dyn std::any::Any { self.request.as_any() }
    fn rand(&self) -> Blake2b512Random { self.request.rand() }
    fn env(&mut self) -> std::collections::HashMap<String, Par> { self.request.env() }
    fn return_channel(&mut self) -> Result<Par, CasperError> { self.request.return_channel() }
}

#[cfg(test)]
mod tests;
