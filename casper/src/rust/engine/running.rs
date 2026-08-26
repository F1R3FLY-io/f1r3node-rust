// See casper/src/main/scala/coop/rchain/casper/engine/Running.scala

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use dashmap::DashSet;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    self, ApprovedBlock, BlockHashMessage, BlockRequest, CasperMessage, HasBlock, HasBlockRequest,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporterInstance;

use crate::rust::blocks::block_processing_queue::BlockProcessingQueueSender;
use crate::rust::casper::MultiParentCasper;
use crate::rust::engine::block_retriever::{self, BlockRetriever};
use crate::rust::engine::engine::{self, Engine};
use crate::rust::engine::engine_cell::EngineCell;
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_HASH_RECEIVED_METRIC, BLOCK_REQUEST_RECEIVED_METRIC, RUNNING_METRICS_SOURCE,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CasperMessageStatus {
    BlockIsInDag,
    BlockIsInCasperBuffer,
    BlockIsReceived,
    BlockIsWaitingForCasper,
    BlockIsInProcessing,
    DoNotIgnore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreCasperMessageStatus {
    pub do_ignore: bool,
    pub status: CasperMessageStatus,
}

/**
 * As we introduced synchrony constraint - there might be situation when node is stuck.
 * As an edge case with `sync = 0.99`, if node misses the block that is the last one to meet sync constraint,
 * it has no way to request it after it was broadcasted. So it will never meet synchrony constraint.
 * To mitigate this issue we can update fork choice tips if current fork-choice tip has old timestamp,
 * which means node does not propose new blocks and no new blocks were received recently.
 */
pub async fn update_fork_choice_tips_if_stuck<T: TransportLayer + Send + Sync>(
    engine_cell: &EngineCell,
    transport: &Arc<T>,
    connections_cell: &ConnectionsCell,
    conf: &RPConf,
    delay_threshold: Duration,
) -> Result<(), CasperError> {
    // Get engine from engine cell
    let engine = engine_cell.get().await;

    // Check if we have casper
    if let Some(casper) = engine.with_casper() {
        // Get latest messages from block dag
        let latest_messages = casper.block_dag().await?.latest_message_hashes();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Check if any latest message is recent
        let mut has_recent_latest_message = false;
        for (_, block_hash) in latest_messages.iter() {
            if let Ok(Some(block)) = casper.block_store().get(block_hash) {
                let block_timestamp = block.header.timestamp;
                if (now - block_timestamp) < delay_threshold.as_millis() as i64 {
                    has_recent_latest_message = true;
                    break;
                }
            }
        }

        // If stuck, request fork choice tips
        let stuck = !has_recent_latest_message;
        let recovering = engine.recover_stuck_validator(delay_threshold).await?;
        if stuck && !recovering {
            tracing::info!(
                "Requesting tips update as newest latest message is more than {:?} old. Might be network is faulty.",
                delay_threshold
            );
            transport
                .send_fork_choice_tip_request(connections_cell, conf)
                .await?;
        }
    }

    Ok(())
}

#[async_trait]
impl<T: TransportLayer + Send + Sync + Clone + 'static> Engine for Running<T> {
    async fn init(&self) -> Result<(), CasperError> {
        {
            let mut init_called = self.init_called.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire init lock".to_string())
            })?;

            if *init_called {
                return Err(CasperError::RuntimeError(
                    "Init function already called".to_string(),
                ));
            }

            *init_called = true;
        }

        // Call the async init function and await it
        (self.the_init)().await?;
        Ok(())
    }

    async fn handle(&self, peer: PeerNode, msg: CasperMessage) -> Result<(), CasperError> {
        match msg {
            CasperMessage::BlockHashMessage(h) => {
                metrics::counter!(BLOCK_HASH_RECEIVED_METRIC, "source" => RUNNING_METRICS_SOURCE)
                    .increment(1);
                self.handle_block_hash_message(peer, h, |hash| self.ignore_casper_message(hash))
                    .await
            }
            CasperMessage::BlockMessage(b) => {
                if let Some(id) = self.casper.get_validator() {
                    if b.sender == id.public_key.bytes {
                        tracing::warn!(
                            "There is another node {} proposing using the same private key as you. Or did you restart your node?",
                            peer
                        );
                    }
                }
                if self.ignore_casper_message(b.block_hash.clone())? {
                    tracing::debug!(
                        "Ignoring BlockMessage {} from {}",
                        PrettyPrinter::build_string_block_message(&b, true),
                        peer.endpoint.host
                    );
                } else {
                    tracing::debug!(
                        "Incoming BlockMessage {} from {}",
                        PrettyPrinter::build_string_block_message(&b, true),
                        peer.endpoint.host
                    );
                    let block_hash = b.block_hash.clone();
                    if !self.blocks_in_processing.insert(block_hash.clone()) {
                        tracing::debug!(
                            "Skipping BlockMessage {} enqueue because it is already queued/in-processing",
                            PrettyPrinter::build_string_bytes(&block_hash)
                        );
                        return Ok(());
                    }
                    match self
                        .block_processing_queue_tx
                        .try_enqueue(self.casper.clone(), b)
                    {
                        Ok(()) => self.block_retriever.ack_receive(block_hash).await?,
                        Err(error) if error.failure.is_temporary() => {
                            self.blocks_in_processing.remove(&block_hash);
                            let tracked = self
                                .block_retriever
                                .defer_for_admission(block_hash.clone(), Some(peer))
                                .await?;
                            if tracked {
                                tracing::info!(
                                    error = %error,
                                    "Deferred BlockMessage {} for re-request",
                                    PrettyPrinter::build_string_bytes(&block_hash)
                                );
                            } else {
                                tracing::warn!(
                                    error = %error,
                                    "Released untracked BlockMessage {} at request-tracker capacity; a later hash announcement or dependency scan must readmit it",
                                    PrettyPrinter::build_string_bytes(&block_hash)
                                );
                            }
                        }
                        Err(error) => {
                            self.blocks_in_processing.remove(&block_hash);
                            return Err(CasperError::RuntimeError(error.to_string()));
                        }
                    }
                }
                Ok(())
            }
            CasperMessage::BlockRequest(br) => {
                metrics::counter!(BLOCK_REQUEST_RECEIVED_METRIC, "source" => RUNNING_METRICS_SOURCE).increment(1);
                self.handle_block_request(peer, br).await
            }

            CasperMessage::HasBlockRequest(hbr) => {
                self.handle_has_block_request(peer, hbr, |hash| self.casper.dag_contains(&hash))
                    .await
            }
            CasperMessage::HasBlock(hb) => {
                self.handle_has_block_message(peer, hb, |hash| self.ignore_casper_message(hash))
                    .await
            }
            CasperMessage::ForkChoiceTipRequest(_) => {
                self.handle_fork_choice_tip_request(peer).await
            }
            CasperMessage::ApprovedBlockRequest(abr) => {
                if abr.trim_state {
                    tracing::info!(
                        "Peer requested legacy trimmed ApprovedBlock; serving canonical genesis approval."
                    );
                }
                self.handle_approved_block_request(peer, self.approved_block.clone())
                    .await
            }
            CasperMessage::NoApprovedBlockAvailable(na) => {
                engine::log_no_approved_block_available(&na.node_identifier);
                Ok(())
            }
            CasperMessage::StoreItemsMessageRequest(req) => {
                let start = req
                    .start_path
                    .iter()
                    .map(RSpaceExporterInstance::path_pretty)
                    .collect::<Vec<_>>()
                    .join(" ");

                tracing::info!(
                    "Received request for store items, startPath: [{}], chunk: {}, skip: {}, from: {}",
                    start,
                    req.take,
                    req.skip,
                    peer
                );

                if !self.disable_state_exporter {
                    self.handle_state_items_message_request(
                        peer,
                        req.start_path,
                        req.skip as u32,
                        req.take as u32,
                    )
                    .await
                } else {
                    tracing::info!(
                        "Received StoreItemsMessage request but the node is configured to not respond to StoreItemsMessage, from {}.",
                        peer
                    );
                    Ok(())
                }
            }
            CasperMessage::MergeableEntryRequest(req) => {
                if self.disable_state_exporter {
                    tracing::debug!(
                        "Received MergeableEntryRequest but state-export is disabled; ignoring (from {}).",
                        peer
                    );
                    return Ok(());
                }
                self.handle_mergeable_entry_request(peer, req.block_hash)
                    .await
            }
            _ => Ok(()),
        }
    }

    /// Running always contains casper; enables `EngineDynExt::with_casper(...)`
    /// to mirror Scala `Engine.withCasper` behavior.
    async fn recover_stuck_validator(
        &self,
        delay_threshold: Duration,
    ) -> Result<bool, CasperError> {
        self.recover_stuck_validator_inner(delay_threshold).await
    }

    fn with_casper(&self) -> Option<Arc<dyn MultiParentCasper + Send + Sync>> {
        Some(Arc::clone(&self.casper) as Arc<dyn MultiParentCasper + Send + Sync>)
    }
}

// NOTE: Changed to use Arc<dyn MultiParentCasper> directly instead of generic M
// based on discussion with Steven for TestFixture compatibility - avoids ?Sized issues
pub struct Running<T: TransportLayer + Send + Sync> {
    block_processing_queue_tx: BlockProcessingQueueSender,
    blocks_in_processing: Arc<DashSet<BlockHash>>,
    casper: Arc<dyn MultiParentCasper + Send + Sync>,
    approved_block: ApprovedBlock,
    // Scala: theInit: F[Unit] - lazy async computation
    the_init: Arc<
        dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> + Send + Sync,
    >,
    init_called: Arc<Mutex<bool>>,
    disable_state_exporter: bool,
    transport: Arc<T>,
    conf: RPConf,
    block_retriever: BlockRetriever<T>,
    recovery_context: Option<RunningRecoveryContext>,
}

#[derive(Clone)]
pub struct RunningRecoveryContext {
    pub connections_cell: ConnectionsCell,
}

impl<T: TransportLayer + Send + Sync> Running<T> {
    pub fn new(
        block_processing_queue_tx: BlockProcessingQueueSender,
        blocks_in_processing: Arc<DashSet<BlockHash>>,
        casper: Arc<dyn MultiParentCasper + Send + Sync>,
        approved_block: ApprovedBlock,
        the_init: Arc<
            dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> + Send + Sync,
        >,
        disable_state_exporter: bool,
        transport: Arc<T>,
        conf: RPConf,
        block_retriever: BlockRetriever<T>,
        recovery_context: Option<RunningRecoveryContext>,
    ) -> Self {
        Running {
            block_processing_queue_tx,
            blocks_in_processing,
            casper,
            approved_block,
            the_init,
            init_called: Arc::new(Mutex::new(false)),
            disable_state_exporter,
            transport,
            conf,
            block_retriever,
            recovery_context,
        }
    }

    fn ignore_casper_message(&self, hash: BlockHash) -> Result<bool, CasperError> {
        let blocks_in_processing = self.blocks_in_processing.contains(&hash);
        let buffer_contains = self.casper.buffer_contains(&hash);
        let dag_contains = self.casper.dag_contains(&hash);
        Ok(blocks_in_processing || buffer_contains || dag_contains)
    }

    async fn recover_stuck_validator_inner(
        &self,
        delay_threshold: Duration,
    ) -> Result<bool, CasperError>
    where
        T: Clone + 'static,
    {
        let Some(recovery_context) = &self.recovery_context else {
            return Ok(false);
        };
        let Some(validator_id) = self.casper.get_validator() else {
            return Ok(false);
        };

        let validator = validator_id.public_key.bytes.clone();
        let dag = self.casper.block_dag().await?;
        let latest_hash = match dag.latest_message_hash(&validator) {
            Some(hash) => hash,
            None => {
                self.casper.set_recovery_sync_active(false);
                return Ok(false);
            }
        };
        if latest_hash == dag.last_finalized_block() {
            self.casper.set_recovery_sync_active(false);
            return Ok(false);
        }

        let latest_block = match self.casper.block_store().get(&latest_hash)? {
            Some(block) => block,
            None => return Ok(false),
        };

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let latest_age_ms = now_ms.saturating_sub(latest_block.header.timestamp);
        if latest_age_ms < delay_threshold.as_millis() as i64 {
            self.casper.set_recovery_sync_active(false);
            return Ok(false);
        }

        tracing::warn!(
            "Validator latest message {} has been stale for {}ms; requesting multi-peer DAG tips and local finalization.",
            PrettyPrinter::build_string_bytes(&latest_hash),
            latest_age_ms
        );

        self.casper.set_recovery_sync_active(true);
        if let Err(error) = self.casper.request_finalization() {
            self.casper.set_recovery_sync_active(false);
            return Err(error);
        }
        self.transport
            .send_fork_choice_tip_request(&recovery_context.connections_cell, &self.conf)
            .await?;

        Ok(true)
    }

    pub async fn handle_block_hash_message(
        &self,
        peer: PeerNode,
        bhm: BlockHashMessage,
        ignore_message_f: impl Fn(BlockHash) -> Result<bool, CasperError>,
    ) -> Result<(), CasperError> {
        let h = bhm.block_hash;
        if ignore_message_f(h.clone())? {
            tracing::debug!(
                "Ignoring {} hash broadcast",
                PrettyPrinter::build_string_bytes(&h)
            );
        } else {
            tracing::debug!(
                "Incoming BlockHashMessage {} from {}",
                PrettyPrinter::build_string_bytes(&h),
                peer.endpoint.host
            );
            self.block_retriever
                .admit_hash(
                    h,
                    Some(peer),
                    block_retriever::AdmitHashReason::HashBroadcastReceived,
                )
                .await?;
        }
        Ok(())
    }

    pub async fn handle_has_block_message(
        &self,
        peer: PeerNode,
        hb: HasBlock,
        ignore_message_f: impl Fn(BlockHash) -> Result<bool, CasperError>,
    ) -> Result<(), CasperError> {
        let h = hb.hash;
        if ignore_message_f(h.clone())? {
            tracing::debug!(
                "Ignoring {} HasBlockMessage",
                PrettyPrinter::build_string_bytes(&h)
            );
        } else {
            tracing::debug!(
                "Incoming HasBlockMessage {} from {}",
                PrettyPrinter::build_string_bytes(&h),
                peer.endpoint.host
            );
            self.block_retriever
                .admit_hash(
                    h,
                    Some(peer),
                    block_retriever::AdmitHashReason::HasBlockMessageReceived,
                )
                .await?;
        }
        Ok(())
    }

    pub async fn handle_block_request(
        &self,
        peer: PeerNode,
        br: BlockRequest,
    ) -> Result<(), CasperError> {
        let maybe_block = self.casper.block_store().get(&br.hash)?;
        if let Some(block) = maybe_block {
            tracing::info!(
                "Received request for block {} from {}. Response sent.",
                PrettyPrinter::build_string_bytes(&br.hash),
                peer
            );
            self.transport
                .stream_message_to_peer(&self.conf, &peer, Arc::new(block.to_proto()))
                .await?;
        } else {
            tracing::info!(
                "Received request for block {} from {}. No response given since block not found.",
                PrettyPrinter::build_string_bytes(&br.hash),
                peer
            );
        }
        Ok(())
    }

    pub async fn handle_has_block_request(
        &self,
        peer: PeerNode,
        hbr: HasBlockRequest,
        block_lookup: impl Fn(BlockHash) -> bool,
    ) -> Result<(), CasperError> {
        if block_lookup(hbr.hash.clone()) {
            let has_block = HasBlock { hash: hbr.hash };
            self.transport
                .send_message_to_peer(&self.conf, &peer, Arc::new(has_block.to_proto()))
                .await?;
        }
        Ok(())
    }

    /**
     * Peer asks for fork-choice tip
     */
    pub async fn handle_fork_choice_tip_request(&self, peer: PeerNode) -> Result<(), CasperError> {
        tracing::info!("Received ForkChoiceTipRequest from {}", peer.endpoint.host);
        let latest_messages = self.casper.block_dag().await?.latest_message_hashes();
        let tips: Vec<BlockHash> = latest_messages
            .iter()
            .map(|(_, hash)| hash.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        tracing::info!(
            "Sending tips {} to {}",
            tips.iter()
                .map(|tip| PrettyPrinter::build_string_bytes(tip))
                .collect::<Vec<_>>()
                .join(", "),
            peer.endpoint.host
        );
        for tip in tips {
            let has_block = HasBlock { hash: tip };
            self.transport
                .send_message_to_peer(&self.conf, &peer, Arc::new(has_block.to_proto()))
                .await?;
        }
        Ok(())
    }

    pub async fn handle_approved_block_request(
        &self,
        peer: PeerNode,
        approved_block: ApprovedBlock,
    ) -> Result<(), CasperError> {
        tracing::info!("Received ApprovedBlockRequest from {}", peer);
        self.transport
            .stream_message_to_peer(&self.conf, &peer, Arc::new(approved_block.to_proto()))
            .await?;
        tracing::info!("ApprovedBlock sent to {}", peer);
        Ok(())
    }

    /// Respond to a `MergeableEntryRequest`.
    ///
    /// - Block not in our store: silent (no response).
    /// - Block present: respond with an empty entry so the peer replays locally.
    async fn handle_mergeable_entry_request(
        &self,
        peer: PeerNode,
        block_hash: BlockHash,
    ) -> Result<(), CasperError> {
        if self.casper.block_store().get(&block_hash)?.is_none() {
            tracing::debug!(
                "MergeableEntryRequest for {} from {}: block not in store; silent ignore.",
                PrettyPrinter::build_string_bytes(&block_hash),
                peer
            );
            return Ok(());
        }

        let resp = casper_message::MergeableEntryResponse {
            block_hash: block_hash.clone(),
            serialized_entry: prost::bytes::Bytes::new(),
        };

        self.transport
            .stream_message_to_peer(&self.conf, &peer, Arc::new(resp.to_proto()))
            .await?;

        tracing::debug!(
            "Unauthenticated mergeable-entry export refused for {} and block {}; peer must replay locally.",
            peer,
            PrettyPrinter::build_string_bytes(&block_hash)
        );
        Ok(())
    }

    async fn handle_state_items_message_request(
        &self,
        peer: PeerNode,
        start_path: Vec<(Blake2b256Hash, Option<u8>)>,
        skip: u32,
        take: u32,
    ) -> Result<(), CasperError> {
        let exporter = self.casper.get_history_exporter().await;

        let (history, data) = RSpaceExporterItems::get_history_and_data(
            exporter,
            start_path.clone(),
            skip as i32,
            take as i32,
        );
        let resp = casper_message::StoreItemsMessage {
            start_path,
            last_path: history.last_path,
            history_items: history
                .items
                .into_iter()
                .map(|(k, v)| (k, prost::bytes::Bytes::from(v)))
                .collect(),
            data_items: data
                .items
                .into_iter()
                .map(|(k, v)| (k, prost::bytes::Bytes::from(v)))
                .collect(),
        };
        let resp_proto = resp.to_proto();

        self.transport
            .stream_message_to_peer(&self.conf, &peer, Arc::new(resp_proto))
            .await?;

        tracing::info!("Store items sent to {}", peer);
        Ok(())
    }
}
