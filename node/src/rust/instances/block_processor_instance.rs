// See node/src/main/scala/coop/rchain/node/instances/BlockProcessorInstance.scala

#[cfg(all(target_os = "linux", target_env = "gnu"))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use casper::rust::blocks::block_processing_queue::{
    BlockAdmissionFailure, BlockProcessingQueueItem, BlockProcessingQueueReceiver,
    BlockProcessingQueueSender,
};
use casper::rust::blocks::block_processor::BlockProcessor;
use casper::rust::casper::MultiParentCasper;
use casper::rust::errors::CasperError;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use casper::rust::metrics_constants::ALLOCATOR_TRIM_TOTAL_METRIC;
use casper::rust::metrics_constants::{
    BLOCKS_IN_PROCESSING_SIZE_METRIC, BLOCK_PROCESSING_ACTIVE_METRIC,
    BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC, BLOCK_PROCESSOR_METRICS_SOURCE, PROCESS_RSS_KB_METRIC,
};
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::{ProposeFunction, ProposeRequestKind, ValidBlockProcessing};
use comm::rust::transport::transport_layer::TransportLayer;
use dashmap::DashSet;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;
use tokio::sync::mpsc;

const MAX_PARALLEL_BLOCKS_DEFAULT: usize = 2;
const MAX_PARALLEL_BLOCKS_ENV: &str = "F1R3_MAX_PARALLEL_BLOCKS";
const BLOCK_PROCESSING_RESULT_QUEUE_CAPACITY: usize = 128;
const MALLOC_TRIM_EVERY_BLOCKS_DEFAULT: usize = 1;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
static BLOCKS_SINCE_ALLOCATOR_TRIM: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(target_os = "linux", target_env = "gnu"))]
static MALLOC_TRIM_EVERY_BLOCKS: OnceLock<usize> = OnceLock::new();
static TRIGGER_PROPOSE_AFTER_BLOCK_PROCESSING: OnceLock<bool> = OnceLock::new();

fn configured_malloc_trim_every_blocks(value: Option<&str>) -> usize {
    value
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(MALLOC_TRIM_EVERY_BLOCKS_DEFAULT)
}

fn next_trim_counter(current: usize, interval: usize) -> (usize, bool) {
    if interval == 0 {
        (current, false)
    } else if current >= interval - 1 {
        (0, true)
    } else {
        (current + 1, false)
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn malloc_trim_every_blocks() -> usize {
    *MALLOC_TRIM_EVERY_BLOCKS.get_or_init(|| {
        configured_malloc_trim_every_blocks(
            std::env::var("F1R3_MALLOC_TRIM_EVERY_BLOCKS")
                .ok()
                .as_deref(),
        )
    })
}

fn configured_max_parallel_blocks(value: Option<&str>) -> usize {
    value
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .map(|v| v.min(tokio::sync::Semaphore::MAX_PERMITS))
        .unwrap_or(MAX_PARALLEL_BLOCKS_DEFAULT)
}

fn max_parallel_blocks() -> usize {
    configured_max_parallel_blocks(std::env::var(MAX_PARALLEL_BLOCKS_ENV).ok().as_deref())
}

fn trigger_propose_after_block_processing_enabled() -> bool {
    *TRIGGER_PROPOSE_AFTER_BLOCK_PROCESSING.get_or_init(|| {
        std::env::var("F1R3_TRIGGER_PROPOSE_AFTER_BLOCK_PROCESSING")
            .ok()
            .map(|v| {
                let normalized = v.trim().to_ascii_lowercase();
                normalized == "1" || normalized == "true" || normalized == "yes"
            })
            .unwrap_or(false)
    })
}

fn is_finalized_floor_validator(validators: &[Validator], validator: &Validator) -> bool {
    validators.contains(validator)
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn maybe_trim_allocator_after_block() {
    let interval = malloc_trim_every_blocks();
    if interval == 0 {
        return;
    }

    let mut current = BLOCKS_SINCE_ALLOCATOR_TRIM.load(Ordering::Relaxed);
    let should_trim = loop {
        let (next, should_trim) = next_trim_counter(current, interval);
        match BLOCKS_SINCE_ALLOCATOR_TRIM.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break should_trim,
            Err(observed) => current = observed,
        }
    };
    if should_trim {
        RuntimeManager::trim_allocator();
        metrics::counter!(ALLOCATOR_TRIM_TOTAL_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .increment(1);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn maybe_trim_allocator_after_block() {}

struct BlockProcessingHeapBoundary;

impl Drop for BlockProcessingHeapBoundary {
    fn drop(&mut self) { maybe_trim_allocator_after_block(); }
}

/// Ensures the in-flight marker is always cleared, even on early-return or
/// panic.
struct InFlightBlockGuard {
    blocks_in_processing: Arc<DashSet<BlockHash>>,
    hash: BlockHash,
}

impl InFlightBlockGuard {
    fn new(blocks_in_processing: Arc<DashSet<BlockHash>>, hash: BlockHash) -> Self {
        Self {
            blocks_in_processing,
            hash,
        }
    }
}

impl Drop for InFlightBlockGuard {
    fn drop(&mut self) { self.blocks_in_processing.remove(&self.hash); }
}

struct ActiveBlockProcessingGuard;

impl ActiveBlockProcessingGuard {
    fn new() -> Self {
        metrics::gauge!(
            BLOCK_PROCESSING_ACTIVE_METRIC,
            "source" => BLOCK_PROCESSOR_METRICS_SOURCE
        )
        .increment(1.0);
        Self
    }
}

impl Drop for ActiveBlockProcessingGuard {
    fn drop(&mut self) {
        metrics::gauge!(
            BLOCK_PROCESSING_ACTIVE_METRIC,
            "source" => BLOCK_PROCESSOR_METRICS_SOURCE
        )
        .decrement(1.0);
    }
}

/// Configuration for BlockProcessorInstance
pub struct BlockProcessorInstance<T: TransportLayer + Send + Sync + 'static> {
    pub blocks_queue_rx: BlockProcessingQueueReceiver,

    pub block_queue_tx: BlockProcessingQueueSender,

    pub block_processor: Arc<BlockProcessor<T>>,

    pub blocks_in_processing: Arc<DashSet<BlockHash>>,

    pub trigger_propose_f: Option<Arc<ProposeFunction>>,

    pub max_parallel_blocks: usize,
}

impl<T: TransportLayer + Send + Sync + 'static> BlockProcessorInstance<T> {
    pub fn new(
        (blocks_queue_rx, block_queue_tx): (
            BlockProcessingQueueReceiver,
            BlockProcessingQueueSender,
        ),
        block_processor: Arc<BlockProcessor<T>>,
        blocks_in_processing: Arc<DashSet<BlockHash>>,
        trigger_propose_f: Option<Arc<ProposeFunction>>,
    ) -> Self {
        Self {
            blocks_queue_rx,
            block_queue_tx,
            block_processor,
            blocks_in_processing,
            trigger_propose_f,
            max_parallel_blocks: max_parallel_blocks(),
        }
    }

    /// Create and start the block processor stream
    /// Returns a handle that can be used to await the processing task
    ///
    /// This is equivalent to Scala's `BlockProcessorInstance.create` method.
    /// It processes blocks with bounded parallelism.
    ///
    /// # Arguments
    ///
    /// * `blocks_queue_tx` - Sender to enqueue blocks for processing (for
    ///   re-enqueuing buffer pendants)
    pub fn create(self) -> Result<mpsc::Receiver<ValidBlockProcessing>, CasperError> {
        let (result_tx, result_rx) = mpsc::channel(BLOCK_PROCESSING_RESULT_QUEUE_CAPACITY);

        tokio::spawn(async move {
            let Self {
                mut blocks_queue_rx,
                block_queue_tx,
                block_processor,
                blocks_in_processing,
                trigger_propose_f,
                max_parallel_blocks,
            } = self;

            tracing::info!(max_parallel_blocks, "Starting bounded block processing");
            metrics::gauge!(
                BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC,
                "source" => BLOCK_PROCESSOR_METRICS_SOURCE
            )
            .set(max_parallel_blocks as f64);
            let semaphore = Arc::new(tokio::sync::Semaphore::new(max_parallel_blocks));

            loop {
                let permit = semaphore.clone().acquire_owned().await.unwrap();
                let Some(BlockProcessingQueueItem {
                    casper,
                    block,
                    reservation: admission_reservation,
                }) = blocks_queue_rx.recv().await
                else {
                    break;
                };
                block_queue_tx.record_dequeue();
                let block_processor = block_processor.clone();
                let blocks_in_processing = blocks_in_processing.clone();
                let trigger_propose_f = trigger_propose_f.clone();
                let block_queue_tx = block_queue_tx.clone();
                let casper = casper.clone();
                let result_tx = result_tx.clone();

                // Spawn task to process the block
                tokio::spawn(async move {
                    let _heap_boundary = BlockProcessingHeapBoundary;
                    let _active_guard = ActiveBlockProcessingGuard::new();
                    let block_str = PrettyPrinter::build_string_bytes(&block.block_hash);
                    let block_hash = block.block_hash.clone();
                    blocks_in_processing.insert(block_hash.clone());

                    let in_flight_guard =
                        InFlightBlockGuard::new(blocks_in_processing.clone(), block_hash);

                    // Process the block with all its validation steps
                    let result =
                        process_block_with_steps(block_processor.clone(), casper.clone(), block)
                            .await;

                    match result {
                        Ok(res) => {
                            tracing::info!("Block {} processing finished.", block_str);
                            match result_tx.send(res).await {
                                Ok(_) => {}
                                Err(err) => {
                                    tracing::error!(error = %err, "block processing result send failed")
                                }
                            }
                        }
                        Err(e) => match &e {
                            CasperError::Other(msg) if msg == "Missing dependencies" => {
                                tracing::warn!(
                                    "Block {} delayed: missing dependencies.",
                                    block_str
                                );
                            }
                            _ => {
                                tracing::error!(block = %block_str, error = %e, "block processing failed");
                            }
                        },
                    }

                    // Release in-flight marker before scanning dependency-free pendants.
                    // This avoids suppressing re-enqueue when another task resolves a dependency
                    // while this task is still in post-processing.
                    drop(in_flight_guard);
                    drop(admission_reservation);

                    // Step 6 (from Scala): Get dependency-free blocks from buffer and enqueue them
                    // Equivalent to: c.getDependencyFreeFromBuffer
                    // In Scala, if this fails, the stream short-circuits and triggerProposeF won't
                    // be called
                    let dependency_scan_guard = block_queue_tx.acquire_dependency_scan().await;
                    match casper.get_dependency_free_hashes_from_buffer() {
                        Ok(buffer_pendant_hashes) => {
                            if !buffer_pendant_hashes.is_empty() {
                                tracing::info!(
                                    count = buffer_pendant_hashes.len(),
                                    "Dependency-free pendants after processing {}",
                                    block_str,
                                );
                            }

                            // Enqueue pendants if we can mark them as queued/in-processing first.
                            for pendant_hash in buffer_pendant_hashes {
                                if blocks_in_processing.insert(pendant_hash.clone()) {
                                    let pendant = match casper.block_store().get(&pendant_hash) {
                                        Ok(Some(block)) => block,
                                        Ok(None) => {
                                            blocks_in_processing.remove(&pendant_hash);
                                            continue;
                                        }
                                        Err(error) => {
                                            blocks_in_processing.remove(&pendant_hash);
                                            tracing::error!(
                                                error = %error,
                                                "Dependency-free pendant load failed"
                                            );
                                            continue;
                                        }
                                    };
                                    match block_queue_tx.try_enqueue(casper.clone(), pendant) {
                                        Ok(()) => tracing::info!(
                                            "Enqueued dependency-free pendant {}",
                                            PrettyPrinter::build_string_bytes(&pendant_hash)
                                        ),
                                        Err(error)
                                            if error.failure
                                                == BlockAdmissionFailure::CountCapacity =>
                                        {
                                            blocks_in_processing.remove(&pendant_hash);
                                            tracing::info!(
                                                error = %error,
                                                "Deferred dependency-free pendant {}",
                                                PrettyPrinter::build_string_bytes(&pendant_hash)
                                            );
                                            break;
                                        }
                                        Err(error) if error.failure.is_temporary() => {
                                            blocks_in_processing.remove(&pendant_hash);
                                            tracing::info!(
                                                error = %error,
                                                "Deferred dependency-free pendant {}",
                                                PrettyPrinter::build_string_bytes(&pendant_hash)
                                            );
                                        }
                                        Err(error) => {
                                            blocks_in_processing.remove(&pendant_hash);
                                            tracing::error!(
                                                error = %error,
                                                "Dependency-free pendant admission failed"
                                            );
                                        }
                                    }
                                } else {
                                    tracing::info!(
                                        "Skipping dependency-free pendant {} enqueue because it \
                                         is already marked in-flight",
                                        PrettyPrinter::build_string_bytes(&pendant_hash)
                                    );
                                }
                            }

                            drop(dependency_scan_guard);

                            // Only call trigger_propose if get_dependency_free_from_buffer
                            // succeeded and this path is explicitly
                            // enabled. Heartbeat proposer is the
                            // default liveness path to avoid propose storms under heavy replay.
                            if trigger_propose_after_block_processing_enabled() {
                                if let Some(trigger_propose) = trigger_propose_f {
                                    // Skip trigger if local validator is not currently bonded.
                                    // This avoids repeated ReadOnlyMode propose attempts on
                                    // non-bonded nodes.
                                    let is_bonded_validator =
                                        if let Some(validator) = casper.get_validator() {
                                            match casper.get_snapshot().await {
                                                Ok(snapshot) => is_finalized_floor_validator(
                                                    &snapshot.finalized_floor_validators(),
                                                    &validator.public_key.bytes,
                                                ),
                                                Err(err) => {
                                                    tracing::warn!(
                                                        "Failed to get Casper snapshot for \
                                                         trigger-propose bond check: {}",
                                                        err
                                                    );
                                                    false
                                                }
                                            }
                                        } else {
                                            false
                                        };

                                    if is_bonded_validator {
                                        match trigger_propose(ProposeRequestKind::PendingDeploy)
                                            .await
                                        {
                                            Ok(_) => {}
                                            Err(err) => {
                                                tracing::error!(error = %err, "propose trigger after block processing failed")
                                            }
                                        }
                                    } else {
                                        tracing::debug!(
                                            "Skipping trigger propose after block processing: \
                                             validator is not bonded"
                                        );
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            tracing::error!(error = %err, "dependency-free block buffer retrieval failed; skipping propose trigger");
                            // Don't call trigger_propose if get_dependency_free_from_buffer failed
                        }
                    }

                    metrics::gauge!(
                        BLOCKS_IN_PROCESSING_SIZE_METRIC,
                        "source" => BLOCK_PROCESSOR_METRICS_SOURCE
                    )
                    .set(blocks_in_processing.len() as f64);
                    if let Some(rss_kb) =
                        casper::rust::util::rholang::mem_profiler::read_vm_rss_kb_always()
                    {
                        metrics::gauge!(
                            PROCESS_RSS_KB_METRIC,
                            "source" => BLOCK_PROCESSOR_METRICS_SOURCE
                        )
                        .set(rss_kb as f64);
                    }

                    drop(permit);
                });
            }

            tracing::info!("Block processing queue closed, stopping processor");

            Result::<(), CasperError>::Ok(())
        });

        Ok(result_rx)
    }
}

/// Process a block through all validation steps
///
/// This implements the Scala pipeline:
/// 1. checkIfOfInterest
/// 2. checkIfWellFormedAndStore
/// 3. checkDependenciesWithEffects
/// 4. validateWithEffects
/// 5. Enqueue dependency-free blocks from buffer
/// 6. Trigger propose if configured
async fn process_block_with_steps<T: TransportLayer + Send + Sync>(
    block_processor: Arc<BlockProcessor<T>>,
    casper: Arc<dyn MultiParentCasper + Send + Sync + 'static>,
    block: BlockMessage,
) -> Result<ValidBlockProcessing, CasperError> {
    let block_str = PrettyPrinter::build_string_bytes(&block.block_hash);

    // Step 1: Check if block is of interest
    // Equivalent to: blockProcessor.checkIfOfInterest(c, b)
    let is_of_interest = match block_processor.check_if_of_interest(casper.clone(), &block) {
        Ok(is_of_interest) => is_of_interest,
        Err(err) => {
            block_processor
                .ack_processed(&block)
                .await
                .map_err(|ack_err| {
                    CasperError::RuntimeError(format!(
                        "check_if_of_interest failed for {}, and cleanup failed: {}",
                        block_str, ack_err
                    ))
                })?;
            return Err(err);
        }
    };

    if !is_of_interest {
        tracing::info!("Block {} is not of interest. Dropped.", block_str);
        block_processor
            .purge_from_buffer_and_ack(&block)
            .await
            .map_err(|err| {
                CasperError::RuntimeError(format!(
                    "Block {} was not of interest, and purge+cleanup failed: {}",
                    block_str, err
                ))
            })?;
        return Err(CasperError::Other("Block not of interest".to_string()));
    }

    // Step 2: Check if well-formed and store
    // Equivalent to: blockProcessor.checkIfWellFormedAndStore(b)
    let is_well_formed = match block_processor.check_if_well_formed_and_store(&block).await {
        Ok(is_well_formed) => is_well_formed,
        Err(err) => {
            block_processor
                .ack_processed(&block)
                .await
                .map_err(|ack_err| {
                    CasperError::RuntimeError(format!(
                        "check_if_well_formed_and_store failed for {}, and cleanup failed: {}",
                        block_str, ack_err
                    ))
                })?;
            return Err(err);
        }
    };

    if !is_well_formed {
        tracing::info!("Block {} is malformed. Dropped.", block_str);
        block_processor
            .purge_from_buffer_and_ack(&block)
            .await
            .map_err(|err| {
                CasperError::RuntimeError(format!(
                    "Malformed block {} purge+cleanup failed: {}",
                    block_str, err
                ))
            })?;
        return Err(CasperError::Other("Block is malformed".to_string()));
    }

    // Step 3: Log started
    tracing::info!("Block {} processing started.", block_str);

    // Step 4: Check dependencies with effects
    // Equivalent to: blockProcessor.checkDependenciesWithEffects(c, b)
    let has_dependencies = match block_processor
        .check_dependencies_with_effects(casper.clone(), &block)
        .await
    {
        Ok(has_dependencies) => has_dependencies,
        Err(err) => {
            block_processor
                .ack_processed(&block)
                .await
                .map_err(|ack_err| {
                    CasperError::RuntimeError(format!(
                        "check_dependencies_with_effects failed for {}, and cleanup failed: {}",
                        block_str, ack_err
                    ))
                })?;
            return Err(err);
        }
    };

    if !has_dependencies {
        tracing::info!("Block {} missing dependencies.", block_str);
        // `check_dependencies_with_effects` already performs ack/cleanup for this path.
        return Err(CasperError::Other("Missing dependencies".to_string()));
    }

    // Step 5: Validate block with effects
    // Equivalent to: blockProcessor.validateWithEffects(c, b, None)
    let validation_result = match block_processor
        .validate_with_effects(casper.clone(), &block, None)
        .await
    {
        Ok(validation_result) => validation_result,
        Err(err) => {
            // ensure this block is no longer tracked in the retriever even when validation
            // fails
            block_processor
                .ack_processed(&block)
                .await
                .map_err(|ack_err| {
                    CasperError::RuntimeError(format!(
                        "validate_with_effects failed for {}, and cleanup failed: {}",
                        block_str, ack_err
                    ))
                })?;
            return Err(err);
        }
    };

    tracing::info!("Block {} validated {:?}.", block_str, validation_result);

    Ok(validation_result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_block_limit_defaults_to_two() {
        assert_eq!(configured_max_parallel_blocks(None), 2);
        assert_eq!(configured_max_parallel_blocks(Some("")), 2);
        assert_eq!(configured_max_parallel_blocks(Some("0")), 2);
        assert_eq!(configured_max_parallel_blocks(Some("invalid")), 2);
    }

    #[test]
    fn parallel_block_limit_accepts_positive_values() {
        assert_eq!(configured_max_parallel_blocks(Some("1")), 1);
        assert_eq!(configured_max_parallel_blocks(Some("4")), 4);
    }

    #[test]
    fn parallel_block_limit_clamps_to_semaphore_max() {
        let max = usize::MAX.to_string();
        assert_eq!(
            configured_max_parallel_blocks(Some(&max)),
            tokio::sync::Semaphore::MAX_PERMITS
        );
    }

    #[test]
    fn allocator_trim_defaults_to_every_completed_block() {
        assert_eq!(configured_malloc_trim_every_blocks(None), 1);
        assert_eq!(configured_malloc_trim_every_blocks(Some("")), 1);
        assert_eq!(configured_malloc_trim_every_blocks(Some("invalid")), 1);
    }

    #[test]
    fn allocator_trim_interval_accepts_explicit_values() {
        assert_eq!(configured_malloc_trim_every_blocks(Some("0")), 0);
        assert_eq!(configured_malloc_trim_every_blocks(Some("8")), 8);
    }

    #[test]
    fn allocator_trim_schedule_is_bounded_and_overflow_safe() {
        assert_eq!(next_trim_counter(usize::MAX, 0), (usize::MAX, false));
        assert_eq!(next_trim_counter(0, 1), (0, true));
        assert_eq!(next_trim_counter(6, 8), (7, false));
        assert_eq!(next_trim_counter(7, 8), (0, true));
        assert_eq!(next_trim_counter(usize::MAX, 8), (0, true));
    }

    #[test]
    fn post_processing_trigger_uses_finalized_floor_membership() {
        let floor_validator = Validator::from(vec![1]);
        let head_only_validator = Validator::from(vec![2]);
        let floor = vec![floor_validator.clone()];

        assert!(is_finalized_floor_validator(&floor, &floor_validator));
        assert!(!is_finalized_floor_validator(&floor, &head_only_validator));
    }

    proptest::proptest! {
        #[test]
        fn allocator_trim_counter_never_exceeds_interval(
            current in proptest::num::usize::ANY,
            interval in 1usize..=usize::MAX,
        ) {
            let (next, should_trim) = next_trim_counter(current, interval);
            proptest::prop_assert!(next < interval);
            proptest::prop_assert_eq!(should_trim, current >= interval - 1);
            proptest::prop_assert_eq!(should_trim, next == 0 && current >= interval - 1);
        }
    }
}
