// See node/src/main/scala/coop/rchain/node/instances/BlockProcessorInstance.scala

use std::sync::Arc;

use casper::rust::blocks::block_processor::{
    BlockProcessor, ValidationFailureDisposition, MAX_BLOCKS_IN_PROCESSING,
};
use casper::rust::casper::MultiParentCasper;
use casper::rust::errors::CasperError;
use casper::rust::metrics_constants::{
    BLOCK_PROCESSING_ACTIVE_METRIC, BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC,
    BLOCK_PROCESSOR_METRICS_SOURCE,
};
use casper::rust::ValidBlockProcessing;
use comm::rust::transport::transport_layer::TransportLayer;
use dashmap::DashSet;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use tokio::sync::mpsc;

/// Pipeline width; replay itself is serialized by the runtime's ReplayLock.
const MAX_PARALLEL_BLOCKS: usize = 2;
const BLOCK_PROCESSING_RESULT_QUEUE_CAPACITY: usize = 128;

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
    pub blocks_queue_rx: mpsc::Receiver<(Arc<dyn MultiParentCasper + Send + Sync>, BlockMessage)>,

    pub block_queue_tx: mpsc::Sender<(Arc<dyn MultiParentCasper + Send + Sync>, BlockMessage)>,

    pub block_processor: Arc<BlockProcessor<T>>,

    pub blocks_in_processing: Arc<DashSet<BlockHash>>,
}

impl<T: TransportLayer + Send + Sync + 'static> BlockProcessorInstance<T> {
    pub fn new(
        (blocks_queue_rx, block_queue_tx): (
            mpsc::Receiver<(Arc<dyn MultiParentCasper + Send + Sync>, BlockMessage)>,
            mpsc::Sender<(Arc<dyn MultiParentCasper + Send + Sync>, BlockMessage)>,
        ),
        block_processor: Arc<BlockProcessor<T>>,
        blocks_in_processing: Arc<DashSet<BlockHash>>,
    ) -> Self {
        Self {
            blocks_queue_rx,
            block_queue_tx,
            block_processor,
            blocks_in_processing,
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
    pub fn create(
        self,
    ) -> Result<mpsc::Receiver<(BlockMessage, ValidBlockProcessing)>, CasperError> {
        let (result_tx, result_rx) = mpsc::channel(BLOCK_PROCESSING_RESULT_QUEUE_CAPACITY);

        tokio::spawn(async move {
            let Self {
                mut blocks_queue_rx,
                block_queue_tx,
                block_processor,
                blocks_in_processing,
            } = self;

            tracing::info!(
                max_parallel_blocks = MAX_PARALLEL_BLOCKS,
                "Starting bounded block processing"
            );
            metrics::gauge!(
                BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC,
                "source" => BLOCK_PROCESSOR_METRICS_SOURCE
            )
            .set(MAX_PARALLEL_BLOCKS as f64);
            let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_PARALLEL_BLOCKS));

            while let Some((casper, block)) = blocks_queue_rx.recv().await {
                let block_processor = block_processor.clone();
                let blocks_in_processing = blocks_in_processing.clone();
                let block_queue_tx = block_queue_tx.clone();
                let casper = casper.clone();
                let result_tx = result_tx.clone();

                let permit = semaphore.clone().acquire_owned().await.unwrap();

                // Spawn task to process the block
                tokio::spawn(async move {
                    let _active_guard = ActiveBlockProcessingGuard::new();
                    let block_str = PrettyPrinter::build_string_bytes(&block.block_hash);
                    if !blocks_in_processing.contains(&block.block_hash) {
                        // Fallback for legacy enqueue paths: mark before processing.
                        blocks_in_processing.insert(block.block_hash.clone());
                        let max_in_flight = MAX_BLOCKS_IN_PROCESSING;
                        if blocks_in_processing.len() > max_in_flight {
                            // Ensure in-flight marker is always cleared, even when ack cleanup
                            // fails.
                            blocks_in_processing.remove(&block.block_hash);
                            if let Err(err) = block_processor.ack_processed(&block).await {
                                tracing::warn!(
                                    "Dropping block {} and cleanup failed: {}",
                                    block_str,
                                    err
                                );
                            }
                            tracing::warn!(
                                "Dropping block {} because in-flight block cap {} is reached",
                                block_str,
                                max_in_flight
                            );
                            return;
                        }
                    }

                    let in_flight_guard = InFlightBlockGuard::new(
                        blocks_in_processing.clone(),
                        block.block_hash.clone(),
                    );

                    // Process the block with all its validation steps
                    let result = process_block_with_steps(
                        block_processor.clone(),
                        casper.clone(),
                        block.clone(),
                    )
                    .await;

                    match result {
                        Ok(BlockProcessOutcome::Processed(block, res)) => {
                            tracing::info!("Block {} processing finished.", block_str);
                            if let Err(err) =
                                block_processor.clear_validation_failures(&block.block_hash)
                            {
                                tracing::warn!(
                                    block = %block_str,
                                    error = %err,
                                    "failed to clear validation-failure ledger"
                                );
                            }
                            match result_tx.send((block, res)).await {
                                Ok(_) => {}
                                Err(err) => {
                                    tracing::error!(error = %err, "block processing result send failed")
                                }
                            }
                        }
                        Ok(BlockProcessOutcome::MissingDependencies) => {
                            tracing::warn!("Block {} delayed: missing dependencies.", block_str);
                        }
                        // Already logged at INFO by the pipeline; a routine
                        // drop is not a failure.
                        Ok(BlockProcessOutcome::NotOfInterest)
                        | Ok(BlockProcessOutcome::Malformed) => {}
                        Err(e) => {
                            tracing::error!(block = %block_str, error = %e, "block processing failed");
                            match block_processor.note_validation_failure(&block.block_hash) {
                                Ok(ValidationFailureDisposition::Retry) => {}
                                Ok(ValidationFailureDisposition::PurgeAndQuarantine) => {
                                    tracing::warn!(
                                        block = %block_str,
                                        "hard-failing block purged from buffer after \
                                         reaching the validation-error attempt cap"
                                    );
                                    if let Err(purge_err) =
                                        block_processor.purge_from_buffer_and_ack(&block).await
                                    {
                                        tracing::warn!(
                                            block = %block_str,
                                            error = %purge_err,
                                            "purge after validation-error cap failed"
                                        );
                                    }
                                }
                                Err(err) => {
                                    tracing::warn!(
                                        block = %block_str,
                                        error = %err,
                                        "failed to record validation failure"
                                    );
                                }
                            }
                        }
                    }

                    // Release in-flight marker before scanning dependency-free pendants.
                    // This avoids suppressing re-enqueue when another task resolves a dependency
                    // while this task is still in post-processing.
                    drop(in_flight_guard);

                    // Step 6 (from Scala): Get dependency-free blocks from buffer and enqueue them
                    // Equivalent to: c.getDependencyFreeFromBuffer
                    match casper.get_dependency_free_from_buffer() {
                        Ok(buffer_pendants) => {
                            if !buffer_pendants.is_empty() {
                                let pendant_hashes = buffer_pendants
                                    .iter()
                                    .map(|p| PrettyPrinter::build_string_bytes(&p.block_hash))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                tracing::info!(
                                    "Dependency-free pendants after processing {}: [{}]",
                                    block_str,
                                    pendant_hashes
                                );
                            }

                            // Enqueue pendants if we can mark them as queued/in-processing first.
                            for pendant in &buffer_pendants {
                                let pendant_hash = BlockHash::from(pendant.block_hash.clone());
                                if block_processor
                                    .is_validation_failure_quarantined(&pendant_hash)
                                    .unwrap_or(false)
                                {
                                    tracing::debug!(
                                        "Skipping dependency-free pendant {} during \
                                         validation-failure quarantine",
                                        PrettyPrinter::build_string_bytes(&pendant.block_hash)
                                    );
                                    continue;
                                }
                                if blocks_in_processing.insert(pendant_hash.clone()) {
                                    let max_in_flight = MAX_BLOCKS_IN_PROCESSING;
                                    if blocks_in_processing.len() > max_in_flight {
                                        blocks_in_processing.remove(&pendant_hash);
                                        tracing::warn!(
                                            "Skipping dependency-free pendant {} enqueue because \
                                             in-flight block cap {} is reached",
                                            PrettyPrinter::build_string_bytes(&pendant.block_hash),
                                            max_in_flight
                                        );
                                        continue;
                                    }
                                    if block_queue_tx
                                        .send((casper.clone(), pendant.clone()))
                                        .await
                                        .is_err()
                                    {
                                        blocks_in_processing.remove(&pendant_hash);
                                        tracing::warn!(
                                            "Dropping dependency-free pendant {} because block \
                                             queue is closed",
                                            PrettyPrinter::build_string_bytes(&pendant.block_hash)
                                        );
                                    } else {
                                        tracing::info!(
                                            "Enqueued dependency-free pendant {}",
                                            PrettyPrinter::build_string_bytes(&pendant.block_hash)
                                        );
                                    }
                                } else {
                                    tracing::info!(
                                        "Skipping dependency-free pendant {} enqueue because it \
                                         is already marked in-flight",
                                        PrettyPrinter::build_string_bytes(&pendant.block_hash)
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            tracing::error!(error = %err, "dependency-free block buffer retrieval failed");
                        }
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

/// A processing attempt's outcome. The non-`Processed` variants are normal
/// pipeline exits — a duplicate delivery, a malformed block, a block waiting
/// on its dependencies — not failures, and they must never travel the error
/// channel: an `Err` here means something actually broke.
enum BlockProcessOutcome {
    Processed(BlockMessage, ValidBlockProcessing),
    NotOfInterest,
    Malformed,
    MissingDependencies,
}

/// Process a block through all validation steps
///
/// This implements the Scala pipeline:
/// 1. checkIfOfInterest
/// 2. checkIfWellFormedAndStore
/// 3. checkDependenciesWithEffects
/// 4. validateWithEffects
/// 5. Enqueue dependency-free blocks from buffer (in the outer loop)
async fn process_block_with_steps<T: TransportLayer + Send + Sync + 'static>(
    block_processor: Arc<BlockProcessor<T>>,
    casper: Arc<dyn MultiParentCasper + Send + Sync + 'static>,
    block: BlockMessage,
) -> Result<BlockProcessOutcome, CasperError> {
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
        return Ok(BlockProcessOutcome::NotOfInterest);
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
        return Ok(BlockProcessOutcome::Malformed);
    }

    // Step 3: Log started
    tracing::info!("Block {} processing started.", block_str);

    // Settled-history door: a signature-checked block at-or-below this node's
    // sync anchor, solicited by a bonded validator's block, enters the DAG the
    // way LFS restore admitted its neighbours — hash-checked, unjudged. Judging
    // it instead runs tip-state validation checks against settled history,
    // which is how a restored joiner recorded verdicts against honest
    // validators. The outer loop's pendant scan then re-enqueues whatever was
    // deferred waiting on this block.
    match block_processor
        .try_admit_settled(casper.clone(), &block)
        .await
    {
        Ok(true) => {
            return Ok(BlockProcessOutcome::Processed(
                block,
                rspace_plus_plus::rspace::history::Either::Left(
                    casper::rust::block_status::BlockError::AdmittedSettled,
                ),
            ));
        }
        Ok(false) => {}
        Err(err) => {
            block_processor
                .ack_processed(&block)
                .await
                .map_err(|ack_err| {
                    CasperError::RuntimeError(format!(
                        "try_admit_settled failed for {}, and cleanup failed: {}",
                        block_str, ack_err
                    ))
                })?;
            return Err(err);
        }
    }

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
        return Ok(BlockProcessOutcome::MissingDependencies);
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

    Ok(BlockProcessOutcome::Processed(block, validation_result))
}

const _: () = assert!(
    MAX_PARALLEL_BLOCKS >= 1 && MAX_PARALLEL_BLOCKS <= tokio::sync::Semaphore::MAX_PERMITS,
    "parallel width must be a valid semaphore permit count"
);
