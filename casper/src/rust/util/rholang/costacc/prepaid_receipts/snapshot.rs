use std::mem::size_of;

use models::rhoapi::ListParWithRandom;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::native_reader::{
    decode_record, NativeLeafKind, NativeReadCharge, NativeReadError, NativeReadMeter,
};
use rspace_plus_plus::rspace::internal::Datum;

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

pub(super) struct ReceiptReadMeter<'a>(pub &'a HostWorkBudget);

impl NativeReadMeter for ReceiptReadMeter<'_> {
    type Error = CasperError;

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), Self::Error> {
        for (dimension, amount) in [
            (HostWorkDimension::VerificationOperations, charge.operations),
            (HostWorkDimension::VerificationBytes, charge.scanned_bytes),
            (HostWorkDimension::SearchStateBytes, charge.backing_bytes),
        ] {
            reserve(self.0, dimension, amount)?;
        }
        Ok(())
    }
}

pub(super) fn native_read_error(error: NativeReadError<CasperError>) -> CasperError {
    match error {
        NativeReadError::Host(error) | NativeReadError::Consumer(error) => error,
        NativeReadError::Store(error) => {
            CasperError::RuntimeError(format!("prepaid snapshot history read failed: {error}"))
        }
        NativeReadError::Invalid(error) => {
            CasperError::RuntimeError(format!("prepaid snapshot history is invalid: {error:?}"))
        }
    }
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
        let reader = self.history_repo.native_history_reader(pre_state_root);
        let meter = ReceiptReadMeter(budget);
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(keys.len())
            .map_err(|_| invalid("snapshot entry allocation failed"))?;
        for key in keys {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            let channel = stable_hash_provider::hash(&receipt_channel(&key));
            let channel_hash: [u8; 32] = channel
                .0
                .as_slice()
                .try_into()
                .map_err(|_| invalid("snapshot channel hash is invalid"))?;
            let captured = reader
                .with_records(NativeLeafKind::Data, &channel_hash, &meter, |records| {
                    if records.len() > 1 {
                        return Err(invalid("multiple stored values"));
                    }
                    let Some(raw) = records.iter().next() else {
                        return Ok(None);
                    };
                    let datum: Datum<ListParWithRandom> =
                        decode_record(raw, &meter).map_err(native_read_error)?;
                    let Some(bytes) = decode_receipt_data(
                        std::slice::from_ref(&datum),
                        limits.value_bytes.min(remaining_bytes),
                    )?
                    else {
                        return Ok(None);
                    };
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
                    Ok(Some(copy))
                })
                .map_err(native_read_error)?
                .flatten();
            entries.push((key, captured));
        }
        Ok(PrepaidReceiptSnapshot {
            root: pre_state_root,
            entries,
        })
    }
}

#[cfg(test)]
mod tests;
