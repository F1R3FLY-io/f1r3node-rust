use std::collections::HashMap;
use std::sync::Arc;

// Changed by G3 (DR-135, S7): the demand filter names the region type.
// use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::Event as CasperEvent;
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::native_cost_evidence::NativeCostFailureClass;
use models::rust::native_wallet_receipt::NativeWalletReceiptLimits;
use models::rust::normalizer_env::normalizer_env_from_envelope;
use models::rust::phlo_grant_creation::VerifiedGrantCreationV1;
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
// Changed by dedup (task 11415, D5): the halves of the settlement pipeline
// name these types.
// use rholang::rust::interpreter::accounting::authority::AuthorityStackBirth;
use rholang::rust::interpreter::accounting::authority::AuthorityStackBirth;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::economic_failure::classify_errors;
// Added by G3 (DR-135, S7): the binding of a recorded owner table (R3, R11).
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
// Changed by dedup (task 11415, D5): the halves of the settlement pipeline
// name these types.
// use rholang::rust::interpreter::accounting::phlo_execution::{
//     check_counted_phlo_execution, check_phlo_funding_family, project_phlo_obligations, PhloFailure,
//     PhloFundingCase, PhloFundingIntentBinding, PhloOutcome, RetainedBirthFunding,
// };
use rholang::rust::interpreter::accounting::phlo_execution::PhloFailure;
use rholang::rust::interpreter::accounting::{
    funding_sig, NativeBudgetRecording, NativeOperationJournalLimits, NativeOperationRecord,
    NativeOperationTraceLimits, NativeRecordingWireLimits, RuntimeBudget,
};
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::interpreter::InterpreterImpl;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::{create_native_replay_env, RhoRuntime};
use rholang::rust::interpreter::system_processes::DeployData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
// Moved by DR-115 with native_user_event_count to offered_evidence.rs.
// use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
use rspace_plus_plus::rspace::trace::event::Event;
use tokio::sync::RwLock;

// Added by dedup (task 11415, D5): the funding half of the settlement
// pipeline names the rooted selection.
// Changed by DR-134 (DR-65 item 5): the later holds of a member.
// use super::held_capacity::{capture_debits, GuardMember, GuardRole};
// Changed by G3 (DR-135, S7): the owner rows of a member (R10).
// use super::settlement::{
//     check_metered_usage, NativeAttemptSettlementInput, NativeAttemptSettlementLimits,
//     NativeCapturedSettlement, NativeFundedSettlement, NativeGrantIssuanceInput,
//     NativeGrantSettlementInput,
// };
use super::settlement::{
    NativeAttemptSettlementInput, NativeAttemptSettlementLimits, NativeCapturedSettlement,
    NativeFundedSettlement, NativeGrantIssuanceInput, NativeGrantSettlementInput,
};
// Changed by dedup (task 11415, D5): the funding half of the settlement
// pipeline names the family limits.
// use super::{
//     select_rooted_native_family, CheckedDirectWalletPolicy, NativeFundedAttempt,
//     NativeFundedExecutionContext, NativeOfferedAttempt, NativeOfferedProductionLimits,
//     OfferedGateInput,
// };
use super::{CheckedDirectWalletPolicy, NativeFundedAttempt, NativeFundedExecutionContext};
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::event_converter;
use crate::rust::util::rholang::acceptance::CandidateReplayPermit;
// Changed by dedup (task 11415, D5): the funding half of the settlement
// pipeline returns the captured settlement.
// use crate::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletPolicySnapshot;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
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
    NativePrepaidConsumption, NativeRetainedReceiptRecords, NativeRetainedSettlementLimits,
    PrepaidStackCaptureLimits, PreparedNativeRetainedReceipts,
};
use crate::rust::util::rholang::costacc::supply::PurseStack;
// Added by G3 (DR-135, S7): the purse locator of the owner binding (R2).
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
