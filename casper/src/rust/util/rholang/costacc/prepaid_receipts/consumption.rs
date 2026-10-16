use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::mem::size_of;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rust::phlo_resource::PhloResourceLimits;
use prost::Message;
use rholang::rust::interpreter::accounting::phlo_execution::{
    CanonicalPhloFundingCapture, PhloExecutionLimits, PhloExecutionResources, PhloFailure,
    PhloOutcome, PhloResourceAmount,
};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::*;
use crate::rust::util::event_converter;
use crate::rust::util::rholang::costacc::supply::{decode_purse_inventory, PurseStack};

pub struct NativeWalletSettlement<'a> {
    pub reservation_id: [u8; 32],
    pub fee_address: &'a VaultAddress,
    pub initial_rand: Blake2b512Random,
}

pub struct NativePrepaidConsumption<'a, 'b> {
    pub captured: &'a CapturedPrepaidStacks<'b>,
    pub draws: &'a [PrepaidStackPop],
    pub limits: PrepaidStackPopLimits,
    pub execution: PhloExecutionLimits,
    pub cell: NativePrepaidCellLimits,
    pub physical_cells: usize,
    pub physical_bytes: usize,
}

pub(super) fn used_resources<'a>(
    capture: &CanonicalPhloFundingCapture<'a>,
) -> impl Iterator<Item = PhloResourceAmount<'a>> {
    let family = capture.intent().bound().consent().family();
    let obligations = family.cases()[capture.branch()].obligations;
    let witness = obligations.execution().witness();
    let retain = retains_prepaid_consumption(obligations.outcome());
    let (occurrences, counted) = match witness {
        PhloExecutionResources::Occurrences(witness) => (witness.used, &[][..]),
        PhloExecutionResources::Counted(witness) => (&[][..], witness.used),
    };
    occurrences
        .iter()
        .map(|resource| PhloResourceAmount {
            resource: *resource,
            quantity: 1,
        })
        .chain(counted.iter().copied())
        .filter(move |_| retain)
}

fn retains_prepaid_consumption(outcome: PhloOutcome<'_>) -> bool {
    matches!(outcome, PhloOutcome::Accepted(failures)
        if failures.iter().all(|failure| *failure == PhloFailure::User))
}

impl RuntimeOps {
    pub async fn apply_prepaid_retained_after_wallet<A: Send + Sync>(
        &mut self,
        prepared: PreparedNativeRetainedReceipts<'_, '_, '_, '_, A>,
        consumption: NativePrepaidConsumption<'_, '_>,
        authenticated_funding_root: [u8; 32],
        expected_wallet_root: [u8; 32],
        limits: NativeRetainedSettlementLimits,
    ) -> Result<NativeRetainedSettlement, CasperError> {
        let checked = prepared.births().settlement();
        if checked.snapshot().wallets().pre_state_root() != authenticated_funding_root
            || consumption.captured.root() != authenticated_funding_root
            || consumption.captured.policy().genesis().genesis_root()
                != prepared.births().policy().genesis().genesis_root()
        {
            return Err(invalid(
                "receipt-only settlement belongs to another funding root",
            ));
        }
        if self.runtime.get_root().await.bytes() != expected_wallet_root {
            return Err(invalid("receipt-only settlement has another wallet root"));
        }
        if consumption.limits.receipts.entries != limits.receipts.entries
            || consumption.limits.receipts.value_bytes != limits.receipts.value_bytes
            || consumption.limits.receipts.batch_bytes != limits.receipts.batch_bytes
        {
            return Err(invalid("receipt-only settlement limits differ"));
        }
        let capture = checked.capture().scoped().capture();
        let mut stacks = consumption.check(capture)?;
        let mut sources = BTreeSet::new();
        for stack in &stacks {
            if sources.insert(stack.source_hash) {
                let key = PrepaidReceiptBucket::key_for_source(&stack.source_hash);
                let expected = consumption
                    .captured
                    .receipts()
                    .receipt(&authenticated_funding_root, &key)?
                    .flatten()
                    .ok_or_else(|| invalid("consumption source has no captured receipt"))?;
                if self
                    .read_prepaid_receipt(&key, consumption.limits.receipts.value_bytes)
                    .await?
                    .as_deref()
                    != Some(expected)
                {
                    return Err(invalid("consumption source receipt is stale"));
                }
            }
        }
        prepared
            .check_live_backing(
                self,
                limits.births.physical_cells,
                limits.births.physical_bytes,
            )
            .await?;
        let count = prepared.changes().len();
        if count > limits.receipts.entries {
            return Err(invalid("retained receipt count exceeds the limit"));
        }
        let mut births = Vec::new();
        births
            .try_reserve_exact(count)
            .map_err(|_| invalid("retained receipt reference allocation failed"))?;
        births.extend(prepared.changes());
        let checkpoint = self.runtime.create_soft_checkpoint().await;
        if !checkpoint.log.is_empty() {
            self.runtime.revert_to_soft_checkpoint(checkpoint).await;
            return Err(invalid(
                "receipt-only settlement has unconsumed prior events",
            ));
        }
        let result = async {
            let birth_log = self
                .replace_prepaid_receipts(&births, limits.receipts)
                .await?;
            consumption.rebind_live_indices(self, &mut stacks).await?;
            let pops = self
                .apply_prepaid_stack_pops_with_changes(
                    &stacks,
                    consumption.draws,
                    consumption.limits,
                )
                .await?;
            let mut first = Vec::new();
            first
                .try_reserve_exact(births.len())
                .map_err(|_| invalid("retained receipt reference allocation failed"))?;
            first.extend(births.iter().copied());
            let mut second = Vec::new();
            second
                .try_reserve_exact(pops.changes().len())
                .map_err(|_| invalid("pop receipt reference allocation failed"))?;
            second.extend(
                pops.changes()
                    .iter()
                    .map(OwnedPrepaidReceiptChange::as_change),
            );
            let composed = compose_prepaid_receipt_changes(&first, &second, limits.receipts)?;
            let mut owned = Vec::new();
            owned
                .try_reserve_exact(composed.len())
                .map_err(|_| invalid("composed receipt allocation failed"))?;
            for change in composed {
                owned.push(OwnedPrepaidReceiptChange::copy_from(change)?);
            }
            let pop_log = pops.into_log();
            let log_count = birth_log
                .len()
                .checked_add(pop_log.len())
                .ok_or_else(|| invalid("receipt-only log count overflow"))?;
            let mut log = Vec::new();
            log.try_reserve_exact(log_count)
                .map_err(|_| invalid("receipt-only log allocation failed"))?;
            log.extend(birth_log.into_iter().map(event_converter::to_casper_event));
            log.extend(pop_log.into_iter().map(event_converter::to_casper_event));
            Ok::<_, CasperError>(NativeRetainedSettlement {
                log,
                mergeable: HashMap::new(),
                prepaid_changes: owned,
            })
        }
        .await;
        match result {
            Ok(settlement) => Ok(settlement),
            Err(error) => {
                self.runtime.revert_to_soft_checkpoint(checkpoint).await;
                Err(error)
            }
        }
    }

    pub async fn apply_prepaid_retained_wallet_settlement<A: Send + Sync>(
        &mut self,
        prepared: PreparedNativeRetainedReceipts<'_, '_, '_, '_, A>,
        consumption: NativePrepaidConsumption<'_, '_>,
        wallet: NativeWalletSettlement<'_>,
        limits: NativeRetainedSettlementLimits,
    ) -> Result<NativeRetainedSettlement, CasperError> {
        let checked = prepared.births().settlement();
        let root = checked.snapshot().wallets().pre_state_root();
        checked.check_execution_root(&self.runtime.get_root().await.bytes())?;
        if consumption.captured.root() != root
            || consumption.captured.policy().genesis().genesis_root()
                != prepared.births().policy().genesis().genesis_root()
        {
            return Err(invalid("prepaid consumption belongs to another pre-state"));
        }
        if consumption.limits.receipts.entries != limits.receipts.entries
            || consumption.limits.receipts.value_bytes != limits.receipts.value_bytes
            || consumption.limits.receipts.batch_bytes != limits.receipts.batch_bytes
        {
            return Err(invalid(
                "prepaid receipt limits differ across one settlement",
            ));
        }
        let capture = checked.capture().scoped().capture();
        let mut stacks = consumption.check(capture)?;
        let mut sources = BTreeSet::new();
        for stack in &stacks {
            if sources.insert(stack.source_hash) {
                let key = PrepaidReceiptBucket::key_for_source(&stack.source_hash);
                let expected = consumption
                    .captured
                    .receipts()
                    .receipt(&root, &key)?
                    .flatten()
                    .ok_or_else(|| invalid("consumption source has no captured receipt"))?;
                if self
                    .read_prepaid_receipt(&key, consumption.limits.receipts.value_bytes)
                    .await?
                    .as_deref()
                    != Some(expected)
                {
                    return Err(invalid("consumption source receipt is stale"));
                }
            }
        }
        let checkpoint = self.runtime.create_soft_checkpoint().await;
        let result = async {
            let mut result = self
                .apply_retained_wallet_settlement_internal(
                    prepared,
                    wallet.reservation_id,
                    wallet.fee_address,
                    wallet.initial_rand,
                    limits,
                )
                .await?;
            consumption.rebind_live_indices(self, &mut stacks).await?;
            let pops = self
                .apply_prepaid_stack_pops_with_changes(
                    &stacks,
                    consumption.draws,
                    consumption.limits,
                )
                .await?;
            let mut first = Vec::new();
            first
                .try_reserve_exact(result.prepaid_changes.len())
                .map_err(|_| invalid("composed receipt reference allocation failed"))?;
            first.extend(
                result
                    .prepaid_changes
                    .iter()
                    .map(OwnedPrepaidReceiptChange::as_change),
            );
            let mut second = Vec::new();
            second
                .try_reserve_exact(pops.changes().len())
                .map_err(|_| invalid("composed receipt reference allocation failed"))?;
            second.extend(
                pops.changes()
                    .iter()
                    .map(OwnedPrepaidReceiptChange::as_change),
            );
            let composed = compose_prepaid_receipt_changes(&first, &second, limits.receipts)?;
            let mut owned = Vec::new();
            owned
                .try_reserve_exact(composed.len())
                .map_err(|_| invalid("composed receipt allocation failed"))?;
            for change in composed {
                owned.push(OwnedPrepaidReceiptChange::copy_from(change)?);
            }
            result.prepaid_changes = owned;
            result.log.extend(
                pops.into_log()
                    .into_iter()
                    .map(event_converter::to_casper_event),
            );
            Ok::<_, CasperError>(result)
        }
        .await;
        match result {
            Ok(mut result) => {
                let mut log: Vec<_> = checkpoint
                    .log
                    .into_iter()
                    .map(event_converter::to_casper_event)
                    .collect();
                log.append(&mut result.log);
                result.log = log;
                Ok(result)
            }
            Err(error) => {
                self.runtime.revert_to_soft_checkpoint(checkpoint).await;
                Err(error)
            }
        }
    }
}

impl NativePrepaidConsumption<'_, '_> {
    async fn rebind_live_indices(
        &self,
        runtime: &RuntimeOps,
        stacks: &mut [PurseStack],
    ) -> Result<(), CasperError> {
        let mut inventories = BTreeMap::new();
        let mut bytes_left = self.physical_bytes;
        let mut cells_left = self.physical_cells;
        for stack in stacks {
            let channel_key = stack.channel.encode_to_vec();
            if let std::collections::btree_map::Entry::Vacant(entry) =
                inventories.entry(channel_key.clone())
            {
                let data = super::read_live_data(runtime, &stack.channel).await;
                if data.len() > cells_left {
                    return Err(invalid("consumption live inventory count limit exceeded"));
                }
                for datum in &data {
                    let count = datum
                        .a
                        .cost_stack
                        .as_ref()
                        .map_or(0, |stack| stack.cells.len());
                    cells_left = cells_left
                        .checked_sub(count)
                        .ok_or_else(|| invalid("consumption live cell limit exceeded"))?;
                    let bytes = datum
                        .a
                        .encoded_len()
                        .checked_add(size_of::<PurseStack>())
                        .ok_or_else(|| invalid("consumption live size overflow"))?;
                    bytes_left = bytes_left
                        .checked_sub(bytes)
                        .ok_or_else(|| invalid("consumption live byte limit exceeded"))?;
                }
                let head = stack
                    .stack
                    .cells
                    .first()
                    .ok_or_else(|| invalid("captured consumption stack is empty"))?;
                let inventory = decode_purse_inventory(&data, head)?;
                entry.insert(
                    inventory
                        .stacks
                        .into_iter()
                        .map(|stack| (stack.instance_id, stack))
                        .collect::<BTreeMap<_, _>>(),
                );
            }
            let live = inventories
                .get(&channel_key)
                .and_then(|inventory| inventory.get(&stack.instance_id))
                .ok_or_else(|| invalid("captured consumption identity is no longer live"))?;
            rebind_stack_index(stack, live)?;
        }
        Ok(())
    }

    fn check(
        &self,
        capture: &CanonicalPhloFundingCapture<'_>,
    ) -> Result<Vec<PurseStack>, CasperError> {
        self.check_resources(used_resources(capture))
    }

    pub(in crate::rust::util::rholang::costacc) fn check_resources<'r>(
        &self,
        used: impl IntoIterator<Item = PhloResourceAmount<'r>>,
    ) -> Result<Vec<PurseStack>, CasperError> {
        if self.draws.len() > self.limits.draws {
            return Err(invalid("consumption draw limit exceeded"));
        }
        let mut expected = BTreeMap::<Vec<u8>, u64>::new();
        let mut bytes_left = self.execution.key_bytes;
        for (index, used) in used.into_iter().enumerate() {
            if index >= self.execution.resource_entries {
                return Err(invalid("consumption resource limit exceeded"));
            }
            if used.quantity == 0 {
                return Err(invalid("consumption resource quantity must be positive"));
            }
            let key = used
                .resource
                .wire_key(self.execution)
                .map_err(|e| invalid(&e.to_string()))?
                .encode(PhloResourceLimits {
                    wire: models::rust::phlo_wire::PhloWireLimits {
                        total_bytes: bytes_left,
                        field_bytes: bytes_left,
                    },
                    authority_nodes: self.execution.authority_nodes,
                })
                .map_err(|e| invalid(&e.to_string()))?;
            bytes_left = bytes_left
                .checked_sub(key.len())
                .ok_or_else(|| invalid("consumption key byte limit exceeded"))?;
            let quantity = expected.entry(key).or_default();
            *quantity = quantity
                .checked_add(used.quantity)
                .ok_or_else(|| invalid("consumption quantity overflow"))?;
        }
        let inventory = self
            .captured
            .resource_inventory(NativePrepaidInventoryLimits {
                cells: self.limits.cells,
                cell: self.cell,
                execution: self.execution,
            })?;
        let prefixes = inventory.prefixes(self.draws, self.limits.draws)?;
        let mut stacks = Vec::new();
        stacks
            .try_reserve_exact(self.draws.len())
            .map_err(|_| invalid("consumption allocation failed"))?;
        for prefix in prefixes {
            let stack = prefix.stack();
            for resource in prefix.resources() {
                let quantity = expected
                    .get_mut(resource.receipt().resource_bytes())
                    .ok_or_else(|| invalid("consumed resource is absent from checked execution"))?;
                *quantity = quantity
                    .checked_sub(1)
                    .ok_or_else(|| invalid("consumed resource exceeds checked execution"))?;
            }
            stacks.push(stack.clone());
        }
        if expected.values().any(|quantity| *quantity != 0) {
            return Err(invalid("consumption omits a checked prepaid resource"));
        }
        Ok(stacks)
    }
}

fn rebind_stack_index(captured: &mut PurseStack, live: &PurseStack) -> Result<(), CasperError> {
    if captured.instance_id != live.instance_id
        || captured.source_hash != live.source_hash
        || captured.channel != live.channel
        || captured.random_state != live.random_state
        || captured.persistent != live.persistent
        || captured.stack != live.stack
    {
        return Err(invalid(
            "consumption stack changed beyond its storage index",
        ));
    }
    captured.datum_index = live.datum_index;
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn index_rebinding_preserves_every_other_capture_field(old in 0_i32..1000, new in 0_i32..1000, mutation in 0_u8..7) {
            let captured = PurseStack {
                instance_id: [1; 32], source_hash: [2; 32], channel: Par::default(),
                datum_index: old, random_state: vec![3], persistent: false,
                stack: models::rhoapi::CostStack { cells: vec![models::rhoapi::CostSignature {
                    value: Some(models::rhoapi::cost_signature::Value::Ground(vec![4])),
                }] },
            };
            let mut live = captured.clone();
            live.datum_index = new;
            match mutation {
                1 => live.instance_id[0] ^= 1,
                2 => live.source_hash[0] ^= 1,
                3 => live.channel = rholang::rust::interpreter::rho_type::RhoByteArray::create_par(vec![5]),
                4 => live.random_state.push(5),
                5 => live.persistent = true,
                6 => live.stack.cells.clear(),
                _ => {}
            }
            let mut result = captured.clone();
            prop_assert_eq!(rebind_stack_index(&mut result, &live).is_ok(), mutation == 0);
            if mutation == 0 { prop_assert_eq!(result, live); }
            else { prop_assert_eq!(result, captured); }
        }

        #[test]
        fn only_success_and_user_failure_retain_prepaid_consumption(
            failures in proptest::collection::vec(0_u8..4, 0..64),
            accepted in any::<bool>(),
        ) {
            let expected = accepted && failures.iter().all(|failure| *failure == 0);
            let failures: Vec<_> = failures.into_iter().map(|failure| match failure {
                0 => PhloFailure::User,
                1 => PhloFailure::Platform,
                2 => PhloFailure::Certificate,
                _ => PhloFailure::Unclassified,
            }).collect();
            let outcome = if accepted { PhloOutcome::Accepted(&failures) } else { PhloOutcome::AdmissionRejected };
            prop_assert_eq!(retains_prepaid_consumption(outcome), expected);
        }
    }
}
