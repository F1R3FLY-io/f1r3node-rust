use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::Event as CasperEvent;
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use models::rust::native_cost_evidence::NativeCostFailureClass;
use models::rust::native_wallet_receipt::NativeWalletReceiptLimits;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::phlo_source::PhloSourcePolicyV1;
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    cost_signature_to_sig, funding_sig_channel, validate_stack_birth, AuthorityBornStack,
    AuthorityStackBirth,
};
use rholang::rust::interpreter::accounting::economic_failure::classify_errors;
use rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource;
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
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use rspace_plus_plus::rspace::trace::event::Event;

use super::held_capacity::{capture_debits, GuardMember, GuardRole};
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
    PrepaidReceiptLimits, PrepaidReceiptSnapshot, PrepaidStackCaptureLimits, PrepaidStackPopLimits,
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

fn match_birth_claims(
    observed: &[AuthorityStackBirth],
    claims: &[RetainedBirthFunding<'_>],
    maximum: usize,
) -> Result<(), CasperError> {
    if observed.len() != claims.len() || claims.len() > maximum {
        return Err(invalid(
            "retained birth claims differ from native execution",
        ));
    }
    let mut matched = Vec::new();
    matched
        .try_reserve_exact(claims.len())
        .map_err(|_| invalid("retained birth match allocation failed"))?;
    matched.resize(claims.len(), false);
    for claim in claims {
        validate_stack_birth(&claim.birth.cells, claim.birth.stack.as_ref()).map_err(invalid)?;
    }
    for birth in observed {
        validate_stack_birth(&birth.cells, birth.stack.as_ref()).map_err(invalid)?;
        let mut selected = None;
        for (index, claim) in claims.iter().enumerate() {
            if birth.produce_hash == claim.birth.produce_hash
                && birth.cells == claim.birth.cells
                && birth.stack == claim.birth.stack
            {
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
) -> Result<Vec<AuthorityBornStack>, CasperError> {
    if observed.len() > maximum {
        return Err(invalid("native birth count exceeds its limit"));
    }
    let mut births = Vec::new();
    births
        .try_reserve_exact(observed.len())
        .map_err(|_| invalid("native birth allocation failed"))?;
    for birth in observed {
        validate_stack_birth(&birth.cells, birth.stack.as_ref()).map_err(invalid)?;
        let mut found = None;
        for stack in rooted_stacks {
            if let Some(tree) = birth.stack.as_ref() {
                if stack.stack != *tree {
                    continue;
                }
            } else if !stack.stack.parallel_tail.is_empty() || stack.stack.cells != birth.cells {
                continue;
            }
            if stack.source_hash != birth.produce_hash {
                continue;
            }
            if found.replace(stack).is_some() {
                return Err(invalid("native birth identifies multiple rooted stacks"));
            }
        }
        let stack = found.ok_or_else(|| invalid("native birth is absent from rooted state"))?;
        births.push(AuthorityBornStack {
            stack_id: stack.instance_id,
            produce_hash: birth.produce_hash,
            cells: birth.cells.clone(),
            stack: birth.stack.clone(),
        });
    }
    Ok(births)
}

pub(super) fn restore_retained_permissions<'a>(
    sources: &'a [PhloSourcePolicyV1<'a>],
    terms: &[u8],
    class_count: usize,
    limits: PhloExecutionLimits,
) -> Result<Vec<RestoredPhloResource<'a>>, CasperError> {
    let count = sources
        .iter()
        .try_fold(0usize, |sum, source| {
            sum.checked_add(source.resources().len())
        })
        .ok_or_else(|| invalid("signed retained permission count overflows"))?;
    let mut restored = Vec::new();
    restored
        .try_reserve_exact(count)
        .map_err(|_| invalid("signed retained permission allocation failed"))?;
    for source in sources {
        for permission in source.resources() {
            if permission.acquisition_terms != terms {
                continue;
            }
            if usize::try_from(permission.class).map_or(true, |class| class >= class_count) {
                return Err(invalid("signed retained permission has an unknown class"));
            }
            restored
                .push(RestoredPhloResource::from_wire_key(permission, limits).map_err(invalid)?);
        }
    }
    Ok(restored)
}

/// Added by dedup (task 11415, D2): why `lane_location` fails. `born_resource`
/// and the check before execution (`gate.rs`) map each case to their own
/// message.
#[derive(Debug)]
pub(super) enum LaneLocationError {
    /// The signature does not convert to an authority.
    Authority(rholang::rust::interpreter::accounting::authority::AuthorityError),
    /// The authority is not a funding former.
    NotFundingFormer,
    /// The authority does not convert to its supply channel.
    Channel(rholang::rust::interpreter::accounting::authority::AuthorityError),
    /// The location does not fit in memory.
    Allocation,
    /// The channel does not encode.
    Encode(prost::EncodeError),
}

/// Added by dedup (task 11415, D2): the conversion of a cost signature to its
/// authority and its location. The location is the encoded supply channel of
/// the authority. `born_resource` and the check before execution (`gate.rs`,
/// `EstimatedResources::new`) share this conversion.
pub(super) fn lane_location(
    signature: &models::rhoapi::CostSignature,
) -> Result<(Sig, Vec<u8>), LaneLocationError> {
    let authority = cost_signature_to_sig(signature).map_err(LaneLocationError::Authority)?;
    if !authority.is_funding_former() {
        return Err(LaneLocationError::NotFundingFormer);
    }
    let channel = funding_sig_channel(&authority).map_err(LaneLocationError::Channel)?;
    let size = channel.encoded_len();
    let mut location = Vec::new();
    location
        .try_reserve_exact(size)
        .map_err(|_| LaneLocationError::Allocation)?;
    channel
        .encode(&mut location)
        .map_err(LaneLocationError::Encode)?;
    Ok((authority, location))
}

fn born_resource<'a>(
    cell: &models::rhoapi::CostSignature,
    permissions: &'a [RestoredPhloResource<'_>],
    terms: &[u8],
) -> Result<PhloResource<'a>, CasperError> {
    // Moved by dedup (task 11415) to `lane_location_metered` in
    // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/producer.rs`.
    // let backing = |operations: usize, scanned: usize, allocation: usize| {
    //     reserve(
    //         budget,
    //         HostWorkDimension::VerificationOperations,
    //         operations.checked_mul(2).ok_or(BackingError::Overflow)?,
    //     )
    //     .map_err(|_| BackingError::Rejected)?;
    //     reserve(budget, HostWorkDimension::VerificationBytes, scanned)
    //         .map_err(|_| BackingError::Rejected)?;
    //     reserve(budget, HostWorkDimension::SearchStateBytes, allocation)
    //         .map_err(|_| BackingError::Rejected)
    // };
    // let authority = cost_signature_to_sig(cell).map_err(invalid)?;
    // if !authority.is_funding_former() {
    //     return Err(invalid("native retained cell is not a funding former"));
    // }
    // let channel = funding_sig_channel(&authority).map_err(invalid)?;
    // // Added by D-E3 (DR-110): `funding_sig_channel` prepays only the
    // // release of its unsorted channel. The surplus of its per-level inspection
    // // paid the length computation below, so a block inspection prepays it
    // // here.
    // clone_backing::inspect_blocks(&channel)
    //     .map_err(|_| invalid("native retained channel inspection is rejected"))?;
    // let size = channel.encoded_len();
    // reserve(
    //     budget,
    //     HostWorkDimension::VerificationOperations,
    //     size.checked_mul(2)
    //         .ok_or_else(|| invalid("native retained channel work overflows"))?,
    // )?;
    // reserve(budget, HostWorkDimension::SearchStateBytes, size)?;
    // let mut location = Vec::new();
    // location
    //     .try_reserve_exact(size)
    //     .map_err(|_| invalid("native retained channel allocation failed"))?;
    // channel.encode(&mut location).map_err(invalid)?;
    let (authority, location) = lane_location(cell).map_err(|error| match error {
        LaneLocationError::Authority(error) | LaneLocationError::Channel(error) => invalid(error),
        LaneLocationError::NotFundingFormer => {
            invalid("native retained cell is not a funding former")
        }
        LaneLocationError::Allocation => invalid("native retained channel allocation failed"),
        LaneLocationError::Encode(error) => invalid(error),
    })?;
    let mut selected: Option<PhloResource<'a>> = None;
    for permission in permissions {
        let resource = permission.resource();
        let born = PhloResource {
            location: &location,
            class: resource.class,
            acquisition_terms: terms,
            authority: &authority,
        };
        if resource != born {
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

pub(super) fn derive_retained_amounts<'a>(
    permissions: &'a [RestoredPhloResource<'_>],
    terms: &[u8],
    births: &[AuthorityBornStack],
    maximum_cells: usize,
) -> Result<Vec<PhloResourceAmount<'a>>, CasperError> {
    let count = births
        .iter()
        .try_fold(0usize, |sum, birth| sum.checked_add(birth.cells.len()))
        .filter(|count| *count <= maximum_cells)
        .ok_or_else(|| invalid("native retained cell count exceeds its limit"))?;
    let mut retained: Vec<PhloResourceAmount<'a>> = Vec::new();
    retained
        .try_reserve_exact(count)
        .map_err(|_| invalid("native retained allocation failed"))?;
    for birth in births {
        for cell in &birth.cells {
            let resource = born_resource(cell, permissions, terms)?;
            let mut match_index = None;
            for (index, row) in retained.iter().enumerate() {
                if row.resource == resource {
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
) -> Result<Vec<Vec<usize>>, CasperError> {
    let mut positions = Vec::new();
    positions
        .try_reserve_exact(births.len())
        .map_err(|_| invalid("native birth positions allocation failed"))?;
    for birth in births {
        let mut row = Vec::new();
        row.try_reserve_exact(birth.cells.len())
            .map_err(|_| invalid("native birth positions allocation failed"))?;
        for cell in &birth.cells {
            let authority = cost_signature_to_sig(cell).map_err(invalid)?;
            row.push(unique_retained_position(obligations.keys(), &authority)?);
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
) -> Result<(), CasperError> {
    let count = births
        .iter()
        .try_fold(0usize, |sum, birth| sum.checked_add(birth.cells.len()));
    let count = count
        .filter(|count| *count <= maximum_cells)
        .ok_or_else(|| invalid("retained physical cell count exceeds its limit"))?;
    let mut attributed = Vec::<(PhloResource<'_>, u64)>::new();
    attributed
        .try_reserve_exact(count)
        .map_err(|_| invalid("retained attribution allocation failed"))?;
    for birth in births {
        for cell in &birth.cells {
            let resource = born_resource(cell, permissions, terms)?;
            let mut match_index = None;
            for (index, (candidate, _)) in attributed.iter().enumerate() {
                if *candidate == resource {
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
        let mut matches = 0;
        for row in retained {
            if row.quantity == quantity && row.resource == resource {
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
) -> Result<usize, CasperError> {
    let mut position = None;
    for (index, key) in keys.iter().enumerate() {
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
) -> Result<(), CasperError> {
    for birth in births {
        if birth.birth.cells.len() != birth.obligation_positions.len() {
            return Err(invalid("retained birth cell and obligation count differ"));
        }
        for (cell, claimed) in birth.birth.cells.iter().zip(birth.obligation_positions) {
            let authority = cost_signature_to_sig(cell).map_err(invalid)?;
            if unique_retained_position(obligations.keys(), &authority)? != *claimed {
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
    ) -> Result<NativeOfferedCandidateResult, CasperError> {
        let settlement = OfferedSettlementContext::from_block_inputs(&envelope, &self.block_data)?;
        let adopted = self.adopted.clone();
        let funding_root = self.preflight.funding_root;
        let preflight = self.preflight;
        let stacks = self
            .read_measured_prepaid_stacks(manager, production.family, production.stack_capture)
            .await?;
        let captured = manager.capture_prepaid_stacks(
            funding_root,
            &stacks,
            &adopted,
            production.stack_capture,
        )?;
        let inventory = captured.resource_inventory(inventory_limits)?;
        let receipt_count = stacks
            .len()
            .checked_add(self.evaluation.authority_stack_births.len())
            .ok_or_else(|| invalid("offered receipt key count overflows"))?;
        let mut receipt_ids = Vec::new();
        receipt_ids
            .try_reserve_exact(receipt_count)
            .map_err(|_| invalid("offered receipt key allocation failed"))?;
        for stack in &stacks {
            receipt_ids.push(PrepaidReceiptBucket::key_for_source(&stack.source_hash));
        }
        for birth in &self.evaluation.authority_stack_births {
            receipt_ids.push(PrepaidReceiptBucket::key_for_source(&birth.produce_hash));
        }
        receipt_ids.sort_unstable();
        receipt_ids.dedup();
        let receipt_snapshot = manager.capture_prepaid_receipts(
            funding_root,
            self.adopted.event_codec_epoch(),
            &receipt_ids,
            receipt_limits,
        )?;
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
    ) -> Result<NativeOfferedCandidateResult, CasperError> {
        // Changed by G1-7 (DR-65 item 5, design §8.3): a failed held-capacity
        // guard on the success path rolls the run back to the execution root
        // and settles it again as a user failure, from the restored balances
        // and with the capacities at R0 (user decision U3).
        // let result = self
        //     .produce_native_offered_candidate_inner(
        //         manager,
        //         envelope,
        //         snapshot,
        //         input,
        //         birth_claims,
        //         receipt_snapshot,
        //         grants,
        //         reservation_id,
        //         fee_address,
        //         initial_rand,
        //         production,
        //         budget,
        //     )
        //     .await;
        let first = self
            .produce_native_offered_candidate_inner(
                manager,
                envelope.clone(),
                snapshot,
                input,
                birth_claims,
                receipt_snapshot,
                grants,
                reservation_id,
                fee_address,
                initial_rand.clone(),
                production,
                false,
            )
            .await;
        let result = match first {
            Ok(ProducedOutcome::Settled(candidate)) => Ok(candidate),
            Ok(ProducedOutcome::GuardFailed) => match self.roll_back_for_guard().await {
                Ok(()) => match self
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
                        true,
                    )
                    .await
                {
                    Ok(ProducedOutcome::Settled(candidate)) => Ok(candidate),
                    Ok(ProducedOutcome::GuardFailed) => Err(invalid(
                        "the rolled-back pass of the held-capacity guard reached the guard",
                    )),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        if result.is_err() {
            let baseline = Blake2b256Hash::from_bytes(self.preflight.execution_root.to_vec());
            self.runtime.runtime.reset(&baseline).await?;
            if self.runtime.runtime.get_root().await.bytes() != self.preflight.execution_root {
                return Err(invalid("offered producer failed to restore its base root"));
            }
        }
        result
    }

    // Changed by G1-7 (DR-65 item 5): one pass after execution. The first
    // pass checks the held-capacity guard on the success path and stops with
    // `GuardFailed` before any settlement when the guard fails. The second
    // pass (`rolled_back`) settles the rolled-back run as a user failure.
    // async fn produce_native_offered_candidate_inner(
    //     &mut self,
    //     manager: &RuntimeManager,
    //     envelope: DeployEnvelope,
    //     snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
    //     input: NativeAttemptSettlementInput<'_, '_, '_>,
    //     birth_claims: &[RetainedBirthFunding<'_>],
    //     receipt_snapshot: &PrepaidReceiptSnapshot,
    //     grants: Option<NativeGrantSettlementInput<'_>>,
    //     reservation_id: [u8; 32],
    //     fee_address: &VaultAddress,
    //     initial_rand: Blake2b512Random,
    //     production: NativeOfferedProductionLimits,
    //     budget: &HostWorkBudget,
    // ) -> Result<NativeOfferedCandidateResult, CasperError> {
    #[allow(clippy::too_many_arguments)]
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
        rolled_back: bool,
    ) -> Result<ProducedOutcome, CasperError> {
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
            // Changed by G1-6 (DR-124): the bound is the resource bound of the
            // check before execution.
            // self.preflight.phlo_limit,
            self.gate.resource_bound(),
        )
        .map_err(invalid)?;
        let controls = checked.controls();
        // Changed by G1-6 (DR-124): the rebind takes the bound source of the
        // check (DR-124 amends DR-113 item 6).
        // // DR-113: the bound of an offer is its signed limit.
        // self.adopted
        //     .bind_execution_contract(controls, &schedule, rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource::SignedLimit)?;
        self.adopted
            .bind_execution_contract(controls, &schedule, self.gate.bound_source())?;
        let measured = self
            .adopted
            .native_rules()
            .measure(
                &self.evaluation.byte_observations,
                limits.measured.observations,
            )
            .map_err(invalid)?;
        let regions = measured
            .region_demands(limits.measured.regions)
            .map_err(invalid)?;
        let located = regions
            .locate_purses(limits.measured.purses)
            .map_err(invalid)?;
        let demand = located
            .prepare_acquisition_demand(&schedule, selected.bytes(), limits.measured.acquisition)
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
            &self.adopted,
            production.stack_capture,
        )?;
        let born = derive_live_born_stacks(
            &self.evaluation.authority_stack_births,
            &post_stacks,
            limits.retained.funding.births,
        )?;
        let permissions = restore_retained_permissions(
            &snapshot.wallets().authorization().record().sources,
            selected.bytes(),
            schedule.descriptor().classes.len(),
            limits.measured.settlement.matching.execution,
        )?;
        let retained = derive_retained_amounts(
            &permissions,
            selected.bytes(),
            &born,
            limits.retained.funding.cells,
        )?;
        check_retained_origin(
            &permissions,
            selected.bytes(),
            &retained,
            &born,
            limits.retained.funding.cells,
        )?;
        if !input.retained.is_empty() {
            if input.retained.len() != retained.len() {
                return Err(invalid(
                    "caller retained funding differs from native births",
                ));
            }
            for row in input.retained {
                if retained.iter().filter(|derived| **derived == *row).count() != 1 {
                    return Err(invalid(
                        "caller retained funding differs from native births",
                    ));
                }
            }
        }
        let branched = input.inventory.has_branched_stacks()?;
        let physical_trace =
            if branched {
                Some(input.inventory.checked_physical_trace(
                    snapshot.wallets(),
                    &self.evaluation.authority_events,
                )?)
            } else {
                None
            };
        let captured_for_trace = if branched {
            Some(input.inventory.clone_captured_stacks()?)
        } else {
            None
        };
        let verified_pre_state = match (&physical_trace, &captured_for_trace) {
            (Some(trace), Some(stacks)) => {
                Some(trace.verify_live(self.preflight.funding_root, stacks)?)
            }
            _ => None,
        };
        let selection = if let Some(verified) = verified_pre_state.as_ref() {
            input.inventory.select_verified_measured_demand(
                verified,
                &demand,
                limits.measured.demand,
            )?
        } else {
            input
                .inventory
                .select_measured_demand(&demand, limits.measured.demand)?
        };
        let demand_input = NativePrepaidDemandInput {
            expected_root: self.preflight.funding_root,
            controls,
            demand: &demand,
            draws: selection.draws(),
            demand_positions: selection.demand_positions(),
        };
        let binding = if let Some(verified) = verified_pre_state.as_ref() {
            input.inventory.bind_measured_demand_verified(
                demand_input,
                verified,
                limits.measured.demand,
            )?
        } else {
            input
                .inventory
                .bind_measured_demand(demand_input, limits.measured.demand)?
        };
        let observed = check_counted_phlo_execution(
            controls,
            binding.witness(),
            limits.measured.settlement.matching.execution,
        )
        .map_err(invalid)?
        .with_retained_acquisitions(&retained)
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
            .union(classify_errors(&self.evaluation.errors).map_err(invalid)?);
        if !summary.permits_retained_charge() {
            // Added by G1-6 (DR-124): a run that passes its certified bound is
            // a correctness failure (design §9). It is rejected with no charge.
            if let (NativeBoundSource::Certificate, true) = (
                self.gate.bound_source(),
                summary.contains(
                    rholang::rust::interpreter::accounting::phlo_execution::PhloFailure::Certificate,
                ),
            ) {
                manager.report_certificate_correctness_failure(
                    &self.preflight.envelope_identity,
                    &self.preflight.funding_root,
                    &self.gate,
                    "the run passed its certified bound",
                );
            }
            return Err(invalid("native offered execution has non-user failure"));
        }
        // Changed by G1-7 (DR-65 item 5): the rolled-back pass after a failed
        // held-capacity guard settles a user failure.
        // let failure_class = if summary
        //     .contains(rholang::rust::interpreter::accounting::phlo_execution::PhloFailure::User)
        // {
        let failure_class = if rolled_back
            || summary
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
            // Changed by G1-7: the rolled-back pass adds the user failure.
            // if summary.contains(failure) {
            let forced = rolled_back
                && matches!(
                    failure,
                    rholang::rust::interpreter::accounting::phlo_execution::PhloFailure::User
                );
            if summary.contains(failure) || forced {
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
        let positions = derive_birth_positions(&obligations, &born)?;
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
        check_birth_positions(&obligations, &derived_claims)?;
        if !birth_claims.is_empty() {
            match_birth_claims(
                &self.evaluation.authority_stack_births,
                birth_claims,
                limits.retained.funding.births,
            )?;
            for expected in birth_claims {
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
        // Changed by G1-6 (DR-124): a selection that fails after execution
        // under the bound source `Certificate` is a correctness failure
        // (design §7, arbitration LOW 6). It is rejected with no charge.
        // let family_selection = select_rooted_native_family(
        //     snapshot,
        //     &obligations,
        //     &self.evaluation.authority_events,
        //     &self.evaluation.authority_byte_events,
        //     limits,
        //     budget,
        // )?;
        let family_selection = match (
            select_rooted_native_family(
                snapshot,
                &obligations,
                &self.evaluation.authority_events,
                &self.evaluation.authority_byte_events,
                limits,
            ),
            self.gate.bound_source(),
        ) {
            (Ok(selection), _) => selection,
            (Err(error), NativeBoundSource::Certificate) => {
                manager.report_certificate_correctness_failure(
                    &self.preflight.envelope_identity,
                    &self.preflight.funding_root,
                    &self.gate,
                    &format!("the selection after execution failed: {error}"),
                );
                return Err(error);
            }
            (Err(error), NativeBoundSource::SignedLimit) => return Err(error),
        };
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
                )
                .map(|checked| checked.bind_execution_root(settlement_root))
                .map_err(invalid)?;
            // Added by G1-7 (DR-65 item 5, design §8.3): the held-capacity
            // guard on the success path. The debits come from the capture that
            // settlement would charge, and the balances from the settlement
            // root, where the run has changed them. A failing guard settles
            // nothing here, and the caller rolls the run back.
            if !rolled_back && matches!(failure_class, NativeCostFailureClass::Success) {
                let debits = capture_debits(checked.capture().amounts())?;
                let balances = manager
                    .purse_balances_at(
                        snapshot,
                        settlement_root,
                        &debits,
                        self.adopted.event_codec_epoch(),
                    )
                    .await?;
                let member = GuardMember {
                    deploy_id: &self.preflight.envelope_identity,
                    funding_root: &self.preflight.funding_root,
                    role: GuardRole::Play,
                };
                if manager
                    .held_capacity_guard_outcome(member, &balances, &debits)
                    .is_err()
                {
                    return Ok(None);
                }
            }
            let births = self
                .runtime
                .capture_retained_births(&checked, &self.adopted, &derived_claims, limits.retained)
                .await?;
            let protocol = offered_funded_v6_limits();
            let wallet_receipt = NativeWalletReceiptLimits {
                wire: PhloWireLimits {
                    total_bytes: protocol.evidence.field_bytes,
                    field_bytes: protocol.evidence.field_bytes,
                },
                payers: protocol.envelope.payload.funding.sources,
            };
            let records = births.prepare_cell_records(production.retained_record)?;
            let prepared_receipts = records.prepare_insertions(
                receipt_snapshot,
                production.receipt_bucket,
                production.retained_settlement.receipts,
            )?;
            let consumption = NativePrepaidConsumption {
                captured: input.inventory.captured(),
                draws: selection.draws(),
                checked_physical: physical_trace.as_ref(),
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
                    &self.adopted,
                    &checked,
                    &permit,
                    reservation_id,
                    fee_address,
                    initial_rand,
                    wallet_receipt,
                    grants,
                    issuances,
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
                )
                .await?;
            let funding_case = encode_measured_funding_case(
                checked.capture().scoped().capture(),
                &selected.cursor_transitions()[0],
                protocol.funding_case,
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
            )?;
            let recording =
                encode_measured_runtime_recording(&self.evaluation, production.recording)?;
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
            // Changed by G1-7: `None` marks a failed held-capacity guard.
            // Ok((result, settlement_root, wallet_settlement_root, phlo_used))
            Ok(Some((
                result,
                settlement_root,
                wallet_settlement_root,
                phlo_used,
            )))
        }
        .await;
        // Changed by G1-7 (DR-65 item 5): a failed guard ends the pass before
        // any settlement.
        // let (result, settlement_root, wallet_settlement_root, phlo_used) = prepared?;
        let Some((result, settlement_root, wallet_settlement_root, phlo_used)) = prepared? else {
            return Ok(ProducedOutcome::GuardFailed);
        };
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
        let processed =
            assemble_offered_processed_deploy(envelope, self.preflight, &self.adopted, prepared)?;
        // Changed by G1-7: the pass returns its settled candidate.
        // Ok(NativeOfferedCandidateResult {
        //     processed,
        //     final_root,
        //     mergeable,
        // })
        Ok(ProducedOutcome::Settled(NativeOfferedCandidateResult {
            processed,
            final_root,
            mergeable,
        }))
    }
}

/// Added by G1-7 (DR-65 item 5): the outcome of one pass of the producer after
/// execution.
enum ProducedOutcome {
    Settled(NativeOfferedCandidateResult),
    /// The held-capacity guard failed on the success path. The pass settled
    /// nothing.
    GuardFailed,
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::{CostSignature, CostStack, Par};
    use proptest::prelude::*;
    use rholang::rust::interpreter::accounting::authority::AuthorityBornStack;

    use super::*;

    #[test]
    fn born_stack_ids_come_from_exact_rooted_physical_matches() {
        let cell = CostSignature {
            value: Some(Value::Ground(vec![1, 2])),
        };
        let observed = [AuthorityStackBirth {
            produce_hash: [3; 32],
            cells: vec![cell.clone()],
            stack: None,
        }];
        let stack = PurseStack {
            instance_id: [4; 32],
            source_hash: [3; 32],
            channel: Par::default(),
            datum_index: 0,
            random_state: Vec::new(),
            persistent: false,
            stack: CostStack {
                cells: vec![cell],
                parallel_tail: Vec::new(),
            },
        };
        let derived = derive_live_born_stacks(&observed, &[stack.clone()], 1).unwrap();
        assert_eq!(derived[0].stack_id, [4; 32]);
        assert_eq!(derived[0].stack, None);
        assert!(derive_live_born_stacks(&observed, &[], 1).is_err());
        assert!(derive_live_born_stacks(&observed, &[stack.clone(), stack], 2).is_err());
    }

    #[test]
    fn born_stack_claims_authenticate_branch_topology_not_only_flat_cells() {
        let signature = |value| CostSignature {
            value: Some(Value::Ground(vec![value])),
        };
        let prefix = signature(1);
        let left = signature(2);
        let right = signature(3);
        let cells = vec![prefix.clone(), left.clone(), right.clone()];
        let tree = CostStack {
            cells: vec![prefix.clone()],
            parallel_tail: vec![
                CostStack {
                    cells: vec![left.clone()],
                    parallel_tail: Vec::new(),
                },
                CostStack {
                    cells: vec![right.clone()],
                    parallel_tail: Vec::new(),
                },
            ],
        };
        let regrouped = CostStack {
            cells: vec![prefix],
            parallel_tail: vec![
                CostStack {
                    cells: vec![left, right],
                    parallel_tail: Vec::new(),
                },
                CostStack {
                    cells: Vec::new(),
                    parallel_tail: Vec::new(),
                },
            ],
        };
        let observed = [AuthorityStackBirth {
            produce_hash: [3; 32],
            cells: cells.clone(),
            stack: Some(tree.clone()),
        }];
        let rooted = PurseStack {
            instance_id: [4; 32],
            source_hash: [3; 32],
            channel: Par::default(),
            datum_index: 0,
            random_state: Vec::new(),
            persistent: false,
            stack: tree.clone(),
        };
        let derived = derive_live_born_stacks(&observed, &[rooted.clone()], 1).unwrap();
        assert_eq!(derived[0].stack, Some(tree));

        let mut false_root = rooted.clone();
        false_root.stack = regrouped.clone();
        assert!(derive_live_born_stacks(&observed, &[false_root], 1).is_err());

        let mut reordered_root = rooted.clone();
        reordered_root.stack.parallel_tail.swap(0, 1);
        assert!(derive_live_born_stacks(&observed, &[reordered_root], 1).is_err());

        let mut false_claim = derived[0].clone();
        false_claim.stack = Some(regrouped);
        let claims = [RetainedBirthFunding {
            birth: &false_claim,
            obligation_positions: &[1],
        }];
        assert!(match_birth_claims(&observed, &claims, 1).is_err());

        let mut false_flat = observed[0].clone();
        false_flat.cells.swap(1, 2);
        assert!(derive_live_born_stacks(&[false_flat], &[rooted], 1).is_err());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn branch_tree_identity_is_required_even_when_flat_cells_match(
            prefix in any::<u8>(), left in any::<u8>(), right in any::<u8>()
        ) {
            let signature = |value| CostSignature { value: Some(Value::Ground(vec![value])) };
            let prefix = signature(prefix);
            let left = signature(left);
            let right = signature(right);
            let cells = vec![prefix.clone(), left.clone(), right.clone()];
            let tree = CostStack {
                cells: vec![prefix.clone()],
                parallel_tail: vec![
                    CostStack { cells: vec![left.clone()], parallel_tail: Vec::new() },
                    CostStack { cells: vec![right.clone()], parallel_tail: Vec::new() },
                ],
            };
            let flat_alias = CostStack {
                cells: vec![prefix],
                parallel_tail: vec![
                    CostStack { cells: vec![left, right], parallel_tail: Vec::new() },
                    CostStack { cells: Vec::new(), parallel_tail: Vec::new() },
                ],
            };
            let birth = AuthorityStackBirth {
                produce_hash: [3; 32],
                cells,
                stack: Some(tree.clone()),
            };
            prop_assert!(validate_stack_birth(&birth.cells, Some(&flat_alias)).is_ok());
            let rooted = PurseStack {
                instance_id: [4; 32],
                source_hash: birth.produce_hash,
                channel: Par::default(),
                datum_index: 0,
                random_state: Vec::new(),
                persistent: false,
                stack: tree,
            };
            prop_assert!(derive_live_born_stacks(&[birth.clone()], &[rooted.clone()], 1).is_ok());
            let forged = PurseStack { stack: flat_alias, ..rooted };
            prop_assert!(derive_live_born_stacks(&[birth], &[forged], 1).is_err());
        }
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
            stack: None,
        };
        let one = vec![RestoredPhloResource::from_wire_key(&first, limits).unwrap()];
        let retained = derive_retained_amounts(&one, terms, &[birth.clone()], 1).unwrap();
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].quantity, 1);
        check_retained_origin(&one, terms, &retained, &[birth.clone()], 1).unwrap();
        let overstated = [PhloResourceAmount {
            quantity: 2,
            ..retained[0]
        }];
        assert!(check_retained_origin(&one, terms, &overstated, &[birth.clone()], 1).is_err());
        let ambiguous = vec![
            RestoredPhloResource::from_wire_key(&first, limits).unwrap(),
            RestoredPhloResource::from_wire_key(&second, limits).unwrap(),
        ];
        assert!(derive_retained_amounts(&ambiguous, terms, &[birth], 1).is_err());
    }

    #[test]
    fn retained_claims_must_match_each_native_birth_exactly_once() {
        let cell = CostSignature {
            value: Some(Value::Ground(vec![1, 2])),
        };
        let observed = [AuthorityStackBirth {
            produce_hash: [3; 32],
            cells: vec![cell.clone()],
            stack: None,
        }];
        let claimed = AuthorityBornStack {
            stack_id: [4; 32],
            produce_hash: [3; 32],
            cells: vec![cell.clone()],
            stack: None,
        };
        let claims = [RetainedBirthFunding {
            birth: &claimed,
            obligation_positions: &[1],
        }];
        assert!(match_birth_claims(&observed, &claims, 1).is_ok());
        assert!(match_birth_claims(&observed, &claims, 0).is_err());
        let duplicate = [claims[0], claims[0]];
        assert!(match_birth_claims(&observed, &duplicate, 2).is_err());
        let duplicate_observed = [observed[0].clone(), observed[0].clone()];
        let unobserved_claim = AuthorityBornStack {
            stack_id: [8; 32],
            produce_hash: [8; 32],
            cells: vec![cell.clone()],
            stack: None,
        };
        let claims_with_unobserved = [claims[0], RetainedBirthFunding {
            birth: &unobserved_claim,
            obligation_positions: &[1],
        }];
        assert!(match_birth_claims(&duplicate_observed, &claims_with_unobserved, 2).is_err());
        let foreign = AuthorityBornStack {
            produce_hash: [5; 32],
            ..claimed.clone()
        };
        let foreign_claim = [RetainedBirthFunding {
            birth: &foreign,
            obligation_positions: &[1],
        }];
        assert!(match_birth_claims(&observed, &foreign_claim, 1).is_err());
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
        assert!(match_birth_claims(&observed, &changed_claim, 1).is_err());
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
        assert!(unique_retained_position(&keys, &authority).is_err());
        assert_eq!(unique_retained_position(&keys[..1], &authority).unwrap(), 0);
    }
}
