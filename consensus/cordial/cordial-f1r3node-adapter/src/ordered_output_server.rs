//! Lightweight HTTP server that serves the latest finalized ordered output.
//!
//! This module closes the gap between the in-process `SharedOrderedOutput`
//! pipeline and external consumers. It exposes two endpoints:
//!
//! | Endpoint                        | Purpose                                    |
//! |---------------------------------|--------------------------------------------|
//! | `GET /ordered-output/latest`    | Full [`OrderedFinalizedOutput`] as JSON    |
//! | `GET /ordered-output/status`    | Lightweight status struct; suitable for polling |
//!
//! ## Architecture
//!
//! The server holds an `Arc<Mutex<SharedOrderedOutput>>` that is shared with
//! the background mirror task. The mirror task calls `.update()` each polling
//! cycle; the HTTP handlers acquire the lock, read the latest output, and
//! release it before returning a response. No f1r3node source changes are
//! required.
//!
//! ## Staleness
//!
//! `/ordered-output/status` includes an `is_stale` field that is `true` when
//! `computed_at_ns` is more than the configured threshold behind wall clock.
//!
//! ## Empty state
//!
//! Both endpoints return `503 Service Unavailable` when no output has been
//! computed yet (no finalized leader exists). They never return a 200 with an
//! empty block list.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::ordered_output::OrderedFinalizedOutput;
use crate::shared_ordered_output::{ReadOrderedOutput, SharedOrderedOutput};

// ── Shared state ─────────────────────────────────────────────────────────────

/// Server-wide state threaded through every axum handler.
#[derive(Clone)]
pub struct OrderedOutputServerState {
    /// Shared container updated by the background mirror task.
    pub shared: Arc<Mutex<SharedOrderedOutput>>,
    /// Maximum age (in nanoseconds) before `/status` reports `is_stale: true`.
    pub stale_threshold_ns: u128,
}

impl OrderedOutputServerState {
    pub fn new(shared: Arc<Mutex<SharedOrderedOutput>>, stale_threshold_ns: u128) -> Self {
        Self {
            shared,
            stale_threshold_ns,
        }
    }
}

// ── Response types ────────────────────────────────────────────────────────────

/// Lightweight status response — no block list.
///
/// Consumers that only need to know whether the server is live and up-to-date
/// should prefer this over `/ordered-output/latest`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderedOutputStatus {
    /// Hex-encoded anchor block content hash, or `null` if no anchor yet.
    pub anchor_hash: Option<String>,
    /// Number of blocks in the finalized ordered prefix.
    pub len: usize,
    /// Number of bonded validators at computation time.
    pub bond_count: usize,
    /// Consensus wavelength used to produce this output.
    pub wavelength: u64,
    /// Wall-clock nanoseconds since Unix epoch when the output was computed.
    pub computed_at_ns: u128,
    /// `true` when the output is older than the server's configured staleness
    /// threshold relative to the current wall clock.
    pub is_stale: bool,
}

impl OrderedOutputStatus {
    pub fn from_output(output: &OrderedFinalizedOutput, stale_threshold_ns: u128) -> Self {
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        let is_stale = now_ns.saturating_sub(output.computed_at_ns) > stale_threshold_ns;

        Self {
            anchor_hash: output.anchor_hash().map(hex::encode),
            len: output.len(),
            bond_count: output.bond_count,
            wavelength: output.wavelength,
            computed_at_ns: output.computed_at_ns,
            is_stale,
        }
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /ordered-output/latest`
///
/// Returns the full [`OrderedFinalizedOutput`] serialized as JSON.
///
/// Returns `503 Service Unavailable` when no output has been computed yet.
pub async fn get_latest(State(state): State<OrderedOutputServerState>) -> Response {
    let guard = state.shared.lock().await;
    let Some(output) = guard.latest().filter(|out| out.anchor.is_some()) else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "no finalized ordered output available yet"
            })),
        )
            .into_response();
    };

    let output: OrderedFinalizedOutput = output.clone();
    drop(guard);

    (axum::http::StatusCode::OK, Json(output)).into_response()
}

/// `GET /ordered-output/status`
///
/// Returns a lightweight [`OrderedOutputStatus`] object without the full block
/// list. Suitable for polling from a demo app without deserializing the entire
/// ordered sequence.
///
/// Returns `503 Service Unavailable` when no output has been computed yet.
pub async fn get_status(State(state): State<OrderedOutputServerState>) -> Response {
    let guard = state.shared.lock().await;
    let Some(output) = guard.latest().filter(|out| out.anchor.is_some()) else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "no finalized ordered output available yet"
            })),
        )
            .into_response();
    };

    let status = OrderedOutputStatus::from_output(output, state.stale_threshold_ns);
    drop(guard);

    (axum::http::StatusCode::OK, Json(status)).into_response()
}

// ── Router ────────────────────────────────────────────────────────────────────

/// Build the axum [`Router`] that wires the two endpoints to `state`.
pub fn ordered_output_router(state: OrderedOutputServerState) -> Router {
    Router::new()
        .route("/ordered-output/latest", get(get_latest))
        .route("/ordered-output/status", get(get_status))
        .with_state(state)
}

// ── Server entry point ────────────────────────────────────────────────────────

/// Start the HTTP server on `addr`.
///
/// Binds the listener and hands control to `axum::serve`. This future runs
/// until the process is killed or the listener encounters an unrecoverable
/// error.
///
/// The caller is responsible for populating `shared` before (or shortly after)
/// calling `serve` — handlers return `503` for as long as the container is
/// empty.
pub async fn serve(
    shared: Arc<Mutex<SharedOrderedOutput>>,
    addr: SocketAddr,
    stale_threshold_ns: u128,
) -> Result<(), std::io::Error> {
    let state = OrderedOutputServerState::new(shared, stale_threshold_ns);
    let router = ordered_output_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router)
        .await
        .map_err(std::io::Error::other)
}

// ── Poll cycle helper ─────────────────────────────────────────────────────────

/// Outcome of a single [`poll_once`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    /// Shared output was updated with a new, prefix-preserving result.
    Updated { finalized_len: usize },
    /// The newly computed output was rejected because it would regress the
    /// previously stored prefix. The existing output is preserved unchanged.
    PrefixRejected,
    /// No finalized leader exists yet; shared output was not changed.
    NoLeader,
}

/// Execute one poll cycle against an already-populated [`SharedOrderedOutput`].
///
/// This function is the testable core of the background poller in the binary.
/// It accepts a pre-built `OrderedFinalizedOutput` (so tests can supply any
/// block set without network access), attempts to push it into `shared` via
/// `SharedOrderedOutput::update`, and returns a [`PollOutcome`] describing
/// what happened.
///
/// The binary's spawn loop calls this indirectly via the same logic; extracting
/// it here lets integration tests exercise prefix-rejection, no-leader, and
/// stale-after-rejection paths without starting a server or connecting to gRPC.
pub fn poll_once(
    shared: &mut SharedOrderedOutput,
    output: crate::ordered_output::OrderedFinalizedOutput,
) -> PollOutcome {
    if output.anchor.is_none() {
        return PollOutcome::NoLeader;
    }

    let finalized_len = output.len();
    match shared.update(output) {
        Ok(()) => PollOutcome::Updated { finalized_len },
        Err(_) => PollOutcome::PrefixRejected,
    }
}
