use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use models::rust::native_cost_evidence::NativeCostFailureClass;
use prost::bytes::Bytes;

use super::backend::{block_data, close_block, exhausts};
use super::body::StateHash;
use super::gate::{GateInput, GateVerdict};
use super::settlement::{
    check_executed, check_settled, executed_of, EnvelopeOutcome, EnvelopeRun, ExecutedEnvelope,
    PurseDebit, SettledEnvelope, Settlement, SettlementOutcome,
};
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::direct_wallet_funding::RecordedRun;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

fn invalid(message: &str) -> CasperError { CasperError::RuntimeError(message.to_string()) }

fn root_of(root: &StateHash) -> Result<[u8; 32], CasperError> {
    root.as_ref()
        .try_into()
        .map_err(|_| invalid("a settlement root is not 32 bytes"))
}

fn offers(block: &BlockMessage) -> Vec<Option<&DeployEnvelope>> {
    block
        .body
        .deploys
        .iter()
        .map(|deploy| match GateInput::from(deploy) {
            GateInput::Offered(envelope) => Some(envelope),
            GateInput::Legacy => None,
        })
        .collect()
}

fn verdict_of(admission: &Result<Vec<([u8; 32], u128)>, String>) -> GateVerdict {
    match admission {
        Ok(holds) => GateVerdict::Admitted {
            holds: holds.clone(),
        },
        Err(cause) => GateVerdict::Rejected {
            cause: cause.clone(),
        },
    }
}

pub struct RhoSettlement {
    runtime_manager: Arc<RuntimeManager>,
    policy: Arc<AdoptedResourcePolicy>,
}

impl RhoSettlement {
    pub fn new(runtime_manager: Arc<RuntimeManager>, policy: Arc<AdoptedResourcePolicy>) -> Self {
        RhoSettlement {
            runtime_manager,
            policy,
        }
    }

    pub async fn run_sequentially(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
    ) -> Result<(Vec<EnvelopeRun>, StateHash), CasperError> {
        let funding_root = root_of(pre_root)?;
        let data = block_data(block);
        let offered = offers(block);
        let plan = self
            .runtime_manager
            .plan_offered_block(funding_root, data.time_stamp, &self.policy, &offered)
            .await?;
        let later = plan.later()?;
        let invalid_blocks = HashMap::new();
        let mut root = funding_root;
        let mut runs = Vec::with_capacity(offered.len());
        for (position, deploy) in block.body.deploys.iter().enumerate() {
            let envelope_id = Bytes::copy_from_slice(deploy.identity_bytes());
            let verdict = verdict_of(&plan.admissions()[position]);
            let (Some(member), Some(envelope)) = (plan.position_of(position), offered[position])
            else {
                runs.push(EnvelopeRun::recorded(ExecutedEnvelope {
                    envelope_id,
                    verdict,
                    outcome: EnvelopeOutcome::NotRun,
                    phlo_used: 0,
                    budget_recording: Bytes::new(),
                    operation_journal: Bytes::new(),
                    user_log: Vec::new(),
                }));
                continue;
            };
            let run = self
                .runtime_manager
                .run_offered_envelope(
                    envelope,
                    funding_root,
                    root,
                    plan.gate(member)?,
                    &later[member],
                    &data,
                    &invalid_blocks,
                    &self.policy,
                )
                .await?;
            let (budget, journal) = run.recording()?;
            let outcome = match run.evaluation().errors.iter().any(exhausts) {
                true => EnvelopeOutcome::Exhausted,
                false if run.failed() => EnvelopeOutcome::UserFailure,
                false => EnvelopeOutcome::Completed,
            };
            root = run.post_root();
            runs.push(EnvelopeRun {
                executed: ExecutedEnvelope {
                    envelope_id,
                    verdict,
                    outcome,
                    phlo_used: run
                        .phlo_used()
                        .ok_or_else(|| invalid("an offered run has no phlo usage"))?,
                    budget_recording: Bytes::from(budget),
                    operation_journal: Bytes::from(journal),
                    user_log: run.user_log(),
                },
                run: Some(Arc::new(run)),
            });
        }
        Ok((runs, Bytes::copy_from_slice(&root)))
    }

    pub async fn replay_runs(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
        runs: &[EnvelopeRun],
    ) -> Result<Vec<EnvelopeRun>, CasperError> {
        let executed = executed_of(runs);
        check_executed(block, &executed).map_err(|error| invalid(&error))?;
        let funding_root = root_of(pre_root)?;
        let data = block_data(block);
        let offered = offers(block);
        let plan = self
            .runtime_manager
            .plan_offered_block(funding_root, data.time_stamp, &self.policy, &offered)
            .await?;
        let invalid_blocks = HashMap::new();
        let mut rebuilt = Vec::with_capacity(runs.len());
        for (position, run) in runs.iter().enumerate() {
            let entry = &run.executed;
            if entry.verdict != verdict_of(&plan.admissions()[position]) {
                return Err(invalid(
                    "an executed verdict differs from the gate at the pre-state",
                ));
            }
            let (Some(member), Some(envelope), None) =
                (plan.position_of(position), offered[position], &run.run)
            else {
                rebuilt.push(run.clone());
                continue;
            };
            let replayed = self
                .runtime_manager
                .replay_offered_run(
                    envelope,
                    funding_root,
                    funding_root,
                    plan.gate(member)?,
                    RecordedRun {
                        phlo_used: entry.phlo_used,
                        failed: matches!(
                            entry.outcome,
                            EnvelopeOutcome::UserFailure | EnvelopeOutcome::Exhausted
                        ),
                        budget_recording: &entry.budget_recording,
                        operation_journal: &entry.operation_journal,
                        user_log: &entry.user_log,
                    },
                    &data,
                    &invalid_blocks,
                    &self.policy,
                )
                .await?;
            if replayed.user_log() != entry.user_log {
                return Err(invalid("the replayed user log differs from the record"));
            }
            rebuilt.push(EnvelopeRun {
                executed: entry.clone(),
                run: Some(Arc::new(replayed)),
            });
        }
        Ok(rebuilt)
    }
}

#[async_trait]
impl Settlement for RhoSettlement {
    async fn settle(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
        mid_root: &StateHash,
        runs: &[EnvelopeRun],
    ) -> Result<SettlementOutcome, CasperError> {
        let executed = executed_of(runs);
        check_executed(block, &executed).map_err(|error| invalid(&error))?;
        let funding_root = root_of(pre_root)?;
        let data = block_data(block);
        let offered = offers(block);
        let plan = self
            .runtime_manager
            .plan_offered_block(funding_root, data.time_stamp, &self.policy, &offered)
            .await?;
        if runs
            .iter()
            .zip(plan.admissions())
            .any(|(run, admission)| run.executed.verdict != verdict_of(admission))
        {
            return Err(invalid(
                "an executed verdict differs from the gate at the pre-state",
            ));
        }
        let later = plan.later()?;
        let invalid_blocks = HashMap::new();
        let evidence_limits = offered_funded_v6_limits().evidence;
        let mut root = root_of(mid_root)?;
        let mut settled = Vec::with_capacity(runs.len());
        for (position, run) in runs.iter().enumerate() {
            let entry = &run.executed;
            let (Some(member), Some(envelope)) = (plan.position_of(position), offered[position])
            else {
                settled.push(SettledEnvelope::unsettled(entry.envelope_id.clone()));
                continue;
            };
            let local = run
                .run
                .as_ref()
                .ok_or_else(|| invalid("an admitted envelope has no local run"))?;
            if local.phlo_used() != Some(entry.phlo_used) {
                return Err(invalid(
                    "the executed phlo usage differs from the local run",
                ));
            }
            let offer = self
                .runtime_manager
                .settle_offered_envelope(
                    envelope,
                    funding_root,
                    root,
                    local,
                    &later[member],
                    entry.outcome == EnvelopeOutcome::RolledBack,
                    &data,
                    &invalid_blocks,
                    &self.policy,
                )
                .await?;
            let completed = offer.failure_class == NativeCostFailureClass::Success;
            if completed != (entry.outcome == EnvelopeOutcome::Completed) {
                return Err(invalid(
                    "the settled failure class differs from the executed outcome",
                ));
            }
            let evidence = offer
                .processed
                .evidence(evidence_limits)
                .map_err(|error| invalid(&error))?;
            if evidence.budget_recording != entry.budget_recording.as_ref()
                || evidence.operation_journal != entry.operation_journal.as_ref()
            {
                return Err(invalid(
                    "the settled recording differs from the executed recording",
                ));
            }
            settled.push(SettledEnvelope {
                envelope_id: entry.envelope_id.clone(),
                rev_spent: evidence
                    .rev_spent()
                    .map_err(|error| invalid(&error.to_string()))?,
                fee_rev: evidence.fee_rev,
                debits: offer
                    .debits
                    .iter()
                    .map(|(purse, resource_rev, fee_rev)| PurseDebit {
                        purse: *purse,
                        resource_rev: *resource_rev,
                        fee_rev: *fee_rev,
                    })
                    .collect(),
                cost_evidence: Bytes::copy_from_slice(offer.processed.native_cost_evidence_bytes()),
            });
            root = offer.final_root;
        }
        check_settled(&executed, &settled).map_err(|error| invalid(&error))?;
        let (post_root, _, system_deploys) = self
            .runtime_manager
            .compute_state(
                &Bytes::copy_from_slice(&root),
                vec![],
                vec![close_block(block)],
                data,
                None,
            )
            .await?;
        Ok(SettlementOutcome {
            post_root,
            settled,
            system_deploys,
        })
    }

    async fn replay(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
        mid_root: &StateHash,
        runs: &[EnvelopeRun],
        outcome: &SettlementOutcome,
    ) -> Result<StateHash, CasperError> {
        let runs = self.replay_runs(block, pre_root, runs).await?;
        let recomputed = self.settle(block, pre_root, mid_root, &runs).await?;
        if recomputed != *outcome {
            return Err(invalid("settlement replay reached another outcome"));
        }
        Ok(recomputed.post_root)
    }
}
