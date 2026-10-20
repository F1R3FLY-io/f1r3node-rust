//! G2-1 (DR-65 implementation note): the single-offer play and replay code,
//! moved with no behavior change from `casper/src/rust/rholang/runtime.rs` and
//! `casper/src/rust/util/rholang/runtime_manager.rs`.

use std::collections::HashMap;

use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{ProcessedSystemDeploy, ProcessedUserDeploy};
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::native_cost_evidence::NativeCostEvidenceV1;
use models::rust::phlo_intent::{PhloConversionCompositionV2, PhloFundingIntentV2Limits};
use models::rust::validator::Validator;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsEndVal;

use super::{NativeFundedExecutionContext, NativeGrantSettlementInput};
use crate::rust::errors::CasperError;
use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;
use crate::rust::rholang::runtime::{offered_candidate_rejection, RuntimeOps};
use crate::rust::util::rholang::acceptance::{prepare_offered_candidate, OfferedCandidateLimits};
use crate::rust::util::rholang::costacc::genesis_resource_policy::{
    AdoptedResourcePolicy, OFFERED_PRODUCTION_READY,
};
#[cfg(any(test, feature = "test-utils"))]
use crate::rust::util::rholang::costacc::offered_acceptance::OfferedUsageKind;
use crate::rust::util::rholang::costacc::offered_grants::{
    offered_grant_issue_call_limits, offered_grant_transition_limits,
};
use crate::rust::util::rholang::costacc::production_limits::{
    offered_funded_v6_prepaid_inventory_limits, offered_funded_v6_prepaid_receipt_limits,
    offered_funded_v6_production_limits, offered_funded_v6_recording_limits,
    offered_funded_v6_replay_limits, offered_funded_v6_trace_limits,
};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub(crate) struct CertifiedOfferedDraft {
    settled_root: [u8; 32],
    envelope_identity: [u8; 32],
    candidate: OfferedProcessedDeploy,
    user_mergeable: NumberChannelsEndVal,
}

impl CertifiedOfferedDraft {
    pub(crate) fn settled_root(&self) -> [u8; 32] { self.settled_root }

    /// DR-102: the certified mergeable map, which the producer and every
    /// validator store for this deploy.
    pub(crate) fn into_user_mergeable(self) -> NumberChannelsEndVal { self.user_mergeable }

    pub(crate) fn envelope_identity(&self) -> [u8; 32] { self.envelope_identity }

    pub(crate) fn matches_candidate(&self, candidate: &OfferedProcessedDeploy) -> bool {
        self.candidate == *candidate
    }
}

impl RuntimeOps {
    pub(crate) async fn compute_offered_envelope_state(
        &mut self,
        start_hash: &StateHash,
        envelope: &DeployEnvelope,
        system_deploys: Vec<crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
        play_budget: Option<std::time::Duration>,
        adopted: Option<&AdoptedResourcePolicy>,
        manager: &RuntimeManager,
    ) -> Result<
        (
            StateHash,
            Vec<(ProcessedUserDeploy, NumberChannelsEndVal)>,
            Vec<(ProcessedSystemDeploy, NumberChannelsEndVal)>,
        ),
        CasperError,
    > {
        let adopted = adopted.ok_or_else(|| {
            CasperError::RuntimeError(
                "offered proposal has no authenticated genesis policy".to_string(),
            )
        })?;
        let root: [u8; 32] = start_hash.as_ref().try_into().map_err(|_| {
            CasperError::RuntimeError(
                "offered proposal requires a 32-byte original root".to_string(),
            )
        })?;
        // Changed by DR-102: the candidate step also returns the certified
        // mergeable map, which the producer stores like every validator.
        // let candidate = async {
        let (candidate, certified_mergeable) = async {
            let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
                return Err(CasperError::RuntimeError(
                    "offered proposal has an unexpected envelope format".to_string(),
                ));
            };
            let base = models::rust::cost_protocol_limits::offered_funded_v6_limits()
                .envelope
                .payload
                .funding;
            let limits = OfferedCandidateLimits {
                funding: PhloFundingIntentV2Limits {
                    wire: base.wire,
                    base,
                    grant_uses: base.wire.total_bytes / 8,
                    grant_id_bytes: base.wire.field_bytes,
                    quote_evidence_bytes: base.wire.field_bytes,
                },
                max_phlo_limit: i64::MAX as u64,
            };
            let prepared = prepare_offered_candidate(signed, adopted, root, root, limits)?;
            if !matches!(
                prepared.intent.conversion,
                PhloConversionCompositionV2::NoConversion
            ) {
                return Err(CasperError::RuntimeError(
                    "offered conversion requires authenticated production composition".to_string(),
                ));
            }
            let grant_limits = offered_grant_transition_limits();
            let grant_context = if prepared.intent.grant_uses.is_empty() {
                None
            } else {
                let authenticated_time = u64::try_from(block_data.time_stamp).map_err(|_| {
                    CasperError::RuntimeError(
                        "offered grant use requires nonnegative block time".to_string(),
                    )
                })?;
                let grant_snapshot =
                    manager.capture_offered_grants_from_signed(root, signed, grant_limits)?;
                let verified = grant_snapshot.verified_source_payers(
                    root,
                    signed,
                    authenticated_time,
                    grant_limits,
                )?;
                Some((grant_snapshot, verified, authenticated_time))
            };
            let snapshot = manager
                .read_direct_offered_wallet_snapshot(
                    envelope,
                    // Added by the merge of feature/cost-accounted-rho (finding
                    // R3-03): F1R3Lang's reader takes the adopted policy.
                    adopted,
                    start_hash,
                    grant_context.as_ref().map(|(_, verified, _)| verified),
                )
                .await?;
            if snapshot.wallets().pre_state_root() != root {
                return Err(CasperError::RuntimeError(
                    "offered wallet snapshot belongs to another original root".to_string(),
                ));
            }
            let selected = prepared.selected_terms(adopted)?;
            // Changed by G1-6 (DR-124): the attempt runs the check before
            // execution with the wallet snapshot at R0.
            let attempt = manager
                .evaluate_native_offered(
                    &prepared,
                    envelope,
                    &selected,
                    limits,
                    &snapshot,
                    NativeFundedExecutionContext {
                        block_data: block_data.clone(),
                        invalid_blocks: invalid_blocks.clone(),
                        trace: offered_funded_v6_trace_limits(),
                    },
                )
                .await?;
            let production = offered_funded_v6_production_limits();
            let candidate = attempt
                .produce_native_offered_candidate_with_rooted_prepaid(
                    manager,
                    envelope.clone(),
                    &snapshot,
                    grant_context
                        .as_ref()
                        .map(|(snapshot, verified, authenticated_time)| {
                            NativeGrantSettlementInput {
                                snapshot,
                                verified,
                                authenticated_time: *authenticated_time,
                                limits: grant_limits,
                            }
                        }),
                    production,
                    offered_funded_v6_prepaid_inventory_limits(),
                    offered_funded_v6_prepaid_receipt_limits(),
                )
                .await?;
            // Changed by DR-102: certify borrows the block context.
            // let replay_budget = HostWorkBudget::new(
            //     models::rust::cost_protocol_limits::offered_funded_v6_host_work_limits(),
            // );
            // replay_budget
            //     .reserve(
            //         HostWorkDimension::SearchStateBytes,
            //         HostWorkUnits::new(u64::try_from(context_copy_bytes).map_err(|_| {
            //             CasperError::RuntimeError(
            //                 "offered replay context copy overflows".to_string(),
            //             )
            //         })?),
            //     )
            //     .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            // let certificate = manager
            //     .certify_offered_draft(
            //         &candidate.processed,
            //         start_hash,
            //         &block_data,
            //         invalid_blocks.clone(),
            //         adopted,
            //         &replay_budget,
            //     )
            //     .await?;
            // let _publication = prepared.authorize_publication_after_replay(
            //     &candidate.processed,
            //     candidate.final_root,
            //     &certificate,
            //     &replay_budget,
            // )?;
            // Ok(candidate)
            let certificate = manager
                .certify_offered_draft(
                    &candidate.processed,
                    start_hash,
                    &block_data,
                    &invalid_blocks,
                    adopted,
                )
                .await?;
            let _publication = prepared.authorize_publication_after_replay(
                &candidate.processed,
                candidate.final_root,
                &certificate,
            )?;
            #[cfg(any(test, feature = "test-utils"))]
            manager.record_offered_budget_usage(
                OfferedUsageKind::ProducerSelfReplay,
                candidate.processed.identity_bytes(),
                start_hash,
            );
            Ok((candidate, certificate.into_user_mergeable()))
        }
        .await
        .map_err(|error| offered_candidate_rejection(envelope, start_hash, error))?;
        let candidate_root: StateHash = candidate.final_root.to_vec().into();
        self.runtime
            .reset(&Blake2b256Hash::from_bytes(candidate.final_root.to_vec()))
            .await?;
        // Changed by DR-102: the producer stores the certified mergeable map,
        // the map that every validator stores, instead of reading the map of
        // its own play again on the execution budget.
        // let mergeable = self
        //     .get_number_channels_data_metered(&candidate.mergeable, budget)
        //     .await?;
        let mergeable = certified_mergeable;
        let (post_root, extra_users, systems) = self
            .compute_state(
                &candidate_root,
                Vec::new(),
                system_deploys,
                block_data,
                invalid_blocks,
                play_budget,
            )
            .await?;
        if !extra_users.is_empty() {
            return Err(CasperError::RuntimeError(
                "offered system continuation unexpectedly evaluated another user".to_string(),
            ));
        }
        Ok((
            post_root,
            vec![(ProcessedUserDeploy::Offered(candidate.processed), mergeable)],
            systems,
        ))
    }
}

impl RuntimeManager {
    // Changed by DR-102: certify borrows the block context.
    // pub(crate) async fn certify_offered_draft(
    //     &self,
    //     processed: &OfferedProcessedDeploy,
    //     start_hash: &StateHash,
    //     block_data: &BlockData,
    //     invalid_blocks: HashMap<BlockHash, Validator>,
    //     adopted: &AdoptedResourcePolicy,
    //     budget: &HostWorkBudget,
    // ) -> Result<CertifiedOfferedDraft, CasperError> {
    pub(crate) async fn certify_offered_draft(
        &self,
        processed: &OfferedProcessedDeploy,
        start_hash: &StateHash,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
        adopted: &AdoptedResourcePolicy,
    ) -> Result<CertifiedOfferedDraft, CasperError> {
        // Changed by the merge of feature/cost-accounted-rho (finding R3-04):
        // F1R3Lang renamed the check, so that protocol 7 also counts as active.
        // if !adopted.offered_funded_v6_active() {
        if !adopted.offered_funded_active() {
            return Err(CasperError::RuntimeError(
                "offered-funded replay is not active for this block".to_string(),
            ));
        }
        let protocol = offered_funded_v6_limits();
        let evidence_bytes = processed.native_cost_evidence_bytes().len();
        if evidence_bytes > protocol.evidence.total_bytes
            || processed.deploy_log().len() > protocol.deploy_log_events
        {
            return Err(CasperError::RuntimeError(
                "offered replay candidate exceeds protocol limits".to_string(),
            ));
        }
        NativeCostEvidenceV1::decode(processed.native_cost_evidence_bytes(), protocol.evidence)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        processed
            .validate_for_epoch(protocol.evidence, adopted.event_codec_epoch())
            .map_err(CasperError::RuntimeError)?;
        let evidence = processed
            .evidence(protocol.evidence)
            .map_err(CasperError::RuntimeError)?;
        let original_root: [u8; 32] = start_hash.as_ref().try_into().map_err(|_| {
            CasperError::RuntimeError("offered replay requires a 32-byte original root".to_string())
        })?;
        if evidence.original_funding_root != original_root {
            return Err(CasperError::RuntimeError(
                "offered replay funding root differs from block pre-state".to_string(),
            ));
        }
        let DeployEnvelopeRef::OfferedFunded(signed) = processed.envelope().view() else {
            return Err(CasperError::RuntimeError(
                "offered replay requires a signed funded envelope".to_string(),
            ));
        };
        let funding = protocol.envelope.payload.funding;
        let candidate_limits = OfferedCandidateLimits {
            funding: PhloFundingIntentV2Limits {
                wire: funding.wire,
                base: funding,
                grant_uses: funding.wire.total_bytes / 8,
                grant_id_bytes: funding.wire.field_bytes,
                quote_evidence_bytes: funding.wire.field_bytes,
            },
            max_phlo_limit: i64::MAX as u64,
        };
        let prepared = prepare_offered_candidate(
            signed,
            adopted,
            original_root,
            original_root,
            candidate_limits,
        )?;
        if !matches!(
            prepared.intent.conversion,
            PhloConversionCompositionV2::NoConversion
        ) {
            return Err(CasperError::RuntimeError(
                "offered replay conversion requires authenticated composition".to_string(),
            ));
        }
        let grant_context = if prepared.intent.grant_uses.is_empty() {
            None
        } else {
            let authenticated_time = u64::try_from(block_data.time_stamp).map_err(|_| {
                CasperError::RuntimeError(
                    "offered replay grant use requires nonnegative block time".to_string(),
                )
            })?;
            let grant_limits = offered_grant_transition_limits();
            let grant_snapshot =
                self.capture_offered_grants_from_signed(original_root, signed, grant_limits)?;
            let verified = grant_snapshot.verified_source_payers(
                original_root,
                signed,
                authenticated_time,
                grant_limits,
            )?;
            Some((grant_snapshot, verified, authenticated_time))
        };
        let snapshot = self
            .read_direct_offered_wallet_snapshot(
                processed.envelope(),
                // Added by the merge of feature/cost-accounted-rho (finding
                // R3-03): F1R3Lang's reader takes the adopted policy.
                adopted,
                start_hash,
                grant_context.as_ref().map(|(_, verified, _)| verified),
            )
            .await?;
        if snapshot.wallets().pre_state_root() != original_root {
            return Err(CasperError::RuntimeError(
                "offered replay wallet snapshot belongs to another root".to_string(),
            ));
        }
        let selected = prepared.selected_terms(adopted)?;
        let schedule = PhloScheduleBinding::new(
            selected.schedule(),
            models::rust::phlo_schedule::PhloGenesisPolicy::LIMITS,
        )
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        if evidence.genesis_policy_commitment != adopted.genesis_policy_commitment()?
            || evidence.schedule_commitment != schedule.schedule().commitment
            || evidence.phlo_price != schedule.schedule().actual_price
            || evidence.phlo_limit != prepared.phlo_limit
            || evidence.phlo_price != prepared.phlo_price
            || evidence.fee_rev != prepared.fee_rev
            || evidence.envelope_commitment != prepared.envelope_identity
        {
            return Err(CasperError::RuntimeError(
                "offered replay evidence differs from adopted policy or selected schedule"
                    .to_string(),
            ));
        }
        // Changed by G1-6 (DR-124): the witness replay runs the check before
        // execution with the wallet snapshot at R0, as play does.
        let witness = self
            .replay_native_offered_witness(
                &prepared,
                processed,
                adopted,
                &schedule,
                &snapshot,
                NativeFundedExecutionContext {
                    block_data: block_data.clone(),
                    // Changed by DR-102: certify borrows the block context.
                    // invalid_blocks,
                    invalid_blocks: invalid_blocks.clone(),
                    trace: offered_funded_v6_trace_limits(),
                },
                candidate_limits,
                offered_funded_v6_recording_limits(),
                offered_funded_v6_replay_limits(),
            )
            .await?;
        let settlement = witness
            .settle_replayed_candidate(
                self,
                processed,
                &snapshot,
                grant_context
                    .as_ref()
                    .map(|(grant_snapshot, verified, authenticated_time)| {
                        NativeGrantSettlementInput {
                            snapshot: grant_snapshot,
                            verified,
                            authenticated_time: *authenticated_time,
                            limits: offered_grant_transition_limits(),
                        }
                    }),
                offered_funded_v6_production_limits(),
                offered_funded_v6_prepaid_inventory_limits(),
                offered_funded_v6_prepaid_receipt_limits(),
            )
            .await?;
        let settled_root = settlement.final_root;
        let mut runtime = RuntimeOps::new(
            self.spawn_offered_runtime(
                adopted.event_codec_epoch(),
                offered_grant_issue_call_limits(),
            )
            .await?,
        );
        runtime
            .runtime
            .reset(&Blake2b256Hash::from_bytes(settled_root.to_vec()))
            .await?;
        if runtime.runtime.get_root().await.bytes() != settled_root {
            return Err(CasperError::RuntimeError(
                "offered certificate mergeable runtime has wrong root".to_string(),
            ));
        }
        let user_mergeable = runtime
            .get_number_channels_data_metered(&settlement.mergeable)
            .await?;
        Ok(CertifiedOfferedDraft {
            settled_root,
            envelope_identity: prepared.envelope_identity,
            candidate: processed.clone(),
            user_mergeable,
        })
    }

    async fn replay_offered_system_continuation(
        &self,
        certificate: &CertifiedOfferedDraft,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
        epoch: rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch,
    ) -> Result<(Blake2b256Hash, Vec<NumberChannelsEndVal>), CasperError> {
        let candidate_root = StateHash::copy_from_slice(&certificate.settled_root());
        let runtime = self
            .spawn_offered_replay_runtime(epoch, offered_grant_issue_call_limits())
            .await?;
        let mut replay = ReplayRuntimeOps::new_from_runtime(runtime);
        replay
            .replay_compute_state(
                &candidate_root,
                Vec::new(),
                system_deploys,
                block_data,
                Some(invalid_blocks),
                false,
            )
            .await
    }

    pub(crate) async fn replay_offered_envelope_state(
        &self,
        start_hash: &StateHash,
        terms: Vec<ProcessedUserDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool,
        adopted: Option<&AdoptedResourcePolicy>,
        expected_post_state: Option<&StateHash>,
    ) -> Result<StateHash, CasperError> {
        let adopted = adopted.ok_or_else(|| {
            CasperError::RuntimeError(
                "offered-funded replay requires authenticated adopted resource policy".to_string(),
            )
        })?;
        // Changed by the merge of feature/cost-accounted-rho (finding R3-04):
        // F1R3Lang renamed the check, so that protocol 7 also counts as active.
        // if is_genesis || !adopted.offered_funded_v6_active() {
        if is_genesis || !adopted.offered_funded_active() {
            return Err(CasperError::RuntimeError(
                "offered-funded replay is not active for this block".to_string(),
            ));
        }
        let [ProcessedUserDeploy::Offered(processed)] = terms.as_slice() else {
            return Err(CasperError::RuntimeError(
                "offered-funded replay requires one isolated user candidate".to_string(),
            ));
        };
        // Changed by DR-102: certify borrows the block context.
        // let budget = HostWorkBudget::new(offered_funded_v6_host_work_limits());
        // let certificate = self
        //     .certify_offered_draft(
        //         processed,
        //         start_hash,
        //         block_data,
        //         invalid_blocks.clone().unwrap_or_default(),
        //         adopted,
        //         &budget,
        //     )
        //     .await?;
        let no_invalid_blocks = HashMap::new();
        let certificate = self
            .certify_offered_draft(
                processed,
                start_hash,
                block_data,
                invalid_blocks.as_ref().unwrap_or(&no_invalid_blocks),
                adopted,
            )
            .await?;
        let (post_root, system_mergeable) = self
            .replay_offered_system_continuation(
                &certificate,
                system_deploys,
                block_data,
                invalid_blocks.unwrap_or_default(),
                adopted.event_codec_epoch(),
            )
            .await?;
        let expected = expected_post_state.ok_or_else(|| {
            CasperError::RuntimeError(
                "offered block replay requires authenticated expected post-state".to_string(),
            )
        })?;
        if post_root.to_bytes_prost() != *expected {
            return Err(CasperError::RuntimeError(
                "offered block replay post-state differs from commitment".to_string(),
            ));
        }
        if !OFFERED_PRODUCTION_READY {
            return Err(CasperError::RuntimeError(
                "offered-funded replay remains closed pending block-level qualification"
                    .to_string(),
            ));
        }
        let channel_count = system_mergeable.len().checked_add(1).ok_or_else(|| {
            CasperError::RuntimeError("offered mergeable result count overflows".to_string())
        })?;
        let mut mergeable_values = Vec::new();
        mergeable_values
            .try_reserve_exact(channel_count)
            .map_err(|_| {
                CasperError::RuntimeError("offered mergeable result allocation failed".to_string())
            })?;
        mergeable_values.push(certificate.user_mergeable);
        mergeable_values.extend(system_mergeable);
        self.save_mergeable_channels_metered(
            post_root.clone(),
            block_data.sender.bytes.clone(),
            block_data.seq_num,
            mergeable_values,
            &Blake2b256Hash::from_bytes_prost(start_hash),
            adopted.event_codec_epoch(),
        )
        .map_err(|error| {
            CasperError::RuntimeError(format!(
                "Failed to save offered mergeable channels: {:?}",
                error
            ))
        })?;
        #[cfg(any(test, feature = "test-utils"))]
        self.record_offered_budget_usage(
            OfferedUsageKind::ValidatorReplay,
            processed.identity_bytes(),
            start_hash,
        );
        Ok(post_root.to_bytes_prost())
    }
}
