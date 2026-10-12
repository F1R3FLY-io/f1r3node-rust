// Phase 7b-1 snapshot chunk-fetch wire glue (2026-08-27).
//
// Bridges the pure-logic `SnapshotChunkRetriever` /
// `snapshot_chunk_server` primitives to the comm-layer's
// TransportLayer.  Two directions:
//
//   * Outbound requests + responses — helper functions that build
//     a Packet from the appropriate proto + hand it to the
//     transport's `send_packet_to_peer` / `send_message_to_peers`.
//
//   * Inbound dispatch — `handle_incoming_snapshot_message` routes
//     a decoded `CasperMessage` variant to the appropriate handler
//     (serve_chunk, admit_response, has_snapshot_announcement).
//
// This module deliberately does NOT own retriever state or peer
// selection — those live in the joiner sync-driver
// (`snapshot_chunk_sync.rs`).  Free functions here so tests can
// exercise the wire path against mock transports without pulling
// in the sync-driver's whole state machine.

use std::path::Path;
use std::sync::Arc;

use comm::rust::errors::CommError;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::casper::{
    GetSnapshotChunkRequestProto, HasSnapshotProto, HasSnapshotRequestProto,
    SnapshotChunkResponseProto,
};
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    CasperMessage, GetSnapshotChunkRequest, HasSnapshotRequest, SnapshotChunkResponse,
};
use prost::bytes::Bytes;
use tracing::{debug, warn};

use crate::rust::engine::snapshot_chunk_retriever::{AdmitOutcome, SnapshotChunkRetriever};
use crate::rust::engine::snapshot_chunk_server::{
    has_snapshot_announcement, serve_chunk, ServeError,
};

/// Send a `GetSnapshotChunkRequest` to a single peer.
pub async fn send_get_snapshot_chunk_request<T: TransportLayer + Send + Sync>(
    transport: &T,
    conf: &RPConf,
    peer: &PeerNode,
    block_hash: &[u8],
    chunk_index: u32,
) -> Result<(), CommError> {
    debug!(
        target: "f1r3fly.casper.snapshot_chunk_wire",
        block_hash = %PrettyPrinter::build_string_no_limit(block_hash),
        chunk_index,
        peer = %peer.endpoint.host,
        "requesting snapshot chunk"
    );
    let proto = GetSnapshotChunkRequestProto {
        block_hash: Bytes::copy_from_slice(block_hash),
        chunk_index,
    };
    transport
        .send_message_to_peer(conf, peer, Arc::new(proto))
        .await
}

/// Broadcast a `HasSnapshotRequest` to all connected peers.
/// Sent when a joiner wants to discover which peers can serve a
/// particular snapshot.
pub async fn broadcast_has_snapshot_request<T: TransportLayer + Send + Sync>(
    transport: &T,
    connections_cell: &ConnectionsCell,
    conf: &RPConf,
    block_hash: &[u8],
) -> Result<(), CommError> {
    debug!(
        target: "f1r3fly.casper.snapshot_chunk_wire",
        block_hash = %PrettyPrinter::build_string_no_limit(block_hash),
        "broadcasting HasSnapshotRequest"
    );
    let proto = HasSnapshotRequestProto {
        block_hash: Bytes::copy_from_slice(block_hash),
    };
    transport
        .send_message_to_peers(connections_cell, conf, Arc::new(proto), None)
        .await
}

/// Reply to a `GetSnapshotChunkRequest` with a
/// `SnapshotChunkResponse`.  Sender is the requesting peer.
pub async fn send_snapshot_chunk_response<T: TransportLayer + Send + Sync>(
    transport: &T,
    conf: &RPConf,
    peer: &PeerNode,
    response: SnapshotChunkResponse,
) -> Result<(), CommError> {
    let proto: SnapshotChunkResponseProto = response.to_proto();
    transport
        .send_message_to_peer(conf, peer, Arc::new(proto))
        .await
}

/// Reply to a `HasSnapshotRequest` with a `HasSnapshot` announcement.
pub async fn send_has_snapshot<T: TransportLayer + Send + Sync>(
    transport: &T,
    conf: &RPConf,
    peer: &PeerNode,
    announcement: models::rust::casper::protocol::casper_message::HasSnapshot,
) -> Result<(), CommError> {
    let proto: HasSnapshotProto = announcement.to_proto();
    transport
        .send_message_to_peer(conf, peer, Arc::new(proto))
        .await
}

/// Serve an inbound `GetSnapshotChunkRequest`.  Looks up the
/// requested snapshot's anchor in `snapshot_merkle_roots`, calls
/// `serve_chunk`, and forwards the response to the requester.
///
/// `sender` is the peer that made the request; `snapshot_dir` is
/// the local snapshot cache dir; `anchor_lookup` returns
/// `(atomic_root, merkle_root)` for a given block hash or None if
/// not cached.  The three arguments are threaded rather than
/// passing a full RuntimeManager reference so this handler is
/// testable in isolation.
pub async fn handle_get_snapshot_chunk_request<T: TransportLayer + Send + Sync, F>(
    transport: &T,
    conf: &RPConf,
    sender: &PeerNode,
    request: &GetSnapshotChunkRequest,
    snapshot_dir: &Path,
    anchor_lookup: F,
) where
    F: FnOnce(&[u8]) -> Option<([u8; 32], [u8; 32])>,
{
    let anchor = match anchor_lookup(request.block_hash.as_ref()) {
        Some(a) => a,
        None => {
            debug!(
                target: "f1r3fly.casper.snapshot_chunk_wire",
                block_hash = %PrettyPrinter::build_string_no_limit(request.block_hash.as_ref()),
                chunk_index = request.chunk_index,
                "no local anchor for requested block; ignoring GetSnapshotChunkRequest"
            );
            return;
        }
    };
    match serve_chunk(
        request.block_hash.as_ref(),
        request.chunk_index,
        anchor,
        snapshot_dir,
    ) {
        Ok(response) => {
            if let Err(e) = send_snapshot_chunk_response(transport, conf, sender, response).await {
                warn!(
                    target: "f1r3fly.casper.snapshot_chunk_wire",
                    error = %e,
                    "failed to send SnapshotChunkResponse"
                );
            }
        }
        Err(e) => {
            warn!(
                target: "f1r3fly.casper.snapshot_chunk_wire",
                block_hash = %PrettyPrinter::build_string_no_limit(request.block_hash.as_ref()),
                chunk_index = request.chunk_index,
                error = ?e,
                "serve_chunk failed"
            );
        }
    }
}

/// Serve an inbound `HasSnapshotRequest`.  Same anchor-lookup
/// pattern as chunk requests; replies with a `HasSnapshot`
/// announcement if the snapshot is available locally.
pub async fn handle_has_snapshot_request<T: TransportLayer + Send + Sync, F>(
    transport: &T,
    conf: &RPConf,
    sender: &PeerNode,
    request: &HasSnapshotRequest,
    snapshot_dir: &Path,
    anchor_lookup: F,
) where
    F: FnOnce(&[u8]) -> Option<([u8; 32], [u8; 32])>,
{
    let anchor = match anchor_lookup(request.block_hash.as_ref()) {
        Some(a) => a,
        None => {
            debug!(
                target: "f1r3fly.casper.snapshot_chunk_wire",
                block_hash = %PrettyPrinter::build_string_no_limit(request.block_hash.as_ref()),
                "no local anchor for HasSnapshotRequest; not replying"
            );
            return;
        }
    };
    match has_snapshot_announcement(request.block_hash.as_ref(), anchor, snapshot_dir) {
        Ok(announcement) => {
            if let Err(e) = send_has_snapshot(transport, conf, sender, announcement).await {
                warn!(
                    target: "f1r3fly.casper.snapshot_chunk_wire",
                    error = %e,
                    "failed to send HasSnapshot"
                );
            }
        }
        Err(ServeError::AtomicRootMismatch) => {
            warn!(
                target: "f1r3fly.casper.snapshot_chunk_wire",
                block_hash = %PrettyPrinter::build_string_no_limit(request.block_hash.as_ref()),
                "local snapshot atomic-root mismatch; not announcing"
            );
        }
        Err(e) => {
            debug!(
                target: "f1r3fly.casper.snapshot_chunk_wire",
                block_hash = %PrettyPrinter::build_string_no_limit(request.block_hash.as_ref()),
                error = ?e,
                "has_snapshot_announcement failed"
            );
        }
    }
}

/// Serve an inbound `SnapshotChunkResponse`.  Looks up the
/// matching retriever by block_hash and calls `admit_response`.
/// Returns the outcome so callers can decide whether to
/// re-request the chunk from a different peer.
pub async fn handle_snapshot_chunk_response<F>(
    response: &SnapshotChunkResponse,
    retriever_lookup: F,
) -> AdmitOutcome
where
    F: FnOnce(&[u8]) -> Option<Arc<SnapshotChunkRetriever>>,
{
    let retriever = match retriever_lookup(response.block_hash.as_ref()) {
        Some(r) => r,
        None => {
            debug!(
                target: "f1r3fly.casper.snapshot_chunk_wire",
                block_hash = %PrettyPrinter::build_string_no_limit(response.block_hash.as_ref()),
                chunk_index = response.chunk_index,
                "no active retriever for received SnapshotChunkResponse"
            );
            return AdmitOutcome::UnknownRequest;
        }
    };
    retriever.admit_response(response).await
}

/// Convenience dispatcher that routes a decoded CasperMessage to
/// the appropriate handler.  Callers plug this into the main
/// packet dispatch loop after `casper_message_from_proto` decodes
/// the proto.  Returns whether the message was routed here
/// (true) so the outer dispatcher can skip its own block-message
/// branches on already-handled messages.
pub async fn maybe_route_snapshot_message<T, FA, FR>(
    transport: &T,
    conf: &RPConf,
    sender: &PeerNode,
    message: &CasperMessage,
    snapshot_dir: &Path,
    anchor_lookup: FA,
    retriever_lookup: FR,
) -> bool
where
    T: TransportLayer + Send + Sync,
    FA: FnOnce(&[u8]) -> Option<([u8; 32], [u8; 32])>,
    FR: FnOnce(&[u8]) -> Option<Arc<SnapshotChunkRetriever>>,
{
    match message {
        CasperMessage::GetSnapshotChunkRequest(req) => {
            handle_get_snapshot_chunk_request(
                transport,
                conf,
                sender,
                req,
                snapshot_dir,
                anchor_lookup,
            )
            .await;
            true
        }
        CasperMessage::HasSnapshotRequest(req) => {
            handle_has_snapshot_request(transport, conf, sender, req, snapshot_dir, anchor_lookup)
                .await;
            true
        }
        CasperMessage::SnapshotChunkResponse(resp) => {
            let _outcome = handle_snapshot_chunk_response(resp, retriever_lookup).await;
            true
        }
        CasperMessage::HasSnapshot(_) => {
            // HasSnapshot announcements are consumed by the sync-driver
            // (via a dedicated queue) — not by this dispatcher.  Return
            // true so the outer dispatcher doesn't double-handle it.
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::io::snapshot::write_snapshot;
    use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

    use super::*;
    use crate::rust::engine::snapshot_chunk_retriever::SnapshotTarget;

    fn mk_entry(tag: &str) -> WalEntry {
        WalEntry {
            op: WalOp::Write,
            path: std::path::PathBuf::from(format!("/{tag}")),
            extra_path: None,
            offset: Some(0),
            length: Some(tag.len() as u64),
            payload_ref: Some(PayloadRef::hash(tag.as_bytes())),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    /// End-to-end: server produces a response, we route it through
    /// `handle_snapshot_chunk_response` against a matching retriever,
    /// and confirm acceptance.
    #[tokio::test]
    async fn response_routes_to_matching_retriever() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("a"), mk_entry("b")];
        let (_p, atomic_root, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let block_hash = vec![0xEEu8; 32];

        // Retriever for this block.
        let retriever = Arc::new(SnapshotChunkRetriever::new(SnapshotTarget {
            block_hash: block_hash.clone(),
            merkle_root,
            chunk_count: 1,
        }));

        // Server-produced response.
        let response = serve_chunk(&block_hash, 0, (atomic_root, merkle_root), dir.path()).unwrap();

        let retriever_for_lookup = Arc::clone(&retriever);
        let outcome = handle_snapshot_chunk_response(&response, |bh| {
            if bh == retriever_for_lookup.target.block_hash.as_slice() {
                Some(Arc::clone(&retriever_for_lookup))
            } else {
                None
            }
        })
        .await;
        assert_eq!(outcome, AdmitOutcome::ChunkAccepted);
        assert!(retriever.is_complete().await);
    }

    /// Response for an unknown block hash returns UnknownRequest.
    #[tokio::test]
    async fn response_for_unknown_block_returns_unknown_request() {
        let response = SnapshotChunkResponse {
            block_hash: Bytes::from_static(&[0xAA; 32]),
            chunk_index: 0,
            chunk_bytes: Bytes::from_static(&[0x99]),
            chunk_hash: Bytes::from_static(&[0x00; 32]),
            merkle_root: Bytes::from_static(&[0x00; 32]),
            chunk_count: 1,
            merkle_proof: vec![],
        };
        let outcome = handle_snapshot_chunk_response(&response, |_| None).await;
        assert_eq!(outcome, AdmitOutcome::UnknownRequest);
    }
}
