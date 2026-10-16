use std::collections::BTreeSet;

use prost::Message;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloAcquisitionDemand, NativePhloLocatedDemand,
};
use rholang::rust::interpreter::accounting::phlo_controls::CheckedPhloControls;
use rholang::rust::interpreter::accounting::phlo_execution::{
    bind_measured_phlo_prepaid, check_counted_phlo_execution, CountedPhloExecutionWitness,
    MeasuredPhloPrepaidUse, PhloCaptureLimits, PhloOutcome, PhloOutcomeMatchLimits,
    PhloResourceAmount, PreparedMeasuredPhloDemand,
};

use super::*;
use crate::rust::util::rholang::costacc::direct_wallet_funding::{
    CheckedDirectWalletPolicy, CheckedDirectWalletSettlement,
};

#[derive(Clone, Copy, Debug)]
pub struct NativePrepaidDemandLimits {
    pub draws: usize,
    pub authority_bytes: usize,
    pub execution: PhloExecutionLimits,
}

pub struct NativePrepaidDemandInput<'a> {
    pub expected_root: [u8; 32],
    pub controls: CheckedPhloControls<'a>,
    pub demand: &'a NativePhloAcquisitionDemand<'a>,
    pub draws: &'a [PrepaidStackPop],
    pub demand_positions: &'a [usize],
}

#[derive(Debug)]
pub struct NativePrepaidDemandBinding<'a> {
    root: [u8; 32],
    controls: CheckedPhloControls<'a>,
    draws: &'a [PrepaidStackPop],
    prepared: PreparedMeasuredPhloDemand<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalPrepaidSelection {
    draws: Vec<PrepaidStackPop>,
    demand_positions: Vec<usize>,
}

impl CanonicalPrepaidSelection {
    pub fn draws(&self) -> &[PrepaidStackPop] { &self.draws }
    pub fn demand_positions(&self) -> &[usize] { &self.demand_positions }
}

#[derive(Clone, Copy, Debug)]
pub struct NativeMeasuredSettlementLimits {
    pub matching: PhloOutcomeMatchLimits,
    pub capture: PhloCaptureLimits,
}

impl NativePrepaidDemandBinding<'_> {
    pub fn root(&self) -> [u8; 32] { self.root }
    pub fn controls(&self) -> CheckedPhloControls<'_> { self.controls }
    pub fn draws(&self) -> &[PrepaidStackPop] { self.draws }
    pub fn witness(&self) -> CountedPhloExecutionWitness<'_> { self.prepared.witness() }

    pub fn capture_settlement<'a, A>(
        &self,
        policy: &CheckedDirectWalletPolicy<'a, A>,
        retained: &[PhloResourceAmount<'_>],
        outcome: PhloOutcome<'_>,
        limits: NativeMeasuredSettlementLimits,
    ) -> Result<CheckedDirectWalletSettlement<'a, A>, CasperError> {
        check_wallet_root(&policy.snapshot().wallets().pre_state_root(), &self.root)?;
        let observed =
            check_counted_phlo_execution(self.controls, self.witness(), limits.matching.execution)
                .and_then(|execution| execution.with_retained_acquisitions(retained))
                .map_err(|error| invalid(&error.to_string()))?;
        policy
            .capture_settlement(observed, outcome, limits.matching, limits.capture)
            .map_err(|error| invalid(&error.to_string()))
    }
}

fn check_wallet_root(wallet_root: &[u8], prepaid_root: &[u8; 32]) -> Result<(), CasperError> {
    if wallet_root != prepaid_root {
        return Err(invalid(
            "wallet and measured prepaid captures have different state roots",
        ));
    }
    Ok(())
}

/// Added by H1 (bug 11420, DR-126): one captured stack as the selection reads
/// it. It holds the source and the instance of the stack, the current
/// authorities of its cells and the receipt records of its source. `records`
/// is `None` when the inventory has no records for the source. The selection
/// then fails at that stack, as before.
struct SelectionStack<'r, Cell, Current> {
    source: [u8; 32],
    instance: [u8; 32],
    current: &'r [Current],
    records: Option<&'r [Vec<Cell>]>,
}

/// Added by H1 (bug 11420, DR-126): the canonical prepaid selection, moved
/// from `select_measured_demand`.
/// - Each stack, in captured order, keeps the first record of its source with
///   the longest funded prefix.
/// - A record that an earlier stack drew stays out, and so does a record
///   whose length differs from the stack.
/// - In a record, cell j takes the first free occurrence above the occurrence
///   of cell j - 1 that `matches` accepts. A miss ends the prefix.
///
/// So the positions of each draw strictly increase, as the canonical check of
/// the evidence requires (G3 design v5 section 1.1). `matches` charges its
/// own comparison work. Each position that the scan reads costs one search
/// candidate.
fn select_prefixes<'r, Cell: 'r, Current: 'r, Occurrence>(
    stacks: impl IntoIterator<Item = SelectionStack<'r, Cell, Current>>,
    occurrences: &[Occurrence],
    draw_limit: usize,
    mut matches: impl FnMut(&Cell, &Current, &Occurrence) -> Result<bool, CasperError>,
) -> Result<CanonicalPrepaidSelection, CasperError> {
    let count = occurrences.len();
    let mut used = Vec::new();
    used.try_reserve_exact(count)
        .map_err(|_| invalid("prepaid selection marker allocation failed"))?;
    used.resize(count, false);
    let mut candidate_used = Vec::new();
    candidate_used
        .try_reserve_exact(count)
        .map_err(|_| invalid("prepaid selection candidate marker allocation failed"))?;
    candidate_used.resize(count, false);
    let mut candidate_positions = Vec::new();
    candidate_positions
        .try_reserve_exact(count)
        .map_err(|_| invalid("prepaid selection candidate allocation failed"))?;
    let mut best_positions = Vec::new();
    best_positions
        .try_reserve_exact(count)
        .map_err(|_| invalid("prepaid selection best allocation failed"))?;
    let mut draws = Vec::new();
    draws
        .try_reserve_exact(draw_limit)
        .map_err(|_| invalid("prepaid selection draw allocation failed"))?;
    let mut demand_positions = Vec::new();
    demand_positions
        .try_reserve_exact(count)
        .map_err(|_| invalid("prepaid selection position allocation failed"))?;
    let mut selected_receipts = BTreeSet::new();
    for stack in stacks {
        let records = stack
            .records
            .ok_or_else(|| invalid("captured prepaid stack has no rooted receipt inventory"))?;
        let mut best_index = None;
        best_positions.clear();
        for (receipt_index, record) in records.iter().enumerate() {
            if selected_receipts.contains(&(stack.source, receipt_index)) {
                continue;
            }
            if record.len() != stack.current.len() {
                continue;
            }
            candidate_positions.clear();
            // Added by H1 (bug 11420, DR-126): the position after the previous
            // cell of this record.
            let mut floor = 0;
            for (cell, current) in record.iter().zip(stack.current) {
                let mut matched = None;
                // Changed by H1 (bug 11420, DR-126): the scan starts at `floor`,
                // so the positions of a prefix strictly increase.
                // for (position, occurrence) in occurrences.iter().enumerate() {
                for (position, occurrence) in occurrences.iter().enumerate().skip(floor) {
                    if used[position] || candidate_used[position] {
                        continue;
                    }
                    if matches(cell, current, occurrence)? {
                        matched = Some(position);
                        break;
                    }
                }
                let Some(position) = matched else { break };
                candidate_used[position] = true;
                candidate_positions.push(position);
                // Added by H1 (bug 11420, DR-126).
                floor = position + 1;
            }
            if candidate_positions.len() > best_positions.len() {
                best_index = Some(receipt_index);
                best_positions.clear();
                best_positions.extend_from_slice(&candidate_positions);
            }
            for position in candidate_positions.drain(..) {
                candidate_used[position] = false;
            }
        }
        let Some(receipt_index) = best_index else {
            continue;
        };
        if draws.len() >= draw_limit {
            return Err(invalid(
                "canonical prepaid selection exceeds its draw limit",
            ));
        }
        let count = u64::try_from(best_positions.len())
            .map_err(|_| invalid("prepaid selected prefix count overflow"))?;
        selected_receipts.insert((stack.source, receipt_index));
        draws.push(PrepaidStackPop {
            stack_id: stack.instance,
            receipt_index,
            count,
        });
        for position in best_positions.drain(..) {
            used[position] = true;
            demand_positions.push(position);
        }
    }
    Ok(CanonicalPrepaidSelection {
        draws,
        demand_positions,
    })
}

impl NativePrepaidInventory<'_, '_> {
    pub fn select_measured_demand(
        &self,
        demand: &NativePhloAcquisitionDemand<'_>,
        limits: NativePrepaidDemandLimits,
    ) -> Result<CanonicalPrepaidSelection, CasperError> {
        // Changed by C8 (DR-87): the entry limit applies to the distinct
        // demand keys, and the selection walks the located occurrences.
        // let count = demand.resources().len();
        // if count > limits.execution.resource_entries {
        if demand.resources().len() > limits.execution.resource_entries {
            return Err(invalid("measured prepaid demand entry limit exceeded"));
        }
        let count = demand.occurrence_count();
        let mut origins = Vec::new();
        origins
            .try_reserve_exact(count)
            .map_err(|_| invalid("prepaid selection origin allocation failed"))?;
        origins.extend(demand.occurrences());
        // Moved by H1 (bug 11420, DR-126) to `select_prefixes` in
        // `casper/src/rust/util/rholang/costacc/prepaid_receipts/inventory/demand.rs`,
        // with the allocations that its scan uses. Each cell there searches
        // only above the position of the previous cell.
        // let mut used = Vec::new();
        // used.try_reserve_exact(count)
        //     .map_err(|_| invalid("prepaid selection marker allocation failed"))?;
        // used.resize(count, false);
        // let mut candidate_used = Vec::new();
        // candidate_used
        //     .try_reserve_exact(count)
        //     .map_err(|_| invalid("prepaid selection candidate marker allocation failed"))?;
        // candidate_used.resize(count, false);
        // let mut candidate_positions = Vec::new();
        // candidate_positions
        //     .try_reserve_exact(count)
        //     .map_err(|_| invalid("prepaid selection candidate allocation failed"))?;
        // let mut best_positions = Vec::new();
        // best_positions
        //     .try_reserve_exact(count)
        //     .map_err(|_| invalid("prepaid selection best allocation failed"))?;
        // let mut draws = Vec::new();
        // draws
        //     .try_reserve_exact(limits.draws)
        //     .map_err(|_| invalid("prepaid selection draw allocation failed"))?;
        // let mut demand_positions = Vec::new();
        // demand_positions
        //     .try_reserve_exact(count)
        //     .map_err(|_| invalid("prepaid selection position allocation failed"))?;
        // let mut selected_receipts = BTreeSet::new();
        // for stack in self.captured.stacks() {
        //     let records = self
        //         .sources
        //         .get(&stack.source_hash)
        //         .ok_or_else(|| invalid("captured prepaid stack has no rooted receipt inventory"))?;
        //     let mut best_index = None;
        //     best_positions.clear();
        //     for (receipt_index, record) in records.iter().enumerate() {
        //         reserve(budget, HostWorkDimension::SearchCandidates, 1)?;
        //         if selected_receipts.contains(&(stack.source_hash, receipt_index)) {
        //             continue;
        //         }
        //         if record.len() != stack.stack.cells.len() {
        //             continue;
        //         }
        //         candidate_positions.clear();
        //         for (cell, current) in record.iter().zip(&stack.stack.cells) {
        //             let mut matched = None;
        //             for (position, (origin, measured)) in origins.iter().enumerate() {
        //                 reserve(budget, HostWorkDimension::SearchCandidates, 1)?;
        //                 if used[position] || candidate_used[position] {
        //                     continue;
        //                 }
        //                 let bytes = cell
        //                     .receipt()
        //                     .resource_bytes()
        //                     .len()
        //                     .checked_add(current.encoded_len())
        //                     .and_then(|bytes| bytes.checked_add(measured.resource.location.len()))
        //                     .and_then(|bytes| {
        //                         bytes.checked_add(measured.resource.acquisition_terms.len())
        //                     })
        //                     .ok_or_else(|| invalid("prepaid comparison work overflow"))?;
        //                 reserve(budget, HostWorkDimension::VerificationBytes, bytes)?;
        //                 reserve(budget, HostWorkDimension::VerificationOperations, bytes)?;
        //                 if cell.resource() == measured.resource
        //                     && current == origin.purse().original_authority()
        //                 {
        //                     matched = Some(position);
        //                     break;
        //                 }
        //             }
        //             let Some(position) = matched else { break };
        //             candidate_used[position] = true;
        //             candidate_positions.push(position);
        //         }
        //         if candidate_positions.len() > best_positions.len() {
        //             best_index = Some(receipt_index);
        //             best_positions.clear();
        //             best_positions.extend_from_slice(&candidate_positions);
        //         }
        //         for position in candidate_positions.drain(..) {
        //             candidate_used[position] = false;
        //         }
        //     }
        //     let Some(receipt_index) = best_index else {
        //         continue;
        //     };
        //     if draws.len() >= limits.draws {
        //         return Err(invalid(
        //             "canonical prepaid selection exceeds its draw limit",
        //         ));
        //     }
        //     let count = u64::try_from(best_positions.len())
        //         .map_err(|_| invalid("prepaid selected prefix count overflow"))?;
        //     reserve(
        //         budget,
        //         HostWorkDimension::SearchStateBytes,
        //         size_of::<([u8; 32], usize)>() + 64,
        //     )?;
        //     selected_receipts.insert((stack.source_hash, receipt_index));
        //     draws.push(PrepaidStackPop {
        //         stack_id: stack.instance_id,
        //         receipt_index,
        //         count,
        //     });
        //     for position in best_positions.drain(..) {
        //         used[position] = true;
        //         demand_positions.push(position);
        //     }
        // }
        // Ok(CanonicalPrepaidSelection {
        //     draws,
        //     demand_positions,
        // })
        select_prefixes(
            self.captured
                .stacks()
                .iter()
                .map(|stack| SelectionStack {
                    source: stack.source_hash,
                    instance: stack.instance_id,
                    current: &stack.stack.cells,
                    records: self.sources.get(&stack.source_hash).map(Vec::as_slice),
                }),
            &origins,
            limits.draws,
            |cell: &NativePrepaidResource<'_>,
             current: &CostSignature,
             (origin, measured): &(NativePhloLocatedDemand<'_>, PhloResourceAmount<'_>)| {
                Ok(cell.resource() == measured.resource
                    && current == origin.purse().original_authority())
            },
        )
    }

    pub fn bind_measured_demand<'a>(
        &'a self,
        input: NativePrepaidDemandInput<'a>,
        limits: NativePrepaidDemandLimits,
    ) -> Result<NativePrepaidDemandBinding<'a>, CasperError> {
        if input.expected_root != self.root() {
            return Err(invalid(
                "measured demand refers to another prepaid state root",
            ));
        }
        if input.demand_positions.len() > limits.execution.resource_entries
            || input.demand.resources().len() > limits.execution.resource_entries
        {
            return Err(invalid("measured prepaid demand entry limit exceeded"));
        }
        self.captured
            .policy()
            .check_measured_acquisition(input.controls, input.demand)?;
        let prefixes = self.prefixes(input.draws, limits.draws)?;
        let count = prefixes.iter().try_fold(0_usize, |count, prefix| {
            count
                .checked_add(prefix.resources().len())
                .ok_or_else(|| invalid("measured prepaid cell count overflow"))
        })?;
        if count != input.demand_positions.len() {
            return Err(invalid(
                "each selected prepaid cell requires one demand position",
            ));
        }
        // Changed by C8 (DR-87): the origins and the remaining quantity of
        // each located occurrence are sized by the occurrence count, because
        // the demand entries are aggregated.
        // reserve(
        //     budget,
        //     HostWorkDimension::SearchStateBytes,
        //     count
        //         .checked_mul(size_of::<MeasuredPhloPrepaidUse<'_>>())
        //         .and_then(|size| {
        //             input
        //                 .demand
        //                 .resources()
        //                 .len()
        //                 .checked_mul(size_of::<NativePhloLocatedDemand<'_>>())
        //                 .and_then(|origins| size.checked_add(origins))
        //         })
        //         .ok_or_else(|| invalid("measured prepaid allocation overflow"))?,
        // )?;
        // let mut origins = Vec::new();
        // origins
        //     .try_reserve_exact(input.demand.resources().len())
        //     .map_err(|_| invalid("measured prepaid occurrence allocation failed"))?;
        // for (origin, _) in input.demand.occurrences() {
        //     reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
        //     origins.push(origin);
        // }
        let occurrences = input.demand.occurrence_count();
        let mut origins = Vec::new();
        origins
            .try_reserve_exact(occurrences)
            .map_err(|_| invalid("measured prepaid occurrence allocation failed"))?;
        let mut remaining = Vec::new();
        remaining
            .try_reserve_exact(occurrences)
            .map_err(|_| invalid("measured prepaid occurrence allocation failed"))?;
        for (origin, amount) in input.demand.occurrences() {
            origins.push(origin);
            remaining.push(amount.quantity);
        }
        let mut assignments = Vec::new();
        assignments
            .try_reserve_exact(count)
            .map_err(|_| invalid("measured prepaid assignment allocation failed"))?;
        let mut positions = input.demand_positions.iter();
        let mut authority_bytes = limits.authority_bytes;
        for prefix in &prefixes {
            for (cell, current) in prefix.resources().iter().zip(prefix.current_authorities()) {
                let index = *positions
                    .next()
                    .ok_or_else(|| invalid("missing prepaid demand position"))?;
                let origin = origins
                    .get(index)
                    .ok_or_else(|| invalid("prepaid demand position is out of range"))?;
                let expected = origin.purse().original_authority();
                for signature in [current, expected] {
                    let length = signature.encoded_len();
                    authority_bytes = authority_bytes
                        .checked_sub(length)
                        .ok_or_else(|| invalid("prepaid demand authority byte limit exceeded"))?;
                }
                if current != expected {
                    return Err(invalid(
                        "prepaid stack head differs from the measured authority",
                    ));
                }
                // C8 (DR-87): each occurrence keeps its own quantity bound, so
                // binding to an aggregated entry decides as binding to the
                // occurrence did.
                let left = remaining
                    .get_mut(index)
                    .ok_or_else(|| invalid("prepaid demand position is out of range"))?;
                *left = left.checked_sub(1).ok_or_else(|| {
                    invalid("prepaid resource exceeds the remaining measured demand")
                })?;
                // Changed by C8 (DR-87): an assignment names the aggregated entry
                // of its occurrence.
                // assignments.push(MeasuredPhloPrepaidUse {
                //     demand_index: index,
                //     resource: cell.resource(),
                // });
                assignments.push(MeasuredPhloPrepaidUse {
                    demand_index: input
                        .demand
                        .occurrence_entry(index)
                        .ok_or_else(|| invalid("prepaid demand position is out of range"))?,
                    resource: cell.resource(),
                });
            }
        }
        let prepared =
            bind_measured_phlo_prepaid(input.demand.resources(), &assignments, limits.execution)
                .map_err(|error| invalid(&error.to_string()))?;
        Ok(NativePrepaidDemandBinding {
            root: self.root(),
            controls: input.controls,
            draws: input.draws,
            prepared,
        })
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn wallet_prepaid_root_guard_refines_exact_snapshot_equality(
            root in any::<[u8; 32]>(), changed in 0_usize..32, bit in 0_u8..8,
        ) {
            prop_assert!(check_wallet_root(&root, &root).is_ok());
            let mut other = root;
            other[changed] ^= 1 << bit;
            prop_assert!(check_wallet_root(&other, &root).is_err());
            prop_assert!(check_wallet_root(&root, &other).is_err());
            prop_assert!(check_wallet_root(&root[..changed], &root).is_err());
        }
    }

    // Added by H1 (bug 11420, DR-126): the selection over abstract inputs. A
    // cell is its class, a current authority is its form, and an occurrence is
    // a class and a form. A cell matches an occurrence when its class and the
    // current form equal the occurrence's class and form, as the production
    // match compares the resource shape and the authority form.

    use std::collections::BTreeMap;

    use models::rust::cost_protocol_limits::offered_funded_v6_limits;
    use models::rust::native_cost_evidence::{
        NativePrepaidDeltaV1, NativePrepaidDraw, NativeSectionError,
    };

    /// One abstract captured stack: the source of its records and the current
    /// form of each cell.
    #[derive(Clone, Debug)]
    struct AbstractStack {
        source: u8,
        current: Vec<u8>,
    }

    #[derive(Clone, Copy, Debug)]
    enum Rule {
        /// The selection before H1 (`old_select_prefixes`).
        Old,
        /// The production selection (`select_prefixes`).
        Forward,
    }

    /// The selection of `rule`. Stack i has the instance [i + 1; 32], so the
    /// captured order is the instance order, as in `CapturedPrepaidStacks`.
    fn run(
        rule: Rule,
        stacks: &[AbstractStack],
        records: &BTreeMap<u8, Vec<Vec<u8>>>,
        occurrences: &[(u8, u8)],
        draw_limit: usize,
    ) -> Result<CanonicalPrepaidSelection, CasperError> {
        let views = stacks
            .iter()
            .enumerate()
            .map(|(index, stack)| SelectionStack {
                source: [stack.source; 32],
                instance: [u8::try_from(index + 1).expect("a test has few stacks"); 32],
                current: stack.current.as_slice(),
                records: records.get(&stack.source).map(Vec::as_slice),
            });
        let matches = |cell: &u8, current: &u8, occurrence: &(u8, u8)| {
            Ok(*cell == occurrence.0 && *current == occurrence.1)
        };
        match rule {
            Rule::Old => old_select_prefixes(views, occurrences, draw_limit, matches),
            Rule::Forward => select_prefixes(views, occurrences, draw_limit, matches),
        }
    }

    /// The delta that the encoder builds from a selection
    /// (`encode_measured_prepaid_delta`): each draw copies its positions in
    /// cell order.
    fn delta(selection: &CanonicalPrepaidSelection) -> NativePrepaidDeltaV1 {
        let mut positions = selection.demand_positions().iter();
        let mut draws = Vec::with_capacity(selection.draws().len());
        for draw in selection.draws() {
            let count = usize::try_from(draw.count).expect("a small prefix");
            let mut selected = Vec::with_capacity(count);
            for _ in 0..count {
                let position = positions.next().expect("each cell has a position");
                selected.push(u32::try_from(*position).expect("a small position"));
            }
            draws.push(NativePrepaidDraw {
                stack_id: draw.stack_id,
                receipt_index: u32::try_from(draw.receipt_index).expect("a small receipt index"),
                positions: selected,
            });
        }
        assert!(
            positions.next().is_none(),
            "every position belongs to a draw"
        );
        NativePrepaidDeltaV1 {
            draws,
            births: Vec::new(),
            replacements: Vec::new(),
        }
    }

    /// The encoding of the selection's delta, which runs the canonical check.
    fn encoded(selection: &CanonicalPrepaidSelection) -> Result<Vec<u8>, NativeSectionError> {
        delta(selection).encode(offered_funded_v6_limits().prepaid_delta)
    }

    /// Added by H1 (bug 11420, DR-126): the selection before H1, kept as the
    /// oracle of the tests. Each cell scans every position from 0, so it
    /// takes the first free matching occurrence anywhere.
    fn old_select_prefixes<'r, Cell: 'r, Current: 'r, Occurrence>(
        stacks: impl IntoIterator<Item = SelectionStack<'r, Cell, Current>>,
        occurrences: &[Occurrence],
        draw_limit: usize,
        mut matches: impl FnMut(&Cell, &Current, &Occurrence) -> Result<bool, CasperError>,
    ) -> Result<CanonicalPrepaidSelection, CasperError> {
        let count = occurrences.len();
        let mut used = vec![false; count];
        let mut candidate_used = vec![false; count];
        let mut candidate_positions = Vec::with_capacity(count);
        let mut best_positions = Vec::with_capacity(count);
        let mut draws = Vec::with_capacity(draw_limit);
        let mut demand_positions = Vec::with_capacity(count);
        let mut selected_receipts = BTreeSet::new();
        for stack in stacks {
            let records = stack
                .records
                .ok_or_else(|| invalid("captured prepaid stack has no rooted receipt inventory"))?;
            let mut best_index = None;
            best_positions.clear();
            for (receipt_index, record) in records.iter().enumerate() {
                if selected_receipts.contains(&(stack.source, receipt_index)) {
                    continue;
                }
                if record.len() != stack.current.len() {
                    continue;
                }
                candidate_positions.clear();
                for (cell, current) in record.iter().zip(stack.current) {
                    let mut matched = None;
                    for (position, occurrence) in occurrences.iter().enumerate() {
                        if used[position] || candidate_used[position] {
                            continue;
                        }
                        if matches(cell, current, occurrence)? {
                            matched = Some(position);
                            break;
                        }
                    }
                    let Some(position) = matched else { break };
                    candidate_used[position] = true;
                    candidate_positions.push(position);
                }
                if candidate_positions.len() > best_positions.len() {
                    best_index = Some(receipt_index);
                    best_positions.clear();
                    best_positions.extend_from_slice(&candidate_positions);
                }
                for position in candidate_positions.drain(..) {
                    candidate_used[position] = false;
                }
            }
            let Some(receipt_index) = best_index else {
                continue;
            };
            if draws.len() >= draw_limit {
                return Err(invalid(
                    "canonical prepaid selection exceeds its draw limit",
                ));
            }
            let count = u64::try_from(best_positions.len())
                .map_err(|_| invalid("prepaid selected prefix count overflow"))?;
            selected_receipts.insert((stack.source, receipt_index));
            draws.push(PrepaidStackPop {
                stack_id: stack.instance,
                receipt_index,
                count,
            });
            for position in best_positions.drain(..) {
                used[position] = true;
                demand_positions.push(position);
            }
        }
        Ok(CanonicalPrepaidSelection {
            draws,
            demand_positions,
        })
    }

    /// Added by H1 (bug 11420, DR-126): the example of the bug. One stack
    /// holds a cell of lane a (class 1) and then a cell of lane b (class 2).
    /// The run measured b at position 0 and a at position 1. The selection
    /// draws one cell at position 1, and the delta encodes. The old rule gave
    /// the positions [1, 0], which encode as Noncanonical.
    #[test]
    fn a_stack_against_occurrences_in_reverse_order_draws_a_forward_prefix() {
        let stacks = [AbstractStack {
            source: 1,
            current: vec![0, 0],
        }];
        let records = BTreeMap::from([(1, vec![vec![1, 2]])]);
        let occurrences = [(2, 0), (1, 0)];
        let selection = run(Rule::Forward, &stacks, &records, &occurrences, 1)
            .expect("the selection completes");
        let bytes = encoded(&selection);
        assert!(
            bytes.is_ok(),
            "the delta of the selection encodes: {bytes:?}"
        );
        assert_eq!(selection.draws().len(), 1);
        assert_eq!(selection.draws()[0].count, 1);
        assert_eq!(selection.demand_positions(), &[1]);
        let old = run(Rule::Old, &stacks, &records, &occurrences, 1)
            .expect("the old selection completes");
        assert_eq!(old.demand_positions(), &[1, 0]);
        assert_eq!(encoded(&old), Err(NativeSectionError::Noncanonical));
    }

    /// Generated selections: occurrences of three classes and two forms,
    /// sources of up to four records, and up to four stacks.
    fn selection_inputs(
    ) -> impl Strategy<Value = (Vec<(u8, u8)>, Vec<Vec<Vec<u8>>>, Vec<(u8, Vec<u8>)>)> {
        (
            proptest::collection::vec((0_u8..3, 0_u8..2), 0..9),
            proptest::collection::vec(
                proptest::collection::vec(proptest::collection::vec(0_u8..3, 1..4), 0..5),
                1..4,
            ),
            proptest::collection::vec((0_u8..3, proptest::collection::vec(0_u8..2, 1..4)), 0..5),
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1024))]

        /// Added by H1 (bug 11420, DR-126), the theorem of G3 design v5
        /// section 1.1: where the old selection is canonical, the forward
        /// selection is the same selection. Where it is not canonical, the
        /// forward selection is canonical. So no valid evidence changes.
        #[test]
        fn the_forward_selection_keeps_canonical_selections_and_repairs_the_rest(
            (occurrences, sources, stacks) in selection_inputs(),
        ) {
            let records = sources
                .iter()
                .enumerate()
                .map(|(index, records)| {
                    (u8::try_from(index).expect("few sources"), records.clone())
                })
                .collect::<BTreeMap<_, _>>();
            let count = u8::try_from(sources.len()).expect("few sources");
            let stacks = stacks
                .into_iter()
                .map(|(source, current)| AbstractStack { source: source % count, current })
                .collect::<Vec<_>>();
            let old = run(Rule::Old, &stacks, &records, &occurrences, stacks.len())
                .expect("the old selection completes");
            let new = run(Rule::Forward, &stacks, &records, &occurrences, stacks.len())
                .expect("the forward selection completes");
            match encoded(&old) {
                Ok(_) => prop_assert_eq!(&new, &old),
                Err(error) => {
                    prop_assert_eq!(error, NativeSectionError::Noncanonical);
                    prop_assert!(encoded(&new).is_ok(), "{:?}", encoded(&new));
                }
            }
            prop_assert!(encoded(&new).is_ok());
        }
    }
}
