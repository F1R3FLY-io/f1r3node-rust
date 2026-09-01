// See casper/src/test/scala/coop/rchain/casper/helper/TestNode.scala

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::block_status::BlockStatus;
use casper::rust::blocks::block_processing_queue::BlockProcessingQueueSender;
use casper::rust::blocks::block_processor::{BlockProcessor, BlockProcessorDependencies};
use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::new_proposer;
use casper::rust::casper::{Casper, CasperShardConf, MultiParentCasper};
use casper::rust::engine::block_retriever::{BlockRetriever, RequestState, RequestedBlocks};
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::multi_parent_casper::MultiParentCasperImpl;
use casper::rust::engine::running::{Running, RunningRecoveryContext};
use casper::rust::errors::CasperError;
use casper::rust::estimator::Estimator;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::safety_oracle::CliqueOracleImpl;
use casper::rust::util::comm::casper_packet_handler::CasperPacketHandler;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::validator_identity::ValidatorIdentity;
use casper::rust::ValidBlockProcessing;
use comm::rust::errors::CommError;
use comm::rust::p2p::packet_handler::{NOPPacketHandler, PacketHandler};
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::handle_messages;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::test_instances::create_rp_conf_ask;
use comm::rust::transport::communication_response::CommunicationResponse;
use comm::rust::transport::grpc_transport_server::TransportLayerServer;
use comm::rust::transport::transport_layer::Blob;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use dashmap::DashSet;
use models::routing::Protocol;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, ApprovedBlockCandidate, BlockMessage, DeployData,
};
use rspace_plus_plus::rspace::history::Either;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::util::comm::transport_layer_test_impl::test_network::TestNetwork;
use crate::util::comm::transport_layer_test_impl::{
    TransportLayerServerTestImpl, TransportLayerTestImpl,
};
use crate::util::genesis_builder::GenesisContext;
use crate::util::rholang::resources;

pub struct TestNode {
    pub name: String,
    pub local: PeerNode,
    pub tle: Arc<TransportLayerTestImpl>,
    pub tls: TransportLayerServerTestImpl,
    pub genesis: BlockMessage,
    pub validator_id_opt: Option<ValidatorIdentity>,
    // Note: blockProcessingPipe implemented as method process_block_through_pipe
    pub block_processor: BlockProcessor<TransportLayerTestImpl>,
    pub block_store: KeyValueBlockStore,
    pub block_dag_storage: BlockDagKeyValueStorage,
    pub deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    pub rejected_deploy_buffer: Arc<
        Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>,
    >,
    pub runtime_manager: RuntimeManager,
    // Note: no log field, logging will come from log crate
    pub requested_blocks: RequestedBlocks,
    pub connections_cell: ConnectionsCell,
    pub rp_conf: RPConf,
    // Casper instance (Arc<Mutex> for shared ownership with interior mutability)
    pub casper: Arc<MultiParentCasperImpl<TransportLayerTestImpl>>,
    // Engine cell for packet handling (matches Scala line 177)
    pub engine_cell: EngineCell,
    // Packet handler for receiving messages (matches Scala line 178)
    pub packet_handler: CasperPacketHandler,
    /// Heartbeat / liveness mode. When true, `create_block` emits an empty
    /// (CloseBlock-only) block instead of returning `NoNewDeploys` when the
    /// proposer has no user deploys — matching a production heartbeat-enabled
    /// shard, where a validator always proposes to keep the chain advancing.
    /// Defaults to false (the historic manual-propose test behavior).
    pub allow_empty_blocks: bool,
}

/// Per-node fs wiring for Casper-level fileio tests.  Passed into
/// `TestNode::create_network_with_fs_provisioning` to hook a node's
/// `RuntimeManager` up to a payload store and populate its root-
/// identity registry — the two pieces the production boot pipeline
/// (`node::runtime::setup`) installs when consensus-static provisioning
/// is present.
///
/// Snapshot writer wiring is intentionally omitted: the canary tests
/// this helper unblocks assert on the pre-finalization `pending_wal_
/// slices` cache + on-disk file bytes, both of which populate without
/// SnapshotWriter involvement (the writer only fires on LFB advance).
/// Tests that need snapshot emission can construct a `SnapshotWriter`
/// directly and call `runtime_manager.set_fs_snapshot_writer` after
/// `create_network_with_fs_provisioning` returns.
#[derive(Debug, Clone)]
pub struct TestFsProvisioning {
    /// Absolute paths to register with the RuntimeManager's root-
    /// identity registry via the legacy identity-collapsed API
    /// (`register_root_identity`, i.e. `logical == on_disk == canon`).
    /// For File-kind bundle entries pass the file's parent
    /// directory; for Dir-kind entries pass the dir itself.  Every
    /// fs handler consults this registry through
    /// `safe_descend_verified` — a missing entry causes the first
    /// syscall on that root to fail with `RootIdentityChanged`, so
    /// tests that never touch the fs can pass `Vec::new()`.
    ///
    /// Use this for Oracular bundle entries and any legacy-shape
    /// registrations that don't need Shape A's per-validator
    /// on-disk remapping.
    pub root_paths: Vec<PathBuf>,
    /// Shape A (Phase 0.1, 2026-08-31): per-validator logical →
    /// on-disk root map.  Each entry `(logical, on_disk)` is
    /// registered via `register_root_remap` — the Consensus-mode
    /// bundle emission (`format_bundle_for_rholang`) puts
    /// `logical = "/@bundle/<X>"` into the composed Rholang source
    /// (validator-independent), and the handler's
    /// `resolve_or_identity(canonRoot)` maps that to
    /// `on_disk = "<validator_subdir>/<X>"` before
    /// `safe_descend_verified`.  The `(dev, inode)` identity is
    /// captured from `on_disk` at provisioning time, so the H-5
    /// fstat-post-open check still verifies against the real
    /// staging directory.
    ///
    /// A missing entry for a Consensus bundle's logical root causes
    /// `resolve_or_identity` to fall through to the logical path
    /// unchanged, which `safe_descend_verified` opens as
    /// `/@bundle/<X>` — a nonexistent path → ENOENT.  This is a
    /// clean failure mode (not a cwd-relative surprise) that
    /// surfaces "you forgot to remap this validator's bundle".
    ///
    /// Left empty for pre-Shape-A tests (only `root_paths` is
    /// consulted).
    pub logical_to_on_disk: Vec<(PathBuf, PathBuf)>,
    /// Directory to back the content-addressed payload store.  Auto-
    /// created on `create_node` (via `DirectoryPayloadStore::insert`,
    /// which mkdirs on first write; the helper mkdirs eagerly so a
    /// unwritable directory surfaces during setup, not deep inside
    /// a deploy).  Per-node in tests; two nodes writing identical
    /// bytes produce identically-named files in their respective
    /// dirs.
    pub payload_dir: PathBuf,
}

/// Shape A (Phase 0.2, 2026-08-31): the per-validator projection of
/// a canonical bundle spec into an isolated on-disk staging tree
/// plus a ready-to-wire `TestFsProvisioning`.
///
/// `project_bundle_per_validator` returns one of these per
/// validator: each holds
///   - `subdir`  — the validator's on-disk bundle root
///     (`<base>/validator-<ix>/bundle`, seeded with per-entry byte
///     copies of the operator's `canon_path` staging).
///   - `provisioning` — a `TestFsProvisioning` whose
///     `logical_to_on_disk` map registers `/@bundle/<X> →
///     <subdir>/<X>` for every Consensus bundle entry
///     (matching the canonRoot form `format_bundle_for_rholang`
///     emits under Shape A), plus a per-validator `payload_dir`.
///
/// Feed the `provisioning` list to `create_network_with_fs_provisioning`
/// (the Phase 0.3 convenience wrapper `create_network_with_per_
/// validator_fs` was retired in Phase 1 — all canaries now use the
/// direct projection pattern because they need the `subdir` handle
/// for fs-restore between play and scratch-replay under the Phase 1
/// re-execute + verify model).
pub struct PerValidatorProjection {
    pub subdir: PathBuf,
    pub provisioning: TestFsProvisioning,
}

/// Shape A (Phase 0.2, 2026-08-31): given a canonical bundle spec
/// (identical across validators — the same one that goes to
/// genesis) and a base directory, materialize a per-validator
/// on-disk projection.
///
/// For each validator ix in `0..validator_count`:
///   1. Creates `<base>/validator-<ix>/bundle/` as that validator's
///      bundle root.
///   2. For each Consensus-mode entry in `bundle`, mirrors the
///      operator's `canon_path` into
///      `<bundle root>/<logical_name>`:
///        - Files: copies the source bytes verbatim.  Nested
///          `logical_name`s (e.g. `cfg/theme.json`) get their
///          parent directories created on the fly.
///        - Dirs: recursively copies the source dir tree
///          (`fs_extra::copy_items` isn't a dep here, so we
///          walk with `walkdir`-style manual recursion via
///          `read_dir`).
///   3. Populates the returned `TestFsProvisioning`'s
///      `logical_to_on_disk` with one entry per distinct
///      `canonRoot` string that `format_bundle_for_rholang`
///      emits — the resolver's HashMap is keyed on exact match, so
///      the projection helper computes the same
///      `(canonRoot, on_disk_root)` split as the emitter to keep
///      the two in lock-step.
///
/// Oracular entries are skipped by projection — Shape A leaves them
/// on the operator's staged `canon_path` (per auto-memory
/// `fileio_consensus_fs_shape_a.md` line 37-38).  Callers that
/// need Oracular file-serving should stage the operator paths
/// themselves and set `root_paths` on the returned
/// `TestFsProvisioning`.
///
/// `payload_dir_leaf` names the per-validator payload store
/// subdirectory under `<base>/validator-<ix>/` — the caller can
/// keep the default `"wal_payload_store"` or override for tests
/// that need a distinguishing path.
pub fn project_bundle_per_validator(
    bundle: &[casper::rust::genesis::contracts::fs_genesis::BundleEntry],
    validator_count: usize,
    base: &Path,
    payload_dir_leaf: &str,
) -> std::io::Result<Vec<PerValidatorProjection>> {
    use casper::rust::genesis::contracts::fs_genesis::{
        BundleConsensusMode, BundleEntryKind, BUNDLE_ROOT_PREFIX,
    };

    let mut result = Vec::with_capacity(validator_count);
    for ix in 0..validator_count {
        let validator_root = base.join(format!("validator-{ix}"));
        let subdir = validator_root.join("bundle");
        std::fs::create_dir_all(&subdir)?;
        let payload_dir = validator_root.join(payload_dir_leaf);

        // Dedupe registrations by `canonRoot` string — a bundle
        // with a bare File `"cap"` and a nested File `"cap2"` both
        // resolve `/@bundle → <subdir>`, so we only register once.
        // Deterministic iteration order (Vec-of-tuples) keeps the
        // registry population reproducible across runs.
        let mut logical_to_on_disk_map: std::collections::BTreeMap<PathBuf, PathBuf> =
            std::collections::BTreeMap::new();

        for entry in bundle {
            if entry.consensus_mode != BundleConsensusMode::Consensus {
                continue;
            }
            let on_disk_target = subdir.join(&entry.logical_name);

            match entry.kind {
                BundleEntryKind::File => {
                    if let Some(parent) = on_disk_target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(&entry.canon_path, &on_disk_target)?;

                    // canonRoot the emitter produces:
                    //   bare `logical_name` (no `/`) → `/@bundle`
                    //   nested → `/@bundle/<parent-segments>`
                    let logical_path = Path::new(&entry.logical_name);
                    let parent_rel = logical_path
                        .parent()
                        .and_then(|p| p.to_str())
                        .unwrap_or("");
                    let (logical_root, on_disk_root) = if parent_rel.is_empty() {
                        (
                            PathBuf::from(BUNDLE_ROOT_PREFIX),
                            subdir.clone(),
                        )
                    } else {
                        (
                            PathBuf::from(format!("{BUNDLE_ROOT_PREFIX}/{parent_rel}")),
                            subdir.join(parent_rel),
                        )
                    };
                    logical_to_on_disk_map.insert(logical_root, on_disk_root);
                }
                BundleEntryKind::Dir => {
                    copy_dir_recursive(&entry.canon_path, &on_disk_target)?;
                    let logical_root = PathBuf::from(format!(
                        "{BUNDLE_ROOT_PREFIX}/{}",
                        entry.logical_name
                    ));
                    logical_to_on_disk_map.insert(logical_root, on_disk_target);
                }
            }
        }

        let logical_to_on_disk = logical_to_on_disk_map.into_iter().collect();
        result.push(PerValidatorProjection {
            subdir,
            provisioning: TestFsProvisioning {
                root_paths: Vec::new(),
                logical_to_on_disk,
                payload_dir,
            },
        });
    }
    Ok(result)
}

/// Recursively copy `src` → `dst`.  Used by
/// `project_bundle_per_validator` (and, out-of-crate, by the
/// RhoSpec-side Shape A wiring in `helper::rho_spec::get_results`)
/// to seed a bundle Dir entry's target tree with an identical
/// mirror of the operator's staging tree.  Follows symlinks (via
/// `std::fs::copy`) — bundle entries are provisioner-configured
/// staging paths, not user input, so link-following matches the
/// operator's intent.
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_child = entry.path();
        let dst_child = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_dir_recursive(&src_child, &dst_child)?;
        } else {
            std::fs::copy(&src_child, &dst_child)?;
        }
    }
    Ok(())
}

impl TestNode {
    /// Creates a block with the given deploys (equivalent to Scala createBlock, line 233-239).
    ///
    /// This method:
    /// 1. Deploys each datum to casper
    /// 2. Gets a snapshot from casper
    /// 3. Gets validator identity
    /// 4. Calls BlockCreator.create to produce the block
    ///
    /// Returns BlockCreatorResult which may be Created, NoNewDeploys, or ReadOnlyMode.
    pub async fn create_block(
        &mut self,
        deploy_datums: &[Signed<DeployData>],
    ) -> Result<BlockCreatorResult, CasperError> {
        // Deploy all datums
        for deploy_datum in deploy_datums {
            self.casper.deploy(deploy_datum.clone())?;
        }

        // Get snapshot
        let snapshot = self.casper.get_snapshot().await?;

        // Get validator
        let validator = self.casper.get_validator().ok_or_else(|| {
            CasperError::RuntimeError("No validator identity available".to_string())
        })?;

        // Create block using block_creator
        block_creator::create(
            &snapshot,
            &validator,
            None, // dummy_deploy_opt
            self.deploy_storage.clone(),
            self.rejected_deploy_buffer.clone(),
            std::sync::Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
            &self.runtime_manager.clone(),
            &mut self.block_store.clone(),
            self.allow_empty_blocks,
        )
        .await
    }

    /// Creates a block with the given deploys, assuming success (equivalent to Scala createBlockUnsafe, line 242-255).
    ///
    /// Unlike create_block, this method:
    /// - Returns the BlockMessage directly (not BlockCreatorResult)
    /// - Errors if block creation fails for any reason
    ///
    /// This is useful for tests that expect block creation to succeed.
    pub async fn create_block_unsafe(
        &mut self,
        deploy_datums: &[Signed<DeployData>],
    ) -> Result<BlockMessage, CasperError> {
        let mut finalization_deadline = None;
        let mut first_attempt = true;
        loop {
            let result = if first_attempt {
                first_attempt = false;
                self.create_block(deploy_datums).await?
            } else {
                self.create_block(&[]).await?
            };
            match result {
                BlockCreatorResult::Created(block, ..) => return Ok(block),
                BlockCreatorResult::RecoveryDeferred(
                    casper::rust::blocks::proposer::propose_result::RecoveryDeferralReason::FinalizedFloorMaterializationPending,
                ) => {
                    let deadline = *finalization_deadline.get_or_insert_with(|| {
                        tokio::time::Instant::now() + std::time::Duration::from_secs(30)
                    });
                    if tokio::time::Instant::now() >= deadline {
                        return Err(CasperError::RuntimeError(
                            "Timed out waiting for finalized-floor materialization".to_string(),
                        ));
                    }
                    self.casper.request_finalization()?;
                    self.wait_for_finalizer_quiescence(deadline).await?;
                }
                other => {
                    return Err(CasperError::RuntimeError(format!(
                        "Failed creating block: {:?}",
                        other
                    )))
                }
            }
        }
    }

    pub async fn wait_for_finalizer_quiescence(
        &self,
        deadline: tokio::time::Instant,
    ) -> Result<(), CasperError> {
        loop {
            if self.casper.finalization_schedule.is_quiescent()
                && self
                    .casper
                    .finalization_in_progress
                    .load(std::sync::atomic::Ordering::Acquire)
                    == 0
            {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(CasperError::RuntimeError(
                    "Timed out waiting for finalization to quiesce".to_string(),
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    /// Processes a block through the validation pipeline (equivalent to Scala processBlock, line 257-260).
    ///
    /// This is the wrapper method that processes an existing block through the full validation pipeline.
    pub async fn process_block(
        &mut self,
        block: BlockMessage,
    ) -> Result<ValidBlockProcessing, CasperError> {
        Self::process_block_through_pipe(self.casper.clone(), &self.block_processor, block).await
    }

    /// Processes a block through the validation pipeline.
    ///
    /// This method:
    /// 1. Checks if block is of interest
    /// 2. Checks if well-formed and stores
    /// 3. Checks dependencies
    /// 4. Validates with effects
    pub async fn process_block_through_pipe(
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block_processor: &BlockProcessor<TransportLayerTestImpl>,
        block: BlockMessage,
    ) -> Result<ValidBlockProcessing, CasperError> {
        // Check if block is of interest
        let is_of_interest = block_processor.check_if_of_interest(casper.clone(), &block)?;

        if !is_of_interest {
            return Ok(Either::Left(BlockStatus::not_of_interest()));
        }

        // Check if well-formed and store
        let is_well_formed = block_processor
            .check_if_well_formed_and_store(&block)
            .await?;

        if !is_well_formed {
            return Ok(Either::Left(BlockStatus::invalid_format()));
        }

        // Check dependencies
        let dependencies_ready = block_processor
            .check_dependencies_with_effects(casper.clone(), &block)
            .await?;

        if !dependencies_ready {
            return Ok(Either::Left(BlockStatus::missing_blocks()));
        }

        // Validate with effects
        block_processor
            .validate_with_effects(casper.clone(), &block, None)
            .await
    }

    /// Adds and processes a block (equivalent to Scala addBlock(block), line 198-199).
    ///
    /// Takes an existing block and processes it through the validation pipeline.
    pub async fn add_block(
        &mut self,
        block: BlockMessage,
    ) -> Result<ValidBlockProcessing, CasperError> {
        Self::process_block_through_pipe(self.casper.clone(), &self.block_processor, block).await
    }

    /// Creates and adds a block from deploys (equivalent to Scala addBlock(deploys), line 201-202).
    ///
    /// This is a convenience method that:
    /// 1. Creates a block from the given deploys
    /// 2. Processes it through the validation pipeline
    /// 3. Returns the block (assuming Valid status)
    pub async fn add_block_from_deploys(
        &mut self,
        deploy_datums: &[Signed<DeployData>],
    ) -> Result<BlockMessage, CasperError> {
        self.add_block_status(deploy_datums, |status| matches!(status, Either::Right(_)))
            .await
    }

    /// Creates and adds a block with expected status validation (equivalent to Scala addBlockStatus, line 223-231).
    ///
    /// This method:
    /// 1. Creates a block from deploys
    /// 2. Processes it through the validation pipeline
    /// 3. Validates the status matches the expected predicate
    /// 4. Returns the block on success
    ///
    /// # Parameters
    /// * `deploy_datums` - Deploys to include in the block
    /// * `expected_status` - Predicate to validate the processing status
    pub async fn add_block_status<F>(
        &mut self,
        deploy_datums: &[Signed<DeployData>],
        expected_status: F,
    ) -> Result<BlockMessage, CasperError>
    where
        F: FnOnce(&ValidBlockProcessing) -> bool,
    {
        let block = self.create_block_unsafe(deploy_datums).await?;

        // Process block
        let status = self.process_block(block.clone()).await?;

        // Validate status
        if !expected_status(&status) {
            return Err(CasperError::RuntimeError(format!(
                "Block status did not match expected: {:?}",
                status
            )));
        }

        Ok(block)
    }

    /// Publishes a block to other nodes (equivalent to Scala publishBlock, line 204-208).
    ///
    /// This method:
    /// 1. Creates a block from deploys
    /// 2. Triggers handleReceive on all other nodes
    /// 3. Returns the created block
    ///
    /// # Parameters
    /// * `deploy_datums` - Deploys to include in the block
    /// * `nodes` - Other nodes to publish to
    pub async fn publish_block(
        &mut self,
        deploy_datums: &[Signed<DeployData>],
        nodes: &mut [&mut TestNode],
    ) -> Result<BlockMessage, CasperError> {
        // Create and add block
        let block = self.add_block_from_deploys(deploy_datums).await?;

        // Trigger handleReceive on all other nodes (excluding self)
        for node in nodes.iter_mut() {
            if node.local != self.local {
                node.handle_receive().await?;
            }
        }

        Ok(block)
    }

    /// Helper method to propagate a block from a node at a specific index in a nodes array.
    ///
    /// This method works around Rust's borrow checker limitation where we cannot do:
    /// ```ignore
    /// nodes[0].propagate_block(&deploys, &mut nodes)
    /// ```
    /// because it would require borrowing `nodes` mutably twice:
    /// - First borrow: `nodes[0]` (mutable access to call the method)
    /// - Second borrow: `&mut nodes` (mutable parameter to pass all nodes)
    ///
    /// This helper uses `split_at_mut` to split the array into non-overlapping parts,
    /// allowing the borrow checker to verify that we're accessing different memory regions.
    ///
    /// # Scala equivalent
    /// In Scala this is simply: `nodes(index).propagateBlock(deploys)(nodes: _*)`
    ///
    /// # Parameters
    /// * `nodes` - All nodes in the network
    /// * `index` - Index of the node that should create and propagate the block
    /// * `deploy_datums` - Deploys to include in the block
    pub async fn propagate_block_at_index(
        nodes: &mut [TestNode],
        index: usize,
        deploy_datums: &[Signed<DeployData>],
    ) -> Result<BlockMessage, CasperError> {
        let (before, rest) = nodes.split_at_mut(index);
        let (current, after) = rest.split_at_mut(1);
        let mut all_others: Vec<&mut TestNode> =
            before.iter_mut().chain(after.iter_mut()).collect();
        current[0]
            .propagate_block(deploy_datums, &mut all_others)
            .await
    }

    /// Helper method to propagate a block from one node to another specific node.
    ///
    /// This method works around Rust's borrow checker limitation where we cannot do:
    /// ```ignore
    /// nodes[from_index].propagate_block(&deploys, &mut [&mut nodes[to_index]])
    /// ```
    /// because it would require borrowing from `nodes` mutably twice.
    ///
    /// This helper uses `split_at_mut` to split the array into non-overlapping parts,
    /// allowing the borrow checker to verify that we're accessing different memory regions.
    ///
    /// # Scala equivalent
    /// In Scala this is simply: `nodes(from_index).propagateBlock(deploys)(nodes(to_index))`
    ///
    /// # Parameters
    /// * `nodes` - All nodes in the network
    /// * `from_index` - Index of the node that should create and propagate the block
    /// * `to_index` - Index of the node that should receive the block
    /// * `deploy_datums` - Deploys to include in the block
    pub async fn propagate_block_to_one(
        nodes: &mut [TestNode],
        from_index: usize,
        to_index: usize,
        deploy_datums: &[Signed<DeployData>],
    ) -> Result<BlockMessage, CasperError> {
        assert_ne!(
            from_index, to_index,
            "from_index and to_index must be different"
        );

        // Split to get mutable references to both nodes without overlapping borrows
        if from_index < to_index {
            let (left, right) = nodes.split_at_mut(to_index);
            let from_node = &mut left[from_index];
            let to_node = &mut right[0];
            from_node
                .propagate_block(deploy_datums, &mut [to_node])
                .await
        } else {
            let (left, right) = nodes.split_at_mut(from_index);
            let to_node = &mut left[to_index];
            let from_node = &mut right[0];
            from_node
                .propagate_block(deploy_datums, &mut [to_node])
                .await
        }
    }

    /// Helper method to publish a block from a node at a specific index to all other nodes.
    ///
    /// This method works around Rust's borrow checker limitation similar to `propagate_block_at_index`.
    ///
    /// # Scala equivalent
    /// In Scala this is simply: `nodes(index).publishBlock(deploys)(nodes: _*)`
    ///
    /// # Parameters
    /// * `nodes` - All nodes in the network
    /// * `index` - Index of the node that should create and publish the block
    /// * `deploy_datums` - Deploys to include in the block
    pub async fn publish_block_at_index(
        nodes: &mut [TestNode],
        index: usize,
        deploy_datums: &[Signed<DeployData>],
    ) -> Result<BlockMessage, CasperError> {
        let (before, rest) = nodes.split_at_mut(index);
        let (current, after) = rest.split_at_mut(1);
        let mut all_others: Vec<&mut TestNode> =
            before.iter_mut().chain(after.iter_mut()).collect();
        current[0]
            .publish_block(deploy_datums, &mut all_others)
            .await
    }

    /// Helper method to publish a block from one node to another specific node.
    ///
    /// # Scala equivalent
    /// In Scala this is simply: `nodes(from_index).publishBlock(deploys)(nodes(to_index))`
    ///
    /// # Parameters
    /// * `nodes` - All nodes in the network
    /// * `from_index` - Index of the node that should create and publish the block
    /// * `to_index` - Index of the node that should receive the block
    /// * `deploy_datums` - Deploys to include in the block
    pub async fn publish_block_to_one(
        nodes: &mut [TestNode],
        from_index: usize,
        to_index: usize,
        deploy_datums: &[Signed<DeployData>],
    ) -> Result<BlockMessage, CasperError> {
        assert_ne!(
            from_index, to_index,
            "from_index and to_index must be different"
        );

        if from_index < to_index {
            let (left, right) = nodes.split_at_mut(to_index);
            let from_node = &mut left[from_index];
            let to_node = &mut right[0];
            from_node.publish_block(deploy_datums, &mut [to_node]).await
        } else {
            let (left, right) = nodes.split_at_mut(from_index);
            let to_node = &mut left[to_index];
            let from_node = &mut right[0];
            from_node.publish_block(deploy_datums, &mut [to_node]).await
        }
    }

    /// Propagates a block to target nodes (equivalent to Scala propagateBlock, line 210-221).
    ///
    /// This method:
    /// 1. Logs block creation
    /// 2. Creates a block from deploys
    /// 3. Logs propagation targets
    /// 4. Calls processBlock on each target node
    /// 5. Returns the created block
    ///
    /// # Parameters
    /// * `deploy_datums` - Deploys to include in the block
    /// * `nodes` - Target nodes to propagate to
    pub async fn propagate_block(
        &mut self,
        deploy_datums: &[Signed<DeployData>],
        nodes: &mut [&mut TestNode],
    ) -> Result<BlockMessage, CasperError> {
        // Log block creation
        tracing::debug!("\n{} creating block", self.name);

        // Create and add block
        let block = self.add_block_from_deploys(deploy_datums).await?;

        // Filter targets (exclude self)
        let targets: Vec<&mut &mut TestNode> = nodes
            .iter_mut()
            .filter(|node| node.local != self.local)
            .collect();

        // Log propagation
        let target_names: Vec<String> = targets.iter().map(|node| node.name.clone()).collect();
        tracing::debug!(
            "{} ! [{}] => {}",
            self.name,
            models::rust::casper::pretty_printer::PrettyPrinter::build_string_block_message(
                &block, true
            ),
            target_names.join(" ; ")
        );

        // Process block on each target
        for node in targets {
            node.process_block(block.clone()).await?;
        }

        Ok(block)
    }

    /// Synchronizes this node with other nodes (equivalent to Scala syncWith, line 293-344).
    ///
    /// This method implements iterative synchronization:
    /// 1. Drains message queues from requested block peers
    /// 2. Handles receive on this node
    /// 3. Repeats until all blocks are received or max attempts reached
    ///
    /// # Parameters
    /// * `nodes` - Nodes to synchronize with
    pub async fn sync_with(&mut self, nodes: &mut [&mut TestNode]) -> Result<(), CasperError> {
        const MAX_SYNC_ATTEMPTS: usize = 10;

        // Build network map (peer -> node index)
        let network_map: std::collections::HashMap<PeerNode, usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.local != self.local)
            .map(|(idx, node)| (node.local.clone(), idx))
            .collect();

        // Initial handleReceive
        self.handle_receive().await?;

        // Check if all synced
        let mut done = {
            let requested = self.requested_blocks.lock().unwrap();
            !requested.values().any(|req| !req.received)
        };

        let mut cnt = 0;

        // Synchronization loop
        while cnt < MAX_SYNC_ATTEMPTS && !done {
            // Get list of peers we're waiting for
            let asked_peers: Vec<PeerNode> = {
                let requested = self.requested_blocks.lock().unwrap();
                requested
                    .values()
                    .flat_map(|req| {
                        if req.peers.is_empty() {
                            // Empty peers means broadcast - check everyone
                            network_map.keys().cloned().collect()
                        } else {
                            req.peers.clone()
                        }
                    })
                    .collect()
            };

            // Drain queues of asked peers
            for peer in asked_peers {
                if let Some(&idx) = network_map.get(&peer) {
                    nodes[idx].handle_receive().await?;
                }
            }

            // Handle receive on this node
            self.handle_receive().await?;

            // Check if we're done
            done = {
                let requested = self.requested_blocks.lock().unwrap();
                !requested.values().any(|req| !req.received)
            };
            cnt += 1;
        }

        // Log results
        if !done {
            let requested = self.requested_blocks.lock().unwrap();
            let pending: Vec<String> = requested
                .iter()
                .filter(|(_, req)| !req.received)
                .map(|(hash, req)| {
                    format!(
                        "{} -> {:?}",
                        models::rust::casper::pretty_printer::PrettyPrinter::build_string_no_limit(
                            hash
                        ),
                        req
                    )
                })
                .collect();

            tracing::warn!(
                "Node {} still pending requests for blocks (after {} attempts): {:?}",
                self.local,
                MAX_SYNC_ATTEMPTS,
                pending
            );
        } else {
            let peer_names: Vec<String> = network_map.keys().map(|p| p.to_string()).collect();
            tracing::info!(
                "Node {} has exchanged all the requested blocks with [{}] after {} round(s)",
                self.local,
                peer_names.join("; "),
                cnt
            );
        }

        Ok(())
    }

    /// Synchronizes with a single node.
    pub async fn sync_with_one(&mut self, node: &mut TestNode) -> Result<(), CasperError> {
        self.sync_with(&mut [node]).await
    }

    /// Checks if this node contains a block (equivalent to Scala contains, line 346).
    pub fn contains(&self, block_hash: &BlockHash) -> bool { self.casper.contains(block_hash) }

    /// Checks if this node knows about a block (in storage or requested) (equivalent to Scala knowsAbout, line 347-348).
    pub fn knows_about(&self, block_hash: &BlockHash) -> bool {
        // Check if in storage
        let in_storage = self.contains(block_hash);

        // Check if in requested blocks
        let in_requested = {
            let requested = self.requested_blocks.lock().unwrap();
            requested.contains_key(block_hash)
        };

        in_storage || in_requested
    }

    /// Shuts off this node by clearing its transport layer queue (equivalent to Scala shutoff, line 350).
    ///
    /// This is useful for simulating network partitions or node failures in tests.
    pub fn shutoff(&self) -> Result<(), CommError> { self.tle.test_network().clear(&self.local) }

    pub async fn handle_receive(&self) -> Result<(), CasperError> {
        let tle = self.tle.clone();
        let connections_cell = self.connections_cell.clone();
        let rp_conf = self.rp_conf.clone();
        let packet_handler = self.packet_handler.clone();

        // Clone casper and block_processor for direct BlockMessage processing
        let casper = self.casper.clone();
        let block_processor = self.block_processor.clone();

        let dispatch = Arc::new(
            move |protocol: Protocol| -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<CommunicationResponse, CommError>>
                        + Send,
                >,
            > {
                let tle = tle.clone();
                let connections_cell = connections_cell.clone();
                let rp_conf = rp_conf.clone();
                let packet_handler = packet_handler.clone();
                let casper = casper.clone();
                let block_processor = block_processor.clone(); // Clone Arc for this invocation

                Box::pin(async move {
                    match protocol.message {
                        Some(models::routing::protocol::Message::Packet(ref packet)) => {
                            // Extract peer from protocol header
                            let header = protocol.header.as_ref().ok_or_else(|| {
                                CommError::UnexpectedMessage("No header in protocol".to_string())
                            })?;

                            let sender_node = header.sender.as_ref().ok_or_else(|| {
                                CommError::UnexpectedMessage("No sender in header".to_string())
                            })?;

                            // Convert Node to PeerNode
                            let peer = PeerNode {
                                id: NodeIdentifier::new(hex::encode(&sender_node.id)),
                                endpoint: Endpoint::new(
                                    String::from_utf8_lossy(&sender_node.host).to_string(),
                                    sender_node.tcp_port,
                                    sender_node.udp_port,
                                ),
                            };

                            // Parse CasperMessage to check if it's a BlockMessage
                            use casper::rust::protocol::{
                                casper_message_from_proto, to_casper_message_proto,
                            };
                            use models::rust::casper::protocol::casper_message::CasperMessage;

                            let parse_result = to_casper_message_proto(packet).get();
                            if let Ok(proto) = parse_result {
                                if let Ok(casper_msg) = casper_message_from_proto(proto) {
                                    match casper_msg {
                                        CasperMessage::BlockMessage(block) => {
                                            // Call process_block_through_pipe (static method)
                                            let _result = TestNode::process_block_through_pipe(
                                                casper.clone(),
                                                &block_processor,
                                                block,
                                            )
                                            .await
                                            .map_err(|e| CommError::CasperError(e.to_string()))?;

                                            return Ok(
                                                CommunicationResponse::handled_without_message(),
                                            );
                                        }
                                        _ => {
                                            // All other messages: use engine as before
                                            packet_handler.handle_packet(&peer, packet).await?;
                                            return Ok(
                                                CommunicationResponse::handled_without_message(),
                                            );
                                        }
                                    }
                                }
                            }

                            // Fallback: if parsing failed, use packet handler
                            packet_handler.handle_packet(&peer, packet).await?;
                            Ok(CommunicationResponse::handled_without_message())
                        }
                        _ => {
                            handle_messages::handle(
                                &protocol,
                                tle.clone(),
                                Arc::new(NOPPacketHandler::new()),
                                &connections_cell,
                                &rp_conf,
                            )
                            .await
                        }
                    }
                })
            },
        );

        let handle_streamed = Arc::new(
            |_blob: Blob| -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<(), CommError>> + Send>,
            > { Box::pin(async move { Ok(()) }) },
        );

        drop(self.tls.handle_receive(dispatch, handle_streamed).await?);

        Ok(())
    }

    /// Creates a standalone TestNode (single node network)
    pub async fn standalone(genesis: GenesisContext) -> Result<TestNode, CasperError> {
        let nodes = Self::create_network(genesis, 1, None, None, None, None).await?;

        Ok(nodes.into_iter().next().unwrap())
    }

    /// Creates a network of TestNodes
    pub async fn create_network(
        genesis: GenesisContext,
        network_size: usize,
        synchrony_constraint_threshold: Option<f64>,
        max_number_of_parents: Option<i32>,
        max_parent_depth: Option<i32>,
        with_read_only_size: Option<usize>,
    ) -> Result<Vec<TestNode>, CasperError> {
        Self::create_network_with_finalization_rate(
            genesis,
            network_size,
            synchrony_constraint_threshold,
            max_number_of_parents,
            max_parent_depth,
            with_read_only_size,
            1,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_network_with_finalization_rate(
        genesis: GenesisContext,
        network_size: usize,
        synchrony_constraint_threshold: Option<f64>,
        max_number_of_parents: Option<i32>,
        max_parent_depth: Option<i32>,
        with_read_only_size: Option<usize>,
        finalization_rate: i32,
    ) -> Result<Vec<TestNode>, CasperError> {
        // Initialize the shared tracing subscriber once per test process.
        // Without this, tracing calls in production code are silently
        // dropped during tests, defeating diagnostic intent. Tests opt
        // in by going through create_network; RUST_LOG is honored.
        crate::init_logger();

        let test_network = TestNetwork::empty();

        // Take the required number of validator keys
        let total_nodes = network_size + with_read_only_size.unwrap_or(0);
        let sks_to_use: Vec<PrivateKey> = genesis
            .validator_sks()
            .into_iter()
            .take(total_nodes)
            .collect();

        Self::network(
            sks_to_use,
            genesis.clone(),
            synchrony_constraint_threshold.unwrap_or(0.0),
            max_number_of_parents.unwrap_or(Estimator::UNLIMITED_PARENTS),
            max_parent_depth,
            with_read_only_size.unwrap_or(0),
            None,
            test_network,
            finalization_rate,
            vec![None; total_nodes],
        )
        .await
    }

    pub async fn create_network_with_bootstrap_index(
        genesis: GenesisContext,
        network_size: usize,
        bootstrap_index: usize,
    ) -> Result<Vec<TestNode>, CasperError> {
        crate::init_logger();

        let test_network = TestNetwork::empty();
        let sks_to_use: Vec<PrivateKey> = genesis
            .validator_sks()
            .into_iter()
            .take(network_size)
            .collect();

        Self::network(
            sks_to_use,
            genesis,
            0.0,
            Estimator::UNLIMITED_PARENTS,
            None,
            0,
            Some(bootstrap_index),
            test_network,
            1,
            vec![None; network_size],
        )
        .await
    }

    /// Creates a network with per-node fs provisioning.  Each entry in
    /// `fs_provisionings` corresponds positionally to a node — passing
    /// `Some(cfg)` wires that node's RuntimeManager with a payload
    /// store + root-identity registry entries, matching what production
    /// boot (`node::runtime::setup:245-466`) does when
    /// `storage.file-io-provisioning` has consensus-static entries.
    ///
    /// Panics if `fs_provisionings.len() != network_size` — position
    /// matters, so a mismatched length would silently drop provisioning
    /// for the trailing nodes.
    pub async fn create_network_with_fs_provisioning(
        genesis: GenesisContext,
        network_size: usize,
        fs_provisionings: Vec<Option<TestFsProvisioning>>,
    ) -> Result<Vec<TestNode>, CasperError> {
        assert_eq!(
            fs_provisionings.len(),
            network_size,
            "TestNode::create_network_with_fs_provisioning: \
             fs_provisionings.len() ({}) must equal network_size ({})",
            fs_provisionings.len(),
            network_size,
        );

        crate::init_logger();

        let test_network = TestNetwork::empty();
        let sks_to_use: Vec<PrivateKey> = genesis
            .validator_sks()
            .into_iter()
            .take(network_size)
            .collect();

        Self::network(
            sks_to_use,
            genesis,
            0.0,
            Estimator::UNLIMITED_PARENTS,
            None,
            0,
            None,
            test_network,
            1,
            fs_provisionings,
        )
        .await
    }

    /// Creates a network of TestNodes
    #[allow(clippy::too_many_arguments)]
    async fn network(
        sks: Vec<PrivateKey>,
        genesis_context: GenesisContext,
        synchrony_constraint_threshold: f64,
        max_number_of_parents: i32,
        max_parent_depth: Option<i32>,
        with_read_only_size: usize,
        bootstrap_index: Option<usize>,
        test_network: TestNetwork,
        finalization_rate: i32,
        fs_provisionings: Vec<Option<TestFsProvisioning>>,
    ) -> Result<Vec<TestNode>, CasperError> {
        let genesis = genesis_context.genesis_block.clone();
        let n = sks.len();
        assert_eq!(
            fs_provisionings.len(),
            n,
            "TestNode::network: fs_provisionings.len() ({}) must equal sks.len() ({})",
            fs_provisionings.len(),
            n,
        );

        // Generate node names: "node-1", "node-2", ..., "readOnly-{i}" for read-only nodes
        let names: Vec<String> = (1..=n)
            .map(|i| {
                if i <= (n - with_read_only_size) {
                    format!("node-{}", i)
                } else {
                    format!("readOnly-{}", i)
                }
            })
            .collect();

        // Generate is_read_only flags
        let is_read_only: Vec<bool> = (1..=n).map(|i| i > (n - with_read_only_size)).collect();

        // Generate peers using port 40400
        let peers: Vec<PeerNode> = names
            .iter()
            .map(|name| Self::peer_node(name, 40400))
            .collect();
        let bootstrap_peer = bootstrap_index.and_then(|index| peers.get(index).cloned());

        // Create nodes
        let mut nodes = Vec::new();
        for ((((name, peer), sk), is_readonly), fs_prov) in names
            .into_iter()
            .zip(peers.into_iter())
            .zip(sks.into_iter())
            .zip(is_read_only.into_iter())
            .zip(fs_provisionings.into_iter())
        {
            let node = Self::create_node(
                name,
                peer,
                genesis.clone(),
                sk,
                synchrony_constraint_threshold,
                max_number_of_parents,
                max_parent_depth,
                is_readonly,
                test_network.clone(),
                &genesis_context,
                bootstrap_peer.clone(),
                finalization_rate,
                fs_prov,
            )
            .await;
            nodes.push(node);
        }

        // Set up connections between all nodes
        for node_a in &nodes {
            for node_b in &nodes {
                if node_a.local != node_b.local {
                    // Add connection from node_a to node_b
                    node_a
                        .connections_cell
                        .flat_modify(|connections| connections.add_conn(node_b.local.clone()))
                        .map_err(|e| {
                            CasperError::RuntimeError(format!("Connection setup failed: {}", e))
                        })?;
                }
            }
        }

        Ok(nodes)
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    async fn create_node(
        name: String,
        current_peer_node: PeerNode,
        genesis: BlockMessage,
        sk: PrivateKey,
        // TODO: logical_time: LogicalTime,
        synchrony_constraint_threshold: f64,
        max_number_of_parents: i32,
        max_parent_depth: Option<i32>,
        is_read_only: bool,
        test_network: TestNetwork,
        genesis_context: &GenesisContext,
        bootstrap_peer: Option<PeerNode>,
        finalization_rate: i32,
        fs_provisioning: Option<TestFsProvisioning>,
    ) -> TestNode {
        let tle = Arc::new(TransportLayerTestImpl::new(test_network.clone()));
        let tls =
            TransportLayerServerTestImpl::new(current_peer_node.clone(), test_network.clone());

        // With shared LMDB, we don't need to copy storage directories.
        // Use the shared LMDB path for data_dir (for logging/debugging purposes only).
        let _new_storage_dir = resources::get_shared_lmdb_path();
        // Use mk_test_rnode_store_manager_with_shared_rspace to get a new scope with genesis data copied
        // This ensures test isolation for blocks/DAG (each TestNode has its own scope)
        // while sharing RSpace scope so all nodes in this test can see each other's state
        let mut kvm = resources::mk_test_rnode_store_manager_with_shared_rspace(
            genesis_context,
            &genesis_context.rspace_scope_id,
        )
        .await
        .expect("Failed to create store manager with shared RSpace");

        let block_store_base = KeyValueBlockStore::create_from_kvm(&mut *kvm)
            .await
            .unwrap();
        let block_store = block_store_base;

        // Initialize block store with genesis block
        block_store
            .put(genesis.block_hash.clone(), &genesis)
            .expect("Failed to store genesis block in TestNode");

        let block_dag_storage = resources::block_dag_storage_from_dyn(&mut *kvm)
            .await
            .unwrap();

        // Initialize DAG storage with genesis block metadata
        block_dag_storage
            .insert(
                &genesis,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
            )
            .expect("Failed to insert genesis into DAG storage in TestNode");
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            resources::key_value_deploy_storage_from_dyn(&mut *kvm)
                .await
                .unwrap(),
        ));

        let rejected_deploy_buffer = Arc::new(Mutex::new(
            resources::key_value_rejected_deploy_buffer_from_dyn(&mut *kvm)
                .await
                .unwrap(),
        ));

        let casper_buffer_storage = resources::casper_buffer_storage_from_dyn(&mut *kvm)
            .await
            .unwrap();

        let rspace_store = (*kvm).r_space_stores().await.unwrap();
        let mergeable_store = resources::mergeable_store_from_dyn(&mut *kvm)
            .await
            .unwrap();
        // Use create_with_history to ensure tests can reset to genesis state root hash
        let (runtime_manager, _) = RuntimeManager::create_with_history(
            rspace_store,
            mergeable_store,
            std::sync::Arc::new(Genesis::default_mergeable_tags()),
            rholang::rust::interpreter::external_services::ExternalServices::noop(),
        );

        // Wire fs provisioning (payload store + root-identity registry)
        // if the test asked for it.  Mirrors `node::runtime::setup:245-466`
        // — the two side-effects there that are consensus-observable
        // for Casper-level fileio tests are (a) the payload store,
        // which is where Consensus-cap writes stash bytes for joining
        // validators to fetch, and (b) the root-identity registry,
        // which every `safe_descend_verified` consults to detect
        // rename-and-recreate.  SnapshotWriter is INTENTIONALLY not
        // wired here — it only fires on LFB advance and the canary
        // tests exercise pre-finalization state.  Tests that need
        // snapshot emission can call `runtime_manager.
        // set_fs_snapshot_writer` themselves post-construction.
        if let Some(prov) = fs_provisioning {
            use casper::rust::engine::wal_payload_server::{
                BlockStorageBackedRecorder, DirectoryPayloadStore, PayloadStoreBundle,
            };
            use rholang::rust::interpreter::io::path::capture_root_identity;

            std::fs::create_dir_all(&prov.payload_dir)
                .unwrap_or_else(|e| panic!(
                    "TestNode({}): create_dir_all({:?}) failed: {e}",
                    name, prov.payload_dir,
                ));

            let store_bundle = PayloadStoreBundle::from_directory(
                DirectoryPayloadStore::new(prov.payload_dir.clone()),
            );
            runtime_manager.set_payload_store(Some(store_bundle)).await;

            // DD-7b-2 (a) Option 2 (2026-08-30): mirror the production
            // wire-in from `node::runtime::setup` — attach the
            // block-storage-backed recorder so leader-side
            // journal_write populates the `payload_source_index`
            // for the joiner-side Option 2 reducer to walk.  Without
            // this, the Option 2 E2E canary would see an empty index
            // even after a Consensus write.
            let recorder = std::sync::Arc::new(
                BlockStorageBackedRecorder::new(block_dag_storage.clone()),
            )
                as std::sync::Arc<
                    dyn rholang::rust::interpreter::io::wal::PayloadSourceRecorder,
                >;
            runtime_manager
                .set_payload_source_recorder(Some(recorder))
                .await;

            for root in &prov.root_paths {
                match capture_root_identity(root) {
                    Ok(id) => runtime_manager.register_root_identity(root.clone(), id),
                    Err(e) => tracing::warn!(
                        target: "f1r3fly.test.fs_provisioning",
                        node = %name,
                        path = ?root,
                        error = %e,
                        "TestFsProvisioning: capture_root_identity failed; \
                         first syscall on this root will fail with RootIdentityChanged"
                    ),
                }
            }

            // Shape A (Phase 0.1, 2026-08-31): per-validator logical
            // → on-disk registrations for Consensus bundle entries.
            // The (dev, inode) identity is captured from `on_disk`
            // (the real staging dir) so H-5's fstat-post-open check
            // still verifies against a genuine filesystem inode.
            // Logical roots (`/@bundle/<X>`) are validator-
            // independent; on-disk staging dirs are per-node.
            for (logical, on_disk) in &prov.logical_to_on_disk {
                match capture_root_identity(on_disk) {
                    Ok(id) => runtime_manager.register_root_remap(
                        logical.clone(),
                        on_disk.clone(),
                        id,
                    ),
                    Err(e) => tracing::warn!(
                        target: "f1r3fly.test.fs_provisioning",
                        node = %name,
                        logical = ?logical,
                        on_disk = ?on_disk,
                        error = %e,
                        "TestFsProvisioning: Shape A remap capture_root_identity failed; \
                         first syscall on this root will fail with RootIdentityChanged"
                    ),
                }
            }
        }

        let connections_cell = ConnectionsCell::new();
        let _clique_oracle = CliqueOracleImpl;
        let estimator = Estimator::apply(max_number_of_parents, max_parent_depth);
        let mut rp_conf = create_rp_conf_ask(current_peer_node.clone(), None, None);
        if let Some(bootstrap_peer) = bootstrap_peer {
            rp_conf.bootstrap = Some(bootstrap_peer);
        }
        let event_publisher = F1r3flyEvents::new();
        // Scala: implicit val requestedBlocks: RequestedBlocks[F] = Ref.unsafe[F, Map[BlockHash, RequestState]](Map.empty)
        let requested_blocks = Arc::new(Mutex::new(HashMap::<BlockHash, RequestState>::new()));
        // Scala: implicit val blockRetriever: BlockRetriever[F] = BlockRetriever.of[F]
        let block_retriever = BlockRetriever::new(
            requested_blocks.clone(),
            tle.clone(),
            connections_cell.clone(),
            rp_conf.clone(),
        );

        let _ = test_network.add_peer(&current_peer_node);

        // Proposer
        let validator_id_opt = if is_read_only {
            None
        } else {
            Some(ValidatorIdentity::new(&sk))
        };

        let _proposer_opt = validator_id_opt.as_ref().map(|vi| {
            new_proposer(
                vi.clone(),
                None,
                runtime_manager.clone(),
                block_store.clone(),
                deploy_storage.clone(),
                rejected_deploy_buffer.clone(),
                std::sync::Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
                block_retriever.clone(),
                tle.clone(),
                connections_cell.clone(),
                rp_conf.clone(),
                event_publisher.clone(),
                false, // allow_empty_blocks - disabled for tests
            )
        });

        let bp_dependencies = BlockProcessorDependencies::new(
            block_store.clone(),
            casper_buffer_storage.clone(),
            block_dag_storage.clone(),
            block_retriever.clone(),
            tle.clone(),
            connections_cell.clone(),
            rp_conf.clone(),
        );

        let block_processor = BlockProcessor::new(bp_dependencies);

        // Creates an unbounded tokio channel for processing (Casper, BlockMessage) tuples
        // - Sender: Non-blocking, cloneable, used to enqueue blocks for processing
        // - Receiver: Thread-safe (Arc<Mutex>), used to dequeue blocks from processing pipeline
        let (block_processor_queue_tx, _block_processor_queue_rx) =
            BlockProcessingQueueSender::channel(1024, 64 * 1024 * 1024)
                .expect("block processing queue");

        let _block_processor_state = Arc::new(RwLock::new(HashSet::<BlockHash>::new()));

        let shard_id = "root".to_string();
        let _approved_block = ApprovedBlock {
            candidate: ApprovedBlockCandidate {
                block: genesis.clone(),
                required_sigs: 0,
            },
            sigs: vec![],
        };
        let shard_conf = CasperShardConf {
            fault_tolerance_threshold: 0.0,
            shard_name: shard_id.clone(),
            parent_shard_id: "".to_string(),
            finalization_rate,
            max_number_of_parents,
            max_parent_depth: max_parent_depth.unwrap_or(i32::MAX),
            synchrony_constraint_threshold: synchrony_constraint_threshold as f32,
            height_constraint_threshold: i64::MAX,
            // Validators will try to put deploy in a block only for next `deployLifespan` blocks.
            // Required to enable protection from re-submitting duplicate deploys
            deploy_lifespan: 50,
            casper_version: casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            config_version: 1,
            bond_minimum: 0,
            bond_maximum: i64::MAX,
            epoch_length: 10000,
            quarantine_length: 20000,
            min_phlo_price: 1,
            disable_late_block_filtering: true, // Disabled to prevent deploy loss
            deploy_heartbeat_wake_enabled: false, // Disabled to prevent deploy loss
            disable_validator_progress_check: false,
            enable_mergeable_channel_gc: false, // Keep mergeable data unless GC is explicitly enabled
            mergeable_channels_gc_depth_buffer: 10,
            ..CasperShardConf::new()
        };

        let casper_impl = MultiParentCasperImpl {
            block_retriever: block_retriever.clone(),
            event_publisher: event_publisher.clone(),
            runtime_manager: Arc::new(runtime_manager.clone()),
            estimator: estimator.clone(),
            block_store: block_store.clone(),
            block_dag_storage: block_dag_storage.clone(),
            deploy_storage: deploy_storage.clone(),
            pending_cosigner_metadata: std::sync::Arc::new(parking_lot::Mutex::new(
                std::collections::HashMap::new(),
            )),
            rejected_deploy_buffer: rejected_deploy_buffer.clone(),
            casper_buffer_storage: casper_buffer_storage.clone(),
            validator_id: validator_id_opt.clone(),
            casper_shard_conf: shard_conf,
            approved_block: genesis.clone(),
            finalization_in_progress: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            recovery_sync_active: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            finalization_schedule: std::sync::Arc::new(
                casper::rust::finality::finalization_schedule::FinalizationSchedule::new(2),
            ),
            heartbeat_signal_ref: casper::rust::heartbeat_signal::new_heartbeat_signal_ref(),
            deploys_in_scope_cache: std::sync::Arc::new(parking_lot::Mutex::new(None)),
            active_validators_cache: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        };

        let casper = Arc::new(casper_impl);

        // Create Running engine

        // Create the_init as a no-op async function
        let the_init: Arc<
            dyn Fn() -> std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<(), CasperError>> + Send>,
                > + Send
                + Sync,
        > = Arc::new(|| Box::pin(async { Ok(()) }));

        let engine_cell = EngineCell::init();

        let running_engine = Running::new(
            block_processor_queue_tx, // block_processing_queue_tx
            Arc::new(DashSet::new()), // blocks_in_processing
            casper.clone() as Arc<dyn MultiParentCasper + Send + Sync>, // casper
            _approved_block.clone(),  // approved_block
            the_init,                 // the_init
            true,                     // disable_state_exporter
            tle.clone(),              // transport
            rp_conf.clone(),          // conf
            block_retriever.clone(),  // block_retriever
            Some(RunningRecoveryContext {
                connections_cell: connections_cell.clone(),
            }),
        );
        engine_cell.set(Arc::new(running_engine)).await;

        // Create CasperPacketHandler
        let packet_handler = CasperPacketHandler::new(engine_cell.clone());

        TestNode {
            name,
            local: current_peer_node,
            tle,
            tls,
            genesis,
            validator_id_opt,
            block_processor,
            block_store,
            block_dag_storage,
            deploy_storage,
            rejected_deploy_buffer,
            runtime_manager,
            requested_blocks,
            connections_cell,
            rp_conf,
            casper,
            engine_cell,
            packet_handler,
            allow_empty_blocks: false,
        }
    }

    /// Creates a PeerNode with the given name and port
    fn peer_node(name: &str, port: u32) -> PeerNode {
        // Convert name bytes to hex string for NodeIdentifier
        let name_hex = hex::encode(name.as_bytes());
        let node_id = NodeIdentifier::new(name_hex);
        let endpoint = Self::endpoint(port);

        PeerNode {
            id: node_id,
            endpoint,
        }
    }

    /// Creates an endpoint with the given port for both TCP and UDP
    fn endpoint(port: u32) -> Endpoint { Endpoint::new("host".to_string(), port, port) }

    /// Propagates messages across all nodes until all queues are empty (equivalent to Scala propagate, line 640-649).
    ///
    /// This static method:
    /// 1. Repeatedly calls handleReceive on all nodes
    /// 2. Checks if all message queues are empty after each round
    /// 3. Continues until all queues are empty (heat death) or max iterations
    ///
    /// This is useful for simulating complete message propagation in tests.
    ///
    /// # Parameters
    /// * `nodes` - All nodes in the network to propagate messages between
    pub async fn propagate(nodes: &mut [&mut TestNode]) -> Result<(), CasperError> {
        if nodes.is_empty() {
            return Ok(());
        }

        const MAX_PROPAGATION_ROUNDS: usize = 100;
        let mut rounds = 0;

        // Keep propagating until queues are empty or max rounds
        loop {
            if rounds >= MAX_PROPAGATION_ROUNDS {
                tracing::warn!(
                    "Propagation stopped after {} rounds - queues may not be empty",
                    MAX_PROPAGATION_ROUNDS
                );
                break;
            }

            // Call handleReceive on all nodes (matching Scala's traverse_)
            for node in nodes.iter() {
                node.handle_receive().await?;
            }

            // Check heat death: all queues empty
            let mut any_messages = false;
            for node in nodes.iter() {
                let queue_size = node
                    .tle
                    .test_network()
                    .peer_queue(&node.local)
                    .unwrap_or_else(|_| std::collections::VecDeque::new())
                    .len();
                if queue_size > 0 {
                    any_messages = true;
                    break;
                }
            }

            // If no messages remain, we've reached heat death
            if !any_messages {
                break;
            }

            rounds += 1;
        }

        tracing::debug!("Propagation completed after {} rounds", rounds);
        Ok(())
    }
}
