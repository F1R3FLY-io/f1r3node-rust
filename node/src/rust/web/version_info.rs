use std::env;

use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

pub fn get_version_info() -> (&'static str, &'static str) {
    let version = env!("CARGO_PKG_VERSION");
    let build_id = env!("F1R3NODE_BUILD_ID");
    (version, build_id)
}

pub fn get_version_info_str() -> String {
    let (version, build_id) = get_version_info();
    format!("F1r3node Rust {} ({})", version, build_id)
}

pub struct VersionInfo;

impl VersionInfo {
    pub fn create_router() -> Router { Router::new().route("/", get(version_info_handler)) }
}

#[utoipa::path(
        get,
        path = "/version",
        responses(
            (status = 200, description = "Node version information", body = String),
        ),
        tag = "System"
    )]
pub async fn version_info_handler() -> Response { get_version_info_str().into_response() }
