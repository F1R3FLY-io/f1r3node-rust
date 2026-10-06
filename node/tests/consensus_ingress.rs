use std::sync::Arc;
use std::time::Duration;

use comm::rust::errors::CommError;
use comm::rust::p2p::packet_handler::PacketHandler;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use consensus_api::{
    AdapterContext, Capabilities, ConsensusAdapter, ConsensusError, NetworkPacket,
    ProtocolDescriptor,
};
use consensus_runtime::{RuntimeBuilder, RuntimeConfig};
use models::routing::Packet;
use node::rust::consensus::ingress::ConsensusPacketHandler;
use tokio::sync::Mutex;

struct Recorder(Arc<Mutex<Vec<NetworkPacket>>>);

#[async_trait::async_trait]
impl ConsensusAdapter for Recorder {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        context.control.ready();
        loop {
            tokio::select! {
                _ = context.control.cancelled() => return Ok(()),
                Some(request) = context.packets.recv() => {
                    self.0.lock().await.push(request.packet);
                    let _ = request.reply.send(Ok(()));
                },
            }
        }
    }
}

#[tokio::test]
async fn ingress_preserves_native_bytes_and_uses_runtime_admission() {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let builder = RuntimeBuilder::new(
        ProtocolDescriptor {
            id: "independent".into(),
            version: 1,
            capabilities: Capabilities::NONE,
        },
        RuntimeConfig {
            command_capacity: 1,
            packet_capacity: 1,
            max_payload_bytes: 4,
            request_timeout: Duration::from_secs(1),
            drain_timeout: Duration::from_secs(1),
        },
    )
    .unwrap();
    let handle = builder.handle();
    let handler = ConsensusPacketHandler(handle.clone());
    let runtime = builder.build(Box::new(Recorder(recorded.clone()))).start();
    handle.wait_ready().await.unwrap();
    let peer = PeerNode {
        id: NodeIdentifier {
            key: vec![1, 2].into(),
        },
        endpoint: Endpoint::new("peer.example".into(), 123, 456),
    };
    let packet = Packet {
        type_id: "independent-native-message".into(),
        content: vec![0, 255, 8].into(),
    };
    handler.handle_packet(&peer, &packet).await.unwrap();
    let packets = recorded.lock().await;
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].peer.id, vec![1, 2]);
    assert_eq!(packets[0].peer.host, "peer.example");
    assert_eq!(packets[0].peer.tcp_port, 123);
    assert_eq!(packets[0].peer.udp_port, 456);
    assert_eq!(packets[0].kind, packet.type_id);
    assert_eq!(packets[0].payload, packet.content.as_ref());
    drop(packets);
    let oversized = Packet {
        type_id: packet.type_id.clone(),
        content: vec![0; 5].into(),
    };
    assert!(matches!(
        handler.handle_packet(&peer, &oversized).await,
        Err(CommError::ProtocolException(_))
    ));
    runtime.shutdown().await.unwrap();
    assert!(matches!(
        handler.handle_packet(&peer, &packet).await,
        Err(CommError::ProtocolException(_))
    ));
    assert_eq!(recorded.lock().await.len(), 1);
}
