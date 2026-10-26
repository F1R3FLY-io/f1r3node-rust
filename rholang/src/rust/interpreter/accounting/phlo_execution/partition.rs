use models::rust::phlo_obligation::{
    PhloObligationKeyError, PhloObligationKeyLimits, PhloObligationKeyV1,
};
use thiserror::Error;

use super::{resource_key, PhloExecutionError, ResourceEntries, WorkBudget};
use crate::rust::interpreter::accounting::monetary_allocation::{
    canonical_funding_key_order, filled_vec, FundingSearchError,
};

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
) -> Result<NormalizedPartition, PhloPartitionError> {
    let mut keys = filled_vec(resources.len(), Vec::new())?;
    let mut quantities = filled_vec(resources.len(), 0_u64)?;
    for ((key, quantity), entry) in keys.iter_mut().zip(&mut quantities).zip(resources.iter()) {
        if entry.quantity == 0 {
            return Err(PhloExecutionError::ZeroQuantity.into());
        }
        *quantity = entry.quantity;
        let resource = resource_key(entry.resource, keys_work)?.into_wire()?;
        let mut key_limits = limits;
        key_limits.wire.total_bytes = key_limits.wire.total_bytes.min(*remaining_bytes);
        let record = PhloObligationKeyV1::Resource(resource);
        let prepared = record.prepare_encoding(key_limits)?;
        *remaining_bytes = remaining_bytes
            .checked_sub(prepared.encoded_len())
            .ok_or(FundingSearchError::Overflow)?;
        *key = prepared.encode()?;
    }
    let mut refs = filled_vec(keys.len(), &[][..])?;
    for (reference, key) in refs.iter_mut().zip(&keys) {
        *reference = key;
    }
    let sorted = canonical_funding_key_order(&refs)?;
    let mut order: Vec<usize> = Vec::new();
    order
        .try_reserve_exact(sorted.len())
        .map_err(|_| FundingSearchError::AllocationFailed)?;
    for index in sorted {
        if let Some(previous) = order.last().copied() {
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
