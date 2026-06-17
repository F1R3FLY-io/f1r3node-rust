use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Path, Query, Request, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use casper::rust::api::block_api::{
    BlockNotFoundError, DeployNotFoundError, DeployValidationError, ExploratoryDeployReadOnlyError,
    InvalidHashError, InvalidPublicKeyError, LatestBlockMessageError, NoNewDeploysError,
    ProposeReadOnlyError,
};
use casper::rust::api::block_report_api::BlockReportAPI;
use casper::rust::errors::CasperError;
use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::rp::connect::ConnectionsCell;
use rholang::rust::interpreter::errors::InterpreterError;
use serde::Serialize;
use serde_json::json;
use shared::rust::shared::f1r3fly_events::{EventStream, StartupBuffer};
use tracing::warn;
use utoipa::ToSchema;

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

/// Structured error response returned by all API endpoints on failure.
/// Every non-2xx response body conforms to this schema.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiErrorResponse {
    /// Machine-readable error kind. Stable across node versions — safe to switch on in client code.
    ///
    /// **400 Bad Request:**
    /// `invalid_request_body`, `invalid_path_parameter`, `invalid_query_parameter`,
    /// `invalid_hash`, `illegal_argument`, `rholang_bad_term`,
    /// `readonly_node_required`,
    ///
    /// **404 Not Found:**
    /// `deploy_not_found`, `block_not_found`, `endpoint_not_found`
    ///
    /// **405 Method Not Allowed:**
    /// `method_not_allowed`
    ///
    /// **422 Unprocessable Entity:**
    /// `out_of_phlogistons`, `user_abort`, `rholang_execution_error`, `aggregate_error`
    ///
    /// **409 Conflict:**
    /// `no_new_deploys`
    ///
    /// **500 Internal Server Error:**
    /// `interpreter_internal_error`, `signing_error`, `replay_failure`,
    /// `kv_store_error`, `history_error`, `system_runtime_error`,
    /// `stream_error`, `lock_error`, `other_error`, `unknown_error`
    ///
    /// **502 Bad Gateway:**
    /// `comm_error`, `external_service_error`
    pub error: String,
    /// Human-readable description of the error.
    pub message: String,
}

pub struct AppError(pub eyre::Error);

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_kind, message) = classify_error(&self.0);

        if status.is_server_error() {
            tracing::warn!("API error: {:#}", self.0);
        } else {
            tracing::debug!("API error: {:#}", self.0);
        }

        (
            status,
            Json(ApiErrorResponse {
                error: error_kind.to_string(),
                message,
            }),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where E: Into<eyre::Error>
{
    fn from(err: E) -> Self { Self(err.into()) }
}

/// Json extractor that returns rejection errors as JSON instead of plain text
pub struct AppJson<T>(pub T);

impl<T, S> FromRequest<S> for AppJson<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AppJson(value)),
            Err(rejection) => Err((
                rejection.status(),
                Json(json!({
                    "error": "invalid_request_body",
                    "message": rejection.body_text(),
                })),
            )
                .into_response()),
        }
    }
}

/// Path extractor that returns rejection errors as JSON instead of plain text
pub struct AppPath<T>(pub T);

impl<T, S> FromRequestParts<S> for AppPath<T>
where
    Path<T>: FromRequestParts<S, Rejection = PathRejection>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Path::<T>::from_request_parts(parts, state).await {
            Ok(Path(value)) => Ok(AppPath(value)),
            Err(rejection) => Err((
                rejection.status(),
                Json(json!({
                    "error": "invalid_path_parameter",
                    "message": rejection.body_text(),
                })),
            )
                .into_response()),
        }
    }
}

/// Query extractor that returns rejection errors as JSON instead of plain text
pub struct AppQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for AppQuery<T>
where
    Query<T>: FromRequestParts<S, Rejection = QueryRejection>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(AppQuery(value)),
            Err(rejection) => Err((
                rejection.status(),
                Json(json!({
                    "error": "invalid_query_parameter",
                    "message": rejection.body_text(),
                })),
            )
                .into_response()),
        }
    }
}

fn classify_error(err: &eyre::Error) -> (StatusCode, &'static str, String) {
    for cause in err.chain() {
        if let Some(ce) = cause.downcast_ref::<CasperError>() {
            return classify_casper_error(ce);
        }
        if cause.downcast_ref::<DeployNotFoundError>().is_some() {
            return (StatusCode::NOT_FOUND, "deploy_not_found", cause.to_string());
        }
        if cause.downcast_ref::<BlockNotFoundError>().is_some() {
            return (StatusCode::NOT_FOUND, "block_not_found", cause.to_string());
        }
        if cause.downcast_ref::<InvalidHashError>().is_some() {
            return (StatusCode::BAD_REQUEST, "invalid_hash", cause.to_string());
        }
        if cause
            .downcast_ref::<ExploratoryDeployReadOnlyError>()
            .is_some()
        {
            return (
                StatusCode::BAD_REQUEST,
                "readonly_node_required",
                cause.to_string(),
            );
        }
        if cause.downcast_ref::<InvalidPublicKeyError>().is_some() {
            return (
                StatusCode::BAD_REQUEST,
                "illegal_argument",
                cause.to_string(),
            );
        }
        if let Some(e) = cause.downcast_ref::<LatestBlockMessageError>() {
            return match e {
                LatestBlockMessageError::NodeReadOnlyError => (
                    StatusCode::BAD_REQUEST,
                    "validator_node_required",
                    cause.to_string(),
                ),
                LatestBlockMessageError::NoBlockMessageError => {
                    (StatusCode::NOT_FOUND, "block_not_found", cause.to_string())
                }
            };
        }
        if cause.downcast_ref::<DeployValidationError>().is_some() {
            return (
                StatusCode::BAD_REQUEST,
                "illegal_argument",
                cause.to_string(),
            );
        }
        if cause.downcast_ref::<ProposeReadOnlyError>().is_some() {
            return (
                StatusCode::BAD_REQUEST,
                "readonly_node_required",
                cause.to_string(),
            );
        }
        if cause.downcast_ref::<NoNewDeploysError>().is_some() {
            return (StatusCode::CONFLICT, "no_new_deploys", cause.to_string());
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

    let internal = |kind| (S::INTERNAL_SERVER_ERROR, kind, err.to_string());

    match err {
        InterpreterError(ie) => classify_interpreter_error(ie),

        CommError(_) => (S::BAD_GATEWAY, "comm_error", err.to_string()),

        SigningError(_) => internal("signing_error"),
        KvStoreError(_) => internal("kv_store_error"),
        HistoryError(_) => internal("history_error"),
        RuntimeError(_) => internal("runtime_error"),
        SystemRuntimeError(_) => internal("system_runtime_error"),
        ReplayFailure(_) => internal("replay_failure"),
        StreamError(_) => internal("stream_error"),
        LockError(_) => internal("lock_error"),
        Other(_) => internal("other_error"),
    }
}

fn classify_interpreter_error(ie: &InterpreterError) -> (StatusCode, &'static str, String) {
    use InterpreterError::*;
    use StatusCode as S;

    match ie {
        // === 400 Bad Request — term rejected before execution ===
        SyntaxError(_)
        | LexerError(_)
        | ParserError(_)
        | NormalizerError(_)
        | UnrecognizedNormalizerError(_)
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
        OutOfPhlogistonsError => (
            S::UNPROCESSABLE_ENTITY,
            "out_of_phlogistons",
            ie.to_string(),
        ),
        UserAbortError => (S::UNPROCESSABLE_ENTITY, "user_abort", ie.to_string()),

        ReduceError(_)
        | MethodNotDefined { .. }
        | MethodArgumentNumberMismatch { .. }
        | OperatorNotDefined { .. }
        | OperatorExpectedError { .. }
        | SubstituteError(_)
        | SortMatchError(_) => (
            S::UNPROCESSABLE_ENTITY,
            "rholang_execution_error",
            ie.to_string(),
        ),

        // === 500 Internal Server Error — node-side problem ===
        BugFoundError(_)
        | RSpaceError(_)
        | SetupError(_)
        | IoError(_)
        | UndefinedRequiredProtobufFieldError(_)
        | EncodeError(_)
        | DecodeError(_)
        | CanNotReplayFailedNonDeterministicProcess
        | UnrecognizedInterpreterError(_) => (
            S::INTERNAL_SERVER_ERROR,
            "interpreter_internal_error",
            ie.to_string(),
        ),

        // === 502 Bad Gateway — upstream non-deterministic service failure ===
        OpenAIError(_)
        | OllamaError(_)
        | ChromaDBError(_)
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
        (status = 200, description = "Node status and connectivity information"),
        (status = 500, description = "Node is unable to report status", body = ApiErrorResponse),
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
        (status = 200, description = "Deploy accepted; returns the deploy ID (hex)", body = String),
        (status = 400, description = "Malformed request body or invalid field value (`invalid_request_body`, `illegal_argument`, `rholang_bad_term`)", body = ApiErrorResponse),
        (status = 422, description = "Term is structurally valid but failed execution (`rholang_execution_error`, `out_of_phlogistons`, `user_abort`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`, `replay_failure`, `signing_error`)", body = ApiErrorResponse),
        (status = 502, description = "Upstream or peer communication failure (`comm_error`, `external_service_error`)", body = ApiErrorResponse),
    ),
    tag = "Deployment"
)]
pub async fn deploy_handler(
    State(app_state): State<AppState>,
    AppJson(request): AppJson<DeployRequest>,
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
        (status = 200, description = "Exploratory deploy executed; returns channel data", body = RhoDataResponse),
        (status = 400, description = "Malformed request body, invalid Rholang term, or node is not read-only (`invalid_request_body`, `rholang_bad_term`, `readonly_node_required`)", body = ApiErrorResponse),
        (status = 422, description = "Term is structurally valid but failed execution (`rholang_execution_error`, `out_of_phlogistons`, `user_abort`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
        (status = 502, description = "External service failure (`external_service_error`)", body = ApiErrorResponse),
    ),
    tag = "Deployment"
)]
pub async fn explore_deploy_handler(
    State(app_state): State<AppState>,
    AppJson(request): AppJson<SimpleExploreDeployRequest>,
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
        (status = 200, description = "Exploratory deploy executed against the specified block; returns channel data", body = RhoDataResponse),
        (status = 400, description = "Malformed request body, invalid Rholang term, invalid block hash, or node is not read-only (`invalid_request_body`, `rholang_bad_term`, `invalid_hash`, `readonly_node_required`)", body = ApiErrorResponse),
        (status = 404, description = "Specified block not found (`block_not_found`)", body = ApiErrorResponse),
        (status = 422, description = "Term is structurally valid but failed execution (`rholang_execution_error`, `out_of_phlogistons`, `user_abort`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
        (status = 502, description = "External service failure (`external_service_error`)", body = ApiErrorResponse),
    ),
    tag = "Deployment"
)]
pub async fn explore_deploy_by_block_hash_handler(
    State(app_state): State<AppState>,
    AppJson(request): AppJson<ExploreDeployRequest>,
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
        ("view" = Option<String>, Query, description = "Response view: `summary` (default) returns block headers only; `full` includes deploy list"),
    ),
    responses(
        (status = 200, description = "Most recent block; array of one element", body = Vec<BlockInfoSerde>),
        (status = 400, description = "Invalid query parameter (`invalid_query_parameter`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`, `history_error`)", body = ApiErrorResponse),
    ),
    tag = "Blocks"
)]
pub async fn get_blocks_handler(
    State(app_state): State<AppState>,
    AppQuery(query): AppQuery<crate::rust::web::web_api_routes::ViewQuery>,
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
        ("hash" = String, Path, description = "Full 64-char hex block hash, or a hex prefix of at least 6 characters for prefix lookup"),
        ("view" = Option<String>, Query, description = "Response view: `full` (default) includes deploy list; `summary` returns block header only"),
    ),
    responses(
        (status = 200, description = "Block information", body = BlockInfoSerde),
        (status = 400, description = "Hash is shorter than 6 characters or contains non-hex characters (`invalid_hash`)", body = ApiErrorResponse),
        (status = 404, description = "No block matches the given hash or prefix (`block_not_found`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`, `history_error`)", body = ApiErrorResponse),
    ),
    tag = "Blocks"
)]
pub async fn get_block_handler(
    State(app_state): State<AppState>,
    AppPath(hash): AppPath<String>,
    AppQuery(query): AppQuery<crate::rust::web::web_api_routes::ViewQuery>,
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
