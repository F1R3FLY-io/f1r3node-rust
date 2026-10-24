use std::collections::{BTreeMap, BTreeSet};

use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::phlo_intent::{PhloConversionCompositionV2, PhloFundingIntentV2Limits};
use models::rust::phlo_schedule::PhloGenesisPolicy;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;

use super::funding_groups::{decide_groups, later_holds, BatchCandidate, GrantDraw};
use super::held_capacity::LaterHolds;
use super::{OfferedGate, OfferedGateInput};
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::{offered_candidate_rejection, RuntimeOps};
use crate::rust::util::rholang::acceptance::{prepare_offered_candidate, OfferedCandidateLimits};
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::costacc::offered_grants::{
    offered_grant_issue_call_limits, offered_grant_transition_limits,
};
use crate::rust::util::rholang::costacc::production_limits::offered_funded_v6_production_limits;
use crate::rust::util::rholang::costacc::vault_payer::VaultPayer;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub type OfferedHolds = Vec<([u8; 32], u128)>;

pub type OfferedAdmission = Result<OfferedHolds, String>;

pub(super) fn offered_member_limits() -> OfferedCandidateLimits {
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

impl RuntimeManager {
    async fn check_offered_member_at_root(
        &self,
        envelope: &DeployEnvelope,
        root: [u8; 32],
        block_time: i64,
        adopted: &AdoptedResourcePolicy,
        runtime: &RuntimeOps,
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
        let start_hash = prost::bytes::Bytes::copy_from_slice(&root);
        let snapshot = self
            .read_direct_offered_wallet_snapshot(envelope, &start_hash, grant_context.as_ref())
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
        })?;
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
        Ok((
            BatchCandidate {
                identity: prepared.envelope_identity,
                custodies,
                holds,
                supply,
                grants,
            },
            gate,
            payers,
        ))
    }

    pub(crate) async fn plan_offered_block(
        &self,
        root: [u8; 32],
        block_time: i64,
        adopted: &AdoptedResourcePolicy,
        envelopes: &[Option<&DeployEnvelope>],
    ) -> Result<OfferedBlockPlan, CasperError> {
        let start_hash = prost::bytes::Bytes::copy_from_slice(&root);
        let runtime = RuntimeOps::new(
            self.spawn_offered_runtime(offered_grant_issue_call_limits())
                .await?,
        );
        let mut admissions: Vec<OfferedAdmission> = Vec::with_capacity(envelopes.len());
        let mut checked = Vec::new();
        let mut seen = BTreeSet::new();
        for envelope in envelopes {
            let Some(envelope) = envelope else {
                admissions.push(Err(
                    "a protocol-6 shard admits only offered-funded envelopes".to_string(),
                ));
                continue;
            };
            if !seen.insert(envelope.identity().as_bytes().to_vec()) {
                admissions.push(Err("the envelope is repeated in the block".to_string()));
                continue;
            }
            match self
                .check_offered_member_at_root(envelope, root, block_time, adopted, &runtime)
                .await
            {
                Ok((candidate, gate, payers)) => {
                    admissions.push(Ok(gate.holds().to_vec()));
                    checked.push((admissions.len() - 1, candidate, gate, payers));
                }
                Err(error) => match offered_candidate_rejection(envelope, &start_hash, error) {
                    CasperError::OfferedCandidateRejected(rejection) => {
                        admissions.push(Err(rejection.reason))
                    }
                    error => return Err(error),
                },
            }
        }
        let candidates: Vec<BatchCandidate> = checked
            .iter()
            .map(|(_, candidate, _, _)| candidate.clone())
            .collect();
        let decision = decide_groups(&candidates)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let mut causes = BTreeMap::new();
        for group in decision.rejected {
            for member in group.members {
                causes.insert(member, group.cause.to_string());
            }
        }
        let mut plan = OfferedBlockPlan {
            admissions,
            positions: Vec::new(),
            candidates: Vec::new(),
            gates: Vec::new(),
            payers: BTreeMap::new(),
        };
        for (member, (position, candidate, gate, payers)) in checked.into_iter().enumerate() {
            match causes.remove(&member) {
                Some(cause) => plan.admissions[position] = Err(cause),
                None => {
                    plan.positions.push(position);
                    plan.candidates.push(candidate);
                    plan.gates.push(gate);
                    plan.payers.extend(payers);
                }
            }
        }
        if !causes.is_empty() {
            return Err(CasperError::RuntimeError(
                "a rejected group names an unknown member".to_string(),
            ));
        }
        Ok(plan)
    }

    pub async fn gate_offered_block(
        &self,
        root: [u8; 32],
        block_time: i64,
        adopted: &AdoptedResourcePolicy,
        envelopes: &[Option<&DeployEnvelope>],
    ) -> Result<Vec<OfferedAdmission>, CasperError> {
        Ok(self
            .plan_offered_block(root, block_time, adopted, envelopes)
            .await?
            .admissions)
    }
}

pub(crate) struct OfferedBlockPlan {
    admissions: Vec<OfferedAdmission>,
    positions: Vec<usize>,
    candidates: Vec<BatchCandidate>,
    gates: Vec<OfferedGate>,
    payers: BTreeMap<[u8; 32], VaultPayer>,
}

impl OfferedBlockPlan {
    pub(crate) fn admissions(&self) -> &[OfferedAdmission] { &self.admissions }

    pub(crate) fn position_of(&self, block_position: usize) -> Option<usize> {
        self.positions
            .iter()
            .position(|position| *position == block_position)
    }

    pub(crate) fn gate(&self, member: usize) -> Result<OfferedGate, CasperError> {
        self.gates.get(member).cloned().ok_or_else(|| {
            CasperError::RuntimeError("an admitted envelope has no check".to_string())
        })
    }

    pub(crate) fn later(&self) -> Result<Vec<LaterHolds>, CasperError> {
        let members: Vec<usize> = (0..self.candidates.len()).collect();
        later_holds(&self.candidates, &members)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?
            .into_iter()
            .map(|holds| {
                LaterHolds::new(holds, &self.payers)
                    .map_err(|error| CasperError::RuntimeError(error.to_string()))
            })
            .collect()
    }
}
