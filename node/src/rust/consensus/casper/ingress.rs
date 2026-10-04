use comm::rust::errors::CommError;
use comm::rust::p2p::packet_handler::PacketHandler;
use comm::rust::peer_node::PeerNode;
use consensus_api::{NetworkPacket, Peer};
use consensus_runtime::ConsensusHandle;
use models::routing::Packet;

pub struct ConsensusPacketHandler(pub ConsensusHandle);

#[async_trait::async_trait]
impl PacketHandler for ConsensusPacketHandler {
    async fn handle_packet(&self, peer: &PeerNode, packet: &Packet) -> Result<(), CommError> {
        self.0
            .handle_packet(NetworkPacket {
                peer: Peer {
                    id: peer.id.key.to_vec(),
                    host: peer.endpoint.host.clone(),
                    tcp_port: peer.endpoint.tcp_port,
                    udp_port: peer.endpoint.udp_port,
                },
                kind: packet.type_id.clone(),
                payload: packet.content.to_vec(),
            })
            .await
            .map_err(|error| CommError::CasperError(error.to_string()))
    }
}
