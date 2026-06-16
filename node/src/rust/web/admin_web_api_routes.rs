use axum::extract::State;
use axum::routing::post;
use axum::Router;

use crate::rust::web::shared_handlers::{ApiErrorResponse, AppError, AppState};

pub struct AdminWebApiRoutes;

impl AdminWebApiRoutes {
    /// Creates the admin Web API router
    pub fn create_router() -> Router<AppState> {
        Router::new().route("/propose", post(propose_handler))
    }
}

#[utoipa::path(
        post,
        path = "/api/propose",
        responses(
            (status = 200, description = "Propose result message (success block hash)", body = String),
            (status = 400, description = "Read-only node (`readonly_node_required`)", body = ApiErrorResponse),
            (status = 500, description = "Node-side propose failure (`unknown_error`, `replay_failure`, `no_new_deploys`)", body = ApiErrorResponse),
        ),
        tag = "AdminAPI"
    )]
pub async fn propose_handler(State(app_state): State<AppState>) -> Result<String, AppError> {
    let result = app_state.admin_web_api.propose().await?;
    Ok(result)
}
