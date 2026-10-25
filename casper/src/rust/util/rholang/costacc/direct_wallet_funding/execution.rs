use std::collections::HashMap;
use std::sync::Arc;

// Added by G3 (DR-135, S7): owner funding of an offered run (R3, R10, R11).
use models::rhoapi::{CostRegion, CostSignature};
use models::rust::block_hash::BlockHash;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::phlo_grant_creation::VerifiedGrantCreationV1;
use models::rust::phlo_intent::PhloConversionCompositionV2;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use models::rust::validator::Validator;
// Added by G3 (DR-135, S7).
use rholang::rust::interpreter::accounting::authority::AuthorityError;
use rholang::rust::interpreter::accounting::native_phlo_rules::NativeBudgetTraceLimits;
// Added by G3 (DR-135, S7).
use rholang::rust::interpreter::accounting::owner_funding::{
    OwnerFundingBinding, OwnerTable, OwnerTableEntry, OwnerTotals,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_offered_phlo_controls, PhloControlsBinding, PhloOffer, PhloScheduleBinding,
};
use rholang::rust::interpreter::accounting::NativeRuntimeConfig;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::trace::event::Event;

// Added by G3 (DR-135, S7): the later holds that cap the owner budgets (R3).
use self::held_capacity::LaterHolds;
// Changed by DR-134 (DR-65): the offered attempt takes its check from the
// decision pass of its batch, so this module reads no wallet snapshot.
// use super::{CheckedDirectWalletPolicy, DirectWalletPolicySnapshot};
use super::CheckedDirectWalletPolicy;
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::acceptance::{
    prepare_offered_candidate, CandidateDraftSettlementPermit, OfferedCandidateLimits,
    PreparedOfferedCandidate,
};
// Added by G3 (DR-135, S7): the owner budgets of the discovery (R3).
use crate::rust::util::rholang::costacc::direct_wallet_funding::snapshot::owner_budget;
use crate::rust::util::rholang::costacc::genesis_resource_policy::{
    AdoptedResourcePolicy, CompatibleAcquisitionTerms,
};
#[cfg(any(test, feature = "test-utils"))]
use crate::rust::util::rholang::costacc::offered_acceptance::{
    OfferedCheckRecord, OfferedCheckRole, OfferedUsageKind,
};
use crate::rust::util::rholang::costacc::offered_grants::{
    offered_grant_issue_call_limits, offered_grant_transition_limits,
};
use crate::rust::util::rholang::costacc::prepaid_receipts::PrepaidStackCaptureLimits;
// Changed by DR-134 (DR-65): the check of an offered attempt moved to the
// decision pass of its batch (`offered_batch.rs`).
// use crate::rust::util::rholang::costacc::production_limits::offered_funded_v6_production_limits;
use crate::rust::util::rholang::costacc::supply::PurseStack;
// Added by G3 (DR-135, S7): the payers of the owner purses (R2).
use crate::rust::util::rholang::costacc::vault_payer::{purse_of, vault_payer, VaultPayer};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub struct NativeFundedExecutionContext {
    pub block_data: BlockData,
    pub invalid_blocks: HashMap<BlockHash, Validator>,
    pub trace: NativeBudgetTraceLimits,
}

pub struct NativeFundedAttempt<'policy, 'a> {
    policy: &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy>,
    adopted: AdoptedResourcePolicy,
    runtime: RuntimeOps,
    evaluation: EvaluateResult,
    replay_log: Arc<[Event]>,
    execution_root: [u8; 32],
    draft_permit: Option<CandidateDraftSettlementPermit>,
}

pub struct NativeOfferedAttempt<'preflight, 'a> {
    preflight: &'preflight PreparedOfferedCandidate<'a>,
    adopted: AdoptedResourcePolicy,
    block_data: BlockData,
    runtime: RuntimeOps,
    evaluation: EvaluateResult,
    replay_log: Arc<[Event]>,
    grant_issuances: Vec<VerifiedGrantCreationV1>,
    grant_issue_log: Arc<[Event]>,
    /// Added by G1-6 (DR-124): the check before execution. Its resource bound
    /// and bound source bind every later control of the attempt.
    gate: OfferedGate,
    /// Added by G1-7 (DR-65 item 5, arbitration M2): the root after the run,
    /// where the held-capacity guard reads the balances. The witness replay
    /// sets it from its export. Play reads its own settlement checkpoint.
    post_run_root: Option<[u8; 32]>,
    /// Added by G1-7 (arbitration M2): the witness replay found a committed
    /// UserFailure for a run without a user failure, that is, a claimed guard
    /// failure. `settle_replayed_candidate` accepts it only if the recomputed
    /// guard fails.
    guard_claimed: bool,
    /// Added by G3 (DR-135, S7): the owner funding of the run (R11): the
    /// binding of the last discovery pass in play, and the binding of the
    /// recorded owner table in replay.
    owner: Option<Arc<OwnerFundingBinding>>,
    /// Added by G3 (DR-135, S7): the billed owner sums of the witness replay
    /// (G-4), which its journal check folded. Play folds its own recording in
    /// the producer.
    owner_totals: Option<OwnerTotals>,
}

impl<'preflight, 'a> NativeOfferedAttempt<'preflight, 'a> {
    /// Added by G1-6 (DR-124).
    pub fn gate(&self) -> &OfferedGate { &self.gate }

    pub fn evaluation(&self) -> &EvaluateResult { &self.evaluation }

    pub fn replay_log(&self) -> &[Event] { &self.replay_log }

    pub fn grant_issuances(&self) -> &[VerifiedGrantCreationV1] { &self.grant_issuances }

    pub fn grant_issue_log(&self) -> &[Event] { &self.grant_issue_log }

    /// Added by G3 (DR-135, S7): whether the demand of the client keeps
    /// `region`. With owner funding an owner region leaves it, because its
    /// owner pays it (R10, G-2). Without owner funding every region stays.
    pub(super) fn keeps_client_region(&self, region: &CostRegion) -> Result<bool, AuthorityError> {
        match self.owner.as_deref() {
            Some(binding) => binding.is_owner_region(region).map(|owner| !owner),
            None => Ok(true),
        }
    }

    /// Added by G3 (DR-135, S8): the owner purses that the owner table of the
    /// run lists, in ascending custody order (R3). A dropped batch member
    /// with a hold on one of them makes a truncated batch run again
    /// (truncation condition (c)).
    pub(crate) fn owner_purses(&self) -> Vec<[u8; 32]> {
        match self.owner.as_deref() {
            Some(binding) => {
                let entries = binding.table().entries();
                let mut purses = Vec::with_capacity(entries.len());
                purses.extend(entries.iter().map(|(custody, _)| *custody));
                purses
            }
            None => Vec::new(),
        }
    }

    pub async fn abort(mut self) -> Result<(), CasperError> {
        let root = Blake2b256Hash::from_bytes(self.preflight.execution_root.to_vec());
        self.runtime.runtime.reset(&root).await?;
        check_execution_root(
            &self.preflight.execution_root,
            &self.runtime.runtime.get_root().await.bytes(),
        )
    }

    /// Added by G1-7 (DR-65 item 5, arbitration M2): the rollback of a run
    /// whose held-capacity guard failed. The runtime returns to the execution
    /// root, as `abort` does. The births of the run, its grant issuances,
    /// their log and its mergeable map go with it. The settlement then binds
    /// to the execution root and charges the run as a user failure.
    pub(super) async fn roll_back_for_guard(&mut self) -> Result<(), CasperError> {
        let root = Blake2b256Hash::from_bytes(self.preflight.execution_root.to_vec());
        self.runtime.runtime.reset(&root).await?;
        check_execution_root(
            &self.preflight.execution_root,
            &self.runtime.runtime.get_root().await.bytes(),
        )?;
        self.evaluation.authority_stack_births.clear();
        self.evaluation.mergeable.clear();
        self.grant_issuances.clear();
        self.grant_issue_log = Arc::from([]);
        Ok(())
    }

    pub async fn read_measured_prepaid_stacks(
        &self,
        manager: &RuntimeManager,
        family: NativeOfferedFamilyLimits,
        capture: PrepaidStackCaptureLimits,
    ) -> Result<Vec<PurseStack>, CasperError> {
        check_execution_root(
            &self.preflight.execution_root,
            &self.runtime.runtime.get_root().await.bytes(),
        )?;
        let selected = self.preflight.selected_terms(&self.adopted)?;
        let schedule = PhloScheduleBinding::new(selected.schedule(), PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let measured = self
            .adopted
            .native_rules()
            .measure(
                &self.evaluation.byte_observations,
                family.measured.observations,
            )
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        // Changed by G3 (DR-135, S7): the demand of the client leaves out the
        // owner regions (R10, G-2).
        // let regions = measured
        //     .region_demands(family.measured.regions)
        //     .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let regions = measured
            .region_demands_filtered(
                family.measured.regions,
                Some(&|region: &CostRegion| self.keeps_client_region(region)),
            )
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let located = regions
            .locate_purses(family.measured.purses)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let demand = located
            .prepare_acquisition_demand(&schedule, selected.bytes(), family.measured.acquisition)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        // Changed by DR-134 (DR-65 amendment of 2026-10-12): the prepaid stacks
        // of a member are read at its execution root R_{i-1}.
        // manager.read_measured_prepaid_stacks(self.preflight.funding_root, &demand, capture)
        manager.read_measured_prepaid_stacks(self.preflight.execution_root, &demand, capture)
    }

    pub fn bind_policy<'policy>(
        self,
        policy: &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy>,
    ) -> Result<NativeFundedAttempt<'policy, 'a>, CasperError> {
        if !self.grant_issuances.is_empty() {
            return Err(CasperError::RuntimeError(
                "offered grant issuance requires composed production settlement".into(),
            ));
        }
        if policy.snapshot().wallets().pre_state_root() != self.preflight.funding_root {
            return Err(CasperError::RuntimeError(
                "native funding family belongs to another original root".into(),
            ));
        }
        let identity = policy
            .policy()
            .policy()
            .signed_intent()
            .envelope()
            .envelope_commitment()
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        check_execution_identity(&self.preflight.envelope_identity, identity.as_ref())?;
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
        if controls.schedule().commitment != self.preflight.intent.base.schedule_commitment
            || controls.schedule().actual_price != self.preflight.phlo_price
            || controls.terms().limit != self.preflight.phlo_limit
        {
            return Err(CasperError::RuntimeError(
                "measured funding family differs from signed execution controls".into(),
            ));
        }
        let draft_permit = self.preflight.authorize_private_draft(
            self.preflight.envelope_identity,
            self.evaluation
                .byte_observations
                .has_complete_measurements(),
            self.evaluation.native_phlo_usage,
        )?;
        Ok(NativeFundedAttempt {
            policy,
            adopted: self.adopted,
            runtime: self.runtime,
            evaluation: self.evaluation,
            replay_log: self.replay_log,
            execution_root: self.preflight.execution_root,
            draft_permit: Some(draft_permit),
        })
    }
}

impl<'policy, 'a> NativeFundedAttempt<'policy, 'a> {
    pub fn policy(&self) -> &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy> {
        self.policy
    }

    pub fn evaluation(&self) -> &EvaluateResult { &self.evaluation }

    pub fn replay_log(&self) -> &[Event] { &self.replay_log }

    pub fn runtime(&mut self) -> &mut RuntimeOps { &mut self.runtime }
}

mod producer;
pub use producer::{
    NativeOfferedCandidateResult, NativeOfferedFamilyLimits, NativeOfferedPreparedResult,
    NativeOfferedProductionLimits, NativeScopedOfferedResult,
};

mod family_selection;
pub(crate) use family_selection::select_rooted_native_family;

mod gate;
pub use gate::{
    gate_offered_candidate, InsufficientCause, OfferedEstimateKind, OfferedGate, OfferedGateInput,
    OfferedGateRejection,
};

// Added by G1-7 (DR-65 item 5): the inputs of the held-capacity guard.
mod held_capacity;

// Added by DR-134 (DR-65 items 1 to 5, O-8, O-9, O-10): the funding groups of an
// offered batch.
mod funding_groups;

mod block_gate;
pub use block_gate::{OfferedAdmission, OfferedHolds};

mod block_settlement;
pub use block_settlement::{OfferedRun, SettledOffer};

/// Added by DR-114: the bytes of external-service replies that one offered
/// play may record. The replay telemetry and the clone-byte check of the
/// deploy log count a record three times, and the replay evidence counts it
/// twice, so the room is the smallest share of those existing caps. It adds
/// no policy parameter, and a room that does not fit is zero (fail closed).
fn native_record_room() -> u64 {
    let protocol = models::rust::cost_protocol_limits::offered_funded_v6_limits();
    let replay =
        crate::rust::util::rholang::costacc::production_limits::offered_funded_v6_replay_limits();
    let room = (replay.trace.telemetry_bytes / 3)
        .min(protocol.deploy_log_bytes / 3)
        .min(protocol.evidence.total_bytes / 2);
    u64::try_from(room).unwrap_or(0)
}

fn check_execution_identity(authorized: &[u8], requested: &[u8]) -> Result<(), CasperError> {
    if authorized != requested {
        return Err(CasperError::RuntimeError(
            "native execution envelope differs from the verified funding policy".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn check_execution_root(authorized: &[u8], actual: &[u8]) -> Result<(), CasperError> {
    if authorized != actual {
        return Err(CasperError::RuntimeError(
            "native execution runtime differs from its authorized state root".to_owned(),
        ));
    }
    Ok(())
}

impl CheckedDirectWalletPolicy<'_, OfferedFundedDeploy> {
    fn prepare_execution(
        &self,
        envelope: &DeployEnvelope,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        limits: NativeBudgetTraceLimits,
    ) -> Result<NativeRuntimeConfig, CasperError> {
        if !matches!(envelope.view(), DeployEnvelopeRef::OfferedFunded(_)) {
            return Err(CasperError::RuntimeError(
                "native wallet execution requires an offered funded envelope".to_owned(),
            ));
        }
        let signed = self.policy().policy().signed_intent();
        let identity = signed
            .envelope()
            .envelope_commitment()
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        check_execution_identity(identity.as_ref(), envelope.identity().as_bytes())?;
        let controls = signed.intent().bound().consent().family().cases()[0]
            .obligations
            .execution()
            .controls();
        // DR-113: this family path has no caller. It keeps the measured family
        // bound, unbilled as before.
        let contract = adopted.bind_execution_contract(controls, schedule, rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource::Certificate)?;
        Ok(NativeRuntimeConfig::new(contract, limits))
    }
}

impl RuntimeManager {
    /// Added by G1-6 (DR-124): the check before execution of one offered
    /// deploy (design §6). Play and every replay call it with the same inputs
    /// before any runtime entry. A rejection has no effect and no charge
    /// (P1:2210-2212). Play turns it into the typed candidate rejection of
    /// DR-72, and replay turns it into a replay failure of the block.
    fn check_offered_before_execution(
        &self,
        input: OfferedGateInput<'_>,
    ) -> Result<OfferedGate, CasperError> {
        #[cfg(any(test, feature = "test-utils"))]
        match self.offered_check_hook(input.envelope.identity().as_bytes()) {
            // Changed by G3 (DR-135, S7, G-1): the hook keeps the client lanes
            // and the owner room at R0, as the check does.
            // Some(OfferedUsageKind::SkipCheckHook) => {
            //     return Ok(OfferedGate::unchecked_signed_limit(
            //         input.prepared.phlo_limit,
            //     ));
            // }
            Some(OfferedUsageKind::SkipCheckHook) => {
                let (client_lanes, owner_room) = gate::offered_owner_terms(&input)
                    .map_err(|rejection| CasperError::RuntimeError(rejection.to_string()))?;
                return Ok(
                    OfferedGate::unchecked_signed_limit(input.prepared.phlo_limit)
                        .with_owner_terms(client_lanes, owner_room),
                );
            }
            Some(OfferedUsageKind::ZeroEstimateHook) => {
                return gate::gate_offered_candidate_without_counts(input)
                    .map_err(|rejection| CasperError::RuntimeError(rejection.to_string()));
            }
            _ => {}
        }
        gate_offered_candidate(input)
            .map_err(|rejection| CasperError::RuntimeError(rejection.to_string()))
    }

    /// Added by G1-6 (DR-124): a correctness failure under the bound source
    /// `Certificate` (design §7 and §9). The offer is rejected with no charge.
    /// The failure means an analyzer defect, so the node logs it.
    fn report_certificate_correctness_failure(
        &self,
        envelope_identity: &[u8],
        funding_root: &[u8],
        gate: &OfferedGate,
        detail: &str,
    ) {
        tracing::error!(
            target: "f1r3fly.cost_accounting.correctness",
            deploy = %hex::encode(envelope_identity),
            funding_root = %hex::encode(funding_root),
            estimate = gate.estimate_phlo(),
            resource_bound = gate.resource_bound(),
            detail,
            "correctness failure: an offer under the bound source Certificate failed after execution"
        );
        #[cfg(any(test, feature = "test-utils"))]
        self.record_offered_budget_usage(
            OfferedUsageKind::CertificateCorrectnessFailure,
            envelope_identity,
            funding_root,
        );
    }

    /// Added by G1-6 (DR-124), test only: the check hook that a test set on
    /// this node for the deploy.
    #[cfg(any(test, feature = "test-utils"))]
    fn offered_check_hook(&self, deploy_id: &[u8]) -> Option<OfferedUsageKind> {
        self.offered_budget_usages()
            .into_iter()
            .find_map(|record| match record.kind {
                OfferedUsageKind::SkipCheckHook | OfferedUsageKind::ZeroEstimateHook
                    if record.deploy_id == deploy_id =>
                {
                    Some(record.kind)
                }
                _ => None,
            })
    }

    /// Added by G1-6 (DR-124), test only: records the decision of one check.
    #[cfg(any(test, feature = "test-utils"))]
    fn record_offered_check(
        &self,
        role: OfferedCheckRole,
        envelope: &DeployEnvelope,
        funding_root: &[u8],
        gate: Option<&OfferedGate>,
    ) {
        let record = match gate {
            Some(gate) => OfferedCheckRecord::Accepted {
                complete: matches!(gate.kind(), OfferedEstimateKind::Complete),
                estimate: gate.estimate_phlo(),
                resource_bound: gate.resource_bound(),
                bound_source: gate.bound_source(),
                total_hold: gate.total_hold(),
            },
            None => OfferedCheckRecord::Rejected,
        };
        self.record_offered_budget_usage(
            OfferedUsageKind::Check(role, record),
            envelope.identity().as_bytes(),
            funding_root,
        );
    }

    /// Added by G1-6 (DR-124), test only: a hook of this node. The node skips
    /// the check for the deploy and binds the signed limit, as every live site
    /// did before G1-6. Other nodes still run the check.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn skip_offered_check_for_tests(&self, deploy_id: &[u8]) {
        self.record_offered_budget_usage(OfferedUsageKind::SkipCheckHook, deploy_id, &[]);
    }

    /// Added by G1-6 (DR-124), test only: a hook of this node. The check of
    /// the deploy sees its estimate without counts, so a complete offer runs
    /// under the certified bound 0 and passes it.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn remove_offered_estimate_counts_for_tests(&self, deploy_id: &[u8]) {
        self.record_offered_budget_usage(OfferedUsageKind::ZeroEstimateHook, deploy_id, &[]);
    }

    // Changed by DR-134 (DR-65 items 2 and 3, GateBeforeExecute): the attempt
    // takes the check of the member from the decision pass of its batch, which
    // checks every member at R0 before the first runtime entry. The attempt no
    // longer reads a wallet snapshot.
    // pub async fn evaluate_native_offered<'preflight, 'a>(
    //     &self,
    //     prepared: &'preflight PreparedOfferedCandidate<'a>,
    //     envelope: &DeployEnvelope,
    //     selected: &CompatibleAcquisitionTerms<'_, '_>,
    //     limits: OfferedCandidateLimits,
    //     snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
    //     context: NativeFundedExecutionContext,
    // ) -> Result<NativeOfferedAttempt<'preflight, 'a>, CasperError> {
    // Changed by G3 (DR-135, S7): the function takes the later holds, a
    // crate-private type, so it is visible in the crate only. Its one caller
    // is `play_offered_member`.
    // pub async fn evaluate_native_offered<'preflight, 'a>(
    pub(crate) async fn evaluate_native_offered<'preflight, 'a>(
        &self,
        prepared: &'preflight PreparedOfferedCandidate<'a>,
        envelope: &DeployEnvelope,
        selected: &CompatibleAcquisitionTerms<'_, '_>,
        limits: OfferedCandidateLimits,
        gate: OfferedGate,
        context: NativeFundedExecutionContext,
        // Added by G3 (DR-135, S7): the later holds of the member, which cap
        // the owner budgets (R3, §7 "Budget"). The gate gives the client
        // lanes, the owner room and the member's own holds (G-1).
        later: &LaterHolds,
    ) -> Result<NativeOfferedAttempt<'preflight, 'a>, CasperError> {
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(CasperError::RuntimeError(
                "native offered execution requires a signed offered envelope".into(),
            ));
        };
        let authenticated = prepare_offered_candidate(
            signed,
            selected.adopted(),
            prepared.funding_root,
            prepared.execution_root,
            limits,
        )?;
        let signed_selection = authenticated.selected_terms(selected.adopted())?;
        let prepared_selection = prepared.selected_terms(selected.adopted())?;
        if signed_selection.bytes() != selected.bytes()
            || prepared_selection.bytes() != selected.bytes()
        {
            return Err(CasperError::RuntimeError(
                "native offered selected schedule differs from signed terms".into(),
            ));
        }
        if authenticated.intent != prepared.intent
            || authenticated.envelope_identity != prepared.envelope_identity
            || authenticated.phlo_limit != prepared.phlo_limit
            || authenticated.phlo_price != prepared.phlo_price
            || authenticated.rev_ceiling != prepared.rev_ceiling
        {
            return Err(CasperError::RuntimeError(
                "native offered preflight differs from its signed envelope".into(),
            ));
        }
        check_execution_identity(&prepared.envelope_identity, envelope.identity().as_bytes())?;
        if !matches!(
            &prepared.intent.conversion,
            PhloConversionCompositionV2::NoConversion
        ) {
            return Err(CasperError::RuntimeError(
                "conversion quotes require authenticated settlement".into(),
            ));
        }
        let schedule = PhloScheduleBinding::new(selected.schedule(), PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        // Changed by G1-6 (DR-124): the play runtime is spawned before the
        // controls, so the check reads the URN map of the runtime that executes
        // the offer (design §6.4). The check runs before any runtime entry.
        let mut runtime = RuntimeOps::new(
            self.spawn_offered_play_runtime(offered_grant_issue_call_limits())
                .await?,
        );
        // Changed by DR-134 (DR-65 items 2 and 3, GateBeforeExecute): the
        // decision pass of the batch ran this check at R0, and the attempt
        // receives its result as `gate`.
        // let gate = self.check_offered_before_execution(OfferedGateInput {
        //     envelope,
        //     prepared,
        //     selected_terms: selected.bytes(),
        //     schedule: &schedule,
        //     minimum_price: selected.adopted().genesis().minimum_price(),
        //     snapshot,
        //     evaluator: &runtime.runtime.reducer,
        //     limits: offered_funded_v6_production_limits().family,
        // });
        // #[cfg(any(test, feature = "test-utils"))]
        // self.record_offered_check(
        //     OfferedCheckRole::Play,
        //     envelope,
        //     &prepared.funding_root,
        //     gate.as_ref().ok(),
        // );
        // let gate = gate?;
        let signed_controls = PhloControlsBinding::new(
            &prepared.intent.base.controls,
            limits.funding.base.controls(),
        )
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let view = signed_controls
            .view()
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let checked = check_offered_phlo_controls(
            schedule.schedule().environment,
            selected.adopted().genesis().minimum_price(),
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
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        // Changed by G1-6 (DR-124): the contract binds the bound source of the
        // check (DR-124 amends DR-113 item 6).
        // // DR-113: the bound of an offer is its signed limit.
        // let contract = selected
        //     .adopted()
        //     .bind_execution_contract(checked.controls(), &schedule, rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource::SignedLimit)?;
        // let host_work = context.host_work.clone();
        // Changed by G3 (DR-135, S7): the run has owner funding, and it runs
        // again while it notes owner purses that its table does not list
        // (R3, discovery). Each pass lists the noted purses with their budgets
        // and runs again from the same checkpoint. A pass adds at least one
        // purse, so at most `owner_room + 1` passes run. The replaced single
        // run follows.
        // let contract = selected.adopted().bind_execution_contract(
        //     checked.controls(),
        //     &schedule,
        //     gate.bound_source(),
        // )?;
        // // Changed by DR-114: play records external-service replies within a
        // // room, and the play runtime calls the node's services.
        // // let config = NativeRuntimeConfig::new(contract, context.trace, context.host_work);
        // // let mut runtime = RuntimeOps::new(
        // //     self.spawn_offered_runtime(offered_grant_issue_call_limits(), host_work.clone())
        // //         .await?,
        // // );
        // let config = NativeRuntimeConfig::new(contract, context.trace)
        //     .with_record_room(native_record_room());
        // // Changed by G1-6 (DR-124): the spawn moved above the controls.
        // // let mut runtime = RuntimeOps::new(
        // //     self.spawn_offered_play_runtime(offered_grant_issue_call_limits(), host_work.clone())
        // //         .await?,
        // // );
        // let block_data = context.block_data.clone();
        // runtime.runtime.set_block_data(context.block_data).await;
        // runtime
        //     .runtime
        //     .set_invalid_blocks(context.invalid_blocks)
        //     .await;
        // runtime
        //     .runtime
        //     .reset(&Blake2b256Hash::from_bytes(
        //         prepared.execution_root.to_vec(),
        //     ))
        //     .await?;
        // check_execution_root(
        //     &prepared.execution_root,
        //     &runtime.runtime.get_root().await.bytes(),
        // )?;
        // let checkpoint = runtime.runtime.create_soft_checkpoint().await;
        // #[cfg(any(test, feature = "test-utils"))]
        // self.record_offered_budget_usage(
        //     OfferedUsageKind::PlayRuntimeEntry,
        //     envelope.identity().as_bytes(),
        //     &prepared.funding_root,
        // );
        // let evaluation = match runtime.evaluate_native_envelope(envelope, config).await {
        //     Ok(evaluation) => evaluation,
        //     Err(error) => {
        //         runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
        //         return Err(error);
        //     }
        // };
        let block_data = context.block_data.clone();
        runtime.runtime.set_block_data(context.block_data).await;
        runtime
            .runtime
            .set_invalid_blocks(context.invalid_blocks)
            .await;
        runtime
            .runtime
            .reset(&Blake2b256Hash::from_bytes(
                prepared.execution_root.to_vec(),
            ))
            .await?;
        check_execution_root(
            &prepared.execution_root,
            &runtime.runtime.get_root().await.bytes(),
        )?;
        let mut table = OwnerTable::empty(gate.owner_room());
        let (owner, checkpoint, evaluation) = loop {
            let binding = Arc::new(OwnerFundingBinding::new(
                gate.client_lanes().clone(),
                purse_of,
                table.clone(),
            ));
            let contract = selected.adopted().bind_execution_contract(
                checked.controls(),
                &schedule,
                gate.bound_source(),
            )?;
            let config = NativeRuntimeConfig::new(contract, context.trace)
                .with_record_room(native_record_room())
                .with_owner_funding(Arc::clone(&binding));
            let checkpoint = runtime.runtime.create_soft_checkpoint().await;
            #[cfg(any(test, feature = "test-utils"))]
            self.record_offered_budget_usage(
                OfferedUsageKind::PlayRuntimeEntry,
                envelope.identity().as_bytes(),
                &prepared.funding_root,
            );
            let evaluation = match runtime.evaluate_native_envelope(envelope, config).await {
                Ok(evaluation) => evaluation,
                Err(error) => {
                    runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
                    return Err(error);
                }
            };
            let noted = binding.take_notes();
            if noted.is_empty() || !table.has_room() {
                break (binding, checkpoint, evaluation);
            }
            let listed = match self
                .list_owner_purses(&table, noted, prepared.execution_root, &gate, later)
                .await
            {
                Ok(listed) => listed,
                Err(error) => {
                    runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
                    return Err(error);
                }
            };
            runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
            runtime.runtime.take_event_log().await;
            table = listed;
        };
        if !evaluation.byte_observations.has_complete_measurements() {
            runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
            return Err(CasperError::RuntimeError(format!(
                "native offered execution has incomplete byte measurements: errors={:?}",
                evaluation.errors,
            )));
        }
        if evaluation.native_phlo_usage.is_none() {
            runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
            return Err(CasperError::RuntimeError(
                "native offered execution has no phlo usage".into(),
            ));
        }
        if runtime.runtime.get_root().await.bytes() != prepared.execution_root {
            runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
            return Err(CasperError::RuntimeError(
                "native offered execution changed its base root".into(),
            ));
        }
        let execution_log = runtime.runtime.take_event_log().await;
        let (grant_issuances, grant_issue_log) = if evaluation.errors.is_empty() {
            let issuances = match runtime
                .collect_grant_issue_requests(
                    true,
                    &execution_log,
                    selected.schedule().network,
                    selected.schedule().shard,
                    offered_grant_issue_call_limits(),
                    offered_grant_transition_limits(),
                )
                .await
            {
                Ok(issuances) => issuances,
                Err(error) => {
                    runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
                    return Err(error);
                }
            };
            (issuances, runtime.runtime.take_event_log().await.into())
        } else {
            (Vec::new(), Arc::from([]))
        };
        let replay_log = execution_log.into();
        if !evaluation.errors.is_empty() {
            runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
        }
        Ok(NativeOfferedAttempt {
            preflight: prepared,
            adopted: selected.adopted().clone(),
            block_data,
            runtime,
            evaluation,
            replay_log,
            grant_issuances,
            grant_issue_log,
            gate,
            // Added by G1-7: play reads the root after its run from its own
            // settlement checkpoint, and it never claims a guard failure.
            post_run_root: None,
            guard_claimed: false,
            // Added by G3 (DR-135, S7): the binding of the last pass. The
            // producer folds the recording for the owner totals.
            owner: Some(owner),
            owner_totals: None,
        })
    }

    /// Added by G3 (DR-135, S7): the table of the next discovery pass (R3):
    /// `table` with the first noted purses that fit its room. The budget of a
    /// purse is its balance at the execution root R_{i-1}, capped by the later
    /// holds of the batch and less the member's own hold (`owner_budget`).
    async fn list_owner_purses(
        &self,
        table: &OwnerTable,
        noted: Vec<rholang::rust::interpreter::accounting::owner_funding::OwnerPurse>,
        execution_root: [u8; 32],
        gate: &OfferedGate,
        later: &LaterHolds,
    ) -> Result<OwnerTable, CasperError> {
        let free = table.room().saturating_sub(table.len());
        let mut fresh = Vec::with_capacity(free.min(noted.len()));
        fresh.extend(noted.into_iter().take(free));
        let mut purses = Vec::with_capacity(fresh.len());
        purses.extend(fresh.iter().map(|purse| (purse.custody, &purse.signature)));
        let budgets = self
            .owner_budgets(&purses, execution_root, gate, later)
            .await?;
        let mut entries = Vec::with_capacity(table.len().saturating_add(fresh.len()));
        entries.extend(table.entries().iter().cloned());
        for (purse, budget) in fresh.into_iter().zip(budgets) {
            entries.push((purse.custody, OwnerTableEntry {
                signature: purse.signature,
                budget,
            }));
        }
        entries.sort_by_key(|(custody, _)| *custody);
        OwnerTable::new(entries, table.room())
            .map_err(|error| CasperError::RuntimeError(error.to_string()))
    }

    /// Added by G3 (DR-135, S7): checks that each entry of a recorded owner
    /// table lists the budget of its purse at the execution root of the
    /// member (R3, plan §6.2 "After the run"). Replay calls it, so a block
    /// whose table raises a budget is invalid.
    pub(super) async fn check_owner_table(
        &self,
        table: &OwnerTable,
        execution_root: [u8; 32],
        gate: &OfferedGate,
        later: &LaterHolds,
    ) -> Result<(), CasperError> {
        let mut purses = Vec::with_capacity(table.len());
        purses.extend(
            table
                .entries()
                .iter()
                .map(|(custody, entry)| (*custody, &entry.signature)),
        );
        let budgets = self
            .owner_budgets(&purses, execution_root, gate, later)
            .await?;
        if table
            .entries()
            .iter()
            .zip(&budgets)
            .any(|((_, entry), budget)| entry.budget != *budget)
        {
            return Err(CasperError::RuntimeError(
                "a recorded owner budget differs from its budget at the execution root".to_string(),
            ));
        }
        Ok(())
    }

    /// Added by G3 (DR-135, S7): the budget of each owner purse in `purses`
    /// (R3, §7 "Budget"): the balance of its vault at the execution root
    /// R_{i-1}, capped by the later holds of the batch and less the member's
    /// own hold at R0 (`owner_budget`). An absent vault has balance 0.
    async fn owner_budgets(
        &self,
        purses: &[([u8; 32], &CostSignature)],
        execution_root: [u8; 32],
        gate: &OfferedGate,
        later: &LaterHolds,
    ) -> Result<Vec<u64>, CasperError> {
        let mut payers: Vec<VaultPayer> = Vec::with_capacity(purses.len());
        for (_, signature) in purses {
            payers.push(
                vault_payer(signature)
                    .map_err(|error| CasperError::RuntimeError(error.to_string()))?,
            );
        }
        let mut located = Vec::with_capacity(payers.len());
        located.extend(payers.iter());
        let balances = self.read_balances_at(execution_root, &located).await?;
        let mut budgets = Vec::with_capacity(purses.len());
        for ((custody, _), balance) in purses.iter().zip(balances) {
            let balance = u64::try_from(balance.unwrap_or(0)).map_err(|_| {
                CasperError::RuntimeError("an owner purse has a negative balance".to_string())
            })?;
            let later_hold = later.holds().get(custody).copied().unwrap_or(0);
            let own_hold = gate
                .holds()
                .iter()
                .find(|(held, _)| held == custody)
                .map_or(0, |(_, hold)| *hold);
            budgets.push(owner_budget(balance, later_hold, own_hold));
        }
        Ok(budgets)
    }

    pub async fn evaluate_native_funded<'policy, 'a>(
        &self,
        policy: &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy>,
        envelope: &DeployEnvelope,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        context: NativeFundedExecutionContext,
    ) -> Result<NativeFundedAttempt<'policy, 'a>, CasperError> {
        let config = policy.prepare_execution(envelope, adopted, schedule, context.trace)?;
        let root = policy.snapshot().wallets().pre_state_root();
        let mut runtime = RuntimeOps::new(self.spawn_runtime().await?);
        runtime.runtime.set_block_data(context.block_data).await;
        runtime
            .runtime
            .set_invalid_blocks(context.invalid_blocks)
            .await;
        runtime
            .runtime
            .reset(&Blake2b256Hash::from_bytes(root.to_vec()))
            .await?;
        check_execution_root(&root, &runtime.runtime.get_root().await.bytes())?;
        let checkpoint = runtime.runtime.create_soft_checkpoint().await;
        let evaluation = match runtime.evaluate_native_envelope(envelope, config).await {
            Ok(evaluation) => evaluation,
            Err(error) => {
                runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
                return Err(error);
            }
        };
        if !evaluation.byte_observations.has_complete_measurements()
            || evaluation.native_phlo_usage.is_none()
            || runtime.runtime.get_root().await.bytes() != root
        {
            runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
            return Err(CasperError::RuntimeError(
                "native funded execution has incomplete evidence or changed its base root".into(),
            ));
        }
        let replay_log = runtime.runtime.take_event_log().await.into();
        if !evaluation.errors.is_empty() {
            runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
        }
        Ok(NativeFundedAttempt {
            policy,
            adopted: adopted.clone(),
            runtime,
            evaluation,
            replay_log,
            execution_root: root,
            draft_permit: None,
        })
    }
}

mod settlement;
pub use settlement::{
    NativeAttemptSettlementInput, NativeAttemptSettlementLimits, NativeCapturedSettlement,
    NativeFundedSettlement, NativeGrantIssuanceInput, NativeGrantSettlementInput,
};

mod replay;
pub use replay::{
    NativeFundedReplayInput, NativeFundedReplayLimits, PendingReplayedNativeSettlement,
    RecordedRun, ReplayedNativeFundedUser,
};

// Added by DR-134 (DR-65, gap G2): several offered deploys in one block.

// Added by DR-134 (DR-71): the exploratory estimate as a native unfunded dry run.

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn native_execution_guards_refine_exact_root_and_envelope_binding(
            root in any::<[u8; 32]>(), identity in any::<[u8; 32]>(),
            other_root in any::<[u8; 32]>(), other_identity in any::<[u8; 32]>(),
            keep_root in any::<bool>(), keep_identity in any::<bool>(),
        ) {
            let requested_root = if keep_root { root } else { other_root };
            let requested_identity = if keep_identity { identity } else { other_identity };
            prop_assert_eq!(check_execution_root(&root, &requested_root).is_ok(), root == requested_root);
            prop_assert_eq!(check_execution_identity(&identity, &requested_identity).is_ok(), identity == requested_identity);
            prop_assert_eq!(
                check_execution_root(&root, &requested_root).is_ok()
                    && check_execution_identity(&identity, &requested_identity).is_ok(),
                root == requested_root && identity == requested_identity,
            );
        }
    }

    /// DR-114: the record room fits every copy of the records in each carrier
    /// (Rocq `RecordedExternalReplay.charge_covers_record`).
    #[test]
    fn native_record_room_fits_every_record_copy() {
        let protocol = models::rust::cost_protocol_limits::offered_funded_v6_limits();
        let replay =
            crate::rust::util::rholang::costacc::production_limits::offered_funded_v6_replay_limits(
            );
        let room = usize::try_from(native_record_room()).expect("the room fits a usize");
        assert!(room > 0);
        assert!(3 * room <= replay.trace.telemetry_bytes);
        assert!(3 * room <= protocol.deploy_log_bytes);
        assert!(2 * room <= protocol.evidence.total_bytes);
    }
}
