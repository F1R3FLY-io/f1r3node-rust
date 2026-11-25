// Axum router + server bootstrap helper.
//
// Separates HTTP wiring from the main entry point so integration
// tests can spin up the server on an ephemeral port (127.0.0.1:0)
// without going through the clap-parsed CLI surface.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::get;
use axum::Router;

use crate::handler::{get_file, list_dir, AppState};
use crate::index::FileReadIndex;

/// Build the axum router with the sidecar's routes.
///
/// * `GET /files/{logical}` — serve bytes (optionally with
///   `?sub_rel`, `?offset`, `?length`) for `oracle-static-*` or
///   `consensus-static-dirs` entries.
/// * `GET /list/{logical}?prefix=` — enumerate immediate children
///   of a Dir entry.  Bypasses the chain's stream-backed
///   `Dir.entries()` which is refused on consensus caps.
pub fn router(index: Arc<FileReadIndex>) -> Router {
    let state = Arc::new(AppState { index });
    Router::new()
        .route("/files/{logical}", get(get_file))
        .route("/list/{logical}", get(list_dir))
        .with_state(state)
}

/// Bind + serve on the given address.  Returns the resolved
/// local address (useful when binding to `:0` for an ephemeral
/// port) + a `JoinHandle` for the server task.
pub async fn serve(
    addr: SocketAddr,
    index: Arc<FileReadIndex>,
) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<std::io::Result<()>>)> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    let router = router(index);
    let handle = tokio::spawn(async move { axum::serve(listener, router).await });
    Ok((local_addr, handle))
}
