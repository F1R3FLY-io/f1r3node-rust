use axum::extract::State;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use casper::rust::api::block_api::InvalidHashError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::rust::api::serde_types::block_event_info::BlockEventInfoSerde;
use crate::rust::web::shared_handlers::{ApiErrorResponse, AppError, AppQuery, AppState};

pub struct ReportingRoutes;

#[derive(Debug, Serialize, ToSchema)]
pub struct BlockTracesReport {
    pub report: BlockEventInfoSerde,
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
            (status = 200, description = "Block trace report containing per-deploy execution events", body = BlockTracesReport),
            (status = 400, description = "`blockHash` query parameter is missing, empty, or contains non-hex characters (`invalid_query_parameter`, `invalid_hash`)", body = ApiErrorResponse),
            (status = 500, description = "Block report replay failed (`unknown_error`)", body = ApiErrorResponse),
        ),
        tag = "Reporting"
    )]
pub async fn trace_handler(
    State(app_state): State<AppState>,
    AppQuery(params): AppQuery<TraceQuery>,
) -> Response {
    if params.block_hash.is_empty() {
        return AppError(eyre::Report::new(InvalidHashError(
            "blockHash query parameter is required and must not be empty".to_string(),
        )))
        .into_response();
    }

    let block_hash_bytes = match hex::decode(&params.block_hash) {
        Ok(bytes) => bytes,
        Err(_) => {
            return AppError(eyre::Report::new(InvalidHashError(format!(
                "'{}' is not valid hex",
                params.block_hash
            ))))
            .into_response();
        }
    };

    let force_replay = params.force_replay.unwrap_or(false);

    match app_state
        .block_report_api
        .block_report(
            Blake2b256Hash::from_bytes(block_hash_bytes).to_bytes_prost(),
            force_replay,
        )
        .await
    {
        Ok(block_event_info) => Json(BlockTracesReport {
            report: block_event_info.into(),
        })
        .into_response(),
        Err(e) => AppError(eyre::eyre!("block report replay failed: {}", e)).into_response(),
    }
}
