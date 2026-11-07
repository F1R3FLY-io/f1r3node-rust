use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use deploy_queue::engine::{Engine, EngineHandle, COMMAND_BUFFER};
use deploy_queue::events::run_event_link;
use deploy_queue::grpc::{QueueGrpc, MAX_MESSAGE_BYTES};
use deploy_queue::http::{router, HttpState};
use deploy_queue::node::NodeHttp;
use deploy_queue::store::QueueStore;
use metrics_exporter_prometheus::PrometheusBuilder;
use models::casper::v1::deploy_service_server::DeployServiceServer;
use models::deployqueue::v1::deploy_queue_server::DeployQueueServer;
use tokio::sync::mpsc;
use tonic::transport::{Endpoint, Server};
use tracing_subscriber::EnvFilter;

/// A per-validator deploy queue. Clients deploy to it in place of the node;
/// it releases deploys to the node at the pace the shard sustains.
#[derive(Parser, Debug)]
#[command(name = "deploy-queue", version)]
struct Args {
    /// Address the client gRPC API listens on.
    #[arg(long, default_value = "0.0.0.0:40401")]
    grpc_listen: SocketAddr,
    /// Address the client HTTP API listens on.
    #[arg(long, default_value = "0.0.0.0:40403")]
    http_listen: SocketAddr,
    /// The node's external gRPC API, e.g. http://validator:40401.
    #[arg(long)]
    node_grpc: String,
    /// The node's HTTP API, e.g. http://validator:40403.
    #[arg(long)]
    node_http: String,
    /// Directory for the queue's durable store.
    #[arg(long)]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let args = Args::parse();
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(target: "deploy_queue", error = %e, "deploy queue stopped");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let metrics = PrometheusBuilder::new().install_recorder()?;
    let node_http = NodeHttp::new(&args.node_http)?;
    let node_channel = Endpoint::from_shared(args.node_grpc.clone())?.connect_lazy();
    let store = QueueStore::open(&args.data_dir)?;

    let (commands, command_receiver) = mpsc::channel(COMMAND_BUFFER);
    let engine = Engine::new(store, node_http.clone(), commands.clone())?;
    let handle = EngineHandle::new(commands.clone());

    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(models::FILE_DESCRIPTOR_SET)
        .build_v1()?;
    let grpc = QueueGrpc::new(handle.clone(), node_channel);
    let grpc_server = Server::builder()
        .add_service(
            DeployServiceServer::new(grpc.clone())
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES),
        )
        .add_service(DeployQueueServer::new(grpc))
        .add_service(reflection)
        .serve_with_shutdown(args.grpc_listen, shutdown_signal());

    let http_listener = tokio::net::TcpListener::bind(args.http_listen).await?;
    let http_server = axum::serve(
        http_listener,
        router(HttpState {
            engine: handle,
            node: node_http.clone(),
            metrics,
        }),
    )
    .with_graceful_shutdown(shutdown_signal());

    tracing::info!(
        target: "deploy_queue",
        grpc = %args.grpc_listen,
        http = %args.http_listen,
        node_grpc = %args.node_grpc,
        node_http = %args.node_http,
        data_dir = %args.data_dir.display(),
        "deploy queue starting"
    );
    let link = tokio::spawn(run_event_link(node_http, commands));
    let engine = tokio::spawn(engine.run(command_receiver));

    let outcome: Result<(), Box<dyn std::error::Error + Send + Sync>> = tokio::select! {
        served = grpc_server => served.map_err(Into::into),
        served = http_server => served.map_err(Into::into),
        stopped = engine => match stopped {
            Ok(Ok(())) => Err("deploy queue engine stopped".into()),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(e.into()),
        },
    };
    link.abort();
    outcome
}

async fn shutdown_signal() {
    let interrupt = tokio::signal::ctrl_c();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("installing a SIGTERM handler");
    tokio::select! {
        _ = interrupt => {}
        _ = terminate.recv() => {}
    }
    tracing::info!(target: "deploy_queue", "shutdown requested");
}
