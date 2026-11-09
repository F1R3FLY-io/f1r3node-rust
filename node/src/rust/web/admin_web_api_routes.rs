use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::rust::api::serde_types::validator_liveness::ValidatorLivenessResponse;
use crate::rust::web::shared_handlers::{ApiErrorResponse, AppError, AppState};

pub struct AdminWebApiRoutes;

impl AdminWebApiRoutes {
    /// Creates the admin Web API router
    pub fn create_router() -> Router<AppState> {
        Router::new()
            .route("/propose", post(propose_handler))
            .route("/validator-liveness", get(validator_liveness_handler))
    }
}

#[utoipa::path(
        post,
        path = "/api/propose",
        responses(
            (status = 200, description = "Propose result message (success block hash)", body = String),
            (status = 400, description = "Read-only node (`readonly_node_required`)", body = ApiErrorResponse),
            (status = 409, description = "No new deploys to propose (`no_new_deploys`)", body = ApiErrorResponse),
            (status = 500, description = "Node-side propose failure (`unknown_error`, `replay_failure`)", body = ApiErrorResponse),
        ),
        tag = "AdminAPI"
    )]
pub async fn propose_handler(State(app_state): State<AppState>) -> Result<String, AppError> {
    let result = app_state.admin_web_api.propose().await?;
    Ok(result)
}

#[utoipa::path(
        get,
        path = "/api/validator-liveness",
        responses(
            (status = 200, description = "This node's view of every validator's bucket, its own next decision, and the claims and ejections its last finalized block records", body = ValidatorLivenessResponse),
            (status = 500, description = "Casper is not available yet, or the finalized state could not be read", body = ApiErrorResponse),
        ),
        tag = "AdminAPI"
    )]
pub async fn validator_liveness_handler(
    State(app_state): State<AppState>,
) -> Result<Json<ValidatorLivenessResponse>, AppError> {
    Ok(Json(app_state.admin_web_api.validator_liveness().await?))
}
