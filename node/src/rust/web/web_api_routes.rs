use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;

use crate::rust::api::serde_types::block_info::BlockInfoSerde;
use crate::rust::api::web_api::{
    DataAtNameByBlockHashRequest, DeployResponse, PrepareRequest, PrepareResponse, RhoDataResponse,
    WebApi,
};
use crate::rust::web::shared_handlers::{
    self, offload, ApiErrorResponse, AppError, AppJson, AppPath, AppQuery, AppState,
};

#[derive(Debug, Deserialize)]
pub struct ViewQuery {
    pub view: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BlockHashQuery {
    pub block_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeployerQuery {
    /// Hex-encoded deployer public key. Empty/absent returns all pending
    /// deploys regardless of deployer.
    pub deployer: Option<String>,
}

pub struct WebApiRoutes;

impl WebApiRoutes {
    pub fn create_router() -> Router<AppState> {
        Router::new()
            .route("/status", get(shared_handlers::status_handler))
            .route("/prepare-deploy", get(prepare_deploy_get_handler))
            .route("/prepare-deploy", post(prepare_deploy_post_handler))
            .route("/deploy", post(shared_handlers::deploy_handler))
            .route(
                "/explore-deploy",
                post(shared_handlers::explore_deploy_handler),
            )
            .route(
                "/explore-deploy-by-block-hash",
                post(shared_handlers::explore_deploy_by_block_hash_handler),
            )
            .route(
                "/data-at-name-by-block-hash",
                post(data_at_name_by_block_hash_handler),
            )
            .route("/last-finalized-block", get(last_finalized_block_handler))
            .route("/block/{hash}", get(shared_handlers::get_block_handler))
            .route("/blocks", get(shared_handlers::get_blocks_handler))
            .route("/blocks/{start}/{end}", get(get_blocks_by_heights_handler))
            .route("/blocks/{depth}", get(get_blocks_by_depth_handler))
            .route("/deploy/{deploy_id}", get(find_deploy_handler))
            .route("/is-finalized/{hash}", get(is_finalized_handler))
            .route(
                "/deploy-finalization-status/{deploy_sig_hex}",
                get(deploy_finalization_status_handler),
            )
            .route("/pending-deploys", get(pending_deploys_handler))
            .route("/balance/{address}", get(balance_handler))
            .route("/registry/{uri}", get(registry_handler))
            .route("/validators", get(validators_handler))
            .route("/validator/{pubkey}", get(validator_handler))
            .route("/epoch", get(epoch_handler))
            .route("/epoch/rewards", get(epoch_rewards_handler))
            .route("/estimate-cost", post(estimate_cost_handler))
            .route("/bond-status/{pubkey}", get(bond_status_handler))
    }
}

#[utoipa::path(
    get,
    path = "/api/prepare-deploy",
    responses(
        (status = 200, description = "Validator's next sequence number (`seqNumber`). This is the validator's internal block counter, not a field the deployer sets.", body = PrepareResponse),
        (status = 500, description = "Node-side failure (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn prepare_deploy_get_handler(State(app_state): State<AppState>) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.prepare_deploy(None).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/api/prepare-deploy",
    request_body = PrepareRequest,
    responses(
        (status = 200, description = "Next deploy sequence number. Legacy protocols can also return pre-generated unforgeable names", body = PrepareResponse),
        (status = 400, description = "Malformed request body or invalid deployer hex (`invalid_request_body`, `invalid_hash`)", body = ApiErrorResponse),
        (status = 409, description = "The active protocol does not support key-and-timestamp private-name preview (`private_name_preview_unavailable`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn prepare_deploy_post_handler(
    State(app_state): State<AppState>,
    AppJson(request): AppJson<PrepareRequest>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.prepare_deploy(Some(request)).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/api/data-at-name-by-block-hash",
    request_body = DataAtNameByBlockHashRequest,
    responses(
        (status = 200, description = "Channel data at the given name in the specified block", body = RhoDataResponse),
        (status = 400, description = "Malformed request body or invalid block hash (`invalid_request_body`, `invalid_hash`)", body = ApiErrorResponse),
        (status = 404, description = "Specified block not found (`block_not_found`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn data_at_name_by_block_hash_handler(
    State(app_state): State<AppState>,
    AppJson(request): AppJson<DataAtNameByBlockHashRequest>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_data_at_par(request).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/last-finalized-block",
    params(
        ("view" = Option<String>, Query, description = "Response view: `full` (default) includes deploy list; `summary` returns block header only"),
    ),
    responses(
        (status = 200, description = "The current last-finalized block", body = BlockInfoSerde),
        (status = 400, description = "Invalid query parameter (`invalid_query_parameter`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure — LFB not yet available (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn last_finalized_block_handler(
    State(app_state): State<AppState>,
    AppQuery(query): AppQuery<ViewQuery>,
) -> Response {
    use crate::rust::api::web_api::ViewMode;

    let view = match query.view.as_deref() {
        Some("summary") => ViewMode::Summary,
        _ => ViewMode::Full,
    };
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.last_finalized_block(view).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/blocks/{start}/{end}",
    params(
        ("start" = i64, Path, description = "Start block height (inclusive)"),
        ("end" = i64, Path, description = "End block height (inclusive); clamped to the configured maximum range"),
        ("view" = Option<String>, Query, description = "Response view: `summary` (default) returns block headers only; `full` includes deploy list"),
    ),
    responses(
        (status = 200, description = "Blocks in the requested height range; may be empty if no blocks exist yet", body = Vec<BlockInfoSerde>),
        (status = 400, description = "Non-integer path segment (`invalid_path_parameter`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`, `history_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn get_blocks_by_heights_handler(
    State(app_state): State<AppState>,
    AppPath((start, end)): AppPath<(i64, i64)>,
    AppQuery(query): AppQuery<ViewQuery>,
) -> Response {
    use crate::rust::api::web_api::ViewMode;

    let view = match query.view.as_deref() {
        Some("full") => ViewMode::Full,
        _ => ViewMode::Summary,
    };
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_blocks_by_heights(start, end, view).await })
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/blocks/{depth}",
    params(
        ("depth" = i32, Path, description = "Number of most-recent blocks to return; clamped to the configured maximum"),
        ("view" = Option<String>, Query, description = "Response view: `summary` (default) returns block headers only; `full` includes deploy list"),
    ),
    responses(
        (status = 200, description = "The `depth` most-recent blocks; may be fewer if the chain is shorter", body = Vec<BlockInfoSerde>),
        (status = 400, description = "Non-integer path segment (`invalid_path_parameter`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`, `history_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn get_blocks_by_depth_handler(
    State(app_state): State<AppState>,
    AppPath(depth): AppPath<i32>,
    AppQuery(query): AppQuery<ViewQuery>,
) -> Response {
    use crate::rust::api::web_api::ViewMode;

    let view = match query.view.as_deref() {
        Some("full") => ViewMode::Full,
        _ => ViewMode::Summary,
    };
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_blocks(depth, view).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/deploy/{deploy_id}",
    params(
        ("deploy_id" = String, Path, description = "Hex-encoded deploy ID"),
        ("view" = Option<String>, Query, description = "Response view: `full` (default) returns all fields including term and transfers; `summary` returns core fields only"),
    ),
    responses(
        (status = 200, description = "Deploy information", body = DeployResponse),
        (status = 400, description = "Deploy ID is not valid hex (`invalid_hash`)", body = ApiErrorResponse),
        (status = 404, description = "No deploy with this ID found in any finalized block (`deploy_not_found`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn find_deploy_handler(
    State(app_state): State<AppState>,
    AppPath(deploy_id): AppPath<String>,
    AppQuery(query): AppQuery<ViewQuery>,
) -> Response {
    use crate::rust::api::web_api::ViewMode;

    let view = match query.view.as_deref() {
        Some("summary") => ViewMode::Summary,
        _ => ViewMode::Full,
    };

    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.find_deploy(deploy_id, view).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/is-finalized/{hash}",
    params(
        ("hash" = String, Path, description = "Full 64-char hex block hash to check"),
    ),
    responses(
        (status = 200, description = "`true` if the block is finalized, `false` if it is known but not yet finalized", body = bool),
        (status = 400, description = "Hash contains non-hex characters (`invalid_hash`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn is_finalized_handler(
    State(app_state): State<AppState>,
    AppPath(hash): AppPath<String>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.is_finalized(hash).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

use crate::rust::api::web_api::{
    BalanceResponse, EpochResponse, PendingDeploysJson, RegistryResponse, ValidatorsResponse,
};

#[utoipa::path(
    get,
    path = "/api/deploy-finalization-status/{deploy_sig_hex}",
    params(
        ("deploy_sig_hex" = String, Path, description = "Hex-encoded deploy signature (with or without `0x` prefix)"),
    ),
    responses(
        (
            status = 200,
            description = "Canonical-state finalization status for the deploy. Prefer this over block-hash finalization polling — a block can finalize while some of its deploys' effects are dropped during merge",
            body = crate::rust::api::web_api::DeployFinalizationStatusJson
        ),
        (status = 400, description = "Signature is not valid hex (`invalid_hash`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn deploy_finalization_status_handler(
    State(app_state): State<AppState>,
    AppPath(deploy_sig_hex): AppPath<String>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.deploy_finalization_status(deploy_sig_hex).await })
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/pending-deploys",
    params(
        ("deployer" = Option<String>, Query, description = "Hex-encoded deployer public key; omit to return all pending deploys"),
    ),
    responses(
        (
            status = 200,
            description = "Bulk snapshot of the node-local pending-deploy queue (deploy_storage + rejected_deploy_buffer). Observers always answer empty",
            body = PendingDeploysJson,
        ),
        (status = 400, description = "Invalid deployer hex (`invalid_public_key`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn pending_deploys_handler(
    State(app_state): State<AppState>,
    AppQuery(query): AppQuery<DeployerQuery>,
) -> Response {
    pending_deploys_logic(app_state.web_api.clone(), query).await
}

/// Core of `pending_deploys_handler`, split out so tests can exercise the
/// exact handler path without constructing a full `AppState`.
async fn pending_deploys_logic(
    web_api: Arc<dyn WebApi + Send + Sync>,
    query: DeployerQuery,
) -> Response {
    match offload(move || async move { web_api.get_pending_deploys(query.deployer).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/balance/{address}",
    params(
        ("address" = String, Path, description = "REV wallet address (Base58-encoded, starts with `1111`)"),
        ("block_hash" = Option<String>, Query, description = "Block hash to query against; defaults to the last-finalized block"),
    ),
    responses(
        (status = 200, description = "REV balance for the address", body = BalanceResponse),
        (status = 400, description = "Invalid block hash or node is not read-only (`invalid_hash`, `readonly_node_required`)", body = ApiErrorResponse),
        (status = 404, description = "Specified block not found (`block_not_found`)", body = ApiErrorResponse),
        (status = 422, description = "Exploratory deploy execution failed (`rholang_execution_error`, `out_of_phlogistons`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
    ),
    tag = "Query"
)]
pub async fn balance_handler(
    State(app_state): State<AppState>,
    AppPath(address): AppPath<String>,
    AppQuery(query): AppQuery<BlockHashQuery>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_balance(address, query.block_hash).await }).await
    {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/registry/{uri}",
    params(
        ("uri" = String, Path, description = "Registry URI (e.g. `rho:id:...`)"),
        ("block_hash" = Option<String>, Query, description = "Block hash to query against; defaults to the last-finalized block"),
    ),
    responses(
        (status = 200, description = "Rholang value stored at the registry URI", body = RegistryResponse),
        (status = 400, description = "Invalid block hash or node is not read-only (`invalid_hash`, `readonly_node_required`)", body = ApiErrorResponse),
        (status = 404, description = "Specified block not found (`block_not_found`)", body = ApiErrorResponse),
        (status = 422, description = "Exploratory deploy execution failed (`rholang_execution_error`, `out_of_phlogistons`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
    ),
    tag = "Query"
)]
pub async fn registry_handler(
    State(app_state): State<AppState>,
    AppPath(uri): AppPath<String>,
    AppQuery(query): AppQuery<BlockHashQuery>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_registry(uri, query.block_hash).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/validators",
    params(
        ("block_hash" = Option<String>, Query, description = "Block hash to query against; defaults to the last-finalized block"),
    ),
    responses(
        (status = 200, description = "Active validator set with stake weights", body = ValidatorsResponse),
        (status = 400, description = "Invalid block hash or node is not read-only (`invalid_hash`, `readonly_node_required`)", body = ApiErrorResponse),
        (status = 404, description = "Specified block not found (`block_not_found`)", body = ApiErrorResponse),
        (status = 422, description = "Exploratory deploy execution failed (`rholang_execution_error`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
    ),
    tag = "Query"
)]
pub async fn validators_handler(
    State(app_state): State<AppState>,
    AppQuery(query): AppQuery<BlockHashQuery>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_validators(query.block_hash).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/epoch",
    params(
        ("block_hash" = Option<String>, Query, description = "Block hash to derive epoch from (defaults to LFB)"),
    ),
    responses(
        (status = 200, description = "Current epoch number and boundary block height", body = EpochResponse),
        (status = 400, description = "Invalid block hash (`invalid_hash`)", body = ApiErrorResponse),
        (status = 404, description = "Specified block not found (`block_not_found`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "Query"
)]
pub async fn epoch_handler(
    State(app_state): State<AppState>,
    AppQuery(query): AppQuery<BlockHashQuery>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_epoch(query.block_hash).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

use crate::rust::api::web_api::{
    BondStatusResponse as BondStatusResp, EpochRewardsResponse, EstimateCostRequest,
    EstimateCostResponse, ValidatorStatusResponse,
};

#[utoipa::path(
    post,
    path = "/api/estimate-cost",
    request_body = EstimateCostRequest,
    params(
        ("block_hash" = Option<String>, Query, description = "Block hash to query against; defaults to the last-finalized block"),
    ),
    responses(
        (status = 200, description = "Estimated phlogiston (gas) cost for the given Rholang term", body = EstimateCostResponse),
        (status = 400, description = "Malformed request body, invalid Rholang term, invalid deployer key, or invalid block hash (`invalid_request_body`, `illegal_argument`, `rholang_bad_term`, `invalid_hash`, `readonly_node_required`)", body = ApiErrorResponse),
        (status = 404, description = "Specified block not found (`block_not_found`)", body = ApiErrorResponse),
        (status = 422, description = "Term is structurally valid but failed execution (`rholang_execution_error`, `out_of_phlogistons`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
    ),
    tag = "Query"
)]
pub async fn estimate_cost_handler(
    State(app_state): State<AppState>,
    AppQuery(query): AppQuery<BlockHashQuery>,
    AppJson(request): AppJson<EstimateCostRequest>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move {
        web_api
            .estimate_cost(request.term, query.block_hash, request.deployer)
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
    path = "/api/epoch/rewards",
    params(
        ("block_hash" = Option<String>, Query, description = "Block hash to query against; defaults to the last-finalized block"),
    ),
    responses(
        (status = 200, description = "Per-validator reward amounts for the current epoch", body = EpochRewardsResponse),
        (status = 400, description = "Invalid block hash or node is not read-only (`invalid_hash`, `readonly_node_required`)", body = ApiErrorResponse),
        (status = 404, description = "Specified block not found (`block_not_found`)", body = ApiErrorResponse),
        (status = 422, description = "Exploratory deploy execution failed, e.g. arithmetic overflow due to node desync (`rholang_execution_error`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
    ),
    tag = "Query"
)]
pub async fn epoch_rewards_handler(
    State(app_state): State<AppState>,
    AppQuery(query): AppQuery<BlockHashQuery>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_epoch_rewards(query.block_hash).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/validator/{pubkey}",
    params(
        ("pubkey" = String, Path, description = "Validator secp256k1 public key as a 65-byte uncompressed hex string"),
        ("block_hash" = Option<String>, Query, description = "Block hash to query against; defaults to the last-finalized block"),
    ),
    responses(
        (status = 200, description = "Validator bond status and stake at the given block", body = ValidatorStatusResponse),
        (status = 400, description = "Invalid public key or block hash, or node is not read-only (`illegal_argument`, `invalid_hash`, `readonly_node_required`)", body = ApiErrorResponse),
        (status = 404, description = "Specified block not found (`block_not_found`)", body = ApiErrorResponse),
        (status = 422, description = "Exploratory deploy execution failed (`rholang_execution_error`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
    ),
    tag = "Query"
)]
pub async fn validator_handler(
    State(app_state): State<AppState>,
    AppPath(pubkey): AppPath<String>,
    AppQuery(query): AppQuery<BlockHashQuery>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_validator(pubkey, query.block_hash).await })
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/bond-status/{pubkey}",
    params(
        ("pubkey" = String, Path, description = "Validator secp256k1 public key as a 65-byte uncompressed hex string"),
    ),
    responses(
        (status = 200, description = "Whether the key is currently bonded as a validator", body = BondStatusResp),
        (status = 400, description = "Invalid public key format (`illegal_argument`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "Query"
)]
pub async fn bond_status_handler(
    State(app_state): State<AppState>,
    AppPath(pubkey): AppPath<String>,
) -> Response {
    let web_api = app_state.web_api.clone();
    match offload(move || async move { web_api.get_bond_status(pubkey).await }).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::StatusCode;
    use tower::ServiceExt;

    use super::*;
    use crate::rust::api::web_api::{
        ApiStatus, DataAtNameByBlockHashRequest, DeployRequest, DeployResponse, RhoDataResponse,
        ViewMode, WebApi,
    };

    /// Stub WebApi that returns sample DeployResponse for testing.
    struct StubWebApi;

    pub(super) fn sample_deploy_response(view: ViewMode) -> DeployResponse {
        let is_full = view == ViewMode::Full;
        DeployResponse {
            deploy_id: "abc123def".to_string(),
            block_hash: "7bf8abc123".to_string(),
            block_number: 52331,
            timestamp: 1770028092477,
            cost: 100,
            errored: false,
            is_finalized: true,
            finalization_state: "Finalized".to_string(),
            rejection_count: 0,
            deployer: if is_full {
                Some("0487def456".to_string())
            } else {
                None
            },
            term: if is_full {
                Some("new ret in { ret!(42) }".to_string())
            } else {
                None
            },
            system_deploy_error: if is_full { Some(String::new()) } else { None },
            sig_algorithm: if is_full {
                Some("secp256k1".to_string())
            } else {
                None
            },
            valid_after_block_number: if is_full { Some(0) } else { None },
            transfers: if is_full { Some(vec![]) } else { None },
        }
    }

    #[async_trait::async_trait]
    impl WebApi for StubWebApi {
        async fn status(&self) -> eyre::Result<ApiStatus> { unimplemented!() }
        async fn prepare_deploy(
            &self,
            _: Option<crate::rust::api::web_api::PrepareRequest>,
        ) -> eyre::Result<crate::rust::api::web_api::PrepareResponse> {
            unimplemented!()
        }
        async fn deploy(&self, _: DeployRequest) -> eyre::Result<String> { unimplemented!() }
        async fn get_data_at_par(
            &self,
            _: DataAtNameByBlockHashRequest,
        ) -> eyre::Result<RhoDataResponse> {
            unimplemented!()
        }
        async fn last_finalized_block(
            &self,
            _: ViewMode,
        ) -> eyre::Result<crate::rust::api::serde_types::block_info::BlockInfoSerde> {
            unimplemented!()
        }
        async fn get_block(
            &self,
            _: String,
            _: ViewMode,
        ) -> eyre::Result<crate::rust::api::serde_types::block_info::BlockInfoSerde> {
            unimplemented!()
        }
        async fn get_blocks(
            &self,
            _: i32,
            _: ViewMode,
        ) -> eyre::Result<Vec<crate::rust::api::serde_types::block_info::BlockInfoSerde>> {
            unimplemented!()
        }
        async fn find_deploy(&self, _: String, view: ViewMode) -> eyre::Result<DeployResponse> {
            Ok(sample_deploy_response(view))
        }
        async fn exploratory_deploy(
            &self,
            _: String,
            _: Option<String>,
            _: bool,
        ) -> eyre::Result<RhoDataResponse> {
            unimplemented!()
        }
        async fn get_blocks_by_heights(
            &self,
            _: i64,
            _: i64,
            _: ViewMode,
        ) -> eyre::Result<Vec<crate::rust::api::serde_types::block_info::BlockInfoSerde>> {
            unimplemented!()
        }
        async fn is_finalized(&self, _: String) -> eyre::Result<bool> { unimplemented!() }
        async fn deploy_finalization_status(
            &self,
            _: String,
        ) -> eyre::Result<crate::rust::api::web_api::DeployFinalizationStatusJson> {
            unimplemented!()
        }
        async fn get_pending_deploys(
            &self,
            deployer: Option<String>,
        ) -> eyre::Result<crate::rust::api::web_api::PendingDeploysJson> {
            use crate::rust::api::web_api::{PendingDeployJson, PendingDeploysJson};

            let deploys = match deployer.as_deref() {
                Some(pk) if !pk.is_empty() => vec![PendingDeployJson {
                    term: "for (x <- ch) { return!(x) }".to_string(),
                    timestamp: 1770028092477,
                    valid_after_block_number: 0,
                    shard_id: String::new(),
                    deployer: pk.to_string(),
                    deploy_id: "aa11".to_string(),
                    sig: "aa11".to_string(),
                    sig_algorithm: "secp256k1".to_string(),
                    expiration_timestamp: None,
                    is_rejected: false,
                }],
                _ => vec![
                    PendingDeployJson {
                        term: "Nil".to_string(),
                        timestamp: 1770028092477,
                        valid_after_block_number: 0,
                        shard_id: String::new(),
                        deployer: "0487def456".to_string(),
                        deploy_id: "aa11".to_string(),
                        sig: "aa11".to_string(),
                        sig_algorithm: "secp256k1".to_string(),
                        expiration_timestamp: None,
                        is_rejected: false,
                    },
                    PendingDeployJson {
                        term: "@0!(42)".to_string(),
                        timestamp: 1770028092478,
                        valid_after_block_number: 0,
                        shard_id: String::new(),
                        deployer: "0499abc789".to_string(),
                        deploy_id: "bb22".to_string(),
                        sig: "bb22".to_string(),
                        sig_algorithm: "secp256k1".to_string(),
                        expiration_timestamp: None,
                        is_rejected: true,
                    },
                ],
            };
            let total_available = deploys.len() as u32;
            Ok(PendingDeploysJson {
                deploys,
                total_available,
            })
        }
        async fn get_balance(
            &self,
            _: String,
            _: Option<String>,
        ) -> eyre::Result<crate::rust::api::web_api::BalanceResponse> {
            unimplemented!()
        }
        async fn get_registry(
            &self,
            _: String,
            _: Option<String>,
        ) -> eyre::Result<crate::rust::api::web_api::RegistryResponse> {
            unimplemented!()
        }
        async fn get_validators(
            &self,
            _: Option<String>,
        ) -> eyre::Result<crate::rust::api::web_api::ValidatorsResponse> {
            unimplemented!()
        }
        async fn get_epoch(
            &self,
            _: Option<String>,
        ) -> eyre::Result<crate::rust::api::web_api::EpochResponse> {
            unimplemented!()
        }
        async fn estimate_cost(
            &self,
            _: String,
            _: Option<String>,
            _: Option<String>,
        ) -> eyre::Result<crate::rust::api::web_api::EstimateCostResponse> {
            unimplemented!()
        }
        async fn get_epoch_rewards(
            &self,
            _: Option<String>,
        ) -> eyre::Result<crate::rust::api::web_api::EpochRewardsResponse> {
            unimplemented!()
        }
        async fn get_validator(
            &self,
            _: String,
            _: Option<String>,
        ) -> eyre::Result<crate::rust::api::web_api::ValidatorStatusResponse> {
            unimplemented!()
        }
        async fn get_bond_status(
            &self,
            _: String,
        ) -> eyre::Result<crate::rust::api::web_api::BondStatusResponse> {
            unimplemented!()
        }
    }

    async fn test_find_deploy_handler(
        State(web_api): State<Arc<dyn WebApi + Send + Sync>>,
        AppPath(deploy_id): AppPath<String>,
        AppQuery(query): AppQuery<ViewQuery>,
    ) -> Response {
        let view = match query.view.as_deref() {
            Some("summary") => ViewMode::Summary,
            _ => ViewMode::Full,
        };
        match web_api.find_deploy(deploy_id, view).await {
            Ok(response) => Json(response).into_response(),
            Err(e) => AppError(e).into_response(),
        }
    }

    fn test_router() -> Router {
        let web_api: Arc<dyn WebApi + Send + Sync> = Arc::new(StubWebApi);
        Router::new()
            .route("/deploy/{deploy_id}", get(test_find_deploy_handler))
            .route(
                "/pending-deploys",
                get(
                    move |State(web_api): State<Arc<dyn WebApi + Send + Sync>>,
                          AppQuery(query): AppQuery<DeployerQuery>| {
                        pending_deploys_logic(web_api, query)
                    },
                ),
            )
            .with_state(web_api)
    }

    async fn body_to_string(body: Body) -> String {
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn test_find_deploy_returns_full_response_by_default() {
        let app = test_router();

        let request: axum::http::Request<Body> = axum::http::Request::builder()
            .uri("/deploy/abc123def")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = body_to_string(response.into_body()).await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();

        // Core fields always present
        assert_eq!(json["deployId"], "abc123def");
        assert_eq!(json["blockHash"], "7bf8abc123");
        assert_eq!(json["blockNumber"], 52331);
        assert_eq!(json["timestamp"], 1770028092477i64);
        assert_eq!(json["cost"], 100);
        assert_eq!(json["errored"], false);
        assert_eq!(json["isFinalized"], true);

        // Full view includes deploy execution details
        assert_eq!(json["deployer"], "0487def456");
        assert!(json.get("term").is_some());
        // D3 (DR-9): the deploy response no longer carries phloPrice / phloLimit.
        assert!(json.get("phloPrice").is_none());
        assert!(json.get("phloLimit").is_none());
        assert!(json.get("sigAlgorithm").is_some());
        assert!(json.get("transfers").is_some());
    }

    #[tokio::test]
    async fn test_find_deploy_returns_summary_response() {
        let app = test_router();

        let request: axum::http::Request<Body> = axum::http::Request::builder()
            .uri("/deploy/abc123def?view=summary")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = body_to_string(response.into_body()).await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();

        // Core fields present
        assert_eq!(json["deployId"], "abc123def");
        assert_eq!(json["blockHash"], "7bf8abc123");
        assert_eq!(json["blockNumber"], 52331);
        assert_eq!(json["cost"], 100);
        assert_eq!(json["isFinalized"], true);

        // Full-only fields omitted
        assert!(json.get("deployer").is_none());
        assert!(json.get("term").is_none());
        assert!(json.get("phloPrice").is_none());
        assert!(json.get("phloLimit").is_none());
        assert!(json.get("sigAlgorithm").is_none());
        assert!(json.get("transfers").is_none());
    }

    #[tokio::test]
    async fn test_find_deploy_unknown_view_defaults_to_full() {
        let app = test_router();

        let request: axum::http::Request<Body> = axum::http::Request::builder()
            .uri("/deploy/abc123def?view=unknown")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = body_to_string(response.into_body()).await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();

        // Unknown view falls back to full
        assert!(json.get("deployer").is_some());
        assert!(json.get("term").is_some());
    }

    #[tokio::test]
    async fn test_pending_deploys_returns_all_when_no_deployer() {
        let app = test_router();

        let request: axum::http::Request<Body> = axum::http::Request::builder()
            .uri("/pending-deploys")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = body_to_string(response.into_body()).await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(json["deploys"].as_array().unwrap().len(), 2);
        assert_eq!(json["totalAvailable"], 2);
        assert_eq!(json["deploys"][0]["isRejected"], false);
        assert_eq!(json["deploys"][1]["isRejected"], true);
        assert_eq!(json["deploys"][0]["deployer"], "0487def456");
        assert_eq!(json["deploys"][1]["deployer"], "0499abc789");
        assert_eq!(json["deploys"][0]["sigAlgorithm"], "secp256k1");
    }

    #[tokio::test]
    async fn test_pending_deploys_filters_by_deployer() {
        let app = test_router();

        let request: axum::http::Request<Body> = axum::http::Request::builder()
            .uri("/pending-deploys?deployer=0487def456")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = body_to_string(response.into_body()).await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(json["deploys"].as_array().unwrap().len(), 1);
        assert_eq!(json["totalAvailable"], 1);
        assert_eq!(json["deploys"][0]["deployer"], "0487def456");
    }
}

#[cfg(test)]
mod router_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use casper::rust::api::block_api::InvalidHashError;
    use casper::rust::api::block_report_api::BlockReportAPI;
    use casper::rust::engine::engine_cell::EngineCell;
    use casper::rust::report_store::ReportStore;
    use casper::rust::safety_oracle::CliqueOracleImpl;
    use comm::rust::peer_node::{NodeIdentifier, PeerNode};
    use comm::rust::rp::connect::ConnectionsCell;
    use comm::rust::rp::rp_conf::{RPConf, RPConfCell};
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
    use tower::ServiceExt;

    use super::*;
    use crate::rust::api::admin_web_api::AdminWebApi;
    use crate::rust::api::serde_types::light_block_info::LightBlockInfoSerde;
    use crate::rust::api::web_api::{
        ApiStatus, BalanceResponse, BondStatusResponse, DeployFinalizationStatusJson,
        DeployRequest, DeployerIdentity, EpochResponse, EpochRewardsResponse, EstimateCostResponse,
        PendingDeploysJson, PrepareResponse, RegistryResponse, RhoExpr, ValidatorInfo,
        ValidatorStatusResponse, ValidatorsResponse, VersionInfo, ViewMode, WebApi,
    };

    struct StubAdminWebApi;

    #[async_trait::async_trait]
    impl AdminWebApi for StubAdminWebApi {
        async fn propose(&self) -> eyre::Result<String> { Ok("proposed".to_string()) }

        async fn propose_result(&self) -> eyre::Result<String> { Ok("result".to_string()) }
    }

    struct StubNodeDiscovery;

    #[async_trait::async_trait]
    impl comm::rust::discovery::node_discovery::NodeDiscovery for StubNodeDiscovery {
        async fn discover(&self) -> Result<(), comm::rust::errors::CommError> { Ok(()) }

        fn peers(&self) -> Result<Vec<PeerNode>, comm::rust::errors::CommError> { Ok(vec![]) }

        fn remove_peer(&self, _peer: &PeerNode) -> Result<(), comm::rust::errors::CommError> {
            Ok(())
        }
    }

    fn light_block() -> LightBlockInfoSerde {
        LightBlockInfoSerde::from(models::casper::LightBlockInfo::default())
    }

    fn rho_data_response() -> RhoDataResponse {
        RhoDataResponse {
            expr: vec![RhoExpr::ExprInt { data: 11 }],
            block: light_block(),
            cost: 7,
        }
    }

    fn block_info() -> BlockInfoSerde { BlockInfoSerde::from(models::casper::BlockInfo::default()) }

    struct CannedWebApi;

    #[async_trait::async_trait]
    impl WebApi for CannedWebApi {
        async fn status(&self) -> eyre::Result<ApiStatus> {
            Ok(ApiStatus {
                version: VersionInfo {
                    api: "1".to_string(),
                    node: "test-node".to_string(),
                },
                address: "f1r3fly://node".to_string(),
                network_id: "testnet".to_string(),
                shard_id: "root".to_string(),
                peers: 1,
                nodes: 2,
                min_phlo_price: 1,
                peer_list: vec![],
                native_token_name: "F1R3".to_string(),
                native_token_symbol: "F1R3".to_string(),
                native_token_decimals: 8,
                last_finalized_block_number: 5,
                is_validator: false,
                is_read_only: true,
                is_ready: true,
                current_epoch: 0,
                epoch_length: 100,
            })
        }

        async fn prepare_deploy(
            &self,
            request: Option<crate::rust::api::web_api::PrepareRequest>,
        ) -> eyre::Result<PrepareResponse> {
            Ok(PrepareResponse {
                names: request.map(|r| vec![r.deployer]).unwrap_or_default(),
                seq_number: 9,
            })
        }

        async fn deploy(&self, _: DeployRequest) -> eyre::Result<String> {
            Ok("deploy-accepted".to_string())
        }

        async fn get_data_at_par(
            &self,
            _: DataAtNameByBlockHashRequest,
        ) -> eyre::Result<RhoDataResponse> {
            Ok(rho_data_response())
        }

        async fn last_finalized_block(&self, _: ViewMode) -> eyre::Result<BlockInfoSerde> {
            Ok(block_info())
        }

        async fn get_block(&self, _: String, _: ViewMode) -> eyre::Result<BlockInfoSerde> {
            Ok(block_info())
        }

        async fn get_blocks(&self, depth: i32, _: ViewMode) -> eyre::Result<Vec<BlockInfoSerde>> {
            Ok((0..depth.min(3)).map(|_| block_info()).collect())
        }

        async fn find_deploy(
            &self,
            _: String,
            view: ViewMode,
        ) -> eyre::Result<crate::rust::api::web_api::DeployResponse> {
            Ok(super::tests::sample_deploy_response(view))
        }

        async fn exploratory_deploy(
            &self,
            _: String,
            _: Option<String>,
            _: bool,
        ) -> eyre::Result<RhoDataResponse> {
            Ok(rho_data_response())
        }

        async fn get_blocks_by_heights(
            &self,
            _: i64,
            _: i64,
            _: ViewMode,
        ) -> eyre::Result<Vec<BlockInfoSerde>> {
            Ok(vec![block_info()])
        }

        async fn is_finalized(&self, _: String) -> eyre::Result<bool> { Ok(true) }

        async fn deploy_finalization_status(
            &self,
            _: String,
        ) -> eyre::Result<DeployFinalizationStatusJson> {
            Ok(DeployFinalizationStatusJson {
                state: "Finalized".to_string(),
                rejection_count: 0,
                latest_block_hash: Some("aa".to_string()),
                finalized_floor_hash: Some("bb".to_string()),
                finalized_floor_height: Some(1),
            })
        }

        async fn get_pending_deploys(&self, _: Option<String>) -> eyre::Result<PendingDeploysJson> {
            Ok(PendingDeploysJson {
                deploys: vec![],
                total_available: 0,
            })
        }

        async fn get_balance(
            &self,
            address: String,
            _: Option<String>,
        ) -> eyre::Result<BalanceResponse> {
            if address == "boom" {
                return Err(eyre::Report::new(InvalidHashError(
                    "bad block hash".to_string(),
                )));
            }
            Ok(BalanceResponse {
                address,
                balance: 1000,
                block_number: 5,
                block_hash: "aa".to_string(),
            })
        }

        async fn get_registry(
            &self,
            uri: String,
            _: Option<String>,
        ) -> eyre::Result<RegistryResponse> {
            Ok(RegistryResponse {
                uri,
                data: vec![RhoExpr::ExprString {
                    data: "registered".to_string(),
                }],
                block_number: 5,
                block_hash: "aa".to_string(),
            })
        }

        async fn get_validators(&self, _: Option<String>) -> eyre::Result<ValidatorsResponse> {
            Ok(ValidatorsResponse {
                validators: vec![ValidatorInfo {
                    public_key: "vk".to_string(),
                    stake: 10,
                }],
                total_stake: 10,
                block_number: 5,
                block_hash: "aa".to_string(),
            })
        }

        async fn get_epoch(&self, _: Option<String>) -> eyre::Result<EpochResponse> {
            Ok(EpochResponse {
                current_epoch: 1,
                epoch_length: 100,
                quarantine_length: 50,
                blocks_until_next_epoch: 42,
                last_finalized_block_number: 158,
                block_hash: "aa".to_string(),
            })
        }

        async fn estimate_cost(
            &self,
            _: String,
            _: Option<String>,
            deployer: Option<String>,
        ) -> eyre::Result<EstimateCostResponse> {
            Ok(EstimateCostResponse {
                cost: 55,
                block_number: 5,
                block_hash: "aa".to_string(),
                deployer_identity: if deployer.is_some() {
                    DeployerIdentity::Provided
                } else {
                    DeployerIdentity::Ephemeral
                },
            })
        }

        async fn get_epoch_rewards(&self, _: Option<String>) -> eyre::Result<EpochRewardsResponse> {
            Ok(EpochRewardsResponse {
                rewards: RhoExpr::ExprInt { data: 3 },
                block_number: 5,
                block_hash: "aa".to_string(),
            })
        }

        async fn get_validator(
            &self,
            pubkey: String,
            _: Option<String>,
        ) -> eyre::Result<ValidatorStatusResponse> {
            Ok(ValidatorStatusResponse {
                public_key: pubkey,
                is_bonded: true,
                stake: Some(10),
                block_number: 5,
                block_hash: "aa".to_string(),
            })
        }

        async fn get_bond_status(&self, pubkey: String) -> eyre::Result<BondStatusResponse> {
            Ok(BondStatusResponse {
                public_key: pubkey,
                is_bonded: false,
            })
        }
    }

    fn app_state() -> AppState {
        let engine_cell = EngineCell::init();
        let block_report_api = BlockReportAPI::new(
            casper::rust::reporting_casper::noop(),
            ReportStore::new(Arc::new(InMemoryKeyValueStore::new())),
            engine_cell,
            block_storage::rust::key_value_block_store::KeyValueBlockStore::new(
                Arc::new(InMemoryKeyValueStore::new()),
                Arc::new(InMemoryKeyValueStore::new()),
            ),
            CliqueOracleImpl,
            false,
        );

        let local = PeerNode::new(
            NodeIdentifier::new("0a0b0c".to_string()),
            "localhost".to_string(),
            40400,
            40404,
        );
        let rp_conf = RPConf::new(
            local,
            "testnet".to_string(),
            None,
            Duration::from_secs(1),
            8,
            2,
        );

        let events = F1r3flyEvents::new();
        let startup_events = events.startup_buffer();

        AppState::new(
            Arc::new(StubAdminWebApi),
            Arc::new(CannedWebApi),
            Arc::new(block_report_api),
            RPConfCell::new(rp_conf),
            Arc::new(ConnectionsCell::new()),
            Arc::new(StubNodeDiscovery),
            Arc::new(events.consume()),
            startup_events,
        )
    }

    fn router() -> Router { WebApiRoutes::create_router().with_state(app_state()) }

    async fn get_response(uri: &str) -> (StatusCode, serde_json::Value) {
        let response = router()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn post_response(uri: &str, body: &str) -> (StatusCode, serde_json::Value) {
        let response = router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn status_route_reports_node_identity() {
        let (status, json) = get_response("/status").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["networkId"], "testnet");
        assert_eq!(json["shardId"], "root");
        assert_eq!(json["isReady"], true);
        assert_eq!(json["lastFinalizedBlockNumber"], 5);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prepare_deploy_routes_answer_for_get_and_post() {
        let (status, json) = get_response("/prepare-deploy").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["seqNumber"], 9);
        assert!(json["names"].as_array().unwrap().is_empty());

        let (status, json) = post_response(
            "/prepare-deploy",
            r#"{"deployer":"04aa","timestamp":1,"nameQty":1}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["names"][0], "04aa");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prepare_deploy_post_rejects_malformed_body() {
        let (status, json) = post_response("/prepare-deploy", r#"{"deployer": 42}"#).await;
        assert!(status.is_client_error());
        assert_eq!(json["error"], "invalid_request_body");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn deploy_and_explore_deploy_routes_answer() {
        let deploy_body = serde_json::json!({
            "data": {
                "term": "Nil",
                "language": "rholang",
                "timestamp": 1,
                "validAfterBlockNumber": 0,
                "shardId": "root",
                "authorityPresentations": [],
            },
            "deployer": "04aa",
            "signature": "bb",
            "sigAlgorithm": "secp256k1",
        });
        let (status, json) = post_response("/deploy", &deploy_body.to_string()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json, serde_json::json!("deploy-accepted"));

        let (status, json) = post_response("/explore-deploy", r#"{"term":"Nil"}"#).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["cost"], 7);

        let (status, json) = post_response(
            "/explore-deploy-by-block-hash",
            r#"{"term":"Nil","blockHash":"aabb"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["cost"], 7);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn explore_deploy_by_block_hash_requires_block_hash() {
        let (status, json) = post_response(
            "/explore-deploy-by-block-hash",
            r#"{"term":"Nil","blockHash":"  "}"#,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"], "invalid_hash");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn data_at_name_by_block_hash_route_answers() {
        let (status, json) = post_response(
            "/data-at-name-by-block-hash",
            r#"{"name":{"UnforgPrivate":{"data":"0102"}},"blockHash":"aabb"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["cost"], 7);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn block_routes_answer_in_both_views() {
        let (status, json) = get_response("/last-finalized-block").await;
        assert_eq!(status, StatusCode::OK);
        assert!(json.get("blockInfo").is_some());

        let (status, _) = get_response("/last-finalized-block?view=summary").await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = get_response("/block/aabbccddeeff").await;
        assert_eq!(status, StatusCode::OK);

        let (status, json) = get_response("/blocks").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.as_array().unwrap().len(), 1);

        let (status, json) = get_response("/blocks/3?view=full").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.as_array().unwrap().len(), 3);

        let (status, json) = get_response("/blocks/1/5").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn blocks_route_rejects_non_integer_depth() {
        let (status, json) = get_response("/blocks/not-a-number").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"], "invalid_path_parameter");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn deploy_lookup_routes_answer() {
        let (status, json) = get_response("/deploy/abc123def?view=summary").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["deployId"], "abc123def");
        assert!(json.get("term").is_none());

        let (status, json) = get_response("/is-finalized/aabb").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json, serde_json::json!(true));

        let (status, json) = get_response("/deploy-finalization-status/ccdd").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["state"], "Finalized");

        let (status, json) = get_response("/pending-deploys?deployer=04aa").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["totalAvailable"], 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn query_routes_answer() {
        let (status, json) = get_response("/balance/wallet-address").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["balance"], 1000);

        let (status, json) = get_response("/registry/rho:id:abc").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["uri"], "rho:id:abc");

        let (status, json) = get_response("/validators").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["totalStake"], 10);

        let (status, json) = get_response("/validator/04aa").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["isBonded"], true);

        let (status, json) = get_response("/epoch").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["currentEpoch"], 1);

        let (status, json) = get_response("/epoch/rewards").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["rewards"]["ExprInt"]["data"], 3);

        let (status, json) = get_response("/bond-status/04aa").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["isBonded"], false);

        let (status, json) =
            post_response("/estimate-cost?block_hash=aa", r#"{"term":"Nil"}"#).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["cost"], 55);
        assert_eq!(json["deployerIdentity"], "ephemeral");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn handler_errors_are_classified_json_responses() {
        let (status, json) = get_response("/balance/boom").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"], "invalid_hash");
        assert_eq!(json["message"], "bad block hash");
    }
}
