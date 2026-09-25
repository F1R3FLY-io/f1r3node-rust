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
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            count
                .checked_mul(size_of::<MeasuredPhloPrepaidUse<'_>>())
                .and_then(|size| {
                    input
                        .demand
                        .resources()
                        .len()
                        .checked_mul(size_of::<NativePhloLocatedDemand<'_>>())
                        .and_then(|origins| size.checked_add(origins))
                })
                .ok_or_else(|| invalid("measured prepaid allocation overflow"))?,
        )?;
        let mut origins = Vec::new();
        origins
            .try_reserve_exact(input.demand.resources().len())
            .map_err(|_| invalid("measured prepaid occurrence allocation failed"))?;
        for (origin, _) in input.demand.occurrences() {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            origins.push(origin);
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
                assignments.push(MeasuredPhloPrepaidUse {
                    demand_index: index,
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
