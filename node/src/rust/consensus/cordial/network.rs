use std::sync::Arc;

use comm::rust::errors::CommError;
use comm::rust::p2p::packet_handler::PacketHandler;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConfCell;
use comm::rust::transport::transport_layer::TransportLayer;
use consensus_api::{ConsensusError, NetworkPacket, Peer};
use consensus_runtime::ConsensusHandle;
use cordial_consensus::PeerNetwork;
use models::routing::Packet;

pub struct CordialTransport<T> {
    pub transport: Arc<T>,
    pub connections: ConnectionsCell,
    pub configuration: RPConfCell,
}

fn neutral(peer: PeerNode) -> Peer {
    Peer {
        id: peer.id.key.to_vec(),
        host: peer.endpoint.host,
        tcp_port: peer.endpoint.tcp_port,
        udp_port: peer.endpoint.udp_port,
    }
}

fn native(peer: &Peer) -> PeerNode {
    PeerNode {
        id: NodeIdentifier {
            key: peer.id.clone().into(),
        },
        endpoint: Endpoint::new(peer.host.clone(), peer.tcp_port, peer.udp_port),
    }
}

#[async_trait::async_trait]
impl<T: TransportLayer + Send + Sync + 'static> PeerNetwork for CordialTransport<T> {
    fn peers(&self) -> Vec<Peer> {
        match self.connections.read() {
            Ok(connections) => connections.into_vec().into_iter().map(neutral).collect(),
            Err(error) => {
                tracing::error!(%error, "Cannot read Cordial peers");
                vec![]
            }
        }
    }

    async fn send(&self, peer: &Peer, kind: &str, payload: Vec<u8>) -> Result<(), ConsensusError> {
        let configuration = self
            .configuration
            .read()
            .map_err(|error| ConsensusError::Protocol(error.to_string()))?;
        self.transport
            .send_packet_to_peer(&configuration, &native(peer), Packet {
                type_id: kind.into(),
                content: payload.into(),
            })
            .await
            .map_err(|error| ConsensusError::Protocol(error.to_string()))
    }
}

pub struct CordialPacketHandler(pub ConsensusHandle);

#[async_trait::async_trait]
impl PacketHandler for CordialPacketHandler {
    async fn handle_packet(&self, peer: &PeerNode, packet: &Packet) -> Result<(), CommError> {
        self.0
            .handle_packet(NetworkPacket {
                peer: neutral(peer.clone()),
                kind: packet.type_id.clone(),
                payload: packet.content.to_vec(),
            })
            .await
            .map_err(|error| CommError::ProtocolException(error.to_string()))
    }
}
