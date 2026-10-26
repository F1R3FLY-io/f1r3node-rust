// `file-read-service` — HTTP sidecar serving operator-declared
// static files from an F1r3fly node's HOCON config.
//
// # Design
//
// Reads the SAME `oracle-static-{files,dirs}` HOCON block the
// node reads, builds an in-memory index at startup, serves bytes
// via `GET /files/{logical}` with POSIX `F_RDLCK` bracket so the
// node's `F_WRLCK`-bracketed writes block us (and vice versa) at
// the kernel level.
//
// # Deployment
//
// Operators run this on the same host as the node (shared
// filesystem).  Both read the same HOCON.  Sidecar listens on an
// operator-chosen port; node is unaware of the sidecar.
//
// Rate limiting, auth, TLS → external reverse proxy (nginx,
// Envoy, tower-governor).  The sidecar's narrow surface is "serve
// bytes with an fcntl bracket."
//
// # Scope
//
// Serves Oracular files + dirs, plus operator-declared
// consensus-static-dirs (the node writes the WAL-replicated bytes
// into `entry.path` verbatim; sidecar reads from the same path).
// Consensus-static-FILES remain 503 — their on-disk layout runs
// through `fs_bundle_staging` and isn't stable from HOCON alone,
// so the sidecar would need the node's `RootIdentityRegistry`.
//
// # Operator notes
//
//   * Config is read ONCE at startup.  Changes to the HOCON file
//     require a sidecar restart to take effect.  v1 has no
//     SIGHUP / inotify-triggered reload — a restart is the only
//     path.
//   * Requires a local POSIX filesystem for the fcntl discipline
//     to work.  NFS and some shared-storage setups have well-
//     known fcntl gaps — on those, the node's F_WRLCK bracket
//     may not block the sidecar's F_RDLCK (or vice versa) and
//     the sidecar can observe torn writes.  Co-locating node +
//     sidecar on a local disk is the supported model.
//   * No built-in auth.  Bind to `127.0.0.1:<port>` and front
//     with a reverse proxy that handles auth / TLS / rate
//     limiting.  Non-loopback binds surface a loud startup
//     warning but are not blocked.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use eyre::{Context, Result};
use file_read_service::config::load_from_path;
use file_read_service::index::FileReadIndex;
use file_read_service::server::serve;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Debug, Parser)]
#[command(
    name = "file-read-service",
    about = "HTTP sidecar serving operator-declared static files from an F1r3fly node's HOCON config"
)]
struct Cli {
    /// Path to the HOCON config file.  Should be the same file
    /// (or a subset) the node reads for its
    /// `storage.{oracle,consensus}-static-{files,dirs}` block.
    #[arg(long, value_name = "PATH")]
    config: PathBuf,

    /// Listen address.  Default `127.0.0.1:0` picks an ephemeral
    /// port (operators who want external access set this to
    /// `0.0.0.0:<port>` behind a reverse proxy).
    #[arg(long, value_name = "ADDR", default_value = "127.0.0.1:0")]
    bind: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let cfg = load_from_path(&cli.config).context("load sidecar config")?;
    let index = Arc::new(FileReadIndex::build(&cfg));
    // Loud warning for non-loopback binds: the sidecar has no
    // built-in auth / TLS / rate-limiting (externalized to a
    // reverse proxy by design).  Binding to a routable address
    // without a proxy in front exposes operator-declared file
    // bytes to anyone who can reach the port — rarely intended.
    if !cli.bind.ip().is_loopback() {
        tracing::warn!(
            target: "file_read_service",
            bind = %cli.bind,
            "sidecar is binding to a non-loopback address.  The sidecar \
             exposes operator-declared file bytes with NO built-in auth, \
             TLS, or rate limiting.  Deploy a reverse proxy (nginx, \
             Envoy, tower-governor) in front, or bind to 127.0.0.1 and \
             let the proxy forward.  If this is intentional, this \
             warning is informational only."
        );
    }
    let (local_addr, handle) = serve(cli.bind, index).await.context("start axum server")?;
    tracing::info!(target: "file_read_service", %local_addr, "sidecar listening");
    handle.await.context("sidecar task join")??;
    Ok(())
}
