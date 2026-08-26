// See comm/src/main/scala/coop/rchain/comm/transport/StreamHandler.scala

use futures::StreamExt;
use models::routing::chunk::Content;
use models::routing::{Chunk, ChunkData, ChunkHeader};
use prost::bytes::Bytes;
use shared::rust::shared::compression::Compression;
use tokio_stream::Stream;
use tracing;

use crate::rust::errors::CommError;
use crate::rust::peer_node::PeerNode;
use crate::rust::rp::protocol_helper;
use crate::rust::transport::messages::StreamMessage;
use crate::rust::transport::payload_budget::{
    PayloadBudget, PayloadBudgetError, PayloadReservation,
};
use crate::rust::transport::transport_layer::Blob;

/// Type alias for circuit breaker function
/// Takes a Streamed state and returns a Circuit decision
pub type CircuitBreaker = fn(&Streamed) -> Circuit;

/// Header information for a streaming operation
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// The peer that sent the stream
    pub sender: PeerNode,
    /// Type identifier of the streamed packet
    pub type_id: String,
    /// Expected content length
    pub content_length: i32,
    /// Network ID for validation
    pub network_id: String,
    /// Whether the content is compressed
    pub compressed: bool,
}

impl Header {
    /// Create a new Header
    pub fn new(
        sender: PeerNode,
        type_id: String,
        content_length: i32,
        network_id: String,
        compressed: bool,
    ) -> Self {
        Self {
            sender,
            type_id,
            content_length,
            network_id,
            compressed,
        }
    }
}

/// Circuit breaker state for stream processing
#[derive(Debug, Clone, PartialEq)]
pub enum Circuit {
    /// Circuit is open (broken) due to an error
    Opened { error: StreamError },
    /// Circuit is closed (normal operation)
    Closed,
}

impl Circuit {
    /// Check if the circuit is broken
    pub fn broken(&self) -> bool { matches!(self, Circuit::Opened { .. }) }

    /// Create an opened circuit with an error
    pub fn opened(error: StreamError) -> Self { Circuit::Opened { error } }

    /// Create a closed circuit
    pub fn closed() -> Self { Circuit::Closed }
}

/// Stream error types
#[derive(Debug, Clone, PartialEq)]
pub enum StreamError {
    /// Wrong network ID detected
    WrongNetworkId,
    /// Maximum size reached
    MaxSizeReached,
    /// Incomplete message received
    NotFullMessage {
        streamed: String,
    },
    /// Unexpected error occurred
    Unexpected {
        error: String,
    },
    ResourceExhausted {
        error: String,
    },
}

impl StreamError {
    /// Get error message string
    pub fn message(&self) -> String {
        match self {
            StreamError::WrongNetworkId => {
                "Could not receive stream! Wrong network id.".to_string()
            }
            StreamError::MaxSizeReached => "Max message size was reached.".to_string(),
            StreamError::NotFullMessage { streamed } => {
                format!(
                    "Received not full stream message, will not process. {}",
                    streamed
                )
            }
            StreamError::Unexpected { error } => {
                format!("Could not receive stream! {}", error)
            }
            StreamError::ResourceExhausted { error } => {
                format!("Could not receive stream: resource exhausted: {}", error)
            }
        }
    }

    /// Create a WrongNetworkId error
    pub fn wrong_network_id() -> Self { StreamError::WrongNetworkId }

    /// Create a MaxSizeReached error (circuit opened)
    pub fn circuit_opened() -> Self { StreamError::MaxSizeReached }

    /// Create a NotFullMessage error
    pub fn not_full_message(streamed: String) -> Self { StreamError::NotFullMessage { streamed } }

    /// Create an Unexpected error
    pub fn unexpected(error: String) -> Self { StreamError::Unexpected { error } }

    pub fn resource_exhausted(error: String) -> Self { StreamError::ResourceExhausted { error } }
}

/// State of an ongoing streaming operation
#[derive(Debug)]
pub struct Streamed {
    /// Header information (if received)
    pub header: Option<Header>,
    /// Number of bytes read so far
    pub read_so_far: u64,
    /// Circuit breaker state
    pub circuit: Circuit,
    payload: Vec<u8>,
    reservation: PayloadReservation,
    max_payload_bytes: usize,
    max_wire_bytes: usize,
}

impl Streamed {
    pub fn new(
        reservation: PayloadReservation,
        max_payload_bytes: usize,
        max_wire_bytes: usize,
    ) -> Self {
        Self {
            header: None,
            read_so_far: 0,
            circuit: Circuit::Closed,
            payload: Vec::new(),
            reservation,
            max_payload_bytes,
            max_wire_bytes,
        }
    }

    /// Update the header
    pub fn with_header(mut self, header: Header) -> Self {
        self.header = Some(header);
        self
    }

    /// Update the read count
    pub fn with_read_so_far(mut self, read_so_far: u64) -> Self {
        self.read_so_far = read_so_far;
        self
    }

    /// Update the circuit state
    pub fn with_circuit(mut self, circuit: Circuit) -> Self {
        self.circuit = circuit;
        self
    }
}

impl std::fmt::Display for Streamed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Streamed(header: {:?}, read_so_far: {}, circuit_broken: {})",
            self.header.as_ref().map(|h| &h.type_id),
            self.read_so_far,
            self.circuit.broken()
        )
    }
}

/// StreamHandler provides functionality for processing streaming data
pub struct StreamHandler;

fn payload_budget_comm_error(error: PayloadBudgetError) -> CommError {
    CommError::ResourceExhausted(error.to_string())
}

fn payload_budget_stream_error(error: PayloadBudgetError) -> StreamError {
    StreamError::resource_exhausted(error.to_string())
}

impl StreamHandler {
    pub fn init(
        budget: &std::sync::Arc<PayloadBudget>,
        max_payload_bytes: usize,
    ) -> Result<Streamed, CommError> {
        let max_wire_bytes = Compression::max_compressed_allocation(max_payload_bytes)
            .ok_or_else(|| CommError::ConfigError("stream size bound overflow".to_string()))?;
        let reservation = budget.try_reserve(0).map_err(payload_budget_comm_error)?;
        Ok(Streamed::new(
            reservation,
            max_payload_bytes,
            max_wire_bytes,
        ))
    }

    /// Handle a stream of chunks with proper resource management
    ///
    /// This method processes a stream of chunks using the circuit breaker pattern
    /// and provides proper cleanup in all scenarios (success, failure, errors).
    pub async fn handle_stream<S, F>(
        stream: S,
        circuit_breaker: F,
        budget: &std::sync::Arc<PayloadBudget>,
        max_payload_bytes: usize,
    ) -> Result<StreamMessage, StreamError>
    where
        S: Stream<Item = Chunk> + Unpin,
        F: Fn(&Streamed) -> Circuit,
    {
        let init_stmd = match Self::init(budget, max_payload_bytes) {
            Ok(stmd) => stmd,
            Err(CommError::ResourceExhausted(error)) => {
                return Err(StreamError::resource_exhausted(error));
            }
            Err(error) => return Err(StreamError::unexpected(error.to_string())),
        };
        let result = Self::collect(init_stmd, stream, circuit_breaker)
            .await
            .and_then(Self::to_result);
        match result {
            Ok(stream_message) => {
                tracing::debug!("Stream collected.");
                Ok(stream_message)
            }
            Err(error) => {
                tracing::warn!("Failed collecting stream.");
                Err(error)
            }
        }
    }

    /// Collect and process chunks from a stream
    ///
    /// Processes each chunk in the stream, building up the Streamed state:
    /// - Header chunks: Creates Header and updates Streamed state
    /// - Data chunks: Appends owned data and updates read count
    /// - Unknown chunks: Sets an error
    pub async fn collect<S, F>(
        init: Streamed,
        mut stream: S,
        circuit_breaker: F,
    ) -> Result<Streamed, StreamError>
    where
        S: Stream<Item = Chunk> + Unpin,
        F: Fn(&Streamed) -> Circuit,
    {
        let mut current_state = init;

        // Process each chunk in the stream
        while let Some(chunk) = stream.next().await {
            current_state = match Self::process_chunk(current_state, chunk)? {
                ChunkProcessResult::Continue(state) => state,
                ChunkProcessResult::Error(error) => {
                    return Err(error);
                }
            };

            // Apply circuit breaker
            let circuit = circuit_breaker(&current_state);
            current_state = current_state.with_circuit(circuit.clone());

            // If circuit is broken, stop processing
            if circuit.broken() {
                if let Circuit::Opened { error } = circuit {
                    return Err(error);
                }
            }
        }

        // Check final state - if circuit is opened, return the error
        match &current_state.circuit {
            Circuit::Opened { error } => Err(error.clone()),
            Circuit::Closed => Ok(current_state),
        }
    }

    /// Convert a Streamed state to a StreamMessage result
    ///
    /// Validates the streamed state and creates a StreamMessage if valid:
    /// - Must have a valid header
    /// - For uncompressed content, read_so_far must equal content_length
    pub fn to_result(streamed: Streamed) -> Result<StreamMessage, StreamError> {
        let not_full_error = StreamError::not_full_message(streamed.to_string());
        let Some(header) = streamed.header else {
            return Err(not_full_error);
        };
        if !header.compressed && streamed.read_so_far != header.content_length as u64 {
            return Err(not_full_error);
        }
        Ok(StreamMessage::new(
            header.sender,
            header.type_id,
            header.compressed,
            header.content_length,
            streamed.payload,
            streamed.reservation,
        ))
    }

    /// Restore an owned StreamMessage into a Blob and its residency reservation
    pub async fn restore(msg: StreamMessage) -> Result<(Blob, PayloadReservation), CommError> {
        let (sender, type_id, compressed, content_length, payload, reservation) = msg.into_parts();
        let content = Self::decompress_content(payload, compressed, content_length).await?;
        let blob = Blob {
            sender,
            packet: models::routing::Packet {
                type_id,
                content: Bytes::from(content),
            },
        };
        Ok((blob, reservation))
    }

    /// Decompress content if compressed, otherwise return as-is
    async fn decompress_content(
        raw: Vec<u8>,
        compressed: bool,
        content_length: i32,
    ) -> Result<Vec<u8>, CommError> {
        let content_length = usize::try_from(content_length).map_err(|_| {
            CommError::InternalCommunicationError("Negative stream content length".to_string())
        })?;
        if compressed {
            match Compression::decompress(&raw, content_length) {
                Some(decompressed) => Ok(decompressed),
                None => {
                    let error = "Could not decompress data".to_string();
                    Err(CommError::InternalCommunicationError(error))
                }
            }
        } else {
            Ok(raw)
        }
    }

    /// Process a single chunk and update the Streamed state
    fn process_chunk(
        mut streamed: Streamed,
        chunk: Chunk,
    ) -> Result<ChunkProcessResult, StreamError> {
        match chunk.content {
            Some(Content::Header(chunk_header)) => {
                if streamed.header.is_some() {
                    return Ok(ChunkProcessResult::Error(StreamError::not_full_message(
                        "Stream contains more than one header".to_string(),
                    )));
                }
                let ChunkHeader {
                    sender,
                    type_id,
                    compressed,
                    content_length,
                    network_id,
                } = chunk_header;
                let declared_bytes = usize::try_from(content_length)
                    .map_err(|_| StreamError::unexpected("Negative content length".to_string()))?;
                if declared_bytes > streamed.max_payload_bytes {
                    return Ok(ChunkProcessResult::Error(StreamError::MaxSizeReached));
                }
                let peer_sender = match sender {
                    Some(node) => protocol_helper::to_peer_node(&node),
                    None => {
                        return Ok(ChunkProcessResult::Error(StreamError::not_full_message(
                            "Header chunk missing sender".to_string(),
                        )));
                    }
                };
                streamed
                    .reservation
                    .try_grow(declared_bytes)
                    .map_err(payload_budget_stream_error)?;
                let header =
                    Header::new(peer_sender, type_id, content_length, network_id, compressed);
                Ok(ChunkProcessResult::Continue(streamed.with_header(header)))
            }
            Some(Content::Data(chunk_data)) => {
                let Some(header) = streamed.header.as_ref() else {
                    return Ok(ChunkProcessResult::Error(StreamError::not_full_message(
                        "Stream data arrived before its header".to_string(),
                    )));
                };
                let ChunkData { content_data } = chunk_data;
                let received_bytes = content_data.len();
                let new_read_so_far = streamed
                    .read_so_far
                    .checked_add(received_bytes as u64)
                    .ok_or_else(|| StreamError::unexpected("Stream size overflow".to_string()))?;
                if (header.compressed && new_read_so_far > streamed.max_wire_bytes as u64)
                    || (!header.compressed && new_read_so_far > header.content_length as u64)
                {
                    return Ok(ChunkProcessResult::Error(StreamError::MaxSizeReached));
                }
                if header.compressed {
                    streamed
                        .reservation
                        .try_grow(received_bytes)
                        .map_err(payload_budget_stream_error)?;
                }
                streamed
                    .payload
                    .try_reserve_exact(received_bytes)
                    .map_err(|error| StreamError::unexpected(error.to_string()))?;
                streamed.payload.extend_from_slice(&content_data);
                Ok(ChunkProcessResult::Continue(
                    streamed.with_read_so_far(new_read_so_far),
                ))
            }
            None => {
                // Unknown/invalid chunk type
                Ok(ChunkProcessResult::Error(StreamError::not_full_message(
                    "Not all data received".to_string(),
                )))
            }
        }
    }
}

/// Result of processing a single chunk
enum ChunkProcessResult {
    /// Continue processing with the updated state
    Continue(Streamed),
    /// Stop processing due to an error
    Error(StreamError),
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::peer_node::{Endpoint, NodeIdentifier};

    fn create_test_peer() -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from("test_peer"),
            },
            endpoint: Endpoint::new("127.0.0.1".to_string(), 40400, 40400),
        }
    }

    #[tokio::test]
    async fn restore_failure_releases_payload_reservation() {
        let budget = PayloadBudget::new("test", 2048, 1).unwrap();
        let msg = StreamMessage::new(
            create_test_peer(),
            "TestPacket".to_string(),
            true,
            1024,
            vec![1, 2, 3, 4, 5, 6],
            budget.try_reserve(1030).unwrap(),
        );
        assert!(StreamHandler::restore(msg).await.is_err());
        assert_eq!(budget.used_bytes(), 0);
        assert_eq!(budget.active_items(), 0);
    }
}
