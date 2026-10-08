// Phase 7b-2 WAL payload sync driver (joiner-side).
//
// Seed module for the eventual full `WalPayloadSyncDriver` port.
// This slice lands only the graceful-stop handle used by the
// future tick-loop; the driver + spawn_periodic_tick + enumeration
// pipeline land in follow-up slices.
//
// The handle lives in a dedicated module (rather than inside
// `wal_payload_server.rs`) because its ownership shape is
// joiner-side: it's created by `spawn_periodic_tick` and raised by
// the block-processing catch-up path on the joiner, never touched
// by the serving validator.  Keeping it adjacent to the eventual
// `WalPayloadSyncDriver` reduces churn when the driver lands.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use comm::rust::peer_node::PeerNode;
use models::rust::casper::protocol::casper_message::{HasWalPayload, WalPayloadResponse};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::rust::engine::wal_payload_retriever::{AdmitOutcome, WalPayloadRetriever};

/// Security cap on the per-payload source set — number of distinct
/// peers that can advertise they can serve a single payload hash.
/// Typical joiner sees ~10 sources; the cap defends against an
/// attacker rotating peer identities to exhaust memory.
pub const MAX_SOURCES: usize = 256;

/// Security cap on the global per-driver blacklist.  Typical
/// blacklist stays near zero.  Cap paired with [`BLACKLIST_TTL_MS`]
/// so sustained attacker churn eventually trips the cap and new
/// bad peers get silently ignored rather than evicting good ones.
pub const MAX_BLACKLISTED: usize = 1024;

/// Time-to-live for a peer's blacklist entry.  A byzantine burst
/// followed by an hour of good behavior lets a peer re-enter the
/// candidate pool.  Not tuned yet — pick a conservative default;
/// operators can revisit if telemetry shows churn.
pub const BLACKLIST_TTL_MS: u64 = 60 * 60 * 1000;

/// Default tick period between outbound-request rounds for the
/// (future) `spawn_periodic_tick` loop.  Matches
/// `snapshot_chunk_sync::TICK_PERIOD_MS` so operators have one
/// knob covering both sync families.
pub const TICK_PERIOD_MS: u64 = 5_000;

/// Per-payload source tracking.  Records which peers advertised
/// they can serve each payload hash (from `HasWalPayload` replies).
#[allow(dead_code)]
#[derive(Debug, Default)]
pub(crate) struct PayloadSources {
    /// Peers known to have this payload.  FIFO for round-robin.
    pub(crate) sources: VecDeque<PeerNode>,
    /// Whether we've broadcast `HasWalPayloadRequest` yet.  Avoids
    /// duplicate broadcasts on repeat ticks.
    pub(crate) broadcasted_has_request: bool,
}

/// Per-hash tick decision.  Isolates the three-way branch in the
/// (future) `tick`'s send-or-skip path.
#[allow(dead_code)]
pub(crate) enum TickAction {
    /// Send a fresh `GetWalPayloadRequest` for this hash.
    SendFresh,
    /// An outstanding request is in flight and the retry budget
    /// has not been exhausted; wait for a response or a timeout.
    WaitInFlight,
    /// Retry budget exhausted; stop sending.  The entry stays in
    /// the retriever until stale-eviction drops it.
    GiveUp,
}

/// Insert into the blacklist with a size cap and a timestamp.
/// Silent no-op past [`MAX_BLACKLISTED`].  Timestamp lets tick
/// eviction drop entries older than [`BLACKLIST_TTL_MS`] so a
/// peer that misfires once doesn't get killed forever.
#[allow(dead_code)]
pub(crate) fn add_blacklist_capped(map: &mut HashMap<PeerNode, u64>, peer: PeerNode, now_ms: u64) {
    if map.len() < MAX_BLACKLISTED {
        map.insert(peer, now_ms);
    }
}

/// The joiner-side driver.  Owns a single [`WalPayloadRetriever`]
/// + a per-hash source map + a global blacklist.  WAL payload
/// namespace is FLAT (hash-addressed), not per-snapshot, so there's
/// a single driver (vs. snapshot-chunk sync's per-snapshot shape).
///
/// This slice lands the struct + the forwarder API
/// (`new`, `enqueue_payload`, `pending_count`, `is_complete`,
/// `take_bytes`).  The wire-protocol methods (`on_has_wal_payload`,
/// `on_payload_response`, `tick`) land in follow-up slices.
#[derive(Debug, Clone)]
pub struct WalPayloadSyncDriver {
    /// Verifies + stores payloads.  Shared with the incoming
    /// message dispatch path.
    pub retriever: Arc<WalPayloadRetriever>,
    /// Per-payload_hash source lists (peers that advertised
    /// they can serve it).
    #[allow(dead_code)]
    per_hash_sources: Arc<RwLock<HashMap<[u8; 32], PayloadSources>>>,
    /// Peers globally blacklisted after producing a byzantine
    /// response.  Once a peer is blacklisted it's skipped for ALL
    /// payloads; this is a stronger stance than snapshot-chunk
    /// blacklisting because a peer producing bad bytes for ONE
    /// hash is very likely to produce bad bytes for others (or is
    /// otherwise adversarial).
    ///
    /// Value is the Unix-ms timestamp at which the peer was
    /// blacklisted.  Entries older than [`BLACKLIST_TTL_MS`] are
    /// evicted at the next tick — a byzantine burst followed by
    /// good behavior lets a peer re-enter the candidate pool.
    #[allow(dead_code)]
    blacklisted: Arc<RwLock<HashMap<PeerNode, u64>>>,
}

impl WalPayloadSyncDriver {
    pub fn new(retriever: Arc<WalPayloadRetriever>) -> Self {
        Self {
            retriever,
            per_hash_sources: Arc::new(RwLock::new(HashMap::new())),
            blacklisted: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a payload_hash we need to fetch.  Idempotent —
    /// repeated calls with the same hash leave the retriever +
    /// source map in the same state.
    pub async fn enqueue_payload(&self, payload_hash: [u8; 32]) {
        self.retriever.enqueue(payload_hash).await;
        let mut g = self.per_hash_sources.write().await;
        g.entry(payload_hash).or_default();
    }

    /// How many payload hashes are still outstanding on the
    /// retriever.  Forwarder for `WalPayloadRetriever::pending_count`.
    pub async fn pending_count(&self) -> usize { self.retriever.pending_count().await }

    /// Query: is all pending work complete?  Forwarder for
    /// `WalPayloadRetriever::is_complete`.
    pub async fn is_complete(&self) -> bool { self.retriever.is_complete().await }

    /// Retrieve verified bytes for a hash, if resolved.  Forwarder
    /// for `WalPayloadRetriever::get_bytes` (not a take — the
    /// retriever's current API retains the bytes; this method
    /// returns a clone).
    pub async fn take_bytes(&self, payload_hash: &[u8; 32]) -> Option<Vec<u8>> {
        self.retriever.get_bytes(payload_hash).await
    }

    /// Called when a `HasWalPayload` reply arrives.  Records the
    /// sender as a source for `announcement.payload_hash`, subject
    /// to:
    ///
    ///   * `payload_hash` must be exactly 32 bytes — a wrong-length
    ///     field is treated as a byzantine send and blacklists the
    ///     sender immediately.
    ///   * active-blacklist senders are silently ignored (expired
    ///     TTL entries are treated as unblacklisted; the next tick
    ///     evicts them).
    ///   * announcements for payloads we did not enqueue are
    ///     logged at debug + dropped (the sender may be out of
    ///     sync with our view of what we need).
    ///   * duplicate inserts are skipped (`sources.iter().any(...)`
    ///     dedup).
    ///   * `MAX_SOURCES` cap defends against identity-rotation
    ///     memory exhaustion — past the cap, further senders are
    ///     silently dropped.
    pub async fn on_has_wal_payload(&self, sender: PeerNode, announcement: &HasWalPayload) {
        let hash = match slice_to_hash(announcement.payload_hash.as_ref()) {
            Some(h) => h,
            None => {
                warn!(
                    target: "f1r3fly.casper.wal_payload_sync",
                    len = announcement.payload_hash.len(),
                    "HasWalPayload payload_hash has wrong length; blacklisting sender"
                );
                let mut b = self.blacklisted.write().await;
                add_blacklist_capped(&mut b, sender, now_ms());
                return;
            }
        };
        {
            let b = self.blacklisted.read().await;
            if let Some(ts) = b.get(&sender) {
                if now_ms().saturating_sub(*ts) < BLACKLIST_TTL_MS {
                    return;
                }
            }
        }
        let mut g = self.per_hash_sources.write().await;
        let sources = match g.get_mut(&hash) {
            Some(s) => s,
            None => {
                debug!(
                    target: "f1r3fly.casper.wal_payload_sync",
                    "HasWalPayload for un-enqueued payload; ignoring"
                );
                return;
            }
        };
        if !sources.sources.iter().any(|p| p == &sender) && sources.sources.len() < MAX_SOURCES {
            sources.sources.push_back(sender);
        }
    }

    /// Dispatch an incoming [`WalPayloadResponse`] to the
    /// retriever.  Returns `true` iff the retriever accepted the
    /// response (payload bytes verified and stored).
    ///
    /// Byzantine outcomes (hash-mismatch, oversized, malformed
    /// payload hash) blacklist the sender via
    /// [`add_blacklist_capped`].  `UnknownRequest` (unsolicited
    /// response) is logged at debug + dropped — benign; a peer may
    /// have raced an eviction on our side.
    pub async fn on_payload_response(
        &self,
        sender: PeerNode,
        response: &WalPayloadResponse,
    ) -> bool {
        let outcome = self.retriever.admit_response(response).await;
        match outcome {
            AdmitOutcome::PayloadAccepted => true,
            AdmitOutcome::UnknownRequest => {
                debug!(
                    target: "f1r3fly.casper.wal_payload_sync",
                    "unsolicited WalPayloadResponse; dropping"
                );
                false
            }
            AdmitOutcome::PayloadHashMismatch
            | AdmitOutcome::PayloadOversized
            | AdmitOutcome::MalformedPayloadHash => {
                warn!(
                    target: "f1r3fly.casper.wal_payload_sync",
                    outcome = ?outcome,
                    "byzantine response; blacklisting sender"
                );
                let mut b = self.blacklisted.write().await;
                add_blacklist_capped(&mut b, sender, now_ms());
                false
            }
        }
    }

    /// Pick the next non-blacklisted source for a payload hash,
    /// rotating the FIFO one position (pop_front + push_back on
    /// every candidate).  Returns `None` if no eligible source is
    /// available.  TTL-expired blacklist entries are treated as
    /// unblacklisted — they'll be evicted by the next
    /// [`evict_expired_blacklist`](Self::evict_expired_blacklist)
    /// pass.
    #[allow(dead_code)]
    pub(crate) async fn next_source_for(&self, hash: &[u8; 32]) -> Option<PeerNode> {
        let now = now_ms();
        let blacklist_snap: HashMap<PeerNode, u64> = self.blacklisted.read().await.clone();
        let mut g = self.per_hash_sources.write().await;
        let sources = g.get_mut(hash)?;
        let n = sources.sources.len();
        for _ in 0..n {
            let peer = sources.sources.pop_front()?;
            sources.sources.push_back(peer.clone());
            let is_active_blacklist = blacklist_snap
                .get(&peer)
                .map(|ts| now.saturating_sub(*ts) < BLACKLIST_TTL_MS)
                .unwrap_or(false);
            if !is_active_blacklist {
                return Some(peer);
            }
        }
        None
    }

    /// Evict blacklist entries whose TTL has expired.  Called
    /// once per tick from the (future) driver tick loop.  Returns
    /// the number evicted (for metrics / testing).
    pub async fn evict_expired_blacklist(&self) -> usize {
        let now = now_ms();
        let mut b = self.blacklisted.write().await;
        let before = b.len();
        b.retain(|_, ts| now.saturating_sub(*ts) < BLACKLIST_TTL_MS);
        before - b.len()
    }
}

/// Narrow a byte slice to a 32-byte payload hash, or `None` if
/// the slice is any other length.  Mirrors the private helper of
/// the same name in `wal_payload_retriever` / `wal_payload_server`
/// / `snapshot_chunk_retriever`; each module keeps its own copy to
/// avoid a cross-module public surface for what is really a 1-line
/// hash-length guard.
fn slice_to_hash(slice: &[u8]) -> Option<[u8; 32]> {
    if slice.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(slice);
    Some(out)
}

/// Milliseconds since Unix epoch.  Private helper used by the
/// driver's blacklist-TTL logic.  `duration_since(UNIX_EPOCH)`
/// cannot error outside of pre-epoch system clocks; the
/// `unwrap_or(Duration::ZERO)` is defense-in-depth that treats
/// such a reading as "0 ms since epoch" (which simply retains all
/// blacklist entries — safer than panicking).
fn now_ms() -> u64 {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

/// Graceful-stop handle for the (future) periodic tick task.  Held
/// by the block-processing catch-up path on the joiner; `.stop()`
/// raises a `tokio::sync::Notify` that the tick loop selects on,
/// causing it to exit cleanly at its next select boundary rather
/// than being hard-aborted mid-`.await`.
///
/// `.stop()` is idempotent — tokio's `Notify` collapses multiple
/// pending notifications into a single permit, so a second call
/// before the loop has consumed the first is a no-op.
///
/// `Clone` is deliberate: the catch-up path and the shutdown
/// orchestrator both need to hold a handle, and the shared
/// `Arc<Notify>` makes multiple-waker semantics free.
#[derive(Clone)]
pub struct WalPayloadTickStop {
    signal: Arc<tokio::sync::Notify>,
}

impl WalPayloadTickStop {
    /// Construct a fresh stop handle.  The eventual
    /// `spawn_periodic_tick` constructs one of these and hands
    /// both ends out — one to its internal loop (via
    /// `signal.notified().await` in the `select!`) and one to the
    /// returned `WalPayloadTickHandle::stop`.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            signal: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Raise the stop signal.  The tick loop selecting on this
    /// signal will exit at its next select boundary (immediately
    /// if idle, or after the current in-flight `driver.tick(...)`
    /// completes).  Idempotent per the type-level docstring.
    pub fn stop(&self) { self.signal.notify_one(); }

    /// Expose the underlying `Arc<Notify>` to the (future) tick
    /// loop so it can `.notified().await` on the same signal
    /// `.stop()` raises.  Private within the engine subtree —
    /// external callers go through `.stop()`.
    #[allow(dead_code)]
    pub(crate) fn signal(&self) -> &Arc<tokio::sync::Notify> { &self.signal }
}

impl Default for WalPayloadTickStop {
    fn default() -> Self { Self::new() }
}

impl std::fmt::Debug for WalPayloadTickStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WalPayloadTickStop")
            .field("signal", &"tokio::sync::Notify")
            .finish()
    }
}

/// Returned by the (future) `spawn_periodic_tick`.  Carries both
/// the tick task's `JoinHandle` (so the caller can `abort()` on
/// shutdown) and a [`WalPayloadTickStop`] handle (so the block-
/// processing catch-up path can raise the graceful-stop signal).
///
/// Fields are `pub` so `spawn_periodic_tick` (follow-up slice)
/// constructs via struct-literal; no accessor churn needed when
/// the spawn site lands.
#[allow(dead_code)]
pub struct WalPayloadTickHandle {
    pub join_handle: tokio::task::JoinHandle<()>,
    pub stop: WalPayloadTickStop,
}

/// Counters produced by the (future) enumerator — how many of the
/// unique payload hashes extracted from a WAL slice the local
/// reducer resolved vs. how many got enqueued for peer fetch.
/// Telemetry carries these so operators can distinguish "reducer
/// worked, no wire traffic needed" from "we're depending on peers
/// to serve every byte."
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnumerateStats {
    /// Payload hashes handed to the retriever with reproduced
    /// bytes (via `mark_resolved`).  Fetch protocol will NOT
    /// contact peers for these.
    pub resolved_locally: usize,
    /// Payload hashes enqueued for peer fetch (via
    /// `enqueue_payload`) — either because the reducer returned
    /// `None` or because its reproduced bytes failed the hash
    /// check.
    pub enqueued_for_fetch: usize,
}

/// Report from the (future) `apply_wal_slice_after_fetch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootApplyReport {
    pub enumerated: EnumerateStats,
    /// Number of unique payload hashes populated in the sidecar
    /// (equal to `enumerated.resolved_locally +
    /// enumerated.enqueued_for_fetch` on the happy path; less if
    /// peers failed to serve some).
    pub sidecar_populated: usize,
    /// Number of WAL entries in the applied slice (informational —
    /// includes observation-only variants the applier skips).
    pub wal_entries: usize,
}

/// Reasons the (future) `apply_wal_slice_after_fetch` can fail.
/// Callers pattern-match to distinguish "byzantine input, log +
/// skip" from "genuine peer/network shortfall, retry later".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootApplyError {
    /// `driver.is_complete()` did not return true within the
    /// configured timeout.  `pending_count` is the number of
    /// hashes still outstanding when the timeout fired.  Callers
    /// may re-issue the flow after peer conditions improve.
    PayloadFetchTimeout { pending_count: usize },
    /// A hash the enumerator populated is missing from the driver
    /// by the time `take_bytes` ran — indicates the driver dropped
    /// a resolved entry between `is_complete()` and the sidecar
    /// build (stale-eviction races with the poll loop, or a
    /// `driver.stop()` called mid-collect).
    MissingResolvedHash { hash_hex: String },
    /// The applier returned an `ApplierError` variant (missing
    /// sidecar / unsupported PayloadRef / out-of-allowed-roots
    /// path / NSS failure / IO error / etc).  Byzantine or
    /// misconfigured input; the subscriber logs + continues to
    /// the next snapshot.
    ApplierFailed { message: String },
    /// The `spawn_blocking` task carrying the applier panicked.
    /// Defense-in-depth variant: today's applier is Result-based
    /// with no panic paths of its own, but a future refactor that
    /// reintroduces a panic will surface here rather than killing
    /// the subscriber loop.
    ApplierPanic { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stop_wakes_a_waiter_on_the_shared_signal() {
        let handle = WalPayloadTickStop::new();
        let signal = handle.signal().clone();
        let waiter = tokio::spawn(async move {
            signal.notified().await;
        });
        handle.stop();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter did not resume within 1s")
            .expect("waiter task panicked");
    }

    #[tokio::test]
    async fn stop_is_idempotent_when_pending() {
        // Two calls to .stop() before the waiter consumes the
        // permit collapse into one — the first .notified().await
        // resumes; a second .notified().await then re-blocks (no
        // orphaned permit).
        let handle = WalPayloadTickStop::new();
        handle.stop();
        handle.stop();
        let signal = handle.signal().clone();
        // First wait: consumes the single permit.
        tokio::time::timeout(std::time::Duration::from_millis(50), signal.notified())
            .await
            .expect("first notified() did not resume");
        // Second wait: no orphan permit, must time out.
        let second =
            tokio::time::timeout(std::time::Duration::from_millis(50), signal.notified()).await;
        assert!(second.is_err(), "idempotent stop leaked a second permit");
    }

    #[test]
    fn clone_shares_the_signal() {
        let a = WalPayloadTickStop::new();
        let b = a.clone();
        assert!(
            Arc::ptr_eq(a.signal(), b.signal()),
            "Clone must share the underlying Arc<Notify>"
        );
    }

    #[test]
    fn debug_impl_describes_shape() {
        let handle = WalPayloadTickStop::new();
        let formatted = format!("{handle:?}");
        assert!(formatted.contains("WalPayloadTickStop"));
        assert!(formatted.contains("tokio::sync::Notify"));
    }

    #[test]
    fn enumerate_stats_default_is_zero() {
        let stats = EnumerateStats::default();
        assert_eq!(stats.resolved_locally, 0);
        assert_eq!(stats.enqueued_for_fetch, 0);
    }

    #[test]
    fn enumerate_stats_eq_and_copy() {
        let a = EnumerateStats {
            resolved_locally: 3,
            enqueued_for_fetch: 7,
        };
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn boot_apply_report_builds_and_compares() {
        let a = BootApplyReport {
            enumerated: EnumerateStats {
                resolved_locally: 5,
                enqueued_for_fetch: 2,
            },
            sidecar_populated: 7,
            wal_entries: 12,
        };
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(
            a.sidecar_populated,
            a.enumerated.resolved_locally + a.enumerated.enqueued_for_fetch,
            "happy-path invariant: sidecar == resolved + enqueued"
        );
    }

    fn mk_peer(name: &str) -> PeerNode {
        use comm::rust::peer_node::{Endpoint, NodeIdentifier};
        use prost::bytes::Bytes;
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::copy_from_slice(name.as_bytes()),
            },
            endpoint: Endpoint {
                host: format!("{name}.local"),
                tcp_port: 40400,
                udp_port: 40404,
            },
        }
    }

    #[test]
    fn payload_sources_default_is_empty() {
        let s = PayloadSources::default();
        assert!(s.sources.is_empty());
        assert!(!s.broadcasted_has_request);
    }

    #[test]
    fn add_blacklist_inserts_under_cap() {
        let mut map: HashMap<PeerNode, u64> = HashMap::new();
        add_blacklist_capped(&mut map, mk_peer("alice"), 1_000);
        add_blacklist_capped(&mut map, mk_peer("bob"), 2_000);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&mk_peer("alice")), Some(&1_000));
        assert_eq!(map.get(&mk_peer("bob")), Some(&2_000));
    }

    #[test]
    fn add_blacklist_is_no_op_at_cap() {
        // LOAD-BEARING: silent no-op past MAX_BLACKLISTED defends
        // against byzantine peer-identity rotation exhausting
        // memory.  Verify directly rather than materializing
        // MAX_BLACKLISTED entries by pre-seeding the map at the
        // cap with sentinel entries.
        let mut map: HashMap<PeerNode, u64> = HashMap::new();
        for i in 0..MAX_BLACKLISTED {
            map.insert(mk_peer(&format!("filler-{i}")), 0);
        }
        assert_eq!(map.len(), MAX_BLACKLISTED);
        add_blacklist_capped(&mut map, mk_peer("overflow"), 9_000);
        assert_eq!(map.len(), MAX_BLACKLISTED, "insert must be a no-op at cap");
        assert!(
            map.get(&mk_peer("overflow")).is_none(),
            "overflow peer must not be in map"
        );
    }

    #[test]
    fn constants_have_expected_values() {
        // Pin consensus-flavored constants so a future refactor
        // can't silently shift the security caps or tick cadence.
        assert_eq!(MAX_SOURCES, 256);
        assert_eq!(MAX_BLACKLISTED, 1024);
        assert_eq!(BLACKLIST_TTL_MS, 60 * 60 * 1000);
        assert_eq!(TICK_PERIOD_MS, 5_000);
    }

    #[tokio::test]
    async fn driver_new_has_zero_pending_and_is_complete() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        assert_eq!(driver.pending_count().await, 0);
        assert!(driver.is_complete().await);
    }

    #[tokio::test]
    async fn enqueue_payload_increments_pending_and_is_idempotent() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xA1; 32];
        driver.enqueue_payload(h).await;
        assert_eq!(driver.pending_count().await, 1);
        assert!(!driver.is_complete().await);
        // Idempotent — repeated enqueue does not re-count.
        driver.enqueue_payload(h).await;
        assert_eq!(
            driver.pending_count().await,
            1,
            "enqueue_payload must be idempotent on the retriever"
        );
    }

    #[tokio::test]
    async fn driver_clone_shares_underlying_state() {
        // LOAD-BEARING: `#[derive(Clone)]` on an Arc-only struct
        // must share the inner retriever + source map + blacklist.
        // Verify by enqueuing on one clone and reading pending_count
        // from another.
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let driver_b = driver.clone();
        driver.enqueue_payload([0xBB; 32]).await;
        assert_eq!(driver_b.pending_count().await, 1);
    }

    fn mk_announcement(hash: &[u8]) -> HasWalPayload {
        use prost::bytes::Bytes;
        HasWalPayload {
            payload_hash: Bytes::copy_from_slice(hash),
            payload_size: 42,
        }
    }

    fn blake2b256(bytes: &[u8]) -> [u8; 32] {
        use crypto::rust::hash::blake2b256::Blake2b256;
        let h = Blake2b256::hash(bytes.to_vec());
        let mut out = [0u8; 32];
        out.copy_from_slice(&h);
        out
    }

    fn mk_response(hash: &[u8], bytes: Vec<u8>) -> WalPayloadResponse {
        use prost::bytes::Bytes;
        WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(hash),
            payload_bytes: Bytes::from(bytes),
        }
    }

    #[tokio::test]
    async fn on_payload_response_accepts_valid_bytes() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let bytes = b"valid payload".to_vec();
        let h = blake2b256(&bytes);
        driver.enqueue_payload(h).await;
        let accepted = driver
            .on_payload_response(mk_peer("alice"), &mk_response(&h, bytes.clone()))
            .await;
        assert!(accepted);
        // Sender NOT blacklisted on happy path.
        let b = driver.blacklisted.read().await;
        assert!(!b.contains_key(&mk_peer("alice")));
    }

    #[tokio::test]
    async fn on_payload_response_byzantine_blacklists_sender() {
        // LOAD-BEARING: a hash-mismatch response is byzantine; the
        // sender must land in the blacklist on the same call.
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let expected = blake2b256(b"expected");
        driver.enqueue_payload(expected).await;
        // Send wrong bytes under the expected hash.
        let response = mk_response(&expected, b"WRONG BYTES".to_vec());
        let accepted = driver
            .on_payload_response(mk_peer("mallory"), &response)
            .await;
        assert!(!accepted);
        let b = driver.blacklisted.read().await;
        assert!(
            b.contains_key(&mk_peer("mallory")),
            "hash-mismatch must blacklist sender"
        );
    }

    #[tokio::test]
    async fn on_payload_response_oversized_blacklists_sender() {
        use crate::rust::engine::wal_payload_retriever::MAX_PAYLOAD_BYTES;
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let bytes = vec![0u8; MAX_PAYLOAD_BYTES + 1];
        let h = blake2b256(&bytes);
        driver.enqueue_payload(h).await;
        let accepted = driver
            .on_payload_response(mk_peer("mallory"), &mk_response(&h, bytes))
            .await;
        assert!(!accepted);
        let b = driver.blacklisted.read().await;
        assert!(b.contains_key(&mk_peer("mallory")));
    }

    #[tokio::test]
    async fn on_payload_response_malformed_hash_blacklists_sender() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        // Response hash is only 16 bytes — malformed.
        let bytes = b"payload".to_vec();
        let bad_hash = &[0u8; 16][..];
        let response = mk_response(bad_hash, bytes);
        let accepted = driver
            .on_payload_response(mk_peer("mallory"), &response)
            .await;
        assert!(!accepted);
        let b = driver.blacklisted.read().await;
        assert!(b.contains_key(&mk_peer("mallory")));
    }

    #[tokio::test]
    async fn on_payload_response_unknown_request_does_not_blacklist() {
        // Unsolicited response — benign (peer may have raced an
        // eviction); must NOT blacklist.
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let bytes = b"unsolicited".to_vec();
        let h = blake2b256(&bytes);
        // NOTE: no enqueue — the request is "unknown" to the
        // retriever.
        let accepted = driver
            .on_payload_response(mk_peer("alice"), &mk_response(&h, bytes))
            .await;
        assert!(!accepted);
        let b = driver.blacklisted.read().await;
        assert!(
            !b.contains_key(&mk_peer("alice")),
            "unsolicited response must NOT blacklist"
        );
    }

    #[tokio::test]
    async fn on_has_wal_payload_records_sender_for_enqueued_hash() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xE1; 32];
        driver.enqueue_payload(h).await;
        driver
            .on_has_wal_payload(mk_peer("alice"), &mk_announcement(&h))
            .await;
        let g = driver.per_hash_sources.read().await;
        let sources = g.get(&h).expect("entry exists");
        assert_eq!(sources.sources.len(), 1);
        assert_eq!(sources.sources[0], mk_peer("alice"));
    }

    #[tokio::test]
    async fn on_has_wal_payload_dedupes_the_same_sender() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xE2; 32];
        driver.enqueue_payload(h).await;
        driver
            .on_has_wal_payload(mk_peer("alice"), &mk_announcement(&h))
            .await;
        driver
            .on_has_wal_payload(mk_peer("alice"), &mk_announcement(&h))
            .await;
        let g = driver.per_hash_sources.read().await;
        assert_eq!(
            g.get(&h).unwrap().sources.len(),
            1,
            "second announcement from same peer must dedup"
        );
    }

    #[tokio::test]
    async fn on_has_wal_payload_ignores_un_enqueued_hash() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xE3; 32];
        driver
            .on_has_wal_payload(mk_peer("alice"), &mk_announcement(&h))
            .await;
        let g = driver.per_hash_sources.read().await;
        assert!(g.get(&h).is_none(), "no source entry created");
    }

    #[tokio::test]
    async fn on_has_wal_payload_wrong_length_blacklists_sender() {
        // LOAD-BEARING: wrong-length payload_hash is byzantine; the
        // sender gets blacklisted immediately (no benefit of the
        // doubt — a well-behaved peer would never emit this).
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let short = [0xE4; 16];
        driver
            .on_has_wal_payload(mk_peer("mallory"), &mk_announcement(&short))
            .await;
        let b = driver.blacklisted.read().await;
        assert!(
            b.contains_key(&mk_peer("mallory")),
            "wrong-length announcement must blacklist sender"
        );
    }

    #[tokio::test]
    async fn on_has_wal_payload_skips_actively_blacklisted_sender() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xE5; 32];
        driver.enqueue_payload(h).await;
        {
            let mut b = driver.blacklisted.write().await;
            b.insert(mk_peer("mallory"), now_ms());
        }
        driver
            .on_has_wal_payload(mk_peer("mallory"), &mk_announcement(&h))
            .await;
        let g = driver.per_hash_sources.read().await;
        assert_eq!(
            g.get(&h).unwrap().sources.len(),
            0,
            "actively blacklisted sender's announcement must be dropped"
        );
    }

    #[tokio::test]
    async fn on_has_wal_payload_accepts_sender_with_expired_blacklist() {
        // LOAD-BEARING TTL semantic: a blacklist entry older than
        // BLACKLIST_TTL_MS is treated as unblacklisted BEFORE the
        // next evict pass runs.
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xE6; 32];
        driver.enqueue_payload(h).await;
        {
            let mut b = driver.blacklisted.write().await;
            b.insert(
                mk_peer("formerly-blacklisted"),
                now_ms().saturating_sub(BLACKLIST_TTL_MS + 1),
            );
        }
        driver
            .on_has_wal_payload(mk_peer("formerly-blacklisted"), &mk_announcement(&h))
            .await;
        let g = driver.per_hash_sources.read().await;
        assert_eq!(
            g.get(&h).unwrap().sources.len(),
            1,
            "expired blacklist must not block fresh announcement"
        );
    }

    #[tokio::test]
    async fn on_has_wal_payload_cap_stops_source_growth() {
        // LOAD-BEARING: MAX_SOURCES cap defends against byzantine
        // identity rotation.  Pre-fill to the cap, then verify
        // the next sender is silently dropped.
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xE7; 32];
        driver.enqueue_payload(h).await;
        {
            let mut g = driver.per_hash_sources.write().await;
            let entry = g.entry(h).or_default();
            for i in 0..MAX_SOURCES {
                entry.sources.push_back(mk_peer(&format!("filler-{i}")));
            }
            assert_eq!(entry.sources.len(), MAX_SOURCES);
        }
        driver
            .on_has_wal_payload(mk_peer("overflow"), &mk_announcement(&h))
            .await;
        let g = driver.per_hash_sources.read().await;
        let sources = g.get(&h).unwrap();
        assert_eq!(sources.sources.len(), MAX_SOURCES, "cap must hold");
        assert!(
            !sources.sources.iter().any(|p| p == &mk_peer("overflow")),
            "overflow sender must not be inserted"
        );
    }

    #[test]
    fn slice_to_hash_narrows_32_byte_input() {
        let input = [0xA5u8; 32];
        assert_eq!(slice_to_hash(&input), Some(input));
        assert_eq!(slice_to_hash(&[0u8; 31]), None);
        assert_eq!(slice_to_hash(&[0u8; 33]), None);
        assert_eq!(slice_to_hash(&[]), None);
    }

    #[tokio::test]
    async fn next_source_for_returns_none_without_enqueue() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        assert!(driver.next_source_for(&[0xD1; 32]).await.is_none());
    }

    #[tokio::test]
    async fn next_source_for_returns_none_with_empty_source_set() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xD2; 32];
        driver.enqueue_payload(h).await;
        // enqueue inits the entry but doesn't add any sources.
        assert!(driver.next_source_for(&h).await.is_none());
    }

    #[tokio::test]
    async fn next_source_for_rotates_fifo_and_skips_blacklisted() {
        // Seed the internal state directly: driver has no public
        // source-insertion method yet (that lands with
        // on_has_wal_payload in a follow-up slice).
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xA2; 32];
        let alice = mk_peer("alice");
        let bob = mk_peer("bob");
        let carol = mk_peer("carol");
        {
            let mut g = driver.per_hash_sources.write().await;
            let entry = g.entry(h).or_default();
            entry.sources.push_back(alice.clone());
            entry.sources.push_back(bob.clone());
            entry.sources.push_back(carol.clone());
        }
        {
            // Blacklist bob with a fresh timestamp.
            let mut b = driver.blacklisted.write().await;
            b.insert(bob.clone(), now_ms());
        }
        // Round 1: alice.  Round 2: bob is blacklisted, so carol
        // (FIFO rotation pushes bob to the back; next eligible is
        // carol).
        let first = driver.next_source_for(&h).await.unwrap();
        assert_eq!(first, alice, "FIFO head must be alice");
        let second = driver.next_source_for(&h).await.unwrap();
        assert_eq!(
            second, carol,
            "bob is actively blacklisted; next_source_for must skip to carol"
        );
    }

    #[tokio::test]
    async fn next_source_for_returns_none_when_all_blacklisted() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xA3; 32];
        let alice = mk_peer("alice");
        let bob = mk_peer("bob");
        {
            let mut g = driver.per_hash_sources.write().await;
            let entry = g.entry(h).or_default();
            entry.sources.push_back(alice.clone());
            entry.sources.push_back(bob.clone());
        }
        {
            let mut b = driver.blacklisted.write().await;
            let t = now_ms();
            b.insert(alice, t);
            b.insert(bob, t);
        }
        assert!(driver.next_source_for(&h).await.is_none());
    }

    #[tokio::test]
    async fn next_source_for_treats_expired_blacklist_as_eligible() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = [0xA4; 32];
        let alice = mk_peer("alice");
        {
            let mut g = driver.per_hash_sources.write().await;
            g.entry(h).or_default().sources.push_back(alice.clone());
        }
        {
            // Timestamp older than the TTL — the entry is logically
            // expired even though `evict_expired_blacklist` hasn't
            // run yet.
            let mut b = driver.blacklisted.write().await;
            b.insert(alice.clone(), now_ms().saturating_sub(BLACKLIST_TTL_MS + 1));
        }
        assert_eq!(driver.next_source_for(&h).await, Some(alice));
    }

    #[tokio::test]
    async fn evict_expired_blacklist_drops_stale_entries() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let alice = mk_peer("alice");
        let bob = mk_peer("bob");
        {
            let mut b = driver.blacklisted.write().await;
            let now = now_ms();
            // alice: fresh.  bob: stale by 1ms.
            b.insert(alice.clone(), now);
            b.insert(bob.clone(), now.saturating_sub(BLACKLIST_TTL_MS + 1));
        }
        let evicted = driver.evict_expired_blacklist().await;
        assert_eq!(evicted, 1, "exactly one stale entry evicted");
        let b = driver.blacklisted.read().await;
        assert!(b.contains_key(&alice), "fresh entry retained");
        assert!(!b.contains_key(&bob), "stale entry gone");
    }

    #[tokio::test]
    async fn evict_expired_blacklist_is_no_op_on_empty_map() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        assert_eq!(driver.evict_expired_blacklist().await, 0);
    }

    #[tokio::test]
    async fn take_bytes_returns_none_for_unresolved_hash() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        driver.enqueue_payload([0xCC; 32]).await;
        assert!(driver.take_bytes(&[0xCC; 32]).await.is_none());
    }

    #[test]
    fn tick_action_variants_pattern_match() {
        // Trivial coverage of the three-way branch.  If a future
        // refactor renames or deletes a variant, this test fires.
        for action in [
            TickAction::SendFresh,
            TickAction::WaitInFlight,
            TickAction::GiveUp,
        ] {
            let _ = match action {
                TickAction::SendFresh => 0u8,
                TickAction::WaitInFlight => 1u8,
                TickAction::GiveUp => 2u8,
            };
        }
    }

    #[test]
    fn boot_apply_error_variants_distinct_under_eq() {
        let timeout = BootApplyError::PayloadFetchTimeout { pending_count: 3 };
        let missing = BootApplyError::MissingResolvedHash {
            hash_hex: "deadbeef".to_string(),
        };
        let failed = BootApplyError::ApplierFailed {
            message: "io".to_string(),
        };
        let panicked = BootApplyError::ApplierPanic {
            message: "boom".to_string(),
        };
        assert_ne!(timeout, missing);
        assert_ne!(missing, failed);
        assert_ne!(failed, panicked);
        assert_eq!(timeout, BootApplyError::PayloadFetchTimeout {
            pending_count: 3
        });
    }
}
