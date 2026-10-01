use std::mem::size_of;
use std::sync::Arc;

use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::host_work::HostWorkDimension;
use thiserror::Error;

use super::{
    NativePhloMeasurementError, NativePhloRegionError, NativePhloRegionLimits, NativePhloRuleError,
    NativePhloRules,
};
use crate::rust::interpreter::accounting::authority::AuthorityError;
use crate::rust::interpreter::accounting::byte_receipts::{
    ByteObservation, ByteObservationSnapshot,
};
use crate::rust::interpreter::accounting::monetary_allocation::{reserve_work, FundingSearchError};
use crate::rust::interpreter::accounting::phlo_controls::{
    CheckedPhloControls, PhloScheduleBinding,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

mod budget_trace;
pub(in crate::rust::interpreter::accounting) use budget_trace::{
    observation_comparison_bytes, CheckedNativeBudgetEvidence,
};
pub use budget_trace::{
    CheckedNativeBudgetTrace, NativeAttemptStage, NativeBudgetAttempt, NativeBudgetOccurrence,
    NativeBudgetReplayDecision, NativeBudgetTraceError, NativeBudgetTraceLimits,
};

#[derive(Debug, Error)]
pub enum NativePhloExecutionError {
    #[error(transparent)]
    Measurement(#[from] NativePhloMeasurementError),
    #[error(transparent)]
    Region(#[from] NativePhloRegionError),
    #[error(transparent)]
    Rule(#[from] NativePhloRuleError),
    #[error(transparent)]
    Authority(#[from] AuthorityError),
    #[error(transparent)]
    Work(#[from] FundingSearchError),
    #[error("native execution controls differ from the selected schedule")]
    ControlsMismatch,
    #[error("native execution charge belongs to another execution contract")]
    ContractMismatch,
    #[error("native execution charge arithmetic overflow")]
    Overflow,
    #[error("native execution exceeds the approved resource bound")]
    BoundExceeded,
}

#[derive(Debug)]
pub struct NativePhloExecutionContract<'a> {
    controls: CheckedPhloControls<'a>,
    policy: Arc<NativePhloExecutionPolicy>,
}

#[derive(Debug)]
struct NativePhloExecutionPolicy {
    rules: NativePhloRules,
    weights: Vec<u64>,
    bound: u64,
}

#[derive(Clone, Debug)]
pub struct PreparedNativePhloCharge {
    policy: Arc<NativePhloExecutionPolicy>,
    observation: Arc<ByteObservation>,
    usage: u64,
}

#[derive(Debug)]
pub struct NativePhloReservation {
    policy: Arc<NativePhloExecutionPolicy>,
    used: u64,
}

#[derive(Clone, Debug)]
pub struct NativePhloChargePreparer {
    policy: Arc<NativePhloExecutionPolicy>,
}

impl<'a> NativePhloExecutionContract<'a> {
    pub fn new(
        controls: CheckedPhloControls<'a>,
        binding: &PhloScheduleBinding<'_>,
    ) -> Result<Self, NativePhloExecutionError> {
        if controls.schedule() != binding.schedule() {
            return Err(NativePhloExecutionError::ControlsMismatch);
        }
        let rules = NativePhloRules::resolve(binding.descriptor())?;
        let mut weights = Vec::new();
        weights
            .try_reserve_exact(controls.schedule().weights.len())
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        weights.extend_from_slice(controls.schedule().weights);
        Ok(Self {
            controls,
            policy: Arc::new(NativePhloExecutionPolicy {
                rules,
                weights,
                bound: controls.resource_bound(),
            }),
        })
    }

    pub fn controls(&self) -> CheckedPhloControls<'a> { self.controls }

    pub fn reservation(self) -> NativePhloReservation {
        NativePhloReservation {
            policy: self.policy,
            used: 0,
        }
    }

    pub fn prepare(
        &self,
        observation: Arc<ByteObservation>,
        limits: NativePhloRegionLimits,
        budget: &HostWorkBudget,
    ) -> Result<PreparedNativePhloCharge, NativePhloExecutionError> {
        self.policy.prepare(observation, limits, budget)
    }
}

impl NativePhloExecutionPolicy {
    fn prepare(
        self: &Arc<Self>,
        observation: Arc<ByteObservation>,
        limits: NativePhloRegionLimits,
        budget: &HostWorkBudget,
    ) -> Result<PreparedNativePhloCharge, NativePhloExecutionError> {
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            size_of::<Arc<ByteObservation>>(),
        )?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(1)
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        rows.push(Arc::clone(&observation));
        let snapshot = ByteObservationSnapshot {
            rows,
            metered_context: true,
            history_lost: false,
        };
        let measured = self.rules.measure(&snapshot, 1)?;
        let regions = measured.region_demands(limits, budget)?;
        let mut usage = 0_u64;
        for demand in regions.occurrences() {
            let signature = demand
                .region()
                .signature
                .as_ref()
                .ok_or(AuthorityError::MissingSignature)?;
            let units = authority_units(signature, budget)?;
            let measurement = demand.measurement();
            let weight = self.weights[measurement.class()];
            usage = weight
                .checked_mul(units)
                .and_then(|amount| amount.checked_mul(measurement.quantity()))
                .and_then(|amount| usage.checked_add(amount))
                .ok_or(NativePhloExecutionError::Overflow)?;
        }
        Ok(PreparedNativePhloCharge {
            policy: Arc::clone(self),
            observation,
            usage,
        })
    }
}

impl PreparedNativePhloCharge {
    pub fn usage(&self) -> u64 { self.usage }
    pub fn observation(&self) -> &Arc<ByteObservation> { &self.observation }
}

impl NativePhloReservation {
    pub fn used(&self) -> u64 { self.used }

    pub fn preparer(&self) -> NativePhloChargePreparer {
        NativePhloChargePreparer {
            policy: Arc::clone(&self.policy),
        }
    }

    pub fn prepare(
        &self,
        observation: Arc<ByteObservation>,
        limits: NativePhloRegionLimits,
        budget: &HostWorkBudget,
    ) -> Result<PreparedNativePhloCharge, NativePhloExecutionError> {
        self.policy.prepare(observation, limits, budget)
    }

    pub fn reserve(
        &mut self,
        charge: &PreparedNativePhloCharge,
    ) -> Result<(), NativePhloExecutionError> {
        if !Arc::ptr_eq(&self.policy, &charge.policy) {
            return Err(NativePhloExecutionError::ContractMismatch);
        }
        reserve_native_usage(&mut self.used, self.policy.bound, charge.usage)
    }
}

impl NativePhloChargePreparer {
    pub fn prepare(
        &self,
        observation: Arc<ByteObservation>,
        limits: NativePhloRegionLimits,
        budget: &HostWorkBudget,
    ) -> Result<PreparedNativePhloCharge, NativePhloExecutionError> {
        self.policy.prepare(observation, limits, budget)
    }
}

fn reserve_native_usage(
    used: &mut u64,
    limit: u64,
    charge: u64,
) -> Result<(), NativePhloExecutionError> {
    let next = used
        .checked_add(charge)
        .ok_or(NativePhloExecutionError::Overflow)?;
    if next > limit {
        return Err(NativePhloExecutionError::BoundExceeded);
    }
    *used = next;
    Ok(())
}

fn authority_units(
    signature: &CostSignature,
    budget: &HostWorkBudget,
) -> Result<u64, NativePhloExecutionError> {
    let mut pending = Vec::new();
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        size_of::<&CostSignature>(),
    )?;
    pending
        .try_reserve_exact(1)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    pending.push(signature);
    let mut units = 0_u64;
    while let Some(current) = pending.pop() {
        reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
        match current.value.as_ref() {
            Some(Value::Unit(true)) => {}
            Some(Value::Ground(_) | Value::Name(_) | Value::Quote(_)) => {
                units = units
                    .checked_add(1)
                    .ok_or(NativePhloExecutionError::Overflow)?;
            }
            Some(Value::Compound(compound)) => {
                reserve_work(
                    budget,
                    HostWorkDimension::SearchStateBytes,
                    compound
                        .elements
                        .len()
                        .checked_mul(size_of::<&CostSignature>())
                        .ok_or(NativePhloExecutionError::Overflow)?,
                )?;
                pending
                    .try_reserve(compound.elements.len())
                    .map_err(|_| FundingSearchError::AllocationFailed)?;
                pending.extend(&compound.elements);
            }
            _ => return Err(AuthorityError::UnsupportedFundingSignature.into()),
        }
    }
    Ok(units)
}

#[cfg(test)]
mod tests;
