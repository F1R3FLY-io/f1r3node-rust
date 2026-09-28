use std::sync::Arc;

use casper::rust::api::block_report_api::{BlockReportAPI, BlockReportError};
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::report_store::CompressedBlockEventInfoStore;
use casper::rust::reporting_casper::NoopReportingCasper;
use casper::rust::safety_oracle::CliqueOracleImpl;
use models::casper::BlockEventInfo;
use models::rust::block_implicits::get_random_block_default;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;

use crate::engine::setup::TestFixture;

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
