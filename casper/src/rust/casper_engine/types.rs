//! Data shape — `MultiParentCasperImpl` struct + module-level constants.
//!
//! Phase 3 Step 2 — extracted from `multi_parent_casper_impl.rs`. The struct
//! fields stay `pub` because cross-crate test fixtures (test_node, api
//! tests) build the struct via field-init expressions. See the parent
//! plan's "Layout C" entry for the wider context.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
// Phase 9 (A-3): the deploy-storage handle migrates to
// `parking_lot::Mutex` (no poison propagation, faster acquire).
// C16 (this commit) follows through on the same migration for
// `deploys_in_scope_cache`, which was previously the lone
// `std::sync::Mutex` holdout. The two `parking_lot::Mutex`s are
// reachable via the `PlMutex` alias used in the field
// declarations below. `runtime_manager` keeps `tokio::sync::Mutex`
// because the lock is held across `.await` points (see field
// doc-comment); migrating it would require either an
// interior-mutability refactor of `RuntimeManager`'s `&mut self`
// methods or a parking_lot variant that allows holding across
// `.await` (which the parking_lot guard's `!Send` makes infeasible).
// An `ArcSwap` migration for the deploys-in-scope cache remains an
// option for a future read-mostly optimization commit; pulling
// forward the consistency fix now (single mutex type for
// non-async mutable state) does not preclude it.
use parking_lot::Mutex as PlMutex;
use prost::bytes::Bytes;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::rust::casper::CasperShardConf;
use crate::rust::engine::block_retriever::BlockRetriever;
use crate::rust::estimator::Estimator;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validator_identity::ValidatorIdentity;

// Phase 13 (TC-2): the previous `MAX_ACTIVE_VALIDATORS_CACHE_ENTRIES`
// constant is now `CasperShardConf::active_validators_cache_max_entries`;
// `snapshot::compute_snapshot` reads it from
// `this.casper_shard_conf.active_validators_cache_max_entries`.
//
// Phase 8 (C-4): `deploy_heartbeat_wake_enabled` is now a
// `CasperShardConf::deploy_heartbeat_wake_enabled` field rather than a
// hardcoded predicate. The caller in `block_admission::add_deploy`
// reads `this.casper_shard_conf.deploy_heartbeat_wake_enabled`.

pub struct MultiParentCasperImpl<T: TransportLayer + Send + Sync> {
    pub block_retriever: BlockRetriever<T>,
    pub event_publisher: F1r3flyEvents,
    /// P4-4 (slashing audit): `tokio::sync::Mutex<RuntimeManager>` is the
    /// required shape because [`RuntimeManager`] exposes several `&mut self`
    /// methods (`compute_state`, `compute_state_with_bonds`, `compute_genesis`,
    /// `replay_compute_state`) that must serialize exclusive access to the
    /// underlying RSpace. Demoting to `Arc<RuntimeManager>` is not possible
    /// without an interior-mutability refactor of the RSpace API itself.
    /// Sharding by shard-id is a separate effort that would benchmark
    /// throughput improvements against the additional complexity; that work
    /// is out of scope for the slashing audit.
    pub runtime_manager: Arc<tokio::sync::Mutex<RuntimeManager>>,
    pub estimator: Estimator,
    pub block_store: KeyValueBlockStore,
    pub block_dag_storage: BlockDagKeyValueStorage,
    pub deploy_storage: Arc<PlMutex<KeyValueDeployStorage>>,
    pub casper_buffer_storage: CasperBufferKeyValueStorage,
    pub validator_id: Option<ValidatorIdentity>,
    // TODO: this should be read from chain, for now read from startup options - OLD
    pub casper_shard_conf: CasperShardConf,
    pub approved_block: BlockMessage,
    /// Flag to track finalization status - block proposals fail fast if finalization is running.
    /// This prevents validators from creating blocks with stale snapshots during finalization.
    pub finalization_in_progress: Arc<AtomicBool>,
    /// Single-flight guard for background finalizer scheduling from propose path.
    pub finalizer_task_in_progress: Arc<AtomicBool>,
    /// Indicates a finalizer run was requested while another run was still in progress.
    /// The next queued run will execute immediately after the current one finishes.
    pub finalizer_task_queued: Arc<AtomicBool>,
    /// Shared reference to heartbeat signal for triggering immediate wake on deploy
    pub heartbeat_signal_ref: crate::rust::heartbeat_signal::HeartbeatSignalRef,
    /// Cache for deploys_in_scope BFS result keyed by DAG generation and snapshot LFB.
    /// Including LFB in the key avoids stale scope reuse across finalization advances.
    /// C16: migrated from `Arc<std::sync::Mutex<...>>` to
    /// `Arc<parking_lot::Mutex<...>>` so all three non-async mutex
    /// types on this struct are uniform (`deploy_storage` already
    /// uses parking_lot). Eliminates the poison-handling
    /// `.map_err(|_| CasperError::RuntimeError(...))` boilerplate
    /// at the call sites in `snapshot.rs`. The lock is held purely
    /// synchronously across read-modify-write of the cache cell.
    pub deploys_in_scope_cache:
        Arc<PlMutex<Option<(u64, BlockHash, Arc<dashmap::DashSet<Bytes>>)>>>,
    /// Cache for get_active_validators results keyed by post_state_hash bytes.
    /// Avoids re-reading from RSpace when the main parent block hasn't changed.
    pub active_validators_cache: Arc<tokio::sync::Mutex<HashMap<Vec<u8>, Vec<Validator>>>>,
}
