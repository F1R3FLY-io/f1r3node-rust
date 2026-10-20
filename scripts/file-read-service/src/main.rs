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
// Oracular files + dirs only in v1.  Consensus-mode entries
// return 503 with a specific diagnostic (sidecar can't resolve
// `/@bundle/<X>` without the node's `RootIdentityRegistry`).

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
    let (local_addr, handle) = serve(cli.bind, index).await.context("start axum server")?;
    tracing::info!(target: "file_read_service", %local_addr, "sidecar listening");
    handle.await.context("sidecar task join")??;
    Ok(())
}
