use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use comm::rust::transport::grpc_transport_receiver::bounded_transport_service;
use futures::stream;
use models::routing::transport_layer_client::TransportLayerClient;
use models::routing::transport_layer_server::TransportLayer;
use models::routing::{protocol, Packet, Protocol, TlRequest, TlResponse};
use prost::bytes::Bytes;
use tonic::{Code, Request, Response, Status};

#[derive(Clone)]
struct TransportProbe {
    calls: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl TransportLayer for TransportProbe {
    async fn send(&self, _: Request<TlRequest>) -> Result<Response<TlResponse>, Status> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Response::new(TlResponse::default()))
    }

    async fn stream(
        &self,
        _: Request<tonic::Streaming<models::routing::Chunk>>,
    ) -> Result<Response<TlResponse>, Status> {
        Err(Status::unimplemented("stream"))
    }
}

#[tokio::test]
async fn transport_service_rejects_oversized_protobuf_before_dispatch() {
    const MAX_MESSAGE_SIZE: usize = 256;

    let calls = Arc::new(AtomicUsize::new(0));
    let service = bounded_transport_service(
        TransportProbe {
            calls: Arc::clone(&calls),
        },
        |request: Request<()>| Ok(request),
        MAX_MESSAGE_SIZE,
    );
    let router = tonic::transport::Server::builder().add_service(service);

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind probe server");
    let address = listener.local_addr().expect("probe server address");
    let incoming = stream::try_unfold(listener, |listener| async move {
        let (socket, _) = listener.accept().await?;
        Ok::<_, std::io::Error>(Some((socket, listener)))
    });
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(router.serve_with_incoming_shutdown(incoming, async move {
        let _ = shutdown_rx.await;
    }));

    let mut client = TransportLayerClient::connect(format!("http://{address}"))
        .await
        .expect("connect probe client");

    client
        .send(TlRequest::default())
        .await
        .expect("under-limit request reaches handler");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let error = client
        .send(TlRequest {
            protocol: Some(Protocol {
                header: None,
                message: Some(protocol::Message::Packet(Packet {
                    type_id: "oversized".to_owned(),
                    content: Bytes::from(vec![0; MAX_MESSAGE_SIZE * 4]),
                })),
            }),
        })
        .await
        .expect_err("over-limit request must be rejected by tonic");
    assert_eq!(error.code(), Code::OutOfRange);
    assert!(error.message().contains("decoded message length too large"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    shutdown_tx.send(()).expect("stop probe server");
    server
        .await
        .expect("join probe server")
        .expect("serve probe");
}
