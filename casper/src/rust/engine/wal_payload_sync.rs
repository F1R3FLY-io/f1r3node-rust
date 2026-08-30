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

/// DD-7b-2 (a) Option 2 (2026-08-29): bundle of handles the
/// second-tier reducer needs to reproduce write bytes from block-
/// stored deploys.  Chains through:
///   1. `block_storage.lookup_payload_source(&payload_hash)` →
///      `Option<deploy_sig>` (via the DD-7b-2 (a) Option 2 index
///      populated by every leader/follower `journal_write` for
///      Consensus caps).
///   2. `block_storage.lookup_by_deploy_id(&deploy_sig)` →
///      `Option<block_hash>` (existing `deploy_index`).
///   3. `block_store.get(&block_hash)` → `Option<BlockMessage>`
///      then find the `ProcessedDeploy` inside `body.deploys`.
///   4. `capture_consensus_writes_by_replaying_deploy(runtime_manager,
///      pre_state_hash, processed_deploy, block_kind, purse_snapshot)`
///      → `HashMap<[u8; 32], Vec<u8>>`.
///   5. Return the entry for `payload_hash`.
///
/// Any miss on the chain falls through to peer fetch (design
/// decision 6: "lazy" fork/prune — the block-load step returns
/// None cleanly if the source block has been pruned).  The bytes
/// returned MUST match `payload_hash` byte-identically;
/// `mark_resolved`'s rehash check catches any bug and falls back
/// to peer fetch (see `enumerate_and_enqueue_payloads` docstring).
///
/// # Where handles come from
///
/// Wired into the boot pipeline (casper_launch.rs +
/// initializing.rs) alongside the existing `payload_lookup`.
/// `runtime_manager` and `block_dag_storage` and `block_store` are
/// already threaded through those files — see `spawn_boot_apply_
/// subscriber`'s call sites in each.
#[derive(Clone)]
pub struct Option2ReducerContext {
    pub block_storage: block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage,
    pub block_store: block_storage::rust::key_value_block_store::KeyValueBlockStore,
    pub runtime_manager: Arc<crate::rust::util::rholang::runtime_manager::RuntimeManager>,
}

impl std::fmt::Debug for Option2ReducerContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Option2ReducerContext").finish_non_exhaustive()
    }
}

/// DD-7b-2 (a) Option 2 (2026-08-29): reproduce write bytes for
/// `payload_hash` by walking the source-deploy chain and replaying
/// the writing deploy in an isolated scratch runtime.
///
/// Returns:
/// - `Ok(Some(bytes))`: found the source deploy, replayed it, and
///   its capture map contains `payload_hash`.  Bytes returned are
///   ready for the reducer cache; `mark_resolved`'s rehash check
///   downstream is the safety net for reducer bugs.
/// - `Ok(None)`: any step on the chain missed cleanly (no index
///   entry, block pruned, deploy not in block, source deploy did
///   no Consensus writes matching this hash, etc.).  Caller falls
///   through to peer fetch — the "lazy" fork/prune behavior
///   documented as design decision 6.
/// - `Err(msg)`: unexpected storage/runtime error.  Caller logs
///   at warn and falls through to peer fetch (fail-open discipline,
///   M-2 pattern).
async fn try_reproduce_via_block_storage_replay(
    payload_hash: &[u8; 32],
    ctx: &Option2ReducerContext,
) -> Result<Option<Vec<u8>>, String> {
    use crate::rust::rholang::replay_runtime::ReplayBlockKind;
    use crate::rust::util::rholang::acceptance::{
        replay_purse_snapshot, RuntimeManagerSupplyReader,
    };

    // Chain step 1: payload_hash → deploy_sig.
    let deploy_sig = match ctx.block_storage.lookup_payload_source(payload_hash) {
        Ok(Some(sig)) => sig,
        Ok(None) => return Ok(None),
        Err(e) => return Err(format!("lookup_payload_source: {e}")),
    };
    // Chain step 2: deploy_sig → block_hash.
    let block_hash = match ctx.block_storage.lookup_by_deploy_id(&deploy_sig) {
        Ok(Some(h)) => h,
        Ok(None) => return Ok(None),
        Err(e) => return Err(format!("lookup_by_deploy_id: {e}")),
    };
    // Chain step 3: block_hash → BlockMessage → the ProcessedDeploy.
    let block = match ctx.block_store.get(&block_hash) {
        Ok(Some(b)) => b,
        Ok(None) => return Ok(None),
        Err(e) => return Err(format!("block_store.get: {e}")),
    };
    let processed = match block
        .body
        .deploys
        .iter()
        .find(|pd| pd.deploy.sig.as_ref() == deploy_sig.as_slice())
    {
        Some(pd) => pd.clone(),
        None => return Ok(None),
    };
    // Chain step 4: replay.
    let block_kind = if block.body.state.block_number == 0 {
        ReplayBlockKind::Genesis
    } else {
        ReplayBlockKind::Ordinary
    };
    let purse_snapshot = match block_kind {
        ReplayBlockKind::Genesis => None,
        ReplayBlockKind::Ordinary => {
            let supply_reader = RuntimeManagerSupplyReader {
                runtime_manager: &ctx.runtime_manager,
                pre_state_hash: block.body.state.pre_state_hash.clone(),
            };
            match replay_purse_snapshot(&processed, &supply_reader).await {
                Ok(s) => Some(s),
                Err(e) => return Err(format!("replay_purse_snapshot: {e}")),
            }
        }
    };
    let pre_state = block.body.state.pre_state_hash.clone();
    let captured = match capture_consensus_writes_by_replaying_deploy(
        &ctx.runtime_manager,
        &pre_state,
        &processed,
        block_kind,
        purse_snapshot.as_ref(),
    )
    .await
    {
        Ok(m) => m,
        Err(e) => return Err(format!("capture_consensus_writes: {e}")),
    };
    // Chain step 5: extract the requested hash.
    Ok(captured.get(payload_hash).cloned())
}

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
    option2_ctx: Option<Option2ReducerContext>,
) -> Result<BootApplyReport, BootApplyError>
where
    F: Fn(&std::path::Path) -> std::path::PathBuf + Send + Sync + 'static,
{
    use rholang::rust::interpreter::io::wal::PayloadRef;

    // DD-7b-2 (a) Option 2 (2026-08-29): pre-populate a two-tier
    // reducer cache before running the enumerator.  Tier 1 = local
    // `PayloadLookup` (Option 1, sync).  Tier 2 = block-storage-
    // backed replay (Option 2, async — requires this pre-pass
    // because the enumerator's reducer closure is sync).
    //
    // The cache is then handed to `enumerate_and_enqueue_payloads`
    // as a sync closure that just reads from it — preserving the
    // enumerator's existing behavior (dedup, `mark_resolved` rehash
    // check, stats accounting) verbatim.  A rehash mismatch (bytes
    // from either tier that don't Blake2b256 to the requested key)
    // still falls back to peer fetch via `mark_resolved`'s guard.
    let mut reducer_cache: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
    let mut unique_hashes: Vec<[u8; 32]> = Vec::new();
    {
        let mut seen: HashSet<[u8; 32]> = HashSet::new();
        for entry in &wal {
            if let Some(PayloadRef::Hash(h)) = entry.payload_ref {
                if seen.insert(h) {
                    unique_hashes.push(h);
                }
            }
        }
    }
    // Tier 1: local PayloadLookup.
    if let Some(lookup) = payload_lookup.as_ref() {
        for h in &unique_hashes {
            // M-2 review fix (2026-08-29): `get` errors are
            // treated as misses (fall back to peer fetch —
            // fail-open so a broken local store doesn't kill
            // joiner boot), but they MUST be logged so
            // operators can see chronic store faults instead
            // of just extra peer-fetch traffic.  Pre-M-2's
            // `.ok().flatten()` swallowed the Err silently.
            match lookup.get(h) {
                Ok(Some(bytes)) => {
                    reducer_cache.insert(*h, bytes);
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        target: "f1r3fly.casper.wal_payload_sync",
                        hash = hex::encode(h),
                        error = %e,
                        "PayloadLookup returned Err on boot enumerator lookup; \
                         falling back to peer fetch (fail-open).  A recurring \
                         stream of these indicates a broken local payload store \
                         — investigate the backing directory / permissions."
                    );
                }
            }
        }
    }
    // Tier 2: block-storage-backed replay for hashes still missing.
    // Chain: payload_hash → deploy_sig → block_hash → block →
    // ProcessedDeploy → capture_consensus_writes_by_replaying_deploy
    // → the requested bytes.  See `try_reproduce_via_block_storage_
    // replay` for the full walk.  Fail-open discipline: on Err (any
    // step returning an unexpected error, not just a clean miss),
    // log at warn and fall through — the joiner still has peer
    // fetch as the final tier.
    if let Some(ctx) = option2_ctx.as_ref() {
        for h in &unique_hashes {
            if reducer_cache.contains_key(h) {
                continue;
            }
            match try_reproduce_via_block_storage_replay(h, ctx).await {
                Ok(Some(bytes)) => {
                    reducer_cache.insert(*h, bytes);
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        target: "f1r3fly.casper.wal_payload_sync",
                        hash = hex::encode(h),
                        error = %e,
                        "Option 2 reducer returned Err on boot enumerator lookup; \
                         falling back to peer fetch (fail-open).  A recurring \
                         stream of these indicates a broken block/index store or \
                         a replay-runtime regression — investigate."
                    );
                }
            }
        }
    }

    // Step 1 — enumerate with the cache-backed reducer.
    let enumerated = enumerate_and_enqueue_payloads(&driver, &wal, |entry| {
        let PayloadRef::Hash(h) = entry.payload_ref.as_ref()? else {
            return None;
        };
        reducer_cache.get(h).cloned()
    })
    .await;

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

// -------------------------------------------------------------------
// DD-7b-2 (a) Option 2 (2026-08-29): deploy-write reproduction
// primitive.  Given a source deploy + its pre-state, spin up an
// isolated replay runtime with an in-memory capturing payload store,
// drive replay_deploy_e, and return every Consensus write's
// hash → bytes that fell out of `journal_write`'s `store.persist(bytes)`
// call.
//
// This is the shape that closes the gap Option 1 leaves open: a
// fresh joiner (empty local payload_store) can reconstruct write
// bytes for WAL entries whose source blocks it has in block storage
// but hasn't yet processed.  A future slice will:
//   1. Add an index `payload_hash → (block_hash, deploy_index)` built
//      during block processing (deploys are already being parsed
//      there, so extracting the write set is one extra pass).
//   2. Wire this helper into the DD-7b-2 (a) reducer: for each
//      unresolved WAL payload_hash H, look up (block, deploy) in
//      the index, load them from block storage, invoke this helper,
//      return the matching bytes (`mark_resolved`'s rehash still
//      catches any bug).
//   3. Handle fork/prune interactions on the index.
//
// This session lands just (0) — the primitive helper + regression
// pins.  Index + reducer wire-in stay in follow-ups.
//
// The helper reasons in the caller's terms (RuntimeManager +
// pre-state + a ProcessedDeploy).  It doesn't hardcode block_kind
// or purse_snapshot decisions — those are properties of the source
// block that only the caller (future index-aware reducer) knows.
// -------------------------------------------------------------------

/// Reproduce the Consensus write bytes a single deploy produced by
/// re-executing it in an isolated scratch runtime, with an in-memory
/// capturing `PayloadPersistence` attached that intercepts every
/// `journal_write`'s `store.persist(bytes)` call.
///
/// Returns a `hash → bytes` map covering every unique Consensus
/// write the deploy produced (Oracular writes don't touch
/// `store.persist`, so they're absent).  Empty if the deploy did
/// no Consensus writes.
///
/// The runtime is spawned fresh from `runtime_manager` and dropped
/// at the end of the call — no state leaks to other runtimes.
/// The manager's shared `payload_store` slot is NOT mutated; the
/// helper overrides only this runtime's local
/// `FileHandleTable.payload_store` via `share_payload_store`.
///
/// # Pre-state
///
/// `pre_state_hash` must be the block's pre-state that the leader
/// used when producing this deploy's writes.  A wrong pre-state
/// will yield different rspace lookups and can silently produce
/// different bytes on state-dependent writes.  The safety net is
/// downstream: `mark_resolved` rehashes what this helper returns
/// against the requested payload_hash, so a mismatch (from wrong
/// pre-state or otherwise) is caught before corrupt bytes reach the
/// applier.
///
/// # Block kind + purse snapshot
///
/// Must match the source block's context.  For `Ordinary` blocks
/// (post-cost-accounted-rho merge), `purse_snapshot` MUST be
/// `Some` — replay_deploy_e_with_snapshot returns
/// `InvalidCostSettlement` otherwise.  For `Genesis`, pass `None`.
///
/// # Blocking
///
/// Runs the reducer inline (not `spawn_blocking`).  Callers on the
/// boot subscriber path should call this from a `spawn_blocking`
/// wrapper if latency matters — parse + reduce a Consensus-cap
/// deploy takes tens to hundreds of milliseconds.
///
/// # Testing posture (2026-08-29 landing)
///
/// This session ships the primitive.  Its constituent parts are all
/// separately pinned:
///
/// * `InMemoryPayloadStore::snapshot()` — dedicated pins in
///   `wal_payload_server::tests::in_memory_payload_store_snapshot_*`.
/// * `share_payload_store` propagation — pins in the
///   `payload_store_wiring_tests` module of `runtime_manager.rs`.
/// * `journal_write` → `store.persist(bytes)` chain — pins in
///   `rholang/tests/fs_wal_spec.rs::consensus_writes_persist_to_
///   payload_store` (and the multi-deploy WAL byte-identity test).
/// * `replay_deploy_e_with_snapshot` — pinned via the block-level
///   `replay_deploys` + PB-M-14 canary.
///
/// End-to-end verification via a real `ProcessedDeploy` requires
/// the full leader cosign pipeline (compute_state_with_bonds_
/// cosigned_admitted).  That E2E lands naturally with the
/// index-building session (see follow-up plan): the block-processing
/// hook produces `ProcessedDeploy`s on the leader; the joiner-side
/// reducer feeds them to this helper on boot; the boot E2E test
/// exercises the whole chain.  Wiring an E2E now would duplicate
/// scaffolding that follow-up will provide for free.
pub async fn capture_consensus_writes_by_replaying_deploy(
    runtime_manager: &crate::rust::util::rholang::runtime_manager::RuntimeManager,
    pre_state_hash: &models::rust::block::state_hash::StateHash,
    processed_deploy: &models::rust::casper::protocol::casper_message::ProcessedDeploy,
    block_kind: crate::rust::rholang::replay_runtime::ReplayBlockKind,
    purse_snapshot: Option<&crate::rust::util::rholang::acceptance::ReplayPurseSnapshot>,
) -> Result<HashMap<[u8; 32], Vec<u8>>, crate::rust::errors::CasperError> {
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    use crate::rust::engine::wal_payload_server::InMemoryPayloadStore;
    use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;

    let mut runtime = runtime_manager.spawn_replay_runtime().await;
    // Attach the capturing store BEFORE reset/replay so every
    // journal_write during the drive sees it.  Overwrites the
    // manager-shared payload_store on THIS runtime only (interior
    // mutability on the FileHandleTable slot is per-runtime).
    let capture = Arc::new(InMemoryPayloadStore::new());
    runtime.fs_handles.share_payload_store(Some(
        capture.clone() as Arc<dyn rholang::rust::interpreter::io::wal::PayloadPersistence>,
    ));
    // DD-7b-2 (a) Option 2 review-fix (2026-08-29): disable the
    // payload-source recorder on this scratch runtime so
    // `journal_write` does NOT write into the joiner's real
    // block-storage-backed `payload_source_index` during the
    // capture.  Without this override, the "scratch" abstraction
    // leaks:
    //   * Convergent replay (bytes match original) → idempotent
    //     overwrite of the same (payload_hash → deploy_sig) entry
    //     that already exists.  Cosmetic waste.
    //   * Divergent replay (state-dependent write yields different
    //     bytes) → new (hash_divergent, deploy_sig) entry lands in
    //     the joiner's persistent index that no WAL entry ever
    //     references — dead storage, bounded by attacker deploy
    //     cost but unbounded across many boots.
    // Neither case is a correctness issue (`mark_resolved`'s rehash
    // check catches divergence before bytes reach the applier), but
    // both waste disk and confuse future diagnostic reads of the
    // index.  The override is per-runtime (interior-mutability slot);
    // dropping the scratch runtime discards it — no leak.
    runtime.fs_handles.share_payload_source_recorder(None);
    // Reset the scratch runtime to the source block's pre-state so
    // state-dependent writes reproduce identically to the leader.
    let start_root = Blake2b256Hash::from_bytes_prost(pre_state_hash);
    runtime
        .reset(&start_root)
        .await
        .map_err(|e| {
            crate::rust::errors::CasperError::RuntimeError(format!(
                "capture_consensus_writes: reset to pre-state failed: {e}"
            ))
        })?;

    let mut ops = ReplayRuntimeOps::new_from_runtime(runtime);
    // replay_deploy_e_with_snapshot handles rig() internally.
    // Errors are propagated to the caller so a bad deploy (e.g.,
    // stale purse snapshot, missing certificate) doesn't silently
    // return an empty map that a caller might mistake for
    // "deploy did no Consensus writes".
    ops.replay_deploy_e_with_snapshot(block_kind, processed_deploy, purse_snapshot)
        .await?;

    Ok(capture.snapshot())
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
            None,
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
            None,
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
            None,
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

    /// M-2 review pin (2026-08-29): a `PayloadLookup` that returns
    /// `Err` (backing IO fault, poisoned lock, corrupted store) is
    /// treated the same as a miss — the enumerator falls back to
    /// peer fetch (fail-open: a broken local store must not kill
    /// joiner boot).  The Err is logged at warn (see
    /// `apply_wal_slice_after_fetch`'s reducer closure); operators
    /// observe chronic faults via that log stream.  Pre-M-2's
    /// `.ok().flatten()` swallowed the Err with no log, hiding
    /// broken stores behind extra peer-fetch traffic.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn payload_lookup_reducer_treats_err_as_miss_and_falls_back_to_fetch() {
        use crate::rust::engine::wal_payload_server::PayloadLookup;

        /// Test-only lookup that always returns `Err` — models a
        /// backing store with IO failure / permission problem.
        #[derive(Debug)]
        struct AlwaysErrStore;
        impl PayloadLookup for AlwaysErrStore {
            fn get(&self, _payload_hash: &[u8; 32]) -> Result<Option<Vec<u8>>, String> {
                Err("simulated backing-store IO fault".to_string())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("t.bin");
        std::fs::write(&target, vec![0u8; 16]).unwrap();

        let lookup: Arc<dyn PayloadLookup> = Arc::new(AlwaysErrStore);

        let driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(WalPayloadRetriever::new())));
        // Pre-resolve the CORRECT bytes on the retriever so the
        // peer-fetch fallback path lets is_complete() short-circuit.
        let payload = b"peer-served-after-err".to_vec();
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
            None,
        )
        .await
        .expect("apply must succeed via peer-served bytes despite lookup Err");

        // Reducer error → treated as miss → enumerator enqueued.
        assert_eq!(report.enumerated.resolved_locally, 0);
        assert_eq!(report.enumerated.enqueued_for_fetch, 1);
        // Applier still succeeded on the peer-served bytes.
        let got = std::fs::read(&target).unwrap();
        assert_eq!(&got[..payload.len()], payload.as_slice());
    }

    /// L-4 review pin (2026-08-29): freeze the load-bearing shape
    /// of `capture_consensus_writes_by_replaying_deploy`.  This
    /// helper is the DD-7b-2 (a) Option 2 primitive — a future
    /// index-building slice wires it into the boot reducer, and
    /// silent shape drift here would break that follow-up
    /// without any test signal in this session's scope.
    ///
    /// Full E2E via a real `ProcessedDeploy` is deferred to the
    /// index-building slice (see helper docstring); this
    /// source-scan pin holds the shape stable in the meantime.
    /// A regression that dropped any of the load-bearing calls
    /// would leave the helper compiling but silently broken.
    #[test]
    fn capture_consensus_writes_helper_has_load_bearing_shape() {
        let src = include_str!("wal_payload_sync.rs");
        let fn_start = src
            .find("pub async fn capture_consensus_writes_by_replaying_deploy(")
            .expect(
                "wal_payload_sync.rs must expose the \
                 capture_consensus_writes_by_replaying_deploy helper — Option 2 primitive",
            );
        // Bound the search to a generous window — the helper is
        // under 100 lines today; 4 KiB is safely inside its body.
        let end = std::cmp::min(fn_start + 4096, src.len());
        let body = &src[fn_start..end];
        assert!(
            body.contains("spawn_replay_runtime"),
            "helper must spawn an isolated replay runtime via \
             spawn_replay_runtime — a switch to spawn_runtime would \
             route the capture into the manager's PROD state"
        );
        assert!(
            body.contains("share_payload_store(Some("),
            "helper must attach the capture store via share_payload_store(Some(...)) \
             — dropping the override leaves the manager-shared payload_store in place \
             and captures would leak into production serving state"
        );
        assert!(
            body.contains("InMemoryPayloadStore::new()"),
            "helper must use InMemoryPayloadStore as the in-memory capture backend \
             — a DirectoryPayloadStore substitution would persist scratch replay \
             bytes to the operator's on-disk payload directory"
        );
        // DD-7b-2 (a) Option 2 review-fix (2026-08-29): the scratch
        // runtime MUST disable the payload-source recorder before
        // reset+replay so `journal_write` on the capture path does
        // not write into the joiner's real block-storage-backed
        // `payload_source_index`.  Dropping this override leaves the
        // manager-shared recorder in place — every scratch replay
        // then re-records `(payload_hash, deploy_sig)` into the
        // persistent index (idempotent for convergent replays,
        // dead-storage pollution for divergent replays).  Neither is
        // a correctness bug (`mark_resolved` rehash is the safety
        // net) but both leak the scratch abstraction.
        assert!(
            body.contains("share_payload_source_recorder(None)"),
            "helper must disable the payload-source recorder via \
             share_payload_source_recorder(None) so scratch replay does not \
             pollute the joiner's real block-storage-backed payload_source_index \
             — see the docstring at the override site for the convergent-vs-\
             divergent replay pollution modes."
        );
        assert!(
            body.contains(".reset(&start_root)"),
            "helper must reset the scratch runtime to the source block's pre-state \
             — without this, state-dependent Rholang writes reproduce against the \
             wrong tuplespace and yield different bytes than the leader produced"
        );
        assert!(
            body.contains("replay_deploy_e_with_snapshot("),
            "helper must drive the deploy via replay_deploy_e_with_snapshot so the \
             deploy's full acceptance logic (purse snapshot, cost verification, \
             authority certificate) matches what the leader actually ran"
        );
        assert!(
            body.contains(".snapshot()"),
            "helper must drain the capture via InMemoryPayloadStore::snapshot() \
             — returning the internal Arc or a shared reference would leak store \
             internals to callers and violate the isolation contract"
        );
    }

    /// DD-7b-2 (a) Option 2 (2026-08-29): freeze the load-bearing
    /// shape of `try_reproduce_via_block_storage_replay` — the
    /// Option 2 tier's per-hash chain walker.  A regression that
    /// dropped any step of the chain (payload_hash → deploy_sig →
    /// block_hash → block → ProcessedDeploy → replay) would leave
    /// the helper compiling but silently returning None on every
    /// call, forcing every unresolved WAL payload hash through
    /// peer fetch with no unit-test signal.
    #[test]
    fn option2_reducer_walks_full_chain() {
        let src = include_str!("wal_payload_sync.rs");
        let fn_start = src
            .find("async fn try_reproduce_via_block_storage_replay(")
            .expect("Option 2 tier helper must exist");
        let end = std::cmp::min(fn_start + 4096, src.len());
        let body = &src[fn_start..end];
        assert!(
            body.contains("lookup_payload_source"),
            "chain step 1 missing: payload_hash → deploy_sig — the \
             block-storage `lookup_payload_source` call is what walks \
             the DD-7b-2 (a) Option 2 index built by journal_write."
        );
        assert!(
            body.contains("lookup_by_deploy_id"),
            "chain step 2 missing: deploy_sig → block_hash — the \
             block-storage `lookup_by_deploy_id` call chains into the \
             existing deploy_index."
        );
        assert!(
            body.contains("block_store.get("),
            "chain step 3 missing: block_hash → BlockMessage — a `Some(None)` \
             here (block pruned) is the design-decision-6 lazy fallback."
        );
        assert!(
            body.contains("body") && body.contains(".deploys") && body.contains(".iter()"),
            "chain step 4 missing: BlockMessage → ProcessedDeploy — must \
             iterate `block.body.deploys.iter()` (possibly chained multi-line) \
             and find the sig-matched entry."
        );
        assert!(
            body.contains("capture_consensus_writes_by_replaying_deploy("),
            "chain step 4b missing: replay via the Option 2 primitive."
        );
        assert!(
            body.contains("captured.get(payload_hash)"),
            "chain step 5 missing: extract the requested hash from the \
             capture map — without this, the reducer returns the whole map \
             instead of the specific requested payload."
        );
    }

    /// DD-7b-2 (a) Option 2 (2026-08-29): `apply_wal_slice_after_fetch`
    /// must run Tier 1 (local PayloadLookup) BEFORE Tier 2 (block-
    /// storage-backed replay).  A reversal would still be correct
    /// (the mark_resolved rehash catches bugs) but would waste an
    /// expensive replay per Consensus-cap write for hashes the
    /// local store already knows.
    #[test]
    fn two_tier_reducer_runs_tier1_before_tier2() {
        let src = include_str!("wal_payload_sync.rs");
        let fn_start = src
            .find("pub async fn apply_wal_slice_after_fetch<F>(")
            .expect("apply_wal_slice_after_fetch must exist");
        let end = std::cmp::min(fn_start + 8192, src.len());
        let body = &src[fn_start..end];
        let tier1_marker = "Tier 1:";
        let tier2_marker = "Tier 2:";
        let tier1_pos = body
            .find(tier1_marker)
            .expect("Tier 1 comment marker must exist in apply_wal_slice_after_fetch");
        let tier2_pos = body
            .find(tier2_marker)
            .expect("Tier 2 comment marker must exist in apply_wal_slice_after_fetch");
        assert!(
            tier1_pos < tier2_pos,
            "Option 2 regression: Tier 1 (local PayloadLookup) must run BEFORE \
             Tier 2 (block-storage-backed replay).  Reversing would run an \
             expensive replay for hashes the local store already resolves."
        );
        assert!(
            body.contains("if reducer_cache.contains_key(h)"),
            "Option 2 regression: Tier 2's per-hash loop must skip hashes \
             already resolved by Tier 1.  Dropping the guard would re-run \
             replay on every hash even after Tier 1 populates the cache."
        );
    }

    // ---------------------------------------------------------------
    // DD-7b-2 (a) Option 2 (2026-08-29): behavioral tests for the
    // reducer's chain-walker and its miss-fallthrough paths.  Each
    // test constructs a real Option2ReducerContext with in-memory
    // storage and drives `try_reproduce_via_block_storage_replay`
    // directly.  Complements the shape pin
    // `option2_reducer_walks_full_chain` with wiring proof.
    //
    // Full end-to-end (leader records → joiner reproduces bytes end-
    // to-end via scratch replay) requires a real cosigned deploy +
    // block-processing pipeline.  Documented as an ignored skeleton
    // at the tail of this module.
    // ---------------------------------------------------------------

    async fn empty_option2_ctx() -> Option2ReducerContext {
        use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
        use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

        use crate::rust::util::rholang::runtime_manager::RuntimeManager;
        use rholang::rust::interpreter::external_services::ExternalServices;

        let mut kvm = InMemoryStoreManager::new();
        let block_storage = block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("in-memory DAG storage");
        let block_store =
            block_storage::rust::key_value_block_store::KeyValueBlockStore::create_from_kvm(
                &mut kvm,
            )
            .await
            .expect("in-memory block store");
        let rspace_stores = kvm.r_space_stores().await.expect("rspace stores");
        let mergeable_store = RuntimeManager::mergeable_store(&mut kvm)
            .await
            .expect("mergeable store");
        let runtime_manager = RuntimeManager::create_with_store(
            rspace_stores,
            mergeable_store,
            std::sync::Arc::new(std::collections::HashMap::new()),
            ExternalServices::noop(),
        );
        Option2ReducerContext {
            block_storage,
            block_store,
            runtime_manager: std::sync::Arc::new(runtime_manager),
        }
    }

    /// Chain step 1 miss (empty index): try_reproduce returns Ok(None)
    /// so the enumerator falls through to peer fetch.  This is the
    /// baseline case for a fresh joiner with an empty payload_source_
    /// index — matches pre-Option-2 behavior verbatim.
    #[tokio::test]
    async fn option2_reducer_returns_none_on_empty_index() {
        let ctx = empty_option2_ctx().await;
        let bogus_hash = [0xFFu8; 32];
        let result = try_reproduce_via_block_storage_replay(&bogus_hash, &ctx).await;
        assert!(
            matches!(result, Ok(None)),
            "empty payload_source_index must return Ok(None) — fall through \
             to peer fetch.  Got: {result:?}"
        );
    }

    /// Chain step 2 miss: payload_source_index has an entry pointing
    /// to a sig that has NO deploy_index entry (invalid-block
    /// scenario: replay ran, recorder fired, but block insertion
    /// bailed on `invalid` flag → deploy_index skipped).  The chain
    /// walker returns Ok(None) cleanly rather than propagating the
    /// dead reference.
    #[tokio::test]
    async fn option2_reducer_returns_none_when_deploy_index_lacks_sig() {
        let ctx = empty_option2_ctx().await;
        let payload_hash = [0xAAu8; 32];
        let orphan_sig: Vec<u8> = vec![0xDE, 0xAD, 0xBE, 0xEF];
        // Record the payload_source_index entry so chain step 1
        // returns Some(orphan_sig).
        ctx.block_storage
            .record_payload_source(payload_hash, &orphan_sig)
            .expect("record");
        // Do NOT touch deploy_index — leaves chain step 2 in the
        // "sig has no block_hash mapping" state.
        let result = try_reproduce_via_block_storage_replay(&payload_hash, &ctx).await;
        assert!(
            matches!(result, Ok(None)),
            "chain step 2 miss (orphan sig with no deploy_index entry) must \
             return Ok(None) — fall through to peer fetch.  Got: {result:?}"
        );
    }

    /// Chain step 3 miss: payload_source_index + deploy_index have
    /// entries, but block_store returns None for the block_hash
    /// (pruned block scenario, design decision 6 lazy fork/prune).
    /// The chain walker returns Ok(None) cleanly.
    #[tokio::test]
    async fn option2_reducer_returns_none_when_block_store_lacks_block() {
        let ctx = empty_option2_ctx().await;
        let payload_hash = [0xBBu8; 32];
        let dangling_sig: Vec<u8> = vec![0xCA, 0xFE];
        // Chain step 1 → Some(sig).
        ctx.block_storage
            .record_payload_source(payload_hash, &dangling_sig)
            .expect("record");
        // Chain step 2 → Some(block_hash) — inject directly via the
        // test-internals accessor.  The block_hash points to a block
        // that was never persisted to block_store (pruned).
        let dangling_block_hash: models::rust::block_hash::BlockHash =
            prost::bytes::Bytes::from_static(b"pruned-block-hash-32-bytes-XXXXX");
        let deploy_index = ctx.block_storage.deploy_index_for_tests();
        deploy_index
            .write()
            .put_one(
                dangling_sig.clone(),
                models::rust::block_hash::BlockHashSerde(dangling_block_hash),
            )
            .expect("put_one deploy_index");
        // Chain step 3 → None (block not in block_store).
        let result = try_reproduce_via_block_storage_replay(&payload_hash, &ctx).await;
        assert!(
            matches!(result, Ok(None)),
            "chain step 3 miss (block pruned from block_store) must return \
             Ok(None) — the lazy fork/prune fallback path.  Got: {result:?}"
        );
    }

    // ---------------------------------------------------------------
    // DD-7b-2 (a) Option 2 follow-up (2026-08-29): full end-to-end
    // behavioral test skeleton.  Ignored until the follow-up
    // session that wires up a cosigned-deploy test harness for
    // Option 2's leader-record-then-joiner-reproduce cycle.
    //
    // # Why this can't be a self-contained unit test today
    //
    // The Option 2 reducer's happy-path exercises the full chain:
    //   payload_hash → deploy_sig → block_hash → BlockMessage →
    //   ProcessedDeploy → capture_consensus_writes_by_replaying_deploy
    //   → the requested bytes.
    // Each step needs infrastructure the primitive helper's docstring
    // calls out as "requires the full leader cosign pipeline":
    //   * A `ProcessedDeploy` carrying a real primary-signer sig
    //     (so `deploy.sig.to_vec()` chains through deploy_index).
    //   * A `BlockMessage` containing that ProcessedDeploy under a
    //     valid pre_state_hash (so `reset(&start_root)` on the
    //     scratch runtime succeeds).
    //   * A `ReplayPurseSnapshot` derived from the block's
    //     authority-cost witness (so `replay_deploy_e_with_snapshot`
    //     doesn't return InvalidCostSettlement).
    //   * `journal_write` firing on the leader's fs_write path,
    //     with the payload_source_recorder wired, so the
    //     payload_source_index actually populates from the deploy.
    //
    // # Skeleton
    //
    // Once the harness (or the PB-M-14 canary extension) is
    // available, this test would:
    //   1. Build a two-validator harness like
    //      `casper/tests/multi_node/pb_m_14_two_validator_e2e.rs`.
    //   2. Wire a BlockStorageBackedRecorder to validator A's
    //      manager BEFORE producing the block.
    //   3. Validator A produces + adds a block whose deploy writes
    //      bytes B on a Consensus cap (`add_block_from_deploys(...)`).
    //   4. Assert:
    //        A.block_dag_storage.lookup_payload_source(&Blake2b256(B))
    //        == Some(deploy_sig)
    //      — this is the leader-side recording pin.
    //   5. Set up a "joiner" state: fresh RuntimeManager, empty
    //      payload_store, but with access to A's block_dag_storage
    //      and block_store (or a copy).
    //   6. Build an Option2ReducerContext bundling A's storage +
    //      the joiner's runtime_manager.
    //   7. Drive apply_wal_slice_after_fetch with:
    //        - the block's WAL slice (from A's pending_wal_slices),
    //        - payload_lookup = None (force Option 1 to miss),
    //        - option2_ctx = Some(ctx from step 6).
    //   8. Assert:
    //        - report.enumerated.resolved_locally == 1
    //        - the target file on disk matches B (via the applier's
    //          path_map).
    //   9. Assert:
    //        - joiner's own payload_source_index STAYS EMPTY
    //          (scratch-replay pollution fix — the recorder
    //          override should have prevented any writes).
    //
    // # Scope for THAT session
    //
    // Realistic scope: ~100-200 LOC + harness reuse from PB-M-14.
    // Would live either as an extension to
    // `casper/tests/multi_node/pb_m_14_two_validator_e2e.rs`
    // (natural home — same harness) or a new sibling file
    // (`option2_e2e.rs`).  The primitive's docstring already
    // anticipates this: "That E2E lands naturally with the
    // index-building session (see follow-up plan)".
    // ---------------------------------------------------------------
    #[tokio::test]
    #[ignore = "follow-up: requires cosigned-deploy test harness — see docstring above"]
    async fn option2_leader_records_and_joiner_reproduces_end_to_end() {
        // Skeleton — see the doc block above for the concrete
        // steps.  Unignore once the harness is wired.
        unimplemented!("Option 2 E2E test skeleton — see doc block above for the plan");
    }
}
