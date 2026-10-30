use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use crypto::rust::signatures::signed::Signed;
use futures::stream::{self, StreamExt, TryStreamExt};
use models::rhoapi::PCost;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, DeployData, Event, ProcessedDeploy,
};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsEndVal;

use super::backend::{block_data, close_block, ExecutionBackend};
use super::block_record::{BlockRecord, EnvelopeOutcome, ExecutedEnvelope, RecordEntry};
use super::body::StateHash;
use super::merge::{merge_runs, RunEffect};
use super::record::ExecutionRecord;
use super::run::EnvelopeRun;
use super::slash::{require_no_slashes, NoSlashing, SlashSource};
use crate::rust::errors::CasperError;
use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Clone, Debug)]
pub struct ExecutedBlock {
    pub mid_root: StateHash,
    pub runs: Vec<EnvelopeRun>,
    pub processed: Vec<ProcessedDeploy>,
    pub merged: Vec<bool>,
}

struct EnvelopeEffect {
    run: EnvelopeRun,
    processed: ProcessedDeploy,
    number_channels: NumberChannelsEndVal,
}

pub fn comm_count(log: &[Event]) -> u64 {
    log.iter()
        .filter(|event| matches!(event, Event::Comm(_)))
        .count() as u64
}

fn invalid(message: impl std::fmt::Display) -> CasperError {
    CasperError::RuntimeError(message.to_string())
}

fn root_bytes(root: &StateHash) -> Result<[u8; 32], CasperError> {
    root.as_ref()
        .try_into()
        .map_err(|_| invalid("a state root is 32 bytes"))
}

fn envelope_run(processed: &ProcessedDeploy, post_root: StateHash) -> EnvelopeRun {
    EnvelopeRun {
        envelope_sig: processed.deploy.sig.clone(),
        comm_phlo: comm_count(&processed.deploy_log),
        compute_phlo: processed.cost.cost,
        failed: processed.is_failed,
        user_log: processed.deploy_log.clone(),
        post_root,
    }
}

pub struct EnvelopeExecutor {
    runtime_manager: Arc<RuntimeManager>,
    workers: usize,
}

impl EnvelopeExecutor {
    pub fn new(runtime_manager: Arc<RuntimeManager>, workers: usize) -> Self {
        EnvelopeExecutor {
            runtime_manager,
            workers: workers.max(1),
        }
    }

    async fn run(
        &self,
        pre_root: &StateHash,
        envelope: Signed<DeployData>,
        block_data: BlockData,
    ) -> Result<EnvelopeEffect, CasperError> {
        let mut ops = RuntimeOps::new(self.runtime_manager.spawn_runtime().await?);
        ops.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(pre_root))
            .await
            .map_err(CasperError::InterpreterError)?;
        ops.runtime.set_block_data(block_data).await;
        ops.runtime.set_invalid_blocks(HashMap::new()).await;
        let (processed, number_channels) = ops.process_deploy_with_mergeable_data(envelope).await?;
        let post_root = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
        Ok(EnvelopeEffect {
            run: envelope_run(&processed, post_root),
            processed,
            number_channels,
        })
    }

    async fn replay_one(
        &self,
        pre_root: &StateHash,
        recorded: ProcessedDeploy,
        block_data: BlockData,
    ) -> Result<EnvelopeEffect, CasperError> {
        let mut replay =
            ReplayRuntimeOps::new_from_runtime(self.runtime_manager.spawn_replay_runtime().await?);
        let runtime = &mut replay.runtime_ops.runtime;
        runtime
            .reset(&Blake2b256Hash::from_bytes_prost(pre_root))
            .await
            .map_err(CasperError::InterpreterError)?;
        runtime.set_block_data(block_data).await;
        runtime.set_invalid_blocks(HashMap::new()).await;
        let number_channels = replay.replay_deploy_e(false, &recorded).await?;
        let post_root = replay
            .runtime_ops
            .runtime
            .create_checkpoint()
            .await
            .root
            .to_bytes_prost();
        Ok(EnvelopeEffect {
            run: envelope_run(&recorded, post_root),
            processed: recorded,
            number_channels,
        })
    }

    fn merge(
        &self,
        pre_root: &StateHash,
        effects: Vec<EnvelopeEffect>,
    ) -> Result<ExecutedBlock, CasperError> {
        let candidates: Vec<usize> = (0..effects.len())
            .filter(|i| !effects[*i].run.failed)
            .collect();
        let roots = candidates
            .iter()
            .map(|i| root_bytes(&effects[*i].run.post_root))
            .collect::<Result<Vec<_>, _>>()?;
        let inputs: Vec<RunEffect<'_>> = candidates
            .iter()
            .zip(roots)
            .map(|(i, post_root)| RunEffect {
                envelope_id: effects[*i].run.envelope_sig.clone(),
                user_log: &effects[*i].run.user_log,
                post_root,
                number_channels: &effects[*i].number_channels,
            })
            .collect();
        let merged = merge_runs(&self.runtime_manager, pre_root, &inputs)?;
        drop(inputs);
        let mut kept = vec![false; effects.len()];
        for (i, survived) in candidates.iter().zip(&merged.merged) {
            kept[*i] = *survived;
        }
        let (runs, processed) = effects
            .into_iter()
            .map(|effect| (effect.run, effect.processed))
            .unzip();
        Ok(ExecutedBlock {
            mid_root: merged.mid_root,
            runs,
            processed,
            merged: kept,
        })
    }

    pub async fn execute(
        &self,
        pre_root: &StateHash,
        envelopes: Vec<Signed<DeployData>>,
        block_data: BlockData,
    ) -> Result<ExecutedBlock, CasperError> {
        let effects: Vec<EnvelopeEffect> = stream::iter(envelopes)
            .map(|envelope| self.run(pre_root, envelope, block_data.clone()))
            .buffered(self.workers)
            .try_collect()
            .await?;
        self.merge(pre_root, effects)
    }

    pub async fn replay(
        &self,
        pre_root: &StateHash,
        recorded: Vec<ProcessedDeploy>,
        block_data: BlockData,
    ) -> Result<ExecutedBlock, CasperError> {
        let effects: Vec<EnvelopeEffect> = stream::iter(recorded)
            .map(|processed| self.replay_one(pre_root, processed, block_data.clone()))
            .buffered(self.workers)
            .try_collect()
            .await?;
        self.merge(pre_root, effects)
    }
}

pub struct BlockExecutor {
    envelopes: EnvelopeExecutor,
    runtime_manager: Arc<RuntimeManager>,
    slashes: Arc<dyn SlashSource>,
}

impl BlockExecutor {
    pub fn new(runtime_manager: Arc<RuntimeManager>, workers: usize) -> Self {
        BlockExecutor {
            envelopes: EnvelopeExecutor::new(Arc::clone(&runtime_manager), workers),
            runtime_manager,
            slashes: Arc::new(NoSlashing),
        }
    }

    pub fn with_slash_source(mut self, slashes: Arc<dyn SlashSource>) -> Self {
        self.slashes = slashes;
        self
    }
}

fn outcome(failed: bool, merged: bool) -> EnvelopeOutcome {
    match (failed, merged) {
        (true, _) => EnvelopeOutcome::UserFailure,
        (false, true) => EnvelopeOutcome::Completed,
        (false, false) => EnvelopeOutcome::Conflicted,
    }
}

fn entries(executed: &ExecutedBlock) -> Vec<RecordEntry> {
    executed
        .runs
        .iter()
        .zip(&executed.merged)
        .map(|(run, merged)| RecordEntry {
            executed: ExecutedEnvelope {
                envelope_id: run.envelope_sig.clone(),
                outcome: outcome(run.failed, *merged),
                comm_phlo: run.comm_phlo,
                compute_phlo: run.compute_phlo,
                user_log: run.user_log.clone(),
            },
        })
        .collect()
}

fn mismatch(what: &str) -> CasperError {
    CasperError::RuntimeError(format!("v6 record replay mismatch: {what}"))
}

#[async_trait]
impl ExecutionBackend for BlockExecutor {
    type Record = BlockRecord;

    async fn execute(
        &self,
        pre_root: &StateHash,
        block: &BlockMessage,
    ) -> Result<BlockRecord, CasperError> {
        require_no_slashes(&self.slashes.slashes(block)?)?;
        let envelopes = block
            .body
            .deploys
            .iter()
            .map(|processed| processed.deploy.clone())
            .collect();
        let executed = self
            .envelopes
            .execute(pre_root, envelopes, block_data(block))
            .await?;
        let (post_root, _, system_deploys) = self
            .runtime_manager
            .compute_state(
                &executed.mid_root,
                vec![],
                vec![close_block(block)],
                block_data(block),
                None,
            )
            .await?;
        BlockRecord::new(
            block.block_hash.clone(),
            pre_root.clone(),
            executed.mid_root.clone(),
            post_root,
            entries(&executed),
            system_deploys,
        )
        .map_err(CasperError::RuntimeError)
    }

    async fn replay(
        &self,
        record: &BlockRecord,
        block: &BlockMessage,
    ) -> Result<StateHash, CasperError> {
        if record.target() != &block.block_hash {
            return Err(mismatch("the record executes another block"));
        }
        require_no_slashes(&self.slashes.slashes(block)?)?;
        let recorded = record.executed();
        if recorded.len() != block.body.deploys.len() {
            return Err(mismatch("the record does not cover the block's envelopes"));
        }
        let processed = block
            .body
            .deploys
            .iter()
            .zip(&recorded)
            .map(|(envelope, entry)| {
                if envelope.deploy.sig != entry.envelope_id {
                    return Err(mismatch("envelope order"));
                }
                Ok(ProcessedDeploy {
                    deploy: envelope.deploy.clone(),
                    cost: PCost {
                        cost: entry.compute_phlo,
                    },
                    deploy_log: entry.user_log.clone(),
                    is_failed: entry.outcome == EnvelopeOutcome::UserFailure,
                    system_deploy_error: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let replayed = self
            .envelopes
            .replay(record.pre_root(), processed, block_data(block))
            .await?;
        if entries(&replayed) != record.entries() {
            return Err(mismatch("envelope outcomes or counts"));
        }
        if replayed.mid_root != *record.mid_root() {
            return Err(mismatch("user-phase root"));
        }
        let post_root = self
            .runtime_manager
            .replay_compute_state(
                record.mid_root(),
                vec![],
                record.system_deploys().to_vec(),
                &block_data(block),
                None,
                false,
            )
            .await?;
        if post_root != *record.post_root() {
            return Err(mismatch("post-state root"));
        }
        Ok(post_root)
    }
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::{CommEvent, ConsumeEvent, ProduceEvent};

    use super::*;

    fn produce() -> ProduceEvent {
        ProduceEvent {
            channels_hash: Default::default(),
            hash: Default::default(),
            persistent: false,
            times_repeated: 0,
            is_deterministic: true,
            output_value: Vec::new(),
            failed: false,
        }
    }

    fn consume() -> ConsumeEvent {
        ConsumeEvent {
            channels_hashes: Vec::new(),
            hash: Default::default(),
            persistent: false,
        }
    }

    fn comm() -> CommEvent {
        CommEvent {
            consume: consume(),
            produces: vec![produce()],
            peeks: Vec::new(),
        }
    }

    #[test]
    fn comm_count_counts_only_comm_events() {
        let log = vec![
            Event::Produce(produce()),
            Event::Comm(comm()),
            Event::Consume(consume()),
            Event::Comm(comm()),
        ];
        assert_eq!(comm_count(&log), 2);
        assert_eq!(comm_count(&[]), 0);
    }
}
