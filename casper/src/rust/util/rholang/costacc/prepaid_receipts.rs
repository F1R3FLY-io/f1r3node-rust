use std::mem::size_of;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::{ListParWithRandom, Par};
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::utils::new_gsys_auth_token_par;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::rho_type::{RhoByteArray, RhoList};
use rspace_plus_plus::rspace::errors::RSpaceError;
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
pub use stack_pops::{NativePrepaidPopResult, PrepaidStackPop, PrepaidStackPopLimits};
mod snapshot;
pub use snapshot::PrepaidReceiptSnapshot;
mod physical;
pub use physical::{CapturedPrepaidStacks, PrepaidStackCaptureLimits};
mod inventory;
pub use inventory::{
    CanonicalPrepaidSelection, NativeMeasuredSettlementLimits, NativePrepaidDemandBinding,
    NativePrepaidDemandInput, NativePrepaidDemandLimits, NativePrepaidInventory,
    NativePrepaidInventoryLimits, NativePrepaidPrefix, NativePrepaidResource,
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
mod composition;
pub use composition::compose_prepaid_receipt_changes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepaidReceiptChange<'a> {
    pub receipt_id: [u8; 32],
    pub expected: Option<&'a [u8]>,
    pub replacement: Option<&'a [u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedPrepaidReceiptChange {
    receipt_id: [u8; 32],
    expected: Option<Vec<u8>>,
    replacement: Option<Vec<u8>>,
}

impl OwnedPrepaidReceiptChange {
    pub fn receipt_id(&self) -> [u8; 32] { self.receipt_id }
    pub fn expected(&self) -> Option<&[u8]> { self.expected.as_deref() }
    pub fn replacement(&self) -> Option<&[u8]> { self.replacement.as_deref() }
    pub fn as_change(&self) -> PrepaidReceiptChange<'_> {
        PrepaidReceiptChange {
            receipt_id: self.receipt_id,
            expected: self.expected(),
            replacement: self.replacement(),
        }
    }

    pub fn copy_from(
        change: PrepaidReceiptChange<'_>,
        budget: &HostWorkBudget,
    ) -> Result<Self, CasperError> {
        let bytes = size_of::<Self>()
            .checked_add(change.expected.map_or(0, <[u8]>::len))
            .and_then(|n| n.checked_add(change.replacement.map_or(0, <[u8]>::len)))
            .ok_or_else(|| invalid("owned receipt change size overflow"))?;
        budget
            .reserve(
                HostWorkDimension::SearchStateBytes,
                HostWorkUnits::new(
                    u64::try_from(bytes)
                        .map_err(|_| invalid("owned receipt change size overflow"))?,
                ),
            )
            .map_err(|error| invalid(&error.to_string()))?;
        let copy = |value: Option<&[u8]>| -> Result<Option<Vec<u8>>, CasperError> {
            value
                .map(|bytes| {
                    let mut output = Vec::new();
                    output
                        .try_reserve_exact(bytes.len())
                        .map_err(|_| invalid("owned receipt change allocation failed"))?;
                    output.extend_from_slice(bytes);
                    Ok(output)
                })
                .transpose()
        };
        Ok(Self {
            receipt_id: change.receipt_id,
            expected: copy(change.expected)?,
            replacement: copy(change.replacement)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepaidReceiptLimits {
    pub entries: usize,
    pub value_bytes: usize,
    pub batch_bytes: usize,
}

fn invalid(reason: &str) -> CasperError {
    CasperError::RuntimeError(format!("prepaid receipt: {reason}"))
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

fn ordered_changes<'v, 'a>(
    changes: &'v [PrepaidReceiptChange<'a>],
    limits: PrepaidReceiptLimits,
) -> Result<Vec<&'v PrepaidReceiptChange<'a>>, CasperError> {
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

/// D-C4 (D-S5, DR-98): the metered read of a settlement channel. The read
/// never fills the store (`get_data_uncached_with_reader`), so its charge
/// depends on the residency of the channel and on the shard population.
///
/// Residency invariant (RI): at every call, the data map of the runtime holds
/// exactly the install channels and the channels that the settlement wrote,
/// by recorded removal or produce, since the last reset or checkpoint of the
/// runtime. The install channels are the same on the play and replay
/// runtimes. Premise P_join: no join group contains a receipt channel or a
/// supply channel, so a settlement produce fetches no other channel's data.
/// Under RI the charge is a function of the root, the channel, and the
/// earlier writes (`NativeSharedReads.cache_state_independent_charge`), so
/// the producer, its self-replay, and every validator charge each read the
/// same.
///
/// Callers: the mergeable reads (`runtime.rs`, also from
/// `runtime_manager.rs`), births (`births.rs`), retained records
/// (`retained_records.rs`), supply inventories (`consumption.rs`,
/// `stack_pops.rs`, `supply.rs`), and receipts (`read_prepaid_receipt_metered`).
/// Three of these reads observe earlier writes: the stack removals of
/// `stack_pops.rs` and the receipts of a bucket that a birth shares with a
/// pop. A read of only the history would be stale there, so the read always
/// probes the store first.
pub(crate) async fn read_live_data_metered(
    runtime: &RuntimeOps,
    channel: &Par,
    budget: &HostWorkBudget,
) -> Result<Vec<Datum<ListParWithRandom>>, CasperError> {
    let reserve = |operations: usize, scanned: usize, backing: usize| {
        for (dimension, amount) in [
            (HostWorkDimension::VerificationOperations, operations),
            (HostWorkDimension::VerificationBytes, scanned),
            (HostWorkDimension::SearchStateBytes, backing),
        ] {
            budget
                .reserve(
                    dimension,
                    HostWorkUnits::new(
                        u64::try_from(amount).map_err(|_| RSpaceError::HostWorkRejected)?,
                    ),
                )
                .map_err(|_| RSpaceError::HostWorkRejected)?;
        }
        Ok(())
    };
    runtime
        .runtime
        .reducer
        .space
        .get_data_metered(channel, &reserve)
        .await
        .map_err(|error| invalid(&format!("metered live read failed: {error}")))
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

    pub async fn read_prepaid_receipt_metered(
        &self,
        receipt_id: &[u8; 32],
        maximum_bytes: usize,
        budget: &HostWorkBudget,
    ) -> Result<Option<Vec<u8>>, CasperError> {
        let data = read_live_data_metered(self, &receipt_channel(receipt_id), budget).await?;
        let bytes = decode_receipt_data(&data, maximum_bytes)?;
        if let Some(bytes) = bytes {
            budget
                .reserve(
                    HostWorkDimension::SearchStateBytes,
                    HostWorkUnits::new(
                        u64::try_from(bytes.len())
                            .map_err(|_| invalid("metered receipt copy length overflow"))?,
                    ),
                )
                .map_err(|error| invalid(&error.to_string()))?;
        }
        Ok(bytes.map(<[u8]>::to_vec))
    }

    pub async fn replace_prepaid_receipts(
        &mut self,
        changes: &[PrepaidReceiptChange<'_>],
        limits: PrepaidReceiptLimits,
    ) -> Result<Log, CasperError> {
        self.replace_prepaid_receipts_with_budget(changes, limits, None)
            .await
    }

    pub async fn replace_prepaid_receipts_metered(
        &mut self,
        changes: &[PrepaidReceiptChange<'_>],
        limits: PrepaidReceiptLimits,
        budget: &HostWorkBudget,
    ) -> Result<Log, CasperError> {
        self.replace_prepaid_receipts_with_budget(changes, limits, Some(budget))
            .await
    }

    async fn replace_prepaid_receipts_with_budget(
        &mut self,
        changes: &[PrepaidReceiptChange<'_>],
        limits: PrepaidReceiptLimits,
        budget: Option<&HostWorkBudget>,
    ) -> Result<Log, CasperError> {
        let ordered = ordered_changes(changes, limits)?;
        for change in &ordered {
            let live = if let Some(budget) = budget {
                self.read_prepaid_receipt_metered(&change.receipt_id, limits.value_bytes, budget)
                    .await?
            } else {
                self.read_prepaid_receipt(&change.receipt_id, limits.value_bytes)
                    .await?
            };
            if live.as_deref() != change.expected {
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
