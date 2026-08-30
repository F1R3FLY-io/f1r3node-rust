// Phase 7b-2 WalPayloadRetriever (2026-08-27).
//
// Consumer-side counterpart of `WalPayloadResponse`.  Tracks which
// `payload_hash`es a joining validator still needs to reconstruct
// its WAL slice between the latest snapshot and the head block;
// verifies each incoming response's bytes against the requested
// hash (Blake2b256 self-consistency); returns verified bytes ready
// to be applied to the joiner's local filesystem via the WAL
// applier.
//
// Mirrors the shape of `SnapshotChunkRetriever` at
// `casper/src/rust/engine/snapshot_chunk_retriever.rs` but keyed on
// `payload_hash: [u8; 32]` instead of `(block_hash, chunk_index)`.
// There is no anchored Merkle root here — a payload_hash IS its own
// anchor: rehashing the returned bytes and comparing to the
// requested hash is the entire verification.  This makes byzantine
// response detection cheap (one Blake2b256 hash + one 32-byte
// compare).
//
// # Layers of separation
//
// This module deliberately does NOT touch the comm layer directly.
// It exposes:
//
//   * `PayloadRequestState` per pending payload — peers tried, last
//     request timestamp, retry count.
//   * `admit_response(response)` — verification + accept path.
//     Returns `AdmitOutcome` describing what to do next.
//
// The comm-layer glue (peer selection, wire send, timeout ticker)
// lives in the sibling `wal_payload_wire` / `wal_payload_sync`
// modules.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::casper::protocol::casper_message::WalPayloadResponse;
use tokio::sync::RwLock;
use tracing::debug;

/// Per-payload request state.  One entry per `payload_hash` the
/// joiner is trying to fetch.
#[derive(Debug, Clone)]
pub struct PayloadRequestState {
    /// The 32-byte Blake2b256 hash identifying this payload.  Also
    /// serves as the map key.
    pub payload_hash: [u8; 32],
    /// Unix timestamp (ms) of the last outbound request for this
    /// payload.  Zero if never requested (fresh entry).
    pub last_request_ms: u64,
    /// Unix timestamp (ms) of the FIRST outbound request.  Used
    /// for stale-eviction: an entry idle for too long gets
    /// dropped.
    pub initial_request_ms: u64,
    /// Peers we've asked so far.  Used to rotate through peers on
    /// retry rather than re-asking the same node.  Represented as
    /// opaque `Vec<u8>` peer identifiers.
    pub peers_tried: Vec<Vec<u8>>,
    /// Number of retry attempts across peers.  Increments on each
    /// timeout without a valid response.  Retriever gives up after
    /// `MAX_RETRIES` and marks the request Failed.
    pub retry_count: u32,
    /// Verified payload bytes, or None while pending.
    pub bytes: Option<Vec<u8>>,
}

/// Configuration constants.  Tuned for typical joiner scenarios;
/// values mirror SnapshotChunkRetriever's defaults so operators
/// have one set of knobs to tune.
pub const MAX_RETRIES: u32 = 5;
pub const REQUEST_TIMEOUT_MS: u64 = 30_000;
pub const STALE_EVICTION_MS: u64 = 300_000;

/// Security cap: max acceptable size for `payload_bytes` in a
/// `WalPayloadResponse`.  Legit payloads are bounded by write-op
/// semantics: the handler-level `MAX_WRITE_BYTES` / `MAX_READ_BYTES`
/// cap in `rholang::interpreter::io` limits a single fs_write /
/// fs_read reply to 64 MiB.  A payload above that cap is either
/// malformed or a byzantine attempt to force us to hash arbitrary
/// bytes; rejected with `AdmitOutcome::PayloadOversized` before any
/// hashing runs.
///
/// **Locked in sync with the handler-level cap** — a review-fix pin
/// (`max_payload_bytes_matches_handler_write_cap`) asserts equality
/// so future drift shows up as a test failure rather than silent
/// protocol-breaking rejection of legit large payloads.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

/// Outcome of `admit_response`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmitOutcome {
    /// The payload verified and its bytes are now available.
    /// Caller can hand them to the WAL applier.
    PayloadAccepted,
    /// The `payload_hash` has no matching request — the response
    /// is unsolicited or belongs to a different join.  Drop
    /// silently (log at debug).
    UnknownRequest,
    /// The response's `payload_hash` is not a 32-byte Blake2b256
    /// digest.  Malformed peer.
    MalformedPayloadHash,
    /// The response's `payload_bytes` exceeds `MAX_PAYLOAD_BYTES`.
    /// Byzantine peer trying to force us to hash arbitrary bytes.
    PayloadOversized,
    /// `Blake2b256(payload_bytes) != payload_hash`.  Peer's
    /// response is byzantine or malformed.
    PayloadHashMismatch,
}

/// Retriever state.  One instance per joiner covering all
/// between-snapshot payloads simultaneously.  Unlike
/// SnapshotChunkRetriever (per-snapshot), the WAL payload namespace
/// is flat by hash, so a single retriever handles all pending
/// payloads across arbitrarily many WAL slices.
#[derive(Debug, Clone, Default)]
pub struct WalPayloadRetriever {
    /// Per-payload request state, keyed by payload_hash.
    pub payloads: Arc<RwLock<HashMap<[u8; 32], PayloadRequestState>>>,
}

impl WalPayloadRetriever {
    pub fn new() -> Self { Self::default() }

    /// Register a payload_hash we need to fetch.  Idempotent: if
    /// the hash is already tracked, this is a no-op.
    pub async fn enqueue(&self, payload_hash: [u8; 32]) {
        let mut g = self.payloads.write().await;
        g.entry(payload_hash).or_insert(PayloadRequestState {
            payload_hash,
            last_request_ms: 0,
            initial_request_ms: 0,
            peers_tried: Vec::new(),
            retry_count: 0,
            bytes: None,
        });
    }

    /// Phase 7b-2 (2026-08-27) — DD-7b-2 (a) reducer path.
    /// Register a payload_hash as ALREADY RESOLVED with the given
    /// bytes, bypassing the fetch machinery entirely.  Used by the
    /// write-payload-determinism reducer: when the joiner can
    /// locally reproduce the bytes for a WAL entry (e.g., they
    /// come from a deploy argument the joiner already has), the
    /// boot enumerator hands the reproduced bytes here instead of
    /// enqueueing a fetch.
    ///
    /// **Safety check:** rehashes the bytes and refuses to store
    /// them if they don't match `payload_hash`.  The reducer is
    /// trusted code (part of the joiner binary) so a mismatch
    /// indicates a bug in the reducer, not adversary input; we
    /// panic in debug builds and return `false` in release.
    /// Callers that see `false` should treat it as "reducer failed
    /// to reproduce, please fetch from peers" and call `enqueue`.
    ///
    /// Idempotent: if the hash is already resolved (bytes: Some),
    /// this is a no-op returning `true`.
    pub async fn mark_resolved(&self, payload_hash: [u8; 32], bytes: Vec<u8>) -> bool {
        let actual = hash_bytes(&bytes);
        if actual != payload_hash {
            debug_assert!(
                false,
                "reducer produced bytes that don't hash to the requested payload_hash \
                 (requested={} actual={}); this is a reducer bug",
                hex::encode(payload_hash),
                hex::encode(actual),
            );
            debug!(
                target: "f1r3fly.casper.wal_payload_retriever",
                requested = hex::encode(payload_hash),
                actual = hex::encode(actual),
                "mark_resolved rejected reducer output: hash mismatch",
            );
            return false;
        }
        let mut g = self.payloads.write().await;
        let entry = g.entry(payload_hash).or_insert(PayloadRequestState {
            payload_hash,
            last_request_ms: 0,
            initial_request_ms: 0,
            peers_tried: Vec::new(),
            retry_count: 0,
            bytes: None,
        });
        if entry.bytes.is_none() {
            entry.bytes = Some(bytes);
        }
        true
    }

    /// Number of payloads still pending (unverified).
    pub async fn pending_count(&self) -> usize {
        let g = self.payloads.read().await;
        g.values().filter(|c| c.bytes.is_none()).count()
    }

    /// True iff every enqueued payload has been received + verified.
    pub async fn is_complete(&self) -> bool { self.pending_count().await == 0 }

    /// Enumerate payload hashes that still need to be fetched.
    /// Ordered by hash for deterministic peer request patterns.
    pub async fn pending_hashes(&self) -> Vec<[u8; 32]> {
        let g = self.payloads.read().await;
        let mut out: Vec<[u8; 32]> = g
            .values()
            .filter(|c| c.bytes.is_none())
            .map(|c| c.payload_hash)
            .collect();
        out.sort();
        out
    }

    /// Retrieve the verified bytes for a payload hash, if any.
    /// Returns None if the payload is unknown or not yet received.
    pub async fn get_bytes(&self, payload_hash: &[u8; 32]) -> Option<Vec<u8>> {
        let g = self.payloads.read().await;
        g.get(payload_hash).and_then(|s| s.bytes.clone())
    }

    /// Ingest a `WalPayloadResponse`.  Verifies (in order):
    ///   1. payload_bytes.len() <= MAX_PAYLOAD_BYTES (cheap size cap).
    ///   2. payload_hash is 32 bytes (cheap length check).
    ///   3. payload_hash is in the pending set AND not yet resolved
    ///      (cheap map lookup — rejects unsolicited + duplicate
    ///      responses BEFORE any hashing to close a CPU-DoS vector).
    ///   4. Blake2b256(payload_bytes) == payload_hash (expensive).
    ///
    /// On success, stores the verified bytes and returns
    /// `PayloadAccepted`.  On failure, returns the specific
    /// mismatch variant so the caller can log + retry appropriately.
    ///
    /// **Check ordering rationale (review-fix 2026-08-27):** the
    /// prior version hashed BEFORE the pending-set lookup, which
    /// meant unsolicited flood packets each burned a Blake2b256
    /// hash (~200ms for a 64 MiB payload) before rejection.
    /// Because `HasWalPayloadRequest` is broadcast, attackers can
    /// enumerate our pending hashes and craft floods that all pass
    /// the hash-check but land on already-accepted or
    /// never-requested slots.  Reordering pushes the expensive
    /// hash behind the cheap lookup so those floods cost O(1)
    /// each.
    pub async fn admit_response(&self, response: &WalPayloadResponse) -> AdmitOutcome {
        // Cheap: size cap.  Rejected before any allocation-heavy
        // work.
        if response.payload_bytes.len() > MAX_PAYLOAD_BYTES {
            debug!(
                target: "f1r3fly.casper.wal_payload_retriever",
                payload_bytes_len = response.payload_bytes.len(),
                cap = MAX_PAYLOAD_BYTES,
                "payload_bytes exceeds MAX_PAYLOAD_BYTES; rejecting"
            );
            return AdmitOutcome::PayloadOversized;
        }
        // Cheap: hash length.
        let requested_hash = match slice_to_hash(&response.payload_hash) {
            Some(h) => h,
            None => return AdmitOutcome::MalformedPayloadHash,
        };
        // Cheap: pending-set lookup.  Rejects unsolicited AND
        // duplicate responses without hashing.  We take the write
        // lock here so the eventual mutation on success is one
        // acquisition rather than upgrading from read → write.
        {
            let g = self.payloads.read().await;
            match g.get(&requested_hash) {
                Some(state) if state.bytes.is_some() => {
                    // Idempotent duplicate for an accepted payload.
                    debug!(
                        target: "f1r3fly.casper.wal_payload_retriever",
                        "duplicate response for already-accepted payload"
                    );
                    return AdmitOutcome::PayloadAccepted;
                }
                Some(_) => { /* pending — fall through to hash-check */ }
                None => return AdmitOutcome::UnknownRequest,
            }
        }
        // Expensive: verify self-consistency (the hash IS the
        // anchor).  Blake2b256 of the returned bytes must equal
        // the requested hash.
        let hash_of_bytes = hash_bytes(&response.payload_bytes);
        if hash_of_bytes != requested_hash {
            return AdmitOutcome::PayloadHashMismatch;
        }
        // Race-safe accept: re-take the map under write lock and
        // re-check pending state (a concurrent admit_response for
        // the same hash may have won the race between our read
        // above and the hash we just computed).
        let mut g = self.payloads.write().await;
        match g.get_mut(&requested_hash) {
            Some(state) => {
                if state.bytes.is_some() {
                    return AdmitOutcome::PayloadAccepted;
                }
                state.bytes = Some(response.payload_bytes.to_vec());
                AdmitOutcome::PayloadAccepted
            }
            None => AdmitOutcome::UnknownRequest,
        }
    }

    /// Mark a payload request as sent — updates timestamps + peer
    /// tracking.  Called by the outbound-request pipeline.
    pub async fn record_request_sent(&self, payload_hash: &[u8; 32], peer_id: &[u8]) {
        let now = now_ms();
        let mut g = self.payloads.write().await;
        if let Some(state) = g.get_mut(payload_hash) {
            if state.initial_request_ms == 0 {
                state.initial_request_ms = now;
            }
            state.last_request_ms = now;
            if !state.peers_tried.iter().any(|p| p == peer_id) {
                state.peers_tried.push(peer_id.to_vec());
            }
        }
    }

    /// Enumerate payload hashes whose last request has timed out
    /// AND still have retries left.  The caller should re-issue
    /// requests for these to alternative peers.
    pub async fn timed_out_hashes(&self) -> Vec<[u8; 32]> {
        let now = now_ms();
        let g = self.payloads.read().await;
        g.values()
            .filter(|c| {
                c.bytes.is_none()
                    && c.last_request_ms > 0
                    && now.saturating_sub(c.last_request_ms) >= REQUEST_TIMEOUT_MS
                    && c.retry_count < MAX_RETRIES
            })
            .map(|c| c.payload_hash)
            .collect()
    }

    /// Increment the retry count for a payload (called after
    /// requeuing it to a new peer).
    pub async fn record_retry(&self, payload_hash: &[u8; 32]) {
        let mut g = self.payloads.write().await;
        if let Some(state) = g.get_mut(payload_hash) {
            state.retry_count += 1;
        }
    }

    /// Drop payloads whose first-request timestamp is older than
    /// STALE_EVICTION_MS.  Called periodically by the tick driver.
    /// Returns how many were evicted (for metrics).
    pub async fn evict_stale(&self) -> usize {
        let now = now_ms();
        let mut g = self.payloads.write().await;
        let before = g.len();
        g.retain(|_, state| {
            state.bytes.is_some()
                || state.initial_request_ms == 0
                || now.saturating_sub(state.initial_request_ms) < STALE_EVICTION_MS
        });
        before - g.len()
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
    use prost::bytes::Bytes;

    use super::*;

    fn hash(bytes: &[u8]) -> [u8; 32] { hash_bytes(bytes) }

    fn make_response(payload: &[u8]) -> WalPayloadResponse {
        let h = hash(payload);
        WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&h),
            payload_bytes: Bytes::copy_from_slice(payload),
        }
    }

    #[tokio::test]
    async fn accepts_valid_payload_response() {
        let payload = b"hello, wal payload".to_vec();
        let response = make_response(&payload);
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(hash(&payload)).await;
        assert_eq!(retriever.pending_count().await, 1);
        let outcome = retriever.admit_response(&response).await;
        assert_eq!(outcome, AdmitOutcome::PayloadAccepted);
        assert_eq!(retriever.pending_count().await, 0);
        assert!(retriever.is_complete().await);
        let got = retriever.get_bytes(&hash(&payload)).await;
        assert_eq!(got, Some(payload));
    }

    #[tokio::test]
    async fn rejects_tampered_payload_bytes() {
        let payload = b"the truth".to_vec();
        let mut response = make_response(&payload);
        // Corrupt the bytes so the hash no longer matches.
        response.payload_bytes = Bytes::from_static(b"a lie");
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(hash(&payload)).await;
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadHashMismatch
        );
        // Still pending — the failed response did not consume the slot.
        assert_eq!(retriever.pending_count().await, 1);
    }

    #[tokio::test]
    async fn rejects_unsolicited_payload() {
        let payload = b"unsolicited".to_vec();
        let response = make_response(&payload);
        let retriever = WalPayloadRetriever::new();
        // Note: we did NOT enqueue.
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::UnknownRequest
        );
    }

    #[tokio::test]
    async fn rejects_malformed_payload_hash() {
        let mut response = make_response(b"payload");
        // Truncate the hash to 16 bytes.
        response.payload_hash = Bytes::copy_from_slice(&[0u8; 16]);
        let retriever = WalPayloadRetriever::new();
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::MalformedPayloadHash
        );
    }

    /// Security cap: an oversized payload_bytes is rejected BEFORE
    /// we hash anything.  Defends against per-response CPU
    /// exhaustion.
    #[tokio::test]
    async fn rejects_oversized_payload_before_hashing() {
        let bogus_hash = [0u8; 32];
        let response = WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&bogus_hash),
            payload_bytes: Bytes::from(vec![0u8; MAX_PAYLOAD_BYTES + 1]),
        };
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(bogus_hash).await;
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadOversized,
        );
    }

    #[tokio::test]
    async fn duplicate_response_for_accepted_payload_is_idempotent() {
        let payload = b"idempotent".to_vec();
        let response = make_response(&payload);
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(hash(&payload)).await;
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadAccepted
        );
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadAccepted
        );
        assert_eq!(retriever.pending_count().await, 0);
    }

    #[tokio::test]
    async fn enqueue_is_idempotent() {
        let payload = b"e".to_vec();
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(hash(&payload)).await;
        retriever.enqueue(hash(&payload)).await;
        assert_eq!(retriever.pending_count().await, 1);
    }

    #[tokio::test]
    async fn record_request_sent_tracks_peers_and_timestamps() {
        let payload = b"track".to_vec();
        let h = hash(&payload);
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(h).await;
        retriever.record_request_sent(&h, b"peer-alice").await;
        retriever.record_request_sent(&h, b"peer-bob").await;
        retriever.record_request_sent(&h, b"peer-alice").await; // dup
        let g = retriever.payloads.read().await;
        let state = g.get(&h).unwrap();
        assert_eq!(state.peers_tried.len(), 2);
        assert!(state.last_request_ms > 0);
        assert!(state.initial_request_ms > 0);
    }

    #[tokio::test]
    async fn pending_hashes_are_sorted() {
        let retriever = WalPayloadRetriever::new();
        let mut hashes: Vec<[u8; 32]> = (0u8..4).map(|i| hash(&[i; 8])).collect();
        for h in &hashes {
            retriever.enqueue(*h).await;
        }
        hashes.sort();
        assert_eq!(retriever.pending_hashes().await, hashes);
    }

    #[tokio::test]
    async fn evict_stale_drops_old_pending_entries() {
        let payload = b"stale".to_vec();
        let h = hash(&payload);
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(h).await;
        // Fake an initial_request_ms in the distant past.
        {
            let mut g = retriever.payloads.write().await;
            let s = g.get_mut(&h).unwrap();
            s.initial_request_ms = 1; // ~= epoch
            s.last_request_ms = 1;
        }
        let evicted = retriever.evict_stale().await;
        assert_eq!(evicted, 1);
        assert_eq!(retriever.pending_count().await, 0);
    }

    /// T-9: `MAX_PAYLOAD_BYTES` MUST equal the handler-level
    /// `MAX_WRITE_BYTES` / `MAX_READ_BYTES` cap.  If the two drift
    /// out of sync, legit large payloads get rejected as
    /// PayloadOversized and joiners silently fail to catch up on
    /// any block containing a large write (review-fix F-1
    /// 2026-08-27).
    #[test]
    fn max_payload_bytes_matches_handler_write_cap() {
        assert_eq!(
            MAX_PAYLOAD_BYTES as u64,
            rholang::rust::interpreter::io::handlers::MAX_WRITE_BYTES,
            "MAX_PAYLOAD_BYTES must equal handler MAX_WRITE_BYTES",
        );
        assert_eq!(
            MAX_PAYLOAD_BYTES as u64,
            rholang::rust::interpreter::io::MAX_READ_BYTES,
            "MAX_PAYLOAD_BYTES must equal handler MAX_READ_BYTES",
        );
    }

    /// T-11: An empty payload is legit — Blake2b256([]) is a
    /// well-defined 32-byte hash.  Retriever should accept.
    #[tokio::test]
    async fn accepts_empty_payload() {
        let payload: Vec<u8> = Vec::new();
        let h = hash(&payload);
        let response = WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&h),
            payload_bytes: Bytes::copy_from_slice(&payload),
        };
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(h).await;
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadAccepted
        );
        assert_eq!(retriever.get_bytes(&h).await, Some(payload));
    }

    /// F-3 pin: an unsolicited response (hash never enqueued) is
    /// rejected as `UnknownRequest` BEFORE any Blake2b256 work.
    /// The way we exercise "no hashing" is to craft a response
    /// whose payload_hash does NOT match its payload_bytes:
    /// if we DID hash first, the outcome would be
    /// `PayloadHashMismatch` (or `PayloadAccepted` if we happened
    /// to hash-collide); if we correctly check the pending set
    /// first, the outcome is `UnknownRequest` — proving the
    /// hash was never computed.
    #[tokio::test]
    async fn unsolicited_response_rejected_before_hashing() {
        let never_enqueued = [0xABu8; 32];
        let response = WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&never_enqueued),
            // Payload bytes hash to something OTHER than
            // never_enqueued.  If admit_response hashed first, we'd
            // see PayloadHashMismatch.
            payload_bytes: Bytes::from_static(b"totally different bytes"),
        };
        let retriever = WalPayloadRetriever::new();
        // Deliberately do NOT enqueue.
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::UnknownRequest,
            "unsolicited response must be UnknownRequest (not \
             PayloadHashMismatch), confirming no hashing occurred",
        );
    }

    /// F-5 pin: a duplicate response for an already-accepted
    /// payload is rejected as `PayloadAccepted` (idempotent)
    /// BEFORE any Blake2b256 work.  Same shape as
    /// `unsolicited_response_rejected_before_hashing`: if the
    /// hash-check ran first, a duplicate with tampered bytes
    /// would return `PayloadHashMismatch`.  With pending-check
    /// first, the accepted-slot short-circuits and returns
    /// `PayloadAccepted`.
    #[tokio::test]
    async fn duplicate_response_for_accepted_slot_short_circuits_before_hashing() {
        let payload = b"pristine".to_vec();
        let h = hash(&payload);
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(h).await;
        let good = WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&h),
            payload_bytes: Bytes::copy_from_slice(&payload),
        };
        assert_eq!(
            retriever.admit_response(&good).await,
            AdmitOutcome::PayloadAccepted
        );
        // Second admission for the same hash — with tampered bytes.
        // If we hashed first, this would be PayloadHashMismatch.
        let tampered = WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&h),
            payload_bytes: Bytes::from_static(b"tampered"),
        };
        assert_eq!(
            retriever.admit_response(&tampered).await,
            AdmitOutcome::PayloadAccepted,
            "duplicate on an accepted slot short-circuits BEFORE the \
             hash check would fire (F-5 review-fix pin)",
        );
    }

    /// DD-7b-2 (a) pin (2026-08-27): `mark_resolved` with correct
    /// bytes stashes them AND marks the entry complete, matching
    /// the shape a successful `admit_response` would produce.  The
    /// applier can then reach the bytes via `get_bytes`.
    #[tokio::test]
    async fn mark_resolved_accepts_valid_reducer_output() {
        let payload = b"reproduced-locally".to_vec();
        let h = hash_bytes(&payload);
        let retriever = WalPayloadRetriever::new();
        assert!(retriever.mark_resolved(h, payload.clone()).await);
        assert!(retriever.is_complete().await);
        assert_eq!(retriever.get_bytes(&h).await, Some(payload));
    }

    /// DD-7b-2 (a) pin: `mark_resolved` on an already-resolved
    /// hash is idempotent — repeat calls with the same bytes are
    /// silent no-ops that leave the resolved entry intact.
    #[tokio::test]
    async fn mark_resolved_is_idempotent_on_already_resolved() {
        let payload = b"once".to_vec();
        let h = hash_bytes(&payload);
        let retriever = WalPayloadRetriever::new();
        assert!(retriever.mark_resolved(h, payload.clone()).await);
        // Second call also succeeds; state is unchanged.
        assert!(retriever.mark_resolved(h, payload.clone()).await);
        assert_eq!(retriever.get_bytes(&h).await, Some(payload));
    }

    /// DD-7b-2 (a) safety pin: `mark_resolved` with bytes that
    /// don't hash to `payload_hash` returns `false` and does NOT
    /// stash the bytes.  Only compiled in release builds — a debug
    /// build hits the `debug_assert!` and panics (also safe: the
    /// applier should never receive corrupt bytes).
    #[cfg(not(debug_assertions))]
    #[tokio::test]
    async fn mark_resolved_rejects_mismatched_bytes() {
        let real_payload = b"pristine".to_vec();
        let h = hash_bytes(&real_payload);
        let retriever = WalPayloadRetriever::new();
        assert!(!retriever.mark_resolved(h, b"different bytes".to_vec()).await);
        // The entry was created (side-effect of the write) but has
        // no bytes → still pending.
        assert!(!retriever.is_complete().await);
        assert_eq!(retriever.get_bytes(&h).await, None);
    }

    /// DD-7b-2 (a) safety pin: the debug-build `debug_assert!`
    /// fires when a reducer bug produces wrong-hash bytes.  We
    /// verify it aborts by catching the panic — same shape as the
    /// existing `debug_assert!` pins in the codebase.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn mark_resolved_debug_asserts_on_mismatched_bytes() {
        let real_payload = b"pristine".to_vec();
        let h = hash_bytes(&real_payload);
        let retriever = WalPayloadRetriever::new();
        let result = std::panic::AssertUnwindSafe(async move {
            retriever.mark_resolved(h, b"different bytes".to_vec()).await
        });
        // `catch_unwind` around an async block requires poll-based
        // catching — easier to catch on the join.  Run in a
        // spawned task and expect a panic.
        let handle = tokio::spawn(result);
        let outcome = handle.await;
        assert!(
            outcome.is_err(),
            "debug_assert! must panic on wrong-hash bytes in debug builds"
        );
    }
}
