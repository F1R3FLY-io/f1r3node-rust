use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use models::rhoapi::{CostSignature, CostStack};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{cost_signature_to_sig, stack_branches};
use rspace_plus_plus::rspace::internal::Datum;

use super::*;
use crate::rust::util::rholang::costacc::supply::{
    self, PlannedPurseStackPublication, PurseStack, VerifiedPurseStackPops,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepaidStackPop {
    pub stack_id: [u8; 32],
    pub receipt_index: usize,
    pub count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepaidStackPopLimits {
    pub draws: usize,
    pub bucket: PrepaidReceiptBucketLimits,
    pub cells: PrepaidCellLimits,
    pub receipts: PrepaidReceiptLimits,
}

pub struct NativePrepaidPopResult {
    log: Log,
    changes: Vec<OwnedPrepaidReceiptChange>,
}

impl NativePrepaidPopResult {
    pub fn changes(&self) -> &[OwnedPrepaidReceiptChange] { &self.changes }
    pub fn into_log(self) -> Log { self.log }
}

struct BucketState {
    original: Option<Vec<u8>>,
    records: Vec<Vec<u8>>,
    channel: Par,
    head: CostSignature,
    physical_counts: Arc<BTreeMap<[u8; 32], usize>>,
}

struct Tail {
    source: [u8; 32],
    channel: Par,
    head: CostSignature,
    record: Vec<u8>,
}

fn copy_bytes(bytes: &[u8]) -> Result<Vec<u8>, CasperError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|_| invalid("record allocation failed"))?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}

/// Partition one original preorder transfer-cell receipt. Each surviving
/// occurrence appears in exactly one new rooted receipt; the remaining count
/// must equal the already verified physical pop count. The returned records
/// are aligned with `publications`, not sorted by source hash.
fn partition_frontier_receipt(
    original: &[&[u8]],
    publications: &[&PlannedPurseStackPublication],
    popped: usize,
    limits: PrepaidCellLimits,
) -> Result<Vec<Vec<u8>>, CasperError> {
    let mut seen = Vec::new();
    seen.try_reserve_exact(original.len())
        .map_err(|_| invalid("frontier marker allocation failed"))?;
    seen.resize(original.len(), false);
    let mut records = Vec::new();
    records
        .try_reserve_exact(publications.len())
        .map_err(|_| invalid("frontier record allocation failed"))?;
    let mut retained = 0usize;
    for publication in publications {
        let indices = &publication.original_transfer_indices;
        if indices.is_empty() || indices.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(invalid("frontier publication indices are not increasing"));
        }
        let mut selected = Vec::new();
        selected
            .try_reserve_exact(indices.len())
            .map_err(|_| invalid("frontier slice allocation failed"))?;
        for &index in indices {
            let cell = original
                .get(index)
                .ok_or_else(|| invalid("frontier receipt index exceeds original record"))?;
            if std::mem::replace(&mut seen[index], true) {
                return Err(invalid("frontier receipt index is duplicated"));
            }
            retained = retained
                .checked_add(1)
                .ok_or_else(|| invalid("frontier retained count overflow"))?;
            selected.push(*cell);
        }
        records.push(OrderedPrepaidCells::encode(&selected, limits)?);
    }
    if retained
        .checked_add(popped)
        .ok_or_else(|| invalid("frontier partition count overflow"))?
        != original.len()
    {
        return Err(invalid(
            "frontier consumed and retained indices do not partition the original receipt",
        ));
    }
    Ok(records)
}

async fn physical_counts(
    runtime: &RuntimeOps,
    channel: &Par,
    head: &CostSignature,
) -> Result<BTreeMap<[u8; 32], usize>, CasperError> {
    let data = runtime
        .runtime
        .reducer
        .space
        .get_data(channel)
        .await
        .map_err(|error| CasperError::InterpreterError(error.into()))?;
    let inventory = supply::decode_purse_inventory(&data, head)?;
    let mut counts = BTreeMap::new();
    for stack in inventory.stacks {
        *counts.entry(stack.source_hash).or_insert(0) += 1;
    }
    Ok(counts)
}

async fn load_bucket(
    runtime: &RuntimeOps,
    states: &mut BTreeMap<[u8; 32], BucketState>,
    source: [u8; 32],
    channel: &Par,
    head: &CostSignature,
    limits: PrepaidStackPopLimits,
    remaining_bytes: &mut usize,
) -> Result<(), CasperError> {
    if states.contains_key(&source) {
        return Ok(());
    }
    if states.len() >= limits.receipts.entries {
        return Err(invalid("stack migration bucket limit exceeded"));
    }
    let counts = match states.values().find(|state| state.channel == *channel) {
        Some(state) => Arc::clone(&state.physical_counts),
        None => Arc::new(physical_counts(runtime, channel, head).await?),
    };
    let count = counts.get(&source).copied().unwrap_or(0);
    let receipt_id = PrepaidReceiptBucket::key_for_source(&source);
    let maximum_bytes = limits.receipts.value_bytes.min(*remaining_bytes);
    let original = runtime
        .read_prepaid_receipt(&receipt_id, maximum_bytes)
        .await?;
    let mut records = Vec::new();
    match original.as_deref() {
        None if count != 0 => return Err(invalid("live stack source has no prepaid provenance")),
        Some(_) if count == 0 => {
            return Err(invalid("prepaid provenance has no live stack source"))
        }
        Some(bytes) => {
            *remaining_bytes = remaining_bytes
                .checked_sub(bytes.len())
                .ok_or_else(|| invalid("stack migration byte limit exceeded"))?;
            let bucket = PrepaidReceiptBucket::decode(bytes, limits.bucket)?;
            bucket.check_occurrences(&source, count)?;
            records
                .try_reserve_exact(count)
                .map_err(|_| invalid("bucket allocation failed"))?;
            for record in bucket.receipts() {
                records.push(copy_bytes(record)?);
            }
        }
        None => {}
    }
    states.insert(source, BucketState {
        original,
        records,
        channel: channel.clone(),
        head: head.clone(),
        physical_counts: counts,
    });
    Ok(())
}

impl RuntimeOps {
    pub async fn apply_prepaid_stack_pops(
        &mut self,
        stacks: &[PurseStack],
        draws: &[PrepaidStackPop],
        limits: PrepaidStackPopLimits,
    ) -> Result<Log, CasperError> {
        self.apply_prepaid_stack_pops_internal(stacks, draws, limits)
            .await
            .map(NativePrepaidPopResult::into_log)
    }

    pub async fn apply_prepaid_stack_pops_with_changes(
        &mut self,
        stacks: &[PurseStack],
        draws: &[PrepaidStackPop],
        limits: PrepaidStackPopLimits,
    ) -> Result<NativePrepaidPopResult, CasperError> {
        self.apply_prepaid_stack_pops_internal(stacks, draws, limits)
            .await
    }

    /// Tree settlement is a separate capability-bearing entry: callers may
    /// not turn raw cumulative child counts into a physical pop. The witness
    /// binds the same captured trees and chronological event draws used here.
    pub async fn apply_prepaid_stack_pops_verified_with_changes(
        &mut self,
        verified: &VerifiedPurseStackPops<'_>,
        draws: &[PrepaidStackPop],
        limits: PrepaidStackPopLimits,
    ) -> Result<NativePrepaidPopResult, CasperError> {
        if draws.len() > limits.draws || verified.stacks().len() > limits.draws {
            return Err(invalid("verified stack migration draw limit exceeded"));
        }
        let mut selected = BTreeMap::<[u8; 32], u64>::new();
        let mut receipt_indices = BTreeMap::<[u8; 32], usize>::new();
        let mut root_popped = BTreeMap::<[u8; 32], usize>::new();
        for draw in draws {
            if draw.count == 0 || selected.insert(draw.stack_id, draw.count).is_some() {
                return Err(invalid("verified stack draw is zero or duplicated"));
            }
            let root = verified
                .root_for_branch(&draw.stack_id)
                .ok_or_else(|| invalid("verified draw names an unknown branch"))?;
            let count =
                usize::try_from(draw.count).map_err(|_| invalid("verified pop count overflow"))?;
            let total = root_popped.entry(root).or_default();
            *total = total
                .checked_add(count)
                .ok_or_else(|| invalid("verified root pop total overflow"))?;
            if receipt_indices
                .insert(root, draw.receipt_index)
                .is_some_and(|old| old != draw.receipt_index)
            {
                return Err(invalid(
                    "branches of one original root select different receipt occurrences",
                ));
            }
        }
        if selected != *verified.pops() {
            return Err(invalid(
                "prepaid branch draws differ from the verified chronological settlement",
            ));
        }
        if draws.is_empty() {
            return Ok(NativePrepaidPopResult {
                log: Vec::new(),
                changes: Vec::new(),
            });
        }
        let plan = supply::plan_verified_purse_stack_pops(
            verified,
            self.runtime.reducer.space.codec_epoch(),
        )?;
        let mut states = BTreeMap::new();
        let mut selections = BTreeSet::new();
        let mut tails = Vec::new();
        tails
            .try_reserve_exact(plan.publications().len())
            .map_err(|_| invalid("verified frontier tail allocation failed"))?;
        let mut remaining_bytes = limits.receipts.batch_bytes;
        let mut remaining_cells = limits.cells.cells;
        for original in plan.originals() {
            let receipt_index = *receipt_indices
                .get(&original.instance_id)
                .ok_or_else(|| invalid("verified root has no selected receipt occurrence"))?;
            let head = original
                .stack
                .cells
                .first()
                .ok_or_else(|| invalid("verified stack root is empty"))?;
            if !selections.insert((original.source_hash, receipt_index)) {
                return Err(invalid(
                    "original receipt occurrence selected more than once",
                ));
            }
            load_bucket(
                self,
                &mut states,
                original.source_hash,
                &original.channel,
                head,
                limits,
                &mut remaining_bytes,
            )
            .await?;
            let state = states
                .get(&original.source_hash)
                .ok_or_else(|| invalid("verified source bucket is absent"))?;
            let record = state
                .records
                .get(receipt_index)
                .ok_or_else(|| invalid("verified receipt occurrence is out of range"))?;
            let cells = OrderedPrepaidCells::decode(record, limits.cells)?;
            let branches = stack_branches(original.instance_id, &original.stack)
                .map_err(|error| invalid(&error.to_string()))?;
            let physical_cells = branches.iter().try_fold(0usize, |count, branch| {
                count
                    .checked_add(branch.stack.cells.len())
                    .ok_or_else(|| invalid("verified physical cell count overflow"))
            })?;
            if cells.cells().len() != physical_cells {
                return Err(invalid(
                    "verified receipt differs from the complete physical tree cell count",
                ));
            }
            remaining_cells = remaining_cells
                .checked_sub(physical_cells)
                .ok_or_else(|| invalid("verified stack aggregate cell limit exceeded"))?;
            let popped = *root_popped
                .get(&original.instance_id)
                .ok_or_else(|| invalid("verified root has no pop total"))?;
            let publication_count = plan
                .publications()
                .iter()
                .filter(|publication| publication.original_root_id == original.instance_id)
                .count();
            let mut publications = Vec::new();
            publications
                .try_reserve_exact(publication_count)
                .map_err(|_| invalid("verified publication reference allocation failed"))?;
            publications.extend(
                plan.publications()
                    .iter()
                    .filter(|publication| publication.original_root_id == original.instance_id),
            );
            let records =
                partition_frontier_receipt(cells.cells(), &publications, popped, limits.cells)?;
            for (publication, record) in publications.into_iter().zip(records) {
                tails.push(Tail {
                    source: publication.source_hash,
                    channel: publication.channel.clone(),
                    head: publication.head().clone(),
                    record,
                });
            }
        }
        for tail in &tails {
            load_bucket(
                self,
                &mut states,
                tail.source,
                &tail.channel,
                &tail.head,
                limits,
                &mut remaining_bytes,
            )
            .await?;
        }
        for (source, index) in selections.into_iter().rev() {
            states
                .get_mut(&source)
                .ok_or_else(|| invalid("verified source bucket is absent"))?
                .records
                .remove(index);
        }
        for tail in tails {
            let records = &mut states
                .get_mut(&tail.source)
                .ok_or_else(|| invalid("verified frontier bucket is absent"))?
                .records;
            if records.len() >= limits.bucket.occurrences {
                return Err(invalid(
                    "verified frontier bucket occurrence limit exceeded",
                ));
            }
            records
                .try_reserve(1)
                .map_err(|_| invalid("verified frontier bucket allocation failed"))?;
            records.push(tail.record);
        }
        let mut replacements = Vec::new();
        replacements
            .try_reserve_exact(states.len())
            .map_err(|_| invalid("verified replacement allocation failed"))?;
        for (source, state) in &states {
            let replacement = if state.records.is_empty() {
                None
            } else {
                let refs = state.records.iter().map(Vec::as_slice).collect::<Vec<_>>();
                Some(
                    PrepaidReceiptBucket::new(*source, &refs, limits.bucket)?
                        .encode(limits.bucket)?,
                )
            };
            replacements.push(replacement);
        }
        let changes = states
            .iter()
            .zip(&replacements)
            .filter_map(|((source, state), replacement)| {
                (state.original != *replacement).then_some(PrepaidReceiptChange {
                    receipt_id: PrepaidReceiptBucket::key_for_source(source),
                    expected: state.original.as_deref(),
                    replacement: replacement.as_deref(),
                })
            })
            .collect::<Vec<_>>();
        ordered_changes(&changes, limits.receipts)?;
        let mut owned_changes = Vec::new();
        owned_changes
            .try_reserve_exact(changes.len())
            .map_err(|_| invalid("verified owned change allocation failed"))?;
        for change in &changes {
            owned_changes.push(OwnedPrepaidReceiptChange::copy_from(*change)?);
        }
        let checkpoint = self.runtime.create_soft_checkpoint().await;
        let result = async {
            supply::apply_verified_stack_plan(self, plan).await?;
            let mut live_counts = BTreeMap::new();
            for (source, state) in &states {
                let key = state.channel.encode_to_vec();
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    live_counts.entry(key.clone())
                {
                    entry.insert(physical_counts(self, &state.channel, &state.head).await?);
                }
                let count = live_counts
                    .get(&key)
                    .and_then(|counts| counts.get(source))
                    .copied()
                    .unwrap_or(0);
                if count != state.records.len() {
                    return Err(invalid(
                        "verified frontier changed physical occurrence count",
                    ));
                }
            }
            let mut log = self
                .replace_prepaid_receipts(&changes, limits.receipts)
                .await?;
            log.extend(self.runtime.take_event_log().await);
            Ok::<_, CasperError>(log)
        }
        .await;
        match result {
            Ok(log) => {
                let mut complete = checkpoint.log;
                complete.extend(log);
                Ok(NativePrepaidPopResult {
                    log: complete,
                    changes: owned_changes,
                })
            }
            Err(error) => {
                self.runtime.revert_to_soft_checkpoint(checkpoint).await;
                Err(error)
            }
        }
    }

    async fn apply_prepaid_stack_pops_internal(
        &mut self,
        stacks: &[PurseStack],
        draws: &[PrepaidStackPop],
        limits: PrepaidStackPopLimits,
    ) -> Result<NativePrepaidPopResult, CasperError> {
        if draws.len() > limits.draws || stacks.len() > limits.draws {
            return Err(invalid("stack migration draw limit exceeded"));
        }
        if draws.is_empty() {
            return Ok(NativePrepaidPopResult {
                log: Vec::new(),
                changes: Vec::new(),
            });
        }
        let by_id = stacks
            .iter()
            .map(|stack| (stack.instance_id, stack))
            .collect::<BTreeMap<_, _>>();
        if by_id.len() != stacks.len() {
            return Err(invalid("duplicate captured stack identity"));
        }
        let mut pops = BTreeMap::new();
        let mut selections = BTreeSet::new();
        let mut states = BTreeMap::new();
        let mut tails = Vec::new();
        tails
            .try_reserve_exact(draws.len())
            .map_err(|_| invalid("tail allocation failed"))?;
        let mut remaining_bytes = limits.receipts.batch_bytes;
        let mut remaining_cells = limits.cells.cells;
        for draw in draws {
            if pops.insert(draw.stack_id, draw.count).is_some() {
                return Err(invalid("stack selected more than once"));
            }
            let stack = by_id
                .get(&draw.stack_id)
                .copied()
                .ok_or_else(|| invalid("unknown captured stack"))?;
            if !stack.stack.parallel_tail.is_empty() {
                return Err(invalid(
                    "branched stack requires a verified event-order prepaid settlement",
                ));
            }
            let head = stack
                .stack
                .cells
                .first()
                .ok_or_else(|| invalid("captured stack is empty"))?;
            if !selections.insert((stack.source_hash, draw.receipt_index)) {
                return Err(invalid("receipt occurrence selected more than once"));
            }
            load_bucket(
                self,
                &mut states,
                stack.source_hash,
                &stack.channel,
                head,
                limits,
                &mut remaining_bytes,
            )
            .await?;
            let state = states
                .get(&stack.source_hash)
                .ok_or_else(|| invalid("source bucket is absent"))?;
            let record = state
                .records
                .get(draw.receipt_index)
                .ok_or_else(|| invalid("receipt occurrence is out of range"))?;
            let cells = OrderedPrepaidCells::decode(record, limits.cells)?;
            if cells.cells().len() != stack.stack.cells.len() {
                return Err(invalid(
                    "provenance cell count differs from the physical stack",
                ));
            }
            remaining_cells = remaining_cells
                .checked_sub(cells.cells().len())
                .ok_or_else(|| invalid("stack migration aggregate cell limit exceeded"))?;
            let count =
                usize::try_from(draw.count).map_err(|_| invalid("stack pop count overflow"))?;
            let (_, remaining) = cells.split_consumed(count)?;
            if !remaining.is_empty() {
                let record = OrderedPrepaidCells::encode(remaining, limits.cells)?;
                let head = stack.stack.cells[count].clone();
                let signature =
                    cost_signature_to_sig(&head).map_err(|e| invalid(&e.to_string()))?;
                let channel = supply::supply_channel(&signature);
                // The predicted receipt key must use the same authenticated
                // codec epoch as the subsequent RSpace produce. In particular,
                // CostV1 omits the branch field while CostV2 includes it.
                let datum = Datum::create_for_epoch(
                    self.runtime.reducer.space.codec_epoch(),
                    &channel,
                    ListParWithRandom {
                        pars: Vec::new(),
                        random_state: stack.random_state.clone(),
                        cost_authority: None,
                        cost_stack: Some(CostStack {
                            cells: stack.stack.cells[count..].to_vec(),
                            parallel_tail: Vec::new(),
                        }),
                    },
                    stack.persistent,
                )
                .map_err(|error| invalid(&error.to_string()))?;
                let source = datum
                    .source
                    .hash
                    .bytes()
                    .try_into()
                    .map_err(|_| invalid("tail source width"))?;
                tails.push(Tail {
                    source,
                    channel,
                    head,
                    record,
                });
            }
        }
        for tail in &tails {
            load_bucket(
                self,
                &mut states,
                tail.source,
                &tail.channel,
                &tail.head,
                limits,
                &mut remaining_bytes,
            )
            .await?;
        }
        for (source, index) in selections.into_iter().rev() {
            states
                .get_mut(&source)
                .ok_or_else(|| invalid("source bucket is absent"))?
                .records
                .remove(index);
        }
        for tail in tails {
            let records = &mut states
                .get_mut(&tail.source)
                .ok_or_else(|| invalid("tail bucket is absent"))?
                .records;
            if records.len() >= limits.bucket.occurrences {
                return Err(invalid("tail bucket occurrence limit exceeded"));
            }
            records
                .try_reserve(1)
                .map_err(|_| invalid("tail bucket allocation failed"))?;
            records.push(tail.record);
        }
        let mut replacements = Vec::new();
        replacements
            .try_reserve_exact(states.len())
            .map_err(|_| invalid("replacement allocation failed"))?;
        for (source, state) in &states {
            let replacement = if state.records.is_empty() {
                None
            } else {
                let refs = state.records.iter().map(Vec::as_slice).collect::<Vec<_>>();
                Some(
                    PrepaidReceiptBucket::new(*source, &refs, limits.bucket)?
                        .encode(limits.bucket)?,
                )
            };
            replacements.push(replacement);
        }
        let changes = states
            .iter()
            .zip(&replacements)
            .filter_map(|((source, state), replacement)| {
                (state.original != *replacement).then_some(PrepaidReceiptChange {
                    receipt_id: PrepaidReceiptBucket::key_for_source(source),
                    expected: state.original.as_deref(),
                    replacement: replacement.as_deref(),
                })
            })
            .collect::<Vec<_>>();
        ordered_changes(&changes, limits.receipts)?;
        let mut owned_changes = Vec::new();
        owned_changes
            .try_reserve_exact(changes.len())
            .map_err(|_| invalid("owned stack-pop change allocation failed"))?;
        for change in &changes {
            owned_changes.push(OwnedPrepaidReceiptChange::copy_from(*change)?);
        }
        let checkpoint = self.runtime.create_soft_checkpoint().await;
        let result = async {
            supply::apply_stack_pops(self, stacks, &pops).await?;
            let mut live_counts = BTreeMap::new();
            for (source, state) in &states {
                let key = state.channel.encode_to_vec();
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    live_counts.entry(key.clone())
                {
                    entry.insert(physical_counts(self, &state.channel, &state.head).await?);
                }
                let count = live_counts
                    .get(&key)
                    .and_then(|counts| counts.get(source))
                    .copied()
                    .unwrap_or(0);
                if count != state.records.len() {
                    return Err(invalid(
                        "stack migration changed the expected physical occurrence count",
                    ));
                }
            }
            let mut log = self
                .replace_prepaid_receipts(&changes, limits.receipts)
                .await?;
            log.extend(self.runtime.take_event_log().await);
            Ok::<_, CasperError>(log)
        }
        .await;
        match result {
            Ok(log) => {
                let mut complete = checkpoint.log;
                complete.extend(log);
                Ok(NativePrepaidPopResult {
                    log: complete,
                    changes: owned_changes,
                })
            }
            Err(error) => {
                self.runtime.revert_to_soft_checkpoint(checkpoint).await;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests;
