use thiserror::Error;

use super::{
    resource_key, CountedPhloExecutionWitness, PhloExecutionError, PhloExecutionLimits,
    PhloResource, PhloResourceAmount, WorkBudget,
};
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchError;

#[derive(Clone, Copy, Debug)]
pub struct MeasuredPhloPrepaidUse<'a> {
    pub demand_index: usize,
    pub resource: PhloResource<'a>,
}

#[derive(Debug, Error)]
pub enum MeasuredPhloPrepaidError {
    #[error(transparent)]
    Execution(#[from] PhloExecutionError),
    #[error(transparent)]
    Work(#[from] FundingSearchError),
    #[error("prepaid resource refers to an absent demand occurrence")]
    MissingDemand,
    #[error("prepaid resource exceeds the remaining measured demand")]
    ExcessConsumption,
    #[error("prepaid resource differs from the measured location, class, or authority")]
    ShapeMismatch,
}

#[derive(Debug)]
pub struct PreparedMeasuredPhloDemand<'a> {
    prepaid: Vec<PhloResourceAmount<'a>>,
    fresh: Vec<PhloResourceAmount<'a>>,
    required: Vec<PhloResourceAmount<'a>>,
}

impl PreparedMeasuredPhloDemand<'_> {
    pub fn witness(&self) -> CountedPhloExecutionWitness<'_> {
        CountedPhloExecutionWitness {
            available: &self.prepaid,
            required: &self.required,
            used: &self.prepaid,
            unused: &[],
            fresh: &self.fresh,
        }
    }
}

pub fn bind_measured_phlo_prepaid<'a>(
    measured: &[PhloResourceAmount<'a>],
    assignments: &[MeasuredPhloPrepaidUse<'a>],
    limits: PhloExecutionLimits,
) -> Result<PreparedMeasuredPhloDemand<'a>, MeasuredPhloPrepaidError> {
    let entries = measured
        .len()
        .checked_add(assignments.len())
        .filter(|count| *count <= limits.resource_entries)
        .ok_or(PhloExecutionError::TooManyResourceEntries)?;
    let mut fresh = Vec::new();
    fresh
        .try_reserve_exact(measured.len())
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut prepaid = Vec::new();
    prepaid
        .try_reserve_exact(assignments.len())
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut required = Vec::new();
    required
        .try_reserve_exact(entries)
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    let mut keys = WorkBudget {
        remaining_nodes: limits.authority_nodes,
        remaining_bytes: limits.key_bytes,
    };
    for amount in measured {
        if amount.quantity == 0 {
            return Err(PhloExecutionError::ZeroQuantity.into());
        }
        resource_key(amount.resource, &mut keys)?;
        fresh.push(*amount);
    }
    for assignment in assignments {
        let remaining = fresh
            .get_mut(assignment.demand_index)
            .ok_or(MeasuredPhloPrepaidError::MissingDemand)?;
        let mut actual = resource_key(assignment.resource, &mut keys)?;
        let mut expected = resource_key(remaining.resource, &mut keys)?;
        actual.acquisition_terms = &[];
        expected.acquisition_terms = &[];
        if actual != expected {
            return Err(MeasuredPhloPrepaidError::ShapeMismatch);
        }
        remaining.quantity = remaining
            .quantity
            .checked_sub(1)
            .ok_or(MeasuredPhloPrepaidError::ExcessConsumption)?;
        prepaid.push(PhloResourceAmount {
            resource: assignment.resource,
            quantity: 1,
        });
    }
    fresh.retain(|amount| amount.quantity > 0);
    required.extend_from_slice(&prepaid);
    required.extend_from_slice(&fresh);
    Ok(PreparedMeasuredPhloDemand {
        prepaid,
        fresh,
        required,
    })
}

#[cfg(test)]
mod tests;
