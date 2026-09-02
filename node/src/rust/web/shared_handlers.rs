use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Path, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use casper::rust::api::block_api::{
    BlockNotFoundError, DeployNotFoundError, DeployValidationError, ExploratoryDeployReadOnlyError,
    ExploratoryDeployRejection, InvalidHashError, InvalidPublicKeyError, LatestBlockMessageError,
    NoNewDeploysError, ProposeReadOnlyError,
};
use casper::rust::api::block_report_api::BlockReportAPI;
use casper::rust::casper::DeployError;
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
    ///
    /// **503 Service Unavailable:**
    /// `observer_busy` — carries `Retry-After`
    ///
    /// **504 Gateway Timeout:**
    /// `exploratory_timeout`
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

        let retry_after = retry_after_secs(&self.0);

        let mut response = (
            status,
            Json(ApiErrorResponse {
                error: error_kind.to_string(),
                message,
            }),
        )
            .into_response();

        if let Some(secs) = retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(secs));
        }

        response
    }
}

/// `Retry-After` hint for the rejections that carry one. Read from the error
/// variant rather than recomputed from the rendered message, so the header and
/// the body cannot disagree.
fn retry_after_secs(err: &eyre::Error) -> Option<u64> {
    match ExploratoryDeployRejection::classify(err) {
        Some(ExploratoryDeployRejection::Busy { retry_after_secs }) => Some(retry_after_secs),
        Some(ExploratoryDeployRejection::Timeout { .. }) | None => None,
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
        if let Some(rejection) = ExploratoryDeployRejection::from_cause(cause) {
            let (status, kind) = match rejection {
                ExploratoryDeployRejection::Busy { .. } => {
                    (StatusCode::SERVICE_UNAVAILABLE, "observer_busy")
                }
                ExploratoryDeployRejection::Timeout { .. } => {
                    (StatusCode::GATEWAY_TIMEOUT, "exploratory_timeout")
                }
            };
            return (status, kind, cause.to_string());
        }
        if let Some(ce) = cause.downcast_ref::<CasperError>() {
            return classify_casper_error(ce);
        }
        if let Some(DeployError::DuplicateDeploy(_)) = cause.downcast_ref::<DeployError>() {
            return (StatusCode::CONFLICT, "duplicate_deploy", cause.to_string());
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

        SlashAuth(_) => (S::FORBIDDEN, "slash_auth_error", err.to_string()),

        // The node is answering about history it does not have, not failing:
        // a node restored from a sync anchor is still filling in below it, and
        // the same request may succeed once it has. 503 tells the caller to
        // retry; the 500 class would say the node is broken.
        BlockNotHeld(_) => (S::SERVICE_UNAVAILABLE, "block_not_held", err.to_string()),

        SigningError(_) => internal("signing_error"),
        KvStoreError(_) => internal("kv_store_error"),
        HistoryError(_) => internal("history_error"),
        RuntimeError(_) => internal("runtime_error"),
        SystemRuntimeError(_) => internal("system_runtime_error"),
        ReplayFailure(_) => internal("replay_failure"),
        StreamError(_) => internal("stream_error"),
        LockError(_) => internal("lock_error"),
        IncompatibleFinalizedFork(_) => internal("incompatible_finalized_fork"),
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
        | IfConditionTypeError { .. }
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
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.status().await }).await {
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
        (status = 409, description = "Deploy is already known (`duplicate_deploy`)", body = ApiErrorResponse),
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
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.deploy(request).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/explore-deploy",
    description = "Executes against the last finalized block post-state. Unfinalized DAG-tip state is not visible.",
    request_body = SimpleExploreDeployRequest,
    responses(
        (status = 200, description = "Exploratory deploy executed; returns channel data", body = RhoDataResponse),
        (status = 400, description = "Malformed request body, invalid Rholang term, or node is not read-only (`invalid_request_body`, `rholang_bad_term`, `readonly_node_required`)", body = ApiErrorResponse),
        (status = 422, description = "Term is structurally valid but failed execution (`rholang_execution_error`, `out_of_phlogistons`, `user_abort`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
        (status = 502, description = "External service failure (`external_service_error`)", body = ApiErrorResponse),
        (status = 503, description = "Observer query capacity is occupied (`observer_busy`); carries `Retry-After`", body = ApiErrorResponse),
        (status = 504, description = "Exploratory execution exceeded its deadline (`exploratory_timeout`)", body = ApiErrorResponse),
    ),
    tag = "Deployment"
)]
pub async fn explore_deploy_handler(
    State(app_state): State<AppState>,
    AppJson(request): AppJson<SimpleExploreDeployRequest>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(
        move || async move { web_api.exploratory_deploy(request.term, None, false).await },
    )
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
        (status = 503, description = "Observer query capacity is occupied (`observer_busy`); carries `Retry-After`", body = ApiErrorResponse),
        (status = 504, description = "Exploratory execution exceeded its deadline (`exploratory_timeout`)", body = ApiErrorResponse),
    ),
    tag = "Deployment"
)]
pub async fn explore_deploy_by_block_hash_handler(
    State(app_state): State<AppState>,
    AppJson(request): AppJson<ExploreDeployRequest>,
) -> Response {
    let block_hash = request.block_hash.trim().to_string();
    if block_hash.is_empty() {
        return AppError(eyre::Report::new(InvalidHashError(
            "blockHash is required".to_string(),
        )))
        .into_response();
    }

    let web_api = app_state.web_api.clone();
    match offload(move || async move {
        web_api
            .exploratory_deploy(request.term, Some(block_hash), request.use_pre_state_hash)
            .await
    })
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
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_blocks(1, view).await }).await {
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
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_block(hash, view).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

pub async fn offload<F, Fut, T>(make_fut: F) -> Result<T, eyre::Error>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = eyre::Result<T>>,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(make_fut())
    })
    .await
    {
        Ok(inner) => inner,
        Err(join_err) => Err(eyre::eyre!("handler task panicked: {}", join_err)),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;
    use casper::rust::api::block_api::{ExploratoryDeployBusyError, ExploratoryDeployTimeoutError};

    use super::{classify_error, retry_after_secs, AppError};

    #[test]
    fn exploratory_deploy_busy_is_service_unavailable() {
        let error = eyre::Report::new(ExploratoryDeployBusyError {
            retry_after_secs: 15,
        });
        let (status, kind, _) = classify_error(&error);

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(kind, "observer_busy");
        assert_eq!(retry_after_secs(&error), Some(15));
    }

    #[test]
    fn exploratory_deploy_busy_response_carries_retry_after() {
        let response = AppError(eyre::Report::new(ExploratoryDeployBusyError {
            retry_after_secs: 15,
        }))
        .into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            Some("15")
        );
    }

    #[test]
    fn exploratory_deploy_timeout_response_has_no_retry_after() {
        let response = AppError(eyre::Report::new(ExploratoryDeployTimeoutError {
            timeout_ms: 15_000,
        }))
        .into_response();

        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        assert!(response.headers().get(header::RETRY_AFTER).is_none());
    }

    #[test]
    fn exploratory_deploy_timeout_is_gateway_timeout() {
        let error = eyre::Report::new(ExploratoryDeployTimeoutError { timeout_ms: 15_000 });
        let (status, kind, _) = classify_error(&error);

        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(kind, "exploratory_timeout");
    }

    mod classification {
        use axum::http::StatusCode;
        use casper::rust::api::block_api::{
            BlockNotFoundError, DeployNotFoundError, DeployValidationError,
            ExploratoryDeployReadOnlyError, InvalidHashError, InvalidPublicKeyError,
            LatestBlockMessageError, NoNewDeploysError, ProposeReadOnlyError,
        };
        use casper::rust::casper::DeployError;
        use casper::rust::errors::CasperError;
        use rholang::rust::interpreter::errors::InterpreterError;

        use super::super::classify_error;

        fn classify(err: impl std::error::Error + Send + Sync + 'static) -> (StatusCode, String) {
            let (status, kind, _message) = classify_error(&eyre::Report::new(err));
            (status, kind.to_string())
        }

        #[test]
        fn typed_block_api_errors_map_to_stable_kinds() {
            assert_eq!(
                classify(DeployNotFoundError {
                    deploy_id: "aa".to_string(),
                }),
                (StatusCode::NOT_FOUND, "deploy_not_found".to_string())
            );
            assert_eq!(
                classify(BlockNotFoundError {
                    hash: "bb".to_string(),
                }),
                (StatusCode::NOT_FOUND, "block_not_found".to_string())
            );
            assert_eq!(
                classify(InvalidHashError("bad hash".to_string())),
                (StatusCode::BAD_REQUEST, "invalid_hash".to_string())
            );
            assert_eq!(
                classify(ExploratoryDeployReadOnlyError),
                (
                    StatusCode::BAD_REQUEST,
                    "readonly_node_required".to_string()
                )
            );
            assert_eq!(
                classify(InvalidPublicKeyError("bad key".to_string())),
                (StatusCode::BAD_REQUEST, "illegal_argument".to_string())
            );
            assert_eq!(
                classify(DeployValidationError {
                    message: "bad deploy".to_string(),
                }),
                (StatusCode::BAD_REQUEST, "illegal_argument".to_string())
            );
            assert_eq!(
                classify(ProposeReadOnlyError),
                (
                    StatusCode::BAD_REQUEST,
                    "readonly_node_required".to_string()
                )
            );
            assert_eq!(
                classify(NoNewDeploysError),
                (StatusCode::CONFLICT, "no_new_deploys".to_string())
            );
        }

        #[test]
        fn latest_block_message_errors_split_by_variant() {
            assert_eq!(
                classify(LatestBlockMessageError::NodeReadOnlyError),
                (
                    StatusCode::BAD_REQUEST,
                    "validator_node_required".to_string()
                )
            );
            assert_eq!(
                classify(LatestBlockMessageError::NoBlockMessageError),
                (StatusCode::NOT_FOUND, "block_not_found".to_string())
            );
        }

        #[test]
        fn duplicate_deploy_is_conflict() {
            let err = DeployError::duplicate_deploy(vec![1u8, 2].into());
            assert_eq!(
                classify(err),
                (StatusCode::CONFLICT, "duplicate_deploy".to_string())
            );
        }

        #[test]
        fn unknown_error_falls_back_to_internal_server_error() {
            let (status, kind, _) = classify_error(&eyre::eyre!("something odd"));
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(kind, "unknown_error");
        }

        #[test]
        fn casper_errors_map_to_stable_kinds() {
            let cases = vec![
                (
                    CasperError::CommError(comm::rust::errors::CommError::UnknownCommError(
                        "peer gone".to_string(),
                    )),
                    StatusCode::BAD_GATEWAY,
                    "comm_error",
                ),
                (
                    CasperError::SigningError("sig".to_string()),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "signing_error",
                ),
                (
                    CasperError::RuntimeError("rt".to_string()),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "runtime_error",
                ),
                (
                    CasperError::StreamError("st".to_string()),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "stream_error",
                ),
                (
                    CasperError::LockError("lk".to_string()),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "lock_error",
                ),
                (
                    CasperError::IncompatibleFinalizedFork("fork".to_string()),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "incompatible_finalized_fork",
                ),
                (
                    CasperError::Other("misc".to_string()),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "other_error",
                ),
                (
                    CasperError::BlockNotHeld(vec![0xab].into()),
                    StatusCode::SERVICE_UNAVAILABLE,
                    "block_not_held",
                ),
            ];

            for (err, expected_status, expected_kind) in cases {
                let (status, kind) = classify(err);
                assert_eq!(status, expected_status, "kind {expected_kind}");
                assert_eq!(kind, expected_kind);
            }
        }

        #[test]
        fn interpreter_bad_term_errors_are_bad_request() {
            for err in [
                InterpreterError::SyntaxError("boom".to_string()),
                InterpreterError::LexerError("boom".to_string()),
                InterpreterError::ParserError("boom".to_string()),
                InterpreterError::PatternReceiveError("boom".to_string()),
            ] {
                let (status, kind) = classify(CasperError::InterpreterError(err));
                assert_eq!(status, StatusCode::BAD_REQUEST);
                assert_eq!(kind, "rholang_bad_term");
            }
        }

        #[test]
        fn interpreter_illegal_argument_is_bad_request() {
            let (status, kind) = classify(CasperError::InterpreterError(
                InterpreterError::IllegalArgumentError("bad arg".to_string()),
            ));
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(kind, "illegal_argument");
        }

        #[test]
        fn interpreter_execution_failures_are_unprocessable() {
            assert_eq!(
                classify(CasperError::InterpreterError(
                    InterpreterError::OutOfPhlogistonsError
                )),
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "out_of_phlogistons".to_string()
                )
            );
            assert_eq!(
                classify(CasperError::InterpreterError(
                    InterpreterError::UserAbortError
                )),
                (StatusCode::UNPROCESSABLE_ENTITY, "user_abort".to_string())
            );
            for err in [
                InterpreterError::ReduceError("boom".to_string()),
                InterpreterError::MethodNotDefined {
                    method: "nth".to_string(),
                    other_type: "Int".to_string(),
                },
                InterpreterError::SubstituteError("boom".to_string()),
            ] {
                let (status, kind) = classify(CasperError::InterpreterError(err));
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
                assert_eq!(kind, "rholang_execution_error");
            }
        }

        #[test]
        fn interpreter_internal_errors_are_internal_server_error() {
            for err in [
                InterpreterError::BugFoundError("boom".to_string()),
                InterpreterError::SetupError("boom".to_string()),
            ] {
                let (status, kind) = classify(CasperError::InterpreterError(err));
                assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
                assert_eq!(kind, "interpreter_internal_error");
            }
        }

        #[test]
        fn interpreter_external_service_errors_are_bad_gateway() {
            for err in [
                InterpreterError::OpenAIError("api down".to_string()),
                InterpreterError::OllamaError("api down".to_string()),
            ] {
                let (status, kind) = classify(CasperError::InterpreterError(err));
                assert_eq!(status, StatusCode::BAD_GATEWAY);
                assert_eq!(kind, "external_service_error");
            }
        }

        #[test]
        fn interpreter_aggregate_error_joins_messages() {
            let err = CasperError::InterpreterError(InterpreterError::AggregateError {
                interpreter_errors: vec![
                    InterpreterError::ReduceError("first".to_string()),
                    InterpreterError::ReduceError("second".to_string()),
                ],
            });
            let (status, kind, message) = classify_error(&eyre::Report::new(err));
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(kind, "aggregate_error");
            assert!(
                message.contains("first") && message.contains("second"),
                "{message}"
            );
        }
    }

    mod extractors {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use axum::response::IntoResponse;
        use axum::routing::{get, post};
        use axum::Router;
        use tower::ServiceExt;

        use super::super::{offload, AppJson, AppPath, AppQuery};

        #[derive(serde::Deserialize)]
        struct TypedBody {
            #[allow(dead_code)]
            value: i64,
        }

        #[derive(serde::Deserialize)]
        struct TypedQuery {
            #[allow(dead_code)]
            count: i64,
        }

        fn router() -> Router {
            Router::new()
                .route(
                    "/json",
                    post(|AppJson(_body): AppJson<TypedBody>| async { "ok".into_response() }),
                )
                .route(
                    "/path/{id}",
                    get(|AppPath(_id): AppPath<i64>| async { "ok".into_response() }),
                )
                .route(
                    "/query",
                    get(|AppQuery(_q): AppQuery<TypedQuery>| async { "ok".into_response() }),
                )
        }

        async fn body_json(response: axum::response::Response) -> serde_json::Value {
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            serde_json::from_slice(&bytes).unwrap()
        }

        #[tokio::test]
        async fn app_json_accepts_valid_body_and_rejects_malformed_body_as_json() {
            let ok = router()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/json")
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"value": 3}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(ok.status(), StatusCode::OK);

            let bad = router()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/json")
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"value": "not a number"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(bad.status().is_client_error());
            let json = body_json(bad).await;
            assert_eq!(json["error"], "invalid_request_body");
            assert!(json["message"].as_str().is_some());
        }

        #[tokio::test]
        async fn app_path_rejects_non_integer_segment_as_json() {
            let ok = router()
                .oneshot(
                    Request::builder()
                        .uri("/path/42")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(ok.status(), StatusCode::OK);

            let bad = router()
                .oneshot(
                    Request::builder()
                        .uri("/path/not-a-number")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
            let json = body_json(bad).await;
            assert_eq!(json["error"], "invalid_path_parameter");
        }

        #[tokio::test]
        async fn app_query_rejects_unparsable_query_as_json() {
            let ok = router()
                .oneshot(
                    Request::builder()
                        .uri("/query?count=5")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(ok.status(), StatusCode::OK);

            let bad = router()
                .oneshot(
                    Request::builder()
                        .uri("/query?count=many")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
            let json = body_json(bad).await;
            assert_eq!(json["error"], "invalid_query_parameter");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn offload_propagates_ok_and_err_results() {
            let ok: Result<i32, eyre::Error> = offload(|| async { Ok(5) }).await;
            assert_eq!(ok.unwrap(), 5);

            let err: Result<i32, eyre::Error> =
                offload(|| async { Err(eyre::eyre!("expected failure")) }).await;
            assert!(err.unwrap_err().to_string().contains("expected failure"));
        }
    }
}
