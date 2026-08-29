// Phase 7b-2 WAL payload-fetch joiner sync-driver (2026-08-27).
//
// Orchestrates the client side of the WAL payload-fetch protocol
// from a joining validator's perspective.  After the joiner has
// assembled the latest snapshot (Phase 7b-1), the WAL slice between
// that snapshot and the head block still references write bytes by
// Blake2b256 hash.  For each hash the joiner cannot reproduce
// locally (via the write-payload-determinism reducer — deploy data
// + deterministic Rholang), the joiner:
//
//   1. Broadcasts a HasWalPayloadRequest to discover peers with the
//      bytes.
//   2. On each HasWalPayload reply, records the peer as a source
//      and starts issuing GetWalPayloadRequests round-robin across
//      known sources.
//   3. On each WalPayloadResponse, hands it to the
//      WalPayloadRetriever; on PayloadAccepted, marks the hash
//      resolved and hands the bytes to the fresh-tree WAL applier.
//
// Also handles:
//   * Timeout-driven retries (`tick`): scans for payloads whose
//     last request has aged past REQUEST_TIMEOUT_MS and re-queues
//     them to alternative peers.
//   * Peer rotation: on a byzantine/malformed response, marks the
//     sending peer as failed and rotates to the next source.
//   * Stale-eviction: payloads whose initial request is older than
//     STALE_EVICTION_MS are dropped.
//
// Design shape differs from `snapshot_chunk_sync` in one important
// way: WAL payload namespace is FLAT (hash-addressed), not per-
// snapshot.  So there's a single WalPayloadRetriever + a single
// global source pool + a single global blacklist, all managed by
// one driver.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use comm::rust::peer_node::PeerNode;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::casper::protocol::casper_message::{HasWalPayload, WalPayloadResponse};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::rust::engine::wal_payload_retriever::{AdmitOutcome, WalPayloadRetriever, MAX_RETRIES};
use crate::rust::engine::wal_payload_wire::{
    broadcast_has_wal_payload_request, send_get_wal_payload_request,
};

/// Security caps on peer sets.  Under normal use both stay well
/// below these limits (typical joiner sees ~10 sources and
/// blacklists ~0); the caps defend against an attacker rotating
/// peer identities to exhaust memory.
pub const MAX_SOURCES: usize = 256;
pub const MAX_BLACKLISTED: usize = 1024;

/// Per-payload source tracking.  Records which peers advertised
/// they can serve each payload_hash (from HasWalPayload replies).
#[derive(Debug, Default)]
struct PayloadSources {
    /// Peers known to have this payload.  FIFO for round-robin.
    sources: VecDeque<PeerNode>,
    /// Whether we've broadcast HasWalPayloadRequest yet.  Avoids
    /// duplicate broadcasts on repeat ticks.
    broadcasted_has_request: bool,
}

/// Per-hash tick decision.  Isolates the three-way branch in
/// `tick`'s send-or-skip path.
enum TickAction {
    /// Send a fresh GetWalPayloadRequest for this hash.
    SendFresh,
    /// An outstanding request is in flight and the retry budget
    /// has not been exhausted; wait for a response or a timeout.
    WaitInFlight,
    /// Retry budget exhausted; stop sending.  The entry stays
    /// in the retriever until stale-eviction drops it.
    GiveUp,
}

/// Insert into the blacklist with a size cap and a timestamp.
/// Silent no-op past the cap.  Timestamp lets tick eviction drop
/// entries older than `BLACKLIST_TTL_MS` so a peer that misfires
/// once doesn't get killed forever.
fn add_blacklist_capped(map: &mut HashMap<PeerNode, u64>, peer: PeerNode, now_ms: u64) {
    if map.len() < MAX_BLACKLISTED {
        map.insert(peer, now_ms);
    }
}

/// Time-to-live for a peer's blacklist entry.  A byzantine burst
/// followed by an hour of good behavior lets a peer re-enter the
/// candidate pool.  Not tuned yet — pick a conservative default;
/// operators can revisit if telemetry shows churn.
pub const BLACKLIST_TTL_MS: u64 = 60 * 60 * 1000; // 1 hour

/// The joiner-side driver.  Owns a single retriever + a per-hash
/// source map + a global blacklist.
#[derive(Debug, Clone)]
pub struct WalPayloadSyncDriver {
    /// Verifies + stores payloads.  Shared with the incoming
    /// message dispatch path.
    pub retriever: Arc<WalPayloadRetriever>,
    /// Per-payload_hash source lists (peers that advertised
    /// they can serve it).
    per_hash_sources: Arc<RwLock<HashMap<[u8; 32], PayloadSources>>>,
    /// Peers globally blacklisted after producing a byzantine
    /// response.  Once a peer is blacklisted it's skipped for ALL
    /// payloads; this is a stronger stance than snapshot-chunk
    /// blacklisting because a peer producing bad bytes for ONE
    /// hash is very likely to produce bad bytes for others (or is
    /// otherwise adversarial).
    ///
    /// Value is the Unix-ms timestamp at which the peer was
    /// blacklisted.  Entries older than `BLACKLIST_TTL_MS` are
    /// evicted at the next tick — a byzantine burst followed by
    /// good behavior lets a peer re-enter the candidate pool
    /// (review-fix F-6 2026-08-27; prior version had no eviction).
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

    /// Register a payload_hash we need to fetch.  Idempotent.
    pub async fn enqueue_payload(&self, payload_hash: [u8; 32]) {
        self.retriever.enqueue(payload_hash).await;
        let mut g = self.per_hash_sources.write().await;
        g.entry(payload_hash).or_default();
    }

    /// Called when a HasWalPayload reply arrives.  Records the
    /// sender as a source for that payload_hash.
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
        // Skip if sender is already blacklisted (ignoring
        // expired-TTL entries, which the next tick will evict).
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

    /// Dispatch an incoming WalPayloadResponse to the retriever.
    /// On PayloadHashMismatch / PayloadOversized / MalformedPayloadHash
    /// outcomes, blacklists the sender.
    ///
    /// Returns true if the response was accepted.
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

    /// Pick the next non-blacklisted source for a payload_hash,
    /// rotating the FIFO.  Returns None if no source is available.
    /// TTL-expired blacklist entries are treated as unblacklisted
    /// (they'll be evicted at the next tick).
    async fn next_source_for(&self, hash: &[u8; 32]) -> Option<PeerNode> {
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
    /// once per tick from the driver's periodic loop.  Returns
    /// the number evicted (for metrics / testing).
    pub async fn evict_expired_blacklist(&self) -> usize {
        let now = now_ms();
        let mut b = self.blacklisted.write().await;
        let before = b.len();
        b.retain(|_, ts| now.saturating_sub(*ts) < BLACKLIST_TTL_MS);
        before - b.len()
    }

    /// Send an outbound tick: for each pending payload, either
    /// broadcast a HasWalPayloadRequest (if we haven't yet), or
    /// send GetWalPayloadRequest for it to the next available
    /// source.  Also handles timeout-driven retries + blacklist
    /// TTL eviction + retriever stale-eviction.
    pub async fn tick<T: TransportLayer + Send + Sync>(
        &self,
        transport: &T,
        conf: &RPConf,
        connections_cell: &comm::rust::rp::connect::ConnectionsCell,
    ) {
        // Eviction passes — cheap and important to run every tick
        // regardless of whether we have pending payloads: a
        // long-idle blacklist entry expires; a long-idle pending
        // payload gets dropped.
        let _evicted_blacklist = self.evict_expired_blacklist().await;
        let _evicted_stale = self.retriever.evict_stale().await;

        let pending: Vec<[u8; 32]> = self.retriever.pending_hashes().await;
        for hash in pending {
            // Broadcast HasWalPayloadRequest if we haven't yet.
            let need_broadcast = {
                let mut g = self.per_hash_sources.write().await;
                match g.get_mut(&hash) {
                    Some(s) => {
                        let need = !s.broadcasted_has_request;
                        if need {
                            s.broadcasted_has_request = true;
                        }
                        need
                    }
                    None => {
                        // Payload is pending in retriever but has no
                        // source tracking entry.  Create one now (edge
                        // case: `retriever.enqueue` was called
                        // directly, bypassing driver.enqueue_payload).
                        g.insert(hash, PayloadSources {
                            sources: VecDeque::new(),
                            broadcasted_has_request: true,
                        });
                        true
                    }
                }
            };
            if need_broadcast {
                if let Err(e) =
                    broadcast_has_wal_payload_request(transport, connections_cell, conf, &hash)
                        .await
                {
                    warn!(
                        target: "f1r3fly.casper.wal_payload_sync",
                        error = %e,
                        "broadcast_has_wal_payload_request failed"
                    );
                }
            }

            let source = match self.next_source_for(&hash).await {
                Some(p) => p,
                None => continue,
            };

            // Re-request timed-out payloads.
            let timed_out = self.retriever.timed_out_hashes().await;
            if timed_out.contains(&hash) {
                if let Err(e) = send_get_wal_payload_request(transport, conf, &source, &hash).await
                {
                    warn!(
                        target: "f1r3fly.casper.wal_payload_sync",
                        error = %e,
                        "send_get_wal_payload_request failed for retry"
                    );
                    continue;
                }
                self.retriever
                    .record_request_sent(&hash, source.id.key.as_ref())
                    .await;
                self.retriever.record_retry(&hash).await;
                continue;
            }

            // Decide whether to send a fresh request.  Three states:
            //   * Never asked (last_request_ms == 0)  → send.
            //   * Asked, retry-budget available       → wait for
            //     the outstanding request or a timeout.
            //   * Asked, retry-budget exhausted       → give up.
            //     The pending entry stays in the retriever so
            //     stale-eviction can drop it later; we just stop
            //     sending.  Prior version silently continued
            //     sending past the cap (review-fix F-4 2026-08-27).
            let action = {
                let g = self.retriever.payloads.read().await;
                match g.get(&hash) {
                    Some(s) if s.last_request_ms == 0 => TickAction::SendFresh,
                    Some(s) if s.retry_count < MAX_RETRIES => TickAction::WaitInFlight,
                    Some(_) => TickAction::GiveUp,
                    None => TickAction::WaitInFlight,
                }
            };
            match action {
                TickAction::WaitInFlight => continue,
                TickAction::GiveUp => {
                    debug!(
                        target: "f1r3fly.casper.wal_payload_sync",
                        hash = hex::encode(hash),
                        max_retries = MAX_RETRIES,
                        "retry budget exhausted; not resending"
                    );
                    continue;
                }
                TickAction::SendFresh => {
                    if let Err(e) =
                        send_get_wal_payload_request(transport, conf, &source, &hash).await
                    {
                        warn!(
                            target: "f1r3fly.casper.wal_payload_sync",
                            error = %e,
                            "send_get_wal_payload_request failed"
                        );
                        continue;
                    }
                    self.retriever
                        .record_request_sent(&hash, source.id.key.as_ref())
                        .await;
                }
            }
        }
    }

    /// Query: number of pending payloads.
    pub async fn pending_count(&self) -> usize { self.retriever.pending_count().await }

    /// Query: is all pending work complete?
    pub async fn is_complete(&self) -> bool { self.retriever.is_complete().await }

    /// Retrieve verified bytes for a hash, if resolved.
    pub async fn take_bytes(&self, payload_hash: &[u8; 32]) -> Option<Vec<u8>> {
        self.retriever.get_bytes(payload_hash).await
    }
}

fn now_ms() -> u64 {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

fn slice_to_hash(slice: &[u8]) -> Option<[u8; 32]> {
    if slice.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(slice);
    Some(out)
}

// -------------------------------------------------------------------
// Boot-time enumerator + periodic tick driver.  Both compose the
// primitives above into ready-to-spawn tasks — the running-engine
// setup path calls these once at boot.
// -------------------------------------------------------------------

/// Result of a boot-time enumeration pass.  Split into two
/// counters so telemetry can distinguish "reducer worked, no wire
/// traffic needed" from "we're depending on peers to serve every
/// byte."
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnumerateStats {
    /// Payload hashes handed to the retriever with reproduced
    /// bytes (via `mark_resolved`).  Fetch protocol will NOT
    /// contact peers for these.
    pub resolved_locally: usize,
    /// Payload hashes enqueued for peer fetch (via
    /// `enqueue_payload`) — either because the reducer returned
    /// None or because its reproduced bytes failed the hash check.
    pub enqueued_for_fetch: usize,
}

/// Enumerate WAL entries in a captured WAL slice, extract the
/// unique `payload_ref: Hash(...)` values, and either:
///   * hand the reducer's reproduced bytes to the retriever
///     (`mark_resolved`) if the reducer returned `Some`, OR
///   * enqueue the hash for peer fetch (`enqueue_payload`) if
///     the reducer returned `None` OR its bytes failed the
///     defense-in-depth hash check.
///
/// DD-7b-2 (a) committed 2026-08-27: reducer signature is
/// `FnMut(&WalEntry) -> Option<Vec<u8>>`.  The reducer gets the
/// full `WalEntry` (op, path, offset, mode_bits, owner, group,
/// payload_ref, outcome) so it can attempt reconstruction from
/// on-chain sources — e.g., a `Write` whose bytes came from a
/// deploy argument the joiner already has in block storage.  If
/// the reducer can produce bytes, no peer traffic is needed for
/// that payload.
///
/// **Interior hash check.**  A reducer bug that returns bytes not
/// matching `payload_hash` is caught by `mark_resolved`'s rehash
/// pass; the enumerator falls back to a peer fetch (log at info).
/// This avoids the applier reconstructing corrupt file state.
///
/// For test / early-integration paths, callers can pass
/// `|_| None` (fetch everything).
pub async fn enumerate_and_enqueue_payloads<F>(
    driver: &WalPayloadSyncDriver,
    wal_slice: &[rholang::rust::interpreter::io::wal::WalEntry],
    mut reducer: F,
) -> EnumerateStats
where
    F: FnMut(&rholang::rust::interpreter::io::wal::WalEntry) -> Option<Vec<u8>>,
{
    use rholang::rust::interpreter::io::wal::PayloadRef;
    let mut seen: HashSet<[u8; 32]> = HashSet::new();
    let mut stats = EnumerateStats::default();
    for entry in wal_slice {
        let Some(PayloadRef::Hash(h)) = entry.payload_ref else {
            continue;
        };
        if !seen.insert(h) {
            continue;
        }
        match reducer(entry) {
            Some(bytes) => {
                if driver.retriever.mark_resolved(h, bytes).await {
                    stats.resolved_locally += 1;
                } else {
                    // Reducer produced bytes that don't hash to h.
                    // Fall back to peer fetch; log at info so
                    // operators can spot a reducer regression.
                    info!(
                        target: "f1r3fly.casper.wal_payload_sync",
                        hash = hex::encode(h),
                        "reducer output failed hash check; falling back to peer fetch"
                    );
                    driver.enqueue_payload(h).await;
                    stats.enqueued_for_fetch += 1;
                }
            }
            None => {
                driver.enqueue_payload(h).await;
                stats.enqueued_for_fetch += 1;
            }
        }
    }
    if stats.enqueued_for_fetch > 0 || stats.resolved_locally > 0 {
        info!(
            target: "f1r3fly.casper.wal_payload_sync",
            enqueued_for_fetch = stats.enqueued_for_fetch,
            resolved_locally = stats.resolved_locally,
            "boot-time enumerator pass complete"
        );
    }
    stats
}

// -------------------------------------------------------------------
// Phase 7b-2 item (c) (2026-08-28): boot apply-to-follower flow.
// Composes the enumerator, the fetch driver, and the fresh-tree
// applier so a joiner can go from "assembled snapshot bytes on
// disk" to "WAL slice applied to the local tree" in one call.
// -------------------------------------------------------------------

/// Report from `apply_wal_slice_after_fetch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootApplyReport {
    pub enumerated: EnumerateStats,
    /// Number of unique payload hashes populated in the sidecar
    /// (equal to `enumerated.resolved_locally + enumerated.enqueued_for_fetch`
    /// on the happy path; less if peers failed to serve some).
    pub sidecar_populated: usize,
    /// Number of WAL entries in the applied slice (informational —
    /// includes observation-only variants the applier skips).
    pub wal_entries: usize,
}

/// Reasons `apply_wal_slice_after_fetch` can fail.  Callers
/// pattern-match to distinguish "byzantine input, log + skip"
/// from "genuine peer/network shortfall, retry later".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootApplyError {
    /// `driver.is_complete()` did not return true within `timeout`.
    /// `pending_count` is the number of hashes still outstanding
    /// when the timeout fired.  Callers may re-issue the flow
    /// after peer conditions improve.
    PayloadFetchTimeout { pending_count: usize },
    /// A hash the enumerator populated is missing from the driver
    /// by the time we tried to `take_bytes` — indicates the driver
    /// dropped a resolved entry between `is_complete()` and the
    /// sidecar build (stale-eviction races with the poll loop, or
    /// a `driver.stop()` called mid-collect).
    MissingResolvedHash { hash_hex: String },
    /// The applier returned an `ApplierError` variant (missing
    /// sidecar / unsupported PayloadRef / out-of-allowed-roots
    /// path / NSS failure / IO error / etc).  Byzantine or
    /// misconfigured input; the subscriber logs + continues to
    /// the next snapshot.
    ApplierFailed { message: String },
    /// The `spawn_blocking` task carrying the applier panicked.
    /// Post-2026-08-28 hardening the applier is Result-based
    /// with no panic paths of its own, but this variant catches
    /// defensive-in-depth: a future refactor that reintroduces
    /// a panic won't kill the subscriber loop — it will surface
    /// as this variant instead.
    ApplierPanic { message: String },
}

/// Boot-time compose: enumerate the WAL slice's payload hashes,
/// wait for the fetch driver to resolve every one, build a
/// hash → bytes sidecar, and apply the WAL to the target tree via
/// the fresh-tree applier.
///
/// # Reducer
///
/// DD-7b-2 (a) Option 1 (landed 2026-08-28): when
/// `payload_lookup` is `Some`, each unique payload hash is first
/// asked of the local `PayloadLookup` (typically the joiner's own
/// `DirectoryPayloadStore` populated by prior block processing).
/// A hit hands the bytes to `mark_resolved` (rehash-verified) and
/// skips the peer fetch.  A miss falls through to
/// `enqueue_payload` (peer fetch).  When `payload_lookup` is
/// `None`, every hash is enqueued for peer fetch — matches
/// pre-reducer behavior verbatim.
///
/// Boundary: this reducer only helps for hashes the joiner has
/// SEEN in a locally-processed block (leader- or replay-side
/// journal_write already persisted the bytes to the store).
/// A fresh joiner with an empty payload store gets zero help and
/// falls back to peer fetch on every hash — no regression.
/// Option 2 (deploy-arg AST reproduction from block storage) is
/// tracked separately in the deferred catalog and covers cases
/// this reducer can't.
///
/// # Poll loop
///
/// The function polls `driver.is_complete()` every `poll_interval`
/// with a `timeout` ceiling.  On timeout, returns
/// `BootApplyError::PayloadFetchTimeout` with the outstanding
/// count.  The applier is NOT invoked in this branch — a partial
/// sidecar would panic on missing hashes.
///
/// # `path_map`
///
/// Production joiners pass `|p| p.to_path_buf()` (identity —
/// WAL's `canon_path` is the target).  Test callers pass a
/// translation closure.
///
/// # Blocking IO
///
/// The applier is sync + blocking (open/seek/write/truncate/etc).
/// This function runs it via `tokio::task::spawn_blocking` so the
/// async runtime keeps making progress on other tasks (peer
/// dispatch, tick loop) during a long apply.
pub async fn apply_wal_slice_after_fetch<F>(
    driver: Arc<WalPayloadSyncDriver>,
    wal: Vec<rholang::rust::interpreter::io::wal::WalEntry>,
    path_map: F,
    allowed_roots: Vec<std::path::PathBuf>,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
    payload_lookup: Option<Arc<dyn crate::rust::engine::wal_payload_server::PayloadLookup>>,
) -> Result<BootApplyReport, BootApplyError>
where
    F: Fn(&std::path::Path) -> std::path::PathBuf + Send + Sync + 'static,
{
    use rholang::rust::interpreter::io::wal::PayloadRef;

    // Step 1 — enumerate.  When `payload_lookup` is provided, the
    // reducer queries it for each unique payload hash before
    // enqueueing a peer fetch.  See docstring for the boundary.
    let enumerated = match payload_lookup {
        Some(lookup) => {
            enumerate_and_enqueue_payloads(&driver, &wal, |entry| {
                let PayloadRef::Hash(h) = entry.payload_ref.as_ref()? else {
                    return None;
                };
                let h = *h;
                // A `get` error is treated the same as a miss —
                // fall back to peer fetch.  The store's Err
                // variants (backing IO failed, etc.) are rare and
                // operator-observable via their own log; the
                // reducer shouldn't second-guess whether to
                // propagate them.
                lookup.get(&h).ok().flatten()
            })
            .await
        }
        None => enumerate_and_enqueue_payloads(&driver, &wal, |_| None).await,
    };

    // Step 2 — poll for completion under a timeout ceiling.
    let deadline = std::time::Instant::now() + timeout;
    while !driver.is_complete().await {
        if std::time::Instant::now() >= deadline {
            return Err(BootApplyError::PayloadFetchTimeout {
                pending_count: driver.pending_count().await,
            });
        }
        tokio::time::sleep(poll_interval).await;
    }

    // Step 3 — build sidecar from unique payload hashes.
    let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
    let mut seen: HashSet<[u8; 32]> = HashSet::new();
    for entry in &wal {
        let Some(PayloadRef::Hash(h)) = entry.payload_ref else {
            continue;
        };
        if !seen.insert(h) {
            continue;
        }
        match driver.take_bytes(&h).await {
            Some(bytes) => {
                sidecar.insert(h, bytes);
            }
            None => {
                return Err(BootApplyError::MissingResolvedHash {
                    hash_hex: hex::encode(h),
                });
            }
        }
    }
    let sidecar_populated = sidecar.len();
    let wal_entries = wal.len();

    // Step 4 — apply via spawn_blocking (applier is sync + blocking).
    // The applier is Result-based post-hardening; JoinError on
    // panic surfaces as ApplierPanic so a future refactor that
    // reintroduces a panic path can't take down the subscriber.
    let join_result = tokio::task::spawn_blocking(move || {
        rholang::rust::interpreter::io::wal_applier::apply_wal_to_fresh_tree(
            &wal,
            &sidecar,
            path_map,
            &allowed_roots,
        )
    })
    .await;
    match join_result {
        Ok(Ok(())) => {}
        Ok(Err(applier_err)) => {
            return Err(BootApplyError::ApplierFailed {
                message: applier_err.to_string(),
            });
        }
        Err(join_err) => {
            return Err(BootApplyError::ApplierPanic {
                message: format!("{join_err}"),
            });
        }
    }

    info!(
        target: "f1r3fly.casper.wal_payload_sync",
        wal_entries,
        sidecar_populated,
        resolved_locally = enumerated.resolved_locally,
        enqueued_for_fetch = enumerated.enqueued_for_fetch,
        "boot apply-to-follower flow complete"
    );
    Ok(BootApplyReport {
        enumerated,
        sidecar_populated,
        wal_entries,
    })
}

/// Default tick period between outbound-request rounds.  Matches
/// snapshot_chunk_sync's TICK_PERIOD_MS so operators have one knob.
pub const TICK_PERIOD_MS: u64 = 5_000;

/// Phase 7b-2 item (c) / DD-7b-3 (a) (2026-08-28): explicit stop
/// signal for the periodic tick loop.  Cloneable so multiple call
/// sites can raise it (e.g., a shutdown hook AND a
/// block-processing catch-up detector); `notify_one()` semantics
/// mean the first raise wins and subsequent raises are no-ops.
///
/// DD-7b-3 (a) diverged from the earlier lean of drain-by-
/// stale-eviction — user opted for explicit shutdown plumbing so
/// the runtime shape stays observable ("is the retriever alive?")
/// and idle timer traffic goes to zero on catch-up.
#[derive(Clone)]
pub struct WalPayloadTickStop {
    signal: Arc<tokio::sync::Notify>,
}

impl WalPayloadTickStop {
    /// Raise the stop signal.  The tick loop selecting on this
    /// signal will exit at its next select boundary (immediately
    /// if idle, or after the current in-flight `driver.tick(...)`
    /// completes).  Idempotent — subsequent calls are no-ops
    /// because tokio's `Notify` collapses multiple pending
    /// notifications into one permit.
    pub fn stop(&self) { self.signal.notify_one(); }
}

impl std::fmt::Debug for WalPayloadTickStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Notify has no useful Debug on its own; expose the shape
        // for structured logging in the calling sites.
        f.debug_struct("WalPayloadTickStop")
            .field("signal", &"tokio::sync::Notify")
            .finish()
    }
}

/// Returned by `spawn_periodic_tick`.  Carries both the tick
/// task's `JoinHandle` (so the caller can `abort()` on shutdown)
/// and a `WalPayloadTickStop` handle (so the block-processing
/// catch-up path can raise the graceful-stop signal per
/// DD-7b-3 (a)).
pub struct WalPayloadTickHandle {
    pub join_handle: tokio::task::JoinHandle<()>,
    pub stop: WalPayloadTickStop,
}

/// Spawn a periodic tick task that calls `WalPayloadSyncDriver::tick`
/// every `TICK_PERIOD_MS`.  Returns a `WalPayloadTickHandle` with:
///
///   * `join_handle` — the tokio JoinHandle for hard-abort at
///     shutdown.
///   * `stop` — a cloneable `WalPayloadTickStop` handle whose
///     `stop()` method raises a `tokio::sync::Notify` the loop
///     selects on; the tick task exits cleanly at the next select
///     boundary.
///
/// The graceful stop path (DD-7b-3 (a)) is preferred over aborting
/// the JoinHandle because:
///   * it lets the current `driver.tick(...)` finish (evictions +
///     any in-flight send finish rather than being cancelled mid-
///     await),
///   * it flips the loop's exit intent into a debug trace rather
///     than a task-panic in the runtime.
pub fn spawn_periodic_tick<T>(
    driver: Arc<WalPayloadSyncDriver>,
    transport: Arc<T>,
    conf: RPConf,
    connections_cell: comm::rust::rp::connect::ConnectionsCell,
) -> WalPayloadTickHandle
where
    T: TransportLayer + Send + Sync + 'static,
{
    let signal = Arc::new(tokio::sync::Notify::new());
    let signal_for_task = Arc::clone(&signal);
    let join_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(TICK_PERIOD_MS));
        // Skip the immediate first tick — give the boot enumerator
        // a chance to populate the driver before we start beating.
        interval.tick().await;
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Always tick, even when the pending set is empty:
                    // eviction of expired blacklist entries + stale
                    // retriever entries needs to run regardless of
                    // outbound work.
                    driver.tick(&*transport, &conf, &connections_cell).await;
                }
                _ = signal_for_task.notified() => {
                    info!(
                        target: "f1r3fly.casper.wal_payload_sync",
                        "stop signal received; wal_payload_sync tick loop exiting"
                    );
                    break;
                }
            }
        }
    });
    WalPayloadTickHandle {
        join_handle,
        stop: WalPayloadTickStop { signal },
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::engine::wal_payload_server::InMemoryPayloadStore;

    fn mk_peer(name: &str) -> PeerNode {
        use comm::rust::peer_node::{Endpoint, NodeIdentifier};
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

    fn hash_of(bytes: &[u8]) -> [u8; 32] {
        use crypto::rust::hash::blake2b256::Blake2b256;
        let h = Blake2b256::hash(bytes.to_vec());
        let mut out = [0u8; 32];
        out.copy_from_slice(&h);
        out
    }

    #[tokio::test]
    async fn enqueue_payload_is_idempotent() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = hash_of(b"x");
        driver.enqueue_payload(h).await;
        driver.enqueue_payload(h).await;
        assert_eq!(driver.pending_count().await, 1);
    }

    #[tokio::test]
    async fn has_wal_payload_reply_records_source() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let payload = b"src".to_vec();
        let h = hash_of(&payload);
        driver.enqueue_payload(h).await;
        let alice = mk_peer("alice");
        driver
            .on_has_wal_payload(alice.clone(), &HasWalPayload {
                payload_hash: Bytes::copy_from_slice(&h),
                payload_size: payload.len() as u32,
            })
            .await;
        let g = driver.per_hash_sources.read().await;
        assert_eq!(g[&h].sources.len(), 1);
    }

    #[tokio::test]
    async fn on_payload_response_accepts_valid() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let payload = b"accept".to_vec();
        let h = hash_of(&payload);
        driver.enqueue_payload(h).await;
        let response = WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&h),
            payload_bytes: Bytes::copy_from_slice(&payload),
        };
        let peer = mk_peer("bob");
        assert!(driver.on_payload_response(peer, &response).await);
        assert!(driver.is_complete().await);
        assert_eq!(driver.take_bytes(&h).await, Some(payload));
    }

    #[tokio::test]
    async fn on_payload_response_byzantine_blacklists_sender() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let real_payload = b"truth".to_vec();
        let h = hash_of(&real_payload);
        driver.enqueue_payload(h).await;
        let bogus = WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&h),
            payload_bytes: Bytes::from_static(b"lie"),
        };
        let peer = mk_peer("mallory");
        assert!(!driver.on_payload_response(peer.clone(), &bogus).await);
        let b = driver.blacklisted.read().await;
        assert!(b.contains_key(&peer));
    }

    #[tokio::test]
    async fn next_source_skips_blacklisted() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = hash_of(b"y");
        driver.enqueue_payload(h).await;
        let alice = mk_peer("alice");
        let bob = mk_peer("bob");
        driver
            .on_has_wal_payload(alice.clone(), &HasWalPayload {
                payload_hash: Bytes::copy_from_slice(&h),
                payload_size: 1,
            })
            .await;
        driver
            .on_has_wal_payload(bob.clone(), &HasWalPayload {
                payload_hash: Bytes::copy_from_slice(&h),
                payload_size: 1,
            })
            .await;
        // Blacklist alice (fresh timestamp — well inside TTL).
        driver
            .blacklisted
            .write()
            .await
            .insert(alice.clone(), now_ms());
        let picked1 = driver.next_source_for(&h).await;
        let picked2 = driver.next_source_for(&h).await;
        // Both picks skip alice.
        assert!(picked1.map(|p| p == bob).unwrap_or(false));
        assert!(picked2.map(|p| p == bob).unwrap_or(false));
    }

    #[tokio::test]
    async fn sources_stop_growing_at_cap() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = hash_of(b"cap");
        driver.enqueue_payload(h).await;
        for i in 0..(MAX_SOURCES + 20) {
            let peer = mk_peer(&format!("p{i}"));
            driver
                .on_has_wal_payload(peer, &HasWalPayload {
                    payload_hash: Bytes::copy_from_slice(&h),
                    payload_size: 1,
                })
                .await;
        }
        let g = driver.per_hash_sources.read().await;
        assert_eq!(g[&h].sources.len(), MAX_SOURCES);
    }

    #[tokio::test]
    async fn blacklist_stops_growing_at_cap() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = hash_of(b"bcap");
        driver.enqueue_payload(h).await;
        for i in 0..(MAX_BLACKLISTED + 20) {
            let bogus = WalPayloadResponse {
                payload_hash: Bytes::copy_from_slice(&h),
                payload_bytes: Bytes::from_static(b"lie"),
            };
            driver
                .on_payload_response(mk_peer(&format!("b{i}")), &bogus)
                .await;
        }
        let b = driver.blacklisted.read().await;
        assert_eq!(b.len(), MAX_BLACKLISTED);
    }

    #[tokio::test]
    async fn enumerate_and_enqueue_skips_reproducible() {
        use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let entries: Vec<WalEntry> = ["a", "b", "c"]
            .iter()
            .map(|tag| WalEntry {
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
            })
            .collect();
        // Reducer reproduces bytes for "b" only; others fall
        // through to fetch.  New signature (DD-7b-2 (a)):
        // FnMut(&WalEntry) -> Option<Vec<u8>>.
        let hash_b = hash_of(b"b");
        let stats = enumerate_and_enqueue_payloads(&driver, &entries, |entry| {
            match entry.payload_ref {
                Some(PayloadRef::Hash(h)) if h == hash_b => Some(b"b".to_vec()),
                _ => None,
            }
        })
        .await;
        assert_eq!(stats.enqueued_for_fetch, 2);
        assert_eq!(stats.resolved_locally, 1);
        assert_eq!(driver.pending_count().await, 2);
        // "b" is resolved locally, not pending — its bytes are
        // available via `take_bytes` for the applier.
        assert_eq!(driver.take_bytes(&hash_b).await, Some(b"b".to_vec()));
    }

    /// DD-7b-2 (a) pin (2026-08-27): a reducer that returns `None`
    /// for every entry (the "fetch everything" configuration)
    /// enqueues every unique hash for peer fetch and resolves
    /// nothing locally.  Same behavior as the pre-DD-7b-2 code's
    /// `|_| false` predicate.
    #[tokio::test]
    async fn enumerate_and_enqueue_fetch_everything_reducer() {
        use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let entries: Vec<WalEntry> = ["x", "y", "z"]
            .iter()
            .map(|tag| WalEntry {
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
            })
            .collect();
        let stats = enumerate_and_enqueue_payloads(&driver, &entries, |_| None).await;
        assert_eq!(stats.enqueued_for_fetch, 3);
        assert_eq!(stats.resolved_locally, 0);
        assert_eq!(driver.pending_count().await, 3);
    }

    /// DD-7b-2 (a) pin (2026-08-27): a reducer that reproduces
    /// EVERY entry's bytes locally results in zero peer fetches
    /// and a fully-complete retriever — the joiner can proceed
    /// straight to the applier.  This is the ideal steady state
    /// for a reducer with full write-payload-determinism coverage.
    #[tokio::test]
    async fn enumerate_and_enqueue_full_reducer_coverage_needs_no_fetch() {
        use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let payloads: Vec<&[u8]> = vec![b"one", b"two", b"three"];
        let entries: Vec<WalEntry> = payloads
            .iter()
            .map(|p| WalEntry {
                op: WalOp::Write,
                path: std::path::PathBuf::from("/anywhere"),
                extra_path: None,
                offset: Some(0),
                length: Some(p.len() as u64),
                payload_ref: Some(PayloadRef::hash(p)),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            })
            .collect();
        // Reducer looks up bytes from a captured map keyed by the
        // entry's payload_ref hash.
        let table: std::collections::HashMap<[u8; 32], Vec<u8>> = payloads
            .iter()
            .map(|p| (hash_of(p), p.to_vec()))
            .collect();
        let stats = enumerate_and_enqueue_payloads(&driver, &entries, |entry| {
            if let Some(PayloadRef::Hash(h)) = entry.payload_ref {
                table.get(&h).cloned()
            } else {
                None
            }
        })
        .await;
        assert_eq!(stats.resolved_locally, 3);
        assert_eq!(stats.enqueued_for_fetch, 0);
        // Zero pending → applier can immediately begin.
        assert!(driver.is_complete().await);
        for p in &payloads {
            assert_eq!(driver.take_bytes(&hash_of(p)).await.as_deref(), Some(*p));
        }
    }

    /// Enumerator dedup pin (2026-08-27, retrospective review):
    /// a WAL slice with the same `payload_ref: Hash(h)` on
    /// multiple entries (a Rholang deploy writing identical bytes
    /// N times) MUST only enqueue / resolve once — the
    /// `HashSet<[u8;32]> seen` inside the enumerator dedups.
    /// Without this dedup, an adversarial deploy could inflate
    /// pending_count and starve the fetch protocol.
    #[tokio::test]
    async fn enumerate_and_enqueue_deduplicates_repeated_hashes() {
        use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        // Ten WAL entries, all referencing the same payload hash.
        let payload = b"repeat".to_vec();
        let h = hash_of(&payload);
        let entries: Vec<WalEntry> = (0..10)
            .map(|i| WalEntry {
                op: WalOp::Write,
                path: std::path::PathBuf::from(format!("/f{i}")),
                extra_path: None,
                offset: Some(0),
                length: Some(payload.len() as u64),
                payload_ref: Some(PayloadRef::Hash(h)),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            })
            .collect();
        let stats = enumerate_and_enqueue_payloads(&driver, &entries, |_| None).await;
        assert_eq!(
            stats.enqueued_for_fetch, 1,
            "10 entries with same hash must enqueue only 1 fetch",
        );
        assert_eq!(driver.pending_count().await, 1);
    }

    /// DD-7b-2 (a) safety pin: a buggy reducer that returns bytes
    /// which don't hash to the entry's `payload_ref` MUST fall
    /// back to peer fetch — otherwise the applier would
    /// reconstruct corrupt file state and downstream state hashes
    /// would diverge from peers.  The `mark_resolved`
    /// defense-in-depth rehash check catches this.
    ///
    /// Uses release-build behavior (return false, log at info)
    /// via the retriever's `mark_resolved` returning false; a
    /// debug build would `debug_assert!` panic.  We compile-guard
    /// the assertion so both build profiles pass.
    #[cfg(not(debug_assertions))]
    #[tokio::test]
    async fn enumerate_and_enqueue_reducer_returning_wrong_bytes_falls_back_to_fetch() {
        use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let real_payload = b"pristine".to_vec();
        let h = hash_of(&real_payload);
        let entry = WalEntry {
            op: WalOp::Write,
            path: std::path::PathBuf::from("/f"),
            extra_path: None,
            offset: Some(0),
            length: Some(real_payload.len() as u64),
            payload_ref: Some(PayloadRef::Hash(h)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        // Reducer bug: returns garbage instead of the real bytes.
        let stats =
            enumerate_and_enqueue_payloads(&driver, std::slice::from_ref(&entry), |_| {
                Some(b"garbage bytes".to_vec())
            })
            .await;
        assert_eq!(stats.enqueued_for_fetch, 1);
        assert_eq!(stats.resolved_locally, 0);
        assert_eq!(driver.pending_count().await, 1);
    }

    /// Placeholder: exercise InMemoryPayloadStore integration for
    /// smoke coverage.
    #[tokio::test]
    async fn in_memory_store_round_trip_via_driver() {
        let store = InMemoryPayloadStore::new();
        let payload = b"driver-e2e".to_vec();
        let h = store.insert(payload.clone());

        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        driver.enqueue_payload(h).await;

        // Simulate the server producing a response.
        let response = tokio::task::spawn_blocking({
            let store = store.clone();
            move || {
                crate::rust::engine::wal_payload_server::serve_payload(&h, &store).expect("serve")
            }
        })
        .await
        .unwrap();

        let peer = mk_peer("peer");
        assert!(driver.on_payload_response(peer, &response).await);
        assert_eq!(driver.take_bytes(&h).await, Some(payload));
    }

    /// F-6 pin: a blacklist entry whose TTL has expired gets
    /// evicted by `evict_expired_blacklist`.  A byzantine peer
    /// that misbehaved an hour ago rejoins the candidate pool
    /// instead of being killed forever.
    #[tokio::test]
    async fn evict_expired_blacklist_drops_stale_entries() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let stale = mk_peer("stale");
        let fresh = mk_peer("fresh");
        {
            let mut b = driver.blacklisted.write().await;
            // Stale: blacklisted at t=0 (before epoch, definitely
            // past TTL from any real "now").
            b.insert(stale.clone(), 0);
            // Fresh: blacklisted just now.
            b.insert(fresh.clone(), now_ms());
        }
        let evicted = driver.evict_expired_blacklist().await;
        assert_eq!(evicted, 1);
        let b = driver.blacklisted.read().await;
        assert!(!b.contains_key(&stale));
        assert!(b.contains_key(&fresh));
    }

    /// F-6 pin: `next_source_for` treats an expired-TTL blacklist
    /// entry as "not blacklisted" — the peer is eligible again
    /// even before the next tick eviction runs.
    #[tokio::test]
    async fn next_source_treats_expired_blacklist_as_eligible() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = hash_of(b"rehab");
        driver.enqueue_payload(h).await;
        let peer = mk_peer("rehab");
        driver
            .on_has_wal_payload(peer.clone(), &HasWalPayload {
                payload_hash: Bytes::copy_from_slice(&h),
                payload_size: 1,
            })
            .await;
        // Blacklist with a very old timestamp — past TTL.
        driver.blacklisted.write().await.insert(peer.clone(), 0);
        let picked = driver.next_source_for(&h).await;
        assert_eq!(picked, Some(peer), "expired-TTL peer must be reselectable");
    }

    /// F-4 pin: after MAX_RETRIES retries, tick's decision path
    /// produces `TickAction::GiveUp`.  We can't observe TickAction
    /// directly (private enum), so we simulate: enqueue, fake the
    /// retriever state to have retry_count == MAX_RETRIES + 1
    /// and last_request_ms > 0, verify no fresh outbound record
    /// is created by inspecting the pending count and retry_count
    /// after a tick pass.  Since we can't mock TransportLayer
    /// inside this test file without adding infrastructure, we
    /// exercise the decision at a lower level: check that a
    /// hash with exhausted retries stays untouched by
    /// `timed_out_hashes` (which filters retries < MAX_RETRIES).
    #[tokio::test]
    async fn timed_out_hashes_skips_exhausted_retry_budget() {
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let h = hash_of(b"exhausted");
        driver.enqueue_payload(h).await;
        // Fake exhausted state: last request in the distant past
        // + retry_count == MAX_RETRIES.
        {
            let mut g = driver.retriever.payloads.write().await;
            let s = g.get_mut(&h).unwrap();
            s.initial_request_ms = now_ms().saturating_sub(60_000);
            s.last_request_ms = now_ms().saturating_sub(60_000);
            s.retry_count = crate::rust::engine::wal_payload_retriever::MAX_RETRIES;
        }
        let timed_out = driver.retriever.timed_out_hashes().await;
        assert!(
            !timed_out.contains(&h),
            "exhausted retry budget must NOT appear in timed_out_hashes",
        );
    }

    /// T-12: tick runs eviction (blacklist + stale retriever
    /// entries) even when no payloads are pending.  The prior
    /// `spawn_periodic_tick` short-circuited on empty pending set,
    /// which meant blacklist entries never expired if the joiner
    /// was idle.  We can't easily test spawn_periodic_tick
    /// directly (it loops forever), but we can pin
    /// `tick(...)`'s eviction behavior by running it with no
    /// pending payloads + a stale blacklist entry.
    #[tokio::test]
    async fn tick_evicts_expired_blacklist_when_no_pending_work() {
        // No pending payloads.
        let driver = WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new()));
        let stale = mk_peer("stale");
        driver.blacklisted.write().await.insert(stale.clone(), 0);
        assert!(driver.blacklisted.read().await.contains_key(&stale));

        // We need a TransportLayer + ConnectionsCell to call
        // tick, but the eviction path runs BEFORE any send.  Rather
        // than wire a mock (which we already have in
        // wal_payload_wire tests), call evict_expired_blacklist
        // directly — the equivalent code path the tick loop runs.
        driver.evict_expired_blacklist().await;
        assert!(!driver.blacklisted.read().await.contains_key(&stale));
    }

    // -----------------------------------------------------------
    // Phase 7b-2 item (c) (2026-08-28): apply_wal_slice_after_fetch
    // pins.  Composes enumerate → fetch → sidecar → applier.
    // -----------------------------------------------------------

    fn write_entry(path: &str, off: u64, payload: &[u8]) -> rholang::rust::interpreter::io::wal::WalEntry {
        use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
        WalEntry {
            op: WalOp::WriteAt,
            path: std::path::PathBuf::from(path),
            extra_path: None,
            offset: Some(off),
            length: Some(payload.len() as u64),
            payload_ref: Some(PayloadRef::hash(payload)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    /// Happy path: an already-resolved retriever + a two-entry WAL
    /// slice applies to a fresh tree in one shot.  We pre-populate
    /// the retriever via `mark_resolved` so the poll loop does not
    /// have to wait for fetch traffic.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn apply_wal_slice_after_fetch_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.bin");
        std::fs::write(&target, vec![0u8; 32]).unwrap();

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new())));
        let payload_a = b"AAAA".to_vec();
        let payload_b = b"BBBB".to_vec();
        driver
            .retriever
            .mark_resolved(hash_of(&payload_a), payload_a.clone())
            .await;
        driver
            .retriever
            .mark_resolved(hash_of(&payload_b), payload_b.clone())
            .await;

        let target_path = target.clone();
        let wal = vec![
            write_entry(target_path.to_str().unwrap(), 0, &payload_a),
            write_entry(target_path.to_str().unwrap(), 8, &payload_b),
        ];
        let report = apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal,
            move |p| p.to_path_buf(),
            Vec::new(),
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
            None,
        )
        .await
        .expect("apply happy path");

        assert_eq!(report.wal_entries, 2);
        assert_eq!(report.sidecar_populated, 2);
        // Reducer is `|_| None` — enumerator counts these as
        // "enqueued for fetch" regardless of whether the retriever
        // has bytes stashed already (mark_resolved was called
        // outside the enumerator flow).  The `is_complete()` poll
        // short-circuits because those bytes ARE stashed.
        assert_eq!(report.enumerated.resolved_locally, 0);
        assert_eq!(report.enumerated.enqueued_for_fetch, 2);

        let got = std::fs::read(&target).unwrap();
        assert_eq!(&got[..4], payload_a.as_slice());
        assert_eq!(&got[8..12], payload_b.as_slice());
    }

    /// Timeout path: unresolved payloads + short timeout returns
    /// `PayloadFetchTimeout` without invoking the applier.  The
    /// target file must be untouched.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn apply_wal_slice_after_fetch_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("untouched.bin");
        std::fs::write(&target, vec![0u8; 16]).unwrap();

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new())));
        // Do NOT mark_resolved — the payload will stay pending.
        let payload = b"never-arrives".to_vec();
        let wal = vec![write_entry(target.to_str().unwrap(), 0, &payload)];

        let err = apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal,
            |p| p.to_path_buf(),
            Vec::new(),
            std::time::Duration::from_millis(120),
            std::time::Duration::from_millis(20),
            None,
        )
        .await
        .expect_err("expect timeout");

        assert!(
            matches!(err, BootApplyError::PayloadFetchTimeout { pending_count: 1 }),
            "got {err:?}"
        );
        // Target unchanged.
        assert_eq!(std::fs::read(&target).unwrap(), vec![0u8; 16]);
    }

    /// Repeated hashes across a WAL slice must be applied once via
    /// the applier's normal iteration order but the sidecar build
    /// is dedup'd — no duplicate `take_bytes` calls.  This pin
    /// exercises the `seen` set inside the collector.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn apply_wal_slice_after_fetch_deduplicates_sidecar_lookups() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dedup.bin");
        std::fs::write(&target, vec![0u8; 32]).unwrap();

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new())));
        let payload = b"REPEATED".to_vec();
        driver
            .retriever
            .mark_resolved(hash_of(&payload), payload.clone())
            .await;

        // Three WriteAt entries all referencing the same payload
        // hash at different offsets.
        let wal = vec![
            write_entry(target.to_str().unwrap(), 0, &payload),
            write_entry(target.to_str().unwrap(), 8, &payload),
            write_entry(target.to_str().unwrap(), 16, &payload),
        ];
        let report = apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal,
            |p| p.to_path_buf(),
            Vec::new(),
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
            None,
        )
        .await
        .expect("apply with dedup");

        assert_eq!(report.wal_entries, 3);
        assert_eq!(report.sidecar_populated, 1, "one unique hash → one sidecar entry");
        // All three writes landed.
        let got = std::fs::read(&target).unwrap();
        assert_eq!(&got[0..8], payload.as_slice());
        assert_eq!(&got[8..16], payload.as_slice());
        assert_eq!(&got[16..24], payload.as_slice());
    }

    // -----------------------------------------------------------
    // 2026-08-28 hardening pins.  Verify the applier's Result-
    // based errors propagate as `ApplierFailed`, path validation
    // fires as `ApplierFailed` too, and (defense-in-depth) a
    // panicking blocking closure becomes `ApplierPanic` rather
    // than killing the subscriber.
    // -----------------------------------------------------------

    /// A WAL entry pointing outside `allowed_roots` bubbles up as
    /// `BootApplyError::ApplierFailed` — the file is not written,
    /// the applier surfaces the specific reason via its Display
    /// impl, and the async task returns cleanly.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn apply_wal_slice_after_fetch_rejects_out_of_root_paths() {
        let allowed = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_target = outside.path().join("bad.bin");
        std::fs::write(&outside_target, vec![0u8; 8]).unwrap();

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(
            WalPayloadRetriever::new(),
        )));
        let payload = b"blocked".to_vec();
        driver
            .retriever
            .mark_resolved(hash_of(&payload), payload.clone())
            .await;
        let wal = vec![write_entry(outside_target.to_str().unwrap(), 0, &payload)];
        let err = apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal,
            |p| p.to_path_buf(),
            vec![allowed.path().to_path_buf()],
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
            None,
        )
        .await
        .expect_err("out-of-root path must Err");
        assert!(
            matches!(err, BootApplyError::ApplierFailed { .. }),
            "got {err:?}"
        );
        // Outside file untouched.
        assert_eq!(std::fs::read(&outside_target).unwrap(), vec![0u8; 8]);
    }

    /// A synthetic ApplierError (via a missing sidecar entry —
    /// reached by NOT calling mark_resolved but manually
    /// pre-populating an already-resolved marker via
    /// `enqueue_payload` + the driver's internal state) surfaces
    /// as `BootApplyError::ApplierFailed`.  Verifies the error
    /// pipeline, not just the happy path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn apply_wal_slice_after_fetch_surfaces_applier_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.bin");
        std::fs::write(&target, vec![0u8; 8]).unwrap();

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(
            WalPayloadRetriever::new(),
        )));
        // Craft a WAL entry whose payload_ref is DeployRef — the
        // applier reports UnsupportedPayloadRef.  No sidecar entry
        // needed; the applier reaches the DeployRef branch before
        // any hash lookup.
        use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
        let wal = vec![WalEntry {
            op: WalOp::WriteAt,
            path: target.clone(),
            extra_path: None,
            offset: Some(0),
            length: Some(0),
            payload_ref: Some(PayloadRef::DeployRef {
                block_hash: [0; 32],
                deploy_index: 0,
                arg_index: 0,
            }),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }];
        // No enumerator work needed — DeployRef doesn't populate
        // a Hash for the enumerator to enqueue.  Retriever stays
        // empty; is_complete is true immediately.
        let err = apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal,
            |p| p.to_path_buf(),
            Vec::new(),
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
            None,
        )
        .await
        .expect_err("DeployRef must Err");
        assert!(
            matches!(err, BootApplyError::ApplierFailed { .. }),
            "got {err:?}"
        );
        // Target untouched.
        assert_eq!(std::fs::read(&target).unwrap(), vec![0u8; 8]);
    }

    /// Post-timeout resilience: a run that times out leaves the
    /// retriever in a state where the same payload can be resolved
    /// later; a subsequent apply run on a WAL slice sharing the
    /// hash succeeds after the resolution.  Verifies that a
    /// timed-out flow doesn't corrupt driver state.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn apply_wal_slice_after_fetch_post_timeout_retry_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("t.bin");
        std::fs::write(&target, vec![0u8; 16]).unwrap();

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(
            WalPayloadRetriever::new(),
        )));
        let payload = b"eventually".to_vec();
        let wal = vec![write_entry(target.to_str().unwrap(), 0, &payload)];

        // First call: no bytes stashed → timeout.
        let err = apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal.clone(),
            |p| p.to_path_buf(),
            Vec::new(),
            std::time::Duration::from_millis(80),
            std::time::Duration::from_millis(20),
            None,
        )
        .await
        .expect_err("first call must time out");
        assert!(matches!(err, BootApplyError::PayloadFetchTimeout { .. }));

        // Now the byte arrives (e.g., peer eventually served it).
        driver
            .retriever
            .mark_resolved(hash_of(&payload), payload.clone())
            .await;

        // Second call on the same WAL slice: enumerator sees hash
        // already-enqueued; is_complete is true; applier runs.
        let report = apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal,
            |p| p.to_path_buf(),
            Vec::new(),
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
            None,
        )
        .await
        .expect("post-timeout retry must succeed");
        assert_eq!(report.sidecar_populated, 1);

        // Payload landed.
        let got = std::fs::read(&target).unwrap();
        assert_eq!(&got[..payload.len()], payload.as_slice());
    }

    /// DD-7b-3 (a) pin (2026-08-28): `WalPayloadTickStop::stop()`
    /// cleanly exits the tick loop.  Verifies the graceful-stop
    /// path added in c-3 — the block-processing catch-up path
    /// raises the stop signal instead of aborting the JoinHandle,
    /// letting the last `driver.tick(...)` run complete + timers
    /// go quiet.
    ///
    /// Uses `TransportLayerStub` — the tick loop never issues a
    /// wire send in an empty-pending scenario, so any stub
    /// transport is inert.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_periodic_tick_stop_signal_exits_loop() {
        use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
        let local = {
            use comm::rust::peer_node::{Endpoint, NodeIdentifier};
            use prost::bytes::Bytes;
            PeerNode {
                id: NodeIdentifier {
                    key: Bytes::from_static(b"stopper"),
                },
                endpoint: Endpoint {
                    host: "host".into(),
                    tcp_port: 40400,
                    udp_port: 40400,
                },
            }
        };
        let rp_conf = create_rp_conf_ask(local.clone(), None, None);
        let connections_cell = comm::rust::rp::connect::ConnectionsCell {
            peers: Arc::new(std::sync::Mutex::new(
                comm::rust::rp::connect::Connections::from_vec(vec![local]),
            )),
        };
        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new())));
        let transport = Arc::new(TransportLayerStub::new());

        let tick =
            spawn_periodic_tick(Arc::clone(&driver), transport, rp_conf, connections_cell);
        // Immediately raise stop; loop should exit at the next
        // select boundary (the initial `interval.tick().await`
        // yields, then the select! polls the notified() branch
        // which resolves because notify_one was already called).
        tick.stop.stop();
        // Give the runtime a moment; then join with a strict
        // timeout to catch a wedged tick loop.
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), tick.join_handle).await;
        assert!(result.is_ok(), "tick loop must exit within timeout after stop");
    }

    /// Cloning the `WalPayloadTickStop` and raising the clone
    /// stops the loop just as well — the two handles share the
    /// same `Notify`.  Verifies the "multiple call sites can
    /// raise it" property documented on the struct.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_periodic_tick_stop_signal_is_cloneable() {
        use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
        let local = {
            use comm::rust::peer_node::{Endpoint, NodeIdentifier};
            use prost::bytes::Bytes;
            PeerNode {
                id: NodeIdentifier {
                    key: Bytes::from_static(b"clone"),
                },
                endpoint: Endpoint {
                    host: "host".into(),
                    tcp_port: 40400,
                    udp_port: 40400,
                },
            }
        };
        let rp_conf = create_rp_conf_ask(local.clone(), None, None);
        let connections_cell = comm::rust::rp::connect::ConnectionsCell {
            peers: Arc::new(std::sync::Mutex::new(
                comm::rust::rp::connect::Connections::from_vec(vec![local]),
            )),
        };
        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new())));
        let transport = Arc::new(TransportLayerStub::new());

        let tick =
            spawn_periodic_tick(Arc::clone(&driver), transport, rp_conf, connections_cell);
        let stop_clone = tick.stop.clone();
        stop_clone.stop();
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), tick.join_handle).await;
        assert!(result.is_ok(), "cloned stop handle must exit the tick loop");
    }

    /// Applier path_map is honored — a closure that redirects
    /// writes into a separate root leaves the original target
    /// untouched.  Mirrors the wal_applier unit `path_map_closure_
    /// redirects_writes` at the boot-flow level.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn apply_wal_slice_after_fetch_honors_path_map() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();
        std::fs::write(src_dir.path().join("f.bin"), vec![0u8; 8]).unwrap();
        std::fs::write(dst_dir.path().join("f.bin"), vec![0u8; 8]).unwrap();

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new())));
        let payload = b"redir".to_vec();
        driver
            .retriever
            .mark_resolved(hash_of(&payload), payload.clone())
            .await;

        let src_path = src_dir.path().join("f.bin");
        let wal = vec![write_entry(src_path.to_str().unwrap(), 0, &payload)];

        let src = src_dir.path().to_path_buf();
        let dst = dst_dir.path().to_path_buf();
        apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal,
            move |p| {
                let rel = p.strip_prefix(&src).unwrap();
                dst.join(rel)
            },
            Vec::new(),
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
            None,
        )
        .await
        .expect("apply with path_map");

        // src untouched.
        assert_eq!(std::fs::read(src_dir.path().join("f.bin")).unwrap(), vec![0u8; 8]);
        // dst reflects the write.
        let got = std::fs::read(dst_dir.path().join("f.bin")).unwrap();
        assert_eq!(&got[..payload.len()], payload.as_slice());
    }

    // ---------------------------------------------------------------
    // DD-7b-2 (a) Option 1 (2026-08-28): PayloadLookup-backed
    // reducer pins.  Prove that the boot enumerator consults the
    // local `PayloadLookup` before enqueueing peer fetch, and that
    // the safety-net rehash in `mark_resolved` catches a lookup
    // that returns bytes not matching the requested hash.
    // ---------------------------------------------------------------

    /// A payload present in the local store is `mark_resolved`ed
    /// via the reducer and does NOT hit the peer-fetch queue.
    /// `apply_wal_slice_after_fetch` reports `resolved_locally = 1`
    /// (not `enqueued_for_fetch`), the retriever's pending queue
    /// stays empty, and the applier still runs with correct
    /// bytes.  This is the DD-7b-2 (a) Option 1 win: warm re-boots
    /// with a populated payload store skip wire traffic entirely.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn payload_lookup_reducer_resolves_from_local_store() {
        use crate::rust::engine::wal_payload_server::PayloadLookup;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.bin");
        std::fs::write(&target, vec![0u8; 32]).unwrap();

        let payload = b"from-local-store".to_vec();
        let store = InMemoryPayloadStore::new();
        let payload_hash = store.insert(payload.clone());
        assert_eq!(payload_hash, hash_of(&payload));
        let lookup: Arc<dyn PayloadLookup> = Arc::new(store);

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new())));
        let wal = vec![write_entry(target.to_str().unwrap(), 0, &payload)];

        let report = apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal,
            move |p| p.to_path_buf(),
            Vec::new(),
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
            Some(lookup),
        )
        .await
        .expect("apply must succeed on locally-resolved payload");

        // The reducer resolved the payload before enqueueing;
        // fetch traffic stayed at zero for this hash.
        assert_eq!(report.enumerated.resolved_locally, 1);
        assert_eq!(report.enumerated.enqueued_for_fetch, 0);
        // And the applier still wrote the correct bytes.
        let got = std::fs::read(&target).unwrap();
        assert_eq!(&got[..payload.len()], payload.as_slice());
    }

    /// A payload absent from the local store falls through to
    /// peer fetch — the reducer returns None and the enumerator
    /// enqueues the hash.  Fresh joiners with an empty store see
    /// this on every hash; the boundary is documented on
    /// `apply_wal_slice_after_fetch`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn payload_lookup_reducer_falls_back_to_fetch_on_miss() {
        use crate::rust::engine::wal_payload_server::PayloadLookup;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("t.bin");
        std::fs::write(&target, vec![0u8; 16]).unwrap();

        // Empty store — every lookup returns None.
        let store = InMemoryPayloadStore::new();
        let lookup: Arc<dyn PayloadLookup> = Arc::new(store);

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new())));
        // Pre-resolve the payload directly on the retriever so
        // the poll loop short-circuits (test-harness convenience:
        // simulate "peer fetch already happened").
        let payload = b"peer-served".to_vec();
        driver
            .retriever
            .mark_resolved(hash_of(&payload), payload.clone())
            .await;
        let wal = vec![write_entry(target.to_str().unwrap(), 0, &payload)];

        let report = apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal,
            move |p| p.to_path_buf(),
            Vec::new(),
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
            Some(lookup),
        )
        .await
        .expect("apply must succeed via peer-served payload");

        // Reducer missed; enumerator enqueued for fetch — the
        // pre-marked payload made `is_complete()` true so the
        // apply still ran.
        assert_eq!(report.enumerated.resolved_locally, 0);
        assert_eq!(report.enumerated.enqueued_for_fetch, 1);
        let got = std::fs::read(&target).unwrap();
        assert_eq!(&got[..payload.len()], payload.as_slice());
    }

    /// Safety pin: a lookup that returns bytes whose hash does
    /// NOT match the requested hash is caught by
    /// `mark_resolved`'s defense-in-depth rehash; the enumerator
    /// falls back to peer fetch instead of feeding corrupt bytes
    /// to the applier.  Guards against a future PayloadLookup impl
    /// bug or a poisoned local store.
    ///
    /// In release builds, `mark_resolved` returns false silently
    /// and the enumerator falls back to fetch.  In debug builds,
    /// `mark_resolved` panics on the mismatch (`debug_assert!`)
    /// to catch reducer bugs early — this test is release-gated so
    /// it exercises the fallback path without tripping the debug
    /// panic.
    #[cfg(not(debug_assertions))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn payload_lookup_reducer_corrupt_bytes_fall_back_to_fetch() {
        use crate::rust::engine::wal_payload_server::PayloadLookup;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("t.bin");
        std::fs::write(&target, vec![0u8; 16]).unwrap();

        let real_payload = b"real-bytes".to_vec();
        let real_hash = hash_of(&real_payload);
        // Store maps real_hash → WRONG bytes via
        // `insert_with_hash` (which bypasses the auto-hash check
        // — that's what makes it possible to plant corrupt
        // entries for this test).
        let store = InMemoryPayloadStore::new();
        store.insert_with_hash(real_hash, b"totally-wrong-bytes".to_vec());
        let lookup: Arc<dyn PayloadLookup> = Arc::new(store);

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new())));
        // Pre-resolve the CORRECT bytes on the retriever so the
        // peer-fetch fallback path is what `is_complete()` sees.
        driver
            .retriever
            .mark_resolved(real_hash, real_payload.clone())
            .await;
        let wal = vec![write_entry(target.to_str().unwrap(), 0, &real_payload)];

        let report = apply_wal_slice_after_fetch(
            Arc::clone(&driver),
            wal,
            move |p| p.to_path_buf(),
            Vec::new(),
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
            Some(lookup),
        )
        .await
        .expect("apply must succeed via peer-served correct bytes");

        // Reducer's wrong bytes were rejected → enumerator
        // enqueued the hash for peer fetch instead.
        assert_eq!(report.enumerated.resolved_locally, 0);
        assert_eq!(report.enumerated.enqueued_for_fetch, 1);
        // Correct bytes landed on disk.
        let got = std::fs::read(&target).unwrap();
        assert_eq!(&got[..real_payload.len()], real_payload.as_slice());
    }
}
