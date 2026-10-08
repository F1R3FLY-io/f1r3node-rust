// See comm/src/main/scala/coop/rchain/comm/rp/HandleMessages.scala

use std::sync::Arc;

use models::routing::{Packet, Protocol};

use crate::rust::errors::CommError;
use crate::rust::metrics_constants::{
    DISCONNECT_METRIC, FOREIGN_CHAIN_REFUSED_METRIC, RP_HANDLE_METRICS_SOURCE,
};
use crate::rust::p2p::packet_handler::PacketHandler;
use crate::rust::peer_node::PeerNode;
use crate::rust::rp::connect::ConnectionsCell;
use crate::rust::rp::protocol_helper;
use crate::rust::rp::rp_conf::RPConf;
use crate::rust::transport::communication_response::CommunicationResponse;
use crate::rust::transport::transport_layer::TransportLayer;

fn peer_chain_id(protocol: &Protocol) -> &[u8] {
    protocol
        .header
        .as_ref()
        .map(|header| header.chain_id.as_ref())
        .unwrap_or_default()
}

fn is_handshake(protocol: &Protocol) -> bool {
    matches!(
        &protocol.message,
        Some(models::routing::protocol::Message::ProtocolHandshake(_))
            | Some(models::routing::protocol::Message::ProtocolHandshakeResponse(_))
    )
}

/// Refuse a peer that does not share this node's chain.
async fn refuse_foreign_chain(
    peer: &PeerNode,
    protocol: &Protocol,
    transport_layer: Arc<dyn TransportLayer + Send + Sync + 'static>,
    connections_cell: &ConnectionsCell,
    rp_conf: &RPConf,
    error: CommError,
) -> Result<CommunicationResponse, CommError> {
    metrics::counter!(FOREIGN_CHAIN_REFUSED_METRIC, "source" => RP_HANDLE_METRICS_SOURCE)
        .increment(1);
    tracing::warn!(peer = %peer, error = %error, "Refused a message from a peer of another chain");

    let _ = connections_cell.flat_modify(|connections| {
        if connections.iter().any(|known| known.id.key == peer.id.key) {
            connections.remove_conn_and_report(peer.clone())
        } else {
            Ok(connections)
        }
    });

    if is_handshake(protocol) {
        let goodbye = protocol_helper::disconnect(
            &rp_conf.local,
            &rp_conf.network_id,
            rp_conf.chain_id.to_wire(),
        );
        if let Err(e) = transport_layer.send(peer, &goodbye).await {
            tracing::debug!(peer = %peer, error = %e, "Could not tell a foreign-chain peer to disconnect");
        }
    }

    Ok(CommunicationResponse::not_handled(error))
}

pub async fn handle(
    protocol: &Protocol,
    transport_layer: Arc<dyn TransportLayer + Send + Sync + 'static>,
    packet_handler: Arc<dyn PacketHandler + Send + Sync + 'static>,
    connections_cell: &ConnectionsCell,
    rp_conf: &RPConf,
) -> Result<CommunicationResponse, CommError> {
    let sender = protocol_helper::sender(protocol);
    let is_packet = matches!(
        &protocol.message,
        Some(models::routing::protocol::Message::Packet(_))
    );

    if !is_packet {
        if let Err(error) =
            rp_conf.check_chain_id(&sender.to_string(), peer_chain_id(protocol), None)
        {
            return refuse_foreign_chain(
                &sender,
                protocol,
                transport_layer,
                connections_cell,
                rp_conf,
                error,
            )
            .await;
        }
    }

    match &protocol.message {
        Some(models::routing::protocol::Message::Heartbeat(_)) => {
            handle_heartbeat(&sender, connections_cell)
        }

        Some(models::routing::protocol::Message::ProtocolHandshake(_)) => {
            handle_protocol_handshake(&sender, transport_layer, connections_cell, rp_conf).await
        }

        Some(models::routing::protocol::Message::ProtocolHandshakeResponse(_)) => {
            handle_protocol_handshake_response(&sender, connections_cell)
        }

        Some(models::routing::protocol::Message::Disconnect(_)) => {
            handle_disconnect(&sender, connections_cell)
        }

        Some(models::routing::protocol::Message::Packet(packet)) => {
            handle_packet(&sender, packet, packet_handler).await
        }

        None => {
            let msg_str = format!("{:?}", protocol.message);
            tracing::error!(message = %msg_str, "unrecognized protocol message type");

            Ok(CommunicationResponse::not_handled(
                CommError::UnexpectedMessage(msg_str),
            ))
        }
    }
}

pub fn handle_disconnect(
    sender: &PeerNode,
    connections_cell: &ConnectionsCell,
) -> Result<CommunicationResponse, CommError> {
    tracing::info!("Forgetting about {}", sender);
    connections_cell
        .flat_modify(|connections| connections.remove_conn_and_report(sender.clone()))?;
    metrics::counter!(DISCONNECT_METRIC, "source" => RP_HANDLE_METRICS_SOURCE).increment(1);
    Ok(CommunicationResponse::handled_without_message())
}

pub async fn handle_packet(
    remote: &PeerNode,
    packet: &Packet,
    packet_handler: Arc<dyn PacketHandler + Send + Sync + 'static>,
) -> Result<CommunicationResponse, CommError> {
    tracing::debug!("Received packet from {}", remote);
    packet_handler.handle_packet(remote, packet).await?;
    Ok(CommunicationResponse::handled_without_message())
}

pub fn handle_protocol_handshake_response(
    peer: &PeerNode,
    connections_cell: &ConnectionsCell,
) -> Result<CommunicationResponse, CommError> {
    tracing::debug!("Received protocol handshake response from {}", peer);
    connections_cell.flat_modify(|connections| connections.add_conn_and_report(peer.clone()))?;
    Ok(CommunicationResponse::handled_without_message())
}

pub async fn handle_protocol_handshake(
    peer: &PeerNode,
    transport_layer: Arc<dyn TransportLayer + Send + Sync + 'static>,
    connections_cell: &ConnectionsCell,
    rp_conf: &RPConf,
) -> Result<CommunicationResponse, CommError> {
    let response = protocol_helper::protocol_handshake_response(
        &rp_conf.local,
        &rp_conf.network_id,
        rp_conf.chain_id.to_wire(),
    );

    match transport_layer.send(peer, &response).await {
        Ok(_) => {
            tracing::info!("Responded to protocol handshake request from {}", peer);
            match connections_cell
                .flat_modify(|connections| connections.add_conn_and_report(peer.clone()))
            {
                Ok(_) => {
                    tracing::info!(
                        "Successfully added {} to connections after responding to handshake",
                        peer
                    );
                }
                Err(e) => {
                    tracing::error!(peer = %peer, error = %e, "failed to add peer to connections after handshake response");
                }
            }
        }
        Err(CommError::DnsResolutionFailed(_, _)) => {}
        Err(e) => {
            tracing::warn!(
                "Failed to send protocol handshake response to {}: {}",
                peer,
                e
            );
        }
    }

    Ok(CommunicationResponse::handled_without_message())
}

pub fn handle_heartbeat(
    peer: &PeerNode,
    connections_cell: &ConnectionsCell,
) -> Result<CommunicationResponse, CommError> {
    let _ = connections_cell.flat_modify(|connections| connections.refresh_conn(peer.clone()));
    Ok(CommunicationResponse::handled_without_message())
}
