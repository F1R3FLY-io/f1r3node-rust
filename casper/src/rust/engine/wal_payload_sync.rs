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

/// Insert into the blacklist with a size cap.  Silent no-op past
/// the cap.
fn add_blacklist_capped(set: &mut HashSet<PeerNode>, peer: PeerNode) {
    if set.len() < MAX_BLACKLISTED {
        set.insert(peer);
    }
}

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
    blacklisted: Arc<RwLock<HashSet<PeerNode>>>,
}

impl WalPayloadSyncDriver {
    pub fn new(retriever: Arc<WalPayloadRetriever>) -> Self {
        Self {
            retriever,
            per_hash_sources: Arc::new(RwLock::new(HashMap::new())),
            blacklisted: Arc::new(RwLock::new(HashSet::new())),
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
                add_blacklist_capped(&mut b, sender);
                return;
            }
        };
        // Skip if sender is already blacklisted.
        {
            let b = self.blacklisted.read().await;
            if b.contains(&sender) {
                return;
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
                add_blacklist_capped(&mut b, sender);
                false
            }
        }
    }

    /// Pick the next non-blacklisted source for a payload_hash,
    /// rotating the FIFO.  Returns None if no source is available.
    async fn next_source_for(&self, hash: &[u8; 32]) -> Option<PeerNode> {
        let blacklist_snap: HashSet<PeerNode> = self.blacklisted.read().await.clone();
        let mut g = self.per_hash_sources.write().await;
        let sources = g.get_mut(hash)?;
        let n = sources.sources.len();
        for _ in 0..n {
            let peer = sources.sources.pop_front()?;
            sources.sources.push_back(peer.clone());
            if !blacklist_snap.contains(&peer) {
                return Some(peer);
            }
        }
        None
    }

    /// Send an outbound tick: for each pending payload, either
    /// broadcast a HasWalPayloadRequest (if we haven't yet), or
    /// send GetWalPayloadRequest for it to the next available
    /// source.  Also handles timeout-driven retries.
    pub async fn tick<T: TransportLayer + Send + Sync>(
        &self,
        transport: &T,
        conf: &RPConf,
        connections_cell: &comm::rust::rp::connect::ConnectionsCell,
    ) {
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

            // Skip if already in flight (last_request_ms > 0 and
            // still within retry budget).
            let already_in_flight = {
                let g = self.retriever.payloads.read().await;
                g.get(&hash)
                    .map(|s| s.last_request_ms > 0 && s.retry_count < MAX_RETRIES)
                    .unwrap_or(true)
            };
            if already_in_flight {
                continue;
            }

            if let Err(e) = send_get_wal_payload_request(transport, conf, &source, &hash).await {
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

    /// Query: number of pending payloads.
    pub async fn pending_count(&self) -> usize { self.retriever.pending_count().await }

    /// Query: is all pending work complete?
    pub async fn is_complete(&self) -> bool { self.retriever.is_complete().await }

    /// Retrieve verified bytes for a hash, if resolved.
    pub async fn take_bytes(&self, payload_hash: &[u8; 32]) -> Option<Vec<u8>> {
        self.retriever.get_bytes(payload_hash).await
    }
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

/// Enumerate WAL entries in a captured WAL slice, extract the
/// unique `payload_ref: Hash(...)` values, and enqueue any that
/// the joiner cannot reproduce locally via the write-payload-
/// determinism reducer.
///
/// The `is_reproducible` predicate is passed by the caller — it
/// answers "can we reconstruct these bytes locally by replaying
/// deploy data + deterministic Rholang?"  For the initial slice,
/// callers can pass `|_| false` (fetch everything); a follow-up
/// slice will wire the actual reducer.
///
/// Returns the number of payload hashes newly enqueued.
pub async fn enumerate_and_enqueue_payloads<F>(
    driver: &WalPayloadSyncDriver,
    wal_slice: &[rholang::rust::interpreter::io::wal::WalEntry],
    is_reproducible: F,
) -> usize
where
    F: Fn(&[u8; 32]) -> bool,
{
    use rholang::rust::interpreter::io::wal::PayloadRef;
    let mut seen: HashSet<[u8; 32]> = HashSet::new();
    let mut enqueued = 0;
    for entry in wal_slice {
        if let Some(PayloadRef::Hash(h)) = entry.payload_ref {
            if seen.insert(h) && !is_reproducible(&h) {
                driver.enqueue_payload(h).await;
                enqueued += 1;
            }
        }
    }
    if enqueued > 0 {
        info!(
            target: "f1r3fly.casper.wal_payload_sync",
            enqueued,
            "boot-time enumerator enqueued payloads for fetch"
        );
    }
    enqueued
}

/// Default tick period between outbound-request rounds.  Matches
/// snapshot_chunk_sync's TICK_PERIOD_MS so operators have one knob.
pub const TICK_PERIOD_MS: u64 = 5_000;

/// Spawn a periodic tick task that calls `WalPayloadSyncDriver::tick`
/// every `TICK_PERIOD_MS`.  Returns the JoinHandle so the caller
/// can abort on shutdown.
pub fn spawn_periodic_tick<T>(
    driver: Arc<WalPayloadSyncDriver>,
    transport: Arc<T>,
    conf: RPConf,
    connections_cell: comm::rust::rp::connect::ConnectionsCell,
) -> tokio::task::JoinHandle<()>
where
    T: TransportLayer + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(TICK_PERIOD_MS));
        // Skip the immediate first tick — give the boot enumerator
        // a chance to populate the driver before we start beating.
        interval.tick().await;
        loop {
            interval.tick().await;
            if driver.pending_count().await == 0 {
                continue;
            }
            driver.tick(&*transport, &conf, &connections_cell).await;
        }
    })
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
        assert!(b.contains(&peer));
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
        // Blacklist alice.
        driver.blacklisted.write().await.insert(alice.clone());
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
        // Mark "b" as reproducible; others should be enqueued.
        let hash_b = hash_of(b"b");
        let enqueued = enumerate_and_enqueue_payloads(&driver, &entries, |h| *h == hash_b).await;
        assert_eq!(enqueued, 2);
        assert_eq!(driver.pending_count().await, 2);
    }

    /// Placeholder: exercise InMemoryPayloadStore integration for
    /// smoke coverage.
    #[tokio::test]
    async fn in_memory_store_round_trip_via_driver() {
        let store = InMemoryPayloadStore::new();
        let payload = b"driver-e2e".to_vec();
        let h = store.insert(payload.clone()).await;

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
}
