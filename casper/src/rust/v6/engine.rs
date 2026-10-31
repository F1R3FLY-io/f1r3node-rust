use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::offer_rejection::OfferRejection;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, F1r3flyState, Header, ProcessedDeploy,
};
use prost::bytes::Bytes;
use shared::rust::store::key_value_store::KvStoreError;

use super::backend::ExecutionBackend;
use super::block_record::BlockRecord;
use super::body::{Attestation, StateHash, V6Body, V6Records};
use super::core::{Action, CoreParams, EmittedBlock, ExecutorCore, InclusionCheck};
use super::evidence::{Evidence, EvidenceSink, MemoryEvidenceSink, Offence};
use super::inclusion::{screen_offers, GateScreen, InclusionScreen};
use super::executor::BlockExecutor;
use super::lag::{Lag, LagGate, Transition};
use super::linearizer::{Advance, Linearizer};
use super::price::{effective_price, FixedPriceHook, NodePriceConf, PriceHook};
use super::propose::{pack_records, select_envelopes, SelectionContext};
use super::record::ExecutionRecord;
use super::resource_policy::ShardResourcePolicy;
use super::store::{
    LmdbRecordStore, MemoryRecordStore, Opened, RecordSource, RecordStore, Retention,
};
use super::validators::{AttestedRootValidators, ValidatorSetSource};
use super::validity::{check_structure, Limits};
use crate::rust::blocks::proposer::propose_result::BlockCreatorResult;
use crate::rust::blocks::proposer::proposer::DeploySelection;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::util::proto_util;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validator_identity::ValidatorIdentity;

pub struct V6Parts<R: ExecutionRecord> {
    pub backend: Arc<dyn ExecutionBackend<Record = R>>,
    pub validators: Arc<dyn ValidatorSetSource>,
    pub evidence: Arc<dyn EvidenceSink>,
    pub inclusion: Arc<dyn InclusionScreen>,
    pub store: Option<Box<dyn RecordStore<R>>>,
    pub limits: Limits,
    pub params: CoreParams,
    pub lag_threshold: u64,
}

pub struct V6Engine<R: ExecutionRecord = BlockRecord> {
    core: parking_lot::Mutex<ExecutorCore<R>>,
    linearizer: parking_lot::Mutex<Linearizer>,
    backend: Arc<dyn ExecutionBackend<Record = R>>,
    validators: Arc<dyn ValidatorSetSource>,
    evidence: Arc<dyn EvidenceSink>,
    inclusion: Arc<dyn InclusionScreen>,
    block_store: KeyValueBlockStore,
    dag_storage: BlockDagKeyValueStorage,
    deploy_storage: Arc<KeyValueDeployStorage>,
    limits: Limits,
    lag_gate: LagGate,
    price_hook: parking_lot::RwLock<Arc<dyn PriceHook>>,
    wake: tokio::sync::Notify,
    started: AtomicBool,
    held: parking_lot::Mutex<Option<BlockHash>>,
}

impl V6Engine<BlockRecord> {
    pub fn new(
        genesis: &BlockMessage,
        runtime_manager: Arc<RuntimeManager>,
        block_store: KeyValueBlockStore,
        dag_storage: BlockDagKeyValueStorage,
        deploy_storage: Arc<KeyValueDeployStorage>,
        lag_threshold: u64,
        fallback_distance: usize,
        policy: ShardResourcePolicy,
        retained_records: usize,
    ) -> Result<Arc<Self>, CasperError> {
        let source: Arc<dyn RecordSource<BlockRecord>> = Arc::new(block_store.clone());
        let (store, opened) = LmdbRecordStore::<BlockRecord>::open(
            dag_storage.v6_executor_state(),
            source,
            genesis.block_hash.clone(),
            genesis.body.state.post_state_hash.clone(),
        )?;
        match opened {
            Opened::Resumed => tracing::info!(
                target: "f1r3fly.v6",
                canonical = store.canonical_len(),
                "v6 executor resumes its stored canonical prefix"
            ),
            Opened::Fresh => {}
            Opened::ReplacedOtherAnchor => tracing::warn!(
                target: "f1r3fly.v6",
                anchor = %hex::encode(&genesis.block_hash),
                "v6 executor state belonged to another anchor; starting from this one"
            ),
            Opened::ReplacedOlderFormat => tracing::warn!(
                target: "f1r3fly.v6",
                "v6 executor state was in an older format; starting afresh"
            ),
        }
        let workers = std::thread::available_parallelism()
            .map(|cores| cores.get())
            .unwrap_or(1);
        let backend = Arc::new(BlockExecutor::new(runtime_manager.clone(), workers));
        let parts = V6Parts {
            backend,
            validators: Arc::new(AttestedRootValidators::new(runtime_manager.clone())),
            evidence: Arc::new(MemoryEvidenceSink::new()),
            inclusion: Arc::new(GateScreen::new(runtime_manager)),
            store: Some(Box::new(store)),
            limits: policy.limits(),
            params: CoreParams {
                fallback_distance,
                retained_records,
                ..CoreParams::default()
            },
            lag_threshold,
        };
        Ok(Self::with_parts(
            genesis,
            parts,
            block_store,
            dag_storage,
            deploy_storage,
        ))
    }
}

impl<R: ExecutionRecord> V6Engine<R> {
    pub fn with_parts(
        genesis: &BlockMessage,
        parts: V6Parts<R>,
        block_store: KeyValueBlockStore,
        dag_storage: BlockDagKeyValueStorage,
        deploy_storage: Arc<KeyValueDeployStorage>,
    ) -> Arc<Self> {
        let V6Parts {
            backend,
            validators,
            evidence,
            inclusion,
            store,
            limits,
            params,
            lag_threshold,
        } = parts;
        let store = store.unwrap_or_else(|| {
            Box::new(MemoryRecordStore::new(
                genesis.block_hash.clone(),
                genesis.body.state.post_state_hash.clone(),
                Retention::default(),
            ))
        });
        Arc::new(V6Engine {
            core: parking_lot::Mutex::new(ExecutorCore::with_store(params, store)),
            linearizer: parking_lot::Mutex::new(Linearizer::new()),
            backend,
            validators,
            evidence,
            inclusion,
            block_store,
            dag_storage,
            deploy_storage,
            limits,
            lag_gate: LagGate::new(lag_threshold),
            price_hook: parking_lot::RwLock::new(Arc::new(
                FixedPriceHook::from_conf(&NodePriceConf::default())
                    .expect("the default price configuration is valid"),
            )),
            wake: tokio::sync::Notify::new(),
            started: AtomicBool::new(false),
            held: parking_lot::Mutex::new(None),
        })
    }

    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let engine = self.clone();
        tokio::spawn(async move {
            loop {
                loop {
                    match engine.tick().await {
                        Ok(true) => continue,
                        Ok(false) => break,
                        Err(error) => {
                            tracing::warn!(target: "f1r3fly.v6", %error, "v6 executor step failed");
                            break;
                        }
                    }
                }
                engine.wake.notified().await;
            }
        });
    }

    pub fn set_price_hook(&self, hook: Arc<dyn PriceHook>) { *self.price_hook.write() = hook; }

    pub fn wake(&self) { self.wake.notify_one(); }

    pub fn attestation(&self) -> Attestation { self.core.lock().attestation() }

    pub fn executed(&self, block: &BlockHash) -> Option<Bytes> { self.core.lock().executed(block) }

    pub fn record(&self, block: &BlockHash) -> Option<R> { self.core.lock().record(block) }

    pub fn evidence(&self) -> Vec<Evidence> {
        self.report_evidence();
        self.evidence.evidence()
    }

    fn report_evidence(&self) {
        let found = self.core.lock().take_evidence();
        for evidence in found {
            self.evidence.report(evidence);
        }
    }

    pub fn record_rejection(
        &self,
        deploy_sig: &[u8],
        pre_state_root: &StateHash,
        block_number: i64,
        reason: &str,
    ) -> Result<bool, CasperError> {
        Ok(self.deploy_storage.record_rejection(OfferRejection {
            deploy_id: deploy_sig.to_vec(),
            pre_state_root: Some(pre_state_root.to_vec()),
            block_number,
            reason: reason.to_string(),
        })?)
    }

    pub fn canonical_len(&self) -> usize { self.core.lock().canonical_len() }

    pub fn resident(&self) -> usize { self.core.lock().resident() }

    pub fn lag_gate_engaged(&self) -> bool { self.lag_gate.engaged() }

    fn admits_envelopes(&self, snapshot: &CasperSnapshot, attestation: &Attestation) -> bool {
        let lfb = snapshot.dag.last_finalized_block();
        let lag = Lag::measure(
            snapshot.dag.block_number(&lfb),
            snapshot.dag.block_number(&attestation.block_hash),
        );
        let lfb_number = snapshot.dag.block_number(&lfb);
        if let Some(lfb_number) = lfb_number {
            let floor_distance = snapshot.max_block_num - lfb_number;
            if crate::rust::util::rholang::interpreter_util::floor_pacing_engaged(
                floor_distance,
                snapshot.on_chain_state.shard_conf.proposal_pacing_floor_distance,
            ) {
                metrics::counter!(
                    crate::rust::metrics_constants::FLOOR_PACING_ENGAGED_METRIC,
                    "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
                )
                .increment(1);
                tracing::info!(
                    target: "f1r3fly.v6",
                    floor_distance,
                    "finality lags the newest block past the pacing threshold; proposing without envelopes"
                );
                return false;
            }
        }
        let (admits, transition) = self.lag_gate.admits(lag);
        let threshold = self.lag_gate.threshold();
        match (lag, transition) {
            (Lag::Unknown, _) => tracing::warn!(
                target: "f1r3fly.v6",
                lfb = %hex::encode(&lfb),
                executed = %hex::encode(&attestation.block_hash),
                "execution lag is unknown; this proposal carries no envelopes"
            ),
            (Lag::Blocks(blocks), Transition::Engaged) => tracing::warn!(
                target: "f1r3fly.v6",
                lag = blocks,
                threshold,
                "execution-lag gate engaged: proposals carry no envelopes until execution catches up"
            ),
            (Lag::Blocks(blocks), Transition::Released) => tracing::info!(
                target: "f1r3fly.v6",
                lag = blocks,
                threshold,
                "execution-lag gate released: proposals carry envelopes again"
            ),
            _ => {}
        }
        admits
    }

    pub fn check_block(
        &self,
        block: &BlockMessage,
        dag: &KeyValueDagRepresentation,
    ) -> Result<(V6Body, Vec<R>), String> {
        check_structure::<R>(block, &self.limits, &|hash| dag.block_number(hash))
    }

    pub fn observe_block(&self, block: &BlockMessage, body: &V6Body) {
        if let Some(attestation) = &body.attestation {
            self.core.lock().observe_attestation(
                &block.block_hash,
                &block.sender,
                attestation,
                !block.body.deploys.is_empty(),
            );
        }
        self.report_evidence();
    }

    async fn check_inclusion(&self, check: InclusionCheck) -> Result<(), CasperError> {
        let block = self.load(&check.block)?;
        let started = std::time::Instant::now();
        let screened = self
            .inclusion
            .refused(&check.root, block.header.timestamp, &block.body.deploys)
            .await;
        let refused = match screened {
            Ok(refused) => refused,
            Err(error) => {
                tracing::warn!(
                    target: "f1r3fly.v6",
                    block = %hex::encode(&check.block),
                    %error,
                    "v6 inclusion check could not run; the block is not checked"
                );
                return Ok(());
            }
        };
        tracing::debug!(
            target: "f1r3fly.v6",
            block = %hex::encode(&check.block),
            envelopes = block.body.deploys.len(),
            refused = refused.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "v6 inclusion check"
        );
        for (envelope_id, cause) in refused {
            self.evidence.report(Evidence {
                offender: check.sender.clone(),
                citer: check.block.clone(),
                target: check.attested.clone(),
                offence: Offence::InclusionViolation {
                    envelope_id,
                    cause,
                },
            });
        }
        Ok(())
    }

    fn load(&self, hash: &BlockHash) -> Result<BlockMessage, CasperError> {
        self.block_store
            .get(hash)?
            .ok_or_else(|| CasperError::KvStoreError(KvStoreError::KeyNotFound(hex::encode(hash))))
    }

    pub async fn tick(&self) -> Result<bool, CasperError> {
        let dag = self.dag_storage.get_representation()?;
        let lfb = dag.last_finalized_block();
        let advance = {
            let core = self.core.lock();
            let emitted = |block: &BlockHash| core.is_emitted(block);
            self.linearizer.lock().advance(&dag, &lfb, &emitted)?
        };
        let holding = matches!(advance, Advance::Hold { .. });
        let mut progressed = false;
        match advance {
            Advance::Emitted(blocks) => {
                for hash in blocks {
                    let block = self.load(&hash)?;
                    let records = block
                        .body
                        .v6
                        .as_ref()
                        .and_then(|b| b.decode_records::<R>().ok())
                        .unwrap_or_default();
                    self.core.lock().emit(EmittedBlock {
                        hash,
                        sender: block.sender.clone(),
                        envelope_ids: block
                            .body
                            .deploys
                            .iter()
                            .map(|d| d.deploy.sig.clone())
                            .collect(),
                        records,
                    });
                    progressed = true;
                }
            }
            Advance::Hold { previous, proposed } => {
                let mut held = self.held.lock();
                if held.as_ref() != Some(&proposed) {
                    metrics::counter!(
                        crate::rust::metrics_constants::V6_LINEARIZER_ALIN_HOLDS_METRIC,
                        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
                    )
                    .increment(1);
                    tracing::error!(
                        target: "f1r3fly.v6",
                        previous = %hex::encode(&previous),
                        proposed = %hex::encode(&proposed),
                        "A-LIN violation: the new LFB is not on the main chain of the previous LFB; holding the linearizer"
                    );
                    *held = Some(proposed);
                }
            }
            Advance::Unchanged => {}
        }
        if !holding {
            *self.held.lock() = None;
        }
        let (action, fixed_before) = {
            let mut core = self.core.lock();
            let before = core.canonical_len();
            (core.next_action(), before)
        };
        if self.core.lock().canonical_len() != fixed_before {
            progressed = true;
        }
        match action {
            Action::Idle => {}
            Action::Replay {
                target,
                citer,
                record,
            } => {
                let block = self.load(&target)?;
                let result = self
                    .backend
                    .replay(&record, &block)
                    .await
                    .map_err(|e| e.to_string());
                if let Err(error) = &result {
                    tracing::warn!(
                        target: "f1r3fly.v6",
                        block = %hex::encode(&target),
                        citer = %hex::encode(&citer),
                        %error,
                        "v6 record replay failed"
                    );
                }
                self.core
                    .lock()
                    .apply_replay(&target, &citer, &record, result);
                progressed = true;
            }
            Action::Execute { target, pre_root } => {
                let block = self.load(&target)?;
                let record = self.backend.execute(&pre_root, &block).await?;
                self.core.lock().apply_execute(record);
                progressed = true;
            }
        }
        let check = self.core.lock().next_inclusion_check();
        if let Some(check) = check {
            self.check_inclusion(check).await?;
            progressed = true;
        }
        self.report_evidence();
        if let Some(error) = self.core.lock().take_store_error() {
            return Err(CasperError::RuntimeError(format!(
                "the v6 executor could not persist a fixed record: {error}"
            )));
        }
        Ok(progressed)
    }

    pub async fn create_block(
        &self,
        snapshot: &CasperSnapshot,
        validator: &ValidatorIdentity,
        selection: DeploySelection,
    ) -> Result<BlockCreatorResult, CasperError> {
        let conf = &snapshot.on_chain_state.shard_conf;
        let self_key: Bytes = validator.public_key.bytes.clone();
        let next_block_number = snapshot.max_block_num + 1;
        let seq_num = snapshot
            .max_seq_nums
            .get(&self_key)
            .map(|s| *s as i32 + 1)
            .unwrap_or(1);
        let mut now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if let Some(max_parent) = snapshot.parents.iter().map(|p| p.header.timestamp).max() {
            now_millis = now_millis.max(max_parent);
        }

        let (attestation, records) = {
            let core = self.core.lock();
            (
                core.attestation(),
                core.records_to_cite(&self_key, self.limits.max_records),
            )
        };
        let records: Vec<_> = records
            .into_iter()
            .filter(|r| {
                snapshot
                    .dag
                    .block_number(r.target())
                    .map(|n| n < next_block_number)
                    .unwrap_or(false)
            })
            .collect();
        let records = pack_records(records, &self.limits);

        let envelopes: Vec<ProcessedDeploy> = if self.admits_envelopes(snapshot, &attestation) {
            let pool = self.deploy_storage.read_all()?;
            let in_scope = |sig: &Bytes| snapshot.deploys_in_scope.contains(sig);
            select_envelopes(pool, &SelectionContext {
                next_block_number,
                deploy_lifespan: conf.deploy_lifespan,
                now_millis,
                shard_id: &conf.shard_name,
                min_phlo_price: effective_price(&**self.price_hook.read(), conf.min_phlo_price),
                in_scope: &in_scope,
                limits: &self.limits,
            })
            .into_iter()
            .map(ProcessedDeploy::empty)
            .collect()
        } else {
            Vec::new()
        };
        let envelopes = screen_offers(
            &*self.inclusion,
            &self.deploy_storage,
            envelopes,
            &attestation.root,
            next_block_number,
            now_millis,
        )
        .await?;
        if envelopes.is_empty() && records.is_empty() && !selection.allows_empty() {
            return Ok(BlockCreatorResult::NoNewDeploys);
        }

        let bonds = match self.validators.bonds(&attestation, next_block_number).await {
            Ok(bonds) => bonds,
            Err(_) => {
                let mut bonds: Vec<Bond> = snapshot
                    .on_chain_state
                    .bonds_map
                    .iter()
                    .map(|(validator, stake)| Bond {
                        validator: validator.clone(),
                        stake: *stake,
                    })
                    .collect();
                bonds.sort_by(|a, b| a.validator.cmp(&b.validator));
                bonds
            }
        };

        let extension = V6Body::with_records(attestation.clone(), &records);
        let body = Body {
            state: F1r3flyState {
                pre_state_hash: attestation.root.clone(),
                post_state_hash: attestation.root.clone(),
                bonds,
                block_number: next_block_number,
            },
            deploys: envelopes,
            rejected_deploys: vec![],
            system_deploys: vec![],
            extra_bytes: Bytes::new(),
            applied_from_scope: vec![],
            merge_base: snapshot
                .parents
                .first()
                .map(|p| p.block_hash.clone())
                .unwrap_or_default(),
            v6: Some(extension),
        };
        let header = Header {
            parents_hash_list: snapshot
                .parents
                .iter()
                .map(|p| p.block_hash.clone())
                .collect(),
            timestamp: now_millis,
            version: conf.casper_version,
            extra_bytes: Bytes::new(),
        };
        let unsigned = proto_util::unsigned_block_proto(
            body,
            header,
            snapshot.justifications.iter().cloned().collect(),
            conf.shard_name.clone(),
            Some(seq_num),
        );
        let block = validator.sign_block(&unsigned);
        self.core.lock().mark_cited(&records);
        Ok(BlockCreatorResult::Created(
            block,
            attestation.root.clone(),
            attestation.root,
        ))
    }
}
