use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use models::rust::phlo_obligation::{
    PhloObligationKeyError, PhloObligationKeyLimits, PhloObligationKeyV1,
};
use models::rust::phlo_wire::PhloWireError;
use thiserror::Error;

use super::{
    push_key_entry, resource_key, resource_weight, CheckedPhloExecution, PhloExecutionError,
    PhloOutcome, PhloResource, WorkBudget,
};
use crate::rust::interpreter::accounting::monetary_allocation::{
    check_funding_assignment, FundingAssignmentError, FundingAssignmentTotals,
};

mod policy;
pub use policy::{
    PhloObligationFunding, PhloObligationFundingError, PhloObligationFundingInput,
    PhloObligationFundingLimits, PhloObligationFundingProposal,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhloObligationKey<'a> {
    Fee,
    Resource(PhloResource<'a>),
    RetainedResource(PhloResource<'a>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedPhloObligations<'a> {
    execution: CheckedPhloExecution<'a>,
    outcome: PhloOutcome<'a>,
    keys: Vec<PhloObligationKey<'a>>,
    amounts: Vec<u64>,
    quantities: Vec<u64>,
    total: u64,
}

impl<'a> CheckedPhloObligations<'a> {
    pub fn execution(&self) -> CheckedPhloExecution<'a> { self.execution }

    pub fn outcome(&self) -> PhloOutcome<'a> { self.outcome }

    pub fn keys(&self) -> &[PhloObligationKey<'a>] { &self.keys }

    pub fn amounts(&self) -> &[u64] { &self.amounts }

    pub fn quantities(&self) -> &[u64] { &self.quantities }

    pub fn total(&self) -> u64 { self.total }

    pub fn encoded_keys(
        &self,
        limits: PhloObligationKeyLimits,
        aggregate_bytes: usize,
    ) -> Result<Vec<Vec<u8>>, PhloObligationError> {
        let mut output = Vec::new();
        let mut output_reserved = 0;
        let mut remaining = aggregate_bytes;
        let mut budget = WorkBudget {
            remaining_nodes: self
                .execution
                .limits
                .authority_nodes
                .min(limits.authority_nodes),
            remaining_bytes: self.execution.limits.key_bytes,
        };
        for key in &self.keys {
            let record = match key {
                PhloObligationKey::Fee => PhloObligationKeyV1::Fee,
                PhloObligationKey::Resource(resource) => PhloObligationKeyV1::Resource(
                    resource_key(*resource, &mut budget)?.into_wire()?,
                ),
                PhloObligationKey::RetainedResource(resource) => {
                    PhloObligationKeyV1::RetainedResource(
                        resource_key(*resource, &mut budget)?.into_wire()?,
                    )
                }
            };
            let mut key_limits = limits;
            key_limits.wire.total_bytes = key_limits.wire.total_bytes.min(remaining);
            let prepared = record.prepare_encoding(key_limits)?;
            let encoded = prepared.encode()?;
            remaining = remaining
                .checked_sub(encoded.len())
                .ok_or(PhloObligationKeyError::Wire(PhloWireError::LimitExceeded))?;
            push_key_entry(&mut output, &mut output_reserved, encoded)?;
        }
        Ok(output)
    }

    pub fn check_assignment(
        &self,
        capacities: &[u64],
        eligible: &[Vec<bool>],
        assignment: &[Vec<u64>],
        source_cap: NonZeroUsize,
        obligation_cap: NonZeroUsize,
    ) -> Result<FundingAssignmentTotals, FundingAssignmentError> {
        check_funding_assignment(
            capacities,
            &self.amounts,
            eligible,
            assignment,
            source_cap,
            obligation_cap,
        )
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloObligationError {
    #[error(transparent)]
    KeyEncoding(#[from] PhloObligationKeyError),
    #[error("phlo obligations exceed the configured occurrence limit")]
    TooManyObligations,
    #[error(transparent)]
    Execution(#[from] PhloExecutionError),
    #[error("phlo obligation arithmetic overflow")]
    ArithmeticOverflow,
    #[error("phlo obligation total differs from the checked retained charge")]
    ChargeMismatch,
    #[error("failed or rejected execution cannot retain newly acquired resources")]
    RetainedResourcesOnFailure,
}

impl<'a> PhloResource<'a> {
    /// The canonical obligation-key encoding of this resource (C8, DR-87).
    /// The key derivation and its work limits are those of the settlement key
    /// encoding, so equal encodings mean equal resource keys.
    pub fn encoded_obligation_key(
        self,
        limits: PhloObligationKeyLimits,
    ) -> Result<Vec<u8>, PhloObligationError> {
        let mut budget = WorkBudget {
            remaining_nodes: limits.authority_nodes,
            remaining_bytes: limits.wire.field_bytes,
        };
        let resource = resource_key(self, &mut budget)?.into_wire()?;
        let record = PhloObligationKeyV1::Resource(resource);
        let prepared = record.prepare_encoding(limits)?;
        Ok(prepared.encode()?)
    }
}

pub fn project_phlo_obligations<'a>(
    execution: CheckedPhloExecution<'a>,
    outcome: PhloOutcome<'a>,
    obligation_cap: NonZeroUsize,
) -> Result<CheckedPhloObligations<'a>, PhloObligationError> {
    let fresh = execution.witness().parts()[4];
    let retained = execution.retained_acquisitions();
    if !retained.is_empty() && !matches!(outcome, PhloOutcome::Accepted([])) {
        return Err(PhloObligationError::RetainedResourcesOnFailure);
    }
    let count = fresh
        .len()
        .checked_add(retained.len())
        .and_then(|n| n.checked_add(1))
        .ok_or(PhloObligationError::TooManyObligations)?
        .min(obligation_cap.get());
    let charge = execution.retained_charge(outcome);
    let billable = charge != 0;
    let mut keys = Vec::new();
    let mut amounts: Vec<u64> = Vec::new();
    let mut quantities: Vec<u64> = Vec::new();
    keys.try_reserve_exact(count)
        .map_err(|_| PhloExecutionError::AllocationFailed)?;
    amounts
        .try_reserve_exact(count)
        .map_err(|_| PhloExecutionError::AllocationFailed)?;
    quantities
        .try_reserve_exact(count)
        .map_err(|_| PhloExecutionError::AllocationFailed)?;
    let mut indices: BTreeMap<_, usize> = BTreeMap::new();
    keys.push(PhloObligationKey::Fee);
    amounts.push(u64::from(billable));
    quantities.push(1);
    let mut total = u64::from(billable);
    let mut budget = WorkBudget {
        remaining_nodes: execution.limits.authority_nodes,
        remaining_bytes: execution.limits.key_bytes,
    };
    let schedule = execution.controls().schedule();
    for (is_retained, entry) in fresh
        .iter()
        .map(|entry| (false, entry))
        .chain(retained.iter().copied().map(|entry| (true, entry)))
    {
        let resource = entry.resource;
        let key = resource_key(resource, &mut budget)?;
        let amount = if billable {
            resource_weight(&key, schedule.weights)?
                .checked_mul(entry.quantity)
                .and_then(|amount| amount.checked_mul(schedule.actual_price))
                .ok_or(PhloObligationError::ArithmeticOverflow)?
        } else {
            0
        };
        total = total
            .checked_add(amount)
            .ok_or(PhloObligationError::ArithmeticOverflow)?;
        let key = (is_retained, key);
        if let Some(index) = indices.get(&key).copied() {
            quantities[index] = quantities[index]
                .checked_add(entry.quantity)
                .ok_or(PhloObligationError::ArithmeticOverflow)?;
            amounts[index] = amounts[index]
                .checked_add(amount)
                .ok_or(PhloObligationError::ArithmeticOverflow)?;
        } else {
            if keys.len() >= obligation_cap.get() {
                return Err(PhloObligationError::TooManyObligations);
            }
            indices.insert(key, keys.len());
            keys.push(if is_retained {
                PhloObligationKey::RetainedResource(resource)
            } else {
                PhloObligationKey::Resource(resource)
            });
            amounts.push(amount);
            quantities.push(entry.quantity);
        }
    }
    if total != charge {
        return Err(PhloObligationError::ChargeMismatch);
    }
    Ok(CheckedPhloObligations {
        execution,
        outcome,
        keys,
        amounts,
        quantities,
        total,
    })
}

#[cfg(test)]
mod tests;
