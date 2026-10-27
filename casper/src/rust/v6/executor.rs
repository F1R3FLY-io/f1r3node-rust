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
use super::settlement::{settled_difference, EnvelopeOutcome, EnvelopeRun, SettlementOutcome};
use super::slash::{require_no_slashes, NoSlashing, SlashSource};
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
    slashes: Arc<dyn SlashSource>,
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
            slashes: Arc::new(NoSlashing),
            workers,
        }
    }

    pub fn with_slash_source(mut self, slashes: Arc<dyn SlashSource>) -> Self {
        self.slashes = slashes;
        self
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
        require_no_slashes(&self.slashes.slashes(block)?)?;
        let started = Instant::now();
        let runs = self
            .settlement
            .run_from_pre_state(block, pre_root, self.workers)
            .await?;
        let runs_time = started.elapsed();
        let (record, mut timings) = self.decide(block, pre_root, runs, None).await?;
        timings.runs = runs_time;
        Ok((record, timings))
    }

    fn configure(
        &self,
        pre_root: &StateHash,
        runs: &mut [EnvelopeRun],
        base: &[EnvelopeOutcome],
        guard_failed: &BTreeSet<usize>,
    ) -> Result<StateHash, CasperError> {
        let candidates: Vec<usize> = (0..runs.len())
            .filter(|position| {
                base[*position] == EnvelopeOutcome::Completed && !guard_failed.contains(position)
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
        let conflicted: BTreeSet<usize> = candidates
            .iter()
            .zip(&merged.merged)
            .filter(|(_, kept)| !**kept)
            .map(|(position, _)| *position)
            .collect();
        for (position, run) in runs.iter_mut().enumerate() {
            run.executed.outcome = if guard_failed.contains(&position) {
                EnvelopeOutcome::RolledBack
            } else if conflicted.contains(&position) {
                EnvelopeOutcome::Conflicted
            } else {
                base[position]
            };
        }
        Ok(merged.mid_root)
    }

    async fn play(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
        runs: &mut [EnvelopeRun],
        base: &[EnvelopeOutcome],
        timings: &mut PhaseTimings,
    ) -> Result<(StateHash, SettlementOutcome, BTreeSet<usize>), CasperError> {
        let mut guard_failed = BTreeSet::new();
        loop {
            let started = Instant::now();
            let mid_root = self.configure(pre_root, runs, base, &guard_failed)?;
            timings.merge += started.elapsed();
            let started = Instant::now();
            let settled = self
                .settlement
                .settle_checked(block, pre_root, &mid_root, runs, None)
                .await?;
            timings.settlement += started.elapsed();
            timings.settlement_passes += 1;
            match settled {
                Ok(outcome) => return Ok((mid_root, outcome, guard_failed)),
                Err(position) => {
                    if runs.get(position).map(|run| run.executed.outcome)
                        != Some(EnvelopeOutcome::Completed)
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

    async fn decide(
        &self,
        block: &BlockMessage,
        pre_root: &StateHash,
        mut runs: Vec<EnvelopeRun>,
        recorded: Option<&BlockRecord>,
    ) -> Result<(BlockRecord, PhaseTimings), CasperError> {
        let base: Vec<EnvelopeOutcome> = runs
            .iter()
            .map(|run| match &run.run {
                Some(local) => base_outcome(local),
                None => run.executed.outcome,
            })
            .collect();
        let mut timings = PhaseTimings::default();
        let (mid_root, outcome) = match recorded {
            None => {
                let (mid_root, outcome, _) = self
                    .play(block, pre_root, &mut runs, &base, &mut timings)
                    .await?;
                (mid_root, outcome)
            }
            Some(record) => {
                let guard_failed: BTreeSet<usize> = record
                    .entries()
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| entry.executed.outcome == EnvelopeOutcome::RolledBack)
                    .map(|(position, _)| position)
                    .collect();
                if !guard_failed.is_empty() {
                    let mut trial = runs.clone();
                    let (_, _, derived) = self
                        .play(block, pre_root, &mut trial, &base, &mut timings)
                        .await?;
                    if derived != guard_failed {
                        return Err(invalid(
                            "the recorded guard rollbacks differ from the replayed ones",
                        ));
                    }
                }
                let started = Instant::now();
                let mid_root = self.configure(pre_root, &mut runs, &base, &guard_failed)?;
                timings.merge += started.elapsed();
                let recorded_outcome = SettlementOutcome {
                    post_root: record.post_root().clone(),
                    settled: record.settled(),
                    system_deploys: record.system_deploys().to_vec(),
                };
                let started = Instant::now();
                let outcome = self
                    .settlement
                    .settle_checked(block, pre_root, &mid_root, &runs, Some(&recorded_outcome))
                    .await?
                    .map_err(|_| invalid("a recorded settlement failed its guard"))?;
                timings.settlement += started.elapsed();
                timings.settlement_passes += 1;
                (mid_root, outcome)
            }
        };
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
            mid_root,
            outcome.post_root,
            entries,
            outcome.system_deploys,
        )
        .map_err(|error| invalid(&error))?;
        Ok((record, timings))
    }
}

fn difference(recorded: &BlockRecord, recomputed: &BlockRecord) -> String {
    if recorded.mid_root() != recomputed.mid_root() {
        return "the merge root differs".to_string();
    }
    if recorded.entries().len() != recomputed.entries().len() {
        return "the entry count differs".to_string();
    }
    for (position, (left, right)) in recorded
        .entries()
        .iter()
        .zip(recomputed.entries())
        .enumerate()
    {
        let (a, b) = (&left.executed, &right.executed);
        let field = if a.outcome != b.outcome {
            format!("outcome {:?} against {:?}", a.outcome, b.outcome)
        } else if a.phlo_used != b.phlo_used {
            format!("phlo used {} against {}", a.phlo_used, b.phlo_used)
        } else if a.budget_recording != b.budget_recording {
            "budget recording".to_string()
        } else if a.operation_journal != b.operation_journal {
            "operation journal".to_string()
        } else if a.user_log != b.user_log {
            "user log".to_string()
        } else if let Some(field) = settled_difference(&left.settled, &right.settled) {
            format!("settlement {field}")
        } else {
            continue;
        };
        return format!("entry {position}: {field}");
    }
    if recorded.post_root() != recomputed.post_root() {
        return "the post-state root differs".to_string();
    }
    "the system deploys differ".to_string()
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
        require_no_slashes(&self.slashes.slashes(block)?)?;
        let recorded: Vec<EnvelopeRun> = record
            .executed()
            .into_iter()
            .map(EnvelopeRun::recorded)
            .collect();
        let rebuilt = self
            .settlement
            .replay_runs(block, record.pre_root(), &recorded)
            .await?;
        let (recomputed, _) = self
            .decide(block, record.pre_root(), rebuilt, Some(record))
            .await?;
        if recomputed != *record {
            return Err(invalid(&format!(
                "the replayed execution reached another record: {}",
                difference(record, &recomputed)
            )));
        }
        Ok(recomputed.post_root().clone())
    }
}
