// See node/src/main/scala/coop/rchain/node/runtime/Setup.scala
// Imports needed for function signature and return type
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use casper::rust::blocks::block_processing_queue::{
    BlockProcessingQueueReceiver, BlockProcessingQueueSender,
};
use casper::rust::blocks::block_processor::BlockProcessor;
use casper::rust::blocks::proposer::proposer::{ProductionProposer, ProposerResult};
use casper::rust::engine::block_retriever::BlockRetriever;
use casper::rust::engine::casper_launch::CasperLaunch;
use casper::rust::errors::CasperError;
use casper::rust::metrics_constants::{
    PROPOSER_QUEUE_PENDING_METRIC, PROPOSER_QUEUE_REJECTED_TOTAL_METRIC, VALIDATOR_METRICS_SOURCE,
};
use casper::rust::state::instances::ProposerState;
use casper::rust::ProposeFunction;
use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::p2p::packet_handler::PacketHandler;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::ApprovedBlock;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{debug, info, trace, warn};

use crate::rust::api::admin_web_api::AdminWebApi;
use crate::rust::api::web_api::WebApi;
use crate::rust::configuration::NodeConf;
use crate::rust::instances::proposer_coalescer::{
    AdmissionOutcome, ProposalRequestKind, ProposerCoalescer,
};
use crate::rust::instances::proposer_instance::ProposeQueueEntry;
use crate::rust::runtime::api_servers::APIServers;
use crate::rust::runtime::node_runtime::{CasperLoop, EngineInit};
use crate::rust::web::reporting_routes::{ReportingHttpRoutes, ReportingRoutes};

const BLOCK_PROCESSOR_QUEUE_MAX_PENDING: usize = 2_048;

fn block_processor_queue_max_pending() -> usize { BLOCK_PROCESSOR_QUEUE_MAX_PENDING }

pub(crate) async fn setup_node_program<T: TransportLayer + Send + Sync + Clone + 'static>(
    rp_connections: ConnectionsCell,
    rp_conf_cell: comm::rust::rp::rp_conf::RPConfCell,
    transport_layer: Arc<T>,
    block_retriever: BlockRetriever<T>,
    conf: NodeConf,
    event_publisher: F1r3flyEvents,
    node_discovery: Arc<dyn NodeDiscovery + Send + Sync>,
    last_approved_block: Arc<Mutex<Option<ApprovedBlock>>>,
) -> Result<
    (
        Arc<dyn PacketHandler>,
        APIServers,
        CasperLoop,
        CasperLoop,
        EngineInit,
        Arc<dyn CasperLaunch>,
        ReportingHttpRoutes,
        Arc<dyn WebApi + Send + Sync + 'static>,
        Arc<dyn AdminWebApi + Send + Sync + 'static>,
        Option<ProductionProposer<T>>,
        mpsc::Receiver<ProposeQueueEntry>,
        Option<Arc<RwLock<ProposerState>>>,
        BlockProcessor<T>,
        Arc<dashmap::DashSet<BlockHash>>,
        BlockProcessingQueueSender,
        BlockProcessingQueueReceiver,
        Option<Arc<ProposeFunction>>,
        Arc<casper::rust::api::block_report_api::BlockReportAPI>,
        block_storage::rust::key_value_block_store::KeyValueBlockStore,
        // Heartbeat dependencies
        Option<casper::rust::validator_identity::ValidatorIdentity>,
        Arc<casper::rust::engine::engine_cell::EngineCell>,
        casper::rust::casper_conf::HeartbeatConf,
        i32, // max_number_of_parents for heartbeat safety check
        casper::rust::heartbeat_signal::HeartbeatSignalRef,
        // Mergeable channels GC loop (optional - only when GC enabled)
        Option<CasperLoop>,
    ),
    CasperError,
> {
    info!(data_dir = ?conf.storage.data_dir, "Initializing key-value store manager");

    // Snapshot the node's data directory before it's consumed by
    // `new_key_value_store_manager` below.  Downstream Phase 7b-2
    // wiring uses this to derive `<data-dir>/wal_payload_store/`
    // for the shared `DirectoryPayloadStore` (DD-7b-1 (a)).
    let data_dir_snapshot: std::path::PathBuf = conf.storage.data_dir.clone();

    // RNode key-value store manager / manages LMDB databases
    let mut rnode_store_manager = {
        use casper::rust::storage::rnode_key_value_store_manager::new_key_value_store_manager;

        new_key_value_store_manager(conf.storage.data_dir, None)
    };

    // Block storage
    let block_store = {
        use block_storage::rust::key_value_block_store::KeyValueBlockStore;

        KeyValueBlockStore::create_from_kvm(&mut rnode_store_manager).await?
    };

    // Last finalized Block storage
    let last_finalized_storage = {
        use block_storage::rust::finality::LastFinalizedKeyValueStorage;

        LastFinalizedKeyValueStorage::create_from_kvm(&mut rnode_store_manager).await?
    };

    // Migrate LastFinalizedStorage to BlockDagStorage
    let lfb_require_migration = last_finalized_storage.require_migration()?;
    if lfb_require_migration {
        use tracing::info;

        info!("Checking whether legacy LastFinalizedStorage can enter protocol-v5 storage.");
        last_finalized_storage
            .migrate_lfb(&mut rnode_store_manager, &block_store)
            .await?;
    }
    info!(
        lfb_migration = lfb_require_migration,
        "LastFinalized storage checked"
    );

    // Block DAG storage
    let block_dag_storage = {
        use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;

        BlockDagKeyValueStorage::new(&mut rnode_store_manager).await?
    };

    // Casper requesting blocks cache
    let casper_buffer_storage = {
        use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;

        CasperBufferKeyValueStorage::new_from_kvm(&mut rnode_store_manager).await?
    };

    // Deploy storage
    let (deploy_storage, deploy_storage_arc) = {
        use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;

        let deploy_storage = KeyValueDeployStorage::new(&mut rnode_store_manager).await?;
        // Phase 9 (A-3): deploy_storage uses parking_lot::Mutex.
        let deploy_storage_arc = Arc::new(parking_lot::Mutex::new(deploy_storage.clone()));
        (deploy_storage, deploy_storage_arc)
    };

    // Buffer of deploys rejected during multi-parent merge; re-proposed in
    // subsequent blocks to avoid silent loss of otherwise-valid user deploys.
    let rejected_deploy_buffer_arc = {
        use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;

        let buffer = KeyValueRejectedDeployBuffer::new(&mut rnode_store_manager).await?;
        Arc::new(Mutex::new(buffer))
    };

    // Safety oracle (clique oracle implementation)
    let oracle = {
        use casper::rust::safety_oracle::CliqueOracleImpl;

        CliqueOracleImpl
    };

    // Estimator
    let estimator = {
        use casper::rust::estimator::Estimator;

        Estimator::apply(
            conf.casper.max_number_of_parents,
            Some(conf.casper.max_parent_depth),
        )
    };

    // Determine if this node is a validator
    let is_validator = conf.casper.validator_private_key.is_some();
    info!(
        validator = is_validator,
        autopropose = conf.autopropose,
        "Node role determined"
    );

    // Create external services based on node type
    // Load OpenAI config from HOCON with environment variable override
    let external_services = {
        use rholang::rust::interpreter::external_services::ExternalServices;
        use rholang::rust::interpreter::ollama_service::OllamaConfig;
        use rholang::rust::interpreter::openai_service::OpenAIConfig;

        // Load config from HOCON values, with env vars taking priority
        let config = OpenAIConfig::from_config_values(
            conf.openai.enabled,
            conf.openai.api_key.clone(),
            conf.openai.validate_api_key,
            conf.openai.validation_timeout_sec,
        );
        let ollama_config = OllamaConfig::from_env();
        ExternalServices::for_node_type(is_validator, &config, &ollama_config)
    };

    // Runtime for `rnode eval`
    let eval_runtime = {
        use rholang::rust::interpreter::matcher::r#match::Matcher;
        use rholang::rust::interpreter::rho_runtime;
        use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

        let eval_stores = rnode_store_manager
            .eval_stores()
            .await
            .map_err(|e| CasperError::Other(format!("Failed to get eval stores: {}", e)))?;

        rho_runtime::create_runtime_from_kv_store(
            eval_stores,
            Arc::new(casper::rust::genesis::genesis::Genesis::default_mergeable_tags()),
            false,
            &mut Vec::new(),
            Arc::new(Box::new(Matcher)),
            external_services.clone(),
        )
        .await
    };

    // Runtime manager (play and replay runtimes)
    let (runtime_manager, history_repo) = {
        use casper::rust::genesis::genesis::Genesis;
        use casper::rust::util::rholang::runtime_manager::RuntimeManager;
        use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

        let rspace_stores = rnode_store_manager
            .r_space_stores()
            .await
            .map_err(|e| CasperError::Other(format!("Failed to get rspace stores: {}", e)))?;

        let mergeable_store = RuntimeManager::mergeable_store(&mut rnode_store_manager).await?;
        tracing::debug!("[Setup] Creating RuntimeManager with history...");
        let result = RuntimeManager::create_with_history(
            rspace_stores,
            mergeable_store,
            Arc::new(Genesis::default_mergeable_tags()),
            external_services.clone(),
        );
        tracing::debug!("[Setup] RuntimeManager created successfully");
        result
    };

    // Slice 33 (HIGH-4 FIPS fix, Phase 7 whole-review): wire the
    // SnapshotWriter into the RuntimeManager so consensus-mode WAL
    // slices actually get persisted to disk on cadence-hit blocks.
    //
    // Slice 30c (LFB-cadence semantics, Phase A): cadence is now a
    // shard-wide `Genesis` parameter — all validators agree at
    // genesis on which block heights are snapshot boundaries, so
    // the join protocol has a canonical answer for "give me the
    // snapshot at finalized block N."  The HOCON key
    // `storage.consensus-fs-snapshot-cadence` is DEPRECATED.  This
    // boot-time site is a transitional bridge that still reads
    // HOCON so slice-30c-1 doesn't regress HIGH-4: a follow-up
    // slice (30c-2) will move the SnapshotWriter attachment to
    // after `Genesis` is loaded and drop the HOCON read.  If both
    // are set and disagree, Genesis will win at that point.
    #[allow(deprecated)]
    {
        use crate::rust::configuration::provisioning_merge::merge_and_validate;
        use crate::rust::configuration::snapshot_config::build_snapshot_writer;

        // L-9 note (2026-08-06): this is one of two boot-time
        // `merge_and_validate` calls (the other is in the
        // genesis-bundle projection ~350 lines below).  Same
        // input, same output — technically wasted work but
        // measured in microseconds, and deduplication would
        // require `Clone` derivation on `FileIoConfigError`
        // (or Arc-wrapping the Result to survive the panic-on-
        // error dispatch on the genesis side).  L-9 finding
        // explicitly rated as "not a correctness issue"; kept
        // separate to preserve the different error-handling
        // policies (soft-fail here, panic there).
        let merged = merge_and_validate(
            conf.storage.file_io_provisioning.clone(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap_or_else(|errs| {
            let _ = errs;
            crate::rust::configuration::file_io_provisioning::FileIoProvisioning::default()
        });
        if conf.storage.consensus_fs_snapshot_cadence.is_some() {
            tracing::warn!(
                target: "f1r3fly.fs_wal.snapshot",
                "storage.consensus-fs-snapshot-cadence is DEPRECATED (slice 30c): \
                 cadence is a shard-wide Genesis parameter.  This HOCON value is \
                 read as a transitional bridge until slice 30c-2 relocates the \
                 SnapshotWriter attachment to after genesis-load."
            );
        }
        // H-4 fix (2026-08-06): decode the validator's identity
        // private key from hex for manifest signing.  Observer
        // nodes without an identity get `None` and produce
        // unsigned manifests (with a boot warning).
        let signer_sk: Option<Vec<u8>> = conf
            .casper
            .validator_private_key
            .as_deref()
            .and_then(|hex_str| hex::decode(hex_str).ok());
        // Phase 7b-2 retention (DD-7b-1 (y), 2026-08-27): thread
        // the payload store dir through the writer so the
        // finalization runner can prune payload files that no
        // retained snapshot references anymore.  Kept in sync with
        // the `data_dir_snapshot.join("wal_payload_store")` path
        // used a few lines below to construct the payload store
        // itself — if these ever disagree, retention would prune
        // the wrong directory.
        let payload_dir_for_retention = Some(data_dir_snapshot.join("wal_payload_store"));
        let writer = build_snapshot_writer(
            &merged,
            conf.storage.consensus_fs_snapshot_cadence,
            conf.storage.consensus_fs_snapshot_dir.as_deref(),
            conf.storage.consensus_fs_snapshot_retain,
            signer_sk,
            payload_dir_for_retention,
        )
        // F-30b-1 (2026-08-24): retain is a required operator value.
        // Boot rejects with RetainTooSmall if <2 while consensus
        // provisioning is present.
        .unwrap_or_else(|e| {
            panic!("snapshot-config validation failed at boot; refusing to start: {e}")
        });
        if writer.is_some() {
            tracing::info!(
                target: "f1r3fly.fs_wal.snapshot",
                cadence = ?conf.storage.consensus_fs_snapshot_cadence,
                dir = ?conf.storage.consensus_fs_snapshot_dir,
                "SnapshotWriter attached — consensus-mode WAL slices will be persisted on cadence-hit blocks"
            );
        } else {
            tracing::debug!(
                target: "f1r3fly.fs_wal.snapshot",
                "no consensus-static provisioning; SnapshotWriter not attached"
            );
        }
        runtime_manager.set_fs_snapshot_writer(writer).await;

        // Phase 7b-2 (2026-08-27): install the shared payload
        // persistence backend.  DD-7b-1 (a) commits to a sibling
        // dir under the node's data directory
        // (`<data-dir>/wal_payload_store/<hex(hash)>`); lifecycle
        // stays independent of the snapshot dir so a snapshot
        // cleanup can't accidentally delete live payloads.
        //
        // Installed on EVERY node (not just consensus-provisioned
        // ones): observer nodes never run a Consensus-cap write
        // handler so the store stays empty, but a node that
        // starts as observer and later gains a validator key
        // won't need a restart to start persisting write bytes.
        //
        // The same `Arc<DirectoryPayloadStore>` is also threaded
        // into `WalPayloadContext.payload_lookup` at the three
        // boot sites (`casper_launch`, `initializing`,
        // `genesis_ceremony_master`) via
        // `RuntimeManager::get_payload_store` — leader-side writes
        // and joiner-side reads hit the same on-disk dir.
        {
            use casper::rust::engine::wal_payload_server::{
                DirectoryPayloadStore, PayloadStoreBundle,
            };
            let payload_dir = data_dir_snapshot.join("wal_payload_store");
            if let Err(e) = std::fs::create_dir_all(&payload_dir) {
                tracing::warn!(
                    target: "f1r3fly.fs_wal.payload_store",
                    error = %e,
                    dir = ?payload_dir,
                    "Phase 7b-2: could not create wal_payload_store dir; \
                     leader-side write persistence will fail per-op with the same \
                     error, but the node can still run as a joiner"
                );
            }
            let bundle =
                PayloadStoreBundle::from_directory(DirectoryPayloadStore::new(payload_dir.clone()));
            runtime_manager.set_payload_store(Some(bundle)).await;
            tracing::info!(
                target: "f1r3fly.fs_wal.payload_store",
                dir = ?payload_dir,
                "Phase 7b-2: payload store attached — Consensus-cap writes will \
                 be persisted content-addressed for joining validators"
            );
        }

        // DD-7b-2 (a) Option 2 (2026-08-29): wire the block-storage-
        // backed payload-source recorder so every Consensus-cap
        // `journal_write` records a `payload_hash → deploy_sig`
        // entry into the LMDB index co-located with `deploy_index`.
        // Joining validators consult this index at boot (via the
        // Option 2 tier of `apply_wal_slice_after_fetch`'s two-tier
        // reducer) to reproduce write bytes from block-stored
        // deploys — closing the gap Option 1's local
        // `PayloadLookup` leaves for first-time joiners with empty
        // payload stores.
        //
        // Installed on EVERY node like the payload store above:
        // observer nodes never fire a Consensus-cap write handler
        // so the index stays empty, but a node that starts as
        // observer and later gains a validator key won't need a
        // restart to start populating the index.
        {
            use casper::rust::engine::wal_payload_server::BlockStorageBackedRecorder;
            let recorder = std::sync::Arc::new(BlockStorageBackedRecorder::new(
                block_dag_storage.clone(),
            ))
                as std::sync::Arc<dyn rholang::rust::interpreter::io::wal::PayloadSourceRecorder>;
            runtime_manager.set_payload_source_recorder(Some(recorder)).await;
            tracing::info!(
                target: "f1r3fly.fs_wal.payload_source_index",
                "DD-7b-2 (a) Option 2: payload-source recorder attached — Consensus-cap \
                 writes will record payload_hash → deploy_sig for joiner-side reproduction"
            );
        }

        // H-5 fix (2026-08-06): populate the root-identity registry
        // with (dev, inode) captured from each provisioned root at
        // boot.  Every subsequent syscall on a shared runtime asks
        // `handles.root_registry.get(&root_pb)` and passes the answer
        // to `safe_descend_verified`, which fstats the opened directory
        // and rejects mismatches with `QuarantineError::RootIdentityChanged`.
        //
        // Shape mirrors `format_bundle_for_rholang` (fs_genesis.rs
        // §356) so the roots handlers see at deploy time are exactly
        // the roots we register at boot:
        //   - FILE entries: root = parent(canon_path)
        //   - DIR  entries: root = canon_path
        //
        // If a provisioned path is missing at boot we skip it with a
        // warn — the same path will fail its first syscall with a
        // real IO error, which is the correct behavior for a
        // vanished root.  We do NOT panic here so an operator can
        // still boot a node whose oracle-static entry is temporarily
        // unavailable.
        {
            use rholang::rust::interpreter::io::path::capture_root_identity;
            let register_file = |path: &std::path::Path| {
                let parent = match path.parent() {
                    Some(p) => p.to_path_buf(),
                    None => {
                        tracing::warn!(
                            target: "f1r3fly.fs_wal.root_identity",
                            path = ?path,
                            "H-5: static-file entry has no parent directory; skipping identity capture"
                        );
                        return;
                    }
                };
                match capture_root_identity(&parent) {
                    Ok(id) => runtime_manager.register_root_identity(parent, id),
                    Err(e) => tracing::warn!(
                        target: "f1r3fly.fs_wal.root_identity",
                        path = ?parent,
                        error = %e,
                        "H-5: could not stat provisioned file's parent; first syscall on this root will fail with QuarantineError::RootIdentityChanged"
                    ),
                }
            };
            let register_dir = |path: &std::path::Path| {
                let owned = path.to_path_buf();
                match capture_root_identity(&owned) {
                    Ok(id) => runtime_manager.register_root_identity(owned, id),
                    Err(e) => tracing::warn!(
                        target: "f1r3fly.fs_wal.root_identity",
                        path = ?owned,
                        error = %e,
                        "H-5: could not stat provisioned dir; first syscall on this root will fail with QuarantineError::RootIdentityChanged"
                    ),
                }
            };
            for entry in merged.oracle_static_files.values() {
                register_file(&entry.path);
            }
            for entry in merged.consensus_static_files.values() {
                register_file(&entry.path);
                // c-2 review-follow-up (2026-08-30): mirror-register
                // as a consensus-static root so the boot subscriber
                // can build `allowed_roots` for defense-in-depth
                // validation of applier target paths.
                runtime_manager
                    .register_consensus_static_root(entry.path.clone())
                    .await;
            }
            for entry in merged.oracle_static_dirs.values() {
                register_dir(&entry.path);
            }
            for entry in merged.consensus_static_dirs.values() {
                register_dir(&entry.path);
                runtime_manager
                    .register_consensus_static_root(entry.path.clone())
                    .await;
            }
            tracing::info!(
                target: "f1r3fly.fs_wal.root_identity",
                registered = runtime_manager.root_identity_count(),
                "H-5: root-identity registry populated from static provisioning"
            );
        }
    }

    // Reporting runtime
    let reporting_runtime = {
        use casper::rust::reporting_casper;
        use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

        if conf.api_server.enable_reporting {
            // In reporting replay channels map is not needed
            let rspace_stores = rnode_store_manager
                .r_space_stores()
                .await
                .map_err(|e| CasperError::Other(format!("Failed to get rspace stores: {}", e)))?;
            reporting_casper::rho_reporter(
                &rspace_stores,
                &block_store,
                &block_dag_storage,
                rholang::rust::interpreter::external_services::ExternalServices::noop(),
            )
        } else {
            reporting_casper::noop()
        }
    };

    // RSpace state manager (for CasperLaunch)
    // Note: rnodeStateManager is created in Scala but never used, so we only create rspaceStateManager
    let rspace_state_manager = {
        use rspace_plus_plus::rspace::state::rspace_state_manager::RSpaceStateManager;

        let exporter = history_repo.exporter();
        let importer = history_repo.importer();
        RSpaceStateManager::new(exporter, importer)
    };

    // Engine dynamic reference
    let engine_cell = {
        use casper::rust::engine::engine_cell::EngineCell;

        EngineCell::init()
    };

    // Block processor queue - mpsc channel connecting producers (CasperLaunch, Running)
    // to consumer (BlockProcessorInstance)
    let block_processor_queue_max_pending = block_processor_queue_max_pending();
    let block_processor_max_bytes =
        usize::try_from(conf.protocol_server.grpc_max_recv_stream_message_size).map_err(|_| {
            CasperError::Other("protocol stream-size limit exceeds usize".to_string())
        })?;
    let (block_processor_queue_tx, block_processor_queue_rx) = BlockProcessingQueueSender::channel(
        block_processor_queue_max_pending,
        block_processor_max_bytes,
    )
    .map_err(|error| CasperError::Other(error.to_string()))?;

    // Block processing state - set of items currently in processing
    let block_processor_state_ref = Arc::new(dashmap::DashSet::<BlockHash>::new());

    // Read RPConf once for use in multiple places
    let rp_conf = rp_conf_cell
        .read()
        .map_err(|e| CasperError::Other(format!("Failed to read RPConf: {}", e)))?;

    // Block processor
    let block_processor = casper::rust::blocks::block_processor::new_block_processor(
        block_store.clone(),
        casper_buffer_storage.clone(),
        block_dag_storage.clone(),
        block_retriever.clone(),
        transport_layer.clone(),
        rp_connections.clone(),
        rp_conf.clone(),
    );

    // Proposer instance
    let validator_identity_opt = {
        use casper::rust::validator_identity::ValidatorIdentity;

        ValidatorIdentity::from_private_key_with_logging(
            conf.casper.validator_private_key.as_deref(),
        )
    };

    // Clone validator_identity for heartbeat (used by both proposer and heartbeat)
    let validator_identity_for_heartbeat = validator_identity_opt.clone();

    let proposer = validator_identity_opt.map(|validator_identity| {
        use crypto::rust::private_key::PrivateKey;

        // Parse dummy deployer key from config
        let dummy_deploy_opt = conf
            .dev
            .deployer_private_key
            .as_ref()
            .and_then(|key_hex| hex::decode(key_hex).ok())
            .map(|bytes| {
                let private_key = PrivateKey::from_bytes(&bytes);
                // TODO: Make term for dummy deploy configurable - OLD
                (private_key, "Nil".to_string())
            });

        casper::rust::blocks::proposer::proposer::new_proposer(
            validator_identity,
            dummy_deploy_opt,
            runtime_manager.clone(),
            block_store.clone(),
            deploy_storage_arc.clone(),
            rejected_deploy_buffer_arc.clone(),
            // Multi-sig cosigner-metadata sidecar (§1.9.5). The casper
            // instance owns the canonical sidecar; the proposer holds a
            // shared Arc clone. In production, setup.rs constructs the
            // casper-side sidecar first and threads it through both sides;
            // this entry point creates the sidecar fresh for the proposer
            // and the casper engine receives the same Arc.
            std::sync::Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
            block_retriever.clone(),
            transport_layer.clone(),
            rp_connections.clone(),
            rp_conf.clone(),
            event_publisher.clone(),
            conf.casper.heartbeat_conf.enabled,
        )
    });
    match &proposer {
        Some(_) => info!("Proposer initialized"),
        None => info!("Running without proposer"),
    }

    metrics::gauge!(
        PROPOSER_QUEUE_PENDING_METRIC,
        "source" => VALIDATOR_METRICS_SOURCE
    )
    .set(0.0);

    let (proposer_queue_tx, proposer_queue_rx) = mpsc::channel::<ProposeQueueEntry>(1);
    let proposer_coalescer = Arc::new(ProposerCoalescer::new());

    // Trigger propose function - wraps proposerQueue to provide propose functionality
    let trigger_propose_f_opt: Option<Arc<ProposeFunction>> = if proposer.is_some() {
        let queue_tx = proposer_queue_tx.clone();
        let coalescer = proposer_coalescer.clone();
        Some(Arc::new(
            move |request_kind: casper::rust::ProposeRequestKind| {
                let queue_tx = queue_tx.clone();
                let coalescer = coalescer.clone();

                Box::pin(async move {
                    match coalescer.try_admit(ProposalRequestKind::from(&request_kind)) {
                        AdmissionOutcome::Coalesced => return Ok(ProposerResult::empty()),
                        AdmissionOutcome::Busy => {
                            metrics::counter!(
                                PROPOSER_QUEUE_REJECTED_TOTAL_METRIC,
                                "source" => VALIDATOR_METRICS_SOURCE
                            )
                            .increment(1);
                            return Ok(ProposerResult::empty());
                        }
                        AdmissionOutcome::Acquired => {}
                    }
                    debug!(?request_kind, "Propose request admitted");
                    metrics::gauge!(
                        PROPOSER_QUEUE_PENDING_METRIC,
                        "source" => VALIDATOR_METRICS_SOURCE
                    )
                    .set(1.0);

                    let (result_tx, result_rx) = oneshot::channel::<ProposerResult>();
                    match queue_tx
                        .send(ProposeQueueEntry {
                            request_kind,
                            result_sender: result_tx,
                            coalescer: coalescer.clone(),
                        })
                        .await
                    {
                        Ok(()) => {}
                        Err(e) => {
                            coalescer.cancel();
                            metrics::gauge!(
                                PROPOSER_QUEUE_PENDING_METRIC,
                                "source" => VALIDATOR_METRICS_SOURCE
                            )
                            .set(0.0);
                            return Err(CasperError::Other(format!(
                                "Failed to send to proposer queue: {}",
                                e
                            )));
                        }
                    }

                    // Wait for result
                    result_rx.await.map_err(|e| {
                        warn!(error = %e, "Failed to enqueue propose request");
                        CasperError::Other(format!("Failed to receive proposer result: {}", e))
                    })
                })
            },
        ))
    } else {
        None
    };

    // Proposer state ref - created if trigger_propose_f_opt exists
    // Wrapped in Arc for sharing across multiple API instances
    let proposer_state_ref_opt: Option<Arc<RwLock<ProposerState>>> = trigger_propose_f_opt
        .as_ref()
        .map(|_| Arc::new(RwLock::new(ProposerState::default())));

    // CasperLaunch - orchestrates the launch of the Casper consensus
    // Create heartbeat signal reference - starts empty, will be set when heartbeat starts
    // Created outside the block so it can be returned for use by HeartbeatProposer
    let heartbeat_signal_ref = casper::rust::heartbeat_signal::new_heartbeat_signal_ref();

    let casper_launch = {
        // Determine which propose function to use based on autopropose config
        let propose_f_for_launch = if conf.autopropose {
            trigger_propose_f_opt.clone()
        } else {
            None
        };

        info!(
            autopropose = conf.autopropose,
            heartbeat = conf.casper.heartbeat_conf.enabled,
            standalone = conf.standalone,
            "Initializing CasperLaunch"
        );
        // Create CasperLaunch with all dependencies
        Arc::new(casper::rust::engine::casper_launch::CasperLaunchImpl::new(
            // Infrastructure dependencies
            transport_layer.clone(),
            rp_conf.clone(),
            rp_connections.clone(),
            last_approved_block,
            event_publisher.clone(),
            block_retriever.clone(),
            Arc::new(engine_cell.clone()),
            block_store.clone(),
            block_dag_storage.clone(),
            deploy_storage,
            rejected_deploy_buffer_arc.clone(),
            casper_buffer_storage.clone(),
            rspace_state_manager,
            Arc::new(runtime_manager.clone()),
            estimator.clone(),
            // Explicit parameters
            block_processor_queue_tx.clone(),
            block_processor_state_ref.clone(),
            propose_f_for_launch,
            conf.casper.clone(),
            !conf.protocol_client.disable_lfs,
            conf.protocol_server.disable_state_exporter,
            heartbeat_signal_ref.clone(),
            conf.standalone,
            // Slice 25 (C-25-1 review-fix wire-up): project the
            // merged FileIoProvisioning into the bundle format
            // `fs_generator` consumes.  merge_and_validate combines
            // config-file entries with any CLI `--*-static-*` flags
            // and applies boot-time invariants (canonicity, mode
            // whitelist, PB-M-16 bucket disjointness, forbidden
            // chars, cross-source name conflicts).  On validation
            // failure we panic — an invalid provisioning would
            // yield a genesis block that no validator could accept,
            // so failing loud at boot is the correct outcome.
            {
                // L-9 note: second of two boot-time
                // `merge_and_validate` calls (see the SnapshotWriter
                // setup ~350 lines above).  Kept separate because the
                // error-handling policies differ — soft-fail there,
                // hard panic here (invalid provisioning would yield
                // a genesis block no validator could accept).
                let merged = crate::rust::configuration::provisioning_merge::merge_and_validate(
                    conf.storage.file_io_provisioning.clone(),
                    // CLI-side entries land in slice 26 wire-up
                    // (RunOptions → project into Vec<CliStatic*>).
                    // For now the CLI vecs are empty; the config
                    // surface alone is honored.
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap_or_else(|errs| {
                    let msg = errs
                        .iter()
                        .map(|e| format!("  - {e}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    panic!(
                        "static-provisioning validation failed at boot; refusing \
                         to proceed to genesis:\n{msg}"
                    );
                });
                crate::rust::configuration::provisioning_merge::project_bundle(&merged)
            },
            // CRIT-2 fix (2026-08-06): plumb HOCON cadence into
            // CasperLaunch → both proposer's Genesis composition
            // and validator's `BlockApproverProtocol`.  Cadence is
            // now embedded as a literal in the fs_generator deploy
            // term, so any HOCON-vs-shard cadence disagreement
            // fails `validate_candidate`'s byte-for-byte deploy
            // diff — closing the "shared Genesis hash but silently
            // divergent snapshot cadence" gap.  The setup.rs
            // SnapshotWriter attach below (line ~284) still reads
            // the same HOCON key; on a bootstrap validator the two
            // are trivially consistent (single source of truth),
            // and on a joining validator a mismatch is caught at
            // BAP validation before the SnapshotWriter is ever
            // used.
            #[allow(deprecated)]
            {
                conf.storage.consensus_fs_snapshot_cadence
            },
        )) as Arc<dyn CasperLaunch>
    };
    info!("CasperLaunch initialized");

    // Packet handler - handles incoming Casper protocol messages
    // Note: Scala has a commented-out fairDispatcher option (Setup.scala:268-277) that uses
    // round-robin dispatching with queue management. Currently using simple handler.
    let packet_handler = casper::rust::util::comm::casper_packet_handler::CasperPacketHandler::new(
        engine_cell.clone(),
    );
    let packet_handler: Arc<dyn PacketHandler> = Arc::new(packet_handler);

    // Reporting store - storage for block event reports with LZ4 compression
    let reporting_store =
        casper::rust::report_store::report_store(&mut rnode_store_manager).await?;

    // Block Report API - API for block reporting
    let block_report_api = casper::rust::api::block_report_api::BlockReportAPI::new(
        reporting_runtime,
        reporting_store,
        engine_cell.clone(),
        block_store.clone(),
        oracle,
        conf.dev_mode,
    );

    // API Servers - gRPC services for REPL, Deploy, Propose, and LSP
    let is_node_read_only = conf.casper.validator_private_key.is_none();

    // Conditional propose function for autopropose.
    // In validator nodes this must remain enabled even without deployer private key
    // so normal deploy flow can trigger propose on-chain in non-dev mode.
    let propose_f_for_api = if conf.autopropose {
        trigger_propose_f_opt.clone()
    } else {
        None
    };

    let block_report_api_for_return = block_report_api.clone();

    // Transfer unforgeable channel — used for transfer extraction from block reports
    let transfer_unforgeable = {
        use crate::rust::web::transaction::transfer_unforgeable;
        transfer_unforgeable()
    };

    // Shared is_ready flag — set to true when engine enters Running state.
    // Used by both HTTP and gRPC status endpoints.
    let is_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Event-driven background tasks: transfer extraction + readiness tracking.
    // Listens on the broadcast event stream and handles:
    // - BlockFinalised: pre-warm ReportStore cache, extract transfers, emit TransfersAvailable
    // - EnteredRunningState: flip is_ready flag for status endpoints
    {
        use futures::StreamExt;
        use shared::rust::shared::f1r3fly_event::F1r3flyEvent;

        let report_api = block_report_api.clone();
        let transfer_unforgeable_for_events = transfer_unforgeable.clone();
        let event_pub = event_publisher.clone();
        let is_ready_flag = is_ready.clone();
        let mut event_stream = event_publisher.consume();

        tokio::spawn(async move {
            while let Some(event) = event_stream.next().await {
                match &event {
                    F1r3flyEvent::BlockFinalised(finalized) => {
                        let api = report_api.clone();
                        let unforgeable = transfer_unforgeable_for_events.clone();
                        let publisher = event_pub.clone();
                        let block_hash = finalized.block_hash.clone();
                        let block_number = finalized.block_number;
                        tokio::spawn(async move {
                            handle_block_finalized(
                                api,
                                unforgeable,
                                publisher,
                                block_hash,
                                block_number,
                            )
                            .await;
                        });
                    }
                    F1r3flyEvent::EnteredRunningState(_) => {
                        is_ready_flag.store(true, std::sync::atomic::Ordering::Release);
                        tracing::info!("Node is ready (EnteredRunningState received)");
                    }
                    _ => {}
                }
            }
        });
    }

    // Clone trigger_propose_f_opt before passing to api_servers since we'll use it later for web_api, admin_web_api, and return value
    let trigger_propose_f_opt_for_web_api = trigger_propose_f_opt.clone();
    let trigger_propose_f_opt_for_admin_web_api = trigger_propose_f_opt.clone();
    let trigger_propose_f_opt_for_return = trigger_propose_f_opt.clone();

    // Clone proposer_state_ref_opt before passing to api_servers since we'll use it later for admin_web_api and return value
    let proposer_state_ref_opt_for_admin_web_api = proposer_state_ref_opt.clone();
    let proposer_state_ref_opt_for_return = proposer_state_ref_opt.clone();

    let api_servers = APIServers::build(
        eval_runtime,
        trigger_propose_f_opt,
        proposer_state_ref_opt,
        conf.api_server.max_blocks_limit as i32,
        conf.dev_mode,
        propose_f_for_api,
        block_report_api,
        transfer_unforgeable.clone(),
        conf.protocol_server.network_id.clone(),
        conf.casper.shard_name.clone(),
        conf.casper.min_phlo_price,
        conf.casper.genesis_block_data.native_token_name.clone(),
        conf.casper.genesis_block_data.native_token_symbol.clone(),
        conf.casper.genesis_block_data.native_token_decimals,
        is_node_read_only,
        engine_cell.clone(),
        block_store.clone(),
        rp_conf_cell.clone(),
        rp_connections.clone(),
        node_discovery.clone(),
        conf.casper.genesis_block_data.epoch_length,
        is_ready.clone(),
    );

    // Reporting HTTP Routes - REST API for block reporting and tracing
    // Note: In Rust with Axum, BlockReportAPI is accessed via State extraction
    // at runtime rather than being captured at route creation time
    let reporting_routes = ReportingRoutes::create_router();

    // Casper Loop - maintenance loop body for Casper consensus
    // This closure is executed repeatedly to:
    // 1. Fetch missing block dependencies from CasperBuffer
    // 2. Maintain requested blocks with timeout management
    // 3. Sleep for the configured interval
    let casper_loop = {
        trace!("Casper loop tick");
        let engine_cell_clone = engine_cell.clone();
        let block_retriever_clone = block_retriever.clone();
        let requested_blocks_timeout = conf.casper.requested_blocks_timeout;
        let casper_loop_interval = conf.casper.casper_loop_interval;

        move || -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> {
            let engine_cell = engine_cell_clone.clone();
            let block_retriever = block_retriever_clone.clone();

            Box::pin(async move {
                // Read the engine from engine cell
                let engine = engine_cell.get().await;

                // Fetch dependencies from CasperBuffer
                if let Some(casper) = engine.with_casper() {
                    trace!("Fetching Casper dependencies");
                    if let Err(err) = casper.fetch_dependencies().await {
                        tracing::warn!("Casper dependency fetch failed: {}", err);
                    }
                } else {
                    warn!("Casper engine present but Casper not initialized yet");
                }

                // Maintain RequestedBlocks for Casper
                if let Err(err) = block_retriever.request_all(requested_blocks_timeout).await {
                    tracing::warn!("RequestedBlocks maintenance failed: {}", err);
                } else {
                    trace!(timeout = ?requested_blocks_timeout, "RequestedBlocks maintenance executed");
                }

                // Sleep for the configured interval
                tokio::time::sleep(casper_loop_interval).await;

                Ok::<(), CasperError>(())
            })
        }
    };

    // Update Fork Choice Loop - requests fork choice tips if node is stuck
    // Broadcast fork choice tips request if current fork choice is more than
    // `forkChoiceStaleThreshold` old, which indicates the node might be stuck.
    // For details, see Running::update_fork_choice_tips_if_stuck description.
    let update_fork_choice_loop = {
        let engine_cell_clone = engine_cell.clone();
        let transport_layer_clone = transport_layer.clone();
        let rp_connections_clone = rp_connections.clone();
        let rp_conf_cell_clone = rp_conf_cell.clone();
        let fork_choice_check_interval = conf.casper.fork_choice_check_if_stale_interval;
        let fork_choice_stale_threshold = conf.casper.fork_choice_stale_threshold;

        move || -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> {
            let engine_cell = engine_cell_clone.clone();
            let transport_layer = transport_layer_clone.clone();
            let rp_connections = rp_connections_clone.clone();
            let rp_conf_cell = rp_conf_cell_clone.clone();

            Box::pin(async move {
                // Sleep first
                tokio::time::sleep(fork_choice_check_interval).await;

                // Read current RPConf
                let rp_conf = rp_conf_cell
                    .read()
                    .map_err(|e| CasperError::Other(e.to_string()))?;

                debug!(stale_threshold = ?fork_choice_stale_threshold, "Checking fork choice staleness");
                // Call the standalone function
                casper::rust::engine::running::update_fork_choice_tips_if_stuck(
                    &engine_cell,
                    &transport_layer,
                    &rp_connections,
                    &rp_conf,
                    fork_choice_stale_threshold,
                )
                .await?;

                Ok::<(), CasperError>(())
            })
        }
    };

    // Engine Init - reads engine from engine cell and calls init
    let engine_init = {
        let engine_cell_clone = engine_cell.clone();

        move || -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> {
            let engine_cell = engine_cell_clone.clone();

            Box::pin(async move {
                let engine = engine_cell.get().await;
                engine.init().await?;
                Ok::<(), CasperError>(())
            })
        }
    };

    // Scala has: runtimeCleanup = NodeRuntime.cleanup(rnodeStoreManager)
    // But it's commented out in NodeRuntime.scala line 321:
    //   //_ <- addShutdownHook(servers, runtimeCleanup, blockStore)
    //
    // Rust implementation notes:
    // - The store managers (LmdbDirStoreManager, LmdbStoreManager) have both:
    //   1. async shutdown() methods for graceful cleanup
    //   2. Drop implementations for fallback cleanup
    // - shutdown() should be called explicitly for proper async cleanup
    // - This should be implemented in the main runtime's signal handler
    //   (SIGTERM, SIGINT, etc.) before program exit
    // - For now, Drop implementations will handle cleanup on program exit
    //
    // When implementing, add shutdown call like:
    //   rnode_store_manager.shutdown().await?;

    // Web API - HTTP REST API implementation
    let web_api = {
        use crate::rust::api::web_api::WebApiImpl;

        let is_node_read_only = conf.casper.validator_private_key.is_none();

        // Conditional propose function for autopropose.
        // Expose deploy-triggered propose from REST API whenever autopropose is enabled.
        let trigger_propose_f = if conf.autopropose {
            trigger_propose_f_opt_for_web_api
        } else {
            None
        };

        WebApiImpl::new(
            conf.api_server.max_blocks_limit as i32,
            conf.dev_mode,
            conf.protocol_server.network_id.clone(),
            conf.casper.shard_name.clone(),
            conf.casper.min_phlo_price,
            conf.casper.genesis_block_data.native_token_name.clone(),
            conf.casper.genesis_block_data.native_token_symbol.clone(),
            conf.casper.genesis_block_data.native_token_decimals,
            is_node_read_only,
            block_report_api_for_return.clone(),
            transfer_unforgeable,
            Arc::new(engine_cell.clone()),
            rp_conf_cell.clone(),
            rp_connections.clone(),
            node_discovery.clone(),
            trigger_propose_f,
            conf.casper.genesis_block_data.epoch_length,
            conf.casper.genesis_block_data.quarantine_length,
            is_ready.clone(),
        )
    };

    // Admin Web API - Admin HTTP REST API implementation
    let admin_web_api = {
        use crate::rust::api::admin_web_api::AdminWebApiImpl;

        AdminWebApiImpl::new(
            trigger_propose_f_opt_for_admin_web_api,
            proposer_state_ref_opt_for_admin_web_api,
            Arc::new(engine_cell.clone()),
        )
    };

    // Mergeable Channels GC Loop - background garbage collection for mergeable channel data
    // Only created when GC is enabled in config (required for multi-parent mode)
    let mergeable_channels_gc_loop: Option<CasperLoop> = if conf.casper.enable_mergeable_channel_gc
    {
        use casper::rust::casper::CasperShardConf;

        let gc_block_dag_storage = block_dag_storage.clone();
        let gc_block_store = block_store.clone();
        let gc_runtime_manager = Arc::new(runtime_manager.clone());
        let gc_interval = conf.casper.mergeable_channels_gc_interval;
        let gc_casper_shard_conf = CasperShardConf {
            fault_tolerance_threshold: conf.casper.fault_tolerance_threshold,
            fault_tolerance_threshold_ppm:
                casper::rust::genesis::contracts::proof_of_stake::ProofOfStake::fault_tolerance_threshold_to_ppm(
                    conf.casper.fault_tolerance_threshold,
                ),
            shard_name: conf.casper.shard_name.clone(),
            parent_shard_id: conf.casper.parent_shard_id.clone(),
            finalization_rate: conf.casper.finalization_rate,
            max_number_of_parents: conf.casper.max_number_of_parents,
            max_parent_depth: conf.casper.max_parent_depth,
            synchrony_constraint_threshold: conf.casper.synchrony_constraint_threshold,
            height_constraint_threshold: conf.casper.height_constraint_threshold,
            deploy_lifespan: 50,
            casper_version: casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            config_version: 1,
            bond_minimum: conf.casper.genesis_block_data.bond_minimum,
            bond_maximum: conf.casper.genesis_block_data.bond_maximum,
            epoch_length: conf.casper.genesis_block_data.epoch_length,
            quarantine_length: conf.casper.genesis_block_data.quarantine_length,
            min_phlo_price: conf.casper.min_phlo_price,
            // Task #13b: this GC-path shard conf drives mergeable-channel GC
            // sizing, NOT block creation, so the genesis client funding-slot list
            // is inert here — default EMPTY (the authoritative wiring is in
            // `casper_launch.rs`, which the block proposer/validator read).
            client_fuel_allocations: Vec::new(),
            disable_late_block_filtering: conf.casper.disable_late_block_filtering,
            deploy_heartbeat_wake_enabled: false,
            disable_validator_progress_check: conf.standalone,
            enable_mergeable_channel_gc: conf.casper.enable_mergeable_channel_gc,
            mergeable_channels_gc_depth_buffer: conf.casper.mergeable_channels_gc_depth_buffer,
            finalizer_conf: conf.casper.finalizer.clone(),
            synchrony_recovery_stall_window: conf.casper.synchrony_recovery_stall_window,
            synchrony_recovery_cooldown: conf.casper.synchrony_recovery_cooldown,
            synchrony_recovery_max_bypasses: conf.casper.synchrony_recovery_max_bypasses,
            synchrony_finalized_baseline_enabled: conf.casper.synchrony_finalized_baseline_enabled,
            synchrony_finalized_baseline_max_distance: conf
                .casper
                .synchrony_finalized_baseline_max_distance,
            max_cosigners_per_deploy: casper::rust::casper_conf::DEFAULT_MAX_COSIGNERS_PER_DEPLOY,
            max_user_deploys_per_block: conf.casper.max_user_deploys_per_block,
            native_token_name: conf.casper.genesis_block_data.native_token_name.clone(),
            native_token_symbol: conf.casper.genesis_block_data.native_token_symbol.clone(),
            native_token_decimals: conf.casper.genesis_block_data.native_token_decimals,
            active_validators_cache_max_entries:
                casper::rust::casper::ACTIVE_VALIDATORS_CACHE_MAX_ENTRIES_DEFAULT,
        };

        Some(Arc::new(
            move || -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> {
                use casper::rust::util::mergeable_channels_gc;

                let gc_block_dag_storage = gc_block_dag_storage.clone();
                let gc_block_store = gc_block_store.clone();
                let gc_runtime_manager = gc_runtime_manager.clone();
                let gc_casper_shard_conf = gc_casper_shard_conf.clone();
                let gc_interval = gc_interval;

                Box::pin(async move {
                    // Sleep for the configured interval
                    tokio::time::sleep(gc_interval).await;

                    // Run GC
                    let dag = gc_block_dag_storage
                        .get_representation()
                        .map_err(|e| CasperError::RuntimeError(e.to_string()))?;
                    mergeable_channels_gc::collect_garbage(
                        &dag,
                        &gc_block_store,
                        &gc_runtime_manager,
                        &gc_casper_shard_conf,
                    )
                    .await
                    .map_err(|e| CasperError::RuntimeError(e.to_string()))?;

                    Ok::<(), CasperError>(())
                })
            },
        ))
    } else {
        None
    };

    // Return all initialized components
    Ok((
        packet_handler,
        api_servers,
        Arc::new(casper_loop),
        Arc::new(update_fork_choice_loop),
        Arc::new(engine_init),
        casper_launch,
        reporting_routes,
        Arc::new(web_api),
        Arc::new(admin_web_api),
        proposer,
        proposer_queue_rx,
        proposer_state_ref_opt_for_return,
        block_processor,
        block_processor_state_ref,
        block_processor_queue_tx,
        block_processor_queue_rx,
        trigger_propose_f_opt_for_return,
        Arc::new(block_report_api_for_return),
        block_store,
        // Heartbeat dependencies
        validator_identity_for_heartbeat,
        Arc::new(engine_cell.clone()),
        conf.casper.heartbeat_conf.clone(),
        conf.casper.max_number_of_parents,
        heartbeat_signal_ref,
        // Mergeable channels GC loop
        mergeable_channels_gc_loop,
    ))
}

/// Pre-warm the ReportStore cache for a finalized block, then extract transfers
/// and publish a `TransfersAvailable` event so WebSocket clients can receive
/// transfer data without polling the REST API.
///
/// Runs as a fire-and-forget task — errors (e.g. on validators where block
/// reports are unavailable) are logged at debug level and silently ignored.
async fn handle_block_finalized(
    report_api: casper::rust::api::block_report_api::BlockReportAPI,
    transfer_unforgeable: models::rhoapi::Par,
    event_publisher: shared::rust::shared::f1r3fly_events::F1r3flyEvents,
    block_hash: String,
    block_number: i64,
) {
    use shared::rust::shared::f1r3fly_event::{DeployTransfers, F1r3flyEvent, TransferEvent};

    use crate::rust::web::block_info_enricher::extract_transfers_from_report;

    let block_hash_bytes: prost::bytes::Bytes = match hex::decode(&block_hash) {
        Ok(bytes) => bytes.into(),
        Err(e) => {
            tracing::warn!(
                %block_hash,
                error = %e,
                "Invalid block hash hex in finalization event"
            );
            return;
        }
    };
    match report_api.block_report(block_hash_bytes, false).await {
        Ok(report) => {
            let transfers_by_deploy = extract_transfers_from_report(&report, &transfer_unforgeable);

            let deploy_transfers: Vec<DeployTransfers> = transfers_by_deploy
                .into_iter()
                .map(|(deploy_id, transfers)| DeployTransfers {
                    deploy_id,
                    transfers: transfers
                        .into_iter()
                        .map(|t| TransferEvent {
                            from_addr: t.from_addr,
                            to_addr: t.to_addr,
                            amount: t.amount,
                            success: t.success,
                        })
                        .collect(),
                })
                .collect();

            if !deploy_transfers.is_empty() {
                if let Err(e) = event_publisher.publish(F1r3flyEvent::transfers_available(
                    block_hash.clone(),
                    block_number,
                    deploy_transfers,
                )) {
                    tracing::warn!(
                        %block_hash,
                        error = %e,
                        "Failed to publish TransfersAvailable event"
                    );
                }
            }
        }
        Err(e) => {
            tracing::debug!(
                target: "f1r3fly.node.transaction",
                %block_hash,
                error = %e,
                "Block report pre-cache skipped (expected on validators)"
            );
        }
    }
}
