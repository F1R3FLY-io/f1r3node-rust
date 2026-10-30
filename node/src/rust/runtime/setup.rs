use std::sync::Arc;

use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::p2p::packet_handler::PacketHandler;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConfCell;
use comm::rust::transport::transport_layer::TransportLayer;
use consensus_runtime::PreparedConsensus;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::rust::configuration::NodeConf;
pub use crate::rust::runtime::application::PreparedApplication;

pub struct PreparedNode {
    pub consensus: PreparedConsensus,
    pub packet_handler: Arc<dyn PacketHandler>,
    pub application: PreparedApplication,
}

pub async fn setup_node_program<T: TransportLayer + Send + Sync + Clone + 'static>(
    connections: ConnectionsCell,
    peer_conf: RPConfCell,
    transport: Arc<T>,
    conf: NodeConf,
    events: F1r3flyEvents,
    discovery: Arc<dyn NodeDiscovery + Send + Sync>,
) -> eyre::Result<PreparedNode> {
    crate::rust::consensus::factory::prepare(
        connections,
        peer_conf,
        transport,
        conf,
        events,
        discovery,
    )
    .await
}
