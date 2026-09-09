// See node/src/main/scala/coop/rchain/node/instances/BlockProcessorInstance.scala

#[cfg(all(target_os = "linux", target_env = "gnu"))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use casper::rust::blocks::block_processing_queue::{
    BlockProcessingIdentities, BlockProcessingQueueItem, BlockProcessingQueueReceiver,
    BlockProcessingQueueSender, RecoverySignal, RecoveryStopGuard,
};
use casper::rust::blocks::block_processor::{
    AcceptedBlockPublication, BlockProcessor, SettledAdmissionResult, StoredPublicationPreparation,
    ValidationFailureDisposition,
};
use casper::rust::casper::MultiParentCasper;
use casper::rust::engine::block_retriever::RequestTracking;
use casper::rust::errors::CasperError;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use casper::rust::metrics_constants::ALLOCATOR_TRIM_TOTAL_METRIC;
use casper::rust::metrics_constants::{
    BLOCKS_IN_PROCESSING_SIZE_METRIC, BLOCK_PROCESSING_ACTIVE_METRIC,
    BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC, BLOCK_PROCESSOR_METRICS_SOURCE, PROCESS_RSS_KB_METRIC,
};
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::{ProposeFunction, ValidBlockProcessing};
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::Validator;

mod recovery_driver;

#[cfg(test)]
mod ownership_tests;

#[cfg(test)]
mod input_closure_tests;

#[cfg(test)]
mod publication_tests;

const INPUT_CLOSURE_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

const MAX_PARALLEL_BLOCKS_DEFAULT: usize = 2;
const MAX_PARALLEL_BLOCKS_ENV: &str = "F1R3_MAX_PARALLEL_BLOCKS";
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

    pub blocks_in_processing: Arc<BlockProcessingIdentities>,

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
        blocks_in_processing: Arc<BlockProcessingIdentities>,
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

    pub fn run(self) -> impl std::future::Future<Output = Result<(), CasperError>> + Send {
        let stop = RecoveryStopGuard(self.block_queue_tx.recovery());
        async move {
            let Self {
                mut blocks_queue_rx,
                block_queue_tx,
                block_processor,
                blocks_in_processing,
                trigger_propose_f,
                max_parallel_blocks,
            } = self;
            if max_parallel_blocks == 0 {
                return Err(CasperError::RuntimeError(
                    "Block worker limit must be positive".into(),
                ));
            }
            let queue = block_queue_tx.downgrade();
            let control = block_queue_tx.recovery();
            drop(block_queue_tx);
            let mut workers = tokio::task::JoinSet::new();
            let mut services = tokio::task::JoinSet::new();
            let mut closure_probe = tokio::time::interval_at(
                tokio::time::Instant::now() + INPUT_CLOSURE_PROBE_INTERVAL,
                INPUT_CLOSURE_PROBE_INTERVAL,
            );
            closure_probe.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let (offers, receiver) = tokio::sync::watch::channel(None);
            services.spawn(recovery_driver::run(
                queue.clone(),
                block_processor.clone(),
                control.clone(),
                offers,
            ));
            services.spawn(recovery_driver::propose(receiver, trigger_propose_f));
            let _stop = stop;
            metrics::gauge!(
                BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC,
                "source" => BLOCK_PROCESSOR_METRICS_SOURCE
            )
            .set(max_parallel_blocks as f64);
            let outcome = loop {
                if blocks_queue_rx.sender_strong_count() == 0 && blocks_queue_rx.is_empty() {
                    break Ok(());
                }
                tokio::select! {
                    _ = closure_probe.tick() => {}
                    service = services.join_next() => {
                        break match service {
                            Some(Ok(result)) => result,
                            Some(Err(error)) => Err(CasperError::RuntimeError(format!("Recovery service failed: {error}"))),
                            None => Err(CasperError::RuntimeError("Recovery services lost their owner".into())),
                        };
                    }
                    worker = workers.join_next(), if !workers.is_empty() => {
                        if let Some(Err(error)) = worker {
                            break Err(CasperError::RuntimeError(format!("Block worker failed: {error}")));
                        }
                    }
                    item = blocks_queue_rx.recv(), if workers.len() < max_parallel_blocks => {
                        let Some(item) = item else { break Ok(()); };
                        queue.record_dequeue(blocks_queue_rx.len());
                        workers.spawn(process_owned_block(
                            block_processor.clone(),
                            item,
                            blocks_in_processing.clone(),
                            control.signal(),
                        ));
                    }
                }
            };
            blocks_queue_rx.close();
            control.stop();
            workers.abort_all();
            services.abort_all();
            while let Ok(item) = blocks_queue_rx.try_recv() {
                drop(item);
            }
            tokio::join!(
                async { while workers.join_next().await.is_some() {} },
                async { while services.join_next().await.is_some() {} },
            );
            outcome
        }
    }
}

async fn process_owned_block<T: TransportLayer + Send + Sync>(
    processor: Arc<BlockProcessor<T>>,
    item: BlockProcessingQueueItem,
    identities: Arc<BlockProcessingIdentities>,
    recovery: Arc<RecoverySignal>,
) {
    let _heap_boundary = BlockProcessingHeapBoundary;
    let _active_guard = ActiveBlockProcessingGuard::new();
    let hash = item.block.block_hash.clone();
    let block = PrettyPrinter::build_string_bytes(&hash);
    let mut retry_delay = std::time::Duration::from_millis(100);
    let mut accepted_publication = None;
    loop {
        if let Some(evidence) = &accepted_publication {
            match processor
                .restore_accepted_publication(item.casper.clone(), &hash, evidence)
                .await
            {
                Ok(()) => break,
                Err(error) => {
                    tracing::warn!(%block, %error, "Accepted publication repair failed; retaining worker lease")
                }
            }
            tokio::time::sleep(retry_delay).await;
            retry_delay = (retry_delay * 2).min(std::time::Duration::from_secs(1));
            continue;
        }
        let result = process_block_with_steps(
            &processor,
            &item.casper,
            &item.block,
            &mut accepted_publication,
        )
        .await;
        let allow_durable_exit = match result {
            Ok(BlockProcessOutcome::Processed(status)) => {
                tracing::info!(%block, ?status, "Block processing finished");
                if let Err(error) = processor.clear_validation_failures(&hash) {
                    tracing::warn!(%block, %error, "Validation-failure ledger cleanup failed");
                }
                break;
            }
            Ok(BlockProcessOutcome::MissingDependencies) => {
                tracing::warn!(%block, "Block delayed by missing dependencies");
                true
            }
            Ok(BlockProcessOutcome::Quarantined) => true,
            Ok(
                BlockProcessOutcome::NotOfInterest
                | BlockProcessOutcome::Malformed
                | BlockProcessOutcome::DuplicateDelivery,
            ) => break,
            Err(BlockProcessFailure::Local(error)) => {
                tracing::error!(%block, %error, "Local block processing failed before validation");
                false
            }
            Err(BlockProcessFailure::Validation(error)) => {
                tracing::error!(%block, %error, "Block processing failed");
                match processor.note_validation_failure(&hash) {
                    Ok(ValidationFailureDisposition::Retry) => {}
                    Ok(ValidationFailureDisposition::RetainAndQuarantine) => {
                        tracing::warn!(%block, "Block and dependencies retained during validation quarantine");
                    }
                    Err(error) => {
                        tracing::warn!(%block, %error, "Validation failure recording failed")
                    }
                }
                true
            }
        };
        if allow_durable_exit {
            match processor.retry_ownership(&hash) {
                Ok(casper::rust::blocks::block_processor::RetryOwnership::Pending) => break,
                Ok(casper::rust::blocks::block_processor::RetryOwnership::Terminal) => {
                    if let Err(error) = processor.forget_hash_tracking(&hash) {
                        tracing::warn!(%block, %error, "Durable handoff request cleanup failed");
                    }
                    break;
                }
                Ok(casper::rust::blocks::block_processor::RetryOwnership::Missing) => {}
                Err(error) => tracing::warn!(%block, %error, "Retry ownership lookup failed"),
            }
        }
        if accepted_publication.is_none() {
            match processor.reopen_after_local_failure(hash.clone()) {
                Ok(RequestTracking::Tracked) => break,
                Ok(RequestTracking::AtCapacity | RequestTracking::Quarantined) => {
                    tracing::warn!(%block, "Retaining worker lease until local publication can succeed");
                }
                Err(error) => tracing::warn!(%block, %error, "Retry ownership handoff failed"),
            }
        }
        tokio::time::sleep(retry_delay).await;
        retry_delay = (retry_delay * 2).min(std::time::Duration::from_secs(1));
    }
    drop(item);
    recovery.request(true);
    metrics::gauge!(BLOCKS_IN_PROCESSING_SIZE_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
        .set(identities.len() as f64);
    if let Some(rss) = casper::rust::util::rholang::mem_profiler::read_vm_rss_kb_always() {
        metrics::gauge!(PROCESS_RSS_KB_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .set(rss as f64);
    }
}

/// A processing attempt's outcome. The non-`Processed` variants are normal
/// pipeline exits — a duplicate delivery, a malformed block, a block waiting
/// on its dependencies — not failures, and they must never travel the error
/// channel: an `Err` here means something actually broke.
enum BlockProcessOutcome {
    Processed(ValidBlockProcessing),
    Quarantined,
    NotOfInterest,
    Malformed,
    MissingDependencies,
    DuplicateDelivery,
}

#[derive(Debug, thiserror::Error)]
enum BlockProcessFailure {
    #[error(transparent)]
    Local(#[from] CasperError),
    #[error("validation operation failed: {0}")]
    Validation(CasperError),
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
    block_processor: &Arc<BlockProcessor<T>>,
    casper: &Arc<dyn MultiParentCasper + Send + Sync + 'static>,
    block: &BlockMessage,
    accepted_publication: &mut Option<AcceptedBlockPublication>,
) -> Result<BlockProcessOutcome, BlockProcessFailure> {
    let block_str = PrettyPrinter::build_string_bytes(&block.block_hash);
    let quarantined = block_processor.is_validation_failure_quarantined(&block.block_hash)?;
    if quarantined {
        match block_processor.prepare_stored_publication(casper.clone(), &block.block_hash)? {
            StoredPublicationPreparation::Terminal => return Ok(BlockProcessOutcome::Quarantined),
            StoredPublicationPreparation::Missing => {}
            StoredPublicationPreparation::Accepted {
                block: stored,
                evidence,
            } => {
                let evidence = accepted_publication.insert(evidence);
                block_processor
                    .publish_prepared_block(casper.clone(), &stored, evidence)
                    .await?;
                return Ok(BlockProcessOutcome::Quarantined);
            }
        }
    }

    // Step 1: Check if block is of interest
    // Equivalent to: blockProcessor.checkIfOfInterest(c, b)
    let interest = block_processor.capture_publication_interest(casper.clone(), block)?;

    let Some(interest) = interest else {
        tracing::info!("Block {} is not of interest. Dropped.", block_str);
        block_processor
            .purge_from_buffer_and_ack(block)
            .await
            .map_err(|err| {
                CasperError::RuntimeError(format!(
                    "Block {} was not of interest, and purge+cleanup failed: {}",
                    block_str, err
                ))
            })?;
        return Ok(BlockProcessOutcome::NotOfInterest);
    };

    // Step 2: Check if well-formed and store
    // Equivalent to: blockProcessor.checkIfWellFormedAndStore(b)
    let (is_well_formed, evidence) = block_processor
        .check_and_store_for_publication(casper.clone(), block, interest)
        .await?;
    *accepted_publication = evidence;

    if !is_well_formed {
        tracing::info!("Block {} is malformed. Dropped.", block_str);
        block_processor
            .purge_from_buffer_and_ack(block)
            .await
            .map_err(|err| {
                CasperError::RuntimeError(format!(
                    "Malformed block {} purge+cleanup failed: {}",
                    block_str, err
                ))
            })?;
        return Ok(BlockProcessOutcome::Malformed);
    }

    if quarantined {
        if let Some(evidence) = accepted_publication.as_ref() {
            block_processor
                .restore_accepted_publication(casper.clone(), &block.block_hash, evidence)
                .await?;
            return Ok(BlockProcessOutcome::Quarantined);
        }
        if block_processor
            .restore_stored_buffer_ownership(casper.clone(), &block.block_hash)
            .await?
        {
            return Ok(BlockProcessOutcome::Quarantined);
        }
        return Err(CasperError::RuntimeError(
            "Quarantined block body disappeared after storage".to_string(),
        )
        .into());
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
        .try_admit_settled(casper.clone(), block)
        .await
    {
        Ok(SettledAdmissionResult::Admitted) => {
            return Ok(BlockProcessOutcome::Processed(
                rspace_plus_plus::rspace::history::Either::Left(
                    casper::rust::block_status::BlockError::AdmittedSettled,
                ),
            ));
        }
        Ok(SettledAdmissionResult::DuplicateInFlight) => {
            return Ok(BlockProcessOutcome::DuplicateDelivery);
        }
        Ok(SettledAdmissionResult::AlreadyAdmitted) => {
            if let Err(error) = block_processor.ack_processed(block).await {
                tracing::warn!(
                    block = %block_str,
                    error = %error,
                    "duplicate settled-history delivery acknowledgement failed"
                );
            }
            return Ok(BlockProcessOutcome::DuplicateDelivery);
        }
        Ok(SettledAdmissionResult::NotSolicited | SettledAdmissionResult::NotEligible) => {}
        Err(err) => return Err(err.into()),
    }

    // Step 4: Check dependencies with effects
    // Equivalent to: blockProcessor.checkDependenciesWithEffects(c, b)
    let has_dependencies = block_processor
        .check_dependencies_with_publication(casper.clone(), block, accepted_publication.as_ref())
        .await?;

    if !has_dependencies {
        tracing::info!("Block {} missing dependencies.", block_str);
        // `check_dependencies_with_effects` already performs ack/cleanup for this path.
        return Ok(BlockProcessOutcome::MissingDependencies);
    }

    // Step 5: Validate block with effects
    // Equivalent to: blockProcessor.validateWithEffects(c, b, None)
    let validation_result = block_processor
        .validate_with_publication(casper.clone(), block, None, accepted_publication.as_ref())
        .await
        .map_err(BlockProcessFailure::Validation)?;

    tracing::info!("Block {} validated {:?}.", block_str, validation_result);

    Ok(BlockProcessOutcome::Processed(validation_result))
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
