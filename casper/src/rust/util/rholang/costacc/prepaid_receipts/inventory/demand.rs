use std::collections::BTreeSet;
use std::mem::size_of;

use prost::Message;
use rholang::rust::interpreter::accounting::authority::reserve_authority_signature_tree;
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
        budget: &HostWorkBudget,
    ) -> Result<CheckedDirectWalletSettlement<'a, A>, CasperError> {
        check_wallet_root(&policy.snapshot().wallets().pre_state_root(), &self.root)?;
        let observed =
            check_counted_phlo_execution(self.controls, self.witness(), limits.matching.execution)
                .and_then(|execution| execution.with_retained_acquisitions(retained, budget))
                .map_err(|error| invalid(&error.to_string()))?;
        policy
            .capture_settlement(observed, outcome, limits.matching, limits.capture, budget)
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

impl NativePrepaidInventory<'_, '_> {
    pub fn select_measured_demand(
        &self,
        demand: &NativePhloAcquisitionDemand<'_>,
        limits: NativePrepaidDemandLimits,
        budget: &HostWorkBudget,
    ) -> Result<CanonicalPrepaidSelection, CasperError> {
        // Changed by C8 (DR-87): the entry limit applies to the distinct
        // demand keys, and the selection walks the located occurrences.
        // let count = demand.resources().len();
        // if count > limits.execution.resource_entries {
        if demand.resources().len() > limits.execution.resource_entries {
            return Err(invalid("measured prepaid demand entry limit exceeded"));
        }
        let count = demand.occurrence_count();
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            count
                .checked_mul(
                    size_of::<(NativePhloLocatedDemand<'_>, PhloResourceAmount<'_>)>()
                        + size_of::<bool>() * 2
                        + size_of::<usize>() * 3,
                )
                .and_then(|size| {
                    limits
                        .draws
                        .checked_mul(size_of::<PrepaidStackPop>())
                        .and_then(|draws| size.checked_add(draws))
                })
                .ok_or_else(|| invalid("prepaid selection allocation overflow"))?,
        )?;
        let mut origins = Vec::new();
        origins
            .try_reserve_exact(count)
            .map_err(|_| invalid("prepaid selection origin allocation failed"))?;
        origins.extend(demand.occurrences());
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
            .try_reserve_exact(limits.draws)
            .map_err(|_| invalid("prepaid selection draw allocation failed"))?;
        let mut demand_positions = Vec::new();
        demand_positions
            .try_reserve_exact(count)
            .map_err(|_| invalid("prepaid selection position allocation failed"))?;
        let mut selected_receipts = BTreeSet::new();
        for stack in self.captured.stacks() {
            let records = self
                .sources
                .get(&stack.source_hash)
                .ok_or_else(|| invalid("captured prepaid stack has no rooted receipt inventory"))?;
            let mut best_index = None;
            best_positions.clear();
            for (receipt_index, record) in records.iter().enumerate() {
                reserve(budget, HostWorkDimension::SearchCandidates, 1)?;
                if selected_receipts.contains(&(stack.source_hash, receipt_index)) {
                    continue;
                }
                if record.len() != stack.stack.cells.len() {
                    continue;
                }
                candidate_positions.clear();
                for (cell, current) in record.iter().zip(&stack.stack.cells) {
                    let mut matched = None;
                    for (position, (origin, measured)) in origins.iter().enumerate() {
                        reserve(budget, HostWorkDimension::SearchCandidates, 1)?;
                        if used[position] || candidate_used[position] {
                            continue;
                        }
                        let bytes = cell
                            .receipt()
                            .resource_bytes()
                            .len()
                            .checked_add(current.encoded_len())
                            .and_then(|bytes| bytes.checked_add(measured.resource.location.len()))
                            .and_then(|bytes| {
                                bytes.checked_add(measured.resource.acquisition_terms.len())
                            })
                            .ok_or_else(|| invalid("prepaid comparison work overflow"))?;
                        reserve(budget, HostWorkDimension::VerificationBytes, bytes)?;
                        reserve(budget, HostWorkDimension::VerificationOperations, bytes)?;
                        if cell.resource() == measured.resource
                            && current == origin.purse().original_authority()
                        {
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
            if draws.len() >= limits.draws {
                return Err(invalid(
                    "canonical prepaid selection exceeds its draw limit",
                ));
            }
            let count = u64::try_from(best_positions.len())
                .map_err(|_| invalid("prepaid selected prefix count overflow"))?;
            reserve(
                budget,
                HostWorkDimension::SearchStateBytes,
                size_of::<([u8; 32], usize)>() + 64,
            )?;
            selected_receipts.insert((stack.source_hash, receipt_index));
            draws.push(PrepaidStackPop {
                stack_id: stack.instance_id,
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

    pub fn bind_measured_demand<'a>(
        &'a self,
        input: NativePrepaidDemandInput<'a>,
        limits: NativePrepaidDemandLimits,
        budget: &HostWorkBudget,
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
        let prefixes = self.prefixes(input.draws, limits.draws, budget)?;
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
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            count
                .checked_mul(size_of::<MeasuredPhloPrepaidUse<'_>>())
                .and_then(|size| {
                    occurrences
                        .checked_mul(size_of::<NativePhloLocatedDemand<'_>>() + size_of::<u64>())
                        .and_then(|origins| size.checked_add(origins))
                })
                .ok_or_else(|| invalid("measured prepaid allocation overflow"))?,
        )?;
        let mut origins = Vec::new();
        origins
            .try_reserve_exact(occurrences)
            .map_err(|_| invalid("measured prepaid occurrence allocation failed"))?;
        let mut remaining = Vec::new();
        remaining
            .try_reserve_exact(occurrences)
            .map_err(|_| invalid("measured prepaid occurrence allocation failed"))?;
        for (origin, amount) in input.demand.occurrences() {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            origins.push(origin);
            remaining.push(amount.quantity);
        }
        let mut assignments = Vec::new();
        assignments
            .try_reserve_exact(count)
            .map_err(|_| invalid("measured prepaid assignment allocation failed"))?;
        let mut positions = input.demand_positions.iter();
        let mut authority_bytes = limits.authority_bytes;
        let mut depth = 0;
        for prefix in &prefixes {
            for (cell, current) in prefix.resources().iter().zip(prefix.current_authorities()) {
                reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
                let index = *positions
                    .next()
                    .ok_or_else(|| invalid("missing prepaid demand position"))?;
                let origin = origins
                    .get(index)
                    .ok_or_else(|| invalid("prepaid demand position is out of range"))?;
                let expected = origin.purse().original_authority();
                for signature in [current, expected] {
                    reserve_authority_signature_tree(signature, budget, &mut depth)
                        .map_err(|error| invalid(&error.to_string()))?;
                    let length = signature.encoded_len();
                    authority_bytes = authority_bytes
                        .checked_sub(length)
                        .ok_or_else(|| invalid("prepaid demand authority byte limit exceeded"))?;
                    reserve(budget, HostWorkDimension::VerificationBytes, length)?;
                    reserve(budget, HostWorkDimension::VerificationOperations, length)?;
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
        let prepared = bind_measured_phlo_prepaid(
            input.demand.resources(),
            &assignments,
            limits.execution,
            budget,
        )
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
}
