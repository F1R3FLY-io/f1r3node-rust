use std::collections::{BTreeMap, BTreeSet};

use models::rhoapi::CostSignature;
use rholang::rust::interpreter::accounting::authority::{
    allocate_physical_settlement, cost_signature_to_sig, stack_branches, AuthorityEvent,
    AuthorityPhysicalEventDraw, AuthorityPhysicalInventory,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    PhloExecutionLimits, PhloResource, RestoredPhloResource,
};

use super::{
    invalid, CapturedPrepaidStacks, CasperError, NativePrepaidCell, NativePrepaidCellLimits,
    OrderedPrepaidCells, PrepaidCellLimits, PrepaidStackPop,
};
use crate::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletSnapshot;
use crate::rust::util::rholang::costacc::supply::{
    verify_purse_stack_pops, PurseStack, VerifiedPurseStackPops,
};

mod demand;
pub use demand::{
    CanonicalPrepaidSelection, NativeMeasuredSettlementLimits, NativePrepaidDemandBinding,
    NativePrepaidDemandInput, NativePrepaidDemandLimits,
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

/// One selected physical trace over the evaluator's original chronological
/// authority events and the authenticated pre-state inventory. It is not a
/// second event stream. A fresh sealed witness is derived from this immutable
/// trace at selection and again after wallet settlement rebinds stack indices.
pub struct CheckedPrepaidPhysicalTrace<'e> {
    pre_state_root: [u8; 32],
    events: &'e [AuthorityEvent<[u8; 32]>],
    signatures: BTreeMap<[u8; 32], CostSignature>,
    inventory: AuthorityPhysicalInventory<[u8; 32]>,
    draws: Vec<AuthorityPhysicalEventDraw<[u8; 32]>>,
}

impl CheckedPrepaidPhysicalTrace<'_> {
    pub fn pre_state_root(&self) -> [u8; 32] { self.pre_state_root }

    pub fn verify_live<'a>(
        &self,
        live_root: [u8; 32],
        stacks: &'a [PurseStack],
    ) -> Result<VerifiedPurseStackPops<'a>, CasperError> {
        verify_purse_stack_pops(
            live_root,
            stacks,
            self.events,
            &self.signatures,
            &self.inventory,
            &self.draws,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NativePrepaidPrefix<'a> {
    stack: &'a PurseStack,
    receipt_index: usize,
    resources: &'a [NativePrepaidResource<'a>],
    current_authorities: &'a [CostSignature],
}

impl<'a> NativePrepaidPrefix<'a> {
    pub fn stack(&self) -> &PurseStack { self.stack }
    pub fn receipt_index(&self) -> usize { self.receipt_index }
    pub fn resources(&self) -> &'a [NativePrepaidResource<'a>] { self.resources }
    pub fn current_authorities(&self) -> &'a [CostSignature] { self.current_authorities }
}

impl<'p> CapturedPrepaidStacks<'p> {
    pub fn resource_inventory<'s>(
        &'s self,
        limits: NativePrepaidInventoryLimits,
    ) -> Result<NativePrepaidInventory<'s, 'p>, CasperError> {
        let mut sources = BTreeMap::new();
        let mut cells_left = limits.cells.cells.min(limits.execution.resource_entries);
        let mut bytes_left = limits.cells.wire.total_bytes;
        let mut keys_left = limits.execution.key_bytes;
        let mut nodes_left = limits.execution.authority_nodes;
        for stack in self.stacks() {
            if sources.contains_key(&stack.source_hash) {
                continue;
            }
            let bucket = self
                .records_for_stack(&self.root(), &stack.instance_id)?
                .ok_or_else(|| invalid("inventory stack has no rooted receipt bucket"))?;
            let mut records = Vec::new();
            records
                .try_reserve_exact(bucket.receipts().len())
                .map_err(|_| invalid("inventory receipt allocation failed"))?;
            for record in bucket.receipts() {
                bytes_left = bytes_left
                    .checked_sub(record.len())
                    .ok_or_else(|| invalid("inventory aggregate record byte limit exceeded"))?;
                let ordered = OrderedPrepaidCells::decode(record, limits.cells)?;
                cells_left = cells_left
                    .checked_sub(ordered.cells().len())
                    .ok_or_else(|| invalid("inventory aggregate cell limit exceeded"))?;
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

    pub fn captured(&self) -> &CapturedPrepaidStacks<'_> { self.captured }

    pub fn has_branched_stacks(&self) -> Result<bool, CasperError> {
        Ok(self
            .captured
            .stacks()
            .iter()
            .any(|stack| !stack.stack.parallel_tail.is_empty()))
    }

    // Changed by the merge of feature/cost-accounted-rho (finding R3-01): the
    // merge spliced `clone_captured_stacks` and `checked_physical_trace` into
    // one function. Both are restored from HEAD without the host-work meter
    // (DR-131).
    pub fn clone_captured_stacks(&self) -> Result<Vec<PurseStack>, CasperError> {
        let count = self.captured.stacks().len();
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(count)
            .map_err(|_| invalid("physical capture clone allocation failed"))?;
        for stack in self.captured.stacks() {
            owned.push((**stack).clone());
        }
        Ok(owned)
    }

    pub fn checked_physical_trace<'e, A>(
        &self,
        wallets: &DirectWalletSnapshot<'_, A>,
        events: &'e [AuthorityEvent<[u8; 32]>],
    ) -> Result<CheckedPrepaidPhysicalTrace<'e>, CasperError> {
        if wallets.pre_state_root() != self.root() {
            return Err(invalid(
                "physical trace wallet and prepaid captures have different roots",
            ));
        }
        let mut inventory = AuthorityPhysicalInventory::default();
        let mut signatures = BTreeMap::new();
        for source in wallets.sources() {
            let custody: [u8; 32] = source
                .source()
                .custody
                .try_into()
                .map_err(|_| invalid("physical wallet custody width"))?;
            let payer = wallets
                .authorization()
                .payers()
                .get(&custody)
                .ok_or_else(|| invalid("physical wallet source has no authenticated payer"))?;
            let lane = cost_signature_to_sig(&payer.signature)
                .map_err(|error| invalid(&error.to_string()))?
                .lane_hash();
            if let Some(existing) = signatures.get(&lane) {
                if existing != &payer.signature {
                    return Err(invalid("physical wallet lane has conflicting signatures"));
                }
            } else {
                signatures.insert(lane, payer.signature.clone());
            }
            inventory
                .insert_balance_lane(lane, custody, source.source().capacity)
                .map_err(|error| invalid(&error.to_string()))?;
        }
        for stack in self.captured.stacks() {
            if inventory
                .stacks
                .insert(stack.instance_id, stack.stack.cells.clone())
                .is_some()
            {
                return Err(invalid("physical capture repeats a rooted stack identity"));
            }
            if !stack.stack.parallel_tail.is_empty() {
                inventory
                    .stack_trees
                    .insert(stack.instance_id, stack.stack.clone());
            }
        }
        let settlement = allocate_physical_settlement(events, &signatures, &inventory)
            .map_err(|error| invalid(&error.to_string()))?;
        Ok(CheckedPrepaidPhysicalTrace {
            pre_state_root: self.root(),
            events,
            signatures,
            inventory,
            draws: settlement.draws,
        })
    }

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
    ) -> Result<Vec<NativePrepaidPrefix<'_>>, CasperError> {
        if draws.len() > maximum_draws {
            return Err(invalid("inventory selection draw limit exceeded"));
        }
        let mut selected = BTreeSet::new();
        let mut occurrences = BTreeSet::new();
        let mut prefixes = Vec::new();
        prefixes
            .try_reserve_exact(draws.len())
            .map_err(|_| invalid("inventory selection allocation failed"))?;
        for draw in draws {
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
                current_authorities: &stack.stack.cells[..count],
            });
        }
        Ok(prefixes)
    }

    /// Bind a branch draw only through a sealed chronological physical witness.
    /// Siblings share one original receipt occurrence, while each selected
    /// branch owns a disjoint contiguous preorder slice of that receipt.
    pub fn prefixes_verified(
        &self,
        draws: &[PrepaidStackPop],
        verified: &VerifiedPurseStackPops<'_>,
        maximum_draws: usize,
    ) -> Result<Vec<NativePrepaidPrefix<'_>>, CasperError> {
        if self.root() != verified.live_root()
            || draws.len() > maximum_draws
            || draws.len() != verified.pops().len()
        {
            return Err(invalid(
                "verified inventory selection has another root or exceeds its draw limit",
            ));
        }
        let mut selected = Vec::new();
        selected
            .try_reserve_exact(draws.len())
            .map_err(|_| invalid("verified inventory marker allocation failed"))?;
        let mut occurrences = Vec::new();
        occurrences
            .try_reserve_exact(draws.len())
            .map_err(|_| invalid("verified inventory occurrence allocation failed"))?;
        let mut prefixes = Vec::new();
        prefixes
            .try_reserve_exact(draws.len())
            .map_err(|_| invalid("verified inventory selection allocation failed"))?;
        for draw in draws {
            if selected.contains(&draw.stack_id) {
                return Err(invalid("verified inventory selection repeats a branch"));
            }
            selected.push(draw.stack_id);
            let count = usize::try_from(draw.count)
                .map_err(|_| invalid("verified inventory selection count overflow"))?;
            if count == 0 || verified.pops().get(&draw.stack_id).copied() != Some(draw.count) {
                return Err(invalid(
                    "verified inventory draw differs from the chronological physical witness",
                ));
            }
            let root_id = verified
                .root_for_branch(&draw.stack_id)
                .ok_or_else(|| invalid("verified inventory branch has no original root"))?;
            let root_index = self
                .captured
                .stacks()
                .binary_search_by_key(&root_id, |stack| stack.instance_id)
                .map_err(|_| invalid("verified inventory root is absent from capture"))?;
            let stack = self.captured.stacks()[root_index];
            let witnessed = verified
                .stacks()
                .iter()
                .find(|candidate| candidate.instance_id == root_id)
                .ok_or_else(|| invalid("verified inventory root is absent from witness"))?;
            if stack.source_hash != witnessed.source_hash || stack.stack != witnessed.stack {
                return Err(invalid(
                    "verified inventory root differs from the captured stack",
                ));
            }
            let branches = stack_branches(root_id, &stack.stack)
                .map_err(|error| invalid(&error.to_string()))?;
            let branch = branches
                .iter()
                .find(|branch| branch.stack_id == draw.stack_id)
                .ok_or_else(|| {
                    invalid("verified inventory branch is absent from the captured tree")
                })?;
            if count > branch.stack.cells.len() {
                return Err(invalid(
                    "verified inventory branch count exceeds its prefix",
                ));
            }
            let total = branches.iter().try_fold(0_usize, |sum, branch| {
                sum.checked_add(branch.stack.cells.len())
                    .ok_or_else(|| invalid("verified inventory transfer length overflow"))
            })?;
            let record = self
                .sources
                .get(&stack.source_hash)
                .and_then(|records| records.get(draw.receipt_index))
                .ok_or_else(|| invalid("verified inventory receipt occurrence is out of range"))?;
            if record.len() != total {
                return Err(invalid(
                    "verified inventory receipt differs from tree preorder",
                ));
            }
            let end = branch
                .transfer_offset
                .checked_add(count)
                .ok_or_else(|| invalid("verified inventory branch offset overflow"))?;
            if let Some((_, source, receipt_index)) = occurrences
                .iter()
                .find(|(candidate, _, _)| *candidate == root_id)
            {
                if *source != stack.source_hash || *receipt_index != draw.receipt_index {
                    return Err(invalid(
                        "verified inventory siblings name different original receipts",
                    ));
                }
            } else {
                occurrences.push((root_id, stack.source_hash, draw.receipt_index));
            }
            prefixes.push(NativePrepaidPrefix {
                stack,
                receipt_index: draw.receipt_index,
                resources: &record[branch.transfer_offset..end],
                current_authorities: &branch.stack.cells[..count],
            });
        }
        Ok(prefixes)
    }
}
