use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::casperbuffer::pending_request_policy::PendingRequestPolicy;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use tracing::{info, warn};

use crate::rust::engine::finalization_certificate_retriever::{
    CertificateRequestOutcome, FinalizationCertificateRetriever,
};
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_DOWNLOAD_END_TO_END_TIME_METRIC, BLOCK_REQUESTS_CAPACITY_DEFERRED_TOTAL_METRIC,
    BLOCK_REQUESTS_RETRIES_METRIC, BLOCK_REQUESTS_RETRY_ACTION_METRIC, BLOCK_REQUESTS_TOTAL_METRIC,
    BLOCK_RETRIEVER_BROADCAST_TRACKING_SIZE_METRIC,
    BLOCK_RETRIEVER_DEP_RECOVERY_TRACKING_SIZE_METRIC, BLOCK_RETRIEVER_METRICS_SOURCE,
    BLOCK_RETRIEVER_PEERS_TOTAL_SIZE_METRIC, BLOCK_RETRIEVER_REQUESTED_BLOCKS_SIZE_METRIC,
    BLOCK_RETRIEVER_WAITING_LIST_TOTAL_SIZE_METRIC,
};

mod dependency_provenance;
mod receipt_policy;
mod request_ownership;

use request_ownership::{Activation, PreparedRetry, RequestOwner, RequestOwners, RetrySelection};

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

impl AdmitHashResult {
    fn ignored() -> Self {
        Self {
            status: AdmitHashStatus::Ignore,
            broadcast_request: false,
            request_block: false,
        }
    }
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
    pub retry_budget_quarantine_until: Option<u64>,
    pub requested_as_dependency: bool,
}

#[derive(Clone)]
struct RequestData {
    peers: HashSet<PeerNode>,
    received: bool,
    in_casper_buffer: bool,
    waiting_list: Vec<PeerNode>,
    initial_dispatch_pending: bool,
}

impl RequestData {
    fn new(pending: bool) -> Self {
        Self {
            peers: HashSet::new(),
            received: pending,
            in_casper_buffer: pending,
            waiting_list: Vec::new(),
            initial_dispatch_pending: !pending,
        }
    }

    fn snapshot(&self, policy: &PendingRequestPolicy) -> RequestState {
        RequestState {
            timestamp: policy.last_request_timestamp,
            initial_timestamp: policy.initial_timestamp,
            peers: self.peers.clone(),
            received: self.received,
            in_casper_buffer: self.in_casper_buffer,
            waiting_list: self.waiting_list.clone(),
            peer_requery_cursor: policy.peer_requery_cursor,
            retry_budget_quarantine_until: policy.retry_budget_quarantine_until,
            requested_as_dependency: policy.requested_as_dependency,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTracking {
    Tracked,
    AtCapacity,
    Quarantined,
}

#[derive(Clone)]
pub struct BlockRetriever<T: TransportLayer + Send + Sync> {
    owners: Arc<RequestOwners<RequestData>>,
    finalization_certificate_retriever: FinalizationCertificateRetriever<T>,
    transport: Arc<T>,
    connections_cell: ConnectionsCell,
    conf: RPConf,
}

impl<T: TransportLayer + Send + Sync> std::fmt::Debug for BlockRetriever<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BlockRetriever")
            .field("active_requests", &self.owners.active_count())
            .field("deferred_completions", &self.owners.deferred_count())
            .finish_non_exhaustive()
    }
}

enum RetryAction {
    WaitingPeer(PeerNode, bool),
    KnownPeer(PeerNode),
    Broadcast,
}

impl RetryAction {
    fn metric_kind(&self) -> &'static str {
        match self {
            Self::WaitingPeer(_, _) => "peer_request",
            Self::KnownPeer(_) => "peer_requery",
            Self::Broadcast => "broadcast_only",
        }
    }
}

impl<T: TransportLayer + Send + Sync> BlockRetriever<T> {
    const MAX_REQUESTED_BLOCKS_ENTRIES: usize = 2048;
    const MAX_RETRY_OPERATIONS: usize = 2048;
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

    pub fn new(
        buffer: CasperBufferKeyValueStorage,
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
            owners: Arc::new(RequestOwners::new(
                buffer,
                Self::MAX_REQUESTED_BLOCKS_ENTRIES,
                Self::MAX_RETRY_OPERATIONS,
                RequestData::new,
            )),
            finalization_certificate_retriever,
            transport,
            connections_cell,
            conf,
        }
    }

    pub fn casper_buffer(&self) -> &CasperBufferKeyValueStorage { self.owners.buffer() }

    fn initial_policy(now: u64, dependency: bool) -> PendingRequestPolicy {
        PendingRequestPolicy {
            revision: 1,
            initial_timestamp: now,
            last_request_timestamp: now,
            requested_as_dependency: dependency,
            retry_attempts: 0,
            peer_requery_attempts: 0,
            peer_requery_cursor: 0,
            retry_budget_quarantine_until: None,
            dependency_recovery_last_request: None,
            broadcast_retry_last_request: None,
            peer_requery_last_request: None,
        }
    }

    fn activate(
        &self,
        hash: BlockHash,
        dependency: bool,
    ) -> Result<Activation<RequestData>, CasperError> {
        let now = Self::current_millis();
        let owner = self.owners.activate_local(
            hash,
            Self::initial_policy(now, dependency),
            now,
            RequestData::new,
        )?;
        if matches!(&owner, Activation::AtCapacity) {
            metrics::counter!(BLOCK_REQUESTS_CAPACITY_DEFERRED_TOTAL_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).increment(1);
        }
        Ok(owner)
    }

    fn lookup(
        &self,
        hash: &BlockHash,
    ) -> Result<Option<Arc<RequestOwner<RequestData>>>, CasperError> {
        Ok(self.owners.lookup(hash, RequestData::new)?)
    }

    pub fn request_states(&self) -> HashMap<BlockHash, RequestState> {
        self.owners
            .active_owners()
            .into_iter()
            .map(|owner| {
                (
                    owner.hash().clone(),
                    owner.inspect(|policy, data| data.snapshot(policy)),
                )
            })
            .collect()
    }

    pub fn request_state(&self, hash: &BlockHash) -> Result<Option<RequestState>, CasperError> {
        Ok(self
            .lookup(hash)?
            .map(|owner| owner.inspect(|policy, data| data.snapshot(policy))))
    }

    fn update_aux_tracking_metrics(&self) {
        let active = self.owners.active_owners();
        let mut waiting = 0usize;
        let mut peers = 0usize;
        let mut dependency = 0usize;
        let mut broadcast = 0usize;
        let mut peer_requery = 0usize;
        let mut quarantined = 0usize;
        let now = Self::current_millis();
        for owner in &active {
            owner.inspect(|policy, data| {
                waiting += data.waiting_list.len();
                peers += data.peers.len();
                dependency += usize::from(policy.dependency_recovery_last_request.is_some());
                broadcast += usize::from(policy.broadcast_retry_last_request.is_some());
                peer_requery += usize::from(policy.peer_requery_last_request.is_some());
                quarantined += usize::from(
                    policy
                        .retry_budget_quarantine_until
                        .is_some_and(|until| until > now),
                );
            });
        }
        metrics::gauge!(BLOCK_RETRIEVER_REQUESTED_BLOCKS_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).set(active.len() as f64);
        metrics::gauge!(BLOCK_RETRIEVER_WAITING_LIST_TOTAL_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).set(waiting as f64);
        metrics::gauge!(BLOCK_RETRIEVER_PEERS_TOTAL_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).set(peers as f64);
        metrics::gauge!(BLOCK_RETRIEVER_DEP_RECOVERY_TRACKING_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).set(dependency as f64);
        metrics::gauge!(BLOCK_RETRIEVER_BROADCAST_TRACKING_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).set(broadcast as f64);
        for (kind, size) in [
            ("peer_requery", peer_requery),
            ("retry_attempts", active.len()),
            ("peer_requery_attempts", active.len()),
            ("retry_budget_quarantine", quarantined),
            ("deferred_completions", self.owners.deferred_count()),
            (
                "retry_operations",
                Self::MAX_RETRY_OPERATIONS - self.owners.available_operations(),
            ),
        ] {
            metrics::gauge!(BLOCK_RETRIEVER_BROADCAST_TRACKING_SIZE_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "kind" => kind).set(size as f64);
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

    pub fn track_finalization_certificate(&self, digest: BlockHash) -> Result<(), CasperError> {
        self.finalization_certificate_retriever.track(digest)
    }

    pub async fn request_tracked_finalization_certificates(&self) -> Result<(), CasperError> {
        self.finalization_certificate_retriever.request_all().await
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

    fn backoff(attempts: u32, base: u64, first: u32, step: u32, maximum: u32) -> u64 {
        if attempts <= first {
            return base;
        }
        base.saturating_mul(u64::from(1 + ((attempts - first) / step).min(maximum)))
    }

    fn retry_due(now: u64, timestamp: u64, attempts: u32) -> bool {
        now.saturating_sub(timestamp)
            > Self::backoff(attempts, Self::MIN_REREQUEST_INTERVAL_MS, 4, 4, 7)
    }

    pub fn was_requested_as_dependency(&self, hash: &BlockHash) -> Result<bool, CasperError> {
        Ok(self
            .lookup(hash)?
            .is_some_and(|owner| owner.policy().requested_as_dependency))
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

    fn append_missing_dependency_peers(data: &mut RequestData, candidates: Vec<PeerNode>) -> usize {
        let mut added = 0;
        for peer in candidates {
            if data.waiting_list.len() >= Self::MAX_WAITING_LIST_PER_HASH {
                break;
            }
            if !data.waiting_list.contains(&peer) && !data.peers.contains(&peer) {
                data.waiting_list.push(peer);
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
        let index = (*cursor as usize) % peers_sorted.len();
        *cursor = cursor.wrapping_add(1);
        peers_sorted.get(index).cloned()
    }

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
        reason: AdmitHashReason,
    ) -> Result<AdmitHashResult, CasperError> {
        let dependency = reason == AdmitHashReason::MissingDependencyRequested;
        let candidates = if peer.is_none() && dependency {
            self.connected_peers_for_missing_dependency()?
        } else {
            Vec::new()
        };
        if dependency {
            if let Some(owner) = self.lookup(&hash)? {
                self.owners.update_policy(&owner, |policy, _| {
                    policy.requested_as_dependency =
                        dependency_provenance::merge_dependency_provenance(
                            policy.requested_as_dependency,
                            true,
                        );
                })?;
            }
        }
        let now = Self::current_millis();
        let Some(owner) = self.owners.activate_eligible(
            hash.clone(),
            Self::initial_policy(now, dependency),
            now,
            None,
            RequestData::new,
        )?
        else {
            return Ok(AdmitHashResult::ignored());
        };
        let now = Self::current_millis();
        let (result, target) = self.owners.update_policy(&owner, |policy, data| {
            policy.requested_as_dependency = dependency_provenance::merge_dependency_provenance(
                policy.requested_as_dependency,
                dependency,
            );
            if policy
                .retry_budget_quarantine_until
                .is_some_and(|until| now < until)
            {
                return (AdmitHashResult::ignored(), None);
            }
            if data.initial_dispatch_pending {
                data.initial_dispatch_pending = false;
                let initial = peer
                    .clone()
                    .map_or_else(|| candidates.clone(), |peer| vec![peer]);
                Self::append_missing_dependency_peers(data, initial);
                let target = peer.clone().or_else(|| candidates.first().cloned());
                return (
                    AdmitHashResult {
                        status: AdmitHashStatus::NewRequestAdded,
                        broadcast_request: target.is_none(),
                        request_block: target.is_some(),
                    },
                    target,
                );
            }
            if data.received {
                return (AdmitHashResult::ignored(), None);
            }
            let before = data.waiting_list.len();
            if let Some(peer) = peer.clone() {
                let added = Self::append_missing_dependency_peers(data, vec![peer.clone()]);
                if added > 0 {
                    return (
                        AdmitHashResult {
                            status: AdmitHashStatus::NewSourcePeerAddedToRequest,
                            broadcast_request: false,
                            request_block: before == 0,
                        },
                        Some(peer),
                    );
                }
            } else if dependency
                && Self::append_missing_dependency_peers(data, candidates.clone()) > 0
            {
                return (
                    AdmitHashResult {
                        status: AdmitHashStatus::NewSourcePeerAddedToRequest,
                        broadcast_request: false,
                        request_block: before == 0,
                    },
                    candidates.first().cloned(),
                );
            }
            (AdmitHashResult::ignored(), None)
        })?;
        if result.status == AdmitHashStatus::NewRequestAdded {
            metrics::counter!(BLOCK_REQUESTS_TOTAL_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).increment(1);
            info!(block = %PrettyPrinter::build_string_bytes(&hash), ?reason, "Tracking block request");
        }
        if result.broadcast_request {
            self.broadcast_request(&hash).await;
        }
        if result.request_block {
            if let Some(target) = target {
                self.request_block(&target, &hash).await;
            }
        }
        Ok(result)
    }

    async fn request_block(&self, peer: &PeerNode, hash: &BlockHash) {
        if let Err(error) = self
            .transport
            .request_for_block(&self.conf, peer, hash.clone())
            .await
        {
            warn!(block = %PrettyPrinter::build_string_bytes(hash), peer = %peer.endpoint.host, %error, "Block request failed");
        }
    }

    async fn broadcast_request(&self, hash: &BlockHash) {
        if let Err(error) = self
            .transport
            .broadcast_has_block_request(&self.connections_cell, &self.conf, hash)
            .await
        {
            warn!(block = %PrettyPrinter::build_string_bytes(hash), %error, "HasBlockRequest broadcast failed");
        }
    }

    pub async fn request_all(&self, age_threshold: Duration) -> Result<(), CasperError> {
        self.request_all_at(age_threshold, Self::current_millis())
            .await
    }

    async fn request_all_at(&self, age_threshold: Duration, now: u64) -> Result<(), CasperError> {
        let base = (age_threshold.as_millis() as u64).max(Self::MIN_REREQUEST_INTERVAL_MS);
        let lifetime = base.saturating_mul(Self::STALE_REQUEST_LIFETIME_MULTIPLIER);
        let mut first_error = None;
        if let Err(error) = self.owners.drain_completions(Self::MAX_RETRY_OPERATIONS) {
            first_error.get_or_insert(CasperError::from(error));
        }
        for owner in self.owners.active_owners() {
            let (retry, reopen) = owner.inspect(|policy, data| {
                if policy
                    .retry_budget_quarantine_until
                    .is_some_and(|until| now < until)
                {
                    return (false, false);
                }
                (
                    !data.received
                        && Self::retry_due(
                            now,
                            policy.last_request_timestamp,
                            policy.retry_attempts,
                        ),
                    data.received
                        && !data.in_casper_buffer
                        && now.saturating_sub(policy.initial_timestamp) > lifetime,
                )
            });
            let result = if retry {
                self.try_rerequest(&owner).await.map(|_| ())
            } else if reopen {
                self.owners
                    .update_policy(&owner, |policy, data| {
                        receipt_policy::reopen_stale_receipt(
                            &mut data.received,
                            data.in_casper_buffer,
                            &mut policy.last_request_timestamp,
                            policy.initial_timestamp,
                            now,
                            lifetime,
                        )
                    })
                    .map(|_| ())
                    .map_err(CasperError::from)
            } else {
                Ok(())
            };
            if let Err(error) = result {
                first_error.get_or_insert(error);
            }
        }
        if let Err(error) = self.owners.renew_expired(now).await {
            first_error.get_or_insert(CasperError::from(error));
        }
        self.update_aux_tracking_metrics();
        if let Err(error) = self.finalization_certificate_retriever.request_all().await {
            first_error.get_or_insert(error);
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn try_rerequest(
        &self,
        owner: &Arc<RequestOwner<RequestData>>,
    ) -> Result<bool, CasperError> {
        let now = Self::current_millis();
        let prepared = self.owners.reserve_retry_with(
            owner,
            Self::MAX_RETRIES_PER_HASH,
            now,
            Self::RETRY_BUDGET_QUARANTINE_MS,
            |policy, data, reserved_peers| {
                if data.received {
                    return RetrySelection::None;
                }
                if !data.waiting_list.is_empty() {
                    let peer = data.waiting_list.remove(0);
                    data.peers.insert(peer.clone());
                    policy.last_request_timestamp = now;
                    return RetrySelection::Dispatch(
                        false,
                        RetryAction::WaitingPeer(peer, data.waiting_list.is_empty()),
                    );
                }
                policy.last_request_timestamp = now;
                let known =
                    Self::pick_next_known_peer(&data.peers, &mut policy.peer_requery_cursor);
                let peer_budget = data
                    .peers
                    .len()
                    .clamp(1, Self::KNOWN_PEER_REQUERY_SOFT_LIMIT as usize);
                if (policy.peer_requery_attempts as usize).saturating_add(reserved_peers)
                    < peer_budget
                {
                    if let Some(peer) = known {
                        let cooldown = Self::backoff(
                            policy.retry_attempts,
                            Self::PEER_REQUERY_COOLDOWN_MS,
                            8,
                            8,
                            4,
                        );
                        if policy
                            .peer_requery_last_request
                            .is_some_and(|last| now.saturating_sub(last) < cooldown)
                        {
                            return RetrySelection::Suppressed(RetryAction::KnownPeer(peer));
                        }
                        policy.peer_requery_last_request = Some(now);
                        return RetrySelection::Dispatch(true, RetryAction::KnownPeer(peer));
                    }
                }
                let cooldown = Self::backoff(
                    policy.retry_attempts,
                    Self::BROADCAST_ONLY_COOLDOWN_MS,
                    8,
                    8,
                    4,
                );
                if policy
                    .broadcast_retry_last_request
                    .is_some_and(|last| now.saturating_sub(last) < cooldown)
                {
                    return RetrySelection::Suppressed(RetryAction::Broadcast);
                }
                policy.broadcast_retry_last_request = Some(now);
                RetrySelection::Dispatch(false, RetryAction::Broadcast)
            },
        )?;
        let (operation, action) = match prepared {
            PreparedRetry::None => return Ok(false),
            PreparedRetry::Suppressed(action) => {
                metrics::counter!(BLOCK_REQUESTS_RETRY_ACTION_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "action" => action.metric_kind()).increment(1);
                let reason = match action {
                    RetryAction::KnownPeer(_) => "peer_requery_suppressed",
                    _ => "broadcast_suppressed",
                };
                metrics::counter!(BLOCK_REQUESTS_RETRY_ACTION_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "action" => reason).increment(1);
                return Ok(false);
            }
            PreparedRetry::Dispatch(operation, action) => (operation, action),
        };
        metrics::counter!(BLOCK_REQUESTS_RETRY_ACTION_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE, "action" => action.metric_kind()).increment(1);
        let hash = owner.hash();
        match action {
            RetryAction::WaitingPeer(peer, last) => {
                self.request_block(&peer, hash).await;
                if last {
                    self.broadcast_request(hash).await;
                }
            }
            RetryAction::KnownPeer(peer) => {
                self.request_block(&peer, hash).await;
            }
            RetryAction::Broadcast => {
                self.broadcast_request(hash).await;
            }
        };
        metrics::counter!(BLOCK_REQUESTS_RETRIES_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).increment(1);
        operation.complete_action()?;
        Ok(true)
    }

    pub async fn recover_dependency(&self, hash: BlockHash) -> Result<(), CasperError> {
        let now = Self::current_millis();
        let Some(owner) = self.owners.activate_eligible(
            hash.clone(),
            Self::initial_policy(now, true),
            now,
            Some(Self::MAX_RETRIES_PER_HASH),
            RequestData::new,
        )?
        else {
            return Ok(());
        };
        self.owners.update_policy(&owner, |policy, _| {
            policy.requested_as_dependency = true;
        })?;
        let candidates = self.connected_peers_for_missing_dependency()?;
        let now = Self::current_millis();
        let prepared = self.owners.reserve_retry_with(
            &owner,
            Self::MAX_RETRIES_PER_HASH,
            now,
            Self::RETRY_BUDGET_QUARANTINE_MS,
            |policy, data, _| {
                if policy.dependency_recovery_last_request.is_some_and(|last| {
                    now.saturating_sub(last) < Self::DEPENDENCY_RECOVERY_COOLDOWN_MS
                }) {
                    return RetrySelection::None;
                }
                policy.dependency_recovery_last_request = Some(now);
                policy.last_request_timestamp = now;
                data.received = false;
                data.in_casper_buffer = false;
                let initial = data.initial_dispatch_pending;
                data.initial_dispatch_pending = false;
                let before = data.waiting_list.len();
                let added = Self::append_missing_dependency_peers(data, candidates.clone());
                let target = if initial || (before == 0 && added > 0) {
                    candidates.first().cloned()
                } else {
                    None
                };
                let broadcast = if initial {
                    target.is_none()
                } else {
                    added == 0
                };
                RetrySelection::Dispatch(false, (target, broadcast))
            },
        )?;
        let PreparedRetry::Dispatch(operation, (target, broadcast)) = prepared else {
            return Ok(());
        };
        if let Some(peer) = target {
            self.request_block(&peer, &hash).await;
        } else if broadcast {
            self.broadcast_request(&hash).await;
        }
        metrics::counter!(BLOCK_REQUESTS_RETRIES_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE).increment(1);
        operation.complete_action()?;
        self.update_aux_tracking_metrics();
        Ok(())
    }

    pub async fn defer_for_admission(
        &self,
        hash: BlockHash,
        source_peer: Option<PeerNode>,
    ) -> Result<bool, CasperError> {
        Ok(self.reopen_request(hash, source_peer)? == RequestTracking::Tracked)
    }

    pub fn reopen_after_local_failure(
        &self,
        hash: BlockHash,
    ) -> Result<RequestTracking, CasperError> {
        self.reopen_request(hash, None)
    }

    fn reopen_request(
        &self,
        hash: BlockHash,
        source_peer: Option<PeerNode>,
    ) -> Result<RequestTracking, CasperError> {
        let owner = match self.activate(hash, false)? {
            Activation::Active(owner) => owner,
            Activation::AtCapacity => return Ok(RequestTracking::AtCapacity),
            Activation::Ineligible => return Ok(RequestTracking::Quarantined),
        };
        self.owners.update_policy(&owner, |policy, data| {
            data.received = false;
            data.in_casper_buffer = false;
            data.initial_dispatch_pending = false;
            policy.last_request_timestamp = Self::current_millis();
            Self::append_missing_dependency_peers(data, source_peer.into_iter().collect());
        })?;
        Ok(RequestTracking::Tracked)
    }

    pub async fn ack_receive(&self, hash: BlockHash) -> Result<(), CasperError> {
        self.record_received(hash).map(|_| ())
    }

    pub fn record_received(&self, hash: BlockHash) -> Result<RequestTracking, CasperError> {
        let owner = match self.activate(hash, false)? {
            Activation::Active(owner) => owner,
            Activation::AtCapacity => return Ok(RequestTracking::AtCapacity),
            Activation::Ineligible => return Ok(RequestTracking::Quarantined),
        };
        let initial = self.owners.update_policy(&owner, |policy, data| {
            let previously_requested = !data.initial_dispatch_pending;
            data.initial_dispatch_pending = false;
            data.received = true;
            previously_requested.then_some(policy.initial_timestamp)
        })?;
        if let Some(timestamp) = initial {
            metrics::histogram!(BLOCK_DOWNLOAD_END_TO_END_TIME_METRIC, "source" => BLOCK_RETRIEVER_METRICS_SOURCE)
                .record(Self::current_millis().saturating_sub(timestamp) as f64 / 1000.0);
        }
        Ok(RequestTracking::Tracked)
    }

    pub fn publish_pending(
        &self,
        hash: BlockHash,
        blocks: HashSet<BlockHashSerde>,
        certificates: HashSet<BlockHashSerde>,
    ) -> Result<(), CasperError> {
        self.publish_pending_with_provenance(hash, blocks, certificates, false)
    }

    pub(crate) fn publish_pending_with_provenance(
        &self,
        hash: BlockHash,
        blocks: HashSet<BlockHashSerde>,
        certificates: HashSet<BlockHashSerde>,
        requested_as_dependency: bool,
    ) -> Result<(), CasperError> {
        self.owners.publish_pending_for(
            hash,
            Self::initial_policy(Self::current_millis(), requested_as_dependency),
            RequestData::new,
            blocks,
            certificates,
        )?;
        Ok(())
    }

    pub async fn ack_in_casper(&self, hash: BlockHash) -> Result<(), CasperError> {
        self.forget_hash_tracking(&hash)
    }

    pub fn forget_hash_tracking(&self, hash: &BlockHash) -> Result<(), CasperError> {
        if let Some(owner) = self.lookup(hash)? {
            self.owners.terminate(&owner)?;
        } else {
            self.owners.forget(hash)?;
        }
        Ok(())
    }

    pub async fn is_received(&self, hash: BlockHash) -> Result<bool, CasperError> {
        Ok(self
            .request_state(&hash)?
            .is_some_and(|state| state.received))
    }

    pub async fn get_waiting_list_size(&self, hash: &BlockHash) -> Result<usize, CasperError> {
        Ok(self
            .request_state(hash)?
            .map_or(0, |state| state.waiting_list.len()))
    }

    pub async fn get_requested_blocks_count(&self) -> Result<usize, CasperError> {
        Ok(self.owners.active_count())
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub async fn set_request_state_for_test(
        &self,
        hash: BlockHash,
        state: RequestState,
    ) -> Result<(), CasperError> {
        self.replace_request_for_test(hash, state, 0, 0)
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn replace_request_for_test(
        &self,
        hash: BlockHash,
        state: RequestState,
        retries: u32,
        peer_retries: u32,
    ) -> Result<(), CasperError> {
        self.forget_hash_tracking(&hash)?;
        let mut policy =
            Self::initial_policy(state.initial_timestamp, state.requested_as_dependency);
        policy.last_request_timestamp = state.timestamp;
        policy.peer_requery_cursor = state.peer_requery_cursor;
        policy.retry_budget_quarantine_until = state.retry_budget_quarantine_until;
        policy.retry_attempts = retries;
        policy.peer_requery_attempts = peer_retries;
        self.owners
            .activate(hash, policy, |_| RequestData {
                peers: state.peers,
                received: state.received,
                in_casper_buffer: state.in_casper_buffer,
                waiting_list: state.waiting_list,
                initial_dispatch_pending: false,
            })?
            .ok_or_else(|| {
                CasperError::RuntimeError("test request exceeds tracker capacity".into())
            })?;
        Ok(())
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub async fn get_request_state_for_test(
        &self,
        hash: &BlockHash,
    ) -> Result<Option<RequestState>, CasperError> {
        self.request_state(hash)
    }

    #[cfg(test)]
    fn retry_attempt_count(&self, hash: &BlockHash) -> Result<u32, CasperError> {
        Ok(self.owners.retry_budget(hash)?.0)
    }

    #[cfg(test)]
    fn seed_retry_attempt_for_test(&self, hash: &BlockHash) -> Result<(), CasperError> {
        let owner = self.lookup(hash)?.expect("seeded request owner");
        self.owners.update_policy(&owner, |policy, _| {
            policy.retry_attempts = policy.retry_attempts.saturating_add(1);
        })?;
        Ok(())
    }

    #[cfg(test)]
    fn seed_quarantine_for_test(&self, hash: &BlockHash, now: u64) -> Result<(), CasperError> {
        let owner = self.lookup(hash)?.expect("seeded request owner");
        self.owners.update_policy(&owner, |policy, _| {
            policy.retry_budget_quarantine_until =
                Some(now.saturating_add(Self::RETRY_BUDGET_QUARANTINE_MS));
        })?;
        Ok(())
    }

    #[cfg(test)]
    fn has_exceeded_retry_budget(&self, hash: &BlockHash) -> Result<bool, CasperError> {
        Ok(self.retry_attempt_count(hash)? >= Self::MAX_RETRIES_PER_HASH)
    }

    #[cfg(test)]
    fn is_retry_budget_quarantined(&self, hash: &BlockHash, now: u64) -> Result<bool, CasperError> {
        Ok(self
            .owners
            .retry_budget(hash)?
            .1
            .is_some_and(|until| until > now))
    }

    #[cfg(test)]
    fn peer_requery_attempt_count(&self, hash: &BlockHash) -> Result<u32, CasperError> {
        Ok(self
            .lookup(hash)?
            .map_or(0, |owner| owner.policy().peer_requery_attempts))
    }

    #[cfg(test)]
    fn reopen_stale_receipt(
        &self,
        hash: &BlockHash,
        now: u64,
        lifetime: u64,
    ) -> Result<bool, CasperError> {
        let Some(owner) = self.lookup(hash)? else {
            return Ok(false);
        };
        Ok(self.owners.update_policy(&owner, |policy, data| {
            receipt_policy::reopen_stale_receipt(
                &mut data.received,
                data.in_casper_buffer,
                &mut policy.last_request_timestamp,
                policy.initial_timestamp,
                now,
                lifetime,
            )
        })?)
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn create_timed_out_timestamp(timeout: Duration) -> u64 {
        Self::current_millis().saturating_sub((2 * timeout.as_millis()) as u64)
    }
}

#[cfg(test)]
#[path = "block_retriever/publication_handoff_tests.rs"]
mod publication_handoff_tests;

#[cfg(test)]
fn test_buffer() -> CasperBufferKeyValueStorage {
    futures::executor::block_on(async {
        let mut manager =
            rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager::new();
        CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
            .await
            .unwrap()
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use comm::rust::errors::CommError;
    use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
    use comm::rust::rp::connect::{Connections, ConnectionsCell};
    use comm::rust::rp::protocol_helper;
    use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
    use proptest::prelude::*;
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
        let buffer = test_buffer();
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever =
            BlockRetriever::new(buffer, transport.clone(), connections_cell, rp_conf);
        let block_hash = Bytes::from(vec![1; models::rust::block_hash::LENGTH]);
        let certificate_digest = Bytes::from(vec![2; models::rust::block_hash::LENGTH]);
        let stale = BlockRetriever::<TransportLayerStub>::current_millis().saturating_sub(120_000);
        block_retriever
            .set_request_state_for_test(block_hash.clone(), RequestState {
                timestamp: stale,
                initial_timestamp: stale,
                peers: HashSet::new(),
                received: false,
                in_casper_buffer: false,
                waiting_list: Vec::new(),
                peer_requery_cursor: 0,
                retry_budget_quarantine_until: None,
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
        let buffer = test_buffer();
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(buffer, transport, connections_cell, rp_conf);
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
        let buffer = test_buffer();
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(buffer, transport, connections_cell, rp_conf);

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
                retry_budget_quarantine_until: None,
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
            "unresolved request must remain tracked; only retry-budget exhaustion may evict it"
        );
    }

    #[tokio::test]
    async fn request_all_retry_exhaustion_retires_transport_and_retains_budget() {
        let (block_retriever, transport) = retriever(vec![]);
        let hash: BlockHash = Bytes::from_static(b"request-all-quarantine-evidence");
        let stale = BlockRetriever::<TransportLayerStub>::current_millis().saturating_sub(120_000);
        let mut state = fresh_state(stale);
        state.requested_as_dependency = true;
        block_retriever
            .set_request_state_for_test(hash.clone(), state)
            .await
            .expect("request state");
        for _ in 0..BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH {
            block_retriever
                .seed_retry_attempt_for_test(&hash)
                .expect("retry attempt");
        }

        block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .expect("maintenance");

        let retained = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .expect("request lookup");
        assert!(retained.is_none());
        assert!(!block_retriever
            .was_requested_as_dependency(&hash)
            .expect("ordinary provenance lookup"));
        assert!(block_retriever
            .is_retry_budget_quarantined(
                &hash,
                BlockRetriever::<TransportLayerStub>::current_millis(),
            )
            .expect("quarantine lookup"));
        assert!(block_retriever
            .has_exceeded_retry_budget(&hash)
            .expect("retry budget lookup"));
        assert_eq!(transport.request_count(), 0);
    }

    #[tokio::test]
    async fn recover_dependency_retry_exhaustion_retires_transport_and_retains_budget() {
        let (block_retriever, transport) = retriever(vec![]);
        let hash: BlockHash = Bytes::from_static(b"recover-quarantine-evidence");
        let now = BlockRetriever::<TransportLayerStub>::current_millis();
        let mut state = fresh_state(now);
        state.requested_as_dependency = true;
        block_retriever
            .set_request_state_for_test(hash.clone(), state)
            .await
            .expect("request state");
        for _ in 0..BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH {
            block_retriever
                .seed_retry_attempt_for_test(&hash)
                .expect("retry attempt");
        }

        block_retriever
            .recover_dependency(hash.clone())
            .await
            .expect("recovery maintenance");

        assert!(!block_retriever
            .was_requested_as_dependency(&hash)
            .expect("provenance lookup"));
        assert!(block_retriever
            .get_request_state_for_test(&hash)
            .await
            .expect("request lookup")
            .is_none());
        assert!(block_retriever
            .has_exceeded_retry_budget(&hash)
            .expect("retry budget lookup"));
        assert!(block_retriever
            .is_retry_budget_quarantined(
                &hash,
                BlockRetriever::<TransportLayerStub>::current_millis(),
            )
            .expect("quarantine lookup"));
        assert_eq!(transport.request_count(), 0);
    }

    #[tokio::test]
    async fn expired_quarantine_reopens_the_retained_request() {
        let remote = peer_node("remote", 40401);
        let (block_retriever, transport) = retriever(vec![remote.clone()]);
        let hash: BlockHash = Bytes::from_static(b"expired-quarantine-reentry");
        let stale = BlockRetriever::<TransportLayerStub>::create_timed_out_timestamp(
            Duration::from_secs(2),
        );
        let mut state = fresh_state(stale);
        state.waiting_list.push(remote);
        state.requested_as_dependency = true;
        state.retry_budget_quarantine_until =
            Some(BlockRetriever::<TransportLayerStub>::current_millis().saturating_sub(1));
        block_retriever
            .set_request_state_for_test(hash.clone(), state)
            .await
            .expect("request state");

        block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .expect("maintenance");

        assert!(transport.request_count() >= 1);
        assert!(block_retriever
            .was_requested_as_dependency(&hash)
            .expect("provenance lookup"));
        assert!(!block_retriever
            .is_retry_budget_quarantined(
                &hash,
                BlockRetriever::<TransportLayerStub>::current_millis(),
            )
            .expect("quarantine lookup"));
    }

    #[tokio::test]
    async fn expired_spent_schedule_requires_retirement_then_explicit_renewal() {
        let remote = peer_node("remote", 40401);
        let (block_retriever, transport) = retriever(vec![remote.clone()]);
        let hash: BlockHash = Bytes::from_static(b"expired-budget-single-probe");
        let stale = BlockRetriever::<TransportLayerStub>::current_millis().saturating_sub(120_000);
        let mut state = fresh_state(stale);
        state.waiting_list.push(remote);
        state.retry_budget_quarantine_until =
            Some(BlockRetriever::<TransportLayerStub>::current_millis().saturating_sub(1));
        block_retriever
            .set_request_state_for_test(hash.clone(), state)
            .await
            .unwrap();
        for _ in 0..BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH {
            block_retriever.seed_retry_attempt_for_test(&hash).unwrap();
        }

        block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .unwrap();
        assert_eq!(transport.request_count(), 0);
        assert!(block_retriever.has_exceeded_retry_budget(&hash).unwrap());
        assert!(block_retriever.request_states().is_empty());
        let deadline = block_retriever
            .owners
            .retry_budget(&hash)
            .expect("retired budget")
            .1
            .expect("new quarantine deadline");

        block_retriever
            .request_all_at(Duration::from_millis(1), deadline - 1)
            .await
            .unwrap();
        assert_eq!(transport.request_count(), 0);
        assert!(block_retriever.has_exceeded_retry_budget(&hash).unwrap());
        assert!(block_retriever
            .is_retry_budget_quarantined(&hash, deadline - 1)
            .unwrap());
        block_retriever
            .request_all_at(Duration::from_millis(1), deadline)
            .await
            .expect("expiry maintenance");
        assert_eq!(transport.request_count(), 0);
        assert!(!block_retriever.has_exceeded_retry_budget(&hash).unwrap());
        assert!(!block_retriever
            .is_retry_budget_quarantined(&hash, deadline)
            .unwrap());
        assert!(block_retriever.request_states().is_empty());
    }

    #[tokio::test]
    async fn received_timeout_reopens_without_losing_dependency_provenance() {
        let (block_retriever, _transport) = retriever(vec![]);
        let hash: BlockHash = Bytes::from_static(b"received-timeout-evidence");
        let stale = BlockRetriever::<TransportLayerStub>::current_millis().saturating_sub(10_000);
        let mut state = fresh_state(stale);
        state.received = true;
        state.requested_as_dependency = true;
        block_retriever
            .set_request_state_for_test(hash.clone(), state)
            .await
            .expect("request state");

        block_retriever
            .request_all(Duration::from_millis(1))
            .await
            .expect("maintenance");

        let reopened = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .expect("request lookup")
            .expect("request must remain tracked");
        assert!(!reopened.received);
        assert!(reopened.requested_as_dependency);
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
        let buffer = test_buffer();
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever =
            BlockRetriever::new(buffer, transport.clone(), connections_cell, rp_conf);

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
        let buffer = test_buffer();
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever =
            BlockRetriever::new(buffer, transport.clone(), connections_cell, rp_conf);
        let hash: BlockHash = Bytes::from_static(b"admission-deferred-hash");

        block_retriever
            .ack_receive(hash.clone())
            .await
            .expect("receipt should be recorded");
        for _ in 0..BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH {
            block_retriever
                .seed_retry_attempt_for_test(&hash)
                .expect("retry attempt should be recorded");
        }
        block_retriever
            .seed_quarantine_for_test(
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
        assert!(block_retriever
            .has_exceeded_retry_budget(&hash)
            .expect("retry budget lookup should succeed"));
        assert!(block_retriever
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
        let buffer = test_buffer();
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(buffer, transport, connections_cell, rp_conf);
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
        assert!(!state.requested_as_dependency);
    }

    #[tokio::test]
    async fn request_capacity_preserves_existing_work_and_defers_new_hashes() {
        let local = peer_node("local", 40400);
        let source = peer_node("source", 40401);
        let rp_conf = create_rp_conf_ask(local, None, None);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![source.clone()]))),
        };
        let buffer = test_buffer();
        let existing = Bytes::from_static(b"existing-request");
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(buffer, transport, connections_cell, rp_conf);
        let mut request = fresh_state(1);
        request.waiting_list = vec![source.clone()];
        block_retriever
            .replace_request_for_test(existing.clone(), request, 0, 0)
            .unwrap();
        for index in 1..BlockRetriever::<TransportLayerStub>::MAX_REQUESTED_BLOCKS_ENTRIES {
            block_retriever
                .replace_request_for_test(
                    Bytes::from(index.to_be_bytes().to_vec()),
                    fresh_state(index as u64 + 1),
                    0,
                    0,
                )
                .unwrap();
        }
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

        let state = block_retriever.request_states();
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
        let buffer = test_buffer();
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever = BlockRetriever::new(buffer, transport, connections_cell, rp_conf);

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
                retry_budget_quarantine_until: None,
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
        let buffer = test_buffer();
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever =
            BlockRetriever::new(buffer, transport.clone(), connections_cell, rp_conf);

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
                retry_budget_quarantine_until: None,
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

        let owner = block_retriever.lookup(&hash).unwrap().unwrap();
        block_retriever
            .owners
            .update_policy(&owner, |policy, _| {
                policy.last_request_timestamp =
                    BlockRetriever::<TransportLayerStub>::create_timed_out_timestamp(
                        Duration::from_secs(2),
                    );
            })
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
        assert_eq!(block_retriever.retry_attempt_count(&hash).unwrap(), 2);
        assert_eq!(
            block_retriever.peer_requery_attempt_count(&hash).unwrap(),
            1
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
        let buffer = test_buffer();
        let transport = Arc::new(TransportLayerStub::new());
        let block_retriever =
            BlockRetriever::new(buffer, transport.clone(), connections_cell, rp_conf);

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
                retry_budget_quarantine_until: None,
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
        let buffer = test_buffer();
        let block_retriever =
            BlockRetriever::new(buffer, transport.clone(), connections_cell, rp_conf);
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
            retry_budget_quarantine_until: None,
            requested_as_dependency: false,
        }
    }

    proptest! {
        #[test]
        fn pending_handoff_preserves_request_counters_and_quarantine(
            retry_attempts in 0_u32..1_000,
            peer_attempts in 0_u32..1_000,
            requested_as_dependency in any::<bool>(),
        ) {
            let (block_retriever, transport) = retriever(vec![]);
            let hash: BlockHash = Bytes::from_static(b"property-quarantine");
            let now = BlockRetriever::<TransportLayerStub>::current_millis();
            let mut state = fresh_state(now);
            state.requested_as_dependency = requested_as_dependency;
            state.retry_budget_quarantine_until = Some(now.saturating_add(10_000));
            block_retriever.replace_request_for_test(hash.clone(), state.clone(), retry_attempts, peer_attempts).unwrap();
            let before = block_retriever.lookup(&hash).unwrap().unwrap().policy();
            block_retriever.publish_pending(hash.clone(), HashSet::new(), HashSet::new()).unwrap();
            prop_assert!(block_retriever.request_states().is_empty());
            let cold = BlockRetriever::new(block_retriever.casper_buffer().clone(), transport.clone(), block_retriever.connections_cell.clone(), block_retriever.conf.clone());
            let restored = cold.lookup(&hash).unwrap().unwrap().policy();
            prop_assert_eq!(restored, before);
            prop_assert_eq!(cold.request_state(&hash).unwrap().unwrap().requested_as_dependency, requested_as_dependency);
            prop_assert_eq!(transport.request_count(), 0);
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
    async fn request_all_reopens_received_expired_entries() {
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

        let reopened = block_retriever
            .get_request_state_for_test(&hash)
            .await
            .unwrap()
            .expect("a received request remains tracked until terminal resolution");
        assert!(!reopened.received);
    }

    #[tokio::test]
    async fn tracker_rejects_overcapacity_and_maintenance_preserves_unresolved_work() {
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

        for i in 0..2047u32 {
            let hash: BlockHash = Bytes::from(format!("bound-filler-{i:04}").into_bytes());
            block_retriever
                .set_request_state_for_test(hash, fresh_state(now))
                .await
                .unwrap();
        }
        assert_eq!(
            block_retriever.get_requested_blocks_count().await.unwrap(),
            2048
        );

        let refused: BlockHash = Bytes::from_static(b"bound-refused");
        let error = block_retriever
            .set_request_state_for_test(refused.clone(), fresh_state(now))
            .await
            .expect_err("tracking must reject an over-capacity insertion");
        assert!(matches!(
            error,
            CasperError::RuntimeError(message)
                if message.contains("test request exceeds tracker capacity")
        ));
        assert!(block_retriever.lookup(&refused).unwrap().is_none());
        let accepted = block_retriever
            .request_states()
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        block_retriever
            .request_all(Duration::from_secs(3_600))
            .await
            .unwrap();
        assert_eq!(
            block_retriever
                .request_states()
                .keys()
                .cloned()
                .collect::<HashSet<_>>(),
            accepted
        );

        assert_eq!(
            block_retriever.get_requested_blocks_count().await.unwrap(),
            2048,
            "maintenance must not conceal corruption by discarding unresolved work"
        );
        assert!(
            block_retriever
                .get_request_state_for_test(&oldest)
                .await
                .unwrap()
                .is_some(),
            "maintenance must preserve the existing unresolved identity"
        );
    }
}
