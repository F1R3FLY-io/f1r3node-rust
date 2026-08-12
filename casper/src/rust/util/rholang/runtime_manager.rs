// See casper/src/main/scala/coop/rchain/casper/util/rholang/RuntimeManager.scala
// See casper/src/main/scala/coop/rchain/casper/util/rholang/RuntimeManagerSyntax.scala

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::signatures::signed::Signed;
use dashmap::DashMap;
use hex::ToHex;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::block::state_hash::{StateHash, StateHashSerde};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    Bond, DeployData, Event, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;
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
    BLOCK_INDEX_CACHE_SIZE_METRIC, CASPER_METRICS_SOURCE, PARENTS_POST_STATE_CACHE_SIZE_METRIC,
    RUNTIME_SPAWN_REPLAY_TIME_METRIC, RUNTIME_SPAWN_TIME_METRIC,
};
use crate::rust::rholang::replay_runtime::ReplayRuntimeOps;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::replay_cache::{
    InMemoryReplayCache, ReplayCache, ReplayCacheEntry, ReplayCacheKey,
};
use crate::rust::util::rholang::state_hash_cache::StateHashCache;

type MergeableStore = KeyValueTypedStoreImpl<ByteVector, Vec<DeployMergeableData>>;

#[derive(serde::Serialize, serde::Deserialize)]
struct MergeableKey {
    state_hash: StateHashSerde,
    #[serde(with = "shared::rust::serde_bytes")]
    creator: prost::bytes::Bytes,
    seq_num: i32,
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
    pub state_hash_cache: Option<Arc<StateHashCache>>,
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

pub type ParentsPostStateCacheVal = (
    StateHash,
    Vec<prost::bytes::Bytes>,
    Vec<crate::rust::merging::rejected_slash::RejectedSlash>,
);

impl RuntimeManager {
    const MAX_BLOCK_INDEX_CACHE_ENTRIES: usize = 128;
    const MAX_PARENTS_POST_STATE_CACHE_ENTRIES: usize = 64;
    const MAX_ACTIVE_VALIDATORS_CACHE_ENTRIES: usize = 256;
    const MAX_BONDS_CACHE_ENTRIES: usize = 64;
    const MAX_REPLAY_CACHE_ENTRIES: usize = 192;
    const MAX_REPLAY_CACHE_EVENT_LOG_ENTRIES: usize = 1_536;
    const MAX_STATE_HASH_CACHE_ENTRIES: usize = 0;

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

    fn max_replay_cache_event_log_entries() -> usize { Self::MAX_REPLAY_CACHE_EVENT_LOG_ENTRIES }

    fn max_state_hash_cache_entries() -> usize { Self::MAX_STATE_HASH_CACHE_ENTRIES }

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
        metrics::histogram!(RUNTIME_SPAWN_REPLAY_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());

        runtime
    }

    pub async fn compute_state(
        &self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
        system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
    ) -> Result<(StateHash, Vec<ProcessedDeploy>, Vec<ProcessedSystemDeploy>), CasperError> {
        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let runtime = self.spawn_runtime().await;
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
                cache.put(key, entry);
                tracing::debug!(
                    "[CACHE] Stored replay cache entry for sender seq={}",
                    seq_num
                );
            } else if !all_logs.is_empty() {
                tracing::debug!(
                    "[CACHE] Skipped replay cache store for sender seq={} (event_log={})",
                    seq_num,
                    all_logs.len()
                );
            }
        }

        // Cache state hash mapping for skip-replay optimization
        if let Some(ref cache) = self.state_hash_cache {
            cache.put(start_hash.clone(), state_hash.clone());
            tracing::debug!("[CACHE] Stored state hash mapping");
        }

        Ok((state_hash, usr_processed, sys_processed))
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

        let invalid_blocks = invalid_blocks.unwrap_or_default();
        let runtime = self.spawn_runtime().await;
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
                cache.put(key, entry);
                tracing::debug!(
                    "[CACHE] Stored replay cache entry for sender seq={}",
                    seq_num
                );
            } else if !all_logs.is_empty() {
                tracing::debug!(
                    "[CACHE] Skipped replay cache store for sender seq={} (event_log={})",
                    seq_num,
                    all_logs.len()
                );
            }
        }

        // Cache state hash mapping for skip-replay optimization
        if let Some(ref cache) = self.state_hash_cache {
            cache.put(start_hash.clone(), state_hash.clone());
            tracing::debug!("[CACHE] Stored state hash mapping");
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

        Ok((state_hash, usr_processed, sys_processed, bonds))
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
        let sender = block_data.sender.clone();
        let seq_num = block_data.seq_num;
        let replay_payload_hash = Self::replay_payload_hash(&terms, &system_deploys, is_genesis);

        // Step 1: Check state-hash cache.
        //
        // IMPORTANT:
        // `StateHashCache` is keyed only by pre-state, while mergeable channels are keyed by
        // (post-state, creator, seq-num). Returning early here can skip writing mergeable data
        // for a distinct block that shares the same pre-state, which later breaks
        // parent-post-state/index reconstruction with "Missing mergeable entry ...".
        //
        // We only fast-return on cache hit if mergeable entry already exists for this block key.
        // For empty blocks we can safely synthesize and persist an empty mergeable entry.
        // Otherwise, fall through to full replay to materialize mergeable data.
        if let Some(ref cache) = self.state_hash_cache {
            if let Some(cached_post) = cache.get(start_hash) {
                let mergeable_key = MergeableKey {
                    state_hash: StateHashSerde(cached_post.clone()),
                    creator: sender.bytes.clone(),
                    seq_num,
                };
                let mergeable_key_encoded = bincode::serialize(&mergeable_key).map_err(|e| {
                    CasperError::KvStoreError(KvStoreError::SerializationError(e.to_string()))
                })?;

                if self
                    .mergeable_store
                    .contains_key(mergeable_key_encoded.clone())?
                {
                    tracing::info!(
                        "[CACHE] StateHashCache hit: mergeable entry present, skipping full replay"
                    );
                    return Ok(cached_post);
                }

                let no_user_deploys = terms.is_empty();
                let no_system_deploys = system_deploys.is_empty();
                if no_user_deploys && no_system_deploys {
                    if cached_post != *start_hash {
                        tracing::warn!(
                            "[CACHE] StateHashCache hit mismatch for empty block (seq={}): pre_state != cached_post, forcing full replay",
                            seq_num
                        );
                        // Continue to full replay path for validation.
                    } else {
                        let pre_state_hash = Blake2b256Hash::from_bytes_prost(start_hash);
                        let post_state_hash = Blake2b256Hash::from_bytes_prost(&cached_post);
                        self.save_mergeable_channels(
                            post_state_hash,
                            sender.bytes.clone(),
                            seq_num,
                            Vec::new(),
                            &pre_state_hash,
                        )?;
                        tracing::warn!(
                            "[CACHE] StateHashCache hit without mergeable entry for empty block (seq={}); synthesized empty mergeable metadata",
                            seq_num
                        );
                        return Ok(cached_post);
                    }
                }

                tracing::warn!(
                    "[CACHE] StateHashCache hit without mergeable entry for seq={}; falling back to full replay",
                    seq_num
                );
            }
        }

        // Step 2: Check replay cache (deterministic replay delta)
        let replay_cache_key = ReplayCacheKey::new(
            start_hash.clone(),
            sender.bytes.to_vec(),
            seq_num as i64,
            replay_payload_hash,
        );
        if let Some(ref cache) = self.replay_cache {
            if let Some(entry) = cache.get(&replay_cache_key) {
                tracing::info!("[CACHE] ReplayCache hit for sender seq={}", seq_num);

                // Rig the replay runtime with cached event log
                let replay_runtime = self.spawn_replay_runtime().await;
                let rspace_events: Vec<_> = entry
                    .event_log
                    .iter()
                    .map(crate::rust::util::event_converter::to_rspace_event)
                    .collect();
                replay_runtime.rig(rspace_events).await?;

                return Ok(entry.post_state);
            }
        }

        // Step 3: Full replay (cache miss)
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
            )
            .await?;

        // Convert from final to diff values and persist mergeable (number) channels for post-state hash
        let pre_state_hash = Blake2b256Hash::from_bytes_prost(start_hash);
        let post_state = state_hash.to_bytes_prost();

        // Phase 9 (G-1): surface persistence failure as a typed
        // `CasperError` instead of a finalization-task panic. A panic
        // here would crash the runtime_manager future with an
        // unhelpful "task panicked" log; the typed Result lets the
        // caller decide how to react (likely abort the proposal, not
        // the whole node).
        self.save_mergeable_channels(
            state_hash.clone(),
            sender.bytes,
            seq_num,
            mergeable_chs,
            &pre_state_hash,
        )
        .map_err(|e| {
            CasperError::RuntimeError(format!("Failed to save mergeable channels: {:?}", e))
        })?;

        // Cache the result for future replays
        if let Some(ref cache) = self.state_hash_cache {
            cache.put(start_hash.clone(), post_state.clone());
        }

        Ok(post_state)
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

    // Executes deploy as user deploy with immediate rollback
    pub async fn play_exploratory_deploy(
        &self,
        term: String,
        hash: &StateHash,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        let runtime = self.spawn_runtime().await;
        let mut runtime_ops = RuntimeOps::new(runtime);
        runtime_ops.play_exploratory_deploy(term, hash).await
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
            Self::touch_cache_key(&self.block_index_cache_order, block_hash);
            metrics::gauge!(BLOCK_INDEX_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
                .set(self.block_index_cache.len() as f64);
            return Ok(cached.clone());
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
            .map_err(CasperError::KvStoreError)
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

        let block_data = BlockData::from_block(block);
        let is_genesis = block.header.parents_hash_list.is_empty();
        self.replay_compute_state(
            &block.body.state.pre_state_hash,
            block.body.deploys.clone(),
            block.body.system_deploys.clone(),
            &block_data,
            Some(invalid_blocks),
            is_genesis,
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
        // Updated 2026-07-04 for the versioned registry FIP: Step 2 wires
        // VersionedRegistry.rho into genesis, adding one more contract to
        // the initial installed set and re-encoding the bootstrap
        // registry's continuations. Coordinated upgrade required.
        // (Prior update: 2026-04-29 by Phase 9 of where-clauses-and-match-guards.)
        hex::decode("facf59ccc55ee2c04802c7399bcff0d15154f70e0d2bc40cf041aac0a89499c1")
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
        let state_hash_cache_size = Self::max_state_hash_cache_entries();

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
            replay_cache: (replay_cache_size > 0)
                .then(|| Arc::new(InMemoryReplayCache::new(replay_cache_size))),
            state_hash_cache: (state_hash_cache_size > 0)
                .then(|| Arc::new(StateHashCache::new(state_hash_cache_size))),
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
            // H-5 (2026-08-06): empty registry at boot.  Boot
            // pipeline calls `register_root_identity` for each
            // provisioned root path before any deploy runs.
            root_id_registry: rholang::rust::interpreter::io::path::RootIdentityRegistry::new(),
            // Phase 8 slice 8a: empty range-lock registry at boot.
            // Populated per-acquire by `fs_lock_range` handlers.
            lock_registry: rholang::rust::interpreter::io::lock::LockRegistry::new(),
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
        };
        manager.set_fs_snapshot_writer(Some(writer)).await;
        let replay_rt = manager.spawn_replay_runtime().await;
        assert!(replay_rt.fs_snapshot_writer.read().await.is_some());
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
