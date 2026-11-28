// See casper/src/main/scala/coop/rchain/casper/api/BlockAPI.scala

use std::collections::HashSet;
use std::future::Future;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use block_storage::rust::dag::block_dag_key_value_storage::{DeployId, KeyValueDagRepresentation};
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use futures::future;
use models::casper::{
    BlockInfo, ContinuationsWithBlockInfo, DataWithBlockInfo, LightBlockInfo, RejectedDeployInfo,
    WaitingContinuationInfo,
};
use models::rhoapi::Par;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use models::rust::deploy_parameters::DeployParameter;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::bytes::Bytes;
use prost::Message;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::trace::event::{Event as RspaceEvent, IOEvent};
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::ByteString;

use crate::rust::blocks::proposer::propose_result::{
    CheckProposeConstraintsFailure, ProposeFailure, ProposeResult, ProposeStatus,
};
use crate::rust::blocks::proposer::proposer::ProposerResult;
use crate::rust::casper::MultiParentCasper;
use crate::rust::engine::engine_cell::EngineCell;
use crate::rust::errors::CasperError;
use crate::rust::genesis::contracts::standard_deploys;
use crate::rust::reporting_proto_transformer::ReportingProtoTransformer;
use crate::rust::safety_oracle::{CliqueOracleImpl, SafetyOracle, MIN_FAULT_TOLERANCE};
use crate::rust::state::instances::proposer_state::ProposerState;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::tools::Tools;
use crate::rust::util::{event_converter, proto_util};
use crate::rust::ProposeFunction;
pub struct BlockAPI;

pub type ApiErr<T> = eyre::Result<T>;

#[derive(Debug, thiserror::Error)]
#[error("Couldn't find block containing deploy with id: {deploy_id}")]
pub struct DeployNotFoundError {
    pub deploy_id: String,
}

// Look at shared/src/main/scala/coop/rchain/shared/Base16.scala
// Scala Base16.decode pads odd-length hex strings with leading zero
fn pad_hex_string(hash: &str) -> String {
    if hash.len().is_multiple_of(2) {
        hash.to_string()
    } else {
        format!("0{}", hash)
    }
}

// Automatic error conversions for common error types used in this API
// We can only implement From for our own types, so we implement for CasperError -> String
impl From<CasperError> for String {
    fn from(err: CasperError) -> String { err.to_string() }
}

fn recoverable_propose_failure_message(status: &ProposeStatus) -> Option<String> {
    match status {
        ProposeStatus::Failure(ProposeFailure::NoNewDeploys) => {
            Some("No new deploys to propose.".to_string())
        }
        ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
            CheckProposeConstraintsFailure::NotEnoughNewBlocks,
        )) => Some("No new blocks from peers yet; synchronize with network first.".to_string()),
        ProposeStatus::Failure(ProposeFailure::InternalDeployError) => {
            Some("Propose skipped due to transient proposal race.".to_string())
        }
        _ => {
            let normalized = format!("{}", status);
            if normalized.contains("Must wait for more blocks from other validators") {
                Some("No new blocks from peers yet; synchronize with network first.".to_string())
            } else {
                None
            }
        }
    }
}

const DEPLOY_PROPOSE_MAX_ATTEMPTS: u32 = 4;
const DEPLOY_PROPOSE_RETRY_DELAY_MS: u64 = 250;

fn deploy_propose_max_attempts() -> u32 { DEPLOY_PROPOSE_MAX_ATTEMPTS }

fn deploy_propose_retry_delay() -> Duration { Duration::from_millis(DEPLOY_PROPOSE_RETRY_DELAY_MS) }

fn deploy_is_block_expired(
    valid_after_block_number: i64,
    latest_block_number: i64,
    deploy_lifespan: i64,
) -> Result<bool, CasperError> {
    Ok(!models::rust::deploy_window::is_open(
        valid_after_block_number,
        latest_block_number,
        deploy_lifespan,
    )?)
}

fn should_retry_deploy_propose(status: &ProposeStatus) -> bool {
    match status {
        ProposeStatus::Failure(ProposeFailure::InternalDeployError)
        | ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
            CheckProposeConstraintsFailure::NotEnoughNewBlocks,
        ))
        | ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
            CheckProposeConstraintsFailure::TooFarAheadOfLastFinalized,
        )) => true,
        _ => {
            let normalized = format!("{}", status);
            normalized.contains("Must wait for more blocks from other validators")
        }
    }
}

fn clamp_depth(requested_depth: i32, max_depth_limit: i32, operation: &str) -> i32 {
    let normalized_limit = max_depth_limit.max(0);
    let effective_depth = requested_depth.max(0).min(normalized_limit);

    if effective_depth != requested_depth {
        tracing::warn!(
            operation,
            requested_depth,
            max_depth_limit,
            effective_depth,
            "Requested depth is out of bounds; clamping to configured maximum."
        );
    }

    effective_depth
}

fn clamp_end_block_number(
    start_block_number: i64,
    requested_end_block_number: i64,
    max_blocks_limit: i32,
) -> i64 {
    let normalized_limit = i64::from(max_blocks_limit.max(0));
    let max_allowed_end = start_block_number.saturating_add(normalized_limit);
    let effective_end_block_number = requested_end_block_number.min(max_allowed_end);

    if effective_end_block_number != requested_end_block_number {
        tracing::warn!(
            start_block_number,
            requested_end_block_number,
            max_blocks_limit,
            effective_end_block_number,
            "Requested block range exceeds configured maximum; clamping end block."
        );
    }

    effective_end_block_number
}

lazy_static::lazy_static! {
    static ref REPORT_TRANSFORMER: ReportingProtoTransformer = ReportingProtoTransformer::new();
}

// TODO: Scala we should refactor BlockApi with applicative errors for better classification of errors and to overcome nesting when validating data.
#[derive(Debug)]
pub struct BlockNotFoundError {
    pub hash: String,
}

impl std::fmt::Display for BlockNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Block not found: {}", self.hash)
    }
}

impl std::error::Error for BlockNotFoundError {}

/// The block was once held by this node but has been deleted by the
/// retention sweep. The DAG still carries its metadata (hash, parents,
/// height) so we can report the height at which it lived, and the
/// current prune floor so consumers can decide whether to ask an
/// archive node. Distinct from [`BlockNotFoundError`], which means
/// "we have no record of this hash at all" — a joiner that hasn't
/// caught up yet, or an outright invalid hash. #923 Option D callers
/// use this split so a client walking `/reporting/trace` can tell
/// deleted history from a gap and route accordingly.
#[derive(Debug)]
pub struct BlockPrunedError {
    pub hash: String,
    /// Height of the block when it was retained. Known from the DAG
    /// metadata index, which is not swept.
    pub block_height: i64,
    /// The node's current body-prune floor: the highest height for
    /// which block bodies have been deleted. Callers can request the
    /// same hash from an archive node with retention above this.
    pub prune_floor: i64,
}

impl std::fmt::Display for BlockPrunedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Block {} (height {}) is below this node's prune floor (height {}). \
             Consult an archive node to retrieve it.",
            self.hash, self.block_height, self.prune_floor
        )
    }
}

impl std::error::Error for BlockPrunedError {}

/// The DAG metadata index names this hash but its body is not in the
/// block store, and the retention sweep has NOT yet advanced past this
/// height (`block_height > prune_floor`). The body is transiently
/// unavailable — an LFS-restore backfill horizon, a pending-body race,
/// or (rarely) a storage inconsistency. Distinct from
/// [`BlockPrunedError`], which the server classifies only when the
/// sweep has reached the height and retry on this node cannot
/// succeed. Mapped to HTTP 503 by the web layer so clients retry.
/// See #1087.
#[derive(Debug)]
pub struct BlockUnavailableError {
    pub hash: String,
    pub block_height: i64,
    pub prune_floor: i64,
}

impl std::fmt::Display for BlockUnavailableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Block {} (height {}) is known to this node but its body is \
             not currently available (prune floor is at height {}). \
             Retry later; this node has not pruned past this height.",
            self.hash, self.block_height, self.prune_floor
        )
    }
}

impl std::error::Error for BlockUnavailableError {}

/// One hash that `topo_sort` returned at a given height but whose block
/// body was no longer available in the block store when
/// [`BlockAPI::get_blocks_by_heights`] walked the range. See #1087.
#[derive(Debug, Clone)]
pub struct MissingBlockEntry {
    pub hash: BlockHash,
    pub block_height: i64,
}

/// Issue #1157: the target block's rspace STATE has been retention-swept.
///
/// Mirrors [`BlockPrunedError`] for the state-reading APIs
/// (`exploratory_deploy`, `get_data_at_par`, block reports). The DAG still
/// carries the block's metadata so we can report the height at which it
/// lived; the trie nodes under its post-state root (or pre-state, for
/// exploratory reads that walk the pre-state view) have been deleted by
/// the `enable_history_pruning` sweep. Distinct from
/// [`BlockPrunedError`] (block BODY vs STATE — the two sweeps have
/// independent floors on an operator who only enabled one) and from
/// [`BlockNotFoundError`] ("we have no record of this hash at all").
#[derive(Debug, Clone)]
pub struct PrunedStateError {
    /// Hash of the block whose state the reader was asked for.
    pub target_block_hash: BlockHash,
    /// Height of that block when it was retained. Known from the DAG
    /// metadata index, which is not swept.
    pub target_block_height: i64,
    /// The node's current rspace-history retention floor
    /// (`CasperShardConf::state_retention_floor(LFB_height)`). The target
    /// block's height is strictly less than this value — blocks at the
    /// floor itself survive the sweep.
    pub retention_floor: i64,
}

impl std::fmt::Display for PrunedStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Block {} (height {}) state has been swept past this node's rspace-history \
             retention floor (height {}). Consult an archive node whose retention \
             window covers it.",
            PrettyPrinter::build_string_bytes(&self.target_block_hash),
            self.target_block_height,
            self.retention_floor
        )
    }
}

impl std::error::Error for PrunedStateError {}

/// Issue #1164: true when an eyre error chain carries a mid-walk
/// `RadixTreeError::MissingNode` surfaced by the rspace history layer.
///
/// Both nesting shapes classify identically — the race is the same
/// regardless of whether the walk descended through `HistoryError`
/// (reset-and-replay path) or straight through `RSpaceError` (direct
/// radix read). Mirrors the shapes [`BlockError::from_validation_error`]
/// already recognises for the validator classifier.
fn eyre_chain_has_missing_node(err: &eyre::Report) -> bool {
    use rholang::rust::interpreter::errors::InterpreterError;
    use rspace_plus_plus::rspace::errors::{HistoryError, RSpaceError, RadixTreeError};

    err.chain().any(|cause| {
        let Some(casper_err) = cause.downcast_ref::<CasperError>() else {
            return false;
        };
        matches!(
            casper_err,
            CasperError::InterpreterError(InterpreterError::RSpaceError(
                RSpaceError::HistoryError(HistoryError::RadixTreeError(
                    RadixTreeError::MissingNode { .. }
                ))
            )) | CasperError::InterpreterError(InterpreterError::RSpaceError(
                RSpaceError::RadixTreeError(RadixTreeError::MissingNode { .. })
            ))
        )
    })
}

/// Envelope for [`BlockAPI::get_blocks_by_heights`] /
/// [`BlockAPI::get_blocks_by_heights_full`]. `served` is the flat list
/// of blocks whose bodies the node still holds, in the same order as
/// before #1087.
///
/// `pruned` and `unavailable` split the "missing body" cases by the
/// same threshold the single-hash path uses:
/// - `pruned`: `block_height <= prune_floor`. The retention sweep has
///   advanced past this height — the gap is permanent on this node.
///   Mirrors [`BlockPrunedError`] semantics.
/// - `unavailable`: `block_height > prune_floor`. The DAG names the
///   hash but the body is not currently in the store and the sweep
///   has not reached this height. Mirrors [`BlockUnavailableError`]
///   semantics; retry may succeed.
///
/// `prune_floor` is the effective-anchor snapshot taken once at the
/// start of the walk; the same value applies to every entry in both
/// buckets.
#[derive(Debug, Clone)]
pub struct BlocksByHeightsResponse<T> {
    pub served: Vec<T>,
    pub pruned: Vec<MissingBlockEntry>,
    pub unavailable: Vec<MissingBlockEntry>,
    pub prune_floor: i64,
}

#[derive(Debug)]
pub struct InvalidHashError(pub String);

impl std::fmt::Display for InvalidHashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

impl std::error::Error for InvalidHashError {}

#[derive(Debug)]
pub struct ExploratoryDeployReadOnlyError;

impl std::fmt::Display for ExploratoryDeployReadOnlyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Exploratory deploy can only be executed on read-only node"
        )
    }
}

impl std::error::Error for ExploratoryDeployReadOnlyError {}

#[derive(Debug, thiserror::Error)]
#[error("Node exploratory query capacity is exhausted; retry in {retry_after_secs} s")]
pub struct ExploratoryDeployBusyError {
    /// Seconds to advertise in `Retry-After`. Derived from the configured
    /// execution budget: capacity is held until the occupying query terminates,
    /// so that budget is the earliest a caller can expect a slot to free.
    pub retry_after_secs: u64,
}

impl ExploratoryDeployBusyError {
    /// Rounds up to one second because `Retry-After` is expressed in whole
    /// seconds — a sub-second budget cannot be advertised as zero without
    /// telling the caller to retry immediately.
    fn with_budget(execution_budget: Duration) -> Self {
        Self {
            retry_after_secs: execution_budget.as_secs().max(1),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(
    "Exploratory query cancelled after exceeding its {timeout_ms} ms execution budget; \
     the budget bounds execution, not response time"
)]
pub struct ExploratoryDeployTimeoutError {
    pub timeout_ms: u64,
}

/// An exploratory-deploy rejection that every transport can express natively —
/// HTTP via a status code, gRPC via `tonic::Status`.
///
/// Classification matches the error variant and carries its data forward; it
/// never inspects the rendered message. That is what keeps the two transport
/// surfaces from drifting apart, and it is why the payload lives here rather
/// than being re-derived per transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExploratoryDeployRejection {
    Busy { retry_after_secs: u64 },
    Timeout { timeout_ms: u64 },
}

impl ExploratoryDeployRejection {
    /// Classify one cause in an error chain, so a caller walking the chain can
    /// keep using that cause's own message for the response body.
    pub fn from_cause(cause: &(dyn std::error::Error + 'static)) -> Option<Self> {
        if let Some(busy) = cause.downcast_ref::<ExploratoryDeployBusyError>() {
            return Some(Self::Busy {
                retry_after_secs: busy.retry_after_secs,
            });
        }
        cause
            .downcast_ref::<ExploratoryDeployTimeoutError>()
            .map(|timeout| Self::Timeout {
                timeout_ms: timeout.timeout_ms,
            })
    }

    pub fn classify(err: &eyre::Error) -> Option<Self> { err.chain().find_map(Self::from_cause) }
}

#[repr(u8)]
enum ExploratoryDeployOutcome {
    Failed,
    Completed,
    TimedOut,
}

struct ExploratoryDeployMetrics {
    started: Instant,
    outcome: Arc<AtomicU8>,
}

impl ExploratoryDeployMetrics {
    fn new(outcome: Arc<AtomicU8>) -> Self {
        metrics::gauge!("exploratory_deploy.active", "source" => "casper").increment(1.0);
        Self {
            started: Instant::now(),
            outcome,
        }
    }
}

impl Drop for ExploratoryDeployMetrics {
    fn drop(&mut self) {
        metrics::gauge!("exploratory_deploy.active", "source" => "casper").decrement(1.0);
        metrics::histogram!("exploratory_deploy.duration", "source" => "casper")
            .record(self.started.elapsed().as_secs_f64());
        match self.outcome.load(Ordering::Relaxed) {
            value if value == ExploratoryDeployOutcome::Completed as u8 => {
                metrics::counter!("exploratory_deploy.completed", "source" => "casper")
                    .increment(1);
            }
            value if value == ExploratoryDeployOutcome::TimedOut as u8 => {
                metrics::counter!("exploratory_deploy.timed_out", "source" => "casper")
                    .increment(1);
            }
            _ => {
                metrics::counter!("exploratory_deploy.failed", "source" => "casper").increment(1);
            }
        }
        RuntimeManager::trim_allocator();
    }
}

enum ExploratoryDeployTaskError {
    Join(tokio::task::JoinError),
    Timeout,
}

async fn await_exploratory_deploy_task<T>(
    mut task: tokio::task::JoinHandle<T>,
    timeout: Duration,
    outcome: Arc<AtomicU8>,
) -> Result<T, ExploratoryDeployTaskError> {
    match tokio::time::timeout(timeout, &mut task).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(ExploratoryDeployTaskError::Join(error)),
        Err(_) => {
            outcome.store(ExploratoryDeployOutcome::TimedOut as u8, Ordering::Relaxed);
            task.abort();
            let _ = task.await;
            Err(ExploratoryDeployTaskError::Timeout)
        }
    }
}

#[derive(Debug)]
pub enum LatestBlockMessageError {
    NodeReadOnlyError,
    NoBlockMessageError,
}

impl std::fmt::Display for LatestBlockMessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LatestBlockMessageError::NodeReadOnlyError => {
                write!(
                    f,
                    "node is running in read-only mode; this endpoint requires a validator"
                )
            }
            LatestBlockMessageError::NoBlockMessageError => {
                write!(f, "no block is available yet")
            }
        }
    }
}

impl std::error::Error for LatestBlockMessageError {}

#[derive(Debug)]
pub struct InvalidPublicKeyError(pub String);

impl std::fmt::Display for InvalidPublicKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

impl std::error::Error for InvalidPublicKeyError {}

#[derive(Debug)]
pub struct DeployValidationError {
    pub message: String,
}

impl std::fmt::Display for DeployValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for DeployValidationError {}

/// A deploy whose block window or expiration timestamp has closed: no block
/// can include it any more.
#[derive(Debug)]
pub struct DeployExpiredError {
    pub message: String,
}

impl std::fmt::Display for DeployExpiredError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for DeployExpiredError {}

/// The node's own validator is not bonded, so it cannot propose a block for
/// the deploy now. It may be bonded later.
#[derive(Debug)]
pub struct ValidatorNotBondedError;

impl std::fmt::Display for ValidatorNotBondedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "validator_not_bonded: Deploy was rejected because node's validator is not bonded."
        )
    }
}

impl std::error::Error for ValidatorNotBondedError {}

#[derive(Debug)]
pub struct ProposeReadOnlyError;

impl std::fmt::Display for ProposeReadOnlyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "propose is not available: this node is running in read-only mode"
        )
    }
}

impl std::error::Error for ProposeReadOnlyError {}

#[derive(Debug)]
pub struct NoNewDeploysError;

impl std::fmt::Display for NoNewDeploysError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "No new deploys to propose.")
    }
}

impl std::error::Error for NoNewDeploysError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposeRefusal {
    NotBonded,
    NotEnoughNewBlocks,
    TooFarAheadOfLastFinalized,
    HistoryIncomplete,
    MissingBlock,
    FloorDistanceHold,
}

impl ProposeRefusal {
    pub fn from_failure(failure: &ProposeFailure) -> Option<Self> {
        match failure {
            ProposeFailure::CheckConstraintsFailure(check) => Some(match check {
                CheckProposeConstraintsFailure::NotBonded => ProposeRefusal::NotBonded,
                CheckProposeConstraintsFailure::NotEnoughNewBlocks => {
                    ProposeRefusal::NotEnoughNewBlocks
                }
                CheckProposeConstraintsFailure::TooFarAheadOfLastFinalized => {
                    ProposeRefusal::TooFarAheadOfLastFinalized
                }
                CheckProposeConstraintsFailure::HistoryIncomplete => {
                    ProposeRefusal::HistoryIncomplete
                }
            }),
            ProposeFailure::MissingBlock(_) => Some(ProposeRefusal::MissingBlock),
            ProposeFailure::FloorDistanceHold => Some(ProposeRefusal::FloorDistanceHold),
            ProposeFailure::NoNewDeploys
            | ProposeFailure::InternalDeployError
            | ProposeFailure::BugError => None,
        }
    }
}

#[derive(Debug)]
pub struct ProposeRefusedError {
    pub refusal: ProposeRefusal,
    pub message: String,
}

impl std::fmt::Display for ProposeRefusedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ProposeRefusedError {}

fn propose_failure_error(status: &ProposeStatus, seq_number: i32) -> eyre::Report {
    let refusal = match status {
        ProposeStatus::Failure(ProposeFailure::NoNewDeploys) => {
            tracing::debug!("Propose: no new deploys (seqNum {})", seq_number);
            return eyre::Report::new(NoNewDeploysError);
        }
        ProposeStatus::Failure(failure) => ProposeRefusal::from_failure(failure),
        ProposeStatus::Success(_) => None,
    };
    let message = format!("Failure: {} (seqNum {})", status, seq_number);
    tracing::debug!("{}", message);
    match refusal {
        Some(refusal) => eyre::Report::new(ProposeRefusedError { refusal, message }),
        None => eyre::eyre!("{}", message),
    }
}

impl BlockAPI {
    fn find_deploy_scan_depth() -> usize { 128 }

    /// Issue #1157: the current rspace-history retention floor derived
    /// from this node's local LFB and shard conf. Shared between the
    /// pre-check and the mid-walk catch (issue #1164) so the two paths
    /// agree on the value they publish.
    ///
    /// Archive readers have no history floor. A pruning node needs a local
    /// finalized block to anchor its floor.
    pub(super) async fn current_state_retention_floor(
        casper: &dyn MultiParentCasper,
    ) -> Option<i64> {
        let conf = casper.casper_shard_conf();
        if !conf.enable_history_pruning {
            return None;
        }
        let lfb = casper.last_finalized_block().await.ok()?;
        Some(conf.state_retention_floor(lfb.body.state.block_number))
    }

    /// Issue #1157: deterministic retention-floor check for state-reading
    /// APIs. Called BEFORE a reader walks the trie for a specific block, so
    /// the API returns a structured [`PrunedStateError`] rather than a
    /// mid-walk `MissingNode` propagated as a generic error.
    ///
    /// Anchor is the local LFB: all honest nodes with the same shard conf
    /// derive the same floor once they have finalized the same LFB, so the
    /// response is stable across the honest set.
    ///
    /// The post-check race with the GC is covered by also classifying
    /// `MissingNode` in validation ([`BlockError::from_validation_error`])
    /// and in reader-side catch sites (issue #1164 — see
    /// [`translate_pruned_race`]).
    ///
    /// Boundary: the GC retains blocks whose `height ≥ state_retention_floor`
    /// (see `compute_gc_retention_roots`), so only `target_height < floor`
    /// falls outside the guaranteed retention window. Genesis at `height = 0` with
    /// `floor = 0` is retained.
    async fn pre_check_state_retention(
        casper: &dyn MultiParentCasper,
        target_block: &BlockMessage,
    ) -> Result<(), PrunedStateError> {
        let Some(floor) = Self::current_state_retention_floor(casper).await else {
            return Ok(());
        };
        let target_height = target_block.body.state.block_number;
        if target_height < floor {
            Err(PrunedStateError {
                target_block_hash: target_block.block_hash.clone(),
                target_block_height: target_height,
                retention_floor: floor,
            })
        } else {
            Ok(())
        }
    }

    /// Issue #1164: reader-side catch for the race the pre-check cannot
    /// close. `pre_check_state_retention` only sees the floor as it was
    /// when the API call entered; if the history-pruning sweep runs
    /// between the pre-check and the state read, the walk descends into
    /// a subtree the sweep has since deleted and surfaces
    /// `RadixTreeError::MissingNode`. Translating that into the same
    /// typed [`PrunedStateError`] the pre-check would have produced
    /// keeps the typed `pruned` answer on `ExploratoryDeployResponse` /
    /// `RhoDataResponse` instead of leaking a prose `eyre::Report`.
    ///
    /// Both nesting shapes of the inner error appear in the wild — the
    /// reset-and-replay path wraps the `RadixTreeError` through
    /// `HistoryError`, while a direct radix-trie read surfaces it
    /// straight from `RSpaceError`. Both must classify identically; the
    /// sweep race is the same condition regardless of which call site
    /// walked into it. Mirrors
    /// [`BlockError::from_validation_error`]'s handling of the same two
    /// shapes.
    async fn translate_pruned_race(
        casper: &dyn MultiParentCasper,
        target_block: &BlockMessage,
        report: eyre::Report,
    ) -> eyre::Report {
        if !eyre_chain_has_missing_node(&report) {
            return report;
        }
        let Some(floor) = Self::current_state_retention_floor(casper).await else {
            return report;
        };
        eyre::Report::new(PrunedStateError {
            target_block_hash: target_block.block_hash.clone(),
            target_block_height: target_block.body.state.block_number,
            retention_floor: floor,
        })
    }

    async fn find_deploy_by_recent_blocks(
        casper: &dyn MultiParentCasper,
        dag: &KeyValueDagRepresentation,
        deploy_id: &DeployId,
    ) -> ApiErr<Option<LightBlockInfo>> {
        let scan_depth = Self::find_deploy_scan_depth();
        if scan_depth == 0 {
            return Ok(None);
        }

        let max_block_number = dag.get_max_height();
        if max_block_number <= 0 {
            return Ok(None);
        }

        let end_height = max_block_number;
        let scan_depth_i64 = i64::try_from(scan_depth)
            .map_err(|_| eyre::eyre!("find-deploy scan depth is out of range"))?;
        let start_height = (end_height - (scan_depth_i64 - 1)).max(0);

        let mut candidate_blocks = match dag.topo_sort(start_height, Some(end_height)) {
            Ok(blocks_by_height) => blocks_by_height,
            Err(err) => {
                tracing::warn!(
                    "Could not run fallback deploy scan in height range {}..={}: {}",
                    start_height,
                    end_height,
                    err
                );
                return Ok(None);
            }
        };

        let mut deploy_sigs = HashSet::with_capacity(1);
        deploy_sigs.insert(deploy_id.to_vec());

        while let Some(blocks_on_height) = candidate_blocks.pop() {
            for hash in blocks_on_height {
                match casper
                    .block_store()
                    .has_any_deploy_sig(&hash, &deploy_sigs)
                    .map_err(|e| eyre::eyre!(e.to_string()))
                {
                    Ok(true) => {
                        let block = casper.block_store().get_unsafe(&hash);
                        let light_block_info =
                            BlockAPI::get_light_block_info(casper, &block).await?;
                        tracing::debug!(
                            "Deploy {:?} found via fallback scan in block {}",
                            PrettyPrinter::build_string_no_limit(deploy_id),
                            PrettyPrinter::build_string_bytes(&hash)
                        );
                        return Ok(Some(light_block_info));
                    }
                    Ok(false) => {}
                    // A block the scan window names but this node does not
                    // hold (an LFS restore horizon) is skipped, not an error.
                    Err(err) => {
                        tracing::debug!(
                            "fallback deploy scan skipping unreadable block {}: {}",
                            PrettyPrinter::build_string_bytes(&hash),
                            err
                        );
                    }
                }
            }
        }

        Ok(None)
    }

    #[tracing::instrument(name = "deploy", target = "f1r3fly.block-api.deploy", skip_all)]
    pub async fn deploy(
        engine_cell: &EngineCell,
        d: Signed<DeployData>,
        trigger_propose: &Option<Arc<ProposeFunction>>,
        is_node_read_only: bool,
        shard_id: &str,
    ) -> ApiErr<String> {
        async fn casper_deploy(
            casper: Arc<dyn MultiParentCasper + Send + Sync>,
            deploy_data: Signed<DeployData>,
            trigger_propose: &Option<Arc<ProposeFunction>>,
        ) -> ApiErr<String> {
            let deploy_id = match casper.deploy(deploy_data)? {
                Either::Left(err) => return Err(err.into()),
                Either::Right(deploy_id) => deploy_id,
            };

            // Trigger propose asynchronously for deploy path to keep do_deploy latency bounded.
            // Deploy success should not block on proposal completion; finalization is checked via
            // propose/finalization APIs separately in integration flows.
            if let Some(tp) = trigger_propose {
                let tp = Arc::clone(tp);
                let casper_for_propose = casper.clone();
                let max_attempts = deploy_propose_max_attempts();
                let retry_delay = deploy_propose_retry_delay();
                tokio::spawn(async move {
                    let mut attempt = 1u32;
                    loop {
                        match tp(casper_for_propose.clone(), true).await {
                            Ok(proposer_result) => match proposer_result {
                                ProposerResult::Failure(status, seq_number) => {
                                    if should_retry_deploy_propose(&status)
                                        && attempt < max_attempts
                                    {
                                        tracing::info!(
                                            "Deploy-triggered propose transient failure (attempt {}/{}, seqNum {}): {}; retrying in {:?}",
                                            attempt,
                                            max_attempts,
                                            seq_number,
                                            status,
                                            retry_delay
                                        );
                                        attempt += 1;
                                        tokio::time::sleep(retry_delay).await;
                                        continue;
                                    }

                                    if let Some(msg) = recoverable_propose_failure_message(&status)
                                    {
                                        tracing::info!("{} (seqNum {})", msg, seq_number);
                                    } else {
                                        tracing::error!(
                                            "Failure: {} (seqNum {})",
                                            status,
                                            seq_number
                                        );
                                    }
                                }
                                ProposerResult::Empty => {
                                    tracing::debug!("Propose already in progress");
                                }
                                ProposerResult::Started(seq_number) => {
                                    tracing::debug!("Propose started (seqNum {})", seq_number);
                                }
                                ProposerResult::Success(_, block) => {
                                    let block_hash_hex =
                                        PrettyPrinter::build_string_no_limit(&block.block_hash);
                                    tracing::info!(
                                        "Success! Block {} created and added.",
                                        block_hash_hex
                                    );
                                }
                            },
                            Err(err) => {
                                if attempt < max_attempts {
                                    tracing::warn!(
                                        "Deploy-triggered propose call failed (attempt {}/{}): {}; retrying in {:?}",
                                        attempt,
                                        max_attempts,
                                        err,
                                        retry_delay
                                    );
                                    attempt += 1;
                                    tokio::time::sleep(retry_delay).await;
                                    continue;
                                }
                                tracing::error!(error = %err, "deploy-triggered propose failed");
                            }
                        }
                        break;
                    }
                });
            }

            Ok(format!(
                "Success!\nDeployId is: {}",
                PrettyPrinter::build_string_no_limit(deploy_id.as_ref())
            ))
        }

        // Validation chain - mimics Scala's whenA pattern
        let validation_result: Result<(), DeployValidationError> = d
            .data
            .validate_amounts()
            .map_err(|message| DeployValidationError { message })
            .and_then(|_| {
                if is_node_read_only {
                    Err(DeployValidationError {
                        message: "Deploy was rejected because node is running in read-only mode."
                            .to_string(),
                    })
                } else {
                    Ok(())
                }
            })
            .and_then(|_| {
                if d.data.shard_id != shard_id {
                    Err(DeployValidationError {
                        message: format!(
                            "Deploy shardId '{}' is not as expected network shard '{}'.",
                            d.data.shard_id, shard_id
                        ),
                    })
                } else {
                    Ok(())
                }
            })
            .and_then(|_| {
                let is_forbidden_key = standard_deploys::system_public_keys()
                    .iter()
                    .any(|pk| **pk == d.pk);
                if is_forbidden_key {
                    Err(DeployValidationError {
                        message: "Deploy refused because it's signed with forbidden private key."
                            .to_string(),
                    })
                } else {
                    Ok(())
                }
            });

        // Return early if validation fails
        validation_result.map_err(|e| eyre::Report::new(e))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| eyre::eyre!("system clock is before the Unix epoch: {}", e))?
            .as_millis() as i64;
        if d.data.is_expired_at(now) {
            return Err(eyre::Report::new(DeployExpiredError {
                message: format!(
                    "deploy_expired: expirationTimestamp={:?} is in the past.",
                    d.data.expiration_timestamp
                ),
            }));
        }

        let log_error_message =
            "Error: Could not deploy, casper instance was not available yet.".to_string();

        let eng = engine_cell.get().await;

        // Helper function for logging - mimic Scala logWarn
        let log_warn = |msg: &str| -> ApiErr<String> {
            tracing::warn!("{}", msg);
            Err(eyre::eyre!("{}", msg))
        };

        if let Some(casper) = eng.with_casper() {
            // The floor check reads the SAME value the validity rule reads
            // (the shard conf), never a second node-conf copy.
            let min_phlo_price = casper.casper_shard_conf().min_phlo_price.clone();
            if d.data.phlo_price < min_phlo_price {
                return Err(eyre::Report::new(DeployValidationError {
                    message: format!(
                        "Phlo price {} is less than minimum price {}.",
                        d.data.phlo_price, min_phlo_price
                    ),
                }));
            }
            let dag = casper.block_dag().await?;
            if !casper.is_local_validator_bonded() {
                return Err(eyre::Report::new(ValidatorNotBondedError));
            }
            // `latest_block_number` is the highest stored block number plus
            // one, which is the number of the next block that block_creator.rs
            // assembles and checks with its own expiry filter.
            let next_block_number = dag.latest_block_number();
            let deploy_lifespan = casper.casper_shard_conf().deploy_lifespan;
            if deploy_is_block_expired(
                d.data.valid_after_block_number,
                next_block_number,
                deploy_lifespan,
            )? {
                return Err(eyre::Report::new(DeployExpiredError {
                    message: format!(
                        "deploy_expired: validAfterBlockNumber {} has expired at block {} with deploy lifespan {}.",
                        d.data.valid_after_block_number, next_block_number, deploy_lifespan
                    ),
                }));
            }
            if models::rust::deploy_window::is_too_far_ahead(
                d.data.valid_after_block_number,
                next_block_number,
                casper.casper_shard_conf().max_parent_depth,
            ) {
                return Err(eyre::Report::new(
                    crate::rust::casper::DeployError::InvalidFreshness,
                ));
            }
            casper_deploy(casper, d, trigger_propose).await
        } else {
            log_warn(&log_error_message)
        }
    }

    #[tracing::instrument(level = "info", skip(engine_cell, trigger_propose_f))]
    pub async fn create_block(
        engine_cell: &EngineCell,
        trigger_propose_f: &Arc<ProposeFunction>,
        is_async: bool,
    ) -> ApiErr<String> {
        let log_debug = |err: &str| -> ApiErr<String> {
            tracing::debug!("{}", err);
            Err(eyre::eyre!("{}", err))
        };
        let log_success = |msg: &str| -> ApiErr<String> {
            tracing::info!("{}", msg);
            Ok(msg.to_string())
        };
        let log_warn = |msg: &str| -> ApiErr<String> {
            tracing::warn!("{}", msg);
            Err(eyre::eyre!("{}", msg))
        };

        let eng = engine_cell.get().await;

        if let Some(casper) = eng.with_casper() {
            // Trigger propose
            let proposer_result = match trigger_propose_f(casper, is_async).await {
                Ok(proposer_result) => proposer_result,
                Err(err) => {
                    let err_message = err.to_string();
                    return log_debug(&err_message);
                }
            };

            let r: ApiErr<String> = match proposer_result {
                ProposerResult::Empty => log_debug("Failure: another propose is in progress"),
                ProposerResult::Failure(ref status, seq_number) => {
                    Err(propose_failure_error(status, seq_number))
                }
                ProposerResult::Started(seq_number) => {
                    log_success(&format!("Propose started (seqNum {})", seq_number))
                }
                ProposerResult::Success(_, block) => {
                    // TODO: Scala [WARNING] Format of this message is hardcoded in pyrchain when checking response result
                    //  Fix to use structured result with transport errors/codes.
                    // https://github.com/rchain/pyrchain/blob/a2959c75bf/rchain/client.py#L42
                    let block_hash_hex = PrettyPrinter::build_string_no_limit(&block.block_hash);
                    log_success(&format!(
                        "Success! Block {} created and added.",
                        block_hash_hex
                    ))
                }
            };

            // yield r
            r
        } else {
            log_warn("Failure: casper instance is not available.")
        }
    }

    pub async fn get_propose_result(proposer_state: &mut ProposerState) -> ApiErr<String> {
        let r = match proposer_state.curr_propose_result.take() {
            // return latest propose result
            None => {
                let default_result = (ProposeResult::not_enough_blocks(), None);
                let result = proposer_state
                    .latest_propose_result
                    .as_ref()
                    .unwrap_or(&default_result);
                let msg = match &result.1 {
                    Some(block) => {
                        let block_hash_hex =
                            PrettyPrinter::build_string_no_limit(&block.block_hash);
                        Ok(format!(
                            "Success! Block {} created and added.",
                            block_hash_hex
                        ))
                    }
                    None => {
                        if let Some(msg) =
                            recoverable_propose_failure_message(&result.0.propose_status)
                        {
                            Ok(msg)
                        } else {
                            Err(eyre::eyre!("{}", result.0.propose_status))
                        }
                    }
                };
                msg
            }
            // wait for current propose to finish and return result
            Some(result_def) => {
                // this will hang API call until propose is complete, and then return result
                // TODO Scala: cancel this get when connection drops
                let result = result_def.await?;
                let msg = match &result.1 {
                    Some(block) => {
                        let block_hash_hex =
                            PrettyPrinter::build_string_no_limit(&block.block_hash);
                        Ok(format!(
                            "Success! Block {} created and added.",
                            block_hash_hex
                        ))
                    }
                    None => {
                        if let Some(msg) =
                            recoverable_propose_failure_message(&result.0.propose_status)
                        {
                            Ok(msg)
                        } else {
                            Err(eyre::eyre!("{}", result.0.propose_status))
                        }
                    }
                };
                msg
            }
        };
        r
    }

    pub async fn get_listening_name_data_response(
        engine_cell: &EngineCell,
        depth: i32,
        listening_name: Par,
        max_blocks_limit: i32,
    ) -> ApiErr<(Vec<DataWithBlockInfo>, i32)> {
        let error_message =
            "Could not get listening name data, casper instance was not available yet.";

        async fn casper_response(
            casper: &dyn MultiParentCasper,
            depth: i32,
            listening_name: Par,
        ) -> ApiErr<(Vec<DataWithBlockInfo>, i32)> {
            let main_chain = BlockAPI::get_main_chain_from_tip(casper, depth).await?;
            let runtime_manager = casper.runtime_manager();
            let sorted_listening_name = ParSortMatcher::sort_match(&listening_name).term;

            let maybe_blocks_with_active_name: Vec<Option<DataWithBlockInfo>> =
                future::try_join_all(main_chain.iter().map(|block| {
                    BlockAPI::get_data_with_block_info(
                        casper,
                        runtime_manager.clone(),
                        &sorted_listening_name,
                        block,
                        false,
                    )
                }))
                .await?;

            let blocks_with_active_name: Vec<DataWithBlockInfo> = maybe_blocks_with_active_name
                .into_iter()
                .flatten()
                .collect();

            Ok((
                blocks_with_active_name.clone(),
                blocks_with_active_name.len() as i32,
            ))
        }

        let effective_depth = clamp_depth(depth, max_blocks_limit, "get-listening-name-data");
        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            casper_response(casper.as_ref(), effective_depth, listening_name).await
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    pub async fn get_listening_name_continuation_response(
        engine_cell: &EngineCell,
        depth: i32,
        listening_names: &[Par],
        max_blocks_limit: i32,
    ) -> ApiErr<(Vec<ContinuationsWithBlockInfo>, i32)> {
        let error_message =
            "Could not get listening names continuation, casper instance was not available yet.";

        async fn casper_response(
            casper: &dyn MultiParentCasper,
            depth: i32,
            listening_names: &[Par],
        ) -> ApiErr<(Vec<ContinuationsWithBlockInfo>, i32)> {
            let main_chain = BlockAPI::get_main_chain_from_tip(casper, depth).await?;
            let runtime_manager = casper.runtime_manager();

            let sorted_listening_names: Vec<Par> = listening_names
                .iter()
                .map(|name| ParSortMatcher::sort_match(name).term)
                .collect();

            let maybe_blocks_with_active_name: Vec<Option<ContinuationsWithBlockInfo>> =
                future::try_join_all(main_chain.iter().map(|block| {
                    BlockAPI::get_continuations_with_block_info(
                        casper,
                        runtime_manager.clone(),
                        &sorted_listening_names,
                        block,
                    )
                }))
                .await?;

            let blocks_with_active_name: Vec<ContinuationsWithBlockInfo> =
                maybe_blocks_with_active_name
                    .into_iter()
                    .flatten()
                    .collect();

            Ok((
                blocks_with_active_name.clone(),
                blocks_with_active_name.len() as i32,
            ))
        }

        let effective_depth =
            clamp_depth(depth, max_blocks_limit, "get-listening-name-continuation");
        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            casper_response(casper.as_ref(), effective_depth, listening_names).await
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    async fn get_main_chain_from_tip<M: MultiParentCasper + ?Sized>(
        casper: &M,
        depth: i32,
    ) -> ApiErr<Vec<BlockMessage>> {
        let mut dag = casper.block_dag().await?;
        let tip_hashes = casper.estimator(&mut dag).await?;

        // With multi-parent merging, estimator returns all validators' latest blocks.
        // Find the tip with the highest block number to use as the main chain head.
        let tips: Vec<BlockMessage> = tip_hashes
            .iter()
            .filter_map(|h| casper.block_store().get(h).ok().flatten())
            .collect();

        let tip = tips
            .into_iter()
            .max_by_key(|b| b.body.state.block_number)
            .ok_or_else(|| eyre::eyre!("No tip"))?;

        let main_chain =
            proto_util::get_main_chain_until_depth(casper.block_store(), tip, Vec::new(), depth)?;
        Ok(main_chain)
    }

    async fn get_data_with_block_info(
        casper: &dyn MultiParentCasper,
        runtime_manager: Arc<RuntimeManager>,
        sorted_listening_name: &Par,
        block: &BlockMessage,
        use_pre_state_hash: bool,
    ) -> ApiErr<Option<DataWithBlockInfo>> {
        // TODO: Scala For Produce it doesn't make sense to have multiple names
        if BlockAPI::is_listening_name_reduced(block, &[sorted_listening_name.clone()]) {
            let state_hash = if use_pre_state_hash {
                proto_util::pre_state_hash(block)
            } else {
                proto_util::post_state_hash(block)
            };
            let data = runtime_manager
                .get_data(state_hash, sorted_listening_name)
                .await?;
            let block_info = BlockAPI::get_light_block_info(casper, block).await?;
            Ok(Some(DataWithBlockInfo {
                post_block_data: data,
                block: Some(block_info).into(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_continuations_with_block_info(
        casper: &dyn MultiParentCasper,
        runtime_manager: Arc<RuntimeManager>,
        sorted_listening_names: &[Par],
        block: &BlockMessage,
    ) -> ApiErr<Option<ContinuationsWithBlockInfo>> {
        if Self::is_listening_name_reduced(block, sorted_listening_names) {
            let state_hash = proto_util::post_state_hash(block);

            let continuations = runtime_manager
                .get_continuation(state_hash, sorted_listening_names.to_vec())
                .await?;

            let continuation_infos: Vec<_> = continuations
                .into_iter()
                .map(
                    |(post_block_patterns, post_block_continuation)| WaitingContinuationInfo {
                        post_block_patterns,
                        post_block_continuation: Some(post_block_continuation),
                    },
                )
                .collect();

            let block_info = BlockAPI::get_light_block_info(casper, block).await?;
            Ok(Some(ContinuationsWithBlockInfo {
                post_block_continuations: continuation_infos,
                block: Some(block_info).into(),
            }))
        } else {
            Ok(None)
        }
    }

    fn is_listening_name_reduced(block: &BlockMessage, sorted_listening_name: &[Par]) -> bool {
        let serialized_log: Vec<_> = block
            .body
            .deploys
            .iter()
            .flat_map(|pd| pd.deploy_log.iter())
            .collect();

        let log: Vec<RspaceEvent> = serialized_log
            .iter()
            .map(|event| event_converter::to_rspace_event(event))
            .collect();

        log.iter().any(|event| match event {
            RspaceEvent::IoEvent(IOEvent::Produce(produce)) => {
                // Produce can only have one channel, so skip if searching for multiple
                // Scala has the same assertion but it works there because exists() finds
                // matching Consume event before iterating to Produce events
                if sorted_listening_name.len() != 1 {
                    return false;
                }
                // channelHash == JNAInterfaceLoader.hashChannel(sortedListeningName.head)
                produce.channel_hash == stable_hash_provider::hash(&sorted_listening_name[0])
            }
            RspaceEvent::IoEvent(IOEvent::Consume(consume)) => {
                let mut expected_hashes: Vec<_> = sorted_listening_name
                    .iter()
                    .map(|name| stable_hash_provider::hash(name))
                    .collect();
                expected_hashes.sort();

                let mut actual_hashes = consume.channel_hashes.clone();
                actual_hashes.sort();

                actual_hashes == expected_hashes
            }

            RspaceEvent::Comm(comm) => {
                let mut expected_hashes: Vec<_> = sorted_listening_name
                    .iter()
                    .map(|name| stable_hash_provider::hash(name))
                    .collect();
                expected_hashes.sort();

                let mut consume_hashes = comm.consume.channel_hashes.clone();
                consume_hashes.sort();

                let consume_matches = consume_hashes == expected_hashes;

                let produce_matches = comm.produces.iter().any(|produce| {
                    produce.channel_hash
                        == stable_hash_provider::hash_from_vec(&sorted_listening_name.to_vec())
                });

                consume_matches || produce_matches
            }
        })
    }

    async fn toposort_dag<A: 'static + Send>(
        engine_cell: &EngineCell,
        depth: i32,
        max_depth_limit: i32,
        do_it: fn((&dyn MultiParentCasper, Vec<Vec<BlockHash>>)) -> ApiErr<A>,
    ) -> ApiErr<A> {
        let error_message =
            "Could not visualize graph, casper instance was not available yet.".to_string();

        async fn casper_response<A: 'static + Send>(
            casper: &dyn MultiParentCasper,
            depth: i32,
            do_it: fn((&dyn MultiParentCasper, Vec<Vec<BlockHash>>)) -> ApiErr<A>,
        ) -> ApiErr<A> {
            let dag = casper.block_dag().await?;

            let latest_block_number = dag.latest_block_number();

            let topo_sort = dag.topo_sort(latest_block_number - depth as i64, None)?;

            do_it((casper, topo_sort))
        }

        let effective_depth = clamp_depth(depth, max_depth_limit, "toposort-dag");

        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            casper_response(casper.as_ref(), effective_depth, do_it).await
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    pub async fn get_blocks_by_heights(
        engine_cell: &EngineCell,
        start_block_number: i64,
        end_block_number: i64,
        max_blocks_limit: i32,
    ) -> ApiErr<BlocksByHeightsResponse<LightBlockInfo>> {
        Self::get_blocks_by_heights_with_constructor(
            engine_cell,
            start_block_number,
            end_block_number,
            max_blocks_limit,
            Self::construct_light_block_info,
        )
        .await
    }

    pub async fn get_blocks_by_heights_full(
        engine_cell: &EngineCell,
        start_block_number: i64,
        end_block_number: i64,
        max_blocks_limit: i32,
    ) -> ApiErr<BlocksByHeightsResponse<BlockInfo>> {
        Self::get_blocks_by_heights_with_constructor(
            engine_cell,
            start_block_number,
            end_block_number,
            max_blocks_limit,
            Self::construct_block_info,
        )
        .await
    }

    async fn get_blocks_by_heights_with_constructor<A: Sized + Send>(
        engine_cell: &EngineCell,
        start_block_number: i64,
        end_block_number: i64,
        max_blocks_limit: i32,
        constructor: fn(&BlockMessage, f32, bool) -> A,
    ) -> ApiErr<BlocksByHeightsResponse<A>> {
        let error_message = format!(
            "Could not retrieve blocks from {} to {}",
            start_block_number, end_block_number
        );

        async fn casper_response<A: Sized + Send>(
            casper: &dyn MultiParentCasper,
            start_block_number: i64,
            end_block_number: i64,
            constructor: fn(&BlockMessage, f32, bool) -> A,
        ) -> ApiErr<BlocksByHeightsResponse<A>> {
            let dag = casper.block_dag().await?;

            let topo_sort_dag = dag.topo_sort(start_block_number, Some(end_block_number))?;

            let hashes: Vec<BlockHash> = topo_sort_dag.into_iter().flatten().collect();
            // Reuse PR #1113's single-snapshot metadata read across both
            // served-block construction and missing-body classification.
            // `lookup_many` reads every hash's metadata in one pass, so
            // the response cannot straddle a finalization — the same
            // consistency guarantee `held_block_infos` gives for the
            // held path extends to the pruned / unavailable split.
            let metadata = dag.lookup_many(&hashes)?;

            let mut held: Vec<BlockMessage> = Vec::new();
            let mut held_metadata: Vec<Option<BlockMetadata>> = Vec::new();
            let mut missing: Vec<MissingBlockEntry> = Vec::new();
            for (hash, meta) in hashes.iter().zip(metadata.iter()) {
                match casper.block_store().get(hash)? {
                    Some(block) => {
                        held.push(block);
                        held_metadata.push(meta.clone());
                    }
                    None => match meta {
                        Some(m) => missing.push(MissingBlockEntry {
                            hash: hash.clone(),
                            block_height: m.block_number,
                        }),
                        None => tracing::debug!(
                            "get-blocks-by-heights skipping hash {} with no DAG metadata",
                            PrettyPrinter::build_string_bytes(hash)
                        ),
                    },
                }
            }

            // Read the retention floor *after* the walk. GC raises the
            // anchor with fetch_max before Delete removes any body, so a
            // body we observed missing must have had its height already
            // passed by the anchor at the moment the delete happened.
            // A post-walk load therefore cannot miss a GC advance that
            // caused one of our observed misses, and a height > floor
            // entry is genuinely not yet reached by the sweep rather
            // than a race window artifact. See PR #1097 review.
            let prune_floor = casper
                .effective_anchor_handle()
                .load(std::sync::atomic::Ordering::Acquire);
            let (pruned, unavailable): (Vec<_>, Vec<_>) = missing
                .into_iter()
                .partition(|entry| entry.block_height <= prune_floor);

            for entry in &pruned {
                tracing::debug!(
                    "get-blocks-by-heights pruned block {} at height {} (prune_floor={})",
                    PrettyPrinter::build_string_bytes(&entry.hash),
                    entry.block_height,
                    prune_floor
                );
            }
            for entry in &unavailable {
                tracing::debug!(
                    "get-blocks-by-heights unavailable block {} at height {} (prune_floor={})",
                    PrettyPrinter::build_string_bytes(&entry.hash),
                    entry.block_height,
                    prune_floor
                );
            }

            let mut served = Vec::with_capacity(held.len());
            for (block, meta) in held.iter().zip(&held_metadata) {
                served.push(
                    BlockAPI::block_info_from_metadata(
                        casper,
                        &dag,
                        block,
                        meta.as_ref(),
                        constructor,
                    )
                    .await?,
                );
            }

            Ok(BlocksByHeightsResponse {
                served,
                pruned,
                unavailable,
                prune_floor,
            })
        }

        let effective_end_block_number =
            clamp_end_block_number(start_block_number, end_block_number, max_blocks_limit);

        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            casper_response(
                casper.as_ref(),
                start_block_number,
                effective_end_block_number,
                constructor,
            )
            .await
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    pub async fn visualize_dag<R: 'static, V, VFut>(
        engine_cell: &EngineCell,
        depth: i32,
        max_depth_limit: i32,
        start_block_number: i32,
        visualizer: V,
        serialize: tokio::sync::oneshot::Receiver<R>,
    ) -> ApiErr<R>
    where
        V: FnOnce(Vec<Vec<Bytes>>, String) -> VFut,
        VFut: Future<Output = eyre::Result<()>>,
    {
        let error_message = "visual dag failed".to_string();

        async fn casper_response<R: 'static, V, VFut>(
            casper: &dyn MultiParentCasper,
            depth: i32,
            start_block_number: i32,
            visualizer: V,
            serialize: tokio::sync::oneshot::Receiver<R>,
        ) -> ApiErr<R>
        where
            V: FnOnce(Vec<Vec<Bytes>>, String) -> VFut,
            VFut: Future<Output = eyre::Result<()>>,
        {
            let dag = casper.block_dag().await?;

            let start_block_num = if start_block_number == 0 {
                dag.latest_block_number()
            } else {
                start_block_number as i64
            };

            let topo_sort_dag =
                dag.topo_sort(start_block_num - depth as i64, Some(start_block_num))?;

            let lfb_hash = dag.last_finalized_block();

            visualizer(topo_sort_dag, PrettyPrinter::build_string_bytes(&lfb_hash)).await?;

            // result <- serialize
            let result = serialize.await?;

            Ok(result)
        }

        let effective_depth = clamp_depth(depth, max_depth_limit, "visualize-dag");

        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            casper_response(
                casper.as_ref(),
                effective_depth,
                start_block_number,
                visualizer,
                serialize,
            )
            .await
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    pub async fn machine_verifiable_dag(
        engine_cell: &EngineCell,
        depth: i32,
        max_depth_limit: i32,
    ) -> ApiErr<String> {
        let do_it = |(_casper, topo_sort): (&dyn MultiParentCasper, Vec<Vec<BlockHash>>)| -> ApiErr<String> {
            // case (_, topoSort) => ...
            // An unheld block (an LFS restore horizon) renders without edges.
            let fetch_parents = |block_hash: &BlockHash| -> Vec<BlockHash> {
                match _casper.block_store().get(block_hash) {
                    Ok(Some(block)) => block.header.parents_hash_list.clone(),
                    _ => Vec::new(),
                }
            };

            //string will be converted to an ApiErr<String>
            let result = topo_sort
                .into_iter()
                .flat_map(|block_hashes| {
                    block_hashes.into_iter().flat_map(|block_hash| {
                        let block_hash_str = PrettyPrinter::build_string_bytes(&block_hash);
                        fetch_parents(&block_hash).into_iter().map(move |parent_hash| {
                            format!("{} {}", block_hash_str, PrettyPrinter::build_string_bytes(&parent_hash))
                        })
                    })
                })
                .collect::<Vec<String>>()
                .join("\n");
            Ok(result)
        };

        BlockAPI::toposort_dag(engine_cell, depth, max_depth_limit, do_it).await
    }

    pub async fn get_blocks(
        engine_cell: &EngineCell,
        depth: i32,
        max_depth_limit: i32,
    ) -> ApiErr<Vec<LightBlockInfo>> {
        let effective_depth = clamp_depth(depth, max_depth_limit, "get-blocks");
        let error_message =
            "Could not get blocks, casper instance was not available yet.".to_string();

        let eng = engine_cell.get().await;
        let Some(casper) = eng.with_casper() else {
            return Err(eyre::eyre!("Error: {}", error_message));
        };

        let dag = casper.block_dag().await?;
        let latest_block_number = dag.latest_block_number();
        let topo_sort = dag.topo_sort(latest_block_number - effective_depth as i64, None)?;

        let hashes: Vec<BlockHash> = topo_sort.into_iter().flatten().collect();
        let mut block_infos = BlockAPI::held_block_infos(
            casper.as_ref(),
            &dag,
            &hashes,
            Self::construct_light_block_info,
            "get-blocks",
        )
        .await?;
        block_infos.reverse();
        Ok(block_infos)
    }

    /// Like `get_blocks` but returns full `BlockInfo` (with deploys).
    pub async fn get_blocks_full(
        engine_cell: &EngineCell,
        depth: i32,
        max_depth_limit: i32,
    ) -> ApiErr<Vec<BlockInfo>> {
        let effective_depth = clamp_depth(depth, max_depth_limit, "get-blocks-full");
        let error_message =
            "Could not get blocks, casper instance was not available yet.".to_string();

        let eng = engine_cell.get().await;
        let Some(casper) = eng.with_casper() else {
            return Err(eyre::eyre!("Error: {}", error_message));
        };

        let dag = casper.block_dag().await?;
        let latest_block_number = dag.latest_block_number();
        let topo_sort = dag.topo_sort(latest_block_number - effective_depth as i64, None)?;

        let hashes: Vec<BlockHash> = topo_sort.into_iter().flatten().collect();
        let mut block_infos = BlockAPI::held_block_infos(
            casper.as_ref(),
            &dag,
            &hashes,
            Self::construct_block_info,
            "get-blocks-full",
        )
        .await?;
        block_infos.reverse();
        Ok(block_infos)
    }

    pub async fn show_main_chain(
        engine_cell: &EngineCell,
        depth: i32,
        max_depth_limit: i32,
    ) -> Vec<LightBlockInfo> {
        let error_message =
            "Could not show main chain, casper instance was not available yet.".to_string();

        async fn casper_response(
            casper: &dyn MultiParentCasper,
            depth: i32,
        ) -> ApiErr<Vec<LightBlockInfo>> {
            let dag = casper.block_dag().await?;

            let mut dag_mut = dag;
            let tip_hashes = casper.estimator(&mut dag_mut).await?;

            let tip_hash = tip_hashes
                .first()
                .cloned()
                .ok_or_else(|| eyre::eyre!("No tip hashes found"))?;

            let tip = casper.block_store().get_unsafe(&tip_hash);

            let main_chain = proto_util::get_main_chain_until_depth(
                casper.block_store(),
                tip,
                Vec::new(),
                depth,
            )?;

            BlockAPI::block_infos_with_dag(
                casper,
                &dag_mut,
                &main_chain,
                BlockAPI::construct_light_block_info,
            )
            .await
        }

        let effective_depth = clamp_depth(depth, max_depth_limit, "show-main-chain");

        let eng = engine_cell.get().await;

        if let Some(casper) = eng.with_casper() {
            casper_response(casper.as_ref(), effective_depth)
                .await
                .unwrap_or_else(|_| Vec::new())
        } else {
            tracing::warn!("{}", error_message);
            Vec::new()
        }
    }

    pub async fn find_deploy(
        engine_cell: &EngineCell,
        deploy_id: &DeployId,
    ) -> ApiErr<LightBlockInfo> {
        let error_message =
            "Could not find block with deploy, casper instance was not available yet.".to_string();

        let eng = engine_cell.get().await;

        if let Some(casper) = eng.with_casper() {
            let dag = casper.block_dag().await?;
            // The sig's most recent canonical appearance, from the
            // lifecycle rows — a pure function of the DAG's bodies, never
            // of node-local insertion order.
            let maybe_block_hash = dag.deploy_canonical_appearance(deploy_id)?;

            // The canonical carrier can sit below an LFS restore horizon;
            // fall through to the recent-blocks scan instead of erroring.
            match maybe_block_hash
                .and_then(|block_hash| casper.block_store().get(&block_hash).ok().flatten())
            {
                Some(block) => {
                    let light_block_info =
                        BlockAPI::get_light_block_info(casper.as_ref(), &block).await?;
                    Ok(light_block_info)
                }
                None => {
                    if let Some(fallback_block_info) =
                        Self::find_deploy_by_recent_blocks(casper.as_ref(), &dag, deploy_id).await?
                    {
                        Ok(fallback_block_info)
                    } else {
                        Err(DeployNotFoundError {
                            deploy_id: PrettyPrinter::build_string_no_limit(deploy_id),
                        }
                        .into())
                    }
                }
            }
        } else {
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    #[tracing::instrument(name = "get-block", target = "f1r3fly.block-api.get-block", skip_all)]
    pub async fn get_block(engine_cell: &EngineCell, hash: &str) -> ApiErr<BlockInfo> {
        let error_message =
            "Could not get block, casper instance was not available yet.".to_string();

        async fn casper_response(casper: &dyn MultiParentCasper, hash: &str) -> ApiErr<BlockInfo> {
            if hash.len() < 6 {
                return Err(eyre::Report::new(InvalidHashError(format!(
                    "'{}' is not a valid block hash (minimum 6 hex characters)",
                    hash
                ))));
            }

            let padded_hash = pad_hex_string(hash);

            let hash_byte_string = hex::decode(&padded_hash).map_err(|_| {
                eyre::Report::new(InvalidHashError(format!(
                    "'{}' is not valid block hash",
                    hash
                )))
            })?;

            // Resolve to the full 32-byte block hash up front, then use
            // that same value for both the body lookup and (on miss) the
            // DAG metadata classification. Reusing the resolved hash
            // keeps a prefix query like `/block/aabbcc` and the matching
            // full hash on the same classification path — without it a
            // 6-char prefix returned 404 for a pruned block because the
            // metadata index is keyed by full hash. See PR #1097 review.
            let resolved_hash: Option<BlockHash> = if hash.len() == 64 {
                Some(prost::bytes::Bytes::from(hash_byte_string))
            } else {
                let dag = casper
                    .block_dag()
                    .await
                    .map_err(|e| eyre::eyre!(e.to_string()))?;
                dag.find(hash).map_err(|e| eyre::eyre!(e.to_string()))?
            };

            let block_opt = match resolved_hash.as_ref() {
                Some(bytes) => casper
                    .block_store()
                    .get(bytes)
                    .map_err(|e| eyre::eyre!(e.to_string()))?,
                None => None,
            };

            let block = match block_opt {
                Some(block) => block,
                None => {
                    // Body is missing. Split "never seen"
                    // (BlockNotFoundError) from the "DAG metadata present
                    // but body absent" cases, which the retention sweep's
                    // floor further splits into pruned-permanent (height
                    // <= floor) vs transiently-unavailable (height >
                    // floor). The DAG metadata index is not swept by
                    // #1010 / #1045, so metadata presence tells us we
                    // knew the block once.
                    let dag = casper.block_dag().await?;
                    let prune_floor = casper
                        .effective_anchor_handle()
                        .load(std::sync::atomic::Ordering::Acquire);
                    return Err(match resolved_hash {
                        Some(resolved) => {
                            let full_hash = hex::encode(&resolved);
                            match dag.lookup(&resolved)? {
                                Some(metadata) if metadata.block_number <= prune_floor => {
                                    eyre::Report::new(BlockPrunedError {
                                        hash: full_hash,
                                        block_height: metadata.block_number,
                                        prune_floor,
                                    })
                                }
                                Some(metadata) => eyre::Report::new(BlockUnavailableError {
                                    hash: full_hash,
                                    block_height: metadata.block_number,
                                    prune_floor,
                                }),
                                None => eyre::Report::new(BlockNotFoundError {
                                    hash: hash.to_string(),
                                }),
                            }
                        }
                        None => eyre::Report::new(BlockNotFoundError {
                            hash: hash.to_string(),
                        }),
                    });
                }
            };

            let dag = casper.block_dag().await?;
            if dag.contains(&block.block_hash)? {
                let block_info = BlockAPI::get_full_block_info(casper, &block).await?;
                Ok(block_info)
            } else {
                Err(eyre::eyre!(
                    "Error: Block with hash {} received but not added yet",
                    hash
                ))
            }
        }

        let eng = engine_cell.get().await;

        if let Some(casper) = eng.with_casper() {
            casper_response(casper.as_ref(), hash).await
        } else {
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    async fn get_block_info_with_dag<M: MultiParentCasper + ?Sized, A: Sized + Send>(
        casper: &M,
        dag: &KeyValueDagRepresentation,
        block: &BlockMessage,
        constructor: fn(&BlockMessage, f32, bool) -> A,
    ) -> ApiErr<A> {
        let metadata = dag.lookup(&block.block_hash)?;
        Self::block_info_from_metadata(casper, dag, block, metadata.as_ref(), constructor).await
    }

    /// Infos for the held blocks among `hashes`, in order, with every block's
    /// metadata read in one `lookup_many`: a response never mixes state from
    /// before and after a finalization. A height window can name blocks this
    /// node does not hold (an LFS restore horizon); those are skipped.
    async fn held_block_infos<M: MultiParentCasper + ?Sized, A: Sized + Send>(
        casper: &M,
        dag: &KeyValueDagRepresentation,
        hashes: &[BlockHash],
        constructor: fn(&BlockMessage, f32, bool) -> A,
        operation: &str,
    ) -> ApiErr<Vec<A>> {
        let mut held = Vec::with_capacity(hashes.len());
        for block_hash in hashes {
            match casper.block_store().get(block_hash)? {
                Some(block) => held.push(block),
                None => tracing::debug!(
                    "{operation} skipping unheld block {}",
                    PrettyPrinter::build_string_bytes(block_hash)
                ),
            }
        }
        Self::block_infos_with_dag(casper, dag, &held, constructor).await
    }

    /// Infos for `blocks`, in order, with every block's metadata read in one
    /// `lookup_many`.
    async fn block_infos_with_dag<M: MultiParentCasper + ?Sized, A: Sized + Send>(
        casper: &M,
        dag: &KeyValueDagRepresentation,
        blocks: &[BlockMessage],
        constructor: fn(&BlockMessage, f32, bool) -> A,
    ) -> ApiErr<Vec<A>> {
        let hashes: Vec<BlockHash> = blocks.iter().map(|b| b.block_hash.clone()).collect();
        let metadata = dag.lookup_many(&hashes)?;
        let mut block_infos = Vec::with_capacity(blocks.len());
        for (block, metadata) in blocks.iter().zip(&metadata) {
            block_infos.push(
                Self::block_info_from_metadata(casper, dag, block, metadata.as_ref(), constructor)
                    .await?,
            );
        }
        Ok(block_infos)
    }

    async fn block_info_from_metadata<M: MultiParentCasper + ?Sized, A: Sized + Send>(
        casper: &M,
        dag: &KeyValueDagRepresentation,
        block: &BlockMessage,
        metadata: Option<&BlockMetadata>,
        constructor: fn(&BlockMessage, f32, bool) -> A,
    ) -> ApiErr<A> {
        let is_finalized = match metadata {
            Some(meta) => meta.finalized,
            None => dag.is_finalized(&block.block_hash)?,
        };

        let normalized_fault_tolerance = match metadata {
            Some(meta) if meta.finalized => meta.fault_tolerance_value,
            _ => Self::fault_tolerance_or_unknown(dag, &block.block_hash).await?,
        };

        let weights_map = proto_util::weight_map(block);
        let initial_fault = casper.normalized_initial_fault(weights_map)?;
        let fault_tolerance = crate::rust::safety::initial_fault::display_projection(
            normalized_fault_tolerance,
            initial_fault,
        );

        let block_info = constructor(block, fault_tolerance, is_finalized);
        Ok(block_info)
    }

    /// An oracle walk that reaches history this node does not hold (an LFS
    /// restore horizon) reads as unknown fault tolerance, never as an API
    /// failure.
    async fn fault_tolerance_or_unknown(
        dag: &KeyValueDagRepresentation,
        block_hash: &BlockHash,
    ) -> ApiErr<f32> {
        let safety_oracle = CliqueOracleImpl;
        match safety_oracle
            .normalized_fault_tolerance(dag, block_hash)
            .await
        {
            Ok(ft) => Ok(ft),
            Err(KvStoreError::MissingBlock { hash, .. }) => {
                tracing::debug!(
                    "fault tolerance unknown for {}: history below the restore \
                     horizon ({})",
                    PrettyPrinter::build_string_bytes(block_hash),
                    PrettyPrinter::build_string_bytes(&hash)
                );
                Ok(MIN_FAULT_TOLERANCE)
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn get_block_info<M: MultiParentCasper + ?Sized, A: Sized + Send>(
        casper: &M,
        block: &BlockMessage,
        constructor: fn(&BlockMessage, f32, bool) -> A,
    ) -> ApiErr<A> {
        let dag = casper.block_dag().await?;
        Self::get_block_info_with_dag(casper, &dag, block, constructor).await
    }

    async fn get_full_block_info<M: MultiParentCasper + ?Sized>(
        casper: &M,
        block: &BlockMessage,
    ) -> ApiErr<BlockInfo> {
        Self::get_block_info(casper, block, Self::construct_block_info).await
    }

    pub async fn get_light_block_info(
        casper: &dyn MultiParentCasper,
        block: &BlockMessage,
    ) -> ApiErr<LightBlockInfo> {
        Self::get_block_info(casper, block, Self::construct_light_block_info).await
    }

    fn construct_block_info(
        block: &BlockMessage,
        fault_tolerance: f32,
        is_finalized: bool,
    ) -> BlockInfo {
        let light_block_info =
            Self::construct_light_block_info(block, fault_tolerance, is_finalized);
        let deploys = block
            .body
            .deploys
            .iter()
            .map(|processed_deploy| processed_deploy.clone().to_deploy_info())
            .collect();

        BlockInfo {
            block_info: Some(light_block_info).into(),
            deploys,
        }
    }

    fn construct_light_block_info(
        block: &BlockMessage,
        fault_tolerance: f32,
        is_finalized: bool,
    ) -> LightBlockInfo {
        LightBlockInfo {
            block_hash: PrettyPrinter::build_string_no_limit(&block.block_hash),
            sender: PrettyPrinter::build_string_no_limit(&block.sender),
            seq_num: block.seq_num as i64,
            sig: PrettyPrinter::build_string_no_limit(&block.sig),
            sig_algorithm: block.sig_algorithm.clone(),
            shard_id: block.shard_id.clone(),
            extra_bytes: block.extra_bytes.clone(),
            version: block.header.version,
            timestamp: block.header.timestamp,
            header_extra_bytes: block.header.extra_bytes.clone(),
            parents_hash_list: block
                .header
                .parents_hash_list
                .iter()
                .map(|h| PrettyPrinter::build_string_no_limit(h))
                .collect(),
            block_number: block.body.state.block_number,
            pre_state_hash: PrettyPrinter::build_string_no_limit(&block.body.state.pre_state_hash),
            post_state_hash: PrettyPrinter::build_string_no_limit(
                &block.body.state.post_state_hash,
            ),
            body_extra_bytes: block.body.extra_bytes.clone(),
            bonds: block
                .body
                .state
                .bonds
                .iter()
                .map(proto_util::bond_to_bond_info)
                .collect(),
            block_size: block.to_proto().encode_to_vec().len().to_string(),
            deploy_count: block.body.deploys.len() as i32,
            fault_tolerance,
            justifications: block
                .justifications
                .iter()
                .map(proto_util::justification_to_justification_info)
                .collect(),
            rejected_deploys: block
                .body
                .rejected_deploys
                .iter()
                .map(|r| RejectedDeployInfo {
                    sig: PrettyPrinter::build_string_no_limit(&r.sig),
                })
                .collect(),
            is_finalized,
        }
    }

    pub fn preview_private_names(
        deployer: &ByteString,
        timestamp: i64,
        valid_after_block_number: i64,
        name_qty: i32,
    ) -> ApiErr<Vec<ByteString>> {
        let mut rand = Tools::unforgeable_name_rng(
            &PublicKey::from_bytes(deployer),
            timestamp,
            valid_after_block_number,
        );
        let safe_qty = name_qty.clamp(0, 1024) as usize;
        let ids: Vec<BlockHash> = (0..safe_qty)
            .map(|_| rand.next().into_iter().map(|b| b as u8).collect())
            .collect();
        Ok(ids.into_iter().map(|bytes| bytes.to_vec()).collect())
    }

    pub async fn last_finalized_block(engine_cell: &EngineCell) -> ApiErr<BlockInfo> {
        let error_message =
            "Could not get last finalized block, casper instance was not available yet.";
        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            let dag = casper.block_dag().await?;
            let lfb_hash = dag.last_finalized_block();
            let last_finalized_block = casper.block_store().get(&lfb_hash)?.ok_or_else(|| {
                eyre::eyre!(
                    "Error: Failure to find last finalized block with hash: {}",
                    PrettyPrinter::build_string_no_limit(&lfb_hash)
                )
            })?;

            // Use the same FT computation path as get_block for consistency.
            // Reads cached FT from DAG metadata (populated at finalization time,
            // propagated upward by propagate_ft_to_finalized_blocks).
            Ok(Self::get_block_info_with_dag(
                casper.as_ref(),
                &dag,
                &last_finalized_block,
                Self::construct_block_info,
            )
            .await?)
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    pub async fn is_finalized(engine_cell: &EngineCell, hash: &str) -> ApiErr<bool> {
        let error_message =
            "Could not check if block is finalized, casper instance was not available yet.";
        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            let dag = casper.block_dag().await?;
            let padded_hash = pad_hex_string(hash);
            let given_block_hash = hex::decode(&padded_hash).map_err(|_| {
                eyre::Report::new(InvalidHashError(format!(
                    "'{}' is not valid block hash",
                    hash
                )))
            })?;
            let result = dag.is_finalized(&given_block_hash.into())?;
            Ok(result)
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    /// Query the finalization status of a deploy by its signature. Clients
    /// should prefer this over block-hash finalization polling: after the
    /// merge fix, a block can finalize while some of its deploys' effects
    /// were dropped during merge — polling by block hash returns a
    /// misleading `true`. Polling by deploy sig via this API correctly
    /// reports the effect's canonical-state presence.
    ///
    /// Thin wrapper around
    /// `deploy_finalization_status::resolve` that unwraps the engine cell.
    pub async fn deploy_finalization_status(
        engine_cell: &EngineCell,
        sig: &[u8],
    ) -> ApiErr<crate::rust::api::deploy_finalization_status::DeployFinalizationStatus> {
        Self::deploy_finalization_status_with_known_block(engine_cell, sig, None).await
    }

    pub async fn deploy_finalization_status_with_known_block(
        engine_cell: &EngineCell,
        sig: &[u8],
        known_block_hash: Option<&BlockHash>,
    ) -> ApiErr<crate::rust::api::deploy_finalization_status::DeployFinalizationStatus> {
        let error_message =
            "Could not compute deploy finalization status, casper instance was not available yet.";
        let eng = engine_cell.get().await;
        let Some(casper) = eng.with_casper() else {
            tracing::warn!("{}", error_message);
            return Err(eyre::eyre!("Error: {}", error_message));
        };

        let dag = casper.block_dag().await?;
        // A pure LOOKUP over the deploy-lifecycle register: terminal
        // verdicts were determined at their threshold crossings by
        // `finality::deploy_lifecycle` and persisted write-once — no
        // per-query parameters.
        crate::rust::api::deploy_finalization_status::resolve(
            &dag,
            casper.block_store(),
            sig,
            known_block_hash,
        )
        .or_else(Self::corruption_to_pending)
    }

    /// Resolve a deploy's finalization status, reconciling a
    /// never-observed sig from the caller's saved signed deploy (#1103).
    ///
    /// Lookup-first: any register record answers as before, and a sig-only
    /// query (`deploy` = `None`) keeps the exact
    /// [`Self::deploy_finalization_status`] behavior. For a sig neither
    /// register table names, a supplied deploy — signature-verified by the
    /// decode path, shard-checked and sig-matched here — is judged against
    /// the floor of the adopted last finalized block:
    /// [`super::deploy_reconciliation`] states the rules and their fail-closed
    /// `Pending` cases. The response names that basis so a fail-closed
    /// caller can use it as evidence.
    pub async fn deploy_finalization_status_reconciled(
        engine_cell: &EngineCell,
        sig: &[u8],
        deploy: Option<&Signed<DeployData>>,
        shard_id: &str,
    ) -> ApiErr<crate::rust::api::deploy_reconciliation::ReconciledStatus> {
        let Some(deploy) = deploy else {
            return Self::deploy_finalization_status(engine_cell, sig)
                .await
                .map(crate::rust::api::deploy_reconciliation::ReconciledStatus::from_lookup);
        };
        if deploy.sig != sig {
            return Err(eyre::Report::new(DeployValidationError {
                message: "deploy.sig does not match the queried deploySig".to_string(),
            }));
        }
        if deploy.data.shard_id != shard_id {
            return Err(eyre::Report::new(DeployValidationError {
                message: "deploy shardId does not match the network shard".to_string(),
            }));
        }
        let error_message =
            "Could not compute deploy finalization status, casper instance was not available yet.";
        let eng = engine_cell.get().await;
        let Some(casper) = eng.with_casper() else {
            tracing::warn!("{}", error_message);
            return Err(eyre::eyre!("Error: {}", error_message));
        };

        let dag = casper.block_dag().await?;
        crate::rust::api::deploy_reconciliation::reconcile(
            &dag,
            casper.block_store(),
            casper.casper_shard_conf(),
            casper.effective_anchor_number(),
            deploy,
        )
        .await
    }

    /// The known-block inconsistency (a caller-claimed block whose body does
    /// not list the sig) converts to `pending_unknown` so HTTP/gRPC callers
    /// see a tractable response; genuine I/O failures keep propagating.
    fn corruption_to_pending(
        err: eyre::Report,
    ) -> ApiErr<crate::rust::api::deploy_finalization_status::DeployFinalizationStatus> {
        if err
            .downcast_ref::<crate::rust::api::deploy_finalization_status::DeployFinalizationCorruption>()
            .is_some()
        {
            Ok(crate::rust::api::deploy_finalization_status::DeployFinalizationStatus::pending_unknown())
        } else {
            Err(err)
        }
    }

    /// Bulk snapshot of pending deploys from both `deploy_storage` (fresh,
    /// not yet proposed) and `rejected_deploy_buffer` (recovering after a
    /// merge conflict). Each entry is paired with an `is_rejected` flag.
    ///
    /// When `deployer` is `Some`, only deploys signed by that public key
    /// are returned. The result is capped at
    /// [`pending_deploys::PENDING_DEPLOYS_MAX_RESULTS`] entries; the
    /// `total_available` field reports the count that matched before
    /// truncation, so callers can detect truncation by comparing
    /// `deploys.len() < total_available`.
    ///
    /// The queue is **node-local**: deploys never gossip, so an observer
    /// node always answers empty (it rejects `doDeploy`). For cross-node
    /// deploy status, use `deployFinalizationStatus` — it is DAG-derived
    /// and consistent across nodes. This API is for validator-side
    /// introspection of the local proposer pool.
    ///
    /// Returns an error on bootstrapping nodes where Casper is not yet
    /// initialised (`with_casper()` returns `None`).
    pub async fn list_pending_deploys(
        engine_cell: &EngineCell,
        deployer: Option<&[u8]>,
    ) -> ApiErr<crate::rust::api::pending_deploys::PendingDeploysSnapshot> {
        use crate::rust::api::pending_deploys::{
            PendingDeploysSnapshot, PENDING_DEPLOYS_MAX_RESULTS,
        };

        let error_message =
            "Could not list pending deploys, casper instance was not available yet.";
        let eng = engine_cell.get().await;
        let Some(casper) = eng.with_casper() else {
            tracing::warn!("{}", error_message);
            return Err(eyre::eyre!("Error: {}", error_message));
        };

        let mut deploys = casper.list_pending_deploys().await?;

        if let Some(pk) = deployer {
            deploys.retain(|(d, _)| d.pk.bytes.as_ref() == pk);
        }

        // Deterministic ordering before cap truncation: by timestamp, then
        // by signature bytes. Makes the truncation result reproducible
        // across calls for the same queue state.
        deploys.sort_by(|(a, _), (b, _)| {
            a.data
                .time_stamp
                .cmp(&b.data.time_stamp)
                .then_with(|| a.sig.as_ref().cmp(b.sig.as_ref()))
        });

        let total_available = deploys.len() as u32;
        deploys.truncate(PENDING_DEPLOYS_MAX_RESULTS);

        Ok(PendingDeploysSnapshot {
            deploys,
            total_available,
        })
    }

    pub async fn bond_status(engine_cell: &EngineCell, public_key: &ByteString) -> ApiErr<bool> {
        let error_message =
            "Could not check if validator is bonded, casper instance was not available yet.";
        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            let last_finalized_block = casper.last_finalized_block().await?;
            let runtime_manager = casper.runtime_manager();
            let post_state_hash = &last_finalized_block.body.state.post_state_hash;
            let bonds = runtime_manager.compute_bonds(post_state_hash).await?;
            let validator_bond_opt = bonds.iter().find(|bond| bond.validator == *public_key);
            Ok(validator_bond_opt.is_some())
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    /// Explore the data or continuation in the tuple space for specific blockHash
    ///
    /// - `term`: the term you want to explore in the request. Be sure the first `new` should be `return`
    /// - `block_hash`: the block hash you want to explore
    /// - `use_pre_state_hash`: Each block has preStateHash and postStateHash. If `use_pre_state_hash` is true, the explore
    ///   would try to execute on preState.
    pub async fn exploratory_deploy(
        engine_cell: &EngineCell,
        term: String,
        block_hash: Option<String>,
        use_pre_state_hash: bool,
        dev_mode: bool,
        deployer: Option<PublicKey>,
        parameters: Vec<DeployParameter>,
    ) -> ApiErr<(Vec<Par>, LightBlockInfo, u64)> {
        let error_message =
            "Could not execute exploratory deploy, casper instance was not available yet.";
        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            let is_read_only = casper.get_validator().is_none();
            if is_read_only || dev_mode {
                let runtime_manager = casper.runtime_manager();
                let execution_budget = runtime_manager.exploratory_deploy_execution_timeout_value();
                let permit = runtime_manager
                    .try_acquire_exploratory_deploy_permit()
                    .ok_or_else(|| {
                        metrics::counter!(
                            "exploratory_deploy.rejected",
                            "source" => "casper"
                        )
                        .increment(1);
                        eyre::Report::new(ExploratoryDeployBusyError::with_budget(execution_budget))
                    })?;

                let (state_hash, target_block) = if block_hash.is_none() {
                    let lfb = casper.last_finalized_block().await?;
                    (proto_util::post_state_hash(&lfb), Some(lfb))
                } else {
                    // Specific block requested: use its post-state
                    let hash_str = block_hash.as_ref().unwrap();
                    let padded_hash = pad_hex_string(hash_str);
                    let hash_byte_string = hex::decode(&padded_hash).map_err(|_| {
                        eyre::Report::new(InvalidHashError(format!(
                            "Input hash value is not valid hex string: {}",
                            hash_str
                        )))
                    })?;
                    let block_opt = casper.block_store().get(&hash_byte_string.into())?;

                    match block_opt {
                        Some(b) => {
                            // Issue #1157: refuse early if the target block's
                            // state has been swept — a structured answer beats
                            // a mid-walk `MissingNode` propagated as prose.
                            Self::pre_check_state_retention(casper.as_ref(), &b)
                                .await
                                .map_err(eyre::Report::new)?;
                            let state = if use_pre_state_hash {
                                proto_util::pre_state_hash(&b)
                            } else {
                                proto_util::post_state_hash(&b)
                            };
                            (state, Some(b))
                        }
                        None => {
                            return Err(eyre::Report::new(BlockNotFoundError {
                                hash: hash_str.to_string(),
                            }));
                        }
                    }
                };

                match target_block {
                    Some(b) => {
                        let timeout = execution_budget;
                        let outcome =
                            Arc::new(AtomicU8::new(ExploratoryDeployOutcome::Failed as u8));
                        let task_outcome = outcome.clone();
                        let task_runtime_manager = runtime_manager.clone();
                        let task = tokio::spawn(async move {
                            let _permit = permit;
                            let _metrics = ExploratoryDeployMetrics::new(task_outcome.clone());
                            let result = task_runtime_manager
                                .play_exploratory_deploy(term, &state_hash, deployer, parameters)
                                .await;
                            if result.is_ok() {
                                task_outcome.store(
                                    ExploratoryDeployOutcome::Completed as u8,
                                    Ordering::Relaxed,
                                );
                            }
                            result
                        });

                        let (res, cost) =
                            match await_exploratory_deploy_task(task, timeout, outcome).await {
                                Ok(Ok(outcome)) => outcome,
                                // Issue #1164: translate a mid-walk
                                // `RadixTreeError::MissingNode` that raced the
                                // pre-check into a structured
                                // `PrunedStateError`, so the typed `pruned`
                                // answer survives a late sweep.
                                Ok(Err(error)) => {
                                    return Err(Self::translate_pruned_race(
                                        casper.as_ref(),
                                        &b,
                                        eyre::Report::new(error),
                                    )
                                    .await);
                                }
                                Err(ExploratoryDeployTaskError::Join(error)) => {
                                    return Err(eyre::eyre!(
                                        "Exploratory query task failed: {}",
                                        error
                                    ));
                                }
                                Err(ExploratoryDeployTaskError::Timeout) => {
                                    return Err(eyre::Report::new(ExploratoryDeployTimeoutError {
                                        timeout_ms: u64::try_from(timeout.as_millis())
                                            .unwrap_or(u64::MAX),
                                    }));
                                }
                            };
                        let light_block_info =
                            Self::get_light_block_info(casper.as_ref(), &b).await?;
                        Ok((res, light_block_info, cost))
                    }
                    None => Err(eyre::eyre!(
                        "target block unexpectedly absent from block store (internal inconsistency)"
                    )),
                }
            } else {
                Err(eyre::Report::new(ExploratoryDeployReadOnlyError))
            }
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    pub async fn get_latest_message(engine_cell: &EngineCell) -> ApiErr<BlockMetadata> {
        let error_message = "Could not get latest message, casper instance was not available yet.";
        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            let validator_opt = casper.get_validator();
            let validator = validator_opt
                .ok_or_else(|| eyre::Report::new(LatestBlockMessageError::NodeReadOnlyError))?;
            let dag = casper.block_dag().await?;
            let latest_message_opt =
                dag.latest_message(&validator.public_key.bytes.clone().into())?;
            let latest_message = latest_message_opt
                .ok_or_else(|| eyre::Report::new(LatestBlockMessageError::NoBlockMessageError))?;
            Ok(latest_message)
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }

    pub async fn get_data_at_par(
        engine_cell: &EngineCell,
        par: &Par,
        block_hash: String,
        use_pre_state_hash: bool,
    ) -> ApiErr<(Vec<Par>, LightBlockInfo)> {
        async fn casper_response(
            casper: &dyn MultiParentCasper,
            par: &Par,
            block_hash: &str,
            use_pre_state_hash: bool,
        ) -> ApiErr<(Vec<Par>, LightBlockInfo)> {
            let padded_hash = pad_hex_string(block_hash);
            let hash_bytes = hex::decode(&padded_hash).map_err(|_| {
                eyre::Report::new(InvalidHashError(format!(
                    "'{}' is not valid block hash",
                    block_hash
                )))
            })?;
            let block_hash_bytes: BlockHash = hash_bytes.into();
            let block = casper
                .block_store()
                .get(&block_hash_bytes)
                .map_err(|e| eyre::eyre!(e.to_string()))?
                .ok_or_else(|| {
                    eyre::Report::new(BlockNotFoundError {
                        hash: block_hash.to_string(),
                    })
                })?;
            // Issue #1157: pre-check retention floor so the API returns a
            // structured `PrunedStateError` instead of a mid-walk
            // `MissingNode` propagated as prose.
            BlockAPI::pre_check_state_retention(casper, &block)
                .await
                .map_err(eyre::Report::new)?;
            let sorted_par = ParSortMatcher::sort_match(par).term;
            let runtime_manager = casper.runtime_manager();
            // Issue #1164: wrap the state read so a mid-walk
            // `RadixTreeError::MissingNode` that raced the pre-check
            // translates into the same `PrunedStateError` the pre-check
            // would have produced.
            let data = match BlockAPI::get_data_with_block_info(
                casper,
                runtime_manager,
                &sorted_par,
                &block,
                use_pre_state_hash,
            )
            .await
            {
                Ok(data) => data,
                Err(report) => {
                    return Err(BlockAPI::translate_pruned_race(casper, &block, report).await);
                }
            };
            if let Some(data_with_block_info) = data {
                Ok((
                    data_with_block_info.post_block_data,
                    data_with_block_info.block.unwrap_or_default(),
                ))
            } else {
                let block_info = BlockAPI::get_light_block_info(casper, &block).await?;
                Ok((vec![], block_info))
            }
        }

        let error_message = "Could not get data at par, casper instance was not available yet.";
        let eng = engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            casper_response(casper.as_ref(), par, &block_hash, use_pre_state_hash).await
        } else {
            tracing::warn!("{}", error_message);
            Err(eyre::eyre!("Error: {}", error_message))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use prost::bytes::Bytes;
    use tokio::sync::Semaphore;

    use super::{
        await_exploratory_deploy_task, deploy_is_block_expired, propose_failure_error,
        ExploratoryDeployOutcome, ExploratoryDeployTaskError, NoNewDeploysError, ProposeRefusal,
        ProposeRefusedError,
    };
    use crate::rust::blocks::proposer::propose_result::{
        CheckProposeConstraintsFailure, ProposeFailure, ProposeStatus,
    };

    struct DropSignal(Arc<AtomicBool>);

    impl Drop for DropSignal {
        fn drop(&mut self) { self.0.store(true, Ordering::Relaxed); }
    }

    #[tokio::test]
    async fn exploratory_timeout_aborts_and_joins_before_releasing_capacity() {
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore.clone().try_acquire_owned().unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        let task_dropped = dropped.clone();
        let task = tokio::spawn(async move {
            let _permit = permit;
            let _drop_signal = DropSignal(task_dropped);
            pending::<()>().await;
        });
        let outcome = Arc::new(AtomicU8::new(ExploratoryDeployOutcome::Failed as u8));

        let result =
            await_exploratory_deploy_task(task, Duration::from_millis(10), outcome.clone()).await;

        assert!(matches!(result, Err(ExploratoryDeployTaskError::Timeout)));
        assert!(dropped.load(Ordering::Relaxed));
        assert_eq!(
            outcome.load(Ordering::Relaxed),
            ExploratoryDeployOutcome::TimedOut as u8
        );
        assert!(semaphore.try_acquire_owned().is_ok());
    }

    #[test]
    fn block_expiration_matches_proposer_window() {
        assert!(deploy_is_block_expired(0, 50, 50).unwrap());
        assert!(!deploy_is_block_expired(1, 50, 50).unwrap());
        assert!(!deploy_is_block_expired(0, 49, 50).unwrap());
    }

    #[test]
    fn expected_propose_refusals_are_typed() {
        let cases = vec![
            (
                ProposeFailure::CheckConstraintsFailure(CheckProposeConstraintsFailure::NotBonded),
                ProposeRefusal::NotBonded,
                "Failure: Proposal failed: validator is not bonded (seqNum 7)",
            ),
            (
                ProposeFailure::CheckConstraintsFailure(
                    CheckProposeConstraintsFailure::NotEnoughNewBlocks,
                ),
                ProposeRefusal::NotEnoughNewBlocks,
                "Failure: Proposal failed: Must wait for more blocks from other validators (seqNum 7)",
            ),
            (
                ProposeFailure::CheckConstraintsFailure(
                    CheckProposeConstraintsFailure::TooFarAheadOfLastFinalized,
                ),
                ProposeRefusal::TooFarAheadOfLastFinalized,
                "Failure: Proposal failed: too far ahead of the last finalized block (seqNum 7)",
            ),
            (
                ProposeFailure::CheckConstraintsFailure(
                    CheckProposeConstraintsFailure::HistoryIncomplete,
                ),
                ProposeRefusal::HistoryIncomplete,
                "Failure: Proposal failed: this node's history does not reach far enough to build a snapshot; it is still catching up (seqNum 7)",
            ),
            (
                ProposeFailure::MissingBlock(Bytes::from_static(&[0xab; 32])),
                ProposeRefusal::MissingBlock,
                "Failure: Proposal failed: MissingBlock abababababababab — requested from peers, retrying (seqNum 7)",
            ),
            (
                ProposeFailure::FloorDistanceHold,
                ProposeRefusal::FloorDistanceHold,
                "Failure: Proposal held: the merge exceeds the floor-distance cap and no witness block could be built; retrying once finality advances (seqNum 7)",
            ),
        ];

        for (failure, expected_refusal, expected_message) in cases {
            let err = propose_failure_error(&ProposeStatus::Failure(failure), 7);
            let refused = err
                .downcast_ref::<ProposeRefusedError>()
                .unwrap_or_else(|| panic!("untyped propose refusal: {}", err));
            assert_eq!(refused.refusal, expected_refusal);
            assert_eq!(err.to_string(), expected_message);
        }
    }

    #[test]
    fn propose_faults_stay_untyped() {
        let no_new_deploys =
            propose_failure_error(&ProposeStatus::Failure(ProposeFailure::NoNewDeploys), 1);
        assert!(no_new_deploys.downcast_ref::<NoNewDeploysError>().is_some());

        for failure in [
            ProposeFailure::InternalDeployError,
            ProposeFailure::BugError,
        ] {
            let err = propose_failure_error(&ProposeStatus::Failure(failure), 1);
            assert!(err.downcast_ref::<ProposeRefusedError>().is_none());
            assert!(err.downcast_ref::<NoNewDeploysError>().is_none());
        }
    }

    /// Issue #1164: the reader-side classifier must recognise a mid-walk
    /// `RadixTreeError::MissingNode` in either nesting shape. The deeper
    /// `RSpaceError → HistoryError → RadixTreeError` form surfaces from
    /// the reset-and-replay path (`exploratory_deploy`'s `play_...`);
    /// the shallower `RSpaceError → RadixTreeError` form surfaces from a
    /// direct radix read (`get_data_at_par`'s `get_data_par`). Both are
    /// the same sweep race. Mirrors `block_status.rs`'s
    /// `a_missing_trie_node_is_awaiting_state_not_a_verdict` for the
    /// reader classifier.
    #[test]
    fn missing_node_recognised_in_both_nesting_shapes() {
        use rholang::rust::interpreter::errors::InterpreterError;
        use rspace_plus_plus::rspace::errors::{HistoryError, RSpaceError, RadixTreeError};
        use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

        use super::{eyre_chain_has_missing_node, CasperError};

        let ptr = Blake2b256Hash::from_bytes(vec![0xCD; 32]);

        let via_history = eyre::Report::new(CasperError::InterpreterError(
            InterpreterError::RSpaceError(RSpaceError::HistoryError(HistoryError::RadixTreeError(
                RadixTreeError::MissingNode { ptr: ptr.clone() },
            ))),
        ));
        assert!(
            eyre_chain_has_missing_node(&via_history),
            "reset-and-replay nesting (RSpaceError → HistoryError → RadixTreeError) must \
             classify as a sweep race"
        );

        let direct = eyre::Report::new(CasperError::InterpreterError(
            InterpreterError::RSpaceError(RSpaceError::RadixTreeError(
                RadixTreeError::MissingNode { ptr },
            )),
        ));
        assert!(
            eyre_chain_has_missing_node(&direct),
            "direct nesting (RSpaceError → RadixTreeError) must classify identically; the \
             sweep race is the same condition regardless of call site"
        );
    }

    /// Issue #1164: the classifier still fires once the typed error is
    /// one `eyre::Report::wrap_err` deep — the gRPC handler's own
    /// `classify_pruned_state` walks the chain, so the reader-side
    /// translator must survive any wrapping callers introduce before it
    /// reaches the handler. Guards the `err.chain()` traversal.
    #[test]
    fn missing_node_recognised_through_eyre_wrap() {
        use rholang::rust::interpreter::errors::InterpreterError;
        use rspace_plus_plus::rspace::errors::{HistoryError, RSpaceError, RadixTreeError};
        use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

        use super::{eyre_chain_has_missing_node, CasperError};

        let ptr = Blake2b256Hash::from_bytes(vec![0xAB; 32]);
        let raw = eyre::Report::new(CasperError::InterpreterError(
            InterpreterError::RSpaceError(RSpaceError::HistoryError(HistoryError::RadixTreeError(
                RadixTreeError::MissingNode { ptr },
            ))),
        ));
        let wrapped = raw.wrap_err("exploratory deploy failed on mid-walk node");
        assert!(eyre_chain_has_missing_node(&wrapped));
    }

    /// Issue #1164: everything that is not the sweep race must pass
    /// through unchanged. A false positive here would translate an
    /// unrelated failure (runtime error, replay failure, store error)
    /// into a `PrunedStateError` and mislead the client into thinking
    /// an archive node could serve the read.
    #[test]
    fn unrelated_errors_do_not_classify_as_pruned_race() {
        use super::{eyre_chain_has_missing_node, CasperError};

        let runtime_err =
            eyre::Report::new(CasperError::RuntimeError("some unrelated failure".into()));
        assert!(!eyre_chain_has_missing_node(&runtime_err));

        let bare = eyre::eyre!("bare prose error without a typed root cause");
        assert!(!eyre_chain_has_missing_node(&bare));
    }
}
