use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::api::block_report_api::BlockReportAPI;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::report_store::ReportStore;
use casper::rust::safety_oracle::CliqueOracleImpl;
use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::peer_node::{NodeIdentifier, PeerNode};
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::{RPConf, RPConfCell};
use models::casper::v1::deploy_service_server::DeployService;
use models::casper::v1::status_response::Message;
use node::rust::api::deploy_grpc_service_v1::DeployGrpcServiceV1Impl;
use node::rust::api::web_api::{WebApi, WebApiImpl};
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

struct StubNodeDiscovery;

#[async_trait::async_trait]
impl NodeDiscovery for StubNodeDiscovery {
    async fn discover(&self) -> Result<(), comm::rust::errors::CommError> { Ok(()) }

    fn peers(&self) -> Result<Vec<PeerNode>, comm::rust::errors::CommError> { Ok(vec![]) }

    fn remove_peer(&self, _peer: &PeerNode) -> Result<(), comm::rust::errors::CommError> { Ok(()) }
}

fn services(
    has_validator_key: bool,
    is_node_read_only: bool,
) -> (WebApiImpl, DeployGrpcServiceV1Impl) {
    let local = PeerNode::new(
        NodeIdentifier::new("0a0b0c0d00000000000000000000000000000000")
            .expect("valid test node ID"),
        "localhost".to_string(),
        40400,
        40404,
    );
    let engine_cell = EngineCell::init();
    let block_store = KeyValueBlockStore::new(
        Arc::new(InMemoryKeyValueStore::new()),
        Arc::new(InMemoryKeyValueStore::new()),
    );
    let block_report_api = BlockReportAPI::new(
        casper::rust::reporting_casper::noop(),
        ReportStore::new(Arc::new(InMemoryKeyValueStore::new())),
        engine_cell.clone(),
        block_store.clone(),
        CliqueOracleImpl,
        false,
    );
    let rp_conf_cell = RPConfCell::new(RPConf::new(
        local,
        "testnet".to_string(),
        None,
        Duration::from_secs(1),
        8,
        2,
    ));
    let connections_cell = ConnectionsCell::new();
    let node_discovery: Arc<dyn NodeDiscovery + Send + Sync> = Arc::new(StubNodeDiscovery);
    let is_ready = Arc::new(AtomicBool::new(true));

    let web = WebApiImpl::new(
        10,
        false,
        "testnet".to_string(),
        "root".to_string(),
        1,
        "F1R3".to_string(),
        "F1R3".to_string(),
        8,
        has_validator_key,
        is_node_read_only,
        block_report_api.clone(),
        models::rhoapi::Par::default(),
        Arc::new(engine_cell.clone()),
        rp_conf_cell.clone(),
        connections_cell.clone(),
        node_discovery.clone(),
        None,
        100,
        10,
        is_ready.clone(),
    );
    let grpc = DeployGrpcServiceV1Impl::new(
        10,
        None,
        false,
        "testnet".to_string(),
        "root".to_string(),
        1,
        "F1R3".to_string(),
        "F1R3".to_string(),
        8,
        has_validator_key,
        is_node_read_only,
        engine_cell,
        block_report_api,
        models::rhoapi::Par::default(),
        block_store,
        rp_conf_cell,
        connections_cell,
        node_discovery,
        100,
        is_ready,
    );

    (web, grpc)
}

#[tokio::test]
async fn status_reports_validator_identity_without_autopropose() {
    for (has_validator_key, is_node_read_only) in
        [(true, false), (false, true), (false, false), (true, true)]
    {
        let (web, grpc) = services(has_validator_key, is_node_read_only);
        let web_status = web.status().await.unwrap();
        assert_eq!(web_status.is_validator, has_validator_key);
        assert_eq!(web_status.is_read_only, is_node_read_only);

        let grpc_response = grpc.status(tonic::Request::new(())).await.unwrap();
        let Some(Message::Status(grpc_status)) = grpc_response.into_inner().message else {
            panic!("expected status response");
        };
        assert_eq!(grpc_status.is_validator, has_validator_key);
        assert_eq!(grpc_status.is_read_only, is_node_read_only);
    }
}
