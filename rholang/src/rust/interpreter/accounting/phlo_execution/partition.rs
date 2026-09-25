use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use models::rust::phlo_obligation::{
    PhloObligationKeyError, PhloObligationKeyLimits, PhloObligationKeyV1,
};
use thiserror::Error;

use super::{resource_key_with_host_work, PhloExecutionError, ResourceEntries, WorkBudget};
use crate::rust::interpreter::accounting::monetary_allocation::{
    canonical_funding_key_order, filled_vec, reserve_work, FundingSearchError,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Debug, Error)]
pub enum PhloPartitionError {
    #[error(transparent)]
    Execution(#[from] PhloExecutionError),
    #[error(transparent)]
    Key(#[from] PhloObligationKeyError),
    #[error(transparent)]
    Search(#[from] FundingSearchError),
}

#[derive(Debug)]
pub(super) struct NormalizedPartition {
    pub(super) keys: Vec<Vec<u8>>,
    pub(super) quantities: Vec<u64>,
    pub(super) order: Vec<usize>,
}

pub(super) fn normalize_partition(
    resources: ResourceEntries<'_>,
    limits: PhloObligationKeyLimits,
    remaining_bytes: &mut usize,
    keys_work: &mut WorkBudget,
    budget: &HostWorkBudget,
) -> Result<NormalizedPartition, PhloPartitionError> {
    reserve_work(budget, HostWorkDimension::SearchCandidates, resources.len())?;
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        resources
            .len()
            .checked_mul(
                size_of::<Vec<u8>>() + size_of::<&[u8]>() + size_of::<u64>() + size_of::<usize>(),
            )
            .ok_or(FundingSearchError::Overflow)?,
    )?;
    let mut keys = filled_vec(resources.len(), Vec::new())?;
    let mut quantities = filled_vec(resources.len(), 0_u64)?;
    for ((key, quantity), entry) in keys.iter_mut().zip(&mut quantities).zip(resources.iter()) {
        if entry.quantity == 0 {
            return Err(PhloExecutionError::ZeroQuantity.into());
        }
        *quantity = entry.quantity;
        let resource =
            resource_key_with_host_work(entry.resource, keys_work, Some(budget))?.into_wire()?;
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            resource
                .authority
                .len()
                .checked_mul(2)
                .and_then(|n| n.checked_add(16))
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let mut key_limits = limits;
        key_limits.wire.total_bytes = key_limits.wire.total_bytes.min(*remaining_bytes);
        let record = PhloObligationKeyV1::Resource(resource);
        let prepared = record.prepare_encoding(key_limits)?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            prepared.encoded_len(),
        )?;
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            prepared.encoded_len(),
        )?;
        *remaining_bytes = remaining_bytes
            .checked_sub(prepared.encoded_len())
            .ok_or(FundingSearchError::Overflow)?;
        *key = prepared.encode()?;
    }
    let mut refs = filled_vec(keys.len(), &[][..])?;
    for (reference, key) in refs.iter_mut().zip(&keys) {
        *reference = key;
    }
    let sorted = canonical_funding_key_order(&refs, budget)?;
    let mut order: Vec<usize> = Vec::new();
    order
        .try_reserve_exact(sorted.len())
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    for index in sorted {
        if let Some(previous) = order.last().copied() {
            reserve_work(
                budget,
                HostWorkDimension::VerificationOperations,
                keys[index]
                    .len()
                    .min(keys[previous].len())
                    .checked_add(1)
                    .ok_or(FundingSearchError::Overflow)?,
            )?;
            if keys[index] == keys[previous] {
                quantities[previous] = quantities[previous]
                    .checked_add(quantities[index])
                    .ok_or(PhloExecutionError::ArithmeticOverflow)?;
                continue;
            }
        }
        order.push(index);
    }
    Ok(NormalizedPartition {
        keys,
        quantities,
        order,
    })
}
