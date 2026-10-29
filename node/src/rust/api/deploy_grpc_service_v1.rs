//! Deploy gRPC Service V1 implementation
//!
//! This module provides a gRPC service for deploy functionality,
//! allowing clients to deploy contracts, query blocks, and perform various blockchain operations.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::api::block_api::{BlockAPI, ExploratoryDeployRejection};
use casper::rust::api::block_report_api::BlockReportAPI;
use casper::rust::api::graph_generator::{GraphConfig, GraphzGenerator};
use casper::rust::casper::DeployError;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::ProposeFunction;
use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::rp::connect::ConnectionsCell;
use crypto::rust::public_key::PublicKey;
use graphz::{GraphSerializer, ListSerializer};
use models::casper::v1::deploy_service_server::DeployService;
use models::casper::v1::{
    BlockInfoResponse, BlockResponse, BondStatusResponse, ContinuationAtNameResponse,
    DeployFinalizationStatusResponse, DeployResponse, EventInfoResponse, ExploratoryDeployResponse,
    FindDeployResponse, IsFinalizedResponse, LastFinalizedBlockResponse, MachineVerifyResponse,
    PendingDeploysResponse, PrivateNamePreviewResponse, RhoDataResponse, StatusResponse,
    VisualizeBlocksResponse,
};
use models::casper::{
    BlockQuery, BlocksQuery, BlocksQueryByHeight, BondStatusQuery, ContinuationAtNameQuery,
    DataAtNameByBlockQuery, DeployDataProto, DeployFinalizationStateProto,
    DeployFinalizationStatusInfo, DeployFinalizationStatusQuery, ExploratoryDeployQuery,
    FindDeployQuery, IsFinalizedQuery, LastFinalizedBlockQuery, MachineVerifyQuery,
    PendingDeployInfo, PendingDeploysQuery, PendingDeploysResponsePayload, PrivateNamePreviewQuery,
    ReportQuery, Status, VersionInfo, VisualizeDagQuery,
};
use models::servicemodelapi::ServiceError;
use tokio::time::{sleep, Duration};
use tracing::error;

use crate::rust::web::version_info::get_version_info_str;

trait IntoServiceError {
    fn into_service_error(self) -> ServiceError;
}

impl IntoServiceError for eyre::Report {
    fn into_service_error(self) -> ServiceError {
        ServiceError {
            messages: vec![self.to_string()],
        }
    }
}

impl IntoServiceError for casper::rust::api::block_report_api::BlockReportError {
    fn into_service_error(self) -> ServiceError {
        ServiceError {
            messages: vec![self.to_string()],
        }
    }
}

const FIND_DEPLOY_RETRY_INTERVAL_MS: u64 = 100;
const FIND_DEPLOY_MAX_ATTEMPTS: u8 = 80;

fn find_deploy_retry_interval_ms() -> u64 { FIND_DEPLOY_RETRY_INTERVAL_MS }

fn find_deploy_max_attempts() -> u8 { FIND_DEPLOY_MAX_ATTEMPTS }

/// Deploy gRPC Service V1 implementation
#[derive(Clone)]
pub struct DeployGrpcServiceV1Impl {
    api_max_blocks_limit: i32,
    trigger_propose_f: Option<Arc<ProposeFunction>>,
    dev_mode: bool,
    network_id: String,
    shard_id: String,
    min_phlo_price: i64,
    native_token_name: String,
    native_token_symbol: String,
    native_token_decimals: u32,
    has_validator_key: bool,
    is_node_read_only: bool,
    engine_cell: EngineCell,
    block_report_api: BlockReportAPI,
    transfer_unforgeable: models::rhoapi::Par,
    key_value_block_store: KeyValueBlockStore,
    rp_conf_cell: comm::rust::rp::rp_conf::RPConfCell,
    connections_cell: ConnectionsCell,
    node_discovery: Arc<dyn NodeDiscovery + Send + Sync>,
    epoch_length: i32,
    is_ready: Arc<AtomicBool>,
}

impl DeployGrpcServiceV1Impl {
    pub fn new(
        api_max_blocks_limit: i32,
        trigger_propose_f: Option<Arc<ProposeFunction>>,
        dev_mode: bool,
        network_id: String,
        shard_id: String,
        min_phlo_price: i64,
        native_token_name: String,
        native_token_symbol: String,
        native_token_decimals: u32,
        has_validator_key: bool,
        is_node_read_only: bool,
        engine_cell: EngineCell,
        block_report_api: BlockReportAPI,
        transfer_unforgeable: models::rhoapi::Par,
        key_value_block_store: KeyValueBlockStore,
        rp_conf_cell: comm::rust::rp::rp_conf::RPConfCell,
        connections_cell: ConnectionsCell,
        node_discovery: Arc<dyn NodeDiscovery + Send + Sync>,
        epoch_length: i32,
        is_ready: Arc<AtomicBool>,
    ) -> Self {
        Self {
            api_max_blocks_limit,
            trigger_propose_f,
            dev_mode,
            network_id,
            shard_id,
            min_phlo_price,
            native_token_name,
            native_token_symbol,
            native_token_decimals,
            has_validator_key,
            is_node_read_only,
            engine_cell,
            block_report_api,
            transfer_unforgeable,
            key_value_block_store,
            rp_conf_cell,
            connections_cell,
            node_discovery,
            epoch_length,
            is_ready,
        }
    }

    /// Enrich proto BlockInfo with transfers from BlockReportAPI.
    /// On readonly: populates deploy transfers. On validators: leaves empty (block report rejected).
    async fn enrich_proto_transfers(&self, block_info: &mut models::casper::BlockInfo) {
        let block_hash_hex = block_info
            .block_info
            .as_ref()
            .map(|bi| bi.block_hash.clone())
            .unwrap_or_default();

        if block_hash_hex.is_empty() {
            return;
        }

        let block_hash_bytes: prost::bytes::Bytes = match hex::decode(&block_hash_hex) {
            Ok(bytes) => bytes.into(),
            Err(_) => return,
        };

        // Cached when available, replayed only when the reporter is idle:
        // block_report_if_idle refuses rather than queues, so this never adds
        // to the load it would be competing with.
        match self
            .block_report_api
            .block_report_if_idle(block_hash_bytes)
            .await
        {
            Ok(report) => {
                let transfers_by_deploy =
                    crate::rust::web::block_info_enricher::extract_transfers_from_report(
                        &report,
                        &self.transfer_unforgeable,
                    );
                for deploy in &mut block_info.deploys {
                    deploy.transfers_available = true;
                    if let Some(transfers) = transfers_by_deploy.get(&deploy.sig) {
                        deploy.transfers = transfers.clone();
                    }
                }
            }
            Err(_) => {
                // Validators, and a reporter busy with another replay:
                // transfers_available stays false (proto default), transfers
                // stays empty Vec. Clients check transfers_available to
                // distinguish "no transfers" from "unavailable."
            }
        }
    }

    /// Helper function to convert errors to ServiceError
    fn create_service_error(message: String) -> ServiceError {
        ServiceError {
            messages: vec![message],
        }
    }

    /// Helper function to create a successful DeployResponse
    fn create_success_deploy_response(
        result: String,
    ) -> Result<tonic::Response<DeployResponse>, tonic::Status> {
        Ok(DeployResponse {
            message: Some(models::casper::v1::deploy_response::Message::Result(result)),
        }
        .into())
    }

    /// Helper function to create an error DeployResponse
    fn create_error_deploy_response(
        error: ServiceError,
    ) -> Result<tonic::Response<DeployResponse>, tonic::Status> {
        Ok(DeployResponse {
            message: Some(models::casper::v1::deploy_response::Message::Error(error)),
        }
        .into())
    }

    fn validate_legacy_transfer_amounts(
        block_info: &models::casper::BlockInfo,
        exact_transfer_amounts: bool,
    ) -> Result<(), ServiceError> {
        if !exact_transfer_amounts {
            for deploy in &block_info.deploys {
                for transfer in &deploy.transfers {
                    if !transfer.amount_raw.is_empty()
                        && transfer.amount_raw.parse::<i64>().is_err()
                    {
                        return Err(Self::create_service_error(format!(
                            "TRANSFER_AMOUNT_OUT_OF_RANGE: transfer amount in deploy {} cannot be represented as int64. Set exactTransferAmounts=true and read amountRaw.",
                            deploy.sig
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    fn create_block_response(
        block_info: models::casper::BlockInfo,
        exact_transfer_amounts: bool,
    ) -> Result<tonic::Response<BlockResponse>, tonic::Status> {
        if let Err(error) =
            Self::validate_legacy_transfer_amounts(&block_info, exact_transfer_amounts)
        {
            return Self::create_error_block_response(error);
        }
        Ok(BlockResponse {
            message: Some(models::casper::v1::block_response::Message::BlockInfo(
                block_info,
            )),
        }
        .into())
    }

    fn create_last_finalized_block_response(
        block_info: models::casper::BlockInfo,
        exact_transfer_amounts: bool,
    ) -> Result<tonic::Response<LastFinalizedBlockResponse>, tonic::Status> {
        use models::casper::v1::last_finalized_block_response::Message;

        let message =
            match Self::validate_legacy_transfer_amounts(&block_info, exact_transfer_amounts) {
                Ok(()) => Message::BlockInfo(block_info),
                Err(error) => Message::Error(error),
            };
        Ok(LastFinalizedBlockResponse {
            message: Some(message),
        }
        .into())
    }

    /// Helper function to create an error BlockResponse
    fn create_error_block_response(
        error: ServiceError,
    ) -> Result<tonic::Response<BlockResponse>, tonic::Status> {
        Ok(BlockResponse {
            message: Some(models::casper::v1::block_response::Message::Error(error)),
        }
        .into())
    }
}

#[async_trait::async_trait]
impl DeployService for DeployGrpcServiceV1Impl {
    type showMainChainStream = tokio_stream::wrappers::ReceiverStream<
        std::result::Result<BlockInfoResponse, tonic::Status>,
    >;

    type visualizeDagStream = tokio_stream::wrappers::ReceiverStream<
        std::result::Result<VisualizeBlocksResponse, tonic::Status>,
    >;
    type getBlocksStream = tokio_stream::wrappers::ReceiverStream<
        std::result::Result<BlockInfoResponse, tonic::Status>,
    >;
    type getBlocksByHeightsStream = tokio_stream::wrappers::ReceiverStream<
        std::result::Result<BlockInfoResponse, tonic::Status>,
    >;

    /// Deploy a contract
    #[tracing::instrument(level = "info", skip(self, request))]
    async fn do_deploy(
        &self,
        request: tonic::Request<DeployDataProto>,
    ) -> Result<tonic::Response<DeployResponse>, tonic::Status> {
        // Convert DeployDataProto to Signed<DeployData>
        let signed_deploy =
            match models::rust::casper::protocol::casper_message::DeployData::from_proto(
                request.into_inner(),
            ) {
                Ok(signed) => signed,
                Err(err_msg) => {
                    let error = Self::create_service_error(err_msg);
                    return Self::create_error_deploy_response(error);
                }
            };

        match BlockAPI::deploy(
            &self.engine_cell,
            signed_deploy,
            &self.trigger_propose_f,
            self.is_node_read_only,
            &self.shard_id,
        )
        .await
        {
            Ok(result) => Self::create_success_deploy_response(result),
            Err(e) => {
                let is_duplicate = e.chain().any(|cause| {
                    matches!(
                        cause.downcast_ref::<DeployError>(),
                        Some(DeployError::DuplicateDeploy(_))
                    )
                });
                if is_duplicate {
                    tracing::debug!("Duplicate deploy rejected: {}", e);
                } else {
                    error!("Deploy service method error do_deploy: {}", e);
                }
                Self::create_error_deploy_response(e.into_service_error())
            }
        }
    }

    /// Get a block by hash
    async fn get_block(
        &self,
        request: tonic::Request<BlockQuery>,
    ) -> Result<tonic::Response<BlockResponse>, tonic::Status> {
        let request = request.into_inner();
        match BlockAPI::get_block(&self.engine_cell, &request.hash).await {
            Ok(mut block_info) => {
                // Enrich transfers from BlockReportAPI (uses ReportStore cache).
                // On readonly: transfers populated. On validators: empty (block report rejected).
                self.enrich_proto_transfers(&mut block_info).await;
                Self::create_block_response(block_info, request.exact_transfer_amounts)
            }
            Err(e) => {
                error!("Deploy service method error get_block: {}", e);
                Self::create_error_block_response(e.into_service_error())
            }
        }
    }

    /// Visualize the DAG
    async fn visualize_dag(
        &self,
        request: tonic::Request<VisualizeDagQuery>,
    ) -> Result<tonic::Response<Self::visualizeDagStream>, tonic::Status> {
        let request = request.into_inner();

        let depth = if request.depth <= 0 {
            self.api_max_blocks_limit
        } else {
            request.depth
        };

        let config = GraphConfig {
            show_justification_lines: request.show_justification_lines,
        };
        let start_block_number = request.start_block_number;
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        let engine_cell = self.engine_cell.clone();
        let key_value_block_store = self.key_value_block_store.clone();

        tokio::spawn(async move {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let ser: Arc<dyn GraphSerializer> = Arc::new(ListSerializer::new(sender));

            match BlockAPI::visualize_dag(
                &engine_cell,
                depth,
                start_block_number,
                |ts, lfb| {
                    let ser = ser.clone();
                    let key_value_block_store = key_value_block_store.clone();
                    async move {
                        let _: graphz::Graphz = GraphzGenerator::dag_as_cluster(
                            ts,
                            lfb,
                            config,
                            ser,
                            &key_value_block_store,
                        )
                        .await?;
                        Ok(())
                    }
                },
                receiver,
            )
            .await
            {
                Ok(content) => {
                    for content_string in content {
                        let response = VisualizeBlocksResponse {
                            message: Some(
                                models::casper::v1::visualize_blocks_response::Message::Content(
                                    content_string,
                                ),
                            ),
                        };
                        if tx.send(Ok(response)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(tonic::Status::internal(e.to_string()))).await;
                }
            }
        });

        Ok(tonic::Response::new(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
    }

    /// Get machine verifiable DAG
    async fn machine_verifiable_dag(
        &self,
        request: tonic::Request<MachineVerifyQuery>,
    ) -> Result<tonic::Response<MachineVerifyResponse>, tonic::Status> {
        let _request = request.into_inner(); // maybe this parameter is should be removed in future, left for compatibility with Scala version
        match BlockAPI::machine_verifiable_dag(
            &self.engine_cell,
            self.api_max_blocks_limit,
            self.api_max_blocks_limit,
        )
        .await
        {
            Ok(content) => Ok(tonic::Response::new(MachineVerifyResponse {
                message: Some(
                    models::casper::v1::machine_verify_response::Message::Content(content),
                ),
            })),
            Err(e) => {
                error!("Deploy service method error machine_verifiable_dag: {}", e);
                Ok(tonic::Response::new(MachineVerifyResponse {
                    message: Some(models::casper::v1::machine_verify_response::Message::Error(
                        e.into_service_error(),
                    )),
                }))
            }
        }
    }

    /// Show main chain
    async fn show_main_chain(
        &self,
        request: tonic::Request<BlocksQuery>,
    ) -> Result<tonic::Response<Self::showMainChainStream>, tonic::Status> {
        let request = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        let engine_cell = self.engine_cell.clone();

        let api_max_blocks_limit = self.api_max_blocks_limit;
        tokio::spawn(async move {
            let blocks =
                BlockAPI::show_main_chain(&engine_cell, request.depth, api_max_blocks_limit).await;

            for block_info in blocks {
                let response = BlockInfoResponse {
                    message: Some(models::casper::v1::block_info_response::Message::BlockInfo(
                        block_info,
                    )),
                };
                if tx.send(Ok(response)).await.is_err() {
                    break;
                }
            }
        });

        Ok(tonic::Response::new(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
    }

    /// Get blocks
    async fn get_blocks(
        &self,
        request: tonic::Request<BlocksQuery>,
    ) -> Result<tonic::Response<Self::getBlocksStream>, tonic::Status> {
        let request = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        let engine_cell = self.engine_cell.clone();
        let api_max_blocks_limit = self.api_max_blocks_limit;

        tokio::spawn(async move {
            match BlockAPI::get_blocks(&engine_cell, request.depth, api_max_blocks_limit).await {
                Ok(blocks) => {
                    for block_info in blocks {
                        let response = BlockInfoResponse {
                            message: Some(
                                models::casper::v1::block_info_response::Message::BlockInfo(
                                    block_info,
                                ),
                            ),
                        };
                        if tx.send(Ok(response)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!("Deploy service method error get_blocks: {}", e);
                    let _ = tx.send(Err(tonic::Status::internal(e.to_string()))).await;
                }
            }
        });

        Ok(tonic::Response::new(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
    }

    /// Get data at name
    async fn get_data_at_name(
        &self,
        request: tonic::Request<DataAtNameByBlockQuery>,
    ) -> Result<tonic::Response<RhoDataResponse>, tonic::Status> {
        let request = request.into_inner();
        match BlockAPI::get_data_at_par(
            &self.engine_cell,
            &request.par.unwrap_or_default(),
            request.block_hash,
            request.use_pre_state_hash,
        )
        .await
        {
            Ok((par, block)) => {
                let payload = models::casper::v1::RhoDataPayload {
                    par,
                    block: Some(block),
                };
                Ok(tonic::Response::new(RhoDataResponse {
                    message: Some(models::casper::v1::rho_data_response::Message::Payload(
                        payload,
                    )),
                }))
            }
            Err(e) => {
                error!("Deploy service method error get_data_at_name: {}", e);
                Ok(tonic::Response::new(RhoDataResponse {
                    message: Some(models::casper::v1::rho_data_response::Message::Error(
                        e.into_service_error(),
                    )),
                }))
            }
        }
    }

    /// Listen for continuation at name
    async fn listen_for_continuation_at_name(
        &self,
        request: tonic::Request<ContinuationAtNameQuery>,
    ) -> Result<tonic::Response<ContinuationAtNameResponse>, tonic::Status> {
        let request = request.into_inner();
        match BlockAPI::get_listening_name_continuation_response(
            &self.engine_cell,
            request.depth,
            &request.names,
            self.api_max_blocks_limit,
        )
        .await
        {
            Ok((block_results, length)) => {
                let payload = models::casper::v1::ContinuationAtNamePayload {
                    block_results,
                    length,
                };
                Ok(tonic::Response::new(ContinuationAtNameResponse {
                    message: Some(
                        models::casper::v1::continuation_at_name_response::Message::Payload(
                            payload,
                        ),
                    ),
                }))
            }
            Err(e) => {
                error!(
                    "Deploy service method error listen_for_continuation_at_name: {}",
                    e
                );
                Ok(tonic::Response::new(ContinuationAtNameResponse {
                    message: Some(
                        models::casper::v1::continuation_at_name_response::Message::Error(
                            e.into_service_error(),
                        ),
                    ),
                }))
            }
        }
    }

    /// Find deploy
    async fn find_deploy(
        &self,
        request: tonic::Request<FindDeployQuery>,
    ) -> Result<tonic::Response<FindDeployResponse>, tonic::Status> {
        let request = request.into_inner();
        let retry_interval_ms = find_deploy_retry_interval_ms();
        let max_attempts = find_deploy_max_attempts();

        let mut attempt = 1;
        loop {
            match BlockAPI::find_deploy(&self.engine_cell, &request.deploy_id.to_vec()).await {
                Ok(block_info) => {
                    let known_block_hash = hex::decode(&block_info.block_hash)
                        .ok()
                        .map(prost::bytes::Bytes::from);
                    let (finalization_state, rejection_count) =
                        match BlockAPI::deploy_finalization_status_with_known_block(
                            &self.engine_cell,
                            &request.deploy_id.to_vec(),
                            known_block_hash.as_ref(),
                        )
                        .await
                        {
                            Ok(status) => (
                                deploy_state_to_proto(status.state) as i32,
                                status.rejection_count,
                            ),
                            Err(err) => {
                                tracing::warn!(
                                    "Could not compute deploy finalization status for findDeploy: {}",
                                    err
                                );
                                (0, 0)
                            }
                        };
                    return Ok(tonic::Response::new(FindDeployResponse {
                        message: Some(
                            models::casper::v1::find_deploy_response::Message::BlockInfo(
                                block_info,
                            ),
                        ),
                        finalization_state,
                        rejection_count,
                    }));
                }
                Err(e) => {
                    let not_found = e
                        .downcast_ref::<casper::rust::api::block_api::DeployNotFoundError>()
                        .is_some();
                    if !not_found || attempt >= max_attempts {
                        error!("Deploy service method error find_deploy: {}", e);
                        return Ok(tonic::Response::new(FindDeployResponse {
                            message: Some(
                                models::casper::v1::find_deploy_response::Message::Error(
                                    e.into_service_error(),
                                ),
                            ),
                            finalization_state: 0,
                            rejection_count: 0,
                        }));
                    }

                    tracing::debug!(
                        ?attempt,
                        ?max_attempts,
                        ?retry_interval_ms,
                        ?request,
                        "Waiting for deploy to become visible in block DAG"
                    );
                    sleep(Duration::from_millis(retry_interval_ms)).await;
                    attempt += 1;
                }
            }
        }
    }

    /// Preview private names
    async fn preview_private_names(
        &self,
        request: tonic::Request<PrivateNamePreviewQuery>,
    ) -> Result<tonic::Response<PrivateNamePreviewResponse>, tonic::Status> {
        let request = request.into_inner();
        match BlockAPI::preview_private_names(
            &request.user.to_vec(),
            request.timestamp,
            request.name_qty,
        ) {
            Ok(ids) => {
                let ids_bytes: Vec<prost::bytes::Bytes> =
                    ids.into_iter().map(|id| id.into()).collect();
                let payload = models::casper::v1::PrivateNamePreviewPayload { ids: ids_bytes };
                Ok(tonic::Response::new(PrivateNamePreviewResponse {
                    message: Some(
                        models::casper::v1::private_name_preview_response::Message::Payload(
                            payload,
                        ),
                    ),
                }))
            }
            Err(e) => {
                error!("Deploy service method error preview_private_names: {}", e);
                Ok(tonic::Response::new(PrivateNamePreviewResponse {
                    message: Some(
                        models::casper::v1::private_name_preview_response::Message::Error(
                            e.into_service_error(),
                        ),
                    ),
                }))
            }
        }
    }

    /// Get last finalized block
    async fn last_finalized_block(
        &self,
        request: tonic::Request<LastFinalizedBlockQuery>,
    ) -> Result<tonic::Response<LastFinalizedBlockResponse>, tonic::Status> {
        let request = request.into_inner();
        match BlockAPI::last_finalized_block(&self.engine_cell).await {
            Ok(mut block_info) => {
                self.enrich_proto_transfers(&mut block_info).await;
                Self::create_last_finalized_block_response(
                    block_info,
                    request.exact_transfer_amounts,
                )
            }
            Err(e) => {
                error!("Deploy service method error last_finalized_block: {}", e);
                Ok(tonic::Response::new(LastFinalizedBlockResponse {
                    message: Some(
                        models::casper::v1::last_finalized_block_response::Message::Error(
                            e.into_service_error(),
                        ),
                    ),
                }))
            }
        }
    }

    /// Check if block is finalized
    async fn is_finalized(
        &self,
        request: tonic::Request<IsFinalizedQuery>,
    ) -> Result<tonic::Response<IsFinalizedResponse>, tonic::Status> {
        let request = request.into_inner();
        match BlockAPI::is_finalized(&self.engine_cell, &request.hash).await {
            Ok(is_finalized) => Ok(tonic::Response::new(IsFinalizedResponse {
                message: Some(
                    models::casper::v1::is_finalized_response::Message::IsFinalized(is_finalized),
                ),
            })),
            Err(e) => {
                error!("Deploy service method error is_finalized: {}", e);
                Ok(tonic::Response::new(IsFinalizedResponse {
                    message: Some(models::casper::v1::is_finalized_response::Message::Error(
                        e.into_service_error(),
                    )),
                }))
            }
        }
    }

    /// Query the finalization status of a deploy by its signature.
    async fn deploy_finalization_status(
        &self,
        request: tonic::Request<DeployFinalizationStatusQuery>,
    ) -> Result<tonic::Response<DeployFinalizationStatusResponse>, tonic::Status> {
        let request = request.into_inner();
        match casper::rust::api::block_api::BlockAPI::deploy_finalization_status(
            &self.engine_cell,
            &request.deploy_sig,
        )
        .await
        {
            Ok(status) => Ok(tonic::Response::new(DeployFinalizationStatusResponse {
                message: Some(
                    models::casper::v1::deploy_finalization_status_response::Message::Status(
                        DeployFinalizationStatusInfo {
                            state: deploy_state_to_proto(status.state) as i32,
                            rejection_count: status.rejection_count,
                            latest_block_hash: status.latest_block_hash,
                        },
                    ),
                ),
            })),
            Err(e) => {
                error!(
                    "Deploy service method error deploy_finalization_status: {}",
                    e
                );
                Ok(tonic::Response::new(DeployFinalizationStatusResponse {
                    message: Some(
                        models::casper::v1::deploy_finalization_status_response::Message::Error(
                            e.into_service_error(),
                        ),
                    ),
                }))
            }
        }
    }

    /// Bulk list of pending deploys (deploy_storage + rejected-recovery
    /// buffer), optionally filtered by deployer public key. Empty
    /// response on read-only nodes (Casper not initialised).
    async fn get_pending_deploys(
        &self,
        request: tonic::Request<PendingDeploysQuery>,
    ) -> Result<tonic::Response<PendingDeploysResponse>, tonic::Status> {
        let request = request.into_inner();
        let deployer = if request.deployer_pubkey.is_empty() {
            None
        } else {
            Some(request.deployer_pubkey.as_ref())
        };

        match BlockAPI::list_pending_deploys(&self.engine_cell, deployer).await {
            Ok(snapshot) => {
                let deploys: Vec<PendingDeployInfo> = snapshot
                    .deploys
                    .into_iter()
                    .map(|(signed, is_rejected)| PendingDeployInfo {
                        deploy: Some(
                            models::rust::casper::protocol::casper_message::DeployData::to_proto(
                                signed,
                            ),
                        ),
                        is_rejected,
                    })
                    .collect();
                let payload = PendingDeploysResponsePayload {
                    deploys,
                    total_available: snapshot.total_available,
                };
                Ok(tonic::Response::new(PendingDeploysResponse {
                    message: Some(
                        models::casper::v1::pending_deploys_response::Message::Payload(payload),
                    ),
                }))
            }
            Err(e) => {
                error!("Deploy service method error get_pending_deploys: {}", e);
                Ok(tonic::Response::new(PendingDeploysResponse {
                    message: Some(
                        models::casper::v1::pending_deploys_response::Message::Error(
                            e.into_service_error(),
                        ),
                    ),
                }))
            }
        }
    }

    /// Get bond status
    async fn bond_status(
        &self,
        request: tonic::Request<BondStatusQuery>,
    ) -> Result<tonic::Response<BondStatusResponse>, tonic::Status> {
        let request = request.into_inner();
        let pk = request.public_key.to_vec();

        if let Err(e) = PublicKey::validate_secp256k1_bytes(&pk) {
            error!("Deploy service method error bond_status: {}", e);
            return Ok(tonic::Response::new(BondStatusResponse {
                message: Some(models::casper::v1::bond_status_response::Message::Error(
                    e.into_service_error(),
                )),
            }));
        }

        match BlockAPI::bond_status(&self.engine_cell, &pk).await {
            Ok(is_bonded) => Ok(tonic::Response::new(BondStatusResponse {
                message: Some(models::casper::v1::bond_status_response::Message::IsBonded(
                    is_bonded,
                )),
            })),
            Err(e) => {
                error!("Deploy service method error bond_status: {}", e);
                Ok(tonic::Response::new(BondStatusResponse {
                    message: Some(models::casper::v1::bond_status_response::Message::Error(
                        e.into_service_error(),
                    )),
                }))
            }
        }
    }

    /// Exploratory deploy
    async fn exploratory_deploy(
        &self,
        request: tonic::Request<ExploratoryDeployQuery>,
    ) -> Result<tonic::Response<ExploratoryDeployResponse>, tonic::Status> {
        let request = request.into_inner();
        let block_hash = if request.block_hash.is_empty() {
            None
        } else {
            Some(request.block_hash.clone())
        };

        match BlockAPI::exploratory_deploy(
            &self.engine_cell,
            request.term,
            block_hash,
            request.use_pre_state_hash,
            self.dev_mode,
            None,
        )
        .await
        {
            Ok((par, block, cost)) => {
                let data_with_block_info = models::casper::DataWithBlockInfo {
                    post_block_data: par,
                    block: Some(block),
                };
                Ok(tonic::Response::new(ExploratoryDeployResponse {
                    message: Some(
                        models::casper::v1::exploratory_deploy_response::Message::Result(
                            data_with_block_info,
                        ),
                    ),
                    cost,
                }))
            }
            Err(e) => {
                error!("Deploy service method error exploratory_deploy: {}", e);
                // Backpressure and deadline overrun have exact gRPC statuses, so
                // they travel on the transport's own status channel instead of
                // being flattened into `ServiceError`'s prose. The status is
                // chosen from the error variant, so it agrees with the HTTP
                // classification of the same failure by construction.
                if let Some(rejection) = ExploratoryDeployRejection::classify(&e) {
                    return Err(match rejection {
                        ExploratoryDeployRejection::Busy { .. } => {
                            tonic::Status::unavailable(e.to_string())
                        }
                        ExploratoryDeployRejection::Timeout { .. } => {
                            tonic::Status::deadline_exceeded(e.to_string())
                        }
                    });
                }
                Ok(tonic::Response::new(ExploratoryDeployResponse {
                    message: Some(
                        models::casper::v1::exploratory_deploy_response::Message::Error(
                            e.into_service_error(),
                        ),
                    ),
                    cost: 0,
                }))
            }
        }
    }

    /// Get event by hash
    async fn get_event_by_hash(
        &self,
        request: tonic::Request<ReportQuery>,
    ) -> Result<tonic::Response<EventInfoResponse>, tonic::Status> {
        let request = request.into_inner();

        let block_hash_bytes: prost::bytes::Bytes = match hex::decode(&request.hash) {
            Ok(bytes) => bytes.into(),
            Err(_) => {
                let error = Self::create_service_error(format!(
                    "Request hash: {} is not valid hex string",
                    request.hash
                ));
                return Ok(tonic::Response::new(EventInfoResponse {
                    message: Some(models::casper::v1::event_info_response::Message::Error(
                        error,
                    )),
                }));
            }
        };

        match self
            .block_report_api
            .block_report(block_hash_bytes, request.force_replay)
            .await
        {
            Ok(block_event_info) => Ok(tonic::Response::new(EventInfoResponse {
                message: Some(models::casper::v1::event_info_response::Message::Result(
                    block_event_info,
                )),
            })),
            Err(e) => {
                error!("Deploy service method error get_event_by_hash: {}", e);
                Ok(tonic::Response::new(EventInfoResponse {
                    message: Some(models::casper::v1::event_info_response::Message::Error(
                        e.into_service_error(),
                    )),
                }))
            }
        }
    }

    /// Get blocks by heights
    async fn get_blocks_by_heights(
        &self,
        request: tonic::Request<BlocksQueryByHeight>,
    ) -> Result<tonic::Response<Self::getBlocksByHeightsStream>, tonic::Status> {
        let request = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        let engine_cell = self.engine_cell.clone();
        let api_max_blocks_limit = self.api_max_blocks_limit;

        tokio::spawn(async move {
            match BlockAPI::get_blocks_by_heights(
                &engine_cell,
                request.start_block_number,
                request.end_block_number,
                api_max_blocks_limit,
            )
            .await
            {
                Ok(blocks) => {
                    for block_info in blocks {
                        let response = BlockInfoResponse {
                            message: Some(
                                models::casper::v1::block_info_response::Message::BlockInfo(
                                    block_info,
                                ),
                            ),
                        };
                        if tx.send(Ok(response)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!("Deploy service method error get_blocks_by_heights: {}", e);
                    let _ = tx.send(Err(tonic::Status::internal(e.to_string()))).await;
                }
            }
        });

        Ok(tonic::Response::new(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
    }

    /// Get status
    async fn status(
        &self,
        _request: tonic::Request<()>,
    ) -> Result<tonic::Response<StatusResponse>, tonic::Status> {
        let rp_conf = self
            .rp_conf_cell
            .read()
            .map_err(|e| tonic::Status::internal(format!("Failed to read RPConf: {}", e)))?;
        let address = rp_conf.local.to_address();

        let connections = match self.connections_cell.read() {
            Ok(conns) => conns,
            Err(e) => {
                error!("Deploy service method error status (connections): {}", e);
                return Err(tonic::Status::internal(e.to_string()));
            }
        };

        let discovered_nodes = match self.node_discovery.peers() {
            Ok(peers) => peers,
            Err(e) => {
                error!("Deploy service method error status (discovery): {}", e);
                return Err(tonic::Status::internal(e.to_string()));
            }
        };

        let peers = connections.len() as i32;
        let nodes = discovered_nodes.len() as i32;

        // Create a set of connected peer IDs for quick lookup
        let connected_ids: std::collections::HashSet<_> =
            connections.iter().map(|p| p.id.key.clone()).collect();

        // Convert PeerNode to PeerInfo protobuf message
        let peer_list: Vec<models::casper::PeerInfo> = discovered_nodes
            .iter()
            .map(|node| models::casper::PeerInfo {
                address: node.to_address(),
                node_id: node.id.to_string(),
                host: node.endpoint.host.clone(),
                protocol_port: node.endpoint.tcp_port as i32,
                discovery_port: node.endpoint.udp_port as i32,
                is_connected: connected_ids.contains(&node.id.key),
            })
            .collect();

        let lfb_number = match BlockAPI::last_finalized_block(&self.engine_cell).await {
            Ok(block_info) => block_info
                .block_info
                .as_ref()
                .map(|bi| bi.block_number)
                .unwrap_or(-1),
            Err(_) => -1,
        };

        let is_ready = self.is_ready.load(Ordering::Relaxed);
        let current_epoch = if self.epoch_length > 0 && lfb_number >= 0 {
            lfb_number / self.epoch_length as i64
        } else {
            0
        };

        // Advertise the floor admission and validity actually enforce — the
        // chain-adopted value; local conf only until casper is up.
        let min_phlo_price = match self.engine_cell.get().await.with_casper() {
            Some(casper) => casper.casper_shard_conf().min_phlo_price,
            None => self.min_phlo_price,
        };

        let status = Status {
            version: Some(VersionInfo {
                api: "1".to_string(),
                node: get_version_info_str(),
            }),
            address,
            network_id: self.network_id.clone(),
            shard_id: self.shard_id.clone(),
            peers,
            nodes,
            min_phlo_price,
            peer_list,
            native_token_name: self.native_token_name.clone(),
            native_token_symbol: self.native_token_symbol.clone(),
            native_token_decimals: self.native_token_decimals,
            last_finalized_block_number: lfb_number,
            is_validator: self.has_validator_key,
            is_read_only: self.is_node_read_only,
            is_ready,
            current_epoch,
            epoch_length: self.epoch_length,
        };

        Ok(tonic::Response::new(StatusResponse {
            message: Some(models::casper::v1::status_response::Message::Status(status)),
        }))
    }
}

fn deploy_state_to_proto(
    state: casper::rust::api::deploy_finalization_status::DeployFinalizationState,
) -> DeployFinalizationStateProto {
    use casper::rust::api::deploy_finalization_status::DeployFinalizationState as S;
    match state {
        S::Finalized => DeployFinalizationStateProto::DeployStateFinalized,
        S::Failed => DeployFinalizationStateProto::DeployStateFailed,
        S::Pending => DeployFinalizationStateProto::DeployStatePending,
        S::Expired => DeployFinalizationStateProto::DeployStateExpired,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use casper::rust::engine::engine_cell::EngineCell;
    use casper::rust::report_store::ReportStore;
    use casper::rust::safety_oracle::CliqueOracleImpl;
    use comm::rust::peer_node::{NodeIdentifier, PeerNode};
    use comm::rust::rp::rp_conf::{RPConf, RPConfCell};
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rust::casper::protocol::casper_message::DeployData as DeployDataMessage;
    use num_bigint::BigInt;
    use prost::Message;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use tokio_stream::StreamExt;

    use super::*;

    #[derive(Clone, PartialEq, Message)]
    struct LegacyBlockQuery {
        #[prost(string, tag = "1")]
        hash: String,
    }

    #[derive(Clone, PartialEq, Message)]
    struct LegacyTransferInfo {
        #[prost(int64, tag = "3")]
        amount: i64,
        #[prost(bool, tag = "4")]
        success: bool,
    }

    #[derive(Clone, PartialEq, Message)]
    struct LegacyDeployInfo {
        #[prost(message, repeated, tag = "13")]
        transfers: Vec<LegacyTransferInfo>,
        #[prost(bool, tag = "14")]
        transfers_available: bool,
    }

    #[derive(Clone, PartialEq, Message)]
    struct LegacyBlockInfo {
        #[prost(message, repeated, tag = "2")]
        deploys: Vec<LegacyDeployInfo>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct LegacyBlockResponse {
        #[prost(oneof = "LegacyBlockResult", tags = "1, 2")]
        message: Option<LegacyBlockResult>,
    }

    #[derive(Clone, PartialEq, prost::Oneof)]
    enum LegacyBlockResult {
        #[prost(message, tag = "1")]
        Error(ServiceError),
        #[prost(message, tag = "2")]
        BlockInfo(LegacyBlockInfo),
    }

    fn transfer_block(amount: &BigInt, success: bool) -> models::casper::BlockInfo {
        models::casper::BlockInfo {
            deploys: vec![models::casper::DeployInfo {
                sig: "synthetic-deploy".to_string(),
                transfers_available: true,
                transfers: vec![
                    models::casper::TransferInfo {
                        amount: 7,
                        amount_raw: "7".to_string(),
                        success: true,
                        ..Default::default()
                    },
                    models::casper::TransferInfo {
                        amount: crate::rust::web::transaction::legacy_amount(amount),
                        amount_raw: amount.to_string(),
                        success,
                        fail_reason: if success {
                            String::new()
                        } else {
                            "Transfer amount rejected".to_string()
                        },
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn block_response_bytes(
        block: models::casper::BlockInfo,
        exact_transfer_amounts: bool,
    ) -> [Vec<u8>; 2] {
        [
            DeployGrpcServiceV1Impl::create_block_response(block.clone(), exact_transfer_amounts)
                .unwrap()
                .into_inner()
                .encode_to_vec(),
            DeployGrpcServiceV1Impl::create_last_finalized_block_response(
                block,
                exact_transfer_amounts,
            )
            .unwrap()
            .into_inner()
            .encode_to_vec(),
        ]
    }

    #[test]
    fn legacy_block_queries_do_not_opt_into_exact_transfer_amounts() {
        let encoded = LegacyBlockQuery {
            hash: "aabbccddeeff".to_string(),
        }
        .encode_to_vec();
        let query = BlockQuery::decode(encoded.as_slice()).unwrap();
        assert_eq!(query.hash, "aabbccddeeff");
        assert!(!query.exact_transfer_amounts);
        assert!(
            !LastFinalizedBlockQuery::decode(&[][..])
                .unwrap()
                .exact_transfer_amounts
        );

        let query = BlockQuery {
            exact_transfer_amounts: true,
            ..query
        };
        assert!(
            BlockQuery::decode(query.encode_to_vec().as_slice())
                .unwrap()
                .exact_transfer_amounts
        );
        let query = LastFinalizedBlockQuery {
            exact_transfer_amounts: true,
        };
        assert!(
            LastFinalizedBlockQuery::decode(query.encode_to_vec().as_slice())
                .unwrap()
                .exact_transfer_amounts
        );
    }

    #[test]
    fn legacy_block_responses_return_range_errors_without_partial_blocks() {
        let amounts: Vec<(BigInt, bool)> = vec![
            (BigInt::from(1) << 63, true),
            (BigInt::from(10_000_000_000_000_000_000u64), true),
            ((BigInt::from(1) << 256) - 1, true),
            (BigInt::from(i64::MIN) - 1, false),
            (BigInt::from(1) << 512, false),
        ];
        for (amount, success) in amounts {
            for encoded in block_response_bytes(transfer_block(&amount, success), false) {
                let decoded = LegacyBlockResponse::decode(encoded.as_slice()).unwrap();
                let Some(LegacyBlockResult::Error(error)) = decoded.message else {
                    panic!("legacy response must reject {amount}");
                };
                assert!(error.messages[0].starts_with("TRANSFER_AMOUNT_OUT_OF_RANGE:"));
                assert!(error.messages[0].contains("exactTransferAmounts=true"));
            }
        }
    }

    #[test]
    fn legacy_block_responses_preserve_representable_transfer_amounts() {
        for amount in [i64::MIN, -1, 0, 1, (1 << 53) + 1, i64::MAX] {
            let success = amount > 0;
            for encoded in
                block_response_bytes(transfer_block(&BigInt::from(amount), success), false)
            {
                let decoded = LegacyBlockResponse::decode(encoded.as_slice()).unwrap();
                let Some(LegacyBlockResult::BlockInfo(block)) = decoded.message else {
                    panic!("legacy response must preserve {amount}");
                };
                assert!(block.deploys[0].transfers_available);
                assert_eq!(block.deploys[0].transfers.len(), 2);
                assert_eq!(block.deploys[0].transfers[0].amount, 7);
                assert_eq!(block.deploys[0].transfers[1].amount, amount);
                assert_eq!(block.deploys[0].transfers[1].success, success);
            }
        }
    }

    #[test]
    fn exact_block_responses_preserve_transfer_amounts_and_failures() {
        let amounts: Vec<(BigInt, bool)> = vec![
            (BigInt::from(1), true),
            ((BigInt::from(1) << 53) + 1, true),
            (BigInt::from(i64::MAX), true),
            (BigInt::from(1) << 63, true),
            (BigInt::from(10_000_000_000_000_000_000u64), true),
            (BigInt::from(1) << 128, true),
            ((BigInt::from(1) << 256) - 1, true),
            (BigInt::from(1) << 512, false),
            (BigInt::from(-1) << 1024, false),
        ];
        for (amount, success) in amounts {
            let expected = transfer_block(&amount, success);
            for encoded in block_response_bytes(expected.clone(), true) {
                let decoded = BlockResponse::decode(encoded.as_slice()).unwrap();
                let Some(models::casper::v1::block_response::Message::BlockInfo(block)) =
                    decoded.message
                else {
                    panic!("exact response must preserve {amount}");
                };
                assert_eq!(block, expected);
                let json =
                    crate::rust::api::serde_types::block_info::BlockInfoSerde::try_from(block)
                        .unwrap();
                let json = serde_json::to_value(json).unwrap();
                assert_eq!(
                    json["deploys"][0]["transfers"][1]["amount"],
                    amount.to_string()
                );
                assert_eq!(json["deploys"][0]["transfers"][1]["success"], success);
            }
        }
    }

    #[test]
    fn block_amount_negotiation_preserves_report_availability() {
        for available in [false, true] {
            let block = models::casper::BlockInfo {
                deploys: vec![models::casper::DeployInfo {
                    transfers_available: available,
                    ..Default::default()
                }],
                ..Default::default()
            };
            for exact in [false, true] {
                for encoded in block_response_bytes(block.clone(), exact) {
                    let decoded = LegacyBlockResponse::decode(encoded.as_slice()).unwrap();
                    let Some(LegacyBlockResult::BlockInfo(block)) = decoded.message else {
                        panic!("a report without transfers must not produce a range error");
                    };
                    assert_eq!(block.deploys[0].transfers_available, available);
                    assert!(block.deploys[0].transfers.is_empty());
                }
            }
        }
    }

    struct StubNodeDiscovery;

    #[async_trait::async_trait]
    impl NodeDiscovery for StubNodeDiscovery {
        async fn discover(&self) -> Result<(), comm::rust::errors::CommError> { Ok(()) }

        fn peers(&self) -> Result<Vec<PeerNode>, comm::rust::errors::CommError> { Ok(vec![]) }

        fn remove_peer(&self, _peer: &PeerNode) -> Result<(), comm::rust::errors::CommError> {
            Ok(())
        }
    }

    fn service() -> DeployGrpcServiceV1Impl {
        let local = PeerNode::new(
            NodeIdentifier::new("0a0b0c0d00000000000000000000000000000000")
                .expect("valid test node ID"),
            "localhost".to_string(),
            40400,
            40404,
        );
        DeployGrpcServiceV1Impl::new(
            10,
            None,
            false,
            "testnet".to_string(),
            "root".to_string(),
            1,
            "F1R3".to_string(),
            "F1R3".to_string(),
            8,
            false,
            true,
            EngineCell::init(),
            BlockReportAPI::new(
                casper::rust::reporting_casper::noop(),
                ReportStore::new(Arc::new(InMemoryKeyValueStore::new())),
                EngineCell::init(),
                KeyValueBlockStore::new(
                    Arc::new(InMemoryKeyValueStore::new()),
                    Arc::new(InMemoryKeyValueStore::new()),
                ),
                CliqueOracleImpl,
                false,
            ),
            models::rhoapi::Par::default(),
            KeyValueBlockStore::new(
                Arc::new(InMemoryKeyValueStore::new()),
                Arc::new(InMemoryKeyValueStore::new()),
            ),
            RPConfCell::new(RPConf::new(
                local,
                "testnet".to_string(),
                None,
                Duration::from_secs(1),
                8,
                2,
            )),
            ConnectionsCell::new(),
            Arc::new(StubNodeDiscovery),
            100,
            Arc::new(AtomicBool::new(true)),
        )
    }

    fn signed_deploy_proto() -> DeployDataProto {
        let (sk, _pk) = Secp256k1.new_key_pair();
        let deploy_data = DeployDataMessage {
            term: "Nil".to_string(),
            time_stamp: 1,
            phlo_price: 1,
            phlo_limit: 1000,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        };
        let signed =
            crypto::rust::signatures::signed::Signed::create(deploy_data, Box::new(Secp256k1), sk)
                .unwrap();
        DeployDataMessage::to_proto(signed)
    }

    #[tokio::test]
    async fn block_rpc_and_http_handlers_preserve_exact_amounts_from_cached_reports() {
        use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
        use casper::rust::engine::engine_with_casper::EngineWithCasper;
        use models::casper::{
            report_proto, BlockEventInfo, DeployInfoWithEventData, ReportCommProto,
            ReportConsumeProto, ReportPhase, ReportProduceProto, ReportProto, SingleReport,
        };
        use models::rhoapi::{ListParWithRandom, Par};
        use models::rust::block_metadata::BlockMetadata;
        use rholang::rust::interpreter::rho_type::{RhoBigInt, RhoString};
        use rholang::rust::interpreter::util::vault_address::VaultAddress;
        use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
        use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
        use tower::ServiceExt;

        use crate::rust::api::admin_web_api::AdminWebApiImpl;
        use crate::rust::api::web_api::WebApiImpl;
        use crate::rust::web::shared_handlers::AppState;
        use crate::rust::web::web_api_routes::WebApiRoutes;

        let mut service = service();
        let processed =
            casper::rust::util::construct_deploy::basic_processed_deploy(1, None).unwrap();
        let from = VaultAddress::from_public_key(&processed.deploy.pk)
            .unwrap()
            .to_base58();
        let deploy_info = processed.clone().to_deploy_info();
        let mut block = models::rust::block_implicits::get_random_block_default();
        block.body.deploys = vec![processed];
        let hash = hex::encode(&block.block_hash);
        let mut snapshot = TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.last_finalized_block = block.block_hash.clone();
        snapshot.dag.last_finalized_block_hash = block.block_hash.clone();
        snapshot.dag.dag_set.insert(block.block_hash.clone());
        snapshot
            .dag
            .finalized_blocks_set
            .insert(block.block_hash.clone());
        snapshot
            .dag
            .block_metadata_index
            .write()
            .add(BlockMetadata::from_block(
                &block,
                false,
                Some(true),
                Some(true),
            ))
            .unwrap();
        service
            .engine_cell
            .set(Arc::new(EngineWithCasper::new(Arc::new(
                TestCasperWithSnapshot::new(snapshot, block.clone()),
            ))))
            .await;

        let amounts: Vec<BigInt> = vec![
            BigInt::from(1),
            (BigInt::from(1) << 53) + 1,
            BigInt::from(i64::MAX),
            BigInt::from(1) << 63,
            BigInt::from(10_000_000_000_000_000_000u64),
            BigInt::from(1) << 128,
            (BigInt::from(1) << 256) - 1,
        ];
        let channel = RhoString::create_par("synthetic-transfer-channel".to_string());
        let report = BlockEventInfo {
            deploys: vec![DeployInfoWithEventData {
                deploy_info: Some(deploy_info).into(),
                report: vec![SingleReport {
                    phase: ReportPhase::User as i32,
                    events: amounts
                        .iter()
                        .map(|amount| ReportProto {
                            report: Some(report_proto::Report::Comm(ReportCommProto {
                                consume: Some(ReportConsumeProto {
                                    channels: vec![channel.clone()],
                                    ..Default::default()
                                }),
                                produces: vec![ReportProduceProto {
                                    data: Some(ListParWithRandom {
                                        pars: vec![
                                            RhoString::create_par(from.clone()),
                                            Par::default(),
                                            RhoString::create_par(
                                                "synthetic-recipient".to_string(),
                                            ),
                                            RhoBigInt::create_par(amount),
                                            Par::default(),
                                            Par::default(),
                                        ],
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }],
                            })),
                        })
                        .collect(),
                }],
            }],
            ..Default::default()
        };
        let store = ReportStore::new(Arc::new(InMemoryKeyValueStore::new()));
        store
            .put(vec![(block.block_hash.to_vec(), report)])
            .unwrap();
        service.transfer_unforgeable = channel;
        service.block_report_api = BlockReportAPI::new(
            casper::rust::reporting_casper::noop(),
            store,
            service.engine_cell.clone(),
            service.key_value_block_store.clone(),
            CliqueOracleImpl,
            false,
        );

        for exact_transfer_amounts in [false, true] {
            let get_block = service
                .get_block(tonic::Request::new(BlockQuery {
                    hash: hash.clone(),
                    exact_transfer_amounts,
                }))
                .await
                .unwrap()
                .into_inner();
            let last_finalized = service
                .last_finalized_block(tonic::Request::new(LastFinalizedBlockQuery {
                    exact_transfer_amounts,
                }))
                .await
                .unwrap()
                .into_inner();
            for encoded in [get_block.encode_to_vec(), last_finalized.encode_to_vec()] {
                let decoded = BlockResponse::decode(encoded.as_slice()).unwrap();
                match decoded.message.unwrap() {
                    models::casper::v1::block_response::Message::BlockInfo(block) => {
                        assert!(exact_transfer_amounts);
                        assert!(block.deploys[0].transfers_available);
                        assert_eq!(block.deploys[0].transfers.len(), amounts.len());
                        for (transfer, amount) in block.deploys[0].transfers.iter().zip(&amounts) {
                            assert_eq!(transfer.amount_raw, amount.to_string());
                            assert!(transfer.success);
                        }
                    }
                    models::casper::v1::block_response::Message::Error(error) => {
                        assert!(!exact_transfer_amounts);
                        assert!(error.messages[0].starts_with("TRANSFER_AMOUNT_OUT_OF_RANGE:"));
                    }
                }
            }
        }

        for decimals in [8, 18] {
            let engine = Arc::new(service.engine_cell.clone());
            let web_api = WebApiImpl::new(
                10,
                false,
                "testnet".to_string(),
                "root".to_string(),
                1,
                "F1R3".to_string(),
                "F1R3".to_string(),
                decimals,
                false,
                true,
                service.block_report_api.clone(),
                service.transfer_unforgeable.clone(),
                engine.clone(),
                service.rp_conf_cell.clone(),
                service.connections_cell.clone(),
                service.node_discovery.clone(),
                None,
                100,
                50,
                service.is_ready.clone(),
            );
            let events = F1r3flyEvents::new();
            let app = WebApiRoutes::create_router().with_state(AppState::new(
                Arc::new(AdminWebApiImpl::new(None, None, engine)),
                Arc::new(web_api),
                Arc::new(service.block_report_api.clone()),
                service.rp_conf_cell.clone(),
                Arc::new(service.connections_cell.clone()),
                service.node_discovery.clone(),
                Arc::new(events.consume()),
                events.startup_buffer(),
            ));
            for path in [
                format!("/block/{hash}?view=full"),
                "/last-finalized-block?view=full".to_string(),
            ] {
                let response = app
                    .clone()
                    .oneshot(
                        axum::http::Request::builder()
                            .uri(path)
                            .body(axum::body::Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), axum::http::StatusCode::OK);
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
                let transfers = json["deploys"][0]["transfers"].as_array().unwrap();
                assert_eq!(transfers.len(), amounts.len());
                for (transfer, amount) in transfers.iter().zip(&amounts) {
                    assert_eq!(
                        transfer["amount"],
                        amount.to_string(),
                        "decimals={decimals}"
                    );
                    assert_eq!(transfer["fromAddr"], from);
                    assert_eq!(transfer["success"], true);
                }
            }
            let response = app
                .oneshot(
                    axum::http::Request::builder()
                        .uri(format!("/block/{hash}?view=summary"))
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(json.get("deploys").is_none());
        }
    }

    #[tokio::test]
    async fn status_reports_node_identity_without_casper() {
        let response = service().status(tonic::Request::new(())).await.unwrap();
        match response.into_inner().message.unwrap() {
            models::casper::v1::status_response::Message::Status(status) => {
                assert_eq!(status.network_id, "testnet");
                assert_eq!(status.shard_id, "root");
                assert!(status.is_read_only);
                assert!(status.is_ready);
                assert!(!status.is_validator);
                assert_eq!(status.last_finalized_block_number, -1);
                assert_eq!(status.current_epoch, 0);
            }
            other => panic!("expected Status, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn preview_private_names_answers_with_ids() {
        let response = service()
            .preview_private_names(tonic::Request::new(PrivateNamePreviewQuery {
                user: vec![1u8; 32].into(),
                timestamp: 1,
                name_qty: 2,
            }))
            .await
            .unwrap();
        match response.into_inner().message.unwrap() {
            models::casper::v1::private_name_preview_response::Message::Payload(payload) => {
                assert_eq!(payload.ids.len(), 2);
                assert!(!payload.ids[0].is_empty());
            }
            other => panic!("expected Payload, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn do_deploy_rejects_unsigned_proto_with_service_error() {
        let response = service()
            .do_deploy(tonic::Request::new(DeployDataProto::default()))
            .await
            .unwrap();
        assert!(matches!(
            response.into_inner().message.unwrap(),
            models::casper::v1::deploy_response::Message::Error(_)
        ));
    }

    #[tokio::test]
    async fn do_deploy_without_casper_answers_service_error_not_transport_error() {
        let response = service()
            .do_deploy(tonic::Request::new(signed_deploy_proto()))
            .await
            .unwrap();
        match response.into_inner().message.unwrap() {
            models::casper::v1::deploy_response::Message::Error(error) => {
                assert!(!error.messages.is_empty());
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn unary_block_queries_answer_service_errors_without_casper() {
        let service = service();

        let get_block = service
            .get_block(tonic::Request::new(BlockQuery {
                hash: "aabbccddeeff".to_string(),
                exact_transfer_amounts: false,
            }))
            .await
            .unwrap();
        assert!(matches!(
            get_block.into_inner().message.unwrap(),
            models::casper::v1::block_response::Message::Error(_)
        ));

        let lfb = service
            .last_finalized_block(tonic::Request::new(LastFinalizedBlockQuery::default()))
            .await
            .unwrap();
        assert!(matches!(
            lfb.into_inner().message.unwrap(),
            models::casper::v1::last_finalized_block_response::Message::Error(_)
        ));

        let is_finalized = service
            .is_finalized(tonic::Request::new(IsFinalizedQuery {
                hash: "aabbccddeeff".to_string(),
            }))
            .await
            .unwrap();
        assert!(matches!(
            is_finalized.into_inner().message.unwrap(),
            models::casper::v1::is_finalized_response::Message::Error(_)
        ));

        let machine_dag = service
            .machine_verifiable_dag(tonic::Request::new(MachineVerifyQuery {}))
            .await
            .unwrap();
        assert!(matches!(
            machine_dag.into_inner().message.unwrap(),
            models::casper::v1::machine_verify_response::Message::Error(_)
        ));

        let find_deploy = service
            .find_deploy(tonic::Request::new(FindDeployQuery {
                deploy_id: vec![1u8, 2].into(),
            }))
            .await
            .unwrap();
        let find_deploy = find_deploy.into_inner();
        assert!(matches!(
            find_deploy.message.unwrap(),
            models::casper::v1::find_deploy_response::Message::Error(_)
        ));
        assert_eq!(find_deploy.finalization_state, 0);
    }

    #[tokio::test]
    async fn deploy_status_and_data_queries_answer_service_errors_without_casper() {
        let service = service();

        let finalization = service
            .deploy_finalization_status(tonic::Request::new(DeployFinalizationStatusQuery {
                deploy_sig: vec![1u8, 2].into(),
            }))
            .await
            .unwrap();
        assert!(matches!(
            finalization.into_inner().message.unwrap(),
            models::casper::v1::deploy_finalization_status_response::Message::Error(_)
        ));

        let pending = service
            .get_pending_deploys(tonic::Request::new(PendingDeploysQuery {
                deployer_pubkey: prost::bytes::Bytes::new(),
            }))
            .await
            .unwrap();
        assert!(matches!(
            pending.into_inner().message.unwrap(),
            models::casper::v1::pending_deploys_response::Message::Error(_)
        ));

        let data_at_name = service
            .get_data_at_name(tonic::Request::new(DataAtNameByBlockQuery::default()))
            .await
            .unwrap();
        assert!(matches!(
            data_at_name.into_inner().message.unwrap(),
            models::casper::v1::rho_data_response::Message::Error(_)
        ));

        let continuation = service
            .listen_for_continuation_at_name(
                tonic::Request::new(ContinuationAtNameQuery::default()),
            )
            .await
            .unwrap();
        assert!(matches!(
            continuation.into_inner().message.unwrap(),
            models::casper::v1::continuation_at_name_response::Message::Error(_)
        ));

        let exploratory = service
            .exploratory_deploy(tonic::Request::new(ExploratoryDeployQuery {
                term: "Nil".to_string(),
                block_hash: String::new(),
                use_pre_state_hash: false,
            }))
            .await
            .unwrap();
        assert!(matches!(
            exploratory.into_inner().message.unwrap(),
            models::casper::v1::exploratory_deploy_response::Message::Error(_)
        ));
    }

    #[tokio::test]
    async fn bond_status_validates_public_key_before_touching_casper() {
        let service = service();

        let invalid = service
            .bond_status(tonic::Request::new(BondStatusQuery {
                public_key: vec![1u8, 2].into(),
            }))
            .await
            .unwrap();
        assert!(matches!(
            invalid.into_inner().message.unwrap(),
            models::casper::v1::bond_status_response::Message::Error(_)
        ));

        let (_sk, pk) = Secp256k1.new_key_pair();
        let valid_key_no_casper = service
            .bond_status(tonic::Request::new(BondStatusQuery {
                public_key: pk.bytes.clone(),
            }))
            .await
            .unwrap();
        assert!(matches!(
            valid_key_no_casper.into_inner().message.unwrap(),
            models::casper::v1::bond_status_response::Message::Error(_)
        ));
    }

    #[tokio::test]
    async fn get_event_by_hash_rejects_invalid_hex_and_missing_block() {
        let service = service();

        let invalid_hex = service
            .get_event_by_hash(tonic::Request::new(ReportQuery {
                hash: "not-hex".to_string(),
                force_replay: false,
            }))
            .await
            .unwrap();
        match invalid_hex.into_inner().message.unwrap() {
            models::casper::v1::event_info_response::Message::Error(error) => {
                assert!(error.messages[0].contains("not valid hex"));
            }
            other => panic!("expected Error, got {:?}", other),
        }

        let missing_block = service
            .get_event_by_hash(tonic::Request::new(ReportQuery {
                hash: "aabb".to_string(),
                force_replay: false,
            }))
            .await
            .unwrap();
        assert!(matches!(
            missing_block.into_inner().message.unwrap(),
            models::casper::v1::event_info_response::Message::Error(_)
        ));
    }

    #[tokio::test]
    async fn streaming_queries_surface_engine_errors_or_end_cleanly() {
        let service = service();

        let mut get_blocks = service
            .get_blocks(tonic::Request::new(BlocksQuery { depth: 3 }))
            .await
            .unwrap()
            .into_inner();
        assert!(get_blocks.next().await.unwrap().is_err());

        let mut by_heights = service
            .get_blocks_by_heights(tonic::Request::new(BlocksQueryByHeight {
                start_block_number: 0,
                end_block_number: 2,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(by_heights.next().await.unwrap().is_err());

        let mut main_chain = service
            .show_main_chain(tonic::Request::new(BlocksQuery { depth: 3 }))
            .await
            .unwrap()
            .into_inner();
        assert!(main_chain.next().await.is_none());

        let mut visualize = service
            .visualize_dag(tonic::Request::new(VisualizeDagQuery {
                depth: 0,
                show_justification_lines: false,
                start_block_number: 0,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(visualize.next().await.unwrap().is_err());
    }

    #[test]
    fn deploy_state_to_proto_maps_every_state() {
        use casper::rust::api::deploy_finalization_status::DeployFinalizationState as S;
        assert_eq!(
            deploy_state_to_proto(S::Finalized),
            DeployFinalizationStateProto::DeployStateFinalized
        );
        assert_eq!(
            deploy_state_to_proto(S::Failed),
            DeployFinalizationStateProto::DeployStateFailed
        );
        assert_eq!(
            deploy_state_to_proto(S::Pending),
            DeployFinalizationStateProto::DeployStatePending
        );
        assert_eq!(
            deploy_state_to_proto(S::Expired),
            DeployFinalizationStateProto::DeployStateExpired
        );
    }
}
