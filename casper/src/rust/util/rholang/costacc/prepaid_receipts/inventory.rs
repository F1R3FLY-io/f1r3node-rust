use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;

use models::rhoapi::CostSignature;
use models::rust::host_work::HostWorkDimension;
use rholang::rust::interpreter::accounting::phlo_execution::{
    PhloExecutionLimits, PhloResource, RestoredPhloResource,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;

use super::snapshot::reserve;
use super::{
    invalid, CapturedPrepaidStacks, CasperError, NativePrepaidCell, NativePrepaidCellLimits,
    OrderedPrepaidCells, PrepaidCellLimits, PrepaidStackPop,
};
use crate::rust::util::rholang::supply::PurseStack;

mod demand;
pub use demand::{
    NativeMeasuredSettlementLimits, NativePrepaidDemandBinding, NativePrepaidDemandInput,
    NativePrepaidDemandLimits,
};

#[derive(Clone, Copy, Debug)]
pub struct NativePrepaidInventoryLimits {
    pub cells: PrepaidCellLimits,
    pub cell: NativePrepaidCellLimits,
    pub execution: PhloExecutionLimits,
}

#[derive(Debug)]
pub struct NativePrepaidResource<'a> {
    receipt: NativePrepaidCell<'a>,
    restored: RestoredPhloResource<'a>,
}

impl NativePrepaidResource<'_> {
    pub fn receipt(&self) -> &NativePrepaidCell<'_> { &self.receipt }
    pub fn resource(&self) -> PhloResource<'_> { self.restored.resource() }
}

#[derive(Debug)]
pub struct NativePrepaidInventory<'s, 'p> {
    captured: &'s CapturedPrepaidStacks<'p>,
    sources: BTreeMap<[u8; 32], Vec<Vec<NativePrepaidResource<'s>>>>,
}

#[derive(Clone, Copy, Debug)]
pub struct NativePrepaidPrefix<'a> {
    stack: &'a PurseStack,
    receipt_index: usize,
    resources: &'a [NativePrepaidResource<'a>],
}

impl<'a> NativePrepaidPrefix<'a> {
    pub fn stack(&self) -> &PurseStack { self.stack }
    pub fn receipt_index(&self) -> usize { self.receipt_index }
    pub fn resources(&self) -> &'a [NativePrepaidResource<'a>] { self.resources }
    pub fn current_authorities(&self) -> &'a [CostSignature] {
        &self.stack.stack.cells[..self.resources.len()]
    }
}

impl<'p> CapturedPrepaidStacks<'p> {
    pub fn resource_inventory<'s>(
        &'s self,
        limits: NativePrepaidInventoryLimits,
        budget: &HostWorkBudget,
    ) -> Result<NativePrepaidInventory<'s, 'p>, CasperError> {
        let mut sources = BTreeMap::new();
        let mut cells_left = limits.cells.cells.min(limits.execution.resource_entries);
        let mut bytes_left = limits.cells.wire.total_bytes;
        let mut keys_left = limits.execution.key_bytes;
        let mut nodes_left = limits.execution.authority_nodes;
        for stack in self.stacks() {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            if sources.contains_key(&stack.source_hash) {
                continue;
            }
            let bucket = self
                .records_for_stack(&self.root(), &stack.instance_id)?
                .ok_or_else(|| invalid("inventory stack has no rooted receipt bucket"))?;
            reserve(
                budget,
                HostWorkDimension::SearchStateBytes,
                bucket
                    .receipts()
                    .len()
                    .checked_mul(size_of::<Vec<NativePrepaidResource<'_>>>())
                    .ok_or_else(|| invalid("inventory receipt size overflow"))?,
            )?;
            let mut records = Vec::new();
            records
                .try_reserve_exact(bucket.receipts().len())
                .map_err(|_| invalid("inventory receipt allocation failed"))?;
            for record in bucket.receipts() {
                bytes_left = bytes_left
                    .checked_sub(record.len())
                    .ok_or_else(|| invalid("inventory aggregate record byte limit exceeded"))?;
                let allocation = record
                    .len()
                    .checked_mul(
                        size_of::<super::NativePrepaidContribution>()
                            + size_of::<models::rust::phlo_resource::PhloAuthorityNode<'_>>()
                            + 2 * size_of::<&[u8]>(),
                    )
                    .ok_or_else(|| invalid("inventory decode allocation overflow"))?;
                reserve(budget, HostWorkDimension::SearchStateBytes, allocation)?;
                reserve(
                    budget,
                    HostWorkDimension::VerificationOperations,
                    record.len(),
                )?;
                let ordered = OrderedPrepaidCells::decode(record, limits.cells)?;
                cells_left = cells_left
                    .checked_sub(ordered.cells().len())
                    .ok_or_else(|| invalid("inventory aggregate cell limit exceeded"))?;
                reserve(
                    budget,
                    HostWorkDimension::SearchStateBytes,
                    ordered
                        .cells()
                        .len()
                        .checked_mul(size_of::<NativePrepaidResource<'_>>())
                        .ok_or_else(|| invalid("inventory cell allocation overflow"))?,
                )?;
                let mut cells = Vec::new();
                cells
                    .try_reserve_exact(ordered.cells().len())
                    .map_err(|_| invalid("inventory cell allocation failed"))?;
                for bytes in ordered.cells() {
                    let receipt = NativePrepaidCell::decode(self.policy(), bytes, limits.cell)?;
                    keys_left = keys_left
                        .checked_sub(receipt.resource_bytes().len())
                        .ok_or_else(|| invalid("inventory encoded-key byte limit exceeded"))?;
                    let restored = RestoredPhloResource::from_wire_key(
                        receipt.resource(),
                        PhloExecutionLimits {
                            authority_nodes: nodes_left,
                            key_bytes: receipt.resource_bytes().len(),
                            ..limits.execution
                        },
                        budget,
                    )
                    .map_err(|error| invalid(&error.to_string()))?;
                    nodes_left = nodes_left
                        .checked_sub(receipt.resource().authority.len())
                        .ok_or_else(|| invalid("inventory authority-node limit exceeded"))?;
                    cells.push(NativePrepaidResource { receipt, restored });
                }
                records.push(cells);
            }
            sources.insert(stack.source_hash, records);
        }
        Ok(NativePrepaidInventory {
            captured: self,
            sources,
        })
    }
}

impl NativePrepaidInventory<'_, '_> {
    pub fn root(&self) -> [u8; 32] { self.captured.root() }

    pub fn records_for_stack(
        &self,
        stack_id: &[u8; 32],
    ) -> Option<&[Vec<NativePrepaidResource<'_>>]> {
        let index = self
            .captured
            .stacks()
            .binary_search_by_key(stack_id, |stack| stack.instance_id)
            .ok()?;
        self.sources
            .get(&self.captured.stacks()[index].source_hash)
            .map(Vec::as_slice)
    }

    pub fn prefixes(
        &self,
        draws: &[PrepaidStackPop],
        maximum_draws: usize,
        budget: &HostWorkBudget,
    ) -> Result<Vec<NativePrepaidPrefix<'_>>, CasperError> {
        if draws.len() > maximum_draws {
            return Err(invalid("inventory selection draw limit exceeded"));
        }
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            draws
                .len()
                .checked_mul(size_of::<NativePrepaidPrefix<'_>>() + 128)
                .ok_or_else(|| invalid("inventory selection allocation overflow"))?,
        )?;
        let mut selected = BTreeSet::new();
        let mut occurrences = BTreeSet::new();
        let mut prefixes = Vec::new();
        prefixes
            .try_reserve_exact(draws.len())
            .map_err(|_| invalid("inventory selection allocation failed"))?;
        for draw in draws {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            if !selected.insert(draw.stack_id) {
                return Err(invalid("inventory selection repeats a physical stack"));
            }
            let index = self
                .captured
                .stacks()
                .binary_search_by_key(&draw.stack_id, |stack| stack.instance_id)
                .map_err(|_| invalid("inventory selection stack is absent from rooted capture"))?;
            let stack = self.captured.stacks()[index];
            if !occurrences.insert((stack.source_hash, draw.receipt_index)) {
                return Err(invalid("inventory selection repeats a receipt occurrence"));
            }
            let record = self
                .sources
                .get(&stack.source_hash)
                .and_then(|records| records.get(draw.receipt_index))
                .ok_or_else(|| invalid("inventory selection receipt occurrence is out of range"))?;
            let count = usize::try_from(draw.count)
                .map_err(|_| invalid("inventory selection count overflow"))?;
            if count == 0 || count > record.len() || record.len() != stack.stack.cells.len() {
                return Err(invalid("inventory selection prefix count is out of range"));
            }
            prefixes.push(NativePrepaidPrefix {
                stack,
                receipt_index: draw.receipt_index,
                resources: &record[..count],
            });
        }
        Ok(prefixes)
    }
}
