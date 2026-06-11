use axum::extract::State;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;

use crate::rust::api::serde_types::block_info::BlockInfoSerde;
use crate::rust::api::web_api::{
    DataAtNameByBlockHashRequest, DeployResponse, PrepareRequest, PrepareResponse, RhoDataResponse,
};
use crate::rust::web::shared_handlers::{
    self, ApiErrorResponse, AppError, AppJson, AppPath, AppQuery, AppState,
};

#[derive(Debug, Deserialize)]
pub struct ViewQuery {
    pub view: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BlockHashQuery {
    pub block_hash: Option<String>,
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
        (status = 200, description = "Next deploy sequence number. Use `seqNumber` as `validAfterBlockNumber` in your deploy", body = PrepareResponse),
        (status = 500, description = "Node-side failure (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn prepare_deploy_get_handler(State(app_state): State<AppState>) -> Response {
    match app_state.web_api.prepare_deploy(None).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/api/prepare-deploy",
    request_body = PrepareRequest,
    responses(
        (status = 200, description = "Next deploy sequence number and pre-generated unforgeable names for the given deployer/timestamp", body = PrepareResponse),
        (status = 400, description = "Malformed request body or invalid deployer hex (`invalid_request_body`, `invalid_hash`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`runtime_error`)", body = ApiErrorResponse),
    ),
    tag = "WebAPI"
)]
pub async fn prepare_deploy_post_handler(
    State(app_state): State<AppState>,
    AppJson(request): AppJson<PrepareRequest>,
) -> Response {
    match app_state.web_api.prepare_deploy(Some(request)).await {
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
    match app_state.web_api.get_data_at_par(request).await {
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
    match app_state.web_api.last_finalized_block(view).await {
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
    match app_state
        .web_api
        .get_blocks_by_heights(start, end, view)
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
    match app_state.web_api.get_blocks(depth, view).await {
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

    match app_state.web_api.find_deploy(deploy_id, view).await {
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
    match app_state.web_api.is_finalized(hash).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

use crate::rust::api::web_api::{
    BalanceResponse, EpochResponse, RegistryResponse, ValidatorsResponse,
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
    match app_state
        .web_api
        .deploy_finalization_status(deploy_sig_hex)
        .await
    {
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
    match app_state
        .web_api
        .get_balance(address, query.block_hash)
        .await
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
    match app_state.web_api.get_registry(uri, query.block_hash).await {
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
    match app_state.web_api.get_validators(query.block_hash).await {
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
    match app_state.web_api.get_epoch(query.block_hash).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(e).into_response(),
    }
}

use crate::rust::api::web_api::{
    BondStatusResponse as BondStatusResp, EpochRewardsResponse, EstimateCostResponse,
    SimpleExploreDeployRequest, ValidatorStatusResponse,
};

#[utoipa::path(
    post,
    path = "/api/estimate-cost",
    request_body = SimpleExploreDeployRequest,
    params(
        ("block_hash" = Option<String>, Query, description = "Block hash to query against; defaults to the last-finalized block"),
    ),
    responses(
        (status = 200, description = "Estimated phlogiston (gas) cost for the given Rholang term", body = EstimateCostResponse),
        (status = 400, description = "Malformed request body, invalid Rholang term, or invalid block hash (`invalid_request_body`, `rholang_bad_term`, `invalid_hash`, `readonly_node_required`)", body = ApiErrorResponse),
        (status = 404, description = "Specified block not found (`block_not_found`)", body = ApiErrorResponse),
        (status = 422, description = "Term is structurally valid but failed execution (`rholang_execution_error`, `out_of_phlogistons`)", body = ApiErrorResponse),
        (status = 500, description = "Node-side failure (`interpreter_internal_error`)", body = ApiErrorResponse),
    ),
    tag = "Query"
)]
pub async fn estimate_cost_handler(
    State(app_state): State<AppState>,
    AppQuery(query): AppQuery<BlockHashQuery>,
    AppJson(request): AppJson<SimpleExploreDeployRequest>,
) -> Response {
    match app_state
        .web_api
        .estimate_cost(request.term, query.block_hash)
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
    match app_state.web_api.get_epoch_rewards(query.block_hash).await {
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
    match app_state
        .web_api
        .get_validator(pubkey, query.block_hash)
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
    match app_state.web_api.get_bond_status(pubkey).await {
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

    fn sample_deploy_response(view: ViewMode) -> DeployResponse {
        let is_full = view == ViewMode::Full;
        DeployResponse {
            deploy_id: "abc123def".to_string(),
            block_hash: "7bf8abc123".to_string(),
            block_number: 52331,
            timestamp: 1770028092477,
            cost: 100,
            errored: false,
            is_finalized: true,
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
            phlo_price: if is_full { Some(10) } else { None },
            phlo_limit: if is_full { Some(100000) } else { None },
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
        assert!(json.get("phloPrice").is_some());
        assert!(json.get("phloLimit").is_some());
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
}
