// See casper/src/main/scala/coop/rchain/casper/engine/LfsBlockRequester.scala

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_stream::stream;
use async_trait::async_trait;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{ApprovedBlock, BlockMessage};
use models::rust::validator::Validator;
use tokio::sync::{mpsc, Semaphore};

use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    CASPER_METRICS_SOURCE, INIT_BLOCK_MESSAGE_QUEUE_PENDING_METRIC,
};
use crate::rust::util::proto_util;

// Last Finalized State processor for receiving blocks.

/// Retry-backoff ceiling shared by all three LFS requesters.
pub(crate) const LFS_MAX_REQUEST_TIMEOUT: Duration = Duration::from_secs(128);

/// Trait that abstracts the operations needed by the LFS block requester
#[async_trait]
pub trait BlockRequesterOps {
    async fn request_for_block(&self, block_hash: &BlockHash) -> Result<(), CasperError>;

    fn contains_block(&self, block_hash: &BlockHash) -> Result<bool, CasperError>;

    fn get_block_from_store(&self, block_hash: &BlockHash) -> BlockMessage;

    fn put_block_to_store(
        &mut self,
        block_hash: BlockHash,
        block: &BlockMessage,
    ) -> Result<(), CasperError>;

    fn validate_block(&self, block: &BlockMessage) -> bool;
}

/// Possible request statuses
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqStatus {
    Init,
    Requested,
    Received,
}

/// Information about block reception status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiveInfo {
    pub requested: bool,
    pub latest: bool,
    pub lastlatest: bool,
}

impl ReceiveInfo {
    pub fn new(requested: bool, latest: bool, lastlatest: bool) -> Self {
        Self {
            requested,
            latest,
            lastlatest,
        }
    }
}

/// State to control processing of requests
#[derive(Debug, Clone)]
pub struct ST<Key: Hash + Eq + Clone> {
    pub d: HashMap<Key, ReqStatus>,
    pub latest: HashSet<Key>,
    pub lower_bound: i64,
    pub height_map: BTreeMap<i64, HashSet<Key>>,
    pub finished: HashSet<Key>,
    pub slash_evidence: HashSet<Key>,
    pub slash_targets: HashSet<Key>,
}

impl<Key: Hash + Eq + Clone> ST<Key> {
    /// Create requests state with initial keys.
    pub fn new(
        initial: HashSet<Key>,
        latest: Option<HashSet<Key>>,
        lower_bound: Option<i64>,
    ) -> Self {
        let latest = latest.unwrap_or_default();
        let lower_bound = lower_bound.unwrap_or(0);

        // Set Init status for initial keys
        let d = initial
            .iter()
            .map(|k| (k.clone(), ReqStatus::Init))
            .collect();

        Self {
            d,
            latest,
            lower_bound,
            height_map: BTreeMap::new(),
            finished: HashSet::new(),
            slash_evidence: HashSet::new(),
            slash_targets: HashSet::new(),
        }
    }

    /// Adds new keys to Init state, ready for processing. Existing keys are skipped.
    /// Returns a new ST instance with the added keys.
    pub fn add(&self, keys: HashSet<Key>) -> Self {
        let promoted: HashSet<Key> = keys.intersection(&self.slash_evidence).cloned().collect();
        let mut finished = self.finished.clone();
        for key in &promoted {
            finished.remove(key);
        }
        let slash_evidence = self.slash_evidence.difference(&keys).cloned().collect();

        // Filter keys that are in Done status or in request Map.
        let new_keys: HashSet<Key> = keys
            .difference(&finished)
            .filter(|k| !self.d.contains_key(k))
            .cloned()
            .collect();

        // Set Init status for new keys
        let mut new_d = self.d.clone();
        for key in new_keys {
            new_d.insert(key, ReqStatus::Init);
        }

        // Return new ST instance with updated request map
        Self {
            d: new_d,
            latest: self.latest.clone(),
            lower_bound: self.lower_bound,
            height_map: self.height_map.clone(),
            finished,
            slash_evidence,
            slash_targets: self.slash_targets.clone(),
        }
    }

    pub fn add_slash_evidence(&self, keys: HashSet<Key>) -> Self {
        let mut next = self.clone();
        next.slash_targets.extend(keys.iter().cloned());
        for key in keys {
            if !next.d.contains_key(&key) && !next.finished.contains(&key) {
                next.d.insert(key.clone(), ReqStatus::Init);
                if !next.latest.contains(&key) {
                    next.slash_evidence.insert(key);
                }
            }
        }
        next
    }

    /// Get next keys not already requested or in case of resend together with Requested.
    /// Returns updated state with requested keys and the set of keys that were marked for request.
    pub fn get_next(&self, resend: bool) -> (Self, HashSet<Key>) {
        // Check if latest are requested
        let requests = if self.latest.is_empty() {
            self.d.clone()
        } else {
            // Add latest to Init if not already in d
            let mut requests = self.d.clone();
            for key in &self.latest {
                if !self.d.contains_key(key) {
                    requests.insert(key.clone(), ReqStatus::Init);
                }
            }
            requests
        };

        let mut new_requests = HashMap::new();
        let mut request_keys = HashSet::new();

        for (key, status) in &requests {
            // Select initialized or re-request if resending
            let check_for_request =
                *status == ReqStatus::Init || (resend && *status == ReqStatus::Requested);

            let should_request = if self.latest.is_empty() {
                // Latest are downloaded, no additional conditions
                check_for_request
            } else {
                // Only latest are requested first
                check_for_request && self.latest.contains(key)
            };

            if should_request {
                new_requests.insert(key.clone(), ReqStatus::Requested);
                request_keys.insert(key.clone());
            }
        }

        // Create new d map with updated requests
        let mut new_d = self.d.clone();
        new_d.extend(new_requests);

        let new_state = Self {
            d: new_d,
            latest: self.latest.clone(),
            lower_bound: self.lower_bound,
            height_map: self.height_map.clone(),
            finished: self.finished.clone(),
            slash_evidence: self.slash_evidence.clone(),
            slash_targets: self.slash_targets.clone(),
        };

        (new_state, request_keys)
    }

    /// Confirm key is Received if it was Requested.
    /// Returns updated state with the flags if Requested and last latest received.
    /// `lowers_bound` is false for the genesis placeholder: it arrives as a
    /// latest message at height 0, and lowering to `height - 1` puts the
    /// acceptance window at -1, where the restore walks to genesis. It is still
    /// consumed from `latest`, so the stream terminates.
    pub fn received(
        &self,
        k: Key,
        height: i64,
        latest_replacement: Option<Key>,
        lowers_bound: bool,
    ) -> (Self, ReceiveInfo) {
        let is_req = self.d.get(&k) == Some(&ReqStatus::Requested);

        if is_req {
            // Remove message from the set of latest messages (if exists)
            let mut adj_latest = self.latest.clone();
            let was_removed = adj_latest.remove(&k);
            let is_latest = was_removed;
            let mut slash_evidence = self.slash_evidence.clone();
            if is_latest {
                slash_evidence.remove(&k);
            }

            // Add replacement message if supplied
            let new_latest = if let Some(replacement) = latest_replacement {
                slash_evidence.remove(&replacement);
                adj_latest.insert(replacement);
                adj_latest
            } else {
                adj_latest
            };

            let is_last_latest = is_latest && new_latest.is_empty();

            // Save in height map
            let mut new_height_map = self.height_map.clone();
            if !slash_evidence.contains(&k) {
                let height_keys = new_height_map.entry(height).or_default();
                height_keys.insert(k.clone());
            }

            // Calculate new minimum height if latest message
            // - we need parents of latest message so it's `-1`
            let new_lower_bound = if is_latest && lowers_bound {
                std::cmp::min(height - 1, self.lower_bound)
            } else {
                self.lower_bound
            };

            // Set new status to Received
            let mut new_d = self.d.clone();
            new_d.insert(k, ReqStatus::Received);

            // Set new minimum height and update latest
            let new_state = Self {
                d: new_d,
                latest: new_latest,
                lower_bound: new_lower_bound,
                height_map: new_height_map,
                finished: self.finished.clone(),
                slash_evidence,
                slash_targets: self.slash_targets.clone(),
            };

            (
                new_state,
                ReceiveInfo::new(is_req, is_latest, is_last_latest),
            )
        } else {
            (self.clone(), ReceiveInfo::new(false, false, false))
        }
    }

    /// Mark key as finished (Done) with optionally set minimum lower bound.
    /// Returns a new ST instance with the key moved from requests to finished.
    pub fn done(&self, k: Key) -> Self {
        let is_received = self.d.get(&k) == Some(&ReqStatus::Received);

        if is_received {
            // If Received key, remove from request Map and add to Done Set
            let mut new_d = self.d.clone();
            new_d.remove(&k);

            let mut new_finished = self.finished.clone();
            new_finished.insert(k);

            Self {
                d: new_d,
                latest: self.latest.clone(),
                lower_bound: self.lower_bound,
                height_map: self.height_map.clone(),
                finished: new_finished,
                slash_evidence: self.slash_evidence.clone(),
                slash_targets: self.slash_targets.clone(),
            }
        } else {
            self.clone()
        }
    }

    pub fn is_finished(&self) -> bool { self.latest.is_empty() && self.d.is_empty() }
}

/// Whether the block filling a latest-message slot is the testimony of the
/// validator whose slot it is. A bonded validator that never proposed has the
/// genesis hash there instead, which no validator signed.
fn slot_is_own_testimony(
    latest_messages: &HashMap<BlockHash, Validator>,
    block: &BlockMessage,
) -> bool {
    latest_messages
        .get(&block.block_hash)
        .is_some_and(|validator| *validator == block.sender)
}

struct StreamProcessor<'a, T: BlockRequesterOps> {
    requester: &'a mut T,
    st: Arc<Mutex<ST<BlockHash>>>,
    latest_messages: HashMap<BlockHash, Validator>,
    response_hash_sender: mpsc::Sender<BlockHash>,
}

impl<'a, T: BlockRequesterOps> StreamProcessor<'a, T> {
    fn new(
        requester: &'a mut T,
        st: Arc<Mutex<ST<BlockHash>>>,
        latest_messages: HashMap<BlockHash, Validator>,
        response_hash_sender: mpsc::Sender<BlockHash>,
    ) -> Self {
        Self {
            requester,
            st,
            latest_messages,
            response_hash_sender,
        }
    }

    async fn process_block(&mut self, block: &BlockMessage) -> Result<bool, CasperError> {
        // Validate and mark received block
        let is_valid = self.validate_received_block(block).await?;

        // Save block to store
        if is_valid {
            self.save_block(block).await?;
        }
        // Trigger request queue (without resend of already requested)
        self.request_next(false).await?;
        Ok(is_valid)
    }

    /// Validate received block, check if it was requested and if block hash is correct.
    /// Following justifications from last finalized block gives us proof for all ancestor blocks.
    async fn validate_received_block(&self, block: &BlockMessage) -> Result<bool, CasperError> {
        let validation_start = std::time::Instant::now();
        let block_number = proto_util::block_number(block);
        let block_hash_str = format!("{:?}", block.block_hash);

        tracing::debug!(
            "Validating received block {} at height {}",
            block_hash_str,
            block_number
        );

        let is_requested = {
            let state = self.st.lock().map_err(|_| {
                CasperError::StreamError("Failed to acquire state lock for validation".to_string())
            })?;
            state.d.get(&block.block_hash) == Some(&ReqStatus::Requested)
        };
        if !is_requested {
            tracing::debug!("Block {} was not requested, ignored", block_hash_str);
            return Ok(false);
        }
        if !self.requester.validate_block(block) {
            tracing::warn!(
                "Received {} with invalid hash or signature. Ignored block.",
                PrettyPrinter::build_string_block_message(block, false)
            );
            return Ok(false);
        }

        // Mark block as received and calculate minimum height (if latest)
        let received_result = {
            let mut state = self.st.lock().map_err(|_| {
                CasperError::StreamError("Failed to acquire state lock for validation".to_string())
            })?;

            // if message received is latest as per approved block - add its self justification
            // to target latest messages that has to be pulled
            let lm_replacement = if self.latest_messages.contains_key(&block.block_hash) {
                tracing::info!(
                    "Block {} is a latest message, checking for self-justification replacement",
                    block_hash_str
                );
                proto_util::creator_justification_block_message(block).map(|justification| {
                    tracing::debug!(
                        "Found self-justification replacement: {:?}",
                        justification.latest_block_hash
                    );
                    justification.latest_block_hash
                })
            } else {
                None
            };

            let lowers_bound = slot_is_own_testimony(&self.latest_messages, block);

            let (new_state, receive_info) = state.received(
                block.block_hash.clone(),
                block_number,
                lm_replacement,
                lowers_bound,
            );
            *state = new_state;
            receive_info
        };

        let ReceiveInfo {
            requested: is_received,
            latest: is_received_latest,
            lastlatest: is_last_latest,
        } = received_result;

        tracing::debug!(
            "Block {} validation status - requested: {}, latest: {}, last_latest: {}",
            block_hash_str,
            is_received,
            is_received_latest,
            is_last_latest
        );

        // Try accept received block if it has valid hash
        let is_received_and_valid = if is_received {
            // Log minimum height when last latest block is received
            if is_last_latest {
                let minimum_height = {
                    let state = self.st.lock().map_err(|_| {
                        CasperError::StreamError(
                            "Failed to acquire state lock for minimum height".to_string(),
                        )
                    })?;
                    state.lower_bound
                };
                tracing::info!(
                    "Latest blocks downloaded. Minimum block height is {}.",
                    minimum_height
                );
            }

            // Accept block if it's requested and satisfy conditions
            // - received one of latest messages
            // - requested and block number is greater than minimum
            let block_is_accepted = {
                let mut state = self.st.lock().map_err(|_| {
                    CasperError::StreamError(
                        "Failed to acquire state lock for acceptance check".to_string(),
                    )
                })?;
                let evidence_only = state.slash_evidence.contains(&block.block_hash);
                let accepted = !evidence_only
                    && (is_received_latest || (is_received && block_number >= state.lower_bound));
                if !evidence_only {
                    *state = state.add_slash_evidence(
                        proto_util::block_slash_targets(block).into_iter().collect(),
                    );
                }
                tracing::debug!(
                    "Block {} acceptance check - latest: {}, height_ok: {} ({}>={}), accepted: {}",
                    block_hash_str,
                    is_received_latest,
                    block_number >= state.lower_bound,
                    block_number,
                    state.lower_bound,
                    accepted
                );
                accepted
            };

            if block_is_accepted {
                // Update dependencies for requesting
                let deps_start = std::time::Instant::now();
                let deps: HashSet<BlockHash> = proto_util::dependencies_hashes_of(block)
                    .into_iter()
                    .collect();
                let deps_duration = deps_start.elapsed();

                tracing::info!(
                    "Block {} accepted, adding {} dependencies (computed in {:?})",
                    block_hash_str,
                    deps.len(),
                    deps_duration
                );

                let mut state = self.st.lock().map_err(|_| {
                    CasperError::StreamError(
                        "Failed to acquire state lock for dependencies".to_string(),
                    )
                })?;
                *state = state.add(deps);
            } else {
                tracing::debug!(
                    "Block {} not accepted due to validation criteria",
                    block_hash_str
                );
            }

            is_received
        } else {
            tracing::debug!(
                "Block {} validation failed or not requested",
                block_hash_str
            );
            false
        };

        let validation_duration = validation_start.elapsed();
        tracing::debug!(
            "Block {} validation completed in {:?}: {}",
            block_hash_str,
            validation_duration,
            is_received_and_valid
        );

        Ok(is_received_and_valid)
    }

    async fn save_block(&mut self, block: &BlockMessage) -> Result<(), CasperError> {
        let save_start = std::time::Instant::now();
        let block_hash_str = format!("{:?}", block.block_hash);

        tracing::debug!("Saving block {} to store", block_hash_str);

        // Save block to the store
        let storage_start = std::time::Instant::now();
        let already_saved = self
            .requester
            .contains_block(&block.block_hash)
            .unwrap_or(false);

        if !already_saved {
            self.requester
                .put_block_to_store(block.block_hash.clone(), block)?;
            let storage_duration = storage_start.elapsed();
            tracing::info!(
                "Block {} saved to store in {:?}",
                block_hash_str,
                storage_duration
            );
        } else {
            tracing::debug!(
                "Block {} already exists in store, skipping save",
                block_hash_str
            );
        }

        let state_update_start = std::time::Instant::now();
        {
            let mut state = self.st.lock().map_err(|_| {
                CasperError::StreamError("Failed to acquire state lock for done".to_string())
            })?;
            *state = state.done(block.block_hash.clone());
        }
        let state_update_duration = state_update_start.elapsed();

        let total_save_duration = save_start.elapsed();
        tracing::debug!(
            "Block {} marked as done (state update: {:?}, total: {:?})",
            block_hash_str,
            state_update_duration,
            total_save_duration
        );

        Ok(())
    }

    /// Reads current state for next blocks and send the requests.
    async fn request_next(&self, resend: bool) -> Result<(), CasperError> {
        let start_time = std::time::Instant::now();

        // Check if stream is finished (no more requests)
        let (is_end, active_count, latest_count, finished_count, lower_bound) = {
            let state = self.st.lock().map_err(|_| {
                CasperError::StreamError("Failed to acquire state lock".to_string())
            })?;
            (
                state.is_finished(),
                state.d.len(),
                state.latest.len(),
                state.finished.len(),
                state.lower_bound,
            )
        };

        if resend {
            tracing::info!(
                "Processing resend request - active: {}, latest: {}, finished: {}, lower_bound: {}",
                active_count,
                latest_count,
                finished_count,
                lower_bound
            );
        } else {
            tracing::debug!(
                "Processing new request - active: {}, latest: {}, finished: {}, lower_bound: {}",
                active_count,
                latest_count,
                finished_count,
                lower_bound
            );
        }

        // Take next set of items to request (w/o duplicates)
        let hashes = {
            let mut state = self.st.lock().map_err(|_| {
                CasperError::StreamError("Failed to acquire state lock".to_string())
            })?;
            let (new_state, next_hashes) = state.get_next(resend);
            *state = new_state;
            next_hashes
        };

        if hashes.is_empty() {
            tracing::debug!("No new blocks to request (resend: {})", resend);
            return Ok(());
        }

        tracing::info!("Requesting {} blocks (resend: {})", hashes.len(), resend);

        // Check existing blocks
        let existing_check_start = std::time::Instant::now();
        let existing_hashes: Vec<BlockHash> = hashes
            .iter()
            .filter(|hash| self.requester.contains_block(hash).unwrap_or(false))
            .cloned()
            .collect();
        let existing_check_duration = existing_check_start.elapsed();

        tracing::debug!(
            "Block existence check completed in {:?} - {} of {} blocks already exist",
            existing_check_duration,
            existing_hashes.len(),
            hashes.len()
        );

        // Enqueue hashes of existing blocks
        if !existing_hashes.is_empty() {
            tracing::info!(
                "Found {} existing blocks in store, queueing for processing",
                existing_hashes.len()
            );
            for hash in existing_hashes.iter() {
                self.response_hash_sender
                    .send(hash.clone())
                    .await
                    .map_err(|_| {
                        CasperError::StreamError("Failed to enqueue existing hash".to_string())
                    })?;
            }
        }

        // Missing blocks not already in the block store
        let missing_blocks: HashSet<BlockHash> = hashes
            .difference(&existing_hashes.into_iter().collect())
            .cloned()
            .collect();

        // Send all requests in parallel for missing blocks
        if !is_end && !missing_blocks.is_empty() {
            let request_start = std::time::Instant::now();
            tracing::info!(
                "Broadcasting requests for {} missing blocks",
                missing_blocks.len()
            );

            // Create parallel futures for all missing block requests
            let request_futures: Vec<_> = missing_blocks
                .into_iter()
                .map(
                    |block_hash| async move { self.requester.request_for_block(&block_hash).await },
                )
                .collect();

            let request_count = request_futures.len();

            // Execute all requests in parallel, short-circuit on first error
            futures::future::try_join_all(request_futures).await?;

            let request_duration = request_start.elapsed();
            tracing::info!(
                "Completed broadcasting {} block requests in {:?}",
                request_count,
                request_duration
            );
        }

        let total_duration = start_time.elapsed();
        tracing::debug!(
            "request_next completed in {:?} (resend: {})",
            total_duration,
            resend
        );

        Ok(())
    }
}

/// Create a stream to receive blocks needed for Last Finalized State.
/// Uses exponential backoff: starts at request_timeout, doubles up to max_request_timeout.
/// Resets to request_timeout when a response is received.
pub async fn stream<'a, T: BlockRequesterOps>(
    approved_block: &'a ApprovedBlock,
    initial_response_messages: &'a VecDeque<BlockMessage>,
    response_message_receiver: mpsc::Receiver<BlockMessage>,
    response_queue_pending: Arc<AtomicUsize>,
    initial_minimum_height: i64,
    request_timeout: Duration,
    block_ops: &'a mut T,
) -> Result<impl futures::stream::Stream<Item = ST<BlockHash>> + use<'a, T>, CasperError> {
    let max_request_timeout = LFS_MAX_REQUEST_TIMEOUT;
    let block = &approved_block.candidate.block;

    // Active validators as per approved block state
    // - for approved state to be complete it is required to have block from each of them
    let latest_messages: HashMap<BlockHash, Validator> = block
        .justifications
        .iter()
        .map(|justification| {
            (
                justification.latest_block_hash.clone(),
                justification.validator.clone(),
            )
        })
        .collect();

    let initial_hashes = {
        let mut set = HashSet::new();
        set.insert(block.block_hash.clone());
        set
    };

    // Requester state, fill with validators for required latest messages
    let st = Arc::new(Mutex::new(ST::new(
        initial_hashes,
        Some(latest_messages.keys().cloned().collect()),
        Some(initial_minimum_height),
    )));

    // Queue to trigger processing of requests. `True` to resend requests.
    let (request_tx, request_rx) = mpsc::channel(2);
    // Response queue for existing blocks in the store.
    let (response_hash_tx, response_hash_rx) = mpsc::channel(1024);

    // Light the fire! / Starts the first request for block
    // - `false` means don't resend already requested blocks
    request_tx
        .send(false)
        .await
        .map_err(|_| CasperError::StreamError("Failed to send initial request".to_string()))?;

    let processor = StreamProcessor::new(block_ops, st.clone(), latest_messages, response_hash_tx);

    // Task 6.2: Enhanced resource cleanup with proper channel management
    tracing::info!("LFS Block Requester stream initialized - starting processing");
    tracing::debug!(
        "Initial messages queue size: {}",
        initial_response_messages.len()
    );
    tracing::debug!("Initial minimum height: {}", initial_minimum_height);
    tracing::debug!("Request timeout: {:?}", request_timeout);

    let stream_result = create_stream_with_processor(
        processor,
        request_rx,
        request_tx,
        response_hash_rx,
        initial_response_messages,
        response_message_receiver,
        response_queue_pending,
        request_timeout,
        max_request_timeout,
    )
    .await;

    match &stream_result {
        Ok(_) => {
            tracing::info!("LFS Block Requester stream created successfully");
        }
        Err(e) => {
            tracing::error!(error = ?e, "LFS Block Requester stream creation failed");
            // Cleanup is handled automatically by Drop implementations for channels
        }
    }

    stream_result
}

async fn create_stream_with_processor<'a, T: BlockRequesterOps>(
    mut processor: StreamProcessor<'a, T>,
    mut request_queue: mpsc::Receiver<bool>,
    request_queue_sender: mpsc::Sender<bool>,
    mut response_hash_queue: mpsc::Receiver<BlockHash>,
    initial_response_messages: &'a VecDeque<BlockMessage>,
    mut response_message_receiver: mpsc::Receiver<BlockMessage>,
    response_queue_pending: Arc<AtomicUsize>,
    request_timeout: Duration,
    max_request_timeout: Duration,
) -> Result<impl futures::stream::Stream<Item = ST<BlockHash>> + use<'a, T>, CasperError> {
    // num_cpus::get() reads the host's core count, not the pod's cgroup CPU
    // quota or the tokio runtime's actual worker pool size — see
    // https://github.com/F1R3FLY-io/f1r3node-rust/issues/147. Bound
    // concurrency to the runtime's real worker count instead so it tracks
    // whatever TOKIO_WORKER_THREADS configured in node/src/main.rs.
    let processor_count = tokio::runtime::Handle::current().metrics().num_workers();
    tracing::info!(
        "LFS Block Requester using {} processor-bounded workers (parEvalMapProcBounded equivalent)",
        processor_count
    );

    // Create semaphore for bounded concurrency
    let response_semaphore = Arc::new(Semaphore::new(processor_count));

    // Exponential backoff: current timeout starts at request_timeout,
    // doubles on each idle timeout (up to max_request_timeout),
    // resets to request_timeout when a response is received.
    let mut current_timeout = request_timeout;
    let mut idle_timeout = Box::pin(tokio::time::sleep(current_timeout));

    // Process all initial messages immediately and merge with regular message stream
    // This ensures proper ordering and efficient processing of pre-existing messages
    let initial_messages_channel_capacity = std::cmp::max(1, initial_response_messages.len());
    let (initial_messages_tx, mut initial_messages_rx) =
        mpsc::channel(initial_messages_channel_capacity);

    // Enqueue all initial messages for processing - this preserves order and integrates with normal flow
    if !initial_response_messages.is_empty() {
        tracing::info!(
            "Processing {} initial response messages",
            initial_response_messages.len()
        );
        for initial_message in initial_response_messages {
            if let Err(_) = initial_messages_tx.send(initial_message.clone()).await {
                tracing::error!("LFS initial message enqueue failed: channel closed");
                return Err(CasperError::StreamError(
                    "Failed to setup initial messages".to_string(),
                ));
            }
        }
    }
    // Close the sender to indicate no more initial messages
    drop(initial_messages_tx);

    // Create the actual stream using async_stream::stream!
    let stream = stream! {
        // Track consecutive errors for potential circuit breaking
        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: u32 = 5;

        // Main stream processing loop
        loop {
            tokio::select! {
                Some(resend_flag) = request_queue.recv() => {
                    match processor.request_next(resend_flag).await {
                        Ok(()) => {
                            consecutive_errors = 0; // Reset error counter on success
                        }
                        Err(e) => {
                            consecutive_errors += 1;
                            tracing::warn!(attempt = consecutive_errors, error = ?e, "LFS request processing failed");

                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                tracing::error!(max = MAX_CONSECUTIVE_ERRORS, "LFS stream terminating: maximum consecutive errors reached");
                                break;
                            }

                            // Continue processing other arms instead of immediate break
                            continue;
                        }
                    }

                    // Map to current state after requestNext
                    let current_state = {
                        match processor.st.lock() {
                            Ok(state) => state.clone(),
                            Err(e) => {
                                tracing::debug!(error = ?e, "failed to acquire state lock for request processing");
                                // Try to continue with other arms instead of breaking immediately
                                continue;
                            }
                        }
                    };

                    // Terminate when state is finished
                    if current_state.is_finished() {
                        tracing::info!("Request processing completed - all blocks downloaded");
                        yield current_state;
                        break;
                    }

                    // Reset idle timeout and backoff due to activity (response received)
                    current_timeout = request_timeout;
                    idle_timeout = Box::pin(tokio::time::sleep(current_timeout));

                    // Emit state to stream
                    yield current_state;
                }

                _ = &mut idle_timeout => {
                    let next_timeout = current_timeout.saturating_mul(2).min(max_request_timeout);
                    tracing::warn!(
                        "No responses for {:?}. Resending requests. (backoff: {:?} -> {:?})",
                        current_timeout,
                        current_timeout,
                        next_timeout
                    );

                    match request_queue_sender.try_send(true) {
                        Ok(()) => {
                            tracing::debug!("Timeout triggered - resend request enqueued successfully");
                            // Exponential backoff: double the timeout up to max_request_timeout
                            current_timeout = next_timeout;
                            idle_timeout = Box::pin(tokio::time::sleep(current_timeout));
                        }
                        Err(e) => {
                            tracing::debug!(error = ?e, "failed to enqueue resend request — channel full or closed");
                            tracing::warn!("request queue channel appears closed or full, checking if stream should terminate");

                            // Check if we should terminate gracefully
                            let should_terminate = {
                                match processor.st.lock() {
                                    Ok(state) => state.is_finished(),
                                    Err(_) => {
                                        tracing::debug!("cannot acquire state lock to check termination condition; assuming terminate");
                                        true // Assume termination if we can't check state
                                    }
                                }
                            };

                            if should_terminate {
                                tracing::info!("Stream terminating gracefully - processing appears complete");
                                break;
                            }
                            // Exponential backoff even on error
                            current_timeout = current_timeout.saturating_mul(2).min(max_request_timeout);
                            idle_timeout = Box::pin(tokio::time::sleep(current_timeout));
                        }
                    }
                }

                // Process initial messages with highest priority (before network messages)
                // This ensures initial messages are handled first, which is often more efficient
                Some(initial_block) = initial_messages_rx.recv() => {
                    let permit = match response_semaphore.clone().acquire_owned().await {
                        Ok(permit) => permit,
                        Err(e) => {
                            tracing::debug!(error = ?e, "failed to acquire semaphore permit for initial response processing");
                            // Continue with other arms instead of breaking
                            continue;
                        }
                    };

                    // Process with bounded concurrency - same as regular response processing
                    let _permit = permit; // Hold permit during processing

                    tracing::info!("Processing initial response message {}",
                        PrettyPrinter::build_string_block_message(&initial_block, true));

                    // Process initial block with same logic as regular response messages
                    let progress = match processor.process_block(&initial_block).await {
                        Ok(progress) => {
                            consecutive_errors = 0; // Reset error counter on success
                            progress
                        }
                        Err(e) => {
                            consecutive_errors += 1;
                            tracing::warn!(attempt = consecutive_errors, error = ?e, "LFS initial block message processing failed");

                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                tracing::error!(max = MAX_CONSECUTIVE_ERRORS, "LFS stream terminating: maximum consecutive errors processing initial messages");
                                break;
                            }
                            continue;
                        }
                    };

                    // Emit current state after processing initial response
                    let current_state = {
                        match processor.st.lock() {
                            Ok(state) => state.clone(),
                            Err(e) => {
                                tracing::debug!(error = ?e, "failed to acquire state lock for initial response processing");
                                continue;
                            }
                        }
                    };

                    // Check termination condition after initial response processing
                    if current_state.is_finished() {
                        tracing::info!("Initial response processing completed - all blocks downloaded");
                        yield current_state;
                        break;
                    }

                    if progress {
                        current_timeout = request_timeout;
                        idle_timeout = Box::pin(tokio::time::sleep(current_timeout));
                    }

                    // Emit state to stream
                    yield current_state;
                }

                Some(block_message) = response_message_receiver.recv() => {
                    let _ = response_queue_pending.fetch_update(
                        Ordering::AcqRel,
                        Ordering::Acquire,
                        |curr| Some(curr.saturating_sub(1)),
                    );
                    metrics::gauge!(
                        INIT_BLOCK_MESSAGE_QUEUE_PENDING_METRIC,
                        "source" => CASPER_METRICS_SOURCE
                    )
                    .set(response_queue_pending.load(Ordering::Relaxed) as f64);

                    let permit = match response_semaphore.clone().acquire_owned().await {
                        Ok(permit) => permit,
                        Err(e) => {
                            tracing::debug!(error = ?e, "failed to acquire semaphore permit for response processing");
                            continue;
                        }
                    };

                    // Process with bounded concurrency
                    let _permit = permit; // Hold permit during processing

                    let progress = match processor.process_block(&block_message).await {
                        Ok(progress) => {
                            consecutive_errors = 0; // Reset error counter on success
                            progress
                        }
                        Err(e) => {
                            consecutive_errors += 1;
                            tracing::warn!(attempt = consecutive_errors, error = ?e, "LFS block message processing failed");

                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                tracing::error!(max = MAX_CONSECUTIVE_ERRORS, "LFS stream terminating: maximum consecutive errors processing block messages");
                                break;
                            }
                            continue;
                        }
                    };

                    // Emit current state after processing response
                    let current_state = {
                        match processor.st.lock() {
                            Ok(state) => state.clone(),
                            Err(e) => {
                                tracing::debug!(error = ?e, "failed to acquire state lock for response processing");
                                continue;
                            }
                        }
                    };

                    // Check termination condition after response processing
                    if current_state.is_finished() {
                        tracing::info!("Response processing completed - all blocks downloaded");
                        yield current_state;
                        break;
                    }

                    if progress {
                        current_timeout = request_timeout;
                        idle_timeout = Box::pin(tokio::time::sleep(current_timeout));
                    }

                    // Emit state to stream
                    yield current_state;
                }

                Some(block_hash) = response_hash_queue.recv() => {
                    let permit = match response_semaphore.clone().acquire_owned().await {
                        Ok(permit) => permit,
                        Err(e) => {
                            tracing::debug!(error = ?e, "failed to acquire semaphore permit for hash response processing");
                            continue;
                        }
                    };

                    // Process with bounded concurrency
                    let _permit = permit; // Hold permit during processing

                    let block = processor.requester.get_block_from_store(&block_hash);

                    tracing::info!("Process existing {}", PrettyPrinter::build_string_block_message(&block, false));

                    let progress = match processor.process_block(&block).await {
                        Ok(progress) => {
                            consecutive_errors = 0; // Reset error counter on success
                            progress
                        }
                        Err(e) => {
                            consecutive_errors += 1;
                            tracing::warn!(attempt = consecutive_errors, error = ?e, "LFS existing block processing failed");

                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                tracing::error!(max = MAX_CONSECUTIVE_ERRORS, "LFS stream terminating: maximum consecutive errors processing existing blocks");
                                break;
                            }
                            continue;
                        }
                    };

                    // Emit current state after processing existing block
                    let current_state = {
                        match processor.st.lock() {
                            Ok(state) => state.clone(),
                            Err(e) => {
                                tracing::debug!(error = ?e, "failed to acquire state lock for hash response processing");
                                continue;
                            }
                        }
                    };

                    // Check termination condition after hash response processing
                    if current_state.is_finished() {
                        tracing::info!("Hash response processing completed - all blocks downloaded");
                        yield current_state;
                        break;
                    }

                    if progress {
                        current_timeout = request_timeout;
                        idle_timeout = Box::pin(tokio::time::sleep(current_timeout));
                    }

                    // Emit state to stream
                    yield current_state;
                }
            }
        }

        // Integration testing and final state validation
        let final_state = {
            match processor.st.lock() {
                Ok(state) => {
                    let final_stats = (
                        state.d.len(),
                        state.latest.len(),
                        state.finished.len(),
                        state.lower_bound,
                        state.is_finished()
                    );
                    tracing::info!("LFS Block Requester stream completed - Final state: active: {}, latest: {}, finished: {}, lower_bound: {}, is_finished: {}",
                        final_stats.0, final_stats.1, final_stats.2, final_stats.3, final_stats.4);

                    if final_stats.4 {
                        tracing::info!("LFS Block Requester completed successfully - all required blocks downloaded");
                    } else {
                        tracing::warn!("LFS Block Requester terminated with incomplete state - some blocks may be missing");
                    }

                    Some(state.clone())
                }
                Err(e) => {
                    tracing::error!(error = ?e, "LFS stream failed to acquire final state lock");
                    None
                }
            }
        };

        // Emit final state if available for integration testing
        if let Some(final_state) = final_state {
            yield final_state;
        }

        tracing::info!("LFS Block Requester stream processing completed");
    };

    Ok(stream)
}

#[cfg(test)]
mod window_tests {
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rust::block_implicits::get_random_block;
    use models::rust::casper::protocol::casper_message::Justification;

    use super::*;
    use crate::rust::validate::Validate;
    use crate::rust::validator_identity::ValidatorIdentity;

    #[derive(Default)]
    struct ProgressBlockOps {
        blocks: HashMap<BlockHash, BlockMessage>,
        requests: Mutex<Vec<BlockHash>>,
    }

    #[async_trait]
    impl BlockRequesterOps for ProgressBlockOps {
        async fn request_for_block(&self, block_hash: &BlockHash) -> Result<(), CasperError> {
            self.requests.lock().unwrap().push(block_hash.clone());
            Ok(())
        }

        fn contains_block(&self, block_hash: &BlockHash) -> Result<bool, CasperError> {
            Ok(self.blocks.contains_key(block_hash))
        }

        fn get_block_from_store(&self, block_hash: &BlockHash) -> BlockMessage {
            self.blocks.get(block_hash).expect("stored block").clone()
        }

        fn put_block_to_store(
            &mut self,
            block_hash: BlockHash,
            block: &BlockMessage,
        ) -> Result<(), CasperError> {
            self.blocks.insert(block_hash, block.clone());
            Ok(())
        }

        fn validate_block(&self, block: &BlockMessage) -> bool { Validate::historical_block(block) }
    }

    fn signed_block_below_window() -> BlockMessage {
        let (secret_key, _) = Secp256k1.new_key_pair();
        let block = get_random_block(
            Some(5),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![hash(0xaa)]),
            Some(vec![Justification {
                validator: Validator::from(vec![0xbb; 65]),
                latest_block_hash: hash(0xcc),
            }]),
            Some(vec![]),
            Some(vec![]),
            None,
            None,
            None,
        );
        ValidatorIdentity::new(&secret_key).sign_block(&block)
    }

    #[tokio::test]
    async fn requested_block_below_window_is_saved_and_reports_progress_without_expanding_dependencies(
    ) {
        let block = signed_block_below_window();
        let mut ops = ProgressBlockOps::default();
        let state = ST::new(HashSet::from([block.block_hash.clone()]), None, Some(10));
        let (state, _) = state.get_next(false);
        let state = Arc::new(Mutex::new(state));
        let (sender, mut receiver) = mpsc::channel(1);
        let mut processor = StreamProcessor::new(&mut ops, state.clone(), HashMap::new(), sender);

        assert!(processor.process_block(&block).await.unwrap());
        assert!(!processor.process_block(&block).await.unwrap());
        assert_eq!(
            processor.requester.blocks.get(&block.block_hash),
            Some(&block)
        );
        assert_eq!(processor.requester.blocks.len(), 1);
        assert!(processor.requester.requests.lock().unwrap().is_empty());
        assert!(receiver.try_recv().is_err());
        let state = state.lock().unwrap();
        assert!(state.is_finished());
        assert_eq!(state.finished, HashSet::from([block.block_hash]));
        assert_eq!(state.lower_bound, 10);
    }

    #[tokio::test]
    async fn invalid_requested_copies_do_not_report_progress_or_prevent_the_original_from_completing(
    ) {
        let block = signed_block_below_window();
        let mut invalid_hash = block.clone();
        invalid_hash.header.timestamp ^= 1;
        let mut invalid_signature = block.clone();
        invalid_signature.sig = Default::default();
        let mut ops = ProgressBlockOps::default();
        let state = ST::new(HashSet::from([block.block_hash.clone()]), None, Some(10));
        let (state, _) = state.get_next(false);
        let state = Arc::new(Mutex::new(state));
        let (sender, _receiver) = mpsc::channel(1);
        let mut processor = StreamProcessor::new(&mut ops, state.clone(), HashMap::new(), sender);

        for invalid in [invalid_hash, invalid_signature] {
            assert!(!processor.process_block(&invalid).await.unwrap());
            assert!(processor.requester.blocks.is_empty());
            let state = state.lock().unwrap();
            assert_eq!(state.d.get(&block.block_hash), Some(&ReqStatus::Requested));
            assert!(state.finished.is_empty());
            assert!(state.height_map.is_empty());
            assert!(!state.is_finished());
        }

        assert!(processor.process_block(&block).await.unwrap());
        assert_eq!(
            processor.requester.blocks.get(&block.block_hash),
            Some(&block)
        );
        assert!(processor.requester.requests.lock().unwrap().is_empty());
        assert!(state.lock().unwrap().is_finished());
    }

    #[tokio::test(start_paused = true)]
    async fn only_valid_requested_blocks_below_window_reset_the_idle_timeout() {
        use futures::{FutureExt, StreamExt};

        for valid in [true, false] {
            let mut block = signed_block_below_window();
            let remaining = hash(0xdd);
            let state = ST::new(
                HashSet::from([block.block_hash.clone(), remaining.clone()]),
                None,
                Some(10),
            );
            let (state, _) = state.get_next(false);
            let state = Arc::new(Mutex::new(state));
            let mut ops = ProgressBlockOps::default();
            let (request_tx, request_rx) = mpsc::channel(2);
            let (hash_tx, hash_rx) = mpsc::channel(1);
            let (message_tx, message_rx) = mpsc::channel(1);
            let initial = VecDeque::new();
            let processor = StreamProcessor::new(&mut ops, state, HashMap::new(), hash_tx);
            let stream = create_stream_with_processor(
                processor,
                request_rx,
                request_tx,
                hash_rx,
                &initial,
                message_rx,
                Arc::new(AtomicUsize::new(1)),
                Duration::from_secs(10),
                Duration::from_secs(40),
            )
            .await
            .unwrap();
            futures::pin_mut!(stream);
            assert!(stream.next().now_or_never().is_none());
            tokio::time::advance(Duration::from_secs(8)).await;
            if !valid {
                block.sig = Default::default();
            }
            message_tx.send(block.clone()).await.unwrap();
            let received = stream.next().await.unwrap();
            assert_eq!(received.finished.contains(&block.block_hash), valid);

            tokio::time::advance(Duration::from_secs(3)).await;
            let retry = stream.next().now_or_never();
            if valid {
                assert!(retry.is_none(), "valid response must postpone the retry");
                tokio::time::advance(Duration::from_secs(7)).await;
                let retry = stream.next().await.unwrap();
                assert_eq!(retry.d.len(), 1);
                assert_eq!(retry.d.get(&remaining), Some(&ReqStatus::Requested));
            } else {
                let retry = retry
                    .expect("invalid response must not postpone the retry")
                    .unwrap();
                assert_eq!(retry.d.len(), 2);
                assert!(retry.finished.is_empty());
            }
        }
    }

    fn hash(byte: u8) -> BlockHash { BlockHash::from(vec![byte; 32]) }

    /// A joiner seeds `latest` from the approved block's justifications, so a
    /// bonded validator that never proposed puts genesis there at height 0.
    /// The window is the only bound on what the restore accepts and never
    /// rises once lowered.
    #[test]
    fn a_genesis_placeholder_does_not_open_the_window_below_the_computed_floor() {
        let floor = 25_824i64;
        let genesis = hash(0xba);

        let state = ST::new(
            HashSet::from([hash(0x01)]),
            Some(HashSet::from([genesis.clone()])),
            Some(floor),
        );
        let (state, requested) = state.get_next(false);
        assert!(
            requested.contains(&genesis),
            "the placeholder is requested as a latest message"
        );

        // Genesis has no sender, so the stream passes `lowers_bound = false`.
        let (state, _) = state.received(genesis.clone(), 0, None, false);

        assert!(
            state.lower_bound >= floor,
            "a latest message at height 0 must not lower the window below the \
             computed floor; got {}",
            state.lower_bound
        );
        assert!(
            !state.latest.contains(&genesis),
            "the seed must still be consumed from `latest`, or the stream never \
             finishes waiting for it"
        );
        // The window is consumed as `block_number >= lower_bound`, so a bound
        // below the floor is what admits the walk toward genesis.
        assert!(
            25_214i64 < state.lower_bound,
            "a block 610 below the floor must fall outside the window"
        );
    }

    /// The slot's own validator is the test, not a non-empty sender: genesis is
    /// the only unsigned block today, but a slot filled by a block someone else
    /// signed is the same bookkeeping, not testimony.
    #[test]
    fn only_the_slot_owners_own_block_lowers_the_window() {
        use models::rust::block_implicits::get_random_block;

        let validator = Validator::from(vec![0x11; 65]);
        let other = Validator::from(vec![0x22; 65]);
        let block = |sender: Option<Validator>| {
            get_random_block(
                Some(10),
                None,
                None,
                None,
                sender,
                None,
                None,
                Some(vec![]),
                None,
                Some(vec![]),
                None,
                None,
                None,
                None,
            )
        };

        let signed = block(Some(validator.clone()));
        let latest = HashMap::from([(signed.block_hash.clone(), validator.clone())]);
        assert!(slot_is_own_testimony(&latest, &signed));

        let by_someone_else = block(Some(other));
        let latest = HashMap::from([(by_someone_else.block_hash.clone(), validator.clone())]);
        assert!(
            !slot_is_own_testimony(&latest, &by_someone_else),
            "a slot filled by another validator's block is not its testimony"
        );

        let genesis = block(Some(Validator::new()));
        let latest = HashMap::from([(genesis.block_hash.clone(), validator)]);
        assert!(!slot_is_own_testimony(&latest, &genesis));
    }

    /// The lowering itself is not the defect and must survive: a validator's
    /// own latest message still opens the window by one, so its parents can be
    /// fetched.
    #[test]
    fn a_signed_latest_message_still_lowers_the_window_by_one() {
        let floor = 25_824i64;
        let signed = hash(0x11);

        let state = ST::new(
            HashSet::from([hash(0x01)]),
            Some(HashSet::from([signed.clone()])),
            Some(floor),
        );
        let (state, _) = state.get_next(false);
        let (state, _) = state.received(signed, floor, None, true);

        assert_eq!(
            state.lower_bound,
            floor - 1,
            "a signed latest message at the floor must still reach its parents"
        );
    }
}
