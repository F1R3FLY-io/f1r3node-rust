//! Deploy gRPC Service V1 implementation
//!
//! This module provides a gRPC service for deploy functionality,
//! allowing clients to deploy contracts, query blocks, and perform various blockchain operations.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::api::block_api::{
    BlockAPI, BlocksByHeightsResponse, ExploratoryDeployRejection, PrunedStateError,
};
use casper::rust::api::block_report_api::BlockReportAPI;
use casper::rust::api::deploy_reconciliation::ReconciledStatus;
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
    BlockInfoResponse, BlockPrunedInfo, BlockResponse, BlockUnavailableInfo, BondStatusResponse,
    ContinuationAtNameResponse, DeployFinalizationStatusResponse, DeployResponse,
    EventInfoResponse, ExploratoryDeployResponse, FindDeployResponse, IsFinalizedResponse,
    LastFinalizedBlockResponse, MachineVerifyResponse, PendingDeploysResponse,
    PrivateNamePreviewResponse, PrunedStateInfo, RhoDataResponse, StatusResponse,
    VisualizeBlocksResponse,
};
use models::casper::{
    BlockQuery, BlocksQuery, BlocksQueryByHeight, BondStatusQuery, ContinuationAtNameQuery,
    DataAtNameByBlockQuery, DeployDataProto, DeployFinalizationStateProto,
    DeployFinalizationStatusInfo, DeployFinalizationStatusQuery, ExploratoryDeployQuery,
    FindDeployQuery, IsFinalizedQuery, LastFinalizedBlockQuery, LightBlockInfo, MachineVerifyQuery,
    PendingDeployInfo, PendingDeploysQuery, PendingDeploysResponsePayload, PrivateNamePreviewQuery,
    ReportQuery, Status, VersionInfo, VisualizeDagQuery,
};
use models::servicemodelapi::ServiceError;
use tokio::time::{sleep, Duration};
use tracing::error;

use crate::rust::api::{DeployRequestLimiter, DeployRequestPermit};
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

/// Issue #1157: extract a [`PrunedStateError`] from an `eyre` chain so the
/// gRPC handler can map it to the `pruned` oneof arm instead of folding
/// its data into a prose `ServiceError.messages`.
fn classify_pruned_state(err: &eyre::Error) -> Option<PrunedStateInfo> {
    err.chain()
        .find_map(|cause| cause.downcast_ref::<PrunedStateError>())
        .map(|p| PrunedStateInfo {
            target_block_hash: p.target_block_hash.clone(),
            target_block_height: p.target_block_height,
            retention_floor: p.retention_floor,
        })
}

/// Flatten a [`BlocksByHeightsResponse`] into the ordered sequence of
/// `BlockInfoResponse` items the `getBlocksByHeights` stream emits:
/// `served` blocks first, then (only if `include_missing` is set) all
/// `pruned` entries, then all `unavailable` entries. `prune_floor` is
/// copied onto each missing-entry message so a consumer reacting
/// item-by-item has the retention context without waiting for a
/// terminal summary. Split out from the handler so the ordering is
/// unit-testable without a live engine; see the stream-order tests.
fn build_blocks_by_heights_stream(
    response: BlocksByHeightsResponse<LightBlockInfo>,
    include_missing: bool,
) -> Vec<BlockInfoResponse> {
    use models::casper::v1::block_info_response::Message;
    let prune_floor = response.prune_floor;
    let mut out: Vec<BlockInfoResponse> = Vec::with_capacity(
        response.served.len()
            + if include_missing {
                response.pruned.len() + response.unavailable.len()
            } else {
                0
            },
    );
    for block_info in response.served {
        out.push(BlockInfoResponse {
            message: Some(Message::BlockInfo(block_info)),
        });
    }
    if include_missing {
        for entry in response.pruned {
            out.push(BlockInfoResponse {
                message: Some(Message::Pruned(BlockPrunedInfo {
                    hash: entry.hash,
                    block_height: entry.block_height,
                    prune_floor,
                })),
            });
        }
        for entry in response.unavailable {
            out.push(BlockInfoResponse {
                message: Some(Message::Unavailable(BlockUnavailableInfo {
                    hash: entry.hash,
                    block_height: entry.block_height,
                    prune_floor,
                })),
            });
        }
    }
    out
}

fn find_deploy_retry_interval_ms() -> u64 { FIND_DEPLOY_RETRY_INTERVAL_MS }

fn find_deploy_max_attempts() -> u8 { FIND_DEPLOY_MAX_ATTEMPTS }

/// Deploy gRPC Service V1 implementation
#[derive(Clone)]
pub struct DeployGrpcServiceV1Impl {
    deploy_limiter: DeployRequestLimiter,
    api_max_blocks_limit: i32,
    trigger_propose_f: Option<Arc<ProposeFunction>>,
    dev_mode: bool,
    network_id: String,
    shard_id: String,
    min_phlo_price: shared::rust::token_amount::TokenAmount,
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
    async fn reconcile_deploy(
        &self,
        sig: prost::bytes::Bytes,
        proto: DeployDataProto,
    ) -> eyre::Result<ReconciledStatus> {
        let permit = self.deploy_limiter.try_acquire()?;
        let engine = self.engine_cell.clone();
        let shard = self.shard_id.clone();
        crate::rust::web::shared_handlers::offload(move || async move {
            let _permit = permit;
            let deploy =
                models::rust::casper::protocol::casper_message::DeployData::from_proto(proto)
                    .map_err(
                        |message| casper::rust::api::block_api::DeployValidationError { message },
                    )?;
            BlockAPI::deploy_finalization_status_reconciled(&engine, &sig, Some(&deploy), &shard)
                .await
        })
        .await
    }

    pub fn new(
        api_max_blocks_limit: i32,
        trigger_propose_f: Option<Arc<ProposeFunction>>,
        dev_mode: bool,
        network_id: String,
        shard_id: String,
        min_phlo_price: shared::rust::token_amount::TokenAmount,
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
            deploy_limiter: DeployRequestLimiter::default(),
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

    pub(crate) fn with_deploy_limiter(mut self, limiter: DeployRequestLimiter) -> Self {
        self.deploy_limiter = limiter;
        self
    }

    pub(crate) fn deploy_limiter(&self) -> &DeployRequestLimiter { &self.deploy_limiter }

    async fn submit_deploy(
        &self,
        proto: DeployDataProto,
    ) -> Result<tonic::Response<DeployResponse>, tonic::Status> {
        let signed_deploy =
            match models::rust::casper::protocol::casper_message::DeployData::from_proto(proto) {
                Ok(signed) => signed,
                Err(error) => {
                    return Self::create_error_deploy_response(Self::create_service_error(error))
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
                let is_expected_refusal = e.chain().any(|cause| {
                    matches!(
                        cause.downcast_ref::<DeployError>(),
                        Some(
                            DeployError::DuplicateDeploy(_)
                                | DeployError::SeedReuse(_)
                                | DeployError::PoolBusy { .. }
                                | DeployError::AdmissionBusy
                                | DeployError::InvalidFreshness
                                | DeployError::PhloLimitAboveCap { .. }
                                | DeployError::RecordTooLarge { .. }
                        )
                    ) || cause
                        .downcast_ref::<casper::rust::api::block_api::DeployExpiredError>()
                        .is_some()
                        || cause
                            .downcast_ref::<casper::rust::api::block_api::ValidatorNotBondedError>()
                            .is_some()
                });
                if is_expected_refusal {
                    tracing::debug!("Deploy admission refused: {}", e);
                } else {
                    error!("Deploy service method error do_deploy: {}", e);
                }
                Self::create_error_deploy_response(e.into_service_error())
            }
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

    /// Helper function to create a successful BlockResponse
    fn create_success_block_response(
        block_info: models::casper::BlockInfo,
    ) -> Result<tonic::Response<BlockResponse>, tonic::Status> {
        Ok(BlockResponse {
            message: Some(models::casper::v1::block_response::Message::BlockInfo(
                block_info,
            )),
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
        mut request: tonic::Request<DeployDataProto>,
    ) -> Result<tonic::Response<DeployResponse>, tonic::Status> {
        let permit = match request.extensions_mut().remove::<DeployRequestPermit>() {
            Some(permit) => permit,
            None => match self.deploy_limiter.try_acquire() {
                Ok(permit) => permit,
                Err(error) => {
                    return Self::create_error_deploy_response(Self::create_service_error(
                        error.to_string(),
                    ))
                }
            },
        };
        if !self.deploy_limiter.is_limited() {
            return self.submit_deploy(request.into_inner()).await;
        }
        let service = self.clone();
        crate::rust::web::shared_handlers::offload(move || async move {
            let _permit = permit;
            Ok(service.submit_deploy(request.into_inner()).await)
        })
        .await
        .map_err(|error| tonic::Status::internal(error.to_string()))?
    }

    /// Get a block by hash
    async fn get_block(
        &self,
        request: tonic::Request<BlockQuery>,
    ) -> Result<tonic::Response<BlockResponse>, tonic::Status> {
        match BlockAPI::get_block(&self.engine_cell, &request.into_inner().hash).await {
            Ok(mut block_info) => {
                // Enrich transfers from BlockReportAPI (uses ReportStore cache).
                // On readonly: transfers populated. On validators: empty (block report rejected).
                self.enrich_proto_transfers(&mut block_info).await;
                Self::create_success_block_response(block_info)
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
        let max_depth_limit = self.api_max_blocks_limit;
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        let engine_cell = self.engine_cell.clone();
        let key_value_block_store = self.key_value_block_store.clone();

        tokio::spawn(async move {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let ser: Arc<dyn GraphSerializer> = Arc::new(ListSerializer::new(sender));

            match BlockAPI::visualize_dag(
                &engine_cell,
                depth,
                max_depth_limit,
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
                // Issue #1157: a pruned-state refusal takes the `pruned`
                // oneof arm so the caller can route to an archive node.
                if let Some(pruned) = classify_pruned_state(&e) {
                    return Ok(tonic::Response::new(RhoDataResponse {
                        message: Some(models::casper::v1::rho_data_response::Message::Pruned(
                            pruned,
                        )),
                    }));
                }
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
        let Some(valid_after_block_number) = request.valid_after_block_number else {
            return Ok(tonic::Response::new(PrivateNamePreviewResponse {
                message: Some(
                    models::casper::v1::private_name_preview_response::Message::Error(
                        ServiceError {
                            messages: vec![
                                "validAfterBlockNumber is required: a deploy's names derive from it"
                                    .to_string(),
                            ],
                        },
                    ),
                ),
            }));
        };
        match BlockAPI::preview_private_names(
            &request.user.to_vec(),
            request.timestamp,
            valid_after_block_number,
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
        let _request = request.into_inner();
        match BlockAPI::last_finalized_block(&self.engine_cell).await {
            Ok(mut block_info) => {
                self.enrich_proto_transfers(&mut block_info).await;
                Ok(tonic::Response::new(LastFinalizedBlockResponse {
                    message: Some(
                        models::casper::v1::last_finalized_block_response::Message::BlockInfo(
                            block_info,
                        ),
                    ),
                }))
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

    /// Query the finalization status of a deploy by its signature. When the
    /// query carries the saved signed deploy (#1103), a sig the register has
    /// never observed is reconciled against the floor of the adopted last
    /// finalized block; the response names that basis.
    async fn deploy_finalization_status(
        &self,
        request: tonic::Request<DeployFinalizationStatusQuery>,
    ) -> Result<tonic::Response<DeployFinalizationStatusResponse>, tonic::Status> {
        let request = request.into_inner();
        let verdict = match request.deploy {
            Some(proto) => self.reconcile_deploy(request.deploy_sig, proto).await,
            None => BlockAPI::deploy_finalization_status(&self.engine_cell, &request.deploy_sig)
                .await
                .map(ReconciledStatus::from_lookup),
        };
        match verdict {
            Ok(reconciled) => Ok(tonic::Response::new(DeployFinalizationStatusResponse {
                message: Some(
                    models::casper::v1::deploy_finalization_status_response::Message::Status(
                        DeployFinalizationStatusInfo {
                            state: deploy_state_to_proto(reconciled.status.state) as i32,
                            rejection_count: reconciled.status.rejection_count,
                            latest_block_hash: reconciled.status.latest_block_hash,
                            basis_block_hash: reconciled.basis.as_ref().map(|f| f.hash.clone()),
                            basis_block_height: reconciled.basis.as_ref().map(|f| f.block_number),
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
            Vec::new(),
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
                // Issue #1157: a pruned-state refusal takes the typed
                // `pruned` oneof arm so the client sees the retention
                // floor and target height without parsing prose.
                if let Some(pruned) = classify_pruned_state(&e) {
                    return Ok(tonic::Response::new(ExploratoryDeployResponse {
                        message: Some(
                            models::casper::v1::exploratory_deploy_response::Message::Pruned(
                                pruned,
                            ),
                        ),
                        cost: 0,
                    }));
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
                // Issue #1157: a pruned-state refusal from the block-report
                // replay path takes the typed `pruned` oneof arm.
                if let casper::rust::api::block_report_api::BlockReportError::PrunedState {
                    target_block_hash,
                    target_block_height,
                    retention_floor,
                } = &e
                {
                    return Ok(tonic::Response::new(EventInfoResponse {
                        message: Some(models::casper::v1::event_info_response::Message::Pruned(
                            PrunedStateInfo {
                                target_block_hash: target_block_hash.clone(),
                                target_block_height: *target_block_height,
                                retention_floor: *retention_floor,
                            },
                        )),
                    }));
                }
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
        // Default is false (proto3), so an existing client that has not
        // regenerated its stubs sees only `blockInfo` items — same shape
        // as before #1087. Clients that opt in receive `pruned` and
        // `unavailable` trailing items for topo_sort hashes whose bodies
        // this node no longer holds.
        let include_missing = request.include_missing;

        tokio::spawn(async move {
            match BlockAPI::get_blocks_by_heights(
                &engine_cell,
                request.start_block_number,
                request.end_block_number,
                api_max_blocks_limit,
            )
            .await
            {
                Ok(response) => {
                    for msg in build_blocks_by_heights_stream(response, include_missing) {
                        if tx.send(Ok(msg)).await.is_err() {
                            return;
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

        // Advertise the values admission and validity actually enforce — the
        // chain-adopted ones; local conf only until casper is up.
        let (min_phlo_price, epoch_length) = match self.engine_cell.get().await.with_casper() {
            Some(casper) => (
                casper.casper_shard_conf().min_phlo_price.clone(),
                casper.casper_shard_conf().epoch_length,
            ),
            None => (self.min_phlo_price.clone(), self.epoch_length),
        };
        let current_epoch = if epoch_length > 0 && lfb_number >= 0 {
            lfb_number / epoch_length as i64
        } else {
            0
        };

        let pacing = crate::rust::api::web_api::DeployPacing::read(&self.engine_cell)
            .await
            .map_err(|e| {
                error!("Deploy service method error status (pacing): {}", e);
                tonic::Status::internal(e.to_string())
            })?;

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
            min_phlo_price: min_phlo_price.to_string(),
            peer_list,
            native_token_name: self.native_token_name.clone(),
            native_token_symbol: self.native_token_symbol.clone(),
            native_token_decimals: self.native_token_decimals,
            last_finalized_block_number: lfb_number,
            is_validator: self.has_validator_key,
            is_read_only: self.is_node_read_only,
            is_ready,
            current_epoch,
            epoch_length,
            latest_block_number: pacing.map(|p| p.latest_block_number),
            deploy_lifespan: pacing.map(|p| p.deploy_lifespan),
            max_user_deploys_per_block: pacing.map(|p| p.max_user_deploys_per_block),
            max_parent_depth: pacing.map(|p| p.max_parent_depth),
            max_phlo_limit: pacing.map(|p| p.max_phlo_limit),
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
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use tokio_stream::StreamExt;

    use super::*;

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
            (1).into(),
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
                comm::rust::rp::chain_id::ChainIdCell::unknown(),
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
            phlo_price: (1).into(),
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
        use num_bigint::BigInt;
        use prost::Message;
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
        snapshot
            .dag
            .fixture_mut()
            .dag_set
            .insert(block.block_hash.clone());
        snapshot
            .dag
            .fixture_mut()
            .finalized_blocks_set
            .insert(block.block_hash.clone());
        snapshot
            .dag
            .block_metadata_index
            .write()
            .add(BlockMetadata::from_block(
                &block,
                None,
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

        let get_block = service
            .get_block(tonic::Request::new(BlockQuery { hash: hash.clone() }))
            .await
            .unwrap()
            .into_inner();
        let last_finalized = service
            .last_finalized_block(tonic::Request::new(LastFinalizedBlockQuery {}))
            .await
            .unwrap()
            .into_inner();
        for encoded in [get_block.encode_to_vec(), last_finalized.encode_to_vec()] {
            let decoded = BlockResponse::decode(encoded.as_slice()).unwrap();
            let Some(models::casper::v1::block_response::Message::BlockInfo(block)) =
                decoded.message
            else {
                panic!("block response must carry the block");
            };
            assert!(block.deploys[0].transfers_available);
            assert_eq!(block.deploys[0].transfers.len(), amounts.len());
            for (transfer, amount) in block.deploys[0].transfers.iter().zip(&amounts) {
                assert_eq!(transfer.amount_raw, amount.to_string());
                assert!(transfer.success);
            }
        }

        for decimals in [8, 18] {
            let engine = Arc::new(service.engine_cell.clone());
            let web_api = WebApiImpl::new(
                10,
                false,
                "testnet".to_string(),
                "root".to_string(),
                (1).into(),
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
                valid_after_block_number: Some(9),
            }))
            .await
            .unwrap();
        match response.into_inner().message.unwrap() {
            models::casper::v1::private_name_preview_response::Message::Payload(payload) => {
                let expected = BlockAPI::preview_private_names(&vec![1u8; 32], 1, 9, 2).unwrap();
                let ids: Vec<Vec<u8>> = payload.ids.iter().map(|id| id.to_vec()).collect();
                assert_eq!(ids, expected);
            }
            other => panic!("expected Payload, got {:?}", other),
        }

        let missing = service()
            .preview_private_names(tonic::Request::new(PrivateNamePreviewQuery {
                user: vec![1u8; 32].into(),
                timestamp: 1,
                name_qty: 2,
                valid_after_block_number: None,
            }))
            .await
            .unwrap();
        assert!(
            matches!(
                missing.into_inner().message.unwrap(),
                models::casper::v1::private_name_preview_response::Message::Error(_)
            ),
            "a query without validAfterBlockNumber is refused"
        );
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
    async fn ingress_generated_client_keeps_busy_in_the_service_envelope_and_recovers() {
        use models::casper::v1::deploy_response;
        use models::casper::v1::deploy_service_client::DeployServiceClient;
        use models::casper::v1::deploy_service_server::DeployServiceServer;

        use crate::rust::api::grpc_package::DeployIngressService;

        let limiter = DeployRequestLimiter::new(Some(1)).unwrap();
        let permit = limiter.try_acquire().unwrap();
        let server = DeployIngressService::new(
            DeployServiceServer::new(service().with_deploy_limiter(limiter.clone())),
            limiter.clone(),
        );
        let mut client = DeployServiceClient::new(server);
        let response = client.do_deploy(DeployDataProto::default()).await.unwrap();
        let Some(deploy_response::Message::Error(error)) = response.into_inner().message else {
            panic!("expected service error");
        };
        assert!(error.messages[0].starts_with("deploy_admission_busy:"));
        drop(permit);
        let response = client.do_deploy(DeployDataProto::default()).await.unwrap();
        let Some(deploy_response::Message::Error(error)) = response.into_inner().message else {
            panic!("expected validation error");
        };
        assert!(!error.messages[0].starts_with("deploy_admission_busy:"));
        assert!(limiter.try_acquire().is_ok());
    }

    #[tokio::test]
    async fn ingress_rejects_without_reading_protobuf_and_does_not_gate_status() {
        use models::casper::v1::deploy_service_client::DeployServiceClient;
        use models::casper::v1::deploy_service_server::DeployServiceServer;
        use tower::ServiceExt;

        use crate::rust::api::grpc_package::DeployIngressService;

        let limiter = DeployRequestLimiter::new(Some(1)).unwrap();
        let _permit = limiter.try_acquire().unwrap();
        let server = DeployIngressService::new(
            DeployServiceServer::new(service().with_deploy_limiter(limiter.clone())),
            limiter,
        );
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = reads.clone();
        let body = axum::body::Body::from_stream(tokio_stream::once(()).map(move |()| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::convert::Infallible>(tonic::codegen::Bytes::from_static(
                b"invalid protobuf",
            ))
        }));
        let request = tonic::codegen::http::Request::builder()
            .uri("/casper.v1.DeployService/doDeploy")
            .header("content-type", "application/grpc")
            .body(tonic::body::Body::new(body))
            .unwrap();
        let response = server.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        let mut client = DeployServiceClient::new(server);
        assert!(client
            .status(())
            .await
            .unwrap()
            .into_inner()
            .message
            .is_some());
    }

    #[tokio::test]
    async fn ingress_direct_service_calls_share_the_transport_limit() {
        let limiter = DeployRequestLimiter::new(Some(1)).unwrap();
        let service = service().with_deploy_limiter(limiter.clone());
        let _permit = limiter.try_acquire().unwrap();
        let response = service
            .do_deploy(tonic::Request::new(DeployDataProto::default()))
            .await
            .unwrap();
        let Some(models::casper::v1::deploy_response::Message::Error(error)) =
            response.into_inner().message
        else {
            panic!("expected service error");
        };
        assert!(error.messages[0].starts_with("deploy_admission_busy:"));
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

    #[test]
    fn pool_busy_uses_the_existing_deploy_service_error_envelope() {
        let error = eyre::Report::new(DeployError::PoolBusy {
            resource: "records",
            required: 513,
            limit: 512,
        });
        let response =
            DeployGrpcServiceV1Impl::create_error_deploy_response(error.into_service_error())
                .unwrap()
                .into_inner();
        match response.message.unwrap() {
            models::casper::v1::deploy_response::Message::Error(error) => {
                assert!(error
                    .messages
                    .iter()
                    .any(|message| message.contains("deploy_pool_busy")));
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn oversized_record_has_a_permanent_error_prefix_in_the_service_envelope() {
        let error = eyre::Report::new(DeployError::RecordTooLarge {
            record_bytes: 100,
            limit: 99,
        });
        let response =
            DeployGrpcServiceV1Impl::create_error_deploy_response(error.into_service_error())
                .unwrap();
        let Some(models::casper::v1::deploy_response::Message::Error(error)) =
            response.into_inner().message
        else {
            panic!("expected a deploy service error");
        };
        assert!(error.messages[0].starts_with("deploy_too_large:"));
        assert!(!error.messages[0].starts_with("deploy_pool_busy:"));
    }

    #[test]
    fn window_refusals_have_distinct_prefixes_in_the_service_envelope() {
        let expired_api = casper::rust::api::block_api::DeployExpiredError {
            message: "deploy_expired: validAfterBlockNumber 0 has expired".to_string(),
        };
        for (error, prefix) in [
            (eyre::Report::new(expired_api), "deploy_expired:"),
            (
                eyre::Report::new(DeployError::InvalidFreshness),
                "invalid_deploy_window:",
            ),
            (
                eyre::Report::new(casper::rust::api::block_api::ValidatorNotBondedError),
                "validator_not_bonded:",
            ),
        ] {
            let response =
                DeployGrpcServiceV1Impl::create_error_deploy_response(error.into_service_error())
                    .unwrap();
            let Some(models::casper::v1::deploy_response::Message::Error(error)) =
                response.into_inner().message
            else {
                panic!("expected a deploy service error");
            };
            assert!(
                error.messages[0].starts_with(prefix),
                "{:?}",
                error.messages
            );
        }
    }

    #[test]
    fn admission_busy_uses_the_existing_deploy_service_error_envelope() {
        let error = eyre::Report::new(DeployError::AdmissionBusy);
        let response =
            DeployGrpcServiceV1Impl::create_error_deploy_response(error.into_service_error())
                .unwrap()
                .into_inner();
        match response.message.unwrap() {
            models::casper::v1::deploy_response::Message::Error(error) => {
                assert!(error
                    .messages
                    .iter()
                    .any(|message| message.contains("deploy_admission_busy")));
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
            }))
            .await
            .unwrap();
        assert!(matches!(
            get_block.into_inner().message.unwrap(),
            models::casper::v1::block_response::Message::Error(_)
        ));

        let lfb = service
            .last_finalized_block(tonic::Request::new(LastFinalizedBlockQuery {}))
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
                deploy: None,
            }))
            .await
            .unwrap();
        assert!(matches!(
            finalization.into_inner().message.unwrap(),
            models::casper::v1::deploy_finalization_status_response::Message::Error(_)
        ));

        // A query carrying a saved signed deploy (#1103) answers the same
        // ServiceError before any reconciliation can run: the decode fails
        // on the unsigned default proto.
        let reconciled = service
            .deploy_finalization_status(tonic::Request::new(DeployFinalizationStatusQuery {
                deploy_sig: vec![1u8, 2].into(),
                deploy: Some(DeployDataProto::default()),
            }))
            .await
            .unwrap();
        assert!(matches!(
            reconciled.into_inner().message.unwrap(),
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
                include_missing: false,
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

    fn envelope_fixture() -> BlocksByHeightsResponse<LightBlockInfo> {
        let served_a = LightBlockInfo {
            block_hash: "aa".to_string(),
            block_number: 1,
            ..LightBlockInfo::default()
        };
        let served_b = LightBlockInfo {
            block_hash: "bb".to_string(),
            block_number: 3,
            ..LightBlockInfo::default()
        };
        BlocksByHeightsResponse {
            served: vec![served_a, served_b],
            pruned: vec![casper::rust::api::block_api::MissingBlockEntry {
                hash: prost::bytes::Bytes::from_static(b"\x01\x02"),
                block_height: 2,
            }],
            unavailable: vec![casper::rust::api::block_api::MissingBlockEntry {
                hash: prost::bytes::Bytes::from_static(b"\x03\x04"),
                block_height: 4,
            }],
            prune_floor: 2,
        }
    }

    #[test]
    fn blocks_by_heights_stream_omits_missing_when_opt_in_is_off() {
        // Opt-out path: existing clients see only `blockInfo` items, same
        // shape as before #1087. Locks in the contract that `include_missing
        // = false` filters out `pruned` and `unavailable` entries entirely,
        // so wire-additive variants cannot leak into an un-regenerated
        // consumer's stream.
        let out = build_blocks_by_heights_stream(envelope_fixture(), false);
        assert_eq!(out.len(), 2);
        for item in &out {
            match item.message.as_ref().expect("message set") {
                models::casper::v1::block_info_response::Message::BlockInfo(_) => {}
                other => panic!("expected BlockInfo variant only, got {other:?}"),
            }
        }
    }

    #[test]
    fn blocks_by_heights_stream_orders_served_then_pruned_then_unavailable() {
        // Opt-in path: served → pruned → unavailable, with `prune_floor`
        // on each missing-entry message. This is the ordering the PR
        // description promises, and the layout opt-in clients will walk.
        use models::casper::v1::block_info_response::Message;
        let out = build_blocks_by_heights_stream(envelope_fixture(), true);
        assert_eq!(out.len(), 4);
        match out[0].message.as_ref().unwrap() {
            Message::BlockInfo(b) => assert_eq!(b.block_hash, "aa"),
            other => panic!("expected BlockInfo, got {other:?}"),
        }
        match out[1].message.as_ref().unwrap() {
            Message::BlockInfo(b) => assert_eq!(b.block_hash, "bb"),
            other => panic!("expected BlockInfo, got {other:?}"),
        }
        match out[2].message.as_ref().unwrap() {
            Message::Pruned(p) => {
                assert_eq!(p.hash.as_ref(), b"\x01\x02");
                assert_eq!(p.block_height, 2);
                assert_eq!(p.prune_floor, 2);
            }
            other => panic!("expected Pruned, got {other:?}"),
        }
        match out[3].message.as_ref().unwrap() {
            Message::Unavailable(u) => {
                assert_eq!(u.hash.as_ref(), b"\x03\x04");
                assert_eq!(u.block_height, 4);
                assert_eq!(u.prune_floor, 2);
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    /// Issue #1157: the classifier pulls a `PrunedStateError` out of an
    /// `eyre` chain even when wrapped — a reader that `.map_err`s or
    /// `.wrap_err`s the typed refusal must not lose the `pruned` oneof
    /// routing. The field data (`target_block_height`, `retention_floor`)
    /// must survive the conversion: those are the values a client parses
    /// off the response to decide whether to try an archive node.
    #[test]
    fn classify_pruned_state_extracts_fields_through_eyre_wrap() {
        let target = prost::bytes::Bytes::from_static(b"target-block-hash-01234567890abcde");
        let raw = eyre::Report::new(PrunedStateError {
            target_block_hash: target.clone(),
            target_block_height: 42,
            retention_floor: 90,
        });
        let wrapped = raw.wrap_err("while serving exploratory deploy");

        let info = classify_pruned_state(&wrapped).expect("must detect PrunedStateError");
        assert_eq!(info.target_block_hash, target);
        assert_eq!(info.target_block_height, 42);
        assert_eq!(info.retention_floor, 90);
    }

    /// A non-pruned error must not be misclassified: the classifier must
    /// walk the chain and return `None` so the generic `error` oneof arm
    /// still fires. Otherwise every reader failure would read as "pruned".
    #[test]
    fn classify_pruned_state_returns_none_for_unrelated_errors() {
        let unrelated = eyre::eyre!("not a retention event");
        assert!(classify_pruned_state(&unrelated).is_none());
    }
}
