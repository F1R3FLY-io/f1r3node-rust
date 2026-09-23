use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, DeployId, InsertMode, KeyValueDagRepresentation,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::casper::{
    Casper, CasperShardConf, CasperSnapshot, DeployError, MultiParentCasper,
};
use casper::rust::engine::engine::Engine;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::errors::CasperError;
use casper::rust::soak_observer::evaluation::{
    AuthorityRequest, AuthorityResponse, CaptureOptions, FloorSelection, Value,
};
use casper::rust::soak_observer::{
    AttachmentError, CaptureEndpoint, EventKind, FloorOutcome, ObserverBinding, ObserverController,
    EVENT_CAPACITY,
};
use casper::rust::util::clique::Clique;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::validator_identity::ValidatorIdentity;
use comm::rust::peer_node::PeerNode;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, CasperMessage, DeployData, Justification,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
    Db, LmdbDirStoreManager, LmdbEnvConfig,
};
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use shared::rust::dag::observation_work::{CheckedWork, WorkKind, WorkLimits, WorkMeter};
use shared::rust::store::key_value_store::KeyValueStore;

const STORES: [&str; 13] = [
    "block-metadata",
    "equivocation-tracker",
    "latest-messages",
    "invalid-blocks",
    "floor-index",
    "frontier-index",
    "deploy-lifecycle-events",
    "deploy-lifecycle-terminal",
    "carrier-index",
    "carrier-index-meta",
    "genesis-hash",
    "blocks",
    "blocks-approved",
];

struct Fixture {
    dag: BlockDagKeyValueStorage,
    blocks: KeyValueBlockStore,
    chain: Vec<BlockMessage>,
    handles: Vec<Arc<dyn KeyValueStore>>,
    _directory: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let validator = Bytes::from(vec![7; 65]);
        let mut chain: Vec<BlockMessage> = Vec::new();
        for height in 0..4 {
            let previous = chain.last().map(|b| b.block_hash.clone());
            let mut block = get_random_block(
                Some(height),
                Some(height as i32),
                None,
                None,
                Some(validator.clone()),
                Some(1),
                Some(height),
                Some(previous.clone().into_iter().collect()),
                Some(
                    previous
                        .into_iter()
                        .map(|hash| Justification {
                            validator: validator.clone(),
                            latest_block_hash: hash,
                        })
                        .collect(),
                ),
                Some(vec![]),
                Some(vec![]),
                Some(vec![Bond {
                    validator: validator.clone(),
                    stake: 100,
                }]),
                Some("root".to_string()),
                None,
            );
            block.block_hash = Bytes::from(vec![height as u8 + 1; 32]);
            chain.push(block);
        }
        Self::from_chain(chain).await
    }

    async fn from_chain(chain: Vec<BlockMessage>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mapping = STORES
            .iter()
            .map(|name| {
                (Db::new(name.to_string(), None), LmdbEnvConfig {
                    name: if name.starts_with("blocks") {
                        "blocks"
                    } else {
                        "dag"
                    }
                    .to_string(),
                    max_env_size: 64 * 1024 * 1024,
                    max_dbs: 32,
                })
            })
            .collect();
        let mut manager = LmdbDirStoreManager::new(directory.path().to_path_buf(), mapping);
        let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
        let blocks = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let mut handles = Vec::new();
        for name in STORES {
            handles.push(manager.store(name.to_string()).await.unwrap());
        }
        for (index, block) in chain.iter().enumerate() {
            dag.insert(
                block,
                if index == 0 {
                    InsertMode::Approved
                } else {
                    InsertMode::Normal
                },
            )
            .unwrap();
            blocks.put_block_message(block).unwrap();
        }
        Self {
            dag,
            blocks,
            chain,
            handles,
            _directory: directory,
        }
    }

    fn stored(&self) -> Vec<BTreeMap<Vec<u8>, Vec<u8>>> {
        self.handles
            .iter()
            .map(|handle| handle.to_map().unwrap())
            .collect()
    }

    fn casper(&self, supported: bool) -> Arc<AttachedCasper> {
        let mut conf = CasperShardConf::new();
        conf.fault_tolerance_threshold_ppm = 1_000_000;
        conf.fault_tolerance_threshold = -1.0;
        conf.max_parent_depth = 12;
        conf.deploy_lifespan = 50;
        Arc::new(AttachedCasper {
            dag: self.dag.clone(),
            blocks: self.blocks.clone(),
            approved: self.chain[0].clone(),
            conf,
            binding: OnceLock::new(),
            supported,
        })
    }

    fn request(&self) -> AuthorityRequest {
        AuthorityRequest {
            capture: CaptureOptions {
                max_value_bytes: 1 << 20,
                max_total_bytes: 1 << 24,
                max_records: 4096,
                max_operations: 100_000,
                max_compressed_bytes: 1 << 20,
                max_decompressed_bytes: 1 << 20,
                max_expansion_ratio: 4096,
                max_blocks: 128,
                max_validators: 64,
                max_edges: 4096,
                max_work: 2_000_000,
                lock_wait_ms: 100,
            },
            evaluation: work_limits(),
            targets: vec![hex::encode(&self.chain[1].block_hash)],
            body_hashes: self
                .chain
                .iter()
                .map(|b| hex::encode(&b.block_hash))
                .collect(),
            floor: Some(FloorSelection::View),
            original: true,
            reference: true,
            strict: false,
        }
    }
}

struct AttachedCasper {
    dag: BlockDagKeyValueStorage,
    blocks: KeyValueBlockStore,
    approved: BlockMessage,
    conf: CasperShardConf,
    binding: OnceLock<ObserverBinding>,
    supported: bool,
}

#[async_trait]
impl MultiParentCasper for AttachedCasper {
    fn attach_observer(
        &self,
        binding: ObserverBinding,
    ) -> Result<CaptureEndpoint, AttachmentError> {
        if !self.supported {
            return Err(AttachmentError::Unsupported);
        }
        self.binding
            .set(binding)
            .map_err(|_| AttachmentError::AlreadyAttached)?;
        Ok(CaptureEndpoint::new(
            self.dag.clone(),
            self.blocks.clone(),
            &self.conf,
            &self.approved,
        ))
    }
    async fn fetch_dependencies(&self) -> Result<(), CasperError> {
        panic!("observer invoked consensus")
    }
    fn normalized_initial_fault(&self, _: HashMap<Validator, u64>) -> Result<f32, CasperError> {
        panic!("observer read the equivocation tracker")
    }
    async fn last_finalized_block(&self) -> Result<BlockMessage, CasperError> {
        panic!("observer invoked consensus")
    }
    async fn block_dag(&self) -> Result<KeyValueDagRepresentation, CasperError> {
        panic!("observer invoked consensus")
    }
    fn block_store(&self) -> &KeyValueBlockStore { panic!("observer invoked consensus") }
    fn casper_shard_conf(&self) -> &CasperShardConf { panic!("observer invoked consensus") }
    fn get_validator(&self) -> Option<ValidatorIdentity> {
        panic!("observer read validator identity")
    }
    async fn get_history_exporter(&self) -> Arc<dyn RSpaceExporter> {
        panic!("observer read runtime state")
    }
    fn runtime_manager(&self) -> Arc<RuntimeManager> { panic!("observer read runtime state") }
    async fn has_pending_deploys_in_storage(&self) -> Result<bool, CasperError> {
        panic!("observer invoked consensus")
    }
}

#[async_trait]
impl Casper for AttachedCasper {
    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
        panic!("observer invoked the production snapshot")
    }
    fn contains(&self, _: &BlockHash) -> bool { panic!("observer invoked consensus") }
    fn dag_contains(&self, _: &BlockHash) -> bool { panic!("observer invoked consensus") }
    fn buffer_contains(&self, _: &BlockHash) -> bool { panic!("observer invoked consensus") }
    fn get_approved_block(&self) -> Result<&BlockMessage, CasperError> {
        panic!("observer invoked consensus")
    }
    fn deploy(&self, _: Signed<DeployData>) -> Result<Either<DeployError, DeployId>, CasperError> {
        panic!("observer invoked consensus")
    }
    async fn estimator(
        &self,
        _: &mut KeyValueDagRepresentation,
    ) -> Result<Vec<BlockHash>, CasperError> {
        panic!("observer invoked consensus")
    }
    fn get_version(&self) -> i64 { panic!("observer invoked consensus") }
    async fn validate(
        &self,
        _: &BlockMessage,
        _: &mut CasperSnapshot,
    ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
        panic!("observer invoked consensus")
    }
    async fn validate_self_created(
        &self,
        _: &BlockMessage,
        _: &mut CasperSnapshot,
        _: Bytes,
        _: Bytes,
    ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
        panic!("observer invoked consensus")
    }
    async fn handle_valid_block(
        &self,
        _: &BlockMessage,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        panic!("observer invoked consensus")
    }
    fn handle_invalid_block(
        &self,
        _: &BlockMessage,
        _: &InvalidBlock,
        _: &KeyValueDagRepresentation,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        panic!("observer invoked consensus")
    }
    fn get_dependency_free_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
        panic!("observer invoked consensus")
    }
    fn get_all_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
        panic!("observer invoked consensus")
    }
}

struct InstalledEngine {
    casper: Option<Arc<AttachedCasper>>,
    accesses: AtomicUsize,
}

#[async_trait]
impl Engine for InstalledEngine {
    async fn init(&self) -> Result<(), CasperError> { panic!("observer invoked engine init") }
    async fn handle(&self, _: PeerNode, _: CasperMessage) -> Result<(), CasperError> {
        panic!("observer invoked engine handle")
    }
    fn with_casper(&self) -> Option<Arc<dyn MultiParentCasper + Send + Sync>> {
        self.accesses.fetch_add(1, Ordering::SeqCst);
        self.casper
            .as_ref()
            .map(|casper| casper.clone() as Arc<dyn MultiParentCasper + Send + Sync>)
    }
}

fn work_limits() -> WorkLimits {
    WorkLimits {
        operations: 2_000_000,
        allocated_bytes: 268_435_456,
        clique_expansions: 100_000,
        recursion_depth: 64,
    }
}

fn meter(limits: WorkLimits) -> CheckedWork {
    CheckedWork::new(
        limits,
        Instant::now() + Duration::from_secs(10),
        Arc::new(AtomicBool::new(false)),
        4096,
        8192,
    )
    .unwrap()
}

async fn evaluate(fixture: &Fixture, controller: &ObserverController) -> AuthorityResponse {
    controller
        .authority_snapshot(fixture.request(), Instant::now() + Duration::from_secs(10))
        .await
        .unwrap()
}

fn available<T: std::fmt::Debug>(value: &Value<T>) -> &T {
    match value {
        Value::Available { value, .. } => value,
        other => panic!("unexpected value: {other:?}"),
    }
}

#[tokio::test]
async fn disabled_engine_installation_does_not_access_casper() {
    let engine = Arc::new(InstalledEngine {
        casper: None,
        accesses: AtomicUsize::new(0),
    });
    let cell = EngineCell::init();
    cell.set(engine.clone()).await;
    assert_eq!(engine.accesses.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn attachment_is_once_only_and_replacement_closes_the_old_interval() {
    let fixture = Fixture::new().await;
    let controller = ObserverController::new("incarnation-a".to_string());
    assert_eq!(controller.status(), "awaiting_casper");
    let cell = EngineCell::observed(controller.clone());
    let unsupported = fixture.casper(false);
    cell.set(Arc::new(InstalledEngine {
        casper: Some(unsupported),
        accesses: AtomicUsize::new(0),
    }))
    .await;
    assert_eq!(controller.status(), "unsupported");
    let first = fixture.casper(true);
    cell.set(Arc::new(InstalledEngine {
        casper: Some(first.clone()),
        accesses: AtomicUsize::new(0),
    }))
    .await;
    let old = first.binding.get().unwrap();
    assert!(old.is_active());
    let second_controller = ObserverController::new("incarnation-b".to_string());
    second_controller.install(Some(first.as_ref()));
    assert_eq!(second_controller.status(), "already_attached");
    assert!(old.is_active());
    let second = fixture.casper(true);
    controller.install(Some(second.as_ref()));
    assert!(!old.is_active());
    assert_eq!(old.operation(), None);
    assert!(second.binding.get().unwrap().is_active());
    let response = evaluate(&fixture, &controller).await;
    let coverage = response.coverage.unwrap();
    assert_eq!(coverage.incarnation, "incarnation-a");
    assert_eq!(coverage.installation, 3);
    controller.install(Some(second.as_ref()));
    assert_eq!(controller.status(), "already_attached");
    assert!(!second.binding.get().unwrap().is_active());
    controller.close();
    controller.install(Some(fixture.casper(true).as_ref()));
    assert_eq!(controller.status(), "closed");
}

#[tokio::test]
async fn detached_evaluation_preserves_stores_and_compares_separate_results() {
    let fixture = Fixture::new().await;
    let before = fixture.stored();
    let controller = ObserverController::new("test".to_string());
    let casper = fixture.casper(true);
    controller.install(Some(casper.as_ref()));
    let response = evaluate(&fixture, &controller).await;
    assert_eq!(before, fixture.stored());
    assert!(!response.live_profile_qualified);
    assert_eq!(response.authority.fault_tolerance_threshold_ppm, 1_000_000);
    let target = &response.targets[0];
    assert!(*available(&target.oracle_decision));
    assert_eq!(
        *available(&target.original_fault_tolerance),
        1.0f32.to_bits()
    );
    assert!(available(&target.reference_comparison).decision_matches);
    assert_eq!(
        available(&target.reference_comparison).original_matches,
        Some(true)
    );
    assert!(available(&response.floor_comparison).matches);
    assert!(
        matches!(&target.display_projection, Value::Unavailable { reason, .. } if reason == "equivocation_snapshot_unavailable")
    );
    assert!(response.work.complete);
    assert!(response.work.measured.metadata > 0);
    assert!(response.work.measured.traversal > 0);
    assert!(response.work.measured.clique_expansions > 0);
    assert!(available(&response.work.original).operations > 0);
    assert!(available(&response.work.reference).operations > 0);
    assert!(response.coverage.unwrap().complete);
    let mut strict = fixture.request();
    strict.strict = true;
    let strict = controller
        .authority_snapshot(strict, Instant::now() + Duration::from_secs(10))
        .await
        .unwrap();
    assert!(!*available(&strict.targets[0].oracle_decision));
    assert_ne!(response.authority_digest, strict.authority_digest);
    assert_eq!(before, fixture.stored());
}

#[tokio::test]
async fn queue_loss_and_effect_failure_never_become_persistence() {
    let fixture = Fixture::new().await;
    let controller = ObserverController::new("test".to_string());
    let casper = fixture.casper(true);
    controller.install(Some(casper.as_ref()));
    let binding = casper.binding.get().unwrap();
    let operation = binding.operation();
    binding.emit(
        EventKind::LiveDerivation,
        operation,
        Some(FloorOutcome::Advance),
        Some(&fixture.chain[1].block_hash),
        Some(1),
        Some(true),
    );
    binding.emit(EventKind::EffectAttempt, operation, None, None, None, None);
    binding.emit(
        EventKind::EffectReturn,
        operation,
        None,
        None,
        None,
        Some(false),
    );
    for _ in 0..EVENT_CAPACITY {
        binding.emit(
            EventKind::LiveDerivation,
            None,
            Some(FloorOutcome::NoAdvance),
            None,
            None,
            Some(true),
        );
    }
    let response = evaluate(&fixture, &controller).await;
    let coverage = response.coverage.unwrap();
    assert!(!coverage.complete);
    assert!(coverage.lost >= 3);
    assert_eq!(response.events.len(), EVENT_CAPACITY);
    assert!(matches!(response.events[0].kind, EventKind::LiveDerivation));
    assert!(matches!(response.events[1].kind, EventKind::EffectAttempt));
    assert!(matches!(response.events[2].kind, EventKind::EffectReturn));
    assert_eq!(response.events[2].success, Some(false));
    assert!(response
        .events
        .iter()
        .all(|e| !matches!(e.kind, EventKind::PersistedObservation)));
    let next = evaluate(&fixture, &controller).await;
    assert!(!next.coverage.unwrap().complete);
}

#[tokio::test]
async fn corrupt_floor_cache_produces_a_reference_disagreement_without_repair() {
    let fixture = Fixture::new().await;
    let target = &fixture.chain[3].block_hash;
    fixture
        .dag
        .floor_index_for_tests()
        .put_one(
            BlockHashSerde(target.clone()),
            BlockHashSerde(target.clone()),
        )
        .unwrap();
    let before = fixture.stored();
    let controller = ObserverController::new("test".to_string());
    let casper = fixture.casper(true);
    controller.install(Some(casper.as_ref()));
    let mut request = fixture.request();
    request.floor = Some(FloorSelection::Block {
        hash: hex::encode(target),
    });
    let response = controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(10))
        .await
        .unwrap();
    assert!(!available(&response.floor_comparison).matches);
    assert_eq!(before, fixture.stored());
}

#[tokio::test]
async fn exhausted_preparation_returns_partial_counters_and_releases_busy() {
    let fixture = Fixture::new().await;
    let controller = ObserverController::new("test".to_string());
    let casper = fixture.casper(true);
    controller.install(Some(casper.as_ref()));
    let mut request = fixture.request();
    request.evaluation.operations = 2;
    let error = controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(10))
        .await
        .unwrap_err();
    assert!(error.reason.contains("operations"));
    assert_eq!(error.work.unwrap().aggregate.operations, 2);
    assert!(evaluate(&fixture, &controller).await.work.complete);
}

#[test]
fn work_paths_share_limits_and_retain_partial_counts() {
    let mut limits = work_limits();
    limits.operations = 3;
    let meter = meter(limits);
    meter
        .for_path(1)
        .unwrap()
        .step(WorkKind::Traversal)
        .unwrap();
    meter.for_path(2).unwrap().step(WorkKind::Oracle).unwrap();
    meter
        .for_path(3)
        .unwrap()
        .step(WorkKind::Signature)
        .unwrap();
    assert!(meter.step(WorkKind::Metadata).is_err());
    let (total, paths, failure) = meter.usage();
    assert_eq!(total.operations, 3);
    assert_eq!(paths[1].traversal, 1);
    assert_eq!(paths[2].oracle, 1);
    assert_eq!(paths[3].signature, 1);
    assert_eq!(failure.as_deref(), Some("operations"));
    assert!(meter.charge(WorkKind::Allocation, 0, 0).is_err());
}

#[test]
fn clique_matches_exhaustive_subsets_for_all_five_vertex_graphs() {
    let weights: HashMap<u8, i64> = (0..5).map(|n| (n, i64::from(n) + 1)).collect();
    let pairs: Vec<_> = (0..5)
        .flat_map(|a| (a + 1..5).map(move |b| (a, b)))
        .collect();
    for graph in 0..1usize << pairs.len() {
        let edges: Vec<_> = pairs
            .iter()
            .enumerate()
            .filter(|(index, _)| graph & (1 << index) != 0)
            .map(|(_, pair)| *pair)
            .collect();
        let mut expected = 0;
        for subset in 1usize..32 {
            let vertices: Vec<_> = (0..5).filter(|n| subset & (1 << n) != 0).collect();
            if vertices
                .iter()
                .all(|a| vertices.iter().all(|b| a >= b || edges.contains(&(*a, *b))))
            {
                expected = expected.max(vertices.iter().map(|v| weights[v]).sum());
            }
        }
        let meter = meter(work_limits());
        assert_eq!(
            Clique::find_maximum_clique_by_weight_metered(&meter, &edges, &weights).unwrap(),
            expected,
            "graph {graph}"
        );
        assert_eq!(
            Clique::find_maximum_clique_by_weight(&edges, &weights),
            expected
        );
    }
}

#[test]
fn clique_stops_at_each_work_limit() {
    let edges = vec![(0u8, 1), (0, 2), (1, 2)];
    let weights = HashMap::from([(0u8, 10), (1, 20), (2, 30)]);
    for (name, limits) in [
        ("operations", WorkLimits {
            operations: 1,
            ..work_limits()
        }),
        ("allocated_bytes", WorkLimits {
            allocated_bytes: 1,
            ..work_limits()
        }),
        ("clique_expansions", WorkLimits {
            clique_expansions: 1,
            ..work_limits()
        }),
        ("recursion_depth", WorkLimits {
            recursion_depth: 1,
            ..work_limits()
        }),
    ] {
        let meter = meter(limits);
        let error =
            Clique::find_maximum_clique_by_weight_metered(&meter, &edges, &weights).unwrap_err();
        assert!(error.to_string().contains(name), "{error}");
        assert_eq!(meter.usage().2.as_deref(), Some(name));
    }
    let cancellation = Arc::new(AtomicBool::new(true));
    let cancelled = CheckedWork::new(
        work_limits(),
        Instant::now() + Duration::from_secs(1),
        cancellation,
        0,
        0,
    )
    .unwrap();
    assert!(cancelled
        .step(WorkKind::Traversal)
        .unwrap_err()
        .to_string()
        .contains("cancelled"));
    let expired = CheckedWork::new(
        work_limits(),
        Instant::now(),
        Arc::new(AtomicBool::new(false)),
        0,
        0,
    )
    .unwrap();
    assert!(expired
        .expand(0)
        .unwrap_err()
        .to_string()
        .contains("deadline"));
}

#[tokio::test]
async fn overlap_replacement_and_cancelled_requests_release_the_observer() {
    let fixture = Fixture::new().await;
    let controller = ObserverController::new("test".to_string());
    let casper = fixture.casper(true);
    controller.install(Some(casper.as_ref()));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut first = Box::pin(controller.authority_snapshot(fixture.request(), deadline));
    assert!(futures::poll!(&mut first).is_pending());
    let error = controller
        .authority_snapshot(fixture.request(), deadline)
        .await
        .unwrap_err();
    assert_eq!(error.reason, "busy");
    let replacement = fixture.casper(true);
    controller.install(Some(replacement.as_ref()));
    assert_eq!(first.await.unwrap_err().reason, "instance_changed");
    let mut cancelled = Box::pin(controller.authority_snapshot(fixture.request(), deadline));
    assert!(futures::poll!(&mut cancelled).is_pending());
    drop(cancelled);
    assert!(evaluate(&fixture, &controller).await.work.complete);
}

#[tokio::test]
async fn missing_targets_have_no_fabricated_witness() {
    let fixture = Fixture::new().await;
    let controller = ObserverController::new("test".to_string());
    let casper = fixture.casper(true);
    controller.install(Some(casper.as_ref()));
    let mut request = fixture.request();
    request.body_hashes.clear();
    request.targets = vec!["ff".repeat(32)];
    let response = controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(10))
        .await
        .unwrap();
    let witness = available(&response.targets[0].oracle_witness);
    assert!(!witness.decision);
    assert!(witness.clique_weight.is_none());
    assert!(available(&response.targets[0].reference_comparison).decision_matches);
    assert!(
        matches!(&response.targets[0].persisted_fault_tolerance, Value::Unavailable { reason, .. } if reason == "target_not_held")
    );
    assert!(available(&response.floor_comparison).matches);
}

fn snapshot(
    fixture: &Fixture,
    bodies: &[BlockHash],
) -> block_storage::rust::dag::soak_snapshot::DetachedDagSnapshot {
    use block_storage::rust::dag::soak_snapshot::{capture, CaptureLimits, CaptureRequest};
    use block_storage::rust::key_value_block_store::BlockDecodeLimits;
    use shared::rust::store::soak_snapshot::ReadLimits;
    capture(&fixture.dag, &fixture.blocks, &CaptureRequest {
        limits: CaptureLimits {
            read: ReadLimits {
                max_value_bytes: 1 << 20,
                max_total_bytes: 1 << 24,
                max_records: 4096,
                max_operations: 100_000,
            },
            block_decode: BlockDecodeLimits {
                max_compressed_bytes: 1 << 20,
                max_decompressed_bytes: 1 << 20,
                max_expansion_ratio: 4096,
            },
            max_blocks: 128,
            max_validators: 64,
            max_edges: 4096,
            max_work: 2_000_000,
            lock_wait: Duration::from_millis(100),
        },
        bodies,
    })
    .unwrap()
}

fn graph_block(
    template: &BlockMessage,
    tag: u8,
    height: i64,
    validator: u8,
    parents: &[u8],
    justifications: &[(u8, u8)],
) -> BlockMessage {
    let mut block = template.clone();
    block.block_hash = Bytes::from(vec![tag; 32]);
    block.sender = Bytes::from(vec![validator; 65]);
    block.seq_num = height as i32;
    block.body.state.block_number = height;
    block.header.parents_hash_list = parents
        .iter()
        .map(|tag| Bytes::from(vec![*tag; 32]))
        .collect();
    block.justifications = justifications
        .iter()
        .map(|(validator, tag)| Justification {
            validator: Bytes::from(vec![*validator; 65]),
            latest_block_hash: Bytes::from(vec![*tag; 32]),
        })
        .collect();
    block.body.state.bonds = vec![
        Bond {
            validator: Bytes::from(vec![7; 65]),
            stake: 6,
        },
        Bond {
            validator: Bytes::from(vec![8; 65]),
            stake: 4,
        },
    ];
    block.body.merge_base = Bytes::new();
    block.body.applied_from_scope.clear();
    block
}

#[tokio::test]
async fn reference_detects_wrong_threshold_traversal_and_clique_controls() {
    use casper::rust::safety::clique_oracle::{ft_decides_exact, CliqueOracle, FtThreshold};
    use casper::rust::soak_observer::reference::Reference;
    let base = Fixture::new().await;
    let template = &base.chain[0];
    let fixture = Fixture::from_chain(vec![
        graph_block(template, 1, 0, 7, &[], &[]),
        graph_block(template, 2, 1, 7, &[1], &[(7, 1)]),
        graph_block(template, 3, 1, 9, &[1], &[(9, 1)]),
        graph_block(template, 4, 2, 7, &[2], &[(7, 2), (8, 1)]),
        graph_block(template, 5, 2, 8, &[3, 2], &[(8, 1), (7, 2)]),
    ])
    .await;
    let captured = snapshot(&fixture, &[]);
    let target = &fixture.chain[1].block_hash;
    let reference = Reference::new(&captured, meter(work_limits()), 500_000);
    let expected = reference
        .oracle(target, &captured.latest_messages, false)
        .unwrap();
    assert!(!expected.decision);
    assert_eq!(expected.agreeing_stake, Some(6));
    assert_eq!(expected.clique_weight, Some(6));
    let mut scratch = captured.scratch_view().unwrap();
    let measured = CliqueOracle::ft_witnessed_exact(
        target,
        &scratch.representation,
        &captured.latest_messages,
        FtThreshold::from_ppm(500_000),
        false,
    )
    .await
    .unwrap();
    assert_eq!(measured, expected.decision);
    let wrong_threshold = CliqueOracle::ft_witnessed_exact(
        target,
        &scratch.representation,
        &captured.latest_messages,
        FtThreshold::from_ppm(200_000),
        false,
    )
    .await
    .unwrap();
    assert_ne!(wrong_threshold, expected.decision);
    let latest_b = &fixture.chain[4].block_hash;
    assert!(scratch
        .representation
        .is_dag_ancestor(target, latest_b)
        .unwrap());
    assert!(!scratch
        .representation
        .is_in_main_chain(target, latest_b)
        .unwrap());
    scratch
        .representation
        .main_parent_map
        .insert(latest_b.clone(), target.clone());
    let wrong_traversal = CliqueOracle::ft_witnessed_exact(
        target,
        &scratch.representation,
        &captured.latest_messages,
        FtThreshold::from_ppm(500_000),
        false,
    )
    .await
    .unwrap();
    assert_ne!(wrong_traversal, expected.decision);
    assert_ne!(
        ft_decides_exact(6, 10, 10, 500_000, 1_000_000, false),
        expected.decision
    );
}

#[tokio::test]
async fn reference_preserves_signature_containment_and_missing_body_coverage() {
    use casper::rust::soak_observer::reference::Reference;
    let base = Fixture::new().await;
    let template = &base.chain[0];
    let genesis = graph_block(template, 1, 0, 7, &[], &[]);
    let mut settled = graph_block(template, 2, 1, 7, &[1], &[(7, 1)]);
    settled.body.applied_from_scope = vec![Bytes::from_static(b"settled-signature")];
    let mut missing = graph_block(template, 3, 2, 8, &[1, 2], &[(7, 2)]);
    missing.body.merge_base = genesis.block_hash.clone();
    let mut contained = graph_block(template, 4, 2, 8, &[1, 2], &[(7, 2)]);
    contained.body.merge_base = genesis.block_hash.clone();
    contained.body.applied_from_scope = settled.body.applied_from_scope.clone();
    let fixture = Fixture::from_chain(vec![
        genesis,
        settled.clone(),
        missing.clone(),
        contained.clone(),
    ])
    .await;
    let hashes: Vec<_> = fixture.chain.iter().map(|b| b.block_hash.clone()).collect();
    let captured = snapshot(&fixture, &hashes);
    let mut reference = Reference::new(&captured, meter(work_limits()), 500_000);
    let scratch = captured.scratch_view().unwrap();
    assert!(scratch
        .representation
        .is_dag_ancestor(&settled.block_hash, &missing.block_hash)
        .unwrap());
    assert!(!reference
        .contains_state(&missing.block_hash, &settled.block_hash)
        .unwrap());
    assert!(reference
        .contains_state(&contained.block_hash, &settled.block_hash)
        .unwrap());
    let no_bodies = snapshot(&fixture, &[]);
    let mut reference = Reference::new(&no_bodies, meter(work_limits()), 500_000);
    assert_eq!(
        reference
            .contains_state(&missing.block_hash, &settled.block_hash)
            .unwrap_err()
            .to_string(),
        "body_not_requested"
    );
}

#[tokio::test]
async fn reference_refuses_restore_seeds_without_provenance() {
    use casper::rust::soak_observer::reference::Reference;
    let base = Fixture::new().await;
    let restored = graph_block(&base.chain[0], 10, 10, 7, &[9], &[(7, 9)]);
    let genesis = graph_block(&base.chain[0], 1, 0, 7, &[], &[]);
    let fixture = Fixture::from_chain(vec![genesis, restored.clone()]).await;
    let captured = snapshot(&fixture, &[]);
    let mut reference = Reference::new(&captured, meter(work_limits()), 500_000);
    assert_eq!(
        reference
            .block_floor(&restored.block_hash)
            .unwrap_err()
            .to_string(),
        "restore_seed_provenance_unavailable"
    );
}

#[test]
fn metered_ordering_preserves_duplicates_and_stops_before_unbounded_sorting() {
    use shared::rust::dag::observation_work::sort_by_metered;
    for code in 0..4096u32 {
        let values: Vec<_> = (0..6)
            .map(|index| ((code >> (index * 2)) & 3) as u8)
            .collect();
        let mut expected = values.clone();
        expected.sort();
        let mut actual = values;
        sort_by_metered(&meter(work_limits()), &mut actual, Ord::cmp).unwrap();
        assert_eq!(actual, expected);
    }
    let work = meter(WorkLimits {
        operations: 2,
        ..work_limits()
    });
    assert!(sort_by_metered(&work, &mut [3, 2, 1], Ord::cmp).is_err());
    assert_eq!(work.usage().0.operations, 2);
    let work = meter(work_limits());
    assert!(work.allocate(usize::MAX, 2).is_err());
    assert_eq!(work.usage().2.as_deref(), Some("allocation_overflow"));
}
