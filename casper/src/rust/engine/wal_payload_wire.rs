// Phase 7b-2 WAL payload-fetch wire glue (2026-08-27).
//
// Bridges the pure-logic `WalPayloadRetriever` /
// `wal_payload_server` primitives to the comm-layer's
// TransportLayer.  Two directions:
//
//   * Outbound requests + responses — helper functions that build
//     a Packet from the appropriate proto + hand it to the
//     transport's `send_message_to_peer` / `send_message_to_peers`.
//
//   * Inbound dispatch — `maybe_route_wal_payload_message` routes
//     a decoded `CasperMessage` variant to the appropriate handler
//     (serve_payload, admit_response, has_wal_payload_announcement).
//
// This module deliberately does NOT own retriever state or peer
// selection — those live in `wal_payload_sync.rs`.  Free functions
// here so tests can exercise the wire path against mock transports
// without pulling in the sync-driver's whole state machine.
//
// Mirrors `snapshot_chunk_wire.rs` in shape.

use std::sync::Arc;

use comm::rust::errors::CommError;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::casper::{
    GetWalPayloadRequestProto, HasWalPayloadProto, HasWalPayloadRequestProto,
    WalPayloadResponseProto,
};
use models::rust::casper::protocol::casper_message::{
    CasperMessage, GetWalPayloadRequest, HasWalPayloadRequest, WalPayloadResponse,
};
use prost::bytes::Bytes;
use tracing::{debug, warn};

use crate::rust::engine::wal_payload_retriever::{AdmitOutcome, WalPayloadRetriever};
use crate::rust::engine::wal_payload_server::{
    has_wal_payload_announcement, serve_payload, PayloadLookup,
};

/// Send a `GetWalPayloadRequest` to a single peer.
pub async fn send_get_wal_payload_request<T: TransportLayer + Send + Sync>(
    transport: &T,
    conf: &RPConf,
    peer: &PeerNode,
    payload_hash: &[u8],
) -> Result<(), CommError> {
    debug!(
        target: "f1r3fly.casper.wal_payload_wire",
        payload_hash = hex::encode(payload_hash),
        peer = %peer.endpoint.host,
        "requesting wal payload"
    );
    let proto = GetWalPayloadRequestProto {
        payload_hash: Bytes::copy_from_slice(payload_hash),
    };
    transport
        .send_message_to_peer(conf, peer, Arc::new(proto))
        .await
}

/// Broadcast a `HasWalPayloadRequest` to all connected peers.
/// Sent when a joiner wants to discover which peers can serve a
/// particular payload.
pub async fn broadcast_has_wal_payload_request<T: TransportLayer + Send + Sync>(
    transport: &T,
    connections_cell: &ConnectionsCell,
    conf: &RPConf,
    payload_hash: &[u8],
) -> Result<(), CommError> {
    debug!(
        target: "f1r3fly.casper.wal_payload_wire",
        payload_hash = hex::encode(payload_hash),
        "broadcasting HasWalPayloadRequest"
    );
    let proto = HasWalPayloadRequestProto {
        payload_hash: Bytes::copy_from_slice(payload_hash),
    };
    transport
        .send_message_to_peers(connections_cell, conf, Arc::new(proto), None)
        .await
}

/// Reply to a `GetWalPayloadRequest` with a `WalPayloadResponse`.
pub async fn send_wal_payload_response<T: TransportLayer + Send + Sync>(
    transport: &T,
    conf: &RPConf,
    peer: &PeerNode,
    response: WalPayloadResponse,
) -> Result<(), CommError> {
    let proto: WalPayloadResponseProto = response.to_proto();
    transport
        .send_message_to_peer(conf, peer, Arc::new(proto))
        .await
}

/// Reply to a `HasWalPayloadRequest` with a `HasWalPayload` announcement.
pub async fn send_has_wal_payload<T: TransportLayer + Send + Sync>(
    transport: &T,
    conf: &RPConf,
    peer: &PeerNode,
    announcement: models::rust::casper::protocol::casper_message::HasWalPayload,
) -> Result<(), CommError> {
    let proto: HasWalPayloadProto = announcement.to_proto();
    transport
        .send_message_to_peer(conf, peer, Arc::new(proto))
        .await
}

/// Serve an inbound `GetWalPayloadRequest`.  Looks up the payload
/// bytes in the backing store and forwards a response to the
/// requester.  Silent on unknown-payload (peer should ask someone
/// else) so we don't advertise our cache misses.
pub async fn handle_get_wal_payload_request<
    T: TransportLayer + Send + Sync,
    L: PayloadLookup + ?Sized,
>(
    transport: &T,
    conf: &RPConf,
    sender: &PeerNode,
    request: &GetWalPayloadRequest,
    lookup: &L,
) {
    match serve_payload(request.payload_hash.as_ref(), lookup) {
        Ok(response) => {
            if let Err(e) = send_wal_payload_response(transport, conf, sender, response).await {
                warn!(
                    target: "f1r3fly.casper.wal_payload_wire",
                    error = %e,
                    "failed to send WalPayloadResponse"
                );
            }
        }
        Err(e) => {
            debug!(
                target: "f1r3fly.casper.wal_payload_wire",
                payload_hash = hex::encode(request.payload_hash.as_ref()),
                error = ?e,
                "serve_payload declined; not replying"
            );
        }
    }
}

/// Serve an inbound `HasWalPayloadRequest`.  Same lookup shape;
/// replies with a `HasWalPayload` announcement if the payload is
/// available locally.
pub async fn handle_has_wal_payload_request<
    T: TransportLayer + Send + Sync,
    L: PayloadLookup + ?Sized,
>(
    transport: &T,
    conf: &RPConf,
    sender: &PeerNode,
    request: &HasWalPayloadRequest,
    lookup: &L,
) {
    match has_wal_payload_announcement(request.payload_hash.as_ref(), lookup) {
        Ok(announcement) => {
            if let Err(e) = send_has_wal_payload(transport, conf, sender, announcement).await {
                warn!(
                    target: "f1r3fly.casper.wal_payload_wire",
                    error = %e,
                    "failed to send HasWalPayload"
                );
            }
        }
        Err(e) => {
            debug!(
                target: "f1r3fly.casper.wal_payload_wire",
                payload_hash = hex::encode(request.payload_hash.as_ref()),
                error = ?e,
                "has_wal_payload_announcement declined; not replying"
            );
        }
    }
}

/// Serve an inbound `WalPayloadResponse` — calls admit_response on
/// the retriever.  Returns the outcome so callers can decide
/// whether to re-request from a different peer.
pub async fn handle_wal_payload_response(
    response: &WalPayloadResponse,
    retriever: &Arc<WalPayloadRetriever>,
) -> AdmitOutcome {
    retriever.admit_response(response).await
}

/// Convenience dispatcher that routes a decoded CasperMessage to
/// the appropriate handler.  Callers plug this into the main
/// packet dispatch loop after `casper_message_from_proto` decodes
/// the proto.  Returns whether the message was routed here (true)
/// so the outer dispatcher can skip its own block-message branches
/// on already-handled messages.
pub async fn maybe_route_wal_payload_message<T, L>(
    transport: &T,
    conf: &RPConf,
    sender: &PeerNode,
    message: &CasperMessage,
    lookup: &L,
    retriever: &Arc<WalPayloadRetriever>,
) -> bool
where
    T: TransportLayer + Send + Sync,
    L: PayloadLookup + ?Sized,
{
    match message {
        CasperMessage::GetWalPayloadRequest(req) => {
            handle_get_wal_payload_request(transport, conf, sender, req, lookup).await;
            true
        }
        CasperMessage::HasWalPayloadRequest(req) => {
            handle_has_wal_payload_request(transport, conf, sender, req, lookup).await;
            true
        }
        CasperMessage::WalPayloadResponse(resp) => {
            let _outcome = handle_wal_payload_response(resp, retriever).await;
            true
        }
        CasperMessage::HasWalPayload(_) => {
            // HasWalPayload announcements are consumed by the sync-driver
            // (via a dedicated queue) — not by this dispatcher.  Return
            // true so the outer dispatcher doesn't double-handle it.
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::engine::wal_payload_server::InMemoryPayloadStore;

    /// End-to-end: server produces a response, we route it through
    /// `handle_wal_payload_response` against a matching retriever,
    /// and confirm acceptance.
    #[tokio::test]
    async fn response_routes_to_retriever() {
        let store = InMemoryPayloadStore::new();
        let payload = b"routed payload".to_vec();
        let h = store.insert(payload.clone()).await;

        let response = tokio::task::spawn_blocking({
            let store = store.clone();
            move || serve_payload(&h, &store).unwrap()
        })
        .await
        .unwrap();

        let retriever = Arc::new(WalPayloadRetriever::new());
        retriever.enqueue(h).await;

        let outcome = handle_wal_payload_response(&response, &retriever).await;
        assert_eq!(outcome, AdmitOutcome::PayloadAccepted);
        assert!(retriever.is_complete().await);
    }
}
