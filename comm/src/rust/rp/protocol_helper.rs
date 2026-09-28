// See comm/src/main/scala/coop/rchain/comm/rp/ProtocolHelper.scala

use models::routing::{
    Disconnect, Header, Heartbeat, Node, Packet, Protocol, ProtocolHandshake,
    ProtocolHandshakeResponse,
};
use models::rust::casper::protocol::packet_type_tag::ToPacket;
use prost::bytes::Bytes;

use crate::rust::errors::{unknown_protocol, CommError};
use crate::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use crate::rust::transport::transport_layer::Blob;

pub fn to_protocol_bytes(x: &str) -> Vec<u8> { x.as_bytes().to_vec() }

pub fn header(src: &PeerNode, network_id: &str) -> Header {
    Header {
        sender: Some(node(src)),
        network_id: network_id.to_string(),
    }
}

pub fn node(n: &PeerNode) -> Node {
    Node {
        id: n.id.key.clone(),
        host: n.endpoint.host.clone().into(),
        tcp_port: n.endpoint.tcp_port,
        udp_port: n.endpoint.udp_port,
    }
}

pub fn sender(proto: &Protocol) -> PeerNode {
    to_peer_node(proto.header.as_ref().unwrap().sender.as_ref().unwrap())
}

pub fn to_peer_node(n: &Node) -> PeerNode {
    PeerNode {
        id: NodeIdentifier { key: n.id.clone() },
        endpoint: Endpoint::new(
            String::from_utf8_lossy(&n.host).to_string(),
            n.tcp_port,
            n.udp_port,
        ),
    }
}

pub fn protocol(src: &PeerNode, network_id: &str) -> Protocol {
    Protocol {
        header: Some(header(src, network_id)),
        ..Default::default()
    }
}

pub fn protocol_handshake(src: &PeerNode, network_id: &str) -> Protocol {
    Protocol {
        header: Some(header(src, network_id)),
        message: Some(models::routing::protocol::Message::ProtocolHandshake(
            ProtocolHandshake {
                nonce: Bytes::new(),
            },
        )),
    }
}

pub fn protocol_handshake_response(src: &PeerNode, network_id: &str) -> Protocol {
    Protocol {
        header: Some(header(src, network_id)),
        message: Some(
            models::routing::protocol::Message::ProtocolHandshakeResponse(
                ProtocolHandshakeResponse {
                    nonce: Bytes::new(),
                },
            ),
        ),
    }
}

pub fn heartbeat(src: &PeerNode, network_id: &str) -> Protocol {
    Protocol {
        header: Some(header(src, network_id)),
        message: Some(models::routing::protocol::Message::Heartbeat(Heartbeat {})),
    }
}

pub fn packet(src: &PeerNode, network_id: &str, packet: Packet) -> Protocol {
    Protocol {
        header: Some(header(src, network_id)),
        message: Some(models::routing::protocol::Message::Packet(packet)),
    }
}

pub fn packet_with_content<A>(src: &PeerNode, network_id: &str, content: A) -> Protocol
where A: ToPacket {
    packet(src, network_id, content.mk_packet())
}

pub fn to_packet(proto: &Protocol) -> Result<Packet, CommError> {
    match &proto.message {
        Some(models::routing::protocol::Message::Packet(packet)) => Ok(packet.clone()),
        _ => Err(unknown_protocol(format!(
            "Was expecting Packet, got {:?}",
            proto.message
        ))),
    }
}

pub fn disconnect(src: &PeerNode, network_id: &str) -> Protocol {
    Protocol {
        header: Some(header(src, network_id)),
        message: Some(models::routing::protocol::Message::Disconnect(
            Disconnect {},
        )),
    }
}

pub fn blob(sender: &PeerNode, type_id: &str, content: &[u8]) -> Blob {
    Blob {
        sender: sender.clone(),
        packet: Packet {
            type_id: type_id.to_string(),
            content: Bytes::copy_from_slice(content),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(name: &str, host: &str) -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from(name.as_bytes().to_vec()),
            },
            endpoint: Endpoint::new(host.to_string(), 40400, 40404),
        }
    }

    #[test]
    fn to_protocol_bytes_returns_utf8_bytes() {
        assert_eq!(to_protocol_bytes("abc"), b"abc".to_vec());
    }

    #[test]
    fn sender_round_trips_through_node() {
        let src = peer("sender-id", "sender-host");
        let proto = protocol(&src, "net");
        assert_eq!(sender(&proto), src);
        assert!(proto.message.is_none());
        assert_eq!(proto.header.as_ref().unwrap().network_id, "net");
    }

    #[test]
    fn disconnect_carries_disconnect_message() {
        let src = peer("id", "host");
        let proto = disconnect(&src, "net");
        assert_eq!(
            proto.message,
            Some(models::routing::protocol::Message::Disconnect(
                Disconnect {}
            ))
        );
        assert_eq!(sender(&proto), src);
    }

    #[test]
    fn handshake_response_carries_response_message() {
        let src = peer("id", "host");
        let proto = protocol_handshake_response(&src, "net");
        assert_eq!(
            proto.message,
            Some(
                models::routing::protocol::Message::ProtocolHandshakeResponse(
                    ProtocolHandshakeResponse {
                        nonce: Bytes::new(),
                    }
                )
            )
        );
    }

    #[test]
    fn to_packet_extracts_packet_message() {
        let src = peer("id", "host");
        let pkt = Packet {
            type_id: "BlockMessage".to_string(),
            content: Bytes::from_static(b"payload"),
        };
        let proto = packet(&src, "net", pkt.clone());
        assert_eq!(to_packet(&proto).unwrap(), pkt);
    }

    #[test]
    fn to_packet_rejects_non_packet_message() {
        let src = peer("id", "host");
        let proto = heartbeat(&src, "net");
        let err = to_packet(&proto).unwrap_err();
        match err {
            CommError::UnknownProtocolError(msg) => {
                assert!(msg.contains("Was expecting Packet"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn blob_copies_content_into_packet() {
        let src = peer("id", "host");
        let b = blob(&src, "DataMessage", b"data");
        assert_eq!(b.sender, src);
        assert_eq!(b.packet.type_id, "DataMessage");
        assert_eq!(b.packet.content, Bytes::from_static(b"data"));
    }
}
