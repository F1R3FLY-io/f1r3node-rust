use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use block_storage::rust::deploy::key_value_deploy_storage::{
    EnvelopeRejection, KeyValueDeployStorage,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, F1r3flyState, Header, ProcessedUserDeploy,
};
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_id::DeployLookupId;
use prost::bytes::Bytes;
use shared::rust::store::key_value_store::KvStoreError;

use super::backend::ExecutionBackend;
use super::block_record::BlockRecord;
use super::body::{Attestation, StateHash, V6Body, V6Records};
use super::core::{Action, CoreParams, EmittedBlock, ExecutorCore};
use super::evidence::{Evidence, EvidenceSink, MemoryEvidenceSink};
use super::executor::BlockExecutor;
use super::gate::{screen, BlockGate, RhoBlockGate};
use super::lag::{Lag, LagGate, Transition};
use super::linearizer::{Advance, Linearizer};
use super::propose::{select_envelopes, PoolEntry, SelectionContext};
use super::record::ExecutionRecord;
use super::store::{LmdbRecordStore, MemoryRecordStore, Opened, RecordStore, Retention};
use super::validators::{AttestedRootValidators, ValidatorSetSource};
use super::validity::{check_structure, Limits};
use crate::rust::blocks::proposer::propose_result::BlockCreatorResult;
use crate::rust::blocks::proposer::proposer::DeploySelection;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::util::proto_util;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validator_identity::ValidatorIdentity;

pub struct V6Parts<R: ExecutionRecord> {
    pub backend: Arc<dyn ExecutionBackend<Record = R>>,
    pub gate: Arc<dyn BlockGate>,
    pub validators: Arc<dyn ValidatorSetSource>,
    pub evidence: Arc<dyn EvidenceSink>,
    pub store: Option<Box<dyn RecordStore<R>>>,
    pub limits: Limits,
    pub params: CoreParams,
    pub lag_threshold: u64,
}

pub struct V6Engine<R: ExecutionRecord = BlockRecord> {
    core: parking_lot::Mutex<ExecutorCore<R>>,
    linearizer: parking_lot::Mutex<Linearizer>,
    backend: Arc<dyn ExecutionBackend<Record = R>>,
    gate: Arc<dyn BlockGate>,
    validators: Arc<dyn ValidatorSetSource>,
    evidence: Arc<dyn EvidenceSink>,
    block_store: KeyValueBlockStore,
    dag_storage: BlockDagKeyValueStorage,
    deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    limits: Limits,
    lag_gate: LagGate,
    anchor: (BlockHash, i64),
    wake: tokio::sync::Notify,
    started: AtomicBool,
}

impl V6Engine<BlockRecord> {
    pub fn new(
        genesis: &BlockMessage,
        runtime_manager: Arc<RuntimeManager>,
        policy: Arc<AdoptedResourcePolicy>,
        block_store: KeyValueBlockStore,
        dag_storage: BlockDagKeyValueStorage,
        deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
        lag_threshold: u64,
    ) -> Result<Arc<Self>, CasperError> {
        let (store, opened) = LmdbRecordStore::<BlockRecord>::open(
            dag_storage.v6_executor_state(),
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
        }
        let backend = Arc::new(BlockExecutor::new(
            runtime_manager.clone(),
            Arc::clone(&policy),
        ));
        let gate = Arc::new(RhoBlockGate::new(runtime_manager.clone(), policy));
        let parts = V6Parts {
            backend,
            gate,
            validators: Arc::new(AttestedRootValidators::new(runtime_manager)),
            evidence: Arc::new(MemoryEvidenceSink::new()),
            store: Some(Box::new(store)),
            limits: Limits {
                ordered_only: true,
                ..Limits::default()
            },
            params: CoreParams::default(),
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
        deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    ) -> Arc<Self> {
        let V6Parts {
            backend,
            gate,
            validators,
            evidence,
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
            core: parking_lot::Mutex::new(ExecutorCore::with_store(
                genesis.block_hash.clone(),
                params,
                store,
            )),
            linearizer: parking_lot::Mutex::new(Linearizer::new(
                genesis.block_hash.clone(),
                genesis.body.state.block_number,
            )),
            backend,
            gate,
            validators,
            evidence,
            block_store,
            dag_storage,
            deploy_storage,
            limits,
            lag_gate: LagGate::new(lag_threshold),
            anchor: (genesis.block_hash.clone(), genesis.body.state.block_number),
            wake: tokio::sync::Notify::new(),
            started: AtomicBool::new(false),
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

    pub fn wake(&self) { self.wake.notify_one(); }

    pub fn attestation(&self) -> Attestation { self.core.lock().attestation() }

    pub fn executed(&self, block: &BlockHash) -> Option<Bytes> { self.core.lock().executed(block) }

    pub fn record(&self, block: &BlockHash) -> Option<R> { self.core.lock().record(block) }

    fn quarantine(
        &self,
        refused: &[(ProcessedUserDeploy, String)],
        root: &StateHash,
        block_number: i64,
    ) -> Result<(), CasperError> {
        let mut storage = self.deploy_storage.lock();
        for (deploy, cause) in refused {
            let Some(envelope) = deploy.as_ordered() else {
                continue;
            };
            let DeployLookupId::V6(deploy_id) = envelope.identity() else {
                continue;
            };
            storage.quarantine_envelope(EnvelopeRejection {
                deploy_id: *deploy_id,
                pre_state_root: Some(root.to_vec()),
                block_number,
                reason: cause.clone(),
            })?;
        }
        Ok(())
    }

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

    pub fn canonical_len(&self) -> usize { self.core.lock().canonical_len() }

    pub fn lag_gate_engaged(&self) -> bool { self.lag_gate.engaged() }

    fn admits_envelopes(&self, snapshot: &CasperSnapshot, attestation: &Attestation) -> bool {
        let lfb = snapshot.dag.last_finalized_block();
        let lag = Lag::measure(
            snapshot.dag.block_number(&lfb),
            snapshot.dag.block_number(&attestation.block_hash),
        );
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
            self.core
                .lock()
                .observe_attestation(&block.block_hash, &block.sender, attestation);
        }
        self.report_evidence();
    }

    fn load(&self, hash: &BlockHash) -> Result<BlockMessage, CasperError> {
        self.block_store
            .get(hash)?
            .ok_or_else(|| CasperError::KvStoreError(KvStoreError::KeyNotFound(hex::encode(hash))))
    }

    pub async fn tick(&self) -> Result<bool, CasperError> {
        let dag = self.dag_storage.get_representation()?;
        let lfb = dag.last_finalized_block();
        let advance = self.linearizer.lock().advance(&dag, &lfb)?;
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
                            .map(|d| Bytes::copy_from_slice(d.identity_bytes()))
                            .collect(),
                        records,
                    });
                    progressed = true;
                }
            }
            Advance::Hold { previous, proposed } => {
                tracing::warn!(
                    target: "f1r3fly.v6",
                    previous = %hex::encode(&previous),
                    proposed = %hex::encode(&proposed),
                    "new LFB is not on the main chain of the previous LFB; holding the linearizer"
                );
            }
            Advance::Unchanged => {}
        }
        if self.core.lock().diverged() {
            tracing::warn!(
                target: "f1r3fly.v6",
                "the re-emitted order disagrees with the stored canonical prefix; rebuilding from the anchor"
            );
            self.core
                .lock()
                .reset()
                .map_err(CasperError::RuntimeError)?;
            *self.linearizer.lock() = Linearizer::new(self.anchor.0.clone(), self.anchor.1);
            return Ok(true);
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

        let envelopes = if self.admits_envelopes(snapshot, &attestation) {
            let pool = self
                .deploy_storage
                .lock()
                .read_all_envelopes(offered_funded_v6_limits().envelope)?
                .into_iter()
                .map(PoolEntry::from);
            let in_scope = |sig: &Bytes| snapshot.deploys_in_scope.contains(sig);
            let selected = select_envelopes(pool, &SelectionContext {
                next_block_number,
                deploy_lifespan: conf.deploy_lifespan,
                now_millis,
                shard_id: &conf.shard_name,
                min_phlo_price: conf.min_phlo_price,
                in_scope: &in_scope,
                limits: &self.limits,
            });
            let (envelopes, refused) = screen(
                self.gate.as_ref(),
                &attestation.root,
                now_millis,
                selected
                    .into_iter()
                    .map(PoolEntry::into_processed)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(CasperError::RuntimeError)?,
            )
            .await?;
            self.quarantine(&refused, &attestation.root, next_block_number)?;
            envelopes
        } else {
            Vec::new()
        };
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
