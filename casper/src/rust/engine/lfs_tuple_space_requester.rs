// See casper/src/main/scala/coop/rchain/casper/engine/LfsTupleSpaceRequester.scala

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::Stream;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, StoreItemsCursorProtocol, StoreItemsMessage,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::import_session::{
    StateImportError, StateImportPage, StateImportPublicationMode, StateImportPublicationWitness,
    StateImportSession, StateImportSpec,
};
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use tokio::sync::mpsc;

use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    CASPER_METRICS_SOURCE, INIT_TUPLE_SPACE_QUEUE_PENDING_METRIC,
};

/// Per-cursor state for Last Finalized State transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqStatus {
    Init,
    Requested,
    Received,
    Done,
}

/// A cursor into the authenticated RSpace radix history.
pub type StatePartPath = Vec<(Blake2b256Hash, Option<u8>)>;

/// Maximum number of history nodes in one LFS page.
pub const PAGE_SIZE: i32 = 750;

/// The sole transport-side capability required by tuple-space state transfer.
///
/// Validation, staging, closure verification, and root publication belong to
/// [`StateImportSession`]. Keeping those operations out of this trait prevents
/// a transport implementation from recreating setter-shaped import authority.
#[async_trait]
pub trait TupleSpaceRequesterOps {
    async fn request_for_store_item(
        &self,
        path: &StatePartPath,
        page_size: i32,
        cursor_protocol: StoreItemsCursorProtocol,
    ) -> Result<(), CasperError>;
}

/// Active request state.
///
/// Completed cursors are retired instead of retained. The map is therefore
/// bounded by the number of concurrently active authenticated cursors rather
/// than by total snapshot size.
#[derive(Debug, Clone, PartialEq)]
pub struct ST<Key: Hash + Eq + Clone> {
    d: HashMap<Key, ReqStatus>,
}

impl<Key: Hash + Eq + Clone> ST<Key> {
    pub fn new(initial: Vec<Key>) -> Self {
        Self {
            d: initial
                .into_iter()
                .map(|key| (key, ReqStatus::Init))
                .collect(),
        }
    }

    pub fn add(&self, keys: HashSet<Key>) -> Self {
        let mut next = self.clone();
        for key in keys {
            next.d.entry(key).or_insert(ReqStatus::Init);
        }
        next
    }

    pub fn get_next(&self, resend: bool) -> (Self, Vec<Key>) {
        let mut next = self.clone();
        let mut requests = Vec::new();
        for (key, status) in &self.d {
            if *status == ReqStatus::Init || (resend && *status == ReqStatus::Requested) {
                next.d.insert(key.clone(), ReqStatus::Requested);
                requests.push(key.clone());
            }
        }
        (next, requests)
    }

    /// Mark a response received only when a request was actually dispatched.
    pub fn received(&self, key: Key) -> (Self, bool) {
        if self.d.get(&key) != Some(&ReqStatus::Requested) {
            return (self.clone(), false);
        }
        let mut next = self.clone();
        next.d.insert(key, ReqStatus::Received);
        (next, true)
    }

    /// Retire a successfully consumed cursor.
    pub fn done(&self, key: Key) -> Self {
        let mut next = self.clone();
        if next.d.get(&key) == Some(&ReqStatus::Received) {
            next.d.remove(&key);
        }
        next
    }

    pub fn is_finished(&self) -> bool { self.d.is_empty() }

    /// Number of authenticated cursors awaiting a response or retry.
    pub fn active_count(&self) -> usize { self.d.len() }

    fn is_requested(&self, key: &Key) -> bool { self.d.get(key) == Some(&ReqStatus::Requested) }

    fn prepare_requests(&mut self, resend: bool) -> Vec<Key> {
        let mut requests = Vec::new();
        for (key, status) in &mut self.d {
            if *status == ReqStatus::Init || (resend && *status == ReqStatus::Requested) {
                *status = ReqStatus::Requested;
                requests.push(key.clone());
            }
        }
        requests
    }

    fn retire_and_enqueue(&mut self, current: &Key, next: Option<Key>) {
        self.d.remove(current);
        if let Some(next) = next {
            self.d.entry(next).or_insert(ReqStatus::Init);
        }
    }
}

/// Single-owner state machine for one affine tuple-space import.
struct TupleSpaceStreamProcessor<T: TupleSpaceRequesterOps> {
    request_ops: T,
    session: Option<Box<dyn StateImportSession>>,
    target_root: Blake2b256Hash,
    cursor_protocol: StoreItemsCursorProtocol,
    st: ST<StatePartPath>,
}

impl<T: TupleSpaceRequesterOps> TupleSpaceStreamProcessor<T> {
    fn new(
        request_ops: T,
        session: Box<dyn StateImportSession>,
        target_root: Blake2b256Hash,
        start_request: StatePartPath,
        cursor_protocol: StoreItemsCursorProtocol,
    ) -> Self {
        Self {
            request_ops,
            session: Some(session),
            target_root,
            cursor_protocol,
            st: ST::new(vec![start_request]),
        }
    }

    fn state(&self) -> ST<StatePartPath> { self.st.clone() }

    async fn request_next(&mut self, resend: bool) -> Result<(), CasperError> {
        for path in self.st.prepare_requests(resend) {
            self.request_ops
                .request_for_store_item(&path, PAGE_SIZE, self.cursor_protocol)
                .await?;
        }
        Ok(())
    }

    /// Validate and stage one response. Delayed duplicates and unsolicited
    /// responses are ignored without touching either session or request state.
    fn process_store_items_message(
        &mut self,
        message: StoreItemsMessage,
    ) -> Result<bool, CasperError> {
        if !self.st.is_requested(&message.start_path) {
            tracing::debug!(
                path_components = message.start_path.len(),
                "Ignoring tuple-space response for an unrequested or retired cursor"
            );
            return Ok(false);
        }

        let advertised_next_path = message
            .canonical_advertised_next_path(self.cursor_protocol)
            .map_err(CasperError::RuntimeError)?;
        let advertised_terminal = message.terminal;
        let StoreItemsMessage {
            start_path,
            history_items,
            data_items,
            ..
        } = message;

        let session = self.session.as_mut().ok_or_else(|| {
            CasperError::StateImportError(StateImportError::InternalInvariant(
                "tuple-space import session was consumed before transfer completion".to_string(),
            ))
        })?;
        let receipt = session.validate_and_stage_page(StateImportPage {
            start_path: start_path.clone(),
            advertised_next_path,
            skip: 0,
            take: PAGE_SIZE,
            history_items: history_items
                .into_iter()
                .map(|(hash, bytes)| (hash, bytes.to_vec()))
                .collect(),
            data_items: data_items
                .into_iter()
                .map(|(hash, bytes)| (hash, bytes.to_vec()))
                .collect(),
        })?;
        if receipt.terminal != advertised_terminal {
            return Err(CasperError::RuntimeError(
                "tuple-space response terminal claim disagrees with authenticated traversal"
                    .to_string(),
            ));
        }

        let next = (!receipt.terminal).then_some(receipt.next_path);
        self.st.retire_and_enqueue(&start_path, next);
        Ok(true)
    }

    fn seal_and_publish(&mut self) -> Result<(), CasperError> {
        if !self.st.is_finished() {
            return Err(CasperError::StateImportError(
                StateImportError::InternalInvariant(
                    "tuple-space import attempted publication with active cursors".to_string(),
                ),
            ));
        }
        let session = self.session.take().ok_or_else(|| {
            CasperError::StateImportError(StateImportError::InternalInvariant(
                "tuple-space import session was already consumed".to_string(),
            ))
        })?;
        let publication = session
            .seal()?
            .publish(&StateImportPublicationWitness::new())?;
        let activation = publication.activation.ok_or_else(|| {
            CasperError::StateImportError(StateImportError::InternalInvariant(
                "tuple-space import published without activating its target root".to_string(),
            ))
        })?;
        if activation.root != self.target_root
            || publication.verified_roots.as_slice() != [self.target_root.clone()]
        {
            return Err(CasperError::StateImportError(
                StateImportError::InternalInvariant(
                    "tuple-space import publication did not exactly activate its target root"
                        .to_string(),
                ),
            ));
        }
        Ok(())
    }
}

/// Receive, validate, stage, seal, and atomically activate one Last Finalized
/// RSpace snapshot.
///
/// Every operational failure is emitted as an `Err` item. The stream never
/// converts a failed request, page, seal, or publication into normal EOF.
pub async fn stream<T: TupleSpaceRequesterOps + Send>(
    approved_block: &ApprovedBlock,
    mut tuple_space_message_receiver: mpsc::Receiver<StoreItemsMessage>,
    tuple_space_queue_pending: Arc<AtomicUsize>,
    request_timeout: Duration,
    request_ops: T,
    state_importer: Arc<dyn RSpaceImporter>,
) -> Result<impl Stream<Item = Result<ST<StatePartPath>, CasperError>>, CasperError> {
    use crate::rust::util::proto_util;

    let state_hash = Blake2b256Hash::from_bytes(
        proto_util::post_state_hash(&approved_block.candidate.block).to_vec(),
    );
    let start_request = vec![(state_hash.clone(), None)];
    let spec = StateImportSpec::new(
        [state_hash.clone()],
        StateImportPublicationMode::Activate(state_hash.clone()),
    )?;
    let session = state_importer.begin_import(spec)?;
    let mut processor = TupleSpaceStreamProcessor::new(
        request_ops,
        session,
        state_hash,
        start_request,
        StoreItemsCursorProtocol::AuthenticatedTerminal,
    );
    let max_request_timeout = Duration::from_secs(128);

    Ok(async_stream::try_stream! {
        processor.request_next(false).await?;
        yield processor.state();

        let mut current_timeout = request_timeout;
        let idle_timeout = tokio::time::sleep(current_timeout);
        tokio::pin!(idle_timeout);

        loop {
            enum Event {
                Response(Option<StoreItemsMessage>),
                Timeout,
            }
            let event = tokio::select! {
                message = tuple_space_message_receiver.recv() => Event::Response(message),
                _ = &mut idle_timeout => Event::Timeout,
            };

            match event {
                Event::Response(message) => {
                    let message = message.ok_or_else(|| CasperError::StreamError(
                        "Tuple-space response channel closed before import completed".to_string(),
                    ))?;
                    let _ = tuple_space_queue_pending.fetch_update(
                        Ordering::AcqRel,
                        Ordering::Acquire,
                        |count| Some(count.saturating_sub(1)),
                    );
                    metrics::gauge!(
                        INIT_TUPLE_SPACE_QUEUE_PENDING_METRIC,
                        "source" => CASPER_METRICS_SOURCE
                    )
                    .set(tuple_space_queue_pending.load(Ordering::Relaxed) as f64);

                    if processor.process_store_items_message(message)? {
                        if processor.st.is_finished() {
                            processor.seal_and_publish()?;
                            yield processor.state();
                            break;
                        }

                        // Dispatch the authenticated receipt cursor directly.
                        processor.request_next(false).await?;
                        yield processor.state();
                        current_timeout = request_timeout;
                        idle_timeout.as_mut().reset(tokio::time::Instant::now() + current_timeout);
                    }
                }
                Event::Timeout => {
                    processor.request_next(true).await?;
                    yield processor.state();
                    current_timeout = current_timeout.saturating_mul(2).min(max_request_timeout);
                    idle_timeout.as_mut().reset(tokio::time::Instant::now() + current_timeout);
                }
            }
        }
    })
}
