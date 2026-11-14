use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use casper::rust::api::block_report_api::{BlockReportAPI, BlockReportError};
use casper::rust::casper::MultiParentCasper;
use casper::rust::engine::engine::Engine;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::errors::CasperError;
use casper::rust::report_store::CompressedBlockEventInfoStore;
use casper::rust::reporting_casper::{NoopReportingCasper, ReplayResult, ReportingCasper};
use casper::rust::safety_oracle::CliqueOracleImpl;
use comm::rust::peer_node::PeerNode;
use models::casper::BlockEventInfo;
use models::rust::block_implicits::{get_random_block, get_random_block_default};
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{BlockMessage, CasperMessage};
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

/// Replays every block successfully, counting the replays.
struct CountingReportingCasper {
    replays: Arc<AtomicUsize>,
}

#[async_trait]
impl ReportingCasper for CountingReportingCasper {
    async fn trace(&self, block: &BlockMessage) -> Result<ReplayResult, String> {
        self.replays.fetch_add(1, Ordering::SeqCst);
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

/// A report API with a cache window of `retention` heights over a finalized
/// chain of one block per height up to `top`, the last finalized block at
/// `top`. Returns the API, the chain indexed by height, and the replay count.
async fn api_over_a_finalized_chain(
    top: i64,
    retention: u32,
    store: CompressedBlockEventInfoStore,
) -> (BlockReportAPI, Vec<BlockMessage>, Arc<AtomicUsize>) {
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
    for block in &chain {
        casper.add_block_to_store(block.clone());
    }
    let engine_cell = EngineCell::init();
    engine_cell
        .set(Arc::new(CasperEngine {
            casper: Arc::new(casper),
        }))
        .await;

    let replays = Arc::new(AtomicUsize::new(0));
    let api = BlockReportAPI::new(
        Arc::new(CountingReportingCasper {
            replays: replays.clone(),
        }),
        store,
        engine_cell,
        fixture.block_store,
        CliqueOracleImpl,
        false,
    )
    .with_report_cache_retention(retention);
    (api, chain, replays)
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
    let (api, chain, _) = api_over_a_finalized_chain(200, 50, store.clone()).await;
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
    let (api, chain, replays) = api_over_a_finalized_chain(200, 50, store.clone()).await;

    api.prewarm_block_report(chain[10].block_hash.clone())
        .await
        .expect("a report below the window is still returned");
    assert_eq!(replays.load(Ordering::SeqCst), 1);
    assert_eq!(cached(&store, &chain[10..=10]), vec![false]);
}

#[tokio::test]
async fn after_a_restart_a_prune_step_crosses_already_pruned_heights() {
    let store = report_store();
    let (api, chain, _) = api_over_a_finalized_chain(6_000, 100, store.clone()).await;
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
    async fn trace(&self, _block: &BlockMessage) -> Result<ReplayResult, String> {
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
