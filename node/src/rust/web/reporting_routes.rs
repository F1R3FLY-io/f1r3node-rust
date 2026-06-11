use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::rust::api::serde_types::block_event_info::BlockEventInfoSerde;
use crate::rust::web::shared_handlers::{ApiErrorResponse, AppQuery, AppState};

pub struct ReportingRoutes;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type")]
pub enum ReportResponse {
    #[serde(rename = "block-traces-report")]
    BlockTracesReport { report: BlockEventInfoSerde },
    #[serde(rename = "block-report-error")]
    BlockReportError { error_message: String },
}

#[derive(Debug, Deserialize)]
pub struct TraceQuery {
    #[serde(rename = "blockHash")]
    pub block_hash: String,
    #[serde(rename = "forceReplay")]
    pub force_replay: Option<bool>,
}

pub type ReportingHttpRoutes = Router<AppState>;

impl ReportingRoutes {
    pub fn create_router() -> Router<AppState> { Router::new().route("/trace", get(trace_handler)) }
}

#[utoipa::path(
        get,
        path = "/reporting/trace",
        params(
            ("blockHash" = String, Query, description = "Full 64-char hex block hash to generate the trace report for"),
            ("forceReplay" = Option<bool>, Query, description = "If `true`, discards any cached trace and re-replays the block from scratch (default: `false`)"),
        ),
        responses(
            (status = 200, description = "Block trace report containing per-deploy execution events, or an inline error if replay failed", body = ReportResponse),
            (status = 400, description = "`blockHash` is missing, shorter than 6 characters, or contains non-hex characters (`invalid_query_parameter`, `invalid_hex`)", body = ApiErrorResponse),
        ),
        tag = "Reporting"
    )]
pub async fn trace_handler(
    State(app_state): State<AppState>,
    AppQuery(params): AppQuery<TraceQuery>,
) -> Response {
    if params.block_hash.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_query_parameter",
                "message": "block_hash parameter is required",
            })),
        )
            .into_response();
    }

    let block_hash_bytes = match hex::decode(&params.block_hash) {
        Ok(bytes) => bytes,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid_hex",
                    "message": format!("invalid block_hash hex: {e}"),
                })),
            )
                .into_response();
        }
    };

    let force_replay = params.force_replay.unwrap_or(false);

    let result = app_state
        .block_report_api
        .block_report(
            Blake2b256Hash::from_bytes(block_hash_bytes).to_bytes_prost(),
            force_replay,
        )
        .await;

    let response = match result {
        Ok(block_event_info) => ReportResponse::BlockTracesReport {
            report: block_event_info.into(),
        },
        Err(error) => ReportResponse::BlockReportError {
            error_message: error.to_string(),
        },
    };

    Json(response).into_response()
}
