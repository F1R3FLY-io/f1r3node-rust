use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{stream, Stream};
use models::casper::v1::deploy_service_client::DeployServiceClient;
use models::casper::v1::deploy_service_server::DeployService;
use models::casper::v1::{
    BlockInfoResponse, BlockResponse, BondStatusResponse, ContinuationAtNameResponse,
    DeployFinalizationStatusResponse, DeployResponse, EventInfoResponse, ExploratoryDeployResponse,
    FindDeployResponse, IsFinalizedResponse, LastFinalizedBlockResponse, MachineVerifyResponse,
    PrivateNamePreviewResponse, RhoDataResponse, StatusResponse, VisualizeBlocksResponse,
};
use models::casper::{
    BlockQuery, BlocksQuery, BlocksQueryByHeight, BondStatusQuery, ContinuationAtNameQuery,
    DataAtNameByBlockQuery, DeployDataProto, DeployFinalizationStatusQuery, ExploratoryDeployQuery,
    FindDeployQuery, IsFinalizedQuery, LastFinalizedBlockQuery, MachineVerifyQuery,
    PrivateNamePreviewQuery, ReportQuery, VisualizeDagQuery,
};
use node::rust::api::grpc_package::acquire_external_server;
use tonic::{Code, Request, Response, Status};

type TestStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

#[derive(Clone)]
struct DeployProbe {
    calls: Arc<AtomicUsize>,
}

macro_rules! impl_deploy_probe {
    (
        unary { $( $unary_name:ident : $unary_request:ty => $unary_response:ty ),* $(,)? }
        stream { $( $stream_name:ident : $stream_request:ty => $stream_type:ident ),* $(,)? }
    ) => {
        #[async_trait::async_trait]
        impl DeployService for DeployProbe {
            type visualizeDagStream = TestStream<VisualizeBlocksResponse>;
            type showMainChainStream = TestStream<BlockInfoResponse>;
            type getBlocksStream = TestStream<BlockInfoResponse>;
            type getBlocksByHeightsStream = TestStream<BlockInfoResponse>;

            async fn do_deploy(
                &self,
                _: Request<DeployDataProto>,
            ) -> Result<Response<DeployResponse>, Status> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(Response::new(DeployResponse::default()))
            }

            $(
                async fn $unary_name(
                    &self,
                    _: Request<$unary_request>,
                ) -> Result<Response<$unary_response>, Status> {
                    Err(Status::unimplemented(stringify!($unary_name)))
                }
            )*

            $(
                async fn $stream_name(
                    &self,
                    _: Request<$stream_request>,
                ) -> Result<Response<Self::$stream_type>, Status> {
                    Err(Status::unimplemented(stringify!($stream_name)))
                }
            )*
        }
    };
}

impl_deploy_probe! {
    unary {
        get_block: BlockQuery => BlockResponse,
        machine_verifiable_dag: MachineVerifyQuery => MachineVerifyResponse,
        get_data_at_name: DataAtNameByBlockQuery => RhoDataResponse,
        listen_for_continuation_at_name: ContinuationAtNameQuery => ContinuationAtNameResponse,
        find_deploy: FindDeployQuery => FindDeployResponse,
        preview_private_names: PrivateNamePreviewQuery => PrivateNamePreviewResponse,
        last_finalized_block: LastFinalizedBlockQuery => LastFinalizedBlockResponse,
        is_finalized: IsFinalizedQuery => IsFinalizedResponse,
        deploy_finalization_status: DeployFinalizationStatusQuery => DeployFinalizationStatusResponse,
        bond_status: BondStatusQuery => BondStatusResponse,
        exploratory_deploy: ExploratoryDeployQuery => ExploratoryDeployResponse,
        get_event_by_hash: ReportQuery => EventInfoResponse,
        status: () => StatusResponse,
    }
    stream {
        visualize_dag: VisualizeDagQuery => visualizeDagStream,
        show_main_chain: BlocksQuery => showMainChainStream,
        get_blocks: BlocksQuery => getBlocksStream,
        get_blocks_by_heights: BlocksQueryByHeight => getBlocksByHeightsStream,
    }
}

#[tokio::test]
async fn external_router_rejects_oversized_protobuf_before_dispatch() {
    const MAX_MESSAGE_SIZE: usize = 256;

    let calls = Arc::new(AtomicUsize::new(0));
    let router = acquire_external_server(
        DeployProbe {
            calls: Arc::clone(&calls),
        },
        MAX_MESSAGE_SIZE,
        Duration::from_secs(60),
        Duration::from_secs(20),
        Duration::from_secs(10),
        Duration::from_secs(60),
        Duration::from_secs(300),
        Duration::from_secs(30),
    )
    .expect("external router");

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

    let mut client = DeployServiceClient::connect(format!("http://{address}"))
        .await
        .expect("connect probe client");

    client
        .do_deploy(DeployDataProto {
            term: "@0!(0)".to_owned(),
            ..DeployDataProto::default()
        })
        .await
        .expect("under-limit request reaches handler");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let error = client
        .do_deploy(DeployDataProto {
            term: "x".repeat(MAX_MESSAGE_SIZE * 4),
            ..DeployDataProto::default()
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
