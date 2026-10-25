use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use models::rust::casper::protocol::casper_message::BlockMessage;

use super::backend::ExecutionBackend;
use super::block_record::{BlockRecord, RecordEntry};
use super::body::StateHash;
use super::merge::{merge_runs, RunEffect};
use super::record::ExecutionRecord;
use super::rho_settlement::{base_outcome, RhoSettlement};
use super::settlement::{EnvelopeOutcome, EnvelopeRun};
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

fn invalid(message: &str) -> CasperError { CasperError::RuntimeError(message.to_string()) }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhaseTimings {
    pub runs: Duration,
    pub merge: Duration,
    pub settlement: Duration,
    pub settlement_passes: usize,
}

pub struct BlockExecutor {
    runtime_manager: Arc<RuntimeManager>,
    settlement: RhoSettlement,
    workers: usize,
}

impl BlockExecutor {
    pub fn new(runtime_manager: Arc<RuntimeManager>, policy: Arc<AdoptedResourcePolicy>) -> Self {
        let workers = std::thread::available_parallelism()
            .map(|cores| cores.get())
            .unwrap_or(1);
        BlockExecutor {
            settlement: RhoSettlement::new(Arc::clone(&runtime_manager), policy),
            runtime_manager,
            workers,
        }
    }

    pub fn with_workers(mut self, workers: usize) -> Self {
        self.workers = workers.max(1);
        self
    }

    pub async fn execute_timed(
        &self,
        pre_root: &StateHash,
        block: &BlockMessage,
    ) -> Result<(BlockRecord, PhaseTimings), CasperError> {
        let started = Instant::now();
        let runs = self
            .settlement
            .run_from_pre_state(block, pre_root, self.workers)
            .await?;
        let runs_time = started.elapsed();
        let (record, mut timings) = self.decide(block, pre_root, runs).await?;
        timings.runs = runs_time;
        Ok((record, timings))
    }

    async fn decide(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
        mut runs: Vec<EnvelopeRun>,
    ) -> Result<(BlockRecord, PhaseTimings), CasperError> {
        let base: Vec<EnvelopeOutcome> = runs
            .iter()
            .map(|run| match &run.run {
                Some(local) => base_outcome(local),
                None => run.executed.outcome,
            })
            .collect();
        let mut guard_failed = BTreeSet::new();
        let mut timings = PhaseTimings::default();
        loop {
            let started = Instant::now();
            let candidates: Vec<usize> = (0..runs.len())
                .filter(|position| {
                    base[*position] == EnvelopeOutcome::Completed
                        && !guard_failed.contains(position)
                })
                .collect();
            let mut effects = Vec::with_capacity(candidates.len());
            for position in &candidates {
                let local = runs[*position]
                    .run
                    .as_ref()
                    .ok_or_else(|| invalid("a completed envelope has no local run"))?;
                effects.push(RunEffect {
                    envelope_id: runs[*position].executed.envelope_id.clone(),
                    user_log: &runs[*position].executed.user_log,
                    post_root: local.post_root(),
                    number_channels: local.number_channels(),
                });
            }
            let merged = merge_runs(&self.runtime_manager, pre_root, &effects)?;
            drop(effects);
            timings.merge += started.elapsed();
            let mut rolled_back = guard_failed.clone();
            for (position, kept) in candidates.iter().zip(&merged.merged) {
                if !kept {
                    rolled_back.insert(*position);
                }
            }
            for (position, run) in runs.iter_mut().enumerate() {
                run.executed.outcome = match rolled_back.contains(&position) {
                    true => EnvelopeOutcome::RolledBack,
                    false => base[position],
                };
            }
            let started = Instant::now();
            let settled = self
                .settlement
                .settle_checked(block, pre_root, &merged.mid_root, &runs)
                .await?;
            timings.settlement += started.elapsed();
            timings.settlement_passes += 1;
            match settled {
                Ok(outcome) => {
                    let entries = runs
                        .iter()
                        .zip(outcome.settled)
                        .map(|(run, settled)| RecordEntry {
                            executed: run.executed.clone(),
                            settled,
                        })
                        .collect();
                    let record = BlockRecord::new(
                        block.block_hash.clone(),
                        pre_root.clone(),
                        merged.mid_root,
                        outcome.post_root,
                        entries,
                        outcome.system_deploys,
                    )
                    .map_err(|error| invalid(&error))?;
                    return Ok((record, timings));
                }
                Err(position) => {
                    if base.get(position) != Some(&EnvelopeOutcome::Completed)
                        || rolled_back.contains(&position)
                    {
                        return Err(invalid(
                            "the held-capacity guard failed for an envelope outside the merge",
                        ));
                    }
                    guard_failed.insert(position);
                }
            }
        }
    }
}

#[async_trait]
impl ExecutionBackend for BlockExecutor {
    type Record = BlockRecord;

    async fn execute(
        &self,
        pre_root: &StateHash,
        block: &BlockMessage,
    ) -> Result<BlockRecord, CasperError> {
        Ok(self.execute_timed(pre_root, block).await?.0)
    }

    async fn replay(
        &self,
        record: &BlockRecord,
        block: &BlockMessage,
    ) -> Result<StateHash, CasperError> {
        if record.target() != &block.block_hash {
            return Err(invalid("the record executes another block"));
        }
        let recorded: Vec<EnvelopeRun> = record
            .executed()
            .into_iter()
            .map(EnvelopeRun::recorded)
            .collect();
        let rebuilt = self
            .settlement
            .replay_runs(block, record.pre_root(), &recorded)
            .await?;
        let (recomputed, _) = self.decide(block, record.pre_root(), rebuilt).await?;
        if recomputed != *record {
            return Err(invalid("the replayed execution reached another record"));
        }
        Ok(recomputed.post_root().clone())
    }
}
