use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use casper::rust::api::block_report_api::BlockReportError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::rust::api::serde_types::block_event_info::BlockEventInfoSerde;
use crate::rust::web::shared_handlers::{AppQuery, AppState};

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
            ("blockHash" = String, Query, description = "Block hash to generate the trace report for"),
            ("forceReplay" = Option<bool>, Query, description = "If `true`, discards any cached trace and re-replays the block from scratch (default: `false`)"),
        ),
        responses(
            (status = 200, description = "Block trace report (tagged: block-traces-report or block-report-error)", body = ReportResponse),
            (status = 400, description = "Invalid parameters"),
        ),
        tag = "Reporting"
    )]
pub async fn trace_handler(
    State(app_state): State<AppState>,
    AppQuery(params): AppQuery<TraceQuery>,
) -> Response {
    if params.block_hash.is_empty() {
        let error_response = ReportResponse::BlockReportError {
            error_message: "blockHash query parameter is required and must not be empty"
                .to_string(),
        };
        return (StatusCode::BAD_REQUEST, Json(error_response)).into_response();
    }

    let block_hash = match Blake2b256Hash::try_from_hex(&params.block_hash) {
        Ok(hash) => hash,
        Err(_) => {
            let error_response = ReportResponse::BlockReportError {
                error_message: format!("'{}' is not a valid hex hash", params.block_hash),
            };
            return (StatusCode::BAD_REQUEST, Json(error_response)).into_response();
        }
    };

    let force_replay = params.force_replay.unwrap_or(false);

    match app_state
        .block_report_api
        .block_report(block_hash.to_bytes_prost(), force_replay)
        .await
    {
        Ok(block_event_info) => Json(ReportResponse::BlockTracesReport {
            report: block_event_info.into(),
        })
        .into_response(),
        Err(e) => {
            let status = match &e {
                BlockReportError::BlockNotFound(_) => StatusCode::NOT_FOUND,
                BlockReportError::ReadOnlyRequired => StatusCode::BAD_REQUEST,
                BlockReportError::CasperNotInitialized => StatusCode::INTERNAL_SERVER_ERROR,
                BlockReportError::ReplayFailed(_) => StatusCode::INTERNAL_SERVER_ERROR,
                BlockReportError::BlockInfoError(_) => StatusCode::INTERNAL_SERVER_ERROR,
                BlockReportError::StoreError(_) => StatusCode::INTERNAL_SERVER_ERROR,
                BlockReportError::SemaphoreError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status,
                Json(ReportResponse::BlockReportError {
                    error_message: e.to_string(),
                }),
            )
                .into_response()
        }
    }
}
