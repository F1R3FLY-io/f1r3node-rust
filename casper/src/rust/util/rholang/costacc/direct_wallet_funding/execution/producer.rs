use std::collections::HashMap;
use std::mem::size_of;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::Event as CasperEvent;
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::native_cost_evidence::NativeCostFailureClass;
use models::rust::native_wallet_receipt::NativeWalletReceiptLimits;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::phlo_source::PhloSourcePolicyV1;
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    cost_signature_to_sig, cost_signature_to_sig_metered, funding_sig_channel_metered,
    reserve_authority_signature_tree, AuthorityBornStack, AuthorityStackBirth,
};
use rholang::rust::interpreter::accounting::economic_failure::classify_errors;
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_offered_phlo_controls, PhloControlsBinding, PhloFundingTerms, PhloOffer,
    PhloScheduleBinding,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, check_phlo_funding_family, project_phlo_obligations,
    CheckedPhloObligations, PhloExecutionLimits, PhloFamilyFundingLimits, PhloFundingCase,
    PhloFundingIntentBinding, PhloObligationKey, PhloOutcome, PhloResource, PhloResourceAmount,
    RestoredPhloResource, RetainedBirthFunding, SignedPhloConsentLimits,
};
use rholang::rust::interpreter::accounting::{NativeRecordingWireLimits, Sig};
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use rspace_plus_plus::rspace::trace::event::Event;
use shared::rust::clone_backing::{self, BackingError};

use super::settlement::{
    check_metered_usage, invalid, NativeFundedSettlement, NativeGrantIssuanceInput,
    NativeGrantSettlementInput,
};
use super::{
    select_rooted_native_family, NativeAttemptSettlementInput, NativeAttemptSettlementLimits,
    NativeOfferedAttempt,
};
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletPolicySnapshot;
use crate::rust::util::rholang::costacc::offered_candidate::assemble_offered_processed_deploy;
use crate::rust::util::rholang::costacc::offered_context::OfferedSettlementContext;
use crate::rust::util::rholang::costacc::offered_evidence::{
    encode_measured_funding_case, encode_measured_prepaid_delta, encode_measured_runtime_recording,
    EncodedNativeRuntimeRecording,
};
use crate::rust::util::rholang::costacc::offered_grants::{
    offered_grant_transition_limits, OfferedGrantChange,
};
use crate::rust::util::rholang::costacc::prepaid_receipts::{
    NativePrepaidCellLimits, NativePrepaidConsumption, NativePrepaidDemandInput,
    NativePrepaidInventoryLimits, NativeRetainedBirthLimits, NativeRetainedRecordLimits,
    NativeRetainedSettlementLimits, PrepaidReceiptBucket, PrepaidReceiptBucketLimits,
    PrepaidReceiptChange, PrepaidReceiptLimits, PrepaidReceiptSnapshot, PrepaidStackCaptureLimits,
    PrepaidStackPopLimits,
};
use crate::rust::util::rholang::costacc::supply::PurseStack;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Clone, Copy, Debug)]
pub struct NativeOfferedFamilyLimits {
    pub measured: NativeAttemptSettlementLimits,
    pub signed: SignedPhloConsentLimits,
    pub policy: PhloFamilyFundingLimits,
    pub retained: NativeRetainedBirthLimits,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeOfferedProductionLimits {
    pub family: NativeOfferedFamilyLimits,
    pub stack_capture: PrepaidStackCaptureLimits,
    pub retained_record: NativeRetainedRecordLimits,
    pub receipt_bucket: PrepaidReceiptBucketLimits,
    pub retained_settlement: NativeRetainedSettlementLimits,
    pub stack_pop: PrepaidStackPopLimits,
    pub prepaid_cell: NativePrepaidCellLimits,
    pub physical_cells: usize,
    pub physical_bytes: usize,
    pub execution: PhloExecutionLimits,
    pub recording: NativeRecordingWireLimits,
}

pub struct NativeScopedOfferedResult {
    pub settlement: NativeFundedSettlement,
    pub wallet_settlement_log_events: usize,
    pub final_root: [u8; 32],
    pub funding_case: Vec<u8>,
    pub prepaid_delta: Vec<u8>,
    pub recording: EncodedNativeRuntimeRecording,
}

pub struct NativeOfferedPreparedResult {
    pub funding_root: [u8; 32],
    pub execution_root: [u8; 32],
    pub settlement_root: [u8; 32],
    pub wallet_settlement_root: [u8; 32],
    pub final_root: [u8; 32],
    pub phlo_used: u64,
    pub fresh_phlo: u64,
    pub retained_phlo: u64,
    pub failure_class: NativeCostFailureClass,
    pub funding_case: Vec<u8>,
    pub prepaid_delta: Vec<u8>,
    pub recording: EncodedNativeRuntimeRecording,
    pub wallet_receipt: Vec<u8>,
    pub wallet_settlement_log_events: usize,
    pub resource_rev: u128,
    pub fee_rev: u128,
    pub rev_spent: u128,
    pub user_log: std::sync::Arc<[Event]>,
    pub grant_drain_log: std::sync::Arc<[Event]>,
    pub settlement_log: Vec<CasperEvent>,
    pub grant_changes: Vec<OfferedGrantChange>,
    pub mergeable: HashMap<Par, MergeType>,
}

pub struct NativeOfferedCandidateResult {
    pub processed: OfferedProcessedDeploy,
    pub final_root: [u8; 32],
    pub mergeable: HashMap<Par, MergeType>,
}

fn reserve(
    budget: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), CasperError> {
    budget
        .reserve(
            dimension,
            HostWorkUnits::new(
                u64::try_from(amount).map_err(|_| invalid("native funding work overflow"))?,
            ),
        )
        .map(|_| ())
        .map_err(invalid)
}

fn match_birth_claims(
    observed: &[AuthorityStackBirth],
    claims: &[RetainedBirthFunding<'_>],
    maximum: usize,
    budget: &HostWorkBudget,
) -> Result<(), CasperError> {
    if observed.len() != claims.len() || claims.len() > maximum {
        return Err(invalid(
            "retained birth claims differ from native execution",
        ));
    }
    reserve(budget, HostWorkDimension::SearchStateBytes, claims.len())?;
    let mut matched = Vec::new();
    matched
        .try_reserve_exact(claims.len())
        .map_err(|_| invalid("retained birth match allocation failed"))?;
    matched.resize(claims.len(), false);
    for birth in observed {
        let mut selected = None;
        let mut depth = 0;
        for (index, claim) in claims.iter().enumerate() {
            for cell in birth.cells.iter().chain(claim.birth.cells.iter()) {
                reserve_authority_signature_tree(cell, budget, &mut depth).map_err(invalid)?;
            }
            let bytes = birth
                .cells
                .iter()
                .chain(claim.birth.cells.iter())
                .try_fold(0usize, |sum, cell| sum.checked_add(cell.encoded_len()))
                .ok_or_else(|| invalid("retained birth comparison work overflow"))?;
            reserve(budget, HostWorkDimension::VerificationOperations, bytes)?;
            reserve(budget, HostWorkDimension::SearchCandidates, 1)?;
            if birth.produce_hash == claim.birth.produce_hash && birth.cells == claim.birth.cells {
                if selected.is_some() || matched[index] {
                    return Err(invalid("retained birth has no unique native claim"));
                }
                selected = Some(index);
            }
        }
        let index = selected.ok_or_else(|| invalid("retained birth has no unique native claim"))?;
        matched[index] = true;
    }
    if matched.iter().any(|matched| !matched) {
        return Err(invalid("retained birth claim was not observed"));
    }
    Ok(())
}

pub(super) fn derive_live_born_stacks(
    observed: &[AuthorityStackBirth],
    rooted_stacks: &[PurseStack],
    maximum: usize,
    budget: &HostWorkBudget,
) -> Result<Vec<AuthorityBornStack>, CasperError> {
    if observed.len() > maximum {
        return Err(invalid("native birth count exceeds its limit"));
    }
    reserve(
        budget,
        HostWorkDimension::SearchStateBytes,
        observed
            .len()
            .checked_mul(size_of::<AuthorityBornStack>())
            .ok_or_else(|| invalid("native birth allocation overflows"))?,
    )?;
    let mut births = Vec::new();
    births
        .try_reserve_exact(observed.len())
        .map_err(|_| invalid("native birth allocation failed"))?;
    for birth in observed {
        let mut found = None;
        for stack in rooted_stacks {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            if stack.source_hash != birth.produce_hash || stack.stack.cells != birth.cells {
                continue;
            }
            if found.replace(stack).is_some() {
                return Err(invalid("native birth identifies multiple rooted stacks"));
            }
        }
        let stack = found.ok_or_else(|| invalid("native birth is absent from rooted state"))?;
        let cell_bytes = birth
            .cells
            .iter()
            .try_fold(0usize, |sum, cell| sum.checked_add(cell.encoded_len()));
        let cell_bytes = cell_bytes.ok_or_else(|| invalid("native birth cells overflow"))?;
        reserve(budget, HostWorkDimension::SearchStateBytes, cell_bytes)?;
        reserve(budget, HostWorkDimension::VerificationBytes, cell_bytes)?;
        births.push(AuthorityBornStack {
            stack_id: stack.instance_id,
            produce_hash: birth.produce_hash,
            cells: birth.cells.clone(),
        });
    }
    Ok(births)
}

pub(super) fn restore_retained_permissions<'a>(
    sources: &'a [PhloSourcePolicyV1<'a>],
    terms: &[u8],
    class_count: usize,
    limits: PhloExecutionLimits,
    budget: &HostWorkBudget,
) -> Result<Vec<RestoredPhloResource<'a>>, CasperError> {
    let count = sources
        .iter()
        .try_fold(0usize, |sum, source| {
            sum.checked_add(source.resources().len())
        })
        .ok_or_else(|| invalid("signed retained permission count overflows"))?;
    reserve(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<RestoredPhloResource<'_>>())
            .ok_or_else(|| invalid("signed retained permission allocation overflows"))?,
    )?;
    let mut restored = Vec::new();
    restored
        .try_reserve_exact(count)
        .map_err(|_| invalid("signed retained permission allocation failed"))?;
    for source in sources {
        for permission in source.resources() {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            if permission.acquisition_terms != terms {
                continue;
            }
            if usize::try_from(permission.class).map_or(true, |class| class >= class_count) {
                return Err(invalid("signed retained permission has an unknown class"));
            }
            restored.push(
                RestoredPhloResource::from_wire_key(permission, limits, budget).map_err(invalid)?,
            );
        }
    }
    Ok(restored)
}

fn born_resource<'a>(
    cell: &models::rhoapi::CostSignature,
    permissions: &'a [RestoredPhloResource<'_>],
    terms: &[u8],
    budget: &HostWorkBudget,
) -> Result<PhloResource<'a>, CasperError> {
    let backing = |operations: usize, scanned: usize, allocation: usize| {
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            operations.checked_mul(2).ok_or(BackingError::Overflow)?,
        )
        .map_err(|_| BackingError::Rejected)?;
        reserve(budget, HostWorkDimension::VerificationBytes, scanned)
            .map_err(|_| BackingError::Rejected)?;
        reserve(budget, HostWorkDimension::SearchStateBytes, allocation)
            .map_err(|_| BackingError::Rejected)
    };
    let authority = cost_signature_to_sig_metered(cell, &backing).map_err(invalid)?;
    if !authority.is_funding_former() {
        return Err(invalid("native retained cell is not a funding former"));
    }
    let channel = funding_sig_channel_metered(&authority, &backing).map_err(invalid)?;
    // Added by D-E3 (DR-110): `funding_sig_channel_metered` prepays only the
    // release of its unsorted channel. The surplus of its per-level inspection
    // paid the length computation below, so a block inspection prepays it
    // here.
    clone_backing::inspect_blocks(&channel, &backing)
        .map_err(|_| invalid("native retained channel inspection is rejected"))?;
    let size = channel.encoded_len();
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        size.checked_mul(2)
            .ok_or_else(|| invalid("native retained channel work overflows"))?,
    )?;
    reserve(budget, HostWorkDimension::SearchStateBytes, size)?;
    let mut location = Vec::new();
    location
        .try_reserve_exact(size)
        .map_err(|_| invalid("native retained channel allocation failed"))?;
    channel.encode(&mut location).map_err(invalid)?;
    let mut selected: Option<PhloResource<'a>> = None;
    for permission in permissions {
        reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
        let resource = permission.resource();
        let born = PhloResource {
            location: &location,
            class: resource.class,
            acquisition_terms: terms,
            authority: &authority,
        };
        if !resource_equal_metered(resource, born, budget)? {
            continue;
        }
        if selected.is_some_and(|prior| prior.class != resource.class) {
            return Err(invalid(
                "native retained authority has ambiguous signed resources",
            ));
        }
        selected = Some(resource);
    }
    selected.ok_or_else(|| invalid("native retained authority has no signed resource"))
}

fn resource_equal_metered(
    left: PhloResource<'_>,
    right: PhloResource<'_>,
    budget: &HostWorkBudget,
) -> Result<bool, CasperError> {
    let bytes = left
        .location
        .len()
        .checked_add(right.location.len())
        .and_then(|n| n.checked_add(left.acquisition_terms.len()))
        .and_then(|n| n.checked_add(right.acquisition_terms.len()))
        .ok_or_else(|| invalid("retained resource comparison size overflows"))?;
    reserve(budget, HostWorkDimension::VerificationOperations, bytes)?;
    reserve(budget, HostWorkDimension::VerificationBytes, bytes)?;
    let backing = |operations: usize, scanned: usize, allocation: usize| {
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            operations.checked_mul(2).ok_or(BackingError::Overflow)?,
        )
        .map_err(|_| BackingError::Rejected)?;
        reserve(budget, HostWorkDimension::VerificationBytes, scanned)
            .map_err(|_| BackingError::Rejected)?;
        reserve(budget, HostWorkDimension::SearchStateBytes, allocation)
            .map_err(|_| BackingError::Rejected)
    };
    // Changed by D-O1 (DR-111): the two authorities are the borrowed sides of
    // the comparison below. A block inspection of each prepays its reads.
    // clone_backing::inspect(left.authority, &backing)
    //     .map_err(|_| invalid("native retained resource comparison work rejected"))?;
    // clone_backing::inspect(right.authority, &backing)
    //     .map_err(|_| invalid("native retained resource comparison work rejected"))?;
    clone_backing::inspect_blocks(left.authority, &backing)
        .map_err(|_| invalid("native retained resource comparison work rejected"))?;
    clone_backing::inspect_blocks(right.authority, &backing)
        .map_err(|_| invalid("native retained resource comparison work rejected"))?;
    Ok(left == right)
}

pub(super) fn derive_retained_amounts<'a>(
    permissions: &'a [RestoredPhloResource<'_>],
    terms: &[u8],
    births: &[AuthorityBornStack],
    maximum_cells: usize,
    budget: &HostWorkBudget,
) -> Result<Vec<PhloResourceAmount<'a>>, CasperError> {
    let count = births
        .iter()
        .try_fold(0usize, |sum, birth| sum.checked_add(birth.cells.len()))
        .filter(|count| *count <= maximum_cells)
        .ok_or_else(|| invalid("native retained cell count exceeds its limit"))?;
    reserve(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<PhloResourceAmount<'_>>())
            .ok_or_else(|| invalid("native retained allocation overflows"))?,
    )?;
    let mut retained: Vec<PhloResourceAmount<'a>> = Vec::new();
    retained
        .try_reserve_exact(count)
        .map_err(|_| invalid("native retained allocation failed"))?;
    for birth in births {
        for cell in &birth.cells {
            let resource = born_resource(cell, permissions, terms, budget)?;
            let mut match_index = None;
            for (index, row) in retained.iter().enumerate() {
                if resource_equal_metered(row.resource, resource, budget)? {
                    match_index = Some(index);
                    break;
                }
            }
            if let Some(index) = match_index {
                let row = &mut retained[index];
                row.quantity = row
                    .quantity
                    .checked_add(1)
                    .ok_or_else(|| invalid("native retained quantity overflows"))?;
            } else {
                retained.push(PhloResourceAmount {
                    resource,
                    quantity: 1,
                });
            }
        }
    }
    Ok(retained)
}

pub(super) fn derive_birth_positions(
    obligations: &CheckedPhloObligations<'_>,
    births: &[AuthorityBornStack],
    budget: &HostWorkBudget,
) -> Result<Vec<Vec<usize>>, CasperError> {
    let mut positions = Vec::new();
    reserve(
        budget,
        HostWorkDimension::SearchStateBytes,
        births
            .len()
            .checked_mul(size_of::<Vec<usize>>())
            .ok_or_else(|| invalid("native birth positions overflow"))?,
    )?;
    positions
        .try_reserve_exact(births.len())
        .map_err(|_| invalid("native birth positions allocation failed"))?;
    for birth in births {
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            birth
                .cells
                .len()
                .checked_mul(size_of::<usize>())
                .ok_or_else(|| invalid("native birth positions overflow"))?,
        )?;
        let mut row = Vec::new();
        row.try_reserve_exact(birth.cells.len())
            .map_err(|_| invalid("native birth positions allocation failed"))?;
        for cell in &birth.cells {
            let authority = cost_signature_to_sig(cell).map_err(invalid)?;
            row.push(unique_retained_position(
                obligations.keys(),
                &authority,
                budget,
            )?);
        }
        positions.push(row);
    }
    Ok(positions)
}

pub(super) fn check_retained_origin(
    permissions: &[RestoredPhloResource<'_>],
    terms: &[u8],
    retained: &[PhloResourceAmount<'_>],
    births: &[AuthorityBornStack],
    maximum_cells: usize,
    budget: &HostWorkBudget,
) -> Result<(), CasperError> {
    let count = births
        .iter()
        .try_fold(0usize, |sum, birth| sum.checked_add(birth.cells.len()));
    let count = count
        .filter(|count| *count <= maximum_cells)
        .ok_or_else(|| invalid("retained physical cell count exceeds its limit"))?;
    reserve(
        budget,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<(PhloResource<'_>, u64)>())
            .ok_or_else(|| invalid("retained attribution size overflow"))?,
    )?;
    let mut attributed = Vec::<(PhloResource<'_>, u64)>::new();
    attributed
        .try_reserve_exact(count)
        .map_err(|_| invalid("retained attribution allocation failed"))?;
    for birth in births {
        for cell in &birth.cells {
            let resource = born_resource(cell, permissions, terms, budget)?;
            let mut match_index = None;
            for (index, (candidate, _)) in attributed.iter().enumerate() {
                if resource_equal_metered(*candidate, resource, budget)? {
                    match_index = Some(index);
                    break;
                }
            }
            if let Some(index) = match_index {
                let (_, quantity) = &mut attributed[index];
                *quantity = quantity
                    .checked_add(1)
                    .ok_or_else(|| invalid("retained physical quantity overflow"))?;
            } else {
                attributed.push((resource, 1));
            }
        }
    }
    if attributed.len() != retained.len() {
        return Err(invalid("retained funding differs from physical births"));
    }
    for (resource, quantity) in attributed {
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            retained.len(),
        )?;
        let mut matches = 0;
        for row in retained {
            if row.quantity == quantity && resource_equal_metered(row.resource, resource, budget)? {
                matches += 1;
            }
        }
        if matches != 1 {
            return Err(invalid(
                "retained funding differs from signed physical births",
            ));
        }
    }
    Ok(())
}

pub(super) fn unique_retained_position(
    keys: &[PhloObligationKey<'_>],
    authority: &Sig,
    budget: &HostWorkBudget,
) -> Result<usize, CasperError> {
    let mut position = None;
    for (index, key) in keys.iter().enumerate() {
        reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
        if let PhloObligationKey::RetainedResource(resource) = key {
            if resource.authority == authority {
                if position.is_some() {
                    return Err(invalid("retained authority has ambiguous obligations"));
                }
                position = Some(index);
            }
        }
    }
    position.ok_or_else(|| invalid("retained authority has no funded obligation"))
}

pub(super) fn check_birth_positions(
    obligations: &CheckedPhloObligations<'_>,
    births: &[RetainedBirthFunding<'_>],
    budget: &HostWorkBudget,
) -> Result<(), CasperError> {
    for birth in births {
        if birth.birth.cells.len() != birth.obligation_positions.len() {
            return Err(invalid("retained birth cell and obligation count differ"));
        }
        for (cell, claimed) in birth.birth.cells.iter().zip(birth.obligation_positions) {
            let authority = cost_signature_to_sig(cell).map_err(invalid)?;
            if unique_retained_position(obligations.keys(), &authority, budget)? != *claimed {
                return Err(invalid(
                    "retained birth obligation position is not canonical",
                ));
            }
        }
    }
    Ok(())
}

impl NativeOfferedAttempt<'_, '_> {
    pub(crate) async fn produce_native_offered_candidate_with_rooted_prepaid(
        self,
        manager: &RuntimeManager,
        envelope: DeployEnvelope,
        snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
        grants: Option<NativeGrantSettlementInput<'_>>,
        production: NativeOfferedProductionLimits,
        inventory_limits: NativePrepaidInventoryLimits,
        receipt_limits: PrepaidReceiptLimits,
        budget: &HostWorkBudget,
    ) -> Result<NativeOfferedCandidateResult, CasperError> {
        let settlement = OfferedSettlementContext::from_block_inputs(&envelope, &self.block_data)?;
        let adopted = self.adopted.clone();
        let funding_root = self.preflight.funding_root;
        let preflight = self.preflight;
        let stacks = self
            .read_measured_prepaid_stacks(
                manager,
                production.family,
                production.stack_capture,
                budget,
            )
            .await?;
        let captured = manager.capture_prepaid_stacks(
            funding_root,
            &stacks,
            &adopted,
            production.stack_capture,
            budget,
        )?;
        let inventory = captured.resource_inventory(inventory_limits, budget)?;
        let receipt_count = stacks
            .len()
            .checked_add(self.evaluation.authority_stack_births.len())
            .ok_or_else(|| invalid("offered receipt key count overflows"))?;
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            receipt_count
                .checked_mul(size_of::<[u8; 32]>())
                .ok_or_else(|| invalid("offered receipt key allocation overflows"))?,
        )?;
        let mut receipt_ids = Vec::new();
        receipt_ids
            .try_reserve_exact(receipt_count)
            .map_err(|_| invalid("offered receipt key allocation failed"))?;
        for stack in &stacks {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            receipt_ids.push(PrepaidReceiptBucket::key_for_source(&stack.source_hash));
        }
        for birth in &self.evaluation.authority_stack_births {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            receipt_ids.push(PrepaidReceiptBucket::key_for_source(&birth.produce_hash));
        }
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            receipt_ids
                .len()
                .checked_mul(1 + receipt_ids.len().checked_ilog2().unwrap_or(0) as usize)
                .ok_or_else(|| invalid("offered receipt sorting work overflows"))?,
        )?;
        receipt_ids.sort_unstable();
        receipt_ids.dedup();
        let receipt_snapshot =
            manager.capture_prepaid_receipts(funding_root, &receipt_ids, receipt_limits, budget)?;
        let selected = preflight.selected_terms(&adopted)?;
        let schedule = PhloScheduleBinding::new(selected.schedule(), PhloGenesisPolicy::LIMITS)
            .map_err(invalid)?;
        self.produce_native_offered_candidate(
            manager,
            envelope,
            snapshot,
            NativeAttemptSettlementInput {
                inventory: &inventory,
                schedule: &schedule,
                terms: selected.bytes(),
                retained: &[],
            },
            &[],
            &receipt_snapshot,
            grants,
            settlement.reservation_id,
            &settlement.fee_address,
            settlement.initial_rand,
            production,
            budget,
        )
        .await
    }

    pub(crate) async fn produce_native_offered_candidate(
        mut self,
        manager: &RuntimeManager,
        envelope: DeployEnvelope,
        snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
        input: NativeAttemptSettlementInput<'_, '_, '_>,
        birth_claims: &[RetainedBirthFunding<'_>],
        receipt_snapshot: &PrepaidReceiptSnapshot,
        grants: Option<NativeGrantSettlementInput<'_>>,
        reservation_id: [u8; 32],
        fee_address: &VaultAddress,
        initial_rand: Blake2b512Random,
        production: NativeOfferedProductionLimits,
        budget: &HostWorkBudget,
    ) -> Result<NativeOfferedCandidateResult, CasperError> {
        let result = self
            .produce_native_offered_candidate_inner(
                manager,
                envelope,
                snapshot,
                input,
                birth_claims,
                receipt_snapshot,
                grants,
                reservation_id,
                fee_address,
                initial_rand,
                production,
                budget,
            )
            .await;
        if result.is_err() {
            let baseline = Blake2b256Hash::from_bytes(self.preflight.execution_root.to_vec());
            self.runtime.runtime.reset(&baseline).await?;
            if self.runtime.runtime.get_root().await.bytes() != self.preflight.execution_root {
                return Err(invalid("offered producer failed to restore its base root"));
            }
        }
        result
    }

    async fn produce_native_offered_candidate_inner(
        &mut self,
        manager: &RuntimeManager,
        envelope: DeployEnvelope,
        snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
        input: NativeAttemptSettlementInput<'_, '_, '_>,
        birth_claims: &[RetainedBirthFunding<'_>],
        receipt_snapshot: &PrepaidReceiptSnapshot,
        grants: Option<NativeGrantSettlementInput<'_>>,
        reservation_id: [u8; 32],
        fee_address: &VaultAddress,
        initial_rand: Blake2b512Random,
        production: NativeOfferedProductionLimits,
        budget: &HostWorkBudget,
    ) -> Result<NativeOfferedCandidateResult, CasperError> {
        let limits = production.family;
        let identity = snapshot
            .wallets()
            .authorization()
            .envelope()
            .envelope_commitment()
            .map_err(invalid)?;
        if identity.as_ref() != self.preflight.envelope_identity
            || envelope.identity().as_bytes() != self.preflight.envelope_identity
            || receipt_snapshot.root() != self.preflight.funding_root
            || self.runtime.runtime.get_root().await.bytes() != self.preflight.execution_root
            || grants.as_ref().is_some_and(|grant| {
                grant.snapshot.root() != self.preflight.funding_root
                    || grant.verified.root() != self.preflight.funding_root
            })
        {
            return Err(invalid(
                "offered production inputs differ from authenticated roots or envelope",
            ));
        }
        if snapshot.wallets().pre_state_root() != self.preflight.funding_root
            || input.inventory.root() != self.preflight.funding_root
        {
            return Err(invalid(
                "measured funding inputs belong to another original root",
            ));
        }
        let user_mergeable = std::mem::take(&mut self.evaluation.mergeable);
        let selected = self.preflight.selected_terms(&self.adopted)?;
        if selected.bytes() != input.terms {
            return Err(invalid(
                "measured funding terms differ from signed selected schedule",
            ));
        }
        let schedule = PhloScheduleBinding::new(selected.schedule(), PhloGenesisPolicy::LIMITS)
            .map_err(invalid)?;
        if schedule.schedule() != input.schedule.schedule() {
            return Err(invalid(
                "measured funding schedule differs from signed selection",
            ));
        }
        let signed_controls = PhloControlsBinding::new(
            &self.preflight.intent.base.controls,
            limits.signed.intent.controls(),
        )
        .map_err(invalid)?;
        let view = signed_controls.view().map_err(invalid)?;
        let checked = check_offered_phlo_controls(
            schedule.schedule().environment,
            self.adopted.genesis().minimum_price(),
            i64::MAX as u64,
            PhloOffer {
                limit: i64::try_from(self.preflight.phlo_limit)
                    .map_err(|_| invalid("signed phlo limit exceeds native range"))?,
                price: i64::try_from(self.preflight.phlo_price)
                    .map_err(|_| invalid("signed phlo price exceeds native range"))?,
            },
            view.terms(),
            schedule.schedule(),
            self.preflight.phlo_limit,
        )
        .map_err(invalid)?;
        let controls = checked.controls();
        // DR-113: the bound of an offer is its signed limit.
        self.adopted
            .bind_execution_contract(controls, &schedule, rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource::SignedLimit)?;
        let measured = self
            .adopted
            .native_rules()
            .measure(
                &self.evaluation.byte_observations,
                limits.measured.observations,
            )
            .map_err(invalid)?;
        let regions = measured
            .region_demands(limits.measured.regions, budget)
            .map_err(invalid)?;
        let located = regions
            .locate_purses(limits.measured.purses, budget)
            .map_err(invalid)?;
        let demand = located
            .prepare_acquisition_demand(
                &schedule,
                selected.bytes(),
                limits.measured.acquisition,
                budget,
            )
            .map_err(invalid)?;
        let settlement_root: [u8; 32] = self
            .runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .bytes()
            .as_slice()
            .try_into()
            .map_err(|_| invalid("offered settlement checkpoint root is not 32 bytes"))?;
        if self.runtime.runtime.get_root().await.bytes() != settlement_root {
            return Err(invalid(
                "offered physical discovery changed its checkpoint root",
            ));
        }
        let post_stacks = manager.read_measured_prepaid_stacks_with_births(
            settlement_root,
            &demand,
            &self.evaluation.authority_stack_births,
            production.stack_capture,
            budget,
        )?;
        let born = derive_live_born_stacks(
            &self.evaluation.authority_stack_births,
            &post_stacks,
            limits.retained.funding.births,
            budget,
        )?;
        let permissions = restore_retained_permissions(
            &snapshot.wallets().authorization().record().sources,
            selected.bytes(),
            schedule.descriptor().classes.len(),
            limits.measured.settlement.matching.execution,
            budget,
        )?;
        let retained = derive_retained_amounts(
            &permissions,
            selected.bytes(),
            &born,
            limits.retained.funding.cells,
            budget,
        )?;
        check_retained_origin(
            &permissions,
            selected.bytes(),
            &retained,
            &born,
            limits.retained.funding.cells,
            budget,
        )?;
        if !input.retained.is_empty() {
            if input.retained.len() != retained.len() {
                return Err(invalid(
                    "caller retained funding differs from native births",
                ));
            }
            for row in input.retained {
                reserve(
                    budget,
                    HostWorkDimension::VerificationOperations,
                    retained.len(),
                )?;
                if retained.iter().filter(|derived| **derived == *row).count() != 1 {
                    return Err(invalid(
                        "caller retained funding differs from native births",
                    ));
                }
            }
        }
        let selection =
            input
                .inventory
                .select_measured_demand(&demand, limits.measured.demand, budget)?;
        let binding = input.inventory.bind_measured_demand(
            NativePrepaidDemandInput {
                expected_root: self.preflight.funding_root,
                controls,
                demand: &demand,
                draws: selection.draws(),
                demand_positions: selection.demand_positions(),
            },
            limits.measured.demand,
            budget,
        )?;
        let observed = check_counted_phlo_execution(
            controls,
            binding.witness(),
            limits.measured.settlement.matching.execution,
        )
        .map_err(invalid)?
        .with_retained_acquisitions(&retained, budget)
        .map_err(invalid)?;
        check_metered_usage(
            self.evaluation
                .byte_observations
                .has_complete_measurements(),
            self.evaluation.native_phlo_usage,
            observed.usage(),
        )?;
        if observed
            .usage()
            .checked_add(observed.retained_usage())
            .is_none_or(|usage| usage > self.preflight.phlo_limit)
        {
            return Err(invalid("native retained usage exceeds signed phlo limit"));
        }
        let fresh_phlo = observed.fresh_usage();
        let retained_phlo = observed.retained_usage();
        let summary = self
            .evaluation
            .economic_failures
            .union(classify_errors(&self.evaluation.errors, Some(budget)).map_err(invalid)?);
        if !summary.permits_retained_charge() {
            return Err(invalid("native offered execution has non-user failure"));
        }
        let failure_class = if summary
            .contains(rholang::rust::interpreter::accounting::phlo_execution::PhloFailure::User)
        {
            NativeCostFailureClass::UserFailure
        } else {
            NativeCostFailureClass::Success
        };
        let mut failures =
            [rholang::rust::interpreter::accounting::phlo_execution::PhloFailure::User; 4];
        let mut failure_count = 0;
        for failure in [
            rholang::rust::interpreter::accounting::phlo_execution::PhloFailure::User,
            rholang::rust::interpreter::accounting::phlo_execution::PhloFailure::Platform,
            rholang::rust::interpreter::accounting::phlo_execution::PhloFailure::Certificate,
            rholang::rust::interpreter::accounting::phlo_execution::PhloFailure::Unclassified,
        ] {
            if summary.contains(failure) {
                failures[failure_count] = failure;
                failure_count += 1;
            }
        }
        let obligations = project_phlo_obligations(
            observed,
            PhloOutcome::Accepted(&failures[..failure_count]),
            limits.policy.funding.obligations,
        )
        .map_err(invalid)?;
        let positions = derive_birth_positions(&obligations, &born, budget)?;
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            born.len()
                .checked_mul(size_of::<RetainedBirthFunding<'_>>())
                .ok_or_else(|| invalid("native birth claim allocation overflows"))?,
        )?;
        let mut derived_claims = Vec::new();
        derived_claims
            .try_reserve_exact(born.len())
            .map_err(|_| invalid("native birth claim allocation failed"))?;
        for (birth, row) in born.iter().zip(&positions) {
            derived_claims.push(RetainedBirthFunding {
                birth,
                obligation_positions: row,
            });
        }
        check_birth_positions(&obligations, &derived_claims, budget)?;
        if !birth_claims.is_empty() {
            match_birth_claims(
                &self.evaluation.authority_stack_births,
                birth_claims,
                limits.retained.funding.births,
                budget,
            )?;
            for expected in birth_claims {
                reserve(
                    budget,
                    HostWorkDimension::VerificationOperations,
                    derived_claims.len(),
                )?;
                if derived_claims
                    .iter()
                    .filter(|derived| {
                        derived.birth == expected.birth
                            && derived.obligation_positions == expected.obligation_positions
                    })
                    .count()
                    != 1
                {
                    return Err(invalid("caller birth claim differs from rooted discovery"));
                }
            }
        }
        let family_selection = select_rooted_native_family(
            snapshot,
            &obligations,
            &self.evaluation.authority_events,
            &self.evaluation.authority_byte_events,
            limits,
            budget,
        )?;
        let sources = &family_selection.sources;
        let eligible = &family_selection.eligible;
        let selected = &family_selection.selected;
        let cases = [PhloFundingCase {
            obligations: &obligations,
            eligible,
            assignment: &selected.assignments()[0],
        }];
        let family = check_phlo_funding_family(
            sources,
            &cases,
            family_selection.total_exposure_limit,
            limits.policy.funding,
        )
        .map_err(invalid)?;
        let intent = PhloFundingIntentBinding::new(
            snapshot.wallets().authorization().record(),
            limits.signed.intent,
        )
        .map_err(invalid)?;
        let view = intent.view().map_err(invalid)?;
        let terms = PhloFundingTerms {
            required_owner_ceilings: controls.terms().required_owner_ceilings,
            asset: controls.schedule().environment.asset,
            schedule_commitment: controls.schedule().commitment,
        };
        let policy = snapshot
            .bind_native_family_in_context(
                &view,
                &family,
                terms,
                limits.signed,
                limits.policy,
                &self.adopted,
                budget,
            )
            .map_err(invalid)?
            .ok_or_else(|| invalid("measured funding family has no valid cursor policy"))?;
        let permit = self.preflight.authorize_private_draft(
            envelope
                .identity()
                .as_bytes()
                .try_into()
                .map_err(|_| invalid("offered envelope identity is not 32 bytes"))?,
            self.evaluation
                .byte_observations
                .has_complete_measurements(),
            self.evaluation.native_phlo_usage,
        )?;
        let issuances = if self.grant_issuances.is_empty() {
            None
        } else {
            Some(NativeGrantIssuanceInput {
                verified: &self.grant_issuances,
                limits: offered_grant_transition_limits(),
            })
        };
        let prepared: Result<_, CasperError> = async {
            if self.runtime.runtime.get_root().await.bytes() != settlement_root {
                return Err(invalid("offered settlement checkpoint root changed"));
            }
            let checked = policy
                .capture_settlement(
                    observed,
                    PhloOutcome::Accepted(&failures[..failure_count]),
                    limits.measured.settlement.matching,
                    limits.measured.settlement.capture,
                    budget,
                )
                .map(|checked| checked.bind_execution_root(settlement_root))
                .map_err(invalid)?;
            let births = self
                .runtime
                .capture_retained_births(
                    &checked,
                    &self.adopted,
                    &derived_claims,
                    limits.retained,
                    budget,
                )
                .await?;
            let protocol = offered_funded_v6_limits();
            let wallet_receipt = NativeWalletReceiptLimits {
                wire: PhloWireLimits {
                    total_bytes: protocol.evidence.field_bytes,
                    field_bytes: protocol.evidence.field_bytes,
                },
                payers: protocol.envelope.payload.funding.sources,
            };
            let records = births.prepare_cell_records(production.retained_record, budget)?;
            let prepared_receipts = records.prepare_insertions(
                receipt_snapshot,
                production.receipt_bucket,
                production.retained_settlement.receipts,
                budget,
            )?;
            let consumption = NativePrepaidConsumption {
                captured: input.inventory.captured(),
                draws: selection.draws(),
                limits: production.stack_pop,
                execution: production.execution,
                cell: production.prepaid_cell,
                physical_cells: production.physical_cells,
                physical_bytes: production.physical_bytes,
            };
            let mut settlement = self
                .runtime
                .settle_checked_wallet_and_capture_balances(
                    manager,
                    &checked,
                    &permit,
                    reservation_id,
                    fee_address,
                    initial_rand,
                    wallet_receipt,
                    grants,
                    issuances,
                    budget,
                )
                .await?;
            let wallet_root: [u8; 32] = settlement
                .post_state_root
                .as_ref()
                .try_into()
                .map_err(|_| invalid("wallet settlement root must be 32 bytes"))?;
            let receipts = self
                .runtime
                .apply_prepaid_retained_after_wallet(
                    prepared_receipts,
                    consumption,
                    self.preflight.funding_root,
                    wallet_root,
                    production.retained_settlement,
                    budget,
                )
                .await?;
            let funding_case = encode_measured_funding_case(
                checked.capture().scoped().capture(),
                &selected.cursor_transitions()[0],
                protocol.funding_case,
                budget,
            )?;
            reserve(
                budget,
                HostWorkDimension::SearchStateBytes,
                receipts
                    .prepaid_changes
                    .len()
                    .checked_mul(size_of::<PrepaidReceiptChange<'_>>())
                    .ok_or_else(|| invalid("prepaid evidence reference size overflow"))?,
            )?;
            let mut changes = Vec::new();
            changes
                .try_reserve_exact(receipts.prepaid_changes.len())
                .map_err(|_| invalid("prepaid evidence reference allocation failed"))?;
            changes.extend(
                receipts
                    .prepaid_changes
                    .iter()
                    .map(|change| change.as_change()),
            );
            let prepaid_delta = encode_measured_prepaid_delta(
                &selection,
                records.records(),
                &changes,
                protocol.prepaid_delta,
                budget,
            )?;
            let recording =
                encode_measured_runtime_recording(&self.evaluation, production.recording, budget)?;
            let final_root: [u8; 32] = self
                .runtime
                .runtime
                .create_checkpoint()
                .await
                .root
                .bytes()
                .try_into()
                .map_err(|_| invalid("offered final root must be 32 bytes"))?;
            let wallet_settlement_log_events = settlement.settlement_log.len();
            settlement.settlement_log.extend(receipts.log);
            if failure_class == NativeCostFailureClass::Success {
                let mut user = user_mergeable;
                let wallet = std::mem::take(&mut settlement.mergeable);
                let map_bytes = wallet
                    .len()
                    .checked_mul(size_of::<(Par, MergeType)>() + 64)
                    .ok_or_else(|| invalid("mergeable channel map size overflow"))?;
                reserve(budget, HostWorkDimension::SearchStateBytes, map_bytes)?;
                user.try_reserve(wallet.len())
                    .map_err(|_| invalid("mergeable channel map allocation failed"))?;
                user.extend(wallet);
                settlement.mergeable = user;
            }
            let result = NativeScopedOfferedResult {
                settlement,
                wallet_settlement_log_events,
                final_root,
                funding_case,
                prepaid_delta,
                recording,
            };
            if result.funding_case.is_empty()
                || result.prepaid_delta.is_empty()
                || result.recording.budget.is_empty()
                || result.recording.journal.is_empty()
                || result.settlement.wallet_receipt.is_empty()
                || self.runtime.runtime.get_root().await.bytes() != result.final_root
            {
                return Err(invalid(
                    "offered producer has incomplete evidence or wrong final root",
                ));
            }
            let wallet_settlement_root: [u8; 32] = result
                .settlement
                .post_state_root
                .as_ref()
                .try_into()
                .map_err(|_| invalid("offered wallet settlement root must be 32 bytes"))?;
            let phlo_used = self
                .evaluation
                .native_phlo_usage
                .ok_or_else(|| invalid("offered producer lost measured phlo usage"))?;
            Ok((result, settlement_root, wallet_settlement_root, phlo_used))
        }
        .await;
        let (result, settlement_root, wallet_settlement_root, phlo_used) = prepared?;
        let prepared = NativeOfferedPreparedResult {
            funding_root: self.preflight.funding_root,
            execution_root: self.preflight.execution_root,
            settlement_root,
            wallet_settlement_root,
            final_root: result.final_root,
            phlo_used,
            fresh_phlo,
            retained_phlo,
            failure_class,
            funding_case: result.funding_case,
            prepaid_delta: result.prepaid_delta,
            recording: result.recording,
            wallet_receipt: result.settlement.wallet_receipt,
            wallet_settlement_log_events: result.wallet_settlement_log_events,
            resource_rev: result.settlement.resource_rev,
            fee_rev: result.settlement.fee_rev,
            rev_spent: result.settlement.rev_spent,
            user_log: self.replay_log.clone(),
            grant_drain_log: self.grant_issue_log.clone(),
            settlement_log: result.settlement.settlement_log,
            grant_changes: result.settlement.grant_changes,
            mergeable: result.settlement.mergeable,
        };
        let final_root = prepared.final_root;
        let mut prepared = prepared;
        let mergeable = std::mem::take(&mut prepared.mergeable);
        let processed = assemble_offered_processed_deploy(
            envelope,
            self.preflight,
            &self.adopted,
            prepared,
            budget,
        )?;
        Ok(NativeOfferedCandidateResult {
            processed,
            final_root,
            mergeable,
        })
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::{CostSignature, CostStack, Par};
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use rholang::rust::interpreter::accounting::authority::AuthorityBornStack;

    use super::*;

    fn budget() -> HostWorkBudget {
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000)))
    }

    #[test]
    fn born_stack_ids_come_from_exact_rooted_physical_matches() {
        let cell = CostSignature {
            value: Some(Value::Ground(vec![1, 2])),
        };
        let observed = [AuthorityStackBirth {
            produce_hash: [3; 32],
            cells: vec![cell.clone()],
        }];
        let stack = PurseStack {
            instance_id: [4; 32],
            source_hash: [3; 32],
            channel: Par::default(),
            datum_index: 0,
            random_state: Vec::new(),
            persistent: false,
            stack: CostStack { cells: vec![cell] },
        };
        let derived = derive_live_born_stacks(&observed, &[stack.clone()], 1, &budget()).unwrap();
        assert_eq!(derived[0].stack_id, [4; 32]);
        assert!(derive_live_born_stacks(&observed, &[], 1, &budget()).is_err());
        assert!(derive_live_born_stacks(&observed, &[stack.clone(), stack], 2, &budget()).is_err());
    }

    #[test]
    fn retained_birth_uses_unique_signed_resource_without_measured_demand() {
        let cell = CostSignature {
            value: Some(Value::Ground(vec![1, 2])),
        };
        let authority = cost_signature_to_sig(&cell).unwrap();
        let location =
            rholang::rust::interpreter::accounting::SignatureChannel::from_sig(&authority)
                .par
                .encode_to_vec();
        let terms = b"selected-schedule";
        let limits = PhloExecutionLimits {
            resource_entries: 1,
            authority_nodes: 8,
            key_bytes: 1024,
        };
        let first = PhloResource {
            location: &location,
            class: 1,
            acquisition_terms: terms,
            authority: &authority,
        }
        .wire_key(limits)
        .unwrap();
        let second = PhloResource {
            class: 2,
            ..PhloResource {
                location: &location,
                class: 1,
                acquisition_terms: terms,
                authority: &authority,
            }
        }
        .wire_key(limits)
        .unwrap();
        let birth = AuthorityBornStack {
            stack_id: [3; 32],
            produce_hash: [4; 32],
            cells: vec![cell],
        };
        let work = budget();
        let one = vec![RestoredPhloResource::from_wire_key(&first, limits, &work).unwrap()];
        let retained = derive_retained_amounts(&one, terms, &[birth.clone()], 1, &work).unwrap();
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].quantity, 1);
        check_retained_origin(&one, terms, &retained, &[birth.clone()], 1, &work).unwrap();
        let overstated = [PhloResourceAmount {
            quantity: 2,
            ..retained[0]
        }];
        assert!(
            check_retained_origin(&one, terms, &overstated, &[birth.clone()], 1, &work).is_err()
        );
        let ambiguous = vec![
            RestoredPhloResource::from_wire_key(&first, limits, &work).unwrap(),
            RestoredPhloResource::from_wire_key(&second, limits, &work).unwrap(),
        ];
        assert!(derive_retained_amounts(&ambiguous, terms, &[birth], 1, &work).is_err());
    }

    #[test]
    fn retained_claims_must_match_each_native_birth_exactly_once() {
        let cell = CostSignature {
            value: Some(Value::Ground(vec![1, 2])),
        };
        let observed = [AuthorityStackBirth {
            produce_hash: [3; 32],
            cells: vec![cell.clone()],
        }];
        let claimed = AuthorityBornStack {
            stack_id: [4; 32],
            produce_hash: [3; 32],
            cells: vec![cell.clone()],
        };
        let claims = [RetainedBirthFunding {
            birth: &claimed,
            obligation_positions: &[1],
        }];
        assert!(match_birth_claims(&observed, &claims, 1, &budget()).is_ok());
        assert!(match_birth_claims(&observed, &claims, 0, &budget()).is_err());
        let duplicate = [claims[0], claims[0]];
        assert!(match_birth_claims(&observed, &duplicate, 2, &budget()).is_err());
        let duplicate_observed = [observed[0].clone(), observed[0].clone()];
        let unobserved_claim = AuthorityBornStack {
            stack_id: [8; 32],
            produce_hash: [8; 32],
            cells: vec![cell.clone()],
        };
        let claims_with_unobserved = [claims[0], RetainedBirthFunding {
            birth: &unobserved_claim,
            obligation_positions: &[1],
        }];
        assert!(
            match_birth_claims(&duplicate_observed, &claims_with_unobserved, 2, &budget()).is_err()
        );
        let foreign = AuthorityBornStack {
            produce_hash: [5; 32],
            ..claimed.clone()
        };
        let foreign_claim = [RetainedBirthFunding {
            birth: &foreign,
            obligation_positions: &[1],
        }];
        assert!(match_birth_claims(&observed, &foreign_claim, 1, &budget()).is_err());
        let changed = AuthorityBornStack {
            cells: vec![CostSignature {
                value: Some(Value::Ground(vec![9])),
            }],
            ..claimed
        };
        let changed_claim = [RetainedBirthFunding {
            birth: &changed,
            obligation_positions: &[1],
        }];
        assert!(match_birth_claims(&observed, &changed_claim, 1, &budget()).is_err());
    }

    #[test]
    fn retained_authority_cannot_select_between_obligations() {
        let cell = CostSignature {
            value: Some(Value::Ground(vec![1, 2])),
        };
        let authority = cost_signature_to_sig(&cell).unwrap();
        let first = PhloResource {
            location: &[3],
            class: 0,
            acquisition_terms: &[4],
            authority: &authority,
        };
        let second = PhloResource {
            location: &[5],
            ..first
        };
        let keys = [
            PhloObligationKey::RetainedResource(first),
            PhloObligationKey::RetainedResource(second),
        ];
        assert!(unique_retained_position(&keys, &authority, &budget()).is_err());
        assert_eq!(
            unique_retained_position(&keys[..1], &authority, &budget()).unwrap(),
            0
        );
    }

    /// D-E3 (DR-110): a retained cell charges the conversion of the cell, the
    /// funding channel, one block inspection of the returned channel for its
    /// length computation, and the location bytes, all before it compares the
    /// permissions. With no permission it fails after those charges, and a
    /// budget that runs the same steps charges the same in every dimension.
    #[test]
    fn born_resource_inspects_the_returned_channel_once() {
        let cell = CostSignature {
            value: Some(Value::Ground(vec![6; 32])),
        };
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        assert!(born_resource(&cell, &[], b"terms", &budget).is_err());
        let mirror = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        let backing = |operations: usize, scanned: usize, allocation: usize| {
            reserve(
                &mirror,
                HostWorkDimension::VerificationOperations,
                operations.checked_mul(2).ok_or(BackingError::Overflow)?,
            )
            .map_err(|_| BackingError::Rejected)?;
            reserve(&mirror, HostWorkDimension::VerificationBytes, scanned)
                .map_err(|_| BackingError::Rejected)?;
            reserve(&mirror, HostWorkDimension::SearchStateBytes, allocation)
                .map_err(|_| BackingError::Rejected)
        };
        let authority = cost_signature_to_sig_metered(&cell, &backing).expect("a signature");
        let channel = funding_sig_channel_metered(&authority, &backing).expect("a channel");
        clone_backing::inspect_blocks(&channel, &backing).expect("an inspection");
        let size = channel.encoded_len();
        reserve(&mirror, HostWorkDimension::VerificationOperations, size * 2)
            .expect("an unlimited budget");
        reserve(&mirror, HostWorkDimension::SearchStateBytes, size).expect("an unlimited budget");
        for dimension in HostWorkDimension::ALL {
            assert_eq!(
                budget.usage(dimension).get(),
                mirror.usage(dimension).get(),
                "{dimension:?}"
            );
        }
    }

    /// The meter of `resource_equal_metered`: walker operations count twice.
    fn doubled_operations(
        budget: &HostWorkBudget,
    ) -> impl Fn(usize, usize, usize) -> Result<(), BackingError> + '_ {
        move |operations, scanned, allocation| {
            reserve(
                budget,
                HostWorkDimension::VerificationOperations,
                operations.checked_mul(2).ok_or(BackingError::Overflow)?,
            )
            .map_err(|_| BackingError::Rejected)?;
            reserve(budget, HostWorkDimension::VerificationBytes, scanned)
                .map_err(|_| BackingError::Rejected)?;
            reserve(budget, HostWorkDimension::SearchStateBytes, allocation)
                .map_err(|_| BackingError::Rejected)
        }
    }

    fn usage(budget: &HostWorkBudget) -> [u64; HostWorkDimension::ALL.len()] {
        HostWorkDimension::ALL.map(|dimension| budget.usage(dimension).get())
    }

    fn unlimited() -> HostWorkBudget {
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)))
    }

    /// D-E4 (DR-111): comparing two retained resources reserves the reads of
    /// both locations and term sets and one block inspection of each borrowed
    /// authority. A 4 KiB ground authority on one side adds exactly the growth
    /// of one block inspection. The comparison accepts its exact credit and is
    /// rejected one unit short in each dimension that it uses.
    #[test]
    fn resource_comparison_inspects_each_authority_once() {
        fn resource<'a>(authority: &'a Sig, location: &'a [u8]) -> PhloResource<'a> {
            PhloResource {
                location,
                class: 0,
                acquisition_terms: b"selected-schedule",
                authority,
            }
        }
        let mirror = |left: PhloResource<'_>, right: PhloResource<'_>| {
            let budget = unlimited();
            let bytes = left.location.len()
                + right.location.len()
                + left.acquisition_terms.len()
                + right.acquisition_terms.len();
            reserve(&budget, HostWorkDimension::VerificationOperations, bytes)
                .expect("an unlimited budget");
            reserve(&budget, HostWorkDimension::VerificationBytes, bytes)
                .expect("an unlimited budget");
            let backing = doubled_operations(&budget);
            clone_backing::inspect_blocks(left.authority, &backing).expect("an inspection");
            clone_backing::inspect_blocks(right.authority, &backing).expect("an inspection");
            usage(&budget)
        };
        let inspection = |authority: &Sig| {
            let budget = unlimited();
            clone_backing::inspect_blocks(authority, &doubled_operations(&budget))
                .expect("an inspection");
            usage(&budget)
        };
        let small = Sig::Ground(vec![1; 32]);
        let large = Sig::Ground(vec![1; 4_096]);
        let location = b"located-purse".to_vec();
        let mut usages = Vec::with_capacity(3);
        for (left, right) in [(&small, &small), (&small, &large), (&large, &small)] {
            let [left, right] = [resource(left, &location), resource(right, &location)];
            let budget = unlimited();
            let equal = resource_equal_metered(left, right, &budget).expect("an unlimited budget");
            assert_eq!(equal, left == right);
            assert_eq!(usage(&budget), mirror(left, right));
            usages.push(usage(&budget));
        }
        let [same, grown] = [inspection(&small), inspection(&large)];
        for side in [1, 2] {
            for dimension in 0..HostWorkDimension::ALL.len() {
                assert_eq!(
                    usages[side][dimension] - usages[0][dimension],
                    grown[dimension] - same[dimension],
                    "side {side}, dimension {dimension}"
                );
            }
        }

        let [left, right] = [resource(&large, &location), resource(&small, &location)];
        let expected = mirror(left, right);
        let limited = |units: [u64; HostWorkDimension::ALL.len()]| {
            let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(0));
            for (dimension, limit) in HostWorkDimension::ALL.into_iter().zip(units) {
                limits.set(dimension, HostWorkLimit::new(limit));
            }
            HostWorkBudget::new(limits)
        };
        resource_equal_metered(left, right, &limited(expected)).expect("the exact credit");
        for dimension in 0..expected.len() {
            if expected[dimension] > 0 {
                let mut short = expected;
                short[dimension] -= 1;
                assert!(
                    resource_equal_metered(left, right, &limited(short)).is_err(),
                    "dimension {dimension}"
                );
            }
        }
    }
}
