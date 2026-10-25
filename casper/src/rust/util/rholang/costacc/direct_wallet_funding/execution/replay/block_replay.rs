use std::collections::HashMap;
use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::Event as CasperEvent;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::normalizer_env::normalizer_env_from_envelope;
use models::rust::phlo_intent::PhloConversionCompositionV2;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::validator::Validator;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::economic_failure::classify_errors;
use rholang::rust::interpreter::accounting::owner_funding::{
    recorded_owner_table, OwnerFundingBinding,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_offered_phlo_controls, PhloControlsBinding, PhloOffer, PhloScheduleBinding,
};
use rholang::rust::interpreter::accounting::phlo_execution::PhloFailure;
use rholang::rust::interpreter::accounting::{
    decode_native_budget_recording, decode_native_operation_journal, funding_sig,
    NativeRuntimeConfig, RuntimeBudget,
};
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::interpreter::InterpreterImpl;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::{BlockData, DeployData};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use tokio::sync::RwLock;

use super::super::block_gate::offered_member_limits;
use super::super::block_settlement::OfferedRun;
use super::super::OfferedGate;
use super::{checked_replay_events, settlement_root};
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::event_converter;
use crate::rust::util::rholang::acceptance::prepare_offered_candidate;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::costacc::offered_evidence::native_user_event_count;
use crate::rust::util::rholang::costacc::offered_grants::{
    grant_issue_definition, offered_grant_issue_call_limits, offered_grant_transition_limits,
};
use crate::rust::util::rholang::costacc::production_limits::{
    offered_funded_v6_production_limits, offered_funded_v6_replay_limits,
    offered_funded_v6_trace_limits,
};
use crate::rust::util::rholang::costacc::vault_payer::purse_of;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::tools::Tools;

pub struct RecordedRun<'a> {
    pub phlo_used: u64,
    pub failed: bool,
    pub budget_recording: &'a [u8],
    pub operation_journal: &'a [u8],
    pub user_log: &'a [CasperEvent],
}

fn invalid(message: impl std::fmt::Display) -> CasperError {
    CasperError::RuntimeError(message.to_string())
}

impl RuntimeManager {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn replay_offered_run(
        &self,
        envelope: &DeployEnvelope,
        funding_root: [u8; 32],
        execution_root: [u8; 32],
        gate: OfferedGate,
        recorded: RecordedRun<'_>,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
        adopted: &AdoptedResourcePolicy,
    ) -> Result<OfferedRun, CasperError> {
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(invalid(
                "an offered replay needs an offered-funded envelope",
            ));
        };
        let limits = offered_member_limits();
        let prepared =
            prepare_offered_candidate(signed, adopted, funding_root, execution_root, limits)?;
        if !matches!(
            prepared.intent.conversion,
            PhloConversionCompositionV2::NoConversion
        ) {
            return Err(invalid(
                "native offered witness conversion is not supported",
            ));
        }
        let selected = prepared.selected_terms(adopted)?;
        let schedule = PhloScheduleBinding::new(selected.schedule(), PhloGenesisPolicy::LIMITS)
            .map_err(invalid)?;
        let signed_controls = PhloControlsBinding::new(
            &prepared.intent.base.controls,
            limits.funding.base.controls(),
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
            gate.resource_bound(),
        )
        .map_err(invalid)?;
        let identity = prepared.envelope_identity;
        let recording_limits = offered_funded_v6_production_limits().recording;
        let replay_limits = offered_funded_v6_replay_limits();
        let mut recording =
            decode_native_budget_recording(recorded.budget_recording, recording_limits)
                .map_err(invalid)?;
        if recording.used != recorded.phlo_used || recording.session != identity {
            return Err(invalid(
                "the recorded budget differs from the executed offer or usage",
            ));
        }
        let operations = decode_native_operation_journal(
            recorded.operation_journal,
            &mut recording,
            recording_limits,
        )
        .map_err(invalid)?;
        if operations
            .iter()
            .any(|row| row.occurrence.session != identity)
        {
            return Err(invalid(
                "the recorded journal differs from the executed offer",
            ));
        }
        let owner_table =
            recorded_owner_table(&recording.owner_table, &purse_of, gate.owner_room())
                .map_err(invalid)?;
        let owner = Arc::new(OwnerFundingBinding::new(
            gate.client_lanes().clone(),
            purse_of,
            owner_table,
        ));
        let contract =
            adopted.bind_execution_contract(checked.controls(), &schedule, gate.bound_source())?;
        let config = NativeRuntimeConfig::new(contract, offered_funded_v6_trace_limits())
            .with_owner_funding(Arc::clone(&owner));
        let user_events = native_user_event_count(&operations, replay_limits.trace.events)?;
        let user_prefix = recorded
            .user_log
            .get(..user_events)
            .ok_or_else(|| invalid("the recorded user log omits native user events"))?;
        let wire_limits = offered_funded_v6_limits().evidence;
        let events =
            checked_replay_events(user_prefix, replay_limits.trace, wire_limits.total_bytes)?;
        let journal = adopted
            .bind_execution_contract(checked.controls(), &schedule, gate.bound_source())?
            .check_operation_journal_with_owners(
                identity,
                &recording,
                operations,
                replay_limits.journal,
                Some(owner.as_ref()),
            )
            .map_err(invalid)?;
        let trace = journal
            .bind_trace(events.clone(), replay_limits.trace)
            .map_err(invalid)?;
        let cost = RuntimeBudget::new(Cost::unsafe_max());
        cost.set_deploy_id_funded(identity, funding_sig(signed))
            .map_err(invalid)?;
        cost.reset_for_native_execution(config)?;
        let before = Blake2b256Hash::from_bytes(execution_root.to_vec());
        let history = Arc::new(self.history_repo.reset(&before).map_err(invalid)?);
        if history.root().bytes() != before.bytes() {
            return Err(invalid("the replay history is at another root"));
        }
        let parsed = InterpreterImpl::parse_source(
            &envelope.body().term,
            normalizer_env_from_envelope(envelope),
        )?;
        let mut offered_definitions =
            vec![grant_issue_definition(offered_grant_issue_call_limits())];
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
                None,
            )
            .await?;
        *replay.block_data.write().await = block_data.clone();
        replay.set_invalid_blocks(invalid_blocks.clone()).await;
        *replay.deploy_data.write().await = DeployData::from_envelope(envelope);
        let evaluation = replay
            .evaluate(parsed, Tools::user_envelope_rng(envelope))
            .await?;
        let failures = evaluation
            .economic_failures
            .union(classify_errors(&evaluation.errors).map_err(invalid)?);
        if !failures.permits_retained_charge() {
            return Err(invalid(
                "the replayed offer has a non-user economic failure",
            ));
        }
        let user_failure = failures.contains(PhloFailure::User);
        if evaluation.native_phlo_usage != Some(recorded.phlo_used)
            || user_failure != recorded.failed
        {
            return Err(invalid(
                "the replayed offer differs from the executed usage or failure",
            ));
        }
        let exported = replay.export().await?;
        let root = settlement_root(before, exported.root().clone(), user_failure);
        let mut runtime = RuntimeOps::new(
            self.spawn_offered_runtime(offered_grant_issue_call_limits())
                .await?,
        );
        runtime.runtime.set_block_data(block_data.clone()).await;
        runtime
            .runtime
            .set_invalid_blocks(invalid_blocks.clone())
            .await;
        runtime
            .runtime
            .set_deploy_data(DeployData::from_envelope(envelope))
            .await;
        runtime.runtime.reset(&root).await?;
        let grant_issuances = if user_failure {
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
        let grant_casper: Vec<CasperEvent> = grant_log
            .iter()
            .cloned()
            .map(event_converter::to_casper_event)
            .collect();
        if recorded.user_log.get(user_events..) != Some(grant_casper.as_slice()) {
            return Err(invalid(
                "the replayed grant log differs from the recorded user log",
            ));
        }
        let post_root: [u8; 32] = runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .bytes()
            .try_into()
            .map_err(|_| invalid("the replayed post-run root is not 32 bytes"))?;
        Ok(OfferedRun::from_parts(
            evaluation,
            events,
            grant_issuances,
            Arc::from(grant_log),
            gate,
            Some(owner),
            post_root,
        ))
    }
}
