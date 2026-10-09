use std::collections::HashMap;
use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::phlo_grant_creation::VerifiedGrantCreationV1;
use models::rust::phlo_intent::PhloConversionCompositionV2;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use models::rust::validator::Validator;
use rholang::rust::interpreter::accounting::native_phlo_rules::NativeBudgetTraceLimits;
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_offered_phlo_controls, PhloControlsBinding, PhloOffer, PhloScheduleBinding,
};
use rholang::rust::interpreter::accounting::NativeRuntimeConfig;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::trace::event::Event;

use super::CheckedDirectWalletPolicy;
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::acceptance::{
    prepare_offered_candidate, CandidateDraftSettlementPermit, OfferedCandidateLimits,
    PreparedOfferedCandidate,
};
use crate::rust::util::rholang::costacc::genesis_resource_policy::{
    AdoptedResourcePolicy, CompatibleAcquisitionTerms,
};
use crate::rust::util::rholang::costacc::offered_grants::{
    offered_grant_issue_call_limits, offered_grant_transition_limits,
};
use crate::rust::util::rholang::costacc::prepaid_receipts::PrepaidStackCaptureLimits;
use crate::rust::util::rholang::costacc::supply::PurseStack;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub struct NativeFundedExecutionContext {
    pub block_data: BlockData,
    pub invalid_blocks: HashMap<BlockHash, Validator>,
    pub trace: NativeBudgetTraceLimits,
    pub host_work: HostWorkBudget,
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
}

impl<'preflight, 'a> NativeOfferedAttempt<'preflight, 'a> {
    pub fn evaluation(&self) -> &EvaluateResult { &self.evaluation }

    pub fn replay_log(&self) -> &[Event] { &self.replay_log }

    pub fn grant_issuances(&self) -> &[VerifiedGrantCreationV1] { &self.grant_issuances }

    pub fn grant_issue_log(&self) -> &[Event] { &self.grant_issue_log }

    pub async fn abort(mut self) -> Result<(), CasperError> {
        let root = Blake2b256Hash::from_bytes(self.preflight.execution_root.to_vec());
        self.runtime.runtime.reset(&root).await?;
        check_execution_root(
            &self.preflight.execution_root,
            &self.runtime.runtime.get_root().await.bytes(),
        )
    }

    pub async fn read_measured_prepaid_stacks(
        &self,
        manager: &RuntimeManager,
        family: NativeOfferedFamilyLimits,
        capture: PrepaidStackCaptureLimits,
        budget: &HostWorkBudget,
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
        let regions = measured
            .region_demands(family.measured.regions, budget)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let located = regions
            .locate_purses(family.measured.purses, budget)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let demand = located
            .prepare_acquisition_demand(
                &schedule,
                selected.bytes(),
                family.measured.acquisition,
                budget,
            )
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        manager.read_measured_prepaid_stacks(self.preflight.funding_root, &demand, capture, budget)
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
        budget: HostWorkBudget,
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
        Ok(NativeRuntimeConfig::new(contract, limits, budget))
    }
}

impl RuntimeManager {
    pub async fn evaluate_native_offered<'preflight, 'a>(
        &self,
        prepared: &'preflight PreparedOfferedCandidate<'a>,
        envelope: &DeployEnvelope,
        selected: &CompatibleAcquisitionTerms<'_, '_>,
        limits: OfferedCandidateLimits,
        context: NativeFundedExecutionContext,
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
            prepared.phlo_limit,
        )
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        // DR-113: the bound of an offer is its signed limit.
        let contract = selected
            .adopted()
            .bind_execution_contract(checked.controls(), &schedule, rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource::SignedLimit)?;
        let host_work = context.host_work.clone();
        // Changed by DR-114: play records external-service replies within a
        // room, and the play runtime calls the node's services.
        // let config = NativeRuntimeConfig::new(contract, context.trace, context.host_work);
        // let mut runtime = RuntimeOps::new(
        //     self.spawn_offered_runtime(offered_grant_issue_call_limits(), host_work.clone())
        //         .await?,
        // );
        let config = NativeRuntimeConfig::new(contract, context.trace, context.host_work)
            .with_record_room(native_record_room());
        let mut runtime = RuntimeOps::new(
            self.spawn_offered_play_runtime(offered_grant_issue_call_limits(), host_work.clone())
                .await?,
        );
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
        let checkpoint = runtime.runtime.create_soft_checkpoint().await;
        let evaluation = match runtime.evaluate_native_envelope(envelope, config).await {
            Ok(evaluation) => evaluation,
            Err(error) => {
                runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
                return Err(error);
            }
        };
        if !evaluation.byte_observations.has_complete_measurements() {
            runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
            return Err(CasperError::RuntimeError(format!(
                "native offered execution has incomplete byte measurements: errors={:?}, host_rejection={:?}",
                evaluation.errors,
                host_work.rejection(),
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
                    &host_work,
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
        })
    }

    pub async fn evaluate_native_funded<'policy, 'a>(
        &self,
        policy: &'policy CheckedDirectWalletPolicy<'a, OfferedFundedDeploy>,
        envelope: &DeployEnvelope,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        context: NativeFundedExecutionContext,
    ) -> Result<NativeFundedAttempt<'policy, 'a>, CasperError> {
        let config = policy.prepare_execution(
            envelope,
            adopted,
            schedule,
            context.trace,
            context.host_work,
        )?;
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
    ReplayedNativeFundedUser,
};

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
