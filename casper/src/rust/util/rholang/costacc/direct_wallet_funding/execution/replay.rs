use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

// Changed by G3 (DR-135, S7): the demand filter names the region type.
// use models::rhoapi::Par;
use models::rhoapi::{CostRegion, Par};
use models::rust::casper::protocol::casper_message::Event as CasperEvent;
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::native_cost_evidence::NativeCostFailureClass;
use models::rust::native_wallet_receipt::NativeWalletReceiptLimits;
use models::rust::normalizer_env::normalizer_env_from_envelope;
use models::rust::phlo_grant_creation::VerifiedGrantCreationV1;
use models::rust::phlo_intent::PhloConversionCompositionV2;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
// Changed by dedup (task 11415, D5): the halves of the settlement pipeline
// name these types.
// use rholang::rust::interpreter::accounting::authority::AuthorityStackBirth;
use rholang::rust::interpreter::accounting::authority::{
    AuthorityBornStack, AuthorityByteEvent, AuthorityEvent, AuthorityStackBirth,
};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::economic_failure::classify_errors;
use rholang::rust::interpreter::accounting::native_phlo_rules::NativePhloAcquisitionDemand;
// Added by G3 (DR-135, S7): the binding of a recorded owner table (R3, R11).
use rholang::rust::interpreter::accounting::owner_funding::{
    recorded_owner_table, OwnerFundingBinding,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_offered_phlo_controls, CheckedPhloControls, PhloControlsBinding, PhloFundingTerms,
    PhloOffer, PhloScheduleBinding,
};
// Changed by dedup (task 11415, D5): the halves of the settlement pipeline
// name these types.
// use rholang::rust::interpreter::accounting::phlo_execution::{
//     check_counted_phlo_execution, check_phlo_funding_family, project_phlo_obligations, PhloFailure,
//     PhloFundingCase, PhloFundingIntentBinding, PhloOutcome, RetainedBirthFunding,
// };
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, check_phlo_funding_family, project_phlo_obligations,
    CheckedPhloExecution, CheckedPhloFundingFamily, CheckedPhloObligations, PhloFailure,
    PhloFundingCase, PhloFundingIntentBinding, PhloFundingIntentView, PhloOutcome,
    PhloResourceAmount, RestoredPhloResource, RetainedBirthFunding,
};
use rholang::rust::interpreter::accounting::{
    funding_sig, NativeBudgetRecording, NativeOperationJournalLimits, NativeOperationRecord,
    NativeOperationTraceLimits, NativeRecordingWireLimits, NativeRuntimeConfig, RuntimeBudget,
};
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::interpreter::InterpreterImpl;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::{create_native_replay_env, RhoRuntime};
use rholang::rust::interpreter::system_processes::DeployData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
// Moved by DR-115 with native_user_event_count to offered_evidence.rs.
// use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
use rspace_plus_plus::rspace::trace::event::Event;
use tokio::sync::RwLock;

// Added by dedup (task 11415, D5): the funding half of the settlement
// pipeline names the rooted selection.
use super::family_selection::NativeRootedFamilySelection;
// Changed by DR-134 (DR-65 item 5): the later holds of a member.
// use super::held_capacity::{capture_debits, GuardMember, GuardRole};
use super::held_capacity::{capture_debits, GuardMember, GuardRole, LaterHolds};
use super::producer::{
    check_birth_positions, check_retained_origin, derive_birth_positions, derive_live_born_stacks,
    derive_retained_amounts, restore_retained_permissions,
};
// Changed by G3 (DR-135, S7): the owner rows of a member (R10).
// use super::settlement::{
//     check_metered_usage, NativeAttemptSettlementInput, NativeAttemptSettlementLimits,
//     NativeCapturedSettlement, NativeFundedSettlement, NativeGrantIssuanceInput,
//     NativeGrantSettlementInput,
// };
use super::settlement::{
    check_metered_usage, NativeAttemptSettlementInput, NativeAttemptSettlementLimits,
    NativeCapturedSettlement, NativeFundedSettlement, NativeGrantIssuanceInput,
    NativeGrantSettlementInput, OwnerSettlement,
};
// Changed by dedup (task 11415, D5): the funding half of the settlement
// pipeline names the family limits.
// use super::{
//     select_rooted_native_family, CheckedDirectWalletPolicy, NativeFundedAttempt,
//     NativeFundedExecutionContext, NativeOfferedAttempt, NativeOfferedProductionLimits,
//     OfferedGateInput,
// };
use super::{
    select_rooted_native_family, CheckedDirectWalletPolicy, NativeFundedAttempt,
    NativeFundedExecutionContext, NativeOfferedAttempt, NativeOfferedFamilyLimits,
    NativeOfferedProductionLimits, OfferedGateInput,
};
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::event_converter;
use crate::rust::util::rholang::acceptance::{
    prepare_offered_candidate, CandidateReplayPermit, OfferedCandidateLimits,
    PreparedOfferedCandidate,
};
// Changed by dedup (task 11415, D5): the funding half of the settlement
// pipeline returns the captured settlement.
// use crate::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletPolicySnapshot;
use crate::rust::util::rholang::costacc::direct_wallet_funding::{
    CheckedDirectWalletSettlement, DirectWalletPolicySnapshot,
};
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
#[cfg(any(test, feature = "test-utils"))]
use crate::rust::util::rholang::costacc::offered_acceptance::{OfferedCheckRole, OfferedUsageKind};
use crate::rust::util::rholang::costacc::offered_context::OfferedSettlementContext;
use crate::rust::util::rholang::costacc::offered_evidence::{
    decode_committed_runtime_recording, encode_measured_funding_case,
    encode_measured_prepaid_delta, native_user_event_count,
};
use crate::rust::util::rholang::costacc::offered_grants::{
    grant_issue_definition, offered_grant_issue_call_limits, offered_grant_transition_limits,
};
// Changed by dedup (task 11415, D5): the measured half of the settlement
// pipeline names the selection and the binding.
// use crate::rust::util::rholang::costacc::prepaid_receipts::{
//     NativePrepaidConsumption, NativePrepaidDemandInput, NativePrepaidInventory,
//     NativePrepaidInventoryLimits, NativeRetainedReceiptRecords, NativeRetainedSettlementLimits,
//     PrepaidReceiptBucket, PrepaidReceiptChange, PrepaidReceiptLimits, PrepaidStackCaptureLimits,
//     PreparedNativeRetainedReceipts,
// };
use crate::rust::util::rholang::costacc::prepaid_receipts::{
    CanonicalPrepaidSelection, NativePrepaidConsumption, NativePrepaidDemandBinding,
    NativePrepaidDemandInput, NativePrepaidInventory, NativePrepaidInventoryLimits,
    NativeRetainedReceiptRecords, NativeRetainedSettlementLimits, PrepaidReceiptBucket,
    PrepaidReceiptLimits, PrepaidStackCaptureLimits, PreparedNativeRetainedReceipts,
};
use crate::rust::util::rholang::costacc::production_limits::{
    offered_funded_v6_production_limits, offered_funded_v6_replay_limits,
};
use crate::rust::util::rholang::costacc::supply::PurseStack;
// Added by G3 (DR-135, S7): the purse locator of the owner binding (R2).
use crate::rust::util::rholang::costacc::vault_payer::purse_of;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::tools::Tools;

#[derive(Clone, Copy)]
pub struct NativeFundedReplayInput<'a> {
    pub recording: &'a NativeBudgetRecording,
    pub operations: &'a Arc<[NativeOperationRecord]>,
    pub events: &'a Arc<[Event]>,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeFundedReplayLimits {
    pub journal: NativeOperationJournalLimits,
    pub trace: NativeOperationTraceLimits,
}

pub struct ReplayedNativeFundedUser<'policy, 'a> {
    attempt: NativeFundedAttempt<'policy, 'a>,
    user_event_count: usize,
    grant_issue_event_count: usize,
    grant_issuances: Vec<VerifiedGrantCreationV1>,
    expected_settlement_runtime_root: [u8; 32],
}

pub struct PendingReplayedNativeSettlement<'processed, 'policy, 'a> {
    attempt: NativeFundedAttempt<'policy, 'a>,
    permit: CandidateReplayPermit<'processed>,
    captured: NativeCapturedSettlement<'a>,
    wallet: NativeFundedSettlement,
}

pub(crate) struct NativeReplayedSettlementResult {
    pub final_root: [u8; 32],
    pub mergeable: HashMap<Par, MergeType>,
}

impl<'preflight, 'a> NativeOfferedAttempt<'preflight, 'a> {
    // Changed by DR-134 (DR-65 item 5, design §8.3): the settlement takes the
    // later holds of the member, and `snapshot` is the settlement snapshot of
    // the member: the wallets at its execution root R_{i-1}, with each source
    // capacity capped by the later holds (option B).
    // pub(crate) async fn settle_replayed_candidate(
    //     mut self,
    //     manager: &RuntimeManager,
    //     processed: &OfferedProcessedDeploy,
    //     snapshot: &DirectWalletPolicySnapshot<'a, OfferedFundedDeploy>,
    //     grants: Option<NativeGrantSettlementInput<'_>>,
    //     production: NativeOfferedProductionLimits,
    //     inventory_limits: NativePrepaidInventoryLimits,
    //     receipt_limits: PrepaidReceiptLimits,
    // ) -> Result<NativeReplayedSettlementResult, CasperError> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn settle_replayed_candidate(
        mut self,
        manager: &RuntimeManager,
        processed: &OfferedProcessedDeploy,
        snapshot: &DirectWalletPolicySnapshot<'a, OfferedFundedDeploy>,
        later: &LaterHolds,
        grants: Option<NativeGrantSettlementInput<'_>>,
        production: NativeOfferedProductionLimits,
        inventory_limits: NativePrepaidInventoryLimits,
        receipt_limits: PrepaidReceiptLimits,
    ) -> Result<NativeReplayedSettlementResult, CasperError> {
        let limits = production.family;
        let funding_root = self.preflight.funding_root;
        // Added by DR-134 (DR-65 amendment of 2026-10-12): the settlement
        // inputs of a member are read at its execution root R_{i-1}. The
        // evidence keeps R0 as its original funding root.
        let execution_root = self.preflight.execution_root;
        let evidence = processed
            .evidence(offered_funded_v6_limits().evidence)
            .map_err(invalid)?;
        let settlement_root: [u8; 32] = self
            .runtime
            .runtime
            .get_root()
            .await
            .bytes()
            .try_into()
            .map_err(|_| invalid("offered replay settlement root must be 32 bytes"))?;
        // Changed by DR-134 (DR-65 amendment of 2026-10-12): the settlement
        // snapshot belongs to the execution root.
        // if settlement_root != evidence.settlement_runtime_root
        //     || snapshot.wallets().pre_state_root() != funding_root
        //     || evidence.original_funding_root != funding_root
        // {
        if settlement_root != evidence.settlement_runtime_root
            || snapshot.wallets().pre_state_root() != execution_root
            || evidence.original_funding_root != funding_root
        {
            return Err(invalid(
                "offered replay settlement inputs have different roots",
            ));
        }
        let adopted = self.adopted.clone();
        let selected_terms = self.preflight.selected_terms(&adopted)?;
        let schedule =
            PhloScheduleBinding::new(selected_terms.schedule(), PhloGenesisPolicy::LIMITS)
                .map_err(invalid)?;
        let signed_controls = PhloControlsBinding::new(
            &self.preflight.intent.base.controls,
            limits.signed.intent.controls(),
        )
        .map_err(invalid)?;
        let signed_view = signed_controls.view().map_err(invalid)?;
        let offered_controls = check_offered_phlo_controls(
            schedule.schedule().environment,
            adopted.genesis().minimum_price(),
            i64::MAX as u64,
            PhloOffer {
                limit: i64::try_from(self.preflight.phlo_limit)
                    .map_err(|_| invalid("signed phlo limit exceeds native range"))?,
                price: i64::try_from(self.preflight.phlo_price)
                    .map_err(|_| invalid("signed phlo price exceeds native range"))?,
            },
            signed_view.terms(),
            schedule.schedule(),
            // Changed by G1-6 (DR-124): the bound is the resource bound of the
            // check that the witness replay ran.
            // self.preflight.phlo_limit,
            self.gate.resource_bound(),
        )
        .map_err(invalid)?;
        let controls = offered_controls.controls();
        // Changed by G1-6 (DR-124): the rebind takes the bound source of the
        // check (DR-124 amends DR-113 item 6).
        // // DR-113: the bound of an offer is its signed limit.
        // adopted.bind_execution_contract(controls, &schedule, rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource::SignedLimit)?;
        adopted.bind_execution_contract(controls, &schedule, self.gate.bound_source())?;
        // Added by G3 (DR-135, S7): the owner terms of the witness replay.
        // Each listed budget must be the budget of its purse at the execution
        // root (R3, §6.2 "After the run"), and the evidence must commit the
        // billed owner sums of the fold (G-4).
        if let Some(binding) = self.owner.as_deref() {
            manager
                .check_owner_table(binding.table(), execution_root, &self.gate, later)
                .await?;
        }
        let owners = self
            .owner_totals
            .as_ref()
            .map(OwnerSettlement::new)
            .transpose()?;
        let owner_usage = self.owner_totals.as_ref().map_or(0, |totals| totals.usage);
        let owner_rev = owners.as_ref().map_or(0, OwnerSettlement::owner_rev);
        if evidence.owner_phlo != owner_usage || evidence.owner_rev != owner_rev {
            return Err(invalid("offered replay owner terms differ from commitment"));
        }
        let owner_payers = owners
            .as_ref()
            .map(OwnerSettlement::payers)
            .unwrap_or_default();
        let measured = adopted
            .native_rules()
            .measure(
                &self.evaluation.byte_observations,
                limits.measured.observations,
            )
            .map_err(invalid)?;
        // Changed by G3 (DR-135, S7): the demand of the client leaves out the
        // owner regions (R10, G-2).
        // let regions = measured
        //     .region_demands(limits.measured.regions)
        //     .map_err(invalid)?;
        let regions = measured
            .region_demands_filtered(
                limits.measured.regions,
                Some(&|region: &CostRegion| self.keeps_client_region(region)),
            )
            .map_err(invalid)?;
        let located = regions
            .locate_purses(limits.measured.purses)
            .map_err(invalid)?;
        let demand = located
            .prepare_acquisition_demand(
                &schedule,
                selected_terms.bytes(),
                limits.measured.acquisition,
            )
            .map_err(invalid)?;
        // Changed by DR-134 (DR-65 amendment of 2026-10-12): the prepaid
        // stacks are read and captured at the execution root.
        // let original_stacks = manager.read_measured_prepaid_stacks(
        //     funding_root,
        //     &demand,
        //     production.stack_capture,
        // )?;
        // let captured_stacks = manager.capture_prepaid_stacks(
        //     funding_root,
        //     &original_stacks,
        //     &adopted,
        //     production.stack_capture,
        // )?;
        let original_stacks = manager.read_measured_prepaid_stacks(
            execution_root,
            &demand,
            production.stack_capture,
        )?;
        let captured_stacks = manager.capture_prepaid_stacks(
            execution_root,
            &original_stacks,
            &adopted,
            production.stack_capture,
        )?;
        let inventory = captured_stacks.resource_inventory(inventory_limits)?;
        // Added by G1-7 (DR-65 item 5, arbitration M2): a claimed guard
        // failure. The success path is recomputed at the root after the run,
        // with the births of the run, and the balances are read at that root,
        // as play reads them. The claim stands only if the guard fails. The
        // rolled-back run then has no births, and it settles from the
        // execution root.
        if self.guard_claimed {
            let post_run_root = self
                .post_run_root
                .ok_or_else(|| invalid("a claimed guard failure has no root after the run"))?;
            let debits = self
                .success_path_debits(
                    manager,
                    snapshot,
                    SuccessPathInput {
                        adopted: &adopted,
                        schedule: &schedule,
                        terms: selected_terms.bytes(),
                        controls,
                        inventory: &inventory,
                        demand: &demand,
                        post_run_root,
                    },
                    production,
                )
                .await?;
            // Added by G3 (DR-135, S7): the owner debits join the guard (§7
            // "Guard").
            let mut debits = debits;
            if let Some(owners) = &owners {
                owners.add_debits(&mut debits)?;
            }
            // Changed by DR-134 (DR-65 item 5): the guard checks the later
            // holds of the member.
            // let balances = manager
            //     .purse_balances_at(snapshot, post_run_root, &debits)
            //     .await?;
            // Changed by G3 (DR-135, S7): the guard reads the owner purses.
            // let balances = manager
            //     .purse_balances_at(snapshot, post_run_root, &debits, later)
            //     .await?;
            let balances = manager
                .purse_balances_at(snapshot, post_run_root, &debits, later, &owner_payers)
                .await?;
            let member = GuardMember {
                deploy_id: &self.preflight.envelope_identity,
                funding_root: &funding_root,
                role: GuardRole::Replay,
            };
            // Changed by DR-134 (DR-65 item 5).
            // if manager
            //     .held_capacity_guard_outcome(member, &balances, &debits)
            //     .is_ok()
            // {
            if manager
                .held_capacity_guard_outcome(member, &balances, &debits, later)
                .is_ok()
            {
                return Err(invalid(
                    "offered replay: the block commits a guard failure, but the held-capacity guard holds",
                ));
            }
            self.evaluation.authority_stack_births.clear();
        }
        // Moved by dedup (task 11415) to `measure_replayed_settlement` in
        // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/replay.rs`.
        // let post_stacks = manager.read_measured_prepaid_stacks_with_births(
        //     settlement_root,
        //     &demand,
        //     &self.evaluation.authority_stack_births,
        //     production.stack_capture,
        //     budget,
        // )?;
        // let born = derive_live_born_stacks(
        //     &self.evaluation.authority_stack_births,
        //     &post_stacks,
        //     limits.retained.funding.births,
        //     budget,
        // )?;
        // let permissions = restore_retained_permissions(
        //     &snapshot.wallets().authorization().record().sources,
        //     selected_terms.bytes(),
        //     schedule.descriptor().classes.len(),
        //     limits.measured.settlement.matching.execution,
        //     budget,
        // )?;
        // let retained = derive_retained_amounts(
        //     &permissions,
        //     selected_terms.bytes(),
        //     &born,
        //     limits.retained.funding.cells,
        //     budget,
        // )?;
        // check_retained_origin(
        //     &permissions,
        //     selected_terms.bytes(),
        //     &retained,
        //     &born,
        //     limits.retained.funding.cells,
        //     budget,
        // )?;
        // let selection =
        //     inventory.select_measured_demand(&demand, limits.measured.demand, budget)?;
        // let binding = inventory.bind_measured_demand(
        //     NativePrepaidDemandInput {
        //         expected_root: funding_root,
        //         controls,
        //         demand: &demand,
        //         draws: selection.draws(),
        //         demand_positions: selection.demand_positions(),
        //     },
        //     limits.measured.demand,
        //     budget,
        // )?;
        // let observed = check_counted_phlo_execution(
        //     controls,
        //     binding.witness(),
        //     limits.measured.settlement.matching.execution,
        // )
        // .map_err(invalid)?
        // .with_retained_acquisitions(&retained, budget)
        // .map_err(invalid)?;
        let mut measured_slots = MeasuredSettlementSlots::default();
        let MeasuredSettlement {
            born,
            selection,
            observed,
        } = measure_replayed_settlement(
            manager,
            &mut measured_slots,
            MeasuredSettlementInput {
                root: settlement_root,
                // Changed by DR-134 (DR-65 amendment of 2026-10-12): the
                // inventory belongs to the execution root.
                // funding_root,
                inventory_root: execution_root,
                births: &self.evaluation.authority_stack_births,
                snapshot,
                schedule: &schedule,
                terms: selected_terms.bytes(),
                controls,
                inventory: &inventory,
                demand: &demand,
            },
            production,
        )?;
        // Changed by G3 (DR-135, S7): the meter also charged the owner phlo,
        // and the fresh phlo of the evidence holds it (R10).
        // check_metered_usage(
        //     self.evaluation
        //         .byte_observations
        //         .has_complete_measurements(),
        //     self.evaluation.native_phlo_usage,
        //     observed.usage(),
        // )?;
        // if observed.fresh_usage() != evidence.fresh_phlo
        check_metered_usage(
            self.evaluation
                .byte_observations
                .has_complete_measurements(),
            self.evaluation.native_phlo_usage,
            observed
                .usage()
                .checked_add(owner_usage)
                .ok_or_else(|| invalid("offered replay owner usage overflows"))?,
        )?;
        if observed.fresh_usage().checked_add(owner_usage) != Some(evidence.fresh_phlo)
            || observed.retained_usage() != evidence.retained_phlo
            || observed
                .usage()
                .checked_add(observed.retained_usage())
                .is_none_or(|usage| usage > evidence.phlo_limit)
        {
            return Err(invalid(
                "offered replay funding units differ from commitment",
            ));
        }
        let failures = self
            .evaluation
            .economic_failures
            .union(classify_errors(&self.evaluation.errors).map_err(invalid)?);
        if !failures.permits_retained_charge() {
            return Err(invalid("offered replay has a non-user economic failure"));
        }
        let user_failure = failures.contains(PhloFailure::User);
        // Changed by G1-7 (arbitration M2): one flag, "user failure or guard
        // failure". The guard of a claimed guard failure failed above.
        // if user_failure != (evidence.failure_class == NativeCostFailureClass::UserFailure) {
        //     return Err(invalid("offered replay failure differs from commitment"));
        // }
        // let user_failures = [PhloFailure::User];
        // let outcome = PhloOutcome::Accepted(if user_failure { &user_failures } else { &[] });
        let rolled_back = user_failure || self.guard_claimed;
        if rolled_back != (evidence.failure_class == NativeCostFailureClass::UserFailure) {
            return Err(invalid("offered replay failure differs from commitment"));
        }
        let user_failures = [PhloFailure::User];
        let outcome = PhloOutcome::Accepted(if rolled_back { &user_failures } else { &[] });
        let obligations =
            project_phlo_obligations(observed, outcome, limits.policy.funding.obligations)
                .map_err(invalid)?;
        let positions = derive_birth_positions(&obligations, &born)?;
        let mut derived_claims = Vec::new();
        derived_claims
            .try_reserve_exact(born.len())
            .map_err(|_| invalid("offered replay birth claim allocation failed"))?;
        for (birth, row) in born.iter().zip(&positions) {
            derived_claims.push(RetainedBirthFunding {
                birth,
                obligation_positions: row,
            });
        }
        check_birth_positions(&obligations, &derived_claims)?;
        // Moved by dedup (task 11415) to `capture_replayed_funding` in
        // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/replay.rs`.
        // let family_selection = select_rooted_native_family(
        //     snapshot,
        //     &obligations,
        //     &self.evaluation.authority_events,
        //     &self.evaluation.authority_byte_events,
        //     limits,
        //     budget,
        // )?;
        // let cases = [PhloFundingCase {
        //     obligations: &obligations,
        //     eligible: &family_selection.eligible,
        //     assignment: &family_selection.selected.assignments()[0],
        // }];
        // let family = check_phlo_funding_family(
        //     &family_selection.sources,
        //     &cases,
        //     family_selection.total_exposure_limit,
        //     limits.policy.funding,
        // )
        // .map_err(invalid)?;
        // let intent = PhloFundingIntentBinding::new(
        //     snapshot.wallets().authorization().record(),
        //     limits.signed.intent,
        // )
        // .map_err(invalid)?;
        // let view = intent.view().map_err(invalid)?;
        // let terms = PhloFundingTerms {
        //     required_owner_ceilings: controls.terms().required_owner_ceilings,
        //     asset: controls.schedule().environment.asset,
        //     schedule_commitment: controls.schedule().commitment,
        // };
        // let policy = snapshot
        //     .bind_native_family_in_context(
        //         &view,
        //         &family,
        //         terms,
        //         limits.signed,
        //         limits.policy,
        //         &adopted,
        //         budget,
        //     )
        //     .map_err(invalid)?
        //     .ok_or_else(|| invalid("replayed funding family has no valid cursor policy"))?;
        // let checked = policy
        //     .capture_settlement(
        //         observed,
        //         outcome,
        //         limits.measured.settlement.matching,
        //         limits.measured.settlement.capture,
        //         budget,
        //     )
        //     .map(|checked| checked.bind_execution_root(settlement_root))
        //     .map_err(invalid)?;
        let mut capture_slots = FundingCaptureSlots::default();
        let FundingCapture {
            family_selection,
            checked,
        } = capture_replayed_funding(
            &mut capture_slots,
            FundingCaptureInput {
                snapshot,
                obligations: &obligations,
                observed,
                outcome,
                root: settlement_root,
                controls,
                adopted: &adopted,
                authority_events: &self.evaluation.authority_events,
                authority_byte_events: &self.evaluation.authority_byte_events,
                no_case: "replayed funding family has no case",
                no_policy: "replayed funding family has no valid cursor policy",
            },
            limits,
        )?;
        // Added by DR-134 (DR-65 item 5, O-13): with later holds, replay
        // recomputes the guard of a committed success, from the debits that
        // settlement charges and the balances at the settlement root, as play
        // does. A success whose guard fails is a block that a correct producer
        // does not make. Without later holds, the wallet settlement itself
        // enforces the guard, so replay checks nothing more.
        if !rolled_back && !later.is_empty() {
            // Changed by G3 (DR-135, S7): the owner debits join the guard,
            // and the guard reads the owner purses (§7 "Guard").
            // let debits = capture_debits(checked.capture().amounts())?;
            // let balances = manager
            //     .purse_balances_at(snapshot, settlement_root, &debits, later)
            //     .await?;
            let mut debits = capture_debits(checked.capture().amounts())?;
            if let Some(owners) = &owners {
                owners.add_debits(&mut debits)?;
            }
            let balances = manager
                .purse_balances_at(snapshot, settlement_root, &debits, later, &owner_payers)
                .await?;
            let member = GuardMember {
                deploy_id: &self.preflight.envelope_identity,
                funding_root: &funding_root,
                role: GuardRole::Replay,
            };
            if manager
                .held_capacity_guard_outcome(member, &balances, &debits, later)
                .is_err()
            {
                return Err(invalid(
                    "offered replay: the block commits a success, but the held-capacity guard fails",
                ));
            }
        }
        let births = self
            .runtime
            .capture_retained_births(&checked, &adopted, &derived_claims, limits.retained)
            .await?;
        let records = births.prepare_cell_records(production.retained_record)?;
        let receipt_count = original_stacks
            .len()
            .checked_add(self.evaluation.authority_stack_births.len())
            .ok_or_else(|| invalid("offered replay receipt key count overflows"))?;
        let mut receipt_ids = Vec::new();
        receipt_ids
            .try_reserve_exact(receipt_count)
            .map_err(|_| invalid("offered replay receipt key allocation failed"))?;
        for stack in &original_stacks {
            receipt_ids.push(PrepaidReceiptBucket::key_for_source(&stack.source_hash));
        }
        for birth in &self.evaluation.authority_stack_births {
            receipt_ids.push(PrepaidReceiptBucket::key_for_source(&birth.produce_hash));
        }
        receipt_ids.sort_unstable();
        receipt_ids.dedup();
        // Changed by DR-134 (DR-65 amendment of 2026-10-12): the receipts are
        // captured at the execution root.
        // let receipt_snapshot =
        //     manager.capture_prepaid_receipts(funding_root, &receipt_ids, receipt_limits)?;
        let receipt_snapshot =
            manager.capture_prepaid_receipts(execution_root, &receipt_ids, receipt_limits)?;
        let prepared_receipts = records.prepare_insertions(
            &receipt_snapshot,
            production.receipt_bucket,
            production.retained_settlement.receipts,
        )?;
        let permit = CandidateReplayPermit::authorize(
            processed,
            settlement_root,
            self.replay_log.len(),
            self.grant_issue_log.len(),
        )?;
        let funding_case = encode_measured_funding_case(
            checked.capture().scoped().capture(),
            &family_selection.selected.cursor_transitions()[0],
            offered_funded_v6_limits().funding_case,
        )?;
        permit.verify_funding_case(&funding_case)?;
        let (wallet_events, receipt_events) = permit.committed_settlement_segments()?;
        let trace_limits = offered_funded_v6_replay_limits().trace;
        let wallet_events = checked_replay_events(
            wallet_events,
            trace_limits,
            offered_funded_v6_limits().deploy_log_bytes,
        )?;
        let receipt_events = checked_replay_events(
            receipt_events,
            trace_limits,
            offered_funded_v6_limits().deploy_log_bytes,
        )?;
        let mut wallet_log = Vec::new();
        wallet_log
            .try_reserve_exact(wallet_events.len())
            .map_err(|_| invalid("offered wallet replay log allocation failed"))?;
        wallet_log.extend(wallet_events.iter().cloned());
        let mut replay_runtime = manager
            .spawn_offered_replay_runtime(offered_grant_issue_call_limits())
            .await?;
        replay_runtime.set_block_data(self.block_data.clone()).await;
        replay_runtime
            .set_deploy_data(DeployData::from_envelope(processed.envelope()))
            .await;
        replay_runtime
            .reset(&Blake2b256Hash::from_bytes(settlement_root.to_vec()))
            .await?;
        if replay_runtime.get_root().await.bytes() != settlement_root {
            return Err(invalid("offered wallet replay starts from another root"));
        }
        replay_runtime.rig(wallet_log).await?;
        self.runtime = RuntimeOps::new(replay_runtime);
        let context =
            OfferedSettlementContext::from_block_inputs(processed.envelope(), &self.block_data)?;
        let issuances = (!self.grant_issuances.is_empty()).then_some(NativeGrantIssuanceInput {
            verified: &self.grant_issuances,
            limits: offered_grant_transition_limits(),
        });
        let wallet_receipt_limits = NativeWalletReceiptLimits {
            wire: PhloWireLimits {
                total_bytes: offered_funded_v6_limits().evidence.field_bytes,
                field_bytes: offered_funded_v6_limits().evidence.field_bytes,
            },
            payers: offered_funded_v6_limits().envelope.payload.funding.sources,
        };
        let mut wallet = self
            .runtime
            .prepare_replayed_wallet_settlement(
                manager,
                &checked,
                &permit,
                context.reservation_id,
                &context.fee_address,
                context.initial_rand,
                wallet_receipt_limits,
                grants,
                issuances,
                // Added by G3 (DR-135, S7): the owners pay their billed
                // debits (R10).
                owners.as_ref(),
            )
            .await?;
        let rigged: Result<(), CasperError> = async {
            let mut receipt_log = Vec::new();
            receipt_log
                .try_reserve_exact(receipt_events.len())
                .map_err(|_| invalid("offered receipt replay log allocation failed"))?;
            receipt_log.extend(receipt_events.iter().cloned());
            self.runtime.runtime.rig(receipt_log).await?;
            Ok(())
        }
        .await;
        if let Err(error) = rigged {
            self.runtime
                .runtime
                .reset(&Blake2b256Hash::from_bytes(settlement_root.to_vec()))
                .await?;
            if self.runtime.runtime.get_root().await.bytes() != settlement_root {
                return Err(invalid("offered replay could not restore settlement root"));
            }
            return Err(error);
        }
        let completion: Result<[u8; 32], CasperError> = async {
            let consumption = NativePrepaidConsumption {
                captured: inventory.captured(),
                draws: selection.draws(),
                limits: production.stack_pop,
                execution: production.execution,
                cell: production.prepaid_cell,
                physical_cells: production.physical_cells,
                physical_bytes: production.physical_bytes,
            };
            let wallet_root: [u8; 32] = wallet
                .post_state_root
                .as_ref()
                .try_into()
                .map_err(|_| invalid("offered replay wallet root must be 32 bytes"))?;
            let receipts = self
                .runtime
                .apply_prepaid_retained_after_wallet(
                    prepared_receipts,
                    consumption,
                    // Changed by DR-134 (DR-65 amendment of 2026-10-12): the
                    // retained apply starts from the execution root.
                    // funding_root,
                    execution_root,
                    wallet_root,
                    production.retained_settlement,
                )
                .await?;
            let count = receipts.prepaid_changes.len();
            let mut changes = Vec::new();
            changes
                .try_reserve_exact(count)
                .map_err(|_| invalid("offered replay prepaid evidence allocation failed"))?;
            changes.extend(
                receipts
                    .prepaid_changes
                    .iter()
                    .map(|change| change.as_change()),
            );
            let prepaid_delta = encode_measured_prepaid_delta(
                // Changed by dedup (task 11415, D5): `measure_replayed_settlement`
                // returns the selection as a reference.
                // &selection,
                selection,
                records.records(),
                &changes,
                offered_funded_v6_limits().prepaid_delta,
            )?;
            permit.verify_prepaid_delta(&prepaid_delta)?;
            self.runtime.runtime.check_replay_data().await?;
            let final_root: [u8; 32] = self
                .runtime
                .runtime
                .create_checkpoint()
                .await
                .root
                .bytes()
                .try_into()
                .map_err(|_| invalid("offered replay final root must be 32 bytes"))?;
            permit.verify_complete(
                &final_root,
                &wallet.wallet_receipt,
                wallet.resource_rev,
                wallet.fee_rev,
            )?;
            wallet
                .settlement_log
                .try_reserve_exact(receipts.log.len())
                .map_err(|_| invalid("offered replay receipt log allocation failed"))?;
            wallet.settlement_log.extend(receipts.log);
            Ok(final_root)
        }
        .await;
        if completion.is_err() {
            self.runtime
                .runtime
                .reset(&Blake2b256Hash::from_bytes(settlement_root.to_vec()))
                .await?;
            if self.runtime.runtime.get_root().await.bytes() != settlement_root {
                return Err(invalid("offered replay could not restore settlement root"));
            }
        }
        let final_root = completion?;
        let mergeable = (|| {
            // Changed by G1-7 (arbitration M2): a rolled-back run, by a user
            // failure or by a guard failure, keeps no mergeable map of its own.
            // let mut mergeable = if user_failure {
            let mut mergeable = if rolled_back {
                HashMap::new()
            } else {
                std::mem::take(&mut self.evaluation.mergeable)
            };
            let wallet_mergeable = std::mem::take(&mut wallet.mergeable);
            mergeable
                .try_reserve(wallet_mergeable.len())
                .map_err(|_| invalid("offered replay mergeable map allocation failed"))?;
            mergeable.extend(wallet_mergeable);
            Ok::<_, CasperError>(mergeable)
        })();
        let mergeable = match mergeable {
            Ok(mergeable) => mergeable,
            Err(error) => {
                self.runtime
                    .runtime
                    .reset(&Blake2b256Hash::from_bytes(settlement_root.to_vec()))
                    .await?;
                if self.runtime.runtime.get_root().await.bytes() != settlement_root {
                    return Err(invalid("offered replay could not restore settlement root"));
                }
                return Err(error);
            }
        };
        Ok(NativeReplayedSettlementResult {
            final_root,
            mergeable,
        })
    }
}

/// Added by dedup (task 11415, D5): the inputs of the measured half of the
/// replayed settlement pipeline. The root is a parameter:
/// `settle_replayed_candidate` passes the settlement root, and
/// `success_path_debits` passes the root after the run.
#[derive(Clone, Copy)]
struct MeasuredSettlementInput<'r, 's, 'p> {
    root: [u8; 32],
    // Changed by DR-134 (DR-65 amendment of 2026-10-12): the root of the
    // captured inventory, the execution root of the member.
    // funding_root: [u8; 32],
    inventory_root: [u8; 32],
    births: &'r [AuthorityStackBirth],
    snapshot: &'r DirectWalletPolicySnapshot<'r, OfferedFundedDeploy>,
    schedule: &'r PhloScheduleBinding<'r>,
    terms: &'r [u8],
    controls: CheckedPhloControls<'r>,
    inventory: &'r NativePrepaidInventory<'s, 'p>,
    demand: &'r NativePhloAcquisitionDemand<'r>,
}

/// Added by dedup (task 11415, D5): the values of the measured half that the
/// observed execution borrows. The caller owns them, so the observed
/// execution outlives the call.
#[derive(Default)]
struct MeasuredSettlementSlots<'r> {
    permissions: Option<Vec<RestoredPhloResource<'r>>>,
    retained: Option<Vec<PhloResourceAmount<'r>>>,
    selection: Option<CanonicalPrepaidSelection>,
    binding: Option<NativePrepaidDemandBinding<'r>>,
}

/// Added by dedup (task 11415, D5): the result of the measured half. It holds
/// the live born stacks, the prepaid selection and the observed execution
/// with its retained acquisitions.
struct MeasuredSettlement<'r> {
    born: Vec<AuthorityBornStack>,
    selection: &'r CanonicalPrepaidSelection,
    observed: CheckedPhloExecution<'r>,
}

/// Added by dedup (task 11415, D5): the measured half of the replayed
/// settlement pipeline, from the stacks at the root to the observed execution.
/// The steps and their errors are those that `settle_replayed_candidate` and
/// `success_path_debits` ran, in the same order.
fn measure_replayed_settlement<'r>(
    manager: &RuntimeManager,
    slots: &'r mut MeasuredSettlementSlots<'r>,
    input: MeasuredSettlementInput<'r, '_, '_>,
    production: NativeOfferedProductionLimits,
) -> Result<MeasuredSettlement<'r>, CasperError> {
    let limits = production.family;
    let MeasuredSettlementSlots {
        permissions: permission_slot,
        retained: retained_slot,
        selection: selection_slot,
        binding: binding_slot,
    } = slots;
    let post_stacks = manager.read_measured_prepaid_stacks_with_births(
        input.root,
        input.demand,
        input.births,
        production.stack_capture,
    )?;
    let born = derive_live_born_stacks(input.births, &post_stacks, limits.retained.funding.births)?;
    let permissions: &'r [RestoredPhloResource<'r>] =
        permission_slot.insert(restore_retained_permissions(
            &input.snapshot.wallets().authorization().record().sources,
            input.terms,
            input.schedule.descriptor().classes.len(),
            limits.measured.settlement.matching.execution,
        )?);
    let retained: &'r [PhloResourceAmount<'r>] = retained_slot.insert(derive_retained_amounts(
        permissions,
        input.terms,
        &born,
        limits.retained.funding.cells,
    )?);
    check_retained_origin(
        permissions,
        input.terms,
        retained,
        &born,
        limits.retained.funding.cells,
    )?;
    let selection: &'r CanonicalPrepaidSelection = selection_slot.insert(
        input
            .inventory
            .select_measured_demand(input.demand, limits.measured.demand)?,
    );
    let binding: &'r NativePrepaidDemandBinding<'r> =
        binding_slot.insert(input.inventory.bind_measured_demand(
            NativePrepaidDemandInput {
                // Changed by DR-134 (DR-65 amendment of 2026-10-12).
                // expected_root: input.funding_root,
                expected_root: input.inventory_root,
                controls: input.controls,
                demand: input.demand,
                draws: selection.draws(),
                demand_positions: selection.demand_positions(),
            },
            limits.measured.demand,
        )?);
    let observed = check_counted_phlo_execution(
        input.controls,
        binding.witness(),
        limits.measured.settlement.matching.execution,
    )
    .map_err(invalid)?
    .with_retained_acquisitions(retained)
    .map_err(invalid)?;
    Ok(MeasuredSettlement {
        born,
        selection,
        observed,
    })
}

/// Added by dedup (task 11415, D5): the inputs of the funding half of the
/// replayed settlement pipeline. The outcome and the root are parameters.
/// Each caller passes its own message for a family without a case and its own
/// message for a family without a cursor policy.
#[derive(Clone, Copy)]
struct FundingCaptureInput<'r> {
    snapshot: &'r DirectWalletPolicySnapshot<'r, OfferedFundedDeploy>,
    obligations: &'r CheckedPhloObligations<'r>,
    observed: CheckedPhloExecution<'r>,
    outcome: PhloOutcome<'r>,
    root: [u8; 32],
    controls: CheckedPhloControls<'r>,
    adopted: &'r AdoptedResourcePolicy,
    authority_events: &'r [AuthorityEvent<[u8; 32]>],
    authority_byte_events: &'r [AuthorityByteEvent],
    no_case: &'static str,
    no_policy: &'static str,
}

/// Added by dedup (task 11415, D5): the values of the funding half that the
/// captured settlement borrows. The caller owns them, so the captured
/// settlement outlives the call.
#[derive(Default)]
struct FundingCaptureSlots<'r> {
    family_selection: Option<NativeRootedFamilySelection<'r>>,
    cases: Option<[PhloFundingCase<'r>; 1]>,
    family: Option<CheckedPhloFundingFamily<'r>>,
    intent: Option<PhloFundingIntentBinding<'r>>,
    view: Option<PhloFundingIntentView<'r>>,
}

/// Added by dedup (task 11415, D5): the result of the funding half: the
/// rooted family selection and the captured settlement, bound to the root.
struct FundingCapture<'r> {
    family_selection: &'r NativeRootedFamilySelection<'r>,
    checked: CheckedDirectWalletSettlement<'r, OfferedFundedDeploy>,
}

/// Added by dedup (task 11415, D5): the funding half of the replayed
/// settlement pipeline, from the rooted family selection to the captured
/// settlement. The steps and their errors are those that
/// `settle_replayed_candidate` and `success_path_debits` ran, in the same
/// order. `select_phlo_funding_family` returns one assignment for
/// each outcome (`family_policy.rs`), so the one requirement always has its
/// case.
fn capture_replayed_funding<'r>(
    slots: &'r mut FundingCaptureSlots<'r>,
    input: FundingCaptureInput<'r>,
    limits: NativeOfferedFamilyLimits,
) -> Result<FundingCapture<'r>, CasperError> {
    let FundingCaptureSlots {
        family_selection: selection_slot,
        cases: case_slot,
        family: family_slot,
        intent: intent_slot,
        view: view_slot,
    } = slots;
    let family_selection: &'r NativeRootedFamilySelection<'r> =
        selection_slot.insert(select_rooted_native_family(
            input.snapshot,
            input.obligations,
            input.authority_events,
            input.authority_byte_events,
            limits,
        )?);
    let assignment = family_selection
        .selected
        .assignments()
        .first()
        .ok_or_else(|| invalid(input.no_case))?;
    let cases: &'r [PhloFundingCase<'r>] = case_slot.insert([PhloFundingCase {
        obligations: input.obligations,
        eligible: &family_selection.eligible,
        assignment,
    }]);
    let family: &'r CheckedPhloFundingFamily<'r> = family_slot.insert(
        check_phlo_funding_family(
            &family_selection.sources,
            cases,
            family_selection.total_exposure_limit,
            limits.policy.funding,
        )
        .map_err(invalid)?,
    );
    let intent: &'r PhloFundingIntentBinding<'r> = intent_slot.insert(
        PhloFundingIntentBinding::new(
            input.snapshot.wallets().authorization().record(),
            limits.signed.intent,
        )
        .map_err(invalid)?,
    );
    let view: &'r PhloFundingIntentView<'r> = view_slot.insert(intent.view().map_err(invalid)?);
    let terms = PhloFundingTerms {
        required_owner_ceilings: input.controls.terms().required_owner_ceilings,
        asset: input.controls.schedule().environment.asset,
        schedule_commitment: input.controls.schedule().commitment,
    };
    let policy = input
        .snapshot
        .bind_native_family_in_context(
            view,
            family,
            terms,
            limits.signed,
            limits.policy,
            input.adopted,
        )
        .map_err(invalid)?
        .ok_or_else(|| invalid(input.no_policy))?;
    let checked = policy
        .capture_settlement(
            input.observed,
            input.outcome,
            limits.measured.settlement.matching,
            limits.measured.settlement.capture,
        )
        .map(|checked| checked.bind_execution_root(input.root))
        .map_err(invalid)?;
    Ok(FundingCapture {
        family_selection,
        checked,
    })
}

/// Added by G1-7 (DR-65 item 5, arbitration M2): the inputs of the success
/// path that `settle_replayed_candidate` already holds.
struct SuccessPathInput<'r, 's, 'p> {
    adopted: &'r AdoptedResourcePolicy,
    schedule: &'r PhloScheduleBinding<'r>,
    terms: &'r [u8],
    controls: CheckedPhloControls<'r>,
    inventory: &'r NativePrepaidInventory<'s, 'p>,
    demand: &'r NativePhloAcquisitionDemand<'r>,
    post_run_root: [u8; 32],
}

impl NativeOfferedAttempt<'_, '_> {
    /// Added by G1-7 (DR-65 item 5, arbitration M2): the debits of the success
    /// path of a run that the block commits as a guard failure. The function
    /// runs the measured settlement of `settle_replayed_candidate` at the
    /// root after the run, with the births of the run and no failure, up to
    /// the capture that settlement would charge. Play computes the same
    /// capture before its own guard.
    // Changed by dedup (task 11415, D5): the function calls the two halves of
    // the settlement pipeline, `measure_replayed_settlement` and
    // `capture_replayed_funding`. It passes the root after the run and the
    // outcome without failures.
    async fn success_path_debits(
        &self,
        manager: &RuntimeManager,
        snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
        input: SuccessPathInput<'_, '_, '_>,
        production: NativeOfferedProductionLimits,
    ) -> Result<BTreeMap<[u8; 32], u128>, CasperError> {
        let limits = production.family;
        // Moved by dedup (task 11415) to `measure_replayed_settlement` and
        // `capture_replayed_funding` in
        // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/replay.rs`.
        // let post_stacks = manager.read_measured_prepaid_stacks_with_births(
        //     input.post_run_root,
        //     input.demand,
        //     &self.evaluation.authority_stack_births,
        //     production.stack_capture,
        //     budget,
        // )?;
        // let born = derive_live_born_stacks(
        //     &self.evaluation.authority_stack_births,
        //     &post_stacks,
        //     limits.retained.funding.births,
        //     budget,
        // )?;
        // let permissions = restore_retained_permissions(
        //     &snapshot.wallets().authorization().record().sources,
        //     input.terms,
        //     input.schedule.descriptor().classes.len(),
        //     limits.measured.settlement.matching.execution,
        //     budget,
        // )?;
        // let retained = derive_retained_amounts(
        //     &permissions,
        //     input.terms,
        //     &born,
        //     limits.retained.funding.cells,
        //     budget,
        // )?;
        // check_retained_origin(
        //     &permissions,
        //     input.terms,
        //     &retained,
        //     &born,
        //     limits.retained.funding.cells,
        //     budget,
        // )?;
        // let selection =
        //     input
        //         .inventory
        //         .select_measured_demand(input.demand, limits.measured.demand, budget)?;
        // let binding = input.inventory.bind_measured_demand(
        //     NativePrepaidDemandInput {
        //         expected_root: self.preflight.funding_root,
        //         controls: input.controls,
        //         demand: input.demand,
        //         draws: selection.draws(),
        //         demand_positions: selection.demand_positions(),
        //     },
        //     limits.measured.demand,
        //     budget,
        // )?;
        // let observed = check_counted_phlo_execution(
        //     input.controls,
        //     binding.witness(),
        //     limits.measured.settlement.matching.execution,
        // )
        // .map_err(invalid)?
        // .with_retained_acquisitions(&retained, budget)
        // .map_err(invalid)?;
        // let obligations = project_phlo_obligations(
        //     observed,
        //     PhloOutcome::Accepted(&[]),
        //     limits.policy.funding.obligations,
        // )
        // .map_err(invalid)?;
        // let family_selection = select_rooted_native_family(
        //     snapshot,
        //     &obligations,
        //     &self.evaluation.authority_events,
        //     &self.evaluation.authority_byte_events,
        //     limits,
        //     budget,
        // )?;
        // let assignment = family_selection
        //     .selected
        //     .assignments()
        //     .first()
        //     .ok_or_else(|| invalid("the success path of a claimed guard failure has no case"))?;
        // let cases = [PhloFundingCase {
        //     obligations: &obligations,
        //     eligible: &family_selection.eligible,
        //     assignment,
        // }];
        // let family = check_phlo_funding_family(
        //     &family_selection.sources,
        //     &cases,
        //     family_selection.total_exposure_limit,
        //     limits.policy.funding,
        // )
        // .map_err(invalid)?;
        // let intent = PhloFundingIntentBinding::new(
        //     snapshot.wallets().authorization().record(),
        //     limits.signed.intent,
        // )
        // .map_err(invalid)?;
        // let view = intent.view().map_err(invalid)?;
        // let terms = PhloFundingTerms {
        //     required_owner_ceilings: input.controls.terms().required_owner_ceilings,
        //     asset: input.controls.schedule().environment.asset,
        //     schedule_commitment: input.controls.schedule().commitment,
        // };
        // let policy = snapshot
        //     .bind_native_family_in_context(
        //         &view,
        //         &family,
        //         terms,
        //         limits.signed,
        //         limits.policy,
        //         input.adopted,
        //         budget,
        //     )
        //     .map_err(invalid)?
        //     .ok_or_else(|| invalid("the success path of a claimed guard failure has no policy"))?;
        // let checked = policy
        //     .capture_settlement(
        //         observed,
        //         PhloOutcome::Accepted(&[]),
        //         limits.measured.settlement.matching,
        //         limits.measured.settlement.capture,
        //         budget,
        //     )
        //     .map(|checked| checked.bind_execution_root(input.post_run_root))
        //     .map_err(invalid)?;
        // capture_debits(checked.capture().amounts(), budget)
        let mut measured_slots = MeasuredSettlementSlots::default();
        let measured = measure_replayed_settlement(
            manager,
            &mut measured_slots,
            MeasuredSettlementInput {
                root: input.post_run_root,
                // Changed by DR-134 (DR-65 amendment of 2026-10-12): the
                // inventory belongs to the execution root.
                // funding_root: self.preflight.funding_root,
                inventory_root: self.preflight.execution_root,
                births: &self.evaluation.authority_stack_births,
                snapshot,
                schedule: input.schedule,
                terms: input.terms,
                controls: input.controls,
                inventory: input.inventory,
                demand: input.demand,
            },
            production,
        )?;
        let outcome = PhloOutcome::Accepted(&[]);
        let obligations = project_phlo_obligations(
            measured.observed,
            outcome,
            limits.policy.funding.obligations,
        )
        .map_err(invalid)?;
        let mut capture_slots = FundingCaptureSlots::default();
        let capture = capture_replayed_funding(
            &mut capture_slots,
            FundingCaptureInput {
                snapshot,
                obligations: &obligations,
                observed: measured.observed,
                outcome,
                root: input.post_run_root,
                controls: input.controls,
                adopted: input.adopted,
                authority_events: &self.evaluation.authority_events,
                authority_byte_events: &self.evaluation.authority_byte_events,
                no_case: "the success path of a claimed guard failure has no case",
                no_policy: "the success path of a claimed guard failure has no policy",
            },
            limits,
        )?;
        capture_debits(capture.checked.capture().amounts())
    }
}

impl<'policy, 'a> ReplayedNativeFundedUser<'policy, 'a> {
    pub fn user_event_count(&self) -> usize { self.user_event_count }

    pub fn grant_issue_event_count(&self) -> usize { self.grant_issue_event_count }

    pub fn expected_settlement_runtime_root(&self) -> [u8; 32] {
        self.expected_settlement_runtime_root
    }

    pub fn runtime(&mut self) -> &mut RuntimeOps { self.attempt.runtime() }

    pub fn observed_births(&self) -> &[AuthorityStackBirth] {
        &self.attempt.evaluation.authority_stack_births
    }

    pub fn capture_measured_settlement(
        &self,
        input: NativeAttemptSettlementInput<'_, '_, '_>,
        limits: NativeAttemptSettlementLimits,
    ) -> Result<NativeCapturedSettlement<'a>, CasperError> {
        self.attempt.capture_measured_settlement(input, limits)
    }

    pub async fn read_measured_prepaid_stacks(
        &self,
        manager: &RuntimeManager,
        schedule: &PhloScheduleBinding<'_>,
        terms: &[u8],
        limits: NativeAttemptSettlementLimits,
        capture: PrepaidStackCaptureLimits,
    ) -> Result<Vec<PurseStack>, CasperError> {
        if self.attempt.runtime.runtime.get_root().await.bytes()
            != self.expected_settlement_runtime_root
        {
            return Err(invalid(
                "offered replay runtime root changed before rooted reads",
            ));
        }
        self.attempt.adopted.check_acquisition_terms(terms)?;
        let measured = self
            .attempt
            .adopted
            .native_rules()
            .measure(
                &self.attempt.evaluation.byte_observations,
                limits.observations,
            )
            .map_err(invalid)?;
        let regions = measured.region_demands(limits.regions).map_err(invalid)?;
        let located = regions.locate_purses(limits.purses).map_err(invalid)?;
        let demand = located
            .prepare_acquisition_demand(schedule, terms, limits.acquisition)
            .map_err(invalid)?;
        manager.read_measured_prepaid_stacks(
            self.attempt.policy.snapshot().wallets().pre_state_root(),
            &demand,
            capture,
        )
    }

    pub async fn prepare_wallet_settlement<'processed>(
        mut self,
        manager: &RuntimeManager,
        processed: &'processed OfferedProcessedDeploy,
        captured: NativeCapturedSettlement<'a>,
        block_data: &rholang::rust::interpreter::system_processes::BlockData,
        receipt_limits: NativeWalletReceiptLimits,
        grants: Option<NativeGrantSettlementInput<'_>>,
    ) -> Result<PendingReplayedNativeSettlement<'processed, 'policy, 'a>, CasperError> {
        let independently_reached_root: [u8; 32] = self
            .attempt
            .runtime
            .runtime
            .get_root()
            .await
            .bytes()
            .try_into()
            .map_err(|_| invalid("independent replay root must be 32 bytes"))?;
        if independently_reached_root != self.expected_settlement_runtime_root {
            return Err(invalid("independent replay settlement root changed"));
        }
        let permit = CandidateReplayPermit::authorize(
            processed,
            independently_reached_root,
            self.user_event_count,
            self.grant_issue_event_count,
        )?;
        if !std::ptr::eq(
            captured.checked().snapshot(),
            self.attempt.policy.snapshot(),
        ) {
            return Err(invalid(
                "replayed funding case belongs to another wallet snapshot",
            ));
        }
        let recomputed_funding = encode_measured_funding_case(
            captured.checked().capture().scoped().capture(),
            captured.cursor_transition(),
            offered_funded_v6_limits().funding_case,
        )?;
        permit.verify_funding_case(&recomputed_funding)?;
        let settlement_context =
            OfferedSettlementContext::from_block_inputs(processed.envelope(), block_data)?;
        let issuances = (!self.grant_issuances.is_empty()).then_some(NativeGrantIssuanceInput {
            verified: &self.grant_issuances,
            limits: offered_grant_transition_limits(),
        });
        let wallet = self
            .attempt
            .runtime
            .prepare_replayed_wallet_settlement(
                manager,
                captured.checked(),
                &permit,
                settlement_context.reservation_id,
                &settlement_context.fee_address,
                settlement_context.initial_rand,
                receipt_limits,
                grants,
                issuances,
                // Added by G3 (DR-135, S7): the family path has no owner
                // funding.
                None,
            )
            .await?;
        Ok(PendingReplayedNativeSettlement {
            attempt: self.attempt,
            permit,
            captured,
            wallet,
        })
    }
}

impl<'processed, 'policy, 'a> PendingReplayedNativeSettlement<'processed, 'policy, 'a> {
    pub async fn apply_prepaid_and_finish<A: Send + Sync>(
        mut self,
        prepared: PreparedNativeRetainedReceipts<'_, '_, '_, '_, A>,
        records: &NativeRetainedReceiptRecords<'_, '_, '_, '_, A>,
        consumption: NativePrepaidConsumption<'_, '_>,
        limits: NativeRetainedSettlementLimits,
    ) -> Result<(NativeFundedAttempt<'policy, 'a>, NativeFundedSettlement), CasperError> {
        let result: Result<(), CasperError> = async {
            if !std::ptr::eq(prepared.births(), records.births())
                || consumption.draws != self.captured.selection().draws()
            {
                return Err(invalid(
                    "replayed prepaid inputs differ from measured selection",
                ));
            }
            let funding_root: [u8; 32] = self
                .attempt
                .policy
                .snapshot()
                .wallets()
                .pre_state_root()
                .try_into()
                .map_err(|_| invalid("offered replay funding root must be 32 bytes"))?;
            let wallet_root: [u8; 32] = self
                .wallet
                .post_state_root
                .as_ref()
                .try_into()
                .map_err(|_| invalid("offered replay wallet root must be 32 bytes"))?;
            let receipts = self
                .attempt
                .runtime
                .apply_prepaid_retained_after_wallet(
                    prepared,
                    consumption,
                    funding_root,
                    wallet_root,
                    limits,
                )
                .await?;
            let count = receipts.prepaid_changes.len();
            let mut changes = Vec::new();
            changes
                .try_reserve_exact(count)
                .map_err(|_| invalid("replayed prepaid evidence allocation failed"))?;
            changes.extend(
                receipts
                    .prepaid_changes
                    .iter()
                    .map(|change| change.as_change()),
            );
            let prepaid_delta = encode_measured_prepaid_delta(
                self.captured.selection(),
                records.records(),
                &changes,
                offered_funded_v6_limits().prepaid_delta,
            )?;
            self.permit.verify_prepaid_delta(&prepaid_delta)?;
            self.attempt.runtime.runtime.check_replay_data().await?;
            let final_root = self
                .attempt
                .runtime
                .runtime
                .create_checkpoint()
                .await
                .root
                .bytes();
            self.permit.verify_complete(
                &final_root,
                &self.wallet.wallet_receipt,
                self.wallet.resource_rev,
                self.wallet.fee_rev,
            )?;
            self.wallet
                .settlement_log
                .try_reserve_exact(receipts.log.len())
                .map_err(|_| invalid("offered replay settlement log allocation failed"))?;
            self.wallet.settlement_log.extend(receipts.log);
            Ok(())
        }
        .await;
        if let Err(error) = result {
            let baseline =
                Blake2b256Hash::from_bytes(self.permit.settlement_runtime_root().to_vec());
            self.attempt.runtime.runtime.reset(&baseline).await?;
            if self.attempt.runtime.runtime.get_root().await.bytes()
                != self.permit.settlement_runtime_root()
            {
                return Err(invalid("offered replay failed to restore settlement root"));
            }
            return Err(error);
        }
        Ok((self.attempt, self.wallet))
    }
}

fn invalid(error: impl std::fmt::Display) -> CasperError {
    CasperError::RuntimeError(error.to_string())
}

fn event_bytes(event: &CasperEvent) -> Result<usize, CasperError> {
    let consume_bytes = |consume: &models::rust::casper::protocol::casper_message::ConsumeEvent| {
        consume
            .channels_hashes
            .iter()
            .try_fold(consume.hash.len(), |sum, hash| sum.checked_add(hash.len()))
            .and_then(|bytes| {
                consume
                    .channels_hashes
                    .len()
                    .checked_mul(64)
                    .and_then(|overhead| bytes.checked_add(overhead))
            })
    };
    let produce_bytes = |produce: &models::rust::casper::protocol::casper_message::ProduceEvent| {
        produce
            .output_value
            .iter()
            .try_fold(
                produce
                    .channels_hash
                    .len()
                    .checked_add(produce.hash.len())?,
                |sum, value| sum.checked_add(value.len()),
            )
            .and_then(|bytes| {
                produce
                    .output_value
                    .len()
                    .checked_mul(64)
                    .and_then(|overhead| bytes.checked_add(overhead))
            })
    };
    let bytes = match event {
        CasperEvent::Produce(produce) => produce_bytes(produce),
        CasperEvent::Consume(consume) => consume_bytes(consume),
        CasperEvent::Comm(comm) => consume_bytes(&comm.consume)
            .and_then(|initial| {
                comm.produces.iter().try_fold(initial, |sum, produce| {
                    sum.checked_add(produce_bytes(produce)?)
                })
            })
            .and_then(|bytes| {
                comm.produces
                    .len()
                    .checked_mul(128)
                    .and_then(|overhead| {
                        comm.peeks
                            .len()
                            .checked_mul(16)
                            .and_then(|peeks| overhead.checked_add(peeks))
                    })
                    .and_then(|overhead| bytes.checked_add(overhead))
            }),
    };
    bytes.ok_or_else(|| invalid("offered replay event byte size overflows"))
}

fn event_items(event: &CasperEvent) -> Option<usize> {
    match event {
        CasperEvent::Produce(produce) => produce.output_value.len().checked_add(1),
        CasperEvent::Consume(consume) => consume.channels_hashes.len().checked_add(1),
        CasperEvent::Comm(comm) => {
            let mut items = comm.consume.channels_hashes.len().checked_add(1)?;
            items = items.checked_add(comm.peeks.len())?;
            for produce in &comm.produces {
                items = items.checked_add(produce.output_value.len().checked_add(1)?)?;
            }
            Some(items)
        }
    }
}

fn event_top_items(event: &CasperEvent) -> Option<usize> {
    match event {
        CasperEvent::Produce(produce) => produce.output_value.len().checked_add(1),
        CasperEvent::Consume(consume) => consume.channels_hashes.len().checked_add(1),
        CasperEvent::Comm(comm) => comm
            .consume
            .channels_hashes
            .len()
            .checked_add(comm.peeks.len())
            .and_then(|count| count.checked_add(comm.produces.len()))
            .and_then(|count| count.checked_add(1)),
    }
}

fn checked_replay_events(
    events: &[CasperEvent],
    limits: NativeOperationTraceLimits,
    bytes_cap: usize,
) -> Result<Arc<[Event]>, CasperError> {
    if events.len() > limits.events {
        return Err(invalid("offered replay event count exceeds protocol limit"));
    }
    let item_cap = limits
        .source_entries
        .checked_add(limits.telemetry_items)
        .and_then(|count| count.checked_add(limits.events))
        .ok_or_else(|| invalid("offered replay event item limit overflows"))?;
    let top_items = events.iter().try_fold(0usize, |count, event| {
        count
            .checked_add(
                event_top_items(event)
                    .ok_or_else(|| invalid("offered replay event item count overflows"))?,
            )
            .ok_or_else(|| invalid("offered replay event item count overflows"))
    })?;
    if top_items > item_cap {
        return Err(invalid("offered replay event items exceed protocol limit"));
    }
    let items = events.iter().try_fold(0usize, |count, event| {
        count
            .checked_add(
                event_items(event)
                    .ok_or_else(|| invalid("offered replay event item count overflows"))?,
            )
            .ok_or_else(|| invalid("offered replay event item count overflows"))
    })?;
    if items > item_cap {
        return Err(invalid("offered replay event items exceed protocol limit"));
    }
    let bytes = events.iter().try_fold(0usize, |sum, event| {
        sum.checked_add(event_bytes(event)?)
            .ok_or_else(|| invalid("offered replay event byte size overflows"))
    })?;
    if bytes > bytes_cap {
        return Err(invalid("offered replay events exceed protocol byte limit"));
    }
    let mut replayed = Vec::new();
    replayed
        .try_reserve_exact(events.len())
        .map_err(|_| invalid("offered replay event allocation failed"))?;
    replayed.extend(events.iter().map(event_converter::to_rspace_event));
    Ok(replayed.into())
}

// Moved by DR-115 to offered_evidence.rs, unchanged: the merge index uses the
// same user-event boundary as replay.
// fn native_user_event_count(
//     operations: &[NativeOperationRecord],
//     maximum: usize,
//     host: &rholang::rust::interpreter::host_work::HostWorkBudget,
// ) -> Result<usize, CasperError> {
//     host.reserve(
//         HostWorkDimension::VerificationOperations,
//         HostWorkUnits::new(
//             u64::try_from(operations.len())
//                 .map_err(|_| invalid("native user event count overflows"))?,
//         ),
//     )
//     .map_err(invalid)?;
//     operations.iter().try_fold(0usize, |count, operation| {
//         let width = match operation.completion {
//             RSpaceOperationCompletion::Rejected => 0,
//             RSpaceOperationCompletion::Stored => 1,
//             RSpaceOperationCompletion::Matched => 2,
//         };
//         count
//             .checked_add(width)
//             .filter(|count| *count <= maximum)
//             .ok_or_else(|| invalid("native user event count exceeds replay limit"))
//     })
// }

fn settlement_root<T>(before: T, after: T, failed: bool) -> T {
    if failed {
        before
    } else {
        after
    }
}

impl NativeFundedAttempt<'_, '_> {
    pub fn replay_input(&self) -> Result<NativeFundedReplayInput<'_>, CasperError> {
        Ok(NativeFundedReplayInput {
            recording: self
                .evaluation
                .native_budget_recording
                .as_ref()
                .ok_or_else(|| invalid("native funded attempt has no budget recording"))?,
            operations: self
                .evaluation
                .native_operation_recording
                .as_ref()
                .ok_or_else(|| invalid("native funded attempt has no operation recording"))?,
            events: &self.replay_log,
        })
    }
}

impl RuntimeManager {
    pub async fn replay_native_funded_user_from_processed<'policy, 'a>(
        &self,
        policy: &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy>,
        processed: &OfferedProcessedDeploy,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        context: NativeFundedExecutionContext,
        wire_limits: PhloWireLimits,
        recording_limits: NativeRecordingWireLimits,
        replay_limits: NativeFundedReplayLimits,
    ) -> Result<ReplayedNativeFundedUser<'policy, 'a>, CasperError> {
        let evidence_bytes = processed.native_cost_evidence_bytes().len();
        if evidence_bytes > wire_limits.total_bytes {
            return Err(invalid("committed native evidence exceeds protocol limit"));
        }
        processed.validate(wire_limits).map_err(invalid)?;
        let evidence = processed.evidence(wire_limits).map_err(invalid)?;
        if evidence.original_funding_root != policy.snapshot().wallets().pre_state_root() {
            return Err(invalid(
                "committed funding evidence belongs to another original root",
            ));
        }
        if evidence.genesis_policy_commitment != adopted.genesis_policy_commitment()?
            || evidence.schedule_commitment != schedule.schedule().commitment
            || evidence.phlo_price != schedule.schedule().actual_price
        {
            return Err(invalid(
                "committed offer uses another adopted policy or selected price schedule",
            ));
        }
        let (recording, operations) =
            decode_committed_runtime_recording(&evidence, recording_limits)?;
        let user_events = native_user_event_count(&operations, replay_limits.trace.events)?;
        let event_prefix = processed
            .deploy_log()
            .get(..user_events)
            .ok_or_else(|| invalid("committed deploy log omits native user events"))?;
        let events =
            checked_replay_events(event_prefix, replay_limits.trace, wire_limits.total_bytes)?;
        let mut attempt = self
            .replay_native_funded(
                policy,
                processed.envelope(),
                adopted,
                schedule,
                context,
                NativeFundedReplayInput {
                    recording: &recording,
                    operations: &operations,
                    events: &events,
                },
                replay_limits,
            )
            .await?;
        let failures = attempt
            .evaluation
            .economic_failures
            .union(classify_errors(&attempt.evaluation.errors).map_err(invalid)?);
        if !failures.permits_retained_charge() {
            return Err(invalid(
                "independent native replay has a non-user economic failure",
            ));
        }
        let user_failure = failures.contains(PhloFailure::User);
        if attempt.evaluation.native_phlo_usage != Some(evidence.phlo_used)
            || user_failure != (evidence.failure_class == NativeCostFailureClass::UserFailure)
        {
            return Err(invalid(
                "independent native replay differs from committed usage or failure",
            ));
        }
        let grant_issuances = if !user_failure {
            attempt
                .runtime
                .collect_grant_issue_requests(
                    true,
                    events.as_ref(),
                    schedule.schedule().environment.network,
                    schedule.schedule().environment.shard,
                    offered_grant_issue_call_limits(),
                    offered_grant_transition_limits(),
                )
                .await?
        } else {
            Vec::new()
        };
        let grant_log = attempt.runtime.runtime.take_event_log().await;
        let grant_end = user_events
            .checked_add(grant_log.len())
            .ok_or_else(|| invalid("grant issue event boundary overflows"))?;
        let expected_grant_events = processed
            .deploy_log()
            .get(user_events..grant_end)
            .ok_or_else(|| invalid("committed deploy log omits grant issue events"))?;
        let expected_grant_log = checked_replay_events(
            expected_grant_events,
            replay_limits.trace,
            wire_limits.total_bytes,
        )?;
        if expected_grant_log.as_ref() != grant_log.as_slice() {
            return Err(invalid(
                "independent grant issue log differs from committed events",
            ));
        }
        let settlement_root = attempt
            .runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .bytes();
        if settlement_root != evidence.settlement_runtime_root {
            return Err(invalid(
                "independent native and grant replay differs from committed settlement root",
            ));
        }
        Ok(ReplayedNativeFundedUser {
            attempt,
            user_event_count: user_events,
            grant_issue_event_count: grant_log.len(),
            grant_issuances,
            expected_settlement_runtime_root: evidence.settlement_runtime_root,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn replay_native_offered_witness<'preflight, 'a>(
        &self,
        prepared: &'preflight PreparedOfferedCandidate<'a>,
        processed: &OfferedProcessedDeploy,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
        context: NativeFundedExecutionContext,
        candidate_limits: OfferedCandidateLimits,
        recording_limits: NativeRecordingWireLimits,
        replay_limits: NativeFundedReplayLimits,
        // Added by DR-134 (DR-70): the logger of a block report; play and
        // validation pass none.
        user_run_logger: Option<rholang::rust::interpreter::rho_runtime::RhoSpaceLogger>,
    ) -> Result<NativeOfferedAttempt<'preflight, 'a>, CasperError> {
        let protocol = offered_funded_v6_limits();
        let wire_limits = protocol.evidence;
        let evidence_bytes = processed.native_cost_evidence_bytes().len();
        let log_events = processed.deploy_log().len();
        if evidence_bytes > wire_limits.total_bytes || log_events > protocol.deploy_log_events {
            return Err(invalid("committed native evidence exceeds protocol limit"));
        }
        processed.validate(wire_limits).map_err(invalid)?;
        let evidence = processed.evidence(wire_limits).map_err(invalid)?;
        let DeployEnvelopeRef::OfferedFunded(signed) = processed.envelope().view() else {
            return Err(invalid("native replay requires an offered funded envelope"));
        };
        let authenticated = prepare_offered_candidate(
            signed,
            adopted,
            prepared.funding_root,
            prepared.execution_root,
            candidate_limits,
        )?;
        let selected = prepared.selected_terms(adopted)?;
        let selected_schedule =
            PhloScheduleBinding::new(selected.schedule(), PhloGenesisPolicy::LIMITS)
                .map_err(invalid)?;
        if selected_schedule.schedule() != schedule.schedule()
            || authenticated.intent != prepared.intent
            || authenticated.envelope_identity != prepared.envelope_identity
            || authenticated.phlo_limit != prepared.phlo_limit
            || authenticated.phlo_price != prepared.phlo_price
            || authenticated.fee_rev != prepared.fee_rev
            || authenticated.rev_ceiling != prepared.rev_ceiling
            || evidence.original_funding_root != prepared.funding_root
            || evidence.genesis_policy_commitment != adopted.genesis_policy_commitment()?
            || evidence.schedule_commitment != schedule.schedule().commitment
            || evidence.phlo_price != schedule.schedule().actual_price
            || evidence.phlo_limit != prepared.phlo_limit
            || evidence.fee_rev != prepared.fee_rev
            || evidence.envelope_commitment != prepared.envelope_identity
        {
            return Err(invalid(
                "committed witness differs from signed offered replay inputs",
            ));
        }
        if !matches!(
            prepared.intent.conversion,
            PhloConversionCompositionV2::NoConversion
        ) {
            return Err(invalid(
                "native offered witness conversion is not supported",
            ));
        }
        super::check_execution_identity(
            &prepared.envelope_identity,
            processed.envelope().identity().as_bytes(),
        )?;
        // Changed by G1-6 (DR-124, map gap fill G-8): the offered runtime is
        // spawned before the controls, so the check reads its URN map. Nothing
        // reads the runtime before the grant collection below, and the replay
        // environment keeps its own budget and merge tracker.
        let mut runtime = RuntimeOps::new(
            self.spawn_offered_runtime(offered_grant_issue_call_limits())
                .await?,
        );
        let gate = self.check_offered_before_execution(OfferedGateInput {
            envelope: processed.envelope(),
            prepared,
            selected_terms: selected.bytes(),
            schedule,
            minimum_price: adopted.genesis().minimum_price(),
            snapshot,
            evaluator: &runtime.runtime.reducer,
            limits: offered_funded_v6_production_limits().family,
        });
        #[cfg(any(test, feature = "test-utils"))]
        self.record_offered_check(
            OfferedCheckRole::Replay,
            processed.envelope(),
            &prepared.funding_root,
            gate.as_ref().ok(),
        );
        let gate = gate?;
        let signed_controls = PhloControlsBinding::new(
            &prepared.intent.base.controls,
            candidate_limits.funding.base.controls(),
        )
        .map_err(invalid)?;
        let view = signed_controls.view().map_err(invalid)?;
        let checked = check_offered_phlo_controls(
            schedule.schedule().environment,
            adopted.genesis().minimum_price(),
            i64::MAX as u64,
            PhloOffer {
                limit: signed.data.phlo_limit(),
                price: signed.data.phlo_price(),
            },
            view.terms(),
            schedule.schedule(),
            // Changed by G1-6 (DR-124): the bound is the resource bound of the
            // check.
            // prepared.phlo_limit,
            gate.resource_bound(),
        )
        .map_err(invalid)?;
        // Changed by G1-6 (DR-124): the replay runtime configuration binds the
        // bound source of the check (DR-124 amends DR-113 item 6).
        // // DR-113: the bound of an offer is its signed limit.
        // let contract = adopted.bind_execution_contract(checked.controls(), schedule, rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource::SignedLimit)?;
        let contract =
            adopted.bind_execution_contract(checked.controls(), schedule, gate.bound_source())?;
        // Changed by G3 (DR-135, S7): the run has the owner funding of the
        // recorded owner table, with the client lanes and the room of the
        // check at R0 (R11, G-1).
        // let config = NativeRuntimeConfig::new(contract, context.trace);
        // let (recording, operations) =
        //     decode_committed_runtime_recording(&evidence, recording_limits)?;
        let (recording, operations) =
            decode_committed_runtime_recording(&evidence, recording_limits)?;
        let owner_table =
            recorded_owner_table(&recording.owner_table, &purse_of, gate.owner_room())
                .map_err(invalid)?;
        let owner = Arc::new(OwnerFundingBinding::new(
            gate.client_lanes().clone(),
            purse_of,
            owner_table,
        ));
        let config = NativeRuntimeConfig::new(contract, context.trace)
            .with_owner_funding(Arc::clone(&owner));
        let user_events = native_user_event_count(&operations, replay_limits.trace.events)?;
        let user_prefix = processed
            .deploy_log()
            .get(..user_events)
            .ok_or_else(|| invalid("committed deploy log omits native user events"))?;
        let events =
            checked_replay_events(user_prefix, replay_limits.trace, wire_limits.total_bytes)?;
        let identity = prepared.envelope_identity;
        // DR-113: replay classifies a denial with the source that play uses.
        // Changed by G1-6 (DR-124): the replay evidence binds the bound source
        // of the check, which play also binds.
        // let trace = adopted
        //     .bind_execution_contract(checked.controls(), schedule, rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource::SignedLimit)?
        // Changed by G3 (DR-135, S7): the journal check folds the owner
        // terms, and the attempt keeps the billed owner sums (G-4).
        // let trace = adopted
        //     .bind_execution_contract(checked.controls(), schedule, gate.bound_source())?
        //     .check_operation_journal(identity, &recording, operations, replay_limits.journal)
        //     .map_err(invalid)?
        //     .bind_trace(events.clone(), replay_limits.trace)
        //     .map_err(invalid)?;
        let journal = adopted
            .bind_execution_contract(checked.controls(), schedule, gate.bound_source())?
            .check_operation_journal_with_owners(
                identity,
                &recording,
                operations,
                replay_limits.journal,
                Some(owner.as_ref()),
            )
            .map_err(invalid)?;
        let owner_totals = journal.owner_totals();
        let trace = journal
            .bind_trace(events.clone(), replay_limits.trace)
            .map_err(invalid)?;
        let cost = RuntimeBudget::new(Cost::unsafe_max());
        cost.set_deploy_id_funded(identity, funding_sig(signed))
            .map_err(|error| invalid(error.to_string()))?;
        cost.reset_for_native_execution(config)?;
        let before = Blake2b256Hash::from_bytes(prepared.execution_root.to_vec());
        let history = Arc::new(self.history_repo.reset(&before).map_err(invalid)?);
        super::check_execution_root(&before.bytes(), &history.root().bytes())?;
        let parsed = InterpreterImpl::parse_source(
            &processed.envelope().body().term,
            normalizer_env_from_envelope(processed.envelope()),
        )?;
        let mut offered_definitions =
            vec![grant_issue_definition(offered_grant_issue_call_limits())];
        // Changed by DR-134 (DR-70): the replay takes the optional logger.
        let mut replay =
            rholang::rust::interpreter::rho_runtime::create_native_replay_env_with_logger(
                trace,
                history,
                Arc::new(Box::new(Matcher)),
                Arc::new(RwLock::new(HashMap::new())),
                self.mergeable_tags.clone(),
                &mut offered_definitions,
                cost,
                ExternalServices::noop(),
                user_run_logger,
            )
            .await?;
        #[cfg(any(test, feature = "test-utils"))]
        self.record_offered_budget_usage(
            OfferedUsageKind::ReplayUrnMap {
                matches_check: replay.urn_map() == runtime.runtime.reducer.urn_map.as_ref(),
            },
            processed.identity_bytes(),
            &prepared.funding_root,
        );
        *replay.block_data.write().await = context.block_data.clone();
        replay
            .set_invalid_blocks(context.invalid_blocks.clone())
            .await;
        *replay.deploy_data.write().await = DeployData::from_envelope(processed.envelope());
        #[cfg(any(test, feature = "test-utils"))]
        self.record_offered_budget_usage(
            OfferedUsageKind::ReplayRuntimeEntry,
            processed.identity_bytes(),
            &prepared.funding_root,
        );
        let evaluation = replay
            .evaluate(parsed, Tools::user_envelope_rng(processed.envelope()))
            .await?;
        let failures = evaluation
            .economic_failures
            .union(classify_errors(&evaluation.errors).map_err(invalid)?);
        if !failures.permits_retained_charge() {
            return Err(invalid(
                "native offered witness has a non-user economic failure",
            ));
        }
        let user_failure = failures.contains(PhloFailure::User);
        // Changed by G1-7 (DR-65 item 5, arbitration M2): one flag, "user
        // failure or guard failure". A committed UserFailure for a run without
        // a user failure claims a failed held-capacity guard.
        // `settle_replayed_candidate` recomputes the guard and accepts the
        // claim only if the guard fails. A run with a user failure that the
        // block commits as a success still rejects.
        // if evaluation.native_phlo_usage != Some(evidence.phlo_used)
        //     || user_failure != (evidence.failure_class == NativeCostFailureClass::UserFailure)
        // {
        //     return Err(invalid(
        //         "native offered witness differs from committed usage or failure",
        //     ));
        // }
        let committed_failure = evidence.failure_class == NativeCostFailureClass::UserFailure;
        let rolled_back = match (
            evaluation.native_phlo_usage == Some(evidence.phlo_used),
            user_failure,
            committed_failure,
        ) {
            (false, _, _) | (true, true, false) => {
                return Err(invalid(
                    "native offered witness differs from committed usage or failure",
                ));
            }
            (true, user, committed) => user || committed,
        };
        let guard_claimed = rolled_back && !user_failure;
        let exported = replay.export().await?;
        let post_run_root: [u8; 32] = exported
            .root()
            .bytes()
            .try_into()
            .map_err(|_| invalid("native offered witness export root must be 32 bytes"))?;
        // Changed by G1-7: the settlement root follows the combined flag.
        // let root = settlement_root(before, exported.root().clone(), user_failure);
        let root = settlement_root(before, exported.root().clone(), rolled_back);
        // Changed by G1-6 (DR-124): the replay reuses the runtime that it
        // spawned before the check.
        // let mut runtime = RuntimeOps::new(
        //     self.spawn_offered_runtime(offered_grant_issue_call_limits(), host.clone())
        //         .await?,
        // );
        runtime
            .runtime
            .set_block_data(context.block_data.clone())
            .await;
        runtime
            .runtime
            .set_invalid_blocks(context.invalid_blocks)
            .await;
        runtime
            .runtime
            .set_deploy_data(DeployData::from_envelope(processed.envelope()))
            .await;
        runtime.runtime.reset(&root).await?;
        super::check_execution_root(&root.bytes(), &runtime.runtime.get_root().await.bytes())?;
        // Changed by G1-7 (arbitration M2): a rolled-back run, by a user
        // failure or by a guard failure, issues no grants.
        // let grant_issuances = if user_failure {
        let grant_issuances = if rolled_back {
            Vec::new()
        } else {
            runtime
                .collect_grant_issue_requests(
                    true,
                    events.as_ref(),
                    schedule.schedule().environment.network,
                    schedule.schedule().environment.shard,
                    offered_grant_issue_call_limits(),
                    offered_grant_transition_limits(),
                )
                .await?
        };
        let grant_log = runtime.runtime.take_event_log().await;
        let grant_end = user_events
            .checked_add(grant_log.len())
            .ok_or_else(|| invalid("offered witness grant boundary overflows"))?;
        let mut grant_casper = Vec::new();
        grant_casper
            .try_reserve_exact(grant_log.len())
            .map_err(|_| invalid("offered witness grant log allocation failed"))?;
        grant_casper.extend(
            grant_log
                .iter()
                .cloned()
                .map(event_converter::to_casper_event),
        );
        if processed.deploy_log().get(user_events..grant_end) != Some(grant_casper.as_slice()) {
            return Err(invalid(
                "native offered witness grant log differs from commitment",
            ));
        }
        let reached = runtime.runtime.create_checkpoint().await.root.bytes();
        if reached != evidence.settlement_runtime_root {
            return Err(invalid(
                "native offered witness settlement root differs from commitment",
            ));
        }
        Ok(NativeOfferedAttempt {
            preflight: prepared,
            adopted: adopted.clone(),
            block_data: context.block_data,
            runtime,
            evaluation,
            replay_log: events,
            grant_issuances,
            grant_issue_log: Arc::from(grant_log),
            gate,
            // Added by G1-7 (arbitration M2): the guard of a claimed guard
            // failure reads the root after the run.
            post_run_root: Some(post_run_root),
            guard_claimed,
            // Added by G3 (DR-135, S7): the binding of the recorded table, and
            // the billed owner sums of its fold.
            owner: Some(owner),
            owner_totals,
        })
    }

    pub async fn replay_native_funded<'policy, 'a>(
        &self,
        policy: &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy>,
        envelope: &DeployEnvelope,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        context: NativeFundedExecutionContext,
        input: NativeFundedReplayInput<'_>,
        limits: NativeFundedReplayLimits,
    ) -> Result<NativeFundedAttempt<'policy, 'a>, CasperError> {
        let config = policy.prepare_execution(envelope, adopted, schedule, context.trace)?;
        let controls = policy
            .policy()
            .policy()
            .signed_intent()
            .intent()
            .bound()
            .consent()
            .family()
            .cases()[0]
            .obligations
            .execution()
            .controls();
        let identity = envelope.identity().as_bytes().try_into().map_err(invalid)?;
        // DR-113: this family path has no caller. It keeps the measured family
        // bound, unbilled as before.
        let trace = adopted
            .bind_execution_contract(controls, schedule, rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource::Certificate)?
            .check_operation_journal(
                identity,
                input.recording,
                input.operations.clone(),
                limits.journal,
            )
            .map_err(invalid)?
            .bind_trace(input.events.clone(), limits.trace)
            .map_err(invalid)?;
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(invalid("native replay requires an offered funded envelope"));
        };
        let cost = RuntimeBudget::new(Cost::unsafe_max());
        cost.set_deploy_id_funded(identity, funding_sig(signed))
            .map_err(|error| invalid(error.to_string()))?;
        cost.reset_for_native_execution(config)?;
        let before =
            Blake2b256Hash::from_bytes(policy.snapshot().wallets().pre_state_root().to_vec());
        let history = Arc::new(self.history_repo.reset(&before).map_err(invalid)?);
        super::check_execution_root(&before.bytes(), &history.root().bytes())?;
        let parsed = InterpreterImpl::parse_source(
            &envelope.body().term,
            normalizer_env_from_envelope(envelope),
        )?;
        let mut offered_definitions =
            vec![grant_issue_definition(offered_grant_issue_call_limits())];
        let mut replay = create_native_replay_env(
            trace,
            history,
            Arc::new(Box::new(Matcher)),
            Arc::new(RwLock::new(HashMap::new())),
            self.mergeable_tags.clone(),
            &mut offered_definitions,
            cost,
            ExternalServices::noop(),
        )
        .await?;
        *replay.block_data.write().await = context.block_data.clone();
        replay
            .set_invalid_blocks(context.invalid_blocks.clone())
            .await;
        *replay.deploy_data.write().await = DeployData::from_envelope(envelope);
        let evaluation = replay
            .evaluate(parsed, Tools::user_envelope_rng(envelope))
            .await?;
        let exported = replay.export().await?;
        let root = settlement_root(
            before,
            exported.root().clone(),
            !evaluation.errors.is_empty(),
        );
        let mut runtime = RuntimeOps::new(
            self.spawn_offered_runtime(offered_grant_issue_call_limits())
                .await?,
        );
        runtime.runtime.set_block_data(context.block_data).await;
        runtime
            .runtime
            .set_invalid_blocks(context.invalid_blocks)
            .await;
        runtime
            .runtime
            .set_deploy_data(DeployData::from_envelope(envelope))
            .await;
        runtime.runtime.reset(&root).await?;
        super::check_execution_root(&root.bytes(), &runtime.runtime.get_root().await.bytes())?;
        Ok(NativeFundedAttempt {
            policy,
            adopted: adopted.clone(),
            runtime,
            evaluation,
            replay_log: input.events.clone(),
            execution_root: root
                .bytes()
                .try_into()
                .map_err(|_| invalid("native replay state root must be 32 bytes"))?,
            draft_permit: None,
        })
    }
}

mod block_replay;
pub use block_replay::RecordedRun;

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::ProduceEvent;
    use proptest::prelude::*;

    use super::*;

    /// Changed by DR-131: the check that the rejection reserved no payload
    /// copy went with host work. The test keeps its check of the item limit.
    #[test]
    fn replay_event_item_limit_rejects_the_events() {
        let events = [CasperEvent::Produce(ProduceEvent {
            channels_hash: Default::default(),
            hash: Default::default(),
            persistent: false,
            times_repeated: 0,
            is_deterministic: true,
            output_value: vec![Default::default(); 4],
            failed: false,
        })];
        let limits = NativeOperationTraceLimits {
            events: 1,
            source_entries: 1,
            source_bytes: 100,
            telemetry_items: 1,
            telemetry_bytes: 100,
        };
        assert!(checked_replay_events(&events, limits, 100).is_err());
    }

    proptest! {
        #[test]
        fn funded_replay_state_selection_preserves_rollback(
            before in any::<[u8; 32]>(), after in any::<[u8; 32]>(),
            other in any::<[u8; 32]>(), failed in any::<bool>(),
        ) {
            let selected = settlement_root(before, after, failed);
            prop_assert_eq!(selected, if failed { before } else { after });
            if failed {
                prop_assert_eq!(selected, settlement_root(before, other, failed));
            }
            prop_assert!(super::super::check_execution_root(&selected, &selected).is_ok());
            prop_assert_eq!(
                super::super::check_execution_root(&selected, &other).is_ok(),
                selected == other,
            );
            let mut changed = selected;
            changed[0] ^= 1;
            prop_assert!(super::super::check_execution_root(&selected, &changed).is_err());
            prop_assert!(super::super::check_execution_root(&selected, &selected[..31]).is_err());
            if !failed && before != after {
                prop_assert!(super::super::check_execution_root(&selected, &before).is_err());
            }
        }
    }
}
