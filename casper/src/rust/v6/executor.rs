use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use crypto::rust::signatures::signed::Signed;
use futures::stream::{self, StreamExt, TryStreamExt};
use models::rhoapi::{BindPattern, ListParWithRandom, PCost, Par, TaggedContinuation};
use models::rust::casper::protocol::casper_message::{
    BlockMessage, DeployData, Event, ProcessedDeploy,
};
use rholang::rust::interpreter::accounting::rev_stack::journal::{
    close_journal, journal_overdrawn, open_journal, JournalEntry,
};
use models::rust::normalizer_env::normalizer_env_from_deploy;
use rholang::rust::interpreter::accounting::rev_stack::process::{has_bare_stack, self_sealed_envelope};
use rholang::rust::interpreter::accounting::rev_stack::region::named_owners;
use rholang::rust::interpreter::accounting::rev_stack::reserve::{
    clear_reservation, set_reservation, Reservation,
};
use rholang::rust::interpreter::accounting::membrane::{BlockLedger, Funding, MembraneMeter, Metered, Refusal};
use rholang::rust::interpreter::accounting::rev_stack::{
    wallet_channel_id, wallet_channel_id_of, HotStoreCells, RevAmount, WalletOwner,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash as stable_hash;
use rspace_plus_plus::rspace::checkpoint::SoftCheckpoint;
use rspace_plus_plus::rspace::hot_store_action::HotStoreAction;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsEndVal;

use super::backend::{block_data, close_block, ExecutionBackend};
use super::block_record::{BlockRecord, EnvelopeOutcome, ExecutedEnvelope, RecordEntry};
use super::body::StateHash;
use super::conservation::Conservation;
use super::footprint::{footprint, Footprint};
use super::gate::{
    check_recorded_with_close, gate_block_with_close, resolve_joint_wallets, GateEntry, GateInput, GateOutcome,
    PhloLimitDemand, StoreReader,
};
use super::merge::{merge_runs, RunEffect};
use super::record::ExecutionRecord;
use super::run::EnvelopeRun;
use super::settle::{self, Boxcar, Settlement};
use super::slash::{require_no_slashes, NoSlashing, SlashSource};
use super::surface::{accept_metered, own_cells, BlockPurses, CloseFunding, EnvelopeFunding, Surfaces};
use crate::rust::errors::CasperError;
use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::runtime_manager::{RuntimeManager, SystemGate};

#[derive(Clone, Debug)]
pub struct ExecutedBlock {
    pub mid_root: StateHash,
    pub runs: Vec<EnvelopeRun>,
    pub processed: Vec<ProcessedDeploy>,
    pub merged: Vec<bool>,
    pub footprints: Vec<Footprint>,
    pub surfaces: Vec<Surfaces>,
    pub work: Vec<Surfaces>,
    pub metered: Vec<RunMeter>,
    pub overdrawn: Vec<bool>,
    pub gate: GateOutcome,
    pub settlement: Settlement,
    pub settled_root: StateHash,
    pub timings: PhaseTimings,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunMeter {
    pub refusals: Vec<Refusal>,
    pub drawn: std::collections::BTreeMap<Vec<u8>, u64>,
    pub reached: std::collections::BTreeMap<Vec<u8>, u64>,
}

impl From<Metered> for RunMeter {
    fn from(m: Metered) -> Self {
        RunMeter {
            refusals: m.refusals,
            drawn: m.drawn,
            reached: m.reached,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhaseTimings {
    pub gate: std::time::Duration,
    pub runs: std::time::Duration,
    pub run_spawn: std::time::Duration,
    pub run_setup: std::time::Duration,
    pub run_eval: std::time::Duration,
    pub run_checkpoint: std::time::Duration,
    pub merge: std::time::Duration,
    pub settlement: std::time::Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockTimings {
    pub envelopes: PhaseTimings,
    pub close_block: std::time::Duration,
    pub audit: std::time::Duration,
    pub record: std::time::Duration,
}

struct MergedBlock {
    mid_root: StateHash,
    runs: Vec<EnvelopeRun>,
    processed: Vec<ProcessedDeploy>,
    kept: Vec<bool>,
    overdrawn: Vec<bool>,
    journals: Vec<Vec<JournalEntry>>,
    footprints: Vec<Footprint>,
    surfaces: Vec<Surfaces>,
    work: Vec<Surfaces>,
    metered: Vec<RunMeter>,
}

struct EnvelopeEffect {
    run: EnvelopeRun,
    surfaces: Surfaces,
    work: Surfaces,
    metered: RunMeter,
    processed: ProcessedDeploy,
    number_channels: NumberChannelsEndVal,
    changes: Vec<HotStoreAction<Par, BindPattern, ListParWithRandom, TaggedContinuation>>,
    journal: Vec<JournalEntry>,
    timing: [std::time::Duration; 4],
    overdrawn: bool,
}

pub fn comm_count(log: &[Event]) -> u64 {
    log.iter()
        .filter(|event| matches!(event, Event::Comm(_)))
        .count() as u64
}

fn invalid(message: impl std::fmt::Display) -> CasperError {
    CasperError::RuntimeError(message.to_string())
}

fn envelope_run(processed: &ProcessedDeploy) -> EnvelopeRun {
    EnvelopeRun {
        envelope_sig: processed.deploy.sig.clone(),
        comm_phlo: comm_count(&processed.deploy_log),
        compute_phlo: processed.cost.cost,
        failed: processed.is_failed,
        user_log: processed.deploy_log.clone(),
    }
}


pub struct EnvelopeExecutor {
    runtime_manager: Arc<RuntimeManager>,
    workers: usize,
    boxcar: Boxcar,
    runs: Arc<Runs>,
}

struct Runs {
    runtime_manager: Arc<RuntimeManager>,
    workers: usize,
    play_pool: parking_lot::Mutex<Vec<Pooled>>,
    replay_pool: parking_lot::Mutex<Vec<Pooled>>,
}

struct Pooled {
    runtime: RhoRuntimeImpl,
    base: Option<(StateHash, SoftCheckpoint<Par, BindPattern, ListParWithRandom, TaggedContinuation>)>,
}

impl EnvelopeExecutor {
    pub fn new(runtime_manager: Arc<RuntimeManager>, workers: usize) -> Self {
        let workers = workers.max(1);
        EnvelopeExecutor {
            runs: Arc::new(Runs {
                runtime_manager: Arc::clone(&runtime_manager),
                workers,
                play_pool: parking_lot::Mutex::new(Vec::new()),
                replay_pool: parking_lot::Mutex::new(Vec::new()),
            }),
            runtime_manager,
            workers,
            boxcar: Boxcar::unbounded(),
        }
    }

    pub fn with_boxcar(mut self, boxcar: Boxcar) -> Self {
        self.boxcar = boxcar;
        self
    }

    fn close_hold(
        block_data: &BlockData,
        precharge: RevAmount,
    ) -> Result<Option<(WalletOwner, RevAmount)>, CasperError> {
        if precharge == 0 {
            return Ok(None);
        }
        let proposer = WalletOwner::from_public_key(&block_data.sender)
            .map_err(|e| invalid(format!("the block sender has no wallet: {e}")))?;
        Ok(Some((proposer, precharge)))
    }
}

impl Runs {

    async fn runtime_at(&self, replay: bool, pre_root: &StateHash) -> Result<Pooled, CasperError> {
        let pool = if replay { &self.replay_pool } else { &self.play_pool };
        let taken = pool.lock().pop();
        let mut pooled = match taken {
            Some(pooled) => pooled,
            None => Pooled {
                runtime: match replay {
                    true => self.runtime_manager.spawn_replay_runtime().await?,
                    false => self.runtime_manager.spawn_runtime().await?,
                },
                base: None,
            },
        };
        let root = Blake2b256Hash::from_bytes_prost(pre_root);
        match &pooled.base {
            Some((at, checkpoint)) if at == pre_root => pooled
                .runtime
                .reset_to_soft_checkpoint(&root, checkpoint.clone())
                .await
                .map_err(CasperError::InterpreterError)?,
            _ => {
                pooled
                    .runtime
                    .reset(&root)
                    .await
                    .map_err(CasperError::InterpreterError)?;
                let checkpoint = pooled.runtime.create_soft_checkpoint().await;
                pooled.base = Some((pre_root.clone(), checkpoint));
            }
        }
        Ok(pooled)
    }

    fn give_back(&self, replay: bool, pooled: Pooled) {
        let mut pool = if replay { self.replay_pool.lock() } else { self.play_pool.lock() };
        if pool.len() < self.workers {
            pool.push(pooled);
        }
    }

    async fn run(
        &self,
        pre_root: &StateHash,
        envelope: Signed<DeployData>,
        block_data: BlockData,
        reservations: Vec<(WalletOwner, Reservation)>,
        meter: Arc<MembraneMeter>,
    ) -> Result<EnvelopeEffect, CasperError> {
        let started = std::time::Instant::now();
        let spawned = started.elapsed();
        let Pooled { runtime, base } = self.runtime_at(false, pre_root).await?;
        let mut ops = RuntimeOps::new(runtime);
        ops.runtime.set_block_data(block_data).await;
        ops.runtime.set_invalid_blocks(HashMap::new()).await;
        ops.runtime.set_seal_records(true);
        ops.runtime.set_funding(Some(meter.clone()));
        ops.runtime.cost.charge_storage_left_in_space();
        ops.runtime.cost.comm_ledger().reset();
        let cells = runtime_cells(&ops.runtime)?;
        hold(&cells, &reservations)?;
        open_journal(&cells).map_err(|e| invalid(format!("journal: {e}")))?;
        let setup = started.elapsed();
        let (mut processed, number_channels) = ops.process_deploy_with_mergeable_data(envelope).await?;
        let evaluated = started.elapsed();
        let overdrawn = journal_overdrawn(Arc::new(runtime_cells(&ops.runtime)?))
            .map_err(|e| invalid(format!("journal: {e}")))?;
        let journal = close_journal(&cells).map_err(|e| invalid(format!("journal: {e}")))?;
        release(&cells, &reservations)?;
        let surfaces: Surfaces = ops.runtime.cost.comm_ledger().take_surfaces().into_iter().collect();
        let work: Surfaces = ops.runtime.cost.comm_ledger().take_work().into_iter().collect();
        let changes = runtime_cells(&ops.runtime)?.0.changes();
        let checkpointed = started.elapsed();
        ops.runtime.set_funding(None);
        self.give_back(false, Pooled { runtime: ops.runtime, base });
        let metered = meter.take();
        processed
            .deploy_log
            .extend(metered.refusals.iter().map(super::stops::to_event));
        Ok(EnvelopeEffect {
            run: envelope_run(&processed),
            surfaces,
            work,
            metered: metered.into(),
            processed,
            number_channels,
            changes,
            journal,
            timing: [
                spawned,
                setup - spawned,
                evaluated - setup,
                checkpointed - evaluated,
            ],
            overdrawn,
        })
    }

    async fn replay_one(
        &self,
        pre_root: &StateHash,
        recorded: ProcessedDeploy,
        block_data: BlockData,
        reservations: Vec<(WalletOwner, Reservation)>,
        meter: Arc<MembraneMeter>,
        logged: Vec<Refusal>,
        recorded_work: Option<Surfaces>,
    ) -> Result<EnvelopeEffect, CasperError> {
        let Pooled { runtime, base } = self.runtime_at(true, pre_root).await?;
        let mut replay = ReplayRuntimeOps::new_from_runtime(runtime);
        let runtime = &mut replay.runtime_ops.runtime;
        runtime.set_block_data(block_data).await;
        runtime.set_invalid_blocks(HashMap::new()).await;
        runtime.set_seal_records(true);
        runtime.set_funding(Some(meter.clone()));
        runtime.cost.charge_storage_left_in_space();
        runtime.cost.comm_ledger().reset();
        let cells = runtime_cells(runtime)?;
        hold(&cells, &reservations)?;
        open_journal(&cells).map_err(|e| invalid(format!("journal: {e}")))?;
        let number_channels = replay.replay_deploy_e(false, &recorded).await?;
        let overdrawn = journal_overdrawn(Arc::new(runtime_cells(&replay.runtime_ops.runtime)?))
            .map_err(|e| invalid(format!("journal: {e}")))?;
        let journal = close_journal(&cells).map_err(|e| invalid(format!("journal: {e}")))?;
        release(&cells, &reservations)?;
        let surfaces: Surfaces = replay
            .runtime_ops
            .runtime
            .cost
            .comm_ledger()
            .take_surfaces()
            .into_iter()
            .collect();
        let work: Surfaces = replay.runtime_ops.runtime.cost.comm_ledger().take_work().into_iter().collect();
        let work = super::stops::settled_work(&logged, work, recorded_work.as_ref())
        .map_err(|e| invalid(format!("a stopped membrane does not settle: {e:?}")))?;
        let changes = runtime_cells(&replay.runtime_ops.runtime)?.0.changes();
        replay.runtime_ops.runtime.set_funding(None);
        self.give_back(true, Pooled { runtime: replay.runtime_ops.runtime, base });
        Ok(EnvelopeEffect {
            run: envelope_run(&recorded),
            surfaces,
            work,
            metered: meter.take().into(),
            processed: recorded,
            number_channels,
            changes,
            journal,
            timing: Default::default(),
            overdrawn,
        })
    }
}

impl EnvelopeExecutor {
    fn refused(deploy: Signed<DeployData>) -> EnvelopeEffect {
        let processed = ProcessedDeploy::empty(deploy);
        EnvelopeEffect {
            run: envelope_run(&processed),
            processed,
            number_channels: NumberChannelsEndVal::default(),
            surfaces: Vec::new(),
            work: Vec::new(),
            metered: RunMeter::default(),
            changes: Vec::new(),
            journal: Vec::new(),
            timing: Default::default(),
            overdrawn: false,
        }
    }

    fn footprints(
        &self,
        pre_root: &StateHash,
        effects: &[EnvelopeEffect],
        admitted: &[bool],
    ) -> Result<Vec<Footprint>, CasperError> {
        let reader = self
            .runtime_manager
            .history_repo
            .get_history_reader(&Blake2b256Hash::from_bytes_prost(pre_root))
            .map_err(CasperError::HistoryError)?;
        let joins = |channel: &prost::bytes::Bytes| {
            reader
                .get_joins(&Blake2b256Hash::from_bytes(channel.to_vec()))
                .map(|joins| {
                    joins
                        .iter()
                        .map(|join| {
                            join.iter()
                                .map(|c| prost::bytes::Bytes::from(stable_hash(c).bytes()))
                                .collect()
                        })
                        .collect()
                })
                .map_err(CasperError::HistoryError)
        };
        effects
            .iter()
            .zip(admitted)
            .map(|(effect, admitted)| match *admitted && !effect.run.failed {
                true => footprint(&effect.run.user_log, &effect.number_channels, joins),
                false => Ok(Footprint::default()),
            })
            .collect()
    }

    fn merge(
        &self,
        pre_root: &StateHash,
        effects: Vec<EnvelopeEffect>,
        admitted: &[bool],
    ) -> Result<MergedBlock, CasperError> {
        let footprints = self.footprints(pre_root, &effects, admitted)?;
        let mut debited: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
        let mut candidates: Vec<usize> = Vec::new();
        for i in 0..effects.len() {
            if !admitted[i] || effects[i].run.failed || effects[i].overdrawn {
                continue;
            }
            let payers: std::collections::BTreeSet<Vec<u8>> = effects[i]
                .journal
                .iter()
                .filter_map(|j| j.from.payer().map(|from| wallet_channel_id_of(from, &j.kind)))
                .collect();
            if payers.iter().any(|p| debited.contains(p)) {
                continue;
            }
            if candidates
                .iter()
                .any(|kept| footprints[*kept].conflicts_with(&footprints[i]))
            {
                continue;
            }
            debited.extend(payers);
            candidates.push(i);
        }
        let inputs: Vec<RunEffect<'_>> = candidates
            .iter()
            .map(|i| RunEffect {
                envelope_id: effects[*i].run.envelope_sig.clone(),
                user_log: &effects[*i].run.user_log,
                changes: &effects[*i].changes,
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
        let mut overdrawn = Vec::with_capacity(effects.len());
        let mut surfaces = Vec::with_capacity(effects.len());
        let mut work = Vec::with_capacity(effects.len());
        let mut metered = Vec::with_capacity(effects.len());
        for effect in effects {
            surfaces.push(effect.surfaces);
            work.push(effect.work);
            metered.push(effect.metered);
            runs.push(effect.run);
            processed.push(effect.processed);
            journals.push(effect.journal);
            overdrawn.push(effect.overdrawn);
        }
        Ok(MergedBlock {
            mid_root: merged.mid_root,
            surfaces,
            work,
            metered,
            runs,
            processed,
            kept,
            journals,
            footprints,
            overdrawn,
        })
    }

    async fn settle(
        &self,
        mid_root: &StateHash,
        inputs: &[GateInput],
        gate: &GateOutcome,
        runs: &[EnvelopeRun],
        surfaces: &[Surfaces],
        work: &[Surfaces],
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
            surfaces,
            work,
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
        mut timings: PhaseTimings,
    ) -> Result<ExecutedBlock, CasperError> {
        let admitted = gate.admitted();
        for effect in &effects {
            timings.run_spawn += effect.timing[0];
            timings.run_setup += effect.timing[1];
            timings.run_eval += effect.timing[2];
            timings.run_checkpoint += effect.timing[3];
        }
        let started = std::time::Instant::now();
        let MergedBlock {
            mid_root,
            runs,
            processed,
            kept: merged,
            journals,
            footprints,
            surfaces,
            work,
            metered,
            overdrawn,
        } = self.merge(pre_root, effects, &admitted)?;
        timings.merge = started.elapsed();
        let started = std::time::Instant::now();
        let (settlement, settled_root) = self
            .settle(
                &mid_root, &inputs, &gate, &runs, &surfaces, &work, &journals, &merged, block_data,
            )
            .await?;
        timings.settlement = started.elapsed();
        Ok(ExecutedBlock {
            mid_root,
            runs,
            processed,
            merged,
            footprints,
            surfaces,
            work,
            metered,
            overdrawn,
            gate,
            settlement,
            settled_root,
            timings,
        })
    }

    pub async fn execute(
        &self,
        pre_root: &StateHash,
        envelopes: Vec<Signed<DeployData>>,
        block_data: BlockData,
    ) -> Result<ExecutedBlock, CasperError> {
        self.execute_held(pre_root, envelopes, block_data, 0).await
    }

    pub async fn execute_held(
        &self,
        pre_root: &StateHash,
        envelopes: Vec<Signed<DeployData>>,
        block_data: BlockData,
        close_precharge: RevAmount,
    ) -> Result<ExecutedBlock, CasperError> {
        let started = std::time::Instant::now();
        let mut inputs = gate_inputs(envelopes.iter())?;
        let reader = StoreReader::at(&self.runtime_manager, pre_root).await?;
        resolve_joint_wallets(&reader, &mut inputs)?;
        let close = Self::close_hold(&block_data, close_precharge)?;
        let gate = gate_block_with_close(
            &reader,
            &PhloLimitDemand,
            &inputs,
            close.as_ref().map(|(owner, amount)| (owner, *amount)),
        )?;
        let purses = Arc::new(BlockPurses::new(Box::new(reader), gate.table.clone()));
        let ledger = BlockLedger::new();
        let gate_time = started.elapsed();
        let effects: Vec<EnvelopeEffect> = stream::iter(envelopes.into_iter().enumerate())
            .map(|(i, envelope)| {
                let gate = &gate;
                let inputs = &inputs;
                let block_data = block_data.clone();
                let admitted = gate.entries[i].admitted();
                let held = reservations(gate, &inputs[i], i);
                let funding = EnvelopeFunding::new(purses.clone(), inputs[i].paying_wallets());
                let meter = MembraneMeter::play(funding, ledger.clone());
                let runs = Arc::clone(&self.runs);
                let pre_root = pre_root.clone();
                async move {
                    if !admitted {
                        return Ok(Self::refused(envelope));
                    }
                    tokio::spawn(async move { runs.run(&pre_root, envelope, block_data, held, meter).await })
                        .await
                        .map_err(|e| invalid(format!("an envelope run stopped: {e}")))?
                }
            })
            .buffered(self.workers)
            .try_collect()
            .await?;
        let timings = PhaseTimings {
            gate: gate_time,
            runs: started.elapsed() - gate_time,
            ..PhaseTimings::default()
        };
        self.finish(pre_root, inputs, gate, effects, &block_data, timings)
            .await
    }

    pub async fn replay(
        &self,
        pre_root: &StateHash,
        recorded: Vec<ProcessedDeploy>,
        recorded_gate: &[GateEntry],
        block_data: BlockData,
    ) -> Result<ExecutedBlock, CasperError> {
        self.replay_recorded(pre_root, recorded, None, recorded_gate, block_data)
            .await
    }

    pub async fn replay_with_refusals(
        &self,
        pre_root: &StateHash,
        recorded: Vec<ProcessedDeploy>,
        recorded_gate: &[GateEntry],
        block_data: BlockData,
        logged: Vec<Vec<Refusal>>,
        recorded_work: Option<Vec<Surfaces>>,
    ) -> Result<ExecutedBlock, CasperError> {
        let recorded = recorded
            .into_iter()
            .zip(logged)
            .map(|(mut processed, logged)| {
                processed
                    .deploy_log
                    .retain(|event| !matches!(event, models::rust::casper::protocol::casper_message::Event::Refusal(_)));
                processed
                    .deploy_log
                    .extend(logged.iter().map(super::stops::to_event));
                processed
            })
            .collect();
        self.replay_recorded(pre_root, recorded, recorded_work, recorded_gate, block_data)
            .await
    }

    pub async fn replay_recorded(
        &self,
        pre_root: &StateHash,
        recorded: Vec<ProcessedDeploy>,
        recorded_work: Option<Vec<Surfaces>>,
        recorded_gate: &[GateEntry],
        block_data: BlockData,
    ) -> Result<ExecutedBlock, CasperError> {
        self.replay_held(pre_root, recorded, recorded_work, recorded_gate, block_data, 0)
            .await
    }

    pub async fn replay_held(
        &self,
        pre_root: &StateHash,
        recorded: Vec<ProcessedDeploy>,
        recorded_work: Option<Vec<Surfaces>>,
        recorded_gate: &[GateEntry],
        block_data: BlockData,
        close_precharge: RevAmount,
    ) -> Result<ExecutedBlock, CasperError> {
        let mut inputs = gate_inputs(recorded.iter().map(|p| &p.deploy))?;
        let reader = StoreReader::at(&self.runtime_manager, pre_root).await?;
        resolve_joint_wallets(&reader, &mut inputs)?;
        let close = Self::close_hold(&block_data, close_precharge)?;
        let gate = check_recorded_with_close(
            &reader,
            &PhloLimitDemand,
            &inputs,
            recorded_gate,
            close.as_ref().map(|(owner, amount)| (owner, *amount)),
        )
            .map_err(|e| invalid(format!("the recorded gate does not replay: {e}")))?;
        let purses = Arc::new(BlockPurses::new(Box::new(reader), gate.table.clone()));
        let logged = recorded
            .iter()
            .map(super::stops::refusals_of)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| invalid(format!("a refusal event does not decode: {e}")))?;
        let ledger = BlockLedger::new();
        let logged_runs = logged.clone();
        let effects: Vec<EnvelopeEffect> = stream::iter(recorded.into_iter().zip(logged).enumerate())
            .map(|(i, (processed, logged))| {
                let gate = &gate;
                let inputs = &inputs;
                let block_data = block_data.clone();
                let admitted = gate.entries[i].admitted();
                let held = reservations(gate, &inputs[i], i);
                let funding = EnvelopeFunding::new(purses.clone(), inputs[i].paying_wallets());
                let work = recorded_work.as_ref().and_then(|w| w.get(i).cloned());
                let meter = MembraneMeter::replay(funding, ledger.clone(), &logged);
                let runs = Arc::clone(&self.runs);
                let pre_root = pre_root.clone();
                async move {
                    if !admitted {
                        return Ok(Self::refused(processed.deploy));
                    }
                    tokio::spawn(async move {
                        runs.replay_one(&pre_root, processed, block_data, held, meter, logged, work)
                            .await
                    })
                    .await
                    .map_err(|e| invalid(format!("an envelope replay stopped: {e}")))?
                }
            })
            .buffered(self.workers)
            .try_collect()
            .await?;
        let logged_all: Vec<Refusal> = logged_runs.into_iter().flatten().collect();
        let reached = effects
            .iter()
            .flat_map(|e| e.metered.reached.iter().map(|(k, v)| (k.clone(), *v)))
            .collect();
        accept_metered(
            &|wallet| purses.free(wallet).unwrap_or(0),
            &logged_all,
            &ledger.drawn(),
            &reached,
        )
        .map_err(|e| invalid(format!("the membrane stops do not replay: {e}")))?;
        self.finish(pre_root, inputs, gate, effects, &block_data, PhaseTimings::default())
            .await
    }
}

pub(crate) fn gate_inputs<'a>(
    deploys: impl Iterator<Item = &'a Signed<DeployData>>,
) -> Result<Vec<GateInput>, CasperError> {
    deploys
        .map(|d| {
            let (named, sealed, bare_stack) = envelope_signatures(d);
            GateInput::from_deploy(d, named)
                .map(|input| input.with_sealed(sealed).with_bare_stack(bare_stack))
        })
        .collect()
}

enum EnvelopeSignatures {
    Unnormalized,
    Sealed(WalletOwner, bool),
    Named(Vec<WalletOwner>, bool),
}

fn envelope_signatures(deploy: &Signed<DeployData>) -> (Vec<WalletOwner>, Vec<WalletOwner>, bool) {
    match classify_envelope(deploy) {
        EnvelopeSignatures::Unnormalized => (Vec::new(), Vec::new(), false),
        EnvelopeSignatures::Sealed(process, bare_stack) => (Vec::new(), vec![process], bare_stack),
        EnvelopeSignatures::Named(named, bare_stack) => (named, Vec::new(), bare_stack),
    }
}

fn classify_envelope(deploy: &Signed<DeployData>) -> EnvelopeSignatures {
    let Some(term) = normalizer_env_from_deploy(deploy)
        .ok()
        .and_then(|env| Compiler::source_to_adt_with_normalizer_env(&deploy.data.term, env).ok())
    else {
        return EnvelopeSignatures::Unnormalized;
    };
    let bare_stack = has_bare_stack(&term);
    match self_sealed_envelope(&term) {
        Some(process) => EnvelopeSignatures::Sealed(process, bare_stack),
        None => EnvelopeSignatures::Named(named_owners(&term), bare_stack),
    }
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
    slashes: Arc<dyn SlashSource>,
    conservation: Option<Arc<Conservation>>,
    close_charge: CloseChargeSource,
}

#[derive(Clone, Copy)]
enum CloseChargeSource {
    Off,
    Fixed(super::close_charge::CloseCharge),
    Consensus,
}

impl BlockExecutor {
    async fn close_gate(
        &self,
        pre_root: &StateHash,
        executed: &ExecutedBlock,
        logged: Option<&[Refusal]>,
        close: Option<super::close_charge::CloseCharge>,
    ) -> Result<(SystemGate, Arc<MembraneMeter>, Arc<dyn Funding>), CasperError> {
        let reader = StoreReader::at(&self.runtime_manager, pre_root).await?;
        let purses = Arc::new(BlockPurses::new(Box::new(reader), executed.gate.table.clone()));
        let funding = CloseFunding::new(purses, executed.metered.iter().map(|m| &m.drawn));
        let meter = match logged {
            Some(logged) => MembraneMeter::replay(funding.clone(), BlockLedger::new(), logged),
            None => MembraneMeter::play(funding.clone(), BlockLedger::new()),
        };
        let gate = SystemGate {
            funding: Some(meter.clone()),
            close,
        };
        Ok((gate, meter, funding))
    }

    pub fn new(runtime_manager: Arc<RuntimeManager>, workers: usize) -> Self {
        BlockExecutor {
            envelopes: EnvelopeExecutor::new(Arc::clone(&runtime_manager), workers),
            runtime_manager,
            slashes: Arc::new(NoSlashing),
            conservation: None,
            close_charge: CloseChargeSource::Off,
        }
    }

    async fn close_charge_at(
        &self,
        pre_root: &StateHash,
    ) -> Result<Option<super::close_charge::CloseCharge>, CasperError> {
        match self.close_charge {
            CloseChargeSource::Off => Ok(None),
            CloseChargeSource::Fixed(close) => Ok(Some(close)),
            CloseChargeSource::Consensus => {
                super::close_charge::CloseCharge::at(&self.runtime_manager, pre_root)
                    .await
                    .map(Some)
            }
        }
    }

    pub async fn execute_timed(
        &self,
        pre_root: &StateHash,
        block: &BlockMessage,
    ) -> Result<(BlockRecord, BlockTimings), CasperError> {
        require_no_slashes(&self.slashes.slashes(block)?)?;
        let envelopes = block
            .body
            .deploys
            .iter()
            .map(|processed| processed.deploy.clone())
            .collect();
        let close = self.close_charge_at(pre_root).await?;
        let executed = self
            .envelopes
            .execute_held(
                pre_root,
                envelopes,
                block_data(block),
                close.map_or(0, |close| close.pre_charge),
            )
            .await?;
        let started = std::time::Instant::now();
        let (gate, close_meter, _) = self.close_gate(pre_root, &executed, None, close).await?;
        let (post_root, _, system_deploys) = self
            .runtime_manager
            .compute_state_gated(
                &executed.settled_root,
                vec![],
                vec![close_block(block)],
                block_data(block),
                None,
                Some(gate),
            )
            .await?;
        let close_refusals = close_meter.take().refusals;
        let system_deploys: Vec<_> = system_deploys
            .into_iter()
            .map(|deploy| super::stops::with_refusals(deploy, &close_refusals))
            .collect();
        let close_block_time = started.elapsed();
        let started = std::time::Instant::now();
        self.audit(block, &executed.settlement, &post_root).await;
        let audit = started.elapsed();
        let started = std::time::Instant::now();
        let record = BlockRecord::new(
            block.block_hash.clone(),
            pre_root.clone(),
            executed.mid_root.clone(),
            post_root,
            entries(&executed),
            system_deploys,
        )
        .and_then(|record| record.with_gate(executed.gate.entries.clone()))
        .map(|record| record.with_settlement(executed.settlement.clone()))
        .map_err(CasperError::RuntimeError)?;
        let timings = BlockTimings {
            envelopes: executed.timings,
            close_block: close_block_time,
            audit,
            record: started.elapsed(),
        };
        Ok((record, timings))
    }

    pub fn with_close_charge(mut self, close: super::close_charge::CloseCharge) -> Self {
        self.close_charge = CloseChargeSource::Fixed(close);
        self
    }

    pub fn with_consensus_close_charge(mut self) -> Self {
        self.close_charge = CloseChargeSource::Consensus;
        self
    }

    pub fn with_conservation(mut self, conservation: Arc<Conservation>) -> Self {
        self.conservation = Some(conservation);
        self
    }

    async fn audit(&self, block: &BlockMessage, settlement: &Settlement, post_root: &StateHash) {
        let Some(conservation) = &self.conservation else {
            return;
        };
        if let Err(error) = conservation
            .after_block(&self.runtime_manager, block, settlement, post_root)
            .await
        {
            tracing::error!(
                target: "f1r3fly.v6",
                block = %hex::encode(&block.block_hash),
                %error,
                "v6 conservation audit could not read the post-state"
            );
        }
    }

    pub fn with_slash_source(mut self, slashes: Arc<dyn SlashSource>) -> Self {
        self.slashes = slashes;
        self
    }

    pub fn with_boxcar(mut self, boxcar: Boxcar) -> Self {
        self.envelopes = self.envelopes.with_boxcar(boxcar);
        self
    }
}

fn outcome(
    admitted: bool,
    exhausted: bool,
    failed: bool,
    overdrawn: bool,
    merged: bool,
) -> EnvelopeOutcome {
    if !admitted {
        return EnvelopeOutcome::NotRun;
    }
    if overdrawn && !failed {
        return EnvelopeOutcome::Overdrawn;
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
        .zip(&executed.overdrawn)
        .zip(&executed.gate.entries)
        .zip(&executed.footprints)
        .zip(&executed.surfaces)
        .zip(&executed.work)
        .map(|((((((run, merged), overdrawn), gate), footprint), surfaces), work)| RecordEntry {
            executed: ExecutedEnvelope {
                envelope_id: run.envelope_sig.clone(),
                outcome: outcome(
                    gate.admitted(),
                    gate.delta > 0
                        && own_cells(surfaces, &run.envelope_sig).saturating_add(run.compute_phlo)
                            >= gate.delta,
                    run.failed,
                    *overdrawn,
                    *merged,
                ),
                comm_phlo: run.comm_phlo,
                compute_phlo: run.compute_phlo,
                user_log: run.user_log.clone(),
                footprint: footprint.clone(),
                surfaces: surfaces.clone(),
                work: work.clone(),
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
        self.execute_timed(pre_root, block)
            .await
            .map(|(record, _)| record)
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
                    is_failed: matches!(
                        entry.outcome,
                        EnvelopeOutcome::UserFailure | EnvelopeOutcome::Exhausted
                    ),
                    system_deploy_error: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let close = self.close_charge_at(record.pre_root()).await?;
        let replayed = self
            .envelopes
            .replay_held(
                record.pre_root(),
                processed,
                Some(recorded.iter().map(|entry| entry.work.clone()).collect()),
                record.gate(),
                block_data(block),
                close.map_or(0, |close| close.pre_charge),
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
        let close_logged = super::stops::system_refusals(record.system_deploys())
            .map_err(|e| mismatch(&format!("a close refusal does not decode: {e}")))?;
        let (gate, close_meter, close_funding) = self
            .close_gate(record.pre_root(), &replayed, Some(&close_logged), close)
            .await?;
        let post_root = self
            .runtime_manager
            .replay_compute_state_gated(
                &replayed.settled_root,
                vec![],
                record.system_deploys().to_vec(),
                &block_data(block),
                None,
                false,
                Some(gate),
            )
            .await?;
        if post_root != *record.post_root() {
            return Err(mismatch("post-state root"));
        }
        let closed = close_meter.take();
        accept_metered(
            &|wallet| close_funding.limit(wallet),
            &close_logged,
            &closed.drawn,
            &closed.reached,
        )
        .map_err(|e| mismatch(&format!("the close's membrane stops: {e}")))?;
        self.audit(block, &replayed.settlement, &post_root).await;
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
