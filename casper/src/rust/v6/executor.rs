use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use crypto::rust::signatures::signed::Signed;
use futures::stream::{self, StreamExt, TryStreamExt};
use models::rhoapi::PCost;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, DeployData, Event, ProcessedDeploy,
};
use rholang::rust::interpreter::accounting::rev_stack::journal::{
    close_journal, open_journal, JournalEntry,
};
use rholang::rust::interpreter::accounting::rev_stack::reserve::{
    clear_reservation, set_reservation, Reservation,
};
use rholang::rust::interpreter::accounting::rev_stack::{
    wallet_channel_id, HotStoreCells, WalletOwner,
};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsEndVal;

use super::backend::{block_data, close_block, ExecutionBackend};
use super::block_record::{BlockRecord, EnvelopeOutcome, ExecutedEnvelope, RecordEntry};
use super::body::StateHash;
use super::gate::{
    check_recorded, gate_block, GateEntry, GateInput, GateOutcome, PhloLimitDemand, StoreReader,
};
use super::merge::{merge_runs, RunEffect};
use super::record::ExecutionRecord;
use super::run::EnvelopeRun;
use super::settle::{self, Boxcar, Settlement};
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
    pub gate: GateOutcome,
    pub settlement: Settlement,
    pub settled_root: StateHash,
}

struct MergedBlock {
    mid_root: StateHash,
    runs: Vec<EnvelopeRun>,
    processed: Vec<ProcessedDeploy>,
    kept: Vec<bool>,
    journals: Vec<Vec<JournalEntry>>,
}

struct EnvelopeEffect {
    run: EnvelopeRun,
    processed: ProcessedDeploy,
    number_channels: NumberChannelsEndVal,
    journal: Vec<JournalEntry>,
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
    boxcar: Boxcar,
}

impl EnvelopeExecutor {
    pub fn new(runtime_manager: Arc<RuntimeManager>, workers: usize) -> Self {
        EnvelopeExecutor {
            runtime_manager,
            workers: workers.max(1),
            boxcar: Boxcar::unbounded(),
        }
    }

    pub fn with_boxcar(mut self, boxcar: Boxcar) -> Self {
        self.boxcar = boxcar;
        self
    }

    async fn run(
        &self,
        pre_root: &StateHash,
        envelope: Signed<DeployData>,
        block_data: BlockData,
        reservations: Vec<(WalletOwner, Reservation)>,
    ) -> Result<EnvelopeEffect, CasperError> {
        let mut ops = RuntimeOps::new(self.runtime_manager.spawn_runtime().await?);
        ops.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(pre_root))
            .await
            .map_err(CasperError::InterpreterError)?;
        ops.runtime.set_block_data(block_data).await;
        ops.runtime.set_invalid_blocks(HashMap::new()).await;
        let cells = runtime_cells(&ops.runtime)?;
        hold(&cells, &reservations)?;
        open_journal(&cells).map_err(|e| invalid(format!("journal: {e}")))?;
        let (processed, number_channels) = ops.process_deploy_with_mergeable_data(envelope).await?;
        let journal = close_journal(&cells).map_err(|e| invalid(format!("journal: {e}")))?;
        release(&cells, &reservations)?;
        let post_root = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
        Ok(EnvelopeEffect {
            run: envelope_run(&processed, post_root),
            processed,
            number_channels,
            journal,
        })
    }

    async fn replay_one(
        &self,
        pre_root: &StateHash,
        recorded: ProcessedDeploy,
        block_data: BlockData,
        reservations: Vec<(WalletOwner, Reservation)>,
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
        let cells = runtime_cells(runtime)?;
        hold(&cells, &reservations)?;
        open_journal(&cells).map_err(|e| invalid(format!("journal: {e}")))?;
        let number_channels = replay.replay_deploy_e(false, &recorded).await?;
        let journal = close_journal(&cells).map_err(|e| invalid(format!("journal: {e}")))?;
        release(&cells, &reservations)?;
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
            journal,
        })
    }

    fn refused(pre_root: &StateHash, deploy: Signed<DeployData>) -> EnvelopeEffect {
        let processed = ProcessedDeploy::empty(deploy);
        EnvelopeEffect {
            run: envelope_run(&processed, pre_root.clone()),
            processed,
            number_channels: NumberChannelsEndVal::default(),
            journal: Vec::new(),
        }
    }

    fn merge(
        &self,
        pre_root: &StateHash,
        effects: Vec<EnvelopeEffect>,
        admitted: &[bool],
    ) -> Result<MergedBlock, CasperError> {
        let mut debited: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
        let mut candidates: Vec<usize> = Vec::new();
        for i in 0..effects.len() {
            if !admitted[i] || effects[i].run.failed {
                continue;
            }
            let payers: std::collections::BTreeSet<Vec<u8>> = effects[i]
                .journal
                .iter()
                .map(|j| j.from.canonical_bytes())
                .collect();
            if payers.iter().any(|p| debited.contains(p)) {
                continue;
            }
            debited.extend(payers);
            candidates.push(i);
        }
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
        let mut runs = Vec::with_capacity(effects.len());
        let mut processed = Vec::with_capacity(effects.len());
        let mut journals = Vec::with_capacity(effects.len());
        for effect in effects {
            runs.push(effect.run);
            processed.push(effect.processed);
            journals.push(effect.journal);
        }
        Ok(MergedBlock {
            mid_root: merged.mid_root,
            runs,
            processed,
            kept,
            journals,
        })
    }

    async fn settle(
        &self,
        mid_root: &StateHash,
        inputs: &[GateInput],
        gate: &GateOutcome,
        runs: &[EnvelopeRun],
        journals: &[Vec<JournalEntry>],
        kept: &[bool],
        block_data: &BlockData,
    ) -> Result<(Settlement, StateHash), CasperError> {
        let proposer = WalletOwner::from_public_key(&block_data.sender)
            .map_err(|e| invalid(format!("the block sender has no wallet: {e}")))?;
        settle::settle_block(
            &self.runtime_manager,
            mid_root,
            inputs,
            &gate.entries,
            runs,
            journals,
            kept,
            &proposer,
            self.boxcar,
        )
        .await
    }

    async fn finish(
        &self,
        pre_root: &StateHash,
        inputs: Vec<GateInput>,
        gate: GateOutcome,
        effects: Vec<EnvelopeEffect>,
        block_data: &BlockData,
    ) -> Result<ExecutedBlock, CasperError> {
        let admitted = gate.admitted();
        let MergedBlock {
            mid_root,
            runs,
            processed,
            kept: merged,
            journals,
        } = self.merge(pre_root, effects, &admitted)?;
        let (settlement, settled_root) = self
            .settle(
                &mid_root, &inputs, &gate, &runs, &journals, &merged, block_data,
            )
            .await?;
        Ok(ExecutedBlock {
            mid_root,
            runs,
            processed,
            merged,
            gate,
            settlement,
            settled_root,
        })
    }

    pub async fn execute(
        &self,
        pre_root: &StateHash,
        envelopes: Vec<Signed<DeployData>>,
        block_data: BlockData,
    ) -> Result<ExecutedBlock, CasperError> {
        let inputs = gate_inputs(envelopes.iter())?;
        let reader = StoreReader::at(&self.runtime_manager, pre_root).await?;
        let gate = gate_block(&reader, &PhloLimitDemand, &inputs)?;
        let effects: Vec<EnvelopeEffect> = stream::iter(envelopes.into_iter().enumerate())
            .map(|(i, envelope)| {
                let gate = &gate;
                let inputs = &inputs;
                let block_data = block_data.clone();
                async move {
                    if !gate.entries[i].admitted() {
                        return Ok(Self::refused(pre_root, envelope));
                    }
                    self.run(
                        pre_root,
                        envelope,
                        block_data,
                        reservations(gate, &inputs[i], i),
                    )
                    .await
                }
            })
            .buffered(self.workers)
            .try_collect()
            .await?;
        self.finish(pre_root, inputs, gate, effects, &block_data)
            .await
    }

    pub async fn replay(
        &self,
        pre_root: &StateHash,
        recorded: Vec<ProcessedDeploy>,
        recorded_gate: &[GateEntry],
        block_data: BlockData,
    ) -> Result<ExecutedBlock, CasperError> {
        let inputs = gate_inputs(recorded.iter().map(|p| &p.deploy))?;
        let reader = StoreReader::at(&self.runtime_manager, pre_root).await?;
        let gate = check_recorded(&reader, &PhloLimitDemand, &inputs, recorded_gate)
            .map_err(|e| invalid(format!("the recorded gate does not replay: {e}")))?;
        let effects: Vec<EnvelopeEffect> = stream::iter(recorded.into_iter().enumerate())
            .map(|(i, processed)| {
                let gate = &gate;
                let inputs = &inputs;
                let block_data = block_data.clone();
                async move {
                    if !gate.entries[i].admitted() {
                        return Ok(Self::refused(pre_root, processed.deploy));
                    }
                    self.replay_one(
                        pre_root,
                        processed,
                        block_data,
                        reservations(gate, &inputs[i], i),
                    )
                    .await
                }
            })
            .buffered(self.workers)
            .try_collect()
            .await?;
        self.finish(pre_root, inputs, gate, effects, &block_data)
            .await
    }
}

fn gate_inputs<'a>(
    deploys: impl Iterator<Item = &'a Signed<DeployData>>,
) -> Result<Vec<GateInput>, CasperError> {
    deploys
        .map(|d| GateInput::from_deploy(d, Vec::new()))
        .collect()
}

fn reservations(
    gate: &GateOutcome,
    input: &GateInput,
    i: usize,
) -> Vec<(WalletOwner, Reservation)> {
    input
        .paying_wallets()
        .into_iter()
        .map(|owner| {
            let wallet = wallet_channel_id(&owner);
            let reservation = Reservation {
                visible: gate.committed_through(i, &wallet),
                held: gate.table.committed(&wallet),
            };
            (owner, reservation)
        })
        .collect()
}

fn runtime_cells(runtime: &RhoRuntimeImpl) -> Result<HotStoreCells, CasperError> {
    runtime
        .reducer
        .space
        .hot_store()
        .map(HotStoreCells)
        .ok_or_else(|| invalid("the runtime's space has no native REV layer"))
}

fn hold(
    cells: &HotStoreCells,
    reservations: &[(WalletOwner, Reservation)],
) -> Result<(), CasperError> {
    for (owner, reservation) in reservations {
        set_reservation(cells, owner, *reservation)
            .map_err(|e| invalid(format!("reservation: {e}")))?;
    }
    Ok(())
}

fn release(
    cells: &HotStoreCells,
    reservations: &[(WalletOwner, Reservation)],
) -> Result<(), CasperError> {
    for (owner, _) in reservations {
        clear_reservation(cells, owner).map_err(|e| invalid(format!("reservation: {e}")))?;
    }
    Ok(())
}

pub struct BlockExecutor {
    envelopes: EnvelopeExecutor,
    runtime_manager: Arc<RuntimeManager>,
}

impl BlockExecutor {
    pub fn new(runtime_manager: Arc<RuntimeManager>, workers: usize) -> Self {
        BlockExecutor {
            envelopes: EnvelopeExecutor::new(Arc::clone(&runtime_manager), workers),
            runtime_manager,
        }
    }

    pub fn with_boxcar(mut self, boxcar: Boxcar) -> Self {
        self.envelopes = self.envelopes.with_boxcar(boxcar);
        self
    }
}

fn outcome(admitted: bool, exhausted: bool, failed: bool, merged: bool) -> EnvelopeOutcome {
    if !admitted {
        return EnvelopeOutcome::NotRun;
    }
    if failed && exhausted {
        return EnvelopeOutcome::Exhausted;
    }
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
        .zip(&executed.gate.entries)
        .map(|((run, merged), gate)| RecordEntry {
            executed: ExecutedEnvelope {
                envelope_id: run.envelope_sig.clone(),
                outcome: outcome(
                    gate.admitted(),
                    gate.delta > 0
                        && executed.settlement.charged_phlo(&run.envelope_sig) == Some(gate.delta),
                    run.failed,
                    *merged,
                ),
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
                &executed.settled_root,
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
        .and_then(|record| record.with_gate(executed.gate.entries.clone()))
        .map(|record| record.with_settlement(executed.settlement.clone()))
        .map_err(CasperError::RuntimeError)
    }

    async fn replay(
        &self,
        record: &BlockRecord,
        block: &BlockMessage,
    ) -> Result<StateHash, CasperError> {
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
                    is_failed: matches!(
                        entry.outcome,
                        EnvelopeOutcome::UserFailure | EnvelopeOutcome::Exhausted
                    ),
                    system_deploy_error: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let replayed = self
            .envelopes
            .replay(
                record.pre_root(),
                processed,
                record.gate(),
                block_data(block),
            )
            .await?;
        if entries(&replayed) != record.entries() {
            return Err(mismatch("envelope outcomes or counts"));
        }
        if replayed.mid_root != *record.mid_root() {
            return Err(mismatch("user-phase root"));
        }
        if record.settlement() != Some(&replayed.settlement) {
            return Err(mismatch("the settled charges"));
        }
        let post_root = self
            .runtime_manager
            .replay_compute_state(
                &replayed.settled_root,
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
