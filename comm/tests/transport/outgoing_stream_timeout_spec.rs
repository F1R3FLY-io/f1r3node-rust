use std::collections::HashMap;
use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use comm::rust::errors::CommError;
use comm::rust::transport::f1r3fly_connector::F1r3flyConnector;
use comm::rust::transport::f1r3fly_server::F1r3flyServer;
use comm::rust::transport::grpc_transport::GrpcTransport;
use comm::rust::transport::grpc_transport_client::GrpcTransportClient;
use comm::rust::transport::ssl_session_client_interceptor::SslSessionClientInterceptor;
use comm::rust::transport::transport_layer::{Blob, TransportLayer};
use models::routing::transport_layer_client::TransportLayerClient;
use models::routing::transport_layer_server::{
    TransportLayer as ServerTransportLayer, TransportLayerServer,
};
use models::routing::{Chunk, Packet, TlRequest, TlResponse};
use prost::bytes::Bytes;
use tokio::sync::{Mutex, Notify};
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

use super::transport_layer_runtime::{get_free_port, TransportLayerTestRuntime};

struct SilentPeer {
    requests_seen: Arc<AtomicUsize>,
    request_started: Arc<Notify>,
    first_request_dropped: Arc<Notify>,
}

struct FirstRequestGuard(Arc<Notify>);

impl Drop for FirstRequestGuard {
    fn drop(&mut self) { self.0.notify_one(); }
}

#[tonic::async_trait]
impl ServerTransportLayer for SilentPeer {
    async fn send(&self, _: Request<TlRequest>) -> Result<Response<TlResponse>, Status> {
        Ok(Response::new(TlResponse::default()))
    }

    async fn stream(
        &self,
        _: Request<tonic::Streaming<Chunk>>,
    ) -> Result<Response<TlResponse>, Status> {
        let first_request = self.requests_seen.fetch_add(1, Ordering::SeqCst) == 0;
        let _guard = first_request.then(|| FirstRequestGuard(self.first_request_dropped.clone()));
        self.request_started.notify_one();
        pending().await
    }
}

#[tokio::test]
async fn timed_out_stream_does_not_block_the_next_queued_stream() {
    let runtime = TransportLayerTestRuntime::new("test".to_string());
    let local = runtime
        .create_environment(get_free_port().await.unwrap())
        .await
        .unwrap();
    let remote = runtime
        .create_environment(get_free_port().await.unwrap())
        .await
        .unwrap();

    let requests_seen = Arc::new(AtomicUsize::new(0));
    let request_started = Arc::new(Notify::new());
    let first_request_dropped = Arc::new(Notify::new());
    let peer = SilentPeer {
        requests_seen: requests_seen.clone(),
        request_started: request_started.clone(),
        first_request_dropped: first_request_dropped.clone(),
    };
    let address = format!("127.0.0.1:{}", remote.port).parse().unwrap();
    let incoming = F1r3flyServer::builder("test".to_string(), &remote.cert, &remote.key, address)
        .unwrap()
        .incoming()
        .await
        .unwrap();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(TransportLayerServer::new(peer))
            .serve_with_incoming(incoming)
            .await
    });

    let client = GrpcTransportClient::new(
        "test".to_string(),
        local.cert,
        local.key,
        runtime.max_message_size,
        runtime.max_message_size,
        8,
        Arc::new(Mutex::new(HashMap::new())),
        Duration::from_secs(1),
    )
    .unwrap();
    let make_blob = |type_id: &str| Blob {
        sender: local.peer.clone(),
        packet: Packet {
            type_id: type_id.to_string(),
            content: Bytes::from_static(b"probe"),
        },
    };

    client
        .stream(&remote.peer, &make_blob("first"))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), request_started.notified())
        .await
        .unwrap();
    assert_eq!(requests_seen.load(Ordering::SeqCst), 1);

    client
        .stream(&remote.peer, &make_blob("second"))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), request_started.notified())
        .await
        .unwrap();
    assert_eq!(requests_seen.load(Ordering::SeqCst), 2);
    tokio::time::timeout(Duration::from_secs(15), first_request_dropped.notified())
        .await
        .unwrap();

    client.disconnect(&remote.peer).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn tls_stream_observes_both_receiver_and_sender_deadlines() {
    let runtime = TransportLayerTestRuntime::new("test".to_string());
    let local = runtime
        .create_environment(get_free_port().await.unwrap())
        .await
        .unwrap();
    let packet = Packet {
        type_id: "deadline".to_string(),
        content: Bytes::from(vec![0; 6 * 1024 * 1024]),
    };

    for (server_timeout, client_timeout, receiver_wins) in [
        (
            Duration::from_millis(200),
            Duration::from_secs(2),
            Some(true),
        ),
        (
            Duration::from_secs(2),
            Duration::from_millis(200),
            Some(false),
        ),
        (Duration::from_millis(300), Duration::from_millis(300), None),
    ] {
        let remote = runtime
            .create_environment(get_free_port().await.unwrap())
            .await
            .unwrap();
        let requests_seen = Arc::new(AtomicUsize::new(0));
        let peer = SilentPeer {
            requests_seen: requests_seen.clone(),
            request_started: Arc::new(Notify::new()),
            first_request_dropped: Arc::new(Notify::new()),
        };
        let address = format!("127.0.0.1:{}", remote.port).parse().unwrap();
        let incoming =
            F1r3flyServer::builder("test".to_string(), &remote.cert, &remote.key, address)
                .unwrap()
                .incoming()
                .await
                .unwrap();
        let server = tokio::spawn(async move {
            Server::builder()
                .timeout(server_timeout)
                .add_service(TransportLayerServer::new(peer))
                .serve_with_incoming(incoming)
                .await
        });

        let connector = F1r3flyConnector::new_with_timeout(
            "test".to_string(),
            &local.cert,
            &local.key,
            hex::encode(&remote.peer.id.key),
            Duration::from_secs(5),
        )
        .unwrap();
        let channel = Channel::from_shared(format!("http://127.0.0.1:{}/", remote.port))
            .unwrap()
            .connect_with_connector(connector)
            .await
            .unwrap();
        let service = InterceptedService::new(
            channel,
            SslSessionClientInterceptor::new("test".to_string()),
        );
        let mut client = TransportLayerClient::new(service);
        let blob = Blob {
            sender: local.peer.clone(),
            packet: packet.clone(),
        };
        let start = tokio::time::Instant::now();
        let outcome = tokio::time::timeout(
            client_timeout,
            GrpcTransport::stream(&mut client, &remote.peer, "test", &blob, 64 * 1024),
        )
        .await;

        match receiver_wins {
            Some(true) => {
                assert!(
                    matches!(outcome, Ok(Err(CommError::TimeOut))),
                    "{outcome:?}"
                );
                assert!(start.elapsed() < client_timeout);
            }
            Some(false) => {
                assert!(outcome.is_err());
                assert!(start.elapsed() >= client_timeout);
            }
            None => {
                assert!(
                    matches!(outcome, Ok(Err(CommError::TimeOut)) | Err(_)),
                    "{outcome:?}"
                );
                assert!(start.elapsed() >= Duration::from_millis(250));
            }
        }
        assert_eq!(requests_seen.load(Ordering::SeqCst), 1);
        server.abort();
    }
}
