// See casper/src/main/scala/coop/rchain/casper/util/rholang/RuntimeManager.scala
// See casper/src/main/scala/coop/rchain/casper/util/rholang/RuntimeManagerSyntax.scala

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::{Cosigned, Signed};
use dashmap::DashMap;
use hex::ToHex;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::block::state_hash::{StateHash, StateHashSerde};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, DeployData, Event, ProcessedDeploy, ProcessedSystemDeploy,
};
use models::rust::host_work::HostWorkLimits;
use models::rust::validator::Validator;
use prost::Message;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::rholang_merging_logic::{
    DeployMergeableData, NumberChannel, RholangMergingLogic,
};
use rholang::rust::interpreter::rho_runtime::{
    self, RhoHistoryRepository, RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
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
    BLOCK_INDEX_CACHE_RETAINED_BYTES_METRIC, BLOCK_INDEX_CACHE_SIZE_METRIC, CASPER_METRICS_SOURCE,
    PARENTS_POST_STATE_CACHE_SIZE_METRIC, REPLAY_CACHE_ENTRIES_METRIC,
    REPLAY_CACHE_RETAINED_BYTES_METRIC, RUNTIME_SPAWN_REPLAY_TIME_METRIC,
    RUNTIME_SPAWN_TIME_METRIC, USER_DEPLOY_EXECUTIONS_METRIC,
};
use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::replay_cache::{
    InMemoryReplayCache, ReplayCache, ReplayCacheContext, ReplayCacheEntry, ReplayCacheKey,
};
use crate::rust::util::rholang::replay_cache_state::persist_before_publish;
use crate::rust::util::rholang::replay_failure::ReplayFailure;

type MergeableStore = KeyValueTypedStoreImpl<ByteVector, Vec<DeployMergeableData>>;

#[derive(serde::Serialize, serde::Deserialize)]
struct MergeableKey {
    post_state_hash: StateHashSerde,
    pre_state_hash: StateHashSerde,
    #[serde(with = "shared::rust::serde_bytes")]
    creator: prost::bytes::Bytes,
    seq_num: i32,
    payload_hash: Vec<u8>,
}

/// c-2 review-follow-up (2026-08-30): mirror of `rholang::rust::
/// interpreter::io::path::canonicalize_lexical`'s normalization for
/// a single already-full path (no `rel` join).  Strips
/// `Component::CurDir` (`.`) segments so registered
/// `consensus_static_roots` entries share the same lexical shape as
/// the leader's WAL entry `canon_path` values (which come from
/// `canonicalize_lexical(root, rel)`).
///
/// Does NOT resolve `..` — same discipline as `canonicalize_lexical`
/// (rejecting `..` in Rholang-facing paths is `safe_descend`'s job
/// upstream).  Does NOT follow symlinks — canonicalize's disk-I/O
/// domain is deliberately excluded so the check stays deterministic
/// under operator-controlled provisioning without host-state
/// coupling.
fn normalize_root_lexical(p: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut normalized = std::path::PathBuf::new();
    for component in p.components() {
        match component {
            Component::CurDir => {} // skip `.`
            other => normalized.push(other),
        }
    }
    normalized
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

pub struct ReplayLock {
    semaphore: Arc<Semaphore>,
    consensus_waiters: std::sync::atomic::AtomicUsize,
    consensus_ready: tokio::sync::Notify,
}

struct ConsensusReplayWaiter<'a>(&'a ReplayLock);

impl Drop for ConsensusReplayWaiter<'_> {
    fn drop(&mut self) {
        self.0
            .consensus_waiters
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        self.0.consensus_ready.notify_waiters();
    }
}

impl ReplayLock {
    pub fn new() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(1)),
            consensus_waiters: std::sync::atomic::AtomicUsize::new(0),
            consensus_ready: tokio::sync::Notify::new(),
        }
    }

    pub async fn acquire_consensus(
        &self,
    ) -> Result<OwnedSemaphorePermit, tokio::sync::AcquireError> {
        self.consensus_waiters
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let waiter = ConsensusReplayWaiter(self);
        let permit = self.semaphore.clone().acquire_owned().await;
        drop(waiter);
        permit
    }

    pub async fn acquire_reporting(
        &self,
    ) -> Result<OwnedSemaphorePermit, tokio::sync::AcquireError> {
        loop {
            while self
                .consensus_waiters
                .load(std::sync::atomic::Ordering::Acquire)
                > 0
            {
                let ready = self.consensus_ready.notified();
                tokio::pin!(ready);
                ready.as_mut().enable();
                if self
                    .consensus_waiters
                    .load(std::sync::atomic::Ordering::Acquire)
                    > 0
                {
                    ready.await;
                }
            }
            let permit = self.semaphore.clone().acquire_owned().await?;
            if self
                .consensus_waiters
                .load(std::sync::atomic::Ordering::Acquire)
                == 0
            {
                return Ok(permit);
            }
            drop(permit);
            tokio::task::yield_now().await;
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
    pub mergeable_tags: std::sync::Arc<
        std::collections::HashMap<Par, rspace_plus_plus::rspace::merger::merging_logic::MergeType>,
    >,
    block_index_cache: Arc<DashMap<BlockHash, BlockIndex>>,
    block_index_cache_order: Arc<Mutex<VecDeque<BlockHash>>>,
    block_index_cache_retained_bytes: Arc<AtomicUsize>,
    block_index_cache_write_lock: Arc<Mutex<()>>,
    pub active_validators_cache: Arc<DashMap<StateHash, Vec<Validator>>>,
    pub active_validators_cache_order: Arc<Mutex<VecDeque<StateHash>>>,
    pub bonds_cache: Arc<DashMap<StateHash, Vec<Bond>>>,
    pub bonds_cache_order: Arc<Mutex<VecDeque<StateHash>>>,
    pub bond_generations_cache: Arc<DashMap<StateHash, HashMap<Validator, i64>>>,
    pub bond_generations_cache_order: Arc<Mutex<VecDeque<StateHash>>>,
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
    /// Slice 30b: shared snapshot-writer config threaded into every
    /// runtime spawned by this manager.  `None` when the operator
    /// has no consensus-static provisioning (backward compat).
    /// Populated at boot via `set_fs_snapshot_writer` from
    /// `node::configuration::snapshot_config::build_snapshot_writer`.
    /// Wrapped in `Arc<RwLock<_>>` so a Cloned `RuntimeManager`
    /// shares the same slot — a boot-time set on one clone is
    /// visible to all others (the runtime is Cloned into every
    /// engine that spawns runtimes).
    pub fs_snapshot_writer:
        Arc<tokio::sync::RwLock<Option<rholang::rust::interpreter::io::snapshot::SnapshotWriter>>>,

    /// H-1 fix (2026-08-06) — slice 30c Phase B: per-block WAL slice
    /// cache, keyed by the block's post-state hash.  Populated by
    /// `play_deploys_for_state` after it computes the per-block WAL
    /// slice; consumed by the finalization runner's `new_lfb_found_
    /// effect` when a newly-finalized block hits a cadence boundary
    /// so `SnapshotWriter::maybe_write` writes the slice for that
    /// specific (finalized) block, not every candidate block at
    /// `block_number % cadence == 0` (the pre-H-1 per-block trigger
    /// forked snapshot writes on sibling non-finalized DAG tips).
    ///
    /// Value pair: `(block_number, slice)` — block_number is
    /// pre-read from the runtime's block_data so the finalizer
    /// doesn't need to re-derive it from the block.
    ///
    /// Cache is bounded by the natural rate at which the LFB
    /// advances — entries are evicted at finalization time (both
    /// the finalized block's own entry and any stale entries whose
    /// block_number is <= the new LFB height, which represent
    /// orphaned sibling forks).  A live shard producing at 1 block/
    /// sec with 10s finalization latency holds ~10 entries.  In
    /// deep-fork edge cases the cache can grow briefly; a
    /// `MAX_PENDING_WAL_SLICES` guard prevents unbounded growth
    /// (defensive; not expected to hit under normal operation).
    pub pending_wal_slices: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<
                Vec<u8>,
                (i64, Vec<rholang::rust::interpreter::io::wal::WalEntry>),
            >,
        >,
    >,

    /// Phase 7b-1 (2026-08-27): per-block snapshot Merkle roots
    /// keyed by finalized block hash.  Populated by the
    /// `WalSnapshotWrite` finalization effect after `maybe_write`
    /// returns `Some((root, merkle_root))`; consumed by the
    /// follow-up wire-fetch layer (`SnapshotChunkRetriever`) so
    /// joiners can verify chunks received over `get_snapshot_chunk`
    /// against a locally-anchored Merkle root without a per-request
    /// disk read.
    ///
    /// Values are `(atomic_root, merkle_root)` — the atomic root is
    /// the Blake2b256 content-address (also on-disk filename) and
    /// the merkle_root is the Phase 7b-1 anchor over 4 MiB chunk
    /// hashes.  See `rholang/src/rust/interpreter/io/snapshot_chunk.rs`.
    ///
    /// Cache eviction: entries are added on each finalized
    /// cadence-hit block and never evicted from this cache
    /// individually — the Merkle root is small (32 bytes) and the
    /// caller (SnapshotChunkRetriever) needs random access by
    /// block hash for arbitrary joiner requests.  Bounded by the
    /// natural rate of finalized cadence hits; on-disk persistence
    /// is a follow-up when the RuntimeManager gains a snapshot-
    /// dedicated store.
    pub snapshot_merkle_roots:
        Arc<tokio::sync::RwLock<std::collections::HashMap<Vec<u8>, ([u8; 32], [u8; 32])>>>,

    /// H-5 fix (2026-08-06): shared root-identity registry.
    /// Populated once at node boot from operator-provisioned
    /// root paths (`(dev, inode)` captured via
    /// `path::capture_root_identity`); consumed on every
    /// `safe_descend_verified` in the fs_* handlers to detect
    /// post-boot rename-and-recreate of the root directory.
    /// Attached to every spawned runtime's `FileHandleTable`
    /// via `share_root_registry` (mirror of `fs_snapshot_writer`
    /// pattern).
    pub root_id_registry: rholang::rust::interpreter::io::path::RootIdentityRegistry,

    /// Phase 8 slice 8a: shared range-lock registry.  Colocated with
    /// `root_id_registry` and broadcast to every spawned runtime via
    /// `FileHandleTable::share_lock_registry`.  Cross-cap lock
    /// coordination on the same `(dev, inode)` — the fresh-mint
    /// posture of slice 27 means each `File` cap has its own fd, so
    /// coordination must live above the fd table.  See X-1 memo in
    /// the plan.
    pub lock_registry: rholang::rust::interpreter::io::lock::LockRegistry,

    /// Phase 7b-2 (2026-08-27): shared write-payload store bundle.
    /// Boot pipeline populates via `set_payload_store` from a
    /// `PayloadStoreBundle::from_directory(...)` pointed at
    /// `<data-dir>/wal_payload_store/` (DD-7b-1 (a)).  The bundle
    /// carries both trait-object aspects of the same underlying
    /// store — `PayloadPersistence` (write side, threaded into
    /// every spawned runtime via
    /// `FileHandleTable::share_payload_store`) and `PayloadLookup`
    /// (read side, threaded into `WalPayloadContext.payload_lookup`).
    ///
    /// `None` when the operator has no consensus-static provisioning
    /// (observer nodes, dev-mode nodes) OR when the boot pipeline
    /// hasn't fired yet.  Handlers see `None` and skip the persist
    /// step; joiners can still fetch from other peers.
    ///
    /// Wrapped in `Arc<RwLock<Option<...>>>` for the same reason as
    /// `fs_snapshot_writer` — a boot-time set on one clone is
    /// visible to all others.
    pub payload_store: Arc<
        tokio::sync::RwLock<Option<crate::rust::engine::wal_payload_server::PayloadStoreBundle>>,
    >,

    /// DD-7b-2 (a) Option 2 (2026-08-29): shared payload-source
    /// recorder.  Boot pipeline populates via
    /// `set_payload_source_recorder` from a
    /// `BlockStorageBackedRecorder` wrapping the
    /// `BlockDagKeyValueStorage` handle.  Threaded into every
    /// spawned runtime's `FileHandleTable::payload_source_recorder`
    /// via `share_payload_source_recorder`.
    ///
    /// `None` when the operator hasn't wired an index yet OR when
    /// the boot pipeline hasn't fired.  Handlers see `None` and
    /// skip the record step; joiners can still fall through to
    /// the local `PayloadLookup` or peer fetch.
    ///
    /// Wrapped in `Arc<RwLock<Option<...>>>` for the same reason
    /// as `payload_store` — a boot-time set on one clone is
    /// visible to every other clone.
    pub payload_source_recorder: Arc<
        tokio::sync::RwLock<
            Option<Arc<dyn rholang::rust::interpreter::io::wal::PayloadSourceRecorder>>,
        >,
    >,

    /// c-2 review-follow-up (2026-08-30): operator's consensus-static
    /// roots, used as defense-in-depth `allowed_roots` for the boot
    /// subscriber's `apply_wal_slice_after_fetch`.  A WAL entry whose
    /// target path does not sit under one of these roots (via
    /// `Path::starts_with`) is rejected by the applier — bounds the
    /// blast radius of a hypothetical leader-canonicalize bug or a
    /// forged snapshot.
    ///
    /// Populated at boot via `register_consensus_static_root` from
    /// each entry in `merged.consensus_static_files` (file paths)
    /// and `merged.consensus_static_dirs` (dir paths).  Both shapes
    /// work with `check_path_allowed`'s `starts_with` check: a file
    /// path matches exactly (its own path); a dir path matches any
    /// child under it.
    ///
    /// Empty by default; the applier skips validation when empty —
    /// preserves pre-plumbing behavior for observer nodes without
    /// consensus provisioning.
    pub consensus_static_roots: Arc<tokio::sync::RwLock<Vec<std::path::PathBuf>>>,
}

#[derive(Clone)]
pub struct StateBoundAdmission {
    pre_state: StateHash,
    block_data: BlockData,
    invalid_blocks: HashMap<BlockHash, Validator>,
    candidate_ids: Arc<[models::rust::deploy_id::DeployLookupId]>,
    outcome: crate::rust::util::rholang::acceptance::AdmissionOutcome,
    evidence: Arc<[ProcessedDeploy]>,
    user_post_state: StateHash,
    user_mergeable: Arc<[NumberChannelsEndVal]>,
    /// PB-M-14 fix (2026-08-28): the aggregated per-block WAL slice
    /// captured during user-deploy execution.  Threaded up from
    /// `state_bound_cost_evidence_for_state_cosigned` so
    /// `compute_state_with_bonds_cosigned_admitted` can insert into
    /// `pending_wal_slices` under the block's final post-state-hash
    /// after system deploys land.  Empty for blocks with no Consensus-
    /// cap fs writes (the common case).
    fs_wal: Arc<[rholang::rust::interpreter::io::wal::WalEntry]>,
}

#[derive(Clone)]
struct StateBoundExecution {
    post_state: StateHash,
    processed: Arc<[ProcessedDeploy]>,
    mergeable: Arc<[NumberChannelsEndVal]>,
    /// PB-M-14 fix (2026-08-28): mirror of `StateBoundAdmission.fs_wal`;
    /// captured from `state_bound_cost_evidence_for_state_cosigned` so
    /// the certifying wrapper can promote it into the admission
    /// token verbatim.
    fs_wal: Arc<[rholang::rust::interpreter::io::wal::WalEntry]>,
}

#[derive(Clone, Copy)]
enum UserExecutionOrigin {
    Proposal,
    Validation,
    Replay,
    Recovery,
}

impl UserExecutionOrigin {
    fn as_str(self) -> &'static str {
        match self {
            Self::Proposal => "proposal",
            Self::Validation => "validation",
            Self::Replay => "replay",
            Self::Recovery => "recovery",
        }
    }
}

fn ensure_terminal_close(
    system_deploys: &mut Vec<super::system_deploy_enum::SystemDeployEnum>,
    block_data: &BlockData,
) -> Result<(), CasperError> {
    let close_positions = system_deploys
        .iter()
        .enumerate()
        .filter_map(|(index, deploy)| deploy.as_close().map(|_| index))
        .collect::<Vec<_>>();
    match close_positions.as_slice() {
        [] => system_deploys.push(
            super::system_deploy_enum::SystemDeployEnum::Close(
                crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy::new(
                    crate::rust::util::rholang::system_deploy_util::generate_close_deploy_random_seed_from_pk(
                        block_data.sender.clone(),
                        block_data.seq_num,
                    ),
                ),
            ),
        ),
        [index] if *index + 1 == system_deploys.len() => {}
        _ => {
            return Err(CasperError::InvalidCostSettlement(
                "ordinary checkpoint must contain exactly one terminal close deploy".to_string(),
            ));
        }
    }
    Ok(())
}

impl StateBoundAdmission {
    pub fn pre_state(&self) -> &StateHash { &self.pre_state }

    pub fn candidate_ids(&self) -> &[models::rust::deploy_id::DeployLookupId] {
        &self.candidate_ids
    }

    pub fn outcome(&self) -> &crate::rust::util::rholang::acceptance::AdmissionOutcome {
        &self.outcome
    }

    pub fn matches_context(
        &self,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
    ) -> bool {
        self.block_data.time_stamp == block_data.time_stamp
            && self.block_data.block_number == block_data.block_number
            && self.block_data.sender == block_data.sender
            && self.block_data.seq_num == block_data.seq_num
            && &self.invalid_blocks == invalid_blocks
    }
}

fn validate_state_bound_admission_partition(
    candidate_ids: &[models::rust::deploy_id::DeployLookupId],
    admitted_ids: &[models::rust::deploy_id::DeployLookupId],
    rejected_ids: &[models::rust::deploy_id::DeployLookupId],
    deferred_ids: &[models::rust::deploy_id::DeployLookupId],
) -> Result<(), CasperError> {
    let candidate_set = candidate_ids.iter().cloned().collect::<BTreeSet<_>>();
    if candidate_set.len() != candidate_ids.len() {
        return Err(CasperError::InvalidCostSettlement(
            "state-bound admission candidate window contains a duplicate identity".to_string(),
        ));
    }
    let mut partition = BTreeSet::new();
    for id in admitted_ids
        .iter()
        .chain(rejected_ids.iter())
        .chain(deferred_ids.iter())
    {
        if !partition.insert(id.clone()) {
            return Err(CasperError::InvalidCostSettlement(
                "state-bound admission partition contains a duplicate identity".to_string(),
            ));
        }
    }
    if partition != candidate_set {
        return Err(CasperError::InvalidCostSettlement(
            "state-bound admission partition does not cover its candidate window".to_string(),
        ));
    }
    let admitted_set = admitted_ids.iter().cloned().collect::<BTreeSet<_>>();
    let rejected_set = rejected_ids.iter().cloned().collect::<BTreeSet<_>>();
    let deferred_set = deferred_ids.iter().cloned().collect::<BTreeSet<_>>();
    let canonical_admitted = candidate_ids
        .iter()
        .filter(|id| admitted_set.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let canonical_rejected = candidate_ids
        .iter()
        .filter(|id| rejected_set.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let canonical_deferred = candidate_ids
        .iter()
        .filter(|id| deferred_set.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    if admitted_ids != canonical_admitted
        || rejected_ids != canonical_rejected
        || deferred_ids != canonical_deferred
    {
        return Err(CasperError::InvalidCostSettlement(
            "state-bound admission partition does not preserve canonical candidate order"
                .to_string(),
        ));
    }
    Ok(())
}

fn is_canonical_admission_rejection(
    processed: &ProcessedDeploy,
    candidate: &Cosigned<DeployData>,
    pre_state: &StateHash,
) -> bool {
    processed == &ProcessedDeploy::admission_rejected(candidate, pre_state.clone())
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ParentsPostStateCacheKey {
    pub main_parent_hash: BlockHash,
    pub sorted_secondary_parent_hashes: Vec<BlockHash>,
    // Snapshot LFB participates in visible-ancestor filtering, so cache key must include it.
    pub snapshot_lfb_hash: BlockHash,
    // The finalized-floor merge base is derived from the block's frozen
    // justification snapshot (finality/floor.rs), so identical parent sets
    // under different justification maps can merge from different floors.
    // Sorted (validator, latest_block_hash) pairs keep such contexts from
    // sharing a cache entry.
    pub sorted_latest_messages: Vec<(Validator, BlockHash)>,
    pub disable_late_block_filtering: bool,
    // Whether the computation ran with a rejected-deploy buffer attached.
    // Buffer population is a side effect of the merge, not part of the
    // cached value — a bufferless computation (exploratory deploy) must
    // never satisfy a lookup from the create/validate path, or that
    // path's buffer populate is silently skipped.
    pub buffer_populated: bool,
}

impl ParentsPostStateCacheKey {
    pub fn new(
        main_parent_hash: BlockHash,
        mut secondary_parent_hashes: Vec<BlockHash>,
        snapshot_lfb_hash: BlockHash,
        mut latest_messages: Vec<(Validator, BlockHash)>,
        disable_late_block_filtering: bool,
        buffer_populated: bool,
    ) -> Self {
        secondary_parent_hashes.sort();
        latest_messages.sort();
        Self {
            main_parent_hash,
            sorted_secondary_parent_hashes: secondary_parent_hashes,
            snapshot_lfb_hash,
            sorted_latest_messages: latest_messages,
            disable_late_block_filtering,
            buffer_populated,
        }
    }
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
    pub rejected_state_effects: Vec<models::rust::casper::protocol::casper_message::StateEffectId>,
    pub applied_state_effects: Vec<models::rust::casper::protocol::casper_message::StateEffectId>,
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
    const MAX_BLOCK_INDEX_CACHE_ENTRIES: usize = 128;
    const MAX_BLOCK_INDEX_CACHE_BYTES: usize = 64 * 1024 * 1024;
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

    fn publish_replay_cache(
        &self,
        key: ReplayCacheKey,
        usr_processed: &[ProcessedDeploy],
        sys_processed: &[ProcessedSystemDeploy],
        state_hash: &StateHash,
    ) {
        let replay_cache_event_log_cap = Self::max_replay_cache_event_log_entries();
        if let Some(ref cache) = self.replay_cache {
            let all_logs = Self::collect_replay_logs(usr_processed, sys_processed);
            if !all_logs.is_empty() && all_logs.len() <= replay_cache_event_log_cap {
                let entry = ReplayCacheEntry::new(all_logs, state_hash.clone());
                cache.put(key, entry);
                Self::record_replay_cache_metrics(cache);
            }
        }
    }

    fn replay_payload_hash(
        usr_processed: &[ProcessedDeploy],
        sys_processed: &[ProcessedSystemDeploy],
        is_genesis: bool,
    ) -> Vec<u8> {
        fn push_len_prefixed(bytes: &mut Vec<u8>, data: &[u8]) {
            bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
            bytes.extend_from_slice(data);
        }

        fn canonicalize_event_log(event_log: &mut [Event]) {
            event_log.sort_by_cached_key(|event| event.to_proto().encode_to_vec());
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"f1r3node:replay-payload:v2");
        bytes.extend_from_slice(&(usr_processed.len() as u64).to_le_bytes());
        for pd in usr_processed {
            let mut canonical = pd.clone();
            canonicalize_event_log(&mut canonical.deploy_log);
            push_len_prefixed(&mut bytes, &canonical.to_proto().encode_to_vec());
        }
        bytes.extend_from_slice(&(sys_processed.len() as u64).to_le_bytes());
        for psd in sys_processed {
            let mut canonical = psd.clone();
            match &mut canonical {
                ProcessedSystemDeploy::Succeeded { event_list, .. }
                | ProcessedSystemDeploy::Failed { event_list, .. } => {
                    canonicalize_event_log(event_list);
                }
            }
            push_len_prefixed(&mut bytes, &canonical.to_proto().encode_to_vec());
        }
        bytes.push(u8::from(is_genesis));
        Blake2b256::hash(bytes)
    }

    fn max_block_index_cache_entries() -> usize { Self::MAX_BLOCK_INDEX_CACHE_ENTRIES }

    fn max_block_index_cache_bytes() -> usize { Self::MAX_BLOCK_INDEX_CACHE_BYTES }

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

    fn record_user_deploy_executions(origin: UserExecutionOrigin, count: usize) {
        metrics::counter!(
            USER_DEPLOY_EXECUTIONS_METRIC,
            "source" => CASPER_METRICS_SOURCE,
            "origin" => origin.as_str()
        )
        .increment(count as u64);
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
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        unsafe {
            unsafe extern "C" {
                fn malloc_trim(pad: usize) -> i32;
            }
            let _ = malloc_trim(0);
        }
    }

    fn touch_cache_key<K>(order: &Mutex<VecDeque<K>>, key: &K)
    where K: Eq + Clone {
        #[cfg(test)]
        cache_lock_tests::before_order_lock();
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

    fn evict_block_index_entry(&self) -> bool {
        let removed = self
            .block_index_cache_order
            .lock()
            .ok()
            .and_then(|mut order| {
                while let Some(key) = order.pop_front() {
                    if let Some((_, value)) = self.block_index_cache.remove(&key) {
                        return Some(value);
                    }
                }
                None
            });
        if let Some(value) = removed {
            let bytes = value.retained_bytes();
            let _ = self.block_index_cache_retained_bytes.fetch_update(
                Ordering::AcqRel,
                Ordering::Acquire,
                |current| Some(current.saturating_sub(bytes)),
            );
            true
        } else {
            false
        }
    }

    fn record_block_index_cache_metrics(&self) {
        metrics::gauge!(BLOCK_INDEX_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(self.block_index_cache.len() as f64);
        metrics::gauge!(BLOCK_INDEX_CACHE_RETAINED_BYTES_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(
                self.block_index_cache_retained_bytes
                    .load(Ordering::Acquire) as f64,
            );
    }

    /// H-7 note (2026-08-06): `fs_handles` (the `FileHandleTable`
    /// carrying WAL + open-fd map + next_fd counter) is
    /// INTENTIONALLY per-runtime.  Leader (play) and follower
    /// (replay) each get their own — a leader-follower pair
    /// operates on distinct backing RSpaces at potentially
    /// different block heights and MUST NOT share fd state.
    ///
    /// Cross-runtime WAL byte-identity is preserved through a
    /// different mechanism: `fs_open`'s `is_replay = true` branch
    /// extracts the leader's fd from the cached `previous` reply
    /// and calls `insert_at(fd, shadow_handle)` on the follower's
    /// own table (C-R1 slice-29 review fix in handle_table.rs).
    /// Subsequent mutating handlers on both sides look up
    /// `(cmode, canon_path)` via `with_mut(fd)` — identical
    /// lookups, identical WAL entries, byte-identical roots.
    ///
    /// What IS shared here (manager → all spawned runtimes):
    ///   - `fs_snapshot_writer` (Arc<RwLock<Option<...>>>) —
    ///     boot config; slice-30b.
    ///   - `pending_wal_slices` cache — LFB-triggered snapshot
    ///     writer input; H-1 slice-30c Phase B.
    ///   - `root_id_registry` — boot-captured (dev, inode) pairs
    ///     for rename-and-recreate detection; H-5.
    ///
    /// A regression that started sharing `fs_handles` here would
    /// silently corrupt cross-runtime fd allocation and violate
    /// the C-R1 shadow-handle invariant.  Pinned by
    /// `spawn_runtime_and_spawn_replay_yield_distinct_fs_handles`
    /// in the wiring tests below.
    pub async fn spawn_runtime(&self) -> RhoRuntimeImpl {
        let start = std::time::Instant::now();
        let new_space = self.space.spawn().expect("Failed to spawn RSpace");
        let mut runtime = rho_runtime::create_rho_runtime(
            new_space,
            self.mergeable_tags.clone(),
            true,
            &mut Vec::new(),
            self.external_services.clone(),
        )
        .await;
        // Slice 30b: attach the shared snapshot writer (if any) so
        // per-block WAL slices from `play_deploys_for_state` can be
        // persisted at the operator-configured cadence.
        // H-30b-2 round-2 fix: SHARE the Arc<RwLock<...>> so every
        // runtime spawned from this manager reads the same slot.
        // Boot-time `RuntimeManager::set_fs_snapshot_writer` is
        // immediately visible to every runtime — no cached-per-spawn
        // staleness.
        runtime.share_fs_snapshot_writer(self.fs_snapshot_writer.clone());
        // H-1 fix (2026-08-06) — slice 30c Phase B: share the
        // pending-WAL-slice cache so `play_deploys_for_state` on
        // this runtime writes into the manager's map, and the
        // finalization runner can read from the same map when the
        // LFB advances.
        runtime.share_pending_wal_slices(self.pending_wal_slices.clone());
        // H-5 fix (2026-08-06): share the root-identity registry
        // so every syscall handler on this runtime consults the
        // same boot-populated (dev, inode) map.
        runtime
            .fs_handles
            .share_root_registry(self.root_id_registry.clone());
        // Phase 8 slice 8a: share the range-lock registry so
        // cross-cap coordination on the same (dev, inode) is
        // visible to every runtime spawned from this manager.
        runtime
            .fs_handles
            .share_lock_registry(self.lock_registry.clone());
        // Phase 7b-2 (2026-08-27): share the payload persistence
        // backend so every Consensus-cap write stashes bytes into
        // a single on-disk directory the WalPayloadContext reads
        // back from.  A `None` slot (observer nodes without
        // consensus-static provisioning) becomes a no-op inside
        // `journal_write`.
        runtime.fs_handles.share_payload_store(
            self.payload_store
                .read()
                .await
                .as_ref()
                .map(|b| b.persistence.clone()),
        );
        // DD-7b-2 (a) Option 2 (2026-08-29): share the payload-
        // source recorder so every Consensus-cap `journal_write`
        // on this runtime records `payload_hash → deploy_sig`
        // into the manager-shared block-storage-backed index.
        // A `None` slot (observer nodes, tests) becomes a no-op
        // inside journal_write.
        runtime.fs_handles.share_payload_source_recorder(
            self.payload_source_recorder.read().await.as_ref().cloned(),
        );
        metrics::histogram!(RUNTIME_SPAWN_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());

        runtime
    }

    pub async fn spawn_replay_runtime(&self) -> RhoRuntimeImpl {
        let start = std::time::Instant::now();
        let new_replay_space = self
            .replay_space
            .spawn()
            .expect("Failed to spawn ReplayRSpace");

        let mut runtime = rho_runtime::create_replay_rho_runtime(
            new_replay_space,
            self.mergeable_tags.clone(),
            true,
            &mut Vec::new(),
            self.external_services.clone(),
        )
        .await;
        // Slice 30b: replay runtimes also get the snapshot writer;
        // they don't write snapshots themselves (that's the leader
        // side), but the writer is cheaply cloneable and keeping
        // parity avoids leader/follower divergence in the runtime
        // shape.
        // H-30b-2 round-2 fix: SHARE the Arc<RwLock<...>> so every
        // runtime spawned from this manager reads the same slot.
        // Boot-time `RuntimeManager::set_fs_snapshot_writer` is
        // immediately visible to every runtime — no cached-per-spawn
        // staleness.
        runtime.share_fs_snapshot_writer(self.fs_snapshot_writer.clone());
        // H-1 (2026-08-06): same rationale on replay side — keep the
        // shared cache attached even though replay runtimes don't
        // write to it in practice (leader-side play_deploys_for_state
        // is the writer).  Preserves runtime-shape parity between
        // leader and follower so any future symmetric use is
        // straightforward.
        runtime.share_pending_wal_slices(self.pending_wal_slices.clone());
        // H-5 fix (2026-08-06): share the root-identity registry
        // so every syscall handler on this runtime consults the
        // same boot-populated (dev, inode) map.
        runtime
            .fs_handles
            .share_root_registry(self.root_id_registry.clone());
        // Phase 8 slice 8a: share the range-lock registry so
        // cross-cap coordination on the same (dev, inode) is
        // visible to every runtime spawned from this manager.
        runtime
            .fs_handles
            .share_lock_registry(self.lock_registry.clone());
        // Phase 7b-2 (2026-08-27): share the payload persistence
        // backend so replay runtimes see the same shape as play
        // runtimes.  Replay runtimes don't write bytes themselves
        // in practice (leader-side journal_write does), but
        // keeping parity avoids leader/follower divergence.
        runtime.fs_handles.share_payload_store(
            self.payload_store
                .read()
                .await
                .as_ref()
                .map(|b| b.persistence.clone()),
        );
        // DD-7b-2 (a) Option 2 (2026-08-29): symmetric with the
        // play-side spawn — replay runtimes also share the
        // payload-source recorder so a follower's replay-branch
        // `journal_write` (through the WalDeployScope-plumbed
        // `current_deploy_sig`) records the same `payload_hash →
        // deploy_sig` mapping.  Keeps leader/follower symmetric so
        // any node whose block processing succeeded can serve the
        // Option 2 tier at boot to a later joiner.
        runtime.fs_handles.share_payload_source_recorder(
            self.payload_source_recorder.read().await.as_ref().cloned(),
        );
        metrics::histogram!(RUNTIME_SPAWN_REPLAY_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());

        runtime
    }

    /// Multi-signature-aware variant of [`Self::compute_state`].
    pub async fn compute_state_cosigned(
        &self,
        start_hash: &StateHash,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
    ) -> Result<(StateHash, Vec<ProcessedDeploy>, Vec<ProcessedSystemDeploy>), CasperError> {
        let (state_hash, user_deploys, system_deploys, _) = self
            .compute_state_with_bonds_cosigned(
                start_hash,
                terms,
                system_deploys,
                block_data,
                invalid_blocks,
            )
            .await?;
        Ok((state_hash, user_deploys, system_deploys))
    }

    pub async fn compute_state(
        &self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
    ) -> Result<(StateHash, Vec<ProcessedDeploy>, Vec<ProcessedSystemDeploy>), CasperError> {
        let cosigned = terms
            .into_iter()
            .map(crypto::rust::signatures::signed::Cosigned::from_single_signer)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        self.compute_state_cosigned(
            start_hash,
            cosigned,
            system_deploys,
            block_data,
            invalid_blocks,
        )
        .await
    }

    /// Multi-signature-aware variant of [`Self::compute_state_with_bonds`].
    /// Accepts `Vec<Cosigned<DeployData>>` so the complete authority envelope
    /// participates in reservation and realized-cost settlement. For legacy
    /// single-signature deploys (1-element Cosigned envelopes) behavior is
    /// byte-identical. Bonds computation is unaffected by the signature shape.
    pub async fn compute_state_with_bonds_cosigned(
        &self,
        start_hash: &StateHash,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
    ) -> Result<
        (
            StateHash,
            Vec<ProcessedDeploy>,
            Vec<ProcessedSystemDeploy>,
            Vec<Bond>,
        ),
        CasperError,
    > {
        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let admission = self
            .certify_state_bound_admission(start_hash, terms, &block_data, &invalid_blocks)
            .await?;
        if !admission.outcome.rejected.is_empty() || !admission.outcome.deferred.is_empty() {
            return Err(CasperError::InvalidCostSettlement(format!(
                "checkpoint received {} rejected and {} deferred deploys without executable state-bound funding evidence",
                admission.outcome.rejected.len(),
                admission.outcome.deferred.len()
            )));
        }
        self.compute_state_with_bonds_cosigned_admitted(admission, system_deploys)
            .await
    }

    pub async fn compute_state_with_bonds_cosigned_admitted(
        &self,
        admission: StateBoundAdmission,
        mut system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
    ) -> Result<
        (
            StateHash,
            Vec<ProcessedDeploy>,
            Vec<ProcessedSystemDeploy>,
            Vec<Bond>,
        ),
        CasperError,
    > {
        let StateBoundAdmission {
            pre_state: start_hash,
            block_data,
            invalid_blocks,
            candidate_ids: _,
            outcome,
            evidence,
            user_post_state,
            user_mergeable,
            fs_wal,
        } = admission;
        let block_number_for_slice = block_data.block_number;
        ensure_terminal_close(&mut system_deploys, &block_data)?;
        if evidence.len() != outcome.admitted.len() {
            return Err(CasperError::InvalidCostSettlement(
                "committed authority evidence count differs from the admitted deploy count"
                    .to_string(),
            ));
        }
        let runtime = self.spawn_runtime().await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;
        let replay_context = ReplayCacheContext::new(&block_data, &invalid_blocks);
        runtime_ops.runtime.set_block_data(block_data).await;
        runtime_ops.runtime.set_invalid_blocks(invalid_blocks).await;
        runtime_ops
            .runtime
            .reset(&Blake2b256Hash::from_bytes_prost(&user_post_state))
            .await?;
        let committed_user_state = user_post_state;
        let usr_processed = evidence.to_vec();
        let (state_hash, sys_deploy_res) = runtime_ops
            .play_system_deploys_for_state(&committed_user_state, system_deploys)
            .await?;

        let (sys_processed, sys_mergeable): (
            Vec<ProcessedSystemDeploy>,
            Vec<NumberChannelsEndVal>,
        ) = sys_deploy_res.into_iter().unzip();
        let mergeable_chs = user_mergeable
            .iter()
            .cloned()
            .chain(sys_mergeable.into_iter())
            .collect();
        let replay_payload_hash = Self::replay_payload_hash(&usr_processed, &sys_processed, false);
        let replay_cache_key = ReplayCacheKey::new(
            start_hash.clone(),
            replay_context,
            replay_payload_hash.clone(),
        );
        persist_before_publish(
            || {
                self.save_mergeable_channels(
                    &state_hash,
                    sender.bytes.clone(),
                    seq_num,
                    mergeable_chs,
                    &start_hash,
                    replay_payload_hash,
                )
            },
            || {
                self.publish_replay_cache(
                    replay_cache_key,
                    &usr_processed,
                    &sys_processed,
                    &state_hash,
                )
            },
        )?;
        // Reuse the same spawned runtime for bonds query (mirrors
        // compute_state_with_bonds).
        let bonds = runtime_ops.compute_bonds(&state_hash).await?;
        drop(runtime_ops);

        // PB-M-14 fix (2026-08-28): publish the aggregated per-block
        // WAL slice into `pending_wal_slices` under the block's final
        // post-state-hash.  The finalization runner's `new_lfb_found_
        // effect` looks up slices by `block.body.state.post_state_hash`
        // (see `finalization_runner.rs`), so the key MUST include
        // any system-deploy state effects — hence the insert happens
        // here (after system deploys land), not inside
        // `state_bound_cost_evidence_for_state_cosigned` (which only
        // knows the user_post_state).
        //
        // Mirrors the legacy `play_deploys_for_state` insert
        // (`runtime.rs:1515-1534`) shape verbatim — same eviction
        // policy, same tracing target.  Pre-fix this insert never
        // happened for cosigned blocks; the LFB-snapshot writer's
        // input starved and the joiner-reconstruction path was
        // unobservable end-to-end.
        if !fs_wal.is_empty() {
            const MAX_PENDING_WAL_SLICES: usize = 1024;
            let mut slices = self.pending_wal_slices.write().await;
            if slices.len() >= MAX_PENDING_WAL_SLICES {
                if let Some(oldest_key) = slices
                    .iter()
                    .min_by_key(|(_, (bn, _))| *bn)
                    .map(|(k, _)| k.clone())
                {
                    slices.remove(&oldest_key);
                    tracing::warn!(
                        target: "f1r3fly.casper.fs_wal",
                        cap = MAX_PENDING_WAL_SLICES,
                        "pending_wal_slices cache full; evicting oldest entry.  \
                         Deep-fork scenario or stalled finalizer?"
                    );
                }
            }
            slices.insert(
                state_hash.to_vec(),
                (block_number_for_slice, fs_wal.to_vec()),
            );
        }

        Ok((state_hash, usr_processed, sys_processed, bonds))
    }

    pub async fn state_bound_cost_evidence(
        &self,
        start_hash: &StateHash,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_data: BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
    ) -> Result<
        (
            Vec<ProcessedDeploy>,
            Vec<models::rust::deploy_id::DeployLookupId>,
            Vec<models::rust::deploy_id::DeployLookupId>,
        ),
        CasperError,
    > {
        let (execution, outcome) = self
            .state_bound_execution(start_hash, terms, block_data, invalid_blocks)
            .await?;
        Ok((
            execution.processed.to_vec(),
            outcome.rejected,
            outcome.deferred,
        ))
    }

    pub async fn state_bound_cost_evidence_with_host_work(
        &self,
        start_hash: &StateHash,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_data: BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
        host_work_limits: HostWorkLimits,
    ) -> Result<
        (
            Vec<ProcessedDeploy>,
            Vec<models::rust::deploy_id::DeployLookupId>,
            Vec<models::rust::deploy_id::DeployLookupId>,
        ),
        CasperError,
    > {
        let (execution, outcome) = self
            .state_bound_execution_internal(
                start_hash,
                terms,
                block_data,
                invalid_blocks,
                Some(host_work_limits),
                UserExecutionOrigin::Proposal,
            )
            .await?;
        Ok((
            execution.processed.to_vec(),
            outcome.rejected,
            outcome.deferred,
        ))
    }

    async fn state_bound_execution(
        &self,
        start_hash: &StateHash,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_data: BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
    ) -> Result<
        (
            StateBoundExecution,
            crate::rust::util::rholang::acceptance::AdmissionOutcome,
        ),
        CasperError,
    > {
        self.state_bound_execution_internal(
            start_hash,
            terms,
            block_data,
            invalid_blocks,
            None,
            UserExecutionOrigin::Proposal,
        )
        .await
    }

    async fn state_bound_execution_internal(
        &self,
        start_hash: &StateHash,
        mut terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_data: BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
        host_work_limits: Option<HostWorkLimits>,
        execution_origin: UserExecutionOrigin,
    ) -> Result<
        (
            StateBoundExecution,
            crate::rust::util::rholang::acceptance::AdmissionOutcome,
        ),
        CasperError,
    > {
        crate::rust::util::rholang::acceptance::canonical_sort(&mut terms);
        let runtime = self.spawn_runtime().await;
        let mut runtime_ops =
            RuntimeOps::new(runtime).with_user_execution_origin(execution_origin.as_str());
        let fee_recipient = block_data.sender.clone();
        runtime_ops.runtime.set_block_data(block_data).await;
        runtime_ops.runtime.set_invalid_blocks(invalid_blocks).await;
        let (post_state, processed, outcome, fs_wal) = match host_work_limits {
            Some(host_work_limits) => {
                runtime_ops
                    .state_bound_cost_evidence_for_state_cosigned_with_host_work(
                        start_hash,
                        terms,
                        &fee_recipient,
                        host_work_limits,
                    )
                    .await?
            }
            None => {
                runtime_ops
                    .state_bound_cost_evidence_for_state_cosigned(start_hash, terms, &fee_recipient)
                    .await?
            }
        };
        Self::record_user_deploy_executions(execution_origin, processed.len());
        let (processed, mergeable): (Vec<_>, Vec<_>) = processed.into_iter().unzip();
        Ok((
            StateBoundExecution {
                post_state,
                processed: Arc::from(processed),
                mergeable: Arc::from(mergeable),
                fs_wal: Arc::from(fs_wal),
            },
            outcome,
        ))
    }

    pub async fn admit_with_state_bound_evidence(
        &self,
        pre_state: &StateHash,
        candidates: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
    ) -> Result<crate::rust::util::rholang::acceptance::AdmissionOutcome, CasperError> {
        self.admit_with_state_bound_evidence_and_witness(
            pre_state,
            candidates,
            block_data,
            invalid_blocks,
            UserExecutionOrigin::Proposal,
        )
        .await
        .map(|(outcome, _)| outcome)
    }

    async fn admit_with_state_bound_evidence_and_witness(
        &self,
        pre_state: &StateHash,
        mut candidates: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
        execution_origin: UserExecutionOrigin,
    ) -> Result<
        (
            crate::rust::util::rholang::acceptance::AdmissionOutcome,
            StateBoundExecution,
        ),
        CasperError,
    > {
        crate::rust::util::rholang::acceptance::canonical_sort(&mut candidates);
        let (execution, outcome) = self
            .state_bound_execution_internal(
                pre_state,
                candidates,
                block_data.clone(),
                invalid_blocks.clone(),
                None,
                execution_origin,
            )
            .await?;
        Ok((outcome, execution))
    }

    pub async fn certify_state_bound_admission(
        &self,
        pre_state: &StateHash,
        candidates: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
    ) -> Result<StateBoundAdmission, CasperError> {
        self.certify_state_bound_admission_for_origin(
            pre_state,
            candidates,
            block_data,
            invalid_blocks,
            UserExecutionOrigin::Proposal,
        )
        .await
    }

    async fn certify_state_bound_admission_for_origin(
        &self,
        pre_state: &StateHash,
        mut candidates: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
        execution_origin: UserExecutionOrigin,
    ) -> Result<StateBoundAdmission, CasperError> {
        crate::rust::util::rholang::acceptance::canonical_sort(&mut candidates);
        let candidate_ids = candidates
            .iter()
            .map(crate::rust::util::rholang::acceptance::admission_deploy_id)
            .collect::<Vec<_>>();
        let (outcome, execution) = self
            .admit_with_state_bound_evidence_and_witness(
                pre_state,
                candidates,
                block_data,
                invalid_blocks,
                execution_origin,
            )
            .await?;
        let admitted_ids = outcome
            .admitted
            .iter()
            .map(crate::rust::util::rholang::acceptance::admission_deploy_id)
            .collect::<Vec<_>>();
        validate_state_bound_admission_partition(
            &candidate_ids,
            &admitted_ids,
            &outcome.rejected,
            &outcome.deferred,
        )?;
        Ok(StateBoundAdmission {
            pre_state: pre_state.clone(),
            block_data: block_data.clone(),
            invalid_blocks: invalid_blocks.clone(),
            candidate_ids: Arc::from(candidate_ids),
            outcome,
            evidence: execution.processed,
            user_post_state: execution.post_state,
            user_mergeable: execution.mergeable,
            fs_wal: execution.fs_wal,
        })
    }

    pub async fn compute_state_with_bonds(
        &self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
    ) -> Result<
        (
            StateHash,
            Vec<ProcessedDeploy>,
            Vec<ProcessedSystemDeploy>,
            Vec<Bond>,
        ),
        CasperError,
    > {
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }
        let cosigned = terms
            .into_iter()
            .map(crypto::rust::signatures::signed::Cosigned::from_single_signer)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let result = self
            .compute_state_with_bonds_cosigned(
                start_hash,
                cosigned,
                system_deploys,
                block_data,
                invalid_blocks,
            )
            .await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_compute_state", rss_kb);
        }

        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "complete", rss_kb);
        }
        Ok(result)
    }

    pub async fn compute_genesis(
        &self,
        terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_time: i64,
        block_number: i64,
    ) -> Result<(StateHash, StateHash, Vec<ProcessedDeploy>), CasperError> {
        let runtime = self.spawn_runtime().await;
        let mut runtime_ops = RuntimeOps::new(runtime);

        let (pre_state, state_hash, processed) = runtime_ops
            .compute_genesis(terms, block_time, block_number)
            .await?;
        let (processed_deploys, mergeable_chs): (Vec<ProcessedDeploy>, Vec<NumberChannelsEndVal>) =
            processed.into_iter().unzip();

        // Convert from final to diff values and persist mergeable (number) channels for post-state hash
        let replay_payload_hash = Self::replay_payload_hash(&processed_deploys, &[], true);

        // Save mergeable channels to store
        self.save_mergeable_channels(
            &state_hash,
            prost::bytes::Bytes::new(),
            0,
            mergeable_chs,
            &pre_state,
            replay_payload_hash,
        )?;

        Ok((pre_state, state_hash, processed_deploys))
    }

    async fn replay_compute_state_uncommitted(
        &self,
        start_hash: &StateHash,
        terms: Vec<ProcessedDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool,
    ) -> Result<(StateHash, Option<Vec<NumberChannelsEndVal>>), CasperError> {
        self.replay_compute_state_uncommitted_internal(
            start_hash,
            terms,
            system_deploys,
            block_data,
            invalid_blocks,
            is_genesis,
            None,
            true,
            UserExecutionOrigin::Replay,
        )
        .await
    }

    async fn replay_compute_state_uncommitted_internal(
        &self,
        start_hash: &StateHash,
        terms: Vec<ProcessedDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool,
        host_work_limits: Option<HostWorkLimits>,
        allow_replay_cache: bool,
        execution_origin: UserExecutionOrigin,
    ) -> Result<(StateHash, Option<Vec<NumberChannelsEndVal>>), CasperError> {
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;
        let replay_payload_hash = Self::replay_payload_hash(&terms, &system_deploys, is_genesis);
        let invalid_blocks = invalid_blocks.unwrap_or_default();

        let replay_cache_key = ReplayCacheKey::new(
            start_hash.clone(),
            ReplayCacheContext::new(block_data, &invalid_blocks),
            replay_payload_hash.clone(),
        );
        if allow_replay_cache && host_work_limits.is_none() {
            if let Some(ref cache) = self.replay_cache {
                if let Some(entry) = cache.get(&replay_cache_key) {
                    let mergeable_key = Self::mergeable_key_for_execution(
                        start_hash,
                        &entry.post_state,
                        sender.bytes.clone(),
                        seq_num,
                        replay_payload_hash.clone(),
                    );
                    let mergeable_key_encoded =
                        bincode::serialize(&mergeable_key).map_err(|e| {
                            CasperError::KvStoreError(KvStoreError::SerializationError(
                                e.to_string(),
                            ))
                        })?;
                    if self.mergeable_store.contains_key(mergeable_key_encoded)? {
                        tracing::info!("[CACHE] ReplayCache hit for sender seq={}", seq_num);
                        return Ok((entry.post_state, None));
                    }
                    tracing::warn!(
                    "[CACHE] ReplayCache hit without mergeable entry for seq={}; falling back to full replay",
                    seq_num
                );
                }
            }
        }

        let _replay_permit = self
            .replay_lock
            .acquire_consensus()
            .await
            .map_err(|error| CasperError::Other(format!("Replay semaphore closed: {}", error)))?;
        let replay_runtime = self.spawn_replay_runtime().await;
        let runtime_ops =
            RuntimeOps::new(replay_runtime).with_user_execution_origin(execution_origin.as_str());
        let mut replay_runtime_ops = ReplayRuntimeOps::new(runtime_ops);

        let executed_count = terms.len();
        let (state_hash, mergeable_chs) = match host_work_limits {
            Some(host_work_limits) => {
                replay_runtime_ops
                    .replay_compute_state_with_host_work(
                        start_hash,
                        terms,
                        system_deploys,
                        block_data,
                        Some(invalid_blocks),
                        is_genesis,
                        Some(self),
                        host_work_limits,
                    )
                    .await?
            }
            None => {
                replay_runtime_ops
                    .replay_compute_state(
                        start_hash,
                        terms,
                        system_deploys,
                        block_data,
                        Some(invalid_blocks),
                        is_genesis,
                        Some(self),
                    )
                    .await?
            }
        };
        Self::record_user_deploy_executions(execution_origin, executed_count);

        let post_state = state_hash.to_bytes_prost();
        Ok((post_state, Some(mergeable_chs)))
    }

    pub async fn replay_compute_state(
        &self,
        start_hash: &StateHash,
        terms: Vec<ProcessedDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool,
    ) -> Result<StateHash, CasperError> {
        self.replay_compute_state_uncommitted(
            start_hash,
            terms,
            system_deploys,
            block_data,
            invalid_blocks,
            is_genesis,
        )
        .await
        .map(|(post_state, _)| post_state)
    }

    pub async fn replay_compute_state_with_host_work(
        &self,
        start_hash: &StateHash,
        terms: Vec<ProcessedDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool,
        host_work_limits: HostWorkLimits,
    ) -> Result<StateHash, CasperError> {
        self.replay_compute_state_uncommitted_internal(
            start_hash,
            terms,
            system_deploys,
            block_data,
            invalid_blocks,
            is_genesis,
            Some(host_work_limits),
            false,
            UserExecutionOrigin::Replay,
        )
        .await
        .map(|(post_state, _)| post_state)
    }

    pub async fn replay_block_from_consensus_data(
        &self,
        start_hash: &StateHash,
        block: &BlockMessage,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
    ) -> Result<StateHash, CasperError> {
        self.replay_block_from_consensus_data_for_origin(
            start_hash,
            block,
            invalid_blocks,
            UserExecutionOrigin::Validation,
        )
        .await
    }

    async fn replay_block_from_consensus_data_for_origin(
        &self,
        start_hash: &StateHash,
        block: &BlockMessage,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        execution_origin: UserExecutionOrigin,
    ) -> Result<StateHash, CasperError> {
        let is_genesis = block.header.parents_hash_list.is_empty();
        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let block_data = BlockData::from_block(block);
        let (deploys, replay_start_hash, user_mergeable, follower_fs_wal) = if is_genesis {
            if block
                .body
                .deploys
                .iter()
                .any(ProcessedDeploy::is_admission_rejected)
            {
                return Err(CasperError::ReplayFailure(
                    ReplayFailure::replay_admission_mismatch(
                        block.body.deploys.len(),
                        0,
                        0,
                        block.body.deploys.len(),
                        "genesis cannot contain funding-admission rejection records".to_string(),
                    ),
                ));
            }
            // Genesis path: user deploys are replayed below (via
            // `replay_user_deploys`), which populates `pending_wal_
            // slices` through the H-1 publish in
            // `replay_deploys_internal`.  No separate follower-side
            // publish needed here.
            (
                block.body.deploys.clone(),
                start_hash.clone(),
                Vec::new(),
                Vec::<rholang::rust::interpreter::io::wal::WalEntry>::new(),
            )
        } else {
            let admission = self
                .verify_state_bound_admission_partition(
                    start_hash,
                    &block.body.deploys,
                    &block_data,
                    &invalid_blocks,
                    block.header.version,
                    execution_origin,
                )
                .await?;
            // S4.2 fix (2026-09-10): capture the state-bound
            // recompute's fs_wal so the follower can publish it into
            // its own `pending_wal_slices` after post-state
            // validation.  Pre-fix, user deploys weren't re-run on the
            // follower's replay path (see `replay_user_deploys =
            // Vec::new()` below — an optimization since state_bound
            // already ran them), so the H-1 publish in
            // `replay_deploys_internal` never fired for the follower
            // and its `pending_wal_slices` starved of entries.
            (
                admission.evidence.to_vec(),
                admission.user_post_state,
                admission.user_mergeable.to_vec(),
                admission.fs_wal.to_vec(),
            )
        };

        let replay_payload_hash =
            Self::replay_payload_hash(&deploys, &block.body.system_deploys, is_genesis);
        let replay_cache_key = ReplayCacheKey::new(
            start_hash.clone(),
            ReplayCacheContext::new(&block_data, &invalid_blocks),
            replay_payload_hash.clone(),
        );
        let replay_user_deploys = if is_genesis {
            deploys.clone()
        } else {
            Vec::new()
        };
        let (computed_post_state, mergeable_chs) = self
            .replay_compute_state_uncommitted_internal(
                &replay_start_hash,
                replay_user_deploys,
                block.body.system_deploys.clone(),
                &block_data,
                Some(invalid_blocks),
                is_genesis,
                None,
                is_genesis,
                execution_origin,
            )
            .await?;
        Self::validate_replayed_post_state(
            &block.block_hash,
            &block.body.state.post_state_hash,
            &computed_post_state,
        )?;
        // S4.2 fix (2026-09-10): publish the follower's aggregated
        // per-block fs_wal into `pending_wal_slices` under the
        // block's final post-state-hash.  Mirrors the leader-side
        // publish in `compute_state_with_bonds_cosigned_admitted`
        // (line 1233 above), which uses the same key shape and
        // eviction policy.  Only fires on non-genesis blocks where
        // `verify_state_bound_admission_partition` captured a
        // non-empty fs_wal from user-deploy execution — genesis is
        // handled by the H-1 publish inside `replay_deploys_
        // internal` since user deploys are replayed there.
        if !follower_fs_wal.is_empty() {
            const MAX_PENDING_WAL_SLICES: usize = 1024;
            let mut slices = self.pending_wal_slices.write().await;
            if slices.len() >= MAX_PENDING_WAL_SLICES {
                if let Some(oldest_key) = slices
                    .iter()
                    .min_by_key(|(_, (bn, _))| *bn)
                    .map(|(k, _)| k.clone())
                {
                    slices.remove(&oldest_key);
                    tracing::warn!(
                        target: "f1r3fly.casper.fs_wal",
                        cap = MAX_PENDING_WAL_SLICES,
                        "follower pending_wal_slices cache full; evicting oldest entry.  \
                         Deep-fork scenario or stalled finalizer?"
                    );
                }
            }
            slices.insert(
                block.body.state.post_state_hash.to_vec(),
                (block.body.state.block_number, follower_fs_wal),
            );
        }
        let publish = || {
            self.publish_replay_cache(
                replay_cache_key,
                &deploys,
                &block.body.system_deploys,
                &computed_post_state,
            )
        };
        if let Some(mergeable_chs) = mergeable_chs {
            let mergeable_chs = user_mergeable.into_iter().chain(mergeable_chs).collect();
            persist_before_publish(
                || {
                    self.save_mergeable_channels(
                        &computed_post_state,
                        block_data.sender.bytes.clone(),
                        block_data.seq_num,
                        mergeable_chs,
                        start_hash,
                        replay_payload_hash,
                    )
                },
                publish,
            )
            .map_err(|error| {
                CasperError::RuntimeError(format!("Failed to save mergeable channels: {error:?}"))
            })?;
        } else {
            publish();
        }
        Ok(computed_post_state)
    }

    fn validate_replayed_post_state(
        block_hash: &BlockHash,
        declared_post_state: &StateHash,
        computed_post_state: &StateHash,
    ) -> Result<(), CasperError> {
        if computed_post_state == declared_post_state {
            return Ok(());
        }
        Err(CasperError::ReplayFailure(
            ReplayFailure::effect_state_mismatch(
                format!("block:{}", hex::encode(block_hash)),
                "final-post-state".to_string(),
                hex::encode(declared_post_state),
                hex::encode(computed_post_state),
            ),
        ))
    }

    async fn verify_state_bound_admission_partition(
        &self,
        start_hash: &StateHash,
        deploys: &[ProcessedDeploy],
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
        protocol_version: i64,
        execution_origin: UserExecutionOrigin,
    ) -> Result<StateBoundAdmission, CasperError> {
        let expected_admitted: Vec<ProcessedDeploy> = deploys
            .iter()
            .filter(|deploy| !deploy.is_admission_rejected())
            .cloned()
            .collect();
        let expected_rejected: Vec<&ProcessedDeploy> = deploys
            .iter()
            .filter(|deploy| deploy.is_admission_rejected())
            .collect();
        let mut candidates = Vec::with_capacity(deploys.len());
        for deploy in deploys {
            candidates.push(deploy.to_cosigned().map_err(|detail| {
                CasperError::ReplayFailure(ReplayFailure::replay_admission_mismatch(
                    expected_admitted.len(),
                    0,
                    expected_rejected.len(),
                    0,
                    detail,
                ))
            })?);
        }
        for deploy in &expected_rejected {
            let candidate = deploy.to_cosigned().map_err(|detail| {
                CasperError::ReplayFailure(ReplayFailure::replay_admission_mismatch(
                    expected_admitted.len(),
                    0,
                    expected_rejected.len(),
                    0,
                    detail,
                ))
            })?;
            if !is_canonical_admission_rejection(deploy, &candidate, start_hash) {
                return Err(CasperError::ReplayFailure(
                    ReplayFailure::replay_admission_mismatch(
                        expected_admitted.len(),
                        0,
                        expected_rejected.len(),
                        0,
                        format!(
                            "funding-admission rejection record differs from its canonical encoding for deploy {}",
                            hex::encode(deploy.deploy_id())
                        ),
                    ),
                ));
            }
        }
        let replay = self
            .certify_state_bound_admission_for_origin(
                start_hash,
                candidates,
                block_data,
                invalid_blocks,
                execution_origin,
            )
            .await
            .map_err(|error| {
                CasperError::ReplayFailure(ReplayFailure::replay_admission_mismatch(
                    expected_admitted.len(),
                    0,
                    expected_rejected.len(),
                    0,
                    error.to_string(),
                ))
            })?;
        let replay_admitted: Vec<_> = replay
            .outcome()
            .admitted
            .iter()
            .map(|deploy| {
                if protocol_version >= 6 {
                    deploy
                        .envelope_commitment()
                        .map_err(|error| {
                            CasperError::ReplayFailure(ReplayFailure::replay_admission_mismatch(
                                expected_admitted.len(),
                                0,
                                expected_rejected.len(),
                                0,
                                error.to_string(),
                            ))
                        })
                        .and_then(|bytes| {
                            models::rust::deploy_id::DeployIdV6::try_from(bytes.as_ref())
                                .map(models::rust::deploy_id::DeployLookupId::V6)
                                .map_err(|error| {
                                    CasperError::ReplayFailure(
                                        ReplayFailure::replay_admission_mismatch(
                                            expected_admitted.len(),
                                            0,
                                            expected_rejected.len(),
                                            0,
                                            error.to_string(),
                                        ),
                                    )
                                })
                        })
                } else {
                    Ok(models::rust::deploy_id::DeployLookupId::Legacy(
                        models::rust::deploy_id::LegacyDeploySignature::new(
                            deploy.primary().sig.to_vec(),
                        ),
                    ))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let expected_admitted_sigs: Vec<_> = expected_admitted
            .iter()
            .map(|deploy| deploy.deploy_id_for_protocol(protocol_version))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                CasperError::ReplayFailure(ReplayFailure::replay_admission_mismatch(
                    expected_admitted.len(),
                    0,
                    expected_rejected.len(),
                    0,
                    error,
                ))
            })?;
        let replay_rejected = replay.outcome().rejected.clone();
        let expected_rejected_sigs: Vec<_> = expected_rejected
            .iter()
            .map(|deploy| deploy.deploy_id_for_protocol(protocol_version))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                CasperError::ReplayFailure(ReplayFailure::replay_admission_mismatch(
                    expected_admitted.len(),
                    0,
                    expected_rejected.len(),
                    0,
                    error,
                ))
            })?;
        if replay_admitted != expected_admitted_sigs || replay_rejected != expected_rejected_sigs {
            return Err(CasperError::ReplayFailure(
                ReplayFailure::replay_admission_mismatch(
                    expected_admitted.len(),
                    replay_admitted.len(),
                    expected_rejected.len(),
                    replay_rejected.len(),
                    "block funding-admission partition differs from state-bound recomputation"
                        .to_string(),
                ),
            ));
        }
        if replay.evidence.as_ref() != expected_admitted.as_slice() {
            let mismatch_index = replay
                .evidence
                .iter()
                .zip(&expected_admitted)
                .position(|(observed, expected)| observed != expected)
                .unwrap_or_else(|| replay.evidence.len().min(expected_admitted.len()));
            return Err(CasperError::ReplayFailure(
                ReplayFailure::replay_admission_mismatch(
                    expected_admitted.len(),
                    replay.evidence.len(),
                    expected_rejected.len(),
                    replay_rejected.len(),
                    format!(
                        "block execution evidence differs from state-bound recomputation at admitted index {mismatch_index}"
                    ),
                ),
            ));
        }
        Ok(replay)
    }

    pub async fn capture_results(
        &self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
    ) -> Result<Vec<Par>, CasperError> {
        let runtime = self.spawn_runtime().await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.capture_results(start, deploy).await?;
        Ok(computed)
    }

    pub async fn capture_results_cosigned(
        &self,
        start: &StateHash,
        deploy: &Cosigned<DeployData>,
    ) -> Result<Vec<Par>, CasperError> {
        let runtime = self.spawn_runtime().await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.capture_results_cosigned(start, deploy).await?;
        Ok(computed)
    }

    pub async fn get_active_validators(
        &self,
        start_hash: &StateHash,
    ) -> Result<Vec<Validator>, CasperError> {
        if let Some(entry) = self.active_validators_cache.get(start_hash) {
            let cached = entry.value().clone();
            drop(entry);
            Self::touch_cache_key(&self.active_validators_cache_order, start_hash);
            return Ok(cached);
        }

        let runtime = self.spawn_runtime().await;
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
        let runtime = self.spawn_runtime().await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        runtime_ops
            .get_fault_tolerance_threshold_ppm(start_hash)
            .await
    }

    pub async fn compute_bonds(&self, hash: &StateHash) -> Result<Vec<Bond>, CasperError> {
        if let Some(entry) = self.bonds_cache.get(hash) {
            let cached = entry.value().clone();
            drop(entry);
            Self::touch_cache_key(&self.bonds_cache_order, hash);
            return Ok(cached);
        }

        let runtime = self.spawn_runtime().await;
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

    pub async fn compute_bond_generations(
        &self,
        hash: &StateHash,
    ) -> Result<HashMap<Validator, i64>, CasperError> {
        if let Some(entry) = self.bond_generations_cache.get(hash) {
            let cached = entry.value().clone();
            drop(entry);
            Self::touch_cache_key(&self.bond_generations_cache_order, hash);
            return Ok(cached);
        }

        let runtime = self.spawn_runtime().await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.compute_bond_generations(hash).await?;

        let max_entries = Self::max_bonds_cache_entries();
        if self.bond_generations_cache.len() >= max_entries {
            Self::evict_fifo_entry(
                &self.bond_generations_cache,
                &self.bond_generations_cache_order,
            );
        }
        self.bond_generations_cache
            .insert(hash.clone(), computed.clone());
        Self::touch_cache_key(&self.bond_generations_cache_order, hash);

        Ok(computed)
    }

    // Executes deploy as user deploy with immediate rollback
    pub async fn play_exploratory_deploy(
        &self,
        term: String,
        hash: &StateHash,
        deployer: Option<PublicKey>,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        let runtime = self.spawn_runtime().await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        runtime_ops
            .play_exploratory_deploy_with_phlo_limit(
                term,
                hash,
                deployer,
                self.exploratory_deploy_phlo_limit,
            )
            .await
    }

    pub async fn play_exploratory_deploy_at_protocol(
        &self,
        term: String,
        hash: &StateHash,
        deployer: Option<PublicKey>,
        protocol_version: i64,
        shard_id: String,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        let runtime = self.spawn_runtime().await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        if protocol_version >= crate::rust::casper::CERTIFIED_FINALIZED_FLOOR_PROTOCOL_VERSION {
            runtime_ops
                .play_exploratory_deploy_v61_with_phlo_limit(
                    term,
                    hash,
                    deployer,
                    shard_id,
                    self.exploratory_deploy_phlo_limit,
                )
                .await
        } else {
            runtime_ops
                .play_exploratory_deploy_with_phlo_limit(
                    term,
                    hash,
                    deployer,
                    self.exploratory_deploy_phlo_limit,
                )
                .await
        }
    }

    pub async fn play_query_par_at_state_strict(
        &self,
        par: Par,
        hash: &StateHash,
    ) -> Result<Vec<Par>, CasperError> {
        let mut runtime = self.spawn_runtime().await;
        runtime
            .reset(&Blake2b256Hash::from_bytes_prost(hash))
            .await?;
        RuntimeOps::new(runtime)
            .play_query_par_current_strict(par)
            .await
    }

    pub async fn get_data(&self, hash: StateHash, channel: &Par) -> Result<Vec<Par>, CasperError> {
        let mut runtime = self.spawn_runtime().await;

        runtime
            .reset(&Blake2b256Hash::from_bytes_prost(&hash))
            .await?;

        let runtime_ops = RuntimeOps::new(runtime);
        let computed = runtime_ops.get_data_par(channel).await;
        Ok(computed)
    }

    pub async fn get_data_datums(
        &self,
        hash: StateHash,
        channel: &Par,
    ) -> Result<
        Vec<rspace_plus_plus::rspace::internal::Datum<models::rhoapi::ListParWithRandom>>,
        CasperError,
    > {
        let mut runtime = self.spawn_runtime().await;
        runtime
            .reset(&Blake2b256Hash::from_bytes_prost(&hash))
            .await?;
        Ok(RuntimeOps::new(runtime).get_data_datums(channel).await)
    }

    pub async fn get_continuation(
        &self,
        hash: StateHash,
        channels: Vec<Par>,
    ) -> Result<Vec<(Vec<BindPattern>, Par)>, CasperError> {
        let mut runtime = self.spawn_runtime().await;

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
    pub fn get_or_compute_block_index(
        &self,
        block_hash: &BlockHash,
        block_number: i64,
        usr_processed_deploys: &Vec<ProcessedDeploy>,
        sys_processed_deploys: &Vec<ProcessedSystemDeploy>,
        pre_state_hash: &Blake2b256Hash,
        post_state_hash: &Blake2b256Hash,
        mergeable_chs: &Vec<NumberChannelsDiff>,
    ) -> Result<BlockIndex, CasperError> {
        if let Some(entry) = self.block_index_cache.get(block_hash) {
            let cached = entry.value().clone();
            drop(entry);
            Self::touch_cache_key(&self.block_index_cache_order, block_hash);
            self.record_block_index_cache_metrics();
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
        let retained_bytes = block_index.retained_bytes();
        let max_entries = Self::max_block_index_cache_entries();
        let max_bytes = Self::max_block_index_cache_bytes();
        #[cfg(test)]
        cache_lock_tests::before_index_recheck();
        let _write_guard = self
            .block_index_cache_write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(entry) = self.block_index_cache.get(block_hash) {
            let cached = entry.value().clone();
            drop(entry);
            Self::touch_cache_key(&self.block_index_cache_order, block_hash);
            self.record_block_index_cache_metrics();
            return Ok(cached);
        }

        while self.block_index_cache.len() >= max_entries
            || self
                .block_index_cache_retained_bytes
                .load(Ordering::Acquire)
                .saturating_add(retained_bytes)
                > max_bytes
        {
            if !self.evict_block_index_entry() {
                break;
            }
        }

        if retained_bytes <= max_bytes {
            if let Some(previous) = self
                .block_index_cache
                .insert(block_hash.clone(), block_index.clone())
            {
                let previous_bytes = previous.retained_bytes();
                let _ = self.block_index_cache_retained_bytes.fetch_update(
                    Ordering::AcqRel,
                    Ordering::Acquire,
                    |current| Some(current.saturating_sub(previous_bytes)),
                );
            }
            self.block_index_cache_retained_bytes
                .fetch_add(retained_bytes, Ordering::AcqRel);
            Self::touch_cache_key(&self.block_index_cache_order, block_hash);
        }
        self.record_block_index_cache_metrics();
        Ok(block_index)
    }

    pub fn has_cached_block_index(&self, block_hash: &BlockHash) -> bool {
        self.block_index_cache.contains_key(block_hash)
    }

    /// Remove BlockIndex from cache (used during finalization)
    pub fn remove_block_index_cache(&self, block_hash: &BlockHash) {
        let _write_guard = self
            .block_index_cache_write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, removed)) = self.block_index_cache.remove(block_hash) {
            let bytes = removed.retained_bytes();
            let _ = self.block_index_cache_retained_bytes.fetch_update(
                Ordering::AcqRel,
                Ordering::Acquire,
                |current| Some(current.saturating_sub(bytes)),
            );
        }
        self.record_block_index_cache_metrics();
    }

    pub fn clear_block_index_cache(&self) {
        let _write_guard = self
            .block_index_cache_write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.block_index_cache.clear();
        self.block_index_cache_order
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.block_index_cache_retained_bytes
            .store(0, Ordering::Release);
        self.record_block_index_cache_metrics();
    }

    pub fn get_cached_parents_post_state(
        &self,
        key: &ParentsPostStateCacheKey,
    ) -> Option<ParentsPostStateCacheVal> {
        let result = self
            .parents_post_state_cache
            .get(key)
            .map(|entry| entry.value().clone());
        if result.is_some() {
            Self::touch_cache_key(&self.parents_post_state_cache_order, key);
        }
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

    fn mergeable_key_for_execution(
        pre_state_hash: &StateHash,
        post_state_hash: &StateHash,
        creator: prost::bytes::Bytes,
        seq_num: i32,
        payload_hash: Vec<u8>,
    ) -> MergeableKey {
        MergeableKey {
            post_state_hash: StateHashSerde(post_state_hash.clone()),
            pre_state_hash: StateHashSerde(pre_state_hash.clone()),
            creator,
            seq_num,
            payload_hash,
        }
    }

    fn admitted_block_deploys(block: &BlockMessage) -> Vec<ProcessedDeploy> {
        if block.header.parents_hash_list.is_empty() {
            block.body.deploys.clone()
        } else {
            block
                .body
                .deploys
                .iter()
                .filter(|deploy| !deploy.is_admission_rejected())
                .cloned()
                .collect()
        }
    }

    pub fn load_mergeable_channels(
        &self,
        block: &BlockMessage,
    ) -> Result<Vec<NumberChannelsDiff>, CasperError> {
        let get_key = Self::mergeable_key_bytes_for_block(block)?;

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
                    "Missing mergeable entry for block {} at state {} (creator={}, seq={})",
                    hex::encode(&block.block_hash),
                    hex::encode(&block.body.state.post_state_hash),
                    block.sender.encode_hex::<String>(),
                    block.seq_num,
                );
                tracing::error!(block_hash = %hex::encode(&block.block_hash), state_hash = %hex::encode(&block.body.state.post_state_hash), creator = %block.sender.encode_hex::<String>(), seq_num = block.seq_num, "mergeable entry missing for block");
                Err(CasperError::KvStoreError(KvStoreError::KeyNotFound(msg)))
            }
        }
    }

    fn mergeable_key_bytes_for_block(block: &BlockMessage) -> Result<Vec<u8>, CasperError> {
        let is_genesis = block.header.parents_hash_list.is_empty();
        let deploys = Self::admitted_block_deploys(block);
        let payload_hash =
            Self::replay_payload_hash(&deploys, &block.body.system_deploys, is_genesis);
        let key = Self::mergeable_key_for_execution(
            &block.body.state.pre_state_hash,
            &block.body.state.post_state_hash,
            block.sender.clone(),
            block.seq_num,
            payload_hash,
        );
        bincode::serialize(&key)
            .map_err(|e| CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string())))
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
    ) -> Result<(), CasperError> {
        if self.has_mergeable_entry(block)? {
            return Ok(());
        }

        let computed_post_state = self
            .replay_block_from_consensus_data_for_origin(
                &block.body.state.pre_state_hash,
                block,
                Some(invalid_blocks),
                UserExecutionOrigin::Recovery,
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

    /// Delete the mergeable channels entry bound to one block execution.
    /// Returns `true` if the entry existed prior to deletion.
    pub fn delete_mergeable_channels(&self, block: &BlockMessage) -> Result<bool, CasperError> {
        let encoded_key = Self::mergeable_key_bytes_for_block(block)?;
        let existed = self.mergeable_store.contains_key(encoded_key.clone())?;
        if existed {
            self.mergeable_store.delete(vec![encoded_key])?;
        }
        Ok(existed)
    }

    /**
     * Converts final mergeable (number) channel values and save to mergeable store.
     *
     * The key binds the pre-state, post-state, creator, sequence number, and
     * canonical replay payload. The pre-state is also used to read the initial
     * value for each difference.
     */
    fn save_mergeable_channels(
        &self,
        post_state_hash: &StateHash,
        creator: prost::bytes::Bytes,
        seq_num: i32,
        channels_data: Vec<NumberChannelsEndVal>,
        pre_state_hash: &StateHash,
        payload_hash: Vec<u8>,
    ) -> Result<(), CasperError> {
        let pre_state_root = Blake2b256Hash::from_bytes_prost(pre_state_hash);
        let diffs = self.convert_number_channels_to_diff(channels_data, &pre_state_root)?;

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

        let mergeable_key = Self::mergeable_key_for_execution(
            pre_state_hash,
            post_state_hash,
            creator,
            seq_num,
            payload_hash,
        );

        let key_encoded = bincode::serialize(&mergeable_key).map_err(|e| {
            CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
        })?;

        // Save to mergeable channels store
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
        let mut initial_values: BTreeMap<Blake2b256Hash, i64> = BTreeMap::new();
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
                None => 0,
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
            move |ch| initial_values.get(ch).copied(),
        ))
    }

    /**
     * This is a hard-coded value for `emptyStateHash` which is calculated by
     * [[coop.rchain.casper.rholang.RuntimeOps.emptyStateHash]].
     * Because of the value is actually the same all
     * the time. For some situations, we can just use the value directly for better performance.
     */
    pub fn empty_state_hash_fixed() -> StateHash {
        // Updated 2026-08-13 for the authority-carrying RSpace schema used by
        // persistent located cost regions. This is a coordinated state-format
        // upgrade: even absent authority fields participate in canonical
        // continuation and datum encoding.
        hex::decode("b38db9a0203b6b9cf5987024f325b83da33be5c1b820b3f86fd979578f2985d5")
            .unwrap()
            .into()
    }

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
            block_index_cache_retained_bytes: Arc::new(AtomicUsize::new(0)),
            block_index_cache_write_lock: Arc::new(Mutex::new(())),
            active_validators_cache: Arc::new(DashMap::new()),
            active_validators_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            bonds_cache: Arc::new(DashMap::new()),
            bonds_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            bond_generations_cache: Arc::new(DashMap::new()),
            bond_generations_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            parents_post_state_cache: Arc::new(DashMap::new()),
            parents_post_state_cache_order: Arc::new(Mutex::new(VecDeque::new())),
            replay_cache: (replay_cache_size > 0).then(|| {
                Arc::new(InMemoryReplayCache::with_limits(
                    replay_cache_size,
                    Self::max_replay_cache_bytes(),
                ))
            }),
            replay_lock: Arc::new(ReplayLock::new()),
            exploratory_deploy_semaphore: Arc::new(Semaphore::new(
                exploratory_deploy_config.max_concurrent,
            )),
            exploratory_deploy_phlo_limit: exploratory_deploy_config.phlo_limit,
            exploratory_deploy_execution_timeout: exploratory_deploy_config.execution_timeout,
            external_services,
            // Slice 30b: default None; boot sets via
            // `set_fs_snapshot_writer`.
            fs_snapshot_writer: Arc::new(tokio::sync::RwLock::new(None)),
            // H-1 fix (2026-08-06) — slice 30c Phase B: empty cache
            // at boot.  Populated by `play_deploys_for_state`,
            // consumed by finalization_runner's LFB-found effect.
            pending_wal_slices: Arc::new(
                tokio::sync::RwLock::new(std::collections::HashMap::new()),
            ),
            // Phase 7b-1 (2026-08-27): empty cache at boot.
            // Populated by finalization_runner's WalSnapshotWrite
            // branch after maybe_write returns; consumed by the
            // follow-up SnapshotChunkRetriever.
            snapshot_merkle_roots: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            // H-5 (2026-08-06): empty registry at boot.  Boot
            // pipeline calls `register_root_identity` for each
            // provisioned root path before any deploy runs.
            root_id_registry: rholang::rust::interpreter::io::path::RootIdentityRegistry::new(),
            // Phase 8 slice 8a: empty range-lock registry at boot.
            // Populated per-acquire by `fs_lock_range` handlers.
            lock_registry: rholang::rust::interpreter::io::lock::LockRegistry::new(),
            // Phase 7b-2 (2026-08-27): default None; boot sets via
            // `set_payload_store`.
            payload_store: Arc::new(tokio::sync::RwLock::new(None)),
            // DD-7b-2 (a) Option 2 (2026-08-29): default None;
            // boot sets via `set_payload_source_recorder`.
            payload_source_recorder: Arc::new(tokio::sync::RwLock::new(None)),
            // c-2 review-follow-up (2026-08-30): empty until boot
            // populates via `register_consensus_static_root`.
            consensus_static_roots: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    /// H-5 fix (2026-08-06): register a boot-captured root
    /// `(dev, inode)` pair.  Called from `node::setup` for each
    /// operator-provisioned canonical root path after
    /// `merge_and_validate` succeeds.  Shared with all spawned
    /// runtimes' `FileHandleTable`s via `share_root_registry`;
    /// consumed on every `safe_descend_verified` by the fs_*
    /// handlers.
    pub fn register_root_identity(&self, canon_root: std::path::PathBuf, id: (u64, u64)) {
        self.root_id_registry.register(canon_root, id);
    }

    /// Shape A (2026-08-31): register a per-validator logical → on-disk
    /// mapping alongside the boot-captured `(dev, inode)` pair.
    /// Called from `node::setup` and the test harness for each
    /// Consensus-mode bundle entry: the composed Rholang source
    /// carries the bundle-relative `/@bundle/<X>` logical root
    /// (validator-independent — see
    /// `format_bundle_for_rholang`), and every fs handler's
    /// `resolve_or_identity(canonRoot)` call joins that logical
    /// root to the validator's own on-disk staging directory before
    /// `safe_descend_verified`.  Both the (dev, inode) identity and
    /// the on-disk join are stored under `logical` — the identity is
    /// captured from the on-disk path so `safe_descend_verified`'s
    /// H-5 fstat-post-open check verifies against the real staging
    /// dir.  Legacy `register_root_identity` (used by Oracular
    /// bundles) collapses to `logical == on_disk`; this method is
    /// the Shape A entry point that decouples them.
    pub fn register_root_remap(
        &self,
        logical: std::path::PathBuf,
        on_disk: std::path::PathBuf,
        id: (u64, u64),
    ) {
        self.root_id_registry
            .register_with_remap(logical, on_disk, id);
    }

    /// H-5 diagnostic — number of registered root identities.
    /// Used by boot to emit a one-line summary after populating.
    pub fn root_identity_count(&self) -> usize { self.root_id_registry.len() }

    /// Slice 30b: boot hook — install (or clear) the shared
    /// snapshot writer.  Every subsequent `spawn_runtime` /
    /// `spawn_replay_runtime` call attaches the current value to
    /// the returned `RhoRuntimeImpl.fs_snapshot_writer`.
    ///
    /// Slice 30c F-30b-2 disposition: hot-reload is INTENTIONAL.
    /// H-30b-2 refactored the writer slot to
    /// `Arc<RwLock<Option<SnapshotWriter>>>` so post-boot
    /// operator adjustments (retention tuning, dir migration,
    /// snapshot disable) can take effect on already-spawned
    /// runtimes without a node restart.  Every runtime reads the
    /// slot on every `SnapshotWriter::maybe_write` call, so the
    /// next block-boundary write picks up the new config.
    ///
    /// Consensus-safety note: `cadence` is NOT a per-node knob
    /// (see slice 30c Phase A — cadence is a shard-wide Genesis
    /// parameter).  Hot-reload here therefore does not fork
    /// consensus even if operators disagree on when to change
    /// `dir` or `retain`, because those are per-node local
    /// concerns.  If a future slice adds Genesis-committed
    /// fields to `SnapshotWriter`, hot-reload semantics must be
    /// revisited (the RwLock write on a live runtime while a
    /// deploy is mid-flight is safe — the runtime reads the
    /// slot ONLY at end-of-block, not during deploy execution).
    pub async fn set_fs_snapshot_writer(
        &self,
        writer: Option<rholang::rust::interpreter::io::snapshot::SnapshotWriter>,
    ) {
        *self.fs_snapshot_writer.write().await = writer;
    }

    /// Phase 7b-2 (2026-08-27): boot hook — install (or clear) the
    /// shared payload persistence backend.  Every subsequent
    /// `spawn_runtime` / `spawn_replay_runtime` call attaches the
    /// current value to the returned runtime's
    /// `fs_handles.payload_store`.  Mirrors
    /// `set_fs_snapshot_writer`'s hot-reload semantics: writes to
    /// the RwLock are picked up on the next spawn.
    ///
    /// Consensus-safety: `journal_write` reads the store slot per
    /// call; a boot-time set is immediately visible to every
    /// live runtime.  Store identity (which dir, which retention)
    /// is a per-node local concern and does not affect consensus —
    /// only the WAL entries themselves are consensus-observable.
    pub async fn set_payload_store(
        &self,
        bundle: Option<crate::rust::engine::wal_payload_server::PayloadStoreBundle>,
    ) {
        *self.payload_store.write().await = bundle;
    }

    /// DD-7b-2 (a) Option 2 (2026-08-29): boot hook — install (or
    /// clear) the shared payload-source recorder.  Every subsequent
    /// `spawn_runtime` / `spawn_replay_runtime` call attaches the
    /// current value to the returned runtime's
    /// `fs_handles.payload_source_recorder`.  Mirrors
    /// `set_payload_store`'s hot-reload semantics — a boot-time set
    /// (or a later toggle) is picked up on the next spawn.
    ///
    /// Consensus-safety: `journal_write` reads the recorder slot
    /// per call; a boot-time set is immediately visible to every
    /// live runtime.  Recorder identity (which storage backend)
    /// is a per-node local concern and does not affect consensus —
    /// only the WAL entries themselves are consensus-observable.
    /// A `None` recorder → journal_write skips the record step,
    /// which matches pre-Option-2 behavior verbatim.
    pub async fn set_payload_source_recorder(
        &self,
        recorder: Option<Arc<dyn rholang::rust::interpreter::io::wal::PayloadSourceRecorder>>,
    ) {
        *self.payload_source_recorder.write().await = recorder;
    }

    /// c-2 review-follow-up (2026-08-30): register a consensus-
    /// static root (file OR directory path) so the boot subscriber
    /// can pass it as an `allowed_roots` entry to
    /// `apply_wal_slice_after_fetch`.  Called from `node::setup` for
    /// each entry in `merged.consensus_static_files` /
    /// `merged.consensus_static_dirs` — mirrors
    /// `register_root_identity`'s registration pattern.
    ///
    /// # Path canonicalization (2026-08-30 review-follow-up)
    ///
    /// The applier's `check_path_allowed` uses `Path::starts_with`,
    /// which is component-based: `/opt/foo/../foo/data/x.bin` does
    /// NOT start with `/opt/foo/data/`.  WAL entry target paths
    /// come from `canonicalize_lexical(root, rel)` on the leader
    /// side, which strips `Component::CurDir` and leaves the join
    /// component-normalized.  If registered roots weren't
    /// normalized the same way, an operator config with `.` (or
    /// symlinks resolved to different lexical forms) would silently
    /// reject legitimate writes at boot.
    ///
    /// This method applies the same `canonicalize_lexical` shape
    /// (with empty `rel`) so registered roots and WAL entry paths
    /// share the same lexical normalization contract.  Symlinks are
    /// deliberately NOT followed — matches the leader's discipline.
    pub async fn register_consensus_static_root(&self, path: std::path::PathBuf) {
        let normalized = normalize_root_lexical(&path);
        self.consensus_static_roots.write().await.push(normalized);
    }

    /// c-2 review-follow-up (2026-08-30): read the operator's
    /// consensus-static roots for use as `allowed_roots` at the
    /// boot subscriber sites.  Empty when the operator has no
    /// consensus-static provisioning (observer nodes) OR when boot
    /// hasn't populated yet — the applier skips validation on
    /// empty, matching pre-plumbing behavior.
    pub async fn consensus_static_roots(&self) -> Vec<std::path::PathBuf> {
        self.consensus_static_roots.read().await.clone()
    }

    /// Phase 7b-2 diagnostic — read the currently-installed
    /// payload store bundle.  Used at boot to thread the same
    /// underlying store into `WalPayloadContext.payload_lookup`
    /// (via `bundle.lookup`) so joiner-side reads and leader-side
    /// writes hit the same on-disk dir.
    pub async fn get_payload_store(
        &self,
    ) -> Option<crate::rust::engine::wal_payload_server::PayloadStoreBundle> {
        self.payload_store.read().await.clone()
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
#[path = "replay_evidence_tests.rs"]
mod replay_evidence_tests;

#[cfg(test)]
#[path = "runtime_cache_lock_tests.rs"]
mod cache_lock_tests;

#[cfg(test)]
mod snapshot_writer_wiring_tests {
    //! C-30b-2 round-2 review-fix tests: `RuntimeManager`
    //! `set_fs_snapshot_writer` + spawn attach chain.
    //!
    //! Pre-fix, this whole chain (Arc<RwLock<Option<SnapshotWriter>>>
    //! shared between manager and every spawned runtime) was
    //! untested.  A regression that forgot the `share_fs_snapshot_writer`
    //! call in `spawn_runtime` / `spawn_replay_runtime` would leave
    //! production silently non-snapshotting.

    use std::path::PathBuf;
    use std::sync::Arc;

    use rholang::rust::interpreter::io::snapshot::SnapshotWriter;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;

    async fn empty_manager() -> RuntimeManager {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        let mergeable_store = RuntimeManager::mergeable_store(&mut kvm).await.unwrap();
        RuntimeManager::create_with_store(
            store,
            mergeable_store,
            Arc::new(HashMap::new()),
            ExternalServices::noop(),
        )
    }

    /// Set-before-spawn: the writer set on the manager is visible
    /// to a subsequently spawned runtime.
    #[tokio::test]
    async fn manager_set_before_spawn_visible_to_runtime() {
        let manager = empty_manager().await;
        let writer = SnapshotWriter {
            dir: PathBuf::from("/tmp/does-not-need-to-exist-for-this-test"),
            cadence: 5,
            retain: 10,
            signer_sk: None,
            payload_dir: None,
        };
        manager.set_fs_snapshot_writer(Some(writer.clone())).await;
        let runtime = manager.spawn_runtime().await;
        let attached = runtime.fs_snapshot_writer.read().await;
        assert!(
            attached.is_some(),
            "spawned runtime must see the manager's set writer"
        );
        assert_eq!(attached.as_ref().unwrap().cadence, 5);
        assert_eq!(attached.as_ref().unwrap().retain, 10);
    }

    /// H-30b-2 round-2 core: set-AFTER-spawn is ALSO visible to
    /// the previously spawned runtime.  This is the property the
    /// pre-fix caching design lacked — each spawn used to snapshot
    /// the value into a per-runtime Option, so subsequent sets
    /// didn't propagate.  Post-fix, both share the Arc<RwLock<_>>.
    #[tokio::test]
    async fn manager_set_after_spawn_still_visible_to_runtime() {
        let manager = empty_manager().await;
        let runtime = manager.spawn_runtime().await;
        // Pre-set, the runtime sees None.
        assert!(runtime.fs_snapshot_writer.read().await.is_none());
        // Boot set fires AFTER spawn.
        let writer = SnapshotWriter {
            dir: PathBuf::from("/tmp"),
            cadence: 7,
            retain: 14,
            signer_sk: None,
            payload_dir: None,
        };
        manager.set_fs_snapshot_writer(Some(writer)).await;
        // The already-spawned runtime observes the new value —
        // shared Arc<RwLock<_>> semantics.
        let attached = runtime.fs_snapshot_writer.read().await;
        assert!(
            attached.is_some(),
            "H-30b-2: post-spawn set MUST propagate to already-spawned runtime \
             (pre-fix cached per-spawn and this failed)"
        );
        assert_eq!(attached.as_ref().unwrap().cadence, 7);
    }

    /// Multiple runtimes spawned from the same manager share the
    /// SAME writer slot.  Setting once updates all.
    #[tokio::test]
    async fn multiple_runtimes_share_same_writer_slot() {
        let manager = empty_manager().await;
        let r1 = manager.spawn_runtime().await;
        let r2 = manager.spawn_runtime().await;
        let writer = SnapshotWriter {
            dir: PathBuf::from("/tmp"),
            cadence: 3,
            retain: 6,
            signer_sk: None,
            payload_dir: None,
        };
        manager.set_fs_snapshot_writer(Some(writer)).await;
        assert_eq!(
            r1.fs_snapshot_writer.read().await.as_ref().unwrap().cadence,
            3
        );
        assert_eq!(
            r2.fs_snapshot_writer.read().await.as_ref().unwrap().cadence,
            3
        );
        // Clear via set-None also propagates.
        manager.set_fs_snapshot_writer(None).await;
        assert!(r1.fs_snapshot_writer.read().await.is_none());
        assert!(r2.fs_snapshot_writer.read().await.is_none());
    }

    /// Replay runtimes also get the shared slot (parity with
    /// leader runtimes).
    #[tokio::test]
    async fn replay_runtimes_also_share_the_writer_slot() {
        let manager = empty_manager().await;
        let writer = SnapshotWriter {
            dir: PathBuf::from("/tmp"),
            cadence: 100,
            retain: 200,
            signer_sk: None,
            payload_dir: None,
        };
        manager.set_fs_snapshot_writer(Some(writer)).await;
        let replay_rt = manager.spawn_replay_runtime().await;
        assert!(replay_rt.fs_snapshot_writer.read().await.is_some());
    }
}

#[cfg(test)]
mod payload_store_wiring_tests {
    //! Phase 7b-2 (2026-08-27): parity pins for the `payload_store`
    //! bundle slot on `RuntimeManager`.  Same shape as the
    //! `snapshot_writer_wiring_tests` above — a regression that
    //! forgot `share_payload_store` in `spawn_runtime` /
    //! `spawn_replay_runtime` would silently disable
    //! leader-side payload persistence and leave joining
    //! validators unable to fetch any bytes.
    use std::sync::Arc;

    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::engine::wal_payload_server::{
        DirectoryPayloadStore, InMemoryPayloadStore, PayloadStoreBundle,
    };

    async fn empty_manager() -> RuntimeManager {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        let mergeable_store = RuntimeManager::mergeable_store(&mut kvm).await.unwrap();
        RuntimeManager::create_with_store(
            store,
            mergeable_store,
            Arc::new(HashMap::new()),
            ExternalServices::noop(),
        )
    }

    /// Set-before-spawn: the bundle set on the manager is visible
    /// to a subsequently spawned runtime's fs_handles.
    #[tokio::test]
    async fn manager_set_before_spawn_visible_to_runtime() {
        let manager = empty_manager().await;
        let bundle = PayloadStoreBundle::from_in_memory(InMemoryPayloadStore::new());
        manager.set_payload_store(Some(bundle)).await;
        let runtime = manager.spawn_runtime().await;
        assert!(
            runtime.fs_handles.payload_store().is_some(),
            "spawned runtime must see the manager's set payload store"
        );
    }

    /// Post-manager-set is NOT retroactively visible to a
    /// previously-spawned runtime.  `set_payload_store` updates
    /// the manager's own slot; only the next `spawn_runtime` /
    /// `spawn_replay_runtime` propagates the new value into the
    /// runtime.  Hot-reload of the payload store therefore
    /// requires a runtime respawn (matches the operator mental
    /// model for the payload dir since it's tied to the on-disk
    /// data directory).
    #[tokio::test]
    async fn post_spawn_manager_set_does_not_retroactively_attach() {
        let manager = empty_manager().await;
        let runtime = manager.spawn_runtime().await;
        assert!(runtime.fs_handles.payload_store().is_none());
        let bundle = PayloadStoreBundle::from_in_memory(InMemoryPayloadStore::new());
        manager.set_payload_store(Some(bundle)).await;
        // Existing runtime still None — manager set went into the
        // manager's own slot, not the already-spawned runtime.
        assert!(runtime.fs_handles.payload_store().is_none());
        // Next spawn picks up the new manager slot.
        let runtime2 = manager.spawn_runtime().await;
        assert!(runtime2.fs_handles.payload_store().is_some());
    }

    /// Replay runtimes also get the shared bundle (parity with
    /// leader runtimes).
    #[tokio::test]
    async fn replay_runtimes_also_share_the_payload_store() {
        let manager = empty_manager().await;
        let bundle = PayloadStoreBundle::from_in_memory(InMemoryPayloadStore::new());
        manager.set_payload_store(Some(bundle)).await;
        let replay_rt = manager.spawn_replay_runtime().await;
        assert!(replay_rt.fs_handles.payload_store().is_some());
    }

    /// Interior-mutability pin: attaching a payload store on the
    /// runtime's `fs_handles` after spawn MUST also be visible
    /// through the `FsProcesses` clone (the reducer's clone
    /// taken at `create_rho_runtime` time).  Documents that the
    /// field's `Arc<RwLock<...>>` shape enables post-spawn shares
    /// to cross the clone boundary — the "obvious" `Option<Arc<>>`
    /// design would leave FsProcesses's clone with a stale None
    /// and break `journal_write`'s persist hook.
    #[tokio::test]
    async fn post_spawn_share_on_runtime_is_visible_to_fs_handles_clones() {
        let manager = empty_manager().await;
        let runtime = manager.spawn_runtime().await;
        // Take a clone of runtime.fs_handles the same way
        // FsProcesses does at reducer-setup time (which happens
        // BEFORE any post-spawn share).
        let clone_of_handles = runtime.fs_handles.clone();
        assert!(clone_of_handles.payload_store().is_none());
        // Now share a store on the runtime's copy — this is the
        // path `RuntimeManager::spawn_runtime` uses, after the
        // FsProcesses clone was already taken.
        let store: Arc<dyn rholang::rust::interpreter::io::wal::PayloadPersistence> =
            Arc::new(InMemoryPayloadStore::new());
        runtime.fs_handles.share_payload_store(Some(store));
        // The clone MUST see the newly-attached store.
        assert!(
            clone_of_handles.payload_store().is_some(),
            "post-spawn share_payload_store must propagate through Arc<RwLock<>> \
             interior mutability — otherwise FsProcesses's clone would strand \
             the store forever"
        );
    }

    /// `get_payload_store` round-trip: what boot installs is what
    /// the casper-launch / initializing / genesis-ceremony-master
    /// wire-up sees when threading the same bundle into
    /// `WalPayloadContext.payload_lookup`.
    #[tokio::test]
    async fn get_payload_store_returns_the_installed_bundle() {
        let manager = empty_manager().await;
        assert!(manager.get_payload_store().await.is_none());
        let bundle = PayloadStoreBundle::from_in_memory(InMemoryPayloadStore::new());
        manager.set_payload_store(Some(bundle)).await;
        let got = manager.get_payload_store().await;
        assert!(got.is_some(), "installed bundle must be readable back");
    }

    /// Directory-backed bundle round-trip:
    /// leader-side `journal_write` populates the store via the
    /// PayloadPersistence trait object, and the joiner-side wire
    /// dispatch reads bytes back via the PayloadLookup trait
    /// object.  Both trait objects must resolve to the same
    /// underlying directory.
    #[tokio::test]
    async fn directory_backed_bundle_round_trip_between_trait_objects() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = PayloadStoreBundle::from_directory(DirectoryPayloadStore::new(
            dir.path().to_path_buf(),
        ));
        let payload = b"cross-trait round trip".to_vec();
        // Write via the persistence side (what journal_write uses).
        let h = bundle.persistence.persist(&payload).unwrap();
        // Read via the lookup side (what serve_payload uses).
        let got = bundle.lookup.get(&h).unwrap().unwrap();
        assert_eq!(got, payload);
    }

    /// L-3 review pin (2026-08-29): overriding the payload_store
    /// on ONE runtime's `fs_handles` MUST NOT leak into a
    /// different runtime spawned from the same manager.  The
    /// Option 2 helper (`capture_consensus_writes_by_replaying_deploy`)
    /// relies on this isolation: it replaces the scratch runtime's
    /// payload_store with an in-memory capture store, and the
    /// helper's docstring promises other runtimes' stores stay
    /// intact.  A future refactor moving the `payload_store` slot
    /// from per-`FileHandleTable` to manager-shared would silently
    /// break this invariant — captures would leak into the
    /// production runtime's serving payload_store, poisoning
    /// downstream wire dispatch.
    #[tokio::test]
    async fn per_runtime_payload_store_override_does_not_leak_across_runtimes() {
        let manager = empty_manager().await;
        // Establish a manager-shared bundle so both runtimes start
        // with the same PROD store visible.
        let prod_bundle = PayloadStoreBundle::from_in_memory(InMemoryPayloadStore::new());
        manager.set_payload_store(Some(prod_bundle.clone())).await;

        let rt_a = manager.spawn_runtime().await;
        let rt_b = manager.spawn_runtime().await;

        // Baseline: both runtimes see the prod bundle.
        assert!(rt_a.fs_handles.payload_store().is_some(), "rt_a baseline");
        assert!(rt_b.fs_handles.payload_store().is_some(), "rt_b baseline");

        // Override rt_a's local slot with a distinct capture store
        // — the pattern `capture_consensus_writes_by_replaying_deploy`
        // uses.  Store the capture Arc so we can compare against
        // rt_b's view.
        let capture = std::sync::Arc::new(InMemoryPayloadStore::new());
        rt_a.fs_handles.share_payload_store(Some(capture.clone()
            as std::sync::Arc<dyn rholang::rust::interpreter::io::wal::PayloadPersistence>));

        // rt_b's slot is unchanged — still the prod bundle, NOT
        // rt_a's capture.  Detect via Arc::ptr_eq on the underlying
        // trait objects: rt_b's persistence Arc should equal
        // prod_bundle.persistence, not the capture Arc.
        let rt_b_store = rt_b
            .fs_handles
            .payload_store()
            .expect("rt_b payload_store must remain set");
        let capture_as_persistence: std::sync::Arc<
            dyn rholang::rust::interpreter::io::wal::PayloadPersistence,
        > = capture.clone();
        assert!(
            !std::sync::Arc::ptr_eq(&rt_b_store, &capture_as_persistence),
            "rt_b's payload_store must NOT have been redirected to rt_a's capture store — \
             per-runtime interior mutability is a load-bearing property of the Option 2 \
             helper's isolation contract"
        );
        assert!(
            std::sync::Arc::ptr_eq(&rt_b_store, &prod_bundle.persistence),
            "rt_b's payload_store must still resolve to the manager-shared prod bundle"
        );
    }
}

#[cfg(test)]
mod payload_source_recorder_wiring_tests {
    //! DD-7b-2 (a) Option 2 (2026-08-29) + review-follow-up (2026-08-30):
    //! parity + isolation pins for the `payload_source_recorder` slot.
    //!
    //! Mirror of `payload_store_wiring_tests` above for the sibling
    //! recorder slot.  The Option 2 primitive
    //! `capture_consensus_writes_by_replaying_deploy` relies on the
    //! interior-mutability shape here to override JUST the scratch
    //! runtime's recorder to `None` without touching the manager's
    //! shared slot — closing the scratch-replay-pollution surface
    //! flagged in the 2026-08-29 review.
    //!
    //! Full E2E via a real `ProcessedDeploy` is deferred (see the
    //! `option2_leader_records_and_joiner_reproduces_end_to_end`
    //! skeleton in wal_payload_sync).  This module covers the
    //! isolation MECHANISM behaviorally — enough to catch a
    //! refactor that broke the per-runtime override without needing
    //! the full cosigned-deploy harness.

    use std::collections::HashMap;
    use std::sync::Arc;

    use rholang::rust::interpreter::io::wal::PayloadSourceRecorder;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;

    async fn empty_manager() -> RuntimeManager {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        let mergeable_store = RuntimeManager::mergeable_store(&mut kvm).await.unwrap();
        RuntimeManager::create_with_store(
            store,
            mergeable_store,
            Arc::new(HashMap::new()),
            ExternalServices::noop(),
        )
    }

    /// A no-op trait-object recorder for the isolation tests.  The
    /// tests observe the SLOT state (Some vs None) rather than the
    /// number of times record was called, so a bare no-op suffices.
    #[derive(Debug)]
    struct NoopRecorder;

    impl PayloadSourceRecorder for NoopRecorder {
        fn record(&self, _payload_hash: [u8; 32], _deploy_sig: &[u8]) -> Result<(), String> {
            Ok(())
        }
    }

    /// Set-before-spawn: the recorder set on the manager is visible
    /// to a subsequently spawned runtime.
    #[tokio::test]
    async fn manager_set_before_spawn_visible_to_runtime() {
        let manager = empty_manager().await;
        let recorder: Arc<dyn PayloadSourceRecorder> = Arc::new(NoopRecorder);
        manager.set_payload_source_recorder(Some(recorder)).await;
        let runtime = manager.spawn_runtime().await;
        assert!(
            runtime.fs_handles.payload_source_recorder().is_some(),
            "spawned runtime must see the manager's set recorder"
        );
    }

    /// Replay runtimes also get the shared recorder (parity with
    /// leader runtimes).
    #[tokio::test]
    async fn replay_runtimes_also_share_the_recorder() {
        let manager = empty_manager().await;
        let recorder: Arc<dyn PayloadSourceRecorder> = Arc::new(NoopRecorder);
        manager.set_payload_source_recorder(Some(recorder)).await;
        let replay_rt = manager.spawn_replay_runtime().await;
        assert!(
            replay_rt.fs_handles.payload_source_recorder().is_some(),
            "spawned replay runtime must see the manager's set recorder"
        );
    }

    // ---------------------------------------------------------------
    // DD-7b-2 (a) Option 2 review-follow-up (2026-08-30):
    // scratch-replay isolation behavioral tests.
    //
    // The primitive `capture_consensus_writes_by_replaying_deploy`
    // calls `runtime.fs_handles.share_payload_source_recorder(None)`
    // on the scratch runtime it spawned.  This test covers the
    // isolation mechanism end-to-end:
    //   1. Manager slot = Some(recorder).
    //   2. Spawn replay runtime → FileHandleTable inherits Some.
    //   3. `share_payload_source_recorder(None)` on that runtime →
    //      FileHandleTable now sees None.
    //   4. Manager slot UNCHANGED = Some.
    //   5. Spawn ANOTHER replay runtime → FileHandleTable inherits
    //      Some (the manager slot wasn't touched).
    //
    // Without the override, a scratch replay of a state-dependent
    // deploy that produced divergent bytes would silently write
    // `(hash_divergent, deploy_sig)` into the recorder's index —
    // dead storage in the joiner's persistent block-storage index.
    // ---------------------------------------------------------------

    /// The primitive's per-runtime override clears just this
    /// runtime's slot without touching the manager's shared slot.
    /// A subsequently spawned runtime still inherits the manager's
    /// slot.
    #[tokio::test]
    async fn share_payload_source_recorder_none_isolates_scratch_runtime() {
        let manager = empty_manager().await;
        let recorder: Arc<dyn PayloadSourceRecorder> = Arc::new(NoopRecorder);
        manager
            .set_payload_source_recorder(Some(Arc::clone(&recorder)))
            .await;

        // Scratch runtime: initially inherits Some(recorder).
        let scratch = manager.spawn_replay_runtime().await;
        assert!(
            scratch.fs_handles.payload_source_recorder().is_some(),
            "scratch runtime must inherit the manager's recorder on spawn — \
             without this, the pre-override state is broken and the follow \
             pin is testing a no-op"
        );

        // Primitive's override: clear the scratch runtime's slot
        // WITHOUT touching the manager.
        scratch.fs_handles.share_payload_source_recorder(None);
        assert!(
            scratch.fs_handles.payload_source_recorder().is_none(),
            "share_payload_source_recorder(None) on the scratch runtime MUST \
             clear its own slot — the fix at `capture_consensus_writes_by_\
             replaying_deploy` depends on this to prevent scratch-replay \
             pollution of the joiner's persistent payload_source_index."
        );

        // Manager's slot: UNCHANGED.
        let manager_slot = manager.payload_source_recorder.read().await.clone();
        assert!(
            manager_slot.is_some(),
            "manager's payload_source_recorder slot must be UNCHANGED by a \
             per-runtime override — the isolation contract is that scratch \
             runtimes get to disable recording locally without disabling it \
             globally"
        );

        // A subsequently spawned runtime STILL inherits Some.
        let post_scratch = manager.spawn_replay_runtime().await;
        assert!(
            post_scratch.fs_handles.payload_source_recorder().is_some(),
            "a runtime spawned AFTER the scratch runtime's override must \
             STILL inherit the manager's slot — the override was per-runtime, \
             not manager-wide"
        );
    }

    /// The scratch override survives the fs_handles Clone taken at
    /// FsProcesses setup time — same interior-mutability rationale
    /// as `post_spawn_share_on_runtime_is_visible_to_fs_handles_clones`
    /// in payload_store_wiring_tests.  Without this, journal_write
    /// on the scratch runtime would see a stale Some(recorder) and
    /// leak entries into the joiner's index.
    #[tokio::test]
    async fn scratch_override_visible_through_fs_handles_clone() {
        let manager = empty_manager().await;
        let recorder: Arc<dyn PayloadSourceRecorder> = Arc::new(NoopRecorder);
        manager
            .set_payload_source_recorder(Some(Arc::clone(&recorder)))
            .await;
        let scratch = manager.spawn_replay_runtime().await;
        // Take a clone the same way FsProcesses does at reducer-
        // setup time.
        let clone_of_handles = scratch.fs_handles.clone();
        assert!(
            clone_of_handles.payload_source_recorder().is_some(),
            "pre-override, the clone must see Some(recorder)"
        );
        // Primitive's per-runtime override.
        scratch.fs_handles.share_payload_source_recorder(None);
        assert!(
            clone_of_handles.payload_source_recorder().is_none(),
            "post-override, the clone MUST see None — otherwise journal_write \
             called via the FsProcesses clone would leak entries into the \
             joiner's index despite the primitive's isolation attempt"
        );
    }
}

#[cfg(test)]
mod consensus_static_roots_wiring_tests {
    //! c-2 review-follow-up (2026-08-30): behavioral wiring pins
    //! for the `consensus_static_roots` slot used by the boot
    //! subscriber's `allowed_roots` defense-in-depth path.

    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;

    async fn empty_manager() -> RuntimeManager {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        let mergeable_store = RuntimeManager::mergeable_store(&mut kvm).await.unwrap();
        RuntimeManager::create_with_store(
            store,
            mergeable_store,
            Arc::new(HashMap::new()),
            ExternalServices::noop(),
        )
    }

    /// Round-trip: registered roots are visible via
    /// `consensus_static_roots()`.
    #[tokio::test]
    async fn register_then_read_round_trip() {
        let manager = empty_manager().await;
        assert!(
            manager.consensus_static_roots().await.is_empty(),
            "fresh manager has no roots"
        );
        manager
            .register_consensus_static_root(PathBuf::from(
                "/opt/f1r3fly/consensus-static-01/data.bin",
            ))
            .await;
        manager
            .register_consensus_static_root(PathBuf::from("/opt/f1r3fly/consensus-static-dir/"))
            .await;
        let roots = manager.consensus_static_roots().await;
        assert_eq!(roots.len(), 2);
        assert!(roots.contains(&PathBuf::from("/opt/f1r3fly/consensus-static-01/data.bin")));
        assert!(roots.contains(&PathBuf::from("/opt/f1r3fly/consensus-static-dir/")));
    }

    /// The roots slot is manager-shared, not per-runtime.  Reads
    /// see the same set regardless of which clone is queried.
    #[tokio::test]
    async fn roots_are_manager_shared_across_clones() {
        let manager = empty_manager().await;
        manager
            .register_consensus_static_root(PathBuf::from("/opt/x"))
            .await;
        let manager_clone = manager.clone();
        let roots_a = manager.consensus_static_roots().await;
        let roots_b = manager_clone.consensus_static_roots().await;
        assert_eq!(roots_a, roots_b);
        assert_eq!(roots_a, vec![PathBuf::from("/opt/x")]);
    }

    // ---------------------------------------------------------------
    // c-2 review-follow-up (2026-08-30): registered roots must be
    // lexically normalized to match WAL entry canon_path values (which
    // come from `canonicalize_lexical(root, rel)` on the leader).
    // A regression that dropped the normalization would silently reject
    // legit writes at boot for operator configs with `.` segments.
    // ---------------------------------------------------------------

    /// `Component::CurDir` (`.`) segments are stripped from
    /// registered roots.  A WAL entry with the same target under
    /// the un-normalized root would fail `starts_with` because
    /// `Path::starts_with` is component-based.
    #[tokio::test]
    async fn register_strips_cur_dir_components() {
        let manager = empty_manager().await;
        manager
            .register_consensus_static_root(PathBuf::from("/opt/./f1r3fly/./data/x.bin"))
            .await;
        let roots = manager.consensus_static_roots().await;
        assert_eq!(
            roots,
            vec![PathBuf::from("/opt/f1r3fly/data/x.bin")],
            "`.` components MUST be stripped so registered roots match \
             WAL entry paths produced by canonicalize_lexical.  A refactor \
             that dropped this normalization would silently reject legit \
             writes on operator configs containing `.` segments."
        );
    }

    /// Roots without `.` segments round-trip unchanged.
    #[tokio::test]
    async fn register_plain_paths_round_trip_unchanged() {
        let manager = empty_manager().await;
        for p in [
            PathBuf::from("/opt/f1r3fly/data/x.bin"),
            PathBuf::from("/opt/f1r3fly/data/"),
            PathBuf::from("/"),
        ] {
            manager.register_consensus_static_root(p.clone()).await;
        }
        let roots = manager.consensus_static_roots().await;
        assert_eq!(roots.len(), 3);
        assert!(roots.contains(&PathBuf::from("/opt/f1r3fly/data/x.bin")));
        assert!(roots.contains(&PathBuf::from("/opt/f1r3fly/data/")));
        assert!(roots.contains(&PathBuf::from("/")));
    }

    /// `..` segments are NOT resolved — matches `canonicalize_lexical`
    /// discipline (rejecting `..` is `safe_descend`'s job upstream).
    /// An operator config with `..` segments would produce a
    /// registered root that WAL entries wouldn't match — that's a
    /// misconfiguration for the operator to fix, not something
    /// register_consensus_static_root papers over.
    #[tokio::test]
    async fn register_does_not_resolve_parent_dir_segments() {
        let manager = empty_manager().await;
        manager
            .register_consensus_static_root(PathBuf::from("/opt/foo/../foo/data/x.bin"))
            .await;
        let roots = manager.consensus_static_roots().await;
        // `..` passes through unchanged (not `/opt/foo/data/x.bin`).
        assert_eq!(
            roots,
            vec![PathBuf::from("/opt/foo/../foo/data/x.bin")],
            "`..` must pass through unchanged — matches canonicalize_lexical's \
             discipline.  Resolving `..` would require disk I/O (symlink \
             traversal semantics) which is deliberately out-of-scope."
        );
    }
}

/// H-7 fix (2026-08-06) regression tests: `RuntimeManager` spawn
/// wiring around `fs_handles`.
///
/// `fs_handles` (the `FileHandleTable`) is INTENTIONALLY per-
/// runtime — leader and follower operate on distinct backing
/// RSpaces and must not share fd state.  Cross-runtime WAL
/// byte-identity is preserved via the C-R1 shadow-handle path
/// (fs_open's is_replay branch calls `insert_at(leader_fd, ...)`
/// on the follower's own table using the leader's fd extracted
/// from `previous`).
///
/// What IS shared by the manager: `fs_snapshot_writer`,
/// `pending_wal_slices` (H-1), `root_id_registry` (H-5).  These
/// three tests pin the sharing/non-sharing contract at the
/// spawn boundary so a regression can't silently regress it.
#[cfg(test)]
mod h7_cross_runtime_wiring_tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use rholang::rust::interpreter::accounting::costs::Cost;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;

    async fn empty_manager() -> RuntimeManager {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        let mergeable_store = RuntimeManager::mergeable_store(&mut kvm).await.unwrap();
        RuntimeManager::create_with_store(
            store,
            mergeable_store,
            Arc::new(HashMap::new()),
            ExternalServices::noop(),
        )
    }

    /// H-7 core invariant: leader and follower spawned from the
    /// same manager get DISTINCT `fs_handles`.  If a future
    /// refactor accidentally shared them (e.g., a well-meaning
    /// "share_fs_handles" mirroring the writer/registry pattern),
    /// this test fires.  Sharing would corrupt fd allocation and
    /// violate the C-R1 shadow-handle design.
    #[tokio::test]
    async fn spawn_runtime_and_spawn_replay_yield_distinct_fs_handles() {
        let manager = empty_manager().await;
        let leader = manager.spawn_runtime().await;
        let follower = manager.spawn_replay_runtime().await;
        // The FileHandleTable is `Clone` (Arc-wrapped Inner), so
        // two runtimes that erroneously "shared" it would have
        // the same Arc backing.  Insert a fake handle into one
        // and confirm the other doesn't see it.
        //
        // Use the WAL as a fast proxy: it's part of the same
        // handle table struct and would follow the same sharing
        // pattern if someone mistakenly shared the whole table.
        leader
            .fs_handles
            .wal
            .append(rholang::rust::interpreter::io::wal::WalEntry {
                op: rholang::rust::interpreter::io::wal::WalOp::Write,
                path: PathBuf::from("/leader-only"),
                extra_path: None,
                offset: None,
                length: Some(0),
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: rholang::rust::interpreter::io::wal::WalOutcome::Success,
            })
            .unwrap();
        assert_eq!(
            leader.fs_handles.wal.len(),
            1,
            "leader WAL received the append"
        );
        assert_eq!(
            follower.fs_handles.wal.len(),
            0,
            "H-7: follower's fs_handles.wal MUST NOT observe the leader's append — \
             regression would indicate `spawn_replay_runtime` accidentally shared \
             fs_handles with the leader, corrupting fd allocation and violating \
             the C-R1 shadow-handle design"
        );
    }

    /// H-5 wiring sanity through the RuntimeManager pair: the
    /// root-identity registry set on the manager is visible to
    /// BOTH the leader's and the follower's `fs_handles`.
    /// Positive counterpart to the H-7 negative pin above — the
    /// registry IS shared even though the enclosing table is
    /// per-runtime.
    #[tokio::test]
    async fn root_registry_is_shared_across_spawn_runtime_and_spawn_replay() {
        let manager = empty_manager().await;
        // Register a root identity BEFORE any spawn.
        let root = PathBuf::from("/tmp/h7-shared-root-fixture");
        manager.register_root_identity(root.clone(), (42, 137));
        let leader = manager.spawn_runtime().await;
        let follower = manager.spawn_replay_runtime().await;
        assert_eq!(
            leader.fs_handles.root_registry.get(&root),
            Some((42, 137)),
            "leader must see the manager's boot-registered identity"
        );
        assert_eq!(
            follower.fs_handles.root_registry.get(&root),
            Some((42, 137)),
            "H-5/H-7: follower must see the same identity — the registry is \
             manager-shared even though the FileHandleTable is per-runtime"
        );
        // Register AFTER both spawns — propagates to both via
        // the shared Arc.
        let late = PathBuf::from("/tmp/h7-late-root");
        manager.register_root_identity(late.clone(), (7, 11));
        assert_eq!(leader.fs_handles.root_registry.get(&late), Some((7, 11)));
        assert_eq!(follower.fs_handles.root_registry.get(&late), Some((7, 11)));
    }

    /// H-7 full E2E: leader + follower spawned from the SAME
    /// manager, run the same Consensus-cap Rholang term with
    /// leader → checkpoint → follower rig → replay, and assert
    /// their WALs are byte-identical.  Complements
    /// `wal_is_byte_identical_on_leader_and_follower` in
    /// fs_wal_spec.rs (which uses raw `create_rho_runtime` /
    /// `create_replay_rho_runtime` — this one goes through the
    /// full RuntimeManager wiring so any regression that broke
    /// spawn_runtime's fs_handles setup surfaces here).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn spawned_leader_and_follower_produce_byte_identical_wal() {
        use crypto::rust::hash::blake2b512_random::Blake2b512Random;

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.bin"), vec![0u8; 128]).unwrap();

        let manager = empty_manager().await;
        let mut leader = manager.spawn_runtime().await;
        let mut follower = manager.spawn_replay_runtime().await;
        leader.cost.set(Cost::unsafe_max());
        follower.cost.set(Cost::unsafe_max());
        leader.disable_fs_native_urn_filter();
        follower.disable_fs_native_urn_filter();

        // Consensus cap → Write + WriteAt + Truncate: exercises
        // all three fd-based WAL sites.  Same shape as
        // fs_wal_spec::wal_is_byte_identical_on_leader_and_follower
        // but here the runtimes come from RuntimeManager, which
        // is the production wiring path.
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                oc, w1, w2, w3
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "aa".hexToBytes(), *w1) |
                for (@_ <- w1) {{
                  fsWriteAt!(fd, 5, "bbcc".hexToBytes(), *w2) |
                  for (@_ <- w2) {{
                    fsTruncate!(fd, 32, *w3) |
                    for (@_ <- w3) {{ Nil }}
                  }}
                }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let rand = Blake2b512Random::create_from_bytes(&[7; 32]);

        leader
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand.clone(),
            )
            .await
            .expect("leader evaluate");
        let leader_wal = leader.fs_handles.wal.snapshot();
        assert!(
            !leader_wal.is_empty(),
            "leader must have journaled Consensus mutations"
        );

        // Rig follower with leader's log + reset to leader's
        // state, then re-execute the same term with is_replay=true
        // driven by the RSpaceWithReplay pairing.
        let checkpoint = leader.create_checkpoint().await;
        let root = checkpoint.root;
        let log = checkpoint.log;
        follower.reset(&root).await.expect("follower reset");
        follower.rig(log).await.expect("follower rig");
        follower
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand,
            )
            .await
            .expect("follower evaluate");
        let follower_wal = follower.fs_handles.wal.snapshot();

        assert_eq!(
            leader_wal.len(),
            follower_wal.len(),
            "H-7: manager-spawned leader/follower WAL lengths differ \
             ({} vs {}) — indicates a regression in the RuntimeManager \
             spawn wiring (fs_handles shared where it shouldn't be, or \
             a shared substrate that should be per-runtime got shared \
             incorrectly)",
            leader_wal.len(),
            follower_wal.len(),
        );
        for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
            assert_eq!(
                l, f,
                "H-7: manager-spawned WAL entry {i} differs: leader={l:?} follower={f:?}"
            );
        }
        follower
            .check_replay_data()
            .await
            .expect("H-7: manager-spawned follower replay-data check must pass");
    }

    /// M-11 fix (2026-08-06): two-runtime state-root equality.
    /// Phase 7's bedrock claim — "same input → same state root
    /// across validators" — had no pin at the test level.
    ///
    /// Runs the same fs-native deploy on two INDEPENDENT play
    /// runtimes (each with its own FileHandleTable, each with
    /// its own manager) and asserts their post-deploy state
    /// roots are byte-identical.  This is a stricter pin than
    /// the leader/follower rig-and-replay tests: those share
    /// the same RSpace store; this one uses truly separate
    /// stores + runtimes, mimicking two validators in the
    /// wild processing the same deploy.
    ///
    /// A regression that made state-hash derivation depend on
    /// non-deterministic input (wall-clock, TLB address, tokio
    /// scheduler order) would fire here.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_independent_runtimes_reach_the_same_state_root() {
        use crypto::rust::hash::blake2b512_random::Blake2b512Random;

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.bin"), vec![0u8; 128]).unwrap();

        // Two independent managers (distinct in-memory stores),
        // each spawning its own play runtime.  These runtimes
        // share no state — a validator on Node A vs Node B.
        let manager_a = empty_manager().await;
        let manager_b = empty_manager().await;
        let mut runtime_a = manager_a.spawn_runtime().await;
        let mut runtime_b = manager_b.spawn_runtime().await;
        runtime_a.cost.set(Cost::unsafe_max());
        runtime_b.cost.set(Cost::unsafe_max());
        runtime_a.disable_fs_native_urn_filter();
        runtime_b.disable_fs_native_urn_filter();

        // Same deploy body + same rand seed → deterministic
        // state-hash derivation MUST produce byte-identical
        // roots on both sides.
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                oc, w1
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "aa".hexToBytes(), *w1) |
                for (@_ <- w1) {{ Nil }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let rand = Blake2b512Random::create_from_bytes(&[42; 32]);

        for (label, rt) in [("A", &mut runtime_a), ("B", &mut runtime_b)] {
            rt.evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand.clone(),
            )
            .await
            .unwrap_or_else(|e| panic!("M-11: runtime {label} evaluate failed: {e}"));
        }

        let root_a = runtime_a.create_checkpoint().await.root;
        let root_b = runtime_b.create_checkpoint().await.root;

        assert_eq!(
            root_a, root_b,
            "M-11: two independent runtimes processing the same fs-native deploy \
             produced different state roots.  The Phase-7 bedrock claim (same \
             input → same state root across validators) is broken — one of \
             fs_open, fs_write, WAL routing, or the state-hash derivation is \
             consuming non-deterministic input."
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    // Merge (2026-09-03): cost-accounted renamed / relocated several
    // types; import them explicitly for the HEAD-added tests below
    // rather than relying on a `use super::*` glob alone.
    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Cosigned;
    use models::rhoapi::PCost;
    use models::rust::block::state_hash::StateHash;
    use models::rust::casper::protocol::casper_message::{
        Body, DeployAdmissionStatus, Event, F1r3flyState, Header, ProcessedDeploy, ProduceEvent,
        SystemDeployData,
    };
    use models::rust::deploy_id::{DeployIdV6, DeployLookupId};
    use proptest::prelude::*;
    use prost::bytes::Bytes;
    use tokio::sync::Semaphore;

    use super::*;
    use crate::rust::errors::CasperError;
    use crate::rust::util::construct_deploy;
    use crate::rust::util::rholang::replay_failure::ReplayFailure;

    fn deploy_data() -> DeployData {
        DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 0,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        }
    }

    fn signed_deploy() -> Signed<DeployData> {
        let alg: Box<dyn SignaturesAlg> = Box::new(Secp256k1);
        let (sk, _) = alg.new_key_pair();
        Signed::create(deploy_data(), alg, sk).expect("signed deploy")
    }

    fn produce_event(tag: u8) -> Event {
        Event::Produce(ProduceEvent {
            channels_hash: vec![tag].into(),
            hash: vec![tag, tag].into(),
            persistent: false,
            times_repeated: 0,
            is_deterministic: true,
            output_value: vec![vec![tag, tag, tag].into()],
            failed: false,
        })
    }

    fn state_bound_admission(block_data: BlockData) -> StateBoundAdmission {
        StateBoundAdmission {
            pre_state: vec![1; 32].into(),
            block_data,
            invalid_blocks: HashMap::new(),
            outcome: crate::rust::util::rholang::acceptance::AdmissionOutcome::default(),
            evidence: Arc::from(Vec::<ProcessedDeploy>::new()),
            user_post_state: vec![1; 32].into(),
            user_mergeable: Arc::from(Vec::<NumberChannelsEndVal>::new()),
            fs_wal: Arc::from(Vec::<rholang::rust::interpreter::io::wal::WalEntry>::new()),
            candidate_ids: Arc::from(Vec::<models::rust::deploy_id::DeployLookupId>::new()),
        }
    }

    #[test]
    fn state_bound_admission_matches_only_its_exact_block_context() {
        let block_data = BlockData {
            time_stamp: 11,
            block_number: 12,
            sender: PublicKey::from_bytes(&[13, 14]),
            seq_num: 15,
        };
        let admission = state_bound_admission(block_data.clone());
        let invalid_blocks = HashMap::new();

        assert!(admission.matches_context(&block_data, &invalid_blocks));

        let mut changed = block_data.clone();
        changed.time_stamp += 1;
        assert!(!admission.matches_context(&changed, &invalid_blocks));

        let mut changed = block_data.clone();
        changed.block_number += 1;
        assert!(!admission.matches_context(&changed, &invalid_blocks));

        let mut changed = block_data.clone();
        changed.sender = PublicKey::from_bytes(&[15, 16]);
        assert!(!admission.matches_context(&changed, &invalid_blocks));

        let mut changed = block_data.clone();
        changed.seq_num += 1;
        assert!(!admission.matches_context(&changed, &invalid_blocks));

        let mut changed_invalid_blocks = HashMap::new();
        changed_invalid_blocks.insert(vec![17; 32].into(), vec![18, 19].into());
        assert!(!admission.matches_context(&block_data, &changed_invalid_blocks));
    }

    #[test]
    fn state_bound_admission_preserves_its_exact_pre_state() {
        let admission = state_bound_admission(BlockData::empty());

        assert_eq!(admission.pre_state(), &StateHash::from(vec![1; 32]));
        assert_ne!(admission.pre_state(), &StateHash::from(vec![2; 32]));
    }

    /// Item (d-2) regression pin (2026-08-28): the cosigned admitted-
    /// checkpoint path publishes the aggregated per-block WAL slice
    /// into `pending_wal_slices` keyed by the block's final post-
    /// state-hash.  Without this insert, the LFB-snapshot writer's
    /// input starves for every cosigned block, breaking joiner
    /// reconstruction end-to-end (see `pb_m_14_two_validator_e2e`).
    ///
    /// Companion pin to
    /// `state_bound_cost_evidence_for_state_cosigned_aggregates_fs_wal`
    /// in `runtime.rs` — that pin guards the aggregation; this one
    /// guards the publish.
    #[test]
    fn compute_state_with_bonds_cosigned_admitted_publishes_pending_wal_slice() {
        let src = include_str!("runtime_manager.rs");
        let start_idx = src
            .find("pub async fn compute_state_with_bonds_cosigned_admitted")
            .expect("compute_state_with_bonds_cosigned_admitted must exist in this file");
        let end_marker = "Ok((state_hash, usr_processed, sys_processed, bonds))";
        let body_end = src[start_idx..].find(end_marker).expect(
            "terminal return `Ok((state_hash, usr_processed, sys_processed, bonds))` \
             must exist inside compute_state_with_bonds_cosigned_admitted",
        );
        let body = &src[start_idx..start_idx + body_end];
        assert!(
            body.contains("self.pending_wal_slices.write().await"),
            "compute_state_with_bonds_cosigned_admitted must acquire a write \
             lock on `pending_wal_slices` to publish the block's aggregated \
             WAL slice (item d-2)"
        );
        assert!(
            body.contains("state_hash.to_vec()"),
            "compute_state_with_bonds_cosigned_admitted must key the \
             `pending_wal_slices` insert by the block's FINAL post-state-hash \
             (`state_hash`, computed after system deploys land), NOT the \
             intermediate user_post_state.  Wrong key → finalization runner \
             lookup by `block.body.state.post_state_hash` misses the slice."
        );
        assert!(
            body.contains("MAX_PENDING_WAL_SLICES"),
            "compute_state_with_bonds_cosigned_admitted must apply the same \
             eviction cap the legacy `play_deploys_for_state` uses \
             (defense-in-depth against deep-fork or stalled-finalizer \
             pending-slice accumulation)"
        );
    }

    fn close() -> super::super::system_deploy_enum::SystemDeployEnum {
        super::super::system_deploy_enum::SystemDeployEnum::Close(
            crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy::new(
                Blake2b512Random::create_from_bytes(&[1]),
            ),
        )
    }

    fn slash() -> super::super::system_deploy_enum::SystemDeployEnum {
        super::super::system_deploy_enum::SystemDeployEnum::Slash(
            crate::rust::util::rholang::costacc::slash_deploy::SlashDeploy {
                invalid_block_hash: vec![2; 32].into(),
                equivocation_block_hash: None,
                pk: PublicKey::from_bytes(&[3]),
                target_activation_epoch: 4,
                target_bond_generation: models::rust::bond_generation::BondGeneration::GENESIS,
                initial_rand: Blake2b512Random::create_from_bytes(&[5]),
            },
        )
    }

    #[test]
    fn ordinary_checkpoint_synthesizes_and_validates_terminal_close() {
        let block_data = BlockData {
            time_stamp: 1,
            block_number: 2,
            sender: PublicKey::from_bytes(&[3]),
            seq_num: 4,
        };
        let mut empty = Vec::new();
        ensure_terminal_close(&mut empty, &block_data).unwrap();
        assert_eq!(empty.len(), 1);
        assert!(empty[0].as_close().is_some());

        let mut terminal = vec![slash(), close()];
        ensure_terminal_close(&mut terminal, &block_data).unwrap();

        let mut nonterminal = vec![close(), slash()];
        assert!(ensure_terminal_close(&mut nonterminal, &block_data).is_err());

        let mut duplicate = vec![close(), close()];
        assert!(ensure_terminal_close(&mut duplicate, &block_data).is_err());
    }

    #[test]
    fn state_bound_admission_retains_the_complete_execution_witness() {
        let mut witness = processed_deploy(signed_deploy(), 3, vec![produce_event(1)]);
        witness.pre_state_hash = vec![2; 32].into();
        witness.post_state_hash = vec![3; 32].into();
        let admission = StateBoundAdmission {
            pre_state: vec![2; 32].into(),
            block_data: BlockData::empty(),
            invalid_blocks: HashMap::new(),
            outcome: crate::rust::util::rholang::acceptance::AdmissionOutcome::default(),
            evidence: Arc::from(vec![witness.clone()]),
            user_post_state: witness.post_state_hash.clone(),
            user_mergeable: Arc::from(vec![NumberChannelsEndVal::new()]),
            fs_wal: Arc::from(Vec::<rholang::rust::interpreter::io::wal::WalEntry>::new()),
            candidate_ids: Arc::from(Vec::<models::rust::deploy_id::DeployLookupId>::new()),
        };

        assert_eq!(admission.evidence.as_ref(), std::slice::from_ref(&witness));
        assert_eq!(admission.user_post_state, witness.post_state_hash);
        assert_eq!(admission.user_mergeable.len(), 1);
    }

    fn processed_deploy(
        deploy: Signed<DeployData>,
        cost: u64,
        deploy_log: Vec<Event>,
    ) -> ProcessedDeploy {
        ProcessedDeploy {
            deploy,
            envelope_commitment: Vec::<u8>::new().into(),
            cost: PCost { cost },
            deploy_log,
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            cosigner_threshold: 0,
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
            authority_funding_certificate: None,
            authority_cost_witness: None,
            admission_status: Default::default(),
        }
    }

    fn processed_deploy_with_authority_byte_event(cost: u64, kind: i32) -> ProcessedDeploy {
        let mut processed = processed_deploy(signed_deploy(), cost, vec![produce_event(1)]);
        processed.authority_cost_witness = Some(models::casper::CostAuthorityWitnessProto {
            byte_events: vec![models::casper::CostAuthorityByteEventProto {
                event_id: vec![3; 32].into(),
                kind,
                authority: Some(models::rhoapi::CostAuthority::default()),
                amount: 5,
            }],
            ..Default::default()
        });
        processed
    }

    fn block_with_processed_deploy(deploy: ProcessedDeploy) -> BlockMessage {
        BlockMessage {
            block_hash: Vec::<u8>::new().into(),
            header: Header {
                parents_hash_list: Vec::new(),
                timestamp: 0,
                version: 1,
                extra_bytes: Vec::<u8>::new().into(),
                sender_bond_generation: Some(
                    models::rust::bond_generation::BondGeneration::GENESIS,
                ),
                objective_equivocation_evidence_delta: Vec::new(),
                finalized_floor: None,
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: vec![0; 32].into(),
                    post_state_hash: vec![1; 32].into(),
                    bonds: Vec::new(),
                    bond_generations: Vec::new(),
                    active_validators: Vec::new(),
                    block_number: 0,
                },
                deploys: vec![deploy],
                rejected_deploys: Vec::new(),
                rejected_state_effects: Vec::new(),
                applied_state_effects: Vec::new(),
                system_deploys: Vec::new(),
                extra_bytes: Vec::<u8>::new().into(),
                applied_from_scope: Vec::new(),
                merge_base: Vec::<u8>::new().into(),
            },
            justifications: Vec::new(),
            sender: vec![7].into(),
            seq_num: 0,
            sig: Vec::<u8>::new().into(),
            sig_algorithm: "secp256k1".to_string(),
            shard_id: "root".to_string(),
            extra_bytes: Vec::<u8>::new().into(),
            finalized_floor_certificate: None,
        }
    }

    fn slash_system_deploy(tag: u8) -> ProcessedSystemDeploy {
        ProcessedSystemDeploy::Succeeded {
            event_list: vec![produce_event(tag)],
            system_deploy: SystemDeployData::Slash {
                invalid_block_hash: vec![tag; 32].into(),
                equivocation_block_hash: None,
                issuer_public_key: PublicKey::from_bytes(&[tag, tag + 1]),
                target_activation_epoch: tag as i64,
                target_bond_generation: models::rust::bond_generation::BondGeneration::GENESIS,
            },
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        }
    }

    #[test]
    fn mergeable_key_binds_complete_execution_identity() {
        let block = block_with_processed_deploy(processed_deploy_with_authority_byte_event(3, 0));
        let base_key = RuntimeManager::mergeable_key_bytes_for_block(&block).unwrap();

        let mut changed = block.clone();
        changed.body.state.pre_state_hash = vec![2; 32].into();
        assert_ne!(
            base_key,
            RuntimeManager::mergeable_key_bytes_for_block(&changed).unwrap()
        );

        let mut changed = block.clone();
        changed.body.state.post_state_hash = vec![3; 32].into();
        assert_ne!(
            base_key,
            RuntimeManager::mergeable_key_bytes_for_block(&changed).unwrap()
        );

        let mut changed = block.clone();
        changed.body.deploys[0].cost.cost += 1;
        assert_ne!(
            base_key,
            RuntimeManager::mergeable_key_bytes_for_block(&changed).unwrap()
        );

        let mut changed = block.clone();
        changed.body.system_deploys.push(slash_system_deploy(9));
        assert_ne!(
            base_key,
            RuntimeManager::mergeable_key_bytes_for_block(&changed).unwrap()
        );

        let mut changed = block.clone();
        changed.sender = vec![8].into();
        assert_ne!(
            base_key,
            RuntimeManager::mergeable_key_bytes_for_block(&changed).unwrap()
        );

        let mut changed = block.clone();
        changed.seq_num += 1;
        assert_ne!(
            base_key,
            RuntimeManager::mergeable_key_bytes_for_block(&changed).unwrap()
        );

        let mut changed = block;
        changed.block_hash = vec![10; 32].into();
        assert_eq!(
            base_key,
            RuntimeManager::mergeable_key_bytes_for_block(&changed).unwrap()
        );
    }

    #[tokio::test]
    async fn replay_cache_publication_obeys_exact_event_log_limits() {
        use rholang::rust::interpreter::external_services::ExternalServices;
        use rholang::rust::interpreter::system_processes::BlockData;
        use rspace_plus_plus::rspace::rspace::RSpaceStore;
        use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
        use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

        use crate::rust::util::rholang::replay_cache::{
            ReplayCache, ReplayCacheContext, ReplayCacheKey,
        };

        let stores = RSpaceStore {
            history: Arc::new(InMemoryKeyValueStore::new()),
            roots: Arc::new(InMemoryKeyValueStore::new()),
            cold: Arc::new(InMemoryKeyValueStore::new()),
        };
        let (manager, _) = RuntimeManager::create_with_history(
            stores,
            KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            Arc::new(Default::default()),
            ExternalServices::noop(),
        );
        let key = ReplayCacheKey::new(
            vec![1; 32].into(),
            ReplayCacheContext::new(&BlockData::empty(), &Default::default()),
            vec![2; 32],
        );
        let cache = manager.replay_cache.as_ref().unwrap();
        let mut processed = execution_evidence();
        let event = processed.deploy_log[0].clone();
        let cap = RuntimeManager::max_replay_cache_event_log_entries();
        for count in [0, 1, cap, cap + 1] {
            cache.clear();
            processed.deploy_log = vec![event.clone(); count];
            manager.publish_replay_cache(
                key.clone(),
                &[processed.clone()],
                &[],
                &processed.post_state_hash,
            );
            let hit = cache.get(&key);
            assert_eq!(hit.is_some(), count > 0 && count <= cap);
            if let Some(hit) = hit {
                assert_eq!(hit.event_log.len(), count);
                assert_eq!(hit.post_state, processed.post_state_hash);
            }
        }
    }

    pub(super) fn execution_evidence() -> ProcessedDeploy {
        let signed = construct_deploy::source_deploy(
            "Nil".to_string(),
            1,
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(0),
            Some("root".to_string()),
        )
        .unwrap();
        let mut processed = ProcessedDeploy::empty(signed);
        processed.pre_state_hash = vec![2; 32].into();
        processed.post_state_hash = vec![3; 32].into();
        processed.cost.cost = 5;
        processed.deploy_log.push(Event::Produce(ProduceEvent {
            channels_hash: vec![4; 32].into(),
            hash: vec![5; 32].into(),
            persistent: false,
            times_repeated: 0,
            is_deterministic: true,
            output_value: vec![vec![6].into()],
            failed: false,
        }));
        processed.authority_funding_certificate =
            Some(models::casper::CostAuthorityFundingCertificateProto {
                protocol_version: 1,
                program_hash: vec![7; 32].into(),
                pre_state_root: vec![2; 32].into(),
                reservation_id: vec![8; 32].into(),
                byte_cost_schedule_version: 1,
                byte_cost_schedule_digest: vec![9; 32].into(),
                byte_cost_bound: 2,
                ..Default::default()
            });
        processed.authority_cost_witness = Some(models::casper::CostAuthorityWitnessProto {
            protocol_version: 1,
            certificate_id: vec![10; 32].into(),
            pre_state_root: vec![2; 32].into(),
            post_state_root: vec![3; 32].into(),
            byte_cost_schedule_version: 1,
            byte_cost_schedule_digest: vec![9; 32].into(),
            byte_cost: 2,
            ..Default::default()
        });
        processed
    }

    fn append_event(processed: &mut ProcessedDeploy, byte: u8) {
        processed.deploy_log.push(Event::Produce(ProduceEvent {
            channels_hash: vec![byte; 32].into(),
            hash: vec![byte.wrapping_add(1); 32].into(),
            persistent: true,
            times_repeated: 1,
            is_deterministic: false,
            output_value: vec![vec![byte].into()],
            failed: false,
        }));
    }

    proptest! {
        #[test]
        fn replay_payload_identity_binds_selected_processed_fields(axis in 0u8..17) {
            let original = execution_evidence();
            let original_hash = RuntimeManager::replay_payload_hash(&[original.clone()], &[], false);
            let mut changed = original;
            match axis {
                0 => changed.deploy.data.term.push_str(" | Nil"),
                1 => changed.deploy.sig = vec![11; 64].into(),
                2 => changed.cost.cost += 1,
                3 => append_event(&mut changed, 13),
                4 => changed.is_failed = true,
                5 => changed.system_deploy_error = Some("changed".to_string()),
                6 => changed.cosigner_threshold += 1,
                7 => changed.pre_state_hash = vec![14; 32].into(),
                8 => changed.post_state_hash = vec![15; 32].into(),
                9 => changed.authority_funding_certificate.as_mut().unwrap().protocol_version += 1,
                10 => changed.authority_funding_certificate.as_mut().unwrap().byte_cost_schedule_digest = vec![16; 32].into(),
                11 => changed.authority_funding_certificate.as_mut().unwrap().byte_cost_bound += 1,
                12 => changed.authority_cost_witness.as_mut().unwrap().protocol_version += 1,
                13 => changed.authority_cost_witness.as_mut().unwrap().byte_cost_schedule_version += 1,
                14 => changed.authority_cost_witness.as_mut().unwrap().events.push(Default::default()),
                15 => changed.authority_cost_witness.as_mut().unwrap().realized.push(Default::default()),
                16 => changed.admission_status = DeployAdmissionStatus::Rejected,
                _ => unreachable!(),
            }
            prop_assert_ne!(
                RuntimeManager::replay_payload_hash(&[changed], &[], false),
                original_hash
            );
        }

        #[test]
        fn canonical_rejection_rejects_every_noncanonical_field(axis in 0u8..10) {
            let signed = construct_deploy::source_deploy(
                "Nil".to_string(),
                1,
                None,
                None,
                Some(construct_deploy::DEFAULT_SEC.clone()),
                Some(0),
                Some("root".to_string()),
            ).unwrap();
            let candidate = Cosigned::create_single_envelope(
                signed.data,
                signed.sig_algorithm,
                construct_deploy::DEFAULT_SEC.clone(),
            ).unwrap();
            let pre_state: StateHash = vec![21; 32].into();
            let mut changed = ProcessedDeploy::admission_rejected(&candidate, pre_state.clone());
            match axis {
                0 => changed.cost.cost = 1,
                1 => append_event(&mut changed, 22),
                2 => changed.is_failed = false,
                3 => changed.system_deploy_error = Some("changed".to_string()),
                4 => changed.pre_state_hash = vec![23; 32].into(),
                5 => changed.post_state_hash = vec![24; 32].into(),
                6 => changed.authority_funding_certificate = Some(Default::default()),
                7 => changed.authority_cost_witness = Some(Default::default()),
                8 => changed.admission_status = DeployAdmissionStatus::Executed,
                9 => changed.envelope_commitment = vec![25; 32].into(),
                _ => unreachable!(),
            }
            prop_assert!(!is_canonical_admission_rejection(&changed, &candidate, &pre_state));
        }

        #[test]
        fn replay_payload_event_order_is_canonical_but_multiplicity_is_bound(
            first in 30u8..120,
            second in 121u8..220,
        ) {
            let mut forward = execution_evidence();
            forward.deploy_log.clear();
            append_event(&mut forward, first);
            append_event(&mut forward, second);
            let mut reverse = forward.clone();
            reverse.deploy_log.reverse();
            let canonical_hash = RuntimeManager::replay_payload_hash(&[forward.clone()], &[], false);
            prop_assert_eq!(
                canonical_hash.clone(),
                RuntimeManager::replay_payload_hash(&[reverse], &[], false)
            );
            append_event(&mut forward, first);
            prop_assert_ne!(
                canonical_hash,
                RuntimeManager::replay_payload_hash(&[forward], &[], false)
            );
        }

        #[test]
        fn state_bound_partitions_are_complete_disjoint_and_canonically_ordered(
            classes in proptest::collection::vec(0u8..3, 0..32),
        ) {
            let candidate_ids = (0..classes.len())
                .map(|index| {
                    let bytes = [u8::try_from(index + 1).unwrap(); 32];
                    DeployLookupId::V6(DeployIdV6::try_from(bytes.as_slice()).unwrap())
                })
                .collect::<Vec<_>>();
            let class = |wanted| {
                candidate_ids
                    .iter()
                    .zip(classes.iter())
                    .filter(|(_, actual)| **actual == wanted)
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>()
            };
            let admitted = class(0);
            let rejected = class(1);
            let deferred = class(2);

            prop_assert!(validate_state_bound_admission_partition(
                &candidate_ids,
                &admitted,
                &rejected,
                &deferred,
            ).is_ok());

            if let Some(first) = candidate_ids.first() {
                let mut duplicate = rejected.clone();
                duplicate.push(first.clone());
                prop_assert!(validate_state_bound_admission_partition(
                    &candidate_ids,
                    &admitted,
                    &duplicate,
                    &deferred,
                ).is_err());

                let mut missing_admitted = admitted.clone();
                let mut missing_rejected = rejected.clone();
                let mut missing_deferred = deferred.clone();
                if let Some(position) = missing_admitted.iter().position(|id| id == first) {
                    missing_admitted.remove(position);
                } else if let Some(position) = missing_rejected.iter().position(|id| id == first) {
                    missing_rejected.remove(position);
                } else if let Some(position) = missing_deferred.iter().position(|id| id == first) {
                    missing_deferred.remove(position);
                }
                prop_assert!(validate_state_bound_admission_partition(
                    &candidate_ids,
                    &missing_admitted,
                    &missing_rejected,
                    &missing_deferred,
                ).is_err());
            }

            if admitted.len() > 1 {
                let mut reordered = admitted.clone();
                reordered.reverse();
                prop_assert!(validate_state_bound_admission_partition(
                    &candidate_ids,
                    &reordered,
                    &rejected,
                    &deferred,
                ).is_err());
            }
            if rejected.len() > 1 {
                let mut reordered = rejected.clone();
                reordered.reverse();
                prop_assert!(validate_state_bound_admission_partition(
                    &candidate_ids,
                    &admitted,
                    &reordered,
                    &deferred,
                ).is_err());
            }
            if deferred.len() > 1 {
                let mut reordered = deferred.clone();
                reordered.reverse();
                prop_assert!(validate_state_bound_admission_partition(
                    &candidate_ids,
                    &admitted,
                    &rejected,
                    &reordered,
                ).is_err());
            }
        }

        #[test]
        fn parent_cache_key_canonicalizes_only_secondary_parents(
            main in any::<[u8; 32]>(),
            mut secondary in proptest::collection::vec(any::<[u8; 32]>(), 1..8),
        ) {
            let main = Bytes::copy_from_slice(&main);
            let secondary = secondary
                .drain(..)
                .map(|hash| Bytes::copy_from_slice(&hash))
                .collect::<Vec<_>>();
            let mut permuted = secondary.clone();
            permuted.reverse();
            let lfb = Bytes::from_static(&[9; 32]);

            let original = ParentsPostStateCacheKey::new(
                main.clone(),
                secondary.clone(),
                lfb.clone(),
                Vec::new(),
                false,
                false,
            );
            let reordered = ParentsPostStateCacheKey::new(
                main.clone(),
                permuted,
                lfb.clone(),
                Vec::new(),
                false,
                false,
            );
            prop_assert_eq!(&original, &reordered);

            if secondary[0] != main {
                let mut swapped_secondary = secondary[1..].to_vec();
                swapped_secondary.push(main);
                let swapped = ParentsPostStateCacheKey::new(
                    secondary[0].clone(),
                    swapped_secondary,
                    lfb,
                    Vec::new(),
                    false,
                    false,
                );
                prop_assert_ne!(original, swapped);
            }
        }

        #[test]
        fn equal_replay_post_state_is_accepted(
            root in any::<[u8; 32]>(),
            block_hash in any::<[u8; 32]>(),
        ) {
            let root = Bytes::copy_from_slice(&root);
            let block_hash = Bytes::copy_from_slice(&block_hash);

            prop_assert!(RuntimeManager::validate_replayed_post_state(
                &block_hash,
                &root,
                &root,
            ).is_ok());
        }

        #[test]
        fn unequal_replay_post_state_is_rejected_before_publication(
            declared in any::<[u8; 32]>(),
            block_hash in any::<[u8; 32]>(),
            index in 0usize..32,
            difference in 1u8..=u8::MAX,
        ) {
            let declared = Bytes::copy_from_slice(&declared);
            let mut computed = declared.to_vec();
            computed[index] ^= difference;
            let computed = Bytes::from(computed);
            let block_hash = Bytes::copy_from_slice(&block_hash);

            let result = RuntimeManager::validate_replayed_post_state(
                &block_hash,
                &declared,
                &computed,
            );
            let rejected = matches!(
                result,
                Err(CasperError::ReplayFailure(ReplayFailure::EffectStateMismatch {
                    boundary,
                    ..
                })) if boundary == "final-post-state"
            );
            prop_assert!(rejected);
        }
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

    #[tokio::test]
    async fn consensus_replay_has_priority_over_queued_reporting() {
        let lock = Arc::new(ReplayLock::new());
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
        let lock = Arc::new(ReplayLock::new());
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
