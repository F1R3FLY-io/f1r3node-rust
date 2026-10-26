use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, StreamExt, TryStreamExt};
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use models::rust::native_cost_evidence::NativeCostFailureClass;
use prost::bytes::Bytes;

use super::backend::{block_data, close_block, exhausts};
use super::body::StateHash;
use super::gate::{GateInput, GateVerdict};
use super::settlement::{
    check_executed, check_settled, executed_of, settled_difference, EnvelopeOutcome, EnvelopeRun,
    ExecutedEnvelope, PurseDebit, SettledEnvelope, Settlement, SettlementOutcome,
};
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::direct_wallet_funding::{OfferedRun, RecordedRun};
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

fn invalid(message: &str) -> CasperError { CasperError::RuntimeError(message.to_string()) }

pub(super) fn root_of(root: &StateHash) -> Result<[u8; 32], CasperError> {
    root.as_ref()
        .try_into()
        .map_err(|_| invalid("a settlement root is not 32 bytes"))
}

pub(super) fn offers(block: &BlockMessage) -> Vec<Option<&DeployEnvelope>> {
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

pub(super) fn verdict_of(admission: &Result<Vec<([u8; 32], u128)>, String>) -> GateVerdict {
    match admission {
        Ok(holds) => GateVerdict::Admitted {
            holds: holds.clone(),
        },
        Err(cause) => GateVerdict::Rejected {
            cause: cause.clone(),
        },
    }
}

pub(super) fn base_outcome(run: &OfferedRun) -> EnvelopeOutcome {
    match run.evaluation().errors.iter().any(exhausts) {
        true => EnvelopeOutcome::Exhausted,
        false if run.failed() => EnvelopeOutcome::UserFailure,
        false => EnvelopeOutcome::Completed,
    }
}

fn not_run(envelope_id: Bytes, verdict: GateVerdict) -> EnvelopeRun {
    EnvelopeRun::recorded(ExecutedEnvelope {
        envelope_id,
        verdict,
        outcome: EnvelopeOutcome::NotRun,
        phlo_used: 0,
        budget_recording: Bytes::new(),
        operation_journal: Bytes::new(),
        user_log: Vec::new(),
    })
}

fn executed_run(
    envelope_id: Bytes,
    verdict: GateVerdict,
    run: OfferedRun,
) -> Result<EnvelopeRun, CasperError> {
    let (budget, journal) = run.recording()?;
    Ok(EnvelopeRun {
        executed: ExecutedEnvelope {
            envelope_id,
            verdict,
            outcome: base_outcome(&run),
            phlo_used: run
                .phlo_used()
                .ok_or_else(|| invalid("an offered run has no phlo usage"))?,
            budget_recording: Bytes::from(budget),
            operation_journal: Bytes::from(journal),
            user_log: run.user_log(),
        },
        run: Some(Arc::new(run)),
    })
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
                runs.push(not_run(envelope_id, verdict));
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
            root = run.post_root();
            runs.push(executed_run(envelope_id, verdict, run)?);
        }
        Ok((runs, Bytes::copy_from_slice(&root)))
    }

    pub async fn run_from_pre_state(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
        workers: usize,
    ) -> Result<Vec<EnvelopeRun>, CasperError> {
        let funding_root = root_of(pre_root)?;
        let data = block_data(block);
        let offered = offers(block);
        let plan = self
            .runtime_manager
            .plan_offered_block(funding_root, data.time_stamp, &self.policy, &offered)
            .await?;
        let later = plan.later()?;
        let mut jobs = Vec::with_capacity(offered.len());
        for (position, deploy) in block.body.deploys.iter().enumerate() {
            let envelope_id = Bytes::copy_from_slice(deploy.identity_bytes());
            let verdict = verdict_of(&plan.admissions()[position]);
            let job = match (plan.position_of(position), offered[position]) {
                (Some(member), Some(envelope)) => {
                    Some((envelope.clone(), plan.gate(member)?, later[member].clone()))
                }
                _ => None,
            };
            jobs.push((envelope_id, verdict, job));
        }
        stream::iter(jobs)
            .map(|(envelope_id, verdict, job)| {
                let runtime_manager = Arc::clone(&self.runtime_manager);
                let policy = Arc::clone(&self.policy);
                let data = data.clone();
                async move {
                    let Some((envelope, gate, later)) = job else {
                        return Ok(not_run(envelope_id, verdict));
                    };
                    let run = tokio::spawn(async move {
                        runtime_manager
                            .run_offered_envelope(
                                &envelope,
                                funding_root,
                                funding_root,
                                gate,
                                &later,
                                &data,
                                &HashMap::new(),
                                &policy,
                            )
                            .await
                    })
                    .await
                    .map_err(|error| invalid(&format!("an offered run task failed: {error}")))??;
                    executed_run(envelope_id, verdict, run)
                }
            })
            .buffered(workers.max(1))
            .try_collect()
            .await
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
            let verdict = verdict_of(&plan.admissions()[position]);
            if entry.verdict != verdict {
                return Err(invalid(&format!(
                    "the executed verdict at position {position} differs from the gate at the \
                     pre-state: recorded {:?}, recomputed {:?}",
                    entry.verdict, verdict
                )));
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

impl RhoSettlement {
    pub async fn settle_checked(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
        mid_root: &StateHash,
        runs: &[EnvelopeRun],
        recorded: Option<&SettlementOutcome>,
    ) -> Result<Result<SettlementOutcome, usize>, CasperError> {
        let executed = executed_of(runs);
        check_executed(block, &executed).map_err(|error| invalid(&error))?;
        if recorded.is_some_and(|outcome| outcome.settled.len() != runs.len()) {
            return Err(invalid(
                "the recorded settlement does not cover every envelope",
            ));
        }
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
            let Some(offer) = self
                .runtime_manager
                .settle_offered_envelope(
                    envelope,
                    funding_root,
                    root,
                    local,
                    &later[member],
                    matches!(
                        entry.outcome,
                        EnvelopeOutcome::RolledBack | EnvelopeOutcome::Conflicted
                    ),
                    recorded.map(|outcome| {
                        let settled = &outcome.settled[position];
                        (&settled.wallet_log[..], &settled.receipt_log[..])
                    }),
                    &data,
                    &invalid_blocks,
                    &self.policy,
                )
                .await?
            else {
                return Ok(Err(position));
            };
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
                wallet_log: offer.wallet_log,
                receipt_log: offer.receipt_log,
            });
            root = offer.final_root;
        }
        check_settled(&executed, &settled).map_err(|error| invalid(&error))?;
        let root = Bytes::copy_from_slice(&root);
        let (post_root, system_deploys) = match recorded {
            Some(outcome) => (
                self.runtime_manager
                    .replay_compute_state(
                        &root,
                        vec![],
                        outcome.system_deploys.clone(),
                        &data,
                        None,
                        false,
                    )
                    .await?,
                outcome.system_deploys.clone(),
            ),
            None => {
                let (post_root, _, system_deploys) = self
                    .runtime_manager
                    .compute_state(&root, vec![], vec![close_block(block)], data, None)
                    .await?;
                (post_root, system_deploys)
            }
        };
        Ok(Ok(SettlementOutcome {
            post_root,
            settled,
            system_deploys,
        }))
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
        self.settle_checked(block, pre_root, mid_root, runs, None)
            .await?
            .map_err(|_| invalid("the held-capacity guard failed after the merge"))
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
        let recomputed = self
            .settle_checked(block, pre_root, mid_root, &runs, Some(outcome))
            .await?
            .map_err(|_| invalid("a recorded settlement failed its guard"))?;
        if recomputed != *outcome {
            let field = recomputed
                .settled
                .iter()
                .zip(&outcome.settled)
                .enumerate()
                .find_map(|(position, (left, right))| {
                    settled_difference(right, left)
                        .map(|field| format!("entry {position}: {field}"))
                })
                .unwrap_or_else(|| "the post-state root or system deploys".to_string());
            return Err(invalid(&format!(
                "settlement replay reached another outcome: {field}"
            )));
        }
        Ok(recomputed.post_root)
    }
}
