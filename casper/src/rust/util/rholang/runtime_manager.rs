// See casper/src/main/scala/coop/rchain/casper/util/rholang/RuntimeManager.scala
// See casper/src/main/scala/coop/rchain/casper/util/rholang/RuntimeManagerSyntax.scala

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::Hash;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block_storage::rust::deploy::key_value_deploy_storage::PendingDeployCandidate;
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use dashmap::DashMap;
use hex::ToHex;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::block::state_hash::{StateHash, StateHashSerde};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    Bond, DeployData, Event, ProcessedDeploy, ProcessedSystemDeploy, ProcessedUserDeploy,
    SystemDeployData,
};
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelopeRef;
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::validator::Validator;
use num_bigint::BigInt;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::accounting::NativeRecordingWireLimits;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::rholang_merging_logic::{
    DeployMergeableData, NumberChannel, RholangMergingLogic,
};
use rholang::rust::interpreter::rho_runtime::{
    self, RhoHistoryRepository, RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::system_processes::{BlockData, Definition};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::native_reader::{
    decode_datum, NativeLeafKind, NativeReadError,
};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::merger::merging_logic::{NumberChannelsDiff, NumberChannelsEndVal};
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::{RSpace, RSpaceStore};
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteVector;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::rust::errors::CasperError;
use crate::rust::merging::block_index::BlockIndex;
use crate::rust::metrics_constants::{
    BLOCK_INDEX_CACHE_SIZE_METRIC, BLOCK_REPLAY_RUNTIME_EXECUTE_TIME_METRIC,
    BLOCK_REPLAY_RUNTIME_LOCK_WAIT_TIME_METRIC, BLOCK_REPLAY_RUNTIME_REPORTING_DEFERRED_METRIC,
    BLOCK_REPLAY_RUNTIME_SAVE_MERGEABLE_TIME_METRIC, CASPER_METRICS_SOURCE,
    PARENTS_POST_STATE_CACHE_SIZE_METRIC, REPLAY_CACHE_ENTRIES_METRIC,
    REPLAY_CACHE_RETAINED_BYTES_METRIC, RUNTIME_SPAWN_REPLAY_CALLS_METRIC,
    RUNTIME_SPAWN_REPLAY_TIME_METRIC, RUNTIME_SPAWN_TIME_METRIC,
};
use crate::rust::rholang::replay_runtime::{validate_offered_replay_preflight, ReplayRuntimeOps};
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::acceptance::{prepare_offered_candidate, OfferedCandidateLimits};
use crate::rust::util::rholang::costacc::direct_wallet_funding::{
    authorize_offered_direct_wallet_funding, authorize_offered_direct_wallet_funding_with_grants,
    CheckedDirectWalletPolicy, DirectWalletFundingLimits, DirectWalletPolicySnapshot,
    NativeFundedExecutionContext, NativeFundedReplayLimits, ReplayedNativeFundedUser,
};
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
#[cfg(any(test, feature = "test-utils"))]
use crate::rust::util::rholang::costacc::offered_acceptance::{
    OfferedBudgetRecorder, OfferedBudgetUsage, OfferedUsageKind,
};
use crate::rust::util::rholang::costacc::offered_grants::{
    grant_issue_definition, offered_grant_issue_call_limits, GrantIssueCallLimits,
    VerifiedOfferedGrantSources,
};
use crate::rust::util::rholang::replay_cache::{
    InMemoryReplayCache, ReplayCache, ReplayCacheEntry, ReplayCacheKey,
};

type MergeableStore = KeyValueTypedStoreImpl<ByteVector, Vec<DeployMergeableData>>;

#[derive(serde::Serialize, serde::Deserialize)]
struct MergeableKey {
    state_hash: StateHashSerde,
    #[serde(with = "shared::rust::serde_bytes")]
    creator: prost::bytes::Bytes,
    seq_num: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct ExploratoryDeployConfig {
    pub max_concurrent: usize,
    pub phlo_limit: i64,
    pub execution_timeout: Duration,
}

impl ExploratoryDeployConfig {
    /// Rejects a non-positive value rather than clamping it. A clamped `0`
    /// yields a node that answers nothing on this endpoint — one phlogiston
    /// fails every query on cost, a one-millisecond deadline times out every
    /// query — with no diagnostic distinguishing that from a working node.
    pub fn new(
        max_concurrent: usize,
        phlo_limit: i64,
        execution_timeout: Duration,
    ) -> Result<Self, CasperError> {
        if max_concurrent == 0 {
            return Err(CasperError::Other(
                "exploratory-deploy-max-concurrent must be at least 1".to_string(),
            ));
        }
        if phlo_limit <= 0 {
            return Err(CasperError::Other(format!(
                "exploratory-deploy-phlo-limit must be positive, got {}",
                phlo_limit
            )));
        }
        if execution_timeout.is_zero() {
            return Err(CasperError::Other(
                "exploratory-deploy-execution-timeout must be greater than zero".to_string(),
            ));
        }
        Ok(Self {
            max_concurrent,
            phlo_limit,
            execution_timeout,
        })
    }

    /// Resolve the operator-facing form of `max_concurrent`: `0` derives the
    /// value from this host's cores (observers are the dedicated servers of
    /// exploratory traffic, and the right ceiling is a host property); an
    /// explicit value is honored, clamped to the semaphore's permit range.
    /// The constructed config always carries a real ceiling — `new` still
    /// rejects 0.
    pub fn resolve(
        max_concurrent: usize,
        phlo_limit: i64,
        execution_timeout: Duration,
    ) -> Result<Self, CasperError> {
        let resolved = if max_concurrent == 0 {
            let cores = std::thread::available_parallelism()
                .map(std::num::NonZeroUsize::get)
                .unwrap_or(4);
            Self::derive_max_concurrent(cores)
        } else {
            max_concurrent.min(Semaphore::MAX_PERMITS)
        };
        Self::new(resolved, phlo_limit, execution_timeout)
    }

    /// Two cores stay reserved for block-following; a floor of two keeps any
    /// host able to serve overlapping clients.
    pub fn derive_max_concurrent(cores: usize) -> usize { cores.saturating_sub(2).max(2) }

    /// Fixture for the test-only constructors. Deliberately not a `Default`
    /// impl: the operator-facing default lives in `defaults.conf` and reaches
    /// the runtime through `create_with_history_config`, so a second
    /// authoritative-looking declaration in Rust could drift from it silently.
    /// These values are not that default — they are only what the test
    /// constructors have always used.
    pub fn for_tests() -> Self {
        Self {
            max_concurrent: 1,
            phlo_limit: 5_000_000,
            execution_timeout: Duration::from_secs(15),
        }
    }
}

const REPLAY_LOCK_PERMITS: usize = 4;
const REPORTING_REPLAY_WAIT_WARN: Duration = Duration::from_secs(30);

pub struct ReplayLock {
    semaphore: Arc<Semaphore>,
    consensus_waiters: std::sync::atomic::AtomicUsize,
    changed: Arc<tokio::sync::Notify>,
}

#[derive(Debug)]
pub struct ReplayPermit {
    permit: Option<OwnedSemaphorePermit>,
    changed: Arc<tokio::sync::Notify>,
}

impl Drop for ReplayPermit {
    fn drop(&mut self) {
        drop(self.permit.take());
        self.changed.notify_waiters();
    }
}

struct ConsensusReplayWaiter<'a>(&'a ReplayLock);

impl Drop for ConsensusReplayWaiter<'_> {
    fn drop(&mut self) {
        self.0
            .consensus_waiters
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        self.0.changed.notify_waiters();
    }
}

impl ReplayLock {
    pub fn new() -> Self { Self::with_permits(REPLAY_LOCK_PERMITS) }

    pub fn with_permits(permits: usize) -> Self {
        assert!(permits > 0);
        Self {
            semaphore: Arc::new(Semaphore::new(permits)),
            consensus_waiters: std::sync::atomic::AtomicUsize::new(0),
            changed: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn permit(&self, permit: OwnedSemaphorePermit) -> ReplayPermit {
        ReplayPermit {
            permit: Some(permit),
            changed: self.changed.clone(),
        }
    }

    fn consensus_waiting(&self) -> bool {
        self.consensus_waiters
            .load(std::sync::atomic::Ordering::Acquire)
            > 0
    }

    pub async fn acquire_consensus(&self) -> Result<ReplayPermit, tokio::sync::AcquireError> {
        self.consensus_waiters
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let waiter = ConsensusReplayWaiter(self);
        let permit = self.semaphore.clone().acquire_owned().await;
        drop(waiter);
        permit.map(|permit| self.permit(permit))
    }

    pub async fn acquire_reporting(&self) -> Result<ReplayPermit, tokio::sync::TryAcquireError> {
        let started = tokio::time::Instant::now();
        let mut warned = false;
        let mut deferred = false;
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.consensus_waiting() {
                match self.semaphore.clone().try_acquire_owned() {
                    Ok(permit) => {
                        let permit = self.permit(permit);
                        if !self.consensus_waiting() {
                            return Ok(permit);
                        }
                        drop(permit);
                        continue;
                    }
                    Err(tokio::sync::TryAcquireError::NoPermits) => {}
                    Err(error) => return Err(error),
                }
            }
            if !deferred {
                deferred = true;
                metrics::counter!(BLOCK_REPLAY_RUNTIME_REPORTING_DEFERRED_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .increment(1);
            }
            if warned {
                changed.await;
            } else if tokio::time::timeout_at(started + REPORTING_REPLAY_WAIT_WARN, changed)
                .await
                .is_err()
            {
                warned = true;
                tracing::warn!(
                    waited_ms = started.elapsed().as_millis() as u64,
                    "report replay is still waiting for a replay permit behind consensus replays"
                );
            }
        }
    }
}

impl Default for ReplayLock {
    fn default() -> Self { Self::new() }
}

#[derive(Clone)]
pub struct RuntimeManager {
    pub space: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    pub replay_space: ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    pub history_repo: RhoHistoryRepository,
    pub mergeable_store: MergeableStore,
    pub(crate) mergeable_tags: std::sync::Arc<
        std::collections::HashMap<Par, rspace_plus_plus::rspace::merger::merging_logic::MergeType>,
    >,
    // TODO: make proper storage for block indices - OLD
    pub block_index_cache: Arc<DashMap<BlockHash, BlockIndex>>,
    pub block_index_cache_order: Arc<Mutex<VecDeque<BlockHash>>>,
    pub active_validators_cache: Arc<DashMap<StateHash, Vec<Validator>>>,
    pub active_validators_cache_order: Arc<Mutex<VecDeque<StateHash>>>,
    pub bonds_cache: Arc<DashMap<StateHash, Vec<Bond>>>,
    pub bonds_cache_order: Arc<Mutex<VecDeque<StateHash>>>,
    /// Cache for merged parent post-state computation keyed by parent-set snapshot context.
    pub parents_post_state_cache: Arc<DashMap<ParentsPostStateCacheKey, ParentsPostStateCacheVal>>,
    pub parents_post_state_cache_order: Arc<Mutex<VecDeque<ParentsPostStateCacheKey>>>,
    /// Optional replay cache for delta replay optimization
    pub replay_cache: Option<Arc<InMemoryReplayCache>>,
    /// Optional state hash cache for skipping known replays
    replay_lock: Arc<ReplayLock>,
    exploratory_deploy_semaphore: Arc<Semaphore>,
    exploratory_deploy_phlo_limit: i64,
    exploratory_deploy_execution_timeout: Duration,
    pub external_services: ExternalServices,
    /// DR-102: test-only records of offered deploy attempts.
    #[cfg(any(test, feature = "test-utils"))]
    offered_budget_recorder: OfferedBudgetRecorder,
    /// Slice 30b: shared snapshot-writer config threaded into every
    /// runtime spawned by this manager.  `None` when the operator
    /// has no consensus-static provisioning (backward compat).
    /// Populated at boot via `set_fs_snapshot_writer`.  Wrapped in
    /// `Arc<RwLock<_>>` so a Cloned `RuntimeManager` shares the
    /// same slot — a boot-time set on one clone is visible to all
    /// others.  Default on triage: `Arc::new(RwLock::new(None))`.
    pub fs_snapshot_writer:
        Arc<tokio::sync::RwLock<Option<rholang::rust::interpreter::io::snapshot::SnapshotWriter>>>,
    /// Phase 7b-1 (2026-08-27): per-block snapshot Merkle roots
    /// keyed by finalized block hash.  Populated by the
    /// `WalSnapshotWrite` finalization effect after `maybe_write`
    /// returns `Some((root, merkle_root))`; consumed by the
    /// snapshot-chunk retriever so joiners can verify chunks over
    /// `get_snapshot_chunk` against a locally-anchored Merkle root.
    /// Values are `(atomic_root, merkle_root)`.  Default on triage:
    /// `Arc::new(RwLock::new(HashMap::new()))` — no writer wires
    /// in production yet; the slice-5.103 `snapshot_chunk_sync`
    /// driver reads this cache in its boot enumerator.
    pub snapshot_merkle_roots:
        Arc<tokio::sync::RwLock<std::collections::HashMap<Vec<u8>, ([u8; 32], [u8; 32])>>>,
    /// Phase 7b-2 shared write-payload store bundle.  Boot pipeline
    /// populates via `set_payload_store` from a
    /// `PayloadStoreBundle::from_directory(...)` pointed at
    /// `<data-dir>/wal_payload_store/`.  The bundle carries both
    /// trait-object aspects of the same underlying store —
    /// `PayloadPersistence` (write side, threaded into every
    /// spawned runtime's handler-table payload store) and
    /// `PayloadLookup` (read side, threaded into the future
    /// `WalPayloadContext.payload_lookup`).
    ///
    /// `None` when the operator has no consensus-static provisioning
    /// (observer nodes, dev-mode nodes) OR when the boot pipeline
    /// hasn't fired yet.  Handlers see `None` and skip the persist
    /// step; joiners can still fetch from other peers.
    ///
    /// Wrapped in `Arc<RwLock<Option<...>>>` for the same reason
    /// as `fs_snapshot_writer` — a boot-time set on one clone is
    /// visible to every other clone.  Default on triage:
    /// `Arc::new(RwLock::new(None))`.
    pub payload_store: Arc<
        tokio::sync::RwLock<Option<crate::rust::engine::wal_payload_server::PayloadStoreBundle>>,
    >,
    /// Operator-provisioned consensus-static roots.  Threaded into
    /// the WAL applier's `allowed_roots` via the boot apply
    /// subscriber (slice 5.131) as defense-in-depth: a WAL entry
    /// whose target path does not sit under one of these roots is
    /// rejected by the applier.
    ///
    /// Populated at boot via `register_consensus_static_root` from
    /// each entry in the operator's fs bundle (file paths + dir
    /// paths).  Empty on observer nodes without consensus
    /// provisioning; the applier skips validation on empty.
    ///
    /// Shared `Arc<RwLock<_>>` so a boot-time register from one
    /// clone is visible to every other clone (same discipline as
    /// `payload_store` / `fs_snapshot_writer`).
    pub consensus_static_roots: Arc<tokio::sync::RwLock<Vec<std::path::PathBuf>>>,
    /// Manager-shared `RootIdentityRegistry`.  Owns the two-layer
    /// indirection (`Arc<RwLock<Arc<RwLock<Inner>>>>`); every
    /// spawned runtime's `fs_handles.root_registry` is pointed at
    /// this backing via `share_root_registry` so a boot-time
    /// `register(logical, root)` on the manager-held handle is
    /// visible to every runtime clone (same discipline as
    /// `payload_store` and the WAL registries).
    ///
    /// Populated at boot via
    /// [`RootIdentityRegistry::register`] for each entry in the
    /// operator's fs bundle (file paths + dir paths).  Empty on
    /// observer nodes without consensus provisioning; handlers'
    /// `safe_descend` falls through to the identity branch
    /// (matches pre-Shape-A behavior).
    pub root_registry: rholang::rust::interpreter::io::path::identity::RootIdentityRegistry,

    /// Shared range-lock registry — broadcast to every spawned
    /// runtime via `FileHandleTable::share_lock_registry` in
    /// `broadcast_fs_slots`.  Cross-cap lock coordination on the
    /// same `(dev, inode)` has to live above the per-fd table
    /// because each `File` cap mints its own fd (slice 27's
    /// fresh-mint posture), so same-file concurrent operations
    /// via different caps need a shared coordinator.  Mirrors
    /// [`root_registry`](Self::root_registry) in shape — a
    /// single manager-owned registry fans out to every
    /// runtime's handle-table at spawn time.
    pub shared_lock_registry: rholang::rust::interpreter::io::lock::SharedLockRegistry,
}

// Moved by G2-1 to
// `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/offered_block.rs`,
// unchanged: `CertifiedOfferedDraft` and its impl.

fn mergeable_read_error(error: NativeReadError<CasperError>) -> CasperError {
    match error {
        NativeReadError::Consumer(error) => error,
        NativeReadError::Store(error) => {
            CasperError::RuntimeError(format!("offered mergeable history read failed: {error}"))
        }
        NativeReadError::Invalid(error) => {
            CasperError::RuntimeError(format!("offered mergeable history is invalid: {error:?}"))
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct ParentsPostStateCacheKey {
    pub sorted_parent_hashes: Vec<BlockHash>,
    // Snapshot LFB participates in visible-ancestor filtering, so cache key must include it.
    pub snapshot_lfb_hash: BlockHash,
    // The finalized-floor merge base is derived from the block's frozen
    // justification snapshot (finality/floor.rs), so identical parent sets
    // under different justification maps can merge from different floors.
    // Sorted (validator, latest_block_hash) pairs keep such contexts from
    // sharing a cache entry.
    pub sorted_latest_messages: Vec<(Validator, BlockHash)>,
    // Whether the computation ran with a rejected-deploy buffer attached.
    // Buffer population is a side effect of the merge, not part of the
    // cached value — a bufferless computation (exploratory deploy) must
    // never satisfy a lookup from the create/validate path, or that
    // path's buffer populate is silently skipped.
    pub buffer_populated: bool,
}

/// The merged pre-state a block builds on, with every fact the merge
/// derived alongside it. One struct through the derivation, the
/// parents-post-state cache, and the checkpoint path — the facts travel
/// together or not at all: a consumer holding the state without the
/// applied set cannot tell which deploys' effects that state already
/// contains, and executing one of them again double-applies it.
#[derive(Clone, Debug)]
pub struct MergedPreState {
    pub state: StateHash,
    /// Rejected user deploys as full records — each names the carrier it
    /// adjudicated and carries the formation-time duplicate flag. These
    /// travel to the block body as-is; the record IS the consensus content.
    pub rejected_user: Vec<models::rust::casper::protocol::casper_message::RejectedDeploy>,
    pub rejected_slashes: Vec<crate::rust::merging::rejected_slash::RejectedSlash>,
    /// User sigs whose chains the merge APPLIED from scope: their effects
    /// are in `state`, so executing any of them on top would double-apply.
    /// Empty on the non-merging shapes (genesis, single parent, covering
    /// parent), where effects arrive via a parent's post-state instead.
    pub applied_from_scope: std::collections::HashSet<prost::bytes::Bytes>,
    /// The block whose committed state `state` derives from: on the merged
    /// path the main parent, or the floor when the main parent's state does
    /// not hold the floor's settled content. `None` where the header already
    /// determines it (genesis, single parent, covering parent).
    pub merge_base: Option<BlockHash>,
}

pub type ParentsPostStateCacheVal = MergedPreState;

impl RuntimeManager {
    /// DR-102: test-only records of offered deploy attempts.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn offered_budget_usages(&self) -> Vec<OfferedBudgetUsage> {
        self.offered_budget_recorder.records()
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn record_offered_budget_usage(
        &self,
        kind: OfferedUsageKind,
        deploy_id: &[u8],
        pre_state_root: &[u8],
    ) {
        self.offered_budget_recorder
            .record(kind, deploy_id, pre_state_root);
    }

    pub async fn read_direct_offered_wallet_snapshot<'a>(
        &self,
        envelope: &'a models::rust::deploy_envelope::DeployEnvelope,
        original_root: &StateHash,
        verified: Option<&VerifiedOfferedGrantSources>,
    ) -> Result<
        DirectWalletPolicySnapshot<'a, models::rust::signed_phlo_deploy::OfferedFundedDeploy>,
        CasperError,
    > {
        let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
            return Err(CasperError::RuntimeError(
                "rooted offered wallet capture requires a signed offered envelope".into(),
            ));
        };
        let protocol = offered_funded_v6_limits();
        let limits = DirectWalletFundingLimits {
            members: protocol.envelope.members,
            funding: protocol.envelope.payload.funding,
        };
        let authorization = match verified {
            Some(verified) => {
                authorize_offered_direct_wallet_funding_with_grants(signed, limits, verified)
            }
            None => authorize_offered_direct_wallet_funding(signed, limits),
        }
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        authorization
            .read_policy_snapshot(self, original_root.clone(), std::num::NonZeroUsize::MIN)
            .await
            .map_err(|error| CasperError::RuntimeError(error.to_string()))
    }

    const MAX_BLOCK_INDEX_CACHE_ENTRIES: usize = 128;
    const MAX_PARENTS_POST_STATE_CACHE_ENTRIES: usize = 64;
    const MAX_ACTIVE_VALIDATORS_CACHE_ENTRIES: usize = 256;
    const MAX_BONDS_CACHE_ENTRIES: usize = 64;
    const MAX_REPLAY_CACHE_ENTRIES: usize = 192;
    const MAX_REPLAY_CACHE_BYTES: usize = 32 * 1024 * 1024;
    const MAX_REPLAY_CACHE_EVENT_LOG_ENTRIES: usize = 1_536;

    fn collect_replay_logs(
        usr_processed: &[ProcessedDeploy],
        sys_processed: &[ProcessedSystemDeploy],
    ) -> Vec<Event> {
        let user_log_len: usize = usr_processed.iter().map(|pd| pd.deploy_log.len()).sum();
        let sys_log_len: usize = sys_processed
            .iter()
            .map(|psd| match psd {
                ProcessedSystemDeploy::Succeeded { event_list, .. } => event_list.len(),
                ProcessedSystemDeploy::Failed { event_list, .. } => event_list.len(),
            })
            .sum();

        let mut all_logs = Vec::with_capacity(user_log_len + sys_log_len);

        for pd in usr_processed {
            all_logs.extend(pd.deploy_log.iter().cloned());
        }

        for psd in sys_processed {
            match psd {
                ProcessedSystemDeploy::Succeeded { event_list, .. } => {
                    all_logs.extend(event_list.iter().cloned());
                }
                ProcessedSystemDeploy::Failed { event_list, .. } => {
                    all_logs.extend(event_list.iter().cloned());
                }
            }
        }

        all_logs
    }

    fn replay_payload_hash(
        usr_processed: &[ProcessedDeploy],
        sys_processed: &[ProcessedSystemDeploy],
        is_genesis: bool,
    ) -> Vec<u8> {
        #[inline]
        fn push_len_prefixed(bytes: &mut Vec<u8>, data: &[u8]) {
            bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
            bytes.extend_from_slice(data);
        }

        // Fingerprint replay-relevant payload so cache keys stay safe under adversarial input.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(usr_processed.len() as u64).to_le_bytes());
        for pd in usr_processed {
            push_len_prefixed(&mut bytes, &pd.deploy.sig);
            bytes.extend_from_slice(&pd.cost.cost.to_le_bytes());
            bytes.push(u8::from(pd.is_failed));
            match &pd.system_deploy_error {
                Some(err) => {
                    bytes.push(1);
                    push_len_prefixed(&mut bytes, err.as_bytes());
                }
                None => bytes.push(0),
            }
        }
        bytes.extend_from_slice(&(sys_processed.len() as u64).to_le_bytes());
        for psd in sys_processed {
            match psd {
                ProcessedSystemDeploy::Succeeded { system_deploy, .. } => {
                    bytes.push(0);
                    match system_deploy {
                        SystemDeployData::Slash {
                            invalid_block_hash,
                            issuer_public_key,
                            target_activation_epoch,
                        } => {
                            bytes.push(0);
                            push_len_prefixed(&mut bytes, invalid_block_hash);
                            push_len_prefixed(&mut bytes, &issuer_public_key.bytes);
                            // Little-endian is consensus-determined for this
                            // hash-affecting encoding — every node must agree
                            // on the bytes fed into the post-state hash. Do
                            // not switch to big-endian or `to_be_bytes`.
                            bytes.extend_from_slice(&target_activation_epoch.to_le_bytes());
                        }
                        SystemDeployData::CloseBlockSystemDeployData => {
                            bytes.push(1);
                        }
                        SystemDeployData::Empty => {
                            bytes.push(2);
                        }
                    }
                }
                ProcessedSystemDeploy::Failed { error_msg, .. } => {
                    bytes.push(1);
                    push_len_prefixed(&mut bytes, error_msg.as_bytes());
                }
            }
        }
        bytes.push(u8::from(is_genesis));
        Blake2b256::hash(bytes)
    }

    fn max_block_index_cache_entries() -> usize { Self::MAX_BLOCK_INDEX_CACHE_ENTRIES }

    fn max_parents_post_state_cache_entries() -> usize {
        Self::MAX_PARENTS_POST_STATE_CACHE_ENTRIES
    }

    fn max_active_validators_cache_entries() -> usize { Self::MAX_ACTIVE_VALIDATORS_CACHE_ENTRIES }

    fn max_bonds_cache_entries() -> usize { Self::MAX_BONDS_CACHE_ENTRIES }

    fn max_replay_cache_entries() -> usize { Self::MAX_REPLAY_CACHE_ENTRIES }

    fn max_replay_cache_bytes() -> usize { Self::MAX_REPLAY_CACHE_BYTES }

    fn max_replay_cache_event_log_entries() -> usize { Self::MAX_REPLAY_CACHE_EVENT_LOG_ENTRIES }

    fn record_replay_cache_metrics(cache: &InMemoryReplayCache) -> (usize, usize) {
        let stats = cache.stats();
        metrics::gauge!(REPLAY_CACHE_ENTRIES_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(stats.0 as f64);
        metrics::gauge!(REPLAY_CACHE_RETAINED_BYTES_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(stats.1 as f64);
        stats
    }

    /// Added by DR-134 (dev parity of the offered replay): the fingerprint of
    /// an offered block for the replay-cache key. It binds the exact encoding
    /// of every processed user deploy (its envelope, cost, deploy log, failure
    /// flag and native evidence), then dev's fingerprint of the system
    /// deploys. Two blocks that differ in any of these bytes never share an
    /// entry.
    pub(crate) fn offered_replay_payload_hash(
        usr_processed: &[ProcessedUserDeploy],
        sys_processed: &[ProcessedSystemDeploy],
    ) -> Vec<u8> {
        const DOMAIN: &[u8] = b"offered-replay-payload-v1";
        let encoded: Vec<Vec<u8>> = usr_processed
            .iter()
            .map(|deploy| prost::Message::encode_to_vec(&deploy.to_proto()))
            .collect();
        let system = Self::replay_payload_hash(&[], sys_processed, false);
        let size = DOMAIN.len()
            + size_of::<u64>()
            + encoded
                .iter()
                .map(|deploy| size_of::<u64>() + deploy.len())
                .sum::<usize>()
            + system.len();
        let mut bytes = Vec::with_capacity(size);
        bytes.extend_from_slice(DOMAIN);
        bytes.extend_from_slice(&(encoded.len() as u64).to_le_bytes());
        for deploy in &encoded {
            bytes.extend_from_slice(&(deploy.len() as u64).to_le_bytes());
            bytes.extend_from_slice(deploy);
        }
        bytes.extend_from_slice(&system);
        Blake2b256::hash(bytes)
    }

    /// Added by DR-134 (dev parity of the offered replay): the replay-cache
    /// admission of an offered play, as dev's play admits a block
    /// (`compute_state_with_bonds`). The node that played the block can then
    /// answer a later replay of the same block from the cache. The length of
    /// the logs is checked before they are copied, so a block above the cap
    /// copies nothing.
    pub(crate) fn admit_offered_replay_cache(
        &self,
        start_hash: &StateHash,
        sender: &PublicKey,
        seq_num: i32,
        usr_processed: &[ProcessedUserDeploy],
        sys_processed: &[ProcessedSystemDeploy],
        post_state: &StateHash,
    ) {
        fn user_logs(deploy: &ProcessedUserDeploy) -> &[Event] {
            match deploy {
                ProcessedUserDeploy::Legacy(deploy) => deploy.deploy_log.as_slice(),
                ProcessedUserDeploy::Offered(deploy) => deploy.deploy_log(),
            }
        }
        fn system_logs(deploy: &ProcessedSystemDeploy) -> &[Event] {
            match deploy {
                ProcessedSystemDeploy::Succeeded { event_list, .. } => event_list.as_slice(),
                ProcessedSystemDeploy::Failed { event_list, .. } => event_list.as_slice(),
            }
        }
        let Some(ref cache) = self.replay_cache else {
            return;
        };
        let log_len = usr_processed
            .iter()
            .map(|deploy| user_logs(deploy).len())
            .chain(sys_processed.iter().map(|deploy| system_logs(deploy).len()))
            .sum::<usize>();
        match (
            log_len,
            log_len <= Self::max_replay_cache_event_log_entries(),
        ) {
            (0, _) => {}
            (_, true) => {
                let mut all_logs = Vec::with_capacity(log_len);
                for deploy in usr_processed {
                    all_logs.extend(user_logs(deploy).iter().cloned());
                }
                for deploy in sys_processed {
                    all_logs.extend(system_logs(deploy).iter().cloned());
                }
                let key = ReplayCacheKey::new(
                    start_hash.clone(),
                    sender.bytes.to_vec(),
                    seq_num as i64,
                    Self::offered_replay_payload_hash(usr_processed, sys_processed),
                );
                let entry = ReplayCacheEntry::new(all_logs, post_state.clone());
                let replay_cached = cache.put(key, entry);
                let (cache_entries, cache_retained_bytes) =
                    Self::record_replay_cache_metrics(cache);
                tracing::debug!("[CACHE] Replay cache admission for sender seq={}: cached={}, entries={}, retained_bytes={}", seq_num, replay_cached, cache_entries, cache_retained_bytes);
            }
            (_, false) => {
                tracing::debug!(
                    "[CACHE] Skipped replay cache store for sender seq={} (event_log={})",
                    seq_num,
                    log_len
                );
            }
        }
    }

    /// Added by DR-134 (dev parity of the offered replay): dev's replay-cache
    /// shortcut (`replay_compute_state`) for an offered block. The node that
    /// played the block returns the cached post-state while the mergeable
    /// entry of that post-state is stored. A missing entry, or a cached
    /// post-state that differs from the block's commitment, falls back to the
    /// full replay, which then decides the verdict. Dev also rigs a replay
    /// runtime with the cached log and drops it at once. The offered replay
    /// never reads a rigged replay space, so the shortcut does not spawn one.
    pub(crate) fn offered_replay_cache_hit(
        &self,
        start_hash: &StateHash,
        sender: &PublicKey,
        seq_num: i32,
        usr_processed: &[ProcessedUserDeploy],
        sys_processed: &[ProcessedSystemDeploy],
        expected_post_state: Option<&StateHash>,
    ) -> Result<Option<StateHash>, CasperError> {
        let Some(ref cache) = self.replay_cache else {
            return Ok(None);
        };
        let key = ReplayCacheKey::new(
            start_hash.clone(),
            sender.bytes.to_vec(),
            seq_num as i64,
            Self::offered_replay_payload_hash(usr_processed, sys_processed),
        );
        let Some(entry) = cache.get(&key) else {
            return Ok(None);
        };
        if expected_post_state.is_some_and(|expected| *expected != entry.post_state) {
            tracing::warn!(
                "[CACHE] ReplayCache hit with another post-state for sender seq={}; falling back to full replay",
                seq_num
            );
            return Ok(None);
        }
        let mergeable_key = MergeableKey {
            state_hash: StateHashSerde(entry.post_state.clone()),
            creator: sender.bytes.clone(),
            seq_num,
        };
        let mergeable_key_encoded = bincode::serialize(&mergeable_key).map_err(|e| {
            CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
        })?;
        match self.mergeable_store.contains_key(mergeable_key_encoded)? {
            true => {
                tracing::info!("[CACHE] ReplayCache hit for sender seq={}", seq_num);
                Ok(Some(entry.post_state))
            }
            false => {
                tracing::warn!(
                    "[CACHE] ReplayCache hit without mergeable entry for sender seq={}; falling back to full replay",
                    seq_num
                );
                Ok(None)
            }
        }
    }

    fn try_acquire_exploratory_deploy_permit_with(
        semaphore: Arc<Semaphore>,
    ) -> Option<OwnedSemaphorePermit> {
        semaphore.try_acquire_owned().ok()
    }

    pub fn try_acquire_exploratory_deploy_permit(&self) -> Option<OwnedSemaphorePermit> {
        Self::try_acquire_exploratory_deploy_permit_with(self.exploratory_deploy_semaphore.clone())
    }

    pub fn replay_lock(&self) -> Arc<ReplayLock> { self.replay_lock.clone() }

    pub fn exploratory_deploy_execution_timeout_value(&self) -> Duration {
        self.exploratory_deploy_execution_timeout
    }

    pub fn trim_allocator() {
        #[cfg(target_os = "linux")]
        unsafe {
            unsafe extern "C" {
                fn malloc_trim(pad: usize) -> i32;
            }
            let _ = malloc_trim(0);
        }
    }

    fn touch_cache_key<K>(order: &Mutex<VecDeque<K>>, key: &K)
    where K: Eq + Clone {
        // LRU touch is O(n) due VecDeque::position/remove. This is intentional for now:
        // these caches are tightly bounded (64-256 entries by default), so linear touch
        // remains cheaper than introducing additional synchronized index maps.
        if let Ok(mut guard) = order.lock() {
            if let Some(pos) = guard.iter().position(|existing| existing == key) {
                guard.remove(pos);
            }
            guard.push_back(key.clone());
        }
    }

    fn get_cached_value<K, V>(map: &DashMap<K, V>, order: &Mutex<VecDeque<K>>, key: &K) -> Option<V>
    where
        K: Eq + Hash + Clone,
        V: Clone,
    {
        let value = map.get(key).map(|entry| entry.value().clone());
        if value.is_some() {
            Self::touch_cache_key(order, key);
        }
        value
    }

    fn evict_fifo_entry<K, V>(map: &DashMap<K, V>, order: &Mutex<VecDeque<K>>)
    where K: Eq + Hash + Clone {
        if let Ok(mut guard) = order.lock() {
            while let Some(evict_key) = guard.pop_front() {
                if map.remove(&evict_key).is_some() {
                    break;
                }
            }
        }
    }

    /// Phase 7b-2 boot hook: install (or clear) the shared
    /// payload persistence backend.  Writes to the `payload_store`
    /// slot are visible across every clone of the `RuntimeManager`
    /// because the outer slot is an `Arc<RwLock<_>>` shared by
    /// construction.
    ///
    /// Consensus-safety: `journal_write` reads the store slot per
    /// call; a boot-time set is immediately visible to every live
    /// runtime.  Store identity (which dir, which retention) is a
    /// per-node local concern and does not affect consensus — only
    /// the WAL entries themselves are consensus-observable.
    ///
    /// Idempotent: a boot pipeline that calls this a second time
    /// with the same bundle replaces the slot's contents atomically
    /// without disturbing concurrent reads.
    pub async fn set_payload_store(
        &self,
        bundle: Option<crate::rust::engine::wal_payload_server::PayloadStoreBundle>,
    ) {
        *self.payload_store.write().await = bundle;
    }

    /// Slice-30b boot hook: install (or clear) the shared
    /// `SnapshotWriter` config.  Same clone-visibility discipline
    /// as [`set_payload_store`](Self::set_payload_store) — the
    /// slot's `Arc<RwLock<_>>` is shared across clones.
    ///
    /// A `None` writer disables Consensus-mode WAL-snapshot
    /// scheduling (the pre-slice-30b default); an observer boot
    /// without consensus provisioning passes `None` here.
    pub async fn set_fs_snapshot_writer(
        &self,
        writer: Option<rholang::rust::interpreter::io::snapshot::SnapshotWriter>,
    ) {
        *self.fs_snapshot_writer.write().await = writer;
    }

    /// Boot hook: append `root` to the operator's consensus-static
    /// root list.  Called from `node::setup` for each entry in the
    /// operator's fs bundle (file paths + dir paths).  Duplicates
    /// are permitted (the applier's `allowed_roots` check uses
    /// `starts_with`, which is set-semantic — duplicate entries
    /// cost one extra prefix compare per lookup but don't change
    /// the accept/reject outcome).  No normalization applied here;
    /// callers are expected to pass canonical absolute paths.
    pub async fn register_consensus_static_root(&self, root: std::path::PathBuf) {
        self.consensus_static_roots.write().await.push(root);
    }

    /// Snapshot the current consensus-static root list.  Used by
    /// the boot apply subscriber (slice 5.131) to populate the
    /// WAL applier's `allowed_roots` defense-in-depth check.
    pub async fn consensus_static_roots(&self) -> Vec<std::path::PathBuf> {
        self.consensus_static_roots.read().await.clone()
    }

    pub async fn spawn_runtime(&self) -> Result<RhoRuntimeImpl, CasperError> {
        self.spawn_runtime_with(Vec::new(), self.external_services.clone())
            .await
    }

    pub async fn spawn_offered_runtime(
        &self,
        limits: GrantIssueCallLimits,
    ) -> Result<RhoRuntimeImpl, CasperError> {
        self.spawn_runtime_with(
            vec![grant_issue_definition(limits)],
            ExternalServices::noop(),
        )
        .await
    }

    /// Added by DR-134 (DR-71): the operator's exploratory limit, which bounds
    /// the native dry run.
    pub(crate) fn exploratory_deploy_phlo_limit(&self) -> i64 { self.exploratory_deploy_phlo_limit }

    /// Added by DR-114: the offered play runtime calls the node's external
    /// services once per recorded call. Every replay keeps the noop services,
    /// because replay produces the record and never calls a service.
    pub async fn spawn_offered_play_runtime(
        &self,
        limits: GrantIssueCallLimits,
    ) -> Result<RhoRuntimeImpl, CasperError> {
        self.spawn_runtime_with(
            vec![grant_issue_definition(limits)],
            self.external_services.clone(),
        )
        .await
    }

    async fn spawn_runtime_with(
        &self,
        mut definitions: Vec<Definition>,
        services: ExternalServices,
    ) -> Result<RhoRuntimeImpl, CasperError> {
        let start = std::time::Instant::now();
        let new_space = self.space.spawn().expect("Failed to spawn RSpace");
        let runtime = rho_runtime::create_rho_runtime(
            new_space,
            self.mergeable_tags.clone(),
            true,
            &mut definitions,
            services,
        )
        .await?;
        self.broadcast_fs_slots(&runtime).await;
        metrics::histogram!(RUNTIME_SPAWN_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());

        Ok(runtime)
    }

    pub async fn spawn_replay_runtime(&self) -> Result<RhoRuntimeImpl, CasperError> {
        self.spawn_replay_runtime_with(Vec::new(), self.external_services.clone())
            .await
    }

    pub async fn spawn_offered_replay_runtime(
        &self,
        limits: GrantIssueCallLimits,
    ) -> Result<RhoRuntimeImpl, CasperError> {
        self.spawn_replay_runtime_with(
            vec![grant_issue_definition(limits)],
            ExternalServices::noop(),
        )
        .await
    }

    async fn spawn_replay_runtime_with(
        &self,
        mut definitions: Vec<Definition>,
        services: ExternalServices,
    ) -> Result<RhoRuntimeImpl, CasperError> {
        let start = std::time::Instant::now();
        let new_replay_space = self
            .replay_space
            .spawn()
            .expect("Failed to spawn ReplayRSpace");

        let runtime = rho_runtime::create_replay_rho_runtime(
            new_replay_space,
            self.mergeable_tags.clone(),
            true,
            &mut definitions,
            services,
        )
        .await?;
        self.broadcast_fs_slots(&runtime).await;
        metrics::counter!(RUNTIME_SPAWN_REPLAY_CALLS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(1);
        metrics::histogram!(RUNTIME_SPAWN_REPLAY_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());

        Ok(runtime)
    }

    /// Phase 7b boot broadcast: push the manager's shared
    /// `payload_store` and `fs_snapshot_writer` slot contents onto
    /// a freshly-spawned runtime.  Called by `spawn_runtime` and
    /// `spawn_replay_runtime` so every runtime the manager hands
    /// out starts with the same handler-facing state regardless
    /// of boot-sequence ordering (the slots can be populated
    /// before OR after any given spawn — both orderings end up
    /// consistent).
    ///
    /// Both slots are `Option<_>`:
    ///   * `payload_store` populated by `set_payload_store` at boot
    ///     — the broadcast threads `bundle.persistence` into
    ///     `fs_handles.share_payload_store` so every Consensus-cap
    ///     write handler can content-address-persist its bytes.
    ///   * `fs_snapshot_writer` populated by `set_fs_snapshot_writer`
    ///     — the broadcast threads it onto the runtime's own
    ///     `fs_snapshot_writer` slot so the WAL-snapshot scheduler
    ///     can fire on cadence-hit blocks.
    ///
    /// A `None` slot is a no-op broadcast — matches the observer /
    /// unconfigured deployment case.
    async fn broadcast_fs_slots(&self, runtime: &RhoRuntimeImpl) {
        if let Some(bundle) = self.payload_store.read().await.as_ref() {
            runtime
                .fs_handles
                .share_payload_store(Some(bundle.persistence.clone()));
        }
        // Snapshot the writer out of the manager's read guard
        // BEFORE awaiting the runtime's own write guard.  Avoids
        // holding `self.fs_snapshot_writer.read()` across the
        // `.await` on `runtime.set_fs_snapshot_writer`.  Both
        // locks are on different objects so no deadlock risk
        // today — this is defensive scoping that keeps the hot
        // path free of cross-object-held async guards.
        let writer = self.fs_snapshot_writer.read().await.clone();
        if let Some(writer) = writer {
            runtime.set_fs_snapshot_writer(Some(writer)).await;
        }
        // Share the manager's `RootIdentityRegistry` backing with
        // the spawned runtime via the two-layer-indirection
        // discipline (see `RootIdentityRegistry::share_from`).
        // After this call, the runtime's own `fs_handles.root_registry`
        // routes every read through the manager's inner map — so a
        // post-spawn `register(logical, root)` on the manager is
        // visible through the runtime's handlers.  Idempotent on
        // re-spawn (sharing the same inner twice is a no-op).
        runtime
            .fs_handles
            .share_root_registry(self.root_registry.clone());
        // Share the manager's `SharedLockRegistry` backing with the
        // spawned runtime via the same two-layer-indirection
        // discipline (see `SharedLockRegistry::share_from`).  Cross-
        // cap lock coordination on the same `(dev, inode)` is now
        // visible across every runtime spawned from this manager —
        // without this call, each runtime would carry its own
        // disjoint `LockRegistry` and a `File` cap holding a
        // range-lock in one runtime would not block a sibling cap's
        // acquire attempt in another runtime, defeating the
        // whole-shard coordination guarantee.
        runtime
            .fs_handles
            .share_lock_registry(self.shared_lock_registry.clone());
    }

    pub async fn compute_state(
        &self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
    ) -> Result<(StateHash, Vec<ProcessedDeploy>, Vec<ProcessedSystemDeploy>), CasperError> {
        // Added by DR-134 (DR-66 item 5): a body-only user deploy returns the
        // compatibility error before a runtime is spawned.
        if !terms.is_empty() {
            return Err(super::costacc::offered_only::body_only_deploys_unsupported());
        }
        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);

        // Block data used for mergeable key
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;

        let (state_hash, usr_deploy_res, sys_deploy_res) = runtime_ops
            .compute_state(
                start_hash,
                terms,
                system_deploys,
                block_data,
                invalid_blocks,
                None,
            )
            .await?;

        let (usr_processed, usr_mergeable): (Vec<ProcessedDeploy>, Vec<NumberChannelsEndVal>) =
            usr_deploy_res.into_iter().unzip();
        let (sys_processed, sys_mergeable): (
            Vec<ProcessedSystemDeploy>,
            Vec<NumberChannelsEndVal>,
        ) = sys_deploy_res.into_iter().unzip();
        let replay_cache_event_log_cap = Self::max_replay_cache_event_log_entries();

        // Concat user and system deploys mergeable channel maps
        let mergeable_chs = usr_mergeable
            .into_iter()
            .chain(sys_mergeable.into_iter())
            .collect();

        // Convert from final to diff values and persist mergeable (number) channels for post-state hash
        let pre_state_hash = Blake2b256Hash::from_bytes_prost(start_hash);
        let post_state_hash = Blake2b256Hash::from_bytes_prost(&state_hash);

        // Save mergeable channels to store
        self.save_mergeable_channels(
            post_state_hash,
            sender.bytes.clone(),
            seq_num,
            mergeable_chs,
            &pre_state_hash,
        )?;

        // Cache replay result for potential replay shortcut (including event logs)
        if let Some(ref cache) = self.replay_cache {
            let all_logs = Self::collect_replay_logs(&usr_processed, &sys_processed);
            let replay_payload_hash =
                Self::replay_payload_hash(&usr_processed, &sys_processed, false);

            if !all_logs.is_empty() && all_logs.len() <= replay_cache_event_log_cap {
                let key = ReplayCacheKey::new(
                    start_hash.clone(),
                    sender.bytes.to_vec(),
                    seq_num as i64,
                    replay_payload_hash,
                );
                let entry = ReplayCacheEntry::new(all_logs, state_hash.clone());
                let replay_cached = cache.put(key, entry);
                let (cache_entries, cache_retained_bytes) =
                    Self::record_replay_cache_metrics(cache);
                tracing::debug!("[CACHE] Replay cache admission for sender seq={}: cached={}, entries={}, retained_bytes={}", seq_num, replay_cached, cache_entries, cache_retained_bytes);
            } else if !all_logs.is_empty() {
                tracing::debug!(
                    "[CACHE] Skipped replay cache store for sender seq={} (event_log={})",
                    seq_num,
                    all_logs.len()
                );
            }
        }

        Ok((state_hash, usr_processed, sys_processed))
    }

    pub async fn compute_state_with_bonds_envelopes(
        &self,
        start_hash: &StateHash,
        terms: Vec<PendingDeployCandidate>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        play_budget: Option<std::time::Duration>,
        adopted: Option<&AdoptedResourcePolicy>,
    ) -> Result<
        (
            StateHash,
            Vec<ProcessedUserDeploy>,
            Vec<ProcessedSystemDeploy>,
            Vec<Bond>,
            Vec<[u8; 32]>,
        ),
        CasperError,
    > {
        if terms
            .iter()
            .all(|candidate| matches!(candidate, PendingDeployCandidate::Legacy(_)))
        {
            // Added by DR-134 (DR-66 item 5): only a block with no user deploy
            // takes this branch. A body-only user deploy returns the
            // compatibility error before a runtime is spawned.
            if !terms.is_empty() {
                return Err(super::costacc::offered_only::body_only_deploys_unsupported());
            }
            let legacy = terms
                .into_iter()
                .map(|candidate| match candidate {
                    PendingDeployCandidate::Legacy(deploy) => deploy,
                    PendingDeployCandidate::Envelope(_) => unreachable!(),
                })
                .collect();
            let (state, deploys, system_deploys, bonds, leaves) = self
                .compute_state_with_bonds(
                    start_hash,
                    legacy,
                    system_deploys,
                    block_data,
                    invalid_blocks,
                    play_budget,
                )
                .await?;
            return Ok((
                state,
                deploys
                    .into_iter()
                    .map(ProcessedUserDeploy::Legacy)
                    .collect(),
                system_deploys,
                bonds,
                leaves,
            ));
        }

        let adopted = adopted.ok_or_else(|| {
            CasperError::RuntimeError(
                "offered proposal requires authenticated approved-genesis policy".into(),
            )
        })?;
        if !adopted.offered_funded_v6_active() {
            return Err(CasperError::RuntimeError(
                "offered proposal format is inactive under approved genesis".into(),
            ));
        }
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;
        let runtime = self
            .spawn_offered_runtime(offered_grant_issue_call_limits())
            .await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let (state_hash, user_results, system_results) = runtime_ops
            .compute_state_envelopes(
                start_hash,
                terms,
                system_deploys,
                block_data,
                invalid_blocks.unwrap_or_default(),
                play_budget,
                Some(adopted),
                self,
            )
            .await?;
        let (user_processed, user_mergeable): (Vec<_>, Vec<_>) = user_results.into_iter().unzip();
        let (system_processed, system_mergeable): (Vec<_>, Vec<_>) =
            system_results.into_iter().unzip();
        let bonds = runtime_ops.compute_bonds(&state_hash).await?;
        self.save_mergeable_channels_metered(
            Blake2b256Hash::from_bytes_prost(&state_hash),
            sender.bytes.clone(),
            seq_num,
            user_mergeable.into_iter().chain(system_mergeable).collect(),
            &Blake2b256Hash::from_bytes_prost(start_hash),
        )?;
        // Added by DR-134 (dev parity of the offered replay): the play admits the
        // block to dev's replay cache, as dev's play does.
        self.admit_offered_replay_cache(
            start_hash,
            &sender,
            seq_num,
            &user_processed,
            &system_processed,
            &state_hash,
        );
        let leaves = user_processed
            .iter()
            .map(crate::rust::util::proto_util::hash_processed_user_deploy_leaf)
            .chain(
                system_processed
                    .iter()
                    .map(crate::rust::util::proto_util::hash_processed_system_deploy_leaf),
            )
            .collect();
        Ok((state_hash, user_processed, system_processed, bonds, leaves))
    }

    pub async fn compute_state_with_bonds(
        &self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        play_budget: Option<std::time::Duration>,
    ) -> Result<
        (
            StateHash,
            Vec<ProcessedDeploy>,
            Vec<ProcessedSystemDeploy>,
            Vec<Bond>,
            // Per-deploy Merkle leaves in commit order (user deploys
            // first, then system deploys).  Fed to
            // `proto_util::hash_block_with_leaves` by the propose path
            // to skip re-hashing the body at package time.
            Vec<[u8; 32]>,
        ),
        CasperError,
    > {
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }

        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let runtime = self.spawn_runtime().await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_spawn_runtime", rss_kb);
        }
        let mut runtime_ops = RuntimeOps::new(runtime);

        // Block data used for mergeable key
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;

        let (state_hash, usr_deploy_res, sys_deploy_res) = runtime_ops
            .compute_state(
                start_hash,
                terms,
                system_deploys,
                block_data,
                invalid_blocks,
                play_budget,
            )
            .await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_compute_state", rss_kb);
        }

        let (usr_processed, usr_mergeable): (Vec<ProcessedDeploy>, Vec<NumberChannelsEndVal>) =
            usr_deploy_res.into_iter().unzip();
        let (sys_processed, sys_mergeable): (
            Vec<ProcessedSystemDeploy>,
            Vec<NumberChannelsEndVal>,
        ) = sys_deploy_res.into_iter().unzip();
        let replay_cache_event_log_cap = Self::max_replay_cache_event_log_entries();

        // Concat user and system deploys mergeable channel maps
        let mergeable_chs = usr_mergeable
            .into_iter()
            .chain(sys_mergeable.into_iter())
            .collect();

        // Convert from final to diff values and persist mergeable (number) channels for post-state hash
        let pre_state_hash = Blake2b256Hash::from_bytes_prost(start_hash);
        let post_state_hash = Blake2b256Hash::from_bytes_prost(&state_hash);

        // Save mergeable channels to store
        self.save_mergeable_channels(
            post_state_hash,
            sender.bytes.clone(),
            seq_num,
            mergeable_chs,
            &pre_state_hash,
        )?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_save_mergeable_channels", rss_kb);
        }

        // Cache replay result for potential replay shortcut (including event logs)
        if let Some(ref cache) = self.replay_cache {
            let all_logs = Self::collect_replay_logs(&usr_processed, &sys_processed);
            let replay_payload_hash =
                Self::replay_payload_hash(&usr_processed, &sys_processed, false);

            if !all_logs.is_empty() && all_logs.len() <= replay_cache_event_log_cap {
                let key = ReplayCacheKey::new(
                    start_hash.clone(),
                    sender.bytes.to_vec(),
                    seq_num as i64,
                    replay_payload_hash,
                );
                let entry = ReplayCacheEntry::new(all_logs, state_hash.clone());
                let replay_cached = cache.put(key, entry);
                let (cache_entries, cache_retained_bytes) =
                    Self::record_replay_cache_metrics(cache);
                tracing::debug!("[CACHE] Replay cache admission for sender seq={}: cached={}, entries={}, retained_bytes={}", seq_num, replay_cached, cache_entries, cache_retained_bytes);
            } else if !all_logs.is_empty() {
                tracing::debug!(
                    "[CACHE] Skipped replay cache store for sender seq={} (event_log={})",
                    seq_num,
                    all_logs.len()
                );
            }
        }

        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_cache_updates", rss_kb);
        }

        // Reuse the same spawned runtime for bonds query to avoid a second runtime init.
        let bonds = runtime_ops.compute_bonds(&state_hash).await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_compute_bonds", rss_kb);
        }
        drop(runtime_ops);
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_drop_runtime_ops", rss_kb);
        }

        // Per-deploy Merkle leaves for `hash_block_with_leaves`.  Computed
        // from the processed deploys after the propose loop; cheaper than
        // rehashing from the body at package time, and keeps the Merkle
        // hash recipe owned by `proto_util`.
        let mut block_hash_leaves: Vec<[u8; 32]> =
            Vec::with_capacity(usr_processed.len() + sys_processed.len());
        for pd in &usr_processed {
            block_hash_leaves.push(crate::rust::util::proto_util::hash_processed_deploy_leaf(
                pd,
            ));
        }
        for psd in &sys_processed {
            block_hash_leaves
                .push(crate::rust::util::proto_util::hash_processed_system_deploy_leaf(psd));
        }

        Ok((
            state_hash,
            usr_processed,
            sys_processed,
            bonds,
            block_hash_leaves,
        ))
    }

    pub async fn compute_genesis(
        &self,
        terms: Vec<Signed<DeployData>>,
        block_time: i64,
        block_number: i64,
    ) -> Result<(StateHash, StateHash, Vec<ProcessedDeploy>), CasperError> {
        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);

        let (pre_state, state_hash, processed) = runtime_ops
            .compute_genesis(terms, block_time, block_number)
            .await?;
        let (processed_deploys, mergeable_chs) = processed.into_iter().unzip();

        // Convert from final to diff values and persist mergeable (number) channels for post-state hash
        let pre_state_hash = Blake2b256Hash::from_bytes_prost(&pre_state);
        let post_state_hash = Blake2b256Hash::from_bytes_prost(&state_hash);

        // Save mergeable channels to store
        self.save_mergeable_channels(
            post_state_hash,
            prost::bytes::Bytes::new(),
            0,
            mergeable_chs,
            &pre_state_hash,
        )?;

        Ok((pre_state, state_hash, processed_deploys))
    }

    pub async fn replay_compute_state(
        &self,
        start_hash: &StateHash,
        terms: Vec<ProcessedDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool, // FIXME have a better way of knowing this. Pass the replayDeploy function maybe? - OLD
    ) -> Result<StateHash, CasperError> {
        // Added by DR-134 (DR-66 item 5): body-only user deploys and a genesis
        // that the legacy meter produced return the compatibility error before
        // the replay cache, any runtime and any state reset.
        if !is_genesis && !terms.is_empty() {
            return Err(super::costacc::offered_only::body_only_deploys_unsupported());
        }
        if is_genesis && super::costacc::offered_only::is_legacy_metered_genesis(&terms) {
            return Err(super::costacc::offered_only::legacy_metered_genesis_unsupported());
        }
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;
        let replay_payload_hash = Self::replay_payload_hash(&terms, &system_deploys, is_genesis);

        // Step 1: Check replay cache (deterministic replay delta)
        let replay_cache_key = ReplayCacheKey::new(
            start_hash.clone(),
            sender.bytes.to_vec(),
            seq_num as i64,
            replay_payload_hash,
        );
        if let Some(ref cache) = self.replay_cache {
            if let Some(entry) = cache.get(&replay_cache_key) {
                // A replay that returns Ok must leave the mergeable entry for its
                // post-state persisted — `ensure_mergeable_entry` exists precisely
                // to rebuild a collected entry and has no other way to do it. The
                // cache carries only the event log and the post-state, so honoring
                // the shortcut while the entry is absent returns success having
                // rebuilt nothing; the caller then reports the entry still missing
                // and that error becomes a slashable verdict against the block's
                // proposer. Verify presence first and fall through to the full
                // replay (which persists) when it is gone.
                let mergeable_key = MergeableKey {
                    state_hash: StateHashSerde(entry.post_state.clone()),
                    creator: sender.bytes.clone(),
                    seq_num,
                };
                let mergeable_key_encoded = bincode::serialize(&mergeable_key).map_err(|e| {
                    CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
                })?;

                if self.mergeable_store.contains_key(mergeable_key_encoded)? {
                    tracing::info!("[CACHE] ReplayCache hit for sender seq={}", seq_num);

                    // Rig the replay runtime with cached event log
                    let replay_runtime = self.spawn_replay_runtime().await?;
                    let rspace_events: Vec<_> = entry
                        .event_log
                        .iter()
                        .map(crate::rust::util::event_converter::to_rspace_event)
                        .collect();
                    replay_runtime.rig(rspace_events).await?;

                    return Ok(entry.post_state);
                }

                tracing::warn!(
                    "[CACHE] ReplayCache hit without mergeable entry for sender seq={}; falling back to full replay",
                    seq_num
                );
            }
        }

        // Step 2: Full replay (cache miss)
        let lock_start = std::time::Instant::now();
        let replay_permit = self.replay_lock.acquire_consensus().await;
        metrics::histogram!(BLOCK_REPLAY_RUNTIME_LOCK_WAIT_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(lock_start.elapsed().as_secs_f64());
        let _replay_permit = replay_permit
            .map_err(|error| CasperError::Other(format!("Replay semaphore closed: {}", error)))?;
        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let replay_runtime = self.spawn_replay_runtime().await?;
        let runtime_ops = RuntimeOps::new(replay_runtime);
        let mut replay_runtime_ops = ReplayRuntimeOps::new(runtime_ops);

        let execute_start = std::time::Instant::now();
        let replay_result = replay_runtime_ops
            .replay_compute_state(
                start_hash,
                terms,
                system_deploys,
                block_data,
                Some(invalid_blocks),
                is_genesis,
            )
            .await;
        metrics::histogram!(BLOCK_REPLAY_RUNTIME_EXECUTE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(execute_start.elapsed().as_secs_f64());
        // Discard per-deploy Merkle leaves at this API boundary: callers of
        // `RuntimeManager::replay_compute_state` only need the state hash.
        // The leaves exist to give fail-fast replay semantics (bad deploy
        // detected the moment its leaf is computed); they are also used on
        // the propose path (`compute_state_with_bonds`) to feed
        // `hash_block_with_leaves` without rehashing.  Validation computes
        // `block_hash` via `Validate::block_hash` which recomputes leaves
        // from the stored block body.
        let (state_hash, mergeable_chs, _block_hash_leaves) = replay_result?;

        // Convert from final to diff values and persist mergeable (number) channels for post-state hash
        let pre_state_hash = Blake2b256Hash::from_bytes_prost(start_hash);
        let post_state = state_hash.to_bytes_prost();

        // Phase 9 (G-1): surface persistence failure as a typed
        // `CasperError` instead of a finalization-task panic. A panic
        // here would crash the runtime_manager future with an
        // unhelpful "task panicked" log; the typed Result lets the
        // caller decide how to react (likely abort the proposal, not
        // the whole node).
        let save_start = std::time::Instant::now();
        let save_result = self.save_mergeable_channels(
            state_hash.clone(),
            sender.bytes,
            seq_num,
            mergeable_chs,
            &pre_state_hash,
        );
        metrics::histogram!(BLOCK_REPLAY_RUNTIME_SAVE_MERGEABLE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(save_start.elapsed().as_secs_f64());
        save_result.map_err(|e| {
            CasperError::RuntimeError(format!("Failed to save mergeable channels: {:?}", e))
        })?;

        Ok(post_state)
    }

    // Moved by G2-1 to
    // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/offered_block.rs`,
    // unchanged: `certify_offered_draft` with its DR-102 comment, and
    // `replay_offered_system_continuation`.

    pub async fn replay_compute_state_envelopes(
        &self,
        start_hash: &StateHash,
        terms: Vec<models::rust::casper::protocol::casper_message::ProcessedUserDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool,
    ) -> Result<StateHash, CasperError> {
        self.replay_compute_state_envelopes_with_policy(
            start_hash,
            terms,
            system_deploys,
            block_data,
            invalid_blocks,
            is_genesis,
            None,
            None,
        )
        .await
    }

    pub async fn replay_compute_state_envelopes_with_policy(
        &self,
        start_hash: &StateHash,
        terms: Vec<ProcessedUserDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool,
        adopted: Option<&AdoptedResourcePolicy>,
        expected_post_state: Option<&StateHash>,
    ) -> Result<StateHash, CasperError> {
        if validate_offered_replay_preflight(start_hash, &terms)? {
            // Moved by G2-1 to
            // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/offered_block.rs`,
            // unchanged except that it records the test-only budget usage through
            // `record_offered_budget_usage`.
            return self
                .replay_offered_envelope_state(
                    start_hash,
                    terms,
                    system_deploys,
                    block_data,
                    invalid_blocks,
                    is_genesis,
                    adopted,
                    expected_post_state,
                )
                .await;
        }
        let legacy = terms
            .into_iter()
            .map(|term| match term {
                models::rust::casper::protocol::casper_message::ProcessedUserDeploy::Legacy(
                    deploy,
                ) => Ok(deploy),
                models::rust::casper::protocol::casper_message::ProcessedUserDeploy::Offered(_) => {
                    Err(CasperError::RuntimeError(
                        "offered-funded replay requires independent native validation".to_string(),
                    ))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.replay_compute_state(
            start_hash,
            legacy,
            system_deploys,
            block_data,
            invalid_blocks,
            is_genesis,
        )
        .await
    }

    pub async fn replay_offered_native_user_checked<'policy, 'a>(
        &self,
        policy: &'policy CheckedDirectWalletPolicy<
            'a,
            models::rust::signed_phlo_deploy::OfferedFundedDeploy,
        >,
        processed: &OfferedProcessedDeploy,
        adopted: &AdoptedResourcePolicy,
        schedule: &PhloScheduleBinding<'_>,
        context: NativeFundedExecutionContext,
        candidate_limits: OfferedCandidateLimits,
        recording_limits: NativeRecordingWireLimits,
        replay_limits: NativeFundedReplayLimits,
    ) -> Result<ReplayedNativeFundedUser<'policy, 'a>, CasperError> {
        let evidence_limits: PhloWireLimits = offered_funded_v6_limits().evidence;
        processed
            .validate(evidence_limits)
            .map_err(CasperError::RuntimeError)?;
        let evidence = processed
            .evidence(evidence_limits)
            .map_err(CasperError::RuntimeError)?;
        let DeployEnvelopeRef::OfferedFunded(signed) = processed.envelope().view() else {
            return Err(CasperError::RuntimeError(
                "native replay requires an offered funded envelope".to_string(),
            ));
        };
        let preflight = prepare_offered_candidate(
            signed,
            adopted,
            evidence.original_funding_root,
            evidence.original_funding_root,
            candidate_limits,
        )?;
        if !adopted.offered_funded_v6_active()
            || evidence.genesis_policy_commitment != adopted.genesis_policy_commitment()?
            || evidence.schedule_commitment != schedule.schedule().commitment
            || evidence.phlo_price != schedule.schedule().actual_price
            || evidence.phlo_limit != preflight.phlo_limit
            || evidence.phlo_price != preflight.phlo_price
            || evidence.fee_rev != preflight.fee_rev
            || evidence.envelope_commitment != preflight.envelope_identity
            || evidence.original_funding_root != policy.snapshot().wallets().pre_state_root()
        {
            return Err(CasperError::RuntimeError(
                "offered replay differs from authenticated genesis, schedule, or funding root"
                    .to_string(),
            ));
        }
        let mut replayed = self
            .replay_native_funded_user_from_processed(
                policy,
                processed,
                adopted,
                schedule,
                context,
                evidence_limits,
                recording_limits,
                replay_limits,
            )
            .await?;
        let covered = replayed
            .user_event_count()
            .checked_add(replayed.grant_issue_event_count())
            .ok_or_else(|| {
                CasperError::RuntimeError("offered replay event boundary overflows".to_string())
            })?;
        if covered > processed.deploy_log().len() {
            return Err(CasperError::RuntimeError(
                "offered native replay omits committed grant events".to_string(),
            ));
        }
        let root = replayed.runtime().runtime.create_checkpoint().await.root;
        if root.bytes() != replayed.expected_settlement_runtime_root() {
            replayed
                .runtime()
                .runtime
                .reset(&Blake2b256Hash::from_bytes(
                    evidence.original_funding_root.to_vec(),
                ))
                .await?;
            return Err(CasperError::RuntimeError(
                "offered native replay checkpoint differs from committed settlement root"
                    .to_string(),
            ));
        }
        Ok(replayed)
    }

    // Changed by DR-134 (DR-71, DR-66 item 3): the capture is the native
    // unfunded dry run of the adopted policy. It reads the term and the signer
    // key of `deploy`; the legacy meter and its evaluate are deleted.
    // pub async fn capture_results(
    //     &self,
    //     start: &StateHash,
    //     deploy: &Signed<DeployData>,
    // ) -> Result<Vec<Par>, CasperError> {
    //     let runtime = self.spawn_runtime().await?;
    //     let mut runtime_ops = RuntimeOps::new(runtime);
    //     let computed = runtime_ops.capture_results(start, deploy).await?;
    //     Ok(computed)
    // }
    pub async fn capture_results(
        &self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
        adopted: &AdoptedResourcePolicy,
    ) -> Result<Vec<Par>, CasperError> {
        let (data, _phlo) = self
            .play_exploratory_dry_run(
                deploy.data.term.clone(),
                start,
                Some(deploy.pk.clone()),
                adopted,
            )
            .await?;
        Ok(data)
    }

    pub async fn get_active_validators(
        &self,
        start_hash: &StateHash,
    ) -> Result<Vec<Validator>, CasperError> {
        if let Some(cached) = Self::get_cached_value(
            &self.active_validators_cache,
            &self.active_validators_cache_order,
            start_hash,
        ) {
            return Ok(cached);
        }

        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.get_active_validators(start_hash).await?;

        let max_entries = Self::max_active_validators_cache_entries();
        if self.active_validators_cache.len() >= max_entries {
            Self::evict_fifo_entry(
                &self.active_validators_cache,
                &self.active_validators_cache_order,
            );
        }
        self.active_validators_cache
            .insert(start_hash.clone(), computed.clone());
        Self::touch_cache_key(&self.active_validators_cache_order, start_hash);

        Ok(computed)
    }

    /// On-chain protocol fault-tolerance threshold (ppm) at `start_hash`, or
    /// `None` when the chain's genesis predates the parameter. Read once at
    /// casper construction (`hash_set_casper`) — not cached here.
    pub async fn get_fault_tolerance_threshold_ppm(
        &self,
        start_hash: &StateHash,
    ) -> Result<Option<i64>, CasperError> {
        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        runtime_ops
            .get_fault_tolerance_threshold_ppm(start_hash)
            .await
    }

    /// On-chain consensus parameters `(max-parent-depth, deploy-lifespan,
    /// min-phlo-price)` at `start_hash`, or `None` when the chain's genesis
    /// predates them. Read once at casper construction (`hash_set_casper`) —
    /// not cached here.
    pub async fn get_genesis_resource_policy(
        &self,
        start_hash: &StateHash,
    ) -> Result<models::rust::phlo_schedule::PhloGenesisPolicy, CasperError> {
        let mut runtime_ops = RuntimeOps::new(self.spawn_runtime().await?);
        runtime_ops.get_genesis_resource_policy(start_hash).await
    }

    pub async fn find_genesis_resource_policy(
        &self,
        start_hash: &StateHash,
    ) -> Result<Option<models::rust::phlo_schedule::PhloGenesisPolicy>, CasperError> {
        let mut runtime_ops = RuntimeOps::new(self.spawn_runtime().await?);
        runtime_ops.find_genesis_resource_policy(start_hash).await
    }

    pub async fn get_consensus_parameters(
        &self,
        start_hash: &StateHash,
    ) -> Result<Option<(i32, i64, i64)>, CasperError> {
        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        runtime_ops.get_consensus_parameters(start_hash).await
    }

    pub async fn compute_bonds(&self, hash: &StateHash) -> Result<Vec<Bond>, CasperError> {
        if let Some(cached) =
            Self::get_cached_value(&self.bonds_cache, &self.bonds_cache_order, hash)
        {
            return Ok(cached);
        }

        let runtime = self.spawn_runtime().await?;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.compute_bonds(hash).await?;

        let max_entries = Self::max_bonds_cache_entries();
        if self.bonds_cache.len() >= max_entries {
            Self::evict_fifo_entry(&self.bonds_cache, &self.bonds_cache_order);
        }
        self.bonds_cache.insert(hash.clone(), computed.clone());
        Self::touch_cache_key(&self.bonds_cache_order, hash);

        Ok(computed)
    }

    // Executes deploy as user deploy with immediate rollback
    // Changed by DR-134 (DR-71, DR-66 item 3): the exploratory deploy is the
    // native unfunded dry run of the adopted policy; the legacy meter and its
    // evaluate are deleted.
    // pub async fn play_exploratory_deploy(
    //     &self,
    //     term: String,
    //     hash: &StateHash,
    //     deployer: Option<PublicKey>,
    // ) -> Result<(Vec<Par>, u64), CasperError> {
    //     let runtime = self.spawn_runtime().await?;
    //     let mut runtime_ops = RuntimeOps::new(runtime);
    //     runtime_ops
    //         .play_exploratory_deploy_with_phlo_limit(
    //             term,
    //             hash,
    //             deployer,
    //             self.exploratory_deploy_phlo_limit,
    //         )
    //         .await
    // }
    pub async fn play_exploratory_deploy(
        &self,
        term: String,
        hash: &StateHash,
        deployer: Option<PublicKey>,
        adopted: &AdoptedResourcePolicy,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        self.play_exploratory_dry_run(term, hash, deployer, adopted)
            .await
    }

    pub async fn get_data(&self, hash: StateHash, channel: &Par) -> Result<Vec<Par>, CasperError> {
        let mut runtime = self.spawn_runtime().await?;

        runtime
            .reset(&Blake2b256Hash::from_bytes_prost(&hash))
            .await?;

        let runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.get_data_par(channel).await;
        Ok(computed)
    }

    pub async fn get_continuation(
        &self,
        hash: StateHash,
        channels: Vec<Par>,
    ) -> Result<Vec<(Vec<BindPattern>, Par)>, CasperError> {
        let mut runtime = self.spawn_runtime().await?;

        runtime
            .reset(&Blake2b256Hash::from_bytes_prost(&hash))
            .await?;

        let runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.get_continuation_par(channels).await;
        Ok(computed)
    }

    pub fn get_history_repo(&self) -> RhoHistoryRepository { self.history_repo.clone() }

    /// Check whether a post-state root is recorded in the local rspace
    /// roots store. Used by joiner-side LFS forward-horizon sync to skip
    /// roots that have already been imported. Pure lookup — no side effects.
    pub fn has_root(&self, root: &Blake2b256Hash) -> Result<bool, CasperError> {
        self.history_repo
            .contains_root(root)
            .map_err(|e| CasperError::RuntimeError(format!("has_root lookup failed: {:?}", e)))
    }

    /// Get or compute BlockIndex with caching
    pub fn get_or_compute_block_index<
        D: crate::rust::merging::block_index::ProcessedIndexDeploy,
    >(
        &self,
        block_hash: &BlockHash,
        block_number: i64,
        usr_processed_deploys: &[D],
        sys_processed_deploys: &Vec<ProcessedSystemDeploy>,
        pre_state_hash: &Blake2b256Hash,
        post_state_hash: &Blake2b256Hash,
        mergeable_chs: &Vec<NumberChannelsDiff>,
    ) -> Result<BlockIndex, CasperError> {
        if let Some(cached) = Self::get_cached_value(
            &self.block_index_cache,
            &self.block_index_cache_order,
            block_hash,
        ) {
            metrics::gauge!(BLOCK_INDEX_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
                .set(self.block_index_cache.len() as f64);
            return Ok(cached);
        }

        // Cache miss - compute the BlockIndex.
        let block_index = crate::rust::merging::block_index::new(
            block_hash,
            block_number,
            usr_processed_deploys,
            sys_processed_deploys,
            pre_state_hash,
            post_state_hash,
            &self.history_repo,
            mergeable_chs,
        )?;

        // Keep index cache bounded for long-running validators.
        // Avoid DashMap re-entrant calls while holding an entry guard.
        let max_entries = Self::max_block_index_cache_entries();
        if self.block_index_cache.len() >= max_entries {
            Self::evict_fifo_entry(&self.block_index_cache, &self.block_index_cache_order);
        }

        self.block_index_cache
            .insert(block_hash.clone(), block_index.clone());
        Self::touch_cache_key(&self.block_index_cache_order, block_hash);
        metrics::gauge!(BLOCK_INDEX_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(self.block_index_cache.len() as f64);
        Ok(block_index)
    }

    /// Remove BlockIndex from cache (used during finalization)
    pub fn remove_block_index_cache(&self, block_hash: &BlockHash) {
        self.block_index_cache.remove(block_hash);
        metrics::gauge!(BLOCK_INDEX_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(self.block_index_cache.len() as f64);
    }

    pub fn get_cached_parents_post_state(
        &self,
        key: &ParentsPostStateCacheKey,
    ) -> Option<ParentsPostStateCacheVal> {
        let result = Self::get_cached_value(
            &self.parents_post_state_cache,
            &self.parents_post_state_cache_order,
            key,
        );
        metrics::gauge!(PARENTS_POST_STATE_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(self.parents_post_state_cache.len() as f64);
        result
    }

    pub fn put_cached_parents_post_state(
        &self,
        key: ParentsPostStateCacheKey,
        value: ParentsPostStateCacheVal,
    ) {
        // Keep cache bounded with simple eviction strategy.
        let max_entries = Self::max_parents_post_state_cache_entries();
        if self.parents_post_state_cache.len() >= max_entries {
            Self::evict_fifo_entry(
                &self.parents_post_state_cache,
                &self.parents_post_state_cache_order,
            );
        }
        self.parents_post_state_cache.insert(key.clone(), value);
        Self::touch_cache_key(&self.parents_post_state_cache_order, &key);
        metrics::gauge!(PARENTS_POST_STATE_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(self.parents_post_state_cache.len() as f64);
    }

    /**
     * Load mergeable channels from store
     */
    pub fn load_mergeable_channels(
        &self,
        state_hash_bs: &StateHash,
        creator: prost::bytes::Bytes,
        seq_num: i32,
    ) -> Result<Vec<NumberChannelsDiff>, CasperError> {
        let state_hash = Blake2b256Hash::from_bytes_prost(state_hash_bs);
        let mergeable_key = MergeableKey {
            state_hash: StateHashSerde(state_hash.to_bytes_prost()),
            creator: creator.clone(),
            seq_num,
        };

        let get_key =
            bincode::serialize(&mergeable_key).expect("Failed to serialize mergeable key");

        let res = self.mergeable_store.get_one(&get_key)?;

        match res {
            Some(res) => {
                let res_map = res
                    .into_iter()
                    .map(|x| {
                        x.channels
                            .into_iter()
                            .map(|y| (y.hash, (y.diff, y.merge_type)))
                            .collect::<BTreeMap<_, _>>()
                    })
                    .collect::<Vec<_>>();
                Ok(res_map)
            }
            None => {
                let msg = format!(
                    "Missing mergeable entry for state {} (creator={}, seq={})",
                    state_hash.bytes().encode_hex::<String>(),
                    creator.encode_hex::<String>(),
                    seq_num
                );
                tracing::error!(state_hash = %state_hash.bytes().encode_hex::<String>(), creator = %creator.encode_hex::<String>(), seq_num, "mergeable entry missing for block");
                Err(CasperError::KvStoreError(KvStoreError::KeyNotFound(msg)))
            }
        }
    }

    /// Build the mergeable-store key bytes for a block.
    pub fn mergeable_key_bytes_for_block(
        block: &models::rust::casper::protocol::casper_message::BlockMessage,
    ) -> Result<Vec<u8>, CasperError> {
        let key = MergeableKey {
            state_hash: StateHashSerde(block.body.state.post_state_hash.clone()),
            creator: block.sender.clone(),
            seq_num: block.seq_num,
        };
        bincode::serialize(&key)
            .map_err(|e| CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string())))
    }

    /// Look up a block's mergeable-channels entry and return its over-the-wire
    /// byte form. Returns `(key_bytes, Some(value_bytes))` if present,
    /// `(key_bytes, None)` if absent. Re-serializes via bincode at the typed
    /// store boundary so the wire format is independent of LMDB's internal
    /// encoding.
    pub fn get_mergeable_entry_bytes(
        &self,
        block: &models::rust::casper::protocol::casper_message::BlockMessage,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>), CasperError> {
        let key_bytes = Self::mergeable_key_bytes_for_block(block)?;
        let value: Option<Vec<DeployMergeableData>> = self.mergeable_store.get_one(&key_bytes)?;
        let value_bytes = value
            .map(|v| bincode::serialize(&v))
            .transpose()
            .map_err(|e| {
                CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
            })?;
        Ok((key_bytes, value_bytes))
    }

    /// Store a mergeable-channels entry received over the wire. Decodes the
    /// transported bytes and writes via the typed store. Empty `value_bytes`
    /// signals "peer had no entry" and is a no-op.
    pub fn put_mergeable_entry_bytes(
        &self,
        key_bytes: Vec<u8>,
        value_bytes: Vec<u8>,
    ) -> Result<(), CasperError> {
        if value_bytes.is_empty() {
            return Ok(());
        }
        let value: Vec<DeployMergeableData> = bincode::deserialize(&value_bytes).map_err(|e| {
            CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
        })?;
        self.mergeable_store
            .put_one(key_bytes, value)
            .map_err(CasperError::from)
    }

    /// True iff this node already holds the mergeable-channels entry for `block`.
    /// Its presence is a byproduct of whether this node executed/replayed the
    /// block: a block imported via LFS without replay — or rejected locally and
    /// never replayed — lacks it, while the multi-parent merge requires it for
    /// every scope block. Without a recompute that makes merge validity node-local.
    pub fn has_mergeable_entry(
        &self,
        block: &models::rust::casper::protocol::casper_message::BlockMessage,
    ) -> Result<bool, CasperError> {
        let key = Self::mergeable_key_bytes_for_block(block)?;
        let value: Option<Vec<DeployMergeableData>> = self.mergeable_store.get_one(&key)?;
        Ok(value.is_some())
    }

    /// Materialize the mergeable-channels entry for `block` by replaying it,
    /// unless already present. The mergeable diffs are a deterministic function
    /// of the block's content, so a full replay reconstructs exactly the entry
    /// the proposer stored — making mergeable presence a function of consensus
    /// data on every node rather than of local execution history.
    ///
    /// `invalid_blocks` MUST be the block's own invalid-block set (the set its
    /// original validation used); otherwise the replay computes a different
    /// post-state and the entry would be stored under the wrong key.
    pub async fn ensure_mergeable_entry(
        &self,
        block: &models::rust::casper::protocol::casper_message::BlockMessage,
        invalid_blocks: HashMap<BlockHash, Validator>,
        // Added by DR-134 (DR-66): the replay of an offered block needs the
        // adopted policy, as validation passes it.
        adopted: Option<&AdoptedResourcePolicy>,
    ) -> Result<(), CasperError> {
        if self.has_mergeable_entry(block)? {
            return Ok(());
        }

        let block_data = BlockData::from_block(block);
        let is_genesis = block.header.parents_hash_list.is_empty();
        // Changed by DR-134 (DR-66): the offered replay refuses to run without
        // the adopted policy and the block's post-state, so the recompute
        // passes both, as validation does (interpreter_util.rs, the replay in
        // validate_block_checkpoint_with_policy). The legacy branch ignores the
        // expected post-state.
        // let computed_post_state = self
        //     .replay_compute_state_envelopes(
        //         &block.body.state.pre_state_hash,
        //         block.body.deploys.clone(),
        //         block.body.system_deploys.clone(),
        //         &block_data,
        //         Some(invalid_blocks),
        //         is_genesis,
        //     )
        //     .await?;
        let computed_post_state = self
            .replay_compute_state_envelopes_with_policy(
                &block.body.state.pre_state_hash,
                block.body.deploys.clone(),
                block.body.system_deploys.clone(),
                &block_data,
                Some(invalid_blocks),
                is_genesis,
                adopted,
                Some(&block.body.state.post_state_hash),
            )
            .await?;

        // The entry is keyed by post-state, so a replay that reproduces a
        // different one stores it where nobody looks. Name that case: it is a
        // replay-determinism failure, not a storage failure, and the two need
        // very different responses.
        if computed_post_state != block.body.state.post_state_hash {
            return Err(CasperError::RuntimeError(format!(
                "recompute for block {} (seq={}) produced post-state {} but the block declares {}; \
                 the mergeable entry was stored under the computed key",
                hex::encode(&block.block_hash),
                block.seq_num,
                hex::encode(&computed_post_state),
                hex::encode(&block.body.state.post_state_hash),
            )));
        }

        // Fail closed: the full-replay path persists the entry. If it is still
        // absent the merge would diverge across nodes, so surface it.
        if !self.has_mergeable_entry(block)? {
            return Err(CasperError::RuntimeError(format!(
                "mergeable entry still absent after recompute for block {} (seq={})",
                hex::encode(&block.block_hash),
                block.seq_num,
            )));
        }

        Ok(())
    }

    /// Delete mergeable channels entry keyed by (post-state-hash, creator, seq-num).
    /// Returns `true` if the entry existed prior to deletion.
    pub fn delete_mergeable_channels(
        &self,
        state_hash_bs: &StateHash,
        creator: prost::bytes::Bytes,
        seq_num: i32,
    ) -> Result<bool, CasperError> {
        let mergeable_key = MergeableKey {
            state_hash: StateHashSerde(state_hash_bs.clone()),
            creator,
            seq_num,
        };
        let encoded_key =
            bincode::serialize(&mergeable_key).expect("Failed to serialize mergeable key");
        let existed = self.mergeable_store.contains_key(encoded_key.clone())?;
        if existed {
            self.mergeable_store.delete(vec![encoded_key])?;
        }
        Ok(existed)
    }

    /**
     * Converts final mergeable (number) channel values and save to mergeable store.
     *
     * Tuple (postStateHash, creator, seqNum) is used as a key, preStateHash is used to
     * read initial value to get the difference.
     */
    fn save_mergeable_channels(
        &self,
        post_state_hash: Blake2b256Hash,
        creator: prost::bytes::Bytes,
        seq_num: i32,
        channels_data: Vec<NumberChannelsEndVal>,
        // Used to calculate value difference from final values
        pre_state_hash: &Blake2b256Hash,
    ) -> Result<(), CasperError> {
        // Calculate difference values from final values on number channels
        let diffs = self.convert_number_channels_to_diff(channels_data, pre_state_hash)?;

        // Convert to storage types
        let deploy_channels = diffs
            .into_iter()
            .map(|data| {
                let channels: Vec<NumberChannel> = data
                    .into_iter()
                    .map(|(hash, (diff, merge_type))| NumberChannel {
                        hash,
                        diff,
                        merge_type,
                    })
                    .collect::<Vec<_>>();

                DeployMergeableData { channels }
            })
            .collect();

        // Key is composed from post-state hash and block creator with seq number
        let mergeable_key = MergeableKey {
            state_hash: StateHashSerde(post_state_hash.to_bytes_prost()),
            creator,
            seq_num,
        };

        let key_encoded = bincode::serialize(&mergeable_key).map_err(|e| {
            CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
        })?;

        // Save to mergeable channels store
        self.mergeable_store.put_one(key_encoded, deploy_channels)?;

        Ok(())
    }

    pub(crate) fn save_mergeable_channels_metered(
        &self,
        post_state_hash: Blake2b256Hash,
        creator: prost::bytes::Bytes,
        seq_num: i32,
        channels_data: Vec<NumberChannelsEndVal>,
        pre_state_hash: &Blake2b256Hash,
    ) -> Result<(), CasperError> {
        let root: [u8; 32] = pre_state_hash.0.as_slice().try_into().map_err(|_| {
            CasperError::RuntimeError("offered mergeable pre-state hash is invalid".into())
        })?;
        if !self
            .history_repo
            .contains_root(pre_state_hash)
            .map_err(|error| {
                CasperError::RuntimeError(format!("offered mergeable root lookup failed: {error}"))
            })?
        {
            return Err(CasperError::RuntimeError(
                "offered mergeable pre-state root is not registered".into(),
            ));
        }
        let reader = self.history_repo.native_history_reader(root);
        let unique_channels = channels_data
            .iter()
            .flat_map(|map| map.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        let mut initial_values: BTreeMap<Blake2b256Hash, BigInt> = BTreeMap::new();
        for channel in unique_channels {
            let hash: [u8; 32] = channel.0.as_slice().try_into().map_err(|_| {
                CasperError::RuntimeError("offered mergeable channel hash is invalid".into())
            })?;
            let value = reader
                .with_records(NativeLeafKind::Data, &hash, |records| {
                    if records.len() > 1 {
                        return Err(CasperError::RuntimeError(
                            "offered mergeable channel has multiple pre-state values".into(),
                        ));
                    }
                    let Some(raw) = records.iter().next() else {
                        return Ok(BigInt::default());
                    };
                    // Changed by D-S2 (DR-95) and DR-131 S6: the row decodes
                    // through the cold-store decoder (`decode_datum`) instead
                    // of the typed decoder.
                    // let datum: Datum<ListParWithRandom> =
                    //     decode_record(raw, &meter).map_err(mergeable_read_error)?;
                    let datum: Datum<ListParWithRandom> =
                        decode_datum(raw).map_err(|error| mergeable_read_error(error.into()))?;
                    RholangMergingLogic::try_get_number_with_rnd(&datum.a)
                        .map(|(number, _)| number)
                        .ok_or_else(|| {
                            CasperError::RuntimeError(
                                "offered mergeable pre-state channel is non-numeric".into(),
                            )
                        })
                })
                .map_err(mergeable_read_error)?
                .unwrap_or_default();
            initial_values.insert(channel, value);
        }
        let diffs =
            RholangMergingLogic::calculate_num_channel_diff(channels_data, move |channel| {
                initial_values.get(channel).cloned()
            });
        let deploy_channels = diffs
            .into_iter()
            .map(|data| DeployMergeableData {
                channels: data
                    .into_iter()
                    .map(|(hash, (diff, merge_type))| NumberChannel {
                        hash,
                        diff,
                        merge_type,
                    })
                    .collect(),
            })
            .collect();
        let mergeable_key = MergeableKey {
            state_hash: StateHashSerde(post_state_hash.to_bytes_prost()),
            creator,
            seq_num,
        };
        let key_encoded = bincode::serialize(&mergeable_key).map_err(|error| {
            CasperError::KvStoreError(KvStoreError::SerializationError(error.to_string()))
        })?;
        self.mergeable_store.put_one(key_encoded, deploy_channels)?;
        Ok(())
    }

    /**
     * Converts number channels final values to difference values. Excludes channels without an initial value.
     *
     * @param channelsData Final values
     * @param preStateHash Inital state
     * @return Map with values as difference on number channel
     */
    pub fn convert_number_channels_to_diff(
        &self,
        channels_data: Vec<NumberChannelsEndVal>,
        // Used to calculate value difference from final values
        pre_state_hash: &Blake2b256Hash,
    ) -> Result<Vec<NumberChannelsDiff>, CasperError> {
        let history_repo = self.history_repo.clone();
        let reader = history_repo
            .get_history_reader(pre_state_hash)
            .map_err(|e| {
                CasperError::RuntimeError(format!(
                    "Failed to get history reader for pre-state hash: {:?}",
                    e
                ))
            })?;

        // Build a one-shot base-value map to avoid repeatedly creating history readers per key.
        let unique_channels = channels_data
            .iter()
            .flat_map(|m| m.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        let mut initial_values: BTreeMap<Blake2b256Hash, BigInt> = BTreeMap::new();
        for ch in unique_channels {
            let data = reader.get_data(&ch).map_err(|e| {
                CasperError::RuntimeError(format!(
                    "Error getting data for channel {:?}: {:?}",
                    ch, e
                ))
            })?;
            if data.len() > 1 {
                return Err(CasperError::RuntimeError(format!(
                    "Expected at most one value for number channel {:?}, found {}",
                    ch,
                    data.len()
                )));
            }
            // None = channel doesn't exist (legitimate; start from 0). Some-but-non-numeric
            // is an invariant violation (channel-type stability is a contract-level
            // guarantee — interior nodes always numeric, leaves always Map). Treat as
            // hard failure so the merge is rejected rather than silently substituting 0.
            let value = match data.first() {
                None => BigInt::default(),
                Some(datum) => match RholangMergingLogic::try_get_number_with_rnd(&datum.a) {
                    Some((n, _)) => n,
                    None => {
                        return Err(CasperError::RuntimeError(format!(
                            "Pre-state value for number channel {:?} is non-numeric; \
                             channel-type invariant violated",
                            ch,
                        )));
                    }
                },
            };
            initial_values.insert(ch, value);
        }

        // Calculate difference values from final values on number channels. The diff is
        // the wrapping group inverse (see calculate_num_channel_diff): it faithfully
        // recovers each deploy's intended delta even when execution overflowed. Over-large
        // deltas are rejected downstream at merge (combine checked_add / apply checked_add).
        Ok(RholangMergingLogic::calculate_num_channel_diff(
            channels_data,
            move |ch| initial_values.get(ch).cloned(),
        ))
    }

    /**
     * This is a hard-coded value for `emptyStateHash` which is calculated by
     * [[coop.rchain.casper.rholang.RuntimeOps.emptyStateHash]].
     * Because of the value is actually the same all
     * the time. For some situations, we can just use the value directly for better performance.
     */
    pub fn empty_state_hash_fixed() -> StateHash {
        // Updated 2026-07-04 for the versioned registry FIP: Step 2 wires
        // VersionedRegistry.rho into genesis, adding one more contract to
        // the initial installed set and re-encoding the bootstrap
        // registry's continuations. Coordinated upgrade required.
        // (Prior update: 2026-04-29 by Phase 9 of where-clauses-and-match-guards.)
        hex::decode("b38db9a0203b6b9cf5987024f325b83da33be5c1b820b3f86fd979578f2985d5")
            .unwrap()
            .into()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_with_space_config(
        rspace: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        replay_rspace: ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        history_repo: RhoHistoryRepository,
        mergeable_store: MergeableStore,
        mergeable_tags: std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
        external_services: ExternalServices,
        exploratory_deploy_config: ExploratoryDeployConfig,
        replay_lock: Arc<ReplayLock>,
    ) -> RuntimeManager {
        let replay_cache_size = Self::max_replay_cache_entries();

        RuntimeManager {
            space: rspace,
            replay_space: replay_rspace,
            history_repo,
            mergeable_store,
            mergeable_tags,
            block_index_cache: Arc::new(DashMap::new()),
            block_index_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            active_validators_cache: Arc::new(DashMap::new()),
            active_validators_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            bonds_cache: Arc::new(DashMap::new()),
            bonds_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            parents_post_state_cache: Arc::new(DashMap::new()),
            parents_post_state_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            replay_cache: (replay_cache_size > 0).then(|| {
                Arc::new(InMemoryReplayCache::with_limits(
                    replay_cache_size,
                    Self::max_replay_cache_bytes(),
                ))
            }),
            replay_lock,
            exploratory_deploy_semaphore: Arc::new(Semaphore::new(
                exploratory_deploy_config.max_concurrent,
            )),
            exploratory_deploy_phlo_limit: exploratory_deploy_config.phlo_limit,
            exploratory_deploy_execution_timeout: exploratory_deploy_config.execution_timeout,
            external_services,
            #[cfg(any(test, feature = "test-utils"))]
            offered_budget_recorder: OfferedBudgetRecorder::default(),
            fs_snapshot_writer: Arc::new(tokio::sync::RwLock::new(None)),
            snapshot_merkle_roots: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            payload_store: Arc::new(tokio::sync::RwLock::new(None)),
            consensus_static_roots: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            root_registry:
                rholang::rust::interpreter::io::path::identity::RootIdentityRegistry::new(),
            shared_lock_registry: rholang::rust::interpreter::io::lock::SharedLockRegistry::new(),
        }
    }

    pub fn create_with_store(
        store: RSpaceStore,
        mergeable_store: MergeableStore,
        mergeable_tags: std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
        external_services: ExternalServices,
    ) -> RuntimeManager {
        let (rt_manager, _) =
            Self::create_with_history(store, mergeable_store, mergeable_tags, external_services);
        rt_manager
    }

    /// Test-only entry point: supplies `ExploratoryDeployConfig::for_tests()`.
    /// Production construction goes through `create_with_history_config` with
    /// the operator's configuration.
    pub fn create_with_history(
        store: RSpaceStore,
        mergeable_store: MergeableStore,
        mergeable_tags: std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
        external_services: ExternalServices,
    ) -> (RuntimeManager, RhoHistoryRepository) {
        Self::create_with_history_config(
            store,
            mergeable_store,
            mergeable_tags,
            external_services,
            ExploratoryDeployConfig::for_tests(),
            Arc::new(ReplayLock::new()),
        )
    }

    pub fn create_with_history_config(
        store: RSpaceStore,
        mergeable_store: MergeableStore,
        mergeable_tags: std::sync::Arc<
            std::collections::HashMap<
                Par,
                rspace_plus_plus::rspace::merger::merging_logic::MergeType,
            >,
        >,
        external_services: ExternalServices,
        exploratory_deploy_config: ExploratoryDeployConfig,
        replay_lock: Arc<ReplayLock>,
    ) -> (RuntimeManager, RhoHistoryRepository) {
        let (rspace, replay_rspace) =
            RSpace::create_with_replay(store, Arc::new(Box::new(Matcher)))
                .expect("Failed to create RSpaceWithReplay");

        let history_repo = rspace.get_history_repository();

        let runtime_manager = RuntimeManager::create_with_space_config(
            rspace,
            replay_rspace,
            history_repo.clone(),
            mergeable_store,
            mergeable_tags,
            external_services,
            exploratory_deploy_config,
            replay_lock,
        );

        (runtime_manager, history_repo)
    }

    /**
     * Creates connection to [[MergeableStore]] database.
     *
     * Mergeable (number) channels store is used in [[RuntimeManager]] implementation.
     * This function provides default instantiation.
     */
    pub async fn mergeable_store(
        kvm: &mut dyn KeyValueStoreManager,
    ) -> Result<MergeableStore, KvStoreError> {
        let store = kvm.store("mergeable-channel-cache".to_string()).await?;

        Ok(KeyValueTypedStoreImpl::new(store))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::Semaphore;

    use super::{ExploratoryDeployConfig, ReplayLock, RuntimeManager, REPLAY_LOCK_PERMITS};

    #[test]
    fn cache_hit_releases_the_shard_before_waiting_for_order_lock() {
        use std::collections::VecDeque;
        use std::sync::{mpsc, Mutex};
        use std::time::Instant;

        use dashmap::try_result::TryResult;
        use dashmap::DashMap;

        struct CloneSignal(mpsc::Sender<()>);

        impl Clone for CloneSignal {
            fn clone(&self) -> Self {
                self.0.send(()).unwrap();
                Self(self.0.clone())
            }
        }

        let (cloned_tx, cloned_rx) = mpsc::channel();
        let cache = Arc::new(DashMap::new());
        cache.insert(1, CloneSignal(cloned_tx));
        let order = Arc::new(Mutex::new(VecDeque::from([1])));
        let order_guard = order.lock().unwrap();
        let reader = {
            let cache = cache.clone();
            let order = order.clone();
            std::thread::spawn(move || RuntimeManager::get_cached_value(&cache, &order, &1))
        };

        let cloned = cloned_rx.recv_timeout(Duration::from_secs(5)).is_ok();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut shard_available = false;
        if cloned {
            loop {
                if let TryResult::Present(_) = cache.try_get_mut(&1) {
                    shard_available = true;
                    break;
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::yield_now();
            }
        }

        drop(order_guard);
        let result = reader.join().unwrap();
        assert!(
            cloned,
            "A cache hit must clone before it waits for the order lock"
        );
        assert!(
            shard_available,
            "A cache hit must release its shard before it waits for the order lock"
        );
        assert!(result.is_some());
    }

    #[test]
    fn cache_reads_preserve_hit_order_and_ignore_misses() {
        use std::collections::VecDeque;
        use std::sync::Mutex;

        use dashmap::DashMap;

        let cache = DashMap::new();
        cache.insert(1, 11);
        cache.insert(2, 22);
        let order = Mutex::new(VecDeque::from([1, 2]));

        assert_eq!(
            RuntimeManager::get_cached_value(&cache, &order, &1),
            Some(11)
        );
        assert_eq!(RuntimeManager::get_cached_value(&cache, &order, &3), None);
        assert_eq!(*order.lock().unwrap(), VecDeque::from([2, 1]));

        RuntimeManager::evict_fifo_entry(&cache, &order);

        assert!(!cache.contains_key(&2));
        assert_eq!(*cache.get(&1).unwrap(), 11);
        assert_eq!(*order.lock().unwrap(), VecDeque::from([1]));
    }

    #[test]
    fn exploratory_deploy_config_rejects_non_positive_values() {
        assert!(ExploratoryDeployConfig::new(0, 5_000_000, Duration::from_secs(15)).is_err());
        assert!(ExploratoryDeployConfig::new(1, 0, Duration::from_secs(15)).is_err());
        assert!(ExploratoryDeployConfig::new(1, -1, Duration::from_secs(15)).is_err());
        assert!(ExploratoryDeployConfig::new(1, 5_000_000, Duration::ZERO).is_err());

        let valid =
            ExploratoryDeployConfig::new(2, 42, Duration::from_millis(500)).expect("valid config");
        assert_eq!(valid.max_concurrent, 2);
        assert_eq!(valid.phlo_limit, 42);
        assert_eq!(valid.execution_timeout, Duration::from_millis(500));
    }

    #[test]
    fn exploratory_deploy_concurrency_derivation() {
        assert_eq!(ExploratoryDeployConfig::derive_max_concurrent(1), 2);
        assert_eq!(ExploratoryDeployConfig::derive_max_concurrent(4), 2);
        assert_eq!(ExploratoryDeployConfig::derive_max_concurrent(8), 6);
        assert_eq!(ExploratoryDeployConfig::derive_max_concurrent(16), 14);

        let derived = ExploratoryDeployConfig::resolve(0, 5_000_000, Duration::from_secs(15))
            .expect("the sentinel derives, never rejects");
        assert!(derived.max_concurrent >= 2);

        let explicit = ExploratoryDeployConfig::resolve(5, 5_000_000, Duration::from_secs(15))
            .expect("explicit value");
        assert_eq!(explicit.max_concurrent, 5);

        let clamped =
            ExploratoryDeployConfig::resolve(usize::MAX, 5_000_000, Duration::from_secs(15))
                .expect("an oversized explicit value clamps instead of panicking");
        assert_eq!(clamped.max_concurrent, Semaphore::MAX_PERMITS);

        assert!(
            ExploratoryDeployConfig::resolve(0, 0, Duration::from_secs(15)).is_err(),
            "the sentinel does not bypass the other range checks"
        );
    }

    #[tokio::test]
    async fn consensus_replay_has_priority_over_queued_reporting() {
        let lock = Arc::new(ReplayLock::with_permits(1));
        let first_consensus = lock.acquire_consensus().await.expect("Replay lock closed");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let reporting_lock = lock.clone();
        let reporting_tx = tx.clone();
        let reporting = tokio::spawn(async move {
            let _permit = reporting_lock
                .acquire_reporting()
                .await
                .expect("Replay lock closed");
            reporting_tx.send("reporting").expect("Receiver closed");
        });
        tokio::time::sleep(Duration::from_millis(10)).await;

        let consensus_lock = lock.clone();
        let consensus = tokio::spawn(async move {
            let _permit = consensus_lock
                .acquire_consensus()
                .await
                .expect("Replay lock closed");
            tx.send("consensus").expect("Receiver closed");
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(first_consensus);

        assert_eq!(rx.recv().await, Some("consensus"));
        assert_eq!(rx.recv().await, Some("reporting"));
        consensus.await.expect("Consensus task failed");
        reporting.await.expect("Reporting task failed");
    }

    #[tokio::test]
    async fn cancelled_consensus_waiter_releases_reporting() {
        let lock = Arc::new(ReplayLock::with_permits(1));
        let reporting_permit = lock.acquire_reporting().await.expect("Replay lock closed");
        let consensus_lock = lock.clone();
        let consensus = tokio::spawn(async move { consensus_lock.acquire_consensus().await });
        tokio::time::sleep(Duration::from_millis(10)).await;
        consensus.abort();
        consensus
            .await
            .expect_err("Consensus task was not cancelled");
        drop(reporting_permit);

        let _permit = tokio::time::timeout(Duration::from_secs(1), lock.acquire_reporting())
            .await
            .expect("Reporting remained blocked")
            .expect("Replay lock closed");
    }

    #[tokio::test]
    async fn consensus_replays_run_concurrently() {
        let lock = ReplayLock::new();
        let _first = lock.acquire_consensus().await.expect("Replay lock closed");
        let _second = tokio::time::timeout(Duration::from_secs(1), lock.acquire_consensus())
            .await
            .expect("Second consensus replay waited for the first")
            .expect("Replay lock closed");
    }

    #[tokio::test]
    async fn reporting_runs_alongside_consensus_replays() {
        let lock = ReplayLock::new();
        let mut consensus = Vec::new();
        for _ in 0..REPLAY_LOCK_PERMITS - 1 {
            consensus.push(lock.acquire_consensus().await.expect("Replay lock closed"));
        }
        let _reporting = tokio::time::timeout(Duration::from_secs(1), lock.acquire_reporting())
            .await
            .expect("Reporting waited for running consensus replays")
            .expect("Replay lock closed");
    }

    #[tokio::test]
    async fn replay_permits_are_bounded() {
        let lock = ReplayLock::new();
        let mut consensus = Vec::new();
        for _ in 0..REPLAY_LOCK_PERMITS {
            consensus.push(lock.acquire_consensus().await.expect("Replay lock closed"));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), lock.acquire_consensus())
                .await
                .is_err(),
            "A replay ran past the permit bound"
        );
        consensus.pop();
        let _permit = tokio::time::timeout(Duration::from_secs(1), lock.acquire_consensus())
            .await
            .expect("A released permit was not reused")
            .expect("Replay lock closed");
    }

    #[tokio::test]
    async fn reporting_yields_to_waiting_consensus_when_permits_run_out() {
        let lock = Arc::new(ReplayLock::with_permits(2));
        let first = lock.acquire_consensus().await.expect("Replay lock closed");
        let second = lock.acquire_consensus().await.expect("Replay lock closed");

        let waiting_lock = lock.clone();
        let waiting = tokio::spawn(async move { waiting_lock.acquire_consensus().await });
        tokio::time::sleep(Duration::from_millis(10)).await;
        let reporting_lock = lock.clone();
        let reporting = tokio::spawn(async move { reporting_lock.acquire_reporting().await });
        tokio::time::sleep(Duration::from_millis(10)).await;

        drop(first);
        let _third = tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("Waiting consensus replay lost its turn to reporting")
            .expect("Consensus task failed")
            .expect("Replay lock closed");
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!reporting.is_finished());

        drop(second);
        tokio::time::timeout(Duration::from_secs(1), reporting)
            .await
            .expect("Reporting remained blocked")
            .expect("Reporting task failed")
            .expect("Replay lock closed");
    }

    #[tokio::test]
    async fn runtime_manager_clones_share_the_constructed_replay_budget() {
        use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
        use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

        let mut stores = InMemoryStoreManager::new();
        let replay_lock = Arc::new(ReplayLock::with_permits(8));
        let (manager, _) = RuntimeManager::create_with_history_config(
            stores.r_space_stores().await.unwrap(),
            RuntimeManager::mergeable_store(&mut stores).await.unwrap(),
            crate::rust::genesis::genesis::Genesis::default_mergeable_tags_arc(),
            rholang::rust::interpreter::external_services::ExternalServices::noop(),
            ExploratoryDeployConfig::for_tests(),
            replay_lock.clone(),
        );
        let runtime_clone = manager.clone();
        let reporting_lock = runtime_clone.replay_lock();
        assert!(Arc::ptr_eq(&manager.replay_lock(), &replay_lock));
        assert!(Arc::ptr_eq(&reporting_lock, &replay_lock));
        let reporting = reporting_lock.acquire_reporting().await.unwrap();
        let mut consensus = Vec::new();
        for _ in 0..7 {
            consensus.push(
                tokio::time::timeout(Duration::from_secs(1), replay_lock.acquire_consensus())
                    .await
                    .expect("The configured replay budget must be available at construction")
                    .unwrap(),
            );
        }
        assert_eq!(replay_lock.semaphore.available_permits(), 0);
        drop(reporting);
        assert_eq!(replay_lock.semaphore.available_permits(), 1);
        drop(consensus);
        assert_eq!(replay_lock.semaphore.available_permits(), 8);
    }

    #[test]
    fn exploratory_deploy_permit_is_bounded_and_released() {
        let semaphore = Arc::new(Semaphore::new(1));
        let first = RuntimeManager::try_acquire_exploratory_deploy_permit_with(semaphore.clone());
        assert!(first.is_some());

        let second = RuntimeManager::try_acquire_exploratory_deploy_permit_with(semaphore.clone());
        assert!(second.is_none());

        drop(first);

        let third = RuntimeManager::try_acquire_exploratory_deploy_permit_with(semaphore);
        assert!(third.is_some());
    }
}
