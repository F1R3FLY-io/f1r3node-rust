// See comm/src/main/scala/coop/rchain/comm/transport/TransportLayer.scala
// See comm/src/main/scala/coop/rchain/comm/transport/TransportLayerSyntax.scala
// See casper/src/main/scala/coop/rchain/casper/util/comm/CommUtil.scala

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use models::casper::{
    ApprovedBlockRequestProto, BlockHashMessageProto, BlockRequestProto, ForkChoiceTipRequestProto,
    HasBlockRequestProto,
};
use models::routing::{Packet, Protocol};
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::packet_type_tag::ToPacket;
use prost::bytes::Bytes;
use tracing::{info, warn};

use crate::rust::errors::CommError;
use crate::rust::peer_node::PeerNode;
use crate::rust::rp::connect::ConnectionsCell;
use crate::rust::rp::protocol_helper;
use crate::rust::rp::rp_conf::RPConf;

#[derive(Clone)]
pub struct Blob {
    pub sender: PeerNode,
    pub packet: Packet,
}

/// A fan-out succeeds if it reached at least one peer. One unreachable or
/// refusing peer must not fail the operation for the peers that received it.
pub(crate) fn best_effort(
    operation: &str,
    peers: &[PeerNode],
    results: Vec<Result<(), CommError>>,
) -> Result<(), CommError> {
    let mut first_error = None;
    let mut delivered = false;
    for (peer, result) in peers.iter().zip(results) {
        match result {
            Ok(()) => delivered = true,
            Err(error) => {
                tracing::debug!(peer = %peer.to_address(), error = %error, "{} to one peer failed", operation);
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) if !delivered => Err(error),
        _ => Ok(()),
    }
}

#[async_trait]
pub trait TransportLayer {
    async fn send(&self, peer: &PeerNode, msg: &Protocol) -> Result<(), CommError>;

    async fn broadcast(&self, peers: &[PeerNode], msg: &Protocol) -> Result<(), CommError>;

    async fn stream(&self, peer: &PeerNode, blob: &Blob) -> Result<(), CommError>;

    async fn stream_mult(&self, peers: &[PeerNode], blob: &Blob) -> Result<(), CommError>;

    /// Disconnect from a peer, shutting down any gRPC channels
    async fn disconnect(&self, peer: &PeerNode) -> Result<(), CommError>;

    /// Get the set of peers that have active channels
    async fn get_channeled_peers(&self) -> Result<std::collections::HashSet<PeerNode>, CommError>;

    // See comm/src/main/scala/coop/rchain/comm/transport/TransportLayerSyntax.scala

    async fn send_packet_to_peer(
        &self,
        conf: &RPConf,
        peer: &PeerNode,
        msg: Packet,
    ) -> Result<(), CommError> {
        let protocol_msg =
            protocol_helper::packet(&conf.local, &conf.network_id, conf.chain_id.to_wire(), msg);
        self.send(peer, &protocol_msg).await
    }

    async fn send_message_to_peer(
        &self,
        conf: &RPConf,
        peer: &PeerNode,
        msg: Arc<dyn ToPacket + Send + Sync>,
    ) -> Result<(), CommError> {
        let packet = msg.mk_packet();
        self.send_packet_to_peer(conf, peer, packet).await
    }

    async fn stream_packet_to_peer(
        &self,
        conf: &RPConf,
        peer: &PeerNode,
        packet: Packet,
    ) -> Result<(), CommError> {
        let blob = Blob {
            sender: conf.local.clone(),
            packet,
        };
        self.stream(peer, &blob).await
    }

    async fn stream_message_to_peer(
        &self,
        conf: &RPConf,
        peer: &PeerNode,
        msg: Arc<dyn ToPacket + Send + Sync>,
    ) -> Result<(), CommError> {
        let packet = msg.mk_packet();
        self.stream_packet_to_peer(conf, peer, packet).await
    }

    async fn send_to_bootstrap(
        &self,
        conf: &RPConf,
        msg: Arc<dyn ToPacket + Send + Sync>,
    ) -> Result<(), CommError> {
        let bootstrap = conf
            .bootstrap
            .as_ref()
            .ok_or_else(|| CommError::ConfigError("No bootstrap peer configured".to_string()))?;
        self.send_message_to_peer(conf, bootstrap, msg).await
    }

    // See casper/src/main/scala/coop/rchain/casper/util/comm/CommUtil.scala

    async fn send_packet_to_peers(
        &self,
        connections_cell: &ConnectionsCell,
        conf: &RPConf,
        message: Packet,
        scope_size: Option<usize>,
    ) -> Result<(), CommError> {
        let max = scope_size.unwrap_or(conf.max_num_of_connections);
        let peers = connections_cell.random(max)?.into_vec();
        let protocol_msg = protocol_helper::packet(
            &conf.local,
            &conf.network_id,
            conf.chain_id.to_wire(),
            message,
        );
        let results =
            futures::future::join_all(peers.iter().map(|peer| self.send(peer, &protocol_msg)))
                .await;

        for (peer, result) in peers.iter().zip(&results) {
            if let Err(error @ CommError::WrongChainId(..)) = result {
                warn!("Disconnecting from {}: {}", peer, error);
                let _ = connections_cell.flat_modify(|connections| {
                    if connections.iter().any(|known| known.id.key == peer.id.key) {
                        connections.remove_conn_and_report(peer.clone())
                    } else {
                        Ok(connections)
                    }
                });
                let _ = self.disconnect(peer).await;
            }
        }

        best_effort("broadcast", &peers, results)
    }

    async fn stream_packet_to_peers(
        &self,
        connections_cell: &ConnectionsCell,
        conf: &RPConf,
        message: Packet,
        scope_size: Option<usize>,
    ) -> Result<(), CommError> {
        let max = scope_size.unwrap_or(conf.max_num_of_connections);
        let peers = connections_cell.random(max)?;
        let blob = Blob {
            sender: conf.local.clone(),
            packet: message,
        };
        self.stream_mult(&peers.0, &blob).await
    }

    async fn send_with_retry(
        &self,
        conf: &RPConf,
        message: Packet,
        peer: &PeerNode,
        retry_after: Duration,
        msg_type_name: &str,
    ) -> Result<(), CommError> {
        let protocol_msg = protocol_helper::packet(
            &conf.local,
            &conf.network_id,
            conf.chain_id.to_wire(),
            message,
        );

        info!("Starting to request {}", msg_type_name);

        loop {
            match self.send(peer, &protocol_msg).await {
                Ok(()) => {
                    info!("Successfully sent {} to {}", msg_type_name, peer);
                    return Ok(());
                }
                Err(error) => {
                    warn!(
                        "Failed to send {} to {} because of {}. Retrying in {:?}...",
                        msg_type_name, peer, error, retry_after
                    );
                    tokio::time::sleep(retry_after).await;
                }
            }
        }
    }

    async fn request_for_block(
        &self,
        conf: &RPConf,
        peer: &PeerNode,
        hash: BlockHash,
    ) -> Result<(), CommError> {
        tracing::debug!(
            "Requesting {} from {}",
            PrettyPrinter::build_string_no_limit(&hash),
            peer.endpoint.host,
        );

        self.send_packet_to_peer(conf, peer, BlockRequestProto { hash }.mk_packet())
            .await
    }

    // CommUtilOps

    async fn send_message_to_peers(
        &self,
        connections_cell: &ConnectionsCell,
        conf: &RPConf,
        message: Arc<dyn ToPacket + Send + Sync>,
        scope_size: Option<usize>,
    ) -> Result<(), CommError> {
        self.send_packet_to_peers(connections_cell, conf, message.mk_packet(), scope_size)
            .await
    }

    async fn stream_message_to_peers(
        &self,
        connections_cell: &ConnectionsCell,
        conf: &RPConf,
        message: Arc<dyn ToPacket + Send + Sync>,
        scope_size: Option<usize>,
    ) -> Result<(), CommError> {
        self.stream_packet_to_peers(connections_cell, conf, message.mk_packet(), scope_size)
            .await
    }

    async fn send_block_hash(
        &self,
        connections_cell: &ConnectionsCell,
        conf: &RPConf,
        hash: &BlockHash,
        block_creator: &Bytes,
    ) -> Result<(), CommError> {
        let result = self
            .send_message_to_peers(
                connections_cell,
                conf,
                Arc::new(BlockHashMessageProto {
                    hash: hash.clone(),
                    block_creator: block_creator.clone(),
                }),
                None,
            )
            .await;

        tracing::info!(
            "Sent hash {} to peers",
            PrettyPrinter::build_string_no_limit(hash)
        );

        result
    }

    async fn broadcast_has_block_request(
        &self,
        connections_cell: &ConnectionsCell,
        conf: &RPConf,
        hash: &BlockHash,
    ) -> Result<(), CommError> {
        self.send_message_to_peers(
            connections_cell,
            conf,
            Arc::new(HasBlockRequestProto { hash: hash.clone() }),
            None,
        )
        .await
    }

    async fn broadcast_request_for_block(
        &self,
        connections_cell: &ConnectionsCell,
        conf: &RPConf,
        hash: &BlockHash,
    ) -> Result<(), CommError> {
        self.send_message_to_peers(
            connections_cell,
            conf,
            Arc::new(BlockRequestProto { hash: hash.clone() }),
            None,
        )
        .await
    }

    async fn send_fork_choice_tip_request(
        &self,
        connections_cell: &ConnectionsCell,
        conf: &RPConf,
    ) -> Result<(), CommError> {
        let result = self
            .send_message_to_peers(
                connections_cell,
                conf,
                Arc::new(ForkChoiceTipRequestProto {}),
                None,
            )
            .await;

        tracing::info!("Requested fork tip from peers");

        result
    }

    async fn request_approved_block(
        &self,
        conf: &RPConf,
        trim_state: Option<bool>,
    ) -> Result<(), CommError> {
        self.send_with_retry(
            conf,
            ApprovedBlockRequestProto {
                identifier: "".to_string(),
                trim_state: trim_state.unwrap_or(true),
            }
            .mk_packet(),
            conf.bootstrap.as_ref().ok_or_else(|| {
                CommError::ConfigError(
                    "StandaloneNodeSendToBootstrapError: No bootstrap peer configured".to_string(),
                )
            })?,
            Duration::from_secs(10),
            "ApprovedBlockRequest",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::peer_node::{Endpoint, NodeIdentifier};
    use crate::rust::rp::connect::Connections;
    use crate::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};

    fn peer(name: &str) -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from(name.as_bytes().to_vec()),
            },
            endpoint: Endpoint::new("host".to_string(), 40400, 40404),
        }
    }

    #[tokio::test]
    async fn broadcast_drops_a_connected_peer_that_refuses_our_chain() {
        let cell = ConnectionsCell::new();
        cell.flat_modify(|_| {
            Ok(Connections::from_vec(vec![
                peer("foreign"),
                peer("slow"),
                peer("good"),
            ]))
        })
        .unwrap();
        let transport = TransportLayerStub::new();
        transport.set_responses(|remote, _| {
            if remote.id.key == peer("foreign").id.key {
                Err(CommError::WrongChainId(
                    "foreign".to_string(),
                    "other chain".to_string(),
                ))
            } else if remote.id.key == peer("slow").id.key {
                Err(CommError::TimeOut)
            } else {
                Ok(())
            }
        });
        let conf = create_rp_conf_ask(peer("local"), None, None);
        let packet = Packet {
            type_id: "BlockHashMessage".to_string(),
            content: Bytes::new(),
        };

        transport
            .send_packet_to_peers(&cell, &conf, packet, None)
            .await
            .unwrap();

        let mut left = cell.read().unwrap().into_vec();
        left.sort_by(|a, b| a.id.key.cmp(&b.id.key));
        assert_eq!(left, vec![peer("good"), peer("slow")]);
    }
}
