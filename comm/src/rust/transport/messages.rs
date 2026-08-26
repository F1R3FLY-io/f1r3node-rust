use models::routing::Protocol;

use crate::rust::peer_node::PeerNode;
use crate::rust::transport::payload_budget::PayloadReservation;

/// Server message types for transport layer communication
#[derive(Debug)]
pub enum ServerMessage {
    /// Send a protocol message
    Send(Send),
    /// Stream message containing metadata about a streamed blob
    StreamMessage(StreamMessage),
}

/// Send message containing a protocol to be transmitted
#[derive(Debug)]
pub struct Send {
    pub msg: Protocol,
    reservation: PayloadReservation,
}

impl Send {
    pub fn new(msg: Protocol, reservation: PayloadReservation) -> Self { Self { msg, reservation } }

    pub fn reserved_bytes(&self) -> usize { self.reservation.bytes() }

    pub fn into_parts(self) -> (Protocol, PayloadReservation) { (self.msg, self.reservation) }
}

/// Stream message containing metadata about a streamed blob
///
/// This represents the result of processing a streaming operation,
/// containing all necessary information to restore the original blob.
#[derive(Debug)]
pub struct StreamMessage {
    /// The peer that sent the stream
    pub sender: PeerNode,
    /// Type identifier of the streamed packet
    pub type_id: String,
    /// Whether the streamed data is compressed
    pub compressed: bool,
    /// Expected content length (for validation)
    pub content_length: i32,
    payload: Vec<u8>,
    reservation: PayloadReservation,
}

impl StreamMessage {
    /// Create a new StreamMessage
    pub fn new(
        sender: PeerNode,
        type_id: String,
        compressed: bool,
        content_length: i32,
        payload: Vec<u8>,
        reservation: PayloadReservation,
    ) -> Self {
        Self {
            sender,
            type_id,
            compressed,
            content_length,
            payload,
            reservation,
        }
    }

    pub fn reserved_bytes(&self) -> usize { self.reservation.bytes() }

    pub(crate) fn into_parts(self) -> (PeerNode, String, bool, i32, Vec<u8>, PayloadReservation) {
        (
            self.sender,
            self.type_id,
            self.compressed,
            self.content_length,
            self.payload,
            self.reservation,
        )
    }
}
