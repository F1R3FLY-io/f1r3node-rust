// See comm/src/main/scala/coop/rchain/comm/transport/GrpcTransportReceiver.scala

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures::stream::StreamExt;
use models::routing::transport_layer_server::{TransportLayer, TransportLayerServer};
use models::routing::{Chunk, Header, Node, Packet, TlRequest, TlResponse};
use prost::Message;
use shared::rust::shared::compression::Compression;
use shared::rust::shared::recent_hash_filter::RecentHashFilter;
use tokio::sync::{mpsc, Mutex, OnceCell};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::activity_gate::{ActivityGate, ActivityGuard};
use super::messages::{Send as CommSend, StreamMessage};
use super::ssl_session_server_interceptor::SslSessionServerInterceptor;
use super::stream_handler::{Circuit, StreamError, StreamHandler, Streamed};
use crate::rust::errors::CommError;
use crate::rust::metrics_constants::{
    PACKETS_DROPPED_METRIC, PACKETS_ENQUEUED_METRIC, PACKETS_RECEIVED_METRIC,
    STREAM_CHUNKS_DROPPED_METRIC, STREAM_CHUNKS_ENQUEUED_METRIC, STREAM_CHUNKS_RECEIVED_METRIC,
    TRANSPORT_DECODER_BYTES_LIMIT_METRIC, TRANSPORT_METRICS_SOURCE,
    TRANSPORT_RESIDENT_BYTES_LIMIT_METRIC,
};
use crate::rust::peer_node::PeerNode;
use crate::rust::rp::protocol_helper;
use crate::rust::rp::rp_conf::RPConf;
use crate::rust::transport::payload_budget::PayloadBudget;

fn calculate_gossip_hash(header: &Header, sender: &Node, packet: &Packet) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    sender.id.hash(&mut hasher);
    sender.host.hash(&mut hasher);
    sender.tcp_port.hash(&mut hasher);
    sender.udp_port.hash(&mut hasher);
    header.network_id.hash(&mut hasher);
    packet.type_id.hash(&mut hasher);
    packet.content.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InboundResourceEnvelope {
    payload_bytes: usize,
    decoder_bytes: usize,
    total_bytes: usize,
    request_capacity: usize,
    max_concurrent_streams: u32,
}

fn inbound_resource_envelope(
    max_message_size: usize,
    max_stream_message_size: usize,
    parallelism: usize,
) -> Result<InboundResourceEnvelope, CommError> {
    if max_message_size == 0 || max_stream_message_size == 0 || parallelism == 0 {
        return Err(CommError::ConfigError(
            "transport size limits and parallelism must be positive".to_string(),
        ));
    }
    let max_wire_bytes = Compression::max_compressed_allocation(max_stream_message_size)
        .ok_or_else(|| CommError::ConfigError("stream size bound overflow".to_string()))?;
    let stream_residency = max_stream_message_size
        .checked_add(max_wire_bytes)
        .ok_or_else(|| CommError::ConfigError("stream residency bound overflow".to_string()))?;
    let payload_bytes = stream_residency.max(max_message_size);
    let decoder_bytes = max_message_size
        .checked_mul(parallelism)
        .ok_or_else(|| CommError::ConfigError("decoder residency bound overflow".to_string()))?;
    let total_bytes = payload_bytes
        .checked_add(decoder_bytes)
        .ok_or_else(|| CommError::ConfigError("inbound residency bound overflow".to_string()))?;
    let request_capacity = parallelism.max(HTTP2_PRE_SETTINGS_REQUEST_LIMIT);
    let max_concurrent_streams = u32::try_from(request_capacity).map_err(|_| {
        CommError::ConfigError("transport parallelism exceeds HTTP/2 limits".to_string())
    })?;
    Ok(InboundResourceEnvelope {
        payload_bytes,
        decoder_bytes,
        total_bytes,
        request_capacity,
        max_concurrent_streams,
    })
}

pub struct MessageBuffers {
    tell_sender: mpsc::Sender<InboundEnvelope<CommSend>>,
    blob_sender: mpsc::Sender<InboundEnvelope<StreamMessage>>,
    tell_task: JoinHandle<()>,
    blob_task: JoinHandle<()>,
    activity: Arc<ActivityGate>,
}

struct InboundEnvelope<T> {
    message: Option<T>,
    _activity: ActivityGuard,
}

impl<T> InboundEnvelope<T> {
    fn new(message: T, activity: ActivityGuard) -> Self {
        Self {
            message: Some(message),
            _activity: activity,
        }
    }

    fn take(&mut self) -> T { self.message.take().expect("inbound message already taken") }
}

impl MessageBuffers {
    fn sender_guard(&self) -> Result<ActivityGuard, CommError> {
        self.activity.try_enter().ok_or_else(|| {
            CommError::ResourceExhausted("peer message queues are retiring".to_string())
        })
    }

    fn try_send_tell(&self, message: CommSend) -> Result<(), CommError> {
        let guard = self.sender_guard()?;
        let envelope = InboundEnvelope::new(message, guard);
        self.tell_sender
            .try_send(envelope)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    CommError::ResourceExhausted("peer protocol-message queue is full".to_string())
                }
                mpsc::error::TrySendError::Closed(_) => CommError::InternalCommunicationError(
                    "peer protocol-message queue is closed".to_string(),
                ),
            })
    }

    fn try_send_blob(&self, message: StreamMessage) -> Result<(), CommError> {
        let guard = self.sender_guard()?;
        let envelope = InboundEnvelope::new(message, guard);
        self.blob_sender
            .try_send(envelope)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    CommError::ResourceExhausted("peer blob queue is full".to_string())
                }
                mpsc::error::TrySendError::Closed(_) => {
                    CommError::InternalCommunicationError("peer blob queue is closed".to_string())
                }
            })
    }

    fn try_retire(&self) -> bool {
        self.activity.try_retire_if(|| {
            self.tell_sender.capacity() == self.tell_sender.max_capacity()
                && self.blob_sender.capacity() == self.blob_sender.max_capacity()
        })
    }
}

impl Drop for MessageBuffers {
    fn drop(&mut self) {
        self.tell_task.abort();
        self.blob_task.abort();
    }
}

#[derive(Clone)]
pub struct PeerBufferSlot {
    pub once_cell: Arc<OnceCell<Arc<MessageBuffers>>>,
    pub last_seen_ms: u64,
    in_progress: Arc<AtomicUsize>,
}

struct PeerBufferSlotGuard {
    in_progress: Arc<AtomicUsize>,
}

impl PeerBufferSlotGuard {
    fn new(in_progress: Arc<AtomicUsize>) -> Self {
        in_progress.fetch_add(1, Ordering::SeqCst);
        Self { in_progress }
    }
}

impl Drop for PeerBufferSlotGuard {
    fn drop(&mut self) { self.in_progress.fetch_sub(1, Ordering::SeqCst); }
}

/// Type alias for message handlers
pub type MessageHandlers = (
    Arc<
        dyn Fn(CommSend) -> Pin<Box<dyn Future<Output = Result<(), CommError>> + Send>>
            + Send
            + Sync,
    >,
    Arc<
        dyn Fn(StreamMessage) -> Pin<Box<dyn Future<Output = Result<(), CommError>> + Send>>
            + Send
            + Sync,
    >,
);

/// Transport Layer Service Implementation
///
/// This implements the tonic-generated TransportLayer trait to handle
/// incoming gRPC requests with SSL session validation.
pub struct TransportLayerService {
    network_id: String,
    rp_config: RPConf,
    max_stream_message_size: usize,
    buffers_map: Arc<Mutex<HashMap<PeerNode, PeerBufferSlot>>>,
    message_handlers: MessageHandlers,
    payload_budget: Arc<PayloadBudget>,
    parallelism: usize,
    cleanup_counter: AtomicUsize,
    /// Filter to avoid redundant gossip of already seen block hashes
    recent_hash_filter: RecentHashFilter,
}

/// Default capacity for the recent hash filter
const RECENT_HASH_FILTER_CAPACITY: usize = 8192;
/// Inbound per-peer queue sizing tuned for catch-up bursts.
/// Small values cause drops that can amplify missing-dependency churn.
const INBOUND_TELL_BUFFER_SIZE: usize = 512;
const INBOUND_BLOB_BUFFER_SIZE: usize = 128;
const PEER_BUFFER_STALE_TTL_MS: u64 = 300_000;
const PEER_BUFFER_CLEANUP_EVERY_REQUESTS: usize = 256;
const PEER_BUFFER_HARD_MAX_ENTRIES: usize = 1024;
const HTTP2_PRE_SETTINGS_REQUEST_LIMIT: usize = 100;

impl TransportLayerService {
    pub fn new(
        network_id: String,
        rp_config: RPConf,
        max_stream_message_size: usize,
        buffers_map: Arc<Mutex<HashMap<PeerNode, PeerBufferSlot>>>,
        message_handlers: MessageHandlers,
        payload_budget: Arc<PayloadBudget>,
        parallelism: usize,
    ) -> Self {
        Self {
            network_id,
            rp_config,
            max_stream_message_size,
            buffers_map,
            message_handlers,
            payload_budget,
            parallelism,
            cleanup_counter: AtomicUsize::new(0),
            recent_hash_filter: RecentHashFilter::new(RECENT_HASH_FILTER_CAPACITY),
        }
    }

    async fn get_buffers(&self, peer: &PeerNode) -> Result<Arc<MessageBuffers>, CommError> {
        self.maybe_cleanup_stale_peer_buffers().await;
        let (once_cell, _slot_guard) = {
            let mut buffers_map = self.buffers_map.lock().await;
            let now_ms = Self::now_millis();
            if let Some(slot) = buffers_map.get_mut(peer) {
                slot.last_seen_ms = now_ms;
                (
                    slot.once_cell.clone(),
                    PeerBufferSlotGuard::new(slot.in_progress.clone()),
                )
            } else {
                if buffers_map.len() >= PEER_BUFFER_HARD_MAX_ENTRIES {
                    return Err(CommError::ResourceExhausted(format!(
                        "peer message queue capacity is exhausted at {} peers",
                        PEER_BUFFER_HARD_MAX_ENTRIES
                    )));
                }
                let once_cell = Arc::new(OnceCell::new());
                let in_progress = Arc::new(AtomicUsize::new(0));
                let guard = PeerBufferSlotGuard::new(in_progress.clone());
                buffers_map.insert(peer.clone(), PeerBufferSlot {
                    once_cell: once_cell.clone(),
                    last_seen_ms: now_ms,
                    in_progress,
                });
                (once_cell, guard)
            }
        };
        let buffers = once_cell
            .get_or_try_init(|| async {
                tracing::info!("Creating inbound message queue for {}.", peer.to_address());
                Ok::<Arc<MessageBuffers>, CommError>(Arc::new(
                    self.create_buffers_with_subscriptions(),
                ))
            })
            .await?;
        if !buffers.activity.is_accepting() {
            return Err(CommError::ResourceExhausted(
                "peer message queues are retiring".to_string(),
            ));
        }
        Ok(buffers.clone())
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    async fn maybe_cleanup_stale_peer_buffers(&self) {
        let activity = self.cleanup_counter.fetch_add(1, Ordering::Relaxed) + 1;
        if !activity.is_multiple_of(PEER_BUFFER_CLEANUP_EVERY_REQUESTS)
            && self.buffers_map.lock().await.len() < PEER_BUFFER_HARD_MAX_ENTRIES
        {
            return;
        }
        let now_ms = Self::now_millis();
        let mut buffers_map = self.buffers_map.lock().await;
        let stale_idle: Vec<PeerNode> = buffers_map
            .iter()
            .filter_map(|(peer, slot)| {
                let stale = now_ms.saturating_sub(slot.last_seen_ms) >= PEER_BUFFER_STALE_TTL_MS;
                if !stale {
                    return None;
                }
                if slot.in_progress.load(Ordering::SeqCst) != 0 {
                    return None;
                }
                match slot.once_cell.get() {
                    Some(buffers) if buffers.try_retire() => Some(peer.clone()),
                    None => Some(peer.clone()),
                    _ => None,
                }
            })
            .collect();
        for peer in &stale_idle {
            buffers_map.remove(peer);
        }
        if !stale_idle.is_empty() {
            tracing::debug!(
                "Retired {} idle peer message queues; {} remain",
                stale_idle.len(),
                buffers_map.len()
            );
        }
    }

    fn create_buffers_with_subscriptions(&self) -> MessageBuffers {
        let (tell_sender, tell_receiver) =
            mpsc::channel::<InboundEnvelope<CommSend>>(INBOUND_TELL_BUFFER_SIZE);
        let (blob_sender, blob_receiver) =
            mpsc::channel::<InboundEnvelope<StreamMessage>>(INBOUND_BLOB_BUFFER_SIZE);
        let parallelism = self.parallelism;

        let tell_handler = self.message_handlers.0.clone();
        let tell_task = tokio::spawn(async move {
            ReceiverStream::new(tell_receiver)
                .for_each_concurrent(Some(parallelism), move |mut envelope| {
                    let handler = tell_handler.clone();
                    let message = envelope.take();
                    async move {
                        let _envelope = envelope;
                        if let Err(error) = handler(message).await {
                            tracing::error!(%error, "protocol-message handler failed");
                        }
                    }
                })
                .await;
        });

        let blob_handler = self.message_handlers.1.clone();
        let blob_task = tokio::spawn(async move {
            ReceiverStream::new(blob_receiver)
                .for_each_concurrent(Some(parallelism), move |mut envelope| {
                    let handler = blob_handler.clone();
                    let message = envelope.take();
                    async move {
                        let _envelope = envelope;
                        if let Err(error) = handler(message).await {
                            tracing::error!(%error, "blob handler failed");
                        }
                    }
                })
                .await;
        });

        MessageBuffers {
            tell_sender,
            blob_sender,
            tell_task,
            blob_task,
            activity: ActivityGate::new(),
        }
    }

    async fn get_tell_buffer(&self, peer: &PeerNode) -> Result<Arc<MessageBuffers>, CommError> {
        self.get_buffers(peer).await
    }

    async fn get_blob_buffer(&self, peer: &PeerNode) -> Result<Arc<MessageBuffers>, CommError> {
        self.get_buffers(peer).await
    }

    /// Create ACK response
    fn create_ack_response(&self, src: &PeerNode) -> TlResponse {
        TlResponse {
            payload: Some(models::routing::tl_response::Payload::Ack(
                models::routing::Ack {
                    header: Some(protocol_helper::header(src, &self.network_id)),
                },
            )),
        }
    }

    /// Create InternalServerError response
    fn create_internal_server_error_response(&self, message: String) -> TlResponse {
        TlResponse {
            payload: Some(models::routing::tl_response::Payload::InternalServerError(
                models::routing::InternalServerError {
                    error: prost::bytes::Bytes::from(message),
                },
            )),
        }
    }

    async fn handle_stream_with_params<S>(
        &self,
        stream: S,
        network_id: &str,
        max_size: usize,
    ) -> Result<StreamMessage, StreamError>
    where
        S: futures::stream::Stream<Item = Chunk> + Unpin,
    {
        let circuit_breaker = |streamed: &Streamed| {
            if let Some(header) = &streamed.header {
                if header.network_id != network_id {
                    return Circuit::opened(StreamError::wrong_network_id());
                }
            }
            Circuit::closed()
        };
        StreamHandler::handle_stream(stream, circuit_breaker, &self.payload_budget, max_size).await
    }
}

#[tonic::async_trait]
impl TransportLayer for TransportLayerService {
    /// Handle Send requests with SSL validation
    async fn send(&self, request: Request<TlRequest>) -> Result<Response<TlResponse>, Status> {
        // Validate the request using SSL session server interceptor
        SslSessionServerInterceptor::validate_tl_request(&request)?;

        let protocol = request
            .into_inner()
            .protocol
            .ok_or_else(|| Status::invalid_argument("Missing protocol in request"))?;

        let header = protocol
            .header
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Missing header in protocol"))?;

        let sender_node = header
            .sender
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Missing sender in header"))?;

        // Extract peer from request
        let peer = PeerNode::from_node(sender_node.clone())
            .map_err(|e| Status::internal(format!("Failed to convert to PeerNode: {}", e)))?;

        metrics::counter!(PACKETS_RECEIVED_METRIC, "source" => TRANSPORT_METRICS_SOURCE)
            .increment(1);

        // Determine if this is a gossip message (not a request/response)
        // Only filter pure gossip announcements: BlockHashMessage and HasBlock
        // Other messages (approvals, requests, handshakes, heartbeats) must pass through
        match &protocol.message {
            Some(models::routing::protocol::Message::Packet(packet))
                if packet.type_id == "BlockHashMessage" || packet.type_id == "HasBlock" =>
            {
                let hash_tag = format!("{:x}", calculate_gossip_hash(header, sender_node, packet));

                if self.recent_hash_filter.seen_before(&hash_tag) {
                    tracing::debug!(
                        "[GOSSIP] Suppressed redundant hash broadcast {} from {}",
                        hash_tag,
                        peer.endpoint.host
                    );
                    return Ok(Response::new(
                        self.create_ack_response(&self.rp_config.local),
                    ));
                }
            }
            _ => {}
        }

        // Get target buffer
        let tell_buffer = self
            .get_tell_buffer(&peer)
            .await
            .map_err(|e| Status::internal(format!("Failed to get tell buffer: {}", e)))?;

        let payload_bytes = protocol.encoded_len().max(1);
        let reservation = match self.payload_budget.try_reserve(payload_bytes) {
            Ok(reservation) => reservation,
            Err(error) => {
                metrics::counter!(PACKETS_DROPPED_METRIC, "source" => TRANSPORT_METRICS_SOURCE)
                    .increment(1);
                return Ok(Response::new(
                    self.create_internal_server_error_response(error.to_string()),
                ));
            }
        };
        let send_msg = CommSend::new(protocol, reservation);

        let response = if tell_buffer.try_send_tell(send_msg).is_ok() {
            metrics::counter!(PACKETS_ENQUEUED_METRIC, "source" => TRANSPORT_METRICS_SOURCE)
                .increment(1);
            self.create_ack_response(&self.rp_config.local)
        } else {
            let packet_dropped_msg = format!(
                "Packet rejected, {} packet queue is full.",
                peer.endpoint.host
            );
            metrics::counter!(PACKETS_DROPPED_METRIC, "source" => TRANSPORT_METRICS_SOURCE)
                .increment(1);
            self.create_internal_server_error_response(packet_dropped_msg)
        };

        Ok(Response::new(response))
    }

    /// Handle Stream requests with SSL validation
    async fn stream(
        &self,
        request: Request<tonic::Streaming<Chunk>>,
    ) -> Result<Response<TlResponse>, Status> {
        // Validate the request using SSL session server interceptor
        // Note: For streaming requests, we validate the TLS session context
        // The actual message content validation happens in StreamHandler
        SslSessionServerInterceptor::validate_stream_request(&request)?;

        let stream = request.into_inner();

        // Convert tonic::Streaming<Chunk> to Stream<Item = Chunk> by handling Results
        let chunk_stream = stream.map(|result| match result {
            Ok(chunk) => chunk,
            Err(status) => {
                tracing::error!(error = %status, "gRPC incoming stream chunk error");
                Chunk { content: None }
            }
        });

        // Use our custom handler with parameters
        let stream_result = self
            .handle_stream_with_params(chunk_stream, &self.network_id, self.max_stream_message_size)
            .await;

        let response = match stream_result {
            Err(StreamError::Unexpected { ref error }) => {
                tracing::error!(error = %error, "blob stream processing failed");
                self.create_internal_server_error_response(error.clone())
            }
            Err(ref error) => {
                tracing::warn!("Stream error: {}", error.message());
                self.create_internal_server_error_response(error.message())
            }
            Ok(stream_msg) => {
                metrics::counter!(STREAM_CHUNKS_RECEIVED_METRIC, "source" => TRANSPORT_METRICS_SOURCE).increment(1);
                let msg_enqueued = format!(
                    "Stream payload pushed to message buffer. Sender {}, message {}, size {}.",
                    stream_msg.sender.endpoint.host, stream_msg.type_id, stream_msg.content_length
                );
                let msg_dropped = format!(
                    "Stream payload rejected, {} stream queue is full.",
                    stream_msg.sender.endpoint.host
                );

                // Get target buffer for the sender
                match self.get_blob_buffer(&stream_msg.sender).await {
                    Ok(target_buffer) => {
                        if target_buffer.try_send_blob(stream_msg).is_ok() {
                            metrics::counter!(STREAM_CHUNKS_ENQUEUED_METRIC, "source" => TRANSPORT_METRICS_SOURCE).increment(1);
                            tracing::debug!("{}", msg_enqueued);
                            self.create_ack_response(&self.rp_config.local)
                        } else {
                            metrics::counter!(STREAM_CHUNKS_DROPPED_METRIC, "source" => TRANSPORT_METRICS_SOURCE).increment(1);
                            tracing::debug!("{}", msg_dropped);
                            self.create_internal_server_error_response(msg_dropped)
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "blob buffer retrieval failed");
                        self.create_internal_server_error_response(format!("Buffer error: {}", e))
                    }
                }
            }
        };

        Ok(Response::new(response))
    }
}

/// GrpcTransportReceiver for handling incoming gRPC messages
pub struct GrpcTransportReceiver;

impl GrpcTransportReceiver {
    /// Create a new gRPC transport receiver with F1r3fly custom TLS
    pub async fn create(
        network_id: String,
        rp_config: RPConf,
        port: u16,
        cert_pem: String,
        key_pem: String,
        max_message_size: i32,
        max_stream_message_size: u64,
        buffers_map: Arc<Mutex<HashMap<PeerNode, PeerBufferSlot>>>,
        message_handlers: MessageHandlers,
        parallelism: usize,
    ) -> Result<JoinHandle<()>, CommError> {
        use std::net::SocketAddr;

        use tonic::service::interceptor::InterceptedService;
        use tonic::transport::Server;
        use tower::limit::GlobalConcurrencyLimitLayer;

        // Import our custom F1r3fly server
        use super::f1r3fly_server::F1r3flyServer;

        let addr: SocketAddr = format!("0.0.0.0:{}", port)
            .parse()
            .map_err(|e| CommError::ConfigError(format!("Invalid address: {}", e)))?;
        let max_message_size = usize::try_from(max_message_size).map_err(|_| {
            CommError::ConfigError("maximum unary message size must be positive".to_string())
        })?;
        let max_stream_message_size = usize::try_from(max_stream_message_size).map_err(|_| {
            CommError::ConfigError("maximum stream message size does not fit usize".to_string())
        })?;
        let envelope =
            inbound_resource_envelope(max_message_size, max_stream_message_size, parallelism)?;
        let payload_budget =
            PayloadBudget::new("inbound", envelope.payload_bytes, envelope.request_capacity)
                .map_err(|error| CommError::ConfigError(error.to_string()))?;
        metrics::gauge!(
            TRANSPORT_DECODER_BYTES_LIMIT_METRIC,
            "source" => TRANSPORT_METRICS_SOURCE,
            "direction" => "inbound"
        )
        .set(envelope.decoder_bytes as f64);
        metrics::gauge!(
            TRANSPORT_RESIDENT_BYTES_LIMIT_METRIC,
            "source" => TRANSPORT_METRICS_SOURCE,
            "direction" => "inbound"
        )
        .set(envelope.total_bytes as f64);

        // Create SSL session server interceptor
        let ssl_interceptor = SslSessionServerInterceptor::new(network_id.clone());

        // A stalled handshake should not outlive the timeout the peer that
        // opened it is actually using — captured before `rp_config` moves
        // into `TransportLayerService::new` below.
        let handshake_timeout = rp_config.default_timeout;

        // Create the transport layer service implementation
        let transport_service = TransportLayerService::new(
            network_id.clone(),
            rp_config,
            max_stream_message_size,
            buffers_map,
            message_handlers,
            payload_budget,
            parallelism,
        );
        let transport_service = TransportLayerServer::new(transport_service)
            .max_decoding_message_size(max_message_size)
            .max_encoding_message_size(max_message_size);
        let transport_service = InterceptedService::new(transport_service, ssl_interceptor);

        // Create F1r3fly server with custom TLS configuration
        let f1r3fly_server = F1r3flyServer::builder(network_id.clone(), &cert_pem, &key_pem, addr)
            .map_err(|e| CommError::ConfigError(format!("F1r3fly server creation failed: {}", e)))?
            .handshake_timeout(handshake_timeout)
            // Configure TCP settings to match the previous tonic configuration
            .tcp_keepalive(Some(std::time::Duration::from_secs(600))) // 10 minutes
            .tcp_nodelay(true)
            .http2_keepalive_interval(Some(std::time::Duration::from_secs(30)))
            .http2_keepalive_timeout(Some(std::time::Duration::from_secs(5)));

        // Create incoming connection stream with F1r3fly TLS
        let incoming = f1r3fly_server.incoming().await.map_err(|e| {
            CommError::ConfigError(format!("Failed to create F1r3fly incoming stream: {}", e))
        })?;

        // Create the gRPC server with F1r3fly TLS configuration
        let server_task = tokio::spawn(async move {
            tracing::info!(
                "Starting F1r3fly TLS-enabled gRPC transport receiver on {}",
                addr
            );

            let server_result = Server::builder()
                .layer(GlobalConcurrencyLimitLayer::new(parallelism))
                .timeout(std::time::Duration::from_secs(30))
                .max_concurrent_streams(envelope.max_concurrent_streams)
                .max_frame_size(Some(max_message_size as u32))
                .add_service(transport_service)
                .serve_with_incoming(incoming)
                .await;

            if let Err(e) = server_result {
                tracing::error!(error = %e, "F1r3fly gRPC server failed");
            }

            Ok::<(), CommError>(())
        });

        // Handle the Result from the spawn task
        Ok(tokio::spawn(async move {
            if let Err(e) = server_task.await {
                tracing::error!(error = %e, "F1r3fly gRPC server task panicked");
            }
        }))
    }
}

#[cfg(test)]
mod resource_envelope_tests {
    use models::routing::{Header, Node, Packet};
    use proptest::prelude::*;
    use prost::bytes::Bytes;

    use super::{
        calculate_gossip_hash, inbound_resource_envelope, HTTP2_PRE_SETTINGS_REQUEST_LIMIT,
    };

    #[test]
    fn inbound_envelope_rejects_zero_and_overflow() {
        assert!(inbound_resource_envelope(0, 1, 1).is_err());
        assert!(inbound_resource_envelope(1, 0, 1).is_err());
        assert!(inbound_resource_envelope(1, 1, 0).is_err());
        assert!(inbound_resource_envelope(usize::MAX, 1, 2).is_err());
        assert!(inbound_resource_envelope(usize::MAX / 2 + 1, 1, 1).is_err());
    }

    #[test]
    fn gossip_hash_is_structural_without_reencoding() {
        let sender = Node {
            id: Bytes::from_static(b"node-a"),
            host: Bytes::from_static(b"host-a"),
            tcp_port: 40400,
            udp_port: 40404,
        };
        let header = Header {
            sender: Some(sender.clone()),
            network_id: "root".to_string(),
        };
        let packet = Packet {
            type_id: "BlockHashMessage".to_string(),
            content: Bytes::from_static(b"block-a"),
        };
        let hash = calculate_gossip_hash(&header, &sender, &packet);
        assert_eq!(hash, calculate_gossip_hash(&header, &sender, &packet));

        let mut other_packet = packet;
        other_packet.content = Bytes::from_static(b"block-b");
        assert_ne!(hash, calculate_gossip_hash(&header, &sender, &other_packet));
    }

    proptest! {
        #[test]
        fn inbound_envelope_composes_decoder_and_payload_bounds(
            max_message_size in 1usize..1_048_576,
            max_stream_message_size in 1usize..1_048_576,
            parallelism in 1usize..1024,
        ) {
            let envelope = inbound_resource_envelope(
                max_message_size,
                max_stream_message_size,
                parallelism,
            ).unwrap();
            prop_assert_eq!(envelope.decoder_bytes, max_message_size * parallelism);
            prop_assert_eq!(
                envelope.total_bytes,
                envelope.payload_bytes + envelope.decoder_bytes,
            );
            prop_assert_eq!(
                envelope.request_capacity,
                parallelism.max(HTTP2_PRE_SETTINGS_REQUEST_LIMIT),
            );
            prop_assert_eq!(
                usize::try_from(envelope.max_concurrent_streams).unwrap(),
                envelope.request_capacity,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::{InboundEnvelope, PeerBufferSlotGuard};
    use crate::rust::transport::activity_gate::ActivityGate;

    #[test]
    fn inbound_envelope_holds_activity_until_handler_ownership_releases() {
        let gate = ActivityGate::new();
        let envelope = InboundEnvelope::new((), gate.try_enter().unwrap());
        assert_eq!(gate.active(), 1);
        assert!(!gate.try_retire_if(|| true));
        drop(envelope);
        assert_eq!(gate.active(), 0);
        assert!(gate.try_retire_if(|| true));
    }

    #[test]
    fn peer_slot_guard_covers_initialization_lifetime() {
        let in_progress = Arc::new(AtomicUsize::new(0));
        let guard = PeerBufferSlotGuard::new(in_progress.clone());
        assert_eq!(in_progress.load(Ordering::SeqCst), 1);
        drop(guard);
        assert_eq!(in_progress.load(Ordering::SeqCst), 0);
    }
}
