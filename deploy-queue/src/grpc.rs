use std::collections::HashSet;

use models::casper::v1::deploy_service_client::DeployServiceClient;
use models::casper::v1::deploy_service_server::DeployService;
use models::casper::v1::{
    deploy_finalization_status_response, deploy_response, BlockInfoResponse, BlockResponse,
    BondStatusResponse, ContinuationAtNameResponse, DeployFinalizationStatusResponse,
    DeployResponse, EventInfoResponse, ExploratoryDeployResponse, FindDeployResponse,
    IsFinalizedResponse, LastFinalizedBlockResponse, MachineVerifyResponse, PendingDeploysResponse,
    PrivateNamePreviewResponse, RhoDataResponse, StatusResponse, VisualizeBlocksResponse,
};
use models::casper::{
    BlockQuery, BlocksQuery, BlocksQueryByHeight, BondStatusQuery, ContinuationAtNameQuery,
    DataAtNameByBlockQuery, DeployDataProto, DeployFinalizationStateProto,
    DeployFinalizationStatusInfo, DeployFinalizationStatusQuery, ExploratoryDeployQuery,
    FindDeployQuery, IsFinalizedQuery, LastFinalizedBlockQuery, MachineVerifyQuery,
    PendingDeploysQuery, PrivateNamePreviewQuery, ReportQuery, VisualizeDagQuery,
};
use models::deployqueue::v1::deploy_queue_server::DeployQueue;
use models::deployqueue::v1::{DeployTransition, WatchDeploysQuery};
use models::rust::casper::protocol::casper_message::DeployData;
use models::servicemodelapi::ServiceError;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;
use tonic::{Request, Response, Status, Streaming};

use crate::engine::EngineHandle;
use crate::error::IntakeError;
use crate::http::accept;
use crate::record::{DeployView, QueueState, Sig};

/// Matches the node's default `grpc-max-recv-message-size`.
pub const MAX_MESSAGE_BYTES: usize = 16_000_000;
const WATCH_BUFFER: usize = 64;

#[derive(Clone)]
pub struct QueueGrpc {
    engine: EngineHandle,
    node: DeployServiceClient<Channel>,
}

impl QueueGrpc {
    pub fn new(engine: EngineHandle, node_channel: Channel) -> Self {
        Self {
            engine,
            node: DeployServiceClient::new(node_channel)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES),
        }
    }

    fn node(&self) -> DeployServiceClient<Channel> { self.node.clone() }
}

fn deploy_error(message: String) -> Result<Response<DeployResponse>, Status> {
    Ok(Response::new(DeployResponse {
        message: Some(deploy_response::Message::Error(ServiceError {
            messages: vec![message],
        })),
    }))
}

/// Carries the refusal's wait as `retry-after` metadata, in seconds, the way
/// the HTTP API carries it in the `Retry-After` header.
fn with_retry_after(mut status: Status, error: &IntakeError) -> Status {
    if let Some(secs) = error.retry_after_secs() {
        status.metadata_mut().insert(
            "retry-after",
            secs.to_string()
                .parse()
                .expect("a decimal number is valid ASCII metadata"),
        );
    }
    status
}

fn state_proto(state: QueueState) -> DeployFinalizationStateProto {
    match state {
        QueueState::Queued | QueueState::Submitted | QueueState::InBlock => {
            DeployFinalizationStateProto::DeployStatePending
        }
        QueueState::Finalized => DeployFinalizationStateProto::DeployStateFinalized,
        QueueState::Failed | QueueState::Rejected => {
            DeployFinalizationStateProto::DeployStateFailed
        }
        QueueState::Expired => DeployFinalizationStateProto::DeployStateExpired,
    }
}

#[async_trait::async_trait]
impl DeployService for QueueGrpc {
    type getBlocksByHeightsStream = Streaming<BlockInfoResponse>;
    type getBlocksStream = Streaming<BlockInfoResponse>;
    type showMainChainStream = Streaming<BlockInfoResponse>;
    type visualizeDagStream = Streaming<VisualizeBlocksResponse>;

    /// Answers as the node's `doDeploy` does: refusals the node would also
    /// make come back as a `ServiceError`. Capacity and availability
    /// refusals, which the node has no counterpart for, are gRPC statuses a
    /// client can retry on.
    async fn do_deploy(
        &self,
        request: Request<DeployDataProto>,
    ) -> Result<Response<DeployResponse>, Status> {
        let deploy = match DeployData::from_proto(request.into_inner()) {
            Ok(deploy) => deploy,
            Err(message) => return deploy_error(message),
        };
        match accept(&self.engine, deploy).await {
            Ok(sig) => Ok(Response::new(DeployResponse {
                message: Some(deploy_response::Message::Result(format!(
                    "Success!\nDeployId is: {}",
                    hex::encode(sig)
                ))),
            })),
            Err(
                error @ (IntakeError::IllegalArgument(_)
                | IntakeError::RholangBadTerm(_)
                | IntakeError::DeployExpired(_)
                | IntakeError::Duplicate(_)),
            ) => deploy_error(error.to_string()),
            Err(error @ IntakeError::QueueFull { .. }) => Err(with_retry_after(
                Status::resource_exhausted(error.to_string()),
                &error,
            )),
            Err(error @ (IntakeError::NodeUnavailable { .. } | IntakeError::Stopped)) => Err(
                with_retry_after(Status::unavailable(error.to_string()), &error),
            ),
            Err(error @ IntakeError::Store(_)) => Err(Status::internal(error.to_string())),
        }
    }

    async fn deploy_finalization_status(
        &self,
        request: Request<DeployFinalizationStatusQuery>,
    ) -> Result<Response<DeployFinalizationStatusResponse>, Status> {
        let sig = request.get_ref().deploy_sig.to_vec();
        let view = self
            .engine
            .status(sig)
            .await
            .map_err(|e| Status::unavailable(e.to_string()))?;
        match view {
            Some(view) => Ok(Response::new(DeployFinalizationStatusResponse {
                message: Some(deploy_finalization_status_response::Message::Status(
                    DeployFinalizationStatusInfo {
                        state: state_proto(view.state) as i32,
                        rejection_count: view.rejection_count,
                        latest_block_hash: view.latest_block_hash.map(Into::into),
                    },
                )),
            })),
            None => {
                self.node()
                    .deploy_finalization_status(request.into_inner())
                    .await
            }
        }
    }

    async fn get_block(
        &self,
        request: Request<BlockQuery>,
    ) -> Result<Response<BlockResponse>, Status> {
        self.node().get_block(request.into_inner()).await
    }

    async fn visualize_dag(
        &self,
        request: Request<VisualizeDagQuery>,
    ) -> Result<Response<Self::visualizeDagStream>, Status> {
        self.node().visualize_dag(request.into_inner()).await
    }

    async fn machine_verifiable_dag(
        &self,
        request: Request<MachineVerifyQuery>,
    ) -> Result<Response<MachineVerifyResponse>, Status> {
        self.node()
            .machine_verifiable_dag(request.into_inner())
            .await
    }

    async fn show_main_chain(
        &self,
        request: Request<BlocksQuery>,
    ) -> Result<Response<Self::showMainChainStream>, Status> {
        self.node().show_main_chain(request.into_inner()).await
    }

    async fn get_blocks(
        &self,
        request: Request<BlocksQuery>,
    ) -> Result<Response<Self::getBlocksStream>, Status> {
        self.node().get_blocks(request.into_inner()).await
    }

    async fn get_data_at_name(
        &self,
        request: Request<DataAtNameByBlockQuery>,
    ) -> Result<Response<RhoDataResponse>, Status> {
        self.node().get_data_at_name(request.into_inner()).await
    }

    async fn listen_for_continuation_at_name(
        &self,
        request: Request<ContinuationAtNameQuery>,
    ) -> Result<Response<ContinuationAtNameResponse>, Status> {
        self.node()
            .listen_for_continuation_at_name(request.into_inner())
            .await
    }

    async fn find_deploy(
        &self,
        request: Request<FindDeployQuery>,
    ) -> Result<Response<FindDeployResponse>, Status> {
        self.node().find_deploy(request.into_inner()).await
    }

    async fn preview_private_names(
        &self,
        request: Request<PrivateNamePreviewQuery>,
    ) -> Result<Response<PrivateNamePreviewResponse>, Status> {
        self.node()
            .preview_private_names(request.into_inner())
            .await
    }

    async fn last_finalized_block(
        &self,
        request: Request<LastFinalizedBlockQuery>,
    ) -> Result<Response<LastFinalizedBlockResponse>, Status> {
        self.node().last_finalized_block(request.into_inner()).await
    }

    async fn is_finalized(
        &self,
        request: Request<IsFinalizedQuery>,
    ) -> Result<Response<IsFinalizedResponse>, Status> {
        self.node().is_finalized(request.into_inner()).await
    }

    async fn bond_status(
        &self,
        request: Request<BondStatusQuery>,
    ) -> Result<Response<BondStatusResponse>, Status> {
        self.node().bond_status(request.into_inner()).await
    }

    async fn exploratory_deploy(
        &self,
        request: Request<ExploratoryDeployQuery>,
    ) -> Result<Response<ExploratoryDeployResponse>, Status> {
        self.node().exploratory_deploy(request.into_inner()).await
    }

    async fn get_blocks_by_heights(
        &self,
        request: Request<BlocksQueryByHeight>,
    ) -> Result<Response<Self::getBlocksByHeightsStream>, Status> {
        self.node()
            .get_blocks_by_heights(request.into_inner())
            .await
    }

    async fn get_event_by_hash(
        &self,
        request: Request<ReportQuery>,
    ) -> Result<Response<EventInfoResponse>, Status> {
        self.node().get_event_by_hash(request.into_inner()).await
    }

    async fn status(&self, request: Request<()>) -> Result<Response<StatusResponse>, Status> {
        self.node().status(request.into_inner()).await
    }

    async fn get_pending_deploys(
        &self,
        request: Request<PendingDeploysQuery>,
    ) -> Result<Response<PendingDeploysResponse>, Status> {
        self.node().get_pending_deploys(request.into_inner()).await
    }
}

fn transition(sig: &[u8], view: Option<DeployView>) -> DeployTransition {
    match view {
        Some(view) => DeployTransition {
            deploy_sig: sig.to_vec(),
            state: view.state.status_label().to_string(),
            queue_state: view.state.name().to_string(),
            rejection_count: view.rejection_count,
            latest_block_hash: view.latest_block_hash,
            detail: view.detail.unwrap_or_default(),
        },
        None => DeployTransition {
            deploy_sig: sig.to_vec(),
            state: "unknown".to_string(),
            queue_state: String::new(),
            rejection_count: 0,
            latest_block_hash: None,
            detail: String::new(),
        },
    }
}

#[async_trait::async_trait]
impl DeployQueue for QueueGrpc {
    type watchDeploysStream = ReceiverStream<Result<DeployTransition, Status>>;

    async fn watch_deploys(
        &self,
        request: Request<WatchDeploysQuery>,
    ) -> Result<Response<Self::watchDeploysStream>, Status> {
        let sigs: Vec<Sig> = request.into_inner().deploy_sigs;
        if sigs.is_empty() {
            return Err(Status::invalid_argument(
                "name at least one deploy signature",
            ));
        }
        let start = self
            .engine
            .watch(sigs)
            .await
            .map_err(|e| Status::unavailable(e.to_string()))?;
        let (sender, receiver) = mpsc::channel(WATCH_BUFFER);
        tokio::spawn(async move {
            let mut open: HashSet<Sig> = HashSet::new();
            for (sig, view) in start.current {
                if view.as_ref().is_some_and(|view| !view.state.is_terminal()) {
                    open.insert(sig.clone());
                }
                if sender.send(Ok(transition(&sig, view))).await.is_err() {
                    return;
                }
            }
            let mut updates = start.updates;
            while !open.is_empty() {
                match updates.recv().await {
                    Ok(update) if open.contains(&update.sig) => {
                        if update.view.state.is_terminal() {
                            open.remove(&update.sig);
                        }
                        if sender
                            .send(Ok(transition(&update.sig, Some(update.view))))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(RecvError::Lagged(_)) => {
                        let _ = sender
                            .send(Err(Status::data_loss(
                                "watch fell behind the queue; watch again to read current states",
                            )))
                            .await;
                        return;
                    }
                    Err(RecvError::Closed) => return,
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(receiver)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_refusals_carry_their_wait() {
        let status = with_retry_after(
            Status::resource_exhausted("full"),
            &IntakeError::QueueFull {
                retry_after_secs: 12,
            },
        );
        assert_eq!(status.metadata().get("retry-after").unwrap(), "12");
        let status = with_retry_after(Status::unavailable("stopped"), &IntakeError::Stopped);
        assert!(status.metadata().get("retry-after").is_none());
    }
}
