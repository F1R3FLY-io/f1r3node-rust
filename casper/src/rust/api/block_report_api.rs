// See casper/src/main/scala/coop/rchain/casper/api/BlockReportAPI.scala

use std::sync::Arc;
use std::time::Duration;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::casper::{
    BlockEventInfo, DeployInfoWithEventData, ReportProto, SingleReport,
    SystemDeployInfoWithEventData,
};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, SystemDeployData};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::reporting_transformer::ReportingTransformer;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::{env, ByteString};
use tokio::sync::Semaphore;

use crate::rust::api::block_api::BlockAPI;
use crate::rust::engine::engine_cell::EngineCell;
use crate::rust::report_store::ReportStore;
use crate::rust::reporting_casper::ReportingCasper;
use crate::rust::reporting_proto_transformer::ReportingProtoTransformer;
use crate::rust::safety_oracle::CliqueOracleImpl;
use crate::rust::util::proto_util;

const REPORT_CACHE_VERSION: &[u8] = b"v2:";
const BLOCK_REPORT_QUEUE_TIMEOUT_MS_DEFAULT: u64 = 2_000;
const BLOCK_REPORT_QUEUE_TIMEOUT_MS_ENV: &str = "F1R3_BLOCK_REPORT_QUEUE_TIMEOUT_MS";

fn report_cache_key(block_hash: &BlockHash) -> ByteString {
    let mut key = Vec::with_capacity(REPORT_CACHE_VERSION.len() + block_hash.len());
    key.extend_from_slice(REPORT_CACHE_VERSION);
    key.extend_from_slice(block_hash);
    key
}

/// Domain-specific errors for BlockReportAPI operations
#[derive(Debug, thiserror::Error)]
pub enum BlockReportError {
    #[error("Casper instance not available")]
    CasperNotInitialized,
    #[error("Block report can only be executed on read-only RNode")]
    ReadOnlyRequired,
    #[error("Block {0:?} not found")]
    BlockNotFound(BlockHash),
    #[error("Block report pre-state is unavailable for block {0:?}")]
    StateUnavailable(BlockHash),
    #[error("Block reporter is busy; retry later")]
    Busy,
    #[error("Failed to trace block: {0}")]
    ReplayFailed(String),
    #[error("Block info error: {0}")]
    BlockInfoError(String),
    #[error("Report store error: {0}")]
    StoreError(String),
    #[error("Failed to acquire semaphore: {0}")]
    SemaphoreError(String),
}

pub type ApiErr<T> = Result<T, BlockReportError>;

/// BlockReportAPI provides functionality to replay blocks and generate event reports
#[derive(Clone)]
pub struct BlockReportAPI {
    reporting_casper: Arc<dyn ReportingCasper>,
    report_store: ReportStore,
    engine_cell: EngineCell,
    #[allow(dead_code)] // Kept for API compatibility, but we use casper's block_store instead
    block_store: KeyValueBlockStore,
    #[allow(dead_code)] // Part of constructor signature matching Scala, not directly used
    oracle: CliqueOracleImpl,
    block_report_semaphore: Arc<Semaphore>,
    block_report_queue_timeout: Duration,
    /// Transformer for converting reporting events to protobuf format
    report_transformer: Arc<ReportingProtoTransformer>,
    /// When true, allows block reports on validator nodes (bypasses read-only check)
    dev_mode: bool,
}

impl BlockReportAPI {
    /// Create a new BlockReportAPI
    pub fn new(
        reporting_casper: Arc<dyn ReportingCasper>,
        report_store: ReportStore,
        engine_cell: EngineCell,
        block_store: KeyValueBlockStore,
        oracle: CliqueOracleImpl,
        dev_mode: bool,
    ) -> Self {
        Self {
            reporting_casper,
            report_store,
            engine_cell,
            block_store,
            oracle,
            block_report_semaphore: Arc::new(Semaphore::new(1)),
            block_report_queue_timeout: Duration::from_millis(
                env::var_or(
                    BLOCK_REPORT_QUEUE_TIMEOUT_MS_ENV,
                    BLOCK_REPORT_QUEUE_TIMEOUT_MS_DEFAULT,
                )
                .max(1),
            ),
            report_transformer: Arc::new(ReportingProtoTransformer::new()),
            dev_mode,
        }
    }

    /// Replay a block and create BlockEventInfo
    async fn replay_block(
        &self,
        block: &BlockMessage,
        casper: &Arc<dyn crate::rust::casper::MultiParentCasper + Send + Sync>,
    ) -> ApiErr<BlockEventInfo> {
        let report_result = self
            .reporting_casper
            .trace(block)
            .await
            .map_err(|e| BlockReportError::ReplayFailed(e))?;

        let light_block = BlockAPI::get_light_block_info(casper.as_ref(), block)
            .await
            .map_err(|e| BlockReportError::BlockInfoError(e.to_string()))?;

        let deploys = self.create_deploy_report(&report_result.deploy_report_result);

        let sys_deploys =
            self.create_system_deploy_report(&report_result.system_deploy_report_result);

        let post_state_hash_bytes: Bytes = report_result.post_state_hash.into();
        Ok(BlockEventInfo {
            block_info: Some(light_block).into(),
            deploys,
            system_deploys: sys_deploys,
            post_state_hash: post_state_hash_bytes,
        })
    }

    /// Get block report with locking to prevent concurrent replays of the same block
    async fn block_report_within_lock(
        &self,
        force_replay: bool,
        block: &BlockMessage,
        casper: &Arc<dyn crate::rust::casper::MultiParentCasper + Send + Sync>,
    ) -> ApiErr<BlockEventInfo> {
        metrics::gauge!("block_report.lock.queue_size", "source" => "casper").increment(1.0);
        let permit = tokio::time::timeout(
            self.block_report_queue_timeout,
            self.block_report_semaphore.acquire(),
        )
        .await;
        metrics::gauge!("block_report.lock.queue_size", "source" => "casper").decrement(1.0);
        let _permit = match permit {
            Ok(Ok(permit)) => permit,
            Ok(Err(error)) => return Err(BlockReportError::SemaphoreError(error.to_string())),
            Err(_) => {
                metrics::counter!("block_report.busy", "source" => "casper").increment(1);
                return Err(BlockReportError::Busy);
            }
        };

        self.block_report_inner(force_replay, block, casper).await
    }

    /// Inner block report logic (separated to ensure lock map cleanup on all paths)
    async fn block_report_inner(
        &self,
        force_replay: bool,
        block: &BlockMessage,
        casper: &Arc<dyn crate::rust::casper::MultiParentCasper + Send + Sync>,
    ) -> ApiErr<BlockEventInfo> {
        let block_hash_bytes = report_cache_key(&block.block_hash);
        let cached = self
            .report_store
            .get(&vec![block_hash_bytes.clone()])
            .map_err(|e| BlockReportError::StoreError(e.to_string()))?;

        if let Some(Some(cached_report)) = cached.first() {
            if !force_replay {
                return Ok(cached_report.clone());
            }
        }

        let pre_state_hash = Blake2b256Hash::from_bytes_prost(&proto_util::pre_state_hash(block));
        let has_pre_state = casper
            .runtime_manager()
            .has_root(&pre_state_hash)
            .map_err(|error| BlockReportError::ReplayFailed(error.to_string()))?;
        if !has_pre_state {
            metrics::counter!("block_report.state_unavailable", "source" => "casper").increment(1);
            return Err(BlockReportError::StateUnavailable(block.block_hash.clone()));
        }

        let report = self.replay_block(block, casper).await?;

        self.report_store
            .put(vec![(block_hash_bytes, report.clone())])
            .map_err(|e| BlockReportError::StoreError(e.to_string()))?;

        Ok(report)
    }

    /// Get block report for a given block hash
    pub async fn block_report(
        &self,
        hash: BlockHash,
        force_replay: bool,
    ) -> ApiErr<BlockEventInfo> {
        let eng = self.engine_cell.get().await;
        let casper = eng
            .with_casper()
            .ok_or(BlockReportError::CasperNotInitialized)?;

        let validator_opt = casper.get_validator();
        if validator_opt.is_some() && !self.dev_mode {
            return Err(BlockReportError::ReadOnlyRequired);
        }

        let casper_block_store = casper.block_store();
        let block_opt = casper_block_store
            .get(&hash)
            .map_err(|e| BlockReportError::StoreError(e.to_string()))?;

        let block = block_opt.ok_or_else(|| BlockReportError::BlockNotFound(hash))?;

        self.block_report_within_lock(force_replay, &block, &casper)
            .await
    }

    pub fn cached_block_report(&self, hash: &BlockHash) -> ApiErr<Option<BlockEventInfo>> {
        let key = report_cache_key(hash);
        let cached = self
            .report_store
            .get(&vec![key])
            .map_err(|e| BlockReportError::StoreError(e.to_string()))?;

        Ok(cached.into_iter().next().flatten())
    }

    /// Create system deploy report from replay results
    fn create_system_deploy_report(
        &self,
        result: &[crate::rust::reporting_casper::SystemDeployReportResult],
    ) -> Vec<SystemDeployInfoWithEventData> {
        result
            .iter()
            .map(|sd| {
                let system_deploy_proto =
                    SystemDeployData::to_proto(sd.processed_system_deploy.clone());

                let report: Vec<SingleReport> = sd
                    .events
                    .iter()
                    .map(|event_batch| {
                        let events: Vec<ReportProto> = event_batch
                            .iter()
                            .map(|event| {
                                ReportingTransformer::transform_event(
                                    self.report_transformer.as_ref(),
                                    event,
                                )
                            })
                            .collect();

                        SingleReport { events }
                    })
                    .collect();

                SystemDeployInfoWithEventData {
                    system_deploy: Some(system_deploy_proto).into(),
                    report,
                }
            })
            .collect()
    }

    /// Create deploy report from replay results
    fn create_deploy_report(
        &self,
        result: &[crate::rust::reporting_casper::DeployReportResult],
    ) -> Vec<DeployInfoWithEventData> {
        result
            .iter()
            .map(|p| {
                let deploy_info = p.processed_deploy.clone().to_deploy_info();

                let report: Vec<SingleReport> = p
                    .events
                    .iter()
                    .map(|event_batch| {
                        let events: Vec<ReportProto> = event_batch
                            .iter()
                            .map(|event| {
                                ReportingTransformer::transform_event(
                                    self.report_transformer.as_ref(),
                                    event,
                                )
                            })
                            .collect();

                        SingleReport { events }
                    })
                    .collect();

                DeployInfoWithEventData {
                    deploy_info: Some(deploy_info).into(),
                    report,
                }
            })
            .collect()
    }
}
