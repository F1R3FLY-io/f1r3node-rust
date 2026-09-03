// See comm/src/main/scala/coop/rchain/comm/transport/GrpcTransportClient.scala

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hex;
use models::routing::transport_layer_client::TransportLayerClient;
use models::routing::Protocol;
use tokio::sync::{Mutex, OnceCell};
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Channel;

use crate::rust::errors::CommError;
use crate::rust::peer_node::PeerNode;
use crate::rust::transport::activity_gate::{ActivityGate, ActivityGuard};
use crate::rust::transport::chunker::Chunker;
use crate::rust::transport::f1r3fly_connector::F1r3flyConnector;
use crate::rust::transport::grpc_transport::GrpcTransport;
use crate::rust::transport::payload_budget::PayloadBudget;
use crate::rust::transport::ssl_session_client_interceptor::SslSessionClientInterceptor;
use crate::rust::transport::stream_observable::{OutboundPayload, StreamObservable};
use crate::rust::transport::transport_layer::{Blob, TransportLayer};
use crate::rust::utils::resolve_hostname_to_ip;

/// GRPC channel with a message buffer protecting it from resource exhaustion
#[derive(Debug)]
pub struct BufferedGrpcStreamChannel {
    /// Underlying gRPC channel
    pub grpc_transport: Channel,
    /// Pre-created transport client with SSL interceptor applied (for reuse)
    pub transport_client: Arc<
        tokio::sync::Mutex<
            TransportLayerClient<InterceptedService<Channel, SslSessionClientInterceptor>>,
        >,
    >,
    /// Buffer implementing some kind of overflow policy
    pub buffer: StreamObservable,
    /// Buffer subscriber handle
    pub buffer_subscriber: tokio::task::JoinHandle<()>,
    /// Max message size (to be applied to individual service clients)
    pub max_message_size: i32,
    activity: Arc<ActivityGate>,
}

pub struct ChannelOperationGuard {
    channel: Arc<BufferedGrpcStreamChannel>,
    _activity: ActivityGuard,
}

impl ChannelOperationGuard {
    fn channel(&self) -> &BufferedGrpcStreamChannel { &self.channel }
}

impl BufferedGrpcStreamChannel {
    /// Create a new BufferedGrpcStreamChannel with pre-created client
    pub fn new(
        grpc_transport: Channel,
        transport_client: TransportLayerClient<
            InterceptedService<Channel, SslSessionClientInterceptor>,
        >,
        buffer: StreamObservable,
        buffer_subscriber: tokio::task::JoinHandle<()>,
        max_message_size: i32,
    ) -> Self {
        let activity = buffer.activity();
        Self {
            grpc_transport,
            transport_client: Arc::new(tokio::sync::Mutex::new(transport_client)),
            buffer,
            buffer_subscriber,
            max_message_size,
            activity,
        }
    }

    fn try_operation(self: &Arc<Self>) -> Result<ChannelOperationGuard, CommError> {
        let activity = self
            .activity
            .try_enter()
            .ok_or_else(|| CommError::ResourceExhausted("peer channel is retiring".to_string()))?;
        Ok(ChannelOperationGuard {
            channel: self.clone(),
            _activity: activity,
        })
    }

    fn try_retire(&self) -> bool { self.activity.try_retire_if(|| true) }

    /// Get a clone of the pre-created transport client (for use in tasks)
    pub fn get_transport_client(
        &self,
    ) -> Arc<
        tokio::sync::Mutex<
            TransportLayerClient<InterceptedService<Channel, SslSessionClientInterceptor>>,
        >,
    > {
        self.transport_client.clone()
    }
}

impl Drop for BufferedGrpcStreamChannel {
    fn drop(&mut self) { self.buffer_subscriber.abort(); }
}

#[derive(Clone)]
pub struct ChannelSlot {
    pub once_cell: Arc<OnceCell<Arc<BufferedGrpcStreamChannel>>>,
    last_seen_ms: u64,
    in_progress: Arc<AtomicUsize>,
}

struct ChannelSlotGuard {
    in_progress: Arc<AtomicUsize>,
}

impl ChannelSlotGuard {
    fn new(in_progress: Arc<AtomicUsize>) -> Self {
        in_progress.fetch_add(1, Ordering::SeqCst);
        Self { in_progress }
    }
}

impl Drop for ChannelSlotGuard {
    fn drop(&mut self) { self.in_progress.fetch_sub(1, Ordering::SeqCst); }
}

pub type ChannelsMap = Arc<Mutex<HashMap<PeerNode, ChannelSlot>>>;

/// GrpcTransportClient - gRPC client implementation
#[derive(Clone)]
pub struct GrpcTransportClient {
    network_id: String,
    cert: String,
    key: String,
    max_message_size: i32,
    packet_chunk_size: i32,
    client_queue_size: i32,
    max_stream_message_size: usize,
    channels_map: ChannelsMap,
    cleanup_counter: Arc<AtomicUsize>,
    default_send_timeout: Duration,
    payload_budget: Arc<PayloadBudget>,
}

const MIN_PEER_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CHANNEL_MAP_ENTRIES: usize = 1024;
const CHANNEL_STALE_TTL_MS: u64 = 300_000;
const CHANNEL_CLEANUP_EVERY_REQUESTS: usize = 256;

impl GrpcTransportClient {
    /// Create a new GrpcTransportClient
    pub fn new(
        network_id: String,
        cert: String,
        key: String,
        max_message_size: i32,
        packet_chunk_size: i32,
        client_queue_size: i32,
        max_stream_message_size: u64,
        channels_map: ChannelsMap,
        network_timeout: Duration,
    ) -> Result<Self, CommError> {
        let max_stream_message_size = usize::try_from(max_stream_message_size).map_err(|_| {
            CommError::ConfigError("maximum stream message size does not fit usize".to_string())
        })?;
        let queue_size = usize::try_from(client_queue_size).map_err(|_| {
            CommError::ConfigError("client stream queue capacity must be positive".to_string())
        })?;
        if max_message_size <= 0
            || packet_chunk_size <= 2048
            || max_stream_message_size == 0
            || queue_size == 0
        {
            return Err(CommError::ConfigError(
                "transport message limits, chunk size, and queue capacity must be positive; chunk size must exceed 2048 bytes".to_string(),
            ));
        }
        let max_compressed =
            shared::rust::shared::compression::Compression::max_compressed_allocation(
                max_stream_message_size,
            )
            .ok_or_else(|| CommError::ConfigError("stream size bound overflow".to_string()))?;
        let payload_capacity = max_stream_message_size
            .checked_add(max_compressed)
            .ok_or_else(|| CommError::ConfigError("stream residency bound overflow".to_string()))?;
        let payload_budget = PayloadBudget::new("outbound", payload_capacity, queue_size)
            .map_err(|error| CommError::ConfigError(error.to_string()))?;
        let effective_timeout = std::cmp::max(network_timeout, MIN_PEER_REQUEST_TIMEOUT);
        if effective_timeout != network_timeout {
            tracing::warn!(
                "Configured network timeout {}ms is too low; using minimum {}ms for peer requests",
                network_timeout.as_millis(),
                effective_timeout.as_millis()
            );
        }

        Ok(Self {
            network_id,
            cert,
            key,
            max_message_size,
            packet_chunk_size,
            client_queue_size,
            max_stream_message_size,
            channels_map,
            cleanup_counter: Arc::new(AtomicUsize::new(0)),
            default_send_timeout: effective_timeout,
            payload_budget,
        })
    }

    async fn create_channel(
        &self,
        peer: &PeerNode,
    ) -> Result<BufferedGrpcStreamChannel, CommError> {
        tracing::info!("Creating new F1r3fly channel to peer {}", peer.to_address());

        // **F1r3fly Custom TLS Integration Architecture**
        // This method creates tonic gRPC channels using F1r3flyConnector with connect_with_connector()
        // providing direct integration of F1r3fly TLS verification with tonic's gRPC layer

        // Step 1: Create F1r3flyConnector with peer's F1r3fly address for TLS hostname verification
        let f1r3fly_id_hex = hex::encode(&peer.id.key);
        tracing::debug!(
            "Creating F1r3flyConnector with F1r3fly address for TLS hostname: {}",
            f1r3fly_id_hex
        );

        let f1r3fly_connector = F1r3flyConnector::new_with_timeout(
            self.network_id.clone(),
            &self.cert,
            &self.key,
            f1r3fly_id_hex.clone(),
            self.default_send_timeout,
        )
        .map_err(|e| CommError::ConfigError(format!("Failed to create F1r3flyConnector: {}", e)))?;

        let uri_address = resolve_hostname_to_ip(&peer.endpoint.host, peer.endpoint.tcp_port)
            .await?
            .ip()
            .to_string();

        // Step 2: Create tonic Endpoint with HTTP scheme (not HTTPS)
        // since F1r3flyConnector handles TLS internally
        let endpoint_uri = format!("http://{}:{}/", uri_address, peer.endpoint.tcp_port);
        tracing::debug!(
            "Creating F1r3fly gRPC channel to {} with TLS hostname verification against: {}",
            endpoint_uri,
            f1r3fly_id_hex
        );

        let endpoint = Channel::from_shared(endpoint_uri.clone()).map_err(|e| {
            tracing::error!(uri = %endpoint_uri, host = %peer.endpoint.host, error = %e, "gRPC endpoint creation failed");
            CommError::InternalCommunicationError(format!("Invalid endpoint URI: {}", e))
        })?;

        // Step 3: Use F1r3flyConnector with tonic's connect_with_connector API
        // The F1r3flyConnector will handle TLS hostname verification against the F1r3fly address
        let grpc_channel = endpoint
            .connect_with_connector(f1r3fly_connector)
            .await
            .map_err(|e| {
                tracing::error!(uri = %endpoint_uri, error = %e, "F1r3flyConnector gRPC channel connect failed");
                CommError::InternalCommunicationError(format!(
                    "Failed to establish gRPC connection: {}",
                    e
                ))
            })?;

        tracing::info!("gRPC channel created for {}", peer.to_address());

        // Step 4: Create SSL session interceptor for application-level validation
        let ssl_interceptor = SslSessionClientInterceptor::new(self.network_id.clone());
        let intercepted_channel = InterceptedService::new(grpc_channel.clone(), ssl_interceptor);

        // Step 5: Create transport client with interceptor
        let transport_client = TransportLayerClient::new(intercepted_channel)
            .max_encoding_message_size(self.max_message_size as usize)
            .max_decoding_message_size(self.max_message_size as usize);

        let (buffer, mut subscription) =
            StreamObservable::new(peer.clone(), self.client_queue_size as usize)?;

        // Create buffer subscriber
        let buffer_subscriber = {
            let peer_clone = peer.clone();
            let network_id = self.network_id.clone();
            let default_send_timeout = self.default_send_timeout;
            let packet_chunk_size = self.packet_chunk_size;

            let client_for_task = Arc::new(tokio::sync::Mutex::new(transport_client.clone()));

            tokio::spawn(async move {
                while let Some(delivery) = subscription.recv().await {
                    let result = Self::stream_blob_file_with_client(
                        &peer_clone,
                        delivery.payload().clone(),
                        &network_id,
                        default_send_timeout,
                        packet_chunk_size,
                        client_for_task.clone(),
                    )
                    .await;

                    if let Err(error) = &result {
                        tracing::debug!(%error, "outbound stream failed");
                    }
                    delivery.complete(result);
                }
            })
        };

        Ok(BufferedGrpcStreamChannel::new(
            grpc_channel,
            transport_client,
            buffer,
            buffer_subscriber,
            self.max_message_size,
        ))
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    async fn cleanup_stale_channels(&self) {
        let activity = self.cleanup_counter.fetch_add(1, Ordering::Relaxed) + 1;
        if !activity.is_multiple_of(CHANNEL_CLEANUP_EVERY_REQUESTS)
            && self.channels_map.lock().await.len() < MAX_CHANNEL_MAP_ENTRIES
        {
            return;
        }
        let now_ms = Self::now_millis();
        let mut channels_map = self.channels_map.lock().await;
        let stale_idle: Vec<PeerNode> = channels_map
            .iter()
            .filter_map(|(peer, slot)| {
                let stale = now_ms.saturating_sub(slot.last_seen_ms) >= CHANNEL_STALE_TTL_MS;
                if !stale || slot.in_progress.load(Ordering::SeqCst) != 0 {
                    return None;
                }
                match slot.once_cell.get() {
                    Some(channel) if channel.try_retire() => Some(peer.clone()),
                    None => Some(peer.clone()),
                    _ => None,
                }
            })
            .collect();
        for peer in &stale_idle {
            channels_map.remove(peer);
        }
        if !stale_idle.is_empty() {
            tracing::debug!(
                "Retired {} idle outbound channels; {} remain",
                stale_idle.len(),
                channels_map.len()
            );
        }
    }

    async fn get_channel(&self, peer: &PeerNode) -> Result<ChannelOperationGuard, CommError> {
        self.cleanup_stale_channels().await;
        loop {
            let (once_cell, _slot_guard) = {
                let mut channels_map = self.channels_map.lock().await;
                let now_ms = Self::now_millis();
                if let Some(slot) = channels_map.get_mut(peer) {
                    slot.last_seen_ms = now_ms;
                    (
                        slot.once_cell.clone(),
                        ChannelSlotGuard::new(slot.in_progress.clone()),
                    )
                } else {
                    if channels_map.len() >= MAX_CHANNEL_MAP_ENTRIES {
                        return Err(CommError::ResourceExhausted(format!(
                            "outbound channel capacity is exhausted at {} peers",
                            MAX_CHANNEL_MAP_ENTRIES
                        )));
                    }
                    let once_cell = Arc::new(OnceCell::new());
                    let in_progress = Arc::new(AtomicUsize::new(0));
                    let guard = ChannelSlotGuard::new(in_progress.clone());
                    channels_map.insert(peer.clone(), ChannelSlot {
                        once_cell: once_cell.clone(),
                        last_seen_ms: now_ms,
                        in_progress: in_progress.clone(),
                    });
                    (once_cell, guard)
                }
            };
            let channel = once_cell
                .get_or_try_init(|| async {
                    Ok::<Arc<BufferedGrpcStreamChannel>, CommError>(Arc::new(
                        self.create_channel(peer).await?,
                    ))
                })
                .await?;
            if Self::is_channel_terminated(channel) {
                tracing::debug!(
                    "Channel to peer {} is terminated; removing from connections map",
                    peer.to_address()
                );
                channel.buffer_subscriber.abort();
                self.channels_map.lock().await.remove(peer);
                continue;
            }
            match channel.try_operation() {
                Ok(guard) => return Ok(guard),
                Err(_) => {
                    self.channels_map.lock().await.remove(peer);
                }
            }
        }
    }

    /// Execute a request with a gRPC client, handling timeouts and errors
    async fn with_client<A, F, Fut>(
        &self,
        peer: &PeerNode,
        timeout: Duration,
        request: F,
    ) -> Result<A, CommError>
    where
        F: FnOnce(
            TransportLayerClient<InterceptedService<Channel, SslSessionClientInterceptor>>,
        ) -> Fut,
        Fut: std::future::Future<Output = Result<A, CommError>>,
    {
        // Apply timeout to the entire operation
        let timed_operation = tokio::time::timeout(timeout, async {
            let operation = self.get_channel(peer).await?;
            let channel = operation.channel();

            let client_guard = channel.transport_client.lock().await;
            let client = client_guard.clone(); // Clone the client for use
            drop(client_guard); // Release the lock immediately

            let result = request(client).await?;

            // Return control to caller thread
            // In Rust, this is handled automatically by the async runtime
            tokio::task::yield_now().await;

            Ok::<A, CommError>(result)
        });

        // Handle timeout and other errors
        match timed_operation.await {
            Ok(Ok(success)) => Ok(success),
            Ok(Err(comm_error)) => {
                tracing::error!(peer = %peer.to_address(), error = %comm_error, "gRPC request failed");
                Err(comm_error)
            }
            Err(_timeout_error) => {
                let timeout_error = crate::rust::errors::protocol_exception(format!(
                    "Request to {} timed out after {}ms",
                    peer.to_address(),
                    timeout.as_millis()
                ));
                tracing::warn!("Request timeout: {}", timeout_error);
                Err(timeout_error)
            }
        }
    }

    async fn stream_blob_file_with_client(
        peer: &PeerNode,
        payload: Arc<OutboundPayload>,
        network_id: &str,
        default_send_timeout: Duration,
        packet_chunk_size: i32,
        client: Arc<
            tokio::sync::Mutex<
                TransportLayerClient<InterceptedService<Channel, SslSessionClientInterceptor>>,
            >,
        >,
    ) -> Result<(), CommError> {
        let blob = payload.blob();
        let packet_based_timeout = Duration::from_micros(blob.packet.content.len() as u64 * 5);
        let timeout = std::cmp::max(packet_based_timeout, default_send_timeout);
        let mut client_guard = client.lock().await;
        match tokio::time::timeout(
            timeout,
            GrpcTransport::stream(
                &mut *client_guard,
                peer,
                network_id,
                blob,
                packet_chunk_size as usize,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(crate::rust::errors::timeout()),
        }
    }

    fn prepare_payload(&self, blob: &Blob) -> Result<Arc<OutboundPayload>, CommError> {
        let content_length = blob.packet.content.len();
        if content_length > self.max_stream_message_size {
            return Err(CommError::ResourceExhausted(format!(
                "stream payload is {} bytes; limit is {} bytes",
                content_length, self.max_stream_message_size
            )));
        }
        let retained_bytes = Chunker::retained_bytes_bound(content_length).ok_or_else(|| {
            CommError::ResourceExhausted("stream size bound overflow".to_string())
        })?;
        let reservation = self
            .payload_budget
            .try_reserve(retained_bytes)
            .map_err(|error| CommError::ResourceExhausted(error.to_string()))?;
        Ok(Arc::new(OutboundPayload::new(blob.clone(), reservation)))
    }

    async fn enqueue_payload(
        &self,
        peer: &PeerNode,
        payload: Arc<OutboundPayload>,
    ) -> Result<(), CommError> {
        let operation = self.get_channel(peer).await?;
        let completion = operation.channel().buffer.enqueue(payload)?;
        drop(operation);
        completion.await.map_err(|_| {
            CommError::InternalCommunicationError(
                "outbound stream worker terminated before reporting completion".to_string(),
            )
        })?
    }

    fn is_channel_terminated(channel: &BufferedGrpcStreamChannel) -> bool {
        channel.buffer_subscriber.is_finished()
    }
}

#[async_trait]
impl TransportLayer for GrpcTransportClient {
    /// Send a Protocol message to a peer
    async fn send(&self, peer: &PeerNode, msg: &Protocol) -> Result<(), CommError> {
        self.with_client(peer, self.default_send_timeout, |mut client| async move {
            GrpcTransport::send(&mut client, peer, msg).await
        })
        .await
    }

    /// Broadcast a Protocol message to multiple peers in parallel
    async fn broadcast(&self, peers: &[PeerNode], msg: &Protocol) -> Result<(), CommError> {
        if peers.is_empty() {
            return Ok(());
        }

        // Create a vector of futures for parallel execution
        let send_futures: Vec<_> = peers.iter().map(|peer| self.send(peer, msg)).collect();

        // Execute all sends in parallel and collect results
        let results = futures::future::join_all(send_futures).await;

        // Check if any send failed - if so, return the first error
        for result in results {
            result?; // Return early on first error
        }

        Ok(())
    }

    /// Stream a blob to a peer by enqueueing it in the buffer
    async fn stream(&self, peer: &PeerNode, blob: &Blob) -> Result<(), CommError> {
        let payload = self.prepare_payload(blob)?;
        self.enqueue_payload(peer, payload).await
    }

    /// Stream a blob to multiple peers in parallel
    async fn stream_mult(&self, peers: &[PeerNode], blob: &Blob) -> Result<(), CommError> {
        if peers.is_empty() {
            return Ok(());
        }

        let payload = self.prepare_payload(blob)?;
        let stream_futures: Vec<_> = peers
            .iter()
            .map(|peer| self.enqueue_payload(peer, payload.clone()))
            .collect();

        // Execute all streams in parallel and collect results
        let results = futures::future::join_all(stream_futures).await;

        // Check if any stream failed - if so, return the first error
        for result in results {
            result?; // Return early on first error
        }

        Ok(())
    }

    /// Disconnect from a peer, shutting down any gRPC channels
    async fn disconnect(&self, peer: &PeerNode) -> Result<(), CommError> {
        let mut channels_map = self.channels_map.lock().await;
        if let Some(channel_slot) = channels_map.remove(peer) {
            tracing::info!("Shutting down gRPC channel to peer {}", peer.to_address());
            if let Some(channel) = channel_slot.once_cell.get() {
                channel.buffer_subscriber.abort();
            }
        }
        Ok(())
    }

    /// Get the set of peers that have active channels
    async fn get_channeled_peers(&self) -> Result<std::collections::HashSet<PeerNode>, CommError> {
        let channels_map = self.channels_map.lock().await;
        Ok(channels_map.keys().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::peer_node::{Endpoint, NodeIdentifier};

    fn peer(name: &str) -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from(name.as_bytes().to_vec()),
            },
            endpoint: Endpoint::new("host".to_string(), 40400, 40404),
        }
    }

    fn client(network_timeout: Duration) -> GrpcTransportClient {
        GrpcTransportClient::new(
            "test".to_string(),
            "cert".to_string(),
            "key".to_string(),
            256 * 1024,
            4 * 1024,
            16,
            200 * 1024 * 1024,
            Arc::new(Mutex::new(HashMap::new())),
            network_timeout,
        )
        .unwrap()
    }

    #[test]
    fn new_accepts_normal_and_too_low_timeouts() {
        client(Duration::from_secs(5));
        client(Duration::from_millis(1));
    }

    #[tokio::test]
    async fn disconnect_unknown_peer_is_a_noop() {
        let c = client(Duration::from_secs(5));
        assert!(c.disconnect(&peer("unknown")).await.is_ok());
    }

    #[tokio::test]
    async fn broadcast_and_stream_mult_with_no_peers_succeed() {
        let c = client(Duration::from_secs(5));
        let msg = Protocol::default();
        assert!(c.broadcast(&[], &msg).await.is_ok());

        let blob = crate::rust::rp::protocol_helper::blob(&peer("s"), "T", b"x");
        assert!(c.stream_mult(&[], &blob).await.is_ok());

        assert!(c.get_channeled_peers().await.unwrap().is_empty());
    }
}
