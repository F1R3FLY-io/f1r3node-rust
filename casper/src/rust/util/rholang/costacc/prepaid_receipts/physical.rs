use std::collections::BTreeMap;
use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::reserve_authority_signature_tree;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;

use super::snapshot::reserve;
use super::*;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::supply::{decode_purse_inventory, PurseStack};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepaidStackCaptureLimits {
    pub stacks: usize,
    pub physical_cells: usize,
    pub physical_bytes: usize,
    pub bucket: PrepaidReceiptBucketLimits,
    pub cells: PrepaidCellLimits,
    pub native_cell: NativePrepaidCellLimits,
    pub receipts: PrepaidReceiptLimits,
}

#[derive(Debug)]
pub struct CapturedPrepaidStacks<'a> {
    policy: &'a AdoptedResourcePolicy,
    stacks: Vec<&'a PurseStack>,
    receipts: PrepaidReceiptSnapshot,
    bucket_limits: PrepaidReceiptBucketLimits,
}

impl<'a> CapturedPrepaidStacks<'a> {
    pub fn policy(&self) -> &'a AdoptedResourcePolicy { self.policy }

    pub fn root(&self) -> [u8; 32] { self.receipts.root() }

    pub fn stacks(&self) -> &[&'a PurseStack] { &self.stacks }

    pub(super) fn receipts(&self) -> &PrepaidReceiptSnapshot { &self.receipts }

    pub fn records_for_stack(
        &self,
        expected_root: &[u8; 32],
        stack_id: &[u8; 32],
    ) -> Result<Option<PrepaidReceiptBucket<'_>>, CasperError> {
        if *expected_root != self.root() {
            return Err(invalid(
                "physical receipt capture belongs to another state root",
            ));
        }
        let Ok(index) = self
            .stacks
            .binary_search_by_key(stack_id, |stack| stack.instance_id)
        else {
            return Ok(None);
        };
        let key = PrepaidReceiptBucket::key_for_source(&self.stacks[index].source_hash);
        let bytes = self
            .receipts
            .receipt(expected_root, &key)?
            .flatten()
            .ok_or_else(|| invalid("captured physical source has no receipt"))?;
        Ok(Some(PrepaidReceiptBucket::decode(
            bytes,
            self.bucket_limits,
        )?))
    }
}

fn charge_bytes(
    budget: &HostWorkBudget,
    bytes: usize,
    remaining: &mut usize,
) -> Result<(), CasperError> {
    *remaining = remaining
        .checked_sub(bytes)
        .ok_or_else(|| invalid("physical capture byte limit exceeded"))?;
    reserve(budget, HostWorkDimension::VerificationOperations, bytes)?;
    reserve(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    Ok(())
}

impl RuntimeManager {
    pub fn capture_prepaid_stacks<'a>(
        &self,
        pre_state_root: [u8; 32],
        selected: &'a [PurseStack],
        policy: &'a AdoptedResourcePolicy,
        limits: PrepaidStackCaptureLimits,
        budget: &HostWorkBudget,
    ) -> Result<CapturedPrepaidStacks<'a>, CasperError> {
        if selected.len() > limits.stacks {
            return Err(invalid("physical capture stack limit exceeded"));
        }
        let mut bytes_left = limits.physical_bytes;
        let mut depth = 0;
        let mut channels = BTreeMap::<Vec<u8>, Vec<&PurseStack>>::new();
        let mut ordered = Vec::new();
        charge_bytes(
            budget,
            selected
                .len()
                .checked_mul(size_of::<&PurseStack>())
                .ok_or_else(|| invalid("physical capture size overflow"))?,
            &mut bytes_left,
        )?;
        ordered
            .try_reserve_exact(selected.len())
            .map_err(|_| invalid("physical capture allocation failed"))?;
        for stack in selected {
            if stack.stack.cells.is_empty() || stack.stack.cells.len() > limits.physical_cells {
                return Err(invalid("physical capture cell count is out of range"));
            }
            for cell in &stack.stack.cells {
                reserve_authority_signature_tree(cell, budget, &mut depth)
                    .map_err(|e| invalid(&e.to_string()))?;
            }
            let bytes = stack
                .channel
                .encoded_len()
                .checked_add(stack.stack.encoded_len())
                .and_then(|n| n.checked_add(stack.random_state.len()))
                .and_then(|n| n.checked_add(size_of::<PurseStack>()))
                .ok_or_else(|| invalid("physical capture size overflow"))?;
            charge_bytes(budget, bytes, &mut bytes_left)?;
            channels
                .entry(stack.channel.encode_to_vec())
                .or_default()
                .push(stack);
            ordered.push(stack);
        }
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            ordered
                .len()
                .checked_mul(1 + ordered.len().checked_ilog2().unwrap_or(0) as usize)
                .ok_or_else(|| invalid("physical sorting work overflow"))?,
        )?;
        ordered.sort_unstable_by_key(|stack| stack.instance_id);
        if ordered
            .windows(2)
            .any(|pair| pair[0].instance_id == pair[1].instance_id)
        {
            return Err(invalid("duplicate physical capture identity"));
        }
        let root = Blake2b256Hash::from_bytes(pre_state_root.to_vec());
        if !self
            .history_repo
            .contains_root(&root)
            .map_err(|e| CasperError::RuntimeError(e.to_string()))?
        {
            return Err(CasperError::RuntimeError(
                "physical capture requires a registered state root".into(),
            ));
        }
        let reader = self
            .history_repo
            .get_history_reader(&root)
            .map_err(|e| CasperError::RuntimeError(e.to_string()))?;
        if reader.root() != root {
            return Err(CasperError::RuntimeError(
                "physical capture reader returned another root".into(),
            ));
        }
        let mut sources = BTreeMap::<[u8; 32], (usize, usize)>::new();
        let mut stacks_left = limits.stacks;
        let mut cells_left = limits.physical_cells;
        for (channel_bytes, captures) in &channels {
            let first = captures[0];
            let data = reader
                .get_data(&stable_hash_provider::hash(&first.channel))
                .map_err(|e| CasperError::RuntimeError(e.to_string()))?;
            stacks_left = stacks_left
                .checked_sub(data.len())
                .ok_or_else(|| invalid("physical inventory stack limit exceeded"))?;
            for datum in &data {
                if let Some(stack) = &datum.a.cost_stack {
                    cells_left = cells_left
                        .checked_sub(stack.cells.len())
                        .ok_or_else(|| invalid("physical inventory cell limit exceeded"))?;
                    for cell in &stack.cells {
                        reserve_authority_signature_tree(cell, budget, &mut depth)
                            .map_err(|e| invalid(&e.to_string()))?;
                    }
                }
                charge_bytes(
                    budget,
                    datum
                        .a
                        .encoded_len()
                        .checked_add(channel_bytes.len())
                        .and_then(|n| n.checked_add(size_of::<PurseStack>()))
                        .ok_or_else(|| invalid("physical inventory size overflow"))?,
                    &mut bytes_left,
                )?;
            }
            let inventory = decode_purse_inventory(&data, &first.stack.cells[0])?;
            let by_index = inventory
                .stacks
                .iter()
                .map(|stack| (stack.datum_index, stack))
                .collect::<BTreeMap<_, _>>();
            let mut counts = BTreeMap::<[u8; 32], (usize, usize)>::new();
            for stack in &inventory.stacks {
                let entry = counts
                    .entry(stack.source_hash)
                    .or_insert((0, stack.stack.cells.len()));
                entry.0 += 1;
                if entry.1 != stack.stack.cells.len() {
                    return Err(invalid("one physical source has conflicting cell counts"));
                }
            }
            for captured in captures {
                if by_index.get(&captured.datum_index).copied() != Some(*captured) {
                    return Err(invalid(
                        "physical capture differs from the complete rooted inventory",
                    ));
                }
                let count = counts[&captured.source_hash];
                if sources
                    .insert(captured.source_hash, count)
                    .is_some_and(|previous| previous != count)
                {
                    return Err(invalid("conflicting physical source captures"));
                }
            }
        }
        if reader.root() != root {
            return Err(CasperError::RuntimeError(
                "physical capture reader changed its root".into(),
            ));
        }
        drop(reader);
        if sources.len() > limits.receipts.entries {
            return Err(invalid("physical capture receipt limit exceeded"));
        }
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            sources
                .len()
                .checked_mul(size_of::<[u8; 32]>())
                .ok_or_else(|| invalid("physical receipt key size overflow"))?,
        )?;
        let mut keys = Vec::new();
        keys.try_reserve_exact(sources.len())
            .map_err(|_| invalid("physical receipt key allocation failed"))?;
        keys.extend(sources.keys().map(PrepaidReceiptBucket::key_for_source));
        let receipts =
            self.capture_prepaid_receipts(pre_state_root, &keys, limits.receipts, budget)?;
        let mut record_cells_left = limits.physical_cells;
        for (source, (occurrences, cells)) in sources {
            let key = PrepaidReceiptBucket::key_for_source(&source);
            let bytes = receipts
                .receipt(&pre_state_root, &key)?
                .flatten()
                .ok_or_else(|| invalid("physical source has no prepaid receipt"))?;
            let decode_memory = bytes
                .len()
                .checked_mul(
                    size_of::<models::rust::phlo_resource::PhloAuthorityNode<'_>>()
                        + size_of::<NativePrepaidContribution>()
                        + 2 * size_of::<&[u8]>(),
                )
                .ok_or_else(|| invalid("physical receipt decode size overflow"))?;
            reserve(budget, HostWorkDimension::SearchStateBytes, decode_memory)?;
            reserve(
                budget,
                HostWorkDimension::VerificationOperations,
                decode_memory,
            )?;
            let bucket = PrepaidReceiptBucket::decode(bytes, limits.bucket)?;
            bucket.check_occurrences(&source, occurrences)?;
            for record in bucket.receipts() {
                let ordered_cells = OrderedPrepaidCells::decode(record, limits.cells)?;
                if ordered_cells.cells().len() != cells {
                    return Err(invalid(
                        "prepaid record differs from the complete physical cell count",
                    ));
                }
                record_cells_left = record_cells_left
                    .checked_sub(cells)
                    .ok_or_else(|| invalid("physical receipt cell limit exceeded"))?;
                for cell in ordered_cells.cells() {
                    NativePrepaidCell::decode(policy, cell, limits.native_cell)?;
                }
            }
        }
        Ok(CapturedPrepaidStacks {
            policy,
            stacks: ordered,
            receipts,
            bucket_limits: limits.bucket,
        })
    }
}
