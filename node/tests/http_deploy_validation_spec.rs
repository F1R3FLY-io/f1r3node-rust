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
use node::rust::api::admin_web_api::AdminWebApiImpl;
use node::rust::api::web_api::WebApiImpl;
use node::rust::web::shared_handlers::AppState;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

struct StubNodeDiscovery;

#[async_trait::async_trait]
impl NodeDiscovery for StubNodeDiscovery {
    async fn discover(&self) -> Result<(), comm::rust::errors::CommError> { Ok(()) }

    fn peers(&self) -> Result<Vec<PeerNode>, comm::rust::errors::CommError> { Ok(vec![]) }

    fn remove_peer(&self, _peer: &PeerNode) -> Result<(), comm::rust::errors::CommError> { Ok(()) }
}

fn app_state(engine_cell: EngineCell) -> AppState {
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

    let engine_cell = Arc::new(engine_cell);
    let web_api = WebApiImpl::new(
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
        block_report_api.clone(),
        models::rhoapi::Par::default(),
        engine_cell.clone(),
        rp_conf_cell.clone(),
        connections_cell.clone(),
        node_discovery.clone(),
        None,
        100,
        10,
        is_ready,
    );
    let events = F1r3flyEvents::new();
    AppState::new(
        Arc::new(AdminWebApiImpl::new(None, None, engine_cell)),
        Arc::new(web_api),
        Arc::new(block_report_api),
        rp_conf_cell,
        Arc::new(connections_cell),
        node_discovery,
        Arc::new(events.consume()),
        events.startup_buffer(),
    )
}

mod http_deploy_validation {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use axum::Router;
    use casper::rust::api::block_api::DeployValidationError;
    use casper::rust::casper::MultiParentCasper;
    use casper::rust::engine::engine::Engine;
    use casper::rust::errors::CasperError;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Signed;
    use models::rust::casper::protocol::casper_message::{CasperMessage, DeployData};
    use node::rust::api::web_api::DeployRequest;
    use node::rust::web::web_api_routes::WebApiRoutes;
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
            parameters: vec![
                models::rust::deploy_parameters::DeployParameter {
                    name: "input".into(),
                    value: models::rust::deploy_parameters::RholangValue::BigInt(
                        (num_bigint::BigInt::from(1) << 512usize) - 1,
                    ),
                },
                models::rust::deploy_parameters::DeployParameter {
                    name: "count".into(),
                    value: models::rust::deploy_parameters::RholangValue::Int(42),
                },
            ],
        };
        let (key, _) = Secp256k1.new_key_pair();
        let signed = Signed::create(data, Box::new(Secp256k1), key).unwrap();
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
        let router = WebApiRoutes::create_router().with_state(app_state(engine));
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
        let invalid_signature_message =
            "Invalid signature: the signature does not verify for this deployer and deploy data";
        let mut public_key = valid.clone();
        public_key.deployer = "00".repeat(65);
        cases.push((public_key, invalid_signature_message));
        let mut signature = valid.clone();
        signature.signature = "0011".into();
        cases.push((signature, invalid_signature_message));
        let mut signature = valid.clone();
        let mut signature_bytes = hex::decode(&signature.signature).unwrap();
        signature_bytes[10] ^= 0xFF;
        signature.signature = hex::encode(signature_bytes);
        cases.push((signature, invalid_signature_message));
        let mut timestamp = valid.clone();
        timestamp.data.time_stamp = 2;
        cases.push((timestamp, invalid_signature_message));
        let mut tampered = valid;
        tampered.data.term = "new x in { x!(1) }".into();
        cases.push((tampered, invalid_signature_message));
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
        assert_eq!(json["data"]["parameters"][1]["value"]["value"], "42");
        assert_eq!(
            json["data"]["parameters"][0]["value"]["value"],
            ((num_bigint::BigInt::from(1) << 512usize) - 1u32).to_string()
        );
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
        let mut invalid_name = valid;
        invalid_name["data"]["parameters"][0]["name"] = json!("invalid:name");
        let (status, body) =
            response(router.clone(), invalid_name.to_string(), "application/json").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["error"], "invalid_request_body");
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
        let web = app_state(EngineCell::init()).web_api;
        let mut request = signed_request();
        request.data.parameters[0].name = "invalid:name".into();
        let error = web.deploy(request).await.unwrap_err();
        assert!(error.downcast_ref::<DeployValidationError>().is_some());
        assert!(error.to_string().contains("Invalid deploy parameters"));
    }

    #[test]
    fn json_routes_document_extraction_errors_in_all_openapi_artifacts() {
        use node::rust::web::web_api_docs::{AdminApi, PublicApi};
        use utoipa::OpenApi;

        let static_api: Value =
            serde_json::from_str(include_str!("../../docs/rnode-api/rnode-openapi.json")).unwrap();
        for api in [PublicApi::openapi(), AdminApi::openapi()] {
            let api = serde_json::to_value(api).unwrap();
            for route in [
                "/deploy",
                "/explore-deploy",
                "/explore-deploy-by-block-hash",
                "/api/prepare-deploy",
                "/api/data-at-name-by-block-hash",
                "/api/estimate-cost",
            ] {
                let responses = &api["paths"][route]["post"]["responses"];
                let static_route = route.strip_prefix("/api").unwrap_or(route);
                let static_responses = &static_api["paths"][static_route]["post"]["responses"];
                for status in ["400", "415", "422"] {
                    let response = &responses[status];
                    assert_eq!(
                        response["content"]["application/json"]["schema"]["$ref"],
                        "#/components/schemas/ApiErrorResponse",
                        "{route}: {status}"
                    );
                    assert_eq!(
                        static_responses[status]["schema"]["$ref"],
                        "#/definitions/ApiErrorResponse",
                        "{route}: {status}"
                    );
                }
                for status in ["415", "422"] {
                    for responses in [responses, static_responses] {
                        assert!(
                            responses[status]["description"]
                                .as_str()
                                .unwrap()
                                .contains("invalid_request_body"),
                            "{route}: {status}"
                        );
                    }
                }
                assert_eq!(
                    static_api["paths"][static_route]["post"]["consumes"],
                    json!(["application/json"])
                );
            }
            assert_eq!(
                api["components"]["schemas"]["ApiErrorResponse"]["required"],
                json!(["error", "message"])
            );
            let error_description = api["components"]["schemas"]["ApiErrorResponse"]["properties"]
                ["error"]["description"]
                .as_str()
                .unwrap();
            for heading in [
                "**415 Unsupported Media Type:**",
                "**422 Unprocessable Entity:**",
            ] {
                let section = error_description.split_once(heading).unwrap().1;
                assert!(section
                    .split("**")
                    .next()
                    .unwrap()
                    .contains("invalid_request_body"));
            }
        }
        assert_eq!(
            static_api["definitions"]["ApiErrorResponse"]["required"],
            json!(["error", "message"])
        );
    }
}
