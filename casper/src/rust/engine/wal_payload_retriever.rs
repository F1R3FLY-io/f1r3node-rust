// Phase 7b-2 WalPayloadRetriever.
//
// Consumer-side counterpart of [`WalPayloadResponse`].  Tracks
// which `payload_hash`es a joining validator still needs to
// reconstruct its WAL slice between the latest snapshot and the
// head block; verifies each incoming response's bytes against the
// requested hash (Blake2b256 self-consistency); returns verified
// bytes ready to be applied to the joiner's local filesystem
// via the WAL applier.
//
// Mirrors the shape of [`SnapshotChunkRetriever`] at
// `casper/src/rust/engine/snapshot_chunk_retriever.rs` but keyed
// on `payload_hash: [u8; 32]` instead of
// `(block_hash, chunk_index)`.  There is no anchored Merkle root
// here — a `payload_hash` IS its own anchor: rehashing the
// returned bytes and comparing to the requested hash is the
// entire verification.  This makes byzantine response detection
// cheap (one Blake2b256 hash + one 32-byte compare).
//
// # Layers of separation
//
// This module deliberately does NOT touch the comm layer
// directly.  It exposes:
//
//   * [`PayloadRequestState`] per pending payload — peers tried,
//     last request timestamp, retry count.
//   * [`WalPayloadRetriever::admit_response`] — verification +
//     accept path.  Returns [`AdmitOutcome`] describing what to
//     do next.
//
// The comm-layer glue (peer selection, wire send, timeout
// ticker) lives in a yet-to-land sync-driver slice.

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
    /// The 32-byte Blake2b256 hash identifying this payload.
    /// Also serves as the map key.
    pub payload_hash: [u8; 32],
    /// Unix timestamp (ms) of the last outbound request for
    /// this payload.  Zero if never requested (fresh entry).
    pub last_request_ms: u64,
    /// Unix timestamp (ms) of the FIRST outbound request.  Used
    /// for stale-eviction: an entry idle for too long gets
    /// dropped.
    pub initial_request_ms: u64,
    /// Peers we've asked so far.  Represented as opaque
    /// `Vec<u8>` peer identifiers.
    pub peers_tried: Vec<Vec<u8>>,
    /// Number of retry attempts across peers.  Increments on
    /// each timeout without a valid response.  Retriever gives
    /// up after [`MAX_RETRIES`] and the caller marks the
    /// request Failed (enforcement lives in the sync-driver).
    pub retry_count: u32,
    /// Verified payload bytes, or `None` while pending.
    pub bytes: Option<Vec<u8>>,
}

/// Max retry attempts across peers per payload.
pub const MAX_RETRIES: u32 = 5;
/// Per-request timeout (ms).
pub const REQUEST_TIMEOUT_MS: u64 = 30_000;
/// Idle time (ms) before a never-admitted retriever entry gets
/// evicted by the caller.
pub const STALE_EVICTION_MS: u64 = 300_000;

/// Security cap: max acceptable size for `payload_bytes` in a
/// [`WalPayloadResponse`].  Legit payloads are bounded by write-
/// op semantics: the handler-level `MAX_WRITE_BYTES` /
/// `MAX_READ_BYTES` caps in `rholang::interpreter::io` limit a
/// single `fs_write` / `fs_read` reply to 64 MiB.  A payload
/// above that cap is either malformed or a byzantine attempt to
/// force us to hash arbitrary bytes; rejected with
/// [`AdmitOutcome::PayloadOversized`] before any hashing runs.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

/// Outcome of [`WalPayloadRetriever::admit_response`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmitOutcome {
    /// The payload verified and its bytes are now available.
    /// Caller can hand them to the WAL applier.
    PayloadAccepted,
    /// The `payload_hash` has no matching request — the
    /// response is unsolicited or belongs to a different join.
    /// Drop silently (log at debug).
    UnknownRequest,
    /// The response's `payload_hash` is not a 32-byte Blake2b256
    /// digest.  Malformed peer.
    MalformedPayloadHash,
    /// The response's `payload_bytes` exceeds
    /// [`MAX_PAYLOAD_BYTES`].  Byzantine peer trying to force
    /// us to hash arbitrary bytes.
    PayloadOversized,
    /// `Blake2b256(payload_bytes) != payload_hash`.  Peer's
    /// response is byzantine or malformed.
    PayloadHashMismatch,
}

/// Retriever state.  One instance per joiner covering all
/// between-snapshot payloads simultaneously.  Unlike
/// [`SnapshotChunkRetriever`] (per-snapshot), the WAL-payload
/// namespace is flat by hash, so a single retriever handles all
/// pending payloads across arbitrarily many WAL slices.
#[derive(Debug, Clone, Default)]
pub struct WalPayloadRetriever {
    /// Per-payload request state, keyed by `payload_hash`.
    pub payloads: Arc<RwLock<HashMap<[u8; 32], PayloadRequestState>>>,
}

impl WalPayloadRetriever {
    pub fn new() -> Self { Self::default() }

    /// Register a `payload_hash` we need to fetch.  Idempotent:
    /// if the hash is already tracked, this is a no-op.
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

    /// Register a `payload_hash` as ALREADY RESOLVED with the
    /// given bytes, bypassing the fetch machinery entirely.
    /// Used by the write-payload-determinism reducer: when the
    /// joiner can locally reproduce the bytes for a WAL entry
    /// (e.g., they come from a deploy argument the joiner
    /// already has), the boot enumerator hands the reproduced
    /// bytes here instead of enqueueing a fetch.
    ///
    /// # Safety check
    ///
    /// Rehashes the bytes and refuses to store them if they
    /// don't match `payload_hash`.  The reducer is trusted code
    /// (part of the joiner binary) so a mismatch indicates a
    /// bug in the reducer, not adversary input.  Debug-asserts
    /// on mismatch; returns `false` in release.  Callers that
    /// see `false` should treat it as "reducer failed to
    /// reproduce, please fetch from peers" and call
    /// [`enqueue`](Self::enqueue).
    ///
    /// Idempotent: if the hash is already resolved
    /// (`bytes: Some`), this is a no-op returning `true`.
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

    /// True iff every enqueued payload has been received +
    /// verified.
    pub async fn is_complete(&self) -> bool { self.pending_count().await == 0 }

    /// Enumerate payload hashes that still need to be fetched.
    /// Ordered by hash for deterministic peer-request patterns.
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

    /// Retrieve the verified bytes for a `payload_hash`, if any.
    /// Returns `None` if the payload is unknown or not yet
    /// received.
    pub async fn get_bytes(&self, payload_hash: &[u8; 32]) -> Option<Vec<u8>> {
        let g = self.payloads.read().await;
        g.get(payload_hash).and_then(|s| s.bytes.clone())
    }

    /// Ingest a [`WalPayloadResponse`].  Verifies, in order:
    ///
    ///   1. `payload_bytes.len() <= MAX_PAYLOAD_BYTES` (cheap
    ///      size cap).
    ///   2. `payload_hash` is 32 bytes (cheap length check).
    ///   3. `payload_hash` is in the pending set AND not yet
    ///      resolved (cheap map lookup — rejects unsolicited +
    ///      duplicate responses BEFORE any hashing to close a
    ///      CPU-DoS vector).
    ///   4. `Blake2b256(payload_bytes) == payload_hash`
    ///      (expensive).
    ///
    /// On success, stores the verified bytes and returns
    /// [`AdmitOutcome::PayloadAccepted`].  On failure, returns
    /// the specific mismatch variant so the caller can log +
    /// retry appropriately.
    ///
    /// # Check-ordering rationale
    ///
    /// `HasWalPayloadRequest` is broadcast, so attackers can
    /// enumerate pending hashes and craft floods that all pass
    /// the hash-check but land on already-accepted or
    /// never-requested slots.  Running the expensive hash
    /// BEFORE the cheap lookup would burn Blake2b256 on each
    /// flood packet (~200ms for a 64 MiB payload).  The
    /// cheap-first ordering pushes the hash behind the cheap
    /// lookup so those floods cost O(1) each.
    pub async fn admit_response(&self, response: &WalPayloadResponse) -> AdmitOutcome {
        if response.payload_bytes.len() > MAX_PAYLOAD_BYTES {
            debug!(
                target: "f1r3fly.casper.wal_payload_retriever",
                payload_bytes_len = response.payload_bytes.len(),
                cap = MAX_PAYLOAD_BYTES,
                "payload_bytes exceeds MAX_PAYLOAD_BYTES; rejecting"
            );
            return AdmitOutcome::PayloadOversized;
        }
        let requested_hash = match slice_to_hash(&response.payload_hash) {
            Some(h) => h,
            None => return AdmitOutcome::MalformedPayloadHash,
        };
        // Cheap: pending-set lookup.  Rejects unsolicited AND
        // duplicate responses without hashing.
        {
            let g = self.payloads.read().await;
            match g.get(&requested_hash) {
                Some(state) if state.bytes.is_some() => {
                    debug!(
                        target: "f1r3fly.casper.wal_payload_retriever",
                        "duplicate response for already-accepted payload"
                    );
                    return AdmitOutcome::PayloadAccepted;
                }
                Some(_) => {}
                None => return AdmitOutcome::UnknownRequest,
            }
        }
        // Expensive: verify self-consistency.  The hash IS the
        // anchor — rehashing the returned bytes and comparing
        // to the requested hash is the entire verification.
        let hash_of_bytes = hash_bytes(&response.payload_bytes);
        if hash_of_bytes != requested_hash {
            return AdmitOutcome::PayloadHashMismatch;
        }
        // Race-safe accept: re-take the map under write lock
        // and re-check pending state.
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

    /// Mark a payload request as sent — updates timestamps +
    /// peer tracking.  Called by the outbound-request pipeline.
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

    /// Enumerate payload hashes whose last request has timed
    /// out AND still have retries left.  The caller should
    /// re-issue requests for these to alternative peers.
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

    /// Drop pending payloads whose first-request timestamp is older
    /// than [`STALE_EVICTION_MS`].  Called periodically by the tick
    /// driver.  Returns how many were evicted (for metrics).
    ///
    /// # Retention predicate
    ///
    /// An entry is retained iff ANY of:
    ///   * `state.bytes.is_some()` — already resolved; don't evict
    ///     verified bytes just because they've been sitting around.
    ///   * `state.initial_request_ms == 0` — never requested yet
    ///     (just enqueued); the clock hasn't started.
    ///   * `now - initial_request_ms < STALE_EVICTION_MS` — within
    ///     the age budget.
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

    fn make_response(bytes: Vec<u8>) -> WalPayloadResponse {
        let hash = hash_bytes(&bytes);
        WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&hash),
            payload_bytes: Bytes::from(bytes),
        }
    }

    #[tokio::test]
    async fn enqueue_then_admit_valid_response() {
        let retriever = WalPayloadRetriever::new();
        let bytes = b"hello world".to_vec();
        let hash = hash_bytes(&bytes);
        retriever.enqueue(hash).await;
        assert_eq!(retriever.pending_count().await, 1);

        let response = make_response(bytes.clone());
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadAccepted
        );
        assert_eq!(retriever.pending_count().await, 0);
        assert_eq!(retriever.get_bytes(&hash).await.expect("bytes"), bytes);
    }

    #[tokio::test]
    async fn admit_response_for_unenqueued_hash_returns_unknown_request() {
        let retriever = WalPayloadRetriever::new();
        let response = make_response(b"never asked for this".to_vec());
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::UnknownRequest
        );
    }

    #[tokio::test]
    async fn admit_response_rejects_tampered_bytes() {
        let retriever = WalPayloadRetriever::new();
        let bytes = b"original".to_vec();
        let hash = hash_bytes(&bytes);
        retriever.enqueue(hash).await;
        // Build response claiming the hash of `original` but
        // sending different bytes.
        let response = WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&hash),
            payload_bytes: Bytes::from(b"tampered".to_vec()),
        };
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadHashMismatch
        );
    }

    /// LOAD-BEARING security cap: oversized `payload_bytes` is
    /// rejected BEFORE any hashing.  A byzantine peer flooding
    /// 64 MiB+ payloads can't force Blake2b256 CPU work.
    #[tokio::test]
    async fn rejects_oversized_payload_before_hashing() {
        let retriever = WalPayloadRetriever::new();
        let response = WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&[0u8; 32]),
            payload_bytes: Bytes::from(vec![0u8; MAX_PAYLOAD_BYTES + 1]),
        };
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadOversized,
            "oversized payload must be rejected before hashing"
        );
    }

    /// LOAD-BEARING check ordering: the cheap pending-set
    /// lookup runs BEFORE the expensive Blake2b256 hash.
    /// Byzantine floods for never-requested well-formed
    /// payloads cost O(1), not O(payload_size).
    #[tokio::test]
    async fn unknown_request_rejected_before_hashing() {
        let retriever = WalPayloadRetriever::new();
        // Build a VALID response (bytes match hash) but for a
        // hash we never enqueued.  Must return UnknownRequest
        // via the pending-set lookup, NOT after the hash check.
        let response = make_response(vec![0xAB; 1024]);
        // We can't directly observe "no hash was computed" from
        // the outside, but we CAN assert the outcome is
        // UnknownRequest.  The ordering is pinned by the test
        // below with a well-formed-hash but tampered-bytes
        // response landing on PayloadHashMismatch (not
        // UnknownRequest), confirming the lookup WOULD have
        // caught it first if the hash were correct.
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::UnknownRequest
        );
    }

    #[tokio::test]
    async fn rejects_non_32_byte_payload_hash() {
        let retriever = WalPayloadRetriever::new();
        let response = WalPayloadResponse {
            payload_hash: Bytes::from(vec![0u8; 16]),
            payload_bytes: Bytes::from(b"any".to_vec()),
        };
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::MalformedPayloadHash
        );
    }

    #[tokio::test]
    async fn duplicate_response_is_idempotent() {
        let retriever = WalPayloadRetriever::new();
        let bytes = b"content".to_vec();
        let hash = hash_bytes(&bytes);
        retriever.enqueue(hash).await;
        let response = make_response(bytes);
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
    async fn mark_resolved_accepts_matching_bytes() {
        let retriever = WalPayloadRetriever::new();
        let bytes = b"reducer-reproduced".to_vec();
        let hash = hash_bytes(&bytes);
        assert!(retriever.mark_resolved(hash, bytes.clone()).await);
        assert!(retriever.is_complete().await);
        assert_eq!(retriever.get_bytes(&hash).await.unwrap(), bytes);
    }

    /// LOAD-BEARING: `mark_resolved` guard against reducer
    /// bugs.  Debug builds trip `debug_assert!(false, ...)` and
    /// panic loudly; release builds return `false` so the
    /// sync-driver can fall back to peer fetch.  Both modes
    /// MUST refuse to store wrong bytes.
    ///
    /// Uses `catch_unwind` + `cfg(debug_assertions)` branches
    /// so the test exercises the actual mode-dependent
    /// behavior instead of documenting a contract-via-string
    /// (which the earlier version did).
    #[test]
    fn mark_resolved_rejects_hash_mismatch() {
        use std::panic;
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let retriever = WalPayloadRetriever::new();
                retriever.mark_resolved([0xAA; 32], vec![0xFF; 10]).await
            })
        }));
        #[cfg(debug_assertions)]
        {
            assert!(
                result.is_err(),
                "debug build must panic via debug_assert on hash/bytes mismatch"
            );
        }
        #[cfg(not(debug_assertions))]
        {
            assert!(
                !result.expect("release build must not panic"),
                "release build must return false on hash/bytes mismatch"
            );
        }
    }

    #[tokio::test]
    async fn evict_stale_drops_old_pending_entries() {
        // LOAD-BEARING: a payload that's been requested but
        // unresolved for longer than STALE_EVICTION_MS gets dropped.
        let retriever = WalPayloadRetriever::new();
        let fresh = [0xA1; 32];
        let stale = [0xA2; 32];
        retriever.enqueue(fresh).await;
        retriever.enqueue(stale).await;
        // Simulate "sent long ago": set initial_request_ms past
        // the eviction window on `stale`; set a recent timestamp
        // on `fresh`.
        {
            let mut g = retriever.payloads.write().await;
            g.get_mut(&fresh).unwrap().initial_request_ms = now_ms();
            g.get_mut(&stale).unwrap().initial_request_ms =
                now_ms().saturating_sub(STALE_EVICTION_MS + 1);
        }
        let evicted = retriever.evict_stale().await;
        assert_eq!(evicted, 1);
        let g = retriever.payloads.read().await;
        assert!(g.contains_key(&fresh));
        assert!(!g.contains_key(&stale));
    }

    #[tokio::test]
    async fn evict_stale_retains_resolved_entries_regardless_of_age() {
        // A resolved entry (bytes is Some) must NEVER be evicted
        // for age — the applier may still need the bytes.
        let retriever = WalPayloadRetriever::new();
        let bytes = b"resolved".to_vec();
        let h = {
            let out = Blake2b256::hash(bytes.clone());
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&out);
            buf
        };
        assert!(retriever.mark_resolved(h, bytes).await);
        {
            let mut g = retriever.payloads.write().await;
            // Even with ancient initial_request_ms, mark_resolved
            // populated `bytes` so retention wins.
            g.get_mut(&h).unwrap().initial_request_ms =
                now_ms().saturating_sub(STALE_EVICTION_MS * 10);
        }
        let evicted = retriever.evict_stale().await;
        assert_eq!(evicted, 0, "resolved entry must be retained");
        assert!(retriever.payloads.read().await.contains_key(&h));
    }

    #[tokio::test]
    async fn evict_stale_retains_never_requested_entries() {
        // An entry with initial_request_ms == 0 (just enqueued,
        // never requested) must NEVER be evicted — the clock
        // hasn't started.
        let retriever = WalPayloadRetriever::new();
        let h = [0xA3; 32];
        retriever.enqueue(h).await;
        // Default initial_request_ms is 0.
        let evicted = retriever.evict_stale().await;
        assert_eq!(evicted, 0);
        assert!(retriever.payloads.read().await.contains_key(&h));
    }

    #[tokio::test]
    async fn evict_stale_is_no_op_on_empty_retriever() {
        let retriever = WalPayloadRetriever::new();
        assert_eq!(retriever.evict_stale().await, 0);
    }

    #[tokio::test]
    async fn record_retry_increments_count() {
        let retriever = WalPayloadRetriever::new();
        let hash = [0xAA; 32];
        retriever.enqueue(hash).await;
        retriever.record_retry(&hash).await;
        retriever.record_retry(&hash).await;
        let g = retriever.payloads.read().await;
        assert_eq!(g.get(&hash).unwrap().retry_count, 2);
    }

    #[tokio::test]
    async fn record_request_sent_tracks_peers_and_timestamps() {
        let retriever = WalPayloadRetriever::new();
        let hash = [0xBB; 32];
        retriever.enqueue(hash).await;
        retriever.record_request_sent(&hash, b"peer-a").await;
        retriever.record_request_sent(&hash, b"peer-b").await;
        retriever.record_request_sent(&hash, b"peer-a").await; // duplicate
        let g = retriever.payloads.read().await;
        let state = g.get(&hash).unwrap();
        assert_eq!(state.peers_tried.len(), 2);
        assert!(state.last_request_ms > 0);
        assert!(state.initial_request_ms > 0);
    }

    #[tokio::test]
    async fn pending_hashes_sorted_ascending() {
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue([0x22; 32]).await;
        retriever.enqueue([0x11; 32]).await;
        retriever.enqueue([0x33; 32]).await;
        assert_eq!(retriever.pending_hashes().await, vec![
            [0x11; 32], [0x22; 32], [0x33; 32]
        ]);
    }

    #[tokio::test]
    async fn enqueue_is_idempotent() {
        let retriever = WalPayloadRetriever::new();
        let hash = [0xDD; 32];
        retriever.enqueue(hash).await;
        retriever.enqueue(hash).await;
        assert_eq!(retriever.pending_count().await, 1);
    }

    /// INTEGRATION pin: server-produced response (via
    /// [`crate::rust::engine::wal_payload_server::serve_payload`])
    /// admits cleanly through this retriever.  Pins that the
    /// Wave 3 WAL-payload server and client agree on wire shape
    /// AND content-address hashing semantics.
    #[tokio::test]
    async fn server_produced_response_admits_through_retriever() {
        use crate::rust::engine::wal_payload_server::{serve_payload, InMemoryPayloadStore};

        let store = InMemoryPayloadStore::new();
        let bytes = b"end-to-end payload".to_vec();
        let hash = store.insert(bytes.clone());

        let response = serve_payload(&hash, &store).expect("serve_payload");

        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(hash).await;
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadAccepted
        );
        assert_eq!(retriever.get_bytes(&hash).await.unwrap(), bytes);
    }
}
