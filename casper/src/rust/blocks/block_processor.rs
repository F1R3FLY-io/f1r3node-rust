// See casper/src/main/scala/coop/rchain/casper/blocks/BlockProcessor.scala

/*
 * ARCHITECTURAL CHOICE: Trait-based Dependency Injection
 *
 * This implementation uses trait-based dependency injection instead of functional closures
 * because Rust's ownership model and async system work better with traits than with complex
 * closure captures. Traits provide zero-cost abstractions, better testability, and seamless
 * async support while maintaining the same flexibility as the original Scala version.
 */

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, CasperMessage};
use prost::Message;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::env;
use tokio::sync::mpsc;

use crate::rust::block_status::{BlockError, InvalidBlock};
use crate::rust::casper::{Casper, CasperSnapshot};
use crate::rust::engine::block_retriever::{AdmitHashReason, BlockRetriever};
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_PROCESSING_STORAGE_TIME_METRIC, BLOCK_PROCESSING_VALIDATION_SETUP_TIME_METRIC,
    BLOCK_PROCESSOR_METRICS_SOURCE, BLOCK_SIZE_METRIC, BLOCK_VALIDATION_FAILED_METRIC,
    BLOCK_VALIDATION_SUCCESS_METRIC, BLOCK_VALIDATION_TIME_METRIC,
};
use crate::rust::util::proto_util;
use crate::rust::validate::Validate;
use crate::rust::ValidBlockProcessing;

/// Logic for processing incoming blocks
/// Blocks created by node itself are not held here, but in Proposer.
#[derive(Clone)]
pub struct BlockProcessor<T: TransportLayer + Send + Sync> {
    dependencies: BlockProcessorDependencies<T>,
}

/// What must happen to a block once validation has returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PostValidation {
    /// The block was judged. Drop it from the buffer and stop tracking it.
    Settled,
    /// The block was NOT judged: validation needed a block this node does not
    /// hold. Keep it buffered against the named dependency and fetch that, or
    /// the block is dropped un-judged and the gap it needs is never requested.
    AwaitingBlock(BlockHash),
    /// The block was NOT judged: replay needed a state root this node does
    /// not hold. Keep it buffered as a pendant — the pendant scan retries it
    /// after each processed block, throttled by the missing-dependency
    /// attempts machinery — and hand the root to the state requester.
    AwaitingState(rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash),
}

/// Withdraw the deferral if this node has no hole in its history.
///
/// `Undecidable` is the one outcome that is not a verdict, so it is also the
/// one an attacker would want: a block that induces it is never judged, never
/// recorded invalid, and produces no evidence. That is only acceptable when the
/// node truly cannot know — which is exactly when its own history is cut short.
///
/// A node built from genesis holds a complete main-parent spine, so a block it
/// cannot find is corruption and must be judged as before. A node restored from
/// a sync anchor has nothing below that anchor and never will:
/// `last_approved_block` is written once — at the genesis ceremony or at LFS
/// restore — and never advances, so its height is a durable statement about
/// what that node can answer, not a transient flag.
pub(crate) fn guard_deferral(
    status: ValidBlockProcessing,
    approved_block_number: i64,
) -> ValidBlockProcessing {
    match status {
        Either::Left(BlockError::Undecidable(hash)) if approved_block_number == 0 => {
            Either::Left(BlockError::BlockException(CasperError::BlockNotHeld(hash)))
        }
        // Same rule for the state artifact: a genesis-rooted node computed or
        // imported every root it ever needed, so a missing one is corruption
        // and must be judged — deferring would hand a crafted block a
        // permanent non-verdict on any full node.
        Either::Left(BlockError::AwaitingState(root)) if approved_block_number == 0 => {
            Either::Left(BlockError::BlockException(CasperError::Other(format!(
                "state root {} missing on a genesis-rooted node — local corruption, not sync",
                root
            ))))
        }
        other => other,
    }
}

/// Whether an arriving block is settled history to be admitted unjudged —
/// the LFS door, opened at runtime.
///
/// A restored node's own restore inserted hundreds of blocks hash-checked and
/// unexecuted; a straggler from the same settled region — cited by gossip the
/// restore could not have known about — is the same kind of block and gets the
/// same treatment. Judging it instead is what broke: the node-state validation
/// checks assume dependency-ordered insertion, which the restore itself
/// bypassed, so the verdicts they produce on old blocks are statements about
/// this node's restore, not about the block.
///
/// Each condition closes a distinct attack; see the truth-table test.
///
/// `seq_below_senders_latest` requires the block's sequence number to sit
/// strictly below the sender's current latest message. Genuine settled
/// stragglers always do — settled history predates the anchor's
/// justification frontier — while a block at-or-above that frontier is
/// live-chain material wearing a sub-anchor height (the CI run 32588262605
/// pollution shape: shared validator keys, foreign seq 40 against a live
/// seq-5 head). A sender with no latest message fails the condition and
/// takes the judged path, which defers safely.
pub(crate) fn admit_as_settled(
    block_number: i64,
    approved_block_number: i64,
    solicited_by_bonded: bool,
    budget_remaining: bool,
    seq_below_senders_latest: bool,
) -> bool {
    approved_block_number > 0
        && block_number <= approved_block_number
        && solicited_by_bonded
        && budget_remaining
        && seq_below_senders_latest
}

/// Classify a validation outcome for post-processing.
///
/// Everything except `Undecidable` is a verdict and is settled. `Undecidable`
/// is the absence of one, so the block must survive to be retried: dropping it
/// loses the block, and the missing hash it names is the only thing that can
/// unstick the node.
pub(crate) fn post_validation(status: &ValidBlockProcessing) -> PostValidation {
    match status {
        Either::Left(BlockError::Undecidable(missing)) => {
            PostValidation::AwaitingBlock(missing.clone())
        }
        Either::Left(BlockError::AwaitingState(root)) => {
            PostValidation::AwaitingState(root.clone())
        }
        _ => PostValidation::Settled,
    }
}

/// Lifetime cap on settled-history admissions. Legitimate joins need single
/// digits (the gaps LFS's closure missed); the cap prices the worst case — a
/// BONDED attacker citing self-signed junk below the anchor — at bounded,
/// alarmed storage. Past it the node degrades to today's deferral, loudly.
const SETTLED_ADMISSION_BUDGET: u64 = 512;

const CASPER_BUFFER_PRUNE_INTERVAL_MS: u64 = 5_000;
const CASPER_BUFFER_STALE_TTL_MS: u64 = 180_000;
const CASPER_BUFFER_MAX_APPROX_NODES: usize = 16_384;
const CASPER_BUFFER_MAX_PRUNE_BATCH: usize = 512;
const CASPER_BUFFER_MAX_APPROX_NODES_ENV: &str = "F1R3_CASPER_BUFFER_MAX_APPROX_NODES";
const CASPER_BUFFER_STALE_TTL_MS_ENV: &str = "F1R3_CASPER_BUFFER_STALE_TTL_MS";
const CASPER_BUFFER_MAX_PRUNE_BATCH_ENV: &str = "F1R3_CASPER_BUFFER_MAX_PRUNE_BATCH";
const CASPER_BUFFER_PRUNE_INTERVAL_MS_ENV: &str = "F1R3_CASPER_BUFFER_PRUNE_INTERVAL_MS";
const CASPER_BUFFER_STALE_PRUNED_METRIC: &str = "casper.buffer.stale-pruned";
const CASPER_BUFFER_OVERFLOW_PRUNED_METRIC: &str = "casper.buffer.overflow-pruned";
const CASPER_BUFFER_APPROX_NODES_METRIC: &str = "casper.buffer.approx-nodes";
const CASPER_BUFFER_DEPENDENCY_LOOP_PRUNED_METRIC: &str = "casper.buffer.dependency-loop-pruned";
const MISSING_DEPENDENCY_ATTEMPTS_MAX_DEFAULT: u32 = 32;
const MISSING_DEPENDENCY_ATTEMPTS_MAX_ENV: &str = "F1R3_MISSING_DEPENDENCY_ATTEMPTS_MAX";
const MISSING_DEPENDENCY_QUARANTINE_MS_DEFAULT: u64 = 120_000;
const MISSING_DEPENDENCY_QUARANTINE_MS_ENV: &str = "F1R3_MISSING_DEPENDENCY_QUARANTINE_MS";
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const MALLOC_TRIM_INTERVAL_BLOCKS_DEFAULT: u64 = 64;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const MALLOC_TRIM_INTERVAL_BLOCKS_ENV: &str = "F1R3_MALLOC_TRIM_EVERY_BLOCKS";
#[cfg(all(target_os = "linux", target_env = "gnu"))]
static MALLOC_TRIM_BLOCK_COUNTER: AtomicU64 = AtomicU64::new(0);
#[cfg(all(target_os = "linux", target_env = "gnu"))]
static MALLOC_TRIM_INTERVAL_BLOCKS: OnceLock<u64> = OnceLock::new();
static CASPER_BUFFER_MAX_APPROX_NODES_CFG: OnceLock<usize> = OnceLock::new();
static CASPER_BUFFER_STALE_TTL_MS_CFG: OnceLock<u64> = OnceLock::new();
static CASPER_BUFFER_MAX_PRUNE_BATCH_CFG: OnceLock<usize> = OnceLock::new();
static CASPER_BUFFER_PRUNE_INTERVAL_MS_CFG: OnceLock<u64> = OnceLock::new();
static MISSING_DEPENDENCY_ATTEMPTS_MAX_CFG: OnceLock<u32> = OnceLock::new();
static MISSING_DEPENDENCY_QUARANTINE_MS_CFG: OnceLock<u64> = OnceLock::new();

#[cfg(all(target_os = "linux", target_env = "gnu"))]
unsafe extern "C" {
    fn malloc_trim(pad: usize) -> i32;
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn malloc_trim_interval_blocks() -> u64 {
    *MALLOC_TRIM_INTERVAL_BLOCKS.get_or_init(|| {
        env::var_or(
            MALLOC_TRIM_INTERVAL_BLOCKS_ENV,
            MALLOC_TRIM_INTERVAL_BLOCKS_DEFAULT,
        )
    })
}

fn casper_buffer_max_approx_nodes() -> usize {
    *CASPER_BUFFER_MAX_APPROX_NODES_CFG.get_or_init(|| {
        env::var_or(
            CASPER_BUFFER_MAX_APPROX_NODES_ENV,
            CASPER_BUFFER_MAX_APPROX_NODES,
        )
    })
}

fn casper_buffer_stale_ttl_ms() -> u64 {
    *CASPER_BUFFER_STALE_TTL_MS_CFG
        .get_or_init(|| env::var_or(CASPER_BUFFER_STALE_TTL_MS_ENV, CASPER_BUFFER_STALE_TTL_MS))
}

fn casper_buffer_max_prune_batch() -> usize {
    *CASPER_BUFFER_MAX_PRUNE_BATCH_CFG.get_or_init(|| {
        env::var_or(
            CASPER_BUFFER_MAX_PRUNE_BATCH_ENV,
            CASPER_BUFFER_MAX_PRUNE_BATCH,
        )
    })
}

fn casper_buffer_prune_interval_ms() -> u64 {
    *CASPER_BUFFER_PRUNE_INTERVAL_MS_CFG.get_or_init(|| {
        env::var_or(
            CASPER_BUFFER_PRUNE_INTERVAL_MS_ENV,
            CASPER_BUFFER_PRUNE_INTERVAL_MS,
        )
    })
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn maybe_trim_allocator_after_block() {
    let interval = malloc_trim_interval_blocks();
    if interval == 0 {
        return;
    }
    let n = MALLOC_TRIM_BLOCK_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    if n.is_multiple_of(interval) {
        use crate::rust::metrics_constants::ALLOCATOR_TRIM_TOTAL_METRIC;
        // Best-effort return of free heap pages to OS to limit RSS ratcheting.
        unsafe {
            let _ = malloc_trim(0);
        }
        metrics::counter!(ALLOCATOR_TRIM_TOTAL_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .increment(1);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn maybe_trim_allocator_after_block() {}

fn missing_dependency_attempts_max() -> u32 {
    *MISSING_DEPENDENCY_ATTEMPTS_MAX_CFG.get_or_init(|| {
        env::var_or_filtered(
            MISSING_DEPENDENCY_ATTEMPTS_MAX_ENV,
            MISSING_DEPENDENCY_ATTEMPTS_MAX_DEFAULT,
            |v: &u32| *v > 0,
        )
    })
}

fn missing_dependency_quarantine_ms() -> u64 {
    *MISSING_DEPENDENCY_QUARANTINE_MS_CFG.get_or_init(|| {
        env::var_or_filtered(
            MISSING_DEPENDENCY_QUARANTINE_MS_ENV,
            MISSING_DEPENDENCY_QUARANTINE_MS_DEFAULT,
            |v: &u64| *v > 0,
        )
    })
}

impl<T: TransportLayer + Send + Sync> BlockProcessor<T> {
    pub fn new(dependencies: BlockProcessorDependencies<T>) -> Self { Self { dependencies } }

    /// The height this node was started from. Zero means genesis — a complete
    /// spine, so nothing below it can legitimately be absent.
    fn approved_block_number(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
    ) -> Result<i64, CasperError> {
        casper
            .get_approved_block()
            .map(|approved| proto_util::block_number(approved))
    }

    /// check if block should be processed
    pub fn check_if_of_interest(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<bool, CasperError> {
        // TODO casper.dag_contains does not take into account equivocation tracker
        let already_processed =
            casper.dag_contains(&block.block_hash) || casper.buffer_contains(&block.block_hash);

        let shard_of_interest = casper.get_approved_block().map(|approved_block| {
            approved_block
                .shard_id
                .eq_ignore_ascii_case(&block.shard_id)
        })?;

        let version_of_interest = casper
            .get_approved_block()
            .map(|approved_block| Validate::version(block, approved_block.header.version))?;

        let old_block = casper.get_approved_block().map(|approved_block| {
            proto_util::block_number(block) < proto_util::block_number(approved_block)
        })?;

        // A block this node requested to satisfy a missing dependency is of
        // interest whatever its height. Dropping it as "old" is why a joiner
        // can never acquire pre-anchor history: it requests the dependency,
        // receives it, discards it here, and the dependent block retries
        // forever — 23,643 attempts on one block before the shard's finality
        // stalled behind the joiner's idle stake. The `old_block` filter still
        // does its real job, since unsolicited gossip is never in this set.
        let requested_as_dependency = self
            .dependencies
            .was_requested_as_dependency(&block.block_hash)?;

        Ok(!already_processed
            && shard_of_interest
            && version_of_interest
            && (!old_block || requested_as_dependency))
    }

    /// check block format and store if check passed
    pub async fn check_if_well_formed_and_store(
        &self,
        block: &BlockMessage,
    ) -> Result<bool, CasperError> {
        let valid_format = Validate::format_of_fields(block);
        let valid_sig = Validate::block_signature(block);
        let is_valid = valid_format && valid_sig;

        if is_valid {
            // Time storage operation
            let storage_start = Instant::now();
            self.dependencies.store_block(block).await?;
            metrics::histogram!(BLOCK_PROCESSING_STORAGE_TIME_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                .record(storage_start.elapsed().as_secs_f64());
        }

        Ok(is_valid)
    }

    /// check if block has all dependencies available and can be validated
    pub async fn check_dependencies_with_effects(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<bool, CasperError> {
        self.dependencies.prune_casper_buffer_if_needed()?;
        self.dependencies
            .sweep_expired_missing_dependency_quarantine()?;
        self.dependencies
            .sweep_orphaned_missing_dependency_attempts()?;
        self.dependencies
            .sweep_orphaned_missing_dependency_quarantine()?;

        if self
            .dependencies
            .is_missing_dependency_quarantined(&block.block_hash)?
        {
            tracing::debug!(
                "Skipping block {} due to missing-dependency quarantine ({}ms).",
                PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true),
                missing_dependency_quarantine_ms()
            );
            metrics::counter!(CASPER_BUFFER_DEPENDENCY_LOOP_PRUNED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE, "reason" => "quarantine")
                .increment(1);
            // Keep buffered block graph intact while quarantined.
            // Dropping buffered blocks here can break dependency chains and stall finality.
            return Ok(false);
        }

        let (is_ready, deps_to_fetch, deps_in_buffer) = self
            .dependencies
            .get_non_validated_dependencies(casper.clone(), block)
            .await?;
        self.dependencies
            .record_settled_solicitations(&casper, block, &deps_to_fetch);

        if is_ready {
            self.dependencies
                .clear_missing_dependency_attempts(&block.block_hash)?;
            // store pendant block in buffer, it will be removed once block is validated and added to DAG
            self.dependencies.commit_to_buffer(block, None).await?;
        } else {
            if self
                .dependencies
                .register_missing_dependency_attempt(&block.block_hash)?
            {
                tracing::warn!(
                    "Throttling block {} after {} missing-dependency checks (keeping in buffer).",
                    PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true),
                    missing_dependency_attempts_max()
                );
                metrics::counter!(CASPER_BUFFER_DEPENDENCY_LOOP_PRUNED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE, "reason" => "attempts")
                    .increment(1);
                self.dependencies
                    .clear_missing_dependency_attempts(&block.block_hash)?;
                self.dependencies
                    .mark_missing_dependency_quarantine(&block.block_hash)?;
            }

            // associate parents with new block in casper buffer
            let mut all_deps = deps_to_fetch.clone();
            all_deps.extend(deps_in_buffer.clone());
            self.dependencies
                .commit_to_buffer(block, Some(all_deps))
                .await?;
            self.dependencies
                .request_missing_dependencies(&deps_to_fetch)
                .await?;
            // Recovery path: if dependency graph is stuck in buffer (no fresh deps to fetch),
            // force a network re-request for buffered dependencies.
            if deps_to_fetch.is_empty() && !deps_in_buffer.is_empty() {
                self.dependencies
                    .recover_stale_buffer_dependencies(&deps_in_buffer)
                    .await?;
            }
            self.dependencies.ack_processed(block).await?;
        }

        Ok(is_ready)
    }

    /// validate block and invoke all effects required
    pub async fn validate_with_effects(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
        // this option is required for tests, as sometimes block without parents available are added, so
        // CasperSnapshot cannot be constructed
        snapshot_opt: Option<CasperSnapshot>,
    ) -> Result<ValidBlockProcessing, CasperError> {
        // Record block size
        let block_size = block.to_proto().encode_to_vec().len();
        metrics::histogram!(BLOCK_SIZE_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .record(block_size as f64);

        // Time validation setup
        let setup_start = Instant::now();
        let mut snapshot = match snapshot_opt {
            Some(snapshot) => snapshot,
            None => match self
                .dependencies
                .get_casper_state_snapshot(casper.clone())
                .await
            {
                Ok(snapshot) => snapshot,
                // The snapshot walks the same history the floor does, so it hits
                // the same edge first on a node whose history is short. Report it
                // as the absence of a verdict rather than erroring the block out
                // of the pipeline un-judged and untracked — but only if this node
                // is entitled to defer at all.
                Err(CasperError::BlockNotHeld(missing)) => {
                    let guarded = guard_deferral(
                        Either::Left(BlockError::Undecidable(missing.clone())),
                        self.approved_block_number(casper.clone())?,
                    );
                    if !matches!(guarded, Either::Left(BlockError::Undecidable(_))) {
                        return Err(CasperError::BlockNotHeld(missing));
                    }
                    tracing::warn!(
                        "Snapshot for block {} needs {}, which this node does not hold.",
                        PrettyPrinter::build_string_bytes(&block.block_hash),
                        PrettyPrinter::build_string_bytes(&missing)
                    );
                    let deps = HashSet::from([missing.clone()]);
                    self.dependencies
                        .record_settled_solicitations(&casper, block, &deps);
                    self.dependencies
                        .commit_to_buffer(block, Some(deps.clone()))
                        .await?;
                    self.dependencies
                        .request_missing_dependencies(&deps)
                        .await?;
                    self.dependencies.ack_processed(block).await?;
                    return Ok(guarded);
                }
                Err(err) => return Err(err),
            },
        };
        metrics::histogram!(BLOCK_PROCESSING_VALIDATION_SETUP_TIME_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .record(setup_start.elapsed().as_secs_f64());

        // Time block validation
        let validation_start = Instant::now();
        let status = self
            .dependencies
            .validate_block(casper.clone(), &mut snapshot, block)
            .await?;
        // Validation reports what it found; whether this node may answer "I
        // cannot judge" is a fact about the node, decided here.
        let status = guard_deferral(status, self.approved_block_number(casper.clone())?);
        metrics::histogram!(BLOCK_VALIDATION_TIME_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .record(validation_start.elapsed().as_secs_f64());

        // Record validation outcome
        let _ = match &status {
            Either::Right(_valid_block) => {
                metrics::counter!(BLOCK_VALIDATION_SUCCESS_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                    .increment(1);
                self.dependencies
                    .effects_for_valid_block(casper.clone(), block)
                    .await
            }
            Either::Left(invalid_block) => {
                metrics::counter!(BLOCK_VALIDATION_FAILED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                    .increment(1);
                // this is to maintain backward compatibility with casper validate method.
                // as it returns not only InvalidBlock or ValidBlock
                match invalid_block {
                    BlockError::Invalid(i) => {
                        self.dependencies
                            .effects_for_invalid_block(casper.clone(), block, i, &snapshot)
                            .await
                    }
                    // BlockException → InvalidTransaction is safe: validation_dispatcher.rs:548
                    // routes every is_slashable() variant through the same record-creation path
                    // as AdmissibleEquivocation, so the slash pipeline fires identically. See
                    // docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.4 and
                    // theorem T-9.3 (`t_9_3_dispatch_complete`, BugFixDispatcher.v:41).
                    BlockError::BlockException(ref err) => {
                        tracing::warn!(
                            "Block {} raised BlockException ({}); recording as InvalidTransaction to prevent dependent-block stall.",
                            PrettyPrinter::build_string_bytes(&block.block_hash),
                            err
                        );
                        self.dependencies
                            .effects_for_invalid_block(
                                casper.clone(),
                                block,
                                &InvalidBlock::InvalidTransaction,
                                &snapshot,
                            )
                            .await
                    }
                    _ => Ok(snapshot.dag.clone()),
                }
            }
        }?;

        match post_validation(&status) {
            PostValidation::Settled => {
                // once block is validated and effects are invoked, it should be removed from buffer
                self.dependencies.remove_from_buffer(block).await?;
                self.dependencies.ack_processed(block).await?;
            }
            PostValidation::AwaitingBlock(missing) => {
                tracing::warn!(
                    "Block {} could not be judged: this node does not hold {}. Keeping it \
                     buffered and requesting that block.",
                    PrettyPrinter::build_string_bytes(&block.block_hash),
                    PrettyPrinter::build_string_bytes(&missing)
                );
                let deps = HashSet::from([missing]);
                self.dependencies
                    .record_settled_solicitations(&casper, block, &deps);
                self.dependencies
                    .commit_to_buffer(block, Some(deps.clone()))
                    .await?;
                self.dependencies
                    .request_missing_dependencies(&deps)
                    .await?;
                self.dependencies.ack_processed(block).await?;
            }
            PostValidation::AwaitingState(root) => {
                tracing::warn!(
                    block = %PrettyPrinter::build_string_bytes(&block.block_hash),
                    %root,
                    "Block could not be judged: this node does not hold the state root its \
                     replay starts from. Keeping it buffered and fetching the root."
                );
                // The pendant scan retries this block after every processed
                // block; the attempts machinery throttles a block whose root
                // never arrives, exactly as it throttles one whose missing
                // BLOCK never arrives.
                if self
                    .dependencies
                    .register_missing_dependency_attempt(&block.block_hash)?
                {
                    self.dependencies
                        .clear_missing_dependency_attempts(&block.block_hash)?;
                    self.dependencies
                        .mark_missing_dependency_quarantine(&block.block_hash)?;
                }
                self.dependencies.commit_to_buffer(block, None).await?;
                self.dependencies.request_state_root(&root);
                self.dependencies.ack_processed(block).await?;
            }
        }
        maybe_trim_allocator_after_block();

        Ok(status)
    }

    /// Equivalent to Scala's: ackProcessed = (b: BlockMessage) => BlockRetriever[F].ackInCasper(b.blockHash)
    pub async fn ack_processed(&self, block: &BlockMessage) -> Result<(), CasperError> {
        self.dependencies.ack_processed(block).await
    }

    /// See [`BlockProcessorDependencies::try_admit_settled`].
    pub async fn try_admit_settled(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<bool, CasperError> {
        self.dependencies.try_admit_settled(casper, block).await
    }

    /// Remove block hash from CasperBuffer dependency graph.
    pub async fn remove_from_buffer(&self, block: &BlockMessage) -> Result<(), CasperError> {
        self.dependencies.remove_from_buffer(block).await
    }

    /// Best-effort purge for stale/uninteresting blocks to prevent infinite buffer requeue loops.
    pub async fn purge_from_buffer_and_ack(&self, block: &BlockMessage) -> Result<(), CasperError> {
        self.dependencies.remove_from_buffer(block).await?;
        self.dependencies.ack_processed(block).await
    }
}

/// Unified dependencies structure - equivalent to Scala companion object approach
/// Contains all dependencies needed for block processing in one place
#[derive(Clone)]
pub struct BlockProcessorDependencies<T: TransportLayer + Send + Sync> {
    block_store: KeyValueBlockStore,
    casper_buffer: CasperBufferKeyValueStorage,
    block_dag_storage: BlockDagKeyValueStorage,
    block_retriever: BlockRetriever<T>,
    transport: Arc<T>,
    connections_cell: ConnectionsCell,
    conf: RPConf,
    casper_buffer_last_prune_ms: Arc<AtomicU64>,
    missing_dependency_attempts: Arc<Mutex<HashMap<BlockHash, u32>>>,
    missing_dependency_quarantine_until: Arc<Mutex<HashMap<BlockHash, u64>>>,
    /// Hashes solicited as dependencies by a block whose sender is bonded in
    /// this node's anchor. Membership is the third condition of
    /// [`admit_as_settled`]; entries are removed when the block arrives, and
    /// the set is capped so no-shows cannot grow it unboundedly.
    settled_solicitations: Arc<Mutex<HashSet<BlockHash>>>,
    /// Blocks admitted as settled history since start; compared against
    /// [`SETTLED_ADMISSION_BUDGET`].
    settled_admissions: Arc<AtomicU64>,
    /// Names missing state roots to the runtime state requester. `None` only
    /// in test constructions; without it a missing root still defers safely,
    /// it just never heals.
    state_root_fetch_tx: Option<mpsc::Sender<Blake2b256Hash>>,
}

impl<T: TransportLayer + Send + Sync> BlockProcessorDependencies<T> {
    pub fn new(
        block_store: KeyValueBlockStore,
        casper_buffer: CasperBufferKeyValueStorage,
        block_dag_storage: BlockDagKeyValueStorage,
        block_retriever: BlockRetriever<T>,
        transport: Arc<T>,
        connections_cell: ConnectionsCell,
        conf: RPConf,
        state_root_fetch_tx: Option<mpsc::Sender<Blake2b256Hash>>,
    ) -> Self {
        Self {
            block_store,
            casper_buffer,
            block_dag_storage,
            block_retriever,
            transport,
            connections_cell,
            conf,
            casper_buffer_last_prune_ms: Arc::new(AtomicU64::new(0)),
            missing_dependency_attempts: Arc::new(Mutex::new(HashMap::new())),
            missing_dependency_quarantine_until: Arc::new(Mutex::new(HashMap::new())),
            settled_solicitations: Arc::new(Mutex::new(HashSet::new())),
            settled_admissions: Arc::new(AtomicU64::new(0)),
            state_root_fetch_tx,
        }
    }

    /// Name a missing root to the state requester, if one is wired.
    fn request_state_root(&self, root: &Blake2b256Hash) {
        match &self.state_root_fetch_tx {
            Some(tx) => {
                if tx.try_send(root.clone()).is_err() {
                    tracing::warn!(
                        %root,
                        "state requester queue full or closed; the root stays absent and \
                         its dependents keep deferring"
                    );
                }
            }
            None => tracing::warn!(
                %root,
                "no state requester wired; the root stays absent and its dependents \
                 keep deferring"
            ),
        }
    }

    /// Record which solicited hashes were cited by a bonded validator's block,
    /// making them candidates for settled-history admission when they arrive.
    ///
    /// Bondedness is judged against the ANCHOR's bond set: the anchor is the
    /// one block a restored node trusts unconditionally, and a validator bonded
    /// there has stake to lose — its blocks are signature-checked before this
    /// runs, so an attacker cannot borrow the status. A citer bonded only after
    /// the anchor does not qualify; its solicitations take the ordinary path,
    /// which fails toward deferral, never toward admission.
    fn record_settled_solicitations(
        &self,
        casper: &Arc<dyn Casper + Send + Sync + 'static>,
        citer: &BlockMessage,
        deps: &HashSet<BlockHash>,
    ) {
        const SETTLED_SOLICITATIONS_CAP: usize = 4_096;

        let Ok(anchor) = casper.get_approved_block() else {
            return;
        };
        let citer_is_bonded = anchor
            .body
            .state
            .bonds
            .iter()
            .any(|bond| bond.validator == citer.sender);
        if !citer_is_bonded {
            return;
        }
        let Ok(mut solicitations) = self.settled_solicitations.lock() else {
            return;
        };
        if solicitations.len() + deps.len() > SETTLED_SOLICITATIONS_CAP {
            tracing::warn!(
                tracked = solicitations.len(),
                incoming = deps.len(),
                "Settled-solicitation set at capacity; new dependencies take the \
                 deferral path instead of the admission door"
            );
            return;
        }
        solicitations.extend(deps.iter().cloned());
    }

    /// Take (and thereby consume) the settled-solicitation marker for a hash.
    fn take_settled_solicitation(&self, hash: &BlockHash) -> bool {
        self.settled_solicitations
            .lock()
            .map(|mut set| set.remove(hash))
            .unwrap_or(false)
    }

    // Public getters for tests
    pub fn transport(&self) -> &Arc<T> { &self.transport }

    pub fn casper_buffer(&self) -> &CasperBufferKeyValueStorage { &self.casper_buffer }

    fn prune_casper_buffer_if_needed(&self) -> Result<(), CasperError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let last_prune = self.casper_buffer_last_prune_ms.load(Ordering::Relaxed);
        let prune_interval_ms = casper_buffer_prune_interval_ms();
        if now_ms.saturating_sub(last_prune) < prune_interval_ms {
            return Ok(());
        }
        self.casper_buffer_last_prune_ms
            .store(now_ms, Ordering::Relaxed);

        let (stale_pruned, overflow_pruned) = self.casper_buffer.enforce_limits(
            casper_buffer_max_approx_nodes(),
            casper_buffer_stale_ttl_ms(),
            casper_buffer_max_prune_batch(),
            prune_interval_ms,
        )?;
        let approx_nodes = self.casper_buffer.approx_node_count();

        metrics::gauge!(CASPER_BUFFER_APPROX_NODES_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .set(approx_nodes as f64);
        if stale_pruned > 0 {
            metrics::counter!(CASPER_BUFFER_STALE_PRUNED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                .increment(stale_pruned as u64);
        }
        if overflow_pruned > 0 {
            metrics::counter!(CASPER_BUFFER_OVERFLOW_PRUNED_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                .increment(overflow_pruned as u64);
        }
        if stale_pruned > 0 || overflow_pruned > 0 {
            tracing::warn!(
                "Pruned CasperBuffer entries: stale={}, overflow={}, approx_nodes={}",
                stale_pruned,
                overflow_pruned,
                approx_nodes
            );
        }

        Ok(())
    }

    /// Equivalent to Scala's: storeBlock = (b: BlockMessage) => BlockStore[F].put(b)
    pub async fn store_block(&self, block: &BlockMessage) -> Result<(), CasperError> {
        self.block_store.put_block_message(block)?;
        Ok(())
    }

    /// Equivalent to Scala's: getCasperStateSnapshot = (c: Casper[F]) => c.getSnapshot
    pub async fn get_casper_state_snapshot(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
    ) -> Result<CasperSnapshot, CasperError> {
        casper.get_snapshot().await
    }

    /// Equivalent to Scala's: getNonValidatedDependencies = (c: Casper[F], b: BlockMessage) => { ... }
    pub async fn get_non_validated_dependencies(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<(bool, HashSet<BlockHash>, HashSet<BlockHash>), CasperError> {
        let all_deps = proto_util::dependencies_hashes_of(block);

        // in addition, equivocation tracker has to be checked, as admissible equivocations are not stored in DAG
        let equivocation_hashes: HashSet<BlockHash> = {
            self.block_dag_storage
                .access_equivocations_tracker(|tracker| {
                    let equivocation_records = tracker.data()?;
                    // Use HashSet to ensure uniqueness and O(1) lookup, just like Scala's Set
                    let hashes: HashSet<BlockHash> = equivocation_records
                        .iter()
                        .flat_map(|record| record.equivocation_detected_block_hashes.iter())
                        .cloned()
                        .collect();
                    Ok(hashes)
                })?
        };
        // Invalid blocks are already known/built into Casper state and should not be re-fetched
        // as unresolved dependencies.
        let invalid_block_hashes: HashSet<BlockHash> = {
            self.block_dag_storage
                .get_representation()?
                .invalid_blocks_map()?
                .into_keys()
                .collect()
        };

        let deps_in_buffer_all: Vec<BlockHash> = {
            all_deps
                .iter()
                .filter_map(|dep| {
                    let block_hash_serde = BlockHashSerde(dep.clone());
                    if self.casper_buffer.contains(&block_hash_serde)
                        || self.casper_buffer.is_pendant(&block_hash_serde)
                    {
                        Some(dep.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };

        let deps_in_dag: Vec<BlockHash> = all_deps
            .iter()
            .filter_map(|dep| {
                if casper.dag_contains(dep) {
                    Some(dep.clone())
                } else {
                    None
                }
            })
            .collect();

        let deps_in_eq_tracker: Vec<BlockHash> = all_deps
            .iter()
            .filter(|&dep| equivocation_hashes.contains(dep))
            .cloned()
            .collect();
        let deps_in_invalid_set: Vec<BlockHash> = all_deps
            .iter()
            .filter(|&dep| invalid_block_hashes.contains(dep))
            .cloned()
            .collect();

        let mut deps_validated: Vec<BlockHash> = deps_in_dag.clone();
        deps_validated.extend(deps_in_eq_tracker.iter().cloned());
        deps_validated.extend(deps_in_invalid_set.iter().cloned());

        // If a dependency is already validated, it should not be treated as a blocking
        // buffer dependency even if stale buffer relations still exist for that hash.
        let deps_in_buffer: Vec<BlockHash> = deps_in_buffer_all
            .iter()
            .filter(|dep| !deps_validated.contains(dep))
            .cloned()
            .collect();

        let deps_to_fetch: Vec<BlockHash> = all_deps
            .iter()
            .filter(|&dep| !deps_in_buffer.contains(dep))
            .filter(|&dep| !deps_validated.contains(dep))
            .cloned()
            .collect();

        let ready = deps_to_fetch.is_empty() && deps_in_buffer.is_empty();

        if !ready {
            tracing::debug!(
                "Block {} waiting on missing dependencies. To fetch: {}. In buffer: {}. Validated: {}.",
                PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), true),
                PrettyPrinter::build_string_hashes(
                    &deps_to_fetch
                        .iter()
                        .map(|h| h.as_ref().to_vec())
                        .collect::<Vec<_>>()
                ),
                PrettyPrinter::build_string_hashes(
                    &deps_in_buffer
                        .iter()
                        .map(|h| h.as_ref().to_vec())
                        .collect::<Vec<_>>()
                ),
                PrettyPrinter::build_string_hashes(
                    &deps_validated
                        .iter()
                        .map(|h| h.as_ref().to_vec())
                        .collect::<Vec<_>>()
                )
            );
        }

        Ok((
            ready,
            deps_to_fetch.into_iter().collect::<HashSet<BlockHash>>(),
            deps_in_buffer.into_iter().collect::<HashSet<BlockHash>>(),
        ))
    }

    /// Equivalent to Scala's: commitToBuffer = (b: BlockMessage, deps: Option[Set[BlockHash]]) => { ... }
    pub async fn commit_to_buffer(
        &self,
        block: &BlockMessage,
        deps: Option<HashSet<BlockHash>>,
    ) -> Result<(), CasperError> {
        match deps {
            None => {
                let block_hash_serde = BlockHashSerde(block.block_hash.clone());
                self.casper_buffer.put_pendant(block_hash_serde)?;
            }
            Some(dependencies) => {
                let block_hash_serde = BlockHashSerde(block.block_hash.clone());
                dependencies.iter().try_for_each(|dep| {
                    let dep_serde = BlockHashSerde(dep.clone());
                    self.casper_buffer
                        .add_relation(dep_serde, block_hash_serde.clone())
                })?;
            }
        }

        Ok(())
    }

    /// Equivalent to Scala's: removeFromBuffer = (b: BlockMessage) => casperBuffer.remove(b.blockHash)
    pub async fn remove_from_buffer(&self, block: &BlockMessage) -> Result<(), CasperError> {
        let block_hash_serde = BlockHashSerde(block.block_hash.clone());
        self.casper_buffer.remove(block_hash_serde)?;
        self.clear_missing_dependency_attempts(&block.block_hash)?;
        self.clear_missing_dependency_quarantine(&block.block_hash)?;

        Ok(())
    }

    fn sweep_orphaned_missing_dependency_attempts(&self) -> Result<(), CasperError> {
        let to_clear: Vec<BlockHash> = {
            let attempts = self.missing_dependency_attempts.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_attempts lock".to_string(),
                )
            })?;

            attempts
                .keys()
                .filter_map(|block_hash| {
                    let block_hash_serde = BlockHashSerde(block_hash.clone());
                    let is_active = self.casper_buffer.contains(&block_hash_serde)
                        || self.casper_buffer.is_pendant(&block_hash_serde);

                    if is_active {
                        None
                    } else {
                        Some(block_hash.clone())
                    }
                })
                .collect()
        };

        if to_clear.is_empty() {
            return Ok(());
        }

        let mut attempts = self.missing_dependency_attempts.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire missing_dependency_attempts lock".to_string(),
            )
        })?;

        for block_hash in to_clear {
            attempts.remove(&block_hash);
        }

        Ok(())
    }

    fn sweep_orphaned_missing_dependency_quarantine(&self) -> Result<(), CasperError> {
        let to_clear: Vec<BlockHash> = {
            let quarantine: Vec<BlockHash> = self
                .missing_dependency_quarantine_until
                .lock()
                .map_err(|_| {
                    CasperError::RuntimeError(
                        "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                    )
                })?
                .keys()
                .cloned()
                .collect();

            quarantine
                .into_iter()
                .filter_map(|block_hash| {
                    let block_hash_serde = BlockHashSerde(block_hash.clone());
                    let is_active = self.casper_buffer.contains(&block_hash_serde)
                        || self.casper_buffer.is_pendant(&block_hash_serde);

                    if is_active {
                        None
                    } else {
                        Some(block_hash)
                    }
                })
                .collect()
        };

        if to_clear.is_empty() {
            return Ok(());
        }

        let mut quarantine = self
            .missing_dependency_quarantine_until
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                )
            })?;

        for block_hash in to_clear {
            quarantine.remove(&block_hash);
        }

        Ok(())
    }

    fn register_missing_dependency_attempt(
        &self,
        block_hash: &BlockHash,
    ) -> Result<bool, CasperError> {
        let mut attempts = self.missing_dependency_attempts.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire missing_dependency_attempts lock".to_string(),
            )
        })?;
        let next = attempts.entry(block_hash.clone()).or_insert(0);
        *next = next.saturating_add(1);
        Ok(*next >= missing_dependency_attempts_max())
    }

    fn clear_missing_dependency_attempts(&self, block_hash: &BlockHash) -> Result<(), CasperError> {
        let mut attempts = self.missing_dependency_attempts.lock().map_err(|_| {
            CasperError::RuntimeError(
                "Failed to acquire missing_dependency_attempts lock".to_string(),
            )
        })?;
        attempts.remove(block_hash);
        Ok(())
    }

    fn clear_missing_dependency_quarantine(
        &self,
        block_hash: &BlockHash,
    ) -> Result<(), CasperError> {
        let mut quarantine = self
            .missing_dependency_quarantine_until
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                )
            })?;
        quarantine.remove(block_hash);
        Ok(())
    }

    fn mark_missing_dependency_quarantine(
        &self,
        block_hash: &BlockHash,
    ) -> Result<(), CasperError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let until = now_ms.saturating_add(missing_dependency_quarantine_ms());
        let mut quarantine = self
            .missing_dependency_quarantine_until
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                )
            })?;
        quarantine.insert(block_hash.clone(), until);
        Ok(())
    }

    fn is_missing_dependency_quarantined(
        &self,
        block_hash: &BlockHash,
    ) -> Result<bool, CasperError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let quarantine = self
            .missing_dependency_quarantine_until
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                )
            })?;
        Ok(quarantine
            .get(block_hash)
            .copied()
            .is_some_and(|until| now_ms < until))
    }

    fn sweep_expired_missing_dependency_quarantine(&self) -> Result<(), CasperError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut quarantine = self
            .missing_dependency_quarantine_until
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "Failed to acquire missing_dependency_quarantine_until lock".to_string(),
                )
            })?;
        quarantine.retain(|_, until| *until > now_ms);
        Ok(())
    }

    pub fn was_requested_as_dependency(&self, hash: &BlockHash) -> Result<bool, CasperError> {
        self.block_retriever.was_requested_as_dependency(hash)
    }

    /// Admit an arriving block as settled history if [`admit_as_settled`]'s
    /// conditions hold: inserted into the DAG hash-checked and unjudged, the
    /// same treatment LFS restore gave every block it downloaded. Returns
    /// whether the block was admitted; a `false` sends it down the ordinary
    /// judged path.
    ///
    /// Insertion cannot touch consensus state: `InsertMode::SettledHistory`
    /// leaves latest messages exactly as they were, and every verdict channel
    /// is untouched because the block never enters validation.
    pub async fn try_admit_settled(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<bool, CasperError> {
        if !self.take_settled_solicitation(&block.block_hash) {
            return Ok(false);
        }
        let approved_block_number = casper
            .get_approved_block()
            .map(|approved| proto_util::block_number(approved))?;
        let admitted_so_far = self.settled_admissions.load(Ordering::Relaxed);
        let seq_below_senders_latest = {
            let representation = self.block_dag_storage.get_representation()?;
            match representation.latest_message_hash(&block.sender) {
                Some(latest_hash) => match representation.lookup(&latest_hash)? {
                    Some(latest_meta) => block.seq_num < latest_meta.sequence_number,
                    None => false,
                },
                None => false,
            }
        };
        if !admit_as_settled(
            proto_util::block_number(block),
            approved_block_number,
            true,
            admitted_so_far < SETTLED_ADMISSION_BUDGET,
            seq_below_senders_latest,
        ) {
            return Ok(false);
        }

        self.block_dag_storage.insert(
            block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::SettledHistory,
        )?;
        let admitted = self.settled_admissions.fetch_add(1, Ordering::Relaxed) + 1;
        if admitted == SETTLED_ADMISSION_BUDGET / 2 {
            tracing::warn!(
                admitted,
                budget = SETTLED_ADMISSION_BUDGET,
                "Settled-history admissions at half budget; a healthy join needs single \
                 digits — investigate what keeps citing unheld settled blocks"
            );
        }
        tracing::info!(
            block = %PrettyPrinter::build_string_bytes(&block.block_hash),
            block_number = proto_util::block_number(block),
            anchor_number = approved_block_number,
            admitted,
            "Admitted solicited block as settled history (below this node's sync anchor)"
        );
        // An admitted block is a legal parent, and a parent's state is read by
        // its children's replay. Fetch its declared roots now, eagerly: a child
        // validating before they land defers on AwaitingState and retries —
        // the fetch is already in flight either way.
        self.request_state_root(&Blake2b256Hash::from_bytes_prost(
            &block.body.state.post_state_hash,
        ));
        self.request_state_root(&Blake2b256Hash::from_bytes_prost(
            &block.body.state.pre_state_hash,
        ));
        self.remove_from_buffer(block).await?;
        self.ack_processed(block).await?;
        Ok(true)
    }

    /// Equivalent to Scala's: requestMissingDependencies = (deps: Set[BlockHash]) => { ... }
    pub async fn request_missing_dependencies(
        &self,
        deps: &HashSet<BlockHash>,
    ) -> Result<(), CasperError> {
        for dep in deps {
            self.block_retriever
                .admit_hash(
                    dep.clone(),
                    None,
                    AdmitHashReason::MissingDependencyRequested,
                )
                .await?;
        }

        Ok(())
    }

    /// Recovery helper for deadlock scenarios where dependencies remain in CasperBuffer
    /// but there are no newly discovered hashes to fetch.
    pub async fn recover_stale_buffer_dependencies(
        &self,
        deps: &HashSet<BlockHash>,
    ) -> Result<(), CasperError> {
        for dep in deps {
            self.block_retriever.recover_dependency(dep.clone()).await?;
        }

        Ok(())
    }

    /// Equivalent to Scala's: validateBlock = (c: Casper[F], s: CasperSnapshot[F], b: BlockMessage) => c.validate(b, s)
    pub async fn validate_block(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        snapshot: &mut CasperSnapshot,
        block: &BlockMessage,
    ) -> Result<ValidBlockProcessing, CasperError> {
        casper.validate(block, snapshot).await
    }

    /// Equivalent to Scala's: ackProcessed = (b: BlockMessage) => BlockRetriever[F].ackInCasper(b.blockHash)
    pub async fn ack_processed(&self, block: &BlockMessage) -> Result<(), CasperError> {
        self.block_retriever
            .ack_in_casper(block.block_hash.clone())
            .await?;

        Ok(())
    }

    /// Equivalent to Scala's: effectsForInvalidBlock = (c: Casper[F], b: BlockMessage, r: InvalidBlock, s: CasperSnapshot[F]) => { ... }
    pub async fn effects_for_invalid_block(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
        invalid_block: &InvalidBlock,
        snapshot: &CasperSnapshot,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        let dag = casper.handle_invalid_block(block, invalid_block, &snapshot.dag)?;

        // Equivalent to Scala's: CommUtil[F].sendBlockHash(b.blockHash, b.sender)
        if let Err(err) = self
            .transport
            .send_block_hash(
                &self.connections_cell,
                &self.conf,
                &block.block_hash,
                &block.sender,
            )
            .await
        {
            tracing::warn!(
                "Failed to send block hash {} to sender during invalid-block effects: {}",
                PrettyPrinter::build_string_bytes(&block.block_hash),
                err
            );
        }

        Ok(dag)
    }

    /// Equivalent to Scala's: effectsForValidBlock = (c: Casper[F], b: BlockMessage) => { ... }
    pub async fn effects_for_valid_block(
        &self,
        casper: Arc<dyn Casper + Send + Sync + 'static>,
        block: &BlockMessage,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        let dag = { casper.handle_valid_block(block).await? };

        // Equivalent to Scala's: CommUtil[F].sendBlockHash(b.blockHash, b.sender)
        if let Err(err) = self
            .transport
            .send_block_hash(
                &self.connections_cell,
                &self.conf,
                &block.block_hash,
                &block.sender,
            )
            .await
        {
            tracing::warn!(
                "Failed to send block hash {} to sender during valid-block effects: {}",
                PrettyPrinter::build_string_bytes(&block.block_hash),
                err
            );
        }

        Ok(dag)
    }
}

/// Constructor function equivalent to Scala's companion object apply method
/// Creates unified dependencies and BlockProcessor
pub fn new_block_processor<T: TransportLayer + Send + Sync>(
    block_store: KeyValueBlockStore,
    casper_buffer: CasperBufferKeyValueStorage,
    block_dag_storage: BlockDagKeyValueStorage,
    block_retriever: BlockRetriever<T>,
    transport: Arc<T>,
    connections_cell: ConnectionsCell,
    conf: RPConf,
    state_root_fetch_tx: Option<mpsc::Sender<Blake2b256Hash>>,
) -> BlockProcessor<T> {
    let dependencies = BlockProcessorDependencies::new(
        block_store,
        casper_buffer,
        block_dag_storage,
        block_retriever,
        transport,
        connections_cell,
        conf,
        state_root_fetch_tx,
    );

    BlockProcessor::new(dependencies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::block_status::ValidBlock;

    /// A block validation could not judge must not be cleaned up like one it
    /// did. Settling it drops it from the buffer, and because an invalid block
    /// counts as a satisfied dependency, the next block in line then becomes
    /// "ready" and is mis-handled the same way — the gap is never fetched and
    /// the node never catches up.
    #[test]
    fn an_undecidable_block_waits_for_the_block_it_named() {
        let missing = BlockHash::from(b"the-block-we-lack".to_vec());

        assert_eq!(
            post_validation(&Either::Left(BlockError::Undecidable(missing.clone()))),
            PostValidation::AwaitingBlock(missing),
            "an undecidable block must stay buffered against the block it needs"
        );
    }

    /// Deferral is only honest for a node that actually has a hole in its
    /// history. A node built from genesis holds a complete spine, so a block it
    /// cannot find is corruption, and it must still judge — otherwise any
    /// crafted block that induces the error would buy its proposer a permanent
    /// non-verdict: never judged, never invalid, no evidence, sitting in the
    /// buffer while every honest node judges it normally.
    ///
    /// A node restored from a sync anchor is truncated for the life of its data
    /// directory: `last_approved_block` is written once, at genesis ceremony or
    /// at restore, and never advances. Its anchor height is therefore a durable
    /// statement that history below it will never arrive.
    #[test]
    fn only_a_node_with_a_hole_in_its_history_may_defer() {
        let missing = BlockHash::from(b"below-my-anchor".to_vec());
        let undecidable = || Either::Left(BlockError::Undecidable(missing.clone()));

        assert_eq!(
            guard_deferral(undecidable(), 87),
            Either::Left(BlockError::Undecidable(missing.clone())),
            "a node restored at an anchor genuinely cannot judge below it"
        );

        assert!(
            matches!(
                guard_deferral(undecidable(), 0),
                Either::Left(BlockError::BlockException(CasperError::BlockNotHeld(_)))
            ),
            "a genesis-rooted node has the whole spine, so a missing block is corruption \
             and must be judged — deferring here is an escape hatch for crafted blocks"
        );
    }

    /// Settled-history admission is the LFS door opened at runtime. A restored
    /// node judges old blocks with checks that assume dependency-ordered
    /// insertion — an assumption its own restore already broke for 298 blocks —
    /// so a straggler from the same settled region must come through the same
    /// door those 298 did: hash-checked, inserted, never judged. Each condition
    /// closes a distinct attack:
    ///
    ///   - only a truncated node (a genesis-rooted node judges everything, so
    ///     no crafted block can buy an unjudged admission there);
    ///   - only at-or-below the anchor (live consensus is always judged);
    ///   - only when solicited by a bonded validator's signature-checked block
    ///     (an unbonded attacker's citations open nothing);
    ///   - only within budget (a staked attacker buys bounded, alarmed storage,
    ///     never unbounded growth — past the budget the node degrades to
    ///     today's deferral, loudly);
    ///   - only seq-strictly-below the sender's latest message (settled
    ///     history predates the anchor's justification frontier; a higher
    ///     seq is live-chain material wearing a sub-anchor height).
    #[test]
    fn settled_history_admission_has_five_conditions() {
        assert!(
            admit_as_settled(9, 87, true, true, true),
            "a below-anchor block solicited by a bonded citer on a truncated node is settled history"
        );
        assert!(
            admit_as_settled(87, 87, true, true, true),
            "the anchor's own height is inside the settled cut"
        );
        assert!(
            !admit_as_settled(88, 87, true, true, true),
            "above the anchor is live consensus and must be judged"
        );
        assert!(
            !admit_as_settled(9, 0, true, true, true),
            "a genesis-rooted node judges everything — same discriminator as guard_deferral"
        );
        assert!(
            !admit_as_settled(9, 87, false, true, true),
            "a citation from an unbonded sender opens no door"
        );
        assert!(
            !admit_as_settled(9, 87, true, false, true),
            "budget exhausted falls back to deferral, never silent growth"
        );
        assert!(
            !admit_as_settled(9, 87, true, true, false),
            "a seq at-or-above the sender's latest message is not settled history"
        );
    }

    /// Every actual verdict — valid, invalid, or a genuine storage fault — is
    /// settled. Only the absence of a verdict waits.
    #[test]
    fn every_verdict_settles() {
        for status in [
            Either::Right(ValidBlock::Valid),
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidTransaction)),
            Either::Left(BlockError::BlockException(CasperError::RuntimeError(
                "disk".into(),
            ))),
            Either::Left(BlockError::MissingBlocks),
        ] {
            assert_eq!(
                post_validation(&status),
                PostValidation::Settled,
                "{status:?} is a verdict and must be settled, not retried"
            );
        }
    }
}
