// See casper/src/main/scala/coop/rchain/casper/engine/BlockRetriever.scala

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use tracing::{debug, info};

use crate::rust::engine::finalization_certificate_retriever::{
    CertificateRequestOutcome, FinalizationCertificateRetriever,
};
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_DOWNLOAD_END_TO_END_TIME_METRIC, BLOCK_REQUESTS_CAPACITY_DEFERRED_TOTAL_METRIC,
    BLOCK_REQUESTS_RETRIES_METRIC, BLOCK_REQUESTS_RETRY_ACTION_METRIC,
    BLOCK_REQUESTS_STALE_EVICTIONS_METRIC, BLOCK_REQUESTS_TOTAL_METRIC,
    BLOCK_RETRIEVER_BROADCAST_TRACKING_SIZE_METRIC,
    BLOCK_RETRIEVER_DEP_RECOVERY_TRACKING_SIZE_METRIC, BLOCK_RETRIEVER_METRICS_SOURCE,
    BLOCK_RETRIEVER_PEERS_TOTAL_SIZE_METRIC, BLOCK_RETRIEVER_REQUESTED_BLOCKS_SIZE_METRIC,
    BLOCK_RETRIEVER_WAITING_LIST_TOTAL_SIZE_METRIC,
};

#[derive(Debug, Clone, PartialEq)]
pub enum AdmitHashReason {
    HasBlockMessageReceived,
    HashBroadcastReceived,
    MissingDependencyRequested,
    BlockReceived,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdmitHashStatus {
    NewSourcePeerAddedToRequest,
    NewRequestAdded,
    Ignore,
}

#[derive(Debug, Clone)]
pub struct AdmitHashResult {
    pub status: AdmitHashStatus,
    pub broadcast_request: bool,
    pub request_block: bool,
}

#[derive(Debug, Clone)]
pub struct RequestState {
    pub timestamp: u64,
    pub initial_timestamp: u64,
    pub peers: HashSet<PeerNode>,
    pub received: bool,
    pub in_casper_buffer: bool,
    pub waiting_list: Vec<PeerNode>,
    pub peer_requery_cursor: u32,
    /// This node asked for the hash to satisfy a missing dependency, rather
    /// than merely hearing it announced. Sticky: a later gossip announcement
    /// does not clear it. Read by `check_if_of_interest`, which must not drop
    /// a solicited dependency as "old" — the block's height is below the
    /// joiner's approved block precisely because it is history the joiner
    /// lacks.
    pub requested_as_dependency: bool,
}

// Scala: type RequestedBlocks[F[_]] = Ref[F, Map[BlockHash, RequestState]]
// In Rust, we use Arc<Mutex<...>> as shared mutable state (passed as implicit in Scala)
pub type RequestedBlocks = Arc<Mutex<HashMap<BlockHash, RequestState>>>;

#[derive(Debug, Clone, PartialEq)]
enum AckReceiveResult {
    AddedAsReceived,
    MarkedAsReceived,
    UntrackedAtCapacity,
}

/**
 * BlockRetriever makes sure block is received once Casper request it.
 * Block is in scope of BlockRetriever until it is added to CasperBuffer.
 *
 * Scala: BlockRetriever.of[F[_]: Monad: RequestedBlocks: ...]
 * In Scala, RequestedBlocks is passed as an implicit parameter (type class constraint).
 * In Rust, we explicitly pass it as a constructor parameter.
 */
#[derive(Debug, Clone)]
pub struct BlockRetriever<T: TransportLayer + Send + Sync> {
    requested_blocks: RequestedBlocks,
    dependency_recovery_last_request: Arc<Mutex<HashMap<BlockHash, u64>>>,
    broadcast_retry_last_request: Arc<Mutex<HashMap<BlockHash, u64>>>,
    peer_requery_last_request: Arc<Mutex<HashMap<BlockHash, u64>>>,
    peer_requery_attempts_by_hash: Arc<Mutex<HashMap<BlockHash, u32>>>,
    retry_attempts_by_hash: Arc<Mutex<HashMap<BlockHash, u32>>>,
    retry_budget_quarantine_until: Arc<Mutex<HashMap<BlockHash, u64>>>,
    finalization_certificate_retriever: FinalizationCertificateRetriever<T>,
    transport: Arc<T>,
    connections_cell: ConnectionsCell,
    conf: RPConf,
}

impl<T: TransportLayer + Send + Sync> BlockRetriever<T> {
    const MAX_REQUESTED_BLOCKS_ENTRIES: usize = 2048;
    const MAX_WAITING_LIST_PER_HASH: usize = 64;
    const PEER_REQUERY_COOLDOWN_MS: u64 = 500;
    const BROADCAST_ONLY_COOLDOWN_MS: u64 = 500;
    const MIN_REREQUEST_INTERVAL_MS: u64 = 500;
    const MAX_RETRIES_PER_HASH: u32 = 32;
    const DEPENDENCY_RECOVERY_COOLDOWN_MS: u64 = 500;
    const STALE_REQUEST_LIFETIME_MULTIPLIER: u64 = 6;
    const KNOWN_PEER_REQUERY_SOFT_LIMIT: u32 = 8;
    const RETRY_BUDGET_QUARANTINE_MS: u64 = 10_000;
    const MISSING_DEPENDENCY_SEED_PEERS: usize = 4;

    fn broadcast_retry_cooldown_ms_for_hash(&self, hash: &BlockHash) -> Result<u64, CasperError> {
        // Increase broadcast backoff when hash resolution is repeatedly failing.
        let attempts = self.retry_attempt_count(hash)?;
        let base = Self::BROADCAST_ONLY_COOLDOWN_MS;
        if attempts <= 8 {
            return Ok(base);
        }

        let multiplier = 1 + std::cmp::min(((attempts - 8) / 8) as u64, 4);
        Ok(base.saturating_mul(multiplier))
    }

    fn update_aux_tracking_metrics(&self) -> Result<(), CasperError> {
        let (requested_size, waiting_list_total_size, peers_total_size) = {
            let state = self.requested_blocks.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?;
            let waiting_total = state.values().map(|r| r.waiting_list.len()).sum::<usize>();
            let peers_total = state.values().map(|r| r.peers.len()).sum::<usize>();
            (state.len(), waiting_total, peers_total)
        };
        let dep_size = {
            let last_requests = self.dependency_recovery_last_request.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire dependency_recovery_last_request lock".to_string(),
                )
            })?;
            last_requests.len()
        };
        let broadcast_size = {
            let broadcast_last = self.broadcast_retry_last_request.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire broadcast_retry_last_request lock".to_string(),
                )
            })?;
            broadcast_last.len()
        };
        let peer_requery_size = {
            let peer_requery_last = self.peer_requery_last_request.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire peer_requery_last_request lock".to_string(),
                )
            })?;
            peer_requery_last.len()
        };
        let retry_attempts_size = {
            let retry_attempts = self.retry_attempts_by_hash.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire retry_attempts_by_hash lock".to_string(),
                )
            })?;
            retry_attempts.len()
        };
        let peer_requery_attempts_size = {
            let peer_requery_attempts =
                self.peer_requery_attempts_by_hash.lock().map_err(|_| {
                    CasperError::RuntimeError(
                        "Failed to acquire peer_requery_attempts_by_hash lock".to_string(),
                    )
                })?;
            peer_requery_attempts.len()
        };
        let quarantine_size = {
            let quarantine = self.retry_budget_quarantine_until.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire retry_budget_quarantine_until lock".to_string(),
                )
            })?;
            quarantine.len()
        };

        metrics::gauge!(BLOCK_RETRIEVER_REQUESTED_BLOCKS_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE)
            .set(requested_size as f64);
        metrics::gauge!(BLOCK_RETRIEVER_WAITING_LIST_TOTAL_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE)
            .set(waiting_list_total_size as f64);
        metrics::gauge!(BLOCK_RETRIEVER_PEERS_TOTAL_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE)
            .set(peers_total_size as f64);
        metrics::gauge!(BLOCK_RETRIEVER_DEP_RECOVERY_TRACKING_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE)
            .set(dep_size as f64);
        metrics::gauge!(BLOCK_RETRIEVER_BROADCAST_TRACKING_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE)
            .set(broadcast_size as f64);
        // Reuse broadcast-tracking gauge as a proxy to include the requery cooldown map pressure.
        metrics::gauge!(BLOCK_RETRIEVER_BROADCAST_TRACKING_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "kind" => "peer_requery")
            .set(peer_requery_size as f64);
        metrics::gauge!(BLOCK_RETRIEVER_BROADCAST_TRACKING_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "kind" => "retry_attempts")
            .set(retry_attempts_size as f64);
        metrics::gauge!(BLOCK_RETRIEVER_BROADCAST_TRACKING_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "kind" => "peer_requery_attempts")
            .set(peer_requery_attempts_size as f64);
        metrics::gauge!(BLOCK_RETRIEVER_BROADCAST_TRACKING_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "kind" => "retry_budget_quarantine")
            .set(quarantine_size as f64);
        Ok(())
    }

    fn cleanup_aux_tracking_for_hash(&self, hash: &BlockHash) -> Result<(), CasperError> {
        {
            let mut last_requests = self.dependency_recovery_last_request.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire dependency_recovery_last_request lock".to_string(),
                )
            })?;
            last_requests.remove(hash);
        }
        {
            let mut broadcast_last = self.broadcast_retry_last_request.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire broadcast_retry_last_request lock".to_string(),
                )
            })?;
            broadcast_last.remove(hash);
        }
        {
            let mut peer_requery_last = self.peer_requery_last_request.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire peer_requery_last_request lock".to_string(),
                )
            })?;
            peer_requery_last.remove(hash);
        }
        {
            let mut retry_attempts = self.retry_attempts_by_hash.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire retry_attempts_by_hash lock".to_string(),
                )
            })?;
            retry_attempts.remove(hash);
        }
        {
            let mut peer_requery_attempts =
                self.peer_requery_attempts_by_hash.lock().map_err(|_| {
                    CasperError::RuntimeError(
                        "Failed to acquire peer_requery_attempts_by_hash lock".to_string(),
                    )
                })?;
            peer_requery_attempts.remove(hash);
        }
        {
            let mut quarantine = self.retry_budget_quarantine_until.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire retry_budget_quarantine_until lock".to_string(),
                )
            })?;
            quarantine.remove(hash);
        }
        Ok(())
    }

    fn cleanup_hash_tracking(&self, hash: &BlockHash) -> Result<(), CasperError> {
        {
            let mut state = self.requested_blocks.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?;
            state.remove(hash);
        }
        self.cleanup_aux_tracking_for_hash(hash)?;
        Ok(())
    }

    fn sweep_orphaned_aux_tracking(&self) -> Result<(), CasperError> {
        let active_hashes: HashSet<BlockHash> = {
            let state = self.requested_blocks.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?;
            state.keys().cloned().collect()
        };

        {
            let mut last_requests = self.dependency_recovery_last_request.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire dependency_recovery_last_request lock".to_string(),
                )
            })?;
            last_requests.retain(|hash, _| active_hashes.contains(hash));
        }

        {
            let mut broadcast_last = self.broadcast_retry_last_request.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire broadcast_retry_last_request lock".to_string(),
                )
            })?;
            broadcast_last.retain(|hash, _| active_hashes.contains(hash));
        }
        {
            let mut peer_requery_last = self.peer_requery_last_request.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire peer_requery_last_request lock".to_string(),
                )
            })?;
            peer_requery_last.retain(|hash, _| active_hashes.contains(hash));
        }
        {
            let mut retry_attempts = self.retry_attempts_by_hash.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire retry_attempts_by_hash lock".to_string(),
                )
            })?;
            retry_attempts.retain(|hash, _| active_hashes.contains(hash));
        }
        {
            let mut peer_requery_attempts =
                self.peer_requery_attempts_by_hash.lock().map_err(|_| {
                    CasperError::RuntimeError(
                        "Failed to acquire peer_requery_attempts_by_hash lock".to_string(),
                    )
                })?;
            peer_requery_attempts.retain(|hash, _| active_hashes.contains(hash));
        }
        {
            let mut quarantine = self.retry_budget_quarantine_until.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire retry_budget_quarantine_until lock".to_string(),
                )
            })?;
            quarantine.retain(|hash, _| active_hashes.contains(hash));
        }

        Ok(())
    }

    fn sweep_expired_retry_budget_quarantine(&self, now: u64) -> Result<(), CasperError> {
        let mut quarantine = self.retry_budget_quarantine_until.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire retry_budget_quarantine_until lock".to_string(),
            )
        })?;
        quarantine.retain(|_, until| *until > now);
        Ok(())
    }

    fn verify_requested_blocks_bound(&self) -> Result<(), CasperError> {
        let size = self
            .requested_blocks
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?
            .len();
        if size > Self::MAX_REQUESTED_BLOCKS_ENTRIES {
            return Err(CasperError::RuntimeError(format!(
                "requested block tracker invariant violated: {size} > {}",
                Self::MAX_REQUESTED_BLOCKS_ENTRIES
            )));
        }
        Ok(())
    }

    /// Creates a new BlockRetriever with shared requested_blocks state.
    ///
    /// # Arguments
    /// * `requested_blocks` - Shared state for tracking block requests (equivalent to Scala implicit RequestedBlocks[F])
    /// * `transport` - Transport layer for network communication
    /// * `connections_cell` - Peer connections
    /// * `conf` - RP configuration
    pub fn new(
        requested_blocks: RequestedBlocks,
        transport: Arc<T>,
        connections_cell: ConnectionsCell,
        conf: RPConf,
    ) -> Self {
        let finalization_certificate_retriever = FinalizationCertificateRetriever::new(
            transport.clone(),
            connections_cell.clone(),
            conf.clone(),
        );
        Self {
            requested_blocks,
            dependency_recovery_last_request: Arc::new(Mutex::new(HashMap::new())),
            broadcast_retry_last_request: Arc::new(Mutex::new(HashMap::new())),
            peer_requery_last_request: Arc::new(Mutex::new(HashMap::new())),
            peer_requery_attempts_by_hash: Arc::new(Mutex::new(HashMap::new())),
            retry_attempts_by_hash: Arc::new(Mutex::new(HashMap::new())),
            retry_budget_quarantine_until: Arc::new(Mutex::new(HashMap::new())),
            finalization_certificate_retriever,
            transport,
            connections_cell,
            conf,
        }
    }

    pub async fn request_finalization_certificate(
        &self,
        digest: BlockHash,
    ) -> Result<CertificateRequestOutcome, CasperError> {
        self.finalization_certificate_retriever
            .request(digest)
            .await
    }

    pub fn finalization_certificate_response_is_expected(
        &self,
        digest: &BlockHash,
    ) -> Result<bool, CasperError> {
        self.finalization_certificate_retriever
            .response_is_expected(digest)
    }

    pub fn complete_finalization_certificate_request(
        &self,
        digest: &BlockHash,
    ) -> Result<(), CasperError> {
        self.finalization_certificate_retriever.complete(digest)
    }

    pub fn retain_active_finalization_certificate_requests(
        &self,
        active: &HashSet<BlockHash>,
    ) -> Result<(), CasperError> {
        self.finalization_certificate_retriever
            .retain_active(active)
    }

    fn has_exceeded_retry_budget(&self, hash: &BlockHash) -> Result<bool, CasperError> {
        let attempts = {
            let retry_attempts = self.retry_attempts_by_hash.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire retry_attempts_by_hash lock".to_string(),
                )
            })?;
            retry_attempts.get(hash).copied().unwrap_or(0)
        };
        Ok(attempts >= Self::MAX_RETRIES_PER_HASH)
    }

    fn retry_attempt_count(&self, hash: &BlockHash) -> Result<u32, CasperError> {
        let retry_attempts = self.retry_attempts_by_hash.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire retry_attempts_by_hash lock".to_string())
        })?;
        Ok(retry_attempts.get(hash).copied().unwrap_or(0))
    }

    fn peer_requery_attempt_count(&self, hash: &BlockHash) -> Result<u32, CasperError> {
        let peer_requery_attempts = self.peer_requery_attempts_by_hash.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire peer_requery_attempts_by_hash lock".to_string(),
            )
        })?;
        Ok(peer_requery_attempts.get(hash).copied().unwrap_or(0))
    }

    fn peer_requery_retry_cooldown_ms_for_hash(
        &self,
        hash: &BlockHash,
    ) -> Result<u64, CasperError> {
        // Back off progressively for hashes that keep failing to resolve, to reduce retry storms.
        let attempts = self.retry_attempt_count(hash)?;
        let base = Self::PEER_REQUERY_COOLDOWN_MS;
        if attempts <= 8 {
            return Ok(base);
        }

        let multiplier = 1 + std::cmp::min(((attempts - 8) / 8) as u64, 4);
        Ok(base.saturating_mul(multiplier))
    }

    fn rerequest_interval_ms_for_hash(
        &self,
        hash: &BlockHash,
        base_interval_ms: u64,
    ) -> Result<u64, CasperError> {
        // Apply adaptive backoff so repeatedly unresolved hashes are retried less aggressively.
        let attempts = self.retry_attempt_count(hash)?;
        if attempts <= 4 {
            return Ok(base_interval_ms);
        }

        let multiplier = 1 + std::cmp::min(((attempts - 4) / 4) as u64, 7);
        Ok(base_interval_ms.saturating_mul(multiplier))
    }

    fn register_retry_attempt(&self, hash: &BlockHash) -> Result<(), CasperError> {
        let mut retry_attempts = self.retry_attempts_by_hash.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire retry_attempts_by_hash lock".to_string())
        })?;
        let counter = retry_attempts.entry(hash.clone()).or_insert(0);
        *counter = counter.saturating_add(1);
        Ok(())
    }

    fn register_peer_requery_attempt(&self, hash: &BlockHash) -> Result<(), CasperError> {
        let mut peer_requery_attempts =
            self.peer_requery_attempts_by_hash.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire peer_requery_attempts_by_hash lock".to_string(),
                )
            })?;
        let counter = peer_requery_attempts.entry(hash.clone()).or_insert(0);
        *counter = counter.saturating_add(1);
        Ok(())
    }

    fn mark_retry_budget_quarantine(&self, hash: &BlockHash, now: u64) -> Result<(), CasperError> {
        let until = now.saturating_add(Self::RETRY_BUDGET_QUARANTINE_MS);
        let mut quarantine = self.retry_budget_quarantine_until.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire retry_budget_quarantine_until lock".to_string(),
            )
        })?;
        quarantine.insert(hash.clone(), until);
        Ok(())
    }

    fn is_retry_budget_quarantined(&self, hash: &BlockHash, now: u64) -> Result<bool, CasperError> {
        let quarantine = self.retry_budget_quarantine_until.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire retry_budget_quarantine_until lock".to_string(),
            )
        })?;
        Ok(quarantine
            .get(hash)
            .copied()
            .is_some_and(|until| now < until))
    }

    /// Get access to the requested_blocks for testing purposes
    pub fn requested_blocks(&self) -> &RequestedBlocks { &self.requested_blocks }

    /// True iff this node asked for the hash to satisfy a missing dependency.
    /// An unsolicited gossip announcement does not qualify.
    pub fn was_requested_as_dependency(&self, hash: &BlockHash) -> Result<bool, CasperError> {
        let state = self.requested_blocks.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
        })?;
        Ok(state
            .get(hash)
            .map(|s| s.requested_as_dependency)
            .unwrap_or(false))
    }

    /// Helper method to add a source peer to an existing request
    fn add_source_peer_to_request(
        init_state: &mut HashMap<BlockHash, RequestState>,
        hash: &BlockHash,
        peer: &PeerNode,
    ) {
        if let Some(request_state) = init_state.get_mut(hash) {
            request_state.waiting_list.push(peer.clone());
        }
    }

    /// Helper method to add a new request
    fn add_new_request(
        init_state: &mut HashMap<BlockHash, RequestState>,
        hash: BlockHash,
        now: u64,
        mark_as_received: bool,
        source_peers: Vec<PeerNode>,
        requested_as_dependency: bool,
    ) -> bool {
        let normalized_waiting_list = {
            let mut deduped = Vec::new();
            let mut seen = HashSet::new();

            for peer in source_peers {
                if deduped.len() >= Self::MAX_WAITING_LIST_PER_HASH {
                    break;
                }

                let id = peer.clone();
                if seen.insert(id.clone()) {
                    deduped.push(peer);
                }
            }
            deduped
        };

        if let Some(existing) = init_state.get_mut(&hash) {
            // Sticky: a hash first heard by gossip and later needed as a
            // dependency must end up marked, or the solicited copy is dropped.
            existing.requested_as_dependency |= requested_as_dependency;
            false // Request already exists
        } else {
            init_state.insert(hash, RequestState {
                timestamp: now,
                initial_timestamp: now,
                peers: HashSet::new(),
                received: mark_as_received,
                in_casper_buffer: false,
                waiting_list: normalized_waiting_list,
                peer_requery_cursor: 0,
                requested_as_dependency,
            });
            true
        }
    }

    fn connected_peers_for_missing_dependency(&self) -> Result<Vec<PeerNode>, CasperError> {
        let connections = self
            .connections_cell
            .read()
            .map_err(|_| CasperError::RuntimeError("Failed to read connections".to_string()))?;

        Ok(connections
            .iter()
            .take(Self::MISSING_DEPENDENCY_SEED_PEERS)
            .cloned()
            .collect())
    }

    fn append_missing_dependency_peers(
        request_state: &mut RequestState,
        candidates: Vec<PeerNode>,
    ) -> usize {
        if request_state.waiting_list.len() >= Self::MAX_WAITING_LIST_PER_HASH {
            return 0;
        }

        let mut added = 0;
        let mut seen = HashSet::new();

        for peer in request_state.waiting_list.iter().cloned() {
            seen.insert(peer);
        }
        for peer in request_state.peers.iter().cloned() {
            seen.insert(peer);
        }

        for candidate in candidates {
            if request_state.waiting_list.len() >= Self::MAX_WAITING_LIST_PER_HASH {
                break;
            }
            if seen.insert(candidate.clone()) {
                request_state.waiting_list.push(candidate);
                added += 1;
            }
        }

        added
    }

    fn pick_next_known_peer(peers: &HashSet<PeerNode>, cursor: &mut u32) -> Option<PeerNode> {
        if peers.is_empty() {
            return None;
        }

        let mut peers_sorted: Vec<_> = peers.iter().cloned().collect();
        peers_sorted.sort_by(|a, b| a.endpoint.host.cmp(&b.endpoint.host));

        let idx = (*cursor as usize) % peers_sorted.len();
        *cursor = cursor.wrapping_add(1);
        peers_sorted.get(idx).cloned()
    }

    /// Get current timestamp in milliseconds
    fn current_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub async fn admit_hash(
        &self,
        hash: BlockHash,
        peer: Option<PeerNode>,
        admit_hash_reason: AdmitHashReason,
    ) -> Result<AdmitHashResult, CasperError> {
        let now = Self::current_millis();
        let missing_dependency_peers = if peer.is_none()
            && matches!(
                admit_hash_reason,
                AdmitHashReason::MissingDependencyRequested
            ) {
            self.connected_peers_for_missing_dependency()?
        } else {
            Vec::new()
        };
        let mut request_from_peer = peer.clone();
        if request_from_peer.is_none() && !missing_dependency_peers.is_empty() {
            request_from_peer = missing_dependency_peers.first().cloned();
        }

        if self.is_retry_budget_quarantined(&hash, now)? {
            debug!(
                "Ignoring {} due to retry-budget quarantine for {}ms.",
                PrettyPrinter::build_string_bytes(&hash),
                Self::RETRY_BUDGET_QUARANTINE_MS
            );
            return Ok(AdmitHashResult {
                status: AdmitHashStatus::Ignore,
                broadcast_request: false,
                request_block: false,
            });
        }

        // Lock the requested_blocks mutex and modify state atomically
        let result = {
            let mut state = self.requested_blocks.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?;

            let unknown_hash = !state.contains_key(&hash);

            if unknown_hash {
                if state.len() >= Self::MAX_REQUESTED_BLOCKS_ENTRIES {
                    metrics::counter!(BLOCK_REQUESTS_CAPACITY_DEFERRED_TOTAL_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).increment(1);
                    AdmitHashResult {
                        status: AdmitHashStatus::Ignore,
                        broadcast_request: false,
                        request_block: false,
                    }
                } else {
                    metrics::counter!(BLOCK_REQUESTS_TOTAL_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).increment(1);
                    let initial_peers = if let Some(peer_node) = peer.clone() {
                        vec![peer_node]
                    } else {
                        missing_dependency_peers
                    };
                    Self::add_new_request(
                        &mut state,
                        hash.clone(),
                        now,
                        false,
                        initial_peers,
                        admit_hash_reason == AdmitHashReason::MissingDependencyRequested,
                    );
                    AdmitHashResult {
                        status: AdmitHashStatus::NewRequestAdded,
                        broadcast_request: request_from_peer.is_none(),
                        request_block: request_from_peer.is_some(),
                    }
                }
            } else if let Some(ref peer_node) = peer {
                // Hash exists, check if peer is already in waiting list
                let request_state = state.get(&hash).unwrap();
                if request_state.received {
                    return Ok(AdmitHashResult {
                        status: AdmitHashStatus::Ignore,
                        broadcast_request: false,
                        request_block: false,
                    });
                }

                let already_waiting = request_state.waiting_list.contains(peer_node);
                let already_queried = request_state.peers.contains(peer_node);
                let waiting_list_full =
                    request_state.waiting_list.len() >= Self::MAX_WAITING_LIST_PER_HASH;
                if already_waiting || already_queried {
                    // Peer is already queued or already queried for this hash, ignore.
                    AdmitHashResult {
                        status: AdmitHashStatus::Ignore,
                        broadcast_request: false,
                        request_block: false,
                    }
                } else if waiting_list_full {
                    debug!(
                        "Ignoring additional source peer for {}: waiting list already at cap {}.",
                        PrettyPrinter::build_string_bytes(&hash),
                        Self::MAX_WAITING_LIST_PER_HASH
                    );
                    AdmitHashResult {
                        status: AdmitHashStatus::Ignore,
                        broadcast_request: false,
                        request_block: false,
                    }
                } else {
                    // Add peer to waiting list
                    let was_empty = request_state.waiting_list.is_empty();
                    Self::add_source_peer_to_request(&mut state, &hash, peer_node);

                    AdmitHashResult {
                        status: AdmitHashStatus::NewSourcePeerAddedToRequest,
                        broadcast_request: false,
                        // Request block if this is the first peer in waiting list
                        request_block: was_empty,
                    }
                }
            } else if matches!(
                admit_hash_reason,
                AdmitHashReason::MissingDependencyRequested
            ) {
                let request_state = state.get_mut(&hash).unwrap();
                if request_state.received {
                    AdmitHashResult {
                        status: AdmitHashStatus::Ignore,
                        broadcast_request: false,
                        request_block: false,
                    }
                } else if request_state.waiting_list.len() >= Self::MAX_WAITING_LIST_PER_HASH {
                    AdmitHashResult {
                        status: AdmitHashStatus::Ignore,
                        broadcast_request: false,
                        request_block: false,
                    }
                } else {
                    let waiting_before = request_state.waiting_list.len();
                    let added = Self::append_missing_dependency_peers(
                        request_state,
                        self.connected_peers_for_missing_dependency()?,
                    );
                    if added == 0 {
                        AdmitHashResult {
                            status: AdmitHashStatus::Ignore,
                            broadcast_request: false,
                            request_block: false,
                        }
                    } else {
                        AdmitHashResult {
                            status: AdmitHashStatus::NewSourcePeerAddedToRequest,
                            broadcast_request: false,
                            request_block: waiting_before == 0,
                        }
                    }
                }
            } else {
                // Hash exists but no peer provided, ignore
                AdmitHashResult {
                    status: AdmitHashStatus::Ignore,
                    broadcast_request: false,
                    request_block: false,
                }
            }
        };

        // Log the result
        match result.status {
            AdmitHashStatus::NewSourcePeerAddedToRequest => {
                if let Some(ref peer_node) = peer {
                    debug!(
                        "Adding {} to waiting list of {} request. Reason: {:?}",
                        peer_node.endpoint.host,
                        PrettyPrinter::build_string_bytes(&hash),
                        admit_hash_reason
                    );
                }
            }
            AdmitHashStatus::NewRequestAdded => {
                info!(
                    "Adding {} hash to RequestedBlocks because of {:?}",
                    PrettyPrinter::build_string_bytes(&hash),
                    admit_hash_reason
                );
            }
            AdmitHashStatus::Ignore => {
                // No logging for ignore case
            }
        }

        // Handle broadcasting and requesting
        if result.broadcast_request {
            self.transport
                .broadcast_has_block_request(&self.connections_cell, &self.conf, &hash)
                .await?;
            debug!(
                "Broadcasted HasBlockRequest for {}",
                PrettyPrinter::build_string_bytes(&hash)
            );
        }

        if result.request_block {
            if let Some(peer_node) = request_from_peer {
                self.transport
                    .request_for_block(&self.conf, &peer_node, hash.clone())
                    .await?;
                debug!(
                    "Requested block {} from {}",
                    PrettyPrinter::build_string_bytes(&hash),
                    peer_node.endpoint.host
                );
            }
        }

        Ok(result)
    }

    pub async fn request_all(&self, age_threshold: Duration) -> Result<(), CasperError> {
        let current_time = Self::current_millis();
        let min_rerequest_interval_ms = Self::MIN_REREQUEST_INTERVAL_MS;
        let stale_request_lifetime_multiplier = Self::STALE_REQUEST_LIFETIME_MULTIPLIER;
        let effective_age_threshold_ms =
            std::cmp::max(age_threshold.as_millis() as u64, min_rerequest_interval_ms);
        let stale_request_lifetime_ms =
            effective_age_threshold_ms.saturating_mul(stale_request_lifetime_multiplier);

        // Get all hashes that need processing
        let hashes_to_process: Vec<BlockHash> = {
            let state = self.requested_blocks.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?;

            debug!(
                "Running BlockRetriever maintenance ({} items unexpired).",
                state.keys().len()
            );

            state.keys().cloned().collect()
        };

        let mut first_dispatch_error = None;

        // Process each hash
        for hash in hashes_to_process {
            // Get the current state for this hash
            let (
                expired,
                received,
                sent_to_casper,
                should_rerequest,
                should_evict_stale,
                rerequest_interval_ms,
            ) = {
                let state = self.requested_blocks.lock().map_err(|_| {
                    CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
                })?;

                if let Some(requested) = state.get(&hash) {
                    let rerequest_interval_ms =
                        self.rerequest_interval_ms_for_hash(&hash, effective_age_threshold_ms)?;
                    let expired =
                        current_time.saturating_sub(requested.timestamp) > rerequest_interval_ms;
                    let received = requested.received;
                    let sent_to_casper = requested.in_casper_buffer;
                    let stale_lifetime = current_time.saturating_sub(requested.initial_timestamp);
                    // Only apply lifetime-based eviction to entries already marked as received.
                    // Unresolved requests must remain tracked until retry-budget/bounds logic
                    // decides eviction, otherwise dependency chains can be dropped prematurely.
                    let should_evict_stale = received && stale_lifetime > stale_request_lifetime_ms;

                    if !received {
                        debug!(
                            "Casper loop: checking if should re-request {}. Received: {}. rerequest_interval_ms={}.",
                            PrettyPrinter::build_string_bytes(&hash),
                            received,
                            rerequest_interval_ms
                        );
                    }

                    (
                        expired,
                        received,
                        sent_to_casper,
                        !received && expired,
                        should_evict_stale,
                        rerequest_interval_ms,
                    )
                } else {
                    continue; // Hash was removed, skip
                }
            };

            // Try to re-request if needed
            if should_rerequest {
                if self.has_exceeded_retry_budget(&hash)? {
                    let mut state = self.requested_blocks.lock().map_err(|_| {
                        CasperError::RuntimeError(
                            "Failed to acquire requested_blocks lock".to_string(),
                        )
                    })?;
                    if state.remove(&hash).is_some() {
                        let now = Self::current_millis();
                        drop(state);
                        self.mark_retry_budget_quarantine(&hash, now)?;
                        self.cleanup_aux_tracking_for_hash(&hash)?;
                        metrics::counter!(BLOCK_REQUESTS_STALE_EVICTIONS_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "reason" => "retry_budget").increment(1);
                        debug!(
                            "Evicting unresolved block request {} after reaching retry budget {}. Quarantine for {}ms.",
                            PrettyPrinter::build_string_bytes(&hash),
                            Self::MAX_RETRIES_PER_HASH,
                            Self::RETRY_BUDGET_QUARANTINE_MS
                        );
                    }
                    continue;
                }

                let did_retry = match self.try_rerequest(&hash).await {
                    Ok(did_retry) => did_retry,
                    Err(error) => {
                        first_dispatch_error.get_or_insert(error);
                        continue;
                    }
                };
                if did_retry {
                    self.register_retry_attempt(&hash)?;
                    metrics::counter!(BLOCK_REQUESTS_RETRIES_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).increment(1);
                } else {
                    debug!(
                        "Skipped retry for {} (interval={}ms, likely cooldown-suppressed action).",
                        PrettyPrinter::build_string_bytes(&hash),
                        rerequest_interval_ms
                    );
                }
            }

            // Remove expired entries that are already received.
            // Unresolved entries are governed by retry-budget and requested-blocks bounds.
            if (received && expired) || should_evict_stale {
                let mut state = self.requested_blocks.lock().map_err(|_| {
                    CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
                })?;
                if state.remove(&hash).is_some() {
                    drop(state);
                    self.cleanup_aux_tracking_for_hash(&hash)?;
                    if received && expired && !sent_to_casper {
                        debug!(
                            "Evicting received/non-buffered block request {} after timeout.",
                            PrettyPrinter::build_string_bytes(&hash)
                        );
                    }
                }
            }
        }

        // Keep cooldown-tracking maps bounded to active requested hashes.
        self.verify_requested_blocks_bound()?;
        self.sweep_orphaned_aux_tracking()?;
        self.sweep_expired_retry_budget_quarantine(current_time)?;
        self.update_aux_tracking_metrics()?;
        if let Err(error) = self.finalization_certificate_retriever.request_all().await {
            first_dispatch_error.get_or_insert(error);
        }

        first_dispatch_error.map_or(Ok(()), Err)
    }

    /// Force dependency recovery by reopening request state and rebroadcasting HasBlockRequest.
    /// This is used when the processor detects buffered dependency deadlocks.
    pub async fn recover_dependency(&self, hash: BlockHash) -> Result<(), CasperError> {
        let now = Self::current_millis();

        if self.is_retry_budget_quarantined(&hash, now)? {
            debug!(
                "Skipping dependency recovery for {} due to retry-budget quarantine ({}ms).",
                PrettyPrinter::build_string_bytes(&hash),
                Self::RETRY_BUDGET_QUARANTINE_MS
            );
            return Ok(());
        }

        if self.has_exceeded_retry_budget(&hash)? {
            let mut state = self.requested_blocks.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?;
            if state.remove(&hash).is_some() {
                drop(state);
                self.mark_retry_budget_quarantine(&hash, now)?;
                self.cleanup_aux_tracking_for_hash(&hash)?;
                metrics::counter!(
                    BLOCK_REQUESTS_STALE_EVICTIONS_METRIC,
                    "source" => BLOCK_RETRIEVER_METRICS_SOURCE,
                    "reason" => "retry_budget_recovery"
                )
                .increment(1);
                debug!(
                    "Evicting dependency {} during recovery after retry budget exhaustion. Quarantine for {}ms.",
                    PrettyPrinter::build_string_bytes(&hash),
                    Self::RETRY_BUDGET_QUARANTINE_MS
                );
            }
            return Ok(());
        }

        let dependency_recovery_rerequest_cooldown_ms = Self::DEPENDENCY_RECOVERY_COOLDOWN_MS;

        {
            let mut last_requests = self.dependency_recovery_last_request.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire dependency_recovery_last_request lock".to_string(),
                )
            })?;

            if let Some(last_ts) = last_requests.get(&hash) {
                if now.saturating_sub(*last_ts) < dependency_recovery_rerequest_cooldown_ms {
                    debug!(
                        "Skipping dependency recovery re-request for {} (cooldown {}ms)",
                        PrettyPrinter::build_string_bytes(&hash),
                        dependency_recovery_rerequest_cooldown_ms
                    );
                    return Ok(());
                }
            }

            last_requests.insert(hash.clone(), now);
        }

        {
            let mut state = self.requested_blocks.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?;

            if let Some(request_state) = state.get_mut(&hash) {
                request_state.received = false;
                request_state.in_casper_buffer = false;
                request_state.timestamp = now;
            }
        }

        let admit_result = self
            .admit_hash(
                hash.clone(),
                None,
                AdmitHashReason::MissingDependencyRequested,
            )
            .await?;
        if matches!(admit_result.status, AdmitHashStatus::Ignore) {
            self.transport
                .broadcast_has_block_request(&self.connections_cell, &self.conf, &hash)
                .await?;
        }

        self.register_retry_attempt(&hash)?;
        metrics::counter!(BLOCK_REQUESTS_RETRIES_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE)
            .increment(1);

        info!(
            "Recovery re-request issued for dependency {}",
            PrettyPrinter::build_string_bytes(&hash)
        );
        self.update_aux_tracking_metrics()?;

        Ok(())
    }

    pub async fn defer_for_admission(
        &self,
        hash: BlockHash,
        source_peer: Option<PeerNode>,
    ) -> Result<bool, CasperError> {
        let now = Self::current_millis();
        let tracked = {
            let mut state = self.requested_blocks.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?;
            if let Some(request) = state.get_mut(&hash) {
                request.received = false;
                request.in_casper_buffer = false;
                request.timestamp = now;
                if let Some(peer) = source_peer {
                    let known = request.peers.contains(&peer)
                        || request
                            .waiting_list
                            .iter()
                            .any(|candidate| candidate == &peer);
                    if !known && request.waiting_list.len() < Self::MAX_WAITING_LIST_PER_HASH {
                        request.waiting_list.push(peer);
                    }
                }
                true
            } else if state.len() >= Self::MAX_REQUESTED_BLOCKS_ENTRIES {
                metrics::counter!(BLOCK_REQUESTS_CAPACITY_DEFERRED_TOTAL_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).increment(1);
                false
            } else {
                Self::add_new_request(
                    &mut state,
                    hash.clone(),
                    now,
                    false,
                    source_peer.into_iter().collect(),
                    true,
                );
                true
            }
        };
        self.cleanup_aux_tracking_for_hash(&hash)?;
        self.verify_requested_blocks_bound()?;
        self.update_aux_tracking_metrics()?;
        Ok(tracked)
    }

    /// Helper method to try re-requesting a block from the next peer in waiting list
    async fn try_rerequest(&self, hash: &BlockHash) -> Result<bool, CasperError> {
        enum RerequestAction {
            RequestPeer(PeerNode, Vec<PeerNode>),
            RequestKnownPeer(PeerNode, u64),
            BroadcastOnly(u64),
            None,
        }

        let peer_requery_attempts = self.peer_requery_attempt_count(hash)?;
        let known_peer_requery_soft_limit = Self::KNOWN_PEER_REQUERY_SOFT_LIMIT;

        // Determine retry action and update request timestamp only when a network request is attempted.
        let action = {
            let now = Self::current_millis();
            let mut state = self.requested_blocks.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?;

            if let Some(request_state) = state.get_mut(hash) {
                if !request_state.waiting_list.is_empty() {
                    let next_peer = request_state.waiting_list.remove(0);
                    request_state.peers.insert(next_peer.clone());
                    request_state.timestamp = now;
                    RerequestAction::RequestPeer(next_peer, request_state.waiting_list.clone())
                } else if let Some(known_peer) = Self::pick_next_known_peer(
                    &request_state.peers,
                    &mut request_state.peer_requery_cursor,
                ) {
                    request_state.timestamp = now;
                    let known_peer_count = request_state.peers.len() as u32;
                    let peer_requery_budget = std::cmp::max(
                        1,
                        std::cmp::min(known_peer_requery_soft_limit, known_peer_count),
                    );
                    // Budget based on known-peer requery attempts only.
                    // Using total retries here incorrectly consumes budget with waiting-list peer requests.
                    if peer_requery_attempts < peer_requery_budget {
                        RerequestAction::RequestKnownPeer(known_peer, now)
                    } else {
                        // After repeated misses with known peers, switch to broadcast-only
                        // retries to discover fresh peers and avoid known-peer retry storms.
                        RerequestAction::BroadcastOnly(now)
                    }
                } else {
                    request_state.timestamp = now;
                    RerequestAction::BroadcastOnly(now)
                }
            } else {
                RerequestAction::None
            }
        };

        match action {
            RerequestAction::RequestPeer(next_peer, remaining_waiting) => {
                metrics::counter!(BLOCK_REQUESTS_RETRY_ACTION_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "action" => "peer_request").increment(1);
                debug!(
                    "Trying {} to query for {} block. Remain waiting: {}.",
                    next_peer.endpoint.host,
                    PrettyPrinter::build_string_bytes(hash),
                    remaining_waiting
                        .iter()
                        .map(|p| p.endpoint.host.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                );

                // Request block from the peer
                self.transport
                    .request_for_block(&self.conf, &next_peer, hash.clone())
                    .await?;

                // If this was the last peer in the waiting list, also broadcast HasBlockRequest.
                if remaining_waiting.is_empty() {
                    debug!(
                        "Last peer in waiting list for block {}. Broadcasting HasBlockRequest.",
                        PrettyPrinter::build_string_bytes(hash)
                    );

                    self.transport
                        .broadcast_has_block_request(&self.connections_cell, &self.conf, hash)
                        .await?;
                }
                Ok(true)
            }
            RerequestAction::BroadcastOnly(now) => {
                metrics::counter!(BLOCK_REQUESTS_RETRY_ACTION_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "action" => "broadcast_only").increment(1);
                let broadcast_only_retry_cooldown_ms =
                    self.broadcast_retry_cooldown_ms_for_hash(hash)?;
                let is_suppressed = {
                    let mut state = self.broadcast_retry_last_request.lock().map_err(|_| {
                        CasperError::RuntimeError(
                            "Failed to acquire broadcast_retry_last_request lock".to_string(),
                        )
                    })?;
                    if let Some(last) = state.get(hash) {
                        if now.saturating_sub(*last) < broadcast_only_retry_cooldown_ms {
                            true
                        } else {
                            state.insert(hash.clone(), now);
                            false
                        }
                    } else {
                        state.insert(hash.clone(), now);
                        false
                    }
                };

                if is_suppressed {
                    metrics::counter!(BLOCK_REQUESTS_RETRY_ACTION_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "action" => "broadcast_suppressed").increment(1);
                    debug!(
                        "Suppressing HasBlockRequest broadcast for {} due to cooldown {}ms.",
                        PrettyPrinter::build_string_bytes(hash),
                        broadcast_only_retry_cooldown_ms
                    );
                    return Ok(false);
                }

                debug!(
                    "No peers in waiting list for block {}. Broadcasting HasBlockRequest.",
                    PrettyPrinter::build_string_bytes(hash)
                );
                self.transport
                    .broadcast_has_block_request(&self.connections_cell, &self.conf, hash)
                    .await?;
                Ok(true)
            }
            RerequestAction::RequestKnownPeer(known_peer, now) => {
                metrics::counter!(BLOCK_REQUESTS_RETRY_ACTION_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "action" => "peer_requery").increment(1);
                let peer_requery_retry_cooldown_ms =
                    self.peer_requery_retry_cooldown_ms_for_hash(hash)?;
                let is_suppressed = {
                    let mut state = self.peer_requery_last_request.lock().map_err(|_| {
                        CasperError::RuntimeError(
                            "Failed to acquire peer_requery_last_request lock".to_string(),
                        )
                    })?;
                    if let Some(last) = state.get(hash) {
                        if now.saturating_sub(*last) < peer_requery_retry_cooldown_ms {
                            true
                        } else {
                            state.insert(hash.clone(), now);
                            false
                        }
                    } else {
                        state.insert(hash.clone(), now);
                        false
                    }
                };

                if is_suppressed {
                    metrics::counter!(BLOCK_REQUESTS_RETRY_ACTION_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "action" => "peer_requery_suppressed").increment(1);
                    debug!(
                        "Suppressing peer requery for {} due to cooldown {}ms.",
                        PrettyPrinter::build_string_bytes(hash),
                        peer_requery_retry_cooldown_ms
                    );
                    return Ok(false);
                }

                debug!(
                    "Re-querying known peer {} for block {}.",
                    known_peer.endpoint.host,
                    PrettyPrinter::build_string_bytes(hash)
                );
                self.transport
                    .request_for_block(&self.conf, &known_peer, hash.clone())
                    .await?;
                self.register_peer_requery_attempt(hash)?;
                Ok(true)
            }
            RerequestAction::None => {
                metrics::counter!(BLOCK_REQUESTS_RETRY_ACTION_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "action" => "none").increment(1);
                Ok(false)
            }
        }
    }

    pub async fn ack_receive(&self, hash: BlockHash) -> Result<(), CasperError> {
        let now = Self::current_millis();

        // Lock the requested_blocks mutex and modify state atomically
        let (result, request_timestamp) = {
            let mut state = self.requested_blocks.lock().map_err(|_| {
                CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
            })?;

            match state.get(&hash) {
                // There might be blocks that are not maintained by RequestedBlocks, e.g. fork-choice tips
                None => {
                    if state.len() >= Self::MAX_REQUESTED_BLOCKS_ENTRIES {
                        metrics::counter!(BLOCK_REQUESTS_CAPACITY_DEFERRED_TOTAL_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).increment(1);
                        (AckReceiveResult::UntrackedAtCapacity, None)
                    } else {
                        Self::add_new_request(
                            &mut state,
                            hash.clone(),
                            now,
                            true,
                            Vec::new(),
                            false,
                        );
                        (AckReceiveResult::AddedAsReceived, None)
                    }
                }
                Some(requested) => {
                    let initial_timestamp = requested.initial_timestamp;
                    // Make Casper loop aware that the block has been received
                    let mut updated_request = requested.clone();
                    updated_request.received = true;
                    state.insert(hash.clone(), updated_request);
                    (AckReceiveResult::MarkedAsReceived, Some(initial_timestamp))
                }
            }
        };

        // Record block download end-to-end time if we have the original request timestamp
        if let Some(timestamp) = request_timestamp {
            let download_time_ms = now.saturating_sub(timestamp);
            let download_time_seconds = download_time_ms as f64 / 1000.0;
            metrics::histogram!(BLOCK_DOWNLOAD_END_TO_END_TIME_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE)
                .record(download_time_seconds);
        }

        // Log based on the result
        match result {
            AckReceiveResult::AddedAsReceived => {
                info!(
                    "Block {} is not in RequestedBlocks. Adding and marking received.",
                    PrettyPrinter::build_string_bytes(&hash)
                );
            }
            AckReceiveResult::MarkedAsReceived => {
                info!(
                    "Block {} marked as received.",
                    PrettyPrinter::build_string_bytes(&hash)
                );
            }
            AckReceiveResult::UntrackedAtCapacity => {
                debug!(
                    "Block {} was received without tracker capacity; processing ownership remains in the block queue.",
                    PrettyPrinter::build_string_bytes(&hash)
                );
            }
        }

        self.cleanup_aux_tracking_for_hash(&hash)?;

        Ok(())
    }

    pub async fn ack_in_casper(&self, hash: BlockHash) -> Result<(), CasperError> {
        // Check if block is already received
        let is_received = self.is_received(hash.clone()).await?;

        // If not received, acknowledge receipt first
        if !is_received {
            self.ack_receive(hash.clone()).await?;
        }

        // Block is now being processed by Casper; no longer needs to remain tracked by
        // BlockRetriever.
        self.cleanup_hash_tracking(&hash)?;

        Ok(())
    }

    /// Explicitly stop tracking a hash when it is no longer required by CasperBuffer dependency graph.
    pub fn forget_hash_tracking(&self, hash: &BlockHash) -> Result<(), CasperError> {
        self.cleanup_hash_tracking(hash)
    }

    pub async fn is_received(&self, hash: BlockHash) -> Result<bool, CasperError> {
        let state = self.requested_blocks.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
        })?;

        match state.get(&hash) {
            Some(request_state) => Ok(request_state.received),
            None => Ok(false),
        }
    }

    /// Get the number of peers in the waiting list for a specific hash
    /// Returns 0 if the hash is not in requested blocks
    pub async fn get_waiting_list_size(&self, hash: &BlockHash) -> Result<usize, CasperError> {
        let state = self.requested_blocks.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
        })?;

        match state.get(hash) {
            Some(request_state) => Ok(request_state.waiting_list.len()),
            None => Ok(0),
        }
    }

    /// Get the total number of hashes being tracked in requested blocks
    pub async fn get_requested_blocks_count(&self) -> Result<usize, CasperError> {
        let state = self.requested_blocks.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
        })?;

        Ok(state.len())
    }

    /// Test-only helper methods for setting up specific test scenarios
    pub async fn set_request_state_for_test(
        &self,
        hash: BlockHash,
        request_state: RequestState,
    ) -> Result<(), CasperError> {
        let mut state = self.requested_blocks.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
        })?;

        state.insert(hash, request_state);
        Ok(())
    }

    /// Test-only helper to get request state for verification
    pub async fn get_request_state_for_test(
        &self,
        hash: &BlockHash,
    ) -> Result<Option<RequestState>, CasperError> {
        let state = self.requested_blocks.lock().map_err(|_| {
            CasperError::RuntimeError("Failed to acquire requested_blocks lock".to_string())
        })?;

        Ok(state.get(hash).cloned())
    }

    /// Test-only helper to create a timed out timestamp
    pub fn create_timed_out_timestamp(timeout: std::time::Duration) -> u64 {
        let now = Self::current_millis();
        now.saturating_sub((2 * timeout.as_millis()) as u64)
    }
}

#[cfg(test)]
mod tests {
    use comm::rust::errors::CommError;
    use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
    use comm::rust::rp::connect::{Connections, ConnectionsCell};
    use comm::rust::rp::protocol_helper;
    use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
    use prost::bytes::Bytes;

    use super::*;

    fn peer_node(name: &str, port: u16) -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from(name.as_bytes().to_vec()),
            },
            endpoint: Endpoint {
                host: "host".to_string(),
                tcp_port: port as u32,
                udp_port: port as u32,
            },
        }
    }

    #[tokio::test]
    async fn failed_block_retry_does_not_suppress_certificate_maintenance() {
        let local = peer_node("local", 40400);
        let rp_conf = create_rp_conf_ask(local.clone(), None, None);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![local]))),
        };
        let requested_blocks: RequestedBlocks = Arc::new(Mutex::new(HashMap::new()));
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(
            requested_blocks,
            transport.clone(),
            connections_cell,
            rp_conf,
        );
        let block_hash = Bytes::from(vec![1; models::rust::block_hash::LENGTH]);
        let certificate_digest = Bytes::from(vec![2; models::rust::block_hash::LENGTH]);
        let stale = BlockRetriever::<TransportLayerStub>::create_timed_out_timestamp(
            Duration::from_secs(2),
        );
        block_retriever
            .set_request_state_for_test(block_hash.clone(), RequestState {
                timestamp: stale,
                initial_timestamp: stale,
                peers: HashSet::new(),
                received: false,
                in_casper_buffer: false,
                waiting_list: Vec::new(),
                peer_requery_cursor: 0,
                requested_as_dependency: false,
            })
            .await
            .unwrap();
        block_retriever
            .request_finalization_certificate(certificate_digest.clone())
            .await
            .unwrap();
        block_retriever
            .finalization_certificate_retriever
            .make_retry_ready(&certificate_digest)
            .unwrap();
        transport.reset();
        transport.set_responses(|_, _| Err(CommError::TimeOut));

        assert!(block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .is_err());
        let packet_types = transport
            .get_all_requests()
            .into_iter()
            .map(|request| protocol_helper::to_packet(&request.msg).unwrap().type_id)
            .collect::<HashSet<_>>();
        assert!(packet_types.contains("HasBlockRequest"));
        assert!(packet_types.contains("FinalizationCertificateRequest"));
        assert!(block_retriever
            .finalization_certificate_response_is_expected(&certificate_digest)
            .unwrap());
    }

    #[tokio::test]
    async fn ack_in_casper_is_idempotent_and_releases_request_tracking() {
        let local = peer_node("local", 40400);
        let rp_conf = create_rp_conf_ask(local.clone(), None, None);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![local]))),
        };
        let requested_blocks: RequestedBlocks = Arc::new(Mutex::new(HashMap::new()));
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever =
            BlockRetriever::new(requested_blocks, transport, connections_cell, rp_conf);
        let block_hash = Bytes::from(vec![3; models::rust::block_hash::LENGTH]);

        block_retriever
            .ack_receive(block_hash.clone())
            .await
            .expect("receipt should be tracked");
        assert!(block_retriever
            .get_request_state_for_test(&block_hash)
            .await
            .expect("request lookup should succeed")
            .is_some());

        block_retriever
            .ack_in_casper(block_hash.clone())
            .await
            .expect("first acknowledgement should release tracking");
        block_retriever
            .ack_in_casper(block_hash.clone())
            .await
            .expect("duplicate acknowledgement should remain safe");

        assert!(block_retriever
            .get_request_state_for_test(&block_hash)
            .await
            .expect("request lookup should succeed")
            .is_none());
    }

    #[tokio::test]
    async fn request_all_should_keep_unresolved_request_tracked_until_retry_budget() {
        let local = peer_node("local", 40400);
        let rp_conf = create_rp_conf_ask(local.clone(), None, None);
        let connections = Connections::from_vec(vec![local]);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(connections)),
        };
        let requested_blocks: RequestedBlocks = Arc::new(Mutex::new(HashMap::new()));
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(
            requested_blocks.clone(),
            transport,
            connections_cell,
            rp_conf,
        );

        let hash: BlockHash = Bytes::from_static(b"stale-unresolved-hash");
        let now = BlockRetriever::<TransportLayerStub>::current_millis();
        let stale_initial = now.saturating_sub(120_000);

        block_retriever
            .set_request_state_for_test(hash.clone(), RequestState {
                timestamp: stale_initial,
                initial_timestamp: stale_initial,
                peers: HashSet::new(),
                received: false,
                in_casper_buffer: false,
                waiting_list: Vec::new(),
                peer_requery_cursor: 0,
                requested_as_dependency: false,
            })
            .await
            .expect("should seed request state");

        block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .expect("maintenance should complete");

        let state = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .expect("state lookup should succeed");
        assert!(
            state.is_some(),
            "unresolved request must remain tracked; only retry-budget/bounds may evict it"
        );
    }

    #[tokio::test]
    async fn recover_dependency_should_seed_connected_peers_for_missing_dependency() {
        let local = peer_node("local", 40400);
        let remote = peer_node("remote", 40401);
        let rp_conf = create_rp_conf_ask(local, None, None);
        let connections = Connections::from_vec(vec![remote.clone()]);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(connections)),
        };
        let requested_blocks: RequestedBlocks = Arc::new(Mutex::new(HashMap::new()));
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(
            requested_blocks.clone(),
            transport.clone(),
            connections_cell,
            rp_conf,
        );

        let hash: BlockHash = Bytes::from_static(b"recover-dependency-hash");
        block_retriever
            .recover_dependency(hash.clone())
            .await
            .expect("recover_dependency should complete");

        let state = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .expect("state lookup should succeed")
            .expect("request state should exist");
        assert!(
            !state.waiting_list.is_empty(),
            "recover_dependency should seed known connected peers"
        );
        assert_eq!(
            transport.request_count(),
            1,
            "recover_dependency should issue a direct request when a connected peer exists"
        );
    }

    #[tokio::test]
    async fn admission_deferral_reopens_received_state_without_immediate_network_retry() {
        let local = peer_node("local", 40400);
        let source = peer_node("source", 40401);
        let rp_conf = create_rp_conf_ask(local, None, None);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![source.clone()]))),
        };
        let requested_blocks: RequestedBlocks = Arc::new(Mutex::new(HashMap::new()));
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(
            requested_blocks,
            transport.clone(),
            connections_cell,
            rp_conf,
        );
        let hash: BlockHash = Bytes::from_static(b"admission-deferred-hash");

        block_retriever
            .ack_receive(hash.clone())
            .await
            .expect("receipt should be recorded");
        for _ in 0..BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH {
            block_retriever
                .register_retry_attempt(&hash)
                .expect("retry attempt should be recorded");
        }
        block_retriever
            .mark_retry_budget_quarantine(
                &hash,
                BlockRetriever::<TransportLayerStub>::current_millis(),
            )
            .expect("quarantine should be recorded");

        block_retriever
            .defer_for_admission(hash.clone(), Some(source.clone()))
            .await
            .expect("admission deferral should complete");

        let state = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .expect("state lookup should succeed")
            .expect("deferred request should remain tracked");
        assert!(!state.received);
        assert!(!state.in_casper_buffer);
        assert!(state.waiting_list.contains(&source));
        assert!(!block_retriever
            .has_exceeded_retry_budget(&hash)
            .expect("retry budget lookup should succeed"));
        assert!(!block_retriever
            .is_retry_budget_quarantined(
                &hash,
                BlockRetriever::<TransportLayerStub>::current_millis(),
            )
            .expect("quarantine lookup should succeed"));
        assert_eq!(transport.request_count(), 0);
    }

    #[tokio::test]
    async fn admission_deferral_creates_request_state_for_unsolicited_block() {
        let local = peer_node("local", 40400);
        let source = peer_node("source", 40401);
        let rp_conf = create_rp_conf_ask(local, None, None);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![source.clone()]))),
        };
        let requested_blocks: RequestedBlocks = Arc::new(Mutex::new(HashMap::new()));
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever =
            BlockRetriever::new(requested_blocks, transport, connections_cell, rp_conf);
        let hash: BlockHash = Bytes::from_static(b"unsolicited-admission-deferred-hash");

        block_retriever
            .defer_for_admission(hash.clone(), Some(source.clone()))
            .await
            .expect("admission deferral should create state");

        let state = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .expect("state lookup should succeed")
            .expect("deferred request should exist");
        assert!(!state.received);
        assert_eq!(state.waiting_list, vec![source]);
    }

    #[tokio::test]
    async fn request_capacity_preserves_existing_work_and_defers_new_hashes() {
        let local = peer_node("local", 40400);
        let source = peer_node("source", 40401);
        let rp_conf = create_rp_conf_ask(local, None, None);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![source.clone()]))),
        };
        let requested_blocks: RequestedBlocks = Arc::new(Mutex::new(HashMap::new()));
        let existing = Bytes::from_static(b"existing-request");
        {
            let mut state = requested_blocks.lock().expect("request state");
            BlockRetriever::<TransportLayerStub>::add_new_request(
                &mut state,
                existing.clone(),
                1,
                false,
                vec![source.clone()],
                false,
            );
            for index in 1..BlockRetriever::<TransportLayerStub>::MAX_REQUESTED_BLOCKS_ENTRIES {
                BlockRetriever::<TransportLayerStub>::add_new_request(
                    &mut state,
                    Bytes::from(index.to_be_bytes().to_vec()),
                    index as u64 + 1,
                    false,
                    Vec::new(),
                    false,
                );
            }
        }
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(
            requested_blocks.clone(),
            transport,
            connections_cell,
            rp_conf,
        );
        let new_hash: BlockHash = Bytes::from_static(b"capacity-deferred-hash");

        let result = block_retriever
            .admit_hash(
                new_hash.clone(),
                Some(source.clone()),
                AdmitHashReason::HashBroadcastReceived,
            )
            .await
            .expect("capacity admission decision");
        assert_eq!(result.status, AdmitHashStatus::Ignore);
        assert!(block_retriever
            .defer_for_admission(existing.clone(), Some(source.clone()))
            .await
            .expect("existing deferral"));
        assert!(!block_retriever
            .defer_for_admission(new_hash.clone(), Some(source))
            .await
            .expect("new deferral"));
        block_retriever
            .ack_receive(new_hash.clone())
            .await
            .expect("untracked receipt");

        let state = requested_blocks.lock().expect("request state");
        assert_eq!(
            state.len(),
            BlockRetriever::<TransportLayerStub>::MAX_REQUESTED_BLOCKS_ENTRIES
        );
        assert!(state.contains_key(&existing));
        assert!(!state.contains_key(&new_hash));
    }

    #[tokio::test]
    async fn forget_hash_tracking_should_remove_unresolved_request_state() {
        let local = peer_node("local", 40400);
        let rp_conf = create_rp_conf_ask(local.clone(), None, None);
        let connections = Connections::from_vec(vec![local]);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(connections)),
        };
        let requested_blocks: RequestedBlocks = Arc::new(Mutex::new(HashMap::new()));
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(
            requested_blocks.clone(),
            transport,
            connections_cell,
            rp_conf,
        );

        let hash: BlockHash = Bytes::from_static(b"orphan-dependency-hash");
        let now = BlockRetriever::<TransportLayerStub>::current_millis();
        block_retriever
            .set_request_state_for_test(hash.clone(), RequestState {
                timestamp: now,
                initial_timestamp: now,
                peers: HashSet::new(),
                received: false,
                in_casper_buffer: false,
                waiting_list: Vec::new(),
                peer_requery_cursor: 0,
                requested_as_dependency: false,
            })
            .await
            .expect("should seed request state");

        block_retriever
            .forget_hash_tracking(&hash)
            .expect("cleanup should succeed");

        let state = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .expect("state lookup should succeed");
        assert!(
            state.is_none(),
            "orphan dependency hash should be fully untracked"
        );
    }

    #[tokio::test]
    async fn single_known_peer_should_not_be_requeried_more_than_once_before_broadcast() {
        let local = peer_node("local", 40400);
        let remote = peer_node("remote", 40401);
        let rp_conf = create_rp_conf_ask(local, None, None);
        let connections = Connections::from_vec(vec![]);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(connections)),
        };
        let requested_blocks: RequestedBlocks = Arc::new(Mutex::new(HashMap::new()));
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(
            requested_blocks.clone(),
            transport.clone(),
            connections_cell,
            rp_conf,
        );

        let hash: BlockHash = Bytes::from_static(b"single-known-peer-requery-budget");
        let stale = BlockRetriever::<TransportLayerStub>::create_timed_out_timestamp(
            Duration::from_secs(2),
        );
        let mut peers = HashSet::new();
        peers.insert(remote);
        block_retriever
            .set_request_state_for_test(hash.clone(), RequestState {
                timestamp: stale,
                initial_timestamp: stale,
                peers,
                received: false,
                in_casper_buffer: false,
                waiting_list: Vec::new(),
                peer_requery_cursor: 0,
                requested_as_dependency: false,
            })
            .await
            .expect("should seed request state");

        block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .expect("first maintenance should complete");
        assert_eq!(
            transport.request_count(),
            1,
            "first retry should requery the single known peer once"
        );

        let mut state = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .expect("state lookup should succeed")
            .expect("request state should still exist");
        state.timestamp = BlockRetriever::<TransportLayerStub>::create_timed_out_timestamp(
            Duration::from_secs(2),
        );
        block_retriever
            .set_request_state_for_test(hash.clone(), state)
            .await
            .expect("should refresh timeout");

        block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .expect("second maintenance should complete");
        assert_eq!(
            transport.request_count(),
            1,
            "second retry should switch to broadcast-only (no direct peer requery)"
        );
    }

    #[tokio::test]
    async fn waiting_list_exhaustion_should_still_allow_a_known_peer_requery() {
        let local = peer_node("local", 40400);
        let waiting_peer = peer_node("waiting", 40401);
        let rp_conf = create_rp_conf_ask(local, None, None);
        let connections = Connections::from_vec(vec![]);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(connections)),
        };
        let requested_blocks: RequestedBlocks = Arc::new(Mutex::new(HashMap::new()));
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(
            requested_blocks.clone(),
            transport.clone(),
            connections_cell,
            rp_conf,
        );

        let hash: BlockHash = Bytes::from_static(b"waiting-list-exhaustion-known-peer-requery");
        let stale = BlockRetriever::<TransportLayerStub>::create_timed_out_timestamp(
            Duration::from_secs(2),
        );
        block_retriever
            .set_request_state_for_test(hash.clone(), RequestState {
                timestamp: stale,
                initial_timestamp: stale,
                peers: HashSet::new(),
                received: false,
                in_casper_buffer: false,
                waiting_list: vec![waiting_peer.clone()],
                peer_requery_cursor: 0,
                requested_as_dependency: false,
            })
            .await
            .expect("should seed request state");

        block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .expect("first maintenance should complete");
        let first_count = transport.request_count();
        assert_eq!(
            first_count, 1,
            "first retry should request from waiting peer (broadcast may be a no-op when no connections)"
        );

        let mut state = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .expect("state lookup should succeed")
            .expect("request state should still exist");
        state.timestamp = BlockRetriever::<TransportLayerStub>::create_timed_out_timestamp(
            Duration::from_secs(2),
        );
        block_retriever
            .set_request_state_for_test(hash.clone(), state)
            .await
            .expect("should refresh timeout");

        block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .expect("second maintenance should complete");

        let second_count = transport.request_count();
        assert_eq!(
            second_count,
            first_count + 1,
            "second retry should add exactly one known-peer direct request"
        );

        let (recipient, protocol) = transport
            .get_request(first_count)
            .expect("second retry request should exist and target known peer");
        assert_eq!(recipient, waiting_peer);
        let packet = crate::rust::protocol::extract_packet_from_protocol(&protocol)
            .expect("packet should decode");
        crate::rust::protocol::verify_block_request(&packet, &hash)
            .expect("known-peer requery should be a direct BlockRequest");
    }

    fn retriever(
        connected: Vec<PeerNode>,
    ) -> (BlockRetriever<TransportLayerStub>, Arc<TransportLayerStub>) {
        let local = peer_node("local", 40400);
        let rp_conf = create_rp_conf_ask(local, None, None);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(connected))),
        };
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(
            Arc::new(Mutex::new(HashMap::new())),
            transport.clone(),
            connections_cell,
            rp_conf,
        );
        (block_retriever, transport)
    }

    fn fresh_state(now: u64) -> RequestState {
        RequestState {
            timestamp: now,
            initial_timestamp: now,
            peers: HashSet::new(),
            received: false,
            in_casper_buffer: false,
            waiting_list: Vec::new(),
            peer_requery_cursor: 0,
            requested_as_dependency: false,
        }
    }

    #[tokio::test]
    async fn admit_hash_with_a_peer_adds_a_request_and_asks_that_peer() {
        let (block_retriever, transport) = retriever(vec![]);
        let source = peer_node("source", 40401);
        let hash: BlockHash = Bytes::from_static(b"admit-new-hash-with-peer");

        let result = block_retriever
            .admit_hash(
                hash.clone(),
                Some(source.clone()),
                AdmitHashReason::HashBroadcastReceived,
            )
            .await
            .expect("admit should succeed");

        assert_eq!(result.status, AdmitHashStatus::NewRequestAdded);
        assert!(result.request_block);
        assert!(!result.broadcast_request);
        assert_eq!(transport.request_count(), 1);

        let state = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .unwrap()
            .expect("request state should exist");
        assert_eq!(state.waiting_list, vec![source]);
        assert!(!state.requested_as_dependency);
        assert!(
            !block_retriever.was_requested_as_dependency(&hash).unwrap(),
            "a gossip announcement is not a solicited dependency"
        );
    }

    #[tokio::test]
    async fn admit_hash_without_a_peer_broadcasts_a_has_block_request() {
        let (block_retriever, _transport) = retriever(vec![]);
        let hash: BlockHash = Bytes::from_static(b"admit-new-hash-no-peer");

        let result = block_retriever
            .admit_hash(hash.clone(), None, AdmitHashReason::HashBroadcastReceived)
            .await
            .expect("admit should succeed");

        assert_eq!(result.status, AdmitHashStatus::NewRequestAdded);
        assert!(result.broadcast_request);
        assert!(!result.request_block);
        assert_eq!(
            block_retriever.get_requested_blocks_count().await.unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn admit_hash_for_a_missing_dependency_seeds_connected_peers_and_marks_it() {
        let remote = peer_node("remote", 40401);
        let (block_retriever, transport) = retriever(vec![remote.clone()]);
        let hash: BlockHash = Bytes::from_static(b"admit-missing-dependency");

        let result = block_retriever
            .admit_hash(
                hash.clone(),
                None,
                AdmitHashReason::MissingDependencyRequested,
            )
            .await
            .expect("admit should succeed");

        assert_eq!(result.status, AdmitHashStatus::NewRequestAdded);
        assert!(
            result.request_block,
            "a connected peer stands in for the missing source peer"
        );
        assert_eq!(transport.request_count(), 1);
        assert!(block_retriever.was_requested_as_dependency(&hash).unwrap());

        let state = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .unwrap()
            .expect("request state should exist");
        assert_eq!(state.waiting_list, vec![remote]);
    }

    #[tokio::test]
    async fn admit_hash_adds_each_new_peer_once_and_ignores_repeats() {
        let (block_retriever, transport) = retriever(vec![]);
        let first = peer_node("first", 40401);
        let second = peer_node("second", 40402);
        let hash: BlockHash = Bytes::from_static(b"admit-peer-dedup");

        block_retriever
            .admit_hash(
                hash.clone(),
                Some(first.clone()),
                AdmitHashReason::HasBlockMessageReceived,
            )
            .await
            .unwrap();

        let added = block_retriever
            .admit_hash(
                hash.clone(),
                Some(second.clone()),
                AdmitHashReason::HasBlockMessageReceived,
            )
            .await
            .unwrap();
        assert_eq!(added.status, AdmitHashStatus::NewSourcePeerAddedToRequest);
        assert!(
            !added.request_block,
            "the waiting list already had a peer, so no immediate request"
        );

        let repeated = block_retriever
            .admit_hash(
                hash.clone(),
                Some(second),
                AdmitHashReason::HasBlockMessageReceived,
            )
            .await
            .unwrap();
        assert_eq!(repeated.status, AdmitHashStatus::Ignore);

        assert_eq!(
            block_retriever.get_waiting_list_size(&hash).await.unwrap(),
            2
        );
        assert_eq!(
            transport.request_count(),
            1,
            "only the initial admit issues a network request"
        );
    }

    #[tokio::test]
    async fn admit_hash_ignores_new_peers_once_the_block_is_received() {
        let (block_retriever, _transport) = retriever(vec![]);
        let hash: BlockHash = Bytes::from_static(b"admit-after-received");
        let now = BlockRetriever::<TransportLayerStub>::current_millis();
        let mut state = fresh_state(now);
        state.received = true;
        block_retriever
            .set_request_state_for_test(hash.clone(), state)
            .await
            .unwrap();

        let result = block_retriever
            .admit_hash(
                hash.clone(),
                Some(peer_node("late", 40401)),
                AdmitHashReason::HasBlockMessageReceived,
            )
            .await
            .unwrap();
        assert_eq!(result.status, AdmitHashStatus::Ignore);
        assert!(!result.request_block);
    }

    #[tokio::test]
    async fn admit_hash_ignores_a_known_hash_with_no_peer_and_no_dependency_reason() {
        let (block_retriever, _transport) = retriever(vec![]);
        let hash: BlockHash = Bytes::from_static(b"admit-known-no-peer");
        let now = BlockRetriever::<TransportLayerStub>::current_millis();
        block_retriever
            .set_request_state_for_test(hash.clone(), fresh_state(now))
            .await
            .unwrap();

        let result = block_retriever
            .admit_hash(hash, None, AdmitHashReason::BlockReceived)
            .await
            .unwrap();
        assert_eq!(result.status, AdmitHashStatus::Ignore);
    }

    #[tokio::test]
    async fn admit_hash_caps_the_waiting_list() {
        let (block_retriever, _transport) = retriever(vec![]);
        let hash: BlockHash = Bytes::from_static(b"admit-waiting-list-cap");
        let now = BlockRetriever::<TransportLayerStub>::current_millis();
        let mut state = fresh_state(now);
        state.waiting_list = (0..64).map(|i| peer_node("filler", 41000 + i)).collect();
        block_retriever
            .set_request_state_for_test(hash.clone(), state)
            .await
            .unwrap();

        let result = block_retriever
            .admit_hash(
                hash.clone(),
                Some(peer_node("overflow", 42000)),
                AdmitHashReason::HasBlockMessageReceived,
            )
            .await
            .unwrap();
        assert_eq!(result.status, AdmitHashStatus::Ignore);
        assert_eq!(
            block_retriever.get_waiting_list_size(&hash).await.unwrap(),
            64
        );
    }

    #[tokio::test]
    async fn missing_dependency_readmission_appends_unseen_connected_peers_once() {
        let remote = peer_node("remote", 40401);
        let (block_retriever, _transport) = retriever(vec![remote.clone()]);
        let hash: BlockHash = Bytes::from_static(b"missing-dependency-readmission");
        let now = BlockRetriever::<TransportLayerStub>::current_millis();
        block_retriever
            .set_request_state_for_test(hash.clone(), fresh_state(now))
            .await
            .unwrap();

        let first = block_retriever
            .admit_hash(
                hash.clone(),
                None,
                AdmitHashReason::MissingDependencyRequested,
            )
            .await
            .unwrap();
        assert_eq!(first.status, AdmitHashStatus::NewSourcePeerAddedToRequest);
        assert!(
            first.request_block,
            "the waiting list was empty before the connected peers were appended"
        );
        assert_eq!(
            block_retriever.get_waiting_list_size(&hash).await.unwrap(),
            1
        );

        let second = block_retriever
            .admit_hash(
                hash.clone(),
                None,
                AdmitHashReason::MissingDependencyRequested,
            )
            .await
            .unwrap();
        assert_eq!(
            second.status,
            AdmitHashStatus::Ignore,
            "every connected peer is already queued"
        );
    }

    #[tokio::test]
    async fn ack_receive_marks_known_hashes_and_registers_unknown_ones() {
        let (block_retriever, _transport) = retriever(vec![]);
        let known: BlockHash = Bytes::from_static(b"ack-receive-known");
        let unknown: BlockHash = Bytes::from_static(b"ack-receive-unknown");
        let now = BlockRetriever::<TransportLayerStub>::current_millis();
        block_retriever
            .set_request_state_for_test(known.clone(), fresh_state(now))
            .await
            .unwrap();

        assert!(!block_retriever.is_received(known.clone()).await.unwrap());
        block_retriever.ack_receive(known.clone()).await.unwrap();
        assert!(block_retriever.is_received(known).await.unwrap());

        assert!(!block_retriever.is_received(unknown.clone()).await.unwrap());
        block_retriever.ack_receive(unknown.clone()).await.unwrap();
        assert!(
            block_retriever.is_received(unknown).await.unwrap(),
            "an unknown hash is added directly as received"
        );
    }

    #[tokio::test]
    async fn ack_in_casper_stops_tracking_the_hash() {
        let (block_retriever, _transport) = retriever(vec![]);
        let hash: BlockHash = Bytes::from_static(b"ack-in-casper");
        let now = BlockRetriever::<TransportLayerStub>::current_millis();
        block_retriever
            .set_request_state_for_test(hash.clone(), fresh_state(now))
            .await
            .unwrap();

        block_retriever.ack_in_casper(hash.clone()).await.unwrap();

        assert!(block_retriever
            .get_request_state_for_test(&hash)
            .await
            .unwrap()
            .is_none());
        assert!(!block_retriever.is_received(hash).await.unwrap());
    }

    #[tokio::test]
    async fn request_all_evicts_received_expired_entries() {
        let (block_retriever, _transport) = retriever(vec![]);
        let hash: BlockHash = Bytes::from_static(b"evict-received-expired");
        let stale = BlockRetriever::<TransportLayerStub>::create_timed_out_timestamp(
            Duration::from_secs(2),
        );
        let mut state = fresh_state(stale);
        state.received = true;
        block_retriever
            .set_request_state_for_test(hash.clone(), state)
            .await
            .unwrap();

        block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .unwrap();

        assert!(
            block_retriever
                .get_request_state_for_test(&hash)
                .await
                .unwrap()
                .is_none(),
            "a received request past its re-request interval is evicted"
        );
    }

    #[tokio::test]
    async fn requested_blocks_bound_evicts_oldest_unresolved_entries_first() {
        let (block_retriever, _transport) = retriever(vec![]);
        let now = BlockRetriever::<TransportLayerStub>::current_millis();

        let oldest: BlockHash = Bytes::from_static(b"bound-oldest-unresolved");
        let mut oldest_state = fresh_state(now);
        oldest_state.initial_timestamp = now.saturating_sub(600_000);
        oldest_state.timestamp = now;
        block_retriever
            .set_request_state_for_test(oldest.clone(), oldest_state)
            .await
            .unwrap();

        for i in 0..2048u32 {
            let hash: BlockHash = Bytes::from(format!("bound-filler-{i:04}").into_bytes());
            block_retriever
                .set_request_state_for_test(hash, fresh_state(now))
                .await
                .unwrap();
        }
        assert_eq!(
            block_retriever.get_requested_blocks_count().await.unwrap(),
            2049
        );

        block_retriever
            .request_all(Duration::from_secs(3_600))
            .await
            .unwrap();

        assert_eq!(
            block_retriever.get_requested_blocks_count().await.unwrap(),
            2048,
            "the bound holds after maintenance"
        );
        assert!(
            block_retriever
                .get_request_state_for_test(&oldest)
                .await
                .unwrap()
                .is_none(),
            "the oldest unresolved entry is the eviction candidate"
        );
    }
}
