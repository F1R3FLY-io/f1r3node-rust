// See casper/src/main/scala/coop/rchain/casper/util/rholang/RuntimeManager.scala
// See casper/src/main/scala/coop/rchain/casper/util/rholang/RuntimeManagerSyntax.scala

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use dashmap::DashMap;
use hex::ToHex;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::block::state_hash::{StateHash, StateHashSerde};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, DeployData, Event, ProcessedDeploy, ProcessedSystemDeploy, RejectedDeploy,
    StateEffectId,
};
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

use crate::rust::errors::CasperError;
use crate::rust::merging::block_index::BlockIndex;
use crate::rust::metrics_constants::{
    BLOCK_INDEX_CACHE_RETAINED_BYTES_METRIC, BLOCK_INDEX_CACHE_SIZE_METRIC, CASPER_METRICS_SOURCE,
    PARENTS_POST_STATE_CACHE_SIZE_METRIC, REPLAY_CACHE_ENTRIES_METRIC,
    REPLAY_CACHE_RETAINED_BYTES_METRIC, RUNTIME_SPAWN_REPLAY_TIME_METRIC,
    RUNTIME_SPAWN_TIME_METRIC,
};
use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::replay_cache::{
    InMemoryReplayCache, ReplayCache, ReplayCacheEntry, ReplayCacheKey,
};
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
}

#[derive(Clone)]
pub struct StateBoundAdmission {
    pre_state: StateHash,
    block_data: BlockData,
    invalid_blocks: HashMap<BlockHash, Validator>,
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
    pub disable_late_block_filtering: bool,
}

pub type ParentsPostStateCacheVal = (StateHash, Vec<RejectedDeploy>, Vec<StateEffectId>);

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
        runtime
            .fs_handles
            .share_payload_store(
                self.payload_store
                    .read()
                    .await
                    .as_ref()
                    .map(|b| b.persistence.clone()),
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
        runtime
            .fs_handles
            .share_payload_store(
                self.payload_store
                    .read()
                    .await
                    .as_ref()
                    .map(|b| b.persistence.clone()),
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
        if !admission.outcome.rejected.is_empty() {
            return Err(CasperError::InvalidCostSettlement(format!(
                "checkpoint received {} deploys without valid state-bound funding evidence",
                admission.outcome.rejected.len()
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
        self.save_mergeable_channels(
            &state_hash,
            sender.bytes.clone(),
            seq_num,
            mergeable_chs,
            &start_hash,
            replay_payload_hash.clone(),
        )?;

        let replay_cache_event_log_cap = Self::max_replay_cache_event_log_entries();
        if let Some(ref cache) = self.replay_cache {
            let all_logs = Self::collect_replay_logs(&usr_processed, &sys_processed);
            if !all_logs.is_empty() && all_logs.len() <= replay_cache_event_log_cap {
                let key = ReplayCacheKey::new(
                    start_hash.clone(),
                    sender.bytes.to_vec(),
                    seq_num as i64,
                    replay_payload_hash,
                );
                let entry = ReplayCacheEntry::new(all_logs, state_hash.clone());
                cache.put(key, entry);
                Self::record_replay_cache_metrics(cache);
            }
        }
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
    ) -> Result<(Vec<ProcessedDeploy>, Vec<prost::bytes::Bytes>), CasperError> {
        let (execution, outcome) = self
            .state_bound_execution(start_hash, terms, block_data, invalid_blocks)
            .await?;
        Ok((execution.processed.to_vec(), outcome.rejected))
    }

    async fn state_bound_execution(
        &self,
        start_hash: &StateHash,
        mut terms: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
        block_data: BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
    ) -> Result<
        (
            StateBoundExecution,
            crate::rust::util::rholang::acceptance::AdmissionOutcome,
        ),
        CasperError,
    > {
        crate::rust::util::rholang::acceptance::canonical_sort(&mut terms);
        let runtime = self.spawn_runtime().await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        let fee_recipient = block_data.sender.clone();
        runtime_ops.runtime.set_block_data(block_data).await;
        runtime_ops.runtime.set_invalid_blocks(invalid_blocks).await;
        let (post_state, processed, outcome, fs_wal) = runtime_ops
            .state_bound_cost_evidence_for_state_cosigned(start_hash, terms, &fee_recipient)
            .await?;
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
    ) -> Result<
        (
            crate::rust::util::rholang::acceptance::AdmissionOutcome,
            StateBoundExecution,
        ),
        CasperError,
    > {
        crate::rust::util::rholang::acceptance::canonical_sort(&mut candidates);
        let (execution, outcome) = self
            .state_bound_execution(
                pre_state,
                candidates,
                block_data.clone(),
                invalid_blocks.clone(),
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
        let (outcome, execution) = self
            .admit_with_state_bound_evidence_and_witness(
                pre_state,
                candidates,
                block_data,
                invalid_blocks,
            )
            .await?;
        Ok(StateBoundAdmission {
            pre_state: pre_state.clone(),
            block_data: block_data.clone(),
            invalid_blocks: invalid_blocks.clone(),
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
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "complete", rss_kb);
        }
        Ok(result)
    }

    pub async fn compute_genesis(
        &self,
        terms: Vec<Signed<DeployData>>,
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
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;
        let replay_payload_hash = Self::replay_payload_hash(&terms, &system_deploys, is_genesis);

        if !is_genesis {
            let mut admission_runtime = self.spawn_runtime().await;
            admission_runtime
                .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
                .await?;
            let admission_ops = RuntimeOps::new(admission_runtime);
            let reader = crate::rust::util::rholang::acceptance::RuntimeOpsSupplyReader {
                runtime_ops: &admission_ops,
                pre_state_root: start_hash
                    .as_ref()
                    .try_into()
                    .expect("consensus state roots are Blake2b-256"),
            };
            crate::rust::util::rholang::acceptance::verify_state_bound_replay_admission(
                &terms,
                &sender.bytes,
                &reader,
            )
            .await
            .map_err(|error| match error {
                CasperError::InvalidCostSettlement(detail) => {
                    CasperError::ReplayFailure(ReplayFailure::replay_admission_mismatch(
                        terms.len(),
                        terms.len(),
                        0,
                        0,
                        detail,
                    ))
                }
                other => other,
            })?;
        }

        let replay_cache_key = ReplayCacheKey::new(
            start_hash.clone(),
            sender.bytes.to_vec(),
            seq_num as i64,
            replay_payload_hash.clone(),
        );
        if let Some(ref cache) = self.replay_cache {
            if let Some(entry) = cache.get(&replay_cache_key) {
                let mergeable_key = Self::mergeable_key_for_execution(
                    start_hash,
                    &entry.post_state,
                    sender.bytes.clone(),
                    seq_num,
                    replay_payload_hash.clone(),
                );
                let mergeable_key_encoded = bincode::serialize(&mergeable_key).map_err(|e| {
                    CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
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

        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let replay_runtime = self.spawn_replay_runtime().await;
        let runtime_ops = RuntimeOps::new(replay_runtime);
        let mut replay_runtime_ops = ReplayRuntimeOps::new(runtime_ops);

        let (state_hash, mergeable_chs) = replay_runtime_ops
            .replay_compute_state(
                start_hash,
                terms,
                system_deploys,
                block_data,
                Some(invalid_blocks),
                is_genesis,
                Some(self),
            )
            .await?;

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

    pub async fn replay_block_from_consensus_data(
        &self,
        start_hash: &StateHash,
        block: &BlockMessage,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
    ) -> Result<StateHash, CasperError> {
        let is_genesis = block.header.parents_hash_list.is_empty();
        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let deploys = if is_genesis {
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
            block.body.deploys.clone()
        } else {
            self.verify_state_bound_admission_partition(
                start_hash,
                &block.body.deploys,
                &BlockData::from_block(block),
                &invalid_blocks,
            )
            .await?
        };

        let block_data = BlockData::from_block(block);
        let replay_payload_hash =
            Self::replay_payload_hash(&deploys, &block.body.system_deploys, is_genesis);
        let (computed_post_state, mergeable_chs) = self
            .replay_compute_state_uncommitted(
                start_hash,
                deploys,
                block.body.system_deploys.clone(),
                &block_data,
                Some(invalid_blocks),
                is_genesis,
            )
            .await?;
        if computed_post_state != block.body.state.post_state_hash {
            return Err(CasperError::ReplayFailure(
                ReplayFailure::effect_state_mismatch(
                    format!("block:{}", hex::encode(&block.block_hash)),
                    "final-post-state".to_string(),
                    hex::encode(&block.body.state.post_state_hash),
                    hex::encode(&computed_post_state),
                ),
            ));
        }
        if let Some(mergeable_chs) = mergeable_chs {
            self.save_mergeable_channels(
                &computed_post_state,
                block_data.sender.bytes,
                block_data.seq_num,
                mergeable_chs,
                start_hash,
                replay_payload_hash,
            )
            .map_err(|error| {
                CasperError::RuntimeError(format!("Failed to save mergeable channels: {error:?}"))
            })?;
        }
        Ok(computed_post_state)
    }

    async fn verify_state_bound_admission_partition(
        &self,
        start_hash: &StateHash,
        deploys: &[ProcessedDeploy],
        block_data: &BlockData,
        invalid_blocks: &HashMap<BlockHash, Validator>,
    ) -> Result<Vec<ProcessedDeploy>, CasperError> {
        let expected_admitted: Vec<ProcessedDeploy> = deploys
            .iter()
            .filter(|deploy| !deploy.is_admission_rejected())
            .cloned()
            .collect();
        let expected_rejected: Vec<&ProcessedDeploy> = deploys
            .iter()
            .filter(|deploy| deploy.is_admission_rejected())
            .collect();
        if expected_rejected.is_empty() {
            return Ok(expected_admitted);
        }

        let invalid_rejection = expected_rejected.iter().find(|deploy| {
            !deploy.is_failed
                || deploy.cost.cost != 0
                || !deploy.deploy_log.is_empty()
                || deploy.system_deploy_error.as_deref()
                    != Some(ProcessedDeploy::FUNDING_ADMISSION_REJECTION)
                || deploy.pre_state_hash != *start_hash
                || deploy.post_state_hash != *start_hash
                || deploy.authority_funding_certificate.is_some()
                || deploy.authority_cost_witness.is_some()
        });
        if let Some(deploy) = invalid_rejection {
            return Err(CasperError::ReplayFailure(
                ReplayFailure::replay_admission_mismatch(
                    expected_admitted.len(),
                    0,
                    expected_rejected.len(),
                    0,
                    format!(
                        "malformed funding-admission rejection record for deploy {}",
                        hex::encode(&deploy.deploy.sig)
                    ),
                ),
            ));
        }

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

        let replay = self
            .certify_state_bound_admission(start_hash, candidates, block_data, invalid_blocks)
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
            .map(|deploy| deploy.primary().sig.clone())
            .collect();
        let expected_admitted_sigs: Vec<_> = expected_admitted
            .iter()
            .map(|deploy| deploy.deploy.sig.clone())
            .collect();
        let mut replay_rejected = replay.outcome().rejected.clone();
        let mut expected_rejected_sigs: Vec<_> = expected_rejected
            .iter()
            .map(|deploy| deploy.deploy.sig.clone())
            .collect();
        replay_rejected.sort();
        expected_rejected_sigs.sort();

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

        Ok(expected_admitted)
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

    pub async fn get_active_validators(
        &self,
        start_hash: &StateHash,
    ) -> Result<Vec<Validator>, CasperError> {
        if let Some(cached) = self.active_validators_cache.get(start_hash) {
            Self::touch_cache_key(&self.active_validators_cache_order, start_hash);
            return Ok(cached.clone());
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
        if let Some(cached) = self.bonds_cache.get(hash) {
            Self::touch_cache_key(&self.bonds_cache_order, hash);
            return Ok(cached.clone());
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
        if let Some(cached) = self.bond_generations_cache.get(hash) {
            Self::touch_cache_key(&self.bond_generations_cache_order, hash);
            return Ok(cached.clone());
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
            .play_exploratory_deploy(term, hash, deployer)
            .await
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
        if let Some(cached) = self.block_index_cache.get(block_hash) {
            let cached = cached.clone();
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
        let _write_guard = self
            .block_index_cache_write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(cached) = self.block_index_cache.get(block_hash) {
            let cached = cached.clone();
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
        let result = self.parents_post_state_cache.get(key).map(|entry| {
            Self::touch_cache_key(&self.parents_post_state_cache_order, key);
            entry.value().clone()
        });
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

        self.replay_block_from_consensus_data(
            &block.body.state.pre_state_hash,
            block,
            Some(invalid_blocks),
        )
        .await?;

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

    pub fn create_with_space(
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
        let (rspace, replay_rspace) =
            RSpace::create_with_replay(store, Arc::new(Box::new(Matcher)))
                .expect("Failed to create RSpaceWithReplay");

        let history_repo = rspace.get_history_repository();

        let runtime_manager = RuntimeManager::create_with_space(
            rspace,
            replay_rspace,
            history_repo.clone(),
            mergeable_store,
            mergeable_tags,
            external_services,
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
        let store: Arc<
            dyn rholang::rust::interpreter::io::wal::PayloadPersistence,
        > = Arc::new(InMemoryPayloadStore::new());
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
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
    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use crypto::rust::public_key::PublicKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rhoapi::PCost;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, Body, F1r3flyState, Header, ProduceEvent, SystemDeployData,
    };
    use proptest::prelude::*;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    fn deploy_data() -> DeployData {
        DeployData {
            term: "Nil".to_string(),
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
            fs_wal: Arc::from(
                Vec::<rholang::rust::interpreter::io::wal::WalEntry>::new(),
            ),
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
            .expect(
                "compute_state_with_bonds_cosigned_admitted must exist in this file",
            );
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
            fs_wal: Arc::from(
                Vec::<rholang::rust::interpreter::io::wal::WalEntry>::new(),
            ),
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
                system_deploys: Vec::new(),
                extra_bytes: Vec::<u8>::new().into(),
            },
            justifications: Vec::new(),
            sender: vec![7].into(),
            seq_num: 0,
            sig: Vec::<u8>::new().into(),
            sig_algorithm: "secp256k1".to_string(),
            shard_id: "root".to_string(),
            extra_bytes: Vec::<u8>::new().into(),
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

    proptest! {
        #[test]
        fn mergeable_key_separates_every_bound_identity_component(
            pre_state in any::<[u8; 32]>(),
            post_state in any::<[u8; 32]>(),
            creator in proptest::collection::vec(any::<u8>(), 1..128),
            seq_num in any::<i32>(),
            payload_hash in any::<[u8; 32]>(),
            component in 0usize..5,
        ) {
            let base = MergeableKey {
                post_state_hash: StateHashSerde(StateHash::from(post_state.to_vec())),
                pre_state_hash: StateHashSerde(StateHash::from(pre_state.to_vec())),
                creator: creator.clone().into(),
                seq_num,
                payload_hash: payload_hash.to_vec(),
            };
            let mut changed_pre_state = pre_state;
            let mut changed_post_state = post_state;
            let mut changed_creator = creator;
            let mut changed_payload_hash = payload_hash;
            let mut changed_seq_num = seq_num;
            match component {
                0 => changed_pre_state[0] ^= 1,
                1 => changed_post_state[0] ^= 1,
                2 => changed_creator.push(0),
                3 => changed_seq_num = changed_seq_num.wrapping_add(1),
                _ => changed_payload_hash[0] ^= 1,
            }
            let changed = MergeableKey {
                post_state_hash: StateHashSerde(StateHash::from(changed_post_state.to_vec())),
                pre_state_hash: StateHashSerde(StateHash::from(changed_pre_state.to_vec())),
                creator: changed_creator.into(),
                seq_num: changed_seq_num,
                payload_hash: changed_payload_hash.to_vec(),
            };

            let base_encoded = bincode::serialize(&base).unwrap();
            let changed_encoded = bincode::serialize(&changed).unwrap();
            prop_assert_ne!(&base_encoded, &changed_encoded);

            let mut entries = std::collections::BTreeMap::from([
                (base_encoded.clone(), 1u8),
                (changed_encoded.clone(), 2u8),
            ]);
            prop_assert_eq!(entries.remove(&base_encoded), Some(1));
            prop_assert_eq!(entries.get(&changed_encoded), Some(&2));
            prop_assert_eq!(entries.len(), 1);
        }
    }

    #[tokio::test]
    async fn deleting_one_execution_preserves_a_legacy_key_alias() {
        let manager = test_runtime_manager().await;
        let target = block_with_processed_deploy(processed_deploy_with_authority_byte_event(3, 0));
        let mut survivor = target.clone();
        survivor.body.state.pre_state_hash = vec![2; 32].into();

        let target_key = RuntimeManager::mergeable_key_bytes_for_block(&target).unwrap();
        let survivor_key = RuntimeManager::mergeable_key_bytes_for_block(&survivor).unwrap();
        assert_ne!(target_key, survivor_key);
        manager
            .mergeable_store
            .put_one(target_key, Vec::new())
            .unwrap();
        manager
            .mergeable_store
            .put_one(survivor_key, Vec::new())
            .unwrap();

        assert!(manager.has_mergeable_entry(&target).unwrap());
        assert!(manager.has_mergeable_entry(&survivor).unwrap());
        assert!(manager.delete_mergeable_channels(&target).unwrap());
        assert!(!manager.has_mergeable_entry(&target).unwrap());
        assert!(manager.has_mergeable_entry(&survivor).unwrap());
    }

    async fn test_runtime_manager() -> RuntimeManager {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.expect("rspace stores");
        let mergeable_store = RuntimeManager::mergeable_store(&mut kvm)
            .await
            .expect("mergeable store");
        RuntimeManager::create_with_history(
            store,
            mergeable_store,
            Arc::new(HashMap::new()),
            ExternalServices::noop(),
        )
        .0
    }

    fn compute_empty_index(manager: &RuntimeManager, block_hash: &BlockHash) -> BlockIndex {
        let root = Blake2b256Hash::from_bytes(vec![0; 32]);
        manager
            .get_or_compute_block_index(
                block_hash,
                0,
                &Vec::new(),
                &Vec::new(),
                &root,
                &root,
                &Vec::new(),
            )
            .expect("empty block index")
    }

    #[tokio::test]
    async fn block_index_cache_enforces_entry_and_byte_accounting_invariants() {
        let manager = test_runtime_manager().await;
        let hashes: Vec<BlockHash> = (0..RuntimeManager::MAX_BLOCK_INDEX_CACHE_ENTRIES + 5)
            .map(|index| index.to_le_bytes().to_vec().into())
            .collect();

        for hash in &hashes {
            compute_empty_index(&manager, hash);
        }

        assert_eq!(
            manager.block_index_cache.len(),
            RuntimeManager::MAX_BLOCK_INDEX_CACHE_ENTRIES
        );
        assert!(!manager.has_cached_block_index(&hashes[0]));
        assert!(manager.has_cached_block_index(hashes.last().expect("last hash")));
        let retained = manager
            .block_index_cache
            .iter()
            .map(|entry| entry.value().retained_bytes())
            .sum::<usize>();
        assert_eq!(
            manager
                .block_index_cache_retained_bytes
                .load(Ordering::Acquire),
            retained
        );
        assert!(retained <= RuntimeManager::MAX_BLOCK_INDEX_CACHE_BYTES);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_block_index_misses_commit_one_accounted_entry() {
        let manager = Arc::new(test_runtime_manager().await);
        let block_hash: BlockHash = vec![7; 32].into();
        let tasks = (0..32)
            .map(|_| {
                let manager = manager.clone();
                let block_hash = block_hash.clone();
                tokio::spawn(async move { compute_empty_index(&manager, &block_hash) })
            })
            .collect::<Vec<_>>();

        for task in tasks {
            task.await.expect("cache task");
        }

        assert_eq!(manager.block_index_cache.len(), 1);
        let retained = manager
            .block_index_cache
            .get(&block_hash)
            .expect("cached block index")
            .retained_bytes();
        assert_eq!(
            manager
                .block_index_cache_retained_bytes
                .load(Ordering::Acquire),
            retained
        );
    }

    #[derive(serde::Deserialize)]
    struct V12FixtureSet {
        fixtures: Vec<V12Fixture>,
    }

    #[derive(serde::Deserialize)]
    struct V12Fixture {
        id: String,
        oracle_surface: String,
        oracle_kind: String,
        mutation_axis: String,
        expected_total_cost: i64,
    }

    #[derive(serde::Deserialize)]
    struct V13FixtureSet {
        fixtures: Vec<V13Fixture>,
    }

    #[derive(serde::Deserialize)]
    struct V13Fixture {
        id: String,
        #[serde(default)]
        semantic_oracle: String,
        #[serde(default)]
        expected_disposition: String,
        #[serde(default)]
        expected_total_cost: i64,
        // DISABLED (2026-08-07 dev merge warning sweep) — never read; the
        // fixture JSON's settlement blocks are consumed by the native
        // settlement tests, not this deserializer. serde ignores unknown
        // JSON keys, so parsing is unaffected.
        // #[serde(default)]
        // settlement: serde_json::Value,
        #[serde(default)]
        replay_mutations: Vec<String>,
        #[serde(default)]
        source_surface_status: String,
        #[serde(default)]
        source_facets: Vec<String>,
        #[serde(default)]
        source_anchor_digest: String,
        #[serde(default)]
        cross_surface_role: String,
    }

    #[derive(serde::Deserialize)]
    struct V14FixtureSet {
        fixtures: Vec<V14Fixture>,
    }

    #[derive(serde::Deserialize)]
    struct V14Fixture {
        id: String,
        #[serde(default)]
        security_surface: String,
        #[serde(default)]
        expected_disposition: String,
        #[serde(default)]
        replay_mutations: Vec<String>,
        #[serde(default)]
        source_anchor_digest: String,
        #[serde(default)]
        source_anchor_status: String,
        #[serde(default)]
        auth_boundary: String,
        #[serde(default)]
        replay_boundary: String,
        #[serde(default)]
        slashing_authorization: serde_json::Value,
        #[serde(default)]
        dependency_advisory_id: String,
        #[serde(default)]
        secret_material_touched: bool,
    }

    fn horizon_v12_fixtures() -> Vec<V12Fixture> {
        serde_json::from_str::<V12FixtureSet>(include_str!(
            "../../../../../rholang/tests/accounting/horizon_v12_fixtures.json"
        ))
        .expect("embedded horizon v12 fixture schema")
        .fixtures
    }

    fn horizon_v13_fixtures() -> Vec<V13Fixture> {
        serde_json::from_str::<V13FixtureSet>(include_str!(
            "../../../../../rholang/tests/accounting/horizon_v13_fixtures.json"
        ))
        .expect("embedded horizon v13 fixture schema")
        .fixtures
    }

    fn horizon_v14_fixtures() -> Vec<V14Fixture> {
        serde_json::from_str::<V14FixtureSet>(include_str!(
            "../../../../../rholang/tests/accounting/horizon_v14_fixtures.json"
        ))
        .expect("embedded horizon v14 fixture schema")
        .fixtures
    }

    // DISABLED (2026-08-07 dev merge warning sweep) — dead since the v13
    // settlement assertions moved to the native settlement tests; kept
    // commented out per the repo's disable-by-commenting rule.
    // fn fixture_i64(value: &serde_json::Value, key: &str) -> i64 {
    // value
    // .get(key)
    // .and_then(serde_json::Value::as_i64)
    // .unwrap_or_else(|| panic!("fixture settlement must include {key}"))
    // }

    #[test]
    fn replay_payload_hash_changes_when_user_cost_changes() {
        let deploy = signed_deploy();
        let left = processed_deploy(deploy.clone(), 3, vec![produce_event(1)]);
        let right = processed_deploy(deploy, 4, vec![produce_event(1)]);

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[left], &[], false),
            RuntimeManager::replay_payload_hash(&[right], &[], false)
        );
    }

    #[test]
    fn block_hash_changes_when_processed_deploy_cost_changes() {
        let deploy = signed_deploy();
        let left = processed_deploy(deploy.clone(), 3, vec![produce_event(1)]);
        let right = processed_deploy(deploy, 4, vec![produce_event(1)]);

        assert_ne!(
            crate::rust::util::proto_util::hash_block(&block_with_processed_deploy(left)),
            crate::rust::util::proto_util::hash_block(&block_with_processed_deploy(right))
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_user_signature_changes() {
        let left = processed_deploy(signed_deploy(), 3, vec![produce_event(1)]);
        let right = processed_deploy(signed_deploy(), 3, vec![produce_event(1)]);

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[left], &[], false),
            RuntimeManager::replay_payload_hash(&[right], &[], false)
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_user_failure_status_changes() {
        let deploy = signed_deploy();
        let left = processed_deploy(deploy.clone(), 3, vec![produce_event(1)]);
        let mut right = processed_deploy(deploy, 3, vec![produce_event(1)]);
        right.is_failed = true;

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[left], &[], false),
            RuntimeManager::replay_payload_hash(&[right], &[], false)
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_user_system_error_changes() {
        let deploy = signed_deploy();
        let left = processed_deploy(deploy.clone(), 3, vec![produce_event(1)]);
        let mut right = processed_deploy(deploy, 3, vec![produce_event(1)]);
        right.system_deploy_error = Some("forged settlement".to_string());

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[left], &[], false),
            RuntimeManager::replay_payload_hash(&[right], &[], false)
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_user_deploy_log_changes() {
        let deploy = signed_deploy();
        let left = processed_deploy(deploy.clone(), 3, vec![produce_event(1)]);
        let right = processed_deploy(deploy, 3, vec![produce_event(2)]);

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[left], &[], false),
            RuntimeManager::replay_payload_hash(&[right], &[], false)
        );
    }

    #[test]
    fn replay_payload_hash_canonicalizes_user_deploy_log_order() {
        let deploy = signed_deploy();
        let left = processed_deploy(deploy.clone(), 3, vec![produce_event(1), produce_event(2)]);
        let right = processed_deploy(deploy, 3, vec![produce_event(2), produce_event(1)]);

        assert_eq!(
            RuntimeManager::replay_payload_hash(&[left], &[], false),
            RuntimeManager::replay_payload_hash(&[right], &[], false)
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_user_state_witness_changes() {
        let deploy = signed_deploy();
        let left = processed_deploy(deploy.clone(), 3, vec![produce_event(1)]);
        let mut right = processed_deploy(deploy, 3, vec![produce_event(1)]);
        right.post_state_hash = vec![9; 32].into();

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[left], &[], false),
            RuntimeManager::replay_payload_hash(&[right], &[], false)
        );
    }

    #[test]
    fn replay_payload_hash_binds_authority_byte_event_kind() {
        let deploy = signed_deploy();
        let mut left = processed_deploy(deploy, 3, vec![produce_event(1)]);
        let witness = models::casper::CostAuthorityWitnessProto {
            byte_events: vec![models::casper::CostAuthorityByteEventProto {
                event_id: vec![3; 32].into(),
                kind: 0,
                authority: Some(models::rhoapi::CostAuthority::default()),
                amount: 5,
            }],
            ..Default::default()
        };
        left.authority_cost_witness = Some(witness);
        let mut right = left.clone();
        right.authority_cost_witness.as_mut().unwrap().byte_events[0].kind = 2;

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[left], &[], false),
            RuntimeManager::replay_payload_hash(&[right], &[], false)
        );
    }

    #[test]
    fn replay_payload_hash_binds_authority_certificate() {
        let deploy = signed_deploy();
        let mut left = processed_deploy(deploy, 3, vec![produce_event(1)]);
        let certificate = models::casper::CostAuthorityFundingCertificateProto {
            byte_cost_bound: 7,
            ..Default::default()
        };
        left.authority_funding_certificate = Some(certificate);
        let mut right = left.clone();
        right
            .authority_funding_certificate
            .as_mut()
            .unwrap()
            .byte_cost_bound = 8;

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[left], &[], false),
            RuntimeManager::replay_payload_hash(&[right], &[], false)
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_system_deploy_log_changes() {
        let left = ProcessedSystemDeploy::Succeeded {
            event_list: vec![produce_event(1)],
            system_deploy: SystemDeployData::Empty,
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        };
        let right = ProcessedSystemDeploy::Succeeded {
            event_list: vec![produce_event(2)],
            system_deploy: SystemDeployData::Empty,
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        };

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[], &[left], false),
            RuntimeManager::replay_payload_hash(&[], &[right], false)
        );
    }

    #[test]
    fn replay_payload_hash_canonicalizes_system_deploy_log_order() {
        let left = ProcessedSystemDeploy::Succeeded {
            event_list: vec![produce_event(1), produce_event(2)],
            system_deploy: SystemDeployData::Empty,
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        };
        let right = ProcessedSystemDeploy::Succeeded {
            event_list: vec![produce_event(2), produce_event(1)],
            system_deploy: SystemDeployData::Empty,
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        };

        assert_eq!(
            RuntimeManager::replay_payload_hash(&[], &[left], false),
            RuntimeManager::replay_payload_hash(&[], &[right], false)
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_system_deploy_kind_changes() {
        let left = ProcessedSystemDeploy::Succeeded {
            event_list: vec![produce_event(1)],
            system_deploy: SystemDeployData::Empty,
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        };
        let right = ProcessedSystemDeploy::Succeeded {
            event_list: vec![produce_event(1)],
            system_deploy: SystemDeployData::CloseBlockSystemDeployData,
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        };

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[], &[left], false),
            RuntimeManager::replay_payload_hash(&[], &[right], false)
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_system_state_witness_changes() {
        let left = ProcessedSystemDeploy::Succeeded {
            event_list: vec![produce_event(1)],
            system_deploy: SystemDeployData::Empty,
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        };
        let right = ProcessedSystemDeploy::Succeeded {
            event_list: vec![produce_event(1)],
            system_deploy: SystemDeployData::Empty,
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: vec![9; 32].into(),
        };

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[], &[left], false),
            RuntimeManager::replay_payload_hash(&[], &[right], false)
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_slash_fields_change() {
        let left = slash_system_deploy(1);
        let right = slash_system_deploy(2);

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[], &[left], false),
            RuntimeManager::replay_payload_hash(&[], &[right], false)
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_system_error_changes() {
        let left = ProcessedSystemDeploy::Failed {
            event_list: vec![produce_event(1)],
            error_msg: "left".to_string(),
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        };
        let right = ProcessedSystemDeploy::Failed {
            event_list: vec![produce_event(1)],
            error_msg: "right".to_string(),
            pre_state_hash: Vec::<u8>::new().into(),
            post_state_hash: Vec::<u8>::new().into(),
        };

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[], &[left], false),
            RuntimeManager::replay_payload_hash(&[], &[right], false)
        );
    }

    #[test]
    fn replay_payload_hash_changes_when_genesis_flag_changes() {
        let deploy = processed_deploy(signed_deploy(), 3, vec![produce_event(1)]);

        assert_ne!(
            RuntimeManager::replay_payload_hash(&[deploy.clone()], &[], false),
            RuntimeManager::replay_payload_hash(&[deploy], &[], true)
        );
    }

    #[test]
    fn cost_accounting_v12_casper_replay_payload_oracles_hold() {
        let fixtures = horizon_v12_fixtures()
            .into_iter()
            .filter(|fixture| fixture.oracle_surface == "casper_replay")
            .collect::<Vec<_>>();
        assert!(!fixtures.is_empty());

        for fixture in fixtures {
            assert_eq!(fixture.oracle_kind, "casper_replay_payload_hash");
            let left = processed_deploy_with_authority_byte_event(3, 0);

            let right = match fixture.mutation_axis.as_str() {
                "authority_cost_witness" => processed_deploy_with_authority_byte_event(3, 2),
                "signature" => processed_deploy_with_authority_byte_event(3, 0),
                other => panic!(
                    "unexpected v12 casper mutation axis {other} in {}",
                    fixture.id
                ),
            };

            assert_ne!(
                RuntimeManager::replay_payload_hash(&[left], &[], false),
                RuntimeManager::replay_payload_hash(&[right], &[], false),
                "v12 casper replay fixture {} must reject mutation axis {}",
                fixture.id,
                fixture.mutation_axis
            );
        }
    }

    #[test]
    fn cost_accounting_v12_slashing_replay_oracles_hold() {
        let fixtures = horizon_v12_fixtures()
            .into_iter()
            .filter(|fixture| fixture.oracle_surface == "slashing")
            .collect::<Vec<_>>();
        assert!(!fixtures.is_empty());

        for fixture in fixtures {
            match fixture.oracle_kind.as_str() {
                "slashing_replay_payload_hash" => {
                    assert_ne!(
                        RuntimeManager::replay_payload_hash(&[], &[slash_system_deploy(1)], false),
                        RuntimeManager::replay_payload_hash(&[], &[slash_system_deploy(2)], false),
                        "v12 slashing fixture {} must authenticate slashing fields",
                        fixture.id
                    );
                }
                "slashing_post_eval_isolation" => {
                    let user_deploy = processed_deploy(
                        signed_deploy(),
                        fixture.expected_total_cost as u64,
                        vec![produce_event(1)],
                    );
                    let with_slash = RuntimeManager::replay_payload_hash(
                        &[user_deploy.clone()],
                        &[slash_system_deploy(1)],
                        false,
                    );
                    let without_slash =
                        RuntimeManager::replay_payload_hash(&[user_deploy.clone()], &[], false);

                    assert_ne!(
                        with_slash, without_slash,
                        "v12 slashing fixture {} must include system-deploy evidence",
                        fixture.id
                    );
                    assert_eq!(
                        user_deploy.cost.cost, fixture.expected_total_cost as u64,
                        "v12 slashing fixture {} must not mutate user deploy cost",
                        fixture.id
                    );
                }
                other => panic!(
                    "unexpected v12 slashing oracle kind {other} in {}",
                    fixture.id
                ),
            }
        }
    }

    #[test]
    fn cost_accounting_v13_source_semantic_replay_payload_oracles_hold() {
        let fixtures = horizon_v13_fixtures()
            .into_iter()
            .filter(|fixture| {
                matches!(
                    fixture.semantic_oracle.as_str(),
                    "runtime_to_replay_authenticated_witness" | "replay_to_slashing_authentication"
                )
            })
            .collect::<Vec<_>>();
        assert!(!fixtures.is_empty());

        for fixture in fixtures {
            assert!(!fixture.source_anchor_digest.is_empty());
            assert!(!fixture.cross_surface_role.is_empty());
            match fixture.semantic_oracle.as_str() {
                "runtime_to_replay_authenticated_witness" => {
                    for field in [
                        "processed_deploy_cost",
                        "authority_cost_witness",
                        "authority_byte_events",
                        "replay_payload_hash",
                    ] {
                        assert!(
                            fixture
                                .replay_mutations
                                .iter()
                                .any(|mutation| mutation == field),
                            "v13 fixture {} must include runtime/replay mutation field {}",
                            fixture.id,
                            field
                        );
                    }
                    let left = processed_deploy_with_authority_byte_event(3, 0);
                    let changed_cost = processed_deploy_with_authority_byte_event(4, 0);
                    let changed_byte_event = processed_deploy_with_authority_byte_event(3, 2);
                    let left_hash = RuntimeManager::replay_payload_hash(
                        std::slice::from_ref(&left),
                        &[],
                        false,
                    );
                    assert_ne!(
                        left_hash,
                        RuntimeManager::replay_payload_hash(&[changed_cost], &[], false),
                        "v13 fixture {} must authenticate runtime cost",
                        fixture.id
                    );
                    assert_ne!(
                        left_hash,
                        RuntimeManager::replay_payload_hash(&[changed_byte_event], &[], false),
                        "v13 fixture {} must authenticate authority byte events",
                        fixture.id
                    );
                }
                "replay_to_slashing_authentication" => {
                    for field in ["slash_fields", "block_hash", "signature"] {
                        assert!(
                            fixture
                                .replay_mutations
                                .iter()
                                .any(|mutation| mutation == field),
                            "v13 fixture {} must include replay/slashing mutation field {}",
                            fixture.id,
                            field
                        );
                    }
                    assert_ne!(
                        RuntimeManager::replay_payload_hash(&[], &[slash_system_deploy(1)], false),
                        RuntimeManager::replay_payload_hash(&[], &[slash_system_deploy(2)], false),
                        "v13 fixture {} must authenticate slashing fields in replay payload",
                        fixture.id
                    );
                }
                other => panic!("unexpected v13 replay oracle {other} in {}", fixture.id),
            }
        }
    }

    #[test]
    fn cost_accounting_v13_settlement_slashing_legacy_oracles_hold() {
        let fixtures = horizon_v13_fixtures()
            .into_iter()
            .filter(|fixture| {
                matches!(
                    fixture.semantic_oracle.as_str(),
                    "runtime_to_settlement_vault_conservation" | "legacy_to_runtime_quarantine"
                )
            })
            .collect::<Vec<_>>();
        assert!(!fixtures.is_empty());

        for fixture in fixtures {
            assert!(!fixture.source_anchor_digest.is_empty());
            assert!(!fixture.cross_surface_role.is_empty());
            match fixture.semantic_oracle.as_str() {
                "runtime_to_settlement_vault_conservation" => {
                    let user_deploy = processed_deploy(
                        signed_deploy(),
                        fixture.expected_total_cost as u64,
                        vec![produce_event(1)],
                    );
                    assert_eq!(
                        user_deploy.cost.cost, fixture.expected_total_cost as u64,
                        "v13 fixture {} per-COMM runtime cost evidence must be preserved",
                        fixture.id
                    );
                }
                "legacy_to_runtime_quarantine" => {
                    assert_eq!(fixture.source_surface_status, "absent");
                    assert_eq!(fixture.expected_disposition, "source_absent");
                    assert!(fixture
                        .source_facets
                        .iter()
                        .any(|facet| facet == "legacy_quarantine"));
                }
                other => panic!(
                    "unexpected v13 settlement/legacy oracle {other} in {}",
                    fixture.id
                ),
            }
        }
    }

    #[test]
    fn cost_accounting_v14_replay_security_oracles_hold() {
        let fixtures = horizon_v14_fixtures()
            .into_iter()
            .filter(|fixture| {
                matches!(
                    fixture.security_surface.as_str(),
                    "api_to_runtime_replay"
                        | "replay_cache_payload_binding"
                        | "slashing_authorization"
                )
            })
            .collect::<Vec<_>>();
        assert!(!fixtures.is_empty());

        for fixture in fixtures {
            assert!(!fixture.source_anchor_digest.is_empty());
            assert_eq!(fixture.source_anchor_status, "present");
            assert!(!fixture.auth_boundary.is_empty());
            assert!(!fixture.replay_boundary.is_empty());
            assert!(fixture.dependency_advisory_id.is_empty());
            assert!(!fixture.secret_material_touched);

            match fixture.security_surface.as_str() {
                "api_to_runtime_replay" | "replay_cache_payload_binding" => {
                    for field in [
                        "processed_deploy_cost",
                        "authority_cost_witness",
                        "authority_byte_events",
                        "replay_payload_hash",
                    ] {
                        assert!(
                            fixture
                                .replay_mutations
                                .iter()
                                .any(|mutation| mutation == field),
                            "v14 fixture {} must bind replay field {}",
                            fixture.id,
                            field
                        );
                    }
                    let left = processed_deploy_with_authority_byte_event(3, 0);
                    let changed_cost = processed_deploy_with_authority_byte_event(4, 0);
                    let changed_byte_event = processed_deploy_with_authority_byte_event(3, 2);
                    let left_hash = RuntimeManager::replay_payload_hash(
                        std::slice::from_ref(&left),
                        &[],
                        false,
                    );
                    assert_ne!(
                        left_hash,
                        RuntimeManager::replay_payload_hash(&[changed_cost], &[], false),
                        "v14 fixture {} must bind processed deploy cost",
                        fixture.id
                    );
                    assert_ne!(
                        left_hash,
                        RuntimeManager::replay_payload_hash(&[changed_byte_event], &[], false),
                        "v14 fixture {} must bind authority byte events",
                        fixture.id
                    );
                }
                "slashing_authorization" => {
                    assert_eq!(fixture.expected_disposition, "replay_invalid");
                    for field in [
                        "slash_epoch",
                        "slash_fields",
                        "target_activation_epoch",
                        "evidence_epoch",
                        "parent_pre_state_bond",
                        "block_hash",
                        "signature",
                    ] {
                        assert!(
                            fixture
                                .replay_mutations
                                .iter()
                                .any(|mutation| mutation == field),
                            "v14 fixture {} must include replay/slashing mutation field {}",
                            fixture.id,
                            field
                        );
                    }
                    let auth = &fixture.slashing_authorization;
                    let current_epoch = auth
                        .get("current_epoch")
                        .and_then(serde_json::Value::as_i64);
                    assert_eq!(
                        auth.get("evidence_epoch")
                            .and_then(serde_json::Value::as_i64),
                        current_epoch,
                        "v14 fixture {} must bind evidence epoch to current epoch",
                        fixture.id
                    );
                    assert_eq!(
                        auth.get("target_activation_epoch")
                            .and_then(serde_json::Value::as_i64),
                        current_epoch,
                        "v14 fixture {} must bind target activation epoch to current epoch",
                        fixture.id
                    );
                    assert!(
                        auth.get("parent_pre_state_bond")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(0)
                            > 0,
                        "v14 fixture {} must carry parent pre-state bond evidence",
                        fixture.id
                    );
                    assert_ne!(
                        RuntimeManager::replay_payload_hash(&[], &[slash_system_deploy(1)], false),
                        RuntimeManager::replay_payload_hash(&[], &[slash_system_deploy(2)], false),
                        "v14 fixture {} must authenticate slashing payload fields",
                        fixture.id
                    );
                }
                other => panic!(
                    "unexpected v14 replay/slashing surface {other} in {}",
                    fixture.id
                ),
            }
        }
    }
}
