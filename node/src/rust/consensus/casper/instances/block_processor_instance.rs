// See node/src/main/scala/coop/rchain/node/instances/BlockProcessorInstance.scala

use std::future::Future;
use std::sync::Arc;

use casper::rust::blocks::block_processor::{
    mark_in_flight, BlockProcessor, BlockQueueItem, InFlightBlocks, InFlightMark,
    SenderAuthorization, StoreVerdict, ValidationFailureDisposition, MAX_BLOCKS_IN_PROCESSING,
};
use casper::rust::casper::MultiParentCasper;
use casper::rust::errors::CasperError;
use casper::rust::metrics_constants::{
    BLOCK_PROCESSING_ACTIVE_METRIC, BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC,
    BLOCK_PROCESSOR_METRICS_SOURCE,
};
use casper::rust::ValidBlockProcessing;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use tokio::sync::mpsc;

use super::release_queue::{run_scheduler, ReleaseQueue};

const BLOCK_PROCESSING_RESULT_QUEUE_CAPACITY: usize = 128;
const RELEASED_BLOCKS_BEFORE_GOSSIP_TURN: usize = 4;
const MIN_PARALLEL_BLOCKS: usize = 3;

fn derive_pipeline_width(cpu_cores: usize) -> usize {
    cpu_cores
        .saturating_sub(2)
        .clamp(MIN_PARALLEL_BLOCKS, tokio::sync::Semaphore::MAX_PERMITS - 2)
}

pub(crate) fn resolve_pipeline_width() -> usize {
    let cpu_cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "Could not determine CPU capacity; using the pipeline floor");
            1
        });
    let width = derive_pipeline_width(cpu_cores);
    tracing::info!(
        pipeline_width = width,
        replay_permits = width + 2,
        cpu_cores,
        "Block processing concurrency resolved"
    );
    width
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
    pub blocks_queue_rx: mpsc::Receiver<BlockQueueItem>,

    pub block_queue_tx: mpsc::Sender<BlockQueueItem>,

    pub block_processor: Arc<BlockProcessor<T>>,

    pub blocks_in_processing: Arc<InFlightBlocks>,
    max_parallel_blocks: usize,
}

impl<T: TransportLayer + Send + Sync + 'static> BlockProcessorInstance<T> {
    pub fn new(
        (blocks_queue_rx, block_queue_tx): (
            mpsc::Receiver<BlockQueueItem>,
            mpsc::Sender<BlockQueueItem>,
        ),
        block_processor: Arc<BlockProcessor<T>>,
        blocks_in_processing: Arc<InFlightBlocks>,
        max_parallel_blocks: usize,
    ) -> Self {
        assert!((1..=tokio::sync::Semaphore::MAX_PERMITS - 2).contains(&max_parallel_blocks));
        Self {
            blocks_queue_rx,
            block_queue_tx,
            block_processor,
            blocks_in_processing,
            max_parallel_blocks,
        }
    }

    pub fn supervised(
        self,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> (
        mpsc::Receiver<(BlockMessage, ValidBlockProcessing)>,
        impl std::future::Future<Output = Result<(), CasperError>> + Send,
    ) {
        let (result_tx, result_rx) = mpsc::channel(BLOCK_PROCESSING_RESULT_QUEUE_CAPACITY);

        let task = async move {
            let Self {
                blocks_queue_rx,
                block_queue_tx,
                block_processor,
                blocks_in_processing,
                max_parallel_blocks: width,
            } = self;
            drop(block_queue_tx);
            tracing::info!(
                max_parallel_blocks = width,
                "Starting bounded block processing"
            );
            metrics::gauge!(
                BLOCK_PROCESSING_PARALLEL_LIMIT_METRIC,
                "source" => BLOCK_PROCESSOR_METRICS_SOURCE
            )
            .set(width as f64);
            let release_processor = block_processor.clone();
            run_block_tasks(blocks_queue_rx, shutdown, width, move |(casper, block, in_flight_guard): BlockQueueItem| {
                let block_processor = block_processor.clone();
                let result_tx = result_tx.clone();

                async move {
                    let _active_guard = ActiveBlockProcessingGuard::new();
                    let block_str = PrettyPrinter::build_string_bytes(&block.block_hash);
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
                        | Ok(BlockProcessOutcome::UnauthorizedSender)
                        | Ok(BlockProcessOutcome::Malformed)
                        | Ok(BlockProcessOutcome::PendingRefused) => {}
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
                    (casper, block_str)
                }
            },
            move |(casper, block_str): (Arc<dyn MultiParentCasper + Send + Sync>, String)| {
                let block_processor = release_processor.clone();
                let blocks_in_processing = blocks_in_processing.clone();
                async move {
                    let mut released = Vec::new();
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
                                match mark_in_flight(&blocks_in_processing, pendant_hash.clone()) {
                                    InFlightMark::Marked(guard) => {
                                        tracing::info!(
                                            "Enqueued dependency-free pendant {}",
                                            PrettyPrinter::build_string_bytes(&pendant.block_hash)
                                        );
                                        released.push((casper.clone(), pendant.clone(), guard));
                                    }
                                    InFlightMark::CapReached => {
                                        block_processor.note_local_backpressure_drop(
                                            &pendant_hash,
                                            "instance-pendant",
                                        );
                                        tracing::warn!(
                                            "Skipping dependency-free pendant {} enqueue because \
                                             in-flight block cap {} is reached",
                                            PrettyPrinter::build_string_bytes(&pendant.block_hash),
                                            MAX_BLOCKS_IN_PROCESSING
                                        );
                                    }
                                    InFlightMark::AlreadyInFlight => {
                                        tracing::info!(
                                            "Skipping dependency-free pendant {} enqueue because it \
                                             is already marked in-flight",
                                            PrettyPrinter::build_string_bytes(&pendant.block_hash)
                                        );
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            tracing::error!(error = %err, "dependency-free block buffer retrieval failed");
                        }
                    }

                    released
                }
            }).await?;

            tracing::info!("Block processing queue closed, stopping processor");

            Result::<(), CasperError>::Ok(())
        };
        (result_rx, task)
    }
}

async fn run_block_tasks<Item, Context, Process, Task, Release, Scan>(
    mut input: mpsc::Receiver<Item>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    width: usize,
    process: Process,
    release: Release,
) -> Result<(), CasperError>
where
    Item: Send + 'static,
    Context: Send + 'static,
    Process: Fn(Item) -> Task + Send + Sync + 'static,
    Task: Future<Output = Context> + Send + 'static,
    Release: Fn(Context) -> Scan + Send + Sync + 'static,
    Scan: Future<Output = Vec<Item>> + Send + 'static,
{
    let queue = Arc::new(ReleaseQueue::new(RELEASED_BLOCKS_BEFORE_GOSSIP_TURN, width));
    let gossip_queue = queue.clone();
    let forward = async move {
        let mut stopping = false;
        loop {
            tokio::select! {
                biased;
                _ = async { let _ = shutdown.wait_for(|stopping| *stopping).await; }, if !stopping => {
                    stopping = true;
                    input.close();
                }
                item = input.recv() => {
                    let Some(item) = item else { break };
                    let push = gossip_queue.push_gossip_wait(item);
                    tokio::pin!(push);
                    tokio::select! {
                        biased;
                        _ = async { let _ = shutdown.wait_for(|stopping| *stopping).await; }, if !stopping => {
                            stopping = true;
                            input.close();
                            push.await;
                        }
                        _ = &mut push => {}
                    }
                }
            }
        }
        gossip_queue.close();
        Ok(())
    };
    tokio::try_join!(forward, run_scheduler(queue, width, process, release),)
        .map_err(block_task_error)?;
    Ok(())
}

fn block_task_error(error: tokio::task::JoinError) -> CasperError {
    CasperError::Other(format!("Block processing task failed: {error}"))
}

#[cfg(test)]
mod tests;

/// A processing attempt's outcome. The non-`Processed` variants are normal
/// pipeline exits — a duplicate delivery, a malformed block, a block waiting
/// on its dependencies — not failures, and they must never travel the error
/// channel: an `Err` here means something actually broke.
enum BlockProcessOutcome {
    Processed(BlockMessage, ValidBlockProcessing),
    NotOfInterest,
    UnauthorizedSender,
    Malformed,
    PendingRefused,
    MissingDependencies,
}

/// Process a block through all validation steps
///
/// This implements the Scala pipeline:
/// 1. checkIfOfInterest
///    then checkSenderAuthorization (drops a sender unbonded in a validated
///    main parent before any store write or dependency request)
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
    let verdict = match block_processor.check_if_of_interest(casper.clone(), &block) {
        Ok(verdict) => verdict,
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

    if !verdict.is_fresh() {
        block_processor
            .dispose_not_of_interest(verdict, &block)
            .await
            .map_err(|err| {
                CasperError::RuntimeError(format!(
                    "Block {} was not of interest, and cleanup failed: {}",
                    block_str, err
                ))
            })?;
        return Ok(BlockProcessOutcome::NotOfInterest);
    }

    let authorization = match block_processor.check_sender_authorization(&block) {
        Ok(authorization) => authorization,
        Err(err) => {
            block_processor
                .ack_processed(&block)
                .await
                .map_err(|ack_err| {
                    CasperError::RuntimeError(format!(
                        "check_sender_authorization failed for {}, and cleanup failed: {}",
                        block_str, ack_err
                    ))
                })?;
            return Err(err);
        }
    };

    if authorization == SenderAuthorization::Unauthorized {
        block_processor
            .purge_from_buffer_and_ack(&block)
            .await
            .map_err(|err| {
                CasperError::RuntimeError(format!(
                    "Unauthorized block {} purge+cleanup failed: {}",
                    block_str, err
                ))
            })?;
        return Ok(BlockProcessOutcome::UnauthorizedSender);
    }

    // Step 2: Check if well-formed and store
    // Equivalent to: blockProcessor.checkIfWellFormedAndStore(b)
    let store_verdict = match block_processor
        .check_if_well_formed_and_store(&block, authorization)
        .await
    {
        Ok(store_verdict) => store_verdict,
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

    if let StoreVerdict::PendingRefused(refusal) = store_verdict {
        block_processor
            .release_refused_pending(&block, refusal)
            .await
            .map_err(|err| {
                CasperError::RuntimeError(format!(
                    "Refused pending block {} cleanup failed: {}",
                    block_str, err
                ))
            })?;
        return Ok(BlockProcessOutcome::PendingRefused);
    }

    if store_verdict == StoreVerdict::Malformed {
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

#[cfg(test)]
mod startup_tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, DeployId, KeyValueDagRepresentation,
    };
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
    use casper::rust::blocks::block_processor::{
        mark_in_flight, BlockProcessor, BlockProcessorDependencies, BlockQueueItem, InFlightBlocks,
        InFlightMark,
    };
    use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
    use casper::rust::casper::{
        Casper, CasperShardConf, CasperSnapshot, DeployError, MultiParentCasper,
    };
    use casper::rust::engine::block_retriever::BlockRetriever;
    use casper::rust::errors::CasperError;
    use casper::rust::genesis::genesis::Genesis;
    use casper::rust::util::rholang::runtime_manager::{
        ExploratoryDeployConfig, ReplayLock, RuntimeManager,
    };
    use casper::rust::validator_identity::ValidatorIdentity;
    use comm::rust::peer_node::{NodeIdentifier, PeerNode};
    use comm::rust::rp::connect::ConnectionsCell;
    use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
    use crypto::rust::signatures::signed::Signed;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_implicits::get_random_block_default;
    use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
    use models::rust::validator::Validator;
    use prost::bytes::Bytes;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rspace_plus_plus::rspace::history::Either;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
    use tokio::sync::mpsc;

    use super::{derive_pipeline_width, BlockProcessorInstance};

    struct StartupCasper {
        snapshot: CasperSnapshot,
        approved: BlockMessage,
        block_store: KeyValueBlockStore,
        runtime_manager: Arc<RuntimeManager>,
        dag_reads: AtomicUsize,
        processed: Mutex<Vec<BlockHash>>,
    }

    #[async_trait]
    impl Casper for StartupCasper {
        async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
            Ok(self.snapshot.clone())
        }

        fn contains(&self, _hash: &BlockHash) -> bool { panic!("Unexpected contains call") }

        fn dag_contains(&self, hash: &BlockHash) -> bool {
            self.processed.lock().unwrap().push(hash.clone());
            true
        }

        fn buffer_contains(&self, _hash: &BlockHash) -> bool { false }

        fn get_approved_block(&self) -> Result<&BlockMessage, CasperError> { Ok(&self.approved) }

        fn deploy(
            &self,
            _deploy: Signed<DeployData>,
        ) -> Result<Either<DeployError, DeployId>, CasperError> {
            panic!("Unexpected deploy call")
        }

        async fn estimator(
            &self,
            _dag: &mut KeyValueDagRepresentation,
        ) -> Result<Vec<BlockHash>, CasperError> {
            panic!("Unexpected estimator call")
        }

        fn get_version(&self) -> i64 { self.approved.header.version }

        async fn validate(
            &self,
            _block: &BlockMessage,
            _snapshot: &mut CasperSnapshot,
        ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
            panic!("Already processed blocks must not be validated again")
        }

        async fn validate_self_created(
            &self,
            _block: &BlockMessage,
            _snapshot: &mut CasperSnapshot,
            _pre_state_hash: Bytes,
            _post_state_hash: Bytes,
        ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
            panic!("Unexpected self-created block")
        }

        async fn handle_valid_block(
            &self,
            _block: &BlockMessage,
        ) -> Result<KeyValueDagRepresentation, CasperError> {
            panic!("Unexpected valid block")
        }

        fn handle_invalid_block(
            &self,
            _block: &BlockMessage,
            _status: &InvalidBlock,
            _dag: &KeyValueDagRepresentation,
        ) -> Result<KeyValueDagRepresentation, CasperError> {
            panic!("Unexpected invalid block")
        }

        fn get_dependency_free_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
            Ok(Vec::new())
        }

        fn get_all_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> { Ok(Vec::new()) }
    }

    #[async_trait]
    impl MultiParentCasper for StartupCasper {
        async fn fetch_dependencies(&self) -> Result<(), CasperError> {
            panic!("Unexpected dependency fetch")
        }

        fn normalized_initial_fault(
            &self,
            _weights: HashMap<Validator, u64>,
        ) -> Result<f32, CasperError> {
            panic!("Unexpected fault calculation")
        }

        async fn last_finalized_block(&self) -> Result<BlockMessage, CasperError> {
            panic!("Startup must read the stored LFB without computing finalization")
        }

        async fn block_dag(&self) -> Result<KeyValueDagRepresentation, CasperError> {
            self.dag_reads.fetch_add(1, Ordering::SeqCst);
            Err(CasperError::Other(
                "Block processing setup must not read the DAG".to_string(),
            ))
        }

        fn block_store(&self) -> &KeyValueBlockStore { &self.block_store }

        fn casper_shard_conf(&self) -> &CasperShardConf { &self.snapshot.on_chain_state.shard_conf }

        fn runtime_manager(&self) -> Arc<RuntimeManager> { self.runtime_manager.clone() }

        fn get_validator(&self) -> Option<ValidatorIdentity> { None }

        fn is_local_validator_bonded(&self) -> bool { false }

        async fn get_history_exporter(&self) -> Arc<dyn RSpaceExporter> {
            panic!("Unexpected history export")
        }

        async fn has_pending_deploys_in_storage(&self) -> Result<bool, CasperError> { Ok(false) }
    }

    struct StartupFixture {
        casper: Arc<StartupCasper>,
        processor: Arc<BlockProcessor<TransportLayerStub>>,
        in_flight: Arc<InFlightBlocks>,
    }

    impl StartupFixture {
        async fn new() -> Self {
            let mut stores = InMemoryStoreManager::new();
            let block_store = KeyValueBlockStore::create_from_kvm(&mut stores)
                .await
                .unwrap();
            let (runtime_manager, _) = RuntimeManager::create_with_history_config(
                stores.r_space_stores().await.unwrap(),
                RuntimeManager::mergeable_store(&mut stores).await.unwrap(),
                Genesis::default_mergeable_tags_arc(),
                ExternalServices::noop(),
                ExploratoryDeployConfig::for_tests(),
                Arc::new(ReplayLock::with_permits(5)),
            );
            let runtime_manager = Arc::new(runtime_manager);
            let approved = get_random_block_default();
            let snapshot = TestCasperWithSnapshot::create_empty_snapshot();

            let transport = Arc::new(TransportLayerStub::new());
            let connections = ConnectionsCell::new();
            let peer = PeerNode::new(
                NodeIdentifier {
                    key: vec![1; 20].into(),
                },
                "localhost".to_string(),
                40400,
                40400,
            );
            let conf = create_rp_conf_ask(peer, None, None);
            let retriever = BlockRetriever::new(
                Arc::new(Mutex::new(HashMap::new())),
                transport.clone(),
                connections.clone(),
                conf.clone(),
            );
            let buffer = CasperBufferKeyValueStorage::new_from_kv_store(
                KeyValueTypedStoreImpl::new(stores.store("parents-map".to_string()).await.unwrap()),
            )
            .await
            .unwrap();
            let dag = BlockDagKeyValueStorage::new(&mut stores).await.unwrap();
            let dependencies = BlockProcessorDependencies::new(
                block_store.clone(),
                buffer,
                dag,
                retriever,
                transport,
                connections,
                conf,
                None,
            );
            Self {
                casper: Arc::new(StartupCasper {
                    snapshot,
                    approved,
                    block_store,
                    runtime_manager,
                    dag_reads: AtomicUsize::new(0),
                    processed: Mutex::new(Vec::new()),
                }),
                processor: Arc::new(BlockProcessor::new(dependencies)),
                in_flight: Arc::new(InFlightBlocks::new()),
            }
        }

        fn instance(
            &self,
        ) -> (
            mpsc::Sender<BlockQueueItem>,
            BlockProcessorInstance<TransportLayerStub>,
        ) {
            let (tx, rx) = mpsc::channel(4);
            let instance = BlockProcessorInstance::new(
                (rx, tx.clone()),
                self.processor.clone(),
                self.in_flight.clone(),
                3,
            );
            (tx, instance)
        }

        fn item(&self, byte: u8) -> BlockQueueItem {
            let mut block = self.casper.approved.clone();
            block.block_hash = vec![byte; 32].into();
            let InFlightMark::Marked(guard) =
                mark_in_flight(&self.in_flight, block.block_hash.clone())
            else {
                panic!("Test block must acquire an in-flight marker")
            };
            (self.casper.clone(), block, guard)
        }
    }

    #[tokio::test]
    async fn startup_does_not_wait_for_a_block() {
        let fixture = StartupFixture::new().await;
        let (tx, instance) = fixture.instance();
        let (_stop, shutdown) = tokio::sync::watch::channel(false);
        let (mut results, processing) = instance.supervised(shutdown);
        let processing = tokio::spawn(processing);
        assert!(matches!(
            results.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert_eq!(fixture.casper.dag_reads.load(Ordering::SeqCst), 0);
        drop(tx);
        assert!(
            tokio::time::timeout(Duration::from_secs(10), results.recv())
                .await
                .expect("The closed queue must finish processing")
                .is_none()
        );
        processing.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn startup_processes_the_first_and_following_blocks_once() {
        let fixture = StartupFixture::new().await;
        let (tx, instance) = fixture.instance();
        let (_stop, shutdown) = tokio::sync::watch::channel(false);
        let (mut results, processing) = instance.supervised(shutdown);
        let processing = tokio::spawn(processing);
        for byte in 21..=23 {
            assert!(tx.send(fixture.item(byte)).await.is_ok());
        }
        drop(tx);
        assert!(
            tokio::time::timeout(Duration::from_secs(10), results.recv())
                .await
                .expect("All queued blocks must finish processing")
                .is_none()
        );
        processing.await.unwrap().unwrap();
        let mut processed = fixture.casper.processed.lock().unwrap().clone();
        processed.sort();
        let expected: Vec<BlockHash> = (21..=23).map(|byte| vec![byte; 32].into()).collect();
        assert_eq!(processed, expected);
        assert!(fixture.in_flight.is_empty());
        assert_eq!(fixture.casper.dag_reads.load(Ordering::SeqCst), 0);
        assert!(fixture
            .casper
            .runtime_manager
            .active_validators_cache_order
            .lock()
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn startup_with_a_closed_empty_queue_finishes_without_reading_casper() {
        let fixture = StartupFixture::new().await;
        let (tx, instance) = fixture.instance();
        drop(tx);
        let (_stop, shutdown) = tokio::sync::watch::channel(false);
        let (mut results, processing) = instance.supervised(shutdown);
        let processing = tokio::spawn(processing);
        assert!(
            tokio::time::timeout(Duration::from_secs(10), results.recv())
                .await
                .expect("An empty closed queue must finish processing")
                .is_none()
        );
        processing.await.unwrap().unwrap();
        assert_eq!(fixture.casper.dag_reads.load(Ordering::SeqCst), 0);
        assert!(fixture.in_flight.is_empty());
    }

    #[test]
    fn pipeline_width_uses_the_cpu_capacity() {
        for (cores, width) in [(6, 4), (8, 6), (16, 14), (32, 30)] {
            assert_eq!(derive_pipeline_width(cores), width);
        }
    }

    #[test]
    fn small_cpu_limits_keep_the_pipeline_floor() {
        for cores in 0..=5 {
            assert_eq!(derive_pipeline_width(cores), 3);
        }
    }

    #[test]
    fn the_replay_reserve_fits_the_semaphore_at_extreme_inputs() {
        let width = derive_pipeline_width(usize::MAX);
        assert_eq!(width, tokio::sync::Semaphore::MAX_PERMITS - 2);
        assert!(width.checked_add(2).is_some());
    }
}
