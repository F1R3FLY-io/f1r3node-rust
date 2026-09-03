// See comm/src/main/scala/coop/rchain/comm/errors.scala

use std::fmt;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CommError {
    UnknownCommError(String),
    DatagramSizeError(usize),
    DatagramFramingError(String),
    DatagramException(String),
    HeaderNotAvailable,
    ProtocolException(String),
    UnknownProtocolError(String),
    PublicKeyNotAvailable(String),
    ParseError(String),
    EncryptionHandshakeIncorrectlySigned,
    BootstrapNotProvided,
    PeerNodeNotFound(String),
    PeerUnavailable(String),
    WrongNetwork(String, String),
    MessageToLarge(String),
    MalformedMessage(String),
    CouldNotConnectToBootstrap,
    InternalCommunicationError(String),
    TimeOut,
    UpstreamNotAvailable,
    UnexpectedMessage(String),
    SenderNotAvailable,
    PongNotReceivedForPing(String),
    UnableToStorePacket(String, String),
    UnableToRestorePacket(String, String),
    ConfigError(String),
    CasperError(String),
    ResourceExhausted(String),
}

impl fmt::Display for CommError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CommError::PeerUnavailable(_) => write!(f, "Peer is currently unavailable"),
            CommError::MessageToLarge(p) => {
                write!(f, "Message rejected by peer {} because it was too large", p)
            }
            CommError::PongNotReceivedForPing(_) => write!(
                f,
                "Peer is behind a firewall and can't be accessed from outside"
            ),
            CommError::CouldNotConnectToBootstrap => {
                write!(f, "Node could not connect to bootstrap node")
            }
            CommError::TimeOut => write!(f, "Timeout"),
            CommError::InternalCommunicationError(msg) => {
                write!(f, "Internal communication error. {}", msg)
            }
            CommError::UnknownProtocolError(msg) => write!(f, "Unknown protocol error. {}", msg),
            CommError::UnableToStorePacket(p, er) => {
                write!(f, "Could not serialize packet {}. Error message: {}", p, er)
            }
            CommError::UnableToRestorePacket(p, er) => write!(
                f,
                "Could not deserialize packet {}. Error message: {}",
                p, er
            ),
            CommError::ProtocolException(msg) => write!(f, "Protocol error. {}", msg),
            CommError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            CommError::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            CommError::CasperError(msg) => write!(f, "Casper error: {}", msg),
            CommError::ResourceExhausted(msg) => write!(f, "Resource exhausted: {}", msg),
            _ => write!(f, "{:?}", self),
        }
    }
}

// Helper functions matching Scala's API
pub fn unknown_comm_error(msg: String) -> CommError { CommError::UnknownCommError(msg) }

pub fn unknown_protocol(msg: String) -> CommError { CommError::UnknownProtocolError(msg) }

pub fn parse_error(msg: String) -> CommError { CommError::ParseError(msg) }

pub fn protocol_exception(msg: String) -> CommError { CommError::ProtocolException(msg) }

pub fn header_not_available() -> CommError { CommError::HeaderNotAvailable }

pub fn peer_node_not_found(peer: String) -> CommError { CommError::PeerNodeNotFound(peer) }

pub fn peer_unavailable(peer: String) -> CommError { CommError::PeerUnavailable(peer) }

pub fn wrong_network(peer: String, msg: String) -> CommError { CommError::WrongNetwork(peer, msg) }

pub fn message_too_large(peer: String) -> CommError { CommError::MessageToLarge(peer) }

pub fn public_key_not_available(peer: String) -> CommError {
    CommError::PublicKeyNotAvailable(peer)
}

pub fn could_not_connect_to_bootstrap() -> CommError { CommError::CouldNotConnectToBootstrap }

pub fn internal_communication_error(msg: String) -> CommError {
    CommError::InternalCommunicationError(msg)
}

pub fn malformed_message(msg: String) -> CommError { CommError::MalformedMessage(msg) }

pub fn upstream_not_available() -> CommError { CommError::UpstreamNotAvailable }

pub fn unexpected_message(msg: String) -> CommError { CommError::UnexpectedMessage(msg) }

pub fn sender_not_available() -> CommError { CommError::SenderNotAvailable }

pub fn pong_not_received_for_ping(peer: String) -> CommError {
    CommError::PongNotReceivedForPing(peer)
}

pub fn timeout() -> CommError { CommError::TimeOut }

pub fn unable_to_store_packet(packet: String, error: String) -> CommError {
    CommError::UnableToStorePacket(packet, error)
}

pub fn unable_to_restore_packet(key: String, error: String) -> CommError {
    CommError::UnableToRestorePacket(key, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_uses_specific_messages() {
        assert_eq!(
            peer_unavailable("p1".to_string()).to_string(),
            "Peer is currently unavailable"
        );
        assert_eq!(
            message_too_large("p1".to_string()).to_string(),
            "Message rejected by peer p1 because it was too large"
        );
        assert_eq!(
            pong_not_received_for_ping("p1".to_string()).to_string(),
            "Peer is behind a firewall and can't be accessed from outside"
        );
        assert_eq!(
            could_not_connect_to_bootstrap().to_string(),
            "Node could not connect to bootstrap node"
        );
        assert_eq!(timeout().to_string(), "Timeout");
        assert_eq!(
            internal_communication_error("oops".to_string()).to_string(),
            "Internal communication error. oops"
        );
        assert_eq!(
            unknown_protocol("bad".to_string()).to_string(),
            "Unknown protocol error. bad"
        );
        assert_eq!(
            unable_to_store_packet("pkt".to_string(), "err".to_string()).to_string(),
            "Could not serialize packet pkt. Error message: err"
        );
        assert_eq!(
            unable_to_restore_packet("key".to_string(), "err".to_string()).to_string(),
            "Could not deserialize packet key. Error message: err"
        );
        assert_eq!(
            protocol_exception("boom".to_string()).to_string(),
            "Protocol error. boom"
        );
        assert_eq!(
            parse_error("nope".to_string()).to_string(),
            "Parse error: nope"
        );
        assert_eq!(
            CommError::ConfigError("bad conf".to_string()).to_string(),
            "Configuration error: bad conf"
        );
        assert_eq!(
            CommError::CasperError("halted".to_string()).to_string(),
            "Casper error: halted"
        );
    }

    #[test]
    fn display_falls_back_to_debug_for_other_variants() {
        assert_eq!(header_not_available().to_string(), "HeaderNotAvailable");
        assert_eq!(
            unknown_comm_error("x".to_string()).to_string(),
            "UnknownCommError(\"x\")"
        );
    }

    #[test]
    fn helper_constructors_build_matching_variants() {
        assert_eq!(
            peer_node_not_found("p".to_string()),
            CommError::PeerNodeNotFound("p".to_string())
        );
        assert_eq!(
            wrong_network("p".to_string(), "m".to_string()),
            CommError::WrongNetwork("p".to_string(), "m".to_string())
        );
        assert_eq!(
            public_key_not_available("p".to_string()),
            CommError::PublicKeyNotAvailable("p".to_string())
        );
        assert_eq!(
            malformed_message("m".to_string()),
            CommError::MalformedMessage("m".to_string())
        );
        assert_eq!(upstream_not_available(), CommError::UpstreamNotAvailable);
        assert_eq!(
            unexpected_message("m".to_string()),
            CommError::UnexpectedMessage("m".to_string())
        );
        assert_eq!(sender_not_available(), CommError::SenderNotAvailable);
    }
}
