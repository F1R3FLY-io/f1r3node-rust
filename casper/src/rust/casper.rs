// See casper/src/main/scala/coop/rchain/casper/Casper.scala

use std::collections::HashMap;
use std::fmt::{self, Display};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, CertifiedAdmissionOutcome, CertifiedSenderAuthority, DeployId,
    KeyValueDagRepresentation,
};
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use crypto::rust::signatures::signed::Signed;
use dashmap::DashSet;
use models::rust::block_hash::BlockHash;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, DeployData, Justification,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::rust::block_status::{CertifiedBlockValidation, InvalidBlock, ValidBlock};
use crate::rust::engine::block_retriever::BlockRetriever;
use crate::rust::engine::multi_parent_casper::MultiParentCasperImpl;
use crate::rust::errors::CasperError;
use crate::rust::estimator::Estimator;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validate::Validate;

pub const LEGACY_CASPER_PROTOCOL_VERSION: i64 = 1;
pub const STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION: i64 =
    models::rust::block_metadata::STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION;
pub const VAULT_BACKED_BYTE_ACCOUNTING_PROTOCOL_VERSION: i64 = 4;
pub const CERTIFIED_VALIDATOR_INCARNATION_PROTOCOL_VERSION: i64 = 5;
pub const CERTIFIED_FINALIZED_FLOOR_PROTOCOL_VERSION: i64 = 6;
pub const CURRENT_CASPER_PROTOCOL_VERSION: i64 = CERTIFIED_FINALIZED_FLOOR_PROTOCOL_VERSION;
use crate::rust::validator_identity::ValidatorIdentity;

pub fn is_supported_casper_protocol_version(version: i64) -> bool {
    version == CURRENT_CASPER_PROTOCOL_VERSION
}

pub fn ensure_supported_casper_protocol_version(version: i64) -> Result<(), CasperError> {
    if is_supported_casper_protocol_version(version) {
        Ok(())
    } else {
        Err(CasperError::UnsupportedProtocolVersion { version })
    }
}

/// Default for `CasperShardConf::active_validators_cache_max_entries`.
pub const ACTIVE_VALIDATORS_CACHE_MAX_ENTRIES_DEFAULT: usize = 4096;

/// Wire convention for `CasperShardConf::max_number_of_parents`: `-1`
/// disables the parent-count cap. C15 / Smell-3: hoisted from two
/// duplicate `const UNLIMITED_PARENTS: i32 = -1;` definitions (one in
/// `validate.rs` and one in `engine/multi_parent_casper/snapshot.rs`) so the wire
/// convention has a single source of truth. NOTE: this is the
/// config-parsing convention; the `Estimator::UNLIMITED_PARENTS`
/// (`i32::MAX`) sentinel used internally by the GHOST estimator is
/// a separate concern.
pub const UNLIMITED_PARENTS: i32 = -1;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeployError {
    ParsingError(String),
    MissingUser,
    UnknownSignatureAlgorithm(String),
    SignatureVerificationFailed,
}

impl DeployError {
    pub fn parsing_error(details: String) -> Self { DeployError::ParsingError(details) }

    pub fn missing_user() -> Self { DeployError::MissingUser }

    pub fn unknown_signature_algorithm(alg: String) -> Self {
        DeployError::UnknownSignatureAlgorithm(alg)
    }

    pub fn signature_verification_failed() -> Self { DeployError::SignatureVerificationFailed }
}

impl Display for DeployError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeployError::ParsingError(details) => write!(f, "Parsing error: {}", details),
            DeployError::MissingUser => write!(f, "Missing user"),
            DeployError::UnknownSignatureAlgorithm(alg) => {
                write!(f, "Unknown signature algorithm '{}'", alg)
            }
            DeployError::SignatureVerificationFailed => write!(f, "Signature verification failed"),
        }
    }
}

#[async_trait]
pub trait Casper {
    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError>;

    fn request_finalization(&self) -> Result<(), CasperError>;

    fn contains(&self, hash: &BlockHash) -> bool;

    fn dag_contains(&self, hash: &BlockHash) -> bool;

    fn buffer_contains(&self, hash: &BlockHash) -> bool;

    fn get_approved_block(&self) -> Result<&BlockMessage, CasperError>;

    fn deploy(
        &self,
        deploy: Signed<DeployData>,
    ) -> Result<Either<DeployError, DeployId>, CasperError>;

    /// Multi-signature aware deploy submission. Default impl rejects
    /// compound deploys (so legacy/test implementations that haven't
    /// overridden it fail loudly rather than silently dropping cosigner
    /// data); production `MultiParentCasperImpl` overrides with the
    /// Cosigned-aware admission path. For single-signer Cosigned
    /// envelopes (the legacy uplift case from `Cosigned::from_single_signer`),
    /// the default delegates to `deploy` for byte-identical observable behavior.
    fn deploy_cosigned(
        &self,
        deploy: crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<Either<DeployError, DeployId>, CasperError> {
        if deploy.is_compound() {
            return Err(CasperError::RuntimeError(
                "deploy_cosigned: implementation does not override the default \
                 multi-sig path; multi-signature deploys are not supported by this \
                 Casper implementation. The production MultiParentCasperImpl \
                 overrides this method."
                    .to_string(),
            ));
        }
        // Single-signer cosigned: legacy delegate.
        self.deploy(deploy.into_legacy_signed_unchecked())
    }

    async fn estimator(
        &self,
        dag: &mut KeyValueDagRepresentation,
    ) -> Result<Vec<BlockHash>, CasperError>;

    fn get_version(&self) -> i64;

    fn recovery_sync_active(&self) -> bool { false }

    fn set_recovery_sync_active(&self, _active: bool) {}

    async fn validate(
        &self,
        block: &BlockMessage,
        snapshot: &mut CasperSnapshot,
    ) -> Result<CertifiedBlockValidation, CasperError>;

    /// Validate a self-created block through the same consensus checks used for a peer block.
    async fn validate_self_created(
        &self,
        block: &BlockMessage,
        snapshot: &mut CasperSnapshot,
        pre_state_hash: Bytes,
        post_state_hash: Bytes,
    ) -> Result<CertifiedBlockValidation, CasperError>;

    async fn handle_valid_block(
        &self,
        block: &BlockMessage,
        certificate: &CertifiedSenderAuthority,
        outcome: &CertifiedAdmissionOutcome,
    ) -> Result<KeyValueDagRepresentation, CasperError>;

    fn handle_invalid_block(
        &self,
        block: &BlockMessage,
        status: &InvalidBlock,
        dag: &KeyValueDagRepresentation,
        certificate: &CertifiedSenderAuthority,
        outcome: &CertifiedAdmissionOutcome,
    ) -> Result<KeyValueDagRepresentation, CasperError>;

    fn get_dependency_free_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError>;

    fn get_dependency_free_hashes_from_buffer(&self) -> Result<Vec<BlockHash>, CasperError> {
        self.get_dependency_free_from_buffer().map(|blocks| {
            blocks
                .into_iter()
                .map(|block| BlockHash::from(block.block_hash))
                .collect()
        })
    }

    fn get_all_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError>;

    fn resolve_finalization_certificate_dependency(
        &self,
        _digest: &BlockHash,
    ) -> Result<(), CasperError> {
        Err(CasperError::RuntimeError(
            "finalization certificate dependency resolution is unavailable".to_string(),
        ))
    }

    fn remove_buffered_hash(&self, _hash: &BlockHash) -> Result<(), CasperError> { Ok(()) }
}

#[async_trait]
pub trait MultiParentCasper: Casper + Send + Sync {
    async fn fetch_dependencies(&self) -> Result<(), CasperError>;

    // This is the weight of faults that have been accumulated so far.
    // We want the clique oracle to give us a fault tolerance that is greater than
    // this initial fault weight combined with our fault tolerance threshold t.
    fn normalized_initial_fault(&self, target: &BlockHash) -> Result<f32, CasperError>;

    async fn last_finalized_block(&self) -> Result<BlockMessage, CasperError>;

    // Equivalent to Scala's blockDag: F[BlockDagRepresentation[F]]
    async fn block_dag(&self) -> Result<KeyValueDagRepresentation, CasperError>;

    fn block_store(&self) -> &KeyValueBlockStore;

    /// Read-only access to the shard configuration. Used by APIs that need
    /// shard-scoped parameters such as `deploy_lifespan` to compute deploy
    /// finalization status.
    fn casper_shard_conf(&self) -> &CasperShardConf;

    fn rejected_deploy_buffer_contains_sig(&self, _sig: &[u8]) -> Result<bool, CasperError> {
        Ok(false)
    }

    fn runtime_manager(&self) -> Arc<RuntimeManager>;

    fn get_validator(&self) -> Option<ValidatorIdentity>;

    async fn get_history_exporter(&self) -> Arc<dyn RSpaceExporter>;

    /// Check if pending deploys exist in storage (not yet included in blocks).
    async fn has_pending_deploys_in_storage(&self) -> Result<bool, CasperError>;

    /// Check if pending deploys exist in storage using an already computed snapshot.
    /// Default fallback uses the legacy method and may compute a fresh snapshot.
    async fn has_pending_deploys_in_storage_for_snapshot(
        &self,
        _snapshot: &CasperSnapshot,
    ) -> Result<bool, CasperError> {
        self.has_pending_deploys_in_storage().await
    }
}

pub async fn hash_set_casper<T: TransportLayer + Send + Sync>(
    block_retriever: BlockRetriever<T>,
    event_publisher: F1r3flyEvents,
    runtime_manager: Arc<RuntimeManager>,
    estimator: Estimator,
    block_store: KeyValueBlockStore,
    block_dag_storage: BlockDagKeyValueStorage,
    deploy_storage: KeyValueDeployStorage,
    rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    casper_buffer_storage: CasperBufferKeyValueStorage,
    validator_id: Option<ValidatorIdentity>,
    mut casper_shard_conf: CasperShardConf,
    genesis_block: BlockMessage,
    heartbeat_signal_ref: crate::rust::heartbeat_signal::HeartbeatSignalRef,
) -> Result<MultiParentCasperImpl<T>, CasperError> {
    casper_shard_conf.validate_parent_bounds()?;
    if genesis_block.body.state.block_number != 0
        || !genesis_block.header.parents_hash_list.is_empty()
        || genesis_block.seq_num != 0
        || !genesis_block.justifications.is_empty()
        || !matches!(Validate::block_hash(&genesis_block), Either::Right(_))
    {
        return Err(CasperError::RuntimeError(
            "Casper construction requires a structurally valid canonical genesis block".to_string(),
        ));
    }
    block_dag_storage.insert(
        &genesis_block,
        block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
    )?;
    block_dag_storage.reconcile_latest_messages(&block_store)?;
    casper_shard_conf.adopt_approved_protocol_version(&genesis_block)?;
    // SINGLE ADOPTION POINT for the protocol fault-tolerance threshold.
    //
    // θ is a CONSENSUS value: the finalized-floor oracle runs on it, and the
    // floor decides the multi-parent merge base — a validated, node-identical
    // quantity. Every node (validator or observer) must therefore run the ppm
    // baked into the PoS contract at genesis, regardless of local config.
    //
    // All three constructors of a running casper (`initializing`,
    // `casper_launch`, `genesis_ceremony_master`) funnel through here, so this
    // is the only place the adoption can be guaranteed for every path.
    //
    // The assignment is UNCONDITIONAL — the `if` gates only the log line. That
    // is precisely Rocq `FtProvenance.reconcile_agrees_on_onchain`:
    // `reconcile(local, onchain) = onchain`, so two nodes with ANY two local
    // configs derive the same floor. Absent/out-of-range now FAILS the node
    // (see `read_on_chain_fault_tolerance_threshold_ppm`) rather than silently
    // reopening node-local floor divergence.
    let onchain_ppm =
        crate::rust::util::token_metadata_check::read_on_chain_fault_tolerance_threshold_ppm(
            &runtime_manager,
            &genesis_block.body.state.post_state_hash,
        )
        .await?;
    if onchain_ppm != casper_shard_conf.fault_tolerance_threshold_ppm {
        tracing::info!(
            onchain_ppm,
            local_ppm = casper_shard_conf.fault_tolerance_threshold_ppm,
            "Adopting on-chain protocol fault-tolerance threshold over local configuration"
        );
    }
    casper_shard_conf.fault_tolerance_threshold_ppm = onchain_ppm;
    // Keep the display f32 in lock-step with the exact ppm that decides
    // finalization, so the API can never report a threshold the oracle is not
    // using. The ppm remains the sole DECISION input; this f32 is display-only.
    casper_shard_conf.fault_tolerance_threshold = (onchain_ppm as f64 / 1_000_000.0) as f32;

    let finalization_worker_limit = casper_shard_conf.finalizer_conf.max_parallel_workers;
    Ok(MultiParentCasperImpl {
        block_retriever,
        event_publisher,
        runtime_manager,
        estimator,
        block_store,
        block_dag_storage,
        deploy_storage: Arc::new(parking_lot::Mutex::new(deploy_storage)),
        pending_cosigner_metadata: Arc::new(parking_lot::Mutex::new(
            std::collections::HashMap::new(),
        )),
        rejected_deploy_buffer,
        casper_buffer_storage,
        validator_id,
        casper_shard_conf,
        approved_block: genesis_block,
        finalization_in_progress: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        recovery_sync_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        finalization_schedule: Arc::new(
            crate::rust::finality::finalization_schedule::FinalizationSchedule::new(
                finalization_worker_limit,
            ),
        ),
        certificate_verification_schedule: Arc::new(
            crate::rust::finality::certificate::CertificateVerificationSchedule::new(
                finalization_worker_limit,
            ),
        ),
        heartbeat_signal_ref,
        deploys_in_scope_cache: Arc::new(parking_lot::Mutex::new(None)),
        active_validators_cache: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    })
}

/**
 * Casper snapshot is a state that is changing in discrete manner with each new block added.
 * This class represents full information about the state. It is required for creating new blocks
 * as well as for validating blocks.
 */
#[derive(Clone)]
pub struct CasperSnapshot {
    pub dag: KeyValueDagRepresentation,
    pub last_finalized_block: BlockHash,
    pub lca: BlockHash,
    pub tips: Vec<BlockHash>,
    pub parents: Vec<BlockMessage>,
    // C13 / Perf-4: `justifications` and `max_seq_nums` are
    // constructed once per snapshot in `compute_snapshot` and
    // observed read-only by all downstream consumers (no
    // production caller mutates either after CasperSnapshot
    // assembly). DashSet/DashMap's stripe-locking overhead is
    // pure cost for a zero-contention workload — plain
    // HashSet/HashMap are strictly cheaper and have the same
    // iteration/lookup API consumers already use.
    pub justifications: Vec<Justification>,
    pub invalid_blocks: HashMap<BlockHash, Validator>,
    /// Signatures of deploys seen in ancestry window.
    /// Keeping signatures avoids retaining full deploy payloads in long-lived snapshots.
    pub deploys_in_scope: Arc<DashSet<Bytes>>,
    /// Signatures of deploys that appeared in a merge block's rejected_deploys list
    /// within the ancestry window. Intersects with `deploys_in_scope` when a deploy
    /// was executed in one block and rejected during a descendant merge; the block
    /// creator uses this set to know which in-scope deploys are eligible for re-inclusion.
    pub rejected_in_scope: Arc<DashSet<Bytes>>,
    pub max_block_num: i64,
    pub max_seq_nums: HashMap<Validator, u64>,
    pub finalized_floor_bonds: Vec<Bond>,
    pub on_chain_state: OnChainCasperState,
    pub consensus_context: crate::rust::causal_equivocation::CertifiedConsensusContext,
    pub finalized_floor_certificate:
        Option<models::rust::casper::protocol::casper_message::FinalizationCertificate>,
}

impl CasperSnapshot {
    pub fn new(dag: KeyValueDagRepresentation) -> Self {
        Self {
            dag,
            last_finalized_block: BlockHash::default(),
            lca: BlockHash::default(),
            tips: vec![],
            parents: vec![],
            justifications: Vec::new(),
            invalid_blocks: HashMap::new(),
            deploys_in_scope: Arc::new(DashSet::new()),
            rejected_in_scope: Arc::new(DashSet::new()),
            max_block_num: 0,
            max_seq_nums: HashMap::new(),
            finalized_floor_bonds: Vec::new(),
            on_chain_state: OnChainCasperState::new(CasperShardConf::new()),
            consensus_context:
                crate::rust::causal_equivocation::CertifiedConsensusContext::pre_genesis(),
            finalized_floor_certificate: None,
        }
    }

    pub fn finalized_floor_validators(&self) -> Vec<Validator> {
        let mut validators = self
            .finalized_floor_bonds
            .iter()
            .filter(|bond| bond.stake > 0)
            .map(|bond| bond.validator.clone())
            .collect::<Vec<_>>();
        validators.sort_unstable();
        validators.dedup();
        validators
    }

    pub fn finalized_floor_weight_map(&self) -> HashMap<Validator, i64> {
        self.finalized_floor_bonds
            .iter()
            .filter(|bond| bond.stake > 0)
            .map(|bond| (bond.validator.clone(), bond.stake))
            .collect()
    }
}

#[derive(Clone)]
pub struct OnChainCasperState {
    pub shard_conf: CasperShardConf,
    pub bonds_map: HashMap<Validator, i64>,
    pub bond_generations: HashMap<Validator, BondGeneration>,
    pub active_validators: Vec<Validator>,
}

impl OnChainCasperState {
    pub fn new(shard_conf: CasperShardConf) -> Self {
        Self {
            shard_conf,
            bonds_map: HashMap::new(),
            bond_generations: HashMap::new(),
            active_validators: vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct CasperShardConf {
    /// Display/back-compat `f32` view of the fault-tolerance threshold θ. The
    /// finalization DECISION is derived from the exact
    /// [`fault_tolerance_threshold_ppm`](Self::fault_tolerance_threshold_ppm),
    /// never from this lossy value.
    pub fault_tolerance_threshold: f32,
    /// Exact fault-tolerance threshold θ as an on-chain ppm numerator
    /// (θ = ppm / 1_000_000). Source of truth for the integer-exact finalization
    /// DECISION (`CliqueOracle::ft_decides_exact`).
    pub fault_tolerance_threshold_ppm: i64,
    pub shard_name: String,
    pub parent_shard_id: String,
    pub finalization_rate: i32,
    pub max_number_of_parents: i32,
    pub max_parent_depth: i32,
    pub synchrony_constraint_threshold: f32,
    pub height_constraint_threshold: i64,
    // Validators will try to put deploy in a block only for next `deployLifespan` blocks.
    // Required to enable protection from re-submitting duplicate deploys
    pub deploy_lifespan: i64,
    pub casper_version: i64,
    pub config_version: i64,
    pub bond_minimum: i64,
    pub bond_maximum: i64,
    pub epoch_length: i32,
    pub quarantine_length: i32,
    pub min_phlo_price: i64,
    /// Additional client SystemVault balances incorporated into the canonical
    /// blessed vault-generator deploys at genesis.
    pub client_fuel_allocations: Vec<(crypto::rust::public_key::PublicKey, i64)>,
    /// Disable late block filtering in DagMerger (for testing or special configurations)
    pub disable_late_block_filtering: bool,
    /// When `true`, `add_deploy` triggers an immediate heartbeat-signal
    /// wake so the heartbeat task picks up the new deploy on the next
    /// tick rather than waiting up to `check_interval` seconds. Defaults
    /// to `false`; Phase 8 (C-4) lifted this from a hardcoded predicate
    /// to a configuration knob so operators can opt in.
    pub deploy_heartbeat_wake_enabled: bool,
    /// Disable validator progress check (for standalone mode)
    pub disable_validator_progress_check: bool,
    /// Enable background garbage collection for mergeable channels.
    /// When enabled, uses safe reachability-based GC (required for multi-parent mode).
    /// When disabled (default), mergeable data is retained.
    pub enable_mergeable_channel_gc: bool,
    /// Depth buffer for mergeable channels garbage collection.
    /// Additional safety margin beyond max-parent-depth before deleting data.
    pub mergeable_channels_gc_depth_buffer: i32,
    pub finalizer_conf: crate::rust::casper_conf::FinalizerConf,
    pub synchrony_recovery_stall_window: Duration,
    pub synchrony_recovery_cooldown: Duration,
    pub synchrony_recovery_max_bypasses: u32,
    pub synchrony_finalized_baseline_enabled: bool,
    pub synchrony_finalized_baseline_max_distance: u64,
    pub max_user_deploys_per_block: u32,
    /// Per-deploy hard cap on number of cosigners in a multi-signature
    /// deploy. Committed by genesis and enforced at the
    /// `admit_deploy_cosigned` ingress boundary before the deploy reaches the
    /// pool. Sourced from
    /// `casper_conf::max_cosigners_per_deploy` (default 64). Configurable
    /// per shard.
    pub max_cosigners_per_deploy: u32,
    /// Native token metadata baked into the TokenMetadata contract at genesis.
    /// Present on every node (joiner, validator, ceremony master, observer, standalone)
    /// so each path can log the effective values at startup.
    pub native_token_name: String,
    pub native_token_symbol: String,
    pub native_token_decimals: u32,
    /// Phase 13 (TC-2): maximum entries in the `active_validators_cache`
    /// inside `compute_snapshot`. Previously a hardcoded `usize = 4096`
    /// constant in `engine/multi_parent_casper/types.rs`; lifted to configuration so
    /// operators can size the cache for their validator set without
    /// recompiling. Distinct from the `runtime_manager`'s own 256-entry
    /// validator-key cache.
    pub active_validators_cache_max_entries: usize,
}

impl CasperShardConf {
    pub fn validate_parent_bounds(&self) -> Result<(), CasperError> {
        crate::rust::casper_conf::validate_parent_bound_values(
            self.max_number_of_parents,
            self.max_parent_depth,
            self.mergeable_channels_gc_depth_buffer,
        )
        .map_err(CasperError::RuntimeError)
    }

    pub fn adopt_approved_protocol_version(
        &mut self,
        approved_block: &BlockMessage,
    ) -> Result<(), CasperError> {
        let version = approved_block.header.version;
        ensure_supported_casper_protocol_version(version)?;
        self.casper_version = version;
        Ok(())
    }

    pub fn new() -> Self {
        Self {
            fault_tolerance_threshold: 0.0,
            fault_tolerance_threshold_ppm: 0,
            shard_name: "".to_string(),
            parent_shard_id: "".to_string(),
            finalization_rate: 0,
            max_number_of_parents: UNLIMITED_PARENTS,
            max_parent_depth: i32::MAX,
            synchrony_constraint_threshold: 0.0,
            height_constraint_threshold: 0,
            deploy_lifespan: 0,
            casper_version: CURRENT_CASPER_PROTOCOL_VERSION,
            config_version: 0,
            bond_minimum: 0,
            bond_maximum: 0,
            epoch_length: 0,
            quarantine_length: 0,
            min_phlo_price: 0,
            // Task #13b: default EMPTY = no genesis client funding-slot seed.
            // Covers every
            // `..CasperShardConf::new()`-spread literal (incl. test sites).
            client_fuel_allocations: Vec::new(),
            disable_late_block_filtering: true,
            deploy_heartbeat_wake_enabled: false,
            disable_validator_progress_check: false,
            enable_mergeable_channel_gc: false,
            mergeable_channels_gc_depth_buffer: 10,
            finalizer_conf: crate::rust::casper_conf::FinalizerConf::default(),
            synchrony_recovery_stall_window: Duration::from_secs(60),
            synchrony_recovery_cooldown: Duration::from_secs(20),
            synchrony_recovery_max_bypasses: 2,
            synchrony_finalized_baseline_enabled: true,
            synchrony_finalized_baseline_max_distance: 2048,
            max_user_deploys_per_block: 128,
            max_cosigners_per_deploy: crate::rust::casper_conf::DEFAULT_MAX_COSIGNERS_PER_DEPLOY,
            native_token_name: "F1R3CAP".to_string(),
            native_token_symbol: "F1R3".to_string(),
            native_token_decimals: 8,
            active_validators_cache_max_entries: ACTIVE_VALIDATORS_CACHE_MAX_ENTRIES_DEFAULT,
        }
    }
}

// TODO(#325): Move test_helpers to a #[cfg(test)] module or separate test-utils crate
// to avoid including test code in production binaries.
/// Test helpers for creating mock Casper implementations.
pub mod test_helpers {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    use super::*;

    /// A test implementation of MultiParentCasper that returns a configurable snapshot and LFB.
    pub struct TestCasperWithSnapshot {
        snapshot: CasperSnapshot,
        lfb: BlockMessage,
        pending_deploy_count: usize,
        block_store: KeyValueBlockStore,
        finalization_requests: AtomicUsize,
    }

    impl TestCasperWithSnapshot {
        fn create_test_block_store() -> KeyValueBlockStore {
            KeyValueBlockStore::new(
                Arc::new(InMemoryKeyValueStore::new()),
                Arc::new(InMemoryKeyValueStore::new()),
            )
        }

        pub fn new(snapshot: CasperSnapshot, lfb: BlockMessage) -> Self {
            Self {
                snapshot,
                lfb,
                pending_deploy_count: 0,
                block_store: Self::create_test_block_store(),
                finalization_requests: AtomicUsize::new(0),
            }
        }

        pub fn new_with_pending_deploys(
            snapshot: CasperSnapshot,
            lfb: BlockMessage,
            pending_deploy_count: usize,
        ) -> Self {
            Self {
                snapshot,
                lfb,
                pending_deploy_count,
                block_store: Self::create_test_block_store(),
                finalization_requests: AtomicUsize::new(0),
            }
        }

        pub fn finalization_request_count(&self) -> usize {
            self.finalization_requests.load(Ordering::SeqCst)
        }

        /// Create an empty CasperSnapshot for testing.
        pub fn create_empty_snapshot() -> CasperSnapshot {
            use std::sync::Arc;

            use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
            use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
            use parking_lot::RwLock;
            use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
            use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

            let block_metadata_store =
                KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
            let dag = KeyValueDagRepresentation {
                dag_set: imbl::HashSet::new(),
                latest_messages_map: imbl::HashMap::new(),
                child_map: imbl::HashMap::new(),
                height_map: imbl::OrdMap::new(),
                block_number_map: imbl::HashMap::new(),
                main_parent_map: imbl::HashMap::new(),
                self_justification_map: imbl::HashMap::new(),
                invalid_blocks_set: imbl::HashSet::new(),
                equivocation_observations: imbl::HashMap::new(),
                last_finalized_block_hash: BlockHash::new(),
                finalized_blocks_set: imbl::HashSet::new(),
                block_metadata_index: Arc::new(RwLock::new(
                    BlockMetadataStore::new(block_metadata_store).unwrap(),
                )),
                deploy_index: Arc::new(RwLock::new(KeyValueTypedStoreImpl::new(Arc::new(
                    InMemoryKeyValueStore::new(),
                )))),
                deploy_occurrence_index: Arc::new(RwLock::new(KeyValueTypedStoreImpl::new(
                    Arc::new(InMemoryKeyValueStore::new()),
                ))),
                floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
                frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            };

            CasperSnapshot::new(dag)
        }

        pub fn bond_validator_in_snapshot(
            snapshot: &mut CasperSnapshot,
            validator: models::rust::validator::Validator,
        ) {
            use models::rust::casper::protocol::casper_message::Bond;

            if snapshot.parents.is_empty() {
                snapshot
                    .parents
                    .push(models::rust::block_implicits::get_random_block_default());
            }
            let parent = &mut snapshot.parents[0];
            if !parent
                .body
                .state
                .bonds
                .iter()
                .any(|bond| bond.validator == validator)
            {
                parent.body.state.bonds.push(Bond {
                    validator: validator.clone(),
                    stake: 100,
                });
            }
            if !snapshot
                .finalized_floor_bonds
                .iter()
                .any(|bond| bond.validator == validator)
            {
                snapshot.finalized_floor_bonds.push(Bond {
                    validator,
                    stake: 100,
                });
            }
        }
    }

    fn certified_test_validation(
        block: &BlockMessage,
    ) -> Result<CertifiedBlockValidation, CasperError> {
        let generation = block.header.sender_bond_generation.ok_or_else(|| {
            CasperError::RuntimeError(
                "accepted test block is missing sender bond generation".to_string(),
            )
        })?;
        let authority_floor = block
            .header
            .parents_hash_list
            .first()
            .cloned()
            .unwrap_or_else(|| block.block_hash.clone());
        let authority_post_state = block.body.state.pre_state_hash.clone();
        let mut preimage = authority_floor.to_vec();
        preimage.extend_from_slice(&authority_post_state);
        let certificate = CertifiedSenderAuthority::new(
            block,
            authority_floor,
            authority_post_state,
            crypto::rust::hash::blake2b256::Blake2b256::hash(preimage).into(),
            generation,
            1,
        )
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        CertifiedBlockValidation::certified(block, Either::Right(ValidBlock::Valid), certificate)
    }

    #[async_trait]
    impl Casper for TestCasperWithSnapshot {
        async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
            Ok(self.snapshot.clone())
        }

        fn request_finalization(&self) -> Result<(), CasperError> {
            self.finalization_requests.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn contains(&self, _hash: &BlockHash) -> bool { false }

        fn dag_contains(&self, _hash: &BlockHash) -> bool { false }

        fn buffer_contains(&self, _hash: &BlockHash) -> bool { false }

        fn get_approved_block(&self) -> Result<&BlockMessage, CasperError> { Ok(&self.lfb) }

        fn deploy(
            &self,
            _deploy: Signed<DeployData>,
        ) -> Result<Either<DeployError, DeployId>, CasperError> {
            Ok(Either::Right(DeployId::default()))
        }

        async fn estimator(
            &self,
            _dag: &mut KeyValueDagRepresentation,
        ) -> Result<Vec<BlockHash>, CasperError> {
            Ok(Vec::new())
        }

        fn get_version(&self) -> i64 { self.snapshot.on_chain_state.shard_conf.casper_version }

        async fn validate(
            &self,
            block: &BlockMessage,
            _snapshot: &mut CasperSnapshot,
        ) -> Result<CertifiedBlockValidation, CasperError> {
            certified_test_validation(block)
        }

        async fn validate_self_created(
            &self,
            block: &BlockMessage,
            _snapshot: &mut CasperSnapshot,
            _pre_state_hash: Bytes,
            _post_state_hash: Bytes,
        ) -> Result<CertifiedBlockValidation, CasperError> {
            certified_test_validation(block)
        }

        async fn handle_valid_block(
            &self,
            _block: &BlockMessage,
            _certificate: &CertifiedSenderAuthority,
            _outcome: &CertifiedAdmissionOutcome,
        ) -> Result<KeyValueDagRepresentation, CasperError> {
            Ok(self.snapshot.dag.clone())
        }

        fn handle_invalid_block(
            &self,
            _block: &BlockMessage,
            _status: &InvalidBlock,
            dag: &KeyValueDagRepresentation,
            _certificate: &CertifiedSenderAuthority,
            _outcome: &CertifiedAdmissionOutcome,
        ) -> Result<KeyValueDagRepresentation, CasperError> {
            Ok(dag.clone())
        }

        fn get_dependency_free_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
            Ok(Vec::new())
        }

        fn get_all_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> { Ok(Vec::new()) }
    }

    #[async_trait]
    impl MultiParentCasper for TestCasperWithSnapshot {
        async fn fetch_dependencies(&self) -> Result<(), CasperError> { Ok(()) }

        fn normalized_initial_fault(&self, _target: &BlockHash) -> Result<f32, CasperError> {
            Ok(0.0)
        }

        async fn last_finalized_block(&self) -> Result<BlockMessage, CasperError> {
            Ok(self.lfb.clone())
        }

        async fn block_dag(&self) -> Result<KeyValueDagRepresentation, CasperError> {
            Ok(self.snapshot.dag.clone())
        }

        fn block_store(&self) -> &KeyValueBlockStore { &self.block_store }

        fn casper_shard_conf(&self) -> &CasperShardConf { &self.snapshot.on_chain_state.shard_conf }

        fn runtime_manager(&self) -> Arc<RuntimeManager> {
            unimplemented!("runtime_manager not needed for heartbeat tests")
        }

        fn get_validator(&self) -> Option<ValidatorIdentity> { None }

        async fn get_history_exporter(&self) -> Arc<dyn RSpaceExporter> {
            unimplemented!("get_history_exporter not needed for heartbeat tests")
        }

        async fn has_pending_deploys_in_storage(&self) -> Result<bool, CasperError> {
            Ok(self.pending_deploy_count > 0)
        }
    }
}

#[cfg(test)]
mod protocol_version_tests {
    use models::rust::block_implicits::get_random_block_default;
    use proptest::prelude::*;
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::finality_recovery_leader;

    #[test]
    fn approved_protocol_version_adoption_accepts_current() {
        let mut block = get_random_block_default();
        block.header.version = CURRENT_CASPER_PROTOCOL_VERSION;
        let mut conf = CasperShardConf::new();
        conf.adopt_approved_protocol_version(&block).unwrap();
        assert_eq!(conf.casper_version, CURRENT_CASPER_PROTOCOL_VERSION);
    }

    #[test]
    fn noncurrent_approved_protocol_versions_fail_without_mutation() {
        for version in [
            LEGACY_CASPER_PROTOCOL_VERSION,
            STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION - 1,
            CURRENT_CASPER_PROTOCOL_VERSION + 1,
        ] {
            let mut block = get_random_block_default();
            block.header.version = version;
            let mut conf = CasperShardConf::new();
            let original = conf.casper_version;
            assert_eq!(
                conf.adopt_approved_protocol_version(&block),
                Err(CasperError::UnsupportedProtocolVersion { version })
            );
            assert_eq!(conf.casper_version, original);
        }
    }

    #[test]
    fn recovery_validators_ignore_divergent_proposal_committee() {
        let mut snapshot = test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        let first = Bytes::from_static(b"a");
        let second = Bytes::from_static(b"b");
        snapshot.finalized_floor_bonds = vec![
            Bond {
                validator: second.clone(),
                stake: 1,
            },
            Bond {
                validator: first.clone(),
                stake: 1,
            },
            Bond {
                validator: second.clone(),
                stake: 1,
            },
        ];
        snapshot.on_chain_state.active_validators = vec![Bytes::from_static(b"head")];

        assert_eq!(snapshot.finalized_floor_validators(), vec![first, second]);
    }

    proptest! {
        #[test]
        fn supported_protocol_versions_are_exactly_the_declared_versions(version in any::<i64>()) {
            let expected = version == CURRENT_CASPER_PROTOCOL_VERSION;
            prop_assert_eq!(is_supported_casper_protocol_version(version), expected);
            prop_assert_eq!(ensure_supported_casper_protocol_version(version).is_ok(), expected);
        }

        #[test]
        fn recovery_leader_is_invariant_under_head_committee_drift(
            head_committee in proptest::collection::vec(any::<u8>(), 0..8),
            finalized_height in 0i64..1_000,
            recovery_round in any::<u64>(),
        ) {
            let mut snapshot = test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            snapshot.finalized_floor_bonds = vec![
                Bond { validator: Bytes::from_static(b"a"), stake: 1 },
                Bond { validator: Bytes::from_static(b"b"), stake: 1 },
                Bond { validator: Bytes::from_static(b"c"), stake: 1 },
            ];
            let expected = finality_recovery_leader(
                snapshot.finalized_floor_validators(),
                finalized_height,
                recovery_round,
            );
            snapshot.on_chain_state.active_validators = head_committee
                .into_iter()
                .map(|validator| Bytes::from(vec![validator]))
                .collect();

            prop_assert_eq!(
                finality_recovery_leader(
                    snapshot.finalized_floor_validators(),
                    finalized_height,
                    recovery_round,
                ),
                expected,
            );
        }
    }
}
