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
use node::rust::api::web_api::{WebApi, WebApiImpl};
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

struct StubNodeDiscovery;

#[async_trait::async_trait]
impl NodeDiscovery for StubNodeDiscovery {
    async fn discover(&self) -> Result<(), comm::rust::errors::CommError> { Ok(()) }

    fn peers(&self) -> Result<Vec<PeerNode>, comm::rust::errors::CommError> { Ok(vec![]) }

    fn remove_peer(&self, _peer: &PeerNode) -> Result<(), comm::rust::errors::CommError> { Ok(()) }
}

fn web_api(engine_cell: EngineCell) -> WebApiImpl {
    let local = PeerNode::new(
        NodeIdentifier::new("0a0b0c0d00000000000000000000000000000000")
            .expect("valid test node ID"),
        "localhost".to_string(),
        40400,
        40404,
    );
    let block_store = KeyValueBlockStore::new(
        Arc::new(InMemoryKeyValueStore::new()),
        Arc::new(InMemoryKeyValueStore::new()),
    );
    let block_report_api = BlockReportAPI::new(
        casper::rust::reporting_casper::noop(),
        ReportStore::new(Arc::new(InMemoryKeyValueStore::new())),
        engine_cell.clone(),
        block_store,
        CliqueOracleImpl,
        false,
    );
    let rp_conf_cell = RPConfCell::new(RPConf::new(
        local,
        "testnet".to_string(),
        comm::rust::rp::chain_id::ChainIdCell::unknown(),
        None,
        Duration::from_secs(1),
        8,
        2,
    ));
    let connections_cell = ConnectionsCell::new();
    let node_discovery: Arc<dyn NodeDiscovery + Send + Sync> = Arc::new(StubNodeDiscovery);
    let is_ready = Arc::new(AtomicBool::new(true));

    WebApiImpl::new(
        10,
        false,
        "testnet".to_string(),
        "root".to_string(),
        1.into(),
        "F1R3".to_string(),
        "F1R3".to_string(),
        8,
        true,
        false,
        block_report_api,
        models::rhoapi::Par::default(),
        Arc::new(engine_cell),
        rp_conf_cell,
        connections_cell,
        node_discovery,
        None,
        100,
        10,
        is_ready,
    )
}

mod http_deploy_validation {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::post;
    use axum::{Json, Router};
    use casper::rust::api::block_api::DeployValidationError;
    use casper::rust::casper::MultiParentCasper;
    use casper::rust::engine::engine::Engine;
    use casper::rust::errors::CasperError;
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::Signed;
    use models::rust::casper::protocol::casper_message::{CasperMessage, DeployData};
    use node::rust::api::web_api::DeployRequest;
    use node::rust::web::shared_handlers::{AppError, AppJson};
    use serde_json::{json, Value};
    use shared::rust::token_amount::TokenAmount;
    use tower::ServiceExt;

    use super::*;

    struct AdmissionProbe(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl Engine for AdmissionProbe {
        async fn init(&self) -> Result<(), CasperError> { Ok(()) }

        async fn handle(&self, _: PeerNode, _: CasperMessage) -> Result<(), CasperError> { Ok(()) }

        fn with_casper(&self) -> Option<Arc<dyn MultiParentCasper + Send + Sync>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            None
        }
    }

    fn signed_request() -> DeployRequest {
        let data = DeployData {
            term: "Nil".into(),
            time_stamp: 1,
            phlo_price: TokenAmount::max(),
            phlo_limit: 1,
            valid_after_block_number: 0,
            shard_id: "root".into(),
            expiration_timestamp: None,
            parameters: vec![models::rust::deploy_parameters::DeployParameter {
                name: "input".into(),
                value: models::rust::deploy_parameters::RholangValue::BigInt(
                    (num_bigint::BigInt::from(1) << 511usize) - 1,
                ),
            }],
        };
        let signed =
            Signed::create(data, Box::new(Secp256k1), PrivateKey::from_bytes(&[1; 32])).unwrap();
        DeployRequest {
            data: signed.data,
            deployer: hex::encode(signed.pk.bytes),
            signature: hex::encode(signed.sig),
            sig_algorithm: "secp256k1".into(),
        }
    }

    async fn router() -> (Router, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let engine = EngineCell::init();
        engine.set(Arc::new(AdmissionProbe(calls.clone()))).await;
        let web = web_api(engine);
        let web = Arc::new(web);
        let router = Router::new().route(
            "/deploy",
            post(move |AppJson(request): AppJson<DeployRequest>| {
                let web = web.clone();
                async move {
                    match web.deploy(request).await {
                        Ok(id) => Json(id).into_response(),
                        Err(error) => AppError(error).into_response(),
                    }
                }
            }),
        );
        (router, calls)
    }

    async fn response(router: Router, body: String, content_type: &str) -> (StatusCode, Value) {
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/deploy")
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn invalid_deploys_return_400_without_reaching_admission() {
        let valid = signed_request();
        let mut cases = Vec::new();
        let mut overflow = valid.clone();
        overflow.data.phlo_limit = 2;
        cases.push((overflow, "Invalid deploy amount: maximum phlo charge"));
        let mut negative_limit = valid.clone();
        negative_limit.data.phlo_limit = -1;
        cases.push((negative_limit, "Invalid deploy amount: Phlo limit"));
        let mut key_hex = valid.clone();
        key_hex.deployer = "zz".into();
        cases.push((key_hex, "Public key is not valid base16"));
        let mut sig_hex = valid.clone();
        sig_hex.signature = "zz".into();
        cases.push((sig_hex, "Signature is not valid base16"));
        let mut algorithm = valid.clone();
        algorithm.sig_algorithm = "unsupported".into();
        cases.push((algorithm, "Signature algorithm not supported"));
        let mut public_key = valid.clone();
        public_key.deployer = "00".repeat(65);
        cases.push((public_key, "Invalid signature or public key"));
        let mut signature = valid.clone();
        signature.signature = "0011".into();
        cases.push((signature, "Invalid signature or public key"));
        let mut tampered = valid;
        tampered.data.term = "new x in { x!(1) }".into();
        cases.push((tampered, "Invalid signature or public key"));
        let (router, calls) = router().await;
        for (request, message) in cases {
            let (status, body) = response(
                router.clone(),
                serde_json::to_string(&request).unwrap(),
                "application/json",
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(body["error"], "illegal_argument");
            assert!(
                body["message"].as_str().unwrap().contains(message),
                "{body}"
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn valid_exact_values_reach_admission_and_internal_failures_remain_500() {
        let (router, calls) = router().await;
        let request = signed_request();
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["data"]["phloPrice"], TokenAmount::max().to_string());
        let (status, body) = response(router, json.to_string(), "application/json").await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "unknown_error");
    }

    #[tokio::test]
    async fn invalid_json_fields_return_422_and_preserve_other_rejection_statuses() {
        let valid = serde_json::to_value(signed_request()).unwrap();
        let (router, calls) = router().await;
        let invalid = vec![
            json!(1),
            json!(1.5),
            json!("01"),
            json!("-0"),
            json!("+1"),
            json!("1e2"),
            json!("1.0"),
            json!("-1"),
            json!((num_bigint::BigInt::from(1) << 256usize).to_string()),
        ];
        for value in invalid {
            let mut request = valid.clone();
            request["data"]["phloPrice"] = value;
            let (status, body) =
                response(router.clone(), request.to_string(), "application/json").await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
            assert_eq!(body["error"], "invalid_request_body");
        }
        for value in [
            json!({"type":"int", "value":42}),
            json!({"type":"int", "value":"01"}),
            json!({"type":"bigInt", "value":(num_bigint::BigInt::from(1) << 512usize).to_string()}),
            json!({"type":"bigInt", "value":(-(num_bigint::BigInt::from(1) << 512usize)).to_string()}),
        ] {
            let mut request = valid.clone();
            request["data"]["parameters"][0]["value"] = value;
            let (status, body) =
                response(router.clone(), request.to_string(), "application/json").await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
            assert_eq!(body["error"], "invalid_request_body");
        }
        for (text, content_type, expected) in [
            ("{", "application/json", StatusCode::BAD_REQUEST),
            ("{}", "text/plain", StatusCode::UNSUPPORTED_MEDIA_TYPE),
        ] {
            let (status, body) = response(router.clone(), text.into(), content_type).await;
            assert_eq!(status, expected);
            assert_eq!(body["error"], "invalid_request_body");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn programmatic_invalid_parameters_retain_validation_error_type() {
        let web = web_api(EngineCell::init());
        let mut request = signed_request();
        request.data.parameters[0].name = "invalid:name".into();
        let error = web.deploy(request).await.unwrap_err();
        assert!(error.downcast_ref::<DeployValidationError>().is_some());
        assert!(error.to_string().contains("Invalid deploy parameters"));
    }
}
