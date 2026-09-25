use std::collections::BTreeMap;
use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::signed_phlo_deploy::FundedDeploy;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    cost_signature_to_sig, reserve_authority_signature_tree, AuthorityBornStack,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    CheckedRetainedBirthFunding, PhloObligationKey, RetainedBirthFunding,
    RetainedBirthFundingLimits,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;

use super::snapshot::reserve;
use super::{invalid, CasperError};
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::costacc::direct_wallet_funding::CheckedDirectWalletSettlement;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::supply::{decode_purse_inventory, supply_channel, PurseStack};

#[derive(Clone, Copy, Debug)]
pub struct NativeRetainedBirthLimits {
    pub funding: RetainedBirthFundingLimits,
    pub physical_cells: usize,
    pub physical_bytes: usize,
}

#[derive(Debug)]
pub struct CapturedNativeRetainedBirths<'s, 'a, 'b, A = FundedDeploy> {
    settlement: &'s CheckedDirectWalletSettlement<'a, A>,
    policy: &'s AdoptedResourcePolicy,
    funding: CheckedRetainedBirthFunding<'s, 'a, 'b>,
    stacks: Vec<PurseStack>,
}

impl<'s, 'a, 'b, A> CapturedNativeRetainedBirths<'s, 'a, 'b, A> {
    pub fn settlement(&self) -> &'s CheckedDirectWalletSettlement<'a, A> { self.settlement }
    pub fn policy(&self) -> &'s AdoptedResourcePolicy { self.policy }
    pub fn funding(&self) -> &CheckedRetainedBirthFunding<'s, 'a, 'b> { &self.funding }
    pub fn stacks(&self) -> &[PurseStack] { &self.stacks }
}

fn charge_physical_bytes(
    budget: &HostWorkBudget,
    bytes: usize,
    remaining: &mut usize,
) -> Result<(), CasperError> {
    *remaining = remaining
        .checked_sub(bytes)
        .ok_or_else(|| invalid("retained birth physical byte limit exceeded"))?;
    reserve(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    reserve(budget, HostWorkDimension::VerificationOperations, bytes)
}

impl RuntimeOps {
    pub async fn capture_retained_births<'s, 'a, 'b, A>(
        &mut self,
        settlement: &'s CheckedDirectWalletSettlement<'a, A>,
        policy: &'s AdoptedResourcePolicy,
        bindings: &[RetainedBirthFunding<'b>],
        limits: NativeRetainedBirthLimits,
        budget: &HostWorkBudget,
    ) -> Result<CapturedNativeRetainedBirths<'s, 'a, 'b, A>, CasperError> {
        let capture = settlement.capture().scoped().capture();
        let funding = capture
            .bind_retained_births(bindings, limits.funding, budget)
            .map_err(|error| invalid(&error.to_string()))?;
        let family = capture.intent().bound().consent().family();
        let controls = family.cases()[capture.branch()]
            .obligations
            .execution()
            .controls();
        policy.check_controls(controls)?;
        for column in capture.obligations() {
            if let PhloObligationKey::RetainedResource(resource) = column.key() {
                reserve(
                    budget,
                    HostWorkDimension::VerificationOperations,
                    resource.acquisition_terms.len(),
                )?;
                reserve(
                    budget,
                    HostWorkDimension::SearchStateBytes,
                    resource.acquisition_terms.len(),
                )?;
                let terms = policy.check_acquisition_terms(resource.acquisition_terms)?;
                let commitment = terms
                    .schedule()
                    .digest(PhloGenesisPolicy::LIMITS)
                    .map_err(|error| invalid(&error.to_string()))?;
                if commitment != controls.schedule().commitment {
                    return Err(invalid(
                        "new retained resource does not use the selected acquisition schedule",
                    ));
                }
            }
        }
        let stacks = self
            .capture_retained_birth_stacks(&funding, limits, budget)
            .await?;
        Ok(CapturedNativeRetainedBirths {
            settlement,
            policy,
            funding,
            stacks,
        })
    }

    pub(super) async fn capture_retained_birth_stacks(
        &self,
        funding: &CheckedRetainedBirthFunding<'_, '_, '_>,
        limits: NativeRetainedBirthLimits,
        budget: &HostWorkBudget,
    ) -> Result<Vec<PurseStack>, CasperError> {
        let mut remaining = limits.physical_bytes;
        let mut cells_left = limits.physical_cells;
        let mut depth = 0;
        let mut groups = BTreeMap::<Vec<u8>, Vec<&AuthorityBornStack>>::new();
        let count = funding.births().len();
        charge_physical_bytes(
            budget,
            count
                .checked_mul(size_of::<PurseStack>() + 256)
                .ok_or_else(|| invalid("retained birth allocation overflow"))?,
            &mut remaining,
        )?;
        for binding in funding.births() {
            let head = &binding.birth.cells[0];
            charge_physical_bytes(budget, head.encoded_len(), &mut remaining)?;
            reserve(
                budget,
                HostWorkDimension::VerificationOperations,
                head.encoded_len()
                    .checked_mul(4 * (1 + count.checked_ilog2().unwrap_or(0) as usize))
                    .ok_or_else(|| invalid("retained birth grouping work overflow"))?,
            )?;
            let group = groups.entry(head.encode_to_vec()).or_default();
            group
                .try_reserve(1)
                .map_err(|_| invalid("retained birth group allocation failed"))?;
            group.push(binding.birth);
        }
        let mut stacks = Vec::new();
        stacks
            .try_reserve_exact(count)
            .map_err(|_| invalid("retained birth capture allocation failed"))?;
        for (_, mut group) in groups {
            reserve(
                budget,
                HostWorkDimension::VerificationOperations,
                group
                    .len()
                    .checked_mul(64 * (1 + group.len().checked_ilog2().unwrap_or(0) as usize))
                    .ok_or_else(|| invalid("retained birth sorting work overflow"))?,
            )?;
            group.sort_unstable_by_key(|birth| birth.produce_hash);
            let head = &group[0].cells[0];
            let signature =
                cost_signature_to_sig(head).map_err(|error| invalid(&error.to_string()))?;
            let channel = supply_channel(&signature);
            let data = self.get_data_datums(&channel).await;
            for datum in &data {
                if let Some(stack) = &datum.a.cost_stack {
                    cells_left = cells_left
                        .checked_sub(stack.cells.len())
                        .ok_or_else(|| invalid("retained birth physical cell limit exceeded"))?;
                    for cell in &stack.cells {
                        reserve_authority_signature_tree(cell, budget, &mut depth)
                            .map_err(|error| invalid(&error.to_string()))?;
                    }
                }
                charge_physical_bytes(budget, datum.a.encoded_len(), &mut remaining)?;
            }
            charge_physical_bytes(
                budget,
                channel
                    .encoded_len()
                    .checked_add(size_of::<PurseStack>())
                    .and_then(|n| n.checked_mul(data.len()))
                    .ok_or_else(|| invalid("retained birth inventory size overflow"))?,
                &mut remaining,
            )?;
            let inventory = decode_purse_inventory(&data, head)?;
            let mut found = Vec::new();
            found
                .try_reserve_exact(group.len())
                .map_err(|_| invalid("retained birth match allocation failed"))?;
            found.resize(group.len(), false);
            for stack in inventory.stacks {
                reserve(
                    budget,
                    HostWorkDimension::VerificationOperations,
                    64 * (1 + group.len().checked_ilog2().unwrap_or(0) as usize),
                )?;
                let Ok(index) =
                    group.binary_search_by_key(&stack.source_hash, |birth| birth.produce_hash)
                else {
                    continue;
                };
                let birth = group[index];
                if found[index]
                    || stack.instance_id != birth.stack_id
                    || stack.stack.cells != birth.cells
                {
                    return Err(invalid(
                        "retained birth does not identify exactly one matching live stack",
                    ));
                }
                found[index] = true;
                stacks.push(stack);
            }
            if found.iter().any(|present| !present) {
                return Err(invalid(
                    "retained birth is absent from the live resource state",
                ));
            }
        }
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            count
                .checked_mul(64 * (1 + count.checked_ilog2().unwrap_or(0) as usize))
                .ok_or_else(|| invalid("retained birth sorting work overflow"))?,
        )?;
        stacks.sort_unstable_by_key(|stack| stack.instance_id);
        Ok(stacks)
    }
}

#[cfg(test)]
mod tests;
