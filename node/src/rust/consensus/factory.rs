use std::sync::Arc;

use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConfCell;
use comm::rust::transport::transport_layer::TransportLayer;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::rust::configuration::NodeConf;
use crate::rust::runtime::setup::PreparedNode;

pub async fn prepare<T: TransportLayer + Send + Sync + Clone + 'static>(
    connections: ConnectionsCell,
    peer_conf: RPConfCell,
    transport: Arc<T>,
    conf: NodeConf,
    events: F1r3flyEvents,
    discovery: Arc<dyn NodeDiscovery + Send + Sync>,
) -> eyre::Result<PreparedNode> {
    match conf.consensus.protocol {
        crate::rust::configuration::model::ConsensusProtocol::CordialMiners => {
            super::cordial::prepare(connections, peer_conf, transport, conf).await
        }
        crate::rust::configuration::model::ConsensusProtocol::CbcCasper => {
            super::casper::assembly::prepare(
                connections,
                peer_conf,
                transport,
                conf,
                events,
                discovery,
            )
            .await
            .map_err(Into::into)
        }
    }
}
