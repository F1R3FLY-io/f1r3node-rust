//! Forward-horizon RSpace history synchronization.
//!
//! Every root missing from the local verified-root catalog is imported under
//! one affine [`StateImportSession`]. Pages become durable only after their
//! content-addresses and authenticated export projection have been checked;
//! roots become authoritative only after the complete multi-root closure is
//! sealed and published atomically. Forward-horizon synchronization never
//! changes the repository's active root.

use std::collections::{BTreeSet, HashMap};
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::Stream;
use models::rust::casper::protocol::casper_message::{StoreItemsCursorProtocol, StoreItemsMessage};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::import_session::{
    StateImportError, StateImportPage, StateImportPublicationMode, StateImportPublicationWitness,
    StateImportSession, StateImportSpec,
};
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use tokio::sync::mpsc;

use crate::rust::engine::lfs_tuple_space_requester::StatePartPath;
use crate::rust::errors::CasperError;

/// Per-page history-object limit used by forward-horizon synchronization.
pub const PAGE_SIZE: i32 = 1024;

/// Maximum number of root-page requests awaiting responses at once.
///
/// This bounds network pressure, the `try_join_all` future set, and peer
/// response buffering independently of the number or depth of horizon roots.
/// It is a concurrency window, not a traversal-depth limit: every pending
/// cursor is admitted as an earlier request completes.
pub const MAX_IN_FLIGHT_REQUESTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqStatus {
    Init,
    Requested,
    Received,
    Done,
}

/// Active pagination cursors and terminal root markers.
///
/// A non-terminal page replaces its received cursor rather than appending a
/// new entry, so retained state is bounded by the number of target roots and
/// cannot slope with pagination depth.
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

    fn take_next(&mut self, resend: bool) -> Vec<Key> {
        if resend {
            return self
                .d
                .iter()
                .filter(|(_, status)| **status == ReqStatus::Requested)
                .take(MAX_IN_FLIGHT_REQUESTS)
                .map(|(key, _)| key.clone())
                .collect();
        }

        let available = MAX_IN_FLIGHT_REQUESTS.saturating_sub(self.requested_count());
        let mut requested_keys = Vec::new();
        for (key, status) in &mut self.d {
            if requested_keys.len() == available {
                break;
            }
            if *status == ReqStatus::Init {
                *status = ReqStatus::Requested;
                requested_keys.push(key.clone());
            }
        }
        requested_keys
    }

    /// Atomically retire a validated response. A continuation replaces its
    /// predecessor; a terminal response leaves one bounded Done marker.
    fn complete_page(&mut self, current: &Key, next: Key, terminal: bool) -> bool {
        if self.d.get(current) != Some(&ReqStatus::Requested) {
            return false;
        }
        if (terminal && current != &next)
            || (!terminal && (current == &next || self.d.contains_key(&next)))
        {
            return false;
        }
        self.d.insert(current.clone(), ReqStatus::Received);
        if terminal {
            self.d.insert(current.clone(), ReqStatus::Done);
        } else {
            self.d.remove(current);
            self.d.insert(next, ReqStatus::Init);
        }
        true
    }

    fn status(&self, key: &Key) -> Option<ReqStatus> { self.d.get(key).copied() }

    fn requested_count(&self) -> usize {
        self.d
            .values()
            .filter(|status| **status == ReqStatus::Requested)
            .count()
    }

    pub fn is_finished(&self) -> bool { self.d.values().all(|status| *status == ReqStatus::Done) }

    pub fn len(&self) -> usize { self.d.len() }

    pub fn is_empty(&self) -> bool { self.d.is_empty() }

    pub fn done_count(&self) -> usize { self.d.values().filter(|s| **s == ReqStatus::Done).count() }
}

#[async_trait]
pub trait HorizonRequesterOps: Send + Sync {
    async fn request_for_horizon_chunk(
        &self,
        path: &StatePartPath,
        page_size: i32,
        cursor_protocol: StoreItemsCursorProtocol,
    ) -> Result<(), CasperError>;
}

/// Read-only verified-root membership query used before import. Root
/// publication authority belongs exclusively to the affine import session.
pub type HasRootFn = Arc<dyn Fn(&Blake2b256Hash) -> Result<bool, CasperError> + Send + Sync>;

struct HorizonStreamProcessor<T: HorizonRequesterOps> {
    request_ops: T,
    session: Option<Box<dyn StateImportSession>>,
    target_roots: Vec<Blake2b256Hash>,
    cursor_protocol: StoreItemsCursorProtocol,
    st: ST<StatePartPath>,
}

impl<T: HorizonRequesterOps> HorizonStreamProcessor<T> {
    fn process_store_items_message(
        &mut self,
        message: StoreItemsMessage,
    ) -> Result<bool, CasperError> {
        if self.st.status(&message.start_path) != Some(ReqStatus::Requested) {
            tracing::debug!(
                "LFS forward-horizon: ignoring unsolicited, duplicate, or stale page response"
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
            CasperError::RuntimeError(
                "LFS forward-horizon: import session was already consumed".to_string(),
            )
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
                "LFS forward-horizon: response terminal claim disagrees with authenticated traversal"
                    .to_string(),
            ));
        }

        // The authenticated traversal, not a transport-side root map,
        // determines which root owns the continuation cursor.
        let response_root = cursor_root(&start_path)?;
        let receipt_root = cursor_root(&receipt.next_path)?;
        if receipt_root != response_root {
            return Err(CasperError::RuntimeError(format!(
                "LFS forward-horizon: authenticated cursor crossed roots ({} -> {})",
                response_root, receipt_root
            )));
        }

        // Only validated/staged pages may advance transport state.
        if !self
            .st
            .complete_page(&start_path, receipt.next_path, receipt.terminal)
        {
            return Err(CasperError::RuntimeError(
                "LFS forward-horizon: validated page could not make a unique forward cursor transition"
                    .to_string(),
            ));
        }
        Ok(true)
    }

    async fn request_next(&mut self, resend: bool) -> Result<(), CasperError> {
        let paths = self.st.take_next(resend);
        if paths.is_empty() {
            return Ok(());
        }

        if resend {
            tracing::info!(
                "LFS forward-horizon: resending {} pending page requests",
                paths.len()
            );
        } else {
            tracing::debug!(
                "LFS forward-horizon: dispatching {} page requests in parallel",
                paths.len()
            );
        }

        let requests = paths.iter().map(|path| {
            self.request_ops
                .request_for_horizon_chunk(path, PAGE_SIZE, self.cursor_protocol)
        });
        futures::future::try_join_all(requests).await?;
        Ok(())
    }

    fn seal_and_publish(&mut self) -> Result<(), CasperError> {
        if !self.st.is_finished() {
            return Err(CasperError::RuntimeError(
                "LFS forward-horizon: attempted publication before every root terminated"
                    .to_string(),
            ));
        }
        let session = self.session.take().ok_or_else(|| {
            CasperError::RuntimeError(
                "LFS forward-horizon: import session publication was already consumed".to_string(),
            )
        })?;
        let publication = session
            .seal()?
            .publish(&StateImportPublicationWitness::new())?;
        if publication.verified_roots != self.target_roots {
            return Err(CasperError::RuntimeError(format!(
                "LFS forward-horizon: importer published an inexact root set (expected {}, got {})",
                self.target_roots.len(),
                publication.verified_roots.len()
            )));
        }
        if publication.activation.is_some() {
            return Err(CasperError::RuntimeError(
                "LFS forward-horizon: PreserveCurrent import unexpectedly activated a root"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

fn cursor_root(path: &StatePartPath) -> Result<&Blake2b256Hash, CasperError> {
    path.first().map(|(root, _)| root).ok_or_else(|| {
        CasperError::RuntimeError("LFS forward-horizon: cursor has no root component".to_string())
    })
}

/// Synchronize all missing forward-horizon roots through one affine import.
///
/// The returned try-stream yields request-state snapshots and surfaces every
/// transport, validation, closure, and publication failure as an `Err` item.
/// Empty input and all-present input produce exactly one successful empty
/// state without constructing an import session.
pub async fn stream<T: HorizonRequesterOps>(
    horizon_roots: Vec<Blake2b256Hash>,
    has_root: HasRootFn,
    state_importer: Arc<dyn RSpaceImporter>,
    request_ops: T,
    mut store_items_message_receiver: mpsc::Receiver<StoreItemsMessage>,
    request_timeout: Duration,
) -> Result<impl Stream<Item = Result<ST<StatePartPath>, CasperError>>, CasperError> {
    if request_timeout.is_zero() {
        return Err(CasperError::RuntimeError(
            "LFS forward-horizon: request timeout must be positive".to_string(),
        ));
    }

    let total_input = horizon_roots.len();
    let canonical_roots = horizon_roots.into_iter().collect::<BTreeSet<_>>();
    for root in &canonical_roots {
        if root.0.len() != 32 {
            return Err(StateImportError::InvalidDigestWidth {
                field: "forward-horizon root",
                available: root.0.len(),
            }
            .into());
        }
    }

    let mut missing_roots = Vec::new();
    for root in canonical_roots {
        if !(has_root)(&root)? {
            missing_roots.push(root);
        }
    }

    tracing::info!(
        "LFS forward-horizon: {} missing canonical roots ({} input entries)",
        missing_roots.len(),
        total_input
    );

    let processor = if missing_roots.is_empty() {
        None
    } else {
        let spec = StateImportSpec::new(
            missing_roots.iter().cloned(),
            StateImportPublicationMode::PreserveCurrent,
        )?;
        let session = state_importer.begin_import(spec.clone())?;
        if session.spec() != &spec {
            return Err(CasperError::RuntimeError(
                "LFS forward-horizon: importer returned a session for a different specification"
                    .to_string(),
            ));
        }
        let initial_paths = missing_roots
            .iter()
            .cloned()
            .map(|root| vec![(root, None)])
            .collect();
        Some(HorizonStreamProcessor {
            request_ops,
            session: Some(session),
            target_roots: missing_roots,
            cursor_protocol: StoreItemsCursorProtocol::AuthenticatedTerminal,
            st: ST::new(initial_paths),
        })
    };

    let max_request_timeout = Duration::from_secs(128);
    let output = async_stream::try_stream! {
        let Some(mut processor) = processor else {
            yield ST::new(Vec::new());
            return;
        };

        processor.request_next(false).await?;
        yield processor.st.clone();

        let mut current_timeout = request_timeout;
        let mut idle_timeout = Box::pin(tokio::time::sleep(current_timeout));
        loop {
            let message = tokio::select! {
                biased;
                message = store_items_message_receiver.recv() => Some(message),
                _ = &mut idle_timeout => None,
            };

            match message {
                Some(message) => {
                    let message = message.ok_or_else(|| {
                        CasperError::RuntimeError(
                            "LFS forward-horizon: response channel closed before import completed"
                                .to_string(),
                        )
                    })?;
                    let accepted = processor.process_store_items_message(message)?;
                    if !accepted {
                        continue;
                    }

                    if processor.st.is_finished() {
                        processor.seal_and_publish()?;
                        tracing::info!(
                            "LFS forward-horizon: atomically published {} verified roots without activation",
                            processor.target_roots.len()
                        );
                        yield processor.st.clone();
                        return;
                    }

                    processor.request_next(false).await?;
                    current_timeout = request_timeout;
                    idle_timeout = Box::pin(tokio::time::sleep(current_timeout));
                    yield processor.st.clone();
                }
                None => {
                    processor.request_next(true).await?;
                    current_timeout = current_timeout
                        .saturating_mul(2)
                        .min(max_request_timeout);
                    tracing::warn!(
                        "LFS forward-horizon: response timeout; next retry in {:?}",
                        current_timeout
                    );
                    idle_timeout = Box::pin(tokio::time::sleep(current_timeout));
                }
            }
        }
    };

    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use futures::{StreamExt, TryStreamExt};
    use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
    use rspace_plus_plus::rspace::state::import_session::{
        SealedStateImport, StateImportActivation, StateImportPageReceipt, StateImportPublication,
    };
    use tokio::sync::{mpsc as test_mpsc, Barrier};

    use super::*;

    #[test]
    fn st_accepts_only_requested_responses() {
        let mut st: ST<i32> = ST::new(vec![1]);
        assert!(!st.complete_page(&1, 1, true));
        let requested = st.take_next(false);
        assert_eq!(requested, vec![1]);
        assert!(st.complete_page(&1, 1, true));
        assert!(!st.complete_page(&1, 1, true));
    }

    #[test]
    fn st_replaces_paginated_cursor_and_remains_bounded() {
        let mut st = ST::new(vec![1]);
        st.take_next(false);
        assert!(st.complete_page(&1, 2, false));
        assert_eq!(st.len(), 1);
        assert_eq!(st.status(&1), None);
        assert_eq!(st.status(&2), Some(ReqStatus::Init));
    }

    #[test]
    fn st_finishes_only_after_received_entries_are_done() {
        let mut st: ST<i32> = ST::new(vec![1, 2]);
        st.take_next(false);
        assert!(st.complete_page(&1, 1, true));
        assert!(!st.is_finished());
        assert!(st.complete_page(&2, 2, true));
        assert!(st.is_finished());
        assert_eq!(st.done_count(), 2);
    }

    #[test]
    fn st_timeout_resends_only_the_bounded_requested_window() {
        let mut st = ST::new((0..MAX_IN_FLIGHT_REQUESTS + 8).collect::<Vec<_>>());
        let first = st.take_next(false);
        assert_eq!(first.len(), MAX_IN_FLIGHT_REQUESTS);
        assert_eq!(st.requested_count(), MAX_IN_FLIGHT_REQUESTS);

        let resend = st.take_next(true);
        assert_eq!(resend.len(), MAX_IN_FLIGHT_REQUESTS);
        assert_eq!(
            first.into_iter().collect::<BTreeSet<_>>(),
            resend.into_iter().collect::<BTreeSet<_>>()
        );
        assert_eq!(st.requested_count(), MAX_IN_FLIGHT_REQUESTS);
    }

    #[derive(Clone, Debug, Default)]
    struct ImportTrace {
        begin_specs: Vec<StateImportSpec>,
        pages: Vec<StateImportPage>,
        seal_count: usize,
        publish_count: usize,
        fail_stage: bool,
        fail_seal: bool,
        fail_publish: bool,
        receipt_override: Option<StateImportPageReceipt>,
        publication_override: Option<StateImportPublication>,
    }

    struct MockImporter {
        trace: Arc<Mutex<ImportTrace>>,
    }

    impl MockImporter {
        fn new() -> (Arc<dyn RSpaceImporter>, Arc<Mutex<ImportTrace>>) {
            let trace = Arc::new(Mutex::new(ImportTrace::default()));
            (
                Arc::new(Self {
                    trace: Arc::clone(&trace),
                }),
                trace,
            )
        }
    }

    impl RSpaceImporter for MockImporter {
        fn begin_import(
            &self,
            spec: StateImportSpec,
        ) -> Result<Box<dyn StateImportSession>, StateImportError> {
            self.trace.lock().unwrap().begin_specs.push(spec.clone());
            Ok(Box::new(MockSession {
                spec,
                trace: Arc::clone(&self.trace),
            }))
        }

        fn get_history_item(
            &self,
            _hash: Blake2b256Hash,
        ) -> Result<Option<Vec<u8>>, StateImportError> {
            Ok(None)
        }
    }

    struct MockSession {
        spec: StateImportSpec,
        trace: Arc<Mutex<ImportTrace>>,
    }

    impl StateImportSession for MockSession {
        fn spec(&self) -> &StateImportSpec { &self.spec }

        fn validate_and_stage_page(
            &mut self,
            page: StateImportPage,
        ) -> Result<StateImportPageReceipt, StateImportError> {
            let mut trace = self.trace.lock().unwrap();
            if trace.fail_stage {
                return Err(StateImportError::InvalidPagination(
                    "injected page rejection".to_string(),
                ));
            }
            let terminal = page.start_path == page.advertised_next_path;
            let default_receipt = StateImportPageReceipt {
                next_path: page.advertised_next_path.clone(),
                terminal,
                history_objects_staged: page.history_items.len(),
                cold_leaves_staged: page.data_items.len(),
            };
            trace.pages.push(page);
            Ok(trace.receipt_override.take().unwrap_or(default_receipt))
        }

        fn seal(self: Box<Self>) -> Result<Box<dyn SealedStateImport>, StateImportError> {
            let mut trace = self.trace.lock().unwrap();
            trace.seal_count += 1;
            if trace.fail_seal {
                return Err(StateImportError::IncompletePagination(Vec::new()));
            }
            drop(trace);
            Ok(Box::new(MockSealed {
                spec: self.spec,
                trace: self.trace,
            }))
        }
    }

    struct MockSealed {
        spec: StateImportSpec,
        trace: Arc<Mutex<ImportTrace>>,
    }

    impl SealedStateImport for MockSealed {
        fn spec(&self) -> &StateImportSpec { &self.spec }

        fn publish(
            self: Box<Self>,
            witness: &StateImportPublicationWitness,
        ) -> Result<StateImportPublication, StateImportError> {
            let mut trace = self.trace.lock().unwrap();
            trace.publish_count += 1;
            if trace.fail_publish {
                return Err(StateImportError::InternalInvariant(
                    "injected publication rejection".to_string(),
                ));
            }
            witness.record_authoritative_publication();
            Ok(trace
                .publication_override
                .clone()
                .unwrap_or_else(|| StateImportPublication {
                    verified_roots: self.spec.roots().to_vec(),
                    activation: None,
                }))
        }
    }

    #[derive(Clone, Default)]
    struct MockOps {
        sends: Arc<Mutex<Vec<StatePartPath>>>,
        barrier: Option<Arc<Barrier>>,
        barrier_limit: usize,
        request_ordinal: Arc<AtomicUsize>,
        fail_requests: bool,
    }

    impl MockOps {
        fn with_barrier(participants: usize) -> Self {
            Self {
                barrier: Some(Arc::new(Barrier::new(participants))),
                barrier_limit: participants,
                ..Self::default()
            }
        }
    }

    #[async_trait]
    impl HorizonRequesterOps for MockOps {
        async fn request_for_horizon_chunk(
            &self,
            path: &StatePartPath,
            page_size: i32,
            cursor_protocol: StoreItemsCursorProtocol,
        ) -> Result<(), CasperError> {
            assert_eq!(page_size, PAGE_SIZE);
            assert_eq!(
                cursor_protocol,
                StoreItemsCursorProtocol::AuthenticatedTerminal
            );
            self.sends.lock().unwrap().push(path.clone());
            if self.fail_requests {
                return Err(CasperError::RuntimeError(
                    "injected request failure".to_string(),
                ));
            }
            let ordinal = self.request_ordinal.fetch_add(1, Ordering::SeqCst);
            if ordinal < self.barrier_limit {
                let barrier = self
                    .barrier
                    .as_ref()
                    .expect("a positive barrier limit requires a barrier");
                barrier.wait().await;
            }
            Ok(())
        }
    }

    fn hash_for(seed: u8) -> Blake2b256Hash {
        let mut bytes = vec![0u8; 32];
        bytes[0] = seed;
        Blake2b256Hash::from_bytes(bytes)
    }

    fn path(root: &Blake2b256Hash) -> StatePartPath { vec![(root.clone(), None)] }

    fn continuation(root: &Blake2b256Hash, seed: u8) -> StatePartPath {
        vec![(root.clone(), None), (hash_for(seed), Some(seed))]
    }

    fn response(start_path: StatePartPath, last_path: StatePartPath) -> StoreItemsMessage {
        let terminal = start_path == last_path;
        StoreItemsMessage {
            start_path,
            last_path,
            history_items: Vec::new(),
            data_items: Vec::new(),
            cursor_protocol: StoreItemsCursorProtocol::AuthenticatedTerminal,
            terminal,
        }
    }

    async fn drain<S>(stream: S) -> Result<Vec<ST<StatePartPath>>, CasperError>
    where S: Stream<Item = Result<ST<StatePartPath>, CasperError>> {
        Box::pin(stream).try_collect().await
    }

    #[tokio::test]
    async fn empty_input_is_one_item_noop_without_import_session() {
        let (importer, trace) = MockImporter::new();
        let ops = MockOps::default();
        let sends = Arc::clone(&ops.sends);
        let (_tx, rx) = test_mpsc::channel(1);
        let states = drain(
            stream(
                Vec::new(),
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(states.len(), 1);
        assert!(states[0].is_empty());
        assert!(trace.lock().unwrap().begin_specs.is_empty());
        assert!(sends.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn all_present_duplicate_roots_are_one_item_noop() {
        let root = hash_for(1);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_query = Arc::clone(&calls);
        let has_root: HasRootFn = Arc::new(move |root| {
            calls_for_query.lock().unwrap().push(root.clone());
            Ok(true)
        });
        let (importer, trace) = MockImporter::new();
        let ops = MockOps::default();
        let (_tx, rx) = test_mpsc::channel(1);
        let states = drain(
            stream(
                vec![root.clone(), root.clone(), root.clone()],
                has_root,
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(states.len(), 1);
        assert!(states[0].is_empty());
        assert_eq!(calls.lock().unwrap().as_slice(), &[root]);
        assert!(trace.lock().unwrap().begin_specs.is_empty());
    }

    #[tokio::test]
    async fn mixed_roots_use_one_canonical_preserve_current_session() {
        let present = hash_for(2);
        let missing = hash_for(3);
        let has_root: HasRootFn = {
            let present = present.clone();
            Arc::new(move |root| Ok(root == &present))
        };
        let (importer, trace) = MockImporter::new();
        let ops = MockOps::default();
        let sends = Arc::clone(&ops.sends);
        let (tx, rx) = test_mpsc::channel(2);
        tx.send(response(path(&missing), path(&missing)))
            .await
            .unwrap();
        drop(tx);

        let states = drain(
            stream(
                vec![missing.clone(), present, missing.clone()],
                has_root,
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap();

        assert!(states.last().unwrap().is_finished());
        assert_eq!(states.last().unwrap().len(), 1);
        assert_eq!(sends.lock().unwrap().as_slice(), &[path(&missing)]);
        let trace = trace.lock().unwrap();
        assert_eq!(trace.begin_specs.len(), 1);
        assert_eq!(trace.begin_specs[0].roots(), &[missing]);
        assert_eq!(
            trace.begin_specs[0].publication_mode(),
            &StateImportPublicationMode::PreserveCurrent
        );
        assert_eq!(trace.seal_count, 1);
        assert_eq!(trace.publish_count, 1);
    }

    #[tokio::test]
    async fn canonical_empty_root_accepts_an_empty_terminal_page() {
        let root = RadixHistory::empty_root_node_hash();
        let (importer, trace) = MockImporter::new();
        let ops = MockOps::default();
        let (tx, rx) = test_mpsc::channel(2);
        tx.send(response(path(&root), path(&root))).await.unwrap();
        drop(tx);

        let states = drain(
            stream(
                vec![root],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap();
        assert!(states.last().unwrap().is_finished());
        assert_eq!(trace.lock().unwrap().pages.len(), 1);
    }

    async fn protocol_rejection_does_not_stage_or_publish(message: StoreItemsMessage) -> String {
        let root = message.start_path[0].0.clone();
        let (importer, trace) = MockImporter::new();
        let ops = MockOps::default();
        let (tx, rx) = test_mpsc::channel(2);
        tx.send(message).await.unwrap();
        drop(tx);

        let error = drain(
            stream(
                vec![root],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap_err();
        let trace = trace.lock().unwrap();
        assert!(trace.pages.is_empty());
        assert_eq!(trace.seal_count, 0);
        assert_eq!(trace.publish_count, 0);
        error.to_string()
    }

    #[tokio::test]
    async fn unspecified_cursor_epoch_is_rejected_before_staging() {
        let root = hash_for(4);
        let mut message = response(path(&root), path(&root));
        message.cursor_protocol = StoreItemsCursorProtocol::Unspecified;
        assert!(protocol_rejection_does_not_stage_or_publish(message)
            .await
            .contains("selected explicitly"));
    }

    #[tokio::test]
    async fn explicitly_legacy_response_cannot_answer_an_authenticated_request() {
        let root = hash_for(5);
        let mut message = response(path(&root), path(&root));
        message.cursor_protocol = StoreItemsCursorProtocol::LegacyEmptyTerminal;
        message.last_path.clear();
        assert!(protocol_rejection_does_not_stage_or_publish(message)
            .await
            .contains("protocol mismatch"));
    }

    #[tokio::test]
    async fn forged_terminal_claim_is_rejected_before_staging() {
        let root = hash_for(6);
        let mut message = response(path(&root), continuation(&root, 42));
        message.terminal = true;
        assert!(protocol_rejection_does_not_stage_or_publish(message)
            .await
            .contains("must repeat startPath"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn distinct_roots_keep_distinct_authenticated_pagination_chains() {
        let root_a = hash_for(10);
        let root_b = hash_for(11);
        let start_a = path(&root_a);
        let start_b = path(&root_b);
        let next_a = continuation(&root_a, 40);
        let next_b = continuation(&root_b, 41);
        let (importer, trace) = MockImporter::new();
        let ops = MockOps::default();
        let sends = Arc::clone(&ops.sends);
        let (tx, rx) = test_mpsc::channel(8);

        let feeder_sends = Arc::clone(&sends);
        let feeder = tokio::spawn({
            let start_a = start_a.clone();
            let start_b = start_b.clone();
            let next_a = next_a.clone();
            let next_b = next_b.clone();
            async move {
                loop {
                    let both_sent = {
                        let sent = feeder_sends.lock().unwrap();
                        sent.contains(&start_a) && sent.contains(&start_b)
                    };
                    if both_sent {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
                tx.send(response(start_a.clone(), next_a.clone()))
                    .await
                    .unwrap();
                tx.send(response(start_b.clone(), next_b.clone()))
                    .await
                    .unwrap();
                loop {
                    let both_sent = {
                        let sent = feeder_sends.lock().unwrap();
                        sent.contains(&next_a) && sent.contains(&next_b)
                    };
                    if both_sent {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
                tx.send(response(next_a.clone(), next_a)).await.unwrap();
                tx.send(response(next_b.clone(), next_b)).await.unwrap();
            }
        });

        let states = drain(
            stream(
                vec![root_b.clone(), root_a.clone()],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap();
        feeder.await.unwrap();

        assert!(states.last().unwrap().is_finished());
        assert!(states.iter().all(|state| state.len() <= 2));
        let trace = trace.lock().unwrap();
        assert_eq!(trace.pages.len(), 4);
        assert_eq!(trace.publish_count, 1);
        assert_eq!(trace.begin_specs[0].roots(), &[root_a, root_b]);
    }

    #[tokio::test]
    async fn duplicate_response_is_ignored_without_staging_scheduling_or_yielding() {
        let root = hash_for(12);
        let start = path(&root);
        let next = continuation(&root, 43);
        let first_page = response(start.clone(), next.clone());
        let (importer, trace) = MockImporter::new();
        let ops = MockOps::default();
        let sends = Arc::clone(&ops.sends);
        let (tx, rx) = test_mpsc::channel(4);
        let stream = stream(
            vec![root],
            Arc::new(|_| Ok(false)),
            importer,
            ops,
            rx,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let mut stream = Box::pin(stream);

        stream
            .try_next()
            .await
            .expect("initial request dispatch")
            .expect("initial state");
        tx.send(first_page.clone()).await.unwrap();
        stream
            .try_next()
            .await
            .expect("accepted first page")
            .expect("paginated state");
        assert_eq!(trace.lock().unwrap().pages.len(), 1);
        assert_eq!(sends.lock().unwrap().len(), 2);

        tx.send(first_page).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), stream.try_next())
                .await
                .is_err(),
            "an ignored duplicate must not yield a state snapshot"
        );
        assert_eq!(trace.lock().unwrap().pages.len(), 1);
        assert_eq!(sends.lock().unwrap().len(), 2);

        tx.send(response(next.clone(), next)).await.unwrap();
        let final_state = stream
            .try_next()
            .await
            .expect("accepted terminal page")
            .expect("final state");
        assert!(final_state.is_finished());
        assert_eq!(trace.lock().unwrap().pages.len(), 2);
    }

    #[tokio::test]
    async fn rejected_page_is_reported_without_publication() {
        let root = hash_for(20);
        let (importer, trace) = MockImporter::new();
        trace.lock().unwrap().fail_stage = true;
        let ops = MockOps::default();
        let (tx, rx) = test_mpsc::channel(2);
        tx.send(response(path(&root), path(&root))).await.unwrap();

        let error = drain(
            stream(
                vec![root],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, CasperError::StateImportError(_)));
        let trace = trace.lock().unwrap();
        assert_eq!(trace.seal_count, 0);
        assert_eq!(trace.publish_count, 0);
    }

    #[tokio::test]
    async fn authenticated_cursor_cannot_cross_into_another_root() {
        let root = hash_for(27);
        let other_root = hash_for(28);
        let (importer, trace) = MockImporter::new();
        trace.lock().unwrap().receipt_override = Some(StateImportPageReceipt {
            next_path: continuation(&other_root, 42),
            terminal: false,
            history_objects_staged: 0,
            cold_leaves_staged: 0,
        });
        let ops = MockOps::default();
        let (tx, rx) = test_mpsc::channel(2);
        tx.send(response(path(&root), continuation(&root, 41)))
            .await
            .unwrap();

        let error = drain(
            stream(
                vec![root],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("authenticated cursor crossed roots"));
        assert_eq!(trace.lock().unwrap().publish_count, 0);
    }

    #[tokio::test]
    async fn publication_must_not_activate_a_root() {
        let root = hash_for(21);
        let (importer, trace) = MockImporter::new();
        trace.lock().unwrap().publication_override = Some(StateImportPublication {
            verified_roots: vec![root.clone()],
            activation: Some(StateImportActivation {
                root: root.clone(),
                generation: 1,
            }),
        });
        let ops = MockOps::default();
        let (tx, rx) = test_mpsc::channel(2);
        tx.send(response(path(&root), path(&root))).await.unwrap();

        let error = drain(
            stream(
                vec![root],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("unexpectedly activated"));
        assert_eq!(trace.lock().unwrap().publish_count, 1);
    }

    #[tokio::test]
    async fn publication_with_an_inexact_root_set_is_a_visible_error() {
        let root = hash_for(24);
        let (importer, trace) = MockImporter::new();
        trace.lock().unwrap().publication_override = Some(StateImportPublication {
            verified_roots: Vec::new(),
            activation: None,
        });
        let ops = MockOps::default();
        let (tx, rx) = test_mpsc::channel(2);
        tx.send(response(path(&root), path(&root))).await.unwrap();

        let error = drain(
            stream(
                vec![root],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("inexact root set"));
        assert_eq!(trace.lock().unwrap().publish_count, 1);
    }

    #[tokio::test]
    async fn seal_failure_is_a_visible_error_and_prevents_publication() {
        let root = hash_for(25);
        let (importer, trace) = MockImporter::new();
        trace.lock().unwrap().fail_seal = true;
        let ops = MockOps::default();
        let (tx, rx) = test_mpsc::channel(2);
        tx.send(response(path(&root), path(&root))).await.unwrap();

        let error = drain(
            stream(
                vec![root],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, CasperError::StateImportError(_)));
        let trace = trace.lock().unwrap();
        assert_eq!(trace.seal_count, 1);
        assert_eq!(trace.publish_count, 0);
    }

    #[tokio::test]
    async fn publish_failure_is_a_visible_error() {
        let root = hash_for(26);
        let (importer, trace) = MockImporter::new();
        trace.lock().unwrap().fail_publish = true;
        let ops = MockOps::default();
        let (tx, rx) = test_mpsc::channel(2);
        tx.send(response(path(&root), path(&root))).await.unwrap();

        let error = drain(
            stream(
                vec![root],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, CasperError::StateImportError(_)));
        assert_eq!(trace.lock().unwrap().publish_count, 1);
    }

    #[tokio::test]
    async fn closed_response_channel_is_a_visible_error() {
        let root = hash_for(22);
        let (importer, trace) = MockImporter::new();
        let ops = MockOps::default();
        let (tx, rx) = test_mpsc::channel(1);
        drop(tx);
        let error = drain(
            stream(
                vec![root],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("response channel closed"));
        assert_eq!(trace.lock().unwrap().publish_count, 0);
    }

    #[tokio::test]
    async fn request_failure_is_a_visible_error() {
        let root = hash_for(23);
        let (importer, trace) = MockImporter::new();
        let ops = MockOps {
            fail_requests: true,
            ..MockOps::default()
        };
        let (_tx, rx) = test_mpsc::channel(1);
        let error = drain(
            stream(
                vec![root],
                Arc::new(|_| Ok(false)),
                importer,
                ops,
                rx,
                Duration::from_secs(1),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("injected request failure"));
        assert_eq!(trace.lock().unwrap().publish_count, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn request_window_is_bounded_and_refilled_as_slots_complete() {
        let roots = (0..MAX_IN_FLIGHT_REQUESTS + 8)
            .map(|index| hash_for((index + 30) as u8))
            .collect::<Vec<_>>();
        let (importer, _trace) = MockImporter::new();
        let ops = MockOps::with_barrier(MAX_IN_FLIGHT_REQUESTS);
        let sends = Arc::clone(&ops.sends);
        let (tx, rx) = test_mpsc::channel(2);
        let stream = stream(
            roots,
            Arc::new(|_| Ok(false)),
            importer,
            ops,
            rx,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        let mut stream = Box::pin(stream);
        let first = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("all request futures must reach their shared barrier")
            .expect("stream item")
            .expect("successful initial dispatch");
        assert_eq!(first.len(), MAX_IN_FLIGHT_REQUESTS + 8);
        assert_eq!(first.requested_count(), MAX_IN_FLIGHT_REQUESTS);
        assert_eq!(sends.lock().unwrap().len(), MAX_IN_FLIGHT_REQUESTS);

        let completed_path = sends.lock().unwrap()[0].clone();
        tx.send(response(completed_path.clone(), completed_path))
            .await
            .unwrap();
        let second = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("a completed slot must admit another Init root")
            .expect("stream item")
            .expect("successful window refill");
        assert_eq!(second.requested_count(), MAX_IN_FLIGHT_REQUESTS);
        assert_eq!(second.done_count(), 1);
        assert_eq!(sends.lock().unwrap().len(), MAX_IN_FLIGHT_REQUESTS + 1);
    }
}
