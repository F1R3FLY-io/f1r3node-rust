//! Added by DR-134 (DR-65, gap G2): several offered deploys in one block.
//!
//! A batch is a list of offered envelopes in canonical order (DR-65 item 1).
//! Play and replay take the same steps:
//!
//! 1. The decision pass at the block pre-state R0 checks every member alone
//!    (DR-64), before the first runtime entry (GateBeforeExecute). It groups
//!    the members that share a purse or a grant and rejects every group whose
//!    holds do not fit its supply at R0, all or none (DR-65 item 4).
//! 2. Member i funds from R0 and executes from R_{i-1}, the settled root of
//!    member i-1 (R0 for the first). Its settlement reads the grants and the
//!    wallets at R_{i-1}, with each source capacity capped by its later holds,
//!    and its held-capacity guard checks those later holds (DR-65 item 5,
//!    option B).
//! 3. The system deploys run from R_n, the settled root of the last member.
//!
//! Play also applies dev's play budget between members. When the budget stops
//! the batch, the shipped prefix runs again from R0 if its results could
//! differ from what validators recompute over that prefix (O-10).

use std::collections::{BTreeMap, HashMap};

use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{ProcessedSystemDeploy, ProcessedUserDeploy};
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::deploy_id::DeployLookupId;
use models::rust::phlo_intent::{PhloConversionCompositionV2, PhloFundingIntentV2Limits};
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::validator::Validator;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsEndVal;

use super::funding_groups::{
    check_canonical_order, decide_groups, later_holds, rerun_after_truncation, BatchCandidate,
    CanonicalKey, FundingGroupError, GrantDraw, GroupDecision, RejectedGroup,
};
use super::held_capacity::LaterHolds;
use super::offered_block::OfferedMemberContext;
use super::{
    CertifiedOfferedDraft, NativeFundedExecutionContext, NativeGrantSettlementInput, OfferedGate,
    OfferedGateInput,
};
use crate::rust::errors::{CasperError, OfferedCandidateRejection};
use crate::rust::rholang::runtime::{offered_candidate_rejection, RuntimeOps};
use crate::rust::util::rholang::acceptance::{prepare_offered_candidate, OfferedCandidateLimits};
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
#[cfg(any(test, feature = "test-utils"))]
use crate::rust::util::rholang::costacc::offered_acceptance::{OfferedCheckRole, OfferedUsageKind};
use crate::rust::util::rholang::costacc::offered_grants::{
    offered_grant_issue_call_limits, offered_grant_transition_limits,
};
use crate::rust::util::rholang::costacc::production_limits::{
    offered_funded_v6_prepaid_inventory_limits, offered_funded_v6_prepaid_receipt_limits,
    offered_funded_v6_production_limits, offered_funded_v6_trace_limits,
};
use crate::rust::util::rholang::costacc::vault_payer::VaultPayer;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum;

/// The path of a decision pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AcceptanceRole {
    Play,
    Replay,
}

/// The decision over a batch at R0: the candidates, the check of each, the
/// payer of every purse of the batch, and the group decision.
pub(crate) struct OfferedBatchPlan {
    candidates: Vec<BatchCandidate>,
    gates: Vec<OfferedGate>,
    payers: BTreeMap<[u8; 32], VaultPayer>,
    decision: GroupDecision,
}

/// Why a batch is rejected before its first runtime entry.
#[derive(Debug, thiserror::Error)]
pub(crate) enum OfferedBatchRejection {
    /// A member fails its own check (DR-64).
    #[error("{error}")]
    Alone { index: usize, error: CasperError },
    /// One or more funding groups do not fit their supply (DR-65 item 4).
    #[error("{}", .0.first().map(|group| group.cause.to_string()).unwrap_or_default())]
    Groups(Vec<RejectedGroup>),
    /// A malformed decision input.
    #[error(transparent)]
    Decision(#[from] FundingGroupError),
}

impl OfferedBatchRejection {
    /// The rejection that play reports to the proposer. A member that fails
    /// alone is rejected as before G2. A rejected group names its first member
    /// and lists the members of every rejected group, which the proposer
    /// quarantines together (O-5).
    fn into_play_error(self, envelopes: &[&DeployEnvelope], start_hash: &StateHash) -> CasperError {
        match self {
            OfferedBatchRejection::Alone { index, error } => match envelopes.get(index) {
                Some(envelope) => offered_candidate_rejection(envelope, start_hash, error),
                None => error,
            },
            OfferedBatchRejection::Groups(groups) => {
                let reason = groups
                    .first()
                    .map(|group| group.cause.to_string())
                    .unwrap_or_default();
                let mut ids = Vec::new();
                for group in &groups {
                    for member in &group.members {
                        if let Some(DeployLookupId::V6(id)) =
                            envelopes.get(*member).map(|envelope| envelope.identity())
                        {
                            ids.push(*id);
                        }
                    }
                }
                match ids.split_first() {
                    Some((first, rest)) => {
                        CasperError::OfferedCandidateRejected(OfferedCandidateRejection {
                            deploy_id: *first,
                            pre_state_root: start_hash.clone(),
                            reason,
                            group: rest.to_vec(),
                        })
                    }
                    None => CasperError::RuntimeError(reason),
                }
            }
            OfferedBatchRejection::Decision(error) => CasperError::RuntimeError(error.to_string()),
        }
    }

    /// The rejection that replay reports. Any rejection fails the block
    /// (DR-65 item 6), with the text of the check.
    fn into_replay_error(self) -> CasperError {
        match self {
            OfferedBatchRejection::Alone { error, .. } => error,
            rejection => CasperError::RuntimeError(rejection.to_string()),
        }
    }
}

impl OfferedBatchPlan {
    /// The check of the member at `position` at R0.
    fn gate(&self, position: usize) -> Result<OfferedGate, CasperError> {
        self.gates.get(position).cloned().ok_or_else(|| {
            CasperError::RuntimeError("an offered batch member has no check".to_string())
        })
    }

    /// The later holds of the first `count` members, which execute in batch
    /// order. A prefix of an accepted batch is accepted, so its decision does
    /// not change.
    fn later(&self, count: usize) -> Result<Vec<LaterHolds>, CasperError> {
        let members: Vec<usize> = (0..count).collect();
        let prefix: Vec<BatchCandidate> =
            self.candidates[..count.min(self.candidates.len())].to_vec();
        let decision =
            decide_groups(&prefix).map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        if !decision.rejected.is_empty() {
            return Err(CasperError::RuntimeError(
                FundingGroupError::SubsetRejected.to_string(),
            ));
        }
        later_holds(&self.candidates, &members)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?
            .into_iter()
            .map(|holds| {
                LaterHolds::new(holds, &self.payers)
                    .map_err(|error| CasperError::RuntimeError(error.to_string()))
            })
            .collect()
    }

    /// O-10: whether the shipped prefix must run again from R0.
    fn rerun_after_truncation(
        &self,
        shipped: usize,
        guard_rolled_back: &[bool],
        // Added by G3 (DR-135, S8): the owner purses of each shipped member.
        shipped_owner_purses: &[Vec<[u8; 32]>],
    ) -> Result<bool, CasperError> {
        // Changed by G3 (DR-135, S8): the rule also reads the holds of the
        // candidates and the owner purses of the shipped members.
        // rerun_after_truncation(&self.decision, shipped, guard_rolled_back)
        rerun_after_truncation(
            &self.decision,
            shipped,
            guard_rolled_back,
            &self.candidates,
            shipped_owner_purses,
        )
        .map_err(|error| CasperError::RuntimeError(error.to_string()))
    }
}

/// The canonical order key of an envelope (DR-65 item 1).
fn canonical_key(envelope: &DeployEnvelope) -> Result<CanonicalKey, CasperError> {
    let identity: [u8; 32] = envelope.identity().as_bytes().try_into().map_err(|_| {
        CasperError::RuntimeError("an offered batch member identity is not 32 bytes".to_string())
    })?;
    let body = envelope.body();
    Ok(CanonicalKey {
        valid_after_block_number: body.valid_after_block_number,
        time_stamp: body.time_stamp,
        identity,
    })
}

/// O-9: a batch must be in strictly increasing canonical order. A repeated
/// member fails as well.
fn check_batch_order(envelopes: &[&DeployEnvelope]) -> Result<(), CasperError> {
    let keys = envelopes
        .iter()
        .map(|envelope| canonical_key(envelope))
        .collect::<Result<Vec<_>, _>>()?;
    check_canonical_order(&keys).map_err(|error| CasperError::RuntimeError(error.to_string()))
}

/// The offered batch of a play request: every pending candidate is an offered
/// envelope. An empty or mixed request is not an offered batch.
pub(crate) fn offered_envelopes(
    terms: &[block_storage::rust::deploy::key_value_deploy_storage::PendingDeployCandidate],
) -> Option<Vec<&DeployEnvelope>> {
    use block_storage::rust::deploy::key_value_deploy_storage::PendingDeployCandidate;
    let mut envelopes = Vec::with_capacity(terms.len());
    for term in terms {
        match term {
            PendingDeployCandidate::Envelope(envelope) => envelopes.push(envelope),
            PendingDeployCandidate::Legacy(_) => return None,
        }
    }
    match envelopes.is_empty() {
        true => None,
        false => Some(envelopes),
    }
}

/// The candidate limits of every offered member, as the single-offer path
/// used them.
fn offered_member_limits() -> OfferedCandidateLimits {
    let base = models::rust::cost_protocol_limits::offered_funded_v6_limits()
        .envelope
        .payload
        .funding;
    OfferedCandidateLimits {
        funding: PhloFundingIntentV2Limits {
            wire: base.wire,
            base,
            grant_uses: base.wire.total_bytes / 8,
            grant_id_bytes: base.wire.field_bytes,
            quote_evidence_bytes: base.wire.field_bytes,
        },
        max_phlo_limit: i64::MAX as u64,
    }
}

/// The result of the play of one member.
pub(crate) struct PlayedOfferedMember {
    pub(crate) processed: OfferedProcessedDeploy,
    pub(crate) final_root: [u8; 32],
    pub(crate) mergeable: NumberChannelsEndVal,
    pub(crate) guard_rolled_back: bool,
    /// Added by G3 (DR-135, S8): the owner purses that the owner table of the
    /// member lists, in ascending custody order (truncation condition (c)).
    pub(crate) owner_purses: Vec<[u8; 32]>,
}

impl RuntimeManager {
    /// The check of one member at R0, before any runtime entry: the steps of
    /// the single-offer play up to its check (preparation, conversion, grant
    /// proofs, wallet snapshot, selected terms), then the check itself with
    /// the reducer of `runtime`, which is never entered.
    #[allow(clippy::too_many_arguments)]
    async fn check_offered_member_at_root(
        &self,
        envelope: &DeployEnvelope,
        start_hash: &StateHash,
        root: [u8; 32],
        block_time: i64,
        adopted: &AdoptedResourcePolicy,
        runtime: &RuntimeOps,
        role: AcceptanceRole,
    ) -> Result<(BatchCandidate, OfferedGate, BTreeMap<[u8; 32], VaultPayer>), CasperError> {
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(CasperError::RuntimeError(
                "offered proposal has an unexpected envelope format".to_string(),
            ));
        };
        let limits = offered_member_limits();
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
        let grant_context = match prepared.intent.grant_uses.is_empty() {
            true => None,
            false => {
                let authenticated_time = u64::try_from(block_time).map_err(|_| {
                    CasperError::RuntimeError(
                        "offered grant use requires nonnegative block time".to_string(),
                    )
                })?;
                let grant_snapshot =
                    self.capture_offered_grants_from_signed(root, signed, grant_limits)?;
                let verified = grant_snapshot.verified_source_payers(
                    root,
                    signed,
                    authenticated_time,
                    grant_limits,
                )?;
                Some(verified)
            }
        };
        let snapshot = self
            .read_direct_offered_wallet_snapshot(envelope, start_hash, grant_context.as_ref())
            .await?;
        if snapshot.wallets().pre_state_root() != root {
            return Err(CasperError::RuntimeError(
                "offered wallet snapshot belongs to another original root".to_string(),
            ));
        }
        let selected = prepared.selected_terms(adopted)?;
        let schedule = PhloScheduleBinding::new(selected.schedule(), PhloGenesisPolicy::LIMITS)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let gate = self.check_offered_before_execution(OfferedGateInput {
            envelope,
            prepared: &prepared,
            selected_terms: selected.bytes(),
            schedule: &schedule,
            minimum_price: adopted.genesis().minimum_price(),
            snapshot: &snapshot,
            evaluator: &runtime.runtime.reducer,
            limits: offered_funded_v6_production_limits().family,
        });
        #[cfg(any(test, feature = "test-utils"))]
        self.record_offered_check(
            match role {
                AcceptanceRole::Play => OfferedCheckRole::Play,
                AcceptanceRole::Replay => OfferedCheckRole::Replay,
            },
            envelope,
            &prepared.funding_root,
            gate.as_ref().ok(),
        );
        #[cfg(not(any(test, feature = "test-utils")))]
        let _ = role;
        let gate = gate?;
        let holds = gate.holds().to_vec();
        let mut custodies = Vec::with_capacity(snapshot.wallets().sources().len());
        let mut supply = Vec::with_capacity(snapshot.wallets().sources().len());
        for source in snapshot.wallets().sources() {
            let custody: [u8; 32] = source.source().custody.try_into().map_err(|_| {
                CasperError::RuntimeError("an offered source custody is not 32 bytes".to_string())
            })?;
            custodies.push(custody);
            supply.push((custody, u128::from(source.source().capacity)));
        }
        let grants = match &grant_context {
            None => Vec::new(),
            Some(verified) => verified
                .grant_ceilings()
                .into_iter()
                .map(|(grant, custody, remaining)| GrantDraw {
                    grant,
                    hold: holds
                        .iter()
                        .find(|(held, _)| *held == custody)
                        .map_or(0, |(_, hold)| *hold),
                    remaining,
                })
                .collect(),
        };
        let payers = snapshot.wallets().authorization().payers().clone();
        let identity: [u8; 32] = prepared.envelope_identity;
        Ok((
            BatchCandidate {
                identity,
                custodies,
                holds,
                supply,
                grants,
            },
            gate,
            payers,
        ))
    }

    /// Added by DR-134 (DR-65 items 2 to 5): the decision pass of a batch at
    /// R0. It checks each member alone, in batch order, and stops at the first
    /// member that fails. It then decides the funding groups. The pass enters
    /// no runtime: one spawned runtime lends its reducer to every check.
    pub(crate) async fn decide_offered_batch(
        &self,
        start_hash: &StateHash,
        envelopes: &[&DeployEnvelope],
        block_time: i64,
        adopted: &AdoptedResourcePolicy,
        role: AcceptanceRole,
    ) -> Result<OfferedBatchPlan, OfferedBatchRejection> {
        let root: [u8; 32] =
            start_hash
                .as_ref()
                .try_into()
                .map_err(|_| OfferedBatchRejection::Alone {
                    index: 0,
                    error: CasperError::RuntimeError(
                        "offered batch requires a 32-byte original root".to_string(),
                    ),
                })?;
        let spawned = match role {
            AcceptanceRole::Play => {
                self.spawn_offered_play_runtime(offered_grant_issue_call_limits())
                    .await
            }
            AcceptanceRole::Replay => {
                self.spawn_offered_runtime(offered_grant_issue_call_limits())
                    .await
            }
        };
        let runtime = RuntimeOps::new(
            spawned.map_err(|error| OfferedBatchRejection::Alone { index: 0, error })?,
        );
        let mut candidates = Vec::with_capacity(envelopes.len());
        let mut gates = Vec::with_capacity(envelopes.len());
        let mut payers = BTreeMap::new();
        for (index, envelope) in envelopes.iter().enumerate() {
            let (candidate, gate, member_payers) = self
                .check_offered_member_at_root(
                    envelope, start_hash, root, block_time, adopted, &runtime, role,
                )
                .await
                .map_err(|error| OfferedBatchRejection::Alone { index, error })?;
            candidates.push(candidate);
            gates.push(gate);
            payers.extend(member_payers);
        }
        let decision = decide_groups(&candidates)?;
        match decision.rejected.is_empty() {
            true => Ok(OfferedBatchPlan {
                candidates,
                gates,
                payers,
                decision,
            }),
            false => Err(OfferedBatchRejection::Groups(decision.rejected)),
        }
    }

    /// Added by DR-134 (DR-65): the play of one member of a batch. It is the
    /// single-offer play, generalized: the member funds from R0 and executes
    /// from `member.execution_root`; its settlement reads the grants and the
    /// wallets at that root, capped by `member.later`; and its self-replay
    /// certifies it from the same context. `gate` is its check at R0.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn play_offered_member(
        &self,
        envelope: &DeployEnvelope,
        start_hash: &StateHash,
        member: OfferedMemberContext<'_>,
        gate: OfferedGate,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
        adopted: &AdoptedResourcePolicy,
    ) -> Result<PlayedOfferedMember, CasperError> {
        let root: [u8; 32] = start_hash.as_ref().try_into().map_err(|_| {
            CasperError::RuntimeError(
                "offered proposal requires a 32-byte original root".to_string(),
            )
        })?;
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(CasperError::RuntimeError(
                "offered proposal has an unexpected envelope format".to_string(),
            ));
        };
        let limits = offered_member_limits();
        let prepared =
            prepare_offered_candidate(signed, adopted, root, member.execution_root, limits)?;
        if !matches!(
            prepared.intent.conversion,
            PhloConversionCompositionV2::NoConversion
        ) {
            return Err(CasperError::RuntimeError(
                "offered conversion requires authenticated production composition".to_string(),
            ));
        }
        let grant_limits = offered_grant_transition_limits();
        let grant_context = match prepared.intent.grant_uses.is_empty() {
            true => None,
            false => {
                let authenticated_time = u64::try_from(block_data.time_stamp).map_err(|_| {
                    CasperError::RuntimeError(
                        "offered grant use requires nonnegative block time".to_string(),
                    )
                })?;
                let grant_snapshot = self.capture_offered_grants_from_signed(
                    member.execution_root,
                    signed,
                    grant_limits,
                )?;
                let verified = grant_snapshot.verified_source_payers(
                    member.execution_root,
                    signed,
                    authenticated_time,
                    grant_limits,
                )?;
                Some((grant_snapshot, verified, authenticated_time))
            }
        };
        let execution_hash = StateHash::copy_from_slice(&member.execution_root);
        let snapshot = self
            .read_direct_offered_wallet_snapshot(
                envelope,
                &execution_hash,
                grant_context.as_ref().map(|(_, verified, _)| verified),
            )
            .await?;
        if snapshot.wallets().pre_state_root() != member.execution_root {
            return Err(CasperError::RuntimeError(
                "offered wallet snapshot belongs to another execution root".to_string(),
            ));
        }
        let snapshot = snapshot.capped_by_later_holds(member.later.holds());
        let selected = prepared.selected_terms(adopted)?;
        let attempt = self
            .evaluate_native_offered(
                &prepared,
                envelope,
                &selected,
                limits,
                gate,
                NativeFundedExecutionContext {
                    block_data: block_data.clone(),
                    invalid_blocks: invalid_blocks.clone(),
                    trace: offered_funded_v6_trace_limits(),
                },
                // Added by G3 (DR-135, S7): the later holds cap the owner
                // budgets of the run (R3).
                member.later,
            )
            .await?;
        // Added by G3 (DR-135, S8): the owner purses of the run, before the
        // producer takes the attempt.
        let owner_purses = attempt.owner_purses();
        let candidate = attempt
            .produce_native_offered_candidate_with_rooted_prepaid(
                self,
                envelope.clone(),
                &snapshot,
                member.later,
                grant_context
                    .as_ref()
                    .map(
                        |(snapshot, verified, authenticated_time)| NativeGrantSettlementInput {
                            snapshot,
                            verified,
                            authenticated_time: *authenticated_time,
                            limits: grant_limits,
                        },
                    ),
                offered_funded_v6_production_limits(),
                offered_funded_v6_prepaid_inventory_limits(),
                offered_funded_v6_prepaid_receipt_limits(),
            )
            .await?;
        let certificate = self
            .certify_offered_member(
                &candidate.processed,
                start_hash,
                member,
                block_data,
                invalid_blocks,
                adopted,
                None,
            )
            .await?;
        let _publication = prepared.authorize_publication_after_replay(
            &candidate.processed,
            candidate.final_root,
            &certificate,
        )?;
        #[cfg(any(test, feature = "test-utils"))]
        self.record_offered_budget_usage(
            OfferedUsageKind::ProducerSelfReplay,
            candidate.processed.identity_bytes(),
            start_hash,
        );
        Ok(PlayedOfferedMember {
            processed: candidate.processed,
            final_root: candidate.final_root,
            mergeable: certificate.into_user_mergeable(),
            guard_rolled_back: candidate.guard_rolled_back,
            // Added by G3 (DR-135, S8).
            owner_purses,
        })
    }

    /// Added by DR-134 (DR-65 items 1, 6 and O-9): the certification of every
    /// member of a shipped batch, as validators replay it. The batch must be in
    /// canonical order. The decision pass at R0 must accept every member, and
    /// each member is certified from the settled root of the member before it.
    /// `loggers` holds the logger of each member for block reports (DR-70).
    pub(crate) async fn certify_offered_batch(
        &self,
        start_hash: &StateHash,
        members: &[&OfferedProcessedDeploy],
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
        adopted: &AdoptedResourcePolicy,
        mut loggers: Vec<Option<rholang::rust::interpreter::rho_runtime::RhoSpaceLogger>>,
    ) -> Result<Vec<CertifiedOfferedDraft>, CasperError> {
        let envelopes: Vec<&DeployEnvelope> =
            members.iter().map(|member| member.envelope()).collect();
        check_batch_order(&envelopes)?;
        let plan = self
            .decide_offered_batch(
                start_hash,
                &envelopes,
                block_data.time_stamp,
                adopted,
                AcceptanceRole::Replay,
            )
            .await
            .map_err(OfferedBatchRejection::into_replay_error)?;
        let later = plan.later(members.len())?;
        let mut execution_root: [u8; 32] = start_hash.as_ref().try_into().map_err(|_| {
            CasperError::RuntimeError("offered replay requires a 32-byte original root".to_string())
        })?;
        let mut certificates = Vec::with_capacity(members.len());
        for (index, member) in members.iter().enumerate() {
            let logger = loggers.get_mut(index).and_then(Option::take);
            let certificate = self
                .certify_offered_member(
                    member,
                    start_hash,
                    OfferedMemberContext {
                        execution_root,
                        later: &later[index],
                    },
                    block_data,
                    invalid_blocks,
                    adopted,
                    logger,
                )
                .await?;
            execution_root = certificate.settled_root();
            certificates.push(certificate);
        }
        Ok(certificates)
    }
}

impl RuntimeOps {
    /// Added by DR-134 (DR-65): the play of an offered batch. It replaces the
    /// single-offer play (`compute_offered_envelope_state`, whose member steps
    /// moved to `RuntimeManager::play_offered_member`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn compute_offered_batch_state(
        &mut self,
        start_hash: &StateHash,
        envelopes: &[&DeployEnvelope],
        system_deploys: Vec<SystemDeployEnum>,
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
        // The batch plays in canonical order (DR-65 item 1), as the proposer
        // orders it, so a request in another order plays the same batch. A
        // repeated member still fails the order check.
        let mut ordered: Vec<&DeployEnvelope> = envelopes.to_vec();
        ordered.sort_by(|left, right| {
            let left_key = (
                left.body().valid_after_block_number,
                left.body().time_stamp,
                left.identity().as_bytes(),
            );
            let right_key = (
                right.body().valid_after_block_number,
                right.body().time_stamp,
                right.identity().as_bytes(),
            );
            left_key.cmp(&right_key)
        });
        let envelopes = ordered.as_slice();
        check_batch_order(envelopes)?;
        let plan = manager
            .decide_offered_batch(
                start_hash,
                envelopes,
                block_data.time_stamp,
                adopted,
                AcceptanceRole::Play,
            )
            .await
            .map_err(|rejection| rejection.into_play_error(envelopes, start_hash))?;
        let mut shipped_count = envelopes.len();
        let mut budget = play_budget;
        let played = loop {
            let later = plan.later(shipped_count)?;
            let started = std::time::Instant::now();
            let mut played: Vec<PlayedOfferedMember> = Vec::with_capacity(shipped_count);
            let mut execution_root = root;
            let mut stopped = None;
            for position in 0..shipped_count {
                let envelope = envelopes[position];
                let member = manager
                    .play_offered_member(
                        envelope,
                        start_hash,
                        OfferedMemberContext {
                            execution_root,
                            later: &later[position],
                        },
                        plan.gate(position)?,
                        &block_data,
                        &invalid_blocks,
                        adopted,
                    )
                    .await
                    .map_err(|error| offered_candidate_rejection(envelope, start_hash, error))?;
                execution_root = member.final_root;
                played.push(member);
                // Dev's play budget (dev `play_deploys_for_state`): checked
                // after each member, so one member always plays and a spent
                // budget defers the rest to the next proposal.
                match (budget, played.len() < shipped_count) {
                    (Some(limit), true) if started.elapsed() >= limit => {
                        tracing::info!(
                            target: "f1r3fly.casper.play_deploys",
                            executed = played.len(),
                            deferred = shipped_count - played.len(),
                            budget_ms = limit.as_millis() as u64,
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            "play budget spent: remaining deploys deferred to a later proposal"
                        );
                        stopped = Some(played.len());
                        break;
                    }
                    _ => {}
                }
            }
            match stopped {
                None => break played,
                Some(shipped) => {
                    let rolled_back: Vec<bool> = played
                        .iter()
                        .map(|member| member.guard_rolled_back)
                        .collect();
                    // Added by G3 (DR-135, S8): the owner purses of each
                    // shipped member.
                    let owner_purses: Vec<Vec<[u8; 32]>> = played
                        .iter()
                        .map(|member| member.owner_purses.clone())
                        .collect();
                    // Changed by G3 (DR-135, S8): condition (c) reads the
                    // owner purses.
                    // match plan.rerun_after_truncation(shipped, &rolled_back)? {
                    match plan.rerun_after_truncation(shipped, &rolled_back, &owner_purses)? {
                        // O-10: the shipped prefix runs again from R0 with
                        // no budget, so the re-run happens at most once.
                        true => {
                            shipped_count = shipped;
                            budget = None;
                        }
                        false => break played,
                    }
                }
            }
        };
        let last_root = played
            .last()
            .map(|member| member.final_root)
            .ok_or_else(|| {
                CasperError::RuntimeError("an offered batch played no member".to_string())
            })?;
        let candidate_root: StateHash = last_root.to_vec().into();
        self.runtime
            .reset(&Blake2b256Hash::from_bytes(last_root.to_vec()))
            .await?;
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
        let mut users = Vec::with_capacity(played.len());
        users.extend(played.into_iter().map(|member| {
            (
                ProcessedUserDeploy::Offered(member.processed),
                member.mergeable,
            )
        }));
        Ok((post_root, users, systems))
    }
}
