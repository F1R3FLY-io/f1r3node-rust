use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use bincode::Options;
use consensus_api::{ConsensusError, Phase};
use consensus_runtime::ConsensusHandle;
use cordial_consensus::CordialQueries;
use models::casper::v1::deploy_service_server::{DeployService, DeployServiceServer};
use models::casper::v1::propose_service_server::{ProposeService, ProposeServiceServer};
use models::casper::v1::*;
use models::casper::*;
use prost::Message;
use serde::Deserialize;
use serde_json::{json, Value};
use tonic::{Request, Response, Status};

use super::{PreparedApplication, Resources};
use crate::rust::configuration::NodeConf;
use crate::rust::runtime::application::ApplicationRoutes;

#[derive(Clone)]
struct Api {
    handle: ConsensusHandle,
    queries: CordialQueries,
    resources: Arc<Resources>,
}

type HttpError = (StatusCode, Json<Value>);

fn http_error(error: ConsensusError) -> HttpError {
    let code = match &error {
        ConsensusError::UnsupportedCapability(_) => StatusCode::NOT_IMPLEMENTED,
        ConsensusError::InvalidInput(_) | ConsensusError::Rejected { .. } => {
            StatusCode::BAD_REQUEST
        }
        ConsensusError::NotReady => StatusCode::CONFLICT,
        ConsensusError::QueueFull => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    (code, Json(json!({"error": error.to_string()})))
}

fn rpc_error(error: ConsensusError) -> Status {
    match error {
        ConsensusError::UnsupportedCapability(_) => Status::unimplemented(error.to_string()),
        ConsensusError::InvalidInput(_) | ConsensusError::Rejected { .. } => {
            Status::invalid_argument(error.to_string())
        }
        ConsensusError::QueueFull => Status::resource_exhausted(error.to_string()),
        ConsensusError::NotReady => Status::failed_precondition(error.to_string()),
        _ => Status::unavailable(error.to_string()),
    }
}

fn unsupported() -> Status {
    Status::unimplemented(
        "This Casper query is not supported by Cordial. Use /api/cordial native queries.",
    )
}

impl PreparedApplication {
    pub(crate) fn routes(self, conf: &NodeConf) -> eyre::Result<ApplicationRoutes> {
        let state = Api {
            handle: self.handle,
            queries: self.queries,
            resources: self.resources,
        };
        let settings = &conf.api_server;
        let server = || {
            crate::rust::api::grpc_package::configure_server(
                settings.grpc_max_recv_message_size as usize,
                settings.keep_alive_time,
                settings.keep_alive_timeout,
                settings.tcp_keepalive_time,
                settings.request_timeout,
                settings.max_connection_age,
                settings.max_connection_age_grace,
            )
        };
        let external = server().add_service(
            DeployServiceServer::new(state.clone())
                .max_decoding_message_size(cordial_consensus::MAX_PAYLOAD_BYTES),
        );
        let internal = server()
            .add_service(ProposeServiceServer::new(state.clone()))
            .add_service(
                DeployServiceServer::new(state.clone())
                    .max_decoding_message_size(cordial_consensus::MAX_PAYLOAD_BYTES),
            );
        let public_http = Router::new()
            .route("/api/status", get(status))
            .route("/api/deploy", post(submit))
            .route("/api/cordial/output", get(output))
            .route("/api/cordial/objects/{id}", get(object))
            .route("/api/cordial/receipts/{index}", get(receipt))
            .route("/api/cordial/equivocations", get(equivocations))
            .route("/api/cordial/data", post(data))
            .layer(DefaultBodyLimit::max(cordial_consensus::MAX_PAYLOAD_BYTES))
            .with_state(state.clone());
        let admin_http = Router::new()
            .route("/api/propose", post(propose))
            .route("/api/status", get(status))
            .layer(DefaultBodyLimit::max(1024))
            .with_state(state);
        Ok(ApplicationRoutes {
            external,
            internal,
            public_http,
            admin_http,
        })
    }
}

async fn status(State(api): State<Api>) -> Result<Json<Value>, HttpError> {
    let status = api.handle.status();
    let progress = api.queries.progress().await.map_err(http_error)?;
    Ok(Json(
        json!({"protocol": "cordial-miners", "version": 2, "phase": format!("{:?}", status.phase), "ready": status.phase == Phase::Ready, "progress": progress}),
    ))
}

async fn submit(
    State(api): State<Api>,
    payload: axum::body::Bytes,
) -> Result<Json<Value>, HttpError> {
    let id = api
        .handle
        .submit(payload.to_vec())
        .await
        .map_err(http_error)?;
    Ok(Json(json!({"deploy_id": id})))
}

async fn propose(State(api): State<Api>) -> Result<Json<Value>, HttpError> {
    let id = api.handle.propose(false).await.map_err(http_error)?;
    Ok(Json(json!({"object_id": id})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    #[serde(default)]
    start: u64,
    #[serde(default = "page_limit")]
    limit: usize,
}
fn page_limit() -> usize { 32 }

async fn output(
    State(api): State<Api>,
    Query(page): Query<Page>,
) -> Result<Json<Value>, HttpError> {
    let objects = api
        .queries
        .output(page.start, page.limit)
        .await
        .map_err(http_error)?;
    let records: Result<Vec<_>, _> = objects.iter().enumerate().map(|(offset, identity)| {
        bincode::DefaultOptions::new().with_fixint_encoding().serialize(identity).map(|bytes| json!({"index": page.start + offset as u64, "id": hex::encode(bytes), "identity": identity}))
    }).collect();
    Ok(Json(
        json!({"records": records.map_err(|error| http_error(ConsensusError::Protocol(error.to_string())))?}),
    ))
}

async fn object(State(api): State<Api>, Path(id): Path<String>) -> Result<Json<Value>, HttpError> {
    if id.len() > 1024 {
        return Err(http_error(ConsensusError::InvalidInput(
            "object identifier exceeds limit".into(),
        )));
    }
    let bytes = hex::decode(id).map_err(|_| {
        http_error(ConsensusError::InvalidInput(
            "invalid object identifier".into(),
        ))
    })?;
    let identity = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(512)
        .reject_trailing_bytes()
        .deserialize(&bytes)
        .map_err(|_| {
            http_error(ConsensusError::InvalidInput(
                "invalid object identifier".into(),
            ))
        })?;
    let packet = api
        .queries
        .object(identity)
        .await
        .map_err(http_error)?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "object not found"})),
        ))?;
    Ok(Json(json!({"packet": hex::encode(packet)})))
}

async fn receipt(State(api): State<Api>, Path(index): Path<u64>) -> Result<Json<Value>, HttpError> {
    let receipt = api
        .queries
        .receipt(index)
        .await
        .map_err(http_error)?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "execution receipt not found"})),
        ))?;
    let summary: cordial_rholang::ExecutionSummary = serde_json::from_slice(&receipt.result)
        .map_err(|error| http_error(ConsensusError::Protocol(error.to_string())))?;
    Ok(Json(json!({"receipt": receipt, "summary": summary})))
}

async fn equivocations(State(api): State<Api>) -> Result<Json<Value>, HttpError> {
    let records = api.queries.equivocations().await.map_err(http_error)?;
    let records: Vec<_> = records
        .into_iter()
        .map(|record| {
            let objects: Result<Vec<_>, _> = record
                .objects
                .iter()
                .map(|identity| {
                    bincode::DefaultOptions::new()
                        .with_fixint_encoding()
                        .serialize(identity)
                        .map(hex::encode)
                })
                .collect();
            objects.map(|objects| {
                json!({"creator": hex::encode(record.creator), "round": record.round, "objects": objects})
            })
        })
        .collect::<Result<_, _>>()
        .map_err(|error| http_error(ConsensusError::Protocol(error.to_string())))?;
    Ok(Json(json!({"equivocations": records})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataRequest {
    channel: models::rhoapi::Par,
    index: Option<u64>,
}

async fn data(
    State(api): State<Api>,
    Json(request): Json<DataRequest>,
) -> Result<Json<Value>, HttpError> {
    let root = match request.index {
        Some(index) => {
            api.queries
                .receipt(index)
                .await
                .map_err(http_error)?
                .ok_or((
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": "execution receipt not found"})),
                ))?
                .post_state
        }
        None => api.queries.progress().await.map_err(http_error)?.state_root,
    };
    let values = api
        .resources
        .runtime
        .get_data(
            prost::bytes::Bytes::copy_from_slice(&root),
            &request.channel,
        )
        .await
        .map_err(|error| http_error(ConsensusError::Protocol(error.to_string())))?;
    Ok(Json(
        json!({"state_root": hex::encode(root), "data": values}),
    ))
}

#[async_trait::async_trait]
impl DeployService for Api {
    type showMainChainStream =
        tokio_stream::wrappers::ReceiverStream<Result<BlockInfoResponse, Status>>;
    type visualizeDagStream =
        tokio_stream::wrappers::ReceiverStream<Result<VisualizeBlocksResponse, Status>>;
    type getBlocksStream =
        tokio_stream::wrappers::ReceiverStream<Result<BlockInfoResponse, Status>>;
    type getBlocksByHeightsStream =
        tokio_stream::wrappers::ReceiverStream<Result<BlockInfoResponse, Status>>;

    async fn do_deploy(
        &self,
        request: Request<DeployDataProto>,
    ) -> Result<Response<DeployResponse>, Status> {
        let id = self
            .handle
            .submit(request.into_inner().encode_to_vec())
            .await
            .map_err(rpc_error)?;
        Ok(Response::new(DeployResponse {
            message: Some(deploy_response::Message::Result(id)),
        }))
    }
    async fn get_block(&self, _: Request<BlockQuery>) -> Result<Response<BlockResponse>, Status> {
        Err(unsupported())
    }
    async fn machine_verifiable_dag(
        &self,
        _: Request<MachineVerifyQuery>,
    ) -> Result<Response<MachineVerifyResponse>, Status> {
        Err(unsupported())
    }
    async fn get_data_at_name(
        &self,
        _: Request<DataAtNameByBlockQuery>,
    ) -> Result<Response<RhoDataResponse>, Status> {
        Err(unsupported())
    }
    async fn listen_for_continuation_at_name(
        &self,
        _: Request<ContinuationAtNameQuery>,
    ) -> Result<Response<ContinuationAtNameResponse>, Status> {
        Err(unsupported())
    }
    async fn find_deploy(
        &self,
        _: Request<FindDeployQuery>,
    ) -> Result<Response<FindDeployResponse>, Status> {
        Err(unsupported())
    }
    async fn preview_private_names(
        &self,
        _: Request<PrivateNamePreviewQuery>,
    ) -> Result<Response<PrivateNamePreviewResponse>, Status> {
        Err(unsupported())
    }
    async fn last_finalized_block(
        &self,
        _: Request<LastFinalizedBlockQuery>,
    ) -> Result<Response<LastFinalizedBlockResponse>, Status> {
        Err(unsupported())
    }
    async fn is_finalized(
        &self,
        _: Request<IsFinalizedQuery>,
    ) -> Result<Response<IsFinalizedResponse>, Status> {
        Err(unsupported())
    }
    async fn deploy_finalization_status(
        &self,
        _: Request<DeployFinalizationStatusQuery>,
    ) -> Result<Response<DeployFinalizationStatusResponse>, Status> {
        Err(unsupported())
    }
    async fn bond_status(
        &self,
        _: Request<BondStatusQuery>,
    ) -> Result<Response<BondStatusResponse>, Status> {
        Err(unsupported())
    }
    async fn exploratory_deploy(
        &self,
        _: Request<ExploratoryDeployQuery>,
    ) -> Result<Response<ExploratoryDeployResponse>, Status> {
        Err(unsupported())
    }
    async fn get_event_by_hash(
        &self,
        _: Request<ReportQuery>,
    ) -> Result<Response<EventInfoResponse>, Status> {
        Err(unsupported())
    }
    async fn status(&self, _: Request<()>) -> Result<Response<StatusResponse>, Status> {
        Err(unsupported())
    }
    async fn get_pending_deploys(
        &self,
        _: Request<PendingDeploysQuery>,
    ) -> Result<Response<PendingDeploysResponse>, Status> {
        Err(unsupported())
    }
    async fn visualize_dag(
        &self,
        _: Request<VisualizeDagQuery>,
    ) -> Result<Response<Self::visualizeDagStream>, Status> {
        Err(unsupported())
    }
    async fn show_main_chain(
        &self,
        _: Request<BlocksQuery>,
    ) -> Result<Response<Self::showMainChainStream>, Status> {
        Err(unsupported())
    }
    async fn get_blocks(
        &self,
        _: Request<BlocksQuery>,
    ) -> Result<Response<Self::getBlocksStream>, Status> {
        Err(unsupported())
    }
    async fn get_blocks_by_heights(
        &self,
        _: Request<BlocksQueryByHeight>,
    ) -> Result<Response<Self::getBlocksByHeightsStream>, Status> {
        Err(unsupported())
    }
}

#[async_trait::async_trait]
impl ProposeService for Api {
    async fn propose(
        &self,
        request: Request<ProposeQuery>,
    ) -> Result<Response<ProposeResponse>, Status> {
        if request.into_inner().is_async {
            return Err(Status::unimplemented(
                "Cordial supports synchronous proposal requests only",
            ));
        }
        let id = self.handle.propose(false).await.map_err(rpc_error)?;
        Ok(Response::new(ProposeResponse {
            message: Some(propose_response::Message::Result(id)),
        }))
    }
    async fn propose_result(
        &self,
        _: Request<ProposeResultQuery>,
    ) -> Result<Response<ProposeResultResponse>, Status> {
        Err(Status::unimplemented(
            "Cordial proposals return their native object identity directly",
        ))
    }
}
