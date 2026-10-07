use std::collections::BTreeMap;
use std::mem::size_of;

use models::rhoapi::{CostSignature, ListParWithRandom, Par};
use models::rust::host_work::HostWorkDimension;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    cost_signature_to_sig_metered, funding_sig_channel_metered, reserve_authority_signature_tree,
    AuthorityStackBirth,
};
use rholang::rust::interpreter::accounting::native_phlo_rules::NativePhloAcquisitionDemand;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::native_reader::{decode_record, NativeLeafKind};
use rspace_plus_plus::rspace::internal::Datum;
use shared::rust::clone_backing::BackingError;

use super::snapshot::{native_read_error, reserve, ReceiptReadMeter};
use super::*;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::costacc::supply::{decode_purse_inventory, PurseStack};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

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

fn reserve_inventory_decode(
    data: &[Datum<ListParWithRandom>],
    budget: &HostWorkBudget,
) -> Result<(), CasperError> {
    let encoded = data.iter().try_fold(0usize, |total, datum| {
        total
            .checked_add(datum.a.encoded_len())
            .ok_or_else(|| invalid("physical inventory decode size overflow"))
    })?;
    let sorting = data
        .len()
        .checked_mul(1 + data.len().checked_ilog2().unwrap_or(0) as usize)
        .and_then(|count| count.checked_mul(128))
        .ok_or_else(|| invalid("physical inventory sorting work overflow"))?;
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        encoded
            .checked_mul(4)
            .and_then(|count| count.checked_add(sorting))
            .ok_or_else(|| invalid("physical inventory decode work overflow"))?,
    )?;
    reserve(
        budget,
        HostWorkDimension::SearchStateBytes,
        encoded
            .checked_mul(8)
            .and_then(|count| {
                data.len()
                    .checked_mul(size_of::<PurseStack>() + 128)
                    .and_then(|overhead| count.checked_add(overhead))
            })
            .ok_or_else(|| invalid("physical inventory decode backing overflow"))?,
    )
}

fn measured_birth_channel(
    head: &CostSignature,
    budget: &HostWorkBudget,
) -> Result<(Vec<u8>, Par), CasperError> {
    let backing = |operations: usize, scanned: usize, allocation: usize| {
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            operations,
        )
        .map_err(|_| BackingError::Rejected)?;
        reserve(budget, HostWorkDimension::VerificationBytes, scanned)
            .map_err(|_| BackingError::Rejected)?;
        reserve(budget, HostWorkDimension::SearchStateBytes, allocation)
            .map_err(|_| BackingError::Rejected)
    };
    let signature = cost_signature_to_sig_metered(head, &backing)
        .map_err(|error| invalid(&error.to_string()))?;
    if !signature.is_funding_former() {
        return Err(invalid("measured birth head is not a funding former"));
    }
    let channel = funding_sig_channel_metered(&signature, &backing)
        .map_err(|error| invalid(&error.to_string()))?;
    let encoded_len = channel.encoded_len();
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        encoded_len
            .checked_mul(2)
            .ok_or_else(|| invalid("measured birth channel work overflows"))?,
    )?;
    reserve(budget, HostWorkDimension::SearchStateBytes, encoded_len)?;
    let mut key = Vec::new();
    key.try_reserve_exact(encoded_len)
        .map_err(|_| invalid("measured birth channel allocation failed"))?;
    channel
        .encode(&mut key)
        .map_err(|_| invalid("measured birth channel encoding failed"))?;
    Ok((key, channel))
}

impl RuntimeManager {
    pub fn read_measured_prepaid_stacks(
        &self,
        pre_state_root: [u8; 32],
        demand: &NativePhloAcquisitionDemand<'_>,
        limits: PrepaidStackCaptureLimits,
        budget: &HostWorkBudget,
    ) -> Result<Vec<PurseStack>, CasperError> {
        self.read_measured_prepaid_stacks_with_births(pre_state_root, demand, &[], limits, budget)
    }

    pub fn read_measured_prepaid_stacks_with_births(
        &self,
        pre_state_root: [u8; 32],
        demand: &NativePhloAcquisitionDemand<'_>,
        births: &[AuthorityStackBirth],
        limits: PrepaidStackCaptureLimits,
        budget: &HostWorkBudget,
    ) -> Result<Vec<PurseStack>, CasperError> {
        let root = Blake2b256Hash::from_bytes(pre_state_root.to_vec());
        if !self
            .history_repo
            .contains_root(&root)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?
        {
            return Err(invalid(
                "measured purse capture requires a registered state root",
            ));
        }
        let mut bytes_left = limits.physical_bytes;
        let mut channels = BTreeMap::<&[u8], (&Par, &CostSignature)>::new();
        for (located, _) in demand.occurrences() {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            let purse = located.purse();
            if !purse.authority().is_funding_former() {
                return Err(invalid("measured purse authority is not a funding former"));
            }
            let key = purse.encoded_channel();
            if let Some((_, previous)) = channels.get(key) {
                if *previous != purse.original_authority() {
                    return Err(invalid("one measured purse channel has conflicting heads"));
                }
                continue;
            }
            if channels.len() >= limits.stacks {
                return Err(invalid("measured purse channel limit exceeded"));
            }
            charge_bytes(
                budget,
                key.len()
                    .checked_add(size_of::<(&Par, &CostSignature)>())
                    .and_then(|bytes| bytes.checked_add(128))
                    .ok_or_else(|| invalid("measured purse channel size overflow"))?,
                &mut bytes_left,
            )?;
            channels.insert(key, (purse.channel(), purse.original_authority()));
        }
        let mut born_channels = BTreeMap::<Vec<u8>, (Par, &CostSignature)>::new();
        for birth in births {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            let head = birth
                .cells
                .first()
                .ok_or_else(|| invalid("measured birth has no stack head"))?;
            let (key, channel) = measured_birth_channel(head, budget)?;
            let comparisons = channels
                .len()
                .checked_add(born_channels.len())
                .and_then(|count| count.checked_add(1))
                .ok_or_else(|| invalid("measured birth channel lookup overflows"))?;
            reserve(
                budget,
                HostWorkDimension::VerificationOperations,
                key.len()
                    .checked_mul(comparisons)
                    .ok_or_else(|| invalid("measured birth channel lookup overflows"))?,
            )?;
            if let Some((_, previous)) = channels.get(key.as_slice()) {
                if *previous != head {
                    return Err(invalid("one measured purse channel has conflicting heads"));
                }
                continue;
            }
            if let Some((_, previous)) = born_channels.get(key.as_slice()) {
                if *previous != head {
                    return Err(invalid("one measured purse channel has conflicting heads"));
                }
                continue;
            }
            if channels
                .len()
                .checked_add(born_channels.len())
                .ok_or_else(|| invalid("measured purse channel count overflow"))?
                >= limits.stacks
            {
                return Err(invalid("measured purse channel limit exceeded"));
            }
            charge_bytes(
                budget,
                key.len()
                    .checked_add(size_of::<(Par, &CostSignature)>())
                    .and_then(|bytes| bytes.checked_add(128))
                    .ok_or_else(|| invalid("measured birth channel size overflow"))?,
                &mut bytes_left,
            )?;
            born_channels.insert(key, (channel, head));
        }
        let reader = self.history_repo.native_history_reader(pre_state_root);
        let meter = ReceiptReadMeter(budget);
        let mut stacks_left = limits.stacks;
        let mut cells_left = limits.physical_cells;
        let mut depth = 0;
        let mut stacks = Vec::new();
        for (channel, head) in channels.into_values().chain(
            born_channels
                .values()
                .map(|(channel, head)| (channel, *head)),
        ) {
            let channel_hash = stable_hash_provider::hash(channel);
            let channel_hash: [u8; 32] = channel_hash
                .0
                .as_slice()
                .try_into()
                .map_err(|_| invalid("measured purse channel hash is invalid"))?;
            let data = reader
                .with_records(NativeLeafKind::Data, &channel_hash, &meter, |records| {
                    if records.len() > stacks_left {
                        return Err(invalid("measured purse stack limit exceeded"));
                    }
                    reserve(
                        budget,
                        HostWorkDimension::SearchStateBytes,
                        records
                            .len()
                            .checked_mul(size_of::<Datum<ListParWithRandom>>())
                            .ok_or_else(|| invalid("measured purse allocation overflow"))?,
                    )?;
                    let mut data: Vec<Datum<ListParWithRandom>> = Vec::new();
                    data.try_reserve_exact(records.len())
                        .map_err(|_| invalid("measured purse allocation failed"))?;
                    let mut raw_left = bytes_left;
                    for raw in records.iter() {
                        raw_left = raw_left
                            .checked_sub(raw.len())
                            .ok_or_else(|| invalid("measured purse byte limit exceeded"))?;
                        data.push(decode_record(raw, &meter).map_err(native_read_error)?);
                    }
                    Ok(data)
                })
                .map_err(native_read_error)?
                .unwrap_or_default();
            stacks_left = stacks_left
                .checked_sub(data.len())
                .ok_or_else(|| invalid("measured purse stack limit exceeded"))?;
            for datum in &data {
                if let Some(stack) = &datum.a.cost_stack {
                    cells_left = cells_left
                        .checked_sub(stack.cells.len())
                        .ok_or_else(|| invalid("measured purse cell limit exceeded"))?;
                    for cell in &stack.cells {
                        reserve_authority_signature_tree(cell, budget, &mut depth)
                            .map_err(|error| invalid(&error.to_string()))?;
                    }
                }
                charge_bytes(
                    budget,
                    datum
                        .a
                        .encoded_len()
                        .checked_add(channel.encoded_len())
                        .and_then(|bytes| bytes.checked_add(size_of::<PurseStack>()))
                        .ok_or_else(|| invalid("measured purse byte size overflow"))?,
                    &mut bytes_left,
                )?;
            }
            reserve_inventory_decode(&data, budget)?;
            let inventory = decode_purse_inventory(&data, head)?;
            stacks
                .try_reserve(inventory.stacks.len())
                .map_err(|_| invalid("measured purse allocation failed"))?;
            stacks.extend(inventory.stacks);
        }
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            stacks
                .len()
                .checked_mul(1 + stacks.len().checked_ilog2().unwrap_or(0) as usize)
                .ok_or_else(|| invalid("measured purse sorting work overflow"))?,
        )?;
        stacks.sort_unstable_by_key(|stack| stack.instance_id);
        if stacks
            .windows(2)
            .any(|pair| pair[0].instance_id == pair[1].instance_id)
        {
            return Err(invalid(
                "measured purse capture has duplicate stack identity",
            ));
        }
        Ok(stacks)
    }

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
        let reader = self.history_repo.native_history_reader(pre_state_root);
        let meter = ReceiptReadMeter(budget);
        let mut sources = BTreeMap::<[u8; 32], (usize, usize)>::new();
        let mut stacks_left = limits.stacks;
        let mut cells_left = limits.physical_cells;
        for (channel_bytes, captures) in &channels {
            let first = captures[0];
            let channel_hash = stable_hash_provider::hash(&first.channel);
            let channel_hash: [u8; 32] = channel_hash
                .0
                .as_slice()
                .try_into()
                .map_err(|_| invalid("physical channel hash is invalid"))?;
            let data = reader
                .with_records(NativeLeafKind::Data, &channel_hash, &meter, |records| {
                    if records.len() > stacks_left {
                        return Err(invalid("physical inventory stack limit exceeded"));
                    }
                    reserve(
                        budget,
                        HostWorkDimension::SearchStateBytes,
                        records
                            .len()
                            .checked_mul(size_of::<Datum<ListParWithRandom>>())
                            .ok_or_else(|| invalid("physical inventory allocation overflow"))?,
                    )?;
                    let mut data: Vec<Datum<ListParWithRandom>> = Vec::new();
                    data.try_reserve_exact(records.len())
                        .map_err(|_| invalid("physical inventory allocation failed"))?;
                    let mut raw_left = bytes_left;
                    for raw in records.iter() {
                        raw_left = raw_left
                            .checked_sub(raw.len())
                            .ok_or_else(|| invalid("physical inventory byte limit exceeded"))?;
                        data.push(decode_record(raw, &meter).map_err(native_read_error)?);
                    }
                    Ok(data)
                })
                .map_err(native_read_error)?
                .unwrap_or_default();
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
            reserve_inventory_decode(&data, budget)?;
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
