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
use shared::rust::dag::observation_work::{
    CheckedWork, WorkKind, WorkLimits, WorkMeter, WORK_PATHS,
};
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
        Self::from_chain_marking(chain, &[]).await
    }

    async fn from_chain_marking(chain: Vec<BlockMessage>, invalid: &[BlockHash]) -> Self {
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
                } else if invalid.contains(&block.block_hash) {
                    InsertMode::Invalid
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
        conf.max_number_of_parents = 100;
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
            fork_choice: None,
            display: None,
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

async fn fork_fixture() -> Fixture {
    let base = Fixture::new().await;
    let template = &base.chain[0];
    Fixture::from_chain(vec![
        graph_block(template, 1, 0, 7, &[], &[]),
        graph_block(template, 2, 1, 7, &[1], &[(7, 1)]),
        graph_block(template, 3, 1, 9, &[1], &[(9, 1)]),
        graph_block(template, 4, 2, 7, &[2], &[(7, 2), (8, 1)]),
        graph_block(template, 5, 2, 8, &[3, 2], &[(8, 1), (7, 2)]),
    ])
    .await
}

fn tag(value: u8) -> BlockHash { Bytes::from(vec![value; 32]) }

fn validator(value: u8) -> Bytes { Bytes::from(vec![value; 65]) }

fn fork_latest_messages() -> HashMap<Bytes, BlockHash> {
    HashMap::from([
        (validator(7), tag(4)),
        (validator(8), tag(5)),
        (validator(9), tag(3)),
    ])
}

fn one_operation() -> CheckedWork {
    let mut limits = work_limits();
    limits.operations = 1;
    meter(limits)
}

fn is_limit_error(text: &str) -> bool { text.contains("observation_work:") }

#[tokio::test]
async fn metered_weight_read_matches_the_production_read() {
    use casper::rust::util::proto_util;
    let fixture = fork_fixture().await;
    let mut dag = fixture.dag.get_representation().unwrap();
    for (block, validator_tag) in [(4, 7), (5, 8), (3, 7), (1, 7)] {
        let expected = proto_util::weight_from_validator_by_dag(
            &mut dag,
            &tag(block),
            &validator(validator_tag),
        )
        .unwrap();
        let wide = meter(work_limits());
        let metered = proto_util::weight_from_validator_by_dag_metered(
            &wide,
            &mut dag,
            &tag(block),
            &validator(validator_tag),
        )
        .unwrap();
        assert_eq!(metered, expected);
        assert!(wide.usage().0.metadata >= 1);
    }
    let missing = tag(200);
    let expected = proto_util::weight_from_validator_by_dag(&mut dag, &missing, &validator(7))
        .unwrap_err()
        .to_string();
    let metered = proto_util::weight_from_validator_by_dag_metered(
        &meter(work_limits()),
        &mut dag,
        &missing,
        &validator(7),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(metered, expected);
    let error = proto_util::weight_from_validator_by_dag_metered(
        &one_operation(),
        &mut dag,
        &tag(4),
        &validator(7),
    )
    .unwrap_err()
    .to_string();
    assert!(is_limit_error(&error), "{error}");
}

#[tokio::test]
async fn metered_common_ancestor_matches_the_production_walk() {
    use casper::rust::util::dag_operations::DagOperations;
    let fixture = fork_fixture().await;
    let dag = fixture.dag.get_representation().unwrap();
    let blocks: Vec<_> = [4u8, 5, 3]
        .iter()
        .map(|block| dag.lookup_unsafe(&tag(*block)).unwrap())
        .collect();
    let floor = dag.lookup_unsafe(&tag(1)).unwrap();
    let expected = DagOperations::lowest_universal_common_ancestor_many(&blocks, &dag, &floor)
        .await
        .unwrap();
    let wide = meter(work_limits());
    let metered =
        DagOperations::lowest_universal_common_ancestor_many_metered(&wide, &blocks, &dag, &floor)
            .await
            .unwrap();
    assert_eq!(metered, expected);
    assert_eq!(expected.block_hash, tag(1));
    let (total, _, failure) = wide.usage();
    assert!(total.traversal >= 1 && total.allocation >= 1);
    assert!(failure.is_none());
    let error = DagOperations::lowest_universal_common_ancestor_many_metered(
        &one_operation(),
        &blocks,
        &dag,
        &floor,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(is_limit_error(&error), "{error}");
}

#[tokio::test]
async fn metered_fork_choice_matches_the_production_estimator() {
    use casper::rust::estimator::Estimator;
    let fixture = fork_fixture().await;
    let mut dag = fixture.dag.get_representation().unwrap();
    let floor = dag.lookup_unsafe(&tag(1)).unwrap();
    let estimator = Estimator::apply();
    for (parents, depth) in [
        (Estimator::UNLIMITED_PARENTS, None),
        (1, None),
        (2, Some(1)),
    ] {
        let expected = estimator
            .tips_with_latest_messages(&mut dag, &floor, fork_latest_messages(), parents, depth)
            .await
            .unwrap();
        let wide = meter(work_limits());
        let metered = estimator
            .tips_with_latest_messages_metered(
                &wide,
                &mut dag,
                &floor,
                fork_latest_messages(),
                parents,
                depth,
            )
            .await
            .unwrap();
        assert_eq!(metered, expected);
        assert_eq!(expected.tips[0], tag(4));
        let (total, _, failure) = wide.usage();
        assert!(total.metadata >= 3 && total.traversal >= 3 && total.allocation >= 1);
        assert!(failure.is_none());
    }
    let error = estimator
        .tips_with_latest_messages_metered(
            &one_operation(),
            &mut dag,
            &floor,
            fork_latest_messages(),
            Estimator::UNLIMITED_PARENTS,
            None,
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(is_limit_error(&error), "{error}");
}

#[tokio::test]
async fn metered_fork_choice_floor_matches_the_production_floor() {
    use casper::rust::finality::floor::{fork_choice_floor, fork_choice_floor_metered};
    use casper::rust::safety::clique_oracle::FtThreshold;
    let fixture = fork_fixture().await;
    let dag = fixture.dag.get_representation().unwrap();
    let approved = dag.lookup_unsafe(&tag(1)).unwrap();
    let latest: Vec<BlockHash> = vec![tag(4), tag(5), tag(3)];
    for ppm in [0, 200_000, 500_000, 990_000] {
        let ftt = FtThreshold::from_ppm(ppm);
        let expected = fork_choice_floor(&dag, &fixture.blocks, &latest, approved.clone(), ftt)
            .await
            .unwrap();
        let wide = meter(work_limits());
        let metered =
            fork_choice_floor_metered(&wide, &dag, &fixture.blocks, &latest, approved.clone(), ftt)
                .await
                .unwrap();
        assert_eq!(metered, expected);
        let (total, _, failure) = wide.usage();
        assert!(failure.is_none());
        if ppm > 0 {
            assert!(total.traversal >= 3);
        }
    }
    let error = fork_choice_floor_metered(
        &one_operation(),
        &dag,
        &fixture.blocks,
        &latest,
        approved,
        FtThreshold::from_ppm(500_000),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(is_limit_error(&error), "{error}");
}

fn endpoint(fixture: &Fixture) -> CaptureEndpoint {
    let mut conf = CasperShardConf::new();
    conf.fault_tolerance_threshold_ppm = 1_000_000;
    conf.fault_tolerance_threshold = -1.0;
    conf.max_parent_depth = 12;
    conf.max_number_of_parents = 100;
    conf.deploy_lifespan = 50;
    CaptureEndpoint::new(
        fixture.dag.clone(),
        fixture.blocks.clone(),
        &conf,
        &fixture.chain[0],
    )
}

#[derive(serde::Serialize)]
struct BatchB2Request<'a> {
    capture: &'a CaptureOptions,
    evaluation: &'a WorkLimits,
    targets: &'a [String],
    body_hashes: &'a [String],
    floor: &'a Option<FloorSelection>,
    original: bool,
    reference: bool,
    strict: bool,
}

#[tokio::test]
async fn a_request_without_a_fork_choice_selection_keeps_the_batch_b2_digest_and_bytes() {
    use casper::rust::soak_observer::evaluation::{parse_hash, AuthorityRequest};
    use crypto::rust::hash::sha_256::Sha256Hasher;
    let fixture = Fixture::new().await;
    let controller = ObserverController::new("incarnation".to_string());
    controller.install(Some(fixture.casper(true).as_ref()));
    let request = fixture.request();
    let response = evaluate(&fixture, &controller).await;
    let batch_b2 = BatchB2Request {
        capture: &request.capture,
        evaluation: &request.evaluation,
        targets: &request.targets,
        body_hashes: &request.body_hashes,
        floor: &request.floor,
        original: request.original,
        reference: request.reference,
        strict: request.strict,
    };
    let bodies: Vec<BlockHash> = request
        .body_hashes
        .iter()
        .map(|hash| parse_hash(hash).unwrap())
        .collect();
    let captured = snapshot(&fixture, &bodies);
    let expected = hex::encode(Sha256Hasher::hash(
        bincode::serialize(&(
            "batch-b2-authority-v1",
            captured.digest(),
            endpoint(&fixture).authority(),
            &batch_b2,
        ))
        .unwrap(),
    ));
    assert_eq!(response.authority_digest, expected);
    assert!(matches!(response.fork_choice, Value::NotRequested));
    let echoed = serde_json::to_value(&response.request).unwrap();
    assert!(echoed.get("fork_choice").is_none());
    let json = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["fork_choice"]["availability"], "not_requested");
    let decoded: AuthorityRequest =
        serde_json::from_str(&serde_json::to_string(&response.request).unwrap()).unwrap();
    assert!(decoded.fork_choice.is_none());
}

#[tokio::test]
async fn a_fork_choice_selection_binds_the_inputs_and_the_digest() {
    use casper::rust::soak_observer::evaluation::{
        fork_choice_input_digest, AuthorityRequest, ForkChoiceSelection,
    };
    let fixture = Fixture::new().await;
    let controller = ObserverController::new("incarnation".to_string());
    controller.install(Some(fixture.casper(true).as_ref()));
    let mut request = fixture.request();
    request.fork_choice = Some(ForkChoiceSelection { reference: true });
    let plain = evaluate(&fixture, &controller).await;
    let response = controller
        .authority_snapshot(request.clone(), Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert_ne!(response.authority_digest, plain.authority_digest);
    let Value::Available {
        input_digest,
        value,
    } = &response.fork_choice
    else {
        panic!(
            "the observation is not available: {:?}",
            response.fork_choice
        );
    };
    let endpoint = endpoint(&fixture);
    let inputs = endpoint.fork_choice_inputs();
    assert_eq!(&value.inputs, inputs);
    assert_eq!(value.inputs.approved_block_number, 0);
    assert_eq!(value.inputs.latest_message_depth, 1000);
    assert_eq!(value.latest_messages.captured, 1);
    let expected =
        fork_choice_input_digest(&response.authority_digest, inputs, &meter(work_limits()))
            .unwrap();
    assert_eq!(input_digest, &expected);
    assert!(
        matches!(&value.bounded, Value::Available { input_digest: d, .. } if d == input_digest)
    );
    assert!(
        matches!(&value.reference, Value::Available { input_digest: d, .. } if d == input_digest)
    );
    assert!(
        matches!(&value.comparison, Value::Available { input_digest: d, .. } if d == input_digest)
    );
    let mut changed = inputs.clone();
    changed.max_number_of_parents += 1;
    assert_ne!(
        fork_choice_input_digest(&response.authority_digest, &changed, &meter(work_limits()))
            .unwrap(),
        expected
    );
    let mut changed = inputs.clone();
    changed.approved_block_number += 1;
    assert_ne!(
        fork_choice_input_digest(&response.authority_digest, &changed, &meter(work_limits()))
            .unwrap(),
        expected
    );
    assert_ne!(
        fork_choice_input_digest("other", inputs, &meter(work_limits())).unwrap(),
        expected
    );
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("\"fork_choice\":{\"reference\":true}"));
    let unknown = json.replace("{\"reference\":true}", "{\"reference\":true,\"extra\":1}");
    assert!(serde_json::from_str::<AuthorityRequest>(&unknown).is_err());
}

#[tokio::test]
async fn bounded_fork_choice_matches_the_production_estimator_on_the_capture() {
    use casper::rust::estimator::Estimator;
    use casper::rust::soak_observer::evaluation::ForkChoiceSelection;
    use casper::rust::soak_observer::fork_choice::{EvaluationMode, LowerBoundRule};
    let fixture = fork_fixture().await;
    let controller = ObserverController::new("incarnation".to_string());
    controller.install(Some(fixture.casper(true).as_ref()));
    let before = fixture.stored();
    let mut request = fixture.request();
    request.targets = vec![hex::encode(tag(4))];
    request.fork_choice = Some(ForkChoiceSelection { reference: true });
    let response = controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(fixture.stored(), before);
    let observation = available(&response.fork_choice);
    assert_eq!(observation.latest_messages.captured, 3);
    assert_eq!(observation.latest_messages.invalid, 0);
    assert_eq!(observation.latest_messages.not_held, 0);
    assert_eq!(observation.latest_messages.not_own_testimony, 0);
    assert_eq!(observation.latest_messages.used, 3);
    let bounded = available(&observation.bounded);
    assert_eq!(bounded.mode, EvaluationMode::Bounded);
    assert_eq!(bounded.lower_bound.rule, LowerBoundRule::ApprovedBlock);
    assert_eq!(bounded.lower_bound.hash, hex::encode(tag(1)));
    assert_eq!(bounded.lower_bound.block_number, 0);
    let endpoint = endpoint(&fixture);
    let mut dag = fixture.dag.get_representation().unwrap();
    let floor = dag.lookup_unsafe(&tag(1)).unwrap();
    let expected = Estimator::apply()
        .tips_with_latest_messages(
            &mut dag,
            &floor,
            fork_latest_messages(),
            endpoint.fork_choice_inputs().max_number_of_parents,
            Some(endpoint.authority().max_parent_depth),
        )
        .await
        .unwrap();
    assert_eq!(bounded.head, hex::encode(&expected.tips[0]));
    assert_eq!(bounded.head, hex::encode(tag(4)));
    assert_eq!(bounded.common_ancestor, hex::encode(&expected.lca));
    let expected_tips: Vec<String> = expected.tips.iter().map(hex::encode).collect();
    assert_eq!(bounded.tips, expected_tips);
    assert_eq!(bounded.tip_scores.len(), bounded.tips.len());
    assert_eq!(bounded.tip_scores[0], expected.scores[&expected.tips[0]]);
    assert_eq!(bounded.score_count, expected.scores.len());
    assert_eq!(bounded.score_digest.len(), 64);
    assert!(bounded.visited_blocks >= 3);
    assert!(bounded.examined_edges >= 3);
    assert!(matches!(&observation.reference, Value::Available { .. }));
    assert!(available(&observation.comparison).head_matches);
    assert!(matches!(&response.floor_result, Value::Available { .. }));
    assert!(matches!(
        &response.targets[0].oracle_decision,
        Value::Available { .. }
    ));
}

#[tokio::test]
async fn a_zero_parent_limit_gives_no_head_and_no_substitute() {
    use casper::rust::soak_observer::evaluation::ForkChoiceSelection;
    let fixture = fork_fixture().await;
    let mut conf = CasperShardConf::new();
    conf.fault_tolerance_threshold_ppm = 1_000_000;
    conf.fault_tolerance_threshold = -1.0;
    conf.max_parent_depth = 12;
    conf.deploy_lifespan = 50;
    assert_eq!(conf.max_number_of_parents, 0);
    let casper = Arc::new(AttachedCasper {
        dag: fixture.dag.clone(),
        blocks: fixture.blocks.clone(),
        approved: fixture.chain[0].clone(),
        conf,
        binding: OnceLock::new(),
        supported: true,
    });
    let controller = ObserverController::new("incarnation".to_string());
    controller.install(Some(casper.as_ref()));
    let mut request = fixture.request();
    request.targets = vec![hex::encode(tag(4))];
    request.fork_choice = Some(ForkChoiceSelection { reference: false });
    let response = controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let observation = available(&response.fork_choice);
    assert_eq!(observation.inputs.max_number_of_parents, 0);
    assert_eq!(observation.latest_messages.used, 3);
    assert!(
        matches!(&observation.bounded, Value::Unavailable { reason, input_digest: Some(d) }
        if reason == "no_head_selected" && d == &observation.input_digest)
    );
}

fn attached(fixture: &Fixture, approved: BlockMessage) -> Arc<AttachedCasper> {
    let mut conf = CasperShardConf::new();
    conf.fault_tolerance_threshold_ppm = 1_000_000;
    conf.fault_tolerance_threshold = -1.0;
    conf.max_parent_depth = 12;
    conf.max_number_of_parents = 100;
    conf.deploy_lifespan = 50;
    Arc::new(AttachedCasper {
        dag: fixture.dag.clone(),
        blocks: fixture.blocks.clone(),
        approved,
        conf,
        binding: OnceLock::new(),
        supported: true,
    })
}

fn refusal<T: std::fmt::Debug>(value: &Value<T>) -> (&'static str, &str) {
    match value {
        Value::Unavailable { reason, .. } => ("unavailable", reason),
        Value::Failed { reason, .. } => ("failed", reason),
        other => panic!("unexpected value: {other:?}"),
    }
}

async fn fork_choice_with(
    fixture: &Fixture,
    approved: BlockMessage,
    target: BlockHash,
) -> casper::rust::soak_observer::fork_choice::ForkChoiceObservation {
    use casper::rust::soak_observer::evaluation::ForkChoiceSelection;
    let controller = ObserverController::new("incarnation".to_string());
    controller.install(Some(attached(fixture, approved).as_ref()));
    let mut request = fixture.request();
    request.targets = vec![hex::encode(target)];
    request.fork_choice = Some(ForkChoiceSelection { reference: true });
    let response = controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    available(&response.fork_choice).clone()
}

#[tokio::test]
async fn an_approved_block_outside_the_capture_refuses_both_evaluations() {
    let fixture = fork_fixture().await;
    let absent = graph_block(&fixture.chain[0], 0xEE, 0, 7, &[], &[]);
    let observation = fork_choice_with(&fixture, absent, tag(4)).await;
    assert_eq!(observation.latest_messages.used, 3);
    assert_eq!(
        refusal(&observation.bounded),
        ("unavailable", "approved_block_not_captured")
    );
    assert_eq!(
        refusal(&observation.reference),
        ("unavailable", "approved_block_not_captured")
    );
    assert_eq!(
        refusal(&observation.comparison),
        ("unavailable", "result_unavailable")
    );
}

#[tokio::test]
async fn an_approved_block_number_mismatch_refuses_both_evaluations() {
    let fixture = fork_fixture().await;
    let mut renumbered = fixture.chain[0].clone();
    renumbered.body.state.block_number = 5;
    let observation = fork_choice_with(&fixture, renumbered, tag(4)).await;
    assert_eq!(observation.inputs.approved_block_number, 5);
    assert_eq!(
        refusal(&observation.bounded),
        ("unavailable", "approved_block_mismatch")
    );
    assert_eq!(
        refusal(&observation.reference),
        ("unavailable", "approved_block_mismatch")
    );
    assert_eq!(
        refusal(&observation.comparison),
        ("unavailable", "result_unavailable")
    );
}

#[tokio::test]
async fn an_incomplete_history_refuses_the_reference_with_its_reason() {
    let base = Fixture::new().await;
    let template = &base.chain[0];
    let fixture = Fixture::from_chain(vec![
        graph_block(template, 1, 0, 7, &[], &[]),
        graph_block(template, 2, 1, 7, &[9], &[(7, 1)]),
        graph_block(template, 3, 1, 8, &[1], &[(8, 1)]),
    ])
    .await;
    let observation = fork_choice_with(&fixture, fixture.chain[0].clone(), tag(3)).await;
    assert_eq!(observation.latest_messages.used, 2);
    assert_eq!(
        refusal(&observation.reference),
        ("unavailable", "history_incomplete")
    );
    assert_eq!(
        refusal(&observation.bounded),
        ("failed", "production_error:missing_block")
    );
    assert_eq!(
        refusal(&observation.comparison),
        ("unavailable", "result_unavailable")
    );
}

#[tokio::test]
async fn reference_fork_choice_matches_the_bounded_result_on_the_capture() {
    use casper::rust::soak_observer::evaluation::ForkChoiceSelection;
    use casper::rust::soak_observer::fork_choice::{EvaluationMode, LowerBoundRule};
    let fixture = fork_fixture().await;
    let controller = ObserverController::new("incarnation".to_string());
    controller.install(Some(fixture.casper(true).as_ref()));
    let mut request = fixture.request();
    request.targets = vec![hex::encode(tag(4))];
    request.fork_choice = Some(ForkChoiceSelection { reference: true });
    let response = controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let observation = available(&response.fork_choice);
    let bounded = available(&observation.bounded);
    let reference = available(&observation.reference);
    assert_eq!(reference.mode, EvaluationMode::Reference);
    assert_eq!(reference.lower_bound.rule, LowerBoundRule::ApprovedBlock);
    assert_eq!(reference.lower_bound, bounded.lower_bound);
    assert_eq!(reference.common_ancestor, bounded.common_ancestor);
    assert_eq!(reference.head, bounded.head);
    assert_eq!(reference.head, hex::encode(tag(4)));
    assert_eq!(reference.tips, bounded.tips);
    assert_eq!(reference.tip_scores, bounded.tip_scores);
    assert_eq!(reference.score_count, bounded.score_count);
    assert_eq!(reference.score_digest, bounded.score_digest);
    assert!(reference.visited_blocks >= 5);
    assert!(reference.examined_edges >= 4);
    let comparison = available(&observation.comparison);
    assert_eq!(comparison.algorithm, "immutable-ghost-reference-v1");
    assert!(comparison.head_matches && comparison.tips_match && !comparison.bounds_differ);
}

#[tokio::test]
async fn a_selection_without_the_reference_leaves_it_not_requested() {
    use casper::rust::soak_observer::evaluation::ForkChoiceSelection;
    let fixture = fork_fixture().await;
    let controller = ObserverController::new("incarnation".to_string());
    controller.install(Some(fixture.casper(true).as_ref()));
    let mut request = fixture.request();
    request.targets = vec![hex::encode(tag(4))];
    request.fork_choice = Some(ForkChoiceSelection { reference: false });
    let response = controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let observation = available(&response.fork_choice);
    assert!(matches!(&observation.bounded, Value::Available { .. }));
    assert!(matches!(&observation.reference, Value::NotRequested));
    assert!(matches!(&observation.comparison, Value::NotRequested));
}

#[tokio::test]
async fn reference_fork_choice_refuses_a_budget_limit_with_the_limit_reason() {
    use casper::rust::soak_observer::fork_choice::ReferenceForkChoice;
    let fixture = fork_fixture().await;
    let endpoint = endpoint(&fixture);
    let captured = snapshot(&fixture, &[]);
    let wide = meter(work_limits());
    let (counts, result) = ReferenceForkChoice::new(
        &captured,
        &wide,
        endpoint.fork_choice_inputs(),
        endpoint.authority(),
    )
    .evaluate();
    let result = result.unwrap();
    assert_eq!(counts.captured, 3);
    assert_eq!(counts.used, 3);
    assert_eq!(result.head, hex::encode(tag(4)));
    assert_eq!(result.tips, vec![hex::encode(tag(4)), hex::encode(tag(5))]);
    let (total, _, failure) = wide.usage();
    assert!(failure.is_none());
    assert!(total.metadata >= 5 && total.traversal >= 4);
    let mut limits = work_limits();
    limits.operations = 2;
    let (_, limited) = ReferenceForkChoice::new(
        &captured,
        &meter(limits),
        endpoint.fork_choice_inputs(),
        endpoint.authority(),
    )
    .evaluate();
    let error = limited.unwrap_err();
    assert!(error.contains("observation_work:"), "{error}");
}

fn bonded_block(
    template: &BlockMessage,
    tag_value: u8,
    height: i64,
    validator_tag: u8,
    parents: &[u8],
    bonds: &[(u8, i64)],
) -> BlockMessage {
    let justifications: Vec<(u8, u8)> = parents.iter().map(|p| (validator_tag, *p)).collect();
    let mut block = graph_block(
        template,
        tag_value,
        height,
        validator_tag,
        parents,
        &justifications,
    );
    block.body.state.bonds = bonds
        .iter()
        .map(|(v, stake)| Bond {
            validator: Bytes::from(vec![*v; 65]),
            stake: *stake,
        })
        .collect();
    block
}

const THREE_STAKES: &[(u8, i64)] = &[(7, 6), (8, 4), (9, 3)];

async fn two_branch_fixture(bonds: &[(u8, i64)], invalid: &[u8]) -> Fixture {
    let base = Fixture::new().await;
    let template = &base.chain[0];
    let invalid: Vec<BlockHash> = invalid.iter().map(|t| tag(*t)).collect();
    Fixture::from_chain_marking(
        vec![
            bonded_block(template, 1, 0, 7, &[], bonds),
            bonded_block(template, 2, 1, 7, &[1], bonds),
            bonded_block(template, 3, 1, 8, &[1], bonds),
            bonded_block(template, 4, 2, 7, &[2], bonds),
            bonded_block(template, 5, 2, 8, &[3, 2], bonds),
            bonded_block(template, 6, 2, 9, &[3], bonds),
        ],
        &invalid,
    )
    .await
}

fn reference_with(
    fixture: &Fixture,
    captured: &block_storage::rust::dag::soak_snapshot::DetachedDagSnapshot,
    control: casper::rust::soak_observer::fork_choice::ReferenceControl,
) -> casper::rust::soak_observer::fork_choice::ForkChoiceResult {
    use casper::rust::soak_observer::fork_choice::ReferenceForkChoice;
    let endpoint = endpoint(fixture);
    let wide = meter(work_limits());
    let (_, result) = ReferenceForkChoice::new(
        captured,
        &wide,
        endpoint.fork_choice_inputs(),
        endpoint.authority(),
    )
    .with_control(control)
    .evaluate();
    result.unwrap()
}

#[tokio::test]
async fn reference_controls_that_change_a_rule_produce_a_head_mismatch() {
    use casper::rust::soak_observer::fork_choice::ReferenceControl;
    let fixture = two_branch_fixture(THREE_STAKES, &[]).await;
    let captured = snapshot(&fixture, &[]);
    let correct = reference_with(&fixture, &captured, ReferenceControl::None);
    assert_eq!(correct.head, hex::encode(tag(5)));
    for control in [
        ReferenceControl::RankTipsByOwnScore,
        ReferenceControl::CreditAllParents,
        ReferenceControl::BoundAt(tag(2)),
    ] {
        let wrong = reference_with(&fixture, &captured, control.clone());
        assert_ne!(wrong.head, correct.head, "{control:?}");
        assert_eq!(wrong.head, hex::encode(tag(4)), "{control:?}");
    }
}

#[tokio::test]
async fn reference_tie_order_control_produces_a_head_mismatch() {
    use casper::rust::soak_observer::fork_choice::ReferenceControl;
    let fixture = two_branch_fixture(&[(7, 5), (8, 5)], &[]).await;
    let captured = snapshot(&fixture, &[]);
    let correct = reference_with(&fixture, &captured, ReferenceControl::None);
    assert_eq!(correct.head, hex::encode(tag(4)));
    let reversed = reference_with(&fixture, &captured, ReferenceControl::ReverseTieOrder);
    assert_eq!(reversed.head, hex::encode(tag(5)));
}

#[tokio::test]
async fn reference_invalid_filter_control_produces_a_head_mismatch() {
    use casper::rust::soak_observer::fork_choice::ReferenceControl;
    let fixture = two_branch_fixture(THREE_STAKES, &[5]).await;
    let captured = snapshot(&fixture, &[]);
    let correct = reference_with(&fixture, &captured, ReferenceControl::None);
    assert_eq!(correct.head, hex::encode(tag(4)));
    let unfiltered = reference_with(&fixture, &captured, ReferenceControl::KeepInvalidMessages);
    assert_eq!(unfiltered.head, hex::encode(tag(5)));
}

#[tokio::test]
async fn bounded_and_reference_agree_on_the_two_branch_fixture_and_on_the_counts() {
    use casper::rust::soak_observer::evaluation::ForkChoiceSelection;
    let fixture = two_branch_fixture(THREE_STAKES, &[5]).await;
    let controller = ObserverController::new("incarnation".to_string());
    controller.install(Some(fixture.casper(true).as_ref()));
    let mut request = fixture.request();
    request.targets = vec![hex::encode(tag(4))];
    request.fork_choice = Some(ForkChoiceSelection { reference: true });
    let response = controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let observation = available(&response.fork_choice);
    assert_eq!(observation.latest_messages.captured, 3);
    assert_eq!(observation.latest_messages.invalid, 1);
    assert_eq!(observation.latest_messages.used, 2);
    let comparison = available(&observation.comparison);
    assert!(comparison.head_matches);
    assert!(comparison.tips_match);
    assert!(!comparison.bounds_differ);
    assert_eq!(available(&observation.bounded).head, hex::encode(tag(4)));
    assert!(
        matches!(&response.work.fork_choice_bounded, Value::Available { value, .. } if value.operations > 0)
    );
    assert!(
        matches!(&response.work.fork_choice_reference, Value::Available { value, .. } if value.operations > 0)
    );
}

#[tokio::test]
async fn work_report_without_a_selection_has_no_fork_choice_work() {
    let fixture = Fixture::new().await;
    let controller = ObserverController::new("incarnation".to_string());
    controller.install(Some(fixture.casper(true).as_ref()));
    let response = evaluate(&fixture, &controller).await;
    assert!(matches!(
        response.work.fork_choice_bounded,
        Value::NotRequested
    ));
    assert!(matches!(
        response.work.fork_choice_reference,
        Value::NotRequested
    ));
    let json = serde_json::to_value(&response.work).unwrap();
    assert_eq!(json["fork_choice_bounded"]["availability"], "not_requested");
}

#[tokio::test]
async fn comparison_refuses_different_captures_and_unavailable_results() {
    use casper::rust::soak_observer::fork_choice::{compare, ReferenceControl};
    let fixture = fork_fixture().await;
    let first = snapshot(&fixture, &[]);
    let second = snapshot(&fixture, &[tag(2)]);
    assert_ne!(first.digest(), second.digest());
    let left = reference_with(&fixture, &first, ReferenceControl::None);
    let right = reference_with(&fixture, &second, ReferenceControl::None);
    assert_eq!(left.head, right.head);
    let comparison = compare(
        &Value::Available {
            input_digest: first.digest_hex(),
            value: left.clone(),
        },
        &Value::Available {
            input_digest: second.digest_hex(),
            value: right.clone(),
        },
    );
    assert!(
        matches!(comparison, Value::Unavailable { reason, .. } if reason == "input_digest_mismatch")
    );
    let comparison = compare(
        &Value::Available {
            input_digest: first.digest_hex(),
            value: left.clone(),
        },
        &Value::Unavailable {
            input_digest: Some(first.digest_hex()),
            reason: "history_incomplete".to_string(),
        },
    );
    assert!(
        matches!(comparison, Value::Unavailable { reason, .. } if reason == "result_unavailable")
    );
    let comparison = compare(
        &Value::Available {
            input_digest: first.digest_hex(),
            value: left.clone(),
        },
        &Value::Available {
            input_digest: first.digest_hex(),
            value: right,
        },
    );
    let value = available(&comparison);
    assert!(value.head_matches && value.tips_match && !value.bounds_differ);
}

#[tokio::test]
async fn a_lower_bound_in_the_head_field_fails_the_schema_check() {
    use casper::rust::soak_observer::fork_choice::{validate_result, ReferenceControl};
    let fixture = fork_fixture().await;
    let captured = snapshot(&fixture, &[]);
    let result = reference_with(&fixture, &captured, ReferenceControl::None);
    validate_result(&result).unwrap();
    let mut substituted = result.clone();
    substituted.head = substituted.lower_bound.hash.clone();
    substituted.tips[0] = substituted.lower_bound.hash.clone();
    assert_eq!(
        validate_result(&substituted).unwrap_err(),
        "schema:head_is_lower_bound"
    );
    let mut misordered = result.clone();
    misordered.tips.swap(0, 1);
    assert_eq!(
        validate_result(&misordered).unwrap_err(),
        "schema:head_not_first_tip"
    );
    let mut short = result.clone();
    short.tip_scores.pop();
    assert_eq!(validate_result(&short).unwrap_err(), "schema:score_count");
    let mut empty = result;
    empty.tips.clear();
    assert_eq!(validate_result(&empty).unwrap_err(), "schema:tip_count");
}

#[tokio::test]
async fn fork_choice_scratch_views_do_not_share_cached_floor_rows() {
    use casper::rust::soak_observer::evaluation::ForkChoiceSelection;
    let fixture = fork_fixture().await;
    let controller = ObserverController::new("incarnation".to_string());
    controller.install(Some(fixture.casper(true).as_ref()));
    let mut request = fixture.request();
    request.targets = vec![hex::encode(tag(4))];
    request.fork_choice = Some(ForkChoiceSelection { reference: true });
    let response = controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert!(matches!(
        available(&response.fork_choice).bounded,
        Value::Available { .. }
    ));
    let captured = snapshot(&fixture, &[]);
    let fresh = captured.scratch_view().unwrap();
    for block in [4u8, 5, 3, 2, 1] {
        assert!(fresh
            .representation
            .get_cached_floor(&tag(block))
            .unwrap()
            .is_none());
    }
    assert!(fixture
        .dag
        .get_representation()
        .unwrap()
        .get_cached_floor(&tag(4))
        .unwrap()
        .is_none());
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % bound
    }
}

async fn random_fixture(seed: u64) -> Fixture {
    let mut rng = Lcg(seed);
    let base = Fixture::new().await;
    let template = &base.chain[0];
    let bonds: Vec<(u8, i64)> = [7u8, 8, 9]
        .iter()
        .map(|v| (*v, 1 + rng.next(9) as i64))
        .collect();
    let mut chain = vec![bonded_block(template, 1, 0, 7, &[], &bonds)];
    let mut previous_level = vec![1u8];
    let mut next_tag = 2u8;
    let levels = 1 + rng.next(3);
    for height in 1..=levels {
        let width = 1 + rng.next(2) as usize;
        let mut level = Vec::new();
        for _ in 0..width {
            let validator = 7 + rng.next(3) as u8;
            let main = previous_level[rng.next(previous_level.len() as u64) as usize];
            let mut parents = vec![main];
            if previous_level.len() > 1 && rng.next(2) == 1 {
                let other = previous_level[rng.next(previous_level.len() as u64) as usize];
                if other != main {
                    parents.push(other);
                }
            }
            chain.push(bonded_block(
                template,
                next_tag,
                height as i64,
                validator,
                &parents,
                &bonds,
            ));
            level.push(next_tag);
            next_tag += 1;
        }
        previous_level = level;
    }
    Fixture::from_chain(chain).await
}

#[tokio::test]
async fn random_dags_give_equal_heads_deterministic_results_and_heads_in_tips() {
    use casper::rust::estimator::Estimator;
    use casper::rust::soak_observer::evaluation::ForkChoiceSelection;
    for seed in 1..=12u64 {
        let fixture = random_fixture(seed).await;
        let controller = ObserverController::new("incarnation".to_string());
        controller.install(Some(fixture.casper(true).as_ref()));
        let mut request = fixture.request();
        request.targets = vec![];
        request.floor = None;
        request.fork_choice = Some(ForkChoiceSelection { reference: true });
        let first = controller
            .authority_snapshot(request.clone(), Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        let second = controller
            .authority_snapshot(request, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        let observation = available(&first.fork_choice);
        let bounded = available(&observation.bounded);
        let reference = available(&observation.reference);
        let comparison = available(&observation.comparison);
        assert!(bounded.tips.contains(&bounded.head), "seed {seed}");
        assert!(reference.tips.contains(&reference.head), "seed {seed}");
        assert!(!comparison.bounds_differ, "seed {seed}");
        assert!(
            comparison.head_matches,
            "seed {seed}: {bounded:?} vs {reference:?}"
        );
        assert!(comparison.tips_match, "seed {seed}");
        assert!(
            reference.score_count <= bounded.score_count,
            "seed {seed}: reference {} scores, bounded {} scores",
            reference.score_count,
            bounded.score_count
        );
        let endpoint = endpoint(&fixture);
        let mut dag = fixture.dag.get_representation().unwrap();
        let floor = dag.lookup_unsafe(&tag(1)).unwrap();
        let latest: HashMap<Bytes, BlockHash> = dag.latest_message_hashes().into_iter().collect();
        let expected = Estimator::apply()
            .tips_with_latest_messages(
                &mut dag,
                &floor,
                latest,
                endpoint.fork_choice_inputs().max_number_of_parents,
                Some(endpoint.authority().max_parent_depth),
            )
            .await
            .unwrap();
        assert_eq!(bounded.head, hex::encode(&expected.tips[0]), "seed {seed}");
        assert_eq!(
            serde_json::to_string(&first.fork_choice).unwrap(),
            serde_json::to_string(&second.fork_choice).unwrap(),
            "seed {seed}"
        );
        assert_eq!(
            serde_json::to_string(&first.work.fork_choice_bounded).unwrap(),
            serde_json::to_string(&second.work.fork_choice_bounded).unwrap(),
            "seed {seed}"
        );
        assert_eq!(
            serde_json::to_string(&first.work.fork_choice_reference).unwrap(),
            serde_json::to_string(&second.work.fork_choice_reference).unwrap(),
            "seed {seed}"
        );
    }
}

#[test]
fn work_budget_has_six_paths_and_the_aggregate_is_their_sum() {
    assert_eq!(WORK_PATHS, 6);
    let meter = meter(work_limits());
    let kinds = [
        WorkKind::Metadata,
        WorkKind::Traversal,
        WorkKind::Oracle,
        WorkKind::Signature,
        WorkKind::Traversal,
        WorkKind::Clique,
    ];
    for (path, kind) in kinds.iter().enumerate() {
        let scoped = meter.for_path(path).unwrap();
        for _ in 0..=path {
            scoped.step(*kind).unwrap();
        }
    }
    assert!(meter.for_path(WORK_PATHS).is_err());
    let (total, paths, failure) = meter.usage();
    assert!(failure.is_none());
    assert_eq!(paths.len(), WORK_PATHS);
    assert_eq!(paths[4].traversal, 5);
    assert_eq!(paths[5].clique, 6);
    assert_eq!(
        total.operations,
        paths.iter().map(|usage| usage.operations).sum::<u64>()
    );
    assert_eq!(total.traversal, paths[1].traversal + paths[4].traversal);
    assert_eq!(total.operations, 21);
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

async fn display_response(fixture: &Fixture, mut request: AuthorityRequest) -> AuthorityResponse {
    use casper::rust::soak_observer::evaluation::DisplaySelection;
    request.display = Some(DisplaySelection {
        max_equivocation_records: 16,
    });
    let controller = ObserverController::new("display-fixture".into());
    controller.install(Some(fixture.casper(true).as_ref()));
    controller
        .authority_snapshot(request, Instant::now() + Duration::from_secs(10))
        .await
        .unwrap()
}

fn add_display_record(fixture: &Fixture, validator: u8, sequence: i32) {
    use models::rust::equivocation_record::EquivocationRecord;
    fixture
        .dag
        .access_equivocations_tracker(|tracker| {
            tracker.add(EquivocationRecord::new(
                vec![validator; 65].into(),
                sequence,
                std::collections::BTreeSet::from([vec![3; 32].into()]),
            ))
        })
        .unwrap();
}

#[test]
fn display_admission_rejects_null_duplicates_and_invalid_limits() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fixture = runtime.block_on(Fixture::new());
    let plain = serde_json::to_value(fixture.request()).unwrap();
    assert!(plain.get("display").is_none());
    for value in [
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!({"max_equivocation_records": 1.5}),
        serde_json::json!({"max_equivocation_records": 1, "extra": true}),
    ] {
        let mut request = plain.clone();
        request["display"] = value;
        assert!(serde_json::from_value::<AuthorityRequest>(request).is_err());
    }
    for limit in [0, 4097] {
        let mut request = plain.clone();
        request["display"] = serde_json::json!({"max_equivocation_records": limit});
        assert!(serde_json::from_value::<AuthorityRequest>(request)
            .unwrap()
            .validate()
            .is_err());
    }
    let mut request = plain.clone();
    request["display"] = serde_json::json!({"max_equivocation_records": 4096});
    assert!(serde_json::from_value::<AuthorityRequest>(request)
        .unwrap()
        .validate()
        .is_ok());
    let serialized = serde_json::to_string(&plain).unwrap();
    let duplicated = serialized.replacen('{', "{\"display\":{\"max_equivocation_records\":1},\"display\":{\"max_equivocation_records\":2},", 1);
    assert!(serde_json::from_str::<AuthorityRequest>(&duplicated).is_err());
}

#[tokio::test]
async fn display_bits_use_shared_arithmetic_and_record_multiplicity_without_live_calls() {
    let fixture = Fixture::new().await;
    add_display_record(&fixture, 7, 0);
    add_display_record(&fixture, 7, 1);
    add_display_record(&fixture, 9, 0);
    let before = fixture.stored();
    let response = display_response(&fixture, fixture.request()).await;
    let target = &response.targets[0];
    let original = f32::from_bits(*available(&target.original_fault_tolerance));
    assert_eq!(
        *available(&target.display_projection),
        (original - 2.0).to_bits()
    );
    let inputs = available(target.display_inputs.as_ref().unwrap());
    assert_eq!(inputs.matched_records, 2);
    assert_eq!(inputs.distinct_equivocators, 1);
    assert_eq!(inputs.total_weight, "100");
    assert_eq!(inputs.equivocating_weight, "200");
    assert_eq!(inputs.base_source, "original_oracle");
    assert_eq!(
        response.display_scope,
        Some("batch-e-detached-display-projection")
    );
    assert_eq!(
        available(response.equivocation_capture.as_ref().unwrap()).row_count,
        3
    );
    assert!(response.work.measured.oracle > 0);
    assert_eq!(before, fixture.stored());
}

#[tokio::test]
async fn display_without_original_refuses_nonfinalized_targets_and_missing_targets() {
    let fixture = Fixture::new().await;
    let mut request = fixture.request();
    request.original = false;
    request.reference = false;
    request.targets.push(hex::encode(tag(99)));
    let response = display_response(&fixture, request).await;
    assert!(
        matches!(&response.targets[0].display_projection, Value::Unavailable { reason, .. } if reason == "original_not_requested")
    );
    assert!(
        matches!(&response.targets[1].display_projection, Value::Unavailable { reason, .. } if reason == "target_not_held")
    );
}

#[tokio::test]
async fn display_finalized_set_and_metadata_flag_remain_separate() {
    use models::rust::block_hash::BlockHashSerde;
    use models::rust::block_metadata::BlockMetadata;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
    let fixture = Fixture::new().await;
    let hash = fixture.chain[1].block_hash.clone();
    fixture
        .dag
        .record_directly_finalized(hash.clone(), 0.75, |_| async { Ok(()) })
        .await
        .unwrap();
    let mut metadata = fixture
        .dag
        .get_representation()
        .unwrap()
        .lookup(&hash)
        .unwrap()
        .unwrap();
    metadata.finalized = false;
    let position = STORES
        .iter()
        .position(|name| *name == "block-metadata")
        .unwrap();
    let store: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> =
        KeyValueTypedStoreImpl::new(fixture.handles[position].clone());
    store.put_one(BlockHashSerde(hash), metadata).unwrap();
    let mut request = fixture.request();
    request.original = false;
    request.reference = false;
    let response = display_response(&fixture, request).await;
    let target = &response.targets[0];
    assert_eq!(*available(&target.display_projection), 0.75f32.to_bits());
    let inputs = available(target.display_inputs.as_ref().unwrap());
    assert!(inputs.finalized_set_member);
    assert!(!inputs.metadata_finalized);
    assert_eq!(inputs.base_source, "persisted_metadata");
    assert!(
        matches!(&target.persisted_fault_tolerance, Value::Unavailable { reason, .. } if reason == "not_finalized")
    );
}

#[tokio::test]
async fn display_digest_changes_with_tracker_inputs_without_store_writes() {
    let fixture = Fixture::new().await;
    let first = display_response(&fixture, fixture.request()).await;
    add_display_record(&fixture, 7, 4);
    let before = fixture.stored();
    let second = display_response(&fixture, fixture.request()).await;
    assert_ne!(first.authority_digest, second.authority_digest);
    assert_ne!(
        available(first.equivocation_capture.as_ref().unwrap()).digest,
        available(second.equivocation_capture.as_ref().unwrap()).digest
    );
    assert_eq!(before, fixture.stored());
}

fn display_capture(
    fixture: &Fixture,
) -> (
    block_storage::rust::dag::soak_snapshot::DetachedDagSnapshot,
    block_storage::rust::dag::soak_equivocations::EquivocationSnapshot,
) {
    use block_storage::rust::dag::soak_snapshot::{
        capture_with_equivocations, CaptureLimits, CaptureRequest,
    };
    use block_storage::rust::key_value_block_store::BlockDecodeLimits;
    use shared::rust::store::soak_snapshot::ReadLimits;
    capture_with_equivocations(
        &fixture.dag,
        &fixture.blocks,
        &CaptureRequest {
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
            bodies: &[],
        },
        16,
    )
    .unwrap()
}

#[tokio::test]
async fn request_without_display_preserves_legacy_fields_and_ignores_tracker_rows() {
    let fixture = Fixture::new().await;
    let position = STORES
        .iter()
        .position(|name| *name == "equivocation-tracker")
        .unwrap();
    fixture.handles[position]
        .put(vec![(vec![1], vec![2])])
        .unwrap();
    let before = fixture.stored();
    let controller = ObserverController::new("legacy-display-fixture".into());
    controller.install(Some(fixture.casper(true).as_ref()));
    let response = evaluate(&fixture, &controller).await;
    let json = serde_json::to_value(&response).unwrap();
    assert!(json.get("display_scope").is_none());
    assert!(json.get("equivocation_capture").is_none());
    assert!(json["targets"][0].get("display_inputs").is_none());
    assert_eq!(
        refusal(&response.targets[0].display_projection),
        ("unavailable", "equivocation_snapshot_unavailable")
    );
    assert_eq!(before, fixture.stored());
}

fn update_display_metadata(
    fixture: &Fixture,
    update: impl FnOnce(&mut models::rust::block_metadata::BlockMetadata),
) {
    use models::rust::block_hash::BlockHashSerde;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
    let hash = fixture.chain[1].block_hash.clone();
    let mut metadata = fixture
        .dag
        .get_representation()
        .unwrap()
        .lookup(&hash)
        .unwrap()
        .unwrap();
    update(&mut metadata);
    let position = STORES
        .iter()
        .position(|name| *name == "block-metadata")
        .unwrap();
    let store = KeyValueTypedStoreImpl::new(fixture.handles[position].clone());
    store.put_one(BlockHashSerde(hash), metadata).unwrap();
}

#[tokio::test]
async fn display_checked_sums_refuse_total_and_record_multiplicity_overflow() {
    use casper::rust::soak_observer::display::calculate;
    let fixture = Fixture::new().await;
    add_display_record(&fixture, 7, 0);
    add_display_record(&fixture, 7, 1);
    for weights in [
        BTreeMap::from([(validator(7), -1), (validator(8), 1)]),
        BTreeMap::from([(validator(7), -1)]),
    ] {
        update_display_metadata(&fixture, |m| m.weight_map = weights);
        let before = fixture.stored();
        let (snapshot, tracker) = display_capture(&fixture);
        assert_eq!(
            calculate(
                &snapshot,
                &tracker,
                &fixture.chain[1].block_hash,
                Some(("original_oracle", 1f32.to_bits())),
                &meter(work_limits())
            )
            .unwrap_err(),
            "initial_fault_weight_overflow"
        );
        assert_eq!(before, fixture.stored());
    }
}

#[tokio::test]
async fn display_zero_weights_and_float_boundaries_keep_shared_production_bits() {
    use casper::rust::soak_observer::display::calculate;
    let fixture = Fixture::new().await;
    add_display_record(&fixture, 7, 0);
    add_display_record(&fixture, 7, 1);
    add_display_record(&fixture, 9, 0);
    for (a, b, fault_bits) in [
        (0, 0, 0),
        (1, 2, 0x3f2a_aaab),
        (16_777_217, 16_777_219, 0x3f7f_fffe),
        (i64::MAX / 2, i64::MAX / 2, 0x3f80_0000),
    ] {
        update_display_metadata(&fixture, |m| {
            m.weight_map = BTreeMap::from([(validator(7), a), (validator(8), b)])
        });
        let before = fixture.stored();
        let (snapshot, tracker) = display_capture(&fixture);
        for bits in [
            0,
            0x8000_0000,
            1,
            f32::MIN.to_bits(),
            f32::MAX.to_bits(),
            0.75f32.to_bits(),
        ] {
            let (actual, inputs) = calculate(
                &snapshot,
                &tracker,
                &fixture.chain[1].block_hash,
                Some(("original_oracle", bits)),
                &meter(work_limits()),
            )
            .unwrap();
            assert_eq!(
                actual,
                (f32::from_bits(bits) - f32::from_bits(fault_bits)).to_bits()
            );
            assert_eq!(inputs.initial_fault_bits, fault_bits);
            assert_eq!(inputs.equivocating_weight, (2 * a as u64).to_string());
            assert_eq!(inputs.total_weight, (a as u64 + b as u64).to_string());
            assert_eq!(inputs.matched_records, 2);
            assert_eq!(inputs.distinct_equivocators, 1);
        }
        assert_eq!(before, fixture.stored());
    }
}

#[tokio::test]
async fn display_charges_work_allocation_deadline_and_cancellation_before_results() {
    use casper::rust::soak_observer::display::calculate;
    let fixture = Fixture::new().await;
    add_display_record(&fixture, 7, 0);
    let before = fixture.stored();
    let (snapshot, tracker) = display_capture(&fixture);
    let hash = &fixture.chain[1].block_hash;
    let base = Some(("original_oracle", 0.75f32.to_bits()));
    for operations in [1, 2, 3, 10, 100] {
        let work = meter(WorkLimits {
            operations,
            ..work_limits()
        });
        assert!(is_limit_error(
            &calculate(&snapshot, &tracker, hash, base, &work).unwrap_err()
        ));
        assert!(work.usage().0.operations <= operations);
    }
    let work = meter(WorkLimits {
        allocated_bytes: 1,
        ..work_limits()
    });
    assert!(is_limit_error(
        &calculate(&snapshot, &tracker, hash, base, &work).unwrap_err()
    ));
    for (deadline, cancelled) in [
        (Instant::now() - Duration::from_secs(1), false),
        (Instant::now() + Duration::from_secs(10), true),
    ] {
        let work = CheckedWork::new(
            work_limits(),
            deadline,
            Arc::new(AtomicBool::new(cancelled)),
            4096,
            8192,
        )
        .unwrap();
        assert!(is_limit_error(
            &calculate(&snapshot, &tracker, hash, base, &work).unwrap_err()
        ));
    }
    assert_eq!(before, fixture.stored());
}

#[tokio::test]
async fn display_allocations_cover_hash_table_capacity_and_input_keys() {
    use casper::rust::soak_observer::display::calculate;
    let fixture = Fixture::new().await;
    let (snapshot, tracker) = display_capture(&fixture);
    let work = meter(work_limits());
    calculate(
        &snapshot,
        &tracker,
        &fixture.chain[1].block_hash,
        Some(("original_oracle", 0.75f32.to_bits())),
        &work,
    )
    .unwrap();
    let n = snapshot.blocks[&fixture.chain[1].block_hash]
        .metadata
        .weight_map
        .len();
    assert!(
        work.usage().0.allocated_bytes
            >= (n * 4 * std::mem::size_of::<(Validator, u64)>() + n * 65 + 256) as u64
    );
}

#[tokio::test]
async fn display_metadata_flag_alone_uses_persisted_bits_without_an_original() {
    use casper::rust::soak_observer::display::calculate;
    let fixture = Fixture::new().await;
    add_display_record(&fixture, 7, 0);
    update_display_metadata(&fixture, |metadata| {
        metadata.finalized = true;
        metadata.fault_tolerance_value = 0.75;
    });
    let before = fixture.stored();
    let (snapshot, tracker) = display_capture(&fixture);
    let hash = &fixture.chain[1].block_hash;
    assert!(!snapshot.finalized_block_set.contains(hash));
    let (bits, inputs) = calculate(&snapshot, &tracker, hash, None, &meter(work_limits())).unwrap();
    assert_eq!(bits, (-0.25f32).to_bits());
    assert_eq!(inputs.base_source, "persisted_metadata");
    assert!(!inputs.finalized_set_member);
    assert!(inputs.metadata_finalized);
    assert_eq!(before, fixture.stored());
}

#[tokio::test]
async fn display_typed_missing_history_uses_minimum_without_a_fabricated_original() {
    let base = Fixture::new().await;
    let fixture = Fixture::from_chain(vec![
        graph_block(&base.chain[0], 1, 0, 7, &[], &[]),
        graph_block(&base.chain[0], 2, 1, 7, &[9], &[(7, 1)]),
        graph_block(&base.chain[0], 3, 1, 8, &[1], &[(8, 1)]),
    ])
    .await;
    let before = fixture.stored();
    let response = display_response(&fixture, fixture.request()).await;
    let target = &response.targets[0];
    assert!(matches!(
        &target.original_fault_tolerance,
        Value::Unavailable { .. }
    ));
    let inputs = available(target.display_inputs.as_ref().unwrap());
    assert_eq!(inputs.base_source, "missing_history_minimum");
    assert_eq!(
        inputs.base_bits,
        casper::rust::safety_oracle::MIN_FAULT_TOLERANCE.to_bits()
    );
    assert_eq!(*available(&target.display_projection), inputs.base_bits);
    assert_eq!(before, fixture.stored());
}

#[tokio::test]
async fn display_refuses_tracker_inputs_from_a_different_capture_interval() {
    use casper::rust::soak_observer::display::calculate;
    let fixture = Fixture::new().await;
    let (snapshot, _) = display_capture(&fixture);
    add_display_record(&fixture, 7, 0);
    let (_, tracker) = display_capture(&fixture);
    let before = fixture.stored();
    assert_eq!(
        snapshot.insertion_generation,
        tracker.insertion_generation()
    );
    assert_ne!(snapshot.transactions, tracker.transactions());
    assert_eq!(
        calculate(
            &snapshot,
            &tracker,
            &fixture.chain[1].block_hash,
            Some(("original_oracle", 0.75f32.to_bits())),
            &meter(work_limits())
        )
        .unwrap_err(),
        "display_capture_mismatch"
    );
    assert_eq!(before, fixture.stored());
}

#[tokio::test]
async fn display_response_binds_separate_values_and_contains_no_tracker_rows() {
    let fixture = Fixture::new().await;
    add_display_record(&fixture, 7, 0);
    let before = fixture.stored();
    let response = display_response(&fixture, fixture.request()).await;
    let json = serde_json::to_value(&response).unwrap();
    let target = &json["targets"][0];
    assert_eq!(
        target["display_projection"]["input_digest"],
        target["display_inputs"]["input_digest"]
    );
    assert_eq!(
        target["display_projection"]["input_digest"],
        target["original_fault_tolerance"]["input_digest"]
    );
    assert_ne!(
        target["display_projection"]["input_digest"],
        target["persisted_fault_tolerance"]["input_digest"]
    );
    assert_eq!(
        target["display_inputs"]["value"]["equivocation_digest"],
        json["equivocation_capture"]["value"]["digest"]
    );
    let capture = json["equivocation_capture"]["value"].as_object().unwrap();
    assert!(capture.contains_key("row_count"));
    assert!(capture.contains_key("byte_count"));
    assert!(!capture.contains_key("rows"));
    assert!(!capture.contains_key("validators"));
    assert!(serde_json::to_vec(&response).unwrap().len() < 65_536);
    assert_eq!(before, fixture.stored());
}
