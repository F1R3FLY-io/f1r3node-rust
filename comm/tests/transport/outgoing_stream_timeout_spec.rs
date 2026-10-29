use std::collections::HashMap;
use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use comm::rust::transport::f1r3fly_server::F1r3flyServer;
use comm::rust::transport::grpc_transport_client::GrpcTransportClient;
use comm::rust::transport::transport_layer::{Blob, TransportLayer};
use models::routing::transport_layer_server::{
    TransportLayer as ServerTransportLayer, TransportLayerServer,
};
use models::routing::{Chunk, Packet, TlRequest, TlResponse};
use prost::bytes::Bytes;
use tokio::sync::{Mutex, Notify};
use tonic::transport::Server;
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
