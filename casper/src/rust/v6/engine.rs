use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, F1r3flyState, Header, ProcessedDeploy,
};
use prost::bytes::Bytes;
use shared::rust::store::key_value_store::KvStoreError;

use super::backend::ExecutionBackend;
use super::block_record::BlockRecord;
use super::body::{Attestation, V6Body};
use super::core::{Action, CoreParams, EmittedBlock, Evidence, ExecutorCore};
use super::executor::BlockExecutor;
use super::linearizer::{Advance, Linearizer};
use super::propose::{select_envelopes, SelectionContext};
use super::record::ExecutionRecord;
use super::resource_policy::ShardResourcePolicy;
use super::validity::{check_structure, Limits};
use crate::rust::blocks::proposer::propose_result::BlockCreatorResult;
use crate::rust::blocks::proposer::proposer::DeploySelection;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::finality::floor::floor_committee;
use crate::rust::util::proto_util;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validator_identity::ValidatorIdentity;

pub struct V6Engine<R: ExecutionRecord = BlockRecord> {
    core: parking_lot::Mutex<ExecutorCore<R>>,
    linearizer: parking_lot::Mutex<Linearizer>,
    backend: Arc<dyn ExecutionBackend<Record = R>>,
    block_store: KeyValueBlockStore,
    dag_storage: BlockDagKeyValueStorage,
    deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    runtime_manager: Arc<RuntimeManager>,
    limits: Limits,
    wake: tokio::sync::Notify,
    started: AtomicBool,
}

impl V6Engine<BlockRecord> {
    pub fn new(
        genesis: &BlockMessage,
        runtime_manager: Arc<RuntimeManager>,
        block_store: KeyValueBlockStore,
        dag_storage: BlockDagKeyValueStorage,
        deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    ) -> Arc<Self> {
        let workers = std::thread::available_parallelism()
            .map(|cores| cores.get())
            .unwrap_or(1);
        let backend = Arc::new(BlockExecutor::new(runtime_manager.clone(), workers));
        Self::with_backend(
            genesis,
            backend,
            runtime_manager,
            block_store,
            dag_storage,
            deploy_storage,
            ShardResourcePolicy::default().limits(),
            CoreParams::default(),
        )
    }
}

impl<R: ExecutionRecord> V6Engine<R> {
    #[allow(clippy::too_many_arguments)]
    pub fn with_backend(
        genesis: &BlockMessage,
        backend: Arc<dyn ExecutionBackend<Record = R>>,
        runtime_manager: Arc<RuntimeManager>,
        block_store: KeyValueBlockStore,
        dag_storage: BlockDagKeyValueStorage,
        deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
        limits: Limits,
        params: CoreParams,
    ) -> Arc<Self> {
        Arc::new(V6Engine {
            core: parking_lot::Mutex::new(ExecutorCore::new(
                genesis.block_hash.clone(),
                genesis.body.state.post_state_hash.clone(),
                params,
            )),
            linearizer: parking_lot::Mutex::new(Linearizer::new(
                genesis.block_hash.clone(),
                genesis.body.state.block_number,
            )),
            backend,
            block_store,
            dag_storage,
            deploy_storage,
            runtime_manager,
            limits,
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

    pub fn executed(&self, block: &BlockHash) -> Option<Bytes> {
        self.core.lock().executed(block).cloned()
    }

    pub fn evidence(&self) -> Vec<Evidence> { self.core.lock().evidence().to_vec() }

    pub fn canonical_len(&self) -> usize { self.core.lock().canonical_len() }

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
                .observe_attestation(&block.block_hash, attestation);
        }
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
                    let records = V6Body::decode(&block.body.extra_bytes)
                        .ok()
                        .flatten()
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
                tracing::warn!(
                    target: "f1r3fly.v6",
                    previous = %hex::encode(&previous),
                    proposed = %hex::encode(&proposed),
                    "new LFB is not on the main chain of the previous LFB; holding the linearizer"
                );
            }
            Advance::Unchanged => {}
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

        let pool = self.deploy_storage.lock().read_all()?;
        let in_scope = |sig: &Bytes| snapshot.deploys_in_scope.contains(sig);
        let envelopes: Vec<ProcessedDeploy> = select_envelopes(pool, &SelectionContext {
            next_block_number,
            deploy_lifespan: conf.deploy_lifespan,
            now_millis,
            shard_id: &conf.shard_name,
            min_phlo_price: conf.min_phlo_price,
            in_scope: &in_scope,
            limits: &self.limits,
        })
        .into_iter()
        .map(ProcessedDeploy::empty)
        .collect();
        if envelopes.is_empty() && records.is_empty() && !selection.allows_empty() {
            return Ok(BlockCreatorResult::NoNewDeploys);
        }

        let bonds = match floor_committee(&self.runtime_manager, &attestation.root).await {
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
            extra_bytes: extension.encode(),
            applied_from_scope: vec![],
            merge_base: snapshot
                .parents
                .first()
                .map(|p| p.block_hash.clone())
                .unwrap_or_default(),
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
