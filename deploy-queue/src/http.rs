use std::collections::HashSet;

use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use futures::{SinkExt, StreamExt, TryStreamExt};
use metrics_exporter_prometheus::PrometheusHandle;
use models::rust::deploy_request::DeployRequest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tokio_tungstenite::tungstenite;

use crate::engine::EngineHandle;
use crate::error::IntakeError;
use crate::intake::check_term;
use crate::node::NodeHttp;
use crate::record::{DeployView, Sig};

/// Matches the node's default `http-max-body-bytes`.
pub const MAX_BODY_BYTES: usize = 16_000_000;

/// Headers that describe one connection, not the message, and are never
/// forwarded (RFC 9110 §7.6.1).
const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

#[derive(Clone)]
pub struct HttpState {
    pub engine: EngineHandle,
    pub node: NodeHttp,
    pub metrics: PrometheusHandle,
}

pub fn router(state: HttpState) -> Router {
    Router::new()
        .route("/api/deploy", post(deploy_handler))
        .route("/api/v1/deploy", post(deploy_handler))
        .route(
            "/api/deploy-finalization-status/{deploy_sig_hex}",
            get(deploy_status_handler),
        )
        .route("/ws/deploys", get(watch_handler))
        .route("/ws/events", get(events_proxy_handler))
        .route("/queue/status", get(queue_status_handler))
        .route("/queue/metrics", get(queue_metrics_handler))
        .fallback(proxy_handler)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

fn error_response(status: StatusCode, kind: &str, message: String) -> Response {
    (status, Json(json!({ "error": kind, "message": message }))).into_response()
}

fn intake_error_response(error: IntakeError) -> Response {
    let mut response = error_response(error.status(), error.kind(), error.to_string());
    if let Some(secs) = error.retry_after_secs() {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from(secs));
    }
    response
}

/// Takes a deploy in the node's `/api/deploy` shape and answers as the node
/// would: the deploy id on success, the node's error envelope otherwise.
async fn deploy_handler(State(state): State<HttpState>, body: Bytes) -> Response {
    let request: DeployRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_body",
                e.to_string(),
            )
        }
    };
    let deploy = match request.to_signed() {
        Ok(deploy) => deploy,
        Err(e) => return intake_error_response(IntakeError::IllegalArgument(e.to_string())),
    };
    match accept(&state.engine, deploy).await {
        Ok(sig) => Json(format!("Success!\nDeployId is: {}", hex::encode(sig))).into_response(),
        Err(e) => intake_error_response(e),
    }
}

/// The checks every transport shares: compile the term off the runtime,
/// then hand the deploy to the engine.
pub async fn accept(
    engine: &EngineHandle,
    deploy: crypto::rust::signatures::signed::Signed<
        models::rust::casper::protocol::casper_message::DeployData,
    >,
) -> Result<Sig, IntakeError> {
    let deploy = tokio::task::spawn_blocking(move || check_term(&deploy).map(|()| deploy))
        .await
        .map_err(|e| IntakeError::Store(format!("term check task failed: {}", e)))??;
    engine.intake(deploy).await
}

/// The node's deploy-status body, plus the queue's own state and detail.
#[derive(Debug, Serialize)]
struct QueueDeployStatus {
    state: &'static str,
    rejection_count: u32,
    latest_block_hash: Option<String>,
    queue_state: &'static str,
    detail: Option<String>,
}

impl From<DeployView> for QueueDeployStatus {
    fn from(view: DeployView) -> Self {
        Self {
            state: view.state.status_label(),
            rejection_count: view.rejection_count,
            latest_block_hash: view.latest_block_hash.map(hex::encode),
            queue_state: view.state.name(),
            detail: view.detail,
        }
    }
}

async fn deploy_status_handler(
    State(state): State<HttpState>,
    Path(deploy_sig_hex): Path<String>,
    request: Request,
) -> Response {
    let Ok(sig) = hex::decode(deploy_sig_hex.trim_start_matches("0x")) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_hash",
            format!("'{}' is not a valid hex deploy signature", deploy_sig_hex),
        );
    };
    match state.engine.status(sig).await {
        Ok(Some(view)) => Json(QueueDeployStatus::from(view)).into_response(),
        Ok(None) => proxy(&state.node, request).await,
        Err(e) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "deploy_queue_stopped",
            e.to_string(),
        ),
    }
}

#[derive(Deserialize)]
struct WatchQuery {
    /// Comma-separated hex deploy signatures.
    sigs: String,
}

/// Streams the named deploys' states: each one's current state first, then
/// every change, closing once all are terminal.
async fn watch_handler(
    State(state): State<HttpState>,
    Query(query): Query<WatchQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let sigs: Result<Vec<Sig>, _> = query
        .sigs
        .split(',')
        .filter(|sig| !sig.is_empty())
        .map(|sig| hex::decode(sig.trim_start_matches("0x")))
        .collect();
    let sigs = match sigs {
        Ok(sigs) if !sigs.is_empty() => sigs,
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_hash",
                "sigs must be a comma-separated list of hex deploy signatures".to_string(),
            )
        }
    };
    upgrade.on_upgrade(move |socket| watch(socket, state.engine, sigs))
}

fn transition_message(sig: &[u8], view: Option<DeployView>) -> Message {
    let body = match view {
        Some(view) => {
            let status = QueueDeployStatus::from(view);
            json!({ "deploy_id": hex::encode(sig), "status": status })
        }
        None => json!({ "deploy_id": hex::encode(sig), "status": null }),
    };
    Message::Text(body.to_string().into())
}

async fn watch(mut socket: WebSocket, engine: EngineHandle, sigs: Vec<Sig>) {
    let Ok(start) = engine.watch(sigs).await else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };
    let mut open: HashSet<Sig> = HashSet::new();
    for (sig, view) in start.current {
        if view.as_ref().is_some_and(|view| !view.state.is_terminal()) {
            open.insert(sig.clone());
        }
        if socket.send(transition_message(&sig, view)).await.is_err() {
            return;
        }
    }
    let mut updates = start.updates;
    while !open.is_empty() {
        match updates.recv().await {
            Ok(transition) if open.contains(&transition.sig) => {
                if transition.view.state.is_terminal() {
                    open.remove(&transition.sig);
                }
                if socket
                    .send(transition_message(&transition.sig, Some(transition.view)))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Ok(_) => {}
            Err(RecvError::Lagged(_)) => {
                // The watcher missed transitions; it reconnects and reads the
                // current states again.
                let _ = socket
                    .send(Message::Text(
                        json!({ "event": "stream-lagged" }).to_string().into(),
                    ))
                    .await;
                break;
            }
            Err(RecvError::Closed) => break,
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

/// Relays the node's event stream, so clients that follow `/ws/events` keep
/// working against the queue.
async fn events_proxy_handler(
    State(state): State<HttpState>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let url = state.node.ws_events_url();
    upgrade.on_upgrade(move |client| relay_events(client, url))
}

async fn relay_events(client: WebSocket, url: String) {
    let node = match tokio_tungstenite::connect_async(&url).await {
        Ok((node, _)) => node,
        Err(e) => {
            tracing::warn!(target: "deploy_queue::http", error = %e, "event relay could not reach the node");
            return;
        }
    };
    let (mut node_sink, mut node_stream) = node.split();
    let (mut client_sink, mut client_stream) = client.split();
    let downstream = async {
        while let Some(Ok(message)) = node_stream.next().await {
            let forwarded = match message {
                tungstenite::Message::Text(text) => Message::Text(text.as_str().into()),
                tungstenite::Message::Binary(bytes) => Message::Binary(bytes),
                tungstenite::Message::Close(_) => break,
                _ => continue,
            };
            if client_sink.send(forwarded).await.is_err() {
                break;
            }
        }
    };
    let upstream = async {
        while let Some(Ok(message)) = client_stream.next().await {
            let forwarded = match message {
                Message::Text(text) => tungstenite::Message::Text(text.as_str().into()),
                Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
                Message::Close(_) => break,
                _ => continue,
            };
            if node_sink.send(forwarded).await.is_err() {
                break;
            }
        }
    };
    tokio::select! {
        _ = downstream => {}
        _ = upstream => {}
    }
}

async fn queue_status_handler(State(state): State<HttpState>) -> Response {
    match state.engine.snapshot().await {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(e) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "deploy_queue_stopped",
            e.to_string(),
        ),
    }
}

async fn queue_metrics_handler(State(state): State<HttpState>) -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.render(),
    )
        .into_response()
}

async fn proxy_handler(State(state): State<HttpState>, request: Request) -> Response {
    proxy(&state.node, request).await
}

/// Forwards a request to the node unchanged and streams the answer back.
async fn proxy(node: &NodeHttp, request: Request) -> Response {
    let path_and_query = request
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| request.uri().path().to_string());
    let method = request.method().clone();
    let headers = forwardable(request.headers());
    let body = reqwest::Body::wrap_stream(request.into_body().into_data_stream());
    let answer = node
        .client()
        .request(method, node.url(&path_and_query))
        .headers(headers)
        .body(body)
        .send()
        .await;
    match answer {
        Ok(answer) => {
            let mut response = Response::builder().status(answer.status());
            if let Some(headers) = response.headers_mut() {
                *headers = forwardable(answer.headers());
            }
            response
                .body(Body::from_stream(
                    answer.bytes_stream().map_err(std::io::Error::other),
                ))
                .unwrap_or_else(|e| {
                    error_response(
                        StatusCode::BAD_GATEWAY,
                        "node_unavailable",
                        format!("node answer could not be relayed: {}", e),
                    )
                })
        }
        Err(e) => error_response(
            StatusCode::BAD_GATEWAY,
            "node_unavailable",
            format!("node did not answer: {}", e),
        ),
    }
}

fn forwardable(headers: &HeaderMap) -> HeaderMap {
    headers
        .iter()
        .filter(|(name, _)| {
            *name != header::HOST
                && *name != header::CONTENT_LENGTH
                && !HOP_BY_HOP.contains(&name.as_str())
        })
        .map(|(name, value)| (HeaderName::clone(name), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_headers_are_not_forwarded() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("queue"));
        headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("3"));
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        let forwarded = forwardable(&headers);
        assert_eq!(forwarded.len(), 1);
        assert_eq!(forwarded[header::ACCEPT], "application/json");
    }

    #[test]
    fn queue_status_speaks_the_node_vocabulary() {
        let status = QueueDeployStatus::from(DeployView {
            state: crate::record::QueueState::Rejected,
            rejection_count: 0,
            latest_block_hash: None,
            detail: Some("illegal_argument: x".to_string()),
        });
        let body = serde_json::to_value(status).unwrap();
        assert_eq!(body["state"], "Failed");
        assert_eq!(body["queue_state"], "rejected");
        assert!(body["latest_block_hash"].is_null());
    }
}
