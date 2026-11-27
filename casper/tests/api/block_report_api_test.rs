use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::api::block_api::BlockAPI;
use casper::rust::api::block_report_api::{BlockReportAPI, BlockReportError};
use casper::rust::casper::MultiParentCasper;
use casper::rust::engine::engine::Engine;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::errors::CasperError;
use casper::rust::report_store::CompressedBlockEventInfoStore;
use casper::rust::reporting_casper::{NoopReportingCasper, ReplayResult, ReportingCasper};
use casper::rust::safety_oracle::CliqueOracleImpl;
use casper::rust::util::construct_deploy;
use comm::rust::peer_node::PeerNode;
use models::casper::BlockEventInfo;
use models::rust::block_implicits::{get_random_block, get_random_block_default};
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, CasperMessage, ProcessedDeploy,
};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use tokio::sync::Notify;
use tokio::time::timeout;

use crate::engine::setup::TestFixture;
use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;

fn report_store() -> CompressedBlockEventInfoStore {
    CompressedBlockEventInfoStore::new(Arc::new(InMemoryKeyValueStore::new()))
}

#[tokio::test]
async fn block_report_requires_a_casper_instance() {
    let fixture = TestFixture::new().await;
    let engine_cell = EngineCell::init();
    let api = BlockReportAPI::new(
        Arc::new(NoopReportingCasper),
        report_store(),
        engine_cell,
        fixture.block_store.clone(),
        CliqueOracleImpl,
        false,
    );

    let result = api
        .block_report(Bytes::from_static(b"any-hash"), false)
        .await;
    assert!(matches!(
        result,
        Err(BlockReportError::CasperNotInitialized)
    ));
}

#[tokio::test]
async fn block_report_reports_a_missing_block() {
    let fixture = TestFixture::new().await;
    let TestFixture {
        engine,
        block_store,
        ..
    } = fixture;
    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;

    let api = BlockReportAPI::new(
        Arc::new(NoopReportingCasper),
        report_store(),
        engine_cell,
        block_store,
        CliqueOracleImpl,
        false,
    );

    let missing = Bytes::from_static(b"missing-report-block-hash");
    let result = api.block_report(missing.clone(), false).await;
    assert!(matches!(
        result,
        Err(BlockReportError::BlockNotFound(hash)) if hash == missing
    ));

    let prewarm = api
        .prewarm_block_report(Bytes::from_static(b"missing-report-block-hash"))
        .await;
    assert!(matches!(prewarm, Err(BlockReportError::BlockNotFound(_))));
}

#[tokio::test]
async fn a_cached_report_is_served_without_replay() {
    let fixture = TestFixture::new().await;
    let TestFixture {
        engine,
        block_store,
        genesis,
        ..
    } = fixture;
    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;

    block_store
        .put(genesis.block_hash.clone(), &genesis)
        .expect("Failed to put genesis block");

    let store = report_store();
    let cached = BlockEventInfo::default();
    store
        .put(vec![(genesis.block_hash.to_vec(), cached.clone())])
        .expect("Failed to seed report cache");

    let api = BlockReportAPI::new(
        Arc::new(NoopReportingCasper),
        store,
        engine_cell,
        block_store,
        CliqueOracleImpl,
        false,
    );

    let report = api
        .block_report(genesis.block_hash.clone(), false)
        .await
        .expect("cached report should be served");
    assert_eq!(report, cached);
}

/// Replays every block successfully, counting the replays and the replays
/// told the block is genesis.
struct CountingReportingCasper {
    replays: Arc<AtomicUsize>,
    genesis_replays: Arc<AtomicUsize>,
}

#[async_trait]
impl ReportingCasper for CountingReportingCasper {
    async fn trace(&self, block: &BlockMessage, is_genesis: bool) -> Result<ReplayResult, String> {
        self.replays.fetch_add(1, Ordering::SeqCst);
        if is_genesis {
            self.genesis_replays.fetch_add(1, Ordering::SeqCst);
        }
        Ok(ReplayResult {
            deploy_report_result: vec![],
            system_deploy_report_result: vec![],
            post_state_hash: block.body.state.post_state_hash.to_vec(),
        })
    }
}

struct CasperEngine {
    casper: Arc<NoOpsCasperEffect>,
}

#[async_trait]
impl Engine for CasperEngine {
    async fn init(&self) -> Result<(), CasperError> { Ok(()) }

    async fn handle(&self, _peer: PeerNode, _msg: CasperMessage) -> Result<(), CasperError> {
        Ok(())
    }

    fn with_casper(&self) -> Option<Arc<dyn MultiParentCasper + Send + Sync>> {
        Some(self.casper.clone())
    }
}

async fn finalized_chain(
    top: i64,
    enable_history_pruning: bool,
) -> (EngineCell, KeyValueBlockStore, Vec<BlockMessage>) {
    let fixture = TestFixture::new().await;
    let mut dag = fixture.block_dag_storage.get_representation().unwrap();
    let state = fixture.genesis.body.state.post_state_hash.clone();
    let mut chain = vec![fixture.genesis.clone()];
    for height in 1..=top {
        let block = get_random_block(
            Some(height),
            Some(height as i32),
            Some(state.clone()),
            Some(state.clone()),
            None,
            Some(1),
            None,
            Some(vec![chain.last().unwrap().block_hash.clone()]),
            Some(vec![]),
            Some(vec![]),
            Some(vec![]),
            Some(vec![]),
            Some("root".to_string()),
            None,
        );
        let fixture = dag.fixture_mut();
        fixture.dag_set.insert(block.block_hash.clone());
        fixture
            .finalized_blocks_set
            .insert(block.block_hash.clone());
        fixture
            .block_number_map
            .insert(block.block_hash.clone(), height);
        fixture
            .height_map
            .entry(height)
            .or_default()
            .insert(block.block_hash.clone());
        dag.block_metadata_index
            .write()
            .add(BlockMetadata::from_block(&block, None, None, None))
            .unwrap();
        chain.push(block);
    }
    dag.last_finalized_block_hash = chain.last().unwrap().block_hash.clone();

    let mut casper = NoOpsCasperEffect::new(
        None,
        None,
        fixture.runtime_manager.clone(),
        fixture.block_store.clone(),
        dag,
    );
    casper.genesis_hash = Some(chain[0].block_hash.clone());
    casper.last_finalized_block = chain.last().cloned();
    casper.shard_conf.enable_history_pruning = enable_history_pruning;
    casper.shard_conf.max_parent_depth = 2;
    casper.shard_conf.mergeable_channels_gc_depth_buffer = 1;
    assert_eq!(casper.shard_conf.state_retention_floor(5), 2);
    for block in &chain {
        casper.add_block_to_store(block.clone());
    }
    let engine_cell = EngineCell::init();
    engine_cell
        .set(Arc::new(CasperEngine {
            casper: Arc::new(casper),
        }))
        .await;

    (engine_cell, fixture.block_store, chain)
}

async fn api_over_a_finalized_chain(
    top: i64,
    retention: u32,
    enable_history_pruning: bool,
    store: CompressedBlockEventInfoStore,
) -> (BlockReportAPI, Vec<BlockMessage>, Arc<AtomicUsize>) {
    let (engine_cell, block_store, chain) = finalized_chain(top, enable_history_pruning).await;
    let replays = Arc::new(AtomicUsize::new(0));
    let api = BlockReportAPI::new(
        Arc::new(CountingReportingCasper {
            replays: replays.clone(),
            genesis_replays: Arc::new(AtomicUsize::new(0)),
        }),
        store,
        engine_cell,
        block_store,
        CliqueOracleImpl,
        false,
    )
    .with_report_cache_retention(retention);
    (api, chain, replays)
}

struct CappedChain {
    api: BlockReportAPI,
    genesis: BlockMessage,
    child: BlockMessage,
    replays: Arc<AtomicUsize>,
    genesis_replays: Arc<AtomicUsize>,
}

/// A report API over genesis and one child carrying a deploy that declares
/// `deploy_phlo_limit`, on a shard whose cap is `max_phlo_limit`. A
/// `parentless` child names no parents, as any block can.
async fn api_over_a_capped_chain(
    max_phlo_limit: i64,
    deploy_phlo_limit: i64,
    parentless: bool,
) -> CappedChain {
    let fixture = TestFixture::new().await;
    let mut dag = fixture.block_dag_storage.get_representation().unwrap();
    let genesis = fixture.genesis.clone();
    let state = genesis.body.state.post_state_hash.clone();
    let parents = if parentless {
        vec![]
    } else {
        vec![genesis.block_hash.clone()]
    };
    let mut child = get_random_block(
        Some(1),
        Some(1),
        Some(state.clone()),
        Some(state),
        None,
        Some(1),
        None,
        Some(parents),
        Some(vec![]),
        Some(vec![]),
        Some(vec![]),
        Some(vec![]),
        Some("root".to_string()),
        None,
    );
    child.body.deploys = vec![ProcessedDeploy::empty(
        construct_deploy::source_deploy_now_full(
            "Nil".to_string(),
            Some(deploy_phlo_limit),
            None,
            None,
            None,
            None,
        )
        .unwrap(),
    )];
    let dag_fixture = dag.fixture_mut();
    dag_fixture.dag_set.insert(child.block_hash.clone());
    dag_fixture
        .block_number_map
        .insert(child.block_hash.clone(), 1);
    dag.block_metadata_index
        .write()
        .add(BlockMetadata::from_block(&child, None, None, None))
        .unwrap();

    let mut casper = NoOpsCasperEffect::new(
        None,
        None,
        fixture.runtime_manager.clone(),
        fixture.block_store.clone(),
        dag,
    );
    casper.shard_conf.max_phlo_limit = max_phlo_limit;
    casper.genesis_hash = Some(genesis.block_hash.clone());
    casper.add_block_to_store(genesis.clone());
    casper.add_block_to_store(child.clone());
    let engine_cell = EngineCell::init();
    engine_cell
        .set(Arc::new(CasperEngine {
            casper: Arc::new(casper),
        }))
        .await;

    let replays = Arc::new(AtomicUsize::new(0));
    let genesis_replays = Arc::new(AtomicUsize::new(0));
    let api = BlockReportAPI::new(
        Arc::new(CountingReportingCasper {
            replays: replays.clone(),
            genesis_replays: genesis_replays.clone(),
        }),
        report_store(),
        engine_cell,
        fixture.block_store,
        CliqueOracleImpl,
        false,
    );
    CappedChain {
        api,
        genesis,
        child,
        replays,
        genesis_replays,
    }
}

#[tokio::test]
async fn a_block_above_the_phlo_cap_is_refused_without_replay() {
    let chain = api_over_a_capped_chain(1_000, 1_001, false).await;

    let report = chain
        .api
        .block_report(chain.child.block_hash.clone(), false)
        .await;
    assert!(matches!(
        report,
        Err(BlockReportError::ExcessivePhloLimit { cap: 1_000, .. })
    ));
    let prewarm = chain
        .api
        .prewarm_block_report(chain.child.block_hash.clone())
        .await;
    assert!(matches!(
        prewarm,
        Err(BlockReportError::ExcessivePhloLimit { .. })
    ));
    assert_eq!(chain.replays.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_block_at_the_phlo_cap_is_replayed() {
    let chain = api_over_a_capped_chain(1_000, 1_000, false).await;

    chain
        .api
        .block_report(chain.child.block_hash.clone(), false)
        .await
        .expect("a block within the cap is reported");
    assert_eq!(chain.replays.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn genesis_is_reported_although_its_deploys_exceed_the_phlo_cap() {
    let chain = api_over_a_capped_chain(1_000, 1_000, false).await;
    assert!(chain
        .genesis
        .body
        .deploys
        .iter()
        .any(|deploy| deploy.deploy.data.phlo_limit > 1_000));

    chain
        .api
        .block_report(chain.genesis.block_hash.clone(), false)
        .await
        .expect("genesis is reported");
    assert_eq!(chain.replays.load(Ordering::SeqCst), 1);
    assert_eq!(chain.genesis_replays.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_parentless_block_that_is_not_genesis_is_held_to_the_phlo_cap() {
    let chain = api_over_a_capped_chain(1_000, 1_001, true).await;
    assert!(chain.child.header.parents_hash_list.is_empty());

    let report = chain
        .api
        .block_report(chain.child.block_hash.clone(), false)
        .await;
    assert!(matches!(
        report,
        Err(BlockReportError::ExcessivePhloLimit { cap: 1_000, .. })
    ));
    assert_eq!(chain.replays.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn only_genesis_is_replayed_as_genesis() {
    let chain = api_over_a_capped_chain(1_000, 1_000, true).await;

    chain
        .api
        .block_report(chain.child.block_hash.clone(), false)
        .await
        .expect("a parentless block within the cap is reported");
    assert_eq!(chain.replays.load(Ordering::SeqCst), 1);
    assert_eq!(
        chain.genesis_replays.load(Ordering::SeqCst),
        0,
        "a parentless block is replayed with cost accounting and the FS filter"
    );

    chain
        .api
        .block_report(chain.genesis.block_hash.clone(), false)
        .await
        .expect("genesis is reported");
    assert_eq!(chain.genesis_replays.load(Ordering::SeqCst), 1);
}

fn cache(store: &CompressedBlockEventInfoStore, blocks: &[BlockMessage]) {
    store
        .put(
            blocks
                .iter()
                .map(|block| (block.block_hash.to_vec(), BlockEventInfo::default()))
                .collect(),
        )
        .unwrap();
}

fn cached(store: &CompressedBlockEventInfoStore, blocks: &[BlockMessage]) -> Vec<bool> {
    store
        .contains(
            blocks
                .iter()
                .map(|block| block.block_hash.to_vec())
                .collect(),
        )
        .unwrap()
}

#[tokio::test]
async fn a_prune_step_deletes_reports_below_the_window_and_keeps_those_inside_it() {
    let store = report_store();
    let (api, chain, _) = api_over_a_finalized_chain(200, 50, false, store.clone()).await;
    let below = &chain[140..150];
    let inside = &chain[150..=160];
    cache(&store, below);
    cache(&store, inside);

    api.prewarm_block_report(chain[170].block_hash.clone())
        .await
        .unwrap();
    assert_eq!(cached(&store, &below[..8]), vec![false; 8]);
    assert_eq!(cached(&store, &below[8..]), vec![true; 2]);

    api.prewarm_block_report(chain[171].block_hash.clone())
        .await
        .unwrap();
    assert_eq!(cached(&store, below), vec![false; below.len()]);
    assert_eq!(cached(&store, inside), vec![true; inside.len()]);
    assert_eq!(cached(&store, &chain[170..=171]), vec![true; 2]);
}

#[tokio::test]
async fn a_report_below_the_window_is_returned_but_not_cached() {
    let store = report_store();
    let (api, chain, replays) = api_over_a_finalized_chain(200, 50, false, store.clone()).await;

    api.prewarm_block_report(chain[10].block_hash.clone())
        .await
        .expect("a report below the window is still returned");
    assert_eq!(replays.load(Ordering::SeqCst), 1);
    assert_eq!(cached(&store, &chain[10..=10]), vec![false]);
}

#[tokio::test]
async fn after_a_restart_a_prune_step_crosses_already_pruned_heights() {
    let store = report_store();
    let (api, chain, _) = api_over_a_finalized_chain(6_000, 100, false, store.clone()).await;
    let near = &chain[900..=900];
    let far = &chain[5_000..=5_000];
    cache(&store, near);
    cache(&store, far);

    api.prewarm_block_report(chain[5_950].block_hash.clone())
        .await
        .unwrap();
    assert_eq!(cached(&store, near), vec![false]);
    assert_eq!(
        cached(&store, far),
        vec![true],
        "one step walks a bounded number of heights"
    );

    api.prewarm_block_report(chain[5_951].block_hash.clone())
        .await
        .unwrap();
    assert_eq!(cached(&store, far), vec![false]);
}

#[tokio::test]
async fn archive_state_queries_serve_a_block_below_the_calculated_floor() {
    let (engine, _, chain) = finalized_chain(5, false).await;
    let hash = hex::encode(&chain[1].block_hash);
    let (_, block, _) = BlockAPI::exploratory_deploy(
        &engine,
        "Nil".to_owned(),
        Some(hash.clone()),
        false,
        false,
        None,
        vec![],
    )
    .await
    .unwrap();
    assert_eq!(block.block_hash, hash);
    let (_, block) = BlockAPI::get_data_at_par(
        &engine,
        &models::rhoapi::Par::default(),
        hash.clone(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(block.block_hash, hash);
}

#[tokio::test]
async fn an_archive_replays_old_state_on_a_cache_miss_and_when_forced() {
    let (api, chain, replays) = api_over_a_finalized_chain(5, 10, false, report_store()).await;
    let hash = &chain[1].block_hash;
    let cold = api.block_report(hash.clone(), false).await.unwrap();
    assert_eq!(replays.load(Ordering::SeqCst), 1);
    assert_eq!(api.block_report(hash.clone(), false).await.unwrap(), cold);
    assert_eq!(replays.load(Ordering::SeqCst), 1);
    assert_eq!(api.block_report(hash.clone(), true).await.unwrap(), cold);
    assert_eq!(replays.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_pruning_node_rejects_replay_below_the_floor_and_allows_it_at_the_floor() {
    let (api, chain, replays) = api_over_a_finalized_chain(5, 10, true, report_store()).await;
    let hash = &chain[1].block_hash;
    assert!(matches!(
        api.block_report(hash.clone(), true).await,
        Err(BlockReportError::PrunedState {
            target_block_hash,
            target_block_height: 1,
            retention_floor: 2,
        }) if target_block_hash == *hash
    ));
    assert_eq!(replays.load(Ordering::SeqCst), 0);
    api.block_report(chain[2].block_hash.clone(), true)
        .await
        .unwrap();
    assert_eq!(replays.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_unavailable_pre_state_refuses_the_replay() {
    let fixture = TestFixture::new().await;
    let TestFixture {
        engine,
        block_store,
        ..
    } = fixture;
    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;

    let block = get_random_block_default();
    block_store
        .put(block.block_hash.clone(), &block)
        .expect("Failed to put block");

    let api = BlockReportAPI::new(
        Arc::new(NoopReportingCasper),
        report_store(),
        engine_cell,
        block_store,
        CliqueOracleImpl,
        false,
    );

    let result = api.block_report(block.block_hash.clone(), true).await;
    assert!(matches!(
        result,
        Err(BlockReportError::StateUnavailable(hash)) if hash == block.block_hash
    ));
}

struct GatedReportingCasper {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl ReportingCasper for GatedReportingCasper {
    async fn trace(
        &self,
        _block: &BlockMessage,
        _is_genesis: bool,
    ) -> Result<ReplayResult, String> {
        self.started.notify_one();
        self.release.notified().await;
        Err("replay released by the test".to_string())
    }
}

#[tokio::test]
async fn an_enrichment_read_does_not_wait_for_a_replay_in_flight() {
    let fixture = TestFixture::new().await;
    let TestFixture {
        engine,
        block_store,
        genesis,
        ..
    } = fixture;
    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;

    block_store
        .put(genesis.block_hash.clone(), &genesis)
        .expect("Failed to put genesis block");

    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let api = BlockReportAPI::new(
        Arc::new(GatedReportingCasper {
            started: started.clone(),
            release: release.clone(),
        }),
        report_store(),
        engine_cell,
        block_store,
        CliqueOracleImpl,
        false,
    );

    let replaying_api = api.clone();
    let replay_hash = genesis.block_hash.clone();
    let replay = tokio::spawn(async move { replaying_api.block_report(replay_hash, true).await });
    timeout(Duration::from_secs(30), started.notified())
        .await
        .expect("the explicit report should start replaying");

    let read = timeout(
        Duration::from_secs(5),
        api.block_report_if_idle(genesis.block_hash.clone()),
    )
    .await
    .expect("an enrichment read must not queue behind a replay in flight");
    assert!(matches!(read, Err(BlockReportError::Busy)));

    release.notify_one();
    let replayed = replay.await.expect("replay task should not panic");
    assert!(matches!(replayed, Err(BlockReportError::ReplayFailed(_))));
}
