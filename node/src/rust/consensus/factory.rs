use std::sync::Arc;

use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConfCell;
use comm::rust::transport::transport_layer::TransportLayer;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::rust::configuration::model::ConsensusProtocol;
use crate::rust::configuration::NodeConf;
use crate::rust::runtime::setup::PreparedNode;

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
#[error("Consensus protocol {protocol} is unavailable in this build; enable the {feature} Cargo feature")]
pub struct UnavailableProtocol {
    pub protocol: &'static str,
    pub feature: &'static str,
}

pub fn ensure_available(protocol: ConsensusProtocol) -> Result<(), UnavailableProtocol> {
    match protocol {
        ConsensusProtocol::CbcCasper if cfg!(feature = "cbc-casper") => Ok(()),
        ConsensusProtocol::CbcCasper => Err(UnavailableProtocol {
            protocol: "cbc-casper",
            feature: "cbc-casper",
        }),
    }
}

pub async fn prepare<T: TransportLayer + Send + Sync + Clone + 'static>(
    connections: ConnectionsCell,
    peer_conf: RPConfCell,
    transport: Arc<T>,
    conf: NodeConf,
    events: F1r3flyEvents,
    discovery: Arc<dyn NodeDiscovery + Send + Sync>,
) -> eyre::Result<PreparedNode> {
    ensure_available(conf.consensus.protocol)?;
    match conf.consensus.protocol {
        #[cfg(feature = "cbc-casper")]
        ConsensusProtocol::CbcCasper => super::casper::assembly::prepare(
            connections,
            peer_conf,
            transport,
            conf,
            events,
            discovery,
        )
        .await
        .map_err(Into::into),
        #[cfg(not(feature = "cbc-casper"))]
        ConsensusProtocol::CbcCasper => {
            let _ = (connections, peer_conf, transport, conf, events, discovery);
            Err(UnavailableProtocol {
                protocol: "cbc-casper",
                feature: "cbc-casper",
            }
            .into())
        }
    }
}
