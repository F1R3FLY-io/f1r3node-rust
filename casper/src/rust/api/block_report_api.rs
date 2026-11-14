// See casper/src/main/scala/coop/rchain/casper/api/BlockReportAPI.scala

use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
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
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::ByteString;
use tokio::sync::Semaphore;

use crate::rust::api::block_api::BlockAPI;
use crate::rust::engine::engine_cell::EngineCell;
use crate::rust::report_store::ReportStore;
use crate::rust::reporting_casper::ReportingCasper;
use crate::rust::reporting_proto_transformer::{to_proto_phase, ReportingProtoTransformer};
use crate::rust::safety_oracle::CliqueOracleImpl;
use crate::rust::util::proto_util;

struct ReportQueueMetricGuard;

impl ReportQueueMetricGuard {
    fn new() -> Self {
        metrics::gauge!("block_report.lock.queue_size", "source" => "casper").increment(1.0);
        Self
    }
}

impl Drop for ReportQueueMetricGuard {
    fn drop(&mut self) {
        metrics::gauge!("block_report.lock.queue_size", "source" => "casper").decrement(1.0);
    }
}

fn report_cache_key(block_hash: &BlockHash) -> ByteString { block_hash.to_vec() }

/// Upper bound on the heights holding reports that one prune step deletes,
/// so a step holds only a few reports in memory.
const REPORT_PRUNE_STEP_HEIGHTS: usize = 8;

/// Upper bound on the heights one prune step walks. A height without cached
/// reports costs only lookups, so a step after a restart, when the cursor is
/// back at 0, crosses many already-pruned heights.
const REPORT_PRUNE_STEP_WALK: i64 = 4_096;

/// The height (exclusive) where the prune step that starts at `cursor` stops
/// walking.
fn report_prune_walk_end(cursor: i64, window_start: i64) -> i64 {
    window_start.min(cursor.saturating_add(REPORT_PRUNE_STEP_WALK))
}

/// True when a block at `block_height` is below the report window. A block
/// that is not in the DAG is not below it.
fn below_report_window(block_height: Option<i64>, window_start: i64) -> bool {
    block_height.is_some_and(|height| height < window_start)
}

/// How a caller waits for the single report permit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportPermit {
    /// Best-effort enrichment of a read. Refuse at once rather than hold the
    /// read behind another replay.
    RejectIfHeld,
    /// Explicit report requests and pre-caching. Queue until the permit frees.
    Wait,
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
    /// Height window of the report cache below the last finalized block; 0
    /// keeps every report.
    report_cache_retention: u32,
    /// First height that may still hold reports below the window.
    report_prune_cursor: Arc<std::sync::Mutex<i64>>,
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
            report_cache_retention: 0,
            report_prune_cursor: Arc::new(std::sync::Mutex::new(0)),
        }
    }

    /// Keep cached reports only for blocks at most `blocks` heights below the
    /// last finalized block. 0 keeps every report.
    pub fn with_report_cache_retention(mut self, blocks: u32) -> Self {
        self.report_cache_retention = blocks;
        self
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

    /// Serialize report generation node-wide.
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
                Err(error) => return Err(BlockReportError::SemaphoreError(error.to_string())),
            },
            ReportPermit::Wait => {
                let queue_guard = ReportQueueMetricGuard::new();
                let permit = self
                    .block_report_semaphore
                    .acquire()
                    .await
                    .map_err(|error| BlockReportError::SemaphoreError(error.to_string()))?;
                drop(queue_guard);
                permit
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
        // Re-checked under the permit: a replay that finished while this caller
        // was queueing has already cached the answer.
        if !force_replay {
            if let Some(cached_report) = self.cached_report(&block.block_hash)? {
                return Ok(cached_report);
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

        let window = self.report_window(casper).await?;
        if let Some((dag, window_start)) = &window {
            let block_number = dag
                .block_number(&block.block_hash)
                .map_err(|e| BlockReportError::StoreError(e.to_string()))?;
            if below_report_window(block_number, *window_start) {
                metrics::counter!("block_report.cache_skipped_below_window", "source" => "casper")
                    .increment(1);
                return Ok(report);
            }
        }

        self.report_store
            .put(vec![(report_cache_key(&block.block_hash), report.clone())])
            .map_err(|e| BlockReportError::StoreError(e.to_string()))?;

        if let Some((dag, window_start)) = window {
            self.prune_step(&dag, window_start);
        }

        Ok(report)
    }

    /// The DAG and the first height of the report window, or `None` when
    /// every report is kept.
    async fn report_window(
        &self,
        casper: &Arc<dyn crate::rust::casper::MultiParentCasper + Send + Sync>,
    ) -> ApiErr<Option<(KeyValueDagRepresentation, i64)>> {
        if self.report_cache_retention == 0 {
            return Ok(None);
        }
        let dag = casper
            .block_dag()
            .await
            .map_err(|e| BlockReportError::StoreError(e.to_string()))?;
        let Some(lfb_height) = dag
            .block_number(&dag.last_finalized_block())
            .map_err(|e| BlockReportError::StoreError(e.to_string()))?
        else {
            return Ok(None);
        };
        let window_start = lfb_height - i64::from(self.report_cache_retention);
        Ok(Some((dag, window_start)))
    }

    /// Delete the reports of the next few heights below `window_start` that
    /// hold any, one height at a time, so a step holds only a few reports in
    /// memory.
    fn prune_step(&self, dag: &KeyValueDagRepresentation, window_start: i64) {
        let mut cursor = self
            .report_prune_cursor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let walk_end = report_prune_walk_end(*cursor, window_start);
        let mut pruned_heights = 0;
        while *cursor < walk_end && pruned_heights < REPORT_PRUNE_STEP_HEIGHTS {
            let height = *cursor;
            match self.prune_height(dag, height) {
                Ok((reports, bytes)) => {
                    if reports > 0 {
                        pruned_heights += 1;
                        metrics::counter!("block_report.cache_pruned_reports", "source" => "casper")
                            .increment(reports as u64);
                        metrics::counter!("block_report.cache_pruned_bytes", "source" => "casper")
                            .increment(bytes);
                    }
                    *cursor = height + 1;
                }
                Err(e) => {
                    tracing::warn!(
                        height,
                        error = %e,
                        "Block report cache prune failed; retrying after the next cached report"
                    );
                    return;
                }
            }
        }
    }

    fn prune_height(
        &self,
        dag: &KeyValueDagRepresentation,
        height: i64,
    ) -> Result<(usize, u64), KvStoreError> {
        let keys = dag
            .topo_sort(height, Some(height))?
            .into_iter()
            .flatten()
            .map(|hash| report_cache_key(&hash))
            .collect();
        self.report_store.delete_measured(keys)
    }

    /// Get block report for a given block hash.
    pub async fn block_report(
        &self,
        hash: BlockHash,
        force_replay: bool,
    ) -> ApiErr<BlockEventInfo> {
        self.block_report_with_permit(hash, force_replay, ReportPermit::Wait)
            .await
    }

    /// Get the block report for enriching a read: the cached report, else a
    /// replay only when no other replay holds the permit, else `Busy` at once.
    pub async fn block_report_if_idle(&self, hash: BlockHash) -> ApiErr<BlockEventInfo> {
        self.block_report_with_permit(hash, false, ReportPermit::RejectIfHeld)
            .await
    }

    /// Generate and cache a report for background pre-caching.
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

        // Answered before the permit is involved: serving a report that already
        // exists costs a store read, so a cached block stays readable while a
        // replay holds the permit. Only generating one has to queue.
        if !force_replay {
            if let Some(cached) = self.cached_report(&block.block_hash)? {
                return Ok(cached);
            }
        }

        self.block_report_within_lock(force_replay, &block, &casper, permit_policy)
            .await
    }

    fn cached_report(&self, hash: &BlockHash) -> ApiErr<Option<BlockEventInfo>> {
        let cached = self
            .report_store
            .get(&vec![report_cache_key(hash)])
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
                    .map(|batch| {
                        let events: Vec<ReportProto> = batch
                            .events
                            .iter()
                            .map(|event| {
                                ReportingTransformer::transform_event(
                                    self.report_transformer.as_ref(),
                                    event,
                                )
                            })
                            .collect();

                        SingleReport {
                            events,
                            phase: to_proto_phase(batch.phase),
                        }
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
                    .map(|batch| {
                        let events: Vec<ReportProto> = batch
                            .events
                            .iter()
                            .map(|event| {
                                ReportingTransformer::transform_event(
                                    self.report_transformer.as_ref(),
                                    event,
                                )
                            })
                            .collect();

                        SingleReport {
                            events,
                            phase: to_proto_phase(batch.phase),
                        }
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

#[cfg(test)]
mod tests {
    use super::{below_report_window, report_prune_walk_end, REPORT_PRUNE_STEP_WALK};

    #[test]
    fn a_prune_step_stops_walking_at_the_window() {
        assert_eq!(report_prune_walk_end(0, 3), 3);
        assert_eq!(report_prune_walk_end(5, 5), 5);
        assert_eq!(report_prune_walk_end(5, -3), -3);
    }

    #[test]
    fn a_prune_step_walks_a_bounded_number_of_heights() {
        assert_eq!(report_prune_walk_end(0, 1_000_000), REPORT_PRUNE_STEP_WALK);
        assert_eq!(
            report_prune_walk_end(REPORT_PRUNE_STEP_WALK, 1_000_000),
            2 * REPORT_PRUNE_STEP_WALK
        );
    }

    #[test]
    fn only_blocks_below_the_window_start_are_below_the_window() {
        assert!(below_report_window(Some(9), 10));
        assert!(!below_report_window(Some(10), 10));
        assert!(!below_report_window(Some(11), 10));
        assert!(!below_report_window(None, 10));
        assert!(!below_report_window(Some(0), -50));
    }
}
