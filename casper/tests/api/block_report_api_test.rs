use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
// Added by DR-134 (DR-70): the offered report tests.
use casper::rust::api::block_api::BlockAPI;
use casper::rust::api::block_report_api::{BlockReportAPI, BlockReportError};
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::report_store::CompressedBlockEventInfoStore;
// Changed by DR-134 (DR-70): the offered report tests.
// use casper::rust::reporting_casper::{NoopReportingCasper, ReplayResult, ReportingCasper};
use casper::rust::reporting_casper::{
    self, NoopReportingCasper, OfferedDeployReport, ReplayResult, ReportingCasper,
};
use casper::rust::safety_oracle::CliqueOracleImpl;
// Added by DR-134 (DR-70): the offered report tests.
use casper::rust::util::rholang::costacc::offered_only::BODY_ONLY_DEPLOYS_UNSUPPORTED;
use models::casper::BlockEventInfo;
use models::rust::block_implicits::get_random_block_default;
// Changed by DR-134 (DR-70): the offered report tests.
// use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ProcessedDeploy, ProcessedUserDeploy,
};
use prost::bytes::Bytes;
// Added by DR-134 (DR-70): the offered report tests.
use rholang::rust::interpreter::external_services::ExternalServices;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use tokio::sync::Notify;
use tokio::time::timeout;

use crate::engine::setup::TestFixture;
// Added by DR-134 (DR-70): the offered report tests.
use crate::helper::offered_deploy;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_test_rnode_store_manager_shared;

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

// Added by DR-134 (DR-70): an offered block reports through the block report
// API with the real reporter. The report reproduces the block's post-state,
// carries the committed native evidence and the receipt of the receipt route,
// and no event batches for the offered deploy. A second request is served
// from the cache, and a forced replay gives the same report.
#[tokio::test]
async fn an_offered_block_reports_its_native_evidence_and_receipt() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("the protocol-6 genesis builds");
    let mut node = TestNode::standalone(genesis.clone())
        .await
        .expect("a standalone node");
    let deploy = offered_deploy::source_deploy_now(
        r#"new ch in { ch!(1) | for (@x <- ch) { Nil } }"#.to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("an offered deploy");
    let block = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("the offered block is added");
    let offered = block.body.deploys[0]
        .as_offered()
        .expect("the block deploy is offered")
        .clone();

    let mut rspace_kvm = mk_test_rnode_store_manager_shared(genesis.rspace_scope_id.clone());
    let rspace_store = rspace_kvm
        .r_space_stores()
        .await
        .expect("the shared RSpace stores open");
    let reporter = reporting_casper::rho_reporter(
        &rspace_store,
        &node.block_dag_storage,
        node.runtime_manager.replay_lock(),
        ExternalServices::noop(),
    );
    // The standalone node is a validator, so the report needs dev mode.
    let api = BlockReportAPI::new(
        reporter,
        report_store(),
        node.engine_cell.clone(),
        node.block_store.clone(),
        CliqueOracleImpl,
        true,
    );

    let report = api
        .block_report(block.block_hash.clone(), false)
        .await
        .expect("the offered block reports");
    assert_eq!(report.post_state_hash, block.body.state.post_state_hash);
    assert_eq!(report.deploys.len(), 1);
    let deploy_report = &report.deploys[0];
    // Changed by DR-134 (DR-70): the offered deploy reports the events of its
    // user run in a USER batch, as dev reports a user deploy.
    // assert!(
    //     deploy_report.report.is_empty(),
    //     "an offered deploy reports no event batches"
    // );
    assert!(
        deploy_report.report.iter().any(|batch| batch.phase
            == models::casper::ReportPhase::User as i32
            && !batch.events.is_empty()),
        "an offered deploy reports its user-run events in a USER batch"
    );
    let receipt = BlockAPI::find_offered_settlement_receipt(
        &node.engine_cell,
        &offered.identity_bytes().to_vec(),
    )
    .await
    .expect("the receipt route answers");
    let expected = OfferedDeployReport {
        native_cost_evidence: offered.native_cost_evidence_bytes().to_vec(),
        receipt,
    }
    .to_proto();
    assert!(
        expected
            .receipt
            .as_ref()
            .is_some_and(|receipt| !receipt.purses.is_empty()),
        "the receipt names the payer purse"
    );
    assert_eq!(deploy_report.offered.as_ref(), Some(&expected));

    let cached = api
        .block_report(block.block_hash.clone(), false)
        .await
        .expect("the cached report is served");
    assert_eq!(cached, report);
    let replayed = api
        .block_report(block.block_hash.clone(), true)
        .await
        .expect("a forced replay reports again");
    assert_eq!(replayed, report);
}

// Added by DR-134 (DR-66 item 5, DR-70): a block with a body-only user deploy
// after genesis returns the compatibility error through the block report API,
// although its pre-state exists.
#[tokio::test]
async fn a_body_only_block_returns_the_compatibility_error() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("the protocol-6 genesis builds");
    let node = TestNode::standalone(genesis.clone())
        .await
        .expect("a standalone node");
    let body_only = casper::rust::util::construct_deploy::source_deploy_now(
        "@0!(0)".to_string(),
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .expect("a body-only deploy");
    let mut block = genesis.genesis_block.clone();
    block.block_hash = Bytes::from(vec![0xbd; 32]);
    block.header.parents_hash_list = vec![genesis.genesis_block.block_hash.clone()];
    block.body.state.pre_state_hash = genesis.genesis_block.body.state.post_state_hash.clone();
    block.body.deploys = vec![ProcessedUserDeploy::Legacy(ProcessedDeploy::empty(
        body_only,
    ))];
    node.block_store
        .put(block.block_hash.clone(), &block)
        .expect("the crafted block is stored");

    let mut rspace_kvm = mk_test_rnode_store_manager_shared(genesis.rspace_scope_id.clone());
    let rspace_store = rspace_kvm
        .r_space_stores()
        .await
        .expect("the shared RSpace stores open");
    let reporter = reporting_casper::rho_reporter(
        &rspace_store,
        &node.block_dag_storage,
        node.runtime_manager.replay_lock(),
        ExternalServices::noop(),
    );
    let api = BlockReportAPI::new(
        reporter,
        report_store(),
        node.engine_cell.clone(),
        node.block_store.clone(),
        CliqueOracleImpl,
        true,
    );

    let result = api.block_report(block.block_hash.clone(), true).await;
    assert!(
        matches!(
            &result,
            Err(BlockReportError::ReplayFailed(message)) if message == BODY_ONLY_DEPLOYS_UNSUPPORTED
        ),
        "unexpected result: {:?}",
        result
    );
}
