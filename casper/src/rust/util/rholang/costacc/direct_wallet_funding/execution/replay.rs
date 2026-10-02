use std::collections::HashMap;
use std::mem::size_of;
use std::sync::Arc;

use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::Event as CasperEvent;
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::native_cost_evidence::{NativeCostEvidenceV1, NativeCostFailureClass};
use models::rust::native_wallet_receipt::NativeWalletReceiptLimits;
use models::rust::normalizer_env::normalizer_env_from_envelope;
use models::rust::phlo_grant_creation::VerifiedGrantCreationV1;
use models::rust::phlo_intent::PhloConversionCompositionV2;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use rholang::rust::interpreter::accounting::authority::AuthorityStackBirth;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::economic_failure::classify_errors;
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_offered_phlo_controls, PhloControlsBinding, PhloFundingTerms, PhloOffer,
    PhloScheduleBinding,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, check_phlo_funding_family, project_phlo_obligations, PhloFailure,
    PhloFundingCase, PhloFundingIntentBinding, PhloOutcome, RetainedBirthFunding,
};
use rholang::rust::interpreter::accounting::{
    funding_sig, NativeBudgetRecording, NativeOperationJournalLimits, NativeOperationRecord,
    NativeOperationTraceLimits, NativeRecordingWireLimits, NativeRuntimeConfig, RuntimeBudget,
};
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::interpreter::InterpreterImpl;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::{create_native_replay_env, RhoRuntime};
use rholang::rust::interpreter::system_processes::DeployData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
use rspace_plus_plus::rspace::trace::event::Event;
use tokio::sync::RwLock;

use super::producer::{
    check_birth_positions, check_retained_origin, derive_birth_positions, derive_live_born_stacks,
    derive_retained_amounts, restore_retained_permissions,
};
use super::settlement::{
    check_metered_usage, NativeAttemptSettlementInput, NativeAttemptSettlementLimits,
    NativeCapturedSettlement, NativeFundedSettlement, NativeGrantIssuanceInput,
    NativeGrantSettlementInput,
};
use super::{
    select_rooted_native_family, CheckedDirectWalletPolicy, NativeFundedAttempt,
    NativeFundedExecutionContext, NativeOfferedAttempt, NativeOfferedProductionLimits,
};
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::event_converter;
use crate::rust::util::rholang::acceptance::{
    prepare_offered_candidate, CandidateReplayPermit, OfferedCandidateLimits,
    PreparedOfferedCandidate,
};
use crate::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletPolicySnapshot;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::costacc::offered_context::OfferedSettlementContext;
use crate::rust::util::rholang::costacc::offered_evidence::{
    decode_committed_runtime_recording, encode_measured_funding_case, encode_measured_prepaid_delta,
};
use crate::rust::util::rholang::costacc::offered_grants::{
    grant_issue_definition, offered_grant_issue_call_limits, offered_grant_transition_limits,
};
use crate::rust::util::rholang::costacc::prepaid_receipts::{
    NativePrepaidConsumption, NativePrepaidDemandInput, NativePrepaidInventoryLimits,
    NativeRetainedReceiptRecords, NativeRetainedSettlementLimits, PrepaidReceiptBucket,
    PrepaidReceiptChange, PrepaidReceiptLimits, PrepaidStackCaptureLimits,
    PreparedNativeRetainedReceipts,
};
use crate::rust::util::rholang::costacc::production_limits::offered_funded_v6_replay_limits;
use crate::rust::util::rholang::costacc::supply::PurseStack;
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
    pub(crate) async fn settle_replayed_candidate(
        mut self,
        manager: &RuntimeManager,
        processed: &OfferedProcessedDeploy,
        snapshot: &DirectWalletPolicySnapshot<'a, OfferedFundedDeploy>,
        grants: Option<NativeGrantSettlementInput<'_>>,
        production: NativeOfferedProductionLimits,
        inventory_limits: NativePrepaidInventoryLimits,
        receipt_limits: PrepaidReceiptLimits,
        budget: &rholang::rust::interpreter::host_work::HostWorkBudget,
    ) -> Result<NativeReplayedSettlementResult, CasperError> {
        let limits = production.family;
        let funding_root = self.preflight.funding_root;
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
        if settlement_root != evidence.settlement_runtime_root
            || snapshot.wallets().pre_state_root() != funding_root
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
            self.preflight.phlo_limit,
        )
        .map_err(invalid)?;
        let controls = offered_controls.controls();
        adopted.bind_execution_contract(controls, &schedule)?;
        let measured = adopted
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
                selected_terms.bytes(),
                limits.measured.acquisition,
                budget,
            )
            .map_err(invalid)?;
        let original_stacks = manager.read_measured_prepaid_stacks(
            funding_root,
            &demand,
            production.stack_capture,
            budget,
        )?;
        let captured_stacks = manager.capture_prepaid_stacks(
            funding_root,
            &original_stacks,
            &adopted,
            production.stack_capture,
            budget,
        )?;
        let inventory = captured_stacks.resource_inventory(inventory_limits, budget)?;
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
            selected_terms.bytes(),
            schedule.descriptor().classes.len(),
            limits.measured.settlement.matching.execution,
            budget,
        )?;
        let retained = derive_retained_amounts(
            &permissions,
            selected_terms.bytes(),
            &born,
            limits.retained.funding.cells,
            budget,
        )?;
        check_retained_origin(
            &permissions,
            selected_terms.bytes(),
            &retained,
            &born,
            limits.retained.funding.cells,
            budget,
        )?;
        let selection =
            inventory.select_measured_demand(&demand, limits.measured.demand, budget)?;
        let binding = inventory.bind_measured_demand(
            NativePrepaidDemandInput {
                expected_root: funding_root,
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
        if observed.fresh_usage() != evidence.fresh_phlo
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
            .union(classify_errors(&self.evaluation.errors, Some(budget)).map_err(invalid)?);
        if !failures.permits_retained_charge() {
            return Err(invalid("offered replay has a non-user economic failure"));
        }
        let user_failure = failures.contains(PhloFailure::User);
        if user_failure != (evidence.failure_class == NativeCostFailureClass::UserFailure) {
            return Err(invalid("offered replay failure differs from commitment"));
        }
        let user_failures = [PhloFailure::User];
        let outcome = PhloOutcome::Accepted(if user_failure { &user_failures } else { &[] });
        let obligations =
            project_phlo_obligations(observed, outcome, limits.policy.funding.obligations)
                .map_err(invalid)?;
        let positions = derive_birth_positions(&obligations, &born, budget)?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            born.len()
                .checked_mul(size_of::<RetainedBirthFunding<'_>>())
                .ok_or_else(|| invalid("offered replay birth claim allocation overflows"))?,
        )?;
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
        check_birth_positions(&obligations, &derived_claims, budget)?;
        let family_selection = select_rooted_native_family(
            snapshot,
            &obligations,
            &self.evaluation.authority_events,
            &self.evaluation.authority_byte_events,
            limits,
            budget,
        )?;
        let cases = [PhloFundingCase {
            obligations: &obligations,
            eligible: &family_selection.eligible,
            assignment: &family_selection.selected.assignments()[0],
        }];
        let family = check_phlo_funding_family(
            &family_selection.sources,
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
                &adopted,
                budget,
            )
            .map_err(invalid)?
            .ok_or_else(|| invalid("replayed funding family has no valid cursor policy"))?;
        let checked = policy
            .capture_settlement(
                observed,
                outcome,
                limits.measured.settlement.matching,
                limits.measured.settlement.capture,
                budget,
            )
            .map(|checked| checked.bind_execution_root(settlement_root))
            .map_err(invalid)?;
        let births = self
            .runtime
            .capture_retained_births(&checked, &adopted, &derived_claims, limits.retained, budget)
            .await?;
        let records = births.prepare_cell_records(production.retained_record, budget)?;
        let receipt_count = original_stacks
            .len()
            .checked_add(self.evaluation.authority_stack_births.len())
            .ok_or_else(|| invalid("offered replay receipt key count overflows"))?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            receipt_count
                .checked_mul(size_of::<[u8; 32]>())
                .ok_or_else(|| invalid("offered replay receipt key allocation overflows"))?,
        )?;
        let mut receipt_ids = Vec::new();
        receipt_ids
            .try_reserve_exact(receipt_count)
            .map_err(|_| invalid("offered replay receipt key allocation failed"))?;
        for stack in &original_stacks {
            reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
            receipt_ids.push(PrepaidReceiptBucket::key_for_source(&stack.source_hash));
        }
        for birth in &self.evaluation.authority_stack_births {
            reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
            receipt_ids.push(PrepaidReceiptBucket::key_for_source(&birth.produce_hash));
        }
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            receipt_ids
                .len()
                .checked_mul(1 + receipt_ids.len().checked_ilog2().unwrap_or(0) as usize)
                .ok_or_else(|| invalid("offered replay receipt sorting work overflows"))?,
        )?;
        receipt_ids.sort_unstable();
        receipt_ids.dedup();
        let receipt_snapshot =
            manager.capture_prepaid_receipts(funding_root, &receipt_ids, receipt_limits, budget)?;
        let prepared_receipts = records.prepare_insertions(
            &receipt_snapshot,
            production.receipt_bucket,
            production.retained_settlement.receipts,
            budget,
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
            budget,
        )?;
        permit.verify_funding_case(&funding_case)?;
        let (wallet_events, receipt_events) = permit.committed_settlement_segments()?;
        let trace_limits = offered_funded_v6_replay_limits().trace;
        let wallet_events = checked_replay_events(
            wallet_events,
            trace_limits,
            offered_funded_v6_limits().deploy_log_bytes,
            budget,
        )?;
        let receipt_events = checked_replay_events(
            receipt_events,
            trace_limits,
            offered_funded_v6_limits().deploy_log_bytes,
            budget,
        )?;
        let mut wallet_log = Vec::new();
        wallet_log
            .try_reserve_exact(wallet_events.len())
            .map_err(|_| invalid("offered wallet replay log allocation failed"))?;
        wallet_log.extend(wallet_events.iter().cloned());
        let mut replay_runtime = manager
            .spawn_offered_replay_runtime(offered_grant_issue_call_limits(), budget.clone())
            .await;
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
                budget,
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
                    funding_root,
                    wallet_root,
                    production.retained_settlement,
                    budget,
                )
                .await?;
            let count = receipts.prepaid_changes.len();
            reserve_work(
                budget,
                HostWorkDimension::SearchStateBytes,
                count
                    .checked_mul(size_of::<PrepaidReceiptChange<'_>>())
                    .ok_or_else(|| invalid("offered replay prepaid evidence size overflows"))?,
            )?;
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
                &selection,
                records.records(),
                &changes,
                offered_funded_v6_limits().prepaid_delta,
                budget,
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
            reserve_work(
                budget,
                HostWorkDimension::SearchStateBytes,
                receipts
                    .log
                    .len()
                    .checked_mul(size_of::<CasperEvent>())
                    .ok_or_else(|| invalid("offered replay receipt log size overflows"))?,
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
            let mut mergeable = if user_failure {
                HashMap::new()
            } else {
                std::mem::take(&mut self.evaluation.mergeable)
            };
            let wallet_mergeable = std::mem::take(&mut wallet.mergeable);
            reserve_work(
                budget,
                HostWorkDimension::SearchStateBytes,
                wallet_mergeable
                    .len()
                    .checked_mul(size_of::<(Par, MergeType)>() + 64)
                    .ok_or_else(|| invalid("offered replay mergeable map size overflows"))?,
            )?;
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
        budget: &rholang::rust::interpreter::host_work::HostWorkBudget,
    ) -> Result<NativeCapturedSettlement<'a>, CasperError> {
        self.attempt
            .capture_measured_settlement(input, limits, budget)
    }

    pub async fn read_measured_prepaid_stacks(
        &self,
        manager: &RuntimeManager,
        schedule: &PhloScheduleBinding<'_>,
        terms: &[u8],
        limits: NativeAttemptSettlementLimits,
        capture: PrepaidStackCaptureLimits,
        budget: &rholang::rust::interpreter::host_work::HostWorkBudget,
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
        let regions = measured
            .region_demands(limits.regions, budget)
            .map_err(invalid)?;
        let located = regions
            .locate_purses(limits.purses, budget)
            .map_err(invalid)?;
        let demand = located
            .prepare_acquisition_demand(schedule, terms, limits.acquisition, budget)
            .map_err(invalid)?;
        manager.read_measured_prepaid_stacks(
            self.attempt.policy.snapshot().wallets().pre_state_root(),
            &demand,
            capture,
            budget,
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
        budget: &rholang::rust::interpreter::host_work::HostWorkBudget,
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
            budget,
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
                budget,
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
        budget: &rholang::rust::interpreter::host_work::HostWorkBudget,
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
                    budget,
                )
                .await?;
            let count = receipts.prepaid_changes.len();
            let bytes = count
                .checked_mul(size_of::<PrepaidReceiptChange<'_>>())
                .ok_or_else(|| invalid("replayed prepaid evidence size overflows"))?;
            budget
                .reserve(
                    HostWorkDimension::SearchStateBytes,
                    HostWorkUnits::new(
                        u64::try_from(bytes)
                            .map_err(|_| invalid("replayed prepaid evidence work overflows"))?,
                    ),
                )
                .map_err(invalid)?;
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
                budget,
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
            let bytes = receipts
                .log
                .len()
                .checked_mul(size_of::<CasperEvent>())
                .ok_or_else(|| invalid("offered replay settlement log allocation overflows"))?;
            budget
                .reserve(
                    HostWorkDimension::SearchStateBytes,
                    HostWorkUnits::new(
                        u64::try_from(bytes)
                            .map_err(|_| invalid("offered replay settlement log work overflows"))?,
                    ),
                )
                .map_err(invalid)?;
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

fn reserve_work(
    budget: &rholang::rust::interpreter::host_work::HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), CasperError> {
    budget
        .reserve(
            dimension,
            HostWorkUnits::new(
                u64::try_from(amount).map_err(|_| invalid("offered replay work overflows"))?,
            ),
        )
        .map(|_| ())
        .map_err(invalid)
}

fn reserve_offered_evidence_decode(
    processed: &OfferedProcessedDeploy,
    limits: PhloWireLimits,
    budget: &HostWorkBudget,
) -> Result<(), CasperError> {
    let bytes = processed.native_cost_evidence_bytes();
    reserve_work(budget, HostWorkDimension::VerificationBytes, bytes.len())?;
    reserve_work(
        budget,
        HostWorkDimension::VerificationOperations,
        bytes.len(),
    )?;
    let evidence = NativeCostEvidenceV1::decode(bytes, limits).map_err(invalid)?;
    let owned_sections = evidence
        .funding_case
        .len()
        .checked_add(evidence.prepaid_delta.len())
        .and_then(|n| n.checked_add(evidence.wallet_settlement.len()))
        .ok_or_else(|| invalid("committed native evidence section size overflows"))?;
    reserve_work(
        budget,
        HostWorkDimension::SearchStateBytes,
        owned_sections
            .checked_mul(16)
            .ok_or_else(|| invalid("committed native evidence allocation overflows"))?,
    )
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
    host: &rholang::rust::interpreter::host_work::HostWorkBudget,
) -> Result<Arc<[Event]>, CasperError> {
    if events.len() > limits.events {
        return Err(invalid("offered replay event count exceeds protocol limit"));
    }
    let item_cap = limits
        .source_entries
        .checked_add(limits.telemetry_items)
        .and_then(|count| count.checked_add(limits.events))
        .ok_or_else(|| invalid("offered replay event item limit overflows"))?;
    host.reserve(
        HostWorkDimension::VerificationOperations,
        HostWorkUnits::new(
            u64::try_from(events.len())
                .map_err(|_| invalid("offered replay event work overflows"))?,
        ),
    )
    .map_err(invalid)?;
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
    host.reserve(
        HostWorkDimension::VerificationOperations,
        HostWorkUnits::new(
            u64::try_from(top_items).map_err(|_| invalid("offered replay event work overflows"))?,
        ),
    )
    .map_err(invalid)?;
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
    host.reserve(
        HostWorkDimension::VerificationOperations,
        HostWorkUnits::new(
            u64::try_from(items - top_items)
                .map_err(|_| invalid("offered replay event work overflows"))?,
        ),
    )
    .map_err(invalid)?;
    let bytes = events.iter().try_fold(0usize, |sum, event| {
        sum.checked_add(event_bytes(event)?)
            .ok_or_else(|| invalid("offered replay event byte size overflows"))
    })?;
    if bytes > bytes_cap {
        return Err(invalid("offered replay events exceed protocol byte limit"));
    }
    let allocation = bytes
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(events.len().checked_mul(size_of::<Event>() * 4)?))
        .ok_or_else(|| invalid("offered replay event allocation size overflows"))?;
    for (dimension, amount) in [
        (HostWorkDimension::SearchStateBytes, allocation),
        (HostWorkDimension::VerificationBytes, bytes),
    ] {
        host.reserve(
            dimension,
            HostWorkUnits::new(
                u64::try_from(amount)
                    .map_err(|_| invalid("offered replay event work overflows"))?,
            ),
        )
        .map_err(invalid)?;
    }
    let mut replayed = Vec::new();
    replayed
        .try_reserve_exact(events.len())
        .map_err(|_| invalid("offered replay event allocation failed"))?;
    replayed.extend(events.iter().map(event_converter::to_rspace_event));
    Ok(replayed.into())
}

fn native_user_event_count(
    operations: &[NativeOperationRecord],
    maximum: usize,
    host: &rholang::rust::interpreter::host_work::HostWorkBudget,
) -> Result<usize, CasperError> {
    host.reserve(
        HostWorkDimension::VerificationOperations,
        HostWorkUnits::new(
            u64::try_from(operations.len())
                .map_err(|_| invalid("native user event count overflows"))?,
        ),
    )
    .map_err(invalid)?;
    operations.iter().try_fold(0usize, |count, operation| {
        let width = match operation.completion {
            RSpaceOperationCompletion::Rejected => 0,
            RSpaceOperationCompletion::Stored => 1,
            RSpaceOperationCompletion::Matched => 2,
        };
        count
            .checked_add(width)
            .filter(|count| *count <= maximum)
            .ok_or_else(|| invalid("native user event count exceeds replay limit"))
    })
}

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
        let host = context.host_work.clone();
        let evidence_bytes = processed.native_cost_evidence_bytes().len();
        if evidence_bytes > wire_limits.total_bytes {
            return Err(invalid("committed native evidence exceeds protocol limit"));
        }
        reserve_offered_evidence_decode(processed, wire_limits, &host)?;
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
            decode_committed_runtime_recording(&evidence, recording_limits, &host)?;
        let user_events = native_user_event_count(&operations, replay_limits.trace.events, &host)?;
        let event_prefix = processed
            .deploy_log()
            .get(..user_events)
            .ok_or_else(|| invalid("committed deploy log omits native user events"))?;
        let events = checked_replay_events(
            event_prefix,
            replay_limits.trace,
            wire_limits.total_bytes,
            &host,
        )?;
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
            .union(classify_errors(&attempt.evaluation.errors, Some(&host)).map_err(invalid)?);
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
                    &host,
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
            &host,
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

    pub(crate) async fn replay_native_offered_witness<'preflight, 'a>(
        &self,
        prepared: &'preflight PreparedOfferedCandidate<'a>,
        processed: &OfferedProcessedDeploy,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        context: NativeFundedExecutionContext,
        candidate_limits: OfferedCandidateLimits,
        recording_limits: NativeRecordingWireLimits,
        replay_limits: NativeFundedReplayLimits,
    ) -> Result<NativeOfferedAttempt<'preflight, 'a>, CasperError> {
        let host = context.host_work.clone();
        let protocol = offered_funded_v6_limits();
        let wire_limits = protocol.evidence;
        let evidence_bytes = processed.native_cost_evidence_bytes().len();
        let log_events = processed.deploy_log().len();
        if evidence_bytes > wire_limits.total_bytes || log_events > protocol.deploy_log_events {
            return Err(invalid("committed native evidence exceeds protocol limit"));
        }
        reserve_offered_evidence_decode(processed, wire_limits, &host)?;
        reserve_work(&host, HostWorkDimension::VerificationOperations, log_events)?;
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
            prepared.phlo_limit,
        )
        .map_err(invalid)?;
        let contract = adopted.bind_execution_contract(checked.controls(), schedule)?;
        let config = NativeRuntimeConfig::new(contract, context.trace, context.host_work.clone());
        let (recording, operations) =
            decode_committed_runtime_recording(&evidence, recording_limits, &host)?;
        let user_events = native_user_event_count(&operations, replay_limits.trace.events, &host)?;
        let user_prefix = processed
            .deploy_log()
            .get(..user_events)
            .ok_or_else(|| invalid("committed deploy log omits native user events"))?;
        let events = checked_replay_events(
            user_prefix,
            replay_limits.trace,
            wire_limits.total_bytes,
            &host,
        )?;
        let identity = prepared.envelope_identity;
        let trace = adopted
            .bind_execution_contract(checked.controls(), schedule)?
            .check_operation_journal(
                identity,
                &recording,
                operations,
                replay_limits.journal,
                &host,
            )
            .map_err(invalid)?
            .bind_trace(events.clone(), replay_limits.trace, &host)
            .map_err(invalid)?;
        let cost = RuntimeBudget::new(Cost::unsafe_max());
        cost.set_deploy_id_funded(identity, funding_sig(signed));
        cost.reset_for_native_execution(config)?;
        let before = Blake2b256Hash::from_bytes(prepared.execution_root.to_vec());
        let history = Arc::new(self.history_repo.reset(&before).map_err(invalid)?);
        super::check_execution_root(&before.bytes(), &history.root().bytes())?;
        let parsed = InterpreterImpl::parse_source(
            &processed.envelope().body().term,
            normalizer_env_from_envelope(processed.envelope()),
            Some(&host),
        )?;
        let mut offered_definitions = vec![grant_issue_definition(
            offered_grant_issue_call_limits(),
            host.clone(),
        )];
        let mut replay = create_native_replay_env(
            trace,
            history,
            Arc::new(Box::new(Matcher)),
            host.clone(),
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
        *replay.deploy_data.write().await = DeployData::from_envelope(processed.envelope());
        let evaluation = replay
            .evaluate(parsed, Tools::user_envelope_rng(processed.envelope()))
            .await?;
        let failures = evaluation
            .economic_failures
            .union(classify_errors(&evaluation.errors, Some(&host)).map_err(invalid)?);
        if !failures.permits_retained_charge() {
            return Err(invalid(
                "native offered witness has a non-user economic failure",
            ));
        }
        let user_failure = failures.contains(PhloFailure::User);
        if evaluation.native_phlo_usage != Some(evidence.phlo_used)
            || user_failure != (evidence.failure_class == NativeCostFailureClass::UserFailure)
        {
            return Err(invalid(
                "native offered witness differs from committed usage or failure",
            ));
        }
        let exported = replay.export().await?;
        let root = settlement_root(before, exported.root().clone(), user_failure);
        let mut runtime = RuntimeOps::new(
            self.spawn_offered_runtime(offered_grant_issue_call_limits(), host.clone())
                .await,
        );
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
                    &host,
                )
                .await?
        };
        let grant_log = runtime.runtime.take_event_log().await;
        let grant_end = user_events
            .checked_add(grant_log.len())
            .ok_or_else(|| invalid("offered witness grant boundary overflows"))?;
        let grant_bytes = grant_log
            .len()
            .checked_mul(size_of::<CasperEvent>())
            .ok_or_else(|| invalid("offered witness grant log allocation overflows"))?;
        for (dimension, amount) in [
            (HostWorkDimension::SearchStateBytes, grant_bytes),
            (HostWorkDimension::VerificationOperations, grant_log.len()),
        ] {
            host.reserve(
                dimension,
                HostWorkUnits::new(
                    u64::try_from(amount)
                        .map_err(|_| invalid("offered witness grant log work overflows"))?,
                ),
            )
            .map_err(invalid)?;
        }
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
        let host = context.host_work.clone();
        let config = policy.prepare_execution(
            envelope,
            adopted,
            schedule,
            context.trace,
            context.host_work,
        )?;
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
        let trace = adopted
            .bind_execution_contract(controls, schedule)?
            .check_operation_journal(
                identity,
                input.recording,
                input.operations.clone(),
                limits.journal,
                &host,
            )
            .map_err(invalid)?
            .bind_trace(input.events.clone(), limits.trace, &host)
            .map_err(invalid)?;
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(invalid("native replay requires an offered funded envelope"));
        };
        let cost = RuntimeBudget::new(Cost::unsafe_max());
        cost.set_deploy_id_funded(identity, funding_sig(signed));
        cost.reset_for_native_execution(config)?;
        let before =
            Blake2b256Hash::from_bytes(policy.snapshot().wallets().pre_state_root().to_vec());
        let history = Arc::new(self.history_repo.reset(&before).map_err(invalid)?);
        super::check_execution_root(&before.bytes(), &history.root().bytes())?;
        let parsed = InterpreterImpl::parse_source(
            &envelope.body().term,
            normalizer_env_from_envelope(envelope),
            Some(&host),
        )?;
        let mut offered_definitions = vec![grant_issue_definition(
            offered_grant_issue_call_limits(),
            host.clone(),
        )];
        let settlement_host = host.clone();
        let mut replay = create_native_replay_env(
            trace,
            history,
            Arc::new(Box::new(Matcher)),
            host,
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
            self.spawn_offered_runtime(offered_grant_issue_call_limits(), settlement_host)
                .await,
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

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::ProduceEvent;
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use proptest::prelude::*;
    use rholang::rust::interpreter::host_work::HostWorkBudget;

    use super::*;

    #[test]
    fn replay_event_item_exhaustion_rejects_before_payload_copy() {
        let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000)));
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
        assert!(checked_replay_events(&events, limits, 100, &host).is_err());
        assert_eq!(host.usage(HostWorkDimension::SearchStateBytes).get(), 0);
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
