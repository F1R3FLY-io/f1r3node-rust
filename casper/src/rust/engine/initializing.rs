// See casper/src/main/scala/coop/rchain/casper/engine/Initializing.scala

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::future::Future;
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
use dashmap::DashSet;
use futures::stream::StreamExt;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, BlockMessage, CasperMessage, FloorCacheResponse, MergeableEntryRequest,
    MergeableEntryResponse, StoreItemsMessage, StoreItemsMessageRequest,
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

fn install_slot<V>(slot: &Arc<Mutex<Option<V>>>, value: V, name: &str) -> Result<(), CasperError> {
    let mut guard = slot
        .lock()
        .map_err(|_| CasperError::RuntimeError(format!("Failed to acquire {} lock", name)))?;
    *guard = Some(value);
    Ok(())
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
    block_processing_queue_tx:
        mpsc::Sender<(Arc<dyn MultiParentCasper + Send + Sync>, BlockMessage)>,
    blocks_in_processing: Arc<DashSet<BlockHash>>,
    casper_shard_conf: CasperShardConf,
    validator_id: Option<ValidatorIdentity>,
    the_init: Arc<
        dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> + Send + Sync,
    >,
    block_message_rx: Arc<Mutex<Option<mpsc::Receiver<BlockMessage>>>>,
    tuple_space_rx: Arc<Mutex<Option<mpsc::Receiver<StoreItemsMessage>>>>,
    /// Receives `MergeableEntryResponse` routed from `handle_message_recv`.
    /// Drained by `lfs_block_requester::stream` during `request_approved_state`.
    mergeable_message_rx: Arc<Mutex<Option<mpsc::Receiver<MergeableEntryResponse>>>>,
    // Senders to enqueue messages from `handle` (producer side)
    pub block_message_tx: Arc<Mutex<Option<mpsc::Sender<BlockMessage>>>>,
    pub tuple_space_tx: Arc<Mutex<Option<mpsc::Sender<StoreItemsMessage>>>>,
    pub mergeable_message_tx: Arc<Mutex<Option<mpsc::Sender<MergeableEntryResponse>>>>,
    block_message_queue_pending: Arc<AtomicUsize>,
    tuple_space_queue_pending: Arc<AtomicUsize>,
    trim_state: bool,
    disable_state_exporter: bool,

    // TEMP: flag for single call for process approved block (Scala: `val startRequester = Ref.unsafe(true)`)
    start_requester: Arc<Mutex<bool>>,
    init_started_at: Arc<Mutex<Option<Instant>>>,
    no_approved_block_retries: Arc<Mutex<u64>>,
    /// Restore attempts that got an approved block and failed to use it. Kept
    /// apart from `no_approved_block_retries`, which counts waiting for a peer
    /// to have an answer at all: these are bounded, that one is not.
    restore_failures: Arc<Mutex<u64>>,
    /// Event publisher for F1r3fly events
    event_publisher: F1r3flyEvents,

    block_retriever: BlockRetriever<T>,
    engine_cell: Arc<EngineCell>,
    runtime_manager: Arc<RuntimeManager>,
    estimator: Arc<Mutex<Option<Estimator>>>,
    /// Shared reference to heartbeat signal for triggering immediate wake on deploy
    heartbeat_signal_ref: crate::rust::heartbeat_signal::HeartbeatSignalRef,
    /// Handed through to Running: routes incoming `StoreItemsMessage`s to the
    /// runtime state requester once this node is past its restore.
    state_items_tx: Option<mpsc::Sender<StoreItemsMessage>>,
    /// Routes incoming `FloorCacheResponse`s to `request_floor_cache`. Created
    /// internally; both halves live here like the other sync channels.
    floor_cache_tx: Arc<Mutex<Option<mpsc::Sender<FloorCacheResponse>>>>,
    floor_cache_rx: Arc<Mutex<Option<mpsc::Receiver<FloorCacheResponse>>>>,
}

/// Write shipped floor-cache entries into the DAG's floor and frontier
/// indices. Only entries that were SOLICITED and whose block this node holds
/// are written — a peer cannot seed floors for blocks we did not ask about.
/// The floor value itself may sit below the held window; a later walk that
/// needs it defers and names it, which is one bounded fetch, not a crawl.
fn apply_floor_cache_entries(
    dag: &block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
    solicited: &HashSet<BlockHash>,
    entries: Vec<models::rust::casper::protocol::casper_message::FloorCacheEntry>,
) -> Result<usize, CasperError> {
    let mut written = 0usize;
    for entry in entries {
        if !solicited.contains(&entry.block_hash) || !dag.contains(&entry.block_hash) {
            continue;
        }
        dag.put_cached_floor(entry.block_hash.clone(), entry.floor_hash)?;
        dag.put_cached_frontier(entry.block_hash, entry.frontier_hash)?;
        written += 1;
    }
    Ok(written)
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
        block_processing_queue_tx: mpsc::Sender<(
            Arc<dyn MultiParentCasper + Send + Sync>,
            BlockMessage,
        )>,
        blocks_in_processing: Arc<DashSet<BlockHash>>,
        casper_shard_conf: CasperShardConf,
        validator_id: Option<ValidatorIdentity>,
        the_init: Arc<
            dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> + Send + Sync,
        >,
        block_message_tx: mpsc::Sender<BlockMessage>,
        block_message_rx: mpsc::Receiver<BlockMessage>,
        tuple_space_tx: mpsc::Sender<StoreItemsMessage>,
        tuple_space_rx: mpsc::Receiver<StoreItemsMessage>,
        mergeable_message_tx: mpsc::Sender<MergeableEntryResponse>,
        mergeable_message_rx: mpsc::Receiver<MergeableEntryResponse>,
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
        let (floor_cache_tx, floor_cache_rx) = mpsc::channel::<FloorCacheResponse>(4);
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
            validator_id,
            the_init,
            block_message_rx: Arc::new(Mutex::new(Some(block_message_rx))),
            tuple_space_rx: Arc::new(Mutex::new(Some(tuple_space_rx))),
            mergeable_message_rx: Arc::new(Mutex::new(Some(mergeable_message_rx))),
            block_message_tx: Arc::new(Mutex::new(Some(block_message_tx))),
            tuple_space_tx: Arc::new(Mutex::new(Some(tuple_space_tx))),
            mergeable_message_tx: Arc::new(Mutex::new(Some(mergeable_message_tx))),
            block_message_queue_pending: Arc::new(AtomicUsize::new(0)),
            tuple_space_queue_pending: Arc::new(AtomicUsize::new(0)),
            trim_state,
            disable_state_exporter,
            start_requester: Arc::new(Mutex::new(true)),
            init_started_at: Arc::new(Mutex::new(None)),
            no_approved_block_retries: Arc::new(Mutex::new(0)),
            restore_failures: Arc::new(Mutex::new(0)),
            event_publisher,
            block_retriever,
            engine_cell,
            runtime_manager,
            estimator: Arc::new(Mutex::new(Some(estimator))),
            heartbeat_signal_ref,
            state_items_tx,
            floor_cache_tx: Arc::new(Mutex::new(Some(floor_cache_tx))),
            floor_cache_rx: Arc::new(Mutex::new(Some(floor_cache_rx))),
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
                // Forward to the channel drained by lfs_block_requester::stream.
                let sender = self.mergeable_message_tx.lock().unwrap().as_ref().cloned();
                if let Some(tx) = sender {
                    if let Err(e) = tx.send(resp).await {
                        tracing::warn!(
                            "Failed to enqueue MergeableEntryResponse into mergeable channel: {:?}",
                            e
                        );
                    }
                } else {
                    tracing::warn!(
                        "mergeable_message_tx sender is None; mergeable channel not available (message dropped)"
                    );
                }
                Ok(())
            }
            CasperMessage::FloorCacheResponse(resp) => {
                let sender = self.floor_cache_tx.lock().unwrap().as_ref().cloned();
                if let Some(tx) = sender {
                    if tx.try_send(resp).is_err() {
                        tracing::warn!(
                            "floor-cache channel full or closed; dropping response (the \
                             request loop re-asks)"
                        );
                    }
                }
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
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
            )?;

            initializing.request_approved_state(approved_block).await?;

            initializing
                .block_store
                .put_approved_block(approved_block)?;

            {
                let mut last_approved = initializing.last_approved_block.lock().unwrap();
                *last_approved = Some(approved_block.clone());
            }

            let _ = initializing
                .event_publisher
                .publish(F1r3flyEvent::approved_block_received(
                    PrettyPrinter::build_string_no_limit(&block.block_hash),
                ));

            tracing::info!(
                "Approved state for block {} is successfully restored.",
                PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true)
            );

            Ok(())
        }

        // TODO: Scala resolve validation of approved block - we should be sure that bootstrap is not lying
        // Might be Validate.approvedBlock is enough but have to check
        let validate_ok = Validate::approved_block(&approved_block);
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

        let start = {
            let mut requester = self.start_requester.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire start_requester lock".to_string())
            })?;
            match (*requester, is_valid) {
                (true, true) => {
                    *requester = false;
                    true
                }
                (true, false) => {
                    // *requester stays true (no change needed)
                    false
                }
                _ => false,
            }
        };

        if start {
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
            // The caller is a task the transport spawned for this message, and it
            // logs an error and drops it. Anything left unhandled here is
            // therefore lost, and the engine waits forever for a restore that
            // already failed.
            if let Err(err) = handle_approved_block(self, &approved_block).await {
                self.recover_from_restore_failure(err).await?;
            }
        }
        Ok(())
    }

    /// Put the engine back where it stood before the approved block arrived, and
    /// ask for one again.
    ///
    /// Two pieces of state outlive a failed attempt and would each defeat the
    /// next one silently: `request_approved_state` TOOK the sync-channel
    /// receivers, and `on_approved_block` closed the gate that lets an approved
    /// block start a restore at all. Both are restored before re-requesting.
    ///
    /// Bounded, unlike the `NoApprovedBlockAvailable` retry. That one waits for a
    /// peer that has no answer yet, and waiting is the right thing to do
    /// indefinitely. This one had an answer and could not use it, which is a
    /// fault to surface rather than to hide behind an endless loop.
    pub async fn recover_from_restore_failure(&self, err: CasperError) -> Result<(), CasperError> {
        const MAX_RESTORE_ATTEMPTS: u64 = 3;

        let attempts = {
            let mut failures = self.restore_failures.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire restore_failures lock".to_string())
            })?;
            *failures += 1;
            *failures
        };

        tracing::error!(
            error = %err,
            attempt = attempts,
            "Approved-state restore failed; this node holds no state it can run on"
        );

        if attempts >= MAX_RESTORE_ATTEMPTS {
            return Err(CasperError::RuntimeError(format!(
                "approved-state restore failed {} times, giving up; last error: {}",
                attempts, err
            )));
        }

        self.reinstall_sync_channels()?;
        {
            let mut requester = self.start_requester.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire start_requester lock".to_string())
            })?;
            *requester = true;
        }

        tracing::info!(
            attempt = attempts,
            "Re-requesting approved state after a failed restore"
        );
        self.transport_layer
            .request_approved_block(&self.rp_conf_ask, Some(self.trim_state))
            .await
            .map_err(CasperError::CommError)
    }

    /// Fresh sync channels for a retry.
    ///
    /// `request_approved_state` takes each receiver out of its slot and hands it
    /// to a requester, so a second attempt finds the slots empty and fails
    /// before it starts. The pending counters go with them: they describe queues
    /// that no longer exist.
    fn reinstall_sync_channels(&self) -> Result<(), CasperError> {
        // The sizing the engine is constructed with (see `engine::transition_to_initializing`).
        const SYNC_CHANNEL_CAPACITY: usize = 50;

        let (block_tx, block_rx) = mpsc::channel::<BlockMessage>(SYNC_CHANNEL_CAPACITY);
        let (tuple_tx, tuple_rx) = mpsc::channel::<StoreItemsMessage>(SYNC_CHANNEL_CAPACITY);
        let (mergeable_tx, mergeable_rx) =
            mpsc::channel::<MergeableEntryResponse>(SYNC_CHANNEL_CAPACITY);

        install_slot(&self.block_message_tx, block_tx, "block_message_tx")?;
        install_slot(&self.block_message_rx, block_rx, "block_message_rx")?;
        install_slot(&self.tuple_space_tx, tuple_tx, "tuple_space_tx")?;
        install_slot(&self.tuple_space_rx, tuple_rx, "tuple_space_rx")?;
        install_slot(
            &self.mergeable_message_tx,
            mergeable_tx,
            "mergeable_message_tx",
        )?;
        install_slot(
            &self.mergeable_message_rx,
            mergeable_rx,
            "mergeable_message_rx",
        )?;

        self.block_message_queue_pending.store(0, Ordering::Relaxed);
        self.tuple_space_queue_pending.store(0, Ordering::Relaxed);
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
        let unseeded_min_block_number =
            crate::rust::util::rspace_history_horizon::lfs_min_block_number(
                start_block_number,
                self.casper_shard_conf.deploy_lifespan,
                self.casper_shard_conf.max_parent_depth,
                self.casper_shard_conf.mergeable_channels_gc_depth_buffer,
            );

        // The anchor's floor cannot be derived from anything above the anchor,
        // so the responder sent it. Widening the window here — before a single
        // block is requested — is the only chance to do it: the requester
        // accepts a block only if its height clears this bound, and the seed's
        // blocks are useless to us unless we hold them.
        let min_block_number_for_deploy_lifespan = match &approved_block.floor_seed {
            Some(seed) => crate::rust::util::rspace_history_horizon::lfs_seeded_min_block_number(
                unseeded_min_block_number,
                seed.floor_number,
                seed.frontier_number,
            ),
            None => unseeded_min_block_number,
        };

        // How deep rspace state and the mergeable replay reach. Shallower than
        // the block floor by design — see `lfs_min_state_block_number`.
        let min_state_block_number =
            crate::rust::util::rspace_history_horizon::lfs_min_state_block_number(
                start_block_number,
                self.casper_shard_conf.max_parent_depth,
                self.casper_shard_conf.mergeable_channels_gc_depth_buffer,
            );

        tracing::info!(
            "request_approved_state: start (block {}, min_height {}, state_min_height {}, unseeded_min_height {}, seeded {})",
            PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true),
            min_block_number_for_deploy_lifespan,
            min_state_block_number,
            unseeded_min_block_number,
            approved_block.floor_seed.is_some()
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

        let mergeable_response_rx = self
            .mergeable_message_rx
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| {
                CasperError::RuntimeError(
                    "Mergeable-entry response receiver not available".to_string(),
                )
            })?;

        // Create block requester wrapper with needed components and stream
        let mut block_requester = BlockRequesterWrapper::new(
            &self.transport_layer,
            &self.connections_cell,
            &self.rp_conf_ask,
            self.block_store.clone(),
            Box::new(|block| self.validate_block(block)),
        )
        .with_runtime_manager(self.runtime_manager.clone());

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
                mergeable_response_rx,
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
            // After the blocks are in, never before: the caches are keyed by
            // hash, and a floor entry pointing at a block the DAG does not hold
            // reads back as a missing block rather than as a floor.
            self.seed_floor_caches(approved_block)?;
            self.request_floor_cache(approved_block).await;
        } else {
            tracing::warn!(
                "request_approved_state: block_request_stream returned no final state (None)"
            );
        }

        // Forward-horizon rspace history sync — ship rspace state for every
        // block from `min_state_block_number` up. That floor is the parent
        // reach, NOT the block-download floor: state is only ever needed for
        // blocks that can be executed, and `validate::parents` rejects any block
        // naming a parent below the reach. See
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
                .map_err(CasperError::from)?;

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
        // Merge of dev (EPOCH-004): dev's `lfs_block_requester::stream` now
        // routes `MergeableEntryResponse` messages through
        // `mergeable_message_rx` to populate the cache during sync, which
        // covers most of the same ground. This explicit replay is the
        // defensive companion path — it catches any block whose mergeable
        // entry was missed by the streaming response (e.g. a peer that
        // didn't ship one), and is a no-op when the cache is already warm.
        // Bounded by the state floor, not the block floor: the replay executes
        // blocks, so it must not walk below the history the horizon sync just
        // imported, and a block below the parent reach is never merged on.
        self.replay_blocks_for_mergeable_channels(approved_block, min_state_block_number)
            .await?;

        // Transition to Running state
        tracing::info!("request_approved_state: transitioning to Running");
        self.create_casper_and_transition_to_running(approved_block)
            .await?;
        tracing::info!("request_approved_state: transition_to_running completed");

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

    /// Receive finality for the restored window instead of re-deriving it.
    ///
    /// `floor_of_block` recurses until a CACHED floor or genesis, and a
    /// restored node has cached floors for nothing below its anchor — so the
    /// first sibling-branch validation walks toward genesis, surfacing one
    /// missing ancient block per retry cycle. The responder computed every
    /// window block's floor when it validated it; asking for those values
    /// makes the recursion terminate inside the window at any chain height,
    /// for O(window) bytes.
    ///
    /// Failure degrades to that crawl — alive and alarmed, never a wedge — so
    /// this never fails the restore.
    async fn request_floor_cache(&self, approved_block: &ApprovedBlock) {
        const ATTEMPTS: u32 = 3;
        const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);

        // A genesis-anchored restore holds a parentless root, so every floor
        // derivation terminates locally — the same discriminator guard_deferral
        // and the settled door use. Nothing to ask for.
        if proto_util::block_number(&approved_block.candidate.block) == 0 {
            return;
        }

        let result: Result<(), CasperError> = async {
            let dag = self.block_dag_storage.get_representation()?;
            let hashes: Vec<BlockHash> = dag.dag_set.iter().cloned().collect();
            let solicited: HashSet<BlockHash> = hashes.iter().cloned().collect();
            let mut rx = self.floor_cache_rx.lock().unwrap().take().ok_or_else(|| {
                CasperError::RuntimeError("floor-cache receiver not available".to_string())
            })?;

            let request = models::rust::casper::protocol::casper_message::FloorCacheRequest {
                hashes: hashes.clone(),
            };
            for attempt in 1..=ATTEMPTS {
                self.transport_layer
                    .send_to_bootstrap(&self.rp_conf_ask, Arc::new(request.clone().to_proto()))
                    .await?;
                match tokio::time::timeout(RESPONSE_TIMEOUT, rx.recv()).await {
                    Ok(Some(response)) => {
                        // The genesis hash rides the same trusted exchange:
                        // a truncated node holds no height-0 block, and the
                        // newly-bonded latest-message placeholder must be
                        // this network-uniform value on every node.
                        if !response.genesis_hash.is_empty() {
                            self.block_dag_storage
                                .record_genesis_hash(response.genesis_hash.clone())?;
                        }
                        let written =
                            apply_floor_cache_entries(&dag, &solicited, response.entries)?;
                        tracing::info!(
                            written,
                            requested = hashes.len(),
                            genesis_learned = !response.genesis_hash.is_empty(),
                            "Floor cache received: restored blocks carry their finality"
                        );
                        return Ok(());
                    }
                    Ok(None) => {
                        return Err(CasperError::RuntimeError(
                            "floor-cache channel closed".to_string(),
                        ));
                    }
                    Err(_) => {
                        tracing::warn!(attempt, "floor-cache request timed out; retrying");
                    }
                }
            }
            Err(CasperError::RuntimeError(format!(
                "no floor-cache response after {} attempts",
                ATTEMPTS
            )))
        }
        .await;

        if let Err(e) = result {
            tracing::warn!(
                error = %e,
                "Proceeding without the shipped floor cache: sibling-branch validation \
                 will re-derive finality gap-by-gap (slow, not wrong)"
            );
        }
    }

    /// Record the anchor's floor and frontier as this node's own, so the first
    /// block above the anchor inherits them instead of trying to re-derive
    /// finality from history that stops at the anchor.
    ///
    /// This is what makes a restored node able to judge at all. Both entries are
    /// pure functions of the anchor, so accepting them from the peer that served
    /// the anchor adds no trust: that peer already chose the anchor, and every
    /// block below it arrived by hash from the anchor's own ancestry.
    ///
    /// A seed naming a block the download did not reach is refused rather than
    /// written — a floor entry pointing at absent history would turn every later
    /// derivation into a deferral, which is worse than having no seed at all,
    /// because it looks like success.
    fn seed_floor_caches(&self, approved_block: &ApprovedBlock) -> Result<(), CasperError> {
        let Some(seed) = &approved_block.floor_seed else {
            tracing::warn!(
                "LFS restore has no floor seed: this node cannot derive finality above \
                 its anchor and will defer on blocks it cannot judge. The serving peer \
                 predates the seed, or could not derive one."
            );
            return Ok(());
        };

        let anchor = approved_block.candidate.block.block_hash.clone();
        let dag = self.block_dag_storage.get_representation()?;
        for (hash, label) in [
            (&seed.floor_hash, "floor"),
            (&seed.frontier_hash, "frontier"),
        ] {
            if !dag.contains(hash) {
                tracing::warn!(
                    seeded = %PrettyPrinter::build_string_bytes(hash),
                    kind = label,
                    "Floor seed names a block the sync did not download; discarding the \
                     seed rather than caching a floor this node cannot read"
                );
                return Ok(());
            }
        }

        dag.put_cached_floor(anchor.clone(), seed.floor_hash.clone())?;
        dag.put_cached_frontier(anchor.clone(), seed.frontier_hash.clone())?;
        tracing::info!(
            anchor = %PrettyPrinter::build_string_bytes(&anchor),
            floor = %PrettyPrinter::build_string_bytes(&seed.floor_hash),
            floor_number = seed.floor_number,
            frontier = %PrettyPrinter::build_string_bytes(&seed.frontier_hash),
            frontier_number = seed.frontier_number,
            "Seeded the anchor's finalized floor and frontier from the approved block"
        );
        Ok(())
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

        let mut inserted = 0usize;

        // Add sorted DAG in order from approved block to oldest
        for hash in blocks_for_dag(&height_map) {
            // NOTE: This is not in original Scala code. Added because we changed block_store
            // to Option<KeyValueBlockStore> to support moving it in create_casper_and_transition_to_running
            let block = self.block_store.get_unsafe(&hash);
            // If sender has stake 0 in approved block, this means that sender has been slashed and block is invalid
            let is_invalid = invalid_blocks.contains(&block.block_hash.to_vec());
            add_block_to_dag(self, &block, is_invalid).await?;
            inserted += 1;
        }

        // What the horizon walk will actually see. The requester's `finished`
        // count is not this number — it counts downloads, and anything the
        // height map dropped never reaches the DAG. `min_height` is the
        // requester's bound, reported because the DAG now reaches below it.
        tracing::info!(
            inserted,
            min_height,
            lowest_height = height_map.keys().next(),
            "Blocks for approved state added to DAG."
        );
        Ok(())
    }

    /// Replay blocks in topological order to populate the mergeable channel cache.
    /// This is necessary for multi-parent block validation, which requires mergeable
    /// channel data from parent blocks to compute merged state.
    ///
    /// The LFS sync transfers the RSpace trie but not the mergeable channel store,
    /// so we must regenerate any entries the streaming `MergeableEntryRequest`/
    /// `MergeableEntryResponse` exchange in `lfs_block_requester` didn't already
    /// fill in (see its `process_mergeable_entry`). That streaming path covers
    /// every block downloaded during sync when the responding peer has an
    /// entry, so this function is expected to find most (often all) blocks
    /// already cached — it exists to catch the remainder (peer soft-misses,
    /// blocks the node already had locally before this sync, etc).
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

        // Replay each block to populate mergeable channels, skipping any block
        // whose entry is already cached (typically filled in by the streaming
        // MergeableEntryResponse exchange during block download above).
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
                .get_mergeable_entry_bytes(&block)
                .map(|(_, value)| value.is_some())
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

        let deploys = proto_util::deploys(block);
        let system_deploys = proto_util::system_deploys(block);
        let block_data = rholang::rust::interpreter::system_processes::BlockData::from_block(block);

        // Genesis starts from empty state
        let pre_state_hash = RuntimeManager::empty_state_hash_fixed();

        // Replay genesis - this will save mergeable channels to the store.
        // `Arc<RuntimeManager>` is interior-mutable post rspace++ rewrite, so no
        // outer Mutex / lock acquisition is required.
        let result = self
            .runtime_manager
            .replay_compute_state(
                &pre_state_hash,
                deploys,
                system_deploys,
                &block_data,
                None, // No invalid blocks for genesis
                true, // isGenesis = true
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

        let deploys = proto_util::deploys(block);
        let system_deploys = proto_util::system_deploys(block);
        let block_data = rholang::rust::interpreter::system_processes::BlockData::from_block(block);
        let is_genesis = parents.is_empty();

        // Get invalid blocks map for replay
        let dag = self.block_dag_storage.get_representation()?;
        let invalid_blocks_map = dag.invalid_blocks_map()?;

        tracing::debug!(
            "Replaying block #{} ({}) with {} deploys, {} parents",
            block_number,
            PrettyPrinter::build_string_bytes(block_hash),
            deploys.len(),
            parents.len()
        );

        // Replay the block - this will save mergeable channels to the store.
        // `Arc<RuntimeManager>` is interior-mutable post rspace++ rewrite, so no
        // outer Mutex / lock acquisition is required.
        let result = self
            .runtime_manager
            .replay_compute_state(
                &pre_state_hash,
                deploys,
                system_deploys,
                &block_data,
                Some(invalid_blocks_map),
                is_genesis,
            )
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

        let estimator = self
            .estimator
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| CasperError::RuntimeError("Estimator not available".to_string()))?;
        let recovery_estimator = estimator.clone();

        // The on-chain fault-tolerance threshold is read and adopted by
        // `hash_set_casper` (the single adoption point shared by all three
        // casper constructors), so this path deliberately does NOT read it
        // again — a second read here would be a second policy site and could
        // drift from the one the running casper actually finalizes with.
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

        // **Scala equivalent**: `transitionToRunning[F](...)`
        tracing::info!("create_casper_and_transition_to_running: calling transition_to_running");

        // Create empty async init (matches Scala ().pure[F])
        let the_init = Arc::new(|| {
            Box::pin(async { Ok(()) })
                as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
        });

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
                last_approved_block: self.last_approved_block.clone(),
                block_store: self.block_store.clone(),
                block_dag_storage: self.block_dag_storage.clone(),
                deploy_storage: self.deploy_storage.clone(),
                rejected_deploy_buffer: self.rejected_deploy_buffer.clone(),
                casper_buffer_storage: self.casper_buffer_storage.clone(),
                rspace_state_manager: self.rspace_state_manager.clone(),
                event_publisher: self.event_publisher.clone(),
                engine_cell: self.engine_cell.clone(),
                runtime_manager: self.runtime_manager.clone(),
                estimator: recovery_estimator,
                casper_shard_conf: self.casper_shard_conf.clone(),
                heartbeat_signal_ref: self.heartbeat_signal_ref.clone(),
            }),
            &self.engine_cell,
            &self.event_publisher,
            self.state_items_tx.clone(),
        )
        .await?;

        if let Some(started_at) = *self.init_started_at.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire init_started_at lock".to_string())
        })? {
            let elapsed = started_at.elapsed();
            metrics::histogram!(
                CASPER_INIT_TIME_TO_RUNNING_METRIC,
                "source" => CASPER_METRICS_SOURCE
            )
            .record(elapsed.as_secs_f64());
        }

        tracing::info!(
            "create_casper_and_transition_to_running: transition_to_running completed successfully"
        );

        // Guard joiners (first-time connections requesting an approved block from
        // peers) against config drift: the node's local native-token-* values
        // must match what this network baked into the TokenMetadata contract at
        // genesis. See casper/src/rust/util/token_metadata_check.rs for details.
        crate::rust::util::token_metadata_check::verify_token_metadata_matches_config(
            &self.runtime_manager,
            &genesis_post_state_hash,
            &self.casper_shard_conf.native_token_name,
            &self.casper_shard_conf.native_token_symbol,
            self.casper_shard_conf.native_token_decimals,
        )
        .await?;

        self.transport_layer
            .send_fork_choice_tip_request(&self.connections_cell, &self.rp_conf_ask)
            .await
            .map_err(CasperError::CommError)?;

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

    async fn request_for_mergeable_entry(&self, block_hash: &BlockHash) -> Result<(), CasperError> {
        let req = MergeableEntryRequest {
            block_hash: block_hash.clone(),
        };
        self.transport_layer
            .send_message_to_peers(
                self.connections_cell,
                self.rp_conf_ask,
                Arc::new(req.to_proto()),
                None,
            )
            .await
            .map_err(|e| CasperError::RuntimeError(e.to_string()))?;
        Ok(())
    }

    fn put_mergeable_entry(
        &self,
        block_hash: &BlockHash,
        serialized_entry: &[u8],
    ) -> Result<(), CasperError> {
        if serialized_entry.is_empty() {
            return Ok(());
        }
        // Look up the block to compute the local mergeable_key (must match
        // the server's key exactly; both sides derive it from the block's
        // post_state/sender/seq).
        let block = self.block_store.get_unsafe(block_hash);
        let key_bytes = RuntimeManager::mergeable_key_bytes_for_block(&block)?;
        let runtime_manager = self.runtime_manager.as_ref().ok_or_else(|| {
            CasperError::RuntimeError(
                "BlockRequesterWrapper missing runtime_manager (mergeable-entry import)"
                    .to_string(),
            )
        })?;
        runtime_manager.put_mergeable_entry_bytes(key_bytes, serialized_entry.to_vec())
    }
}

/// Wrapper struct for block request operations
pub struct BlockRequesterWrapper<'a, T: TransportLayer> {
    transport_layer: &'a T,
    connections_cell: &'a ConnectionsCell,
    rp_conf_ask: &'a RPConf,
    block_store: KeyValueBlockStore,
    validate_block_fn: Box<dyn Fn(&BlockMessage) -> bool + Send + Sync + 'a>,
    /// Optional runtime_manager handle for the mergeable-channels store
    /// import path. Required in production; optional for tests that don't
    /// exercise the mergeable path.
    runtime_manager: Option<Arc<RuntimeManager>>,
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
            runtime_manager: None,
        }
    }

    /// Attach a `RuntimeManager` so the wrapper can import mergeable-channel
    /// entries.
    pub fn with_runtime_manager(mut self, runtime_manager: Arc<RuntimeManager>) -> Self {
        self.runtime_manager = Some(runtime_manager);
        self
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
        );
        Ok(())
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

/// The blocks the restored DAG must hold, highest height first.
///
/// The height map is the requester's own record of what it downloaded and
/// validated, and it fetched each of those because something needs it — it
/// reaches below its own bound to pick up the parents of every latest message.
/// A second bound applied here can only disagree with the first, and the DAG
/// then holds blocks whose parents it threw away.
fn blocks_for_dag(height_map: &BTreeMap<i64, HashSet<BlockHash>>) -> Vec<BlockHash> {
    height_map
        .iter()
        .rev()
        .flat_map(|(_, hashes)| hashes.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(tag: &[u8]) -> BlockHash { BlockHash::from(tag.to_vec()) }

    /// Shipped floor entries are written only for blocks this node asked about
    /// AND holds. Anything else in the response is a peer trying to seed
    /// finality for blocks outside the window — ignored, not an error, so a
    /// partially-covering response still lands everything legitimate.
    #[test]
    fn shipped_floor_entries_apply_only_to_solicited_held_blocks() {
        use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
        use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
        use models::rust::casper::protocol::casper_message::FloorCacheEntry;
        use parking_lot::RwLock as PlRwLock;
        use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
        use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

        let held = BlockHash::from(vec![0x11; 32]);
        let unheld = BlockHash::from(vec![0x22; 32]);
        let floor = BlockHash::from(vec![0x33; 32]);

        let mut dag_set = imbl::HashSet::new();
        dag_set.insert(held.clone());
        let dag = KeyValueDagRepresentation {
            dag_set,
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: imbl::HashMap::new(),
            main_parent_map: imbl::HashMap::new(),
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: prost::bytes::Bytes::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index: Arc::new(PlRwLock::new(BlockMetadataStore::new(
                KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            ))),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            lifecycle: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(
                ),
            )),
        };

        let entry = |hash: &BlockHash| FloorCacheEntry {
            block_hash: hash.clone(),
            floor_hash: floor.clone(),
            frontier_hash: floor.clone(),
        };
        let solicited = HashSet::from([held.clone(), unheld.clone()]);
        let written = apply_floor_cache_entries(&dag, &solicited, vec![
            entry(&held),
            entry(&unheld),
            entry(&BlockHash::from(vec![0x44; 32])),
        ])
        .expect("apply");

        assert_eq!(
            written, 1,
            "only the solicited AND held block is written; the unheld and the \
             unsolicited entries are ignored"
        );
        assert_eq!(
            dag.get_cached_floor(&held).expect("read"),
            Some(floor.clone()),
            "the held block's floor landed in the same cache its own validation \
             would have filled"
        );
        assert_eq!(
            dag.get_cached_floor(&unheld).expect("read"),
            None,
            "a peer cannot seed finality for a block this node does not hold"
        );
    }

    /// An LFS restore must keep every block it fetched. The requester lowers its
    /// own bound to `height - 1` for each latest message, so the download reaches
    /// under `min_height` by design; discarding those leaves the DAG's oldest
    /// blocks with parents it does not hold, and every walk that expands them
    /// dies on DAGStorageMissingHash. Observed twice: block #82 dropped under a
    /// window starting at 84, then #68 under one starting at 70 — each time the
    /// block had been downloaded from every peer moments earlier.
    #[test]
    fn every_downloaded_block_reaches_the_dag() {
        let deep = hash(b"below-the-window");
        let boundary = hash(b"at-the-window");
        let tip = hash(b"tip");
        let height_map = BTreeMap::from([
            (68, HashSet::from([deep.clone()])),
            (70, HashSet::from([boundary.clone()])),
            (120, HashSet::from([tip.clone()])),
        ]);

        let selected = blocks_for_dag(&height_map);

        assert!(
            selected.contains(&deep),
            "a block the requester downloaded below the window must still reach the \
             DAG; dropping it leaves #70's ancestry unresolvable"
        );
        assert_eq!(
            selected.len(),
            3,
            "every height-map entry is a block that was fetched and validated"
        );
        assert_eq!(
            selected.first(),
            Some(&tip),
            "highest height first: inserting descending keeps each sender's latest \
             message at its highest sequence number"
        );
    }
}
