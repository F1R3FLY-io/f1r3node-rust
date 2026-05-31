//! Data shape — `MultiParentCasperImpl` struct + module-level constants.
//!
//! Phase 3 Step 2 — extracted from `engine::multi_parent_casper`. The struct
//! fields stay `pub` because cross-crate test fixtures (test_node, api
//! tests) build the struct via field-init expressions. See the parent
//! plan's "Layout C" entry for the wider context.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
// Phase 9 (A-3): the deploy-storage handle migrates to
// `parking_lot::Mutex` (no poison propagation, faster acquire).
// C16 (this commit) followed through on the same migration for
// `deploys_in_scope_cache`, which was previously the lone
// `std::sync::Mutex` holdout. The two `parking_lot::Mutex`s are
// reachable via the `PlMutex` alias used in the field
// declarations below.
//
// Merge of dev (EPOCH-004) into feature/slashing: the rspace++
// concurrency rewrite made `RuntimeManager` interior-mutable (every
// method on `&self`), so the historical justification for wrapping
// the manager in `Arc<tokio::sync::Mutex<RuntimeManager>>` is no
// longer load-bearing — the field is now `Arc<RuntimeManager>` and
// callers hand it out without `.lock().await`. The
// `rejected_deploy_buffer` field comes in from dev and uses
// `std::sync::Mutex` (held purely synchronously inside the
// proposer / validator flows, never across `.await`).
use parking_lot::Mutex as PlMutex;
use prost::bytes::Bytes;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::rust::casper::CasperShardConf;
use crate::rust::engine::block_retriever::BlockRetriever;
use crate::rust::estimator::Estimator;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validator_identity::ValidatorIdentity;

/// Cosigner metadata for an in-flight multi-signature deploy, held in the
/// `pending_cosigner_metadata` sidecar map keyed by primary signature.
///
/// `cosigners` is the canonical-order list of cosigners EXCLUDING the
/// primary (primary lives in the legacy `Signed<DeployData>` already stored
/// in `deploy_storage`). It round-trips the wire `CompoundSigner` data
/// (proto field 14) from `DeployDataProto`. D3 (DR-9): no per-signer
/// phlo_share.
#[derive(Clone, Debug)]
pub struct PendingCosignerMetadata {
    pub cosigners: Vec<models::casper::CompoundSigner>,
}

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
    /// P4-4 (slashing audit) originally required
    /// `Arc<tokio::sync::Mutex<RuntimeManager>>` because `RuntimeManager`
    /// exposed `&mut self` methods (`compute_state`, `compute_state_with_bonds`,
    /// `compute_genesis`, `replay_compute_state`) that needed serialized
    /// exclusive access to the underlying RSpace.
    ///
    /// The dev (EPOCH-004) rspace++ concurrency rewrite performed exactly the
    /// interior-mutability refactor that P4-4 deferred: every method on
    /// `RuntimeManager` now takes `&self`, with per-channel locks inside
    /// `RSpace` / `ReplayRSpace` handling the serialization. So the outer
    /// `tokio::sync::Mutex` is no longer load-bearing and was dropped at
    /// merge time. Callers reach the manager by cloning the `Arc` directly.
    pub runtime_manager: Arc<RuntimeManager>,
    pub estimator: Estimator,
    pub block_store: KeyValueBlockStore,
    pub block_dag_storage: BlockDagKeyValueStorage,
    pub deploy_storage: Arc<PlMutex<KeyValueDeployStorage>>,
    /// In-memory side-map of cosigner metadata for in-flight multi-signature
    /// deploys, keyed by the primary signer's signature bytes (the same key
    /// `KeyValueDeployStorage` uses for the legacy `Signed<DeployData>` pool).
    /// Populated by `block_admission::admit_deploy_cosigned` when a compound
    /// `Cosigned<DeployData>` deploy is submitted; consulted by
    /// `block_creator` (proposer-side) to reconstruct the full Cosigned
    /// envelope when handing deploys off to the runtime fan-out. Drained
    /// alongside `deploy_storage.remove(...)` when blocks finalize, by
    /// `admit_handle_valid_block`. Empty for single-signature deploys.
    ///
    /// The side-map design (per §1.9.5) deliberately preserves the legacy
    /// `KeyValueDeployStorage` shape — selection-by-primary-signer is the
    /// right semantics for the proposer, so cosigner data lives in this
    /// memory-resident sidecar rather than forcing a full storage migration.
    /// Node restarts lose in-flight cosigner metadata (clients resubmit
    /// unfinalized deploys); this matches the §1.9.5 greenfield policy.
    pub pending_cosigner_metadata:
        Arc<PlMutex<std::collections::HashMap<Bytes, PendingCosignerMetadata>>>,
    /// Persistence buffer for in-scope-but-rejected deploys, surfaced from
    /// dev (EPOCH-004). Held under `std::sync::Mutex` because all accesses
    /// happen synchronously inside the proposer (block_creator) and
    /// validator (validate.rs::repeat_deploy) — never across `.await`.
    pub rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
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
    ///
    /// Merge of dev: tuple grew to 4 elements — the trailing
    /// `Arc<DashSet<Bytes>>` is the `rejected_in_scope` companion set
    /// to `deploys_in_scope`, used by `validate.rs::repeat_deploy` and
    /// `block_creator.rs` to distinguish in-scope deploys that were
    /// merge-rejected (and therefore eligible for re-inclusion) from
    /// those that were both executed and finalized.
    pub deploys_in_scope_cache: Arc<
        PlMutex<
            Option<(
                u64,
                BlockHash,
                Arc<dashmap::DashSet<Bytes>>,
                Arc<dashmap::DashSet<Bytes>>,
            )>,
        >,
    >,
    /// Cache for get_active_validators results keyed by post_state_hash bytes.
    /// Avoids re-reading from RSpace when the main parent block hasn't changed.
    pub active_validators_cache: Arc<tokio::sync::Mutex<HashMap<Vec<u8>, Vec<Validator>>>>,
}
