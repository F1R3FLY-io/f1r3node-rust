use std::mem::size_of;

use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;

use super::{decode_receipt_data, invalid, receipt_channel, CasperError, PrepaidReceiptLimits};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

type SnapshotEntry = ([u8; 32], Option<Vec<u8>>);

#[derive(Debug, PartialEq, Eq)]
pub struct PrepaidReceiptSnapshot {
    root: [u8; 32],
    entries: Vec<SnapshotEntry>,
}

impl PrepaidReceiptSnapshot {
    pub fn root(&self) -> [u8; 32] { self.root }

    pub fn receipt(
        &self,
        expected_root: &[u8; 32],
        receipt_id: &[u8; 32],
    ) -> Result<Option<Option<&[u8]>>, CasperError> {
        if *expected_root != self.root {
            return Err(invalid("receipt snapshot belongs to another state root"));
        }
        Ok(self
            .entries
            .binary_search_by_key(receipt_id, |entry| entry.0)
            .ok()
            .map(|index| self.entries[index].1.as_deref()))
    }
}

pub(super) fn reserve(
    budget: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), CasperError> {
    let amount = u64::try_from(amount).map_err(|_| invalid("snapshot work overflow"))?;
    budget
        .reserve(dimension, HostWorkUnits::new(amount))
        .map_err(|error| invalid(&error.to_string()))?;
    Ok(())
}

impl RuntimeManager {
    pub fn capture_prepaid_receipts(
        &self,
        pre_state_root: [u8; 32],
        receipt_ids: &[[u8; 32]],
        limits: PrepaidReceiptLimits,
        budget: &HostWorkBudget,
    ) -> Result<PrepaidReceiptSnapshot, CasperError> {
        if receipt_ids.len() > limits.entries {
            return Err(invalid("receipt snapshot entry limit exceeded"));
        }
        let overhead = receipt_ids
            .len()
            .checked_mul(size_of::<[u8; 32]>() + size_of::<SnapshotEntry>())
            .ok_or_else(|| invalid("receipt snapshot size overflow"))?;
        let mut remaining_bytes = limits
            .batch_bytes
            .checked_sub(overhead)
            .ok_or_else(|| invalid("receipt snapshot byte limit exceeded"))?;
        reserve(budget, HostWorkDimension::SearchStateBytes, overhead)?;
        let sorting_work = receipt_ids
            .len()
            .checked_mul(1 + receipt_ids.len().checked_ilog2().unwrap_or(0) as usize)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| invalid("snapshot sorting work overflow"))?;
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            sorting_work,
        )?;
        let mut keys = Vec::new();
        keys.try_reserve_exact(receipt_ids.len())
            .map_err(|_| invalid("snapshot key allocation failed"))?;
        keys.extend_from_slice(receipt_ids);
        keys.sort_unstable();
        if keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid("duplicate receipt snapshot key"));
        }
        let root = Blake2b256Hash::from_bytes(pre_state_root.to_vec());
        let registered = self.history_repo.contains_root(&root).map_err(|error| {
            CasperError::RuntimeError(format!("prepaid snapshot root lookup failed: {error}"))
        })?;
        if !registered {
            return Err(CasperError::RuntimeError(
                "prepaid snapshot requires a registered state root".into(),
            ));
        }
        let reader = self
            .history_repo
            .get_history_reader(&root)
            .map_err(|error| {
                CasperError::RuntimeError(format!("prepaid snapshot root read failed: {error}"))
            })?;
        if reader.root() != root {
            return Err(CasperError::RuntimeError(
                "prepaid snapshot reader returned another root".into(),
            ));
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(keys.len())
            .map_err(|_| invalid("snapshot entry allocation failed"))?;
        for key in keys {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            let channel = stable_hash_provider::hash(&receipt_channel(&key));
            let data = reader.get_data(&channel).map_err(|error| {
                CasperError::RuntimeError(format!("prepaid snapshot data read failed: {error}"))
            })?;
            let value = decode_receipt_data(&data, limits.value_bytes.min(remaining_bytes))?;
            let captured = if let Some(bytes) = value {
                remaining_bytes = remaining_bytes
                    .checked_sub(bytes.len())
                    .ok_or_else(|| invalid("receipt snapshot byte limit exceeded"))?;
                reserve(budget, HostWorkDimension::SearchStateBytes, bytes.len())?;
                reserve(
                    budget,
                    HostWorkDimension::VerificationOperations,
                    bytes.len(),
                )?;
                let mut copy = Vec::new();
                copy.try_reserve_exact(bytes.len())
                    .map_err(|_| invalid("snapshot value allocation failed"))?;
                copy.extend_from_slice(bytes);
                Some(copy)
            } else {
                None
            };
            entries.push((key, captured));
        }
        if reader.root() != root {
            return Err(CasperError::RuntimeError(
                "prepaid snapshot reader changed its root".into(),
            ));
        }
        Ok(PrepaidReceiptSnapshot {
            root: pre_state_root,
            entries,
        })
    }
}

#[cfg(test)]
mod tests;
