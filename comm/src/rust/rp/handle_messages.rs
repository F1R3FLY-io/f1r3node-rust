// See comm/src/main/scala/coop/rchain/comm/rp/HandleMessages.scala

use std::sync::Arc;

use models::routing::{Packet, Protocol};

use crate::rust::errors::CommError;
use crate::rust::metrics_constants::{DISCONNECT_METRIC, RP_HANDLE_METRICS_SOURCE};
use crate::rust::p2p::packet_handler::PacketHandler;
use crate::rust::peer_node::PeerNode;
use crate::rust::rp::connect::ConnectionsCell;
use crate::rust::rp::protocol_helper::{self, PROTOCOL_FORMAT_VERSION};
use crate::rust::rp::rp_conf::RPConf;
use crate::rust::transport::communication_response::CommunicationResponse;
use crate::rust::transport::transport_layer::TransportLayer;

/// Compare a peer's advertised `format_version` against our own and log
/// anything surprising.  Returns the peer's version so callers can
/// decide what to do with it; the current policy is advisory-only
/// (log + continue) so a mid-rollout mixed network can still handshake.
///
/// Policy:
///   * `peer == 0`  — the peer predates the field; accept quietly.
///   * `peer == ours` — perfect; `debug!`.
///   * `peer != ours && peer != 0` — explicit mismatch; `warn!`.  The
///     connection still proceeds because the paper (§5 of
///     capnp-vs-protobuf.pdf) frames this as a diagnostic aid: operators
///     should see a clear log line before a block fails validation,
///     not an opaque decode error later.  Hard disconnect is a future
///     tightening.
fn note_peer_format_version(peer: &PeerNode, peer_version: u32, kind: &str) -> u32 {
    if peer_version == 0 {
        tracing::debug!(
            peer = %peer, kind,
            "peer did not advertise a protocol format version (field unset)",
        );
    } else if peer_version == PROTOCOL_FORMAT_VERSION {
        tracing::debug!(
            peer = %peer, kind, version = peer_version,
            "peer advertises matching protocol format version",
        );
    } else {
        tracing::warn!(
            peer = %peer, kind,
            peer_version, our_version = PROTOCOL_FORMAT_VERSION,
            "peer advertises a different protocol format version — \
             expect decode or validation errors on messages that depend \
             on the format (e.g. block hash scheme)",
        );
    }
    peer_version
}

pub async fn handle(
    protocol: &Protocol,
    transport_layer: Arc<dyn TransportLayer + Send + Sync + 'static>,
    packet_handler: Arc<dyn PacketHandler + Send + Sync + 'static>,
    connections_cell: &ConnectionsCell,
    rp_conf: &RPConf,
) -> Result<CommunicationResponse, CommError> {
    let sender = protocol_helper::sender(protocol);

    match &protocol.message {
        Some(models::routing::protocol::Message::Heartbeat(_)) => {
            handle_heartbeat(&sender, connections_cell)
        }

        Some(models::routing::protocol::Message::ProtocolHandshake(hs)) => {
            note_peer_format_version(&sender, hs.format_version, "handshake");
            handle_protocol_handshake(&sender, transport_layer, connections_cell, rp_conf).await
        }

        Some(models::routing::protocol::Message::ProtocolHandshakeResponse(hs)) => {
            note_peer_format_version(&sender, hs.format_version, "handshake_response");
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
    let response =
        protocol_helper::protocol_handshake_response(&rp_conf.local, &rp_conf.network_id);

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
