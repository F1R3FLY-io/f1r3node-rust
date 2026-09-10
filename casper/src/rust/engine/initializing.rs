// See casper/src/main/scala/coop/rchain/casper/engine/Initializing.scala

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use futures::stream::StreamExt;
use futures::FutureExt;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, BlockMessage, CasperMessage, ProcessedSystemDeploy, StoreItemsMessage,
    StoreItemsMessageRequest, SystemDeployData,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::state::rspace_importer::{RSpaceImporter, RSpaceImporterInstance};
use rspace_plus_plus::rspace::state::rspace_state_manager::RSpaceStateManager;
use shared::rust::shared::f1r3fly_event::F1r3flyEvent;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use shared::rust::ByteString;
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::rust::block_status::ValidBlock;
use crate::rust::blocks::block_processing_queue::{
    BlockProcessingIdentities, BlockProcessingQueueSender,
};
use crate::rust::casper::{CasperShardConf, MultiParentCasper};
use crate::rust::engine::block_retriever::BlockRetriever;
use crate::rust::engine::engine::{
    log_no_approved_block_available, send_no_approved_block_available, transition_to_running,
    Engine,
};
use crate::rust::engine::engine_cell::EngineCell;
use crate::rust::engine::lfs_block_requester::{self, BlockRequesterOps};
use crate::rust::engine::lfs_tuple_space_requester::{self, StatePartPath, TupleSpaceRequesterOps};
use crate::rust::engine::running::RunningRecoveryContext;
use crate::rust::errors::CasperError;
use crate::rust::estimator::Estimator;
use crate::rust::metrics_constants::{
    CASPER_INIT_APPROVED_BLOCK_RECEIVED_METRIC, CASPER_INIT_ATTEMPTS_METRIC,
    CASPER_INIT_RETRY_NO_APPROVED_BLOCK_METRIC, CASPER_INIT_TIME_TO_APPROVED_BLOCK_METRIC,
    CASPER_INIT_TIME_TO_RUNNING_METRIC, CASPER_METRICS_SOURCE,
    INIT_BLOCK_MESSAGE_QUEUE_PENDING_METRIC, INIT_TUPLE_SPACE_QUEUE_PENDING_METRIC,
};
use crate::rust::util::proto_util;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validate::Validate;
use crate::rust::validator_identity::ValidatorIdentity;

const MAX_RESTORE_FAILURES: u64 = 3;
const SYNC_CHANNEL_CAPACITY: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestorePhase {
    Idle,
    Restoring,
    Running,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RestoreLifecycle {
    phase: RestorePhase,
    failures: u64,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RestoreLease(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoreFailureDisposition {
    Retry,
    Terminal,
}

impl RestoreLifecycle {
    fn new() -> Self {
        Self {
            phase: RestorePhase::Idle,
            failures: 0,
            generation: 0,
        }
    }

    fn try_begin(&mut self, valid: bool) -> Option<RestoreLease> {
        if valid && self.phase == RestorePhase::Idle {
            self.generation = self.generation.saturating_add(1);
            self.phase = RestorePhase::Restoring;
            Some(RestoreLease(self.generation))
        } else {
            None
        }
    }

    fn record_failure(&mut self, lease: RestoreLease) -> Option<RestoreFailureDisposition> {
        if self.phase != RestorePhase::Restoring || self.generation != lease.0 {
            return None;
        }
        self.failures = self.failures.saturating_add(1);
        if self.failures >= MAX_RESTORE_FAILURES {
            self.phase = RestorePhase::Terminal;
            Some(RestoreFailureDisposition::Terminal)
        } else {
            Some(RestoreFailureDisposition::Retry)
        }
    }

    fn release_retry(&mut self, lease: RestoreLease) -> bool {
        if self.phase == RestorePhase::Restoring
            && self.generation == lease.0
            && self.failures < MAX_RESTORE_FAILURES
        {
            self.phase = RestorePhase::Idle;
            true
        } else {
            false
        }
    }

    fn commit_running(&mut self, lease: RestoreLease) -> bool {
        if self.phase == RestorePhase::Restoring && self.generation == lease.0 {
            self.phase = RestorePhase::Running;
            true
        } else {
            false
        }
    }

    fn terminate_active(&mut self, lease: RestoreLease) -> bool {
        if self.phase == RestorePhase::Restoring && self.generation == lease.0 {
            self.phase = RestorePhase::Terminal;
            true
        } else {
            false
        }
    }

    fn terminate_retry_request(&mut self, lease: RestoreLease) -> bool {
        if self.phase == RestorePhase::Idle && self.generation == lease.0 {
            self.phase = RestorePhase::Terminal;
            true
        } else {
            false
        }
    }
}

/// Scala equivalent: `class Initializing[F[_]](...) extends Engine[F]`
///
/// Initializing engine makes sure node receives Approved State and transitions to Running after
pub struct Initializing<T: TransportLayer + Send + Sync + Clone + 'static> {
    transport_layer: T,
    rp_conf_ask: RPConf,
    connections_cell: ConnectionsCell,
    last_approved_block: Arc<Mutex<Option<ApprovedBlock>>>,
    block_store: KeyValueBlockStore,
    block_dag_storage: BlockDagKeyValueStorage,
    deploy_storage: KeyValueDeployStorage,
    rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    casper_buffer_storage: CasperBufferKeyValueStorage,
    rspace_state_manager: RSpaceStateManager,

    // Block processing queue - matches Scala's blockProcessingQueue: Queue[F, (Casper[F], BlockMessage)]
    // Using trait object to support different MultiParentCasper implementations
    block_processing_queue_tx: BlockProcessingQueueSender,
    blocks_in_processing: Arc<BlockProcessingIdentities>,
    casper_shard_conf: CasperShardConf,
    required_genesis_signatures: i32,
    validator_id: Option<ValidatorIdentity>,
    the_init: Arc<
        dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> + Send + Sync,
    >,
    block_message_rx: Arc<Mutex<Option<mpsc::Receiver<BlockMessage>>>>,
    tuple_space_rx: Arc<Mutex<Option<mpsc::Receiver<StoreItemsMessage>>>>,
    // Senders to enqueue messages from `handle` (producer side)
    pub block_message_tx: Arc<Mutex<Option<mpsc::Sender<BlockMessage>>>>,
    pub tuple_space_tx: Arc<Mutex<Option<mpsc::Sender<StoreItemsMessage>>>>,
    block_message_queue_pending: Arc<AtomicUsize>,
    tuple_space_queue_pending: Arc<AtomicUsize>,
    trim_state: bool,
    disable_state_exporter: bool,

    restore_lifecycle: Arc<Mutex<RestoreLifecycle>>,
    init_started_at: Arc<Mutex<Option<Instant>>>,
    no_approved_block_retries: Arc<Mutex<u64>>,
    /// Event publisher for F1r3fly events
    event_publisher: F1r3flyEvents,

    block_retriever: BlockRetriever<T>,
    engine_cell: Arc<EngineCell>,
    runtime_manager: Arc<RuntimeManager>,
    estimator: Arc<Mutex<Option<Estimator>>>,
    /// Shared reference to heartbeat signal for triggering immediate wake on deploy
    heartbeat_signal_ref: crate::rust::heartbeat_signal::HeartbeatSignalRef,
    state_items_tx: Option<mpsc::Sender<StoreItemsMessage>>,
}

impl<T: TransportLayer + Send + Sync + Clone> Initializing<T> {
    /// Scala equivalent: Constructor for `Initializing` class
    #[allow(clippy::too_many_arguments)]
    // NOTE: Parameter types adapted to match GenesisValidator changes
    // based on discussion with Steven for TestFixture compatibility
    pub fn new(
        transport_layer: T,
        rp_conf_ask: RPConf,
        connections_cell: ConnectionsCell,
        last_approved_block: Arc<Mutex<Option<ApprovedBlock>>>,
        block_store: KeyValueBlockStore,
        block_dag_storage: BlockDagKeyValueStorage,
        deploy_storage: KeyValueDeployStorage,
        rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
        casper_buffer_storage: CasperBufferKeyValueStorage,
        rspace_state_manager: RSpaceStateManager,
        block_processing_queue_tx: BlockProcessingQueueSender,
        blocks_in_processing: Arc<BlockProcessingIdentities>,
        casper_shard_conf: CasperShardConf,
        required_genesis_signatures: i32,
        validator_id: Option<ValidatorIdentity>,
        the_init: Arc<
            dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> + Send + Sync,
        >,
        block_message_tx: mpsc::Sender<BlockMessage>,
        block_message_rx: mpsc::Receiver<BlockMessage>,
        tuple_space_tx: mpsc::Sender<StoreItemsMessage>,
        tuple_space_rx: mpsc::Receiver<StoreItemsMessage>,
        trim_state: bool,
        disable_state_exporter: bool,
        event_publisher: F1r3flyEvents,
        block_retriever: BlockRetriever<T>,
        engine_cell: Arc<EngineCell>,
        runtime_manager: Arc<RuntimeManager>,
        estimator: Estimator,
        heartbeat_signal_ref: crate::rust::heartbeat_signal::HeartbeatSignalRef,
        state_items_tx: Option<mpsc::Sender<StoreItemsMessage>>,
    ) -> Self {
        let state = Self {
            transport_layer,
            rp_conf_ask,
            connections_cell,
            last_approved_block,
            block_store,
            block_dag_storage,
            deploy_storage,
            rejected_deploy_buffer,
            casper_buffer_storage,
            rspace_state_manager,
            block_processing_queue_tx,
            blocks_in_processing,
            casper_shard_conf,
            required_genesis_signatures,
            validator_id,
            the_init,
            block_message_rx: Arc::new(Mutex::new(Some(block_message_rx))),
            tuple_space_rx: Arc::new(Mutex::new(Some(tuple_space_rx))),
            block_message_tx: Arc::new(Mutex::new(Some(block_message_tx))),
            tuple_space_tx: Arc::new(Mutex::new(Some(tuple_space_tx))),
            block_message_queue_pending: Arc::new(AtomicUsize::new(0)),
            tuple_space_queue_pending: Arc::new(AtomicUsize::new(0)),
            trim_state,
            disable_state_exporter,
            restore_lifecycle: Arc::new(Mutex::new(RestoreLifecycle::new())),
            init_started_at: Arc::new(Mutex::new(None)),
            no_approved_block_retries: Arc::new(Mutex::new(0)),
            event_publisher,
            block_retriever,
            engine_cell,
            runtime_manager,
            estimator: Arc::new(Mutex::new(Some(estimator))),
            heartbeat_signal_ref,
            state_items_tx,
        };
        metrics::gauge!(
            INIT_BLOCK_MESSAGE_QUEUE_PENDING_METRIC,
            "source" => CASPER_METRICS_SOURCE
        )
        .set(0.0);
        metrics::gauge!(
            INIT_TUPLE_SPACE_QUEUE_PENDING_METRIC,
            "source" => CASPER_METRICS_SOURCE
        )
        .set(0.0);
        state
    }

    fn update_init_queue_metrics(&self) {
        metrics::gauge!(
            INIT_BLOCK_MESSAGE_QUEUE_PENDING_METRIC,
            "source" => CASPER_METRICS_SOURCE
        )
        .set(self.block_message_queue_pending.load(Ordering::Relaxed) as f64);
        metrics::gauge!(
            INIT_TUPLE_SPACE_QUEUE_PENDING_METRIC,
            "source" => CASPER_METRICS_SOURCE
        )
        .set(self.tuple_space_queue_pending.load(Ordering::Relaxed) as f64);
    }
}

#[async_trait]
impl<T: TransportLayer + Send + Sync + Clone + 'static> Engine for Initializing<T> {
    async fn init(&self) -> Result<(), CasperError> {
        metrics::counter!(
            CASPER_INIT_ATTEMPTS_METRIC,
            "source" => CASPER_METRICS_SOURCE
        )
        .increment(1);
        {
            let mut started_at = self.init_started_at.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire init_started_at lock".to_string())
            })?;
            if started_at.is_none() {
                *started_at = Some(Instant::now());
            }
        }
        (self.the_init)().await?;
        // Proactively request ApprovedBlock on init to handle the race condition where
        // the ApprovedBlock was broadcast while this node was still in GenesisValidator state
        // (verifying the UnapprovedBlock). Without this, the node could get stuck forever
        // waiting for an ApprovedBlock that was already sent and dropped.
        self.transport_layer
            .request_approved_block(&self.rp_conf_ask, Some(self.trim_state))
            .await
            .map_err(CasperError::CommError)
    }

    async fn handle(&self, peer: PeerNode, msg: CasperMessage) -> Result<(), CasperError> {
        match msg {
            CasperMessage::ApprovedBlock(approved_block) => {
                self.on_approved_block(peer, approved_block).await
            }
            CasperMessage::ApprovedBlockRequest(approved_block_request) => {
                send_no_approved_block_available(
                    &self.rp_conf_ask,
                    &self.transport_layer,
                    &approved_block_request.identifier,
                    peer,
                )
                .await
            }
            CasperMessage::NoApprovedBlockAvailable(no_approved_block_available) => {
                let retry_count = {
                    let mut retries = self.no_approved_block_retries.lock().map_err(|_| {
                        CasperError::RuntimeError(
                            "Failed to acquire no_approved_block_retries lock".to_string(),
                        )
                    })?;
                    *retries += 1;
                    *retries
                };
                metrics::counter!(
                    CASPER_INIT_RETRY_NO_APPROVED_BLOCK_METRIC,
                    "source" => CASPER_METRICS_SOURCE
                )
                .increment(1);
                log_no_approved_block_available(&no_approved_block_available.node_identifier);
                tracing::info!(
                    retry_count = retry_count,
                    "Retrying approved block request after NoApprovedBlockAvailable"
                );
                sleep(Duration::from_secs(10)).await;
                self.transport_layer
                    .request_approved_block(&self.rp_conf_ask, Some(self.trim_state))
                    .await
                    .map_err(CasperError::CommError)
            }
            CasperMessage::StoreItemsMessage(store_items_message) => {
                tracing::info!(
                    "Received {} from {}.",
                    store_items_message.clone().pretty(),
                    peer
                );
                // Enqueue into tuple space channel for requester stream
                let sender = self.tuple_space_tx.lock().unwrap().as_ref().cloned();
                if let Some(tx) = sender {
                    match tx.send(store_items_message).await {
                        Ok(()) => {
                            let _ = self.tuple_space_queue_pending.fetch_update(
                                Ordering::AcqRel,
                                Ordering::Acquire,
                                |curr| Some(curr + 1),
                            );
                            self.update_init_queue_metrics();
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to enqueue StoreItemsMessage into tuple_space channel: {:?}",
                                e
                            );
                        }
                    }
                } else {
                    tracing::warn!(
                        "tuple_space_tx sender is None; tuple space channel not available (message not enqueued)"
                    );
                }
                Ok(())
            }
            CasperMessage::BlockMessage(block_message) => {
                tracing::info!(
                    "BlockMessage received {} from {}.",
                    PrettyPrinter::build_string_block_message(&block_message, true),
                    peer
                );
                // Enqueue into block message channel for requester stream
                let sender = self.block_message_tx.lock().unwrap().as_ref().cloned();
                if let Some(tx) = sender {
                    match tx.send(block_message).await {
                        Ok(()) => {
                            let _ = self.block_message_queue_pending.fetch_update(
                                Ordering::AcqRel,
                                Ordering::Acquire,
                                |curr| Some(curr + 1),
                            );
                            self.update_init_queue_metrics();
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to enqueue BlockMessage into block_message channel: {:?}",
                                e
                            );
                        }
                    }
                } else {
                    tracing::warn!(
                        "block_message_tx sender is None; block message channel not available (message not enqueued)"
                    );
                }
                Ok(())
            }
            CasperMessage::MergeableEntryResponse(resp) => {
                tracing::warn!(
                    block_hash = %hex::encode(&resp.block_hash),
                    "ignored unauthenticated mergeable-entry response during initialization"
                );
                Ok(())
            }
            _ => {
                // **Scala equivalent**: `case _ => ().pure`
                Ok(())
            }
        }
    }

    /// Scala equivalent: Engine trait - Initializing doesn't have casper yet, so withCasper returns default
    /// In Scala: `def withCasper[A](f: MultiParentCasper[F] => F[A], default: F[A]): F[A] = default`
    fn with_casper(&self) -> Option<Arc<dyn MultiParentCasper + Send + Sync>> { None }
}

impl<T: TransportLayer + Send + Sync + Clone> Initializing<T> {
    async fn on_approved_block(
        &self,
        sender: PeerNode,
        approved_block: ApprovedBlock,
    ) -> Result<(), CasperError> {
        let sender_is_bootstrap = self
            .rp_conf_ask
            .bootstrap
            .as_ref()
            .map(|bootstrap| bootstrap == &sender)
            .unwrap_or(false);
        let received_shard = approved_block.candidate.block.shard_id.clone();
        let expected_shard = self.casper_shard_conf.shard_name.clone();
        let shard_name_is_valid = received_shard == expected_shard;

        async fn handle_approved_block<T: TransportLayer + Send + Sync + Clone>(
            initializing: &Initializing<T>,
            approved_block: &ApprovedBlock,
        ) -> Result<(), CasperError> {
            let block = &approved_block.candidate.block;

            tracing::info!(
                "Valid approved block {} received. Restoring approved state.",
                PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true)
            );

            initializing.block_dag_storage.insert(
                block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
            )?;

            initializing.request_approved_state(approved_block).await?;

            let _ = initializing
                .event_publisher
                .publish(F1r3flyEvent::approved_block_received(
                    PrettyPrinter::build_string_no_limit(&block.block_hash),
                ));

            tracing::info!("Approved state is ready; transitioning to Running");
            initializing
                .create_casper_and_transition_to_running(approved_block)
                .await?;

            tracing::info!(
                "Approved state for block {} is successfully restored.",
                PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true)
            );

            Ok(())
        }

        // TODO: Scala resolve validation of approved block - we should be sure that bootstrap is not lying
        // Might be Validate.approvedBlock is enough but have to check
        let validate_ok =
            Validate::approved_block(&approved_block, self.required_genesis_signatures);
        let is_valid = sender_is_bootstrap && shard_name_is_valid && validate_ok;

        if is_valid {
            tracing::info!("Received approved block from bootstrap node.");
        } else {
            tracing::info!("Invalid LastFinalizedBlock received; refusing to add.");
        }

        if !shard_name_is_valid {
            tracing::info!(
                "Connected to the wrong shard. Approved block received from bootstrap is in shard \
                '{}' but expected is '{}'. Check configuration option shard-name.",
                received_shard,
                expected_shard
            );
        }

        let start = self
            .restore_lifecycle
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError("Failed to acquire restore_lifecycle lock".to_string())
            })?
            .try_begin(is_valid);

        if let Some(lease) = start {
            metrics::counter!(
                CASPER_INIT_APPROVED_BLOCK_RECEIVED_METRIC,
                "source" => CASPER_METRICS_SOURCE
            )
            .increment(1);
            let no_approved_block_retries =
                *self.no_approved_block_retries.lock().map_err(|_| {
                    CasperError::RuntimeError(
                        "Failed to acquire no_approved_block_retries lock".to_string(),
                    )
                })?;
            if let Some(started_at) = *self.init_started_at.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire init_started_at lock".to_string())
            })? {
                let elapsed = started_at.elapsed();
                metrics::histogram!(
                    CASPER_INIT_TIME_TO_APPROVED_BLOCK_METRIC,
                    "source" => CASPER_METRICS_SOURCE
                )
                .record(elapsed.as_secs_f64());
                tracing::info!(
                    retries = no_approved_block_retries,
                    elapsed_ms = elapsed.as_millis(),
                    "Approved block accepted during initialization"
                );
            }
            match AssertUnwindSafe(handle_approved_block(self, &approved_block))
                .catch_unwind()
                .await
            {
                Ok(Ok(())) => {
                    let committed = self
                        .restore_lifecycle
                        .lock()
                        .map_err(|_| {
                            CasperError::RuntimeError(
                                "Failed to acquire restore_lifecycle lock".to_string(),
                            )
                        })?
                        .commit_running(lease);
                    if !committed {
                        tracing::error!("Approved-state restore completed outside Restoring phase");
                    }
                }
                Ok(Err(error)) => self.recover_from_restore_failure(lease, error).await?,
                Err(_) => {
                    self.recover_from_restore_failure(
                        lease,
                        CasperError::RuntimeError(
                            "approved-state restore panicked while validating received state"
                                .to_string(),
                        ),
                    )
                    .await?
                }
            }
        }
        Ok(())
    }

    async fn recover_from_restore_failure(
        &self,
        lease: RestoreLease,
        error: CasperError,
    ) -> Result<(), CasperError> {
        let (disposition, failures) = {
            let mut lifecycle = self.restore_lifecycle.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire restore_lifecycle lock".to_string())
            })?;
            (lifecycle.record_failure(lease), lifecycle.failures)
        };

        let Some(disposition) = disposition else {
            tracing::error!(error = %error, "Ignored restore failure outside Restoring phase");
            return Ok(());
        };

        tracing::error!(
            error = %error,
            failures,
            "Approved-state restore failed"
        );

        if disposition == RestoreFailureDisposition::Terminal {
            let terminal_error = CasperError::RuntimeError(format!(
                "approved-state restore failed {} times; last error: {}",
                failures, error
            ));
            self.engine_cell
                .report_startup_failure(terminal_error.clone());
            return Err(terminal_error);
        }

        if let Err(channel_error) = self.reinstall_sync_channels() {
            return self.terminate_active_restore(lease, channel_error);
        }

        let released = self
            .restore_lifecycle
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError("Failed to acquire restore_lifecycle lock".to_string())
            })?
            .release_retry(lease);
        if !released {
            return self.terminate_active_restore(
                lease,
                CasperError::RuntimeError(
                    "restore retry ownership could not return to Idle".to_string(),
                ),
            );
        }

        tracing::info!(
            failures,
            "Requesting another approved block after restore failure"
        );
        if let Err(comm_error) = self
            .transport_layer
            .request_approved_block(&self.rp_conf_ask, Some(self.trim_state))
            .await
        {
            return self.resolve_retry_request_failure(lease, CasperError::CommError(comm_error));
        }
        Ok(())
    }

    fn terminate_active_restore(
        &self,
        lease: RestoreLease,
        error: CasperError,
    ) -> Result<(), CasperError> {
        let terminated = self
            .restore_lifecycle
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError("Failed to acquire restore_lifecycle lock".to_string())
            })?
            .terminate_active(lease);
        if terminated {
            self.engine_cell.report_startup_failure(error.clone());
        }
        Err(error)
    }

    fn resolve_retry_request_failure(
        &self,
        lease: RestoreLease,
        error: CasperError,
    ) -> Result<(), CasperError> {
        let terminated = self
            .restore_lifecycle
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError("Failed to acquire restore_lifecycle lock".to_string())
            })?
            .terminate_retry_request(lease);
        if terminated {
            self.engine_cell.report_startup_failure(error.clone());
            Err(error)
        } else {
            tracing::info!(
                generation = lease.0,
                error = %error,
                "Ignored superseded approved-block retry request failure"
            );
            Ok(())
        }
    }

    fn reinstall_sync_channels(&self) -> Result<(), CasperError> {
        let (block_tx, block_rx) = mpsc::channel::<BlockMessage>(SYNC_CHANNEL_CAPACITY);
        let (tuple_tx, tuple_rx) = mpsc::channel::<StoreItemsMessage>(SYNC_CHANNEL_CAPACITY);

        *self.block_message_tx.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire block_message_tx lock".to_string())
        })? = Some(block_tx);
        *self.block_message_rx.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire block_message_rx lock".to_string())
        })? = Some(block_rx);
        *self.tuple_space_tx.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire tuple_space_tx lock".to_string())
        })? = Some(tuple_tx);
        *self.tuple_space_rx.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire tuple_space_rx lock".to_string())
        })? = Some(tuple_rx);

        self.block_message_queue_pending.store(0, Ordering::Release);
        self.tuple_space_queue_pending.store(0, Ordering::Release);
        self.update_init_queue_metrics();
        Ok(())
    }

    /// **Scala equivalent**: `def requestApprovedState(approvedBlock: ApprovedBlock): F[Unit]`
    ///
    /// This function is functionally equivalent to the Scala version, though the implementation differs
    /// due to fundamental differences between Scala fs2 streams and Rust tokio channels:
    ///
    /// Scala approach:
    /// - Uses fs2 Queue (async) for both blockMessageQueue and tupleSpaceQueue
    /// - Passes queues directly to LfsBlockRequester.stream and LfsTupleSpaceRequester.stream
    /// - fs2 handles async message passing internally
    ///
    /// Rust approach (this implementation):
    /// - block_message_queue is Arc<Mutex<VecDeque>> (sync) for thread-safe access
    /// - tuple_space_queue is mpsc::Sender (async channel sender)
    /// - For block messages: drains existing sync queue into new async channel, then uses that channel
    /// - For tuple space: uses existing sender directly
    ///
    /// The functional result is identical: both block and tuple space streams are processed
    /// concurrently, DAG is populated with final state, and system transitions to Running.
    /// The difference is in the underlying queue/channel implementation details.
    async fn request_approved_state(
        &self,
        approved_block: &ApprovedBlock,
    ) -> Result<(), CasperError> {
        // Starting minimum block height. When latest blocks are downloaded new minimum will be calculated.
        let block = &approved_block.candidate.block;
        let start_block_number = proto_util::block_number(block);
        // Compute the LFS lower bound: take the lower (= older floor) of
        // (a) deploy_lifespan window and (b) forward-horizon parent reach.
        // See `rspace_history_horizon::lfs_min_block_number` for the rule
        // and `casper/tests/util/rspace_history_horizon_test.rs` plus the
        // module's `#[cfg(test)] mod tests` for the spec.
        let min_block_number_for_deploy_lifespan =
            crate::rust::util::rspace_history_horizon::lfs_min_block_number(
                start_block_number,
                self.casper_shard_conf.deploy_lifespan,
                self.casper_shard_conf.max_parent_depth,
                self.casper_shard_conf.mergeable_channels_gc_depth_buffer,
            );
        let min_state_block_number =
            crate::rust::util::rspace_history_horizon::lfs_min_state_block_number(
                start_block_number,
                self.casper_shard_conf.max_parent_depth,
                self.casper_shard_conf.mergeable_channels_gc_depth_buffer,
            );

        tracing::info!(
            "request_approved_state: start (block {}, min_height {})",
            PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true),
            min_block_number_for_deploy_lifespan
        );

        // Use external block message receiver provided by test (equivalent to Scala blockMessageQueue)
        let response_message_rx =
            self.block_message_rx
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| {
                    CasperError::RuntimeError("Block message receiver not available".to_string())
                })?;

        // Create block requester wrapper with needed components and stream
        let mut block_requester = BlockRequesterWrapper::new(
            &self.transport_layer,
            &self.connections_cell,
            &self.rp_conf_ask,
            self.block_store.clone(),
            Box::new(|block| self.validate_block(block)),
        );

        // Create empty queue for block requester (must be created outside tokio::join! for lifetime reasons)
        let empty_queue = VecDeque::new(); // Empty queue since we drained it above

        // Use external tuple space message receiver provided by test (equivalent to Scala tupleSpaceQueue)
        let tuple_space_rx = self.tuple_space_rx.lock().unwrap().take().ok_or_else(|| {
            CasperError::RuntimeError("Tuple space receiver not available".to_string())
        })?;
        let tuple_space_requester =
            TupleSpaceRequester::new(&self.transport_layer, &self.rp_conf_ask);

        // Keep LFS retry cadence configurable instead of hard-coding a long startup delay.
        // Falls back to 5s when env var is absent or invalid.
        let lfs_request_timeout = Duration::from_secs(5);
        const LFS_SYNC_DEADLINE: Duration = Duration::from_secs(600);

        // **Scala equivalent**: Create both streams (blockRequestStream and tupleSpaceStream)
        let (block_request_stream_result, tuple_space_stream_result) = tokio::join!(
            lfs_block_requester::stream(
                approved_block,
                &empty_queue,
                response_message_rx,
                self.block_message_queue_pending.clone(),
                min_block_number_for_deploy_lifespan,
                lfs_request_timeout,
                &mut block_requester,
            ),
            lfs_tuple_space_requester::stream(
                approved_block,
                tuple_space_rx,
                self.tuple_space_queue_pending.clone(),
                lfs_request_timeout,
                tuple_space_requester,
                self.rspace_state_manager.importer.clone(),
                LFS_SYNC_DEADLINE,
            )
        );

        let block_request_stream = block_request_stream_result?;
        let (tuple_space_stream, tuple_space_err) = tuple_space_stream_result?;

        // **Scala equivalent**: `blockRequestAddDagStream = blockRequestStream.last.unNoneTerminate.evalMap { st => populateDag(...) }`
        // Process block request stream and return the final state for later DAG population
        let block_request_future = async move {
            // Process the stream to completion and get the last state
            let mut stream = Box::pin(block_request_stream);
            let mut last_st = None;
            while let Some(st) = stream.next().await {
                last_st = Some(st);
            }
            Ok::<Option<lfs_block_requester::ST<BlockHash>>, CasperError>(last_st)
        };

        // **Scala equivalent**: `tupleSpaceLogStream = tupleSpaceStream ++ fs2.Stream.eval(Log[F].info(...)).drain`
        // Process tuple space stream and log completion message
        let tuple_space_future = async move {
            // Stream items are processed by the stream itself, we just consume them to completion
            let mut stream = Box::pin(tuple_space_stream);
            while let Some(_) = stream.next().await {}
            if let Some(e) = tuple_space_err.lock().unwrap().take() {
                return Err(e);
            }
            tracing::info!("Rholang state received and saved to store.");
            Ok::<(), CasperError>(())
        };

        // **Scala equivalent**: `fs2.Stream(blockRequestAddDagStream, tupleSpaceLogStream).parJoinUnbounded.compile.drain`
        // Run both futures to completion; avoid canceling one branch if the other errors first.
        let (final_state_result, tuple_space_result) =
            tokio::join!(block_request_future, tuple_space_future);
        let final_state_result = final_state_result?;
        tuple_space_result?;

        // Now populate DAG with the final state (equivalent to evalMap in Scala)
        if let Some(st) = final_state_result {
            self.populate_dag(
                approved_block.candidate.block.clone(),
                st.lower_bound,
                st.height_map,
            )
            .await?;
        } else {
            tracing::warn!(
                "request_approved_state: block_request_stream returned no final state (None)"
            );
        }

        // Forward-horizon rspace history sync — ship rspace post-state for
        // every block within `max_parent_depth + depth_buffer` of LFB so
        // subsequent block validation never hits `UnknownRootError`. See
        // `casper/src/rust/util/rspace_history_horizon.rs` for the
        // reachability calc and `casper/src/rust/engine/lfs_horizon_requester.rs`
        // for the orchestrator. Companion to the proposer-side
        // `Estimator::filterDeepParents` and the validator-side parent-depth
        // check in `validate::parents`.
        {
            let dag = self.block_dag_storage.get_representation()?;
            let horizon_roots =
                crate::rust::util::rspace_history_horizon::compute_forward_horizon_roots(
                    &dag,
                    &self.block_store,
                    &approved_block.candidate.block,
                    &self.casper_shard_conf,
                    min_state_block_number,
                )
                .map_err(|e| CasperError::KvStoreError(e))?;

            if !horizon_roots.is_empty() {
                // Phase 1's tuple_space_message_receiver was consumed by
                // lfs_tuple_space_requester::stream. Install a fresh
                // (tx, rx) pair on `tuple_space_tx` so handle_message_recv
                // routes incoming `StoreItemsMessage`s to the orchestrator.
                let (horizon_tx, horizon_rx) = mpsc::channel::<StoreItemsMessage>(50);
                {
                    let mut sender_slot = self.tuple_space_tx.lock().unwrap();
                    *sender_slot = Some(horizon_tx);
                }

                let request_timeout = Duration::from_secs(30);
                tracing::info!(
                    "LFS forward-horizon: requesting {} ancestor rspace roots below LFB",
                    horizon_roots.len()
                );

                // Consume the streaming-parallel orchestrator the same way
                // `lfs_tuple_space_requester::stream` is consumed above:
                // drive to completion, then check the final ST.is_finished()
                // to detect incomplete sync.
                use futures::StreamExt;
                let horizon_requester =
                    HorizonRequester::new(&self.transport_layer, &self.rp_conf_ask);
                let rm_for_has_root = self.runtime_manager.clone();
                let has_root: crate::rust::engine::lfs_horizon_requester::HasRootFn =
                    Arc::new(move |root| rm_for_has_root.has_root(root));
                let horizon_stream = crate::rust::engine::lfs_horizon_requester::stream(
                    horizon_roots,
                    has_root,
                    self.rspace_state_manager.importer.clone(),
                    horizon_requester,
                    horizon_rx,
                    request_timeout,
                    LFS_SYNC_DEADLINE,
                )
                .await;

                // Drop the temporary sender so subsequent StoreItemsMessages
                // (none expected once Running) don't queue indefinitely.
                let (final_horizon_state, horizon_err) = match horizon_stream {
                    Ok((stream, err_handle)) => {
                        let mut stream = Box::pin(stream);
                        let mut final_state = None;
                        while let Some(st) = stream.next().await {
                            final_state = Some(st);
                        }
                        (Ok(final_state), err_handle.lock().unwrap().take())
                    }
                    Err(e) => (Err(e), None),
                };
                {
                    let mut sender_slot = self.tuple_space_tx.lock().unwrap();
                    *sender_slot = None;
                }

                if let Some(e) = horizon_err {
                    return Err(e);
                }

                // Loud failure: cannot transition to Running without a
                // complete forward horizon. Subsequent block validation
                // would hit `UnknownRootError` and cascade-invalidate.
                match final_horizon_state? {
                    Some(st) if st.is_finished() => {}
                    Some(st) => {
                        return Err(CasperError::RuntimeError(format!(
                            "LFS forward-horizon: incomplete sync; {} chunk paths still pending (state machine has {} entries)",
                            st.len() - st.done_count(),
                            st.len(),
                        )));
                    }
                    None => {
                        return Err(CasperError::RuntimeError(
                            "LFS forward-horizon: stream produced no final state".to_string(),
                        ));
                    }
                }
            } else {
                tracing::info!(
                    "LFS forward-horizon: skipped (max_parent_depth unlimited or LFB at genesis)"
                );
            }
        }

        // Replay blocks to populate the mergeable channel cache. Needed for
        // multi-parent block validation: any block whose mergeable-channel
        // metadata predates this node's LFS sync must be replayed against the
        // stored post-state so `RuntimeManager::load_mergeable_channels`
        // succeeds during validation.
        //
        // Mergeable-channel metadata is not committed by the block and cannot
        // be authenticated from a peer response. Reconstruct it from the
        // locally replayed block instead.
        self.replay_blocks_for_mergeable_channels(approved_block, min_state_block_number)
            .await?;

        Ok(())
    }

    fn validate_block(&self, block: &BlockMessage) -> bool {
        let block_number = proto_util::block_number(block);
        if block_number == 0 {
            // TODO: validate genesis (zero) block correctly - OLD
            true
        } else {
            match Validate::block_hash(block) {
                Either::Right(ValidBlock::Valid) => true,
                _ => false,
            }
        }
    }

    async fn populate_dag(
        &self,
        start_block: BlockMessage,
        min_height: i64,
        height_map: BTreeMap<i64, HashSet<BlockHash>>,
    ) -> Result<(), CasperError> {
        async fn add_block_to_dag<T: TransportLayer + Send + Sync + Clone>(
            initializing: &Initializing<T>,
            block: &BlockMessage,
            is_invalid: bool,
        ) -> Result<(), CasperError> {
            tracing::info!(
                "Adding {}, invalid = {}.",
                PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true),
                is_invalid
            );

            // Scala equivalent: `BlockDagStorage[F].insert(block, invalid = isInvalid)`
            initializing.block_dag_storage.insert(
                block,
                if is_invalid {
                    block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid
                } else {
                    block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal
                },
            )?;

            Ok(())
        }

        tracing::info!("Adding blocks for approved state to DAG.");

        let slashed_validators: Vec<ByteString> = start_block
            .body
            .state
            .bonds
            .iter()
            .filter(|bond| bond.stake == 0)
            .map(|bond| bond.validator.to_vec())
            .collect();

        let invalid_blocks: HashSet<ByteString> = start_block
            .justifications
            .iter()
            .filter(|justification| slashed_validators.contains(&justification.validator.to_vec()))
            .map(|justification| justification.latest_block_hash.to_vec())
            .collect();

        // Add sorted DAG in order from approved block to oldest
        for hash in height_map
            .values()
            .flat_map(|hashes| hashes.iter())
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            // NOTE: This is not in original Scala code. Added because we changed block_store
            // to Option<KeyValueBlockStore> to support moving it in create_casper_and_transition_to_running
            let block = self.block_store.get_unsafe(&hash);
            if block.block_hash == start_block.block_hash {
                continue;
            }
            // If sender has stake 0 in approved block, this means that sender has been slashed and block is invalid
            let is_invalid = invalid_blocks.contains(&block.block_hash.to_vec());
            // Filter older not necessary blocks
            let block_height = proto_util::block_number(&block);
            let block_height_ok = block_height >= min_height;

            // Add block to DAG
            if block_height_ok {
                add_block_to_dag(self, &block, is_invalid).await?;
            }
        }

        tracing::info!("Blocks for approved state added to DAG.");
        Ok(())
    }

    /// Replay blocks in topological order to populate the mergeable channel cache.
    /// This is necessary for multi-parent block validation, which requires mergeable
    /// channel data from parent blocks to compute merged state.
    ///
    /// The LFS sync transfers the RSpace trie but not authenticated mergeable
    /// evidence, so every absent entry is regenerated by local replay. A peer
    /// response cannot be trusted because the block does not commit that
    /// auxiliary value.
    ///
    /// Before this fix, this loop replayed EVERY block from `min_block_number`
    /// to the LFB unconditionally, regardless of whether its cache entry was
    /// already present. With `disable-lfs=true` (or any join where
    /// `min_block_number` resolves to genesis), that meant single-threaded,
    /// full-history block execution gating the transition to `Running` — and
    /// since the chain tip keeps advancing while this runs, a joiner that
    /// falls far enough behind can never finish before it needs to start
    /// over, wedging permanently short of the tip (see
    /// f1r3fly-io/f1r3node-rust "observer permanent early stall" report).
    /// Skipping blocks whose entry already exists bounds the actual replay
    /// work to genuinely missing entries instead of full chain depth.
    async fn replay_blocks_for_mergeable_channels(
        &self,
        approved_block: &ApprovedBlock,
        min_block_number: i64,
    ) -> Result<(), CasperError> {
        // Bounds the replay so a stuck cold-start produces an explicit error
        // instead of an infinite hang (see issues/05-casper-cold-restart-replay-hang.md).
        // 20 minutes matches the slowest recovery window the issue itself
        // considers acceptable (49-71 finalized blocks).
        const MERGEABLE_CHANNEL_REPLAY_DEADLINE: Duration = Duration::from_secs(1200);

        match tokio::time::timeout(
            MERGEABLE_CHANNEL_REPLAY_DEADLINE,
            self.replay_blocks_for_mergeable_channels_inner(approved_block, min_block_number),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(CasperError::RuntimeError(format!(
                "replay_blocks_for_mergeable_channels timed out after {:?}; cold-start cannot proceed",
                MERGEABLE_CHANNEL_REPLAY_DEADLINE
            ))),
        }
    }

    async fn replay_blocks_for_mergeable_channels_inner(
        &self,
        _approved_block: &ApprovedBlock,
        min_block_number: i64,
    ) -> Result<(), CasperError> {
        tracing::info!("Replaying blocks to populate mergeable channel cache...");

        // Get DAG representation for traversal
        let dag = self.block_dag_storage.get_representation()?;

        // Get all blocks in the DAG that need replay (from minBlockNumber to LFB)
        // We process in topological order (by block number, then by hash for determinism)
        let all_blocks = dag.topo_sort(min_block_number, None)?;
        let blocks_to_replay: Vec<BlockHash> = all_blocks.into_iter().flatten().collect();

        tracing::info!(
            "Found {} blocks in range for mergeable channel cache check.",
            blocks_to_replay.len()
        );

        // Replay each block to populate mergeable channels, skipping entries
        // already derived by local execution or replay.
        let mut skipped = 0usize;
        let mut replayed = 0usize;
        for block_hash in blocks_to_replay {
            let block = self.block_store.get(&block_hash)?.ok_or_else(|| {
                CasperError::RuntimeError(format!(
                    "Block {} missing from block store during mergeable channel replay",
                    PrettyPrinter::build_string_bytes(&block_hash)
                ))
            })?;

            if self
                .runtime_manager
                .has_mergeable_entry(&block)
                .unwrap_or(false)
            {
                skipped += 1;
                continue;
            }

            let parents = &block.header.parents_hash_list;
            if parents.is_empty() {
                // Genesis block - replay from empty state
                self.replay_genesis_block(&block).await?;
            } else {
                self.replay_single_block(&block).await?;
            }
            replayed += 1;
        }

        tracing::info!(
            "Mergeable channel cache populated: {} already cached (skipped), {} replayed.",
            skipped,
            replayed
        );
        Ok(())
    }

    /// Replay genesis block to populate its mergeable channel cache entry.
    /// Genesis is special because it starts from empty state.
    async fn replay_genesis_block(&self, block: &BlockMessage) -> Result<(), CasperError> {
        let block_hash = &block.block_hash;
        let block_number = proto_util::block_number(block);

        tracing::debug!(
            "Replaying genesis block #{} ({})",
            block_number,
            PrettyPrinter::build_string_bytes(block_hash)
        );

        // Genesis starts from empty state
        let pre_state_hash = RuntimeManager::empty_state_hash_fixed();

        // Replay genesis - this will save mergeable channels to the store.
        // `Arc<RuntimeManager>` is interior-mutable post rspace++ rewrite, so no
        // outer Mutex / lock acquisition is required.
        let result = self
            .runtime_manager
            .replay_block_from_consensus_data(
                &pre_state_hash,
                block,
                None, // No invalid blocks for genesis
            )
            .await;

        // A replay failure or state mismatch means the mergeable channel
        // store would be silently incomplete — a consensus hazard once the
        // node is Running. Fail bootstrap loudly instead.
        let computed_post_state = result.map_err(|e| {
            CasperError::RuntimeError(format!(
                "Genesis block replay failed while populating mergeable channel cache: {:?}",
                e
            ))
        })?;
        let expected_post_state = &block.body.state.post_state_hash;
        if computed_post_state != *expected_post_state {
            return Err(CasperError::RuntimeError(format!(
                "Genesis block replay state mismatch: computed={}, expected={}",
                PrettyPrinter::build_string_bytes(&computed_post_state),
                PrettyPrinter::build_string_bytes(expected_post_state)
            )));
        }
        tracing::debug!("Genesis block replayed successfully.");

        Ok(())
    }

    /// Replay a single block to populate its mergeable channel cache entry.
    async fn replay_single_block(&self, block: &BlockMessage) -> Result<(), CasperError> {
        let block_hash = &block.block_hash;
        let block_number = proto_util::block_number(block);
        let parents = &block.header.parents_hash_list;

        // For single-parent blocks, use parent's post-state
        // For multi-parent blocks, we need the pre-state from the block itself
        // (by the time we reach a multi-parent block, all its parents have been replayed)
        let pre_state_hash = if parents.len() == 1 {
            let parent_block = self.block_store.get(&parents[0])?.ok_or_else(|| {
                CasperError::RuntimeError(format!(
                    "Parent block {} missing from block store during mergeable channel replay",
                    PrettyPrinter::build_string_bytes(&parents[0])
                ))
            })?;
            parent_block.body.state.post_state_hash.clone()
        } else {
            // Multi-parent: use the block's recorded pre-state
            // This works because we're replaying in topological order
            block.body.state.pre_state_hash.clone()
        };

        // Get invalid blocks map for replay
        let dag = self.block_dag_storage.get_representation()?;
        let slashed_hashes = block
            .body
            .system_deploys
            .iter()
            .filter_map(|deploy| match deploy {
                ProcessedSystemDeploy::Succeeded {
                    system_deploy:
                        SystemDeployData::Slash {
                            invalid_block_hash, ..
                        },
                    ..
                } => Some(invalid_block_hash.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let invalid_blocks_map = proto_util::slashed_block_senders(&dag, &slashed_hashes)?;

        tracing::debug!(
            "Replaying block #{} ({}) with {} deploys, {} parents",
            block_number,
            PrettyPrinter::build_string_bytes(block_hash),
            block.body.deploys.len(),
            parents.len()
        );

        // Replay the block - this will save mergeable channels to the store.
        // `Arc<RuntimeManager>` is interior-mutable post rspace++ rewrite, so no
        // outer Mutex / lock acquisition is required.
        let result = self
            .runtime_manager
            .replay_block_from_consensus_data(&pre_state_hash, block, Some(invalid_blocks_map))
            .await;

        // A replay failure or state mismatch means the mergeable channel
        // store would be silently incomplete — a consensus hazard once the
        // node is Running. Fail bootstrap loudly instead.
        let computed_post_state = result.map_err(|e| {
            CasperError::RuntimeError(format!(
                "Block #{} replay failed while populating mergeable channel cache: {:?}",
                block_number, e
            ))
        })?;
        let expected_post_state = &block.body.state.post_state_hash;
        if computed_post_state != *expected_post_state {
            return Err(CasperError::RuntimeError(format!(
                "Block #{} replay state mismatch: computed={}, expected={}",
                block_number,
                PrettyPrinter::build_string_bytes(&computed_post_state),
                PrettyPrinter::build_string_bytes(expected_post_state)
            )));
        }
        tracing::debug!("Block #{} replayed successfully.", block_number);

        Ok(())
    }

    /// **Scala equivalent**: `private def createCasperAndTransitionToRunning(approvedBlock: ApprovedBlock): F[Unit]`
    async fn create_casper_and_transition_to_running(
        &self,
        approved_block: &ApprovedBlock,
    ) -> Result<(), CasperError> {
        let ab = approved_block.candidate.block.clone();
        let genesis_post_state_hash = ab.body.state.post_state_hash.clone();

        // RuntimeManager is lock-free Arc<RuntimeManager>; clone the Arc.
        let runtime_manager = self.runtime_manager.clone();

        if let Err(error) =
            crate::rust::util::token_metadata_check::verify_token_metadata_matches_config(
                &runtime_manager,
                &genesis_post_state_hash,
                &self.casper_shard_conf.native_token_name,
                &self.casper_shard_conf.native_token_symbol,
                self.casper_shard_conf.native_token_decimals,
            )
            .await
        {
            self.engine_cell.report_startup_failure(error.clone());
            return Err(error);
        }

        let estimator = self
            .estimator
            .lock()
            .unwrap()
            .as_ref()
            .cloned()
            .ok_or_else(|| CasperError::RuntimeError("Estimator not available".to_string()))?;
        // The on-chain fault-tolerance threshold is read and adopted by
        // `hash_set_casper` (the single adoption point shared by all three
        // casper constructors), so this path deliberately does NOT read it
        // again — a second read here would be a second policy site and could
        // drift from the one the running casper actually finalizes with.
        // (The pre-merge re-read that used to live here was removed in the
        // 2026-08-07 dev merge for exactly that reason; see
        // `casper::hash_set_casper`'s "SINGLE ADOPTION POINT" reconcile, which
        // discharges `FtProvenance.reconcile_agrees_on_onchain`.)
        let casper_shard_conf = self.casper_shard_conf.clone();

        // Pass Arc<RuntimeManager> directly to hash_set_casper
        let casper = crate::rust::casper::hash_set_casper(
            self.block_retriever.clone(),
            self.event_publisher.clone(),
            runtime_manager,
            estimator,
            self.block_store.clone(),
            self.block_dag_storage.clone(),
            self.deploy_storage.clone(),
            self.rejected_deploy_buffer.clone(),
            self.casper_buffer_storage.clone(),
            self.validator_id.clone(),
            casper_shard_conf,
            ab,
            self.heartbeat_signal_ref.clone(),
        )
        .await?;
        tracing::info!(
            "create_casper_and_transition_to_running: MultiParentCasper instance created"
        );

        self.block_store.put_approved_block(approved_block)?;
        {
            let mut last_approved = self.last_approved_block.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire last_approved_block lock".to_string())
            })?;
            *last_approved = Some(approved_block.clone());
        }

        // **Scala equivalent**: `transitionToRunning[F](...)`
        tracing::info!("create_casper_and_transition_to_running: calling transition_to_running");

        // Create empty async init (matches Scala ().pure[F])
        let the_init = Arc::new(|_| {
            Box::pin(async { Ok(()) })
                as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
        });

        // Phase 7b-1 (2026-08-27): build snapshot chunk-fetch
        // context if this node has an fs_snapshot_writer.
        let snapshot_chunk_ctx =
            crate::rust::engine::snapshot_chunk_sync::build_snapshot_chunk_context(
                &self.runtime_manager,
            )
            .await;

        // Phase 7b-2 (2026-08-27): build a WAL payload-fetch
        // context.  Payload lookup comes from the shared
        // `RuntimeManager.payload_store` bundle so joiner-side
        // reads hit the same on-disk dir the leader-side writes
        // populate.  Fresh `InMemoryPayloadStore` fallback for
        // test harnesses that don't wire the boot pipeline.
        let wal_payload_ctx = {
            use std::sync::Arc as StdArc;

            use crate::rust::engine::running::WalPayloadContext;
            use crate::rust::engine::wal_payload_retriever::WalPayloadRetriever;
            use crate::rust::engine::wal_payload_server::InMemoryPayloadStore;
            use crate::rust::engine::wal_payload_sync::WalPayloadSyncDriver;
            let retriever = StdArc::new(WalPayloadRetriever::new());
            let sync_driver = StdArc::new(WalPayloadSyncDriver::new(StdArc::clone(&retriever)));
            let lookup: StdArc<dyn crate::rust::engine::wal_payload_server::PayloadLookup> =
                match self.runtime_manager.get_payload_store().await {
                    Some(b) => b.lookup,
                    None => StdArc::new(InMemoryPayloadStore::new()),
                };
            Some(WalPayloadContext {
                sync_driver,
                payload_lookup: lookup,
                // DD-7b-3 (a) tick-stop handle is installed later
                // inside `transition_to_running` after `spawn_
                // periodic_tick` runs; None here means "no live
                // tick loop yet" (default for the pre-transition
                // ctx).
                tick_stop: None,
            })
        };

        // Phase 7b-2 item (c) (2026-08-28): boot wire-in for the
        // apply-to-follower flow.  Mirrors `casper_launch`'s hook:
        // install a completion sink on the snapshot chunk driver
        // + spawn the subscriber that decodes each completed
        // snapshot and drives the WAL payload fetch + applier.
        if let (Some(snap_ctx), Some(wal_ctx)) =
            (snapshot_chunk_ctx.as_ref(), wal_payload_ctx.as_ref())
        {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<
                crate::rust::engine::snapshot_chunk_sync::SnapshotCompletion,
            >();
            snap_ctx.sync_driver.install_completion_sink(tx);
            // c-2 review-follow-up (2026-08-30): mirror of the
            // casper_launch.rs plumbing — see that file for the
            // full rationale.
            let allowed_roots = self.runtime_manager.consensus_static_roots().await;
            // DD-7b-2 (a) Option 1: reproduce locally via the
            // joiner's own PayloadLookup before falling back to
            // peer fetch.  See casper_launch.rs for full rationale.
            // DD-7b-2 (a) Option 2 (2026-08-29): block-storage-
            // backed reproduction tier — walks the
            // payload_hash → deploy_sig chain and re-executes the
            // source deploy in a scratch runtime to reproduce
            // bytes for hashes the local `PayloadLookup` misses.
            // See casper_launch.rs for the full flow.
            let option2_ctx = Some(
                crate::rust::engine::wal_payload_sync::Option2ReducerContext {
                    block_storage: self.block_dag_storage.clone(),
                    block_store: self.block_store.clone(),
                    runtime_manager: self.runtime_manager.clone(),
                },
            );
            let _handle = crate::rust::engine::wal_apply_boot::spawn_boot_apply_subscriber(
                rx,
                std::sync::Arc::clone(&wal_ctx.sync_driver),
                snap_ctx.snapshot_dir.clone(),
                self.runtime_manager.root_id_registry.clone(),
                allowed_roots,
                Some(std::sync::Arc::clone(&wal_ctx.payload_lookup)),
                option2_ctx,
            );
        }

        transition_to_running(
            self.block_processing_queue_tx.clone(),
            self.blocks_in_processing.clone(),
            Arc::new(casper),
            approved_block.clone(),
            the_init,
            self.disable_state_exporter,
            Arc::new(self.transport_layer.clone()),
            self.rp_conf_ask.clone(),
            self.block_retriever.clone(),
            Some(RunningRecoveryContext {
                connections_cell: self.connections_cell.clone(),
            }),
            snapshot_chunk_ctx,
            wal_payload_ctx,
            &self.engine_cell,
            &self.event_publisher,
            self.state_items_tx.clone(),
        )
        .await?;

        self.estimator.lock().unwrap().take();

        if let Ok(started_at) = self.init_started_at.lock() {
            if let Some(started_at) = *started_at {
                let elapsed = started_at.elapsed();
                metrics::histogram!(
                    CASPER_INIT_TIME_TO_RUNNING_METRIC,
                    "source" => CASPER_METRICS_SOURCE
                )
                .record(elapsed.as_secs_f64());
            }
        }

        tracing::info!(
            "create_casper_and_transition_to_running: transition_to_running completed successfully"
        );

        if let Err(error) = self
            .transport_layer
            .send_fork_choice_tip_request(&self.connections_cell, &self.rp_conf_ask)
            .await
        {
            tracing::warn!(
                error = %error,
                "Fork-choice tip request failed after Running commit"
            );
        }

        Ok(())
    }
}

/// **Scala equivalent**: Engine trait implementation
// Remove the following block:
// impl<T: TransportLayer + Send + Sync> Engine for Initializing<T> { ... }

// Implement BlockRequesterOps trait for the wrapper struct
#[async_trait]
impl<T: TransportLayer + Send + Sync> BlockRequesterOps for BlockRequesterWrapper<'_, T> {
    async fn request_for_block(&self, block_hash: &BlockHash) -> Result<(), CasperError> {
        self.transport_layer
            .broadcast_request_for_block(self.connections_cell, self.rp_conf_ask, block_hash)
            .await?;
        Ok(())
    }

    fn contains_block(&self, block_hash: &BlockHash) -> Result<bool, CasperError> {
        Ok(self.block_store.contains(block_hash)?)
    }

    fn get_block_from_store(&self, block_hash: &BlockHash) -> BlockMessage {
        self.block_store.get_unsafe(block_hash)
    }

    fn put_block_to_store(
        &mut self,
        block_hash: BlockHash,
        block: &BlockMessage,
    ) -> Result<(), CasperError> {
        Ok(self.block_store.put(block_hash, block)?)
    }

    fn validate_block(&self, block: &BlockMessage) -> bool { (self.validate_block_fn)(block) }
}

/// Wrapper struct for block request operations
pub struct BlockRequesterWrapper<'a, T: TransportLayer> {
    transport_layer: &'a T,
    connections_cell: &'a ConnectionsCell,
    rp_conf_ask: &'a RPConf,
    block_store: KeyValueBlockStore,
    validate_block_fn: Box<dyn Fn(&BlockMessage) -> bool + Send + Sync + 'a>,
}

impl<'a, T: TransportLayer> BlockRequesterWrapper<'a, T> {
    pub fn new(
        transport_layer: &'a T,
        connections_cell: &'a ConnectionsCell,
        rp_conf_ask: &'a RPConf,
        block_store: KeyValueBlockStore,
        validate_block_fn: Box<dyn Fn(&BlockMessage) -> bool + Send + Sync + 'a>,
    ) -> Self {
        Self {
            transport_layer,
            connections_cell,
            rp_conf_ask,
            block_store,
            validate_block_fn,
        }
    }
}

/// Wrapper struct for tuple space request operations
pub struct TupleSpaceRequester<'a, T: TransportLayer> {
    transport_layer: &'a T,
    rp_conf_ask: &'a RPConf,
}

impl<'a, T: TransportLayer> TupleSpaceRequester<'a, T> {
    pub fn new(transport_layer: &'a T, rp_conf_ask: &'a RPConf) -> Self {
        Self {
            transport_layer,
            rp_conf_ask,
        }
    }
}

// Implement TupleSpaceRequesterOps trait for the wrapper struct
#[async_trait]
impl<T: TransportLayer + Send + Sync> TupleSpaceRequesterOps for TupleSpaceRequester<'_, T> {
    async fn request_for_store_item(
        &self,
        path: &StatePartPath,
        page_size: i32,
    ) -> Result<(), CasperError> {
        let message = StoreItemsMessageRequest {
            start_path: path.clone(),
            skip: 0,
            take: page_size,
        };

        let message_proto = message.to_proto();

        self.transport_layer
            .send_to_bootstrap(self.rp_conf_ask, Arc::new(message_proto))
            .await?;
        Ok(())
    }

    fn validate_tuple_space_items(
        &self,
        history_items: Vec<(Blake2b256Hash, Vec<u8>)>,
        data_items: Vec<(Blake2b256Hash, Vec<u8>)>,
        start_path: StatePartPath,
        page_size: i32,
        skip: i32,
        get_from_history: Arc<dyn RSpaceImporter>,
    ) -> Result<(), CasperError> {
        RSpaceImporterInstance::validate_state_items(
            history_items,
            data_items,
            start_path,
            page_size,
            skip,
            get_from_history,
        )
        .map_err(CasperError::RuntimeError)
    }
}

/// Wrapper struct for forward-horizon request operations. Mirrors
/// `TupleSpaceRequester` — same trait shape, same single-peer
/// `send_to_bootstrap` body. Lives here (not in the requester module)
/// for the same reason: the requester module is transport-agnostic and
/// this wrapper is the seam where `TransportLayer` is plugged in.
pub struct HorizonRequester<'a, T: TransportLayer> {
    transport_layer: &'a T,
    rp_conf_ask: &'a RPConf,
}

impl<'a, T: TransportLayer> HorizonRequester<'a, T> {
    pub fn new(transport_layer: &'a T, rp_conf_ask: &'a RPConf) -> Self {
        Self {
            transport_layer,
            rp_conf_ask,
        }
    }
}

#[async_trait]
impl<T: TransportLayer + Send + Sync>
    crate::rust::engine::lfs_horizon_requester::HorizonRequesterOps for HorizonRequester<'_, T>
{
    async fn request_for_horizon_chunk(
        &self,
        path: &StatePartPath,
        page_size: i32,
    ) -> Result<(), CasperError> {
        let message = StoreItemsMessageRequest {
            start_path: path.clone(),
            skip: 0,
            take: page_size,
        };
        let message_proto = message.to_proto();
        self.transport_layer
            .send_to_bootstrap(self.rp_conf_ask, Arc::new(message_proto))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod restore_lifecycle_tests {
    use proptest::prelude::*;

    use super::{RestoreFailureDisposition, RestoreLifecycle, RestorePhase, MAX_RESTORE_FAILURES};

    #[test]
    fn duplicate_approved_blocks_do_not_acquire_a_second_restore() {
        let mut lifecycle = RestoreLifecycle::new();
        assert!(lifecycle.try_begin(true).is_some());
        assert!(lifecycle.try_begin(true).is_none());
        assert_eq!(lifecycle.phase, RestorePhase::Restoring);
    }

    #[test]
    fn terminal_failure_cannot_reopen_restore() {
        let mut lifecycle = RestoreLifecycle::new();
        for expected in 1..=MAX_RESTORE_FAILURES {
            let lease = lifecycle.try_begin(true).unwrap();
            let disposition = lifecycle.record_failure(lease).unwrap();
            assert_eq!(lifecycle.failures, expected);
            if expected < MAX_RESTORE_FAILURES {
                assert_eq!(disposition, RestoreFailureDisposition::Retry);
                assert!(lifecycle.release_retry(lease));
            } else {
                assert_eq!(disposition, RestoreFailureDisposition::Terminal);
            }
        }
        assert!(lifecycle.try_begin(true).is_none());
        assert_eq!(lifecycle.phase, RestorePhase::Terminal);
    }

    proptest! {
        #[test]
        fn retry_ownership_matches_the_failure_budget(
            recoverable_failures in 0_u64..MAX_RESTORE_FAILURES,
            duplicate_count in 0_usize..32,
        ) {
            let mut lifecycle = RestoreLifecycle::new();
            for _ in 0..recoverable_failures {
                let lease = lifecycle.try_begin(true).unwrap();
                for _ in 0..duplicate_count {
                    prop_assert!(lifecycle.try_begin(true).is_none());
                }
                prop_assert_eq!(
                    lifecycle.record_failure(lease),
                    Some(RestoreFailureDisposition::Retry)
                );
                prop_assert!(lifecycle.release_retry(lease));
                prop_assert_eq!(lifecycle.phase, RestorePhase::Idle);
            }
        }

        #[test]
        fn running_commit_is_permanent(
            terminal_attempts in 0_usize..32,
            duplicate_count in 0_usize..32,
        ) {
            let mut lifecycle = RestoreLifecycle::new();
            let lease = lifecycle.try_begin(true).unwrap();
            prop_assert!(lifecycle.commit_running(lease));
            for _ in 0..duplicate_count {
                prop_assert!(lifecycle.try_begin(true).is_none());
                prop_assert_eq!(lifecycle.record_failure(lease), None);
            }
            for _ in 0..terminal_attempts {
                prop_assert!(!lifecycle.terminate_active(lease));
                prop_assert!(!lifecycle.terminate_retry_request(lease));
            }
            prop_assert_eq!(lifecycle.phase, RestorePhase::Running);
        }

        #[test]
        fn invalid_approved_blocks_do_not_change_restore_ownership(
            invalid_count in 0_usize..64,
        ) {
            let mut lifecycle = RestoreLifecycle::new();
            for _ in 0..invalid_count {
                prop_assert!(lifecycle.try_begin(false).is_none());
            }
            prop_assert_eq!(lifecycle, RestoreLifecycle::new());
        }

        #[test]
        fn stale_retry_results_cannot_mutate_newer_generations(
            stale_result_count in 0_usize..32,
        ) {
            let mut lifecycle = RestoreLifecycle::new();
            let stale_lease = lifecycle.try_begin(true).unwrap();
            prop_assert_eq!(
                lifecycle.record_failure(stale_lease),
                Some(RestoreFailureDisposition::Retry)
            );
            prop_assert!(lifecycle.release_retry(stale_lease));
            let current_lease = lifecycle.try_begin(true).unwrap();

            for _ in 0..stale_result_count {
                prop_assert!(!lifecycle.terminate_retry_request(stale_lease));
                prop_assert!(!lifecycle.terminate_active(stale_lease));
            }

            prop_assert_eq!(lifecycle.phase, RestorePhase::Restoring);
            prop_assert_eq!(lifecycle.generation, current_lease.0);
            prop_assert!(lifecycle.commit_running(current_lease));
        }

        #[test]
        fn aba_stale_retry_results_preserve_newer_idle_generation(
            stale_result_count in 0_usize..32,
        ) {
            let mut lifecycle = RestoreLifecycle::new();
            let stale_lease = lifecycle.try_begin(true).unwrap();
            prop_assert_eq!(
                lifecycle.record_failure(stale_lease),
                Some(RestoreFailureDisposition::Retry)
            );
            prop_assert!(lifecycle.release_retry(stale_lease));

            let current_lease = lifecycle.try_begin(true).unwrap();
            prop_assert_eq!(
                lifecycle.record_failure(current_lease),
                Some(RestoreFailureDisposition::Retry)
            );
            prop_assert!(lifecycle.release_retry(current_lease));
            let current_state = lifecycle;

            for _ in 0..stale_result_count {
                prop_assert!(!lifecycle.terminate_retry_request(stale_lease));
                prop_assert_eq!(lifecycle, current_state);
            }

            prop_assert!(lifecycle.terminate_retry_request(current_lease));
            prop_assert_eq!(lifecycle.phase, RestorePhase::Terminal);
        }
    }
}
