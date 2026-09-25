use std::mem::size_of;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::{ListParWithRandom, Par};
use models::rust::utils::new_gsys_auth_token_par;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::rho_type::{RhoByteArray, RhoList};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::Log;

use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;

const RECEIPT_DOMAIN: &[u8] = b"f1r3node:prepaid-resource-receipt:v1";
const REPLACEMENT_DOMAIN: &[u8] = b"f1r3node:prepaid-resource-receipt-replacement:v1";

mod bucket;
pub use bucket::{PrepaidReceiptBucket, PrepaidReceiptBucketLimits};
mod ordered;
pub use ordered::{OrderedPrepaidCells, PrepaidCellLimits};
mod cell;
pub use cell::{
    NativePrepaidCell, NativePrepaidCellLimits, NativePrepaidContribution, NativePrepaidOrigin,
};
mod stack_pops;
pub use stack_pops::{PrepaidStackPop, PrepaidStackPopLimits};
mod snapshot;
pub use snapshot::PrepaidReceiptSnapshot;
mod physical;
pub use physical::{CapturedPrepaidStacks, PrepaidStackCaptureLimits};
mod inventory;
pub use inventory::{
    NativeMeasuredSettlementLimits, NativePrepaidDemandBinding, NativePrepaidDemandInput,
    NativePrepaidDemandLimits, NativePrepaidInventory, NativePrepaidInventoryLimits,
    NativePrepaidPrefix, NativePrepaidResource,
};
mod births;
pub use births::{CapturedNativeRetainedBirths, NativeRetainedBirthLimits};
pub(in crate::rust::util::rholang::costacc) mod retained_records;
pub use retained_records::{
    NativeRetainedReceiptRecords, NativeRetainedRecordLimits, NativeRetainedStackRecord,
    PreparedNativeRetainedReceipts,
};
mod settlement;
pub use settlement::{NativeRetainedSettlement, NativeRetainedSettlementLimits};
mod consumption;
pub use consumption::{NativePrepaidConsumption, NativeWalletSettlement};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepaidReceiptChange<'a> {
    pub receipt_id: [u8; 32],
    pub expected: Option<&'a [u8]>,
    pub replacement: Option<&'a [u8]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepaidReceiptLimits {
    pub entries: usize,
    pub value_bytes: usize,
    pub batch_bytes: usize,
}

fn invalid(reason: &str) -> CasperError {
    CasperError::InvalidCostSettlement(format!("prepaid receipt: {reason}"))
}

fn receipt_channel(receipt_id: &[u8; 32]) -> Par {
    RhoList::create_par(vec![
        new_gsys_auth_token_par(Vec::new(), false),
        RhoByteArray::create_par(RECEIPT_DOMAIN.to_vec()),
        RhoByteArray::create_par(receipt_id.to_vec()),
    ])
}

fn receipt_datum(bytes: &[u8]) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![RhoByteArray::create_par(bytes.to_vec())],
        random_state: Vec::new(),
        cost_authority: None,
        cost_stack: None,
    }
}

fn replacement_id(change: PrepaidReceiptChange<'_>) -> Vec<u8> {
    Blake2b256::hash_stream(|feed| {
        feed(REPLACEMENT_DOMAIN);
        feed(&change.receipt_id);
        for value in [change.expected, change.replacement] {
            match value {
                None => feed(&[0]),
                Some(bytes) => {
                    feed(&[1]);
                    feed(&(bytes.len() as u64).to_be_bytes());
                    feed(bytes);
                }
            }
        }
    })
}

fn ordered_changes<'a>(
    changes: &'a [PrepaidReceiptChange<'a>],
    limits: PrepaidReceiptLimits,
) -> Result<Vec<&'a PrepaidReceiptChange<'a>>, CasperError> {
    if changes.len() > limits.entries {
        return Err(invalid("entry limit exceeded"));
    }
    let mut bytes = changes
        .len()
        .checked_mul(size_of::<&PrepaidReceiptChange<'_>>())
        .ok_or_else(|| invalid("batch size overflow"))?;
    for change in changes {
        if change.expected == change.replacement {
            return Err(invalid("replacement must change the receipt"));
        }
        for value in [change.expected, change.replacement].into_iter().flatten() {
            if value.is_empty() || value.len() > limits.value_bytes {
                return Err(invalid("empty or oversized value"));
            }
            bytes = bytes
                .checked_add(value.len())
                .ok_or_else(|| invalid("batch size overflow"))?;
        }
    }
    if bytes > limits.batch_bytes {
        return Err(invalid("batch byte limit exceeded"));
    }
    let mut ordered: Vec<&PrepaidReceiptChange<'_>> = Vec::new();
    ordered
        .try_reserve_exact(changes.len())
        .map_err(|_| invalid("batch allocation failed"))?;
    ordered.extend(changes);
    ordered.sort_unstable_by_key(|change| change.receipt_id);
    if ordered
        .windows(2)
        .any(|pair| pair[0].receipt_id == pair[1].receipt_id)
    {
        return Err(invalid("duplicate receipt identity"));
    }
    Ok(ordered)
}

fn decode_receipt_data(
    data: &[Datum<ListParWithRandom>],
    maximum_bytes: usize,
) -> Result<Option<&[u8]>, CasperError> {
    let [datum] = data else {
        return if data.is_empty() {
            Ok(None)
        } else {
            Err(invalid("multiple stored values"))
        };
    };
    let [value] = datum.a.pars.as_slice() else {
        return Err(invalid("malformed stored value"));
    };
    let [models::rhoapi::Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::GByteArray(bytes)),
    }] = value.exprs.as_slice()
    else {
        return Err(invalid("stored value is not bytes"));
    };
    if bytes.is_empty() || bytes.len() > maximum_bytes {
        return Err(invalid("empty or oversized stored value"));
    }
    if datum.persist || datum.a != receipt_datum(bytes) {
        return Err(invalid("noncanonical stored value"));
    }
    Ok(Some(bytes))
}

impl RuntimeOps {
    pub async fn read_prepaid_receipt(
        &self,
        receipt_id: &[u8; 32],
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, CasperError> {
        let data = self
            .runtime
            .reducer
            .space
            .get_data(&receipt_channel(receipt_id))
            .await;
        Ok(decode_receipt_data(&data, maximum_bytes)?.map(<[u8]>::to_vec))
    }

    pub async fn replace_prepaid_receipts(
        &mut self,
        changes: &[PrepaidReceiptChange<'_>],
        limits: PrepaidReceiptLimits,
    ) -> Result<Log, CasperError> {
        let ordered = ordered_changes(changes, limits)?;
        for change in &ordered {
            if self
                .read_prepaid_receipt(&change.receipt_id, limits.value_bytes)
                .await?
                .as_deref()
                != change.expected
            {
                return Err(invalid("stored value differs from the captured receipt"));
            }
        }
        if ordered.is_empty() {
            return Ok(Vec::new());
        }
        let checkpoint = self.runtime.create_soft_checkpoint().await;
        let result = async {
            for change in ordered {
                let channel = receipt_channel(&change.receipt_id);
                if change.expected.is_some() {
                    self.runtime
                        .reducer
                        .space
                        .remove_data_at_recorded(&channel, 0, &replacement_id(*change))
                        .await
                        .map_err(|error| {
                            CasperError::RuntimeError(format!(
                                "prepaid receipt recorded removal failed: {error}"
                            ))
                        })?;
                }
                if let Some(bytes) = change.replacement {
                    let matched = self
                        .runtime
                        .reducer
                        .space
                        .produce(channel, receipt_datum(bytes), false)
                        .await
                        .map_err(|error| {
                            CasperError::RuntimeError(format!(
                                "prepaid receipt replacement failed: {error}"
                            ))
                        })?;
                    if matched.is_some() {
                        return Err(invalid(
                            "protected receipt matched an unexpected continuation",
                        ));
                    }
                }
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            self.runtime.revert_to_soft_checkpoint(checkpoint).await;
            return Err(error);
        }
        let mut log = checkpoint.log;
        log.extend(self.runtime.take_event_log().await);
        Ok(log)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod checkpoint_tests;
