// See casper/src/main/scala/coop/rchain/casper/engine/CasperLaunch.scala

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_trait::async_trait;
use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use dashmap::DashSet;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{ApprovedBlock, BlockMessage};
use rspace_plus_plus::rspace::state::rspace_state_manager::RSpaceStateManager;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::rust::blocks::block_processing_queue::BlockProcessingQueueSender;
use crate::rust::casper::{hash_set_casper, CasperShardConf, MultiParentCasper};
use crate::rust::casper_conf::CasperConf;
use crate::rust::engine::approve_block_protocol::ApproveBlockProtocolFactory;
use crate::rust::engine::block_approver_protocol::BlockApproverProtocol;
use crate::rust::engine::block_retriever::BlockRetriever;
use crate::rust::engine::engine::{
    record_direct_to_running_init_metrics, transition_to_initializing, transition_to_running,
};
use crate::rust::engine::engine_cell::EngineCell;
use crate::rust::engine::genesis_ceremony_master::GenesisCeremonyMaster;
use crate::rust::engine::genesis_validator::GenesisValidator;
use crate::rust::engine::multi_parent_casper::MultiParentCasperImpl;
use crate::rust::engine::running::RunningRecoveryContext;
use crate::rust::errors::CasperError;
use crate::rust::estimator::Estimator;
use crate::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use crate::rust::util::bonds_parser::BondsParser;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::vault_parser::VaultParser;
use crate::rust::validate::Validate;
use crate::rust::validator_identity::ValidatorIdentity;
use crate::rust::ProposeRequestKind;

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
    block_retriever: BlockRetriever<T>,
    engine_cell: Arc<EngineCell>,
    block_store: KeyValueBlockStore,
    block_dag_storage: BlockDagKeyValueStorage,
    deploy_storage: KeyValueDeployStorage,
    rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    casper_buffer_storage: CasperBufferKeyValueStorage,
    rspace_state_manager: RSpaceStateManager,
    runtime_manager: Arc<RuntimeManager>,
    estimator: Estimator,
    casper_shard_conf: CasperShardConf,

    // Explicit parameters from Scala (in same order as Scala signature)
    block_processing_queue_tx: BlockProcessingQueueSender,
    blocks_in_processing: Arc<DashSet<BlockHash>>,
    propose_f_opt: Option<Arc<crate::rust::ProposeFunction>>,
    conf: CasperConf,
    trim_state: bool,
    disable_state_exporter: bool,
    /// Shared reference to heartbeat signal for triggering immediate wake on deploy
    heartbeat_signal_ref: crate::rust::heartbeat_signal::HeartbeatSignalRef,
    /// Static-provisioning bundle for the FsGenesis deploy (Phase
    /// 7 slice 25, C-25-1 review-fix wire-up).  Threaded through
    /// both the proposer path (`ApproveBlockProtocolFactory::create`)
    /// and the validator path (`BlockApproverProtocol::new`) so
    /// they agree on the genesis blessed-deploy sequence.  Node's
    /// boot pipeline populates this via `merge_and_validate` +
    /// `project_bundle`; if it isn't set, defaults to empty (safe
    /// pre-slice-25 behavior).
    fs_bundle: Vec<crate::rust::genesis::contracts::fs_genesis::BundleEntry>,

    /// CRIT-2 fix (2026-08-06): shard-wide consensus filesystem
    /// snapshot cadence, plumbed from the operator's HOCON
    /// `storage.consensus-fs-snapshot-cadence`.  Threaded into both
    /// the proposer's `Genesis.consensus_fs_snapshot_cadence` (via
    /// `ApproveBlockProtocolFactory::create`) and the validator's
    /// `BlockApproverProtocol.consensus_fs_snapshot_cadence` (via
    /// `BlockApproverProtocol::new`).  Cadence is embedded as a
    /// literal in the composed fs_generator source
    /// (`fs_genesis.rs::compose_fs_genesis_source`), so any
    /// cadence mismatch between the proposer's HOCON and any
    /// validator's HOCON causes the reconstructed deploy term to
    /// diverge and `validate_candidate` rejects the candidate —
    /// closing the "shared Genesis hash but silently divergent
    /// snapshot cadence" CRIT-2 gap.
    consensus_fs_snapshot_cadence: Option<u64>,
}

impl<T: TransportLayer + Send + Sync + Clone + 'static> CasperLaunchImpl<T> {
    /// Helper method to create MultiParentCasper instance
    /// Scala equivalent: MultiParentCasper.hashSetCasper[F](validatorId, casperShardConf, ab)
    async fn create_casper(
        &self,
        validator_id: Option<ValidatorIdentity>,
        ab: BlockMessage,
    ) -> Result<MultiParentCasperImpl<T>, CasperError> {
        let runtime_manager = self.runtime_manager.clone();

        hash_set_casper(
            self.block_retriever.clone(),
            self.event_publisher.clone(),
            runtime_manager,
            self.estimator.clone(),
            self.block_store.clone(),
            self.block_dag_storage.clone(),
            self.deploy_storage.clone(),
            self.rejected_deploy_buffer.clone(),
            self.casper_buffer_storage.clone(),
            validator_id,
            self.casper_shard_conf.clone(),
            ab,
            self.heartbeat_signal_ref.clone(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        // Infrastructure dependencies (Scala implicit parameters)
        transport_layer: Arc<T>,
        rp_conf_ask: RPConf,
        connections_cell: ConnectionsCell,
        last_approved_block: Arc<Mutex<Option<ApprovedBlock>>>,
        event_publisher: F1r3flyEvents,
        block_retriever: BlockRetriever<T>,
        engine_cell: Arc<EngineCell>,
        block_store: KeyValueBlockStore,
        block_dag_storage: BlockDagKeyValueStorage,
        deploy_storage: KeyValueDeployStorage,
        rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
        casper_buffer_storage: CasperBufferKeyValueStorage,
        rspace_state_manager: RSpaceStateManager,
        runtime_manager: Arc<RuntimeManager>,
        estimator: Estimator,
        // Explicit parameters (matching Scala signature order)
        block_processing_queue_tx: BlockProcessingQueueSender,
        blocks_in_processing: Arc<DashSet<BlockHash>>,
        propose_f_opt: Option<Arc<crate::rust::ProposeFunction>>,
        conf: CasperConf,
        trim_state: bool,
        disable_state_exporter: bool,
        heartbeat_signal_ref: crate::rust::heartbeat_signal::HeartbeatSignalRef,
        standalone: bool,
        fs_bundle: Vec<crate::rust::genesis::contracts::fs_genesis::BundleEntry>,
        // CRIT-2 (2026-08-06): plumbed from setup.rs's
        // `conf.storage.consensus_fs_snapshot_cadence`.  Threaded
        // into both the proposer (`ApproveBlockProtocolFactory::create`)
        // and validator (`BlockApproverProtocol::new`) genesis paths
        // so cadence appears in the fs_generator deploy term and
        // cadence disagreement fails `validate_candidate` loudly.
        consensus_fs_snapshot_cadence: Option<u64>,
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
            deploy_lifespan: 50,
            casper_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            config_version: 1,
            bond_minimum: conf.genesis_block_data.bond_minimum,
            bond_maximum: conf.genesis_block_data.bond_maximum,
            epoch_length: conf.genesis_block_data.epoch_length,
            quarantine_length: conf.genesis_block_data.quarantine_length,
            min_phlo_price: conf.min_phlo_price,
            // Task #13b: genesis client funding-slot allocations, wired from the
            // shard-genesis `GenesisBlockData` (default EMPTY = back-compat) and
            // hex-lowered once here so a malformed key fails fast at launch. Same
            // shard constant on every node ⇒ the genesis client seed is
            // replay-deterministic.
            client_fuel_allocations: conf
                .genesis_block_data
                .lowered_client_fuel_allocations()
                .expect("invalid client-fuel-allocations in genesis-block-data"),
            // Late block filtering disabled = deploys from "late" blocks (blocks not yet seen by
            // all validators) are included in merged state. Prevents deploy loss during network
            // partitions or validator catchup. Default is true (disabled).
            disable_late_block_filtering: conf.disable_late_block_filtering,
            deploy_heartbeat_wake_enabled: false,
            disable_validator_progress_check: standalone,
            enable_mergeable_channel_gc: conf.enable_mergeable_channel_gc,
            mergeable_channels_gc_depth_buffer: conf.mergeable_channels_gc_depth_buffer,
            finalizer_conf: conf.finalizer.clone(),
            synchrony_recovery_stall_window: conf.synchrony_recovery_stall_window,
            synchrony_recovery_cooldown: conf.synchrony_recovery_cooldown,
            synchrony_recovery_max_bypasses: conf.synchrony_recovery_max_bypasses,
            synchrony_finalized_baseline_enabled: conf.synchrony_finalized_baseline_enabled,
            synchrony_finalized_baseline_max_distance: conf
                .synchrony_finalized_baseline_max_distance,
            max_user_deploys_per_block: conf.max_user_deploys_per_block,
            max_cosigners_per_deploy: conf.genesis_block_data.max_cosigners_per_deploy,
            native_token_name: conf.genesis_block_data.native_token_name.clone(),
            native_token_symbol: conf.genesis_block_data.native_token_symbol.clone(),
            native_token_decimals: conf.genesis_block_data.native_token_decimals,
            // Phase 13: defaults match the previous hardcoded constants
            // (`FINALIZER_BLOCKING_TIMEOUT = 15s`,
            // `MAX_ACTIVE_VALIDATORS_CACHE_ENTRIES = 4096`). When CasperConf
            // gains corresponding fields, plumb them through here.
            active_validators_cache_max_entries: 4096,
        };

        Self {
            // Infrastructure dependencies (implicit parameters)
            transport_layer,
            rp_conf_ask,
            connections_cell,
            last_approved_block,
            event_publisher,
            block_retriever,
            engine_cell,
            block_store,
            block_dag_storage,
            deploy_storage,
            rejected_deploy_buffer,
            casper_buffer_storage,
            rspace_state_manager,
            runtime_manager,
            estimator,
            casper_shard_conf,
            // Explicit parameters
            block_processing_queue_tx,
            blocks_in_processing,
            propose_f_opt,
            conf,
            trim_state,
            disable_state_exporter,
            heartbeat_signal_ref,
            fs_bundle,
            consensus_fs_snapshot_cadence,
        }
    }

    async fn connect_to_existing_network(
        &self,
        approved_block: ApprovedBlock,
        disable_state_exporter: bool,
    ) -> Result<(), CasperError> {
        async fn ask_peers_for_fork_choice_tips<T: TransportLayer + Send + Sync + Clone>(
            transport_layer: &T,
            connections_cell: &ConnectionsCell,
            rp_conf_ask: &RPConf,
        ) -> Result<(), CasperError> {
            transport_layer
                .send_fork_choice_tip_request(connections_cell, rp_conf_ask)
                .await?;
            Ok(())
        }

        async fn send_buffer_pendants_to_casper<T: TransportLayer + Send + Sync + Clone>(
            casper: Arc<dyn MultiParentCasper + Send + Sync>,
            casper_buffer_storage: &CasperBufferKeyValueStorage,
            block_store: &KeyValueBlockStore,
            block_retriever: &BlockRetriever<T>,
            blocks_in_processing: &Arc<DashSet<BlockHash>>,
            block_processing_queue_tx: &BlockProcessingQueueSender,
        ) -> Result<(), CasperError> {
            let _dependency_scan_guard = block_processing_queue_tx.acquire_dependency_scan().await;
            let pendants = casper_buffer_storage.get_pendants();

            // Filter pendants to only those that exist in BlockStore
            let mut pendants_stored = Vec::new();
            for hash_serde in pendants.iter() {
                // Convert BlockHashSerde wrapper to BlockHash (Bytes)
                let hash: BlockHash = hash_serde.0.clone();

                // Check if this hash exists in BlockStore
                let contains = block_store.contains(&hash)?;

                // If block exists, add hash to filtered list
                if contains {
                    pendants_stored.push(hash);
                }
            }

            tracing::info!(
                "Checking pendant hashes: {} items in CasperBuffer.",
                pendants_stored.len()
            );

            // Process each pendant hash and send block to Casper for processing
            for hash in pendants_stored {
                // Retrieve block from BlockStore (returns Option)
                let block = block_store.get(&hash)?;

                if let Some(block) = block {
                    tracing::info!(
                        "Pendant {} is available in BlockStore, sending to Casper.",
                        PrettyPrinter::build_string_bytes(&hash)
                    );

                    // Check if block already exists in DAG
                    let dag_contains = casper.dag_contains(&hash);

                    // Resume-time reconciliation closing the (c) drift
                    // state from Bug #17 / T-9.20. The same purge logic
                    // is provided as a documented helper at
                    // `block_storage::rust::dag::buffer_dag_transition::
                    //  reconcile_buffer_against_dag` — kept inline here
                    // because we additionally clean up the BlockRetriever's
                    // hash-tracking state (a launch-specific concern that
                    // the generic recon helper doesn't know about).
                    // See docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.20.
                    if dag_contains {
                        tracing::warn!(
                            "Pendant {} is already in DAG; purging stale CasperBuffer entry to prevent requeue loops.",
                            PrettyPrinter::build_string_bytes(&hash)
                        );
                        let hash_serde = BlockHashSerde(hash.clone());
                        if let Err(err) = casper_buffer_storage.remove(hash_serde) {
                            tracing::warn!(
                                "Failed to purge stale pendant {} from CasperBuffer: {}",
                                PrettyPrinter::build_string_bytes(&hash),
                                err
                            );
                        }
                        if let Err(err) = block_retriever.forget_hash_tracking(&hash) {
                            tracing::warn!(
                                "Failed to forget stale pendant {} in BlockRetriever: {}",
                                PrettyPrinter::build_string_bytes(&hash),
                                err
                            );
                        }
                        continue;
                    }

                    // Send block to processing queue for validation and addition to DAG
                    let block_hash = block.block_hash.clone();
                    if !blocks_in_processing.insert(block_hash.clone()) {
                        tracing::debug!(
                            "Skipping pendant {} enqueue because it is already queued/in-processing",
                            PrettyPrinter::build_string_bytes(&block_hash)
                        );
                        continue;
                    }
                    match block_processing_queue_tx.try_enqueue(casper.clone(), block) {
                        Ok(()) => block_retriever.ack_receive(hash).await?,
                        Err(error) if error.failure.is_temporary() => {
                            blocks_in_processing.remove(&block_hash);
                            tracing::info!(
                                error = %error,
                                "Deferred buffered pendant {}",
                                PrettyPrinter::build_string_bytes(&block_hash)
                            );
                        }
                        Err(error) => {
                            blocks_in_processing.remove(&block_hash);
                            return Err(CasperError::Other(error.to_string()));
                        }
                    }
                }
            }

            Ok(())
        }

        let validator_id = ValidatorIdentity::from_private_key_with_logging(
            self.conf.validator_private_key.as_deref(),
        );

        let ab = approved_block.candidate.block.clone();
        let genesis_post_state_hash = ab.body.state.post_state_hash.clone();

        let casper = self.create_casper(validator_id.clone(), ab).await?;
        let casper_arc = Arc::new(casper);

        // Scala equivalent: init = for { _ <- askPeersForForkChoiceTips; _ <- sendBufferPendantsToCasper(casper); _ <- proposeFOpt.traverse(...) } yield ()
        // Create lazy async init computation (matches Scala F[Unit])

        // Note: Double cloning is necessary because:
        // 1. First clone: capture in outer closure (needs to be Fn, not FnOnce)
        // 2. Second clone: move into inner async block
        let transport_layer_for_init = self.transport_layer.clone();
        let connections_cell_for_init = self.connections_cell.clone();
        let rp_conf_ask_for_init = self.rp_conf_ask.clone();
        let casper_for_init = casper_arc.clone();
        let casper_buffer_storage_for_init = self.casper_buffer_storage.clone();
        let block_store_for_init = self.block_store.clone();
        let block_retriever_for_init = self.block_retriever.clone();
        let blocks_in_processing_for_init = self.blocks_in_processing.clone();
        let block_processing_queue_tx_for_init = self.block_processing_queue_tx.clone();
        let propose_f_opt_for_init = self.propose_f_opt.clone();

        let the_init = Arc::new(move || {
            let transport_layer = transport_layer_for_init.clone();
            let connections_cell = connections_cell_for_init.clone();
            let rp_conf_ask = rp_conf_ask_for_init.clone();
            let casper = casper_for_init.clone();
            let casper_buffer_storage = casper_buffer_storage_for_init.clone();
            let block_store = block_store_for_init.clone();
            let block_retriever = block_retriever_for_init.clone();
            let blocks_in_processing = blocks_in_processing_for_init.clone();
            let block_processing_queue_tx = block_processing_queue_tx_for_init.clone();
            let propose_f_opt = propose_f_opt_for_init.clone();

            Box::pin(async move {
                ask_peers_for_fork_choice_tips(&*transport_layer, &connections_cell, &rp_conf_ask)
                    .await?;

                send_buffer_pendants_to_casper(
                    casper.clone(),
                    &casper_buffer_storage,
                    &block_store,
                    &block_retriever,
                    &blocks_in_processing,
                    &block_processing_queue_tx,
                )
                .await?;

                if let Some(propose_f) = propose_f_opt.as_ref() {
                    propose_f(ProposeRequestKind::PendingDeploy).await?;
                }

                Ok(())
            }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
        });

        // Direct-to-running path: emit init metrics that are otherwise produced in Initializing.
        record_direct_to_running_init_metrics();

        // Phase 7b-1 (2026-08-27): build the snapshot chunk-fetch
        // context if this node has an fs_snapshot_writer.  On
        // observer nodes or misconfigured deployments this returns
        // None and snapshot dispatch stays disabled.
        let snapshot_chunk_ctx =
            crate::rust::engine::snapshot_chunk_sync::build_snapshot_chunk_context(
                &self.runtime_manager,
            )
            .await;

        // Phase 7b-2 (2026-08-27): build the WAL payload-fetch
        // context.  The payload lookup is derived from the shared
        // `RuntimeManager.payload_store` bundle, which the boot
        // pipeline populated with a `DirectoryPayloadStore`
        // pointing at `<data-dir>/wal_payload_store/`.  Leader-side
        // writes and joiner-side reads hit the same on-disk dir.
        //
        // Falls back to an empty in-memory store if the runtime
        // manager slot is None (test harnesses that don't wire the
        // boot pipeline).  The empty store just returns
        // UnknownPayload on every request, which is safe.
        let wal_payload_ctx = {
            use std::sync::Arc as StdArc;

            use crate::rust::engine::running::WalPayloadContext;
            use crate::rust::engine::wal_payload_retriever::WalPayloadRetriever;
            use crate::rust::engine::wal_payload_server::{InMemoryPayloadStore, PayloadLookup};
            use crate::rust::engine::wal_payload_sync::WalPayloadSyncDriver;
            let retriever = StdArc::new(WalPayloadRetriever::new());
            let sync_driver = StdArc::new(WalPayloadSyncDriver::new(StdArc::clone(&retriever)));
            let lookup: StdArc<dyn PayloadLookup> =
                match self.runtime_manager.get_payload_store().await {
                    Some(b) => b.lookup,
                    None => StdArc::new(InMemoryPayloadStore::new()),
                };
            Some(WalPayloadContext {
                sync_driver,
                payload_lookup: lookup,
                // DD-7b-3 (a) tick-stop handle is installed later
                // inside `transition_to_running` after `spawn_
                // periodic_tick` runs; None here means "no live
                // tick loop yet" (default for the pre-transition
                // ctx).
                tick_stop: None,
            })
        };

        // Phase 7b-2 item (c) (2026-08-28): boot wire-in for the
        // apply-to-follower flow.  Install a completion sink on
        // the snapshot chunk driver + spawn the subscriber that
        // decodes each completed snapshot and drives the WAL
        // payload fetch + applier.  Only wired when BOTH contexts
        // are Some — otherwise there's no fetch driver or
        // snapshot dir to drive against.
        if let (Some(snap_ctx), Some(wal_ctx)) =
            (snapshot_chunk_ctx.as_ref(), wal_payload_ctx.as_ref())
        {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<
                crate::rust::engine::snapshot_chunk_sync::SnapshotCompletion,
            >();
            snap_ctx.sync_driver.install_completion_sink(tx);
            // TODO(fileio): plumb the consensus-static roots from
            // the operator's provisioning config into `allowed_roots`
            // as defense-in-depth against a leader-canonicalize bug
            // or forged snapshot writing outside the joiner's
            // managed tree.  Empty vector = validation skipped;
            // the trust anchor is Blake2b256 preimage resistance
            // via the Merkle root check upstream.
            let allowed_roots: Vec<std::path::PathBuf> = Vec::new();
            let _handle = crate::rust::engine::wal_apply_boot::spawn_boot_apply_subscriber(
                rx,
                std::sync::Arc::clone(&wal_ctx.sync_driver),
                snap_ctx.snapshot_dir.clone(),
                allowed_roots,
            );
        }

        // Scala equivalent: Engine.transitionToRunning[F](...)
        transition_to_running(
            self.block_processing_queue_tx.clone(),
            self.blocks_in_processing.clone(),
            casper_arc,
            approved_block,
            the_init,
            disable_state_exporter,
            self.transport_layer.clone(),
            self.rp_conf_ask.clone(),
            self.block_retriever.clone(),
            Some(RunningRecoveryContext {
                connections_cell: self.connections_cell.clone(),
            }),
            snapshot_chunk_ctx,
            wal_payload_ctx,
            &self.engine_cell,
            &self.event_publisher,
        )
        .await?;

        // Guard against config drift: a joiner's local native-token-* values
        // must match what this network actually baked into the TokenMetadata
        // contract at genesis. If they disagree, the node's /api/status would
        // advertise values that contradict on-chain state, which misleads
        // block explorers and wallets.
        crate::rust::util::token_metadata_check::verify_token_metadata_matches_config(
            &self.runtime_manager,
            &genesis_post_state_hash,
            &self.conf.genesis_block_data.native_token_name,
            &self.conf.genesis_block_data.native_token_symbol,
            self.conf.genesis_block_data.native_token_decimals,
        )
        .await?;

        Ok(())
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
            self.conf.genesis_block_data.bond_minimum,
            self.conf.genesis_block_data.bond_maximum,
            self.conf.genesis_block_data.epoch_length,
            self.conf.genesis_block_data.quarantine_length,
            self.conf.genesis_block_data.number_of_active_validators,
            self.casper_shard_conf.fault_tolerance_threshold_ppm,
            self.conf.genesis_ceremony.required_signatures,
            self.conf
                .genesis_block_data
                .pos_multi_sig_public_keys
                .clone(),
            self.conf.genesis_block_data.pos_multi_sig_quorum,
            self.conf.genesis_block_data.max_cosigners_per_deploy,
            self.conf.genesis_block_data.initial_phlogiston,
            self.conf.genesis_block_data.epoch_phlogiston,
            self.casper_shard_conf.casper_version,
            self.casper_shard_conf.client_fuel_allocations.clone(),
            self.conf.genesis_block_data.native_token_name.clone(),
            self.conf.genesis_block_data.native_token_symbol.clone(),
            self.conf.genesis_block_data.native_token_decimals,
            self.fs_bundle.clone(),
            // CRIT-2 (2026-08-06): propagate cadence to BAP so
            // validator-side `validate_candidate` reconstructs an
            // fs_generator deploy term that matches the proposer's
            // (or rejects loudly on mismatch).
            self.consensus_fs_snapshot_cadence,
            self.transport_layer.clone(),
            Arc::new(self.rp_conf_ask.clone()),
        )?;

        // Scala equivalent: EngineCell[F].set(new GenesisValidator(...))
        let genesis_validator = GenesisValidator::new(
            self.block_processing_queue_tx.clone(),
            self.blocks_in_processing.clone(),
            self.casper_shard_conf.clone(),
            validator_id,
            bap,
            self.transport_layer.clone(),
            self.rp_conf_ask.clone(),
            self.connections_cell.clone(),
            self.last_approved_block.clone(),
            self.event_publisher.clone(),
            self.block_retriever.clone(),
            self.engine_cell.clone(),
            self.block_store.clone(),
            self.block_dag_storage.clone(),
            self.deploy_storage.clone(),
            self.rejected_deploy_buffer.clone(),
            self.casper_buffer_storage.clone(),
            self.rspace_state_manager.clone(),
            self.runtime_manager.clone(),
            self.estimator.clone(),
            self.heartbeat_signal_ref.clone(),
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
            bond_minimum = self.conf.genesis_block_data.bond_minimum,
            bond_maximum = self.conf.genesis_block_data.bond_maximum,
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
            self.conf.genesis_block_data.bond_minimum,
            self.conf.genesis_block_data.bond_maximum,
            self.conf.genesis_block_data.epoch_length,
            self.conf.genesis_block_data.quarantine_length,
            self.conf.genesis_block_data.number_of_active_validators,
            self.casper_shard_conf.fault_tolerance_threshold_ppm,
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
            self.conf.genesis_block_data.max_cosigners_per_deploy,
            self.conf.genesis_block_data.initial_phlogiston,
            self.conf.genesis_block_data.epoch_phlogiston,
            self.casper_shard_conf.casper_version,
            self.casper_shard_conf.client_fuel_allocations.clone(),
            self.conf.genesis_block_data.native_token_name.clone(),
            self.conf.genesis_block_data.native_token_symbol.clone(),
            self.conf.genesis_block_data.native_token_decimals,
            self.fs_bundle.clone(),
            // CRIT-2 (2026-08-06): plumb cadence to the proposer's
            // Genesis composition so it lands in the fs_generator
            // deploy term.
            self.consensus_fs_snapshot_cadence,
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
            let block_processing_queue_tx = self.block_processing_queue_tx.clone();
            let blocks_in_processing = self.blocks_in_processing.clone();
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
            let casper_buffer_storage = self.casper_buffer_storage.clone();
            let event_publisher = self.event_publisher.clone();
            let block_retriever = self.block_retriever.clone();
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
                    block_retriever,
                    engine_cell,
                    block_store,
                    block_dag_storage,
                    deploy_storage,
                    rejected_deploy_buffer,
                    casper_buffer_storage,
                    runtime_manager,
                    estimator,
                    block_processing_queue_tx,
                    blocks_in_processing,
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

    async fn connect_and_query_approved_block(
        &self,
        trim_state: bool,
        disable_state_exporter: bool,
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
            &self.block_processing_queue_tx,
            &self.blocks_in_processing,
            &self.casper_shard_conf,
            self.conf.genesis_ceremony.required_signatures,
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
            &self.casper_buffer_storage,
            &self.rspace_state_manager,
            self.event_publisher.clone(),
            self.block_retriever.clone(),
            &self.engine_cell,
            &self.runtime_manager,
            &self.estimator,
            &self.heartbeat_signal_ref,
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
                let msg = "Approved block found, reconnecting to existing network";
                let action_result = if Validate::approved_block(
                    &approved_block,
                    self.conf.genesis_ceremony.required_signatures,
                ) {
                    self.connect_to_existing_network(approved_block, self.disable_state_exporter)
                        .await
                } else {
                    Err(CasperError::RuntimeError(
                        "stored ApprovedBlock is not a valid canonical genesis approval"
                            .to_string(),
                    ))
                };
                (msg, action_result)
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
                    .connect_and_query_approved_block(self.trim_state, self.disable_state_exporter)
                    .await;
                (msg, action_result)
            }
        };

        // Scala equivalent: case (msg, action) => Log[F].info(msg) >> action
        tracing::info!("{}", msg);
        action_result
    }
}
