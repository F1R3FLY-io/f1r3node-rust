use std::num::NonZeroUsize;

use models::rust::host_work::HostWorkDimension;

use super::{reserve_work, FundingAssignmentError, FundingSearchError};
use crate::rust::interpreter::host_work::HostWorkBudget;

pub fn can_complete_unit_fee(
    capacities: &[u64],
    resource_draws: &[u64],
    fee_eligible: &[bool],
    source_cap: NonZeroUsize,
    budget: &HostWorkBudget,
) -> Result<bool, FundingSearchError> {
    let count = capacities.len();
    if count == 0 {
        return Err(FundingAssignmentError::EmptySources.into());
    }
    if count > source_cap.get() {
        return Err(FundingAssignmentError::TooManySources.into());
    }
    if resource_draws.len() != count || fee_eligible.len() != count {
        return Err(FundingAssignmentError::InvalidDimensions.into());
    }
    reserve_work(budget, HostWorkDimension::VerificationOperations, count)?;
    let mut payable = false;
    for ((capacity, draw), eligible) in capacities.iter().zip(resource_draws).zip(fee_eligible) {
        if draw > capacity {
            return Err(FundingAssignmentError::InsufficientSourceCapacity.into());
        }
        payable |= *eligible && draw < capacity;
    }
    Ok(payable)
}

#[cfg(test)]
#[path = "funding_fee_completion_tests.rs"]
mod tests;
