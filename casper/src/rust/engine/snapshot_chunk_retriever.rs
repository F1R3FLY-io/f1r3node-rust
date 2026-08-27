// Phase 7b-1 SnapshotChunkRetriever (2026-08-27).
//
// Consumer-side counterpart of `SnapshotChunkResponse`.  Tracks
// which (block_hash, chunk_index) pairs a joining validator still
// needs; validates each incoming chunk against the anchored Merkle
// root (from `RuntimeManager.snapshot_merkle_roots`); returns
// verified chunk bytes ready for assembly into the reconstructed
// snapshot.
//
// Mirrors the shape of `BlockRetriever` at
// `casper/src/rust/engine/block_retriever.rs` but scoped to the
// per-chunk problem: a snapshot is many chunks, each fetched
// independently, each verified independently.  Under partial
// success (M of N chunks arrive, one is byzantine) we can accept
// the M and re-request the failing index without discarding
// progress.
//
// # Layers of separation
//
// This module deliberately does NOT touch the comm layer directly.
// It exposes:
//
//   * `RequestState` per pending chunk — peers tried, last request
//     timestamp, retry count.
//   * `admit_response(response, expected_merkle_root)` — the
//     verification + accept path.  Returns AdmitOutcome describing
//     what to do next (deliver bytes, retry chunk, ignore).
//
// The comm-layer glue (peer selection, wire send, timeout ticker)
// lives in a follow-up slice; this module is testable in isolation
// with no network.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::casper::protocol::casper_message::SnapshotChunkResponse;
use rholang::rust::interpreter::io::snapshot_chunk::{verify_merkle_proof, MerkleProof};
use tokio::sync::RwLock;
use tracing::debug;

/// Per-chunk request state.  One entry per (block_hash, chunk_index)
/// pair the joiner is trying to fetch.
#[derive(Debug, Clone)]
pub struct ChunkRequestState {
    /// Which chunk this is (0..chunk_count).
    pub chunk_index: u32,
    /// Unix timestamp (ms) of the last outbound request for this
    /// chunk.  Zero if never requested (fresh entry).
    pub last_request_ms: u64,
    /// Unix timestamp (ms) of the FIRST outbound request.  Used
    /// for stale-eviction: an entry idle for too long gets
    /// dropped.
    pub initial_request_ms: u64,
    /// Peers we've asked so far.  Used to rotate through peers on
    /// retry rather than re-asking the same node.  Represented as
    /// opaque `Vec<u8>` peer identifiers (matches BlockRetriever's
    /// `HashSet<PeerNode>`; kept as bytes here to avoid pulling in
    /// comm types).
    pub peers_tried: Vec<Vec<u8>>,
    /// Number of retry attempts across peers.  Increments on each
    /// timeout without a valid response.  Retriever gives up after
    /// `MAX_RETRIES` and marks the request Failed.
    pub retry_count: u32,
    /// Verified chunk bytes, or None while pending.
    pub bytes: Option<Vec<u8>>,
}

/// Configuration constants.  Tuned for typical joiner scenarios;
/// values mirror BlockRetriever's defaults.
pub const MAX_RETRIES: u32 = 5;
pub const REQUEST_TIMEOUT_MS: u64 = 30_000;
pub const STALE_EVICTION_MS: u64 = 300_000;

/// Snapshot's expected shape as announced by peers.  Populated
/// from a HasSnapshotResponse OR from local `snapshot_merkle_roots`
/// lookup.  The joiner pins the snapshot's merkle_root + chunk
/// count up front so every incoming chunk verifies against a
/// known target.
#[derive(Debug, Clone)]
pub struct SnapshotTarget {
    pub block_hash: Vec<u8>,
    pub merkle_root: [u8; 32],
    pub chunk_count: u32,
}

/// Outcome of `admit_response`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmitOutcome {
    /// The chunk verified and its bytes are now available.  Caller
    /// can hand them to the snapshot assembler.
    ChunkAccepted,
    /// The (block_hash, chunk_index) key has no matching request
    /// — the response is unsolicited or belongs to a different
    /// snapshot.  Drop it silently (log at debug).
    UnknownRequest,
    /// The response's `block_hash` does not match this retriever's
    /// active target.  Caller should route to the correct
    /// retriever if multi-target fetching is in play.
    WrongTarget,
    /// The response's `merkle_root` disagrees with the anchored
    /// target.  Peer is byzantine or lied about which snapshot it
    /// has.  Caller should re-request from a different peer + log.
    MerkleRootMismatch,
    /// The chunk_hash doesn't match `Blake2b256(chunk_bytes)`.
    /// Peer's response is malformed.
    ChunkHashMismatch,
    /// The Merkle inclusion proof doesn't verify against the
    /// anchored root.  Peer sent a chunk that isn't in this
    /// snapshot.
    MerkleProofInvalid,
    /// The `chunk_count` doesn't match the anchored target's
    /// count.  Peer lied about the shape.
    ChunkCountMismatch,
}

/// Per-snapshot retriever state.  One instance per snapshot the
/// joiner is fetching in parallel (typically 1-2 concurrently
/// during initial sync).
#[derive(Debug, Clone)]
pub struct SnapshotChunkRetriever {
    /// The snapshot being fetched.
    pub target: SnapshotTarget,
    /// Per-chunk request state, keyed by chunk_index.
    pub chunks: Arc<RwLock<HashMap<u32, ChunkRequestState>>>,
}

impl SnapshotChunkRetriever {
    pub fn new(target: SnapshotTarget) -> Self {
        let mut chunks = HashMap::with_capacity(target.chunk_count as usize);
        for i in 0..target.chunk_count {
            chunks.insert(i, ChunkRequestState {
                chunk_index: i,
                last_request_ms: 0,
                initial_request_ms: 0,
                peers_tried: Vec::new(),
                retry_count: 0,
                bytes: None,
            });
        }
        Self {
            target,
            chunks: Arc::new(RwLock::new(chunks)),
        }
    }

    /// Number of chunks still needed (pending or in-flight, not
    /// yet verified).
    pub async fn pending_count(&self) -> usize {
        let g = self.chunks.read().await;
        g.values().filter(|c| c.bytes.is_none()).count()
    }

    /// True iff every chunk has been received + verified.
    pub async fn is_complete(&self) -> bool { self.pending_count().await == 0 }

    /// Enumerate chunk indices that still need to be fetched.
    /// Ordered ascending for deterministic peer request patterns.
    pub async fn pending_indices(&self) -> Vec<u32> {
        let g = self.chunks.read().await;
        let mut out: Vec<u32> = g
            .values()
            .filter(|c| c.bytes.is_none())
            .map(|c| c.chunk_index)
            .collect();
        out.sort();
        out
    }

    /// Assemble the verified chunk bytes into the reconstructed
    /// snapshot.  Returns None if not yet complete.  Chunks are
    /// concatenated in index order.
    pub async fn assemble(&self) -> Option<Vec<u8>> {
        let g = self.chunks.read().await;
        let mut out = Vec::new();
        for i in 0..self.target.chunk_count {
            let entry = g.get(&i)?;
            let bytes = entry.bytes.as_ref()?;
            out.extend_from_slice(bytes);
        }
        Some(out)
    }

    /// Ingest a `SnapshotChunkResponse`.  Verifies:
    ///   1. block_hash matches this retriever's target.
    ///   2. merkle_root matches this retriever's target.
    ///   3. chunk_count matches this retriever's target.
    ///   4. chunk_hash == Blake2b256(chunk_bytes).
    ///   5. Merkle inclusion proof verifies against target root.
    ///
    /// On success, stores the verified bytes and returns
    /// `ChunkAccepted`.  On failure, returns the specific
    /// mismatch variant so the caller can log + retry appropriately.
    pub async fn admit_response(&self, response: &SnapshotChunkResponse) -> AdmitOutcome {
        if response.block_hash.as_ref() != self.target.block_hash.as_slice() {
            return AdmitOutcome::WrongTarget;
        }
        if response.chunk_count != self.target.chunk_count {
            return AdmitOutcome::ChunkCountMismatch;
        }
        let anchored_root: [u8; 32] = self.target.merkle_root;
        let response_root = match slice_to_hash(&response.merkle_root) {
            Some(h) => h,
            None => return AdmitOutcome::MerkleRootMismatch,
        };
        if response_root != anchored_root {
            return AdmitOutcome::MerkleRootMismatch;
        }
        // Chunk-hash self-consistency: hash of returned bytes must
        // equal the response's chunk_hash.  Defends against a
        // peer that swaps bytes but forgets to update the hash
        // (or vice versa).
        let hash_of_bytes = hash_bytes(&response.chunk_bytes);
        let claimed_hash = match slice_to_hash(&response.chunk_hash) {
            Some(h) => h,
            None => return AdmitOutcome::ChunkHashMismatch,
        };
        if hash_of_bytes != claimed_hash {
            return AdmitOutcome::ChunkHashMismatch;
        }
        // Merkle inclusion: the chunk_hash at chunk_index must
        // participate in the tree rooted at anchored_root.
        let siblings: Vec<([u8; 32], bool)> = response
            .merkle_proof
            .iter()
            .filter_map(|step| {
                slice_to_hash(&step.sibling_hash).map(|h| (h, step.is_sibling_right))
            })
            .collect();
        if siblings.len() != response.merkle_proof.len() {
            // At least one step had a malformed sibling hash.
            return AdmitOutcome::MerkleProofInvalid;
        }
        let proof = MerkleProof {
            index: response.chunk_index,
            siblings,
        };
        if !verify_merkle_proof(&anchored_root, &claimed_hash, &proof) {
            return AdmitOutcome::MerkleProofInvalid;
        }
        // All checks pass — accept the chunk.
        let mut g = self.chunks.write().await;
        match g.get_mut(&response.chunk_index) {
            Some(state) => {
                if state.bytes.is_some() {
                    // Duplicate arrival for a chunk we already
                    // verified.  Idempotent: drop silently.
                    debug!(
                        target: "f1r3fly.casper.snapshot_chunk_retriever",
                        chunk_index = response.chunk_index,
                        "duplicate response for already-accepted chunk"
                    );
                    return AdmitOutcome::ChunkAccepted;
                }
                state.bytes = Some(response.chunk_bytes.to_vec());
                AdmitOutcome::ChunkAccepted
            }
            None => AdmitOutcome::UnknownRequest,
        }
    }

    /// Mark a chunk request as sent — updates timestamps + peer
    /// tracking.  Called by the outbound-request pipeline.
    pub async fn record_request_sent(&self, chunk_index: u32, peer_id: &[u8]) {
        let now = now_ms();
        let mut g = self.chunks.write().await;
        if let Some(state) = g.get_mut(&chunk_index) {
            if state.initial_request_ms == 0 {
                state.initial_request_ms = now;
            }
            state.last_request_ms = now;
            if !state.peers_tried.iter().any(|p| p == peer_id) {
                state.peers_tried.push(peer_id.to_vec());
            }
        }
    }

    /// Enumerate chunks whose last request has timed out AND still
    /// have retries left.  The caller should re-issue requests for
    /// these to alternative peers.
    pub async fn timed_out_indices(&self) -> Vec<u32> {
        let now = now_ms();
        let g = self.chunks.read().await;
        g.values()
            .filter(|c| {
                c.bytes.is_none()
                    && c.last_request_ms > 0
                    && now.saturating_sub(c.last_request_ms) >= REQUEST_TIMEOUT_MS
                    && c.retry_count < MAX_RETRIES
            })
            .map(|c| c.chunk_index)
            .collect()
    }

    /// Increment the retry count for a chunk (called after
    /// requeuing it to a new peer).
    pub async fn record_retry(&self, chunk_index: u32) {
        let mut g = self.chunks.write().await;
        if let Some(state) = g.get_mut(&chunk_index) {
            state.retry_count += 1;
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    let h = Blake2b256::hash(bytes.to_vec());
    assert_eq!(h.len(), 32, "Blake2b256 must produce 32-byte digest");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

fn slice_to_hash(slice: &[u8]) -> Option<[u8; 32]> {
    if slice.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(slice);
    Some(out)
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::MerkleProofStep;
    use prost::bytes::Bytes;
    use rholang::rust::interpreter::io::snapshot_chunk::{
        build_merkle_proof, chunk_snapshot, snapshot_merkle_root, CHUNK_SIZE,
    };

    use super::*;

    /// Fixture: build a small multi-chunk snapshot and produce a
    /// well-formed SnapshotChunkResponse for `chunk_index`.
    fn make_target_and_response(
        block_hash: &[u8],
        payload_size: usize,
        chunk_index: u32,
    ) -> (SnapshotTarget, SnapshotChunkResponse) {
        let bytes: Vec<u8> = (0..payload_size).map(|i| (i % 251) as u8).collect();
        let chunks = chunk_snapshot(&bytes);
        let hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
        let merkle_root = snapshot_merkle_root(&hashes);
        let target = SnapshotTarget {
            block_hash: block_hash.to_vec(),
            merkle_root,
            chunk_count: chunks.len() as u32,
        };
        let proof = build_merkle_proof(&hashes, chunk_index).expect("proof");
        let chunk = &chunks[chunk_index as usize];
        let response = SnapshotChunkResponse {
            block_hash: Bytes::copy_from_slice(block_hash),
            chunk_index,
            chunk_bytes: Bytes::copy_from_slice(&chunk.bytes),
            chunk_hash: Bytes::copy_from_slice(&chunk.hash),
            merkle_root: Bytes::copy_from_slice(&merkle_root),
            chunk_count: chunks.len() as u32,
            merkle_proof: proof
                .siblings
                .into_iter()
                .map(|(sibling_hash, is_sibling_right)| MerkleProofStep {
                    sibling_hash: Bytes::copy_from_slice(&sibling_hash),
                    is_sibling_right,
                })
                .collect(),
        };
        (target, response)
    }

    #[tokio::test]
    async fn accepts_valid_chunk_response() {
        let (target, response) = make_target_and_response(&[0x99; 32], CHUNK_SIZE + 100, 0);
        let retriever = SnapshotChunkRetriever::new(target);
        assert_eq!(retriever.pending_count().await, 2);
        let outcome = retriever.admit_response(&response).await;
        assert_eq!(outcome, AdmitOutcome::ChunkAccepted);
        assert_eq!(retriever.pending_count().await, 1);
    }

    #[tokio::test]
    async fn rejects_wrong_block_hash() {
        let (target, mut response) = make_target_and_response(&[0x11; 32], CHUNK_SIZE + 100, 0);
        response.block_hash = Bytes::from_static(&[0x22; 32]);
        let retriever = SnapshotChunkRetriever::new(target);
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::WrongTarget
        );
    }

    #[tokio::test]
    async fn rejects_wrong_merkle_root() {
        let (target, mut response) = make_target_and_response(&[0x33; 32], CHUNK_SIZE + 100, 0);
        response.merkle_root = Bytes::from_static(&[0xEE; 32]);
        let retriever = SnapshotChunkRetriever::new(target);
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::MerkleRootMismatch
        );
    }

    #[tokio::test]
    async fn rejects_tampered_chunk_bytes() {
        let (target, mut response) = make_target_and_response(&[0x44; 32], CHUNK_SIZE + 100, 0);
        // Flip a byte in the payload.  chunk_hash still claims the
        // original, so the self-consistency check catches it.
        let mut b = response.chunk_bytes.to_vec();
        b[0] ^= 0x01;
        response.chunk_bytes = Bytes::from(b);
        let retriever = SnapshotChunkRetriever::new(target);
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::ChunkHashMismatch
        );
    }

    #[tokio::test]
    async fn rejects_bogus_merkle_proof() {
        let (target, mut response) = make_target_and_response(&[0x55; 32], CHUNK_SIZE + 100, 0);
        // Corrupt the first step's sibling hash.
        if let Some(step) = response.merkle_proof.first_mut() {
            step.sibling_hash = Bytes::from_static(&[0xFF; 32]);
        }
        let retriever = SnapshotChunkRetriever::new(target);
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::MerkleProofInvalid
        );
    }

    #[tokio::test]
    async fn rejects_wrong_chunk_count() {
        let (target, mut response) = make_target_and_response(&[0x66; 32], CHUNK_SIZE + 100, 0);
        response.chunk_count += 1;
        let retriever = SnapshotChunkRetriever::new(target);
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::ChunkCountMismatch
        );
    }

    #[tokio::test]
    async fn assemble_returns_none_until_complete() {
        let (target, response) = make_target_and_response(&[0x77; 32], CHUNK_SIZE + 100, 0);
        let retriever = SnapshotChunkRetriever::new(target);
        assert!(retriever.assemble().await.is_none());
        retriever.admit_response(&response).await;
        assert!(retriever.assemble().await.is_none()); // still 1 pending
    }

    #[tokio::test]
    async fn assemble_returns_full_bytes_when_complete() {
        // Two-chunk fixture.  Admit both chunks, then assemble.
        let block_hash = [0x88u8; 32];
        let payload_size = CHUNK_SIZE + 100;
        let bytes: Vec<u8> = (0..payload_size).map(|i| (i % 251) as u8).collect();
        let (target, r0) = make_target_and_response(&block_hash, payload_size, 0);
        let (_, r1) = make_target_and_response(&block_hash, payload_size, 1);
        let retriever = SnapshotChunkRetriever::new(target);
        retriever.admit_response(&r0).await;
        retriever.admit_response(&r1).await;
        assert!(retriever.is_complete().await);
        let assembled = retriever.assemble().await.expect("complete → Some");
        assert_eq!(assembled, bytes);
    }

    #[tokio::test]
    async fn duplicate_response_for_accepted_chunk_is_idempotent() {
        let (target, response) = make_target_and_response(&[0x99; 32], CHUNK_SIZE + 100, 0);
        let retriever = SnapshotChunkRetriever::new(target);
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::ChunkAccepted
        );
        // Second admission of the same response.
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::ChunkAccepted
        );
        assert_eq!(retriever.pending_count().await, 1);
    }

    #[tokio::test]
    async fn record_request_sent_tracks_peers_and_timestamps() {
        let (target, _r) = make_target_and_response(&[0xAA; 32], CHUNK_SIZE + 100, 0);
        let retriever = SnapshotChunkRetriever::new(target);
        retriever.record_request_sent(0, b"peer-alice").await;
        retriever.record_request_sent(0, b"peer-bob").await;
        // Duplicate peer add is a no-op.
        retriever.record_request_sent(0, b"peer-alice").await;
        let g = retriever.chunks.read().await;
        let state = g.get(&0).unwrap();
        assert_eq!(state.peers_tried.len(), 2);
        assert!(state.last_request_ms > 0);
        assert!(state.initial_request_ms > 0);
    }

    #[tokio::test]
    async fn pending_indices_are_sorted_ascending() {
        let (target, r0) = make_target_and_response(&[0xBB; 32], CHUNK_SIZE * 3 + 500, 0);
        assert_eq!(target.chunk_count, 4);
        let retriever = SnapshotChunkRetriever::new(target);
        assert_eq!(retriever.pending_indices().await, vec![0, 1, 2, 3]);
        retriever.admit_response(&r0).await;
        assert_eq!(retriever.pending_indices().await, vec![1, 2, 3]);
    }
}
