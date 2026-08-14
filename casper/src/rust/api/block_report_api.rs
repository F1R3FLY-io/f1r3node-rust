// See casper/src/main/scala/coop/rchain/casper/api/BlockReportAPI.scala

use std::sync::Arc;

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
use shared::rust::ByteString;
use tokio::sync::Semaphore;

use crate::rust::api::block_api::BlockAPI;
use crate::rust::engine::engine_cell::EngineCell;
use crate::rust::report_store::ReportStore;
use crate::rust::reporting_casper::ReportingCasper;
use crate::rust::reporting_proto_transformer::ReportingProtoTransformer;
use crate::rust::safety_oracle::CliqueOracleImpl;
use crate::rust::util::proto_util;

const REPORT_CACHE_VERSION: &[u8] = b"v2:";

/// How a caller wants the single report permit acquired.
///
/// The two callers differ in kind rather than degree, which is why there is no
/// shared timeout to tune: a client is either waiting on the response or nothing
/// is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportPermit {
    /// A request is blocked on this. Refuse immediately rather than hold the
    /// client while another replay runs.
    RejectIfHeld,
    /// Background pre-caching with no client attached. Queue until the permit
    /// frees; giving up would drop the block with nothing to retry it.
    Wait,
}

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
    #[error(
        "computed post-state {computed} does not match recorded post-state {recorded} for block {block}"
    )]
    PostStateMismatch {
        block: String,
        computed: String,
        recorded: String,
    },
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
    /// One permit for the whole node: report generation replays a block, so
    /// concurrent replays are what this API exists to bound.
    block_report_semaphore: Arc<Semaphore>,
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

        let expected_post_state = proto_util::post_state_hash(block);
        if report_result.post_state_hash.as_slice() != expected_post_state.as_ref() {
            // A replay that succeeded yet produced divergent state is a state-integrity signal, not
            // routine reporting noise. Counted and logged here because callers of this API vary in
            // what they do with the error — one discards it outright and another files it under an
            // expected condition — so neither the metric nor the record can be left to them.
            metrics::counter!("block_report.post_state_mismatch", "source" => "casper")
                .increment(1);
            tracing::error!(
                target: "f1r3fly.casper.reporting",
                block = %hex::encode(&block.block_hash),
                computed = %hex::encode(&report_result.post_state_hash),
                recorded = %hex::encode(&expected_post_state),
                "Replay post-state does not match the block's recorded post-state; refusing to cache the report"
            );
            return Err(BlockReportError::PostStateMismatch {
                block: hex::encode(&block.block_hash),
                computed: hex::encode(&report_result.post_state_hash),
                recorded: hex::encode(&expected_post_state),
            });
        }

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

    /// Serialize report generation node-wide, acquiring the permit as the caller
    /// asked for it.
    async fn block_report_within_lock(
        &self,
        force_replay: bool,
        block: &BlockMessage,
        casper: &Arc<dyn crate::rust::casper::MultiParentCasper + Send + Sync>,
        permit_policy: ReportPermit,
    ) -> ApiErr<BlockEventInfo> {
        let _permit = match permit_policy {
            ReportPermit::RejectIfHeld => match self.block_report_semaphore.try_acquire() {
                Ok(permit) => permit,
                Err(tokio::sync::TryAcquireError::NoPermits) => {
                    metrics::counter!("block_report.busy", "source" => "casper").increment(1);
                    return Err(BlockReportError::Busy);
                }
                Err(error) => {
                    return Err(BlockReportError::SemaphoreError(error.to_string()));
                }
            },
            ReportPermit::Wait => {
                metrics::gauge!("block_report.lock.queue_size", "source" => "casper")
                    .increment(1.0);
                let permit = self.block_report_semaphore.acquire().await;
                metrics::gauge!("block_report.lock.queue_size", "source" => "casper")
                    .decrement(1.0);
                permit.map_err(|error| BlockReportError::SemaphoreError(error.to_string()))?
            }
        };

        self.block_report_inner(force_replay, block, casper).await
    }

    /// Inner block report logic, run while holding the permit.
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

    /// Get block report for a given block hash, refusing rather than queueing
    /// when another replay holds the permit.
    pub async fn block_report(
        &self,
        hash: BlockHash,
        force_replay: bool,
    ) -> ApiErr<BlockEventInfo> {
        self.block_report_with_permit(hash, force_replay, ReportPermit::RejectIfHeld)
            .await
    }

    /// Generate and cache a report for background pre-caching. Queues for the
    /// permit instead of refusing, because no client is waiting and a refusal
    /// here drops the block with nothing to retry it.
    pub async fn prewarm_block_report(&self, hash: BlockHash) -> ApiErr<BlockEventInfo> {
        self.block_report_with_permit(hash, false, ReportPermit::Wait)
            .await
    }

    async fn block_report_with_permit(
        &self,
        hash: BlockHash,
        force_replay: bool,
        permit_policy: ReportPermit,
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

        self.block_report_within_lock(force_replay, &block, &casper, permit_policy)
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
