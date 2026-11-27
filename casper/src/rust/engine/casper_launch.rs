// See casper/src/main/scala/coop/rchain/casper/engine/CasperLaunch.scala

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::{
    KeyValueDeployStorage, KeyValueRejectedDeployBuffer,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{ApprovedBlock, BlockMessage};
use rspace_plus_plus::rspace::state::rspace_state_manager::RSpaceStateManager;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::rust::casper::{hash_set_casper, CasperShardConf, MultiParentCasper};
use crate::rust::casper_conf::CasperConf;
use crate::rust::engine::approve_block_protocol::ApproveBlockProtocolFactory;
use crate::rust::engine::block_approver_protocol::BlockApproverProtocol;
use crate::rust::engine::engine::{
    record_direct_to_running_init_metrics, transition_to_initializing, transition_to_running,
};
use crate::rust::engine::engine_cell::EngineCell;
use crate::rust::engine::genesis_ceremony_master::GenesisCeremonyMaster;
use crate::rust::engine::genesis_validator::GenesisValidator;
use crate::rust::engine::multi_parent_casper::MultiParentCasperImpl;
use crate::rust::errors::CasperError;
use crate::rust::estimator::Estimator;
use crate::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use crate::rust::util::bonds_parser::BondsParser;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::vault_parser::VaultParser;
use crate::rust::validator_identity::ValidatorIdentity;

#[async_trait]
pub trait CasperLaunch {
    async fn launch(&self) -> Result<(), CasperError>;
}

pub struct CasperLaunchImpl<T: TransportLayer + Send + Sync + Clone + 'static> {
    // Infrastructure dependencies (Scala implicit parameters - Transport, State, Storage, etc.)
    transport_layer: Arc<T>,
    rp_conf_ask: RPConf,
    connections_cell: ConnectionsCell,
    last_approved_block: Arc<Mutex<Option<ApprovedBlock>>>,
    event_publisher: F1r3flyEvents,
    engine_cell: Arc<EngineCell>,
    block_store: KeyValueBlockStore,
    block_dag_storage: BlockDagKeyValueStorage,
    deploy_storage: KeyValueDeployStorage,
    rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    rspace_state_manager: RSpaceStateManager,
    runtime_manager: Arc<RuntimeManager>,
    estimator: Estimator,
    casper_shard_conf: CasperShardConf,

    // Explicit parameters from Scala (in same order as Scala signature)
    ingest: IngestHandle,
    conf: CasperConf,
    trim_state: bool,
    disable_state_exporter: bool,
    /// Shared reference to heartbeat signal for triggering immediate wake on deploy
    heartbeat_signal_ref: crate::rust::heartbeat_signal::HeartbeatSignalRef,
    /// Operator-supplied static-provisioning bundle, projected from
    /// `node::configuration::provisioning_merge::merge_and_validate`
    /// at boot.  Threaded verbatim into
    /// `Genesis.fs_bundle` on the ceremony-master path so every
    /// validator's genesis block carries the same operator-visible
    /// cap bundle.
    fs_bundle: Vec<crate::rust::genesis::contracts::fs_genesis::BundleEntry>,
    state_items_tx: Option<
        tokio::sync::mpsc::Sender<
            models::rust::casper::protocol::casper_message::StoreItemsMessage,
        >,
    >,
}

use crate::rust::blocks::ingest::block_ingest::IngestHandle;

pub(crate) fn ensure_compatible_approved_block(
    approved_block: &ApprovedBlock,
) -> Result<(), CasperError> {
    let block = &approved_block.candidate.block;
    if matches!(
        crate::rust::validate::Validate::block_hash(block),
        rspace_plus_plus::rspace::history::Either::Left(_)
    ) {
        return Err(CasperError::RuntimeError(format!(
            "The stored approved block {} does not match its block hash under this build. \
             An incompatible build created the data directory. Stop all nodes, remove their \
             data directories, and start a new network from a new genesis.",
            PrettyPrinter::build_string_bytes(&block.block_hash)
        )));
    }
    Ok(())
}

pub(crate) fn horizon_probe_peer(rp_conf: &RPConf) -> Option<&comm::rust::peer_node::PeerNode> {
    rp_conf
        .bootstrap
        .as_ref()
        .filter(|bootstrap| bootstrap.id != rp_conf.local.id)
}

impl<T: TransportLayer + Send + Sync + Clone + 'static> CasperLaunchImpl<T> {
    /// Build the MultiParentCasper from an approved block anchor.
    async fn create_casper(
        &self,
        validator_id: Option<ValidatorIdentity>,
        ab: BlockMessage,
    ) -> Result<MultiParentCasperImpl, CasperError> {
        hash_set_casper(
            self.ingest.clone(),
            self.event_publisher.clone(),
            self.runtime_manager.clone(),
            self.estimator.clone(),
            self.block_store.clone(),
            self.block_dag_storage.clone(),
            self.deploy_storage.clone(),
            self.rejected_deploy_buffer.clone(),
            validator_id,
            self.casper_shard_conf.clone(),
            ab,
            self.heartbeat_signal_ref.clone(),
        )
        .await
    }

    /// Resume straight to Running without probing peers. Used when
    /// `self.rp_conf_ask.bootstrap.is_none()` (standalone / no-peer
    /// deployments): the horizon probe inside `Initializing` would send
    /// an `ApprovedBlockRequest` to the bootstrap, which does not exist,
    /// so the probe cannot apply here. The networked resume path still
    /// goes through `connect_and_query_approved_block` and benefits from
    /// the probe (#1059).
    async fn connect_direct_to_running(
        &self,
        approved_block: ApprovedBlock,
        disable_state_exporter: bool,
    ) -> Result<(), CasperError> {
        let validator_id = ValidatorIdentity::from_private_key_with_logging(
            self.conf.validator_private_key.as_deref(),
        );

        let ab = approved_block.candidate.block.clone();
        let genesis_post_state_hash = ab.body.state.post_state_hash.clone();
        let casper = self.create_casper(validator_id.clone(), ab).await?;
        let casper_arc = Arc::new(casper);

        // Standalone nodes have no peers to ask for tips; the init closure
        // is a no-op so the engine fires its periodic cadence immediately.
        let the_init = Arc::new(|| {
            Box::pin(async { Ok(()) })
                as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
        });

        record_direct_to_running_init_metrics();

        let snapshot_chunk_ctx = {
            let ctx = crate::rust::engine::snapshot_chunk_sync::build_snapshot_chunk_context(
                &self.runtime_manager,
            )
            .await;
            if let Some(ref ctx) = ctx {
                let _tick_handle = crate::rust::engine::snapshot_chunk_sync::spawn_periodic_tick(
                    Arc::clone(&ctx.sync_driver),
                    self.transport_layer.clone(),
                    self.rp_conf_ask.clone(),
                    self.connections_cell.clone(),
                );
            }
            ctx
        };

        let wal_payload_ctx = {
            let mut ctx = crate::rust::engine::running::WalPayloadContext::from_runtime_manager(
                &self.runtime_manager,
            )
            .await;
            let tick = crate::rust::engine::wal_payload_sync::spawn_periodic_tick(
                Arc::clone(&ctx.sync_driver),
                self.transport_layer.clone(),
                self.rp_conf_ask.clone(),
                self.connections_cell.clone(),
            );
            ctx.tick_stop = Some(tick.stop);
            drop(tick.join_handle);
            Some(ctx)
        };

        if let (Some(snap_ctx), Some(wal_ctx)) =
            (snapshot_chunk_ctx.as_ref(), wal_payload_ctx.as_ref())
        {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<
                crate::rust::engine::snapshot_chunk_sync::SnapshotCompletion,
            >();
            snap_ctx.sync_driver.install_completion_sink(tx);
            let registry = self.runtime_manager.root_registry.clone();
            let allowed_roots = self.runtime_manager.consensus_static_roots().await;
            let _subscriber_handle =
                crate::rust::engine::wal_apply_boot::spawn_boot_apply_subscriber(
                    rx,
                    Arc::clone(&wal_ctx.sync_driver),
                    snap_ctx.snapshot_dir.clone(),
                    registry,
                    allowed_roots,
                    Some(Arc::clone(&wal_ctx.payload_lookup)),
                );
        }

        transition_to_running(
            self.ingest.clone(),
            casper_arc as Arc<dyn MultiParentCasper + Send + Sync>,
            approved_block,
            the_init,
            disable_state_exporter,
            self.transport_layer.clone(),
            self.rp_conf_ask.clone(),
            snapshot_chunk_ctx,
            wal_payload_ctx,
            &self.engine_cell,
            &self.event_publisher,
            self.state_items_tx.clone(),
        )
        .await?;

        // Guard against config drift. The node's local native-token-*
        // values must match what the stored genesis baked into the
        // TokenMetadata contract. Mismatch at startup aborts here, same
        // as the networked path's check inside
        // `Initializing::create_casper_and_transition_to_running`.
        crate::rust::util::token_metadata_check::verify_token_metadata_matches_config(
            &self.runtime_manager,
            &genesis_post_state_hash,
            &self.casper_shard_conf.native_token_name,
            &self.casper_shard_conf.native_token_symbol,
            self.casper_shard_conf.native_token_decimals,
        )
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        // Infrastructure dependencies (Scala implicit parameters)
        transport_layer: Arc<T>,
        rp_conf_ask: RPConf,
        connections_cell: ConnectionsCell,
        last_approved_block: Arc<Mutex<Option<ApprovedBlock>>>,
        event_publisher: F1r3flyEvents,
        engine_cell: Arc<EngineCell>,
        block_store: KeyValueBlockStore,
        block_dag_storage: BlockDagKeyValueStorage,
        deploy_storage: KeyValueDeployStorage,
        rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
        rspace_state_manager: RSpaceStateManager,
        runtime_manager: Arc<RuntimeManager>,
        estimator: Estimator,
        // Explicit parameters (matching Scala signature order)
        ingest: IngestHandle,
        conf: CasperConf,
        trim_state: bool,
        disable_state_exporter: bool,
        heartbeat_signal_ref: crate::rust::heartbeat_signal::HeartbeatSignalRef,
        standalone: bool,
        fs_bundle: Vec<crate::rust::genesis::contracts::fs_genesis::BundleEntry>,
        state_items_tx: Option<
            tokio::sync::mpsc::Sender<
                models::rust::casper::protocol::casper_message::StoreItemsMessage,
            >,
        >,
    ) -> Self {
        // Scala equivalent: val casperShardConf = CasperShardConf(...)
        let casper_shard_conf = CasperShardConf {
            fault_tolerance_threshold: conf.fault_tolerance_threshold,
            // Locally-derived exact ppm from the configured f32. On a joining/
            // existing chain, `initializing` overwrites it with the on-chain ppm
            // (the single exact conversion point).
            fault_tolerance_threshold_ppm: ProofOfStake::fault_tolerance_threshold_to_ppm(
                conf.fault_tolerance_threshold,
            ),
            shard_name: conf.shard_name.clone(),
            parent_shard_id: conf.parent_shard_id.clone(),
            finalization_rate: conf.finalization_rate,
            max_number_of_parents: conf.max_number_of_parents,
            max_parent_depth: conf.max_parent_depth,
            synchrony_constraint_threshold: conf.synchrony_constraint_threshold,
            height_constraint_threshold: conf.height_constraint_threshold,
            deploy_lifespan: conf.deploy_lifespan,
            // Zero derives the budget from the citability window
            // (max-parent-depth heights of cadence) with a 5x margin. The
            // launch always produces a bound: `None` (unbounded) is
            // reserved for constructions that bypass operator conf.
            deploy_play_budget: Some(if conf.deploy_play_budget.is_zero() {
                std::time::Duration::from_millis(
                    ((conf.max_parent_depth as i64).max(1)
                        * (conf.heartbeat_conf.check_interval.as_millis() as i64)
                        / 5) as u64,
                )
            } else {
                conf.deploy_play_budget
            }),
            // A derived budget is provisional: hash_set_casper recomputes it
            // from the ADOPTED max-parent-depth.
            deploy_play_budget_is_derived: conf.deploy_play_budget.is_zero(),
            heartbeat_check_interval: conf.heartbeat_conf.check_interval,
            casper_version: crate::rust::casper::CASPER_PROTOCOL_VERSION,
            bond_minimum: conf.genesis_block_data.bond_minimum.clone(),
            bond_maximum: conf.genesis_block_data.bond_maximum.clone(),
            epoch_length: conf.genesis_block_data.epoch_length,
            quarantine_length: conf.genesis_block_data.quarantine_length,
            min_phlo_price: conf.min_phlo_price.clone(),
            max_phlo_limit: conf.max_phlo_limit,
            deploy_heartbeat_wake_enabled: false,
            disable_validator_progress_check: standalone,
            enable_mergeable_channel_gc: conf.enable_mergeable_channel_gc,
            mergeable_channels_gc_depth_buffer: conf.mergeable_channels_gc_depth_buffer,
            enable_block_store_gc: conf.enable_block_store_gc,
            enable_history_pruning: conf.enable_history_pruning,
            enable_cold_gc_bookkeeping: conf.enable_cold_gc_bookkeeping,
            enable_cold_gc_sweep: conf.enable_cold_gc_sweep,
            synchrony_recovery_stall_window: conf.synchrony_recovery_stall_window,
            synchrony_recovery_cooldown: conf.synchrony_recovery_cooldown,
            synchrony_recovery_max_bypasses: conf.synchrony_recovery_max_bypasses,
            synchrony_finalized_baseline_enabled: conf.synchrony_finalized_baseline_enabled,
            synchrony_finalized_baseline_max_distance: conf
                .synchrony_finalized_baseline_max_distance,
            max_user_deploys_per_block: conf.max_user_deploys_per_block,
            deploy_admission: conf.deploy_admission,
            native_token_name: conf.genesis_block_data.native_token_name.clone(),
            native_token_symbol: conf.genesis_block_data.native_token_symbol.clone(),
            native_token_decimals: conf.genesis_block_data.native_token_decimals,
            active_validators_cache_max_entries:
                crate::rust::casper::ACTIVE_VALIDATORS_CACHE_MAX_ENTRIES_DEFAULT,
        };

        Self {
            // Infrastructure dependencies (implicit parameters)
            transport_layer,
            rp_conf_ask,
            connections_cell,
            last_approved_block,
            event_publisher,
            engine_cell,
            block_store,
            block_dag_storage,
            deploy_storage,
            rejected_deploy_buffer,
            rspace_state_manager,
            runtime_manager,
            estimator,
            casper_shard_conf,
            // Explicit parameters
            ingest,
            conf,
            trim_state,
            disable_state_exporter,
            heartbeat_signal_ref,
            fs_bundle,
            state_items_tx,
        }
    }

    #[tracing::instrument(level = "info", skip(self))]
    async fn connect_as_genesis_validator(&self) -> Result<(), CasperError> {
        // As a genesis validator, native-token-* values from local config are
        // what will be baked into the TokenMetadata contract at genesis (via
        // default_blessed_terms). On-chain state cannot disagree with local
        // config here by construction, so no post-genesis verification is
        // performed on this path.
        tracing::info!(
            event = "native_token_metadata_startup",
            role = "genesis_validator",
            native_token_name = %self.conf.genesis_block_data.native_token_name,
            native_token_symbol = %self.conf.genesis_block_data.native_token_symbol,
            native_token_decimals = self.conf.genesis_block_data.native_token_decimals,
            "Genesis validator: native token metadata will be derived from local config"
        );

        let timestamp = self
            .conf
            .genesis_block_data
            .deploy_timestamp
            .unwrap_or_else(|| {
                SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64
            });

        let bonds = BondsParser::parse_with_autogen(
            &self.conf.genesis_block_data.bonds_file,
            self.conf.genesis_ceremony.autogen_shard_size as usize,
        )
        .map_err(|e| CasperError::RuntimeError(format!("Failed to parse bonds: {}", e)))?;

        let validator_id = ValidatorIdentity::from_private_key_with_logging(
            self.conf.validator_private_key.as_deref(),
        )
        .ok_or_else(|| {
            CasperError::RuntimeError(
                "Validator identity required for genesis validator".to_string(),
            )
        })?;

        let vaults =
            VaultParser::parse_from_path_str(&self.conf.genesis_block_data.wallets_file)
                .map_err(|e| CasperError::RuntimeError(format!("Failed to parse vaults: {}", e)))?;

        let bap = BlockApproverProtocol::new(
            validator_id.clone(),
            timestamp,
            vaults,
            bonds,
            self.conf.genesis_block_data.bond_minimum.clone(),
            self.conf.genesis_block_data.bond_maximum.clone(),
            self.conf.genesis_block_data.epoch_length,
            self.conf.genesis_block_data.quarantine_length,
            self.conf.genesis_block_data.number_of_active_validators,
            self.casper_shard_conf.fault_tolerance_threshold_ppm,
            self.casper_shard_conf.max_parent_depth,
            self.casper_shard_conf.deploy_lifespan,
            self.casper_shard_conf.min_phlo_price.clone(),
            self.casper_shard_conf.max_phlo_limit,
            self.conf.genesis_ceremony.required_signatures,
            self.conf
                .genesis_block_data
                .pos_multi_sig_public_keys
                .clone(),
            self.conf.genesis_block_data.pos_multi_sig_quorum,
            self.conf.genesis_block_data.native_token_name.clone(),
            self.conf.genesis_block_data.native_token_symbol.clone(),
            self.conf.genesis_block_data.native_token_decimals,
            // Threaded from `CasperLaunchImpl::fs_bundle` (populated
            // by `node::rust::runtime::setup::setup_node_program`
            // via `provisioning_merge::project_bundle(&merged)`),
            // so the validator reconstructs the ceremony master's
            // deploy list byte-for-byte.  See
            // `BlockApproverProtocol::validate_candidate` for the
            // shape invariant.
            self.fs_bundle.clone(),
            // Hardcoded `None` matches the ceremony-master's
            // `approve_block_protocol.rs:206`.  When either side
            // lifts cadence from None, update both call sites in
            // lockstep — a divergence trips the deploy-list length
            // check at validate_candidate.
            None,
            self.transport_layer.clone(),
            Arc::new(self.rp_conf_ask.clone()),
        )?;

        // Scala equivalent: EngineCell[F].set(new GenesisValidator(...))
        let genesis_validator = GenesisValidator::new(
            self.ingest.clone(),
            self.casper_shard_conf.clone(),
            validator_id,
            bap,
            self.transport_layer.clone(),
            self.rp_conf_ask.clone(),
            self.connections_cell.clone(),
            self.last_approved_block.clone(),
            self.event_publisher.clone(),
            self.engine_cell.clone(),
            self.block_store.clone(),
            self.block_dag_storage.clone(),
            self.deploy_storage.clone(),
            self.rejected_deploy_buffer.clone(),
            self.rspace_state_manager.clone(),
            self.runtime_manager.clone(),
            self.estimator.clone(),
            self.heartbeat_signal_ref.clone(),
            self.state_items_tx.clone(),
        );

        self.engine_cell.set(Arc::new(genesis_validator)).await;

        Ok(())
    }

    #[tracing::instrument(level = "info", skip(self))]
    async fn init_bootstrap(&self, disable_state_exporter: bool) -> Result<(), CasperError> {
        let validator_id = ValidatorIdentity::from_private_key_with_logging(
            self.conf.validator_private_key.as_deref(),
        );

        // As ceremony master, native-token-* values from local config will be
        // baked into the TokenMetadata contract at genesis (via
        // default_blessed_terms). On-chain state matches local config by
        // construction on this path, so no post-genesis verification is
        // performed. If your chain should use different values, update
        // casper.genesis-block-data.native-token-* before genesis.
        tracing::info!(
            event = "native_token_metadata_startup",
            role = "ceremony_master",
            native_token_name = %self.conf.genesis_block_data.native_token_name,
            native_token_symbol = %self.conf.genesis_block_data.native_token_symbol,
            native_token_decimals = self.conf.genesis_block_data.native_token_decimals,
            "Ceremony master: native token metadata will be baked into genesis from local config"
        );

        tracing::info!(
            bonds_file = %self.conf.genesis_block_data.bonds_file,
            wallets_file = %self.conf.genesis_block_data.wallets_file,
            bond_minimum = %self.conf.genesis_block_data.bond_minimum,
            bond_maximum = %self.conf.genesis_block_data.bond_maximum,
            epoch_length = self.conf.genesis_block_data.epoch_length,
            quarantine_length = self.conf.genesis_block_data.quarantine_length,
            number_of_active_validators = self.conf.genesis_block_data.number_of_active_validators,
            shard_name = %self.casper_shard_conf.shard_name,
            deploy_timestamp = self.conf.genesis_block_data.deploy_timestamp,
            genesis_block_number = self.conf.genesis_block_data.genesis_block_number,
            required_signatures = self.conf.genesis_ceremony.required_signatures,
            approve_duration_ms = self.conf.genesis_ceremony.approve_duration.as_millis(),
            approve_interval_ms = self.conf.genesis_ceremony.approve_interval.as_millis(),
            pos_multi_sig_quorum = self.conf.genesis_block_data.pos_multi_sig_quorum,
            "bootstrap genesis input",
        );

        // Scala equivalent: abp <- ApproveBlockProtocol.of[F](...)
        let abp = ApproveBlockProtocolFactory::create(
            self.conf.genesis_block_data.bonds_file.clone(),
            self.conf.genesis_ceremony.autogen_shard_size,
            self.conf.genesis_block_data.wallets_file.clone(),
            self.conf.genesis_block_data.bond_minimum.clone(),
            self.conf.genesis_block_data.bond_maximum.clone(),
            self.conf.genesis_block_data.epoch_length,
            self.conf.genesis_block_data.quarantine_length,
            self.conf.genesis_block_data.number_of_active_validators,
            self.casper_shard_conf.fault_tolerance_threshold_ppm,
            self.casper_shard_conf.max_parent_depth,
            self.casper_shard_conf.deploy_lifespan,
            self.casper_shard_conf.min_phlo_price.clone(),
            self.casper_shard_conf.max_phlo_limit,
            self.casper_shard_conf.shard_name.clone(),
            self.conf.genesis_block_data.deploy_timestamp,
            self.conf.genesis_ceremony.required_signatures,
            self.conf.genesis_ceremony.approve_duration,
            self.conf.genesis_ceremony.approve_interval,
            self.conf.genesis_block_data.genesis_block_number,
            self.conf
                .genesis_block_data
                .pos_multi_sig_public_keys
                .clone(),
            self.conf.genesis_block_data.pos_multi_sig_quorum,
            self.conf.genesis_block_data.native_token_name.clone(),
            self.conf.genesis_block_data.native_token_symbol.clone(),
            self.conf.genesis_block_data.native_token_decimals,
            self.fs_bundle.clone(),
            &self.runtime_manager,
            self.last_approved_block.clone(),
            Some(self.event_publisher.clone()),
            self.transport_layer.clone(),
            Arc::new(self.connections_cell.clone()),
            Arc::new(self.rp_conf_ask.clone()),
        )
        .await?;

        // Scala equivalent: Concurrent[F].start(GenesisCeremonyMaster.waitingForApprovedBlockLoop[F](...))
        tokio::spawn({
            let ingest = self.ingest.clone();
            let casper_shard_conf = self.casper_shard_conf.clone();
            let validator_id = validator_id.clone();
            let transport_layer = self.transport_layer.clone();
            let rp_conf_ask = self.rp_conf_ask.clone();
            let connections_cell = self.connections_cell.clone();
            let last_approved_block = self.last_approved_block.clone();
            let block_store = self.block_store.clone();
            let block_dag_storage = self.block_dag_storage.clone();
            let deploy_storage = self.deploy_storage.clone();
            let rejected_deploy_buffer = self.rejected_deploy_buffer.clone();
            let event_publisher = self.event_publisher.clone();
            let engine_cell = self.engine_cell.clone();
            let runtime_manager = self.runtime_manager.clone();
            let estimator = self.estimator.clone();
            let heartbeat_signal_ref = self.heartbeat_signal_ref.clone();

            async move {
                if let Err(e) = GenesisCeremonyMaster::waiting_for_approved_block_loop(
                    transport_layer,
                    rp_conf_ask,
                    connections_cell,
                    last_approved_block,
                    &event_publisher,
                    engine_cell,
                    block_store,
                    block_dag_storage,
                    deploy_storage,
                    rejected_deploy_buffer,
                    runtime_manager,
                    estimator,
                    ingest,
                    casper_shard_conf,
                    validator_id,
                    disable_state_exporter,
                    heartbeat_signal_ref,
                )
                .await
                {
                    tracing::error!(error = ?e, "waiting for approved block loop failed");
                }
            }
        });

        let genesis_ceremony_master = GenesisCeremonyMaster::new(Arc::new(abp));
        self.engine_cell
            .set(Arc::new(genesis_ceremony_master))
            .await;

        Ok(())
    }

    /// The unified has-data / no-data launch path. `stored_approved_block`
    /// carries the on-disk approved block (if any) into Initializing's
    /// startup horizon probe: within window, Initializing short-circuits
    /// to Running using that anchor and no LFS is performed; below window,
    /// Initializing runs the usual `request_approved_state` flow (#1059).
    async fn connect_and_query_approved_block(
        &self,
        trim_state: bool,
        disable_state_exporter: bool,
        stored_approved_block: Option<ApprovedBlock>,
    ) -> Result<(), CasperError> {
        let validator_id = ValidatorIdentity::from_private_key_with_logging(
            self.conf.validator_private_key.as_deref(),
        );

        // Scala: CommUtil[F].requestApprovedBlock(trimState) - passed as init to transitionToInitializing
        let transport_layer_for_init = self.transport_layer.clone();
        let rp_conf_ask_for_init = self.rp_conf_ask.clone();

        let init = Arc::new(move || {
            let transport_layer = transport_layer_for_init.clone();
            let rp_conf_ask = rp_conf_ask_for_init.clone();

            Box::pin(async move {
                transport_layer
                    .request_approved_block(&rp_conf_ask, Some(trim_state))
                    .await?;
                Ok(())
            }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
        });

        // Scala equivalent: Engine.transitionToInitializing(...)
        transition_to_initializing(
            &self.ingest,
            &self.casper_shard_conf,
            &validator_id,
            init,
            trim_state,
            disable_state_exporter,
            &self.transport_layer,
            &self.rp_conf_ask,
            &self.connections_cell,
            &self.last_approved_block,
            &self.block_store,
            &self.block_dag_storage,
            &self.deploy_storage,
            &self.rejected_deploy_buffer,
            &self.rspace_state_manager,
            self.event_publisher.clone(),
            &self.engine_cell,
            &self.runtime_manager,
            &self.estimator,
            &self.heartbeat_signal_ref,
            self.state_items_tx.clone(),
            stored_approved_block,
        )
        .await?;

        Ok(())
    }
}

#[async_trait]
impl<T: TransportLayer + Send + Sync + Clone + 'static> CasperLaunch for CasperLaunchImpl<T> {
    async fn launch(&self) -> Result<(), CasperError> {
        let approved_block_opt = self.block_store.get_approved_block()?;

        let (msg, action_result) = match approved_block_opt {
            Some(approved_block) => {
                ensure_compatible_approved_block(&approved_block)?;
                if horizon_probe_peer(&self.rp_conf_ask).is_none() {
                    // Standalone / no-bootstrap: the horizon probe inside
                    // Initializing broadcasts an ApprovedBlockRequest to the
                    // bootstrap. With no bootstrap configured the broadcast
                    // fails deterministically (StandaloneNodeSendToBootstrap
                    // error), so skip the probe and go straight to Running
                    // from the stored anchor. A bootstrap that is this node
                    // cannot answer before this node's servers start, so it
                    // takes the same path (#1120). Networked nodes still route
                    // through Initializing (#1059).
                    let msg = if self.rp_conf_ask.bootstrap.is_some() {
                        "Approved block found, the bootstrap is this node — resuming from the stored anchor"
                    } else {
                        "Approved block found, no bootstrap configured — resuming as standalone"
                    };
                    let action_result = self
                        .connect_direct_to_running(approved_block, self.disable_state_exporter)
                        .await;
                    (msg, action_result)
                } else {
                    // #1059: has-data resume hands the stored block to
                    // Initializing, which sends `ApprovedBlockRequest(trim_
                    // state=true)`, learns the peer-reported `floor_seed.
                    // floor_number`, and either short-circuits to Running
                    // (within window) or runs LFS (below horizon). TLA+
                    // contract: `formal/tlaplus/below_horizon_recovery`.
                    let msg = "Approved block found, probing peer horizon before resuming";
                    let action_result = self
                        .connect_and_query_approved_block(
                            true,
                            self.disable_state_exporter,
                            Some(approved_block),
                        )
                        .await;
                    (msg, action_result)
                }
            }

            None if self.conf.genesis_ceremony.genesis_validator_mode => {
                let msg = "Approved block not found, taking part in ceremony as genesis validator";
                let action_result = self.connect_as_genesis_validator().await;
                (msg, action_result)
            }

            None if self.conf.genesis_ceremony.ceremony_master_mode => {
                let msg = "Approved block not found, taking part in ceremony as ceremony master";
                let action_result = self.init_bootstrap(self.disable_state_exporter).await;
                (msg, action_result)
            }

            None => {
                let msg = "Approved block not found, connecting to existing network";
                let action_result = self
                    .connect_and_query_approved_block(
                        self.trim_state,
                        self.disable_state_exporter,
                        None,
                    )
                    .await;
                (msg, action_result)
            }
        };

        // Scala equivalent: case (msg, action) => Log[F].info(msg) >> action
        tracing::info!("{}", msg);
        action_result
    }
}

#[cfg(test)]
mod tests {
    use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
    use comm::rust::test_instances::create_rp_conf_ask;
    use prost::bytes::Bytes;

    use super::horizon_probe_peer;

    fn peer(key: &str, host: &str) -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from(key.as_bytes().to_vec()),
            },
            endpoint: Endpoint::new(host.to_string(), 40400, 40404),
        }
    }

    #[test]
    fn a_node_without_a_bootstrap_has_no_probe_peer() {
        let mut conf = create_rp_conf_ask(peer("local", "local-host"), None, None);
        conf.bootstrap = None;
        assert_eq!(horizon_probe_peer(&conf), None);
    }

    #[test]
    fn a_node_that_is_its_own_bootstrap_has_no_probe_peer() {
        let mut conf = create_rp_conf_ask(peer("local", "0.0.0.0"), None, None);
        conf.bootstrap = Some(peer("local", "rnode.bootstrap"));
        assert_eq!(horizon_probe_peer(&conf), None);
    }

    #[test]
    fn a_node_with_another_bootstrap_probes_it() {
        let mut conf = create_rp_conf_ask(peer("local", "local-host"), None, None);
        let bootstrap = peer("bootstrap", "rnode.bootstrap");
        conf.bootstrap = Some(bootstrap.clone());
        assert_eq!(horizon_probe_peer(&conf), Some(&bootstrap));
    }

    #[test]
    fn a_bootstrap_with_another_id_on_the_same_host_is_probed() {
        let mut conf = create_rp_conf_ask(peer("local", "shared-host"), None, None);
        let bootstrap = peer("bootstrap", "shared-host");
        conf.bootstrap = Some(bootstrap.clone());
        assert_eq!(horizon_probe_peer(&conf), Some(&bootstrap));
    }
}
