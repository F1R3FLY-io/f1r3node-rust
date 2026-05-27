use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use casper::rust::api::block_report_api::BlockReportAPI;
use casper::rust::errors::CasperError;
use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::rp::connect::ConnectionsCell;
use rholang::rust::interpreter::errors::InterpreterError;
use serde_json::json;
use shared::rust::shared::f1r3fly_events::{EventStream, StartupBuffer};
use tracing::warn;

use crate::rust::api::admin_web_api::AdminWebApi;
use crate::rust::api::serde_types::block_info::BlockInfoSerde;
use crate::rust::api::web_api::{
    DeployRequest, ExploreDeployRequest, RhoDataResponse, SimpleExploreDeployRequest, ViewMode,
    WebApi,
};

#[derive(Clone)]
pub struct AppState {
    pub admin_web_api: Arc<dyn AdminWebApi + Send + Sync + 'static>,
    pub web_api: Arc<dyn WebApi + Send + Sync + 'static>,
    pub block_report_api: Arc<BlockReportAPI>,
    pub rp_conf_cell: comm::rust::rp::rp_conf::RPConfCell,
    pub connections_cell: Arc<ConnectionsCell>,
    pub node_discovery: Arc<dyn NodeDiscovery + Send + Sync + 'static>,
    pub event_stream: Arc<EventStream>,
    pub startup_events: StartupBuffer,
}

impl AppState {
    pub fn new(
        admin_web_api: Arc<dyn AdminWebApi + Send + Sync + 'static>,
        web_api: Arc<dyn WebApi + Send + Sync + 'static>,
        block_report_api: Arc<BlockReportAPI>,
        rp_conf_cell: comm::rust::rp::rp_conf::RPConfCell,
        connections_cell: Arc<ConnectionsCell>,
        node_discovery: Arc<dyn NodeDiscovery + Send + Sync + 'static>,
        event_consumer: Arc<EventStream>,
        startup_events: StartupBuffer,
    ) -> Self {
        Self {
            admin_web_api,
            web_api,
            block_report_api,
            rp_conf_cell,
            connections_cell,
            node_discovery,
            event_stream: event_consumer,
            startup_events,
        }
    }
}

pub struct AppError(pub eyre::Error);

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::warn!("API error: {:#}", self.0);

        let (status, error_kind, message) = classify_error(&self.0);

        let body = Json(json!({
            "error": error_kind,
            "message": message,
        }));

        (status, body).into_response()
    }
}

impl<E> From<E> for AppError
where E: Into<eyre::Error>
{
    fn from(err: E) -> Self { Self(err.into()) }
}

fn classify_error(err: &eyre::Error) -> (StatusCode, &'static str, String) {
    for cause in err.chain() {
        if let Some(ce) = cause.downcast_ref::<CasperError>() {
            return classify_casper_error(ce);
        }
    }

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "unknown_error",
        err.to_string(),
    )
}

fn classify_casper_error(err: &CasperError) -> (StatusCode, &'static str, String) {
    use CasperError::*;
    use StatusCode as S;

    let internal = |kind| {
        (
            S::INTERNAL_SERVER_ERROR,
            kind,
            err.to_string(),
        )
    };

    match err {
        InterpreterError(ie) => classify_interpreter_error(ie),

        CommError(_) => (
            S::BAD_GATEWAY,
            "comm_error",
            err.to_string(),
        ),

        SigningError(_)         => internal("signing_error"),
        KvStoreError(_)         => internal("kv_store_error"),
        HistoryError(_)         => internal("history_error"),
        RuntimeError(_)         => internal("runtime_error"),
        SystemRuntimeError(_)   => internal("system_runtime_error"),
        ReplayFailure(_)        => internal("replay_failure"),
        StreamError(_)          => internal("stream_error"),
        LockError(_)            => internal("lock_error"),
        Other(_)                => internal("other_error"),
    }
}

fn classify_interpreter_error(ie: &InterpreterError) -> (StatusCode, &'static str, String) {
    use InterpreterError::*;
    use StatusCode as S;

    match ie {
        // === 400 Bad Request — term rejected before execution ===
        SyntaxError(_) | LexerError(_) | ParserError(_)
        | NormalizerError(_) | UnrecognizedNormalizerError(_)
        | TopLevelWildcardsNotAllowedError(_)
        | TopLevelFreeVariablesNotAllowedError(_)
        | TopLevelLogicalConnectivesNotAllowedError(_)
        | UnexpectedProcContext { .. }
        | UnexpectedReuseOfProcContextFree { .. }
        | UnexpectedNameContext { .. }
        | UnexpectedReuseOfNameContextFree { .. }
        | UnboundVariableRefSpan { .. }
        | UnboundVariableRefPos { .. }
        | ReceiveOnSameChannelsError { .. }
        | PatternReceiveError(_)
        | UnexpectedBundleContent(_) => (S::BAD_REQUEST, "rholang_bad_term", ie.to_string()),

        // Bad arguments to a system process (e.g. rho:io:stdout) — client error
        IllegalArgumentError(_) => (S::BAD_REQUEST, "illegal_argument", ie.to_string()),

        // === 422 Unprocessable Entity — term valid, execution failed ===
        OutOfPhlogistonsError => (S::UNPROCESSABLE_ENTITY, "out_of_phlogistons", ie.to_string()),
        UserAbortError => (S::UNPROCESSABLE_ENTITY, "user_abort", ie.to_string()),

        ReduceError(_)
        | MethodNotDefined { .. }
        | MethodArgumentNumberMismatch { .. }
        | OperatorNotDefined { .. }
        | OperatorExpectedError { .. }
        | SubstituteError(_)
        | SortMatchError(_) => {
            (S::UNPROCESSABLE_ENTITY, "rholang_execution_error", ie.to_string())
        }

        // === 500 Internal Server Error — node-side problem ===
        BugFoundError(_) | RSpaceError(_) | SetupError(_) | IoError(_)
        | UndefinedRequiredProtobufFieldError(_) | EncodeError(_) | DecodeError(_)
        | CanNotReplayFailedNonDeterministicProcess
        | UnrecognizedInterpreterError(_) => (
            S::INTERNAL_SERVER_ERROR,
            "interpreter_internal_error",
            ie.to_string(),
        ),

        // === 502 Bad Gateway — upstream non-deterministic service failure ===
        OpenAIError(_) | OllamaError(_) | ChromaDBError(_)
        | NonDeterministicProcessFailure { .. }
        | ProduceFailureWithOutput { .. } => {
            (S::BAD_GATEWAY, "external_service_error", ie.to_string())
        }

        AggregateError { interpreter_errors } => {
            let msg = interpreter_errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            (S::UNPROCESSABLE_ENTITY, "aggregate_error", msg)
        }
    }
}

#[utoipa::path(
    get,
    path = "/status",
    responses(
        (status = 200, description = "API status information"),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal node error"),
    ),
    tag = "Status"
)]
pub async fn status_handler(State(app_state): State<AppState>) -> Response {
    const STATUS_HANDLER_SLOW_THRESHOLD: Duration = Duration::from_millis(500);
    let started = Instant::now();
    match app_state.web_api.status().await {
        Ok(response) => {
            let elapsed = started.elapsed();
            if elapsed >= STATUS_HANDLER_SLOW_THRESHOLD {
                warn!(?elapsed, "HTTP /status handler responded slowly");
            }
            Json(response).into_response()
        }
        Err(e) => {
            let elapsed = started.elapsed();
            warn!(?elapsed, error = %e, "HTTP /status handler failed");
            AppError(e).into_response()
        }
    }
}

#[utoipa::path(
    post,
    path = "/deploy",
    request_body = DeployRequest,
    responses(
        (status = 200, description = "Deploy submitted successfully", body = String),
        (status = 400, description = "Malformed deploy request"),
        (status = 422, description = "Deploy request is valid in structure but cannot be executed"),
        (status = 500, description = "Internal node error while processing deploy"),
    ),
    tag = "Deployment"
)]
pub async fn deploy_handler(
    State(app_state): State<AppState>,
    Json(request): Json<DeployRequest>,
) -> Response {
    match app_state.web_api.deploy(request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/explore-deploy",
    request_body = SimpleExploreDeployRequest,
    responses(
        (status = 200, description = "Deploy submitted successfully", body = RhoDataResponse),
        (status = 400, description = "Malformed deploy request"),
        (status = 422, description = "Deploy request is valid in structure but cannot be executed"),
        (status = 500, description = "Internal node error while processing deploy"),
    ),
    tag = "Deployment"
)]
pub async fn explore_deploy_handler(
    State(app_state): State<AppState>,
    Json(request): Json<SimpleExploreDeployRequest>,
) -> Response {
    match app_state
        .web_api
        .exploratory_deploy(request.term, None, false)
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/explore-deploy-by-block-hash",
    request_body = ExploreDeployRequest,
    responses(
        (status = 200, description = "Exploratory deploy successful", body = RhoDataResponse),
        (status = 400, description = "Malformed deploy request"),
        (status = 422, description = "Deploy request is valid in structure but cannot be executed"),
        (status = 500, description = "Internal node error while processing deploy"),
    ),
    tag = "Deployment"
)]
pub async fn explore_deploy_by_block_hash_handler(
    State(app_state): State<AppState>,
    Json(request): Json<ExploreDeployRequest>,
) -> Response {
    let request_block_hash = if request.block_hash.is_empty() {
        None
    } else {
        Some(request.block_hash)
    };

    match app_state
        .web_api
        .exploratory_deploy(request.term, request_block_hash, false)
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/blocks",
    params(
        ("view" = Option<String>, Query, description = "Response view: 'summary' (default) block headers only, 'full' includes deploys"),
    ),
    responses(
        (status = 200, description = "Blocks retrieved successfully", body = Vec<BlockInfoSerde>),
        (status = 400, description = "Error retrieving blocks"),
        (status = 500, description = "Internal node error"),
    ),
    tag = "Blocks"
)]
pub async fn get_blocks_handler(
    State(app_state): State<AppState>,
    Query(query): Query<crate::rust::web::web_api_routes::ViewQuery>,
) -> Response {
    let view = match query.view.as_deref() {
        Some("full") => ViewMode::Full,
        _ => ViewMode::Summary,
    };
    match app_state.web_api.get_blocks(1, view).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/block/{hash}",
    params(
        ("hash" = String, Path, description = "Block hash in hex format"),
        ("view" = Option<String>, Query, description = "Response view: 'full' (default) includes deploys, 'summary' block header only"),
    ),
    responses(
        (status = 200, description = "Block information retrieved successfully", body = BlockInfoSerde),
        (status = 400, description = "Block not found or invalid hash"),
        (status = 500, description = "Internal node error"),
    ),
    tag = "Blocks"
)]
pub async fn get_block_handler(
    State(app_state): State<AppState>,
    Path(hash): Path<String>,
    Query(query): Query<crate::rust::web::web_api_routes::ViewQuery>,
) -> Response {
    let view = match query.view.as_deref() {
        Some("summary") => ViewMode::Summary,
        _ => ViewMode::Full,
    };
    match app_state.web_api.get_block(hash, view).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}
