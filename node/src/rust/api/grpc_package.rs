// See node/src/main/scala/coop/rchain/node/api/package.scala

use std::time::Duration;

use models::casper::v1::deploy_service_server::DeployServiceServer;
use models::casper::v1::propose_service_server::ProposeServiceServer;
use tonic::transport::Server as TonicServer;

use crate::rust::api::deploy_grpc_service_v1::DeployGrpcServiceV1Impl;
use crate::rust::api::lsp_grpc_service::lsp::lsp_server::LspServer;
use crate::rust::api::lsp_grpc_service::LspGrpcServiceImpl;
use crate::rust::api::propose_grpc_service_v1::ProposeGrpcServiceV1Impl;
use crate::rust::api::repl_grpc_service::repl::repl_server::ReplServer;
use crate::rust::api::repl_grpc_service::ReplGrpcServiceImpl;

pub const FILE_DESCRIPTOR_SET: &[u8] =
    include_bytes!("../../../build/descriptors/reflection_protos.bin");

// Note: Deploy and Propose services are defined in the models crate
// These would be imported from models::casper::v1::{deploy_service_v1_server, propose_service_v1_server}

/// Shared transport configuration for both the internal and external gRPC servers.
fn configure_server(
    max_message_size: usize,
    keep_alive_time: Duration,
    keep_alive_timeout: Duration,
    tcp_keepalive_time: Duration,
    request_timeout: Duration,
    max_connection_age: Duration,
    max_connection_age_grace: Duration,
) -> TonicServer {
    TonicServer::builder()
        .tcp_keepalive(Some(tcp_keepalive_time))
        .max_frame_size(Some(max_message_size as u32))
        .http2_keepalive_interval(Some(keep_alive_time))
        .http2_keepalive_timeout(Some(keep_alive_timeout))
        .http2_adaptive_window(Some(true))
        .timeout(request_timeout)
        .max_connection_age(max_connection_age)
        .max_connection_age_grace(max_connection_age_grace)
        .concurrency_limit_per_connection(1024)
}

/// Create an internal gRPC server with all services (Repl, Propose, Deploy, Lsp)
///
/// This function creates a gRPC server that includes all available services:
/// - REPL service for executing Rholang code
/// - Propose service for block proposals
/// - Deploy service for deploying contracts and querying blocks
/// - LSP service for code validation
///
/// Returns a router that can be started with `GrpcServer::start_with_router`.
///
/// # Arguments
/// * `repl_grpc_service` - REPL service implementation
/// * `deploy_grpc_service` - Deploy service implementation
/// * `propose_grpc_service` - Propose service implementation
/// * `lsp_grpc_service` - LSP service implementation
/// * `max_message_size` - Maximum inbound message size in bytes
/// * `keep_alive_time` - Duration for keep-alive ping interval
/// * `keep_alive_timeout` - Duration to wait for keep-alive ping acknowledgment
/// * `tcp_keepalive_time` - TCP keep-alive duration
/// * `request_timeout` - Per-request timeout
/// * `max_connection_age` - Maximum age of a connection before it is recycled
/// * `max_connection_age_grace` - Grace period for closing connections after max_connection_age
pub async fn acquire_internal_server(
    repl_grpc_service: ReplGrpcServiceImpl,
    deploy_grpc_service: DeployGrpcServiceV1Impl,
    propose_grpc_service: ProposeGrpcServiceV1Impl,
    lsp_grpc_service: LspGrpcServiceImpl,
    max_message_size: usize,
    keep_alive_time: Duration,
    keep_alive_timeout: Duration,
    tcp_keepalive_time: Duration,
    request_timeout: Duration,
    max_connection_age: Duration,
    max_connection_age_grace: Duration,
) -> Result<tonic::transport::server::Router, Box<dyn std::error::Error + Send + Sync>> {
    // Create adapter wrappers that implement the proto-generated server traits
    // Note: These adapters need to be implemented separately to bridge between
    // the trait-based service implementations and the proto-generated server traits
    let repl_server = ReplServer::new(repl_grpc_service);
    let lsp_server = LspServer::new(lsp_grpc_service);
    let propose_server = ProposeServiceServer::new(propose_grpc_service);
    let deploy_server = DeployServiceServer::new(deploy_grpc_service);

    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()?;

    // Build the server router with all services
    let router = configure_server(
        max_message_size,
        keep_alive_time,
        keep_alive_timeout,
        tcp_keepalive_time,
        request_timeout,
        max_connection_age,
        max_connection_age_grace,
    )
    .add_service(repl_server)
    .add_service(lsp_server)
    .add_service(deploy_server)
    .add_service(propose_server)
    .add_service(reflection_service);

    Ok(router)
}

/// Create an external gRPC server with only the Deploy service
///
/// This function creates a gRPC server that only includes the Deploy service,
/// intended for external access without internal administrative services.
///
/// Returns a router that can be started with `GrpcServer::start_with_router`.
///
/// # Arguments
/// * `deploy_grpc_service` - Deploy service implementation
/// * `max_message_size` - Maximum inbound message size in bytes
/// * `keep_alive_time` - Duration for keep-alive ping interval
/// * `keep_alive_timeout` - Duration to wait for keep-alive ping acknowledgment
/// * `tcp_keepalive_time` - TCP keep-alive duration
/// * `request_timeout` - Per-request timeout
/// * `max_connection_age` - Maximum age of a connection before it is recycled
/// * `max_connection_age_grace` - Grace period for closing connections after max_connection_age
pub fn acquire_external_server(
    deploy_grpc_service: DeployGrpcServiceV1Impl,
    max_message_size: usize,
    keep_alive_time: Duration,
    keep_alive_timeout: Duration,
    tcp_keepalive_time: Duration,
    request_timeout: Duration,
    max_connection_age: Duration,
    max_connection_age_grace: Duration,
) -> Result<tonic::transport::server::Router, Box<dyn std::error::Error + Send + Sync>> {
    // Create adapter wrappers that implement the proto-generated server traits
    // Note: These adapters need to be implemented separately to bridge between
    // the trait-based service implementations and the proto-generated server traits
    let deploy_server = DeployServiceServer::new(deploy_grpc_service);

    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()?;

    // Build the server router with all services
    let router = configure_server(
        max_message_size,
        keep_alive_time,
        keep_alive_timeout,
        tcp_keepalive_time,
        request_timeout,
        max_connection_age,
        max_connection_age_grace,
    )
    .add_service(deploy_server)
    .add_service(reflection_service);

    Ok(router)
}

#[cfg(test)]
mod tests {
    use std::panic;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tonic::transport::server::TcpIncoming;
    use tonic::transport::Endpoint;

    use super::{configure_server, FILE_DESCRIPTOR_SET};

    const RESUMED_AFTER_COMPLETION: &str = "resumed after completion";
    const MAX_CONNECTION_AGE: Duration = Duration::from_millis(300);
    const MAX_CONNECTION_AGE_GRACE: Duration = Duration::from_secs(5);
    const CONNECTIONS: usize = 4;

    /// A connection reaching `max_connection_age` must not panic the task serving
    /// it. tonic's serve loop polls its connection-timeout future again after the
    /// graceful-shutdown branch runs, so that future must never be able to
    /// complete without also terminating the loop.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn connection_age_expiry_does_not_panic_the_serving_task() {
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let original = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let rendered = info.to_string();
            if rendered.contains(RESUMED_AFTER_COMPLETION) {
                if let Ok(mut hits) = sink.lock() {
                    hits.push(rendered);
                }
            }
            original(info);
        }));

        let incoming =
            TcpIncoming::bind("127.0.0.1:0".parse().unwrap()).expect("bind ephemeral port");
        let addr = incoming.local_addr().expect("resolve bound address");

        let reflection_service = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
            .build_v1()
            .expect("build reflection service");

        let router = configure_server(
            4 * 1024 * 1024,
            Duration::from_secs(10),
            Duration::from_secs(5),
            Duration::from_secs(10),
            Duration::from_secs(30),
            MAX_CONNECTION_AGE,
            MAX_CONNECTION_AGE_GRACE,
        )
        .add_service(reflection_service);

        let server = tokio::spawn(async move { router.serve_with_incoming(incoming).await });

        let mut channels = Vec::with_capacity(CONNECTIONS);
        for _ in 0..CONNECTIONS {
            channels.push(
                Endpoint::from_shared(format!("http://{addr}"))
                    .expect("valid endpoint")
                    .connect()
                    .await
                    .expect("connect to test server"),
            );
        }

        tokio::time::sleep(MAX_CONNECTION_AGE * 4).await;

        drop(channels);
        server.abort();

        let panics = captured.lock().expect("panic capture not poisoned").clone();
        let _ = panic::take_hook();

        assert!(
            panics.is_empty(),
            "serving task panicked when connections reached max_connection_age \
             ({} panic(s) across {CONNECTIONS} connections): {panics:#?}",
            panics.len(),
        );
    }
}
