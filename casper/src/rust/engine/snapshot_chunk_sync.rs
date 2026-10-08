// Phase 7b-1 snapshot chunk-fetch joiner sync-driver (2026-08-27).
//
// Orchestrates the client side of the snapshot chunk-fetch
// protocol from a joining validator's perspective.  On boot the
// joiner enumerates finalized blocks whose snapshots it lacks
// locally, and for each one it:
//
//   1. Broadcasts a HasSnapshotRequest to discover peers with the
//      snapshot.
//   2. On each HasSnapshot reply, pins the merkle_root + chunk_count
//      target (if we haven't already), records the peer as a source,
//      and starts issuing GetSnapshotChunkRequests round-robin
//      across known sources.
//   3. On each SnapshotChunkResponse, hands the response to the
//      matching SnapshotChunkRetriever; on ChunkAccepted, moves
//      on to the next pending index.
//   4. When the retriever completes (`is_complete`), assembles the
//      bytes and writes them to the local snapshot dir; the joiner's
//      main sync loop can then hand them to the Phase-7-WAL replay
//      path.
//
// Also handles:
//   * Timeout-driven retries (`retry_tick`): scans for chunks whose
//     last request has aged past REQUEST_TIMEOUT_MS and re-queues
//     them to alternative peers.
//   * Peer rotation: on a byzantine/malformed response, marks the
//     sending peer as failed for this snapshot and rotates to
//     the next known source.
//   * Stale-eviction: a retriever whose initial request is older
//     than STALE_EVICTION_MS is dropped (joiner should re-broadcast
//     HasSnapshotRequest to refresh source list).
//
// Not covered here (production shim work):
//   * TransportLayer wiring — this module is transport-agnostic;
//     the caller passes a T: TransportLayer to `tick`.
//   * Peer discovery beyond HasSnapshot broadcast responses.
//   * Cryptographic peer authentication of chunk responses (the
//     retriever's Merkle-proof check is the load-bearing check;
//     peer identity is a spam-defense layer for a follow-up slice).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use comm::rust::peer_node::PeerNode;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::casper::protocol::casper_message::{HasSnapshot, SnapshotChunkResponse};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::rust::engine::snapshot_chunk_retriever::{
    AdmitOutcome, SnapshotChunkRetriever, SnapshotTarget, MAX_RETRIES,
};
use crate::rust::engine::snapshot_chunk_wire::{
    broadcast_has_snapshot_request, send_get_snapshot_chunk_request,
};

/// Security caps on per-snapshot peer sets.  Under normal use both
/// stay well below these limits (typical joiner sees ~10 sources
/// and blacklists ~0); the caps defend against an attacker
/// rotating peer identities to exhaust memory.
pub const MAX_SOURCES_PER_SNAPSHOT: usize = 256;
pub const MAX_BLACKLISTED_PER_SNAPSHOT: usize = 1024;

/// Per-snapshot sync state: retriever + known peers + round-robin
/// cursor for peer rotation.
#[derive(Debug)]
pub struct SnapshotSyncState {
    pub retriever: Arc<SnapshotChunkRetriever>,
    /// Peers known to have this snapshot (from HasSnapshot replies).
    /// FIFO for round-robin selection.
    pub sources: VecDeque<PeerNode>,
    /// Peers that produced malformed responses for this snapshot;
    /// skipped on rotation.
    pub blacklisted_sources: HashSet<PeerNode>,
    /// Whether we've already broadcast the initial
    /// HasSnapshotRequest.  Avoids duplicate broadcasts on repeat
    /// ticks before we've seen any HasSnapshot replies.
    pub broadcasted_has_request: bool,
}

/// Insert into the per-snapshot blacklist with a size cap.
/// Silent no-op once the cap is reached — the pathological case
/// where an attacker has flooded us with `MAX_BLACKLISTED_PER_SNAPSHOT`
/// distinct malicious identities is exceedingly rare; degrading
/// to "we can't blacklist more" is preferable to unbounded memory.
fn add_blacklist_capped(set: &mut HashSet<PeerNode>, peer: PeerNode) {
    if set.len() < MAX_BLACKLISTED_PER_SNAPSHOT {
        set.insert(peer);
    }
}

impl SnapshotSyncState {
    /// Pick the next non-blacklisted source, rotating the FIFO so
    /// consecutive picks land on different peers.  Returns None if
    /// no source is available.
    pub fn next_source(&mut self) -> Option<PeerNode> {
        let n = self.sources.len();
        for _ in 0..n {
            let peer = self.sources.pop_front()?;
            self.sources.push_back(peer.clone());
            if !self.blacklisted_sources.contains(&peer) {
                return Some(peer);
            }
        }
        None
    }
}

/// Phase 7b-2 item (c) (2026-08-28): notification emitted when a
/// snapshot has been fully assembled + written to disk.  Consumers
/// wire this into the WAL apply-to-follower flow via a channel:
/// `casper_launch` / `initializing` install an unbounded mpsc
/// sink at boot; the sync driver fires one message per completed
/// snapshot; a spawned reader decodes the snapshot bytes and
/// invokes `apply_wal_slice_after_fetch` against the WAL payload
/// driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotCompletion {
    pub block_hash: Vec<u8>,
    pub atomic_root: [u8; 32],
    pub path: PathBuf,
}

/// The joiner-side driver.  Owns a per-snapshot sync-state map
/// keyed by block hash.
#[derive(Debug, Clone)]
pub struct SnapshotChunkSyncDriver {
    snapshots: Arc<RwLock<HashMap<Vec<u8>, SnapshotSyncState>>>,
    /// Local snapshot cache directory — where assembled bytes get
    /// written.
    snapshot_dir: PathBuf,
    /// Optional mpsc sink for per-completion notifications
    /// (Phase 7b-2 item (c), 2026-08-28).  Install-once at boot
    /// via `install_completion_sink`.  If Some, `on_chunk_response`
    /// sends a `SnapshotCompletion` after each snapshot is
    /// assembled + written to disk.
    completion_sink:
        Arc<std::sync::RwLock<Option<tokio::sync::mpsc::UnboundedSender<SnapshotCompletion>>>>,
}

impl SnapshotChunkSyncDriver {
    pub fn new(snapshot_dir: PathBuf) -> Self {
        Self {
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            snapshot_dir,
            completion_sink: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Phase 7b-2 item (c) (2026-08-28): install an mpsc sink that
    /// receives `SnapshotCompletion` messages each time
    /// `on_chunk_response` finishes writing an assembled snapshot
    /// to disk.  Install-once at boot; a second call overwrites
    /// (documented; no current caller re-installs).
    ///
    /// The driver holds a `sync` RwLock (not tokio) so
    /// `on_chunk_response` can take a snapshot of the sender
    /// without an await-point in the middle of the receive path.
    pub fn install_completion_sink(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<SnapshotCompletion>,
    ) {
        let mut g = self
            .completion_sink
            .write()
            .expect("completion_sink poisoned");
        *g = Some(tx);
    }

    /// Register a snapshot to fetch.  Idempotent: re-registering
    /// an existing snapshot is a no-op (the existing retriever's
    /// state is preserved).
    pub async fn enqueue_snapshot(&self, block_hash: Vec<u8>) {
        let mut g = self.snapshots.write().await;
        if g.contains_key(&block_hash) {
            return;
        }
        // Retriever is created with a placeholder target — the
        // actual merkle_root + chunk_count arrive via HasSnapshot
        // reply.  Setting chunk_count = 0 marks it as "unresolved"
        // so `ready_to_fetch` skips it until we learn the target.
        let placeholder_target = SnapshotTarget {
            block_hash: block_hash.clone(),
            merkle_root: [0u8; 32],
            chunk_count: 0,
        };
        let state = SnapshotSyncState {
            retriever: Arc::new(SnapshotChunkRetriever::new(placeholder_target)),
            sources: VecDeque::new(),
            blacklisted_sources: HashSet::new(),
            broadcasted_has_request: false,
        };
        g.insert(block_hash.clone(), state);
        info!(
            target: "f1r3fly.casper.snapshot_chunk_sync",
            block_hash = ?block_hash,
            "enqueued snapshot for fetch"
        );
    }

    /// Called when a HasSnapshot reply arrives.  If we've already
    /// pinned a target (from an earlier reply), verify consistency;
    /// otherwise, install this reply's target and start issuing
    /// chunk requests on subsequent ticks.
    ///
    /// Also records `sender` as a source for this snapshot.
    pub async fn on_has_snapshot(&self, sender: PeerNode, announcement: &HasSnapshot) {
        let block_hash: Vec<u8> = announcement.block_hash.to_vec();
        let mut g = self.snapshots.write().await;
        let state = match g.get_mut(&block_hash) {
            Some(s) => s,
            None => {
                debug!(
                    target: "f1r3fly.casper.snapshot_chunk_sync",
                    "HasSnapshot for un-enqueued snapshot; ignoring"
                );
                return;
            }
        };
        // Convert wire merkle_root to fixed-size array.
        let mut new_merkle = [0u8; 32];
        if announcement.merkle_root.len() != 32 {
            warn!(
                target: "f1r3fly.casper.snapshot_chunk_sync",
                len = announcement.merkle_root.len(),
                "HasSnapshot merkle_root has wrong length; blacklisting sender"
            );
            add_blacklist_capped(&mut state.blacklisted_sources, sender);
            return;
        }
        new_merkle.copy_from_slice(announcement.merkle_root.as_ref());

        // First reply: install the target.
        if state.retriever.target.chunk_count == 0 {
            let target = SnapshotTarget {
                block_hash: block_hash.clone(),
                merkle_root: new_merkle,
                chunk_count: announcement.chunk_count,
            };
            state.retriever = Arc::new(SnapshotChunkRetriever::new(target));
            state.sources.push_back(sender);
            info!(
                target: "f1r3fly.casper.snapshot_chunk_sync",
                block_hash = ?block_hash,
                chunk_count = announcement.chunk_count,
                "installed snapshot target from first HasSnapshot reply"
            );
        } else {
            // Subsequent replies must agree with the pinned target.
            if state.retriever.target.merkle_root != new_merkle
                || state.retriever.target.chunk_count != announcement.chunk_count
            {
                warn!(
                    target: "f1r3fly.casper.snapshot_chunk_sync",
                    "HasSnapshot disagrees with pinned target; blacklisting sender"
                );
                add_blacklist_capped(&mut state.blacklisted_sources, sender);
                return;
            }
            // Consistent reply — add sender as another source if
            // we don't already know them AND we're under the
            // security cap.
            if !state.sources.iter().any(|p| p == &sender)
                && !state.blacklisted_sources.contains(&sender)
                && state.sources.len() < MAX_SOURCES_PER_SNAPSHOT
            {
                state.sources.push_back(sender);
            }
        }
    }

    /// Dispatch an incoming SnapshotChunkResponse to its matching
    /// retriever.  On MerkleProofInvalid / ChunkHashMismatch /
    /// MerkleRootMismatch outcomes, blacklists the sender for
    /// this snapshot so we stop picking them.
    ///
    /// Returns true if the snapshot is now complete + written to
    /// disk.
    pub async fn on_chunk_response(
        &self,
        sender: PeerNode,
        response: &SnapshotChunkResponse,
    ) -> bool {
        let block_hash: Vec<u8> = response.block_hash.to_vec();
        let (retriever, outcome) = {
            let mut g = self.snapshots.write().await;
            let state = match g.get_mut(&block_hash) {
                Some(s) => s,
                None => {
                    debug!(
                        target: "f1r3fly.casper.snapshot_chunk_sync",
                        "chunk response for un-enqueued snapshot; ignoring"
                    );
                    return false;
                }
            };
            let retriever = Arc::clone(&state.retriever);
            let outcome = retriever.admit_response(response).await;
            match outcome {
                AdmitOutcome::MerkleProofInvalid
                | AdmitOutcome::ChunkHashMismatch
                | AdmitOutcome::MerkleRootMismatch => {
                    warn!(
                        target: "f1r3fly.casper.snapshot_chunk_sync",
                        outcome = ?outcome,
                        "byzantine response; blacklisting sender"
                    );
                    add_blacklist_capped(&mut state.blacklisted_sources, sender);
                }
                _ => {}
            }
            (retriever, outcome)
        };
        if outcome != AdmitOutcome::ChunkAccepted {
            return false;
        }
        if !retriever.is_complete().await {
            return false;
        }
        // Complete — assemble + write to disk.
        let assembled = match retriever.assemble().await {
            Some(b) => b,
            None => return false,
        };
        // Content-address filename.
        let atomic_root = {
            use crypto::rust::hash::blake2b256::Blake2b256;
            let h = Blake2b256::hash(assembled.clone());
            let mut out = [0u8; 32];
            out.copy_from_slice(&h);
            out
        };
        let path = rholang::rust::interpreter::io::snapshot::snapshot_path(
            &self.snapshot_dir,
            &atomic_root,
        );
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, &assembled) {
            warn!(
                target: "f1r3fly.casper.snapshot_chunk_sync",
                error = %e,
                "failed to write assembled snapshot"
            );
            return false;
        }
        info!(
            target: "f1r3fly.casper.snapshot_chunk_sync",
            block_hash = ?block_hash,
            path = %path.display(),
            "wrote assembled snapshot"
        );
        // Remove from active snapshots (done).
        self.snapshots.write().await.remove(&block_hash);
        // Phase 7b-2 item (c) (2026-08-28): notify any registered
        // completion sink.  The mpsc send is best-effort — if the
        // receiver dropped, the notification is silently discarded.
        // Reader tasks that exit (e.g., on driver.stop) will cause
        // any subsequent notification to fail here; the fetch
        // driver stays functional.
        let sink_snap = self
            .completion_sink
            .read()
            .expect("completion_sink poisoned")
            .clone();
        if let Some(tx) = sink_snap {
            let completion = SnapshotCompletion {
                block_hash: block_hash.clone(),
                atomic_root,
                path: path.clone(),
            };
            if tx.send(completion).is_err() {
                debug!(
                    target: "f1r3fly.casper.snapshot_chunk_sync",
                    "completion sink receiver dropped; ignoring notification"
                );
            }
        }
        true
    }

    /// Send an outbound tick: for each active snapshot, either
    /// broadcast a HasSnapshotRequest (if we haven't yet), or send
    /// GetSnapshotChunkRequest for the next pending chunk to the
    /// next available source.  Also handles timeout-driven retries
    /// via `retriever.timed_out_indices`.
    pub async fn tick<T: TransportLayer + Send + Sync>(
        &self,
        transport: &T,
        conf: &RPConf,
        connections_cell: &comm::rust::rp::connect::ConnectionsCell,
    ) {
        let block_hashes: Vec<Vec<u8>> = self.snapshots.read().await.keys().cloned().collect();
        for block_hash in block_hashes {
            // Take a snapshot of state so we can call transport
            // methods without holding the map lock.
            let (need_broadcast, source_opt, timed_out, retriever) = {
                let mut g = self.snapshots.write().await;
                let state = match g.get_mut(&block_hash) {
                    Some(s) => s,
                    None => continue,
                };
                let need_broadcast = !state.broadcasted_has_request;
                if need_broadcast {
                    state.broadcasted_has_request = true;
                }
                let retriever = Arc::clone(&state.retriever);
                // If target still unresolved (chunk_count == 0),
                // don't try to request chunks yet.
                let source_opt = if retriever.target.chunk_count > 0 {
                    state.next_source()
                } else {
                    None
                };
                let timed_out = retriever.timed_out_indices().await;
                (need_broadcast, source_opt, timed_out, retriever)
            };

            if need_broadcast {
                if let Err(e) =
                    broadcast_has_snapshot_request(transport, connections_cell, conf, &block_hash)
                        .await
                {
                    warn!(
                        target: "f1r3fly.casper.snapshot_chunk_sync",
                        error = %e,
                        "broadcast_has_snapshot_request failed"
                    );
                }
            }

            let source = match source_opt {
                Some(p) => p,
                None => continue,
            };

            // Re-request timed-out chunks first.
            for idx in &timed_out {
                if let Err(e) =
                    send_get_snapshot_chunk_request(transport, conf, &source, &block_hash, *idx)
                        .await
                {
                    warn!(
                        target: "f1r3fly.casper.snapshot_chunk_sync",
                        error = %e,
                        "send_get_snapshot_chunk_request failed for retry"
                    );
                    continue;
                }
                retriever
                    .record_request_sent(*idx, source.id.key.as_ref())
                    .await;
                retriever.record_retry(*idx).await;
            }

            // Then issue fresh requests for pending indices we haven't
            // yet asked for (last_request_ms == 0).
            let pending = retriever.pending_indices().await;
            for idx in pending {
                let already_in_flight = {
                    let chunks = retriever.chunks.read().await;
                    chunks
                        .get(&idx)
                        .map(|s| s.last_request_ms > 0 && s.retry_count < MAX_RETRIES)
                        .unwrap_or(true)
                };
                if already_in_flight {
                    continue;
                }
                if let Err(e) =
                    send_get_snapshot_chunk_request(transport, conf, &source, &block_hash, idx)
                        .await
                {
                    warn!(
                        target: "f1r3fly.casper.snapshot_chunk_sync",
                        error = %e,
                        "send_get_snapshot_chunk_request failed"
                    );
                    continue;
                }
                retriever
                    .record_request_sent(idx, source.id.key.as_ref())
                    .await;
            }
        }
    }

    /// Query: how many snapshots are currently being fetched.
    pub async fn active_count(&self) -> usize { self.snapshots.read().await.len() }

    /// Query: is a specific snapshot done (assembled + written)?
    /// Returns true when the map entry has been removed.
    pub async fn is_done(&self, block_hash: &[u8]) -> bool {
        !self.snapshots.read().await.contains_key(block_hash)
    }
}

// -------------------------------------------------------------------
// Boot-time enumerator + periodic tick driver.  Both compose the
// primitives above into ready-to-spawn tasks — the running-engine
// setup path calls these once at boot.
// -------------------------------------------------------------------

/// Enumerate finalized blocks whose Merkle-root anchor is cached in
/// `snapshot_merkle_roots` but whose on-disk snapshot file is
/// missing.  For each, call `driver.enqueue_snapshot(block_hash)`.
///
/// Returns the number of snapshots newly enqueued.
///
/// A snapshot's on-disk file lives at
/// `snapshot_dir / <hex(atomic_root)>.wal` — the content-addressed
/// filename produced by `SnapshotWriter::write_snapshot`.  If the
/// file exists we treat the snapshot as already assembled; if not,
/// the joiner needs to fetch it.
pub async fn enumerate_and_enqueue_missing_snapshots(
    driver: &SnapshotChunkSyncDriver,
    snapshot_merkle_roots: &Arc<tokio::sync::RwLock<HashMap<Vec<u8>, ([u8; 32], [u8; 32])>>>,
) -> usize {
    let anchors: HashMap<Vec<u8>, ([u8; 32], [u8; 32])> =
        snapshot_merkle_roots.read().await.clone();
    let mut enqueued = 0;
    for (block_hash, (atomic_root, _merkle_root)) in anchors {
        let on_disk = rholang::rust::interpreter::io::snapshot::snapshot_path(
            &driver.snapshot_dir,
            &atomic_root,
        );
        if on_disk.exists() {
            continue;
        }
        driver.enqueue_snapshot(block_hash).await;
        enqueued += 1;
    }
    if enqueued > 0 {
        info!(
            target: "f1r3fly.casper.snapshot_chunk_sync",
            enqueued,
            "boot-time enumerator enqueued snapshots for fetch"
        );
    }
    enqueued
}

/// Default tick period between outbound-request rounds.  Tuned to
/// give peers time to respond before we re-request; matches
/// BlockRetriever's REQUEST_INTERVAL scale.
pub const TICK_PERIOD_MS: u64 = 5_000;

/// Spawn a periodic tick task that calls
/// `SnapshotChunkSyncDriver::tick` every `TICK_PERIOD_MS`.
/// Returns the JoinHandle so the caller can abort on shutdown.
///
/// The task loops forever until aborted or the driver is dropped
/// (in which case `Arc::weak_count` drops to 0 and the tick becomes
/// a no-op).
pub fn spawn_periodic_tick<T>(
    driver: Arc<SnapshotChunkSyncDriver>,
    transport: Arc<T>,
    conf: comm::rust::rp::rp_conf::RPConf,
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
            if driver.active_count().await == 0 {
                // Nothing pending — cheap short-circuit that
                // avoids allocating per-tick.
                continue;
            }
            driver.tick(&*transport, &conf, &connections_cell).await;
        }
    })
}

// snapshot_dir accessor for the enumerator to call snapshot_path
// without exposing the whole struct.
impl SnapshotChunkSyncDriver {
    pub fn snapshot_dir(&self) -> &std::path::Path { &self.snapshot_dir }
}

/// Boot-time factory: assemble the joiner-side snapshot chunk-fetch
/// wiring from a live RuntimeManager.  Returns `None` when the node
/// has no `fs_snapshot_writer` configured — that includes:
///   * Observer nodes without a snapshot cache directory.
///   * Test harnesses that construct RuntimeManager without a
///     writer.
///   * Boot windows before `set_fs_snapshot_writer` runs (callers
///     should invoke this AFTER the writer is installed).
///
/// The returned `SnapshotChunkContext` holds an `Arc` to the
/// runtime_manager's `snapshot_merkle_roots` cache (shared with
/// finalization_runner's write side) and a fresh
/// `SnapshotChunkSyncDriver` that owns its own per-block sync state.
/// Callers thread the context into `Running::install_snapshot_chunk_context`
/// AND `spawn_periodic_tick`.
pub async fn build_snapshot_chunk_context(
    runtime_manager: &crate::rust::util::rholang::runtime_manager::RuntimeManager,
) -> Option<crate::rust::engine::running::SnapshotChunkContext> {
    let writer_opt = runtime_manager.fs_snapshot_writer.read().await.clone();
    let writer = writer_opt?;
    let snapshot_dir = writer.dir.clone();
    let sync_driver = Arc::new(SnapshotChunkSyncDriver::new(snapshot_dir.clone()));
    // Boot enumerator: any anchors we already know about but
    // haven't materialized on disk get enqueued for fetch.
    let _ = enumerate_and_enqueue_missing_snapshots(
        &sync_driver,
        &runtime_manager.snapshot_merkle_roots,
    )
    .await;
    Some(crate::rust::engine::running::SnapshotChunkContext {
        sync_driver,
        snapshot_dir,
        snapshot_merkle_roots: Arc::clone(&runtime_manager.snapshot_merkle_roots),
    })
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;
    use rholang::rust::interpreter::io::snapshot::write_snapshot;
    use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

    use super::*;
    use crate::rust::engine::snapshot_chunk_server::serve_chunk;

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

    /// Boot: enqueue a snapshot; before any HasSnapshot arrives,
    /// active_count is 1 + retriever has a placeholder target
    /// (chunk_count = 0 means "unresolved").
    #[tokio::test]
    async fn enqueue_creates_placeholder_state() {
        let dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dir.path().to_path_buf());
        let block_hash = vec![0xAB; 32];
        driver.enqueue_snapshot(block_hash.clone()).await;
        assert_eq!(driver.active_count().await, 1);
        let g = driver.snapshots.read().await;
        assert_eq!(g[&block_hash].retriever.target.chunk_count, 0);
    }

    /// enqueue_snapshot is idempotent.
    #[tokio::test]
    async fn enqueue_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dir.path().to_path_buf());
        let block_hash = vec![0xCD; 32];
        driver.enqueue_snapshot(block_hash.clone()).await;
        driver.enqueue_snapshot(block_hash.clone()).await;
        assert_eq!(driver.active_count().await, 1);
    }

    /// HasSnapshot reply installs the target.
    #[tokio::test]
    async fn has_snapshot_reply_installs_target() {
        let dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dir.path().to_path_buf());
        let block_hash = vec![0x11; 32];
        driver.enqueue_snapshot(block_hash.clone()).await;

        let peer = mk_peer("alice");
        let merkle = [0x99u8; 32];
        let announcement = HasSnapshot {
            block_hash: Bytes::copy_from_slice(&block_hash),
            merkle_root: Bytes::copy_from_slice(&merkle),
            chunk_count: 3,
        };
        driver.on_has_snapshot(peer.clone(), &announcement).await;

        let g = driver.snapshots.read().await;
        let state = &g[&block_hash];
        assert_eq!(state.retriever.target.chunk_count, 3);
        assert_eq!(state.retriever.target.merkle_root, merkle);
        assert_eq!(state.sources.len(), 1);
    }

    /// Byzantine HasSnapshot (disagreeing merkle_root) blacklists
    /// the source rather than mutating the pinned target.
    #[tokio::test]
    async fn conflicting_has_snapshot_blacklists_sender() {
        let dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dir.path().to_path_buf());
        let block_hash = vec![0x22; 32];
        driver.enqueue_snapshot(block_hash.clone()).await;
        let alice = mk_peer("alice");
        let bob = mk_peer("bob");

        driver
            .on_has_snapshot(alice.clone(), &HasSnapshot {
                block_hash: Bytes::copy_from_slice(&block_hash),
                merkle_root: Bytes::from_static(&[0xAA; 32]),
                chunk_count: 4,
            })
            .await;
        driver
            .on_has_snapshot(bob.clone(), &HasSnapshot {
                block_hash: Bytes::copy_from_slice(&block_hash),
                merkle_root: Bytes::from_static(&[0xBB; 32]),
                chunk_count: 4,
            })
            .await;

        let g = driver.snapshots.read().await;
        let state = &g[&block_hash];
        // Pinned target is Alice's; Bob is blacklisted.
        assert_eq!(state.retriever.target.merkle_root, [0xAA; 32]);
        assert!(state.blacklisted_sources.contains(&bob));
    }

    /// End-to-end: enqueue, on_has_snapshot, feed all chunks via
    /// on_chunk_response, assert on-disk write.
    #[tokio::test]
    async fn full_fetch_writes_assembled_snapshot() {
        let src_dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("a"), mk_entry("b"), mk_entry("c")];
        let (_p, atomic_root, merkle_root) = write_snapshot(src_dir.path(), &entries).unwrap();
        let block_hash = vec![0xDD; 32];

        let dest_dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dest_dir.path().to_path_buf());
        driver.enqueue_snapshot(block_hash.clone()).await;
        let peer = mk_peer("charlie");
        driver
            .on_has_snapshot(peer.clone(), &HasSnapshot {
                block_hash: Bytes::copy_from_slice(&block_hash),
                merkle_root: Bytes::copy_from_slice(&merkle_root),
                chunk_count: 1,
            })
            .await;

        // Server produces the single chunk.
        let response =
            serve_chunk(&block_hash, 0, (atomic_root, merkle_root), src_dir.path()).unwrap();
        let done = driver.on_chunk_response(peer, &response).await;
        assert!(done, "single-chunk snapshot completes in one response");
        assert!(driver.is_done(&block_hash).await);

        // File exists at content-address in dest_dir.
        let path =
            rholang::rust::interpreter::io::snapshot::snapshot_path(dest_dir.path(), &atomic_root);
        assert!(path.exists(), "assembled snapshot must exist on disk");
    }

    /// Phase 7b-2 item (c) (2026-08-28): an installed completion
    /// sink receives a `SnapshotCompletion` message per assembled
    /// snapshot.  Wire-in test for the joiner-side apply-to-follower
    /// pipeline.
    #[tokio::test]
    async fn completion_sink_receives_notification_on_full_fetch() {
        let src_dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("a")];
        let (_p, atomic_root, merkle_root) = write_snapshot(src_dir.path(), &entries).unwrap();
        let block_hash = vec![0xEEu8; 32];

        let dest_dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dest_dir.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SnapshotCompletion>();
        driver.install_completion_sink(tx);

        driver.enqueue_snapshot(block_hash.clone()).await;
        let peer = mk_peer("charlie");
        driver
            .on_has_snapshot(peer.clone(), &HasSnapshot {
                block_hash: Bytes::copy_from_slice(&block_hash),
                merkle_root: Bytes::copy_from_slice(&merkle_root),
                chunk_count: 1,
            })
            .await;
        let response =
            serve_chunk(&block_hash, 0, (atomic_root, merkle_root), src_dir.path()).unwrap();
        assert!(driver.on_chunk_response(peer, &response).await);

        let msg = rx.try_recv().expect("completion sink must receive");
        assert_eq!(msg.block_hash, block_hash);
        assert_eq!(msg.atomic_root, atomic_root);
        assert!(msg.path.exists());
    }

    /// If no sink is installed, `on_chunk_response` still succeeds
    /// (the notification is a no-op).  Pre-Phase-7b-2-item-(c)
    /// call sites that don't wire the apply flow stay functional.
    #[tokio::test]
    async fn on_chunk_response_no_sink_installed_is_ok() {
        let src_dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("a")];
        let (_p, atomic_root, merkle_root) = write_snapshot(src_dir.path(), &entries).unwrap();
        let block_hash = vec![0xEFu8; 32];
        let dest_dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dest_dir.path().to_path_buf());
        // Do NOT install a sink.
        driver.enqueue_snapshot(block_hash.clone()).await;
        let peer = mk_peer("dave");
        driver
            .on_has_snapshot(peer.clone(), &HasSnapshot {
                block_hash: Bytes::copy_from_slice(&block_hash),
                merkle_root: Bytes::copy_from_slice(&merkle_root),
                chunk_count: 1,
            })
            .await;
        let response =
            serve_chunk(&block_hash, 0, (atomic_root, merkle_root), src_dir.path()).unwrap();
        assert!(driver.on_chunk_response(peer, &response).await);
    }

    /// 2026-08-28 hardening pin: a second `install_completion_sink`
    /// call replaces the first sender.  Documented behavior — the
    /// current boot pipeline only installs one sink, but a caller
    /// that re-installs must know the old receiver stops getting
    /// notifications.  Regression pin against future accidental
    /// multi-subscribe assumptions.
    #[tokio::test]
    async fn second_install_completion_sink_replaces_first() {
        let src_dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("a")];
        let (_p, atomic_root, merkle_root) = write_snapshot(src_dir.path(), &entries).unwrap();
        let block_hash = vec![0xEB; 32];

        let dest_dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dest_dir.path().to_path_buf());

        // Install first sink.  Its receiver is `rx1`.
        let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel::<SnapshotCompletion>();
        driver.install_completion_sink(tx1);
        // Install second sink.  Its receiver is `rx2`.  The first
        // sender is dropped when the RwLock overwrites it.
        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel::<SnapshotCompletion>();
        driver.install_completion_sink(tx2);

        driver.enqueue_snapshot(block_hash.clone()).await;
        let peer = mk_peer("frank");
        driver
            .on_has_snapshot(peer.clone(), &HasSnapshot {
                block_hash: Bytes::copy_from_slice(&block_hash),
                merkle_root: Bytes::copy_from_slice(&merkle_root),
                chunk_count: 1,
            })
            .await;
        let response =
            serve_chunk(&block_hash, 0, (atomic_root, merkle_root), src_dir.path()).unwrap();
        assert!(driver.on_chunk_response(peer, &response).await);

        // rx1 receives nothing (it was orphaned).
        assert!(
            rx1.try_recv().is_err(),
            "first sink must not receive after replace"
        );
        // rx2 receives the notification.
        let msg = rx2.try_recv().expect("second sink must receive");
        assert_eq!(msg.atomic_root, atomic_root);
    }

    /// A dropped receiver does not break the driver.  The next
    /// on_chunk_response continues to write bytes to disk; the
    /// notification failure is silent.
    #[tokio::test]
    async fn dropped_completion_receiver_is_silently_tolerated() {
        let src_dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("a")];
        let (_p, atomic_root, merkle_root) = write_snapshot(src_dir.path(), &entries).unwrap();
        let block_hash = vec![0xEA; 32];

        let dest_dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dest_dir.path().to_path_buf());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SnapshotCompletion>();
        driver.install_completion_sink(tx);
        // Drop the receiver end.  A send should now fail.
        drop(rx);

        driver.enqueue_snapshot(block_hash.clone()).await;
        let peer = mk_peer("eve");
        driver
            .on_has_snapshot(peer.clone(), &HasSnapshot {
                block_hash: Bytes::copy_from_slice(&block_hash),
                merkle_root: Bytes::copy_from_slice(&merkle_root),
                chunk_count: 1,
            })
            .await;
        let response =
            serve_chunk(&block_hash, 0, (atomic_root, merkle_root), src_dir.path()).unwrap();
        // Should still return true — the write happened, only the
        // notification silently failed.
        assert!(driver.on_chunk_response(peer, &response).await);
    }

    /// Boot enumerator: enqueue only anchors whose on-disk file is
    /// missing.  A present file is skipped.
    #[tokio::test]
    async fn enumerator_enqueues_missing_but_skips_present() {
        let snap_dir = tempfile::tempdir().unwrap();
        // Anchor A: on-disk file present.  Anchor B: on-disk
        // file missing.
        let entries = vec![mk_entry("z")];
        let (_p, atomic_a, merkle_a) = write_snapshot(snap_dir.path(), &entries).unwrap();
        let block_a = vec![0x0A; 32];
        let block_b = vec![0x0B; 32];
        let atomic_b = [0xFFu8; 32];
        let merkle_b = [0xEEu8; 32];

        let anchors = Arc::new(tokio::sync::RwLock::new(HashMap::from([
            (block_a.clone(), (atomic_a, merkle_a)),
            (block_b.clone(), (atomic_b, merkle_b)),
        ])));
        let driver = SnapshotChunkSyncDriver::new(snap_dir.path().to_path_buf());
        let n = enumerate_and_enqueue_missing_snapshots(&driver, &anchors).await;
        assert_eq!(n, 1, "only block_b should be enqueued (block_a is on disk)");
        assert!(driver.snapshots.read().await.contains_key(&block_b));
        assert!(!driver.snapshots.read().await.contains_key(&block_a));
    }

    /// Security cap: the per-snapshot sources set stops growing
    /// at MAX_SOURCES_PER_SNAPSHOT.  Defends against a peer-flood
    /// memory-exhaustion attack.
    #[tokio::test]
    async fn sources_set_stops_growing_at_cap() {
        let dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dir.path().to_path_buf());
        let block_hash = vec![0x51u8; 32];
        driver.enqueue_snapshot(block_hash.clone()).await;
        let merkle = [0x77u8; 32];
        let chunk_count = 8u32;

        // First reply installs target + adds the first source.
        driver
            .on_has_snapshot(mk_peer("peer0"), &HasSnapshot {
                block_hash: Bytes::copy_from_slice(&block_hash),
                merkle_root: Bytes::copy_from_slice(&merkle),
                chunk_count,
            })
            .await;

        // Flood with distinct peers past the cap.
        for i in 1..(MAX_SOURCES_PER_SNAPSHOT + 20) {
            driver
                .on_has_snapshot(mk_peer(&format!("peer{i}")), &HasSnapshot {
                    block_hash: Bytes::copy_from_slice(&block_hash),
                    merkle_root: Bytes::copy_from_slice(&merkle),
                    chunk_count,
                })
                .await;
        }

        let g = driver.snapshots.read().await;
        assert_eq!(
            g[&block_hash].sources.len(),
            MAX_SOURCES_PER_SNAPSHOT,
            "sources set must stop at MAX_SOURCES_PER_SNAPSHOT"
        );
    }

    /// Security cap: the per-snapshot blacklist stops growing at
    /// MAX_BLACKLISTED_PER_SNAPSHOT.  Silent no-op past the cap.
    #[tokio::test]
    async fn blacklist_stops_growing_at_cap() {
        let dir = tempfile::tempdir().unwrap();
        let driver = SnapshotChunkSyncDriver::new(dir.path().to_path_buf());
        let block_hash = vec![0x52u8; 32];
        driver.enqueue_snapshot(block_hash.clone()).await;
        // First reply installs target.
        driver
            .on_has_snapshot(mk_peer("honest"), &HasSnapshot {
                block_hash: Bytes::copy_from_slice(&block_hash),
                merkle_root: Bytes::from_static(&[0xAA; 32]),
                chunk_count: 4,
            })
            .await;
        // Flood with byzantine (disagreeing merkle_root) replies from
        // distinct peers past the cap.
        for i in 0..(MAX_BLACKLISTED_PER_SNAPSHOT + 20) {
            driver
                .on_has_snapshot(mk_peer(&format!("byz{i}")), &HasSnapshot {
                    block_hash: Bytes::copy_from_slice(&block_hash),
                    merkle_root: Bytes::from_static(&[0xBB; 32]),
                    chunk_count: 4,
                })
                .await;
        }
        let g = driver.snapshots.read().await;
        assert_eq!(
            g[&block_hash].blacklisted_sources.len(),
            MAX_BLACKLISTED_PER_SNAPSHOT,
            "blacklist must stop at MAX_BLACKLISTED_PER_SNAPSHOT"
        );
    }

    /// next_source rotation: consecutive picks land on different
    /// non-blacklisted peers.  Blacklisted peers are silently
    /// skipped in the rotation.
    #[tokio::test]
    async fn next_source_rotates_and_skips_blacklisted() {
        let mut state = SnapshotSyncState {
            retriever: Arc::new(SnapshotChunkRetriever::new(SnapshotTarget {
                block_hash: vec![0; 32],
                merkle_root: [0; 32],
                chunk_count: 4,
            })),
            sources: VecDeque::from(vec![mk_peer("alice"), mk_peer("byz"), mk_peer("bob")]),
            blacklisted_sources: HashSet::from([mk_peer("byz")]),
            broadcasted_has_request: false,
        };
        // First two picks should be alice, bob (skipping byz).
        // Third pick rotates back to alice.
        let a = state.next_source().unwrap();
        let b = state.next_source().unwrap();
        let c = state.next_source().unwrap();
        assert_ne!(a.id.key, b.id.key, "consecutive picks must differ");
        assert_eq!(a.id.key, c.id.key, "3rd pick rotates back to 1st");
        assert!(!a.id.key.iter().eq(b"byz".iter()));
        assert!(!b.id.key.iter().eq(b"byz".iter()));
    }

    /// A byzantine chunk response blacklists the sender.
    #[tokio::test]
    async fn byzantine_chunk_blacklists_sender() {
        let src_dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("x")];
        let (_p, _atomic, merkle_root) = write_snapshot(src_dir.path(), &entries).unwrap();
        let block_hash = vec![0xEE; 32];

        let driver = SnapshotChunkSyncDriver::new(src_dir.path().to_path_buf());
        driver.enqueue_snapshot(block_hash.clone()).await;
        let mallory = mk_peer("mallory");
        driver
            .on_has_snapshot(mallory.clone(), &HasSnapshot {
                block_hash: Bytes::copy_from_slice(&block_hash),
                merkle_root: Bytes::copy_from_slice(&merkle_root),
                chunk_count: 1,
            })
            .await;

        // Malformed response: wrong chunk_bytes (hash mismatch).
        let bogus = SnapshotChunkResponse {
            block_hash: Bytes::copy_from_slice(&block_hash),
            chunk_index: 0,
            chunk_bytes: Bytes::from_static(&[0xFF; 16]),
            chunk_hash: Bytes::from_static(&[0x00; 32]),
            merkle_root: Bytes::copy_from_slice(&merkle_root),
            chunk_count: 1,
            merkle_proof: vec![],
        };
        let done = driver.on_chunk_response(mallory.clone(), &bogus).await;
        assert!(!done);
        assert!(!driver.is_done(&block_hash).await);
        let g = driver.snapshots.read().await;
        assert!(g[&block_hash].blacklisted_sources.contains(&mallory));
    }
}
